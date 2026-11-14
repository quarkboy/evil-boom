// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Carrying out a planned route a tic at a time.
//!
//! Each tic the follower gets an Observation (where she is, which way she faces, the live
//! sector heights) and gives Controls. Live heights decide whether the next crossing on the
//! path can be made now; if not, she opens the door in front, or waits for the lift or door,
//! and as a last resort uses a trigger nearby for that sector. Off course (into a pit, over a
//! teleporter) or stuck, she replans the rest of the step from where she is.

mod barrels;
mod hazards;
pub use hazards::nearest_heading;
mod interactions;
mod jumping;
pub mod lanes;
mod state;
mod trouble;
mod walking;

use super::{heading, norm, Controls, Motion, Observation, Track, FULL_SPEED, RUN_TURN, TICRATE};
use crate::geom::{dist, segments_cross, Point};
use crate::mesh::inside;
use crate::nav::{Doors, Heights};
use crate::navigator::{Jump, Path, PathNote, States, View, MAX_STEP, PLAYER_RADIUS};
use crate::specials::{Key, Keys};
use crate::walkthrough::{plan_from, steps_from_legs, Action, Expect, Rest, Step};
use state::{BarrelShot, Crushers, Jumping, LitChain, OnTrack, Unstick, Weave};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
pub use trouble::Trouble;

/// The point `d` along the polyline `pts` (its end, past that).
fn along(pts: &[Point], d: f64) -> Point {
    let mut left = d;
    for w in pts.windows(2) {
        let l = dist(w[0], w[1]);
        if left <= l {
            let f = if l > 1e-6 { left / l } else { 0.0 };
            return (
                w[0].0 + (w[1].0 - w[0].0) * f,
                w[0].1 + (w[1].1 - w[0].1) * f,
            );
        }
        left -= l;
    }
    pts.last().copied().unwrap_or((0.0, 0.0))
}

/// How far a blast at `a` takes `b` to be, as P_RadiusAttack has it: the greater of the gaps
/// along x and along y, not the straight line (155 off in a straight line can be 122 by this,
/// inside the blast).
pub fn blast_gap(a: Point, b: Point) -> f64 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// What to shoot a barrel this far off with: the shotgun (or the super shotgun) when there are
/// shells to spare and it's near enough for most of the pellets to land, one blast and it's gone
/// (a barrel has 20 health: surer than taps); else the pistol, single taps.
pub fn barrel_gun(obs: &Observation, d: f64) -> i32 {
    if d < BARREL_SHOTGUN_RANGE
        && obs.weapons.as_ref().is_some_and(|w| w.contains(&3))
        && obs
            .ammo
            .iter()
            .flatten()
            .any(|&(s, n)| s == 3 && n >= BARREL_SHELLS)
    {
        3
    } else {
        tap_gun(obs)
    }
}

/// a barrel shot with the shotgun only this near (its spread)...
const BARREL_SHOTGUN_RANGE: f64 = 320.0;
/// ...and with this many shells or more
const BARREL_SHELLS: i32 = 12;

/// What to shoot something with: the pistol (the shotgun if there's nothing for it), whatever's
/// in hand: not the BFG with fewer cells than the 40 it needs, nor a missile, which sets off no
/// gun line.
pub fn tap_gun(obs: &Observation) -> i32 {
    TAP_GUNS
        .into_iter()
        .find(|&slot| tap_rounds(obs, slot) > 0)
        .unwrap_or(2)
}

/// the guns that tap (hitscan), in the order taken up: pistol, chaingun, shotgun
const TAP_GUNS: [i32; 3] = [2, 4, 3];

/// Rounds for the gun in `slot`, if she has it (none if she hasn't: shells are nothing with no
/// shotgun to fire them; ammo is (weapon slot, rounds for it)).
pub fn tap_rounds(obs: &Observation, slot: i32) -> i32 {
    if obs.weapons.as_ref().is_some_and(|w| !w.contains(&slot)) {
        return 0;
    }
    obs.ammo
        .as_ref()
        .and_then(|a| a.iter().find(|w| w.0 == slot))
        .map_or(1, |w| w.1)
}

/// tics between shots at a barrel or a wall (the pistol ready again, the trigger let go: a first shot each time)
pub const BARREL_TAP: i64 = 20;

/// a barrel is the one there was if it's this near where that was: shots push them (a hit
/// thrusts by its damage times 100 over the mass, a barrel's 100: 6-20 units a pistol shot)
pub const BARREL_TRACK: f64 = 48.0;

/// The barrel there is now, of one that was at `at` (pushed about by shots).
pub fn barrel_now(obs: &Observation, at: Point) -> Option<Point> {
    obs.barrels
        .iter()
        .flatten()
        .copied()
        .filter(|&b| dist(b, at) < BARREL_TRACK)
        .min_by(|a, b| dist(*a, at).total_cmp(&dist(*b, at)))
}

/// the longest wait for a lift to come back before crossing its line
const CROSS_REST_WAIT: i64 = 8 * 35;
/// a lift still this long isn't on its way anywhere (vanilla's waits 3 s at the bottom before
/// coming back)
const LIFT_STILL: i64 = 5 * 35;

/// tics between use presses waiting at a door
const USE_EVERY: i64 = 15;

/// off the path and outside its polys by this much: off course
const STRAY: f64 = 40.0;
/// this far from the path anywhere: off course
const OFF_COURSE: f64 = 96.0;
/// ...for this long
const OFF_COURSE_TICS: i64 = 10;
/// a step
const MAX_REPLANS: i32 = 6;

/// how near a door to press it (use reaches 64)
const PRESS_RANGE: f64 = 56.0;
/// don't stand still for moves to finish: some floors are hopped onto rising
const SETTLE_MAX: f64 = 0.0;
/// a route (replans that add steps start the per-step count again)
const MAX_TOTAL_REPLANS: i32 = 30;
/// one step taking this long: give up
const GIVE_UP_TICS: i64 = 60 * 35;

/// The actions she stops at the end of (the explorer's goto included: where it goes next is
/// anyone's guess, and arriving at a ledge running is sliding off it).
fn stops(a: Action) -> bool {
    matches!(
        a,
        Action::Use
            | Action::Shoot
            | Action::Exit
            | Action::Face
            | Action::Kill
            | Action::Endure
            | Action::Walk
    )
}

/// A trigger's expected effect being checked as it plays out.
#[derive(Clone, Debug)]
struct Watch {
    expect: Expect,
    /// the tic it fired
    since: i64,
    line: i64,
    /// when it got there
    reached: Option<i64>,
    late: bool,
    /// how far off the expected heights it was last tic, and the last tic it got nearer: a
    /// sector already moving keeps its mover and ignores the trigger (vanilla's EV_DoFloor
    /// skips a sector with one), so one not closing in isn't on its way
    gap: Option<f64>,
    closing: i64,
    /// its hold window has run out
    over: bool,
    /// the step that fired it
    step: i64,
    /// its closing has prompted a replan already
    used: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FollowerLog {
    pub events: Vec<String>,
}

impl FollowerLog {
    pub fn add(&mut self, tic: i64, msg: impl AsRef<str>) {
        if crate::debug::on().pace {
            eprintln!("    follower tic {tic}: {}", msg.as_ref());
        }
        self.events
            .push(format!("[{:6.1}s] {}", tic as f64 / TICRATE, msg.as_ref()));
    }
}

/// A run-jump, as whoever's playing would want to hear of it.
#[derive(Clone, Debug)]
pub enum JumpNews {
    /// lined up for it, about to run
    Going(Jump),
    /// came down where it was meant to
    Made(Jump),
    /// came down somewhere else (the sector)
    Missed(Jump, usize),
}

/// What a drop ahead the plan didn't mean to take came to.
enum Drop {
    None,
    /// another way on, without it
    Replanned,
    /// nothing but such drops: the objective's to be judged again
    GiveBack(Trouble),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegEnd {
    Teleport,
    Jump,
    Goal,
}

/// Part of a step's path walked without a break; at its last point: a teleport (walk through
/// the line), a jump (line up and sprint), or the goal.
#[derive(Clone, Debug)]
struct Segment {
    track: Track,
    end: SegEnd,
    jump: Option<Jump>,
    /// the track and where to look past its end
    look: Track,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Walk,
    Fetch,
    Settle,
    Act,
}

/// Running over sinking floors.
#[derive(Clone, Copy, Debug)]
struct Run {
    heading: f64,
    sector: Option<usize>,
    next: i64,
}

/// What blocks the next crossing: (the poly before it, the sector beyond, the portal, a point
/// to press for a gate door).
type Blocked = (usize, usize, usize, Option<Point>);

/// Full thrust towards world `direction` (running and strafing as needed), turning to face it.
fn run_controls(angle: f64, direction: f64) -> Controls {
    let d = norm(direction - angle);
    let rad = d.to_radians();
    Controls {
        forward: 50.0 * rad.cos(),
        strafe: -50.0 * rad.sin(),
        turn: (-d).clamp(-RUN_TURN, RUN_TURN),
        ..Controls::default()
    }
}

fn last_point(path: &Path) -> Option<Point> {
    path.waypoints.iter().rev().find_map(|w| *w)
}

pub struct RouteFollower {
    pub view: Rc<View>,
    pub steps: Vec<Step>,
    /// the plan only goes through doors she can open
    doors: Doors,
    lift_sectors: BTreeSet<usize>,
    pub keys: Keys,
    /// the heights the world can take, as the planner has it, kept up to date for replanning
    pub states: States,
    /// a step's goal out of reach: plan the rest of the level again (else give up)
    replan_level: bool,
    pub log: FollowerLog,
    pub motion: Motion,
    step_i: i64,
    pub done: bool,
    pub failed: bool,
    /// ...stuck (a step given up on for not getting anywhere), as against any other failure
    pub failed_stuck: bool,
    /// why it failed: for the log as it's always been, and plainly for whoever's playing
    pub trouble: Option<Trouble>,
    pub replans: i32,
    /// once-only stair switches used (for replans)
    pub fired: BTreeSet<usize>,
    /// she can be hurt (a mortal run): no barrel shot inside its blast
    pub mortal: bool,
    /// a run out of a field of barrels: no stopping for them (none shot, no waiting on a chain)
    pub barrel_run: bool,
    /// a monster about that shoots, as far as she knows (the fight's `shooters`): a barrel in
    /// the way shot only from out of every blast, never from beside it to run after (its shot
    /// can set it off while she's there)
    pub shooters_about: bool,
    /// steering between the barrels she sees, which the mesh's way knows nothing of
    weave: Weave,
    /// run-jumps that came down off their landing (for the explorer)
    pub missed: Vec<Jump>,
    /// the run-jumps set off and how each came down, for whoever's to hear of them (taken by
    /// the explorer each tic)
    pub jumps: Vec<JumpNews>,
    /// (where, line) pressed from there to no effect (for the explorer)
    pub failed_presses: Vec<(Point, usize)>,
    drops_ok: BTreeSet<(usize, usize, [u64; 4])>,
    edge_wait: bool,
    /// a lift on its way round across her path: which, as last seen, and since when
    lift_back: Option<(usize, Heights, i64)>,
    settle_logged: i64,
    cross_ready: bool,
    cross_wait_logged: bool,
    goal_tic: i64,
    last_pos: Option<Point>,
    /// a run-jump or a run over sinking floors in hand
    jump: Jumping,
    rest_logged: i64,
    rest_tic: i64,
    seen_heights: Vec<Heights>,
    /// a barrel in the way being shot: where from, the way there, the run off after
    shot: BarrelShot,
    /// a chain of barrels going off that she didn't set off: kept off it, or run out of it
    chain: LitChain,
    reopening: bool,
    watches: Vec<Watch>,
    settle: Vec<Expect>,
    settle_tic: i64,
    look: Option<f64>,
    near_cache: BTreeMap<usize, BTreeSet<usize>>,
    // the step
    step_replans: i32,
    step_tic: i64,
    crossed: bool,
    item: Option<Point>,
    press_tries: i32,
    before: Option<Vec<(usize, Option<Heights>)>>,
    cross_tic: Option<i64>,
    height_wait: Option<i64>,
    /// the floor under her last tic, waiting to shoot (still moving?)
    shot_floor: Option<f64>,
    segments: Vec<Segment>,
    seg_i: usize,
    poly_i: usize,
    phase: Phase,
    path_polys: BTreeSet<usize>,
    crossing_lines: BTreeSet<usize>,
    off_since: Option<i64>,
    /// backing off from where she got nowhere
    unstick: Unstick,
    /// where she is on the segment's track
    at: OnTrack,
    settle_until: i64,
    progress_tic: i64,
    wait_tic: Option<i64>,
    action_tic: Option<i64>,
    mover_heights: Option<Heights>,
    mover_still_since: i64,
    last_use: i64,
    /// the tic she last stood turned for a press at a door or switch she was waiting at
    press_tic: Option<i64>,
    speed_cap: f64,
    live: Vec<Heights>,
    /// the heights the tic before (which way a ceiling's going)
    prev_live: Vec<Heights>,
    /// ceilings coming down: what she's seen of them, and the wait or the crossing in hand
    crusher: Crushers,
    /// when the heights were last looked at (the fight may have had her between)
    seen_tic: Option<i64>,
}

impl RouteFollower {
    /// `keys` held and `states` (the heights the world can take) at the start; `tic`: the game
    /// tic it starts at.
    pub fn new(
        view: Rc<View>,
        steps: Vec<Step>,
        keys: Keys,
        states: Option<States>,
        replan_level: bool,
        tic: i64,
    ) -> RouteFollower {
        let doors = view.local_doors(Key::ALL.into_iter().collect());
        let lift_sectors = view
            .triggers
            .iter()
            .filter(|t| !t.permanent())
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .collect();
        let states = states.unwrap_or_else(|| view.initial_states());
        let sinking = view.nav.sinking_sectors().clone();
        let mut f = RouteFollower {
            view,
            steps,
            doors,
            lift_sectors,
            keys,
            states,
            replan_level,
            log: FollowerLog::default(),
            motion: Motion::default(),
            step_i: -1,
            done: false,
            failed: false,
            failed_stuck: false,
            trouble: None,
            replans: 0,
            fired: BTreeSet::new(),
            mortal: false,
            barrel_run: false,
            shooters_about: false,
            weave: Weave {
                barrels: Vec::new(),
                detour: Vec::new(),
                detour_tic: -1000,
            },
            missed: Vec::new(),
            jumps: Vec::new(),
            failed_presses: Vec::new(),
            drops_ok: BTreeSet::new(),
            edge_wait: false,
            lift_back: None,
            settle_logged: -1,
            cross_ready: false,
            cross_wait_logged: false,
            goal_tic: 0,
            last_pos: None,
            jump: Jumping {
                sprint: None,
                sprints_short: 0,
                touchdown: None,
                run: None,
                run_for: None,
                lineup_tic: None,
                brake_until: -1,
                wait: None,
                sinking,
            },
            rest_logged: -1,
            rest_tic: 0,
            seen_heights: Vec::new(),
            shot: BarrelShot {
                target: None,
                post: None,
                way: Vec::new(),
                best: None,
                last_shot: -1_000_000_000,
                stood: -1_000_000_000,
                tapped: None,
                flee_until: None,
                flee_way: Vec::new(),
                stay: false,
                step: None,
                settle: None,
                failed_posts: Vec::new(),
            },
            chain: LitChain {
                waiting: false,
                flee: Vec::new(),
                flee_tic: -1000,
            },
            reopening: false,
            watches: Vec::new(),
            settle: Vec::new(),
            settle_tic: 0,
            look: None,
            near_cache: BTreeMap::new(),
            step_replans: 0,
            step_tic: 0,
            crossed: false,
            item: None,
            press_tries: 0,
            before: None,
            cross_tic: None,
            height_wait: None,
            shot_floor: None,
            segments: Vec::new(),
            seg_i: 0,
            poly_i: 0,
            phase: Phase::Walk,
            path_polys: BTreeSet::new(),
            crossing_lines: BTreeSet::new(),
            off_since: None,
            unstick: Unstick {
                count: 0,
                until: -1,
                dir: 0.0,
            },
            at: OnTrack {
                index: 0,
                along: 0.0,
                off: 0.0,
                furthest: f64::NEG_INFINITY,
            },
            settle_until: 0,
            progress_tic: 0,
            wait_tic: None,
            action_tic: None,
            mover_heights: None,
            mover_still_since: 0,
            press_tic: None,
            last_use: -USE_EVERY,
            speed_cap: f64::INFINITY,
            live: Vec::new(),
            prev_live: Vec::new(),
            crusher: Crushers {
                wait: None,
                cross: None,
                kick: None,
                ceiling_pace: BTreeMap::new(),
                moved_at: BTreeMap::new(),
                pinned_since: None,
            },
            seen_tic: None,
        };
        f.next_step(tic);
        f
    }

    fn step(&self) -> &Step {
        &self.steps[self.step_i as usize]
    }

    fn trigger_line_no(&self) -> usize {
        self.step()
            .trigger
            .as_ref()
            .and_then(|t| t.linedef())
            .unwrap_or(0)
    }

    /// In the middle of a run-jump (lining up, sprinting, running over sinking floors).
    /// A crusher's crossing timed and begun, or one being set off from its edge: the route's to
    /// finish, not to be stopped part way.
    pub fn crossing_crusher(&self) -> bool {
        self.crusher.cross.is_some() || self.crusher.kick.is_some()
    }

    pub fn in_jump(&self) -> bool {
        self.jump.sprint.is_some() || self.jump.run.is_some() || self.jump.lineup_tic.is_some()
    }

    /// Off on a jump: sprinting for the takeoff or running over sinking floors. Not lining up at
    /// the run-up: standing there is no jump yet, and the fight has the legs (else she stands
    /// still at the run-up with a monster at her).
    pub fn jumping(&self) -> bool {
        self.jump.sprint.is_some() || self.jump.run.is_some()
    }

    /// Running over (or off) floors going down under her: done only on a floor that stays put.
    pub fn running_off(&self) -> bool {
        self.jump.run.is_some()
    }

    /// At a jump's run-up with what it needs there, settling on the spot square on to the
    /// takeoff, not off yet: her facing is what the route is waiting on.
    pub fn lining_up(&self) -> bool {
        self.jump.lineup_tic.is_some() && self.jump.sprint.is_none()
    }

    /// Stood to shoot a barrel this tic (where it's shot from, turning to it or tapping): the
    /// trigger and the way she faces are what the route is waiting on.
    pub fn stood_to_shoot(&self, tic: i64) -> bool {
        self.shot.stood == tic
    }

    /// Shooting something with the gun it picks for that (a barrel in the way, its blasts
    /// waited out; a switch to shoot): the gun's the route's for now.
    pub fn shooting(&self) -> bool {
        self.shot.target.is_some()
            || self.shot.flee_until.is_some()
            || self.shot.settle.is_some()
            || usize::try_from(self.step_i)
                .ok()
                .and_then(|i| self.steps.get(i))
                .is_some_and(|s| s.action == crate::walkthrough::Action::Shoot)
    }

    /// Waiting at a door or switch to press it this tic: her turn is the press's (use goes
    /// where she faces).
    pub fn pressing(&self, tic: i64) -> bool {
        self.press_tic == Some(tic)
    }

    /// Run-jumps still to come on the route.
    pub fn jumps_ahead(&self) -> bool {
        let from = usize::try_from(self.step_i).unwrap_or(0);
        self.steps
            .iter()
            .skip(from)
            .filter_map(|st| st.path.as_ref())
            .any(|p| {
                p.notes
                    .iter()
                    .any(|n| matches!(n, PathNote::RunJump { .. }))
            })
    }

    // --- what she's doing (for display) -----------------------------------------------------

    pub fn objective(&self) -> String {
        if self.done {
            return "route finished".into();
        }
        if self.failed {
            return "gave up".into();
        }
        let step = self.step();
        let subs = step.substeps();
        let sub = match self.phase {
            Phase::Settle => self
                .settle
                .iter()
                .map(|e| format!("wait for sector {}", e.sector))
                .collect::<Vec<_>>()
                .join(", "),
            Phase::Walk => subs[0].clone(),
            _ => subs
                .get(1)
                .cloned()
                .unwrap_or_else(|| step.action.name().to_string()),
        };
        format!(
            "step {}/{}: {sub} ({})",
            self.step_i + 1,
            self.steps.len(),
            step.label
        )
    }

    /// The rest of the current step's path from `pos` (None: a teleport or jump).
    pub fn planned_path(&self, pos: Option<Point>) -> Vec<Option<Point>> {
        if self.done || self.failed {
            return Vec::new();
        }
        let mut out: Vec<Option<Point>> = pos.into_iter().map(Some).collect();
        for i in self.seg_i..self.segments.len() {
            let pts = &self.segments[i].track.pts;
            let from = if i == self.seg_i {
                (self.at.index + 1).min(pts.len())
            } else {
                0
            };
            out.extend(pts[from..].iter().map(|&p| Some(p)));
            if i + 1 < self.segments.len() {
                out.push(None);
            }
        }
        out
    }

    /// How far the rest of the route is from `pos`, along it.
    pub fn way_left(&self, pos: Point) -> f64 {
        let length = |pts: &[Option<Point>]| {
            pts.windows(2)
                .filter_map(|w| Some(dist(w[0]?, w[1]?)))
                .sum::<f64>()
        };
        let later = usize::try_from(self.step_i + 1)
            .ok()
            .and_then(|i| self.steps.get(i..))
            .unwrap_or_default();
        length(&self.planned_path(Some(pos)))
            + later
                .iter()
                .filter_map(|st| st.path.as_ref())
                .map(|p| length(&p.waypoints))
                .sum::<f64>()
    }

    // --- step and segment bookkeeping ---------------------------------------------------------

    fn next_step(&mut self, tic: i64) {
        if self.step_i >= 0 && (self.step_i as usize) < self.steps.len() {
            let done = self.step().clone();
            if let Some(t) = &done.trigger {
                let effects = self.view.nav.effects_now(t, &self.states);
                self.states = self.view.nav.apply_trigger(&self.states, t);
                if t.permanent() {
                    // gone for good, risen or lowered: replans mustn't count on the old height
                    for (sec, hs) in effects {
                        self.states[sec] = hs;
                    }
                }
                if t.once() && t.cumulative() {
                    if let Some(li) = t.linedef() {
                        self.fired.insert(li);
                    }
                }
                for e in &done.expects {
                    self.watches.push(Watch {
                        expect: e.clone(),
                        since: tic,
                        line: t.line,
                        reached: None,
                        late: false,
                        gap: None,
                        closing: tic,
                        over: false,
                        step: self.step_i,
                        used: false,
                    });
                }
            }
            if let Some(k) = done.key {
                self.keys = self.keys.with(k);
            }
            // a quick permanent move the next path needs: let it finish first
            let settle: Vec<Expect> = done
                .expects
                .iter()
                .filter(|e| e.needed && e.hold.is_none() && e.travel <= SETTLE_MAX)
                .cloned()
                .collect();
            if !settle.is_empty() {
                self.settle = settle;
                self.settle_tic = tic;
                self.phase = Phase::Settle;
                return;
            }
        }
        self.advance(tic);
    }

    fn advance(&mut self, tic: i64) {
        self.step_i += 1;
        self.step_replans = 0;
        self.drops_ok.clear();
        self.cross_ready = false;
        self.cross_wait_logged = false;
        if self.step_i as usize >= self.steps.len() {
            self.done = true;
            self.log.add(tic, "route finished");
            return;
        }
        let subs = self.step().substeps().join(" -> ");
        self.log.add(
            tic,
            format!("step {}/{}: {subs}", self.step_i + 1, self.steps.len()),
        );
        self.step_tic = tic;
        self.crossed = false;
        self.item = self.item_of();
        self.press_tries = 0;
        self.before = None;
        self.cross_tic = None;
        self.height_wait = None;
        self.shot_floor = None;
        self.start_step(tic);
    }

    fn start_step(&mut self, tic: i64) {
        self.segments = self.split();
        self.seg_i = 0;
        self.poly_i = 0;
        self.phase = Phase::Walk;
        let path = self.step().path.clone().unwrap_or_else(|| Path {
            polys: Vec::new(),
            waypoints: Vec::new(),
            cost: 0.0,
            notes: Vec::new(),
            jumps: Vec::new(),
            squeezes: 0,
        });
        self.path_polys = path.polys.iter().copied().collect();
        // ledge edges the path means to cross (any others count as walls cutting corners)
        let mesh = &self.view.nav.mesh;
        self.crossing_lines = path
            .polys
            .windows(2)
            .flat_map(|w| {
                let (a, b) = (w[0], w[1]);
                mesh.poly(a)
                    .portals
                    .iter()
                    .filter(move |&&pi| mesh.portals[pi].other(a) == b)
                    .filter_map(|&pi| mesh.portals[pi].linedef)
            })
            .collect();
        self.off_since = None;
        self.unstick.count = 0;
        self.unstick.until = -1;
        self.enter_segment(tic);
    }

    fn enter_segment(&mut self, tic: i64) {
        self.at.index = 0;
        self.at.along = 0.0;
        self.at.off = 0.0;
        self.settle_until = tic + OFF_COURSE_TICS;
        self.reset_progress(tic);
    }

    /// The waypoints in segments, split at teleports and jump run-ups.
    fn split(&self) -> Vec<Segment> {
        let step = self.step();
        let Some(path) = &step.path else {
            return Vec::new();
        };
        let pts = &path.waypoints;
        let mut raw: Vec<(Vec<Point>, SegEnd, Option<Jump>)> = Vec::new();
        let mut cur: Vec<Point> = Vec::new();
        let mut i = 0;
        while i < pts.len() {
            let Some(p) = pts[i] else {
                if cur.len() >= 2 {
                    // (on a little past the line, so it's really crossed)
                    let (a, b) = (cur[cur.len() - 2], cur[cur.len() - 1]);
                    let d = dist(a, b).max(1.0);
                    let n = cur.len();
                    cur[n - 1] = (b.0 + (b.0 - a.0) / d * 24.0, b.1 + (b.1 - a.1) / d * 24.0);
                }
                raw.push((std::mem::take(&mut cur), SegEnd::Teleport, None));
                i += 1;
                continue;
            };
            cur.push(p);
            if let Some(jump) = path.jumps.iter().find(|j| dist(p, j.runup) < 1.0) {
                raw.push((std::mem::take(&mut cur), SegEnd::Jump, Some(jump.clone())));
                // on from the landing, then the waypoints after the takeoff (one at the landing
                // itself is dropped; a nearby one can be where the step ends, a switch a little
                // past the landing, and a track of just that point is "already there")
                cur = vec![jump.dest];
                let land =
                    (i + 2..pts.len()).find(|&k| pts[k].is_some_and(|q| dist(q, jump.dest) < 32.0));
                i = land.unwrap_or(i + 2);
                if i < pts.len() && pts[i].is_some_and(|q| dist(q, jump.dest) < 4.0) {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
        raw.push((cur, SegEnd::Goal, None));
        let n = raw.len();
        raw.into_iter()
            .enumerate()
            .map(|(idx, (mut seg_pts, end, jump))| {
                if seg_pts.is_empty() {
                    seg_pts = vec![jump
                        .as_ref()
                        .map_or_else(|| last_point(path).unwrap_or((0.0, 0.0)), |j| j.dest)];
                }
                let tail = self.look_tail(&seg_pts, end, jump.as_ref(), idx == n - 1);
                let mut look = seg_pts.clone();
                look.extend(tail);
                Segment {
                    track: Track::new(seg_pts),
                    end,
                    jump,
                    look: Track::new(look),
                }
            })
            .collect()
    }

    /// Where to look once the segment's own points run out.
    fn look_tail(
        &self,
        pts: &[Point],
        end: SegEnd,
        jump: Option<&Jump>,
        _last: bool,
    ) -> Vec<Point> {
        match end {
            SegEnd::Jump => return jump.map(|j| vec![j.takeoff]).unwrap_or_default(),
            SegEnd::Teleport => {
                if pts.len() >= 2 {
                    let h = heading(pts[pts.len() - 2], pts[pts.len() - 1]).to_radians();
                    let e = pts[pts.len() - 1];
                    return vec![(e.0 + h.cos() * 64.0, e.1 + h.sin() * 64.0)];
                }
                return Vec::new();
            }
            SegEnd::Goal => {}
        }
        let step = self.step();
        if let Some(li) = step.trigger.as_ref().and_then(|t| t.linedef()) {
            if stops(step.action) {
                let (a, b) = self.view.nav.map.line_seg(li);
                return vec![((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)];
            }
        }
        if let Some(next) = self.steps.get(self.step_i as usize + 1) {
            // rolling on into the next step: look where it goes
            if let Some(p) = &next.path {
                return p
                    .waypoints
                    .iter()
                    .skip(1)
                    .take(5)
                    .filter_map(|w| *w)
                    .take(3)
                    .collect();
            }
        }
        Vec::new()
    }

    /// Something else had the controls till now (the explorer looking round): that time isn't
    /// time stuck.
    pub fn resume(&mut self, tic: i64) {
        self.reset_progress(tic);
        // (a line-up broken off starts again: its clock would send her off from wherever)
        self.jump.lineup_tic = None;
    }

    fn reset_progress(&mut self, tic: i64) {
        self.at.furthest = f64::NEG_INFINITY;
        self.progress_tic = tic;
        self.wait_tic = None;
        self.action_tic = None;
        self.mover_heights = None;
        self.mover_still_since = tic;
        self.last_use = -USE_EVERY;
    }

    // --- the main entry -----------------------------------------------------------------

    pub fn act(&mut self, obs: &Observation) -> Controls {
        if self.done || self.failed {
            return Controls::default();
        }
        let nav = Rc::clone(&self.view);
        let obs_owned;
        let obs = if nav.nav.trenches.is_empty() {
            obs
        } else {
            // slots too narrow to stand in, at the floor beside them, as planned (otherwise the
            // real floor reads as a step to wait out)
            let mut o = obs.clone();
            for &t in nav.nav.trenches.keys() {
                if t < o.heights.len() {
                    let f = nav
                        .nav
                        .trench_floor(t, |x| obs.heights.get(x).map_or(f64::INFINITY, |h| h.floor));
                    o.heights[t].floor = f;
                }
            }
            obs_owned = o;
            &obs_owned
        };
        self.motion.observe(obs);
        self.judge_landing(obs);
        if self.seen_heights.len() != obs.heights.len() {
            self.seen_heights = obs.heights.clone();
        }
        // (a pace by the tics since she last looked: the fight may have had her in between, and
        // a slow crusher isn't one that moved all that way in a tic)
        let since = self.seen_tic.map_or(1, |t| (obs.tic - t).max(1)) as f64;
        self.seen_tic = Some(obs.tic);
        for (sec, &h) in obs.heights.iter().enumerate() {
            if self.seen_heights[sec] != h {
                self.crusher.moved_at.insert(sec, obs.tic);
                let moved = (h.ceiling - self.seen_heights[sec].ceiling).abs();
                if moved > 0.01 {
                    self.crusher.ceiling_pace.insert(sec, moved / since);
                }
            }
            self.seen_heights[sec] = h;
        }
        // Keep both height buffers: the crusher logic needs the previous tic, while the
        // current snapshot can reuse the allocation from two tics ago.
        std::mem::swap(&mut self.prev_live, &mut self.live);
        self.live.clone_from(&obs.heights);
        let pos = obs.pos();
        if self.last_pos.is_some_and(|l| dist(pos, l) > 64.0) {
            // (to where the teleporter puts her, that is: a jump in where she is to anywhere else
            // is the fight's doing, her legs its own for a while, not the teleporter's; taken for
            // the teleport ahead, the way on from it is followed from where she stands, off
            // course, and planned again and again)
            let lands = self
                .segments
                .get(self.seg_i + 1)
                .map(|seg| seg.track.point_at(0.0));
            if self.segments[self.seg_i].end == SegEnd::Teleport
                && lands.is_some_and(|at| dist(pos, at) <= 64.0)
            {
                self.log.add(obs.tic, "teleported");
                self.seg_i += 1;
                self.enter_segment(obs.tic);
            } else {
                self.log.add(
                    obs.tic,
                    format!("moved unexpectedly to ({:.0}, {:.0})", obs.x, obs.y),
                );
                if matches!(self.phase, Phase::Act | Phase::Fetch) {
                    // (teleported off the spot she was acting from: a press from there is no
                    // try, the use ray reaches 64. Walk back to it: a dodge onto a teleporter
                    // beside a switch isn't the switch doing nothing)
                    self.start_step(obs.tic);
                }
                self.settle_until = obs.tic; // (judge where she is straight away)
            }
        }
        if let Some(last) = self.last_pos {
            if self.step().action == Action::Cross && !self.crossed {
                let (a, b) = nav.nav.map.line_seg(self.trigger_line_no());
                // (a teleporter goes off only crossed from its front, the right of its run, as
                // the engine has it: over it from behind is not crossing it)
                let from_front = (b.0 - a.0) * (last.1 - a.1) - (b.1 - a.1) * (last.0 - a.0) < 0.0;
                let front_only = self
                    .step()
                    .trigger
                    .as_ref()
                    .is_some_and(|t| t.info.category == crate::specials::Category::Teleport);
                self.crossed = segments_cross(last, pos, a, b) && (from_front || !front_only);
            }
        }
        self.last_pos = Some(pos);
        if obs.tic - self.step_tic > GIVE_UP_TICS {
            return self.fail(obs, Trouble::StepSlow(self.step_i as usize + 1));
        }
        self.check_watches(obs);
        if let Some(k) = obs.keys {
            self.keys = Keys(self.keys.0 | k.0);
        }
        if self.mortal && !self.segments.is_empty() {
            // (mortal: out from under a ceiling coming down before anything, whatever she's at,
            // a lift's window waited for included)
            if let Some(c) = self.pinned_under_ceiling(obs) {
                return c;
            }
            if self.phase != Phase::Walk {
                if let Some(c) = self.crusher_guard(obs) {
                    return c;
                }
            }
        }
        match self.phase {
            Phase::Walk => self.walk(obs),
            Phase::Fetch => self.fetch(obs),
            Phase::Settle => self.settle_step(obs),
            Phase::Act => self.act_at_goal(obs),
        }
    }

    fn fail(&mut self, obs: &Observation, why: Trouble) -> Controls {
        self.failed = true;
        let label = self.step().label.clone();
        let line = format!(
            "FAILED at ({:.0}, {:.0}): {why} (step {}: {label})",
            obs.x,
            obs.y,
            self.step_i + 1
        );
        self.failed_stuck = line.contains("stuck");
        self.log.add(obs.tic, line);
        self.trouble = Some(why);
        Controls::default()
    }

    // --- recovering ---------------------------------------------------------------------------

    /// (poly, point in it) of the floor she's really standing on. Doom holds the box up on the
    /// highest floor under any of it, so her centre can be out over a pit beside a walkway
    /// narrower than the box with nothing wrong.
    fn standing(&self, obs: &Observation) -> (Option<usize>, Point) {
        let mesh = &self.view.nav.mesh;
        let here = mesh.poly_at(obs.pos());
        let (Some(z), Some(h)) = (obs.z, here) else {
            return (here, obs.pos());
        };
        if (obs.heights[mesh.poly(h).sector].floor - z).abs() < 0.5 {
            return (here, obs.pos());
        }
        let r = PLAYER_RADIUS - 1.0;
        let offs = [-r, -r / 2.0, 0.0, r / 2.0, r];
        let mut best: Option<(f64, usize, Point)> = None;
        for dx in offs {
            for dy in offs {
                let q = (obs.x + dx, obs.y + dy);
                if let Some(p) = mesh.poly_at(q) {
                    let poly = mesh.poly(p);
                    if (obs.heights[poly.sector].floor - z).abs() < 0.5
                        && inside(&poly.vertices, q.0, q.1)
                    {
                        let d = dx.hypot(dy);
                        if best.is_none_or(|(bd, _, _)| d < bd) {
                            best = Some((d, p, q));
                        }
                    }
                }
            }
        }
        best.map_or((here, obs.pos()), |(_, p, q)| (Some(p), q))
    }

    /// Off the planned path (into a pit, pushed, an unexpected teleport) for a while.
    fn lost(&mut self, obs: &Observation) -> bool {
        if obs.tic < self.settle_until {
            return false;
        }
        let mesh = &self.view.nav.mesh;
        let here = self.standing(obs).0;
        let mut off = self.at.off > OFF_COURSE;
        if !off && here.is_none_or(|h| !self.path_polys.contains(&h)) {
            if self.at.off > STRAY {
                off = true;
            } else {
                // beside the path, but somewhere she can't step back up from
                let q = mesh.poly_at(self.segments[self.seg_i].track.point_at(self.at.along));
                if let (Some(h), Some(q)) = (here, q) {
                    let (hs, qs) = (mesh.poly(h).sector, mesh.poly(q).sector);
                    if hs != qs && obs.heights[hs].floor < obs.heights[qs].floor - MAX_STEP {
                        off = true;
                    }
                }
            }
        }
        if !off {
            self.off_since = None;
            return false;
        }
        let since = *self.off_since.get_or_insert(obs.tic);
        obs.tic - since >= OFF_COURSE_TICS
    }

    /// The heights the world can take for replanning: what the plan has unlocked so far, and
    /// what she can see now.
    pub fn world_states(&self, obs: &Observation) -> States {
        let mut states = self.states.clone();
        for (s, &h) in obs.heights.iter().enumerate() {
            if s < states.len() && !states[s].contains(&h) {
                states[s].push(h);
            }
        }
        states
    }

    fn live_states(obs: &Observation) -> States {
        obs.heights.iter().map(|&h| vec![h]).collect()
    }

    /// Just landed a jump: the way on to the step's goal from where she is (the plan's path on
    /// from a landing was smoothed across the jump and can wander back off the ledge it lands
    /// on). Not a replan: the plan holds.
    fn path_on(&mut self, obs: &Observation) {
        let Some(goal) = self.step().path.as_ref().and_then(last_point) else {
            return;
        };
        let doors = self.view.local_doors(self.keys);
        if let Some(path) = self.view.find_path(
            self.standing(obs).1,
            goal,
            &self.world_states(obs),
            Some(&doors),
        ) {
            self.steps[self.step_i as usize].path = Some(path);
            self.start_step(obs.tic);
        }
    }

    /// Re-path the current step from here. If its goal can't be walked to, plan a way there with
    /// triggers (lifts, switches out of a pit); failing that, plan the level again. `live_only`:
    /// with only the heights as they are now (a lift that went back up has to be called again).
    fn replan(&mut self, obs: &Observation, why: &str, live_only: bool) -> bool {
        self.replans += 1;
        self.step_replans += 1;
        if self.step_replans > MAX_REPLANS || self.replans > MAX_TOTAL_REPLANS {
            self.log.add(obs.tic, format!("{why}: replanned too often"));
            return false;
        }
        let view = Rc::clone(&self.view);
        let pos = self.standing(obs).1;
        let step = self.step().clone();
        let Some(goal) = step.path.as_ref().and_then(last_point) else {
            return false;
        };
        let states = if live_only {
            Self::live_states(obs)
        } else {
            self.world_states(obs)
        };
        let doors = view.local_doors(self.keys);
        if let Some(path) = view.find_path(pos, goal, &states, Some(&doors)) {
            self.steps[self.step_i as usize].path = Some(path);
            self.log.add(
                obs.tic,
                format!(
                    "{why} at ({:.0}, {:.0}): new path to ({:.0}, {:.0})",
                    obs.x, obs.y, goal.0, goal.1
                ),
            );
        } else {
            let route = view.plan(
                pos,
                &[goal],
                &crate::navigator::Plan {
                    keys: self.keys,
                    states: Some(&states),
                    max_expansions: 200,
                    max_seconds: 600.0,
                    fired: &self.fired,
                    ..Default::default()
                },
            );
            if let Some(route) = route {
                let new = steps_from_legs(&view, &route.legs, Some(step), Some(&states));
                let n = new.len();
                let i = self.step_i as usize;
                self.steps.splice(i..=i, new);
                self.log.add(
                    obs.tic,
                    format!(
                        "{why} at ({:.0}, {:.0}): back on track via {} extra step(s)",
                        obs.x,
                        obs.y,
                        n as i64 - 1
                    ),
                );
            } else if self.replan_level {
                let plan = plan_from(
                    &view,
                    pos,
                    &Rest {
                        keys: self.keys,
                        states: Some(&states),
                        reach: None,
                        max_seconds: 600.0,
                        max_expansions: 1000,
                        fired: &self.fired,
                    },
                );
                if !plan.complete {
                    self.log.add(
                        obs.tic,
                        format!("{why}: no way to the step goal or the exit"),
                    );
                    return false;
                }
                let n = plan.steps.len();
                self.steps.truncate(self.step_i as usize);
                self.steps.extend(plan.steps);
                self.log.add(
                    obs.tic,
                    format!(
                        "{why} at ({:.0}, {:.0}): replanned the rest of the level ({n} steps)",
                        obs.x, obs.y
                    ),
                );
            } else {
                self.log
                    .add(obs.tic, format!("{why}: no way to the step goal"));
                return false;
            }
        }
        self.jump.sprint = None;
        self.start_step(obs.tic);
        true
    }
}

/// Speed to pass the end of a step not stopped at, by where the next one goes.
fn roll_speed(seg: &Segment) -> f64 {
    let look = &seg.look.pts;
    let n = seg.track.pts.len();
    if n < 2 || look.len() <= n {
        return FULL_SPEED * 0.6;
    }
    let turn = norm(heading(look[n - 1], look[n]) - heading(look[n - 2], look[n - 1])).abs();
    super::corner_speed(turn)
}
