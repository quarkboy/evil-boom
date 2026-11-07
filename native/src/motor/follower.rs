//! Carrying out a planned route a tic at a time.
//!
//! Each tic the follower gets an Observation (where she is, which way she faces, the live
//! sector heights) and gives Controls. Live heights decide whether the next crossing on the
//! path can be made now; if not, she opens the door in front, or waits for the lift or door,
//! and as a last resort uses a trigger nearby for that sector. Off course (into a pit, over a
//! teleporter) or stuck, she replans the rest of the step from where she is.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::{fmt_live, heading, norm, Controls, Motion, Observation, Track, BRAKE, FULL_SPEED, RUN_TURN, TICRATE};
use crate::geom::{cells, closest_point, dist, segment_distance, segments_cross, Point};
use crate::jumps::RunStart;
use crate::mesh::{inside, Portal};
use crate::nav::{can_cross, Doors, Heights};
use crate::navigator::{closest_on_portal, rep, Jump, Path, States, View, ITEM_RADIUS, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS, TELEPORTERS_FOR_PLAYERS};
use crate::specials::{Category, Key, Keys, Trigger};
use crate::walkthrough::{fmt_heights, plan_from, steps_from_legs, Action, Expect, Rest, Step};

/// arrival radius for points passed through
const REACHED: f64 = 12.0;
/// ...for points stopped at
const FINAL_REACHED: f64 = 10.0;
/// ...for pickup spots (they can be near the edge of touching range)
const PICKUP_REACHED: f64 = 6.0;
/// tics between re-aims running over sinking floors
const RUN_REAIM: i64 = 4;
/// how much lower a sinking floor is taken to be by the time she's off it
const RUN_AHEAD_SINK: f64 = 4.0;
/// a barrel this close when stuck is what's in the way
const BARREL_IN_WAY: f64 = 40.0;
/// back off this far before shooting one (blast radius 128)
const BARREL_SAFE: f64 = 150.0;
const BARREL_GIVE_UP: i64 = 8 * 35;
/// What to shoot something with: the pistol (the shotgun if there's nothing for it), whatever's
/// in hand: not the BFG with 20 cells of the 40 it needs (MAP23), nor a missile, which sets off no
/// gun line.
pub fn tap_gun(obs: &Observation) -> i32 {
    // (ammo: (weapon slot, rounds for it))
    let rounds = |slot: i32| obs.ammo.as_ref().and_then(|a| a.iter().find(|w| w.0 == slot)).map_or(1, |w| w.1);
    if rounds(2) > 0 {
        2
    } else {
        3
    }
}

/// the longest wait under a ceiling too low for her before it counts as being stuck
const PINNED_WAIT: i64 = 10 * 35;
/// how far into its range of floors a gun line is shot from
const SHOT_MARGIN: f64 = 2.0;
/// a drop this near ahead onto a narrow landing...
const CREEP_RANGE: f64 = 48.0;
/// ...narrower than this the way she's going...
const LANDING_ROOM: f64 = 192.0;
/// ...is stepped off no slower than this (units a tic)
const DROP_CREEP: f64 = 1.0;
/// tics between shots at a barrel or a wall (the pistol ready again, the trigger let go: a first shot each time)
pub const BARREL_TAP: i64 = 20;
/// a jump goes without settling if what it needs runs out sooner
const JUMP_HURRY: f64 = 2.0 * TICRATE;
/// how much lower than planned a ceiling can be for a jump
const JUMP_CEILING_SLACK: f64 = 16.0;
/// the longest wait for a lift to come back before crossing its line
const CROSS_REST_WAIT: i64 = 8 * 35;
/// a sector that moved this lately may still be busy (Doom ignores a switch on it)...
const BUSY_TICS: i64 = 200;
/// ...what moves this near a switch counts (she can see it moving)
const BUSY_NEAR: f64 = 384.0;
/// ...unless it's back where it rests and has been still this long
const REST_TICS: i64 = 12;
/// the longest wait for things round a switch to settle before pressing it
const SETTLE_WAIT: i64 = 10 * 35;
/// how far ahead to look for crossings that became one-way drops
const DROP_CHECK_DIST: f64 = 48.0;
/// tics between use presses waiting at a door
const USE_EVERY: i64 = 15;
/// how exactly to hit a jump's run-up
const RUNUP_TOLERANCE: f64 = 6.0;
/// going down steps: in the air between them braking does nothing
const STAIRS_DOWN_SPEED: f64 = 8.0;
/// along a thin ledge (drops either side of the box): walk, don't run
const LEDGE_SPEED: f64 = 6.0;
const LEDGE_NEAR: f64 = PLAYER_RADIUS + 8.0;
const CAUTION_AHEAD: f64 = 160.0;
/// steer towards the path this far ahead
const TRACK_AHEAD: f64 = 32.0;
/// ...and look this far ahead
const LOOK_AHEAD: f64 = 128.0;
/// look-aheads to try steering for (longest first)
const CARROT_STEPS: [f64; 4] = [72.0, 56.0, 40.0, 28.0];
/// how close to a lift or mover before live heights are checked
const BLOCK_CHECK_DIST: f64 = 48.0;
/// ...and to a door (so she slows and walks up to it)
const DOOR_CHECK_DIST: f64 = 128.0;
/// where to stand to press a door
const DOOR_STANDOFF: f64 = 36.0;
/// no progress this long while trying to move: back off and try again
const STUCK_TICS: i64 = 35;
const UNSTICK_TICS: i64 = 12;
const UNSTICKS_BEFORE_REPLAN: i32 = 2;
/// off the path and outside its polys by this much: off course
const STRAY: f64 = 40.0;
/// this far from the path anywhere: off course
const OFF_COURSE: f64 = 96.0;
/// ...for this long
const OFF_COURSE_TICS: i64 = 10;
/// a step
const MAX_REPLANS: i32 = 6;
/// presses (creeping closer) before giving up on a switch
const PRESS_TRIES: i32 = 3;
/// how near a door to press it (use reaches 64)
const PRESS_RANGE: f64 = 56.0;
/// don't stand still for moves to finish: some floors are hopped onto rising
const SETTLE_MAX: f64 = 0.0;
/// a route (replans that add steps start the per-step count again)
const MAX_TOTAL_REPLANS: i32 = 30;
/// one step taking this long: give up
const GIVE_UP_TICS: i64 = 60 * 35;

/// The actions she stops at the end of (the explorer's goto included: where it goes next is
/// anyone's guess; E3M3 slid off a ledge it arrived at running).
fn stops(a: Action) -> bool {
    matches!(a, Action::Use | Action::Shoot | Action::Exit | Action::Face | Action::Kill | Action::Endure | Action::Walk)
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
        self.events.push(format!("[{:6.1}s] {}", tic as f64 / TICRATE, msg.as_ref()));
    }
}

/// What a drop ahead the plan didn't mean to take came to.
enum Drop {
    None,
    /// another way on, without it
    Replanned,
    /// nothing but such drops: the objective's to be judged again
    GiveBack(String),
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
    Controls { forward: 50.0 * rad.cos(), strafe: -50.0 * rad.sin(), turn: (-d).clamp(-RUN_TURN, RUN_TURN), ..Controls::default() }
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
    pub replans: i32,
    /// once-only stair switches used (for replans)
    pub fired: BTreeSet<usize>,
    /// run-jumps that came down off their landing (for the explorer)
    pub missed: Vec<Jump>,
    /// (where, line) pressed from there to no effect (for the explorer)
    pub failed_presses: Vec<(Point, usize)>,
    drops_ok: BTreeSet<(usize, usize, [u64; 4])>,
    edge_wait: bool,
    settle_logged: i64,
    cross_ready: bool,
    cross_wait_logged: bool,
    goal_tic: i64,
    last_pos: Option<Point>,
    /// (jump, heading, start tic) while sprinting
    sprint: Option<(Jump, f64, i64)>,
    /// (jump, tic) a run-jump done: where does it come down?
    touchdown: Option<(Jump, i64)>,
    rest_logged: i64,
    rest_tic: i64,
    seen_heights: Vec<Heights>,
    moved_at: BTreeMap<usize, i64>,
    run: Option<Run>,
    /// (where, since tic) a barrel in the way being shot
    barrel: Option<(Point, i64)>,
    /// the tic of the last shot at it
    barrel_shot: i64,
    sinking: BTreeSet<usize>,
    lineup_tic: Option<i64>,
    brake_until: i64,
    /// (sector, live heights, since)
    jump_wait: Option<(usize, Option<Heights>, i64)>,
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
    /// since when the box has been under a ceiling too low for her
    pinned_since: Option<i64>,
    /// the floor under her last tic, waiting to shoot (still moving?)
    shot_floor: Option<f64>,
    segments: Vec<Segment>,
    seg_i: usize,
    poly_i: usize,
    phase: Phase,
    path_polys: BTreeSet<usize>,
    crossing_lines: BTreeSet<usize>,
    off_since: Option<i64>,
    unsticks: i32,
    unstick_until: i64,
    unstick_dir: f64,
    // the segment
    k: usize,
    s: f64,
    cross: f64,
    settle_until: i64,
    best_s: f64,
    progress_tic: i64,
    wait_tic: Option<i64>,
    action_tic: Option<i64>,
    mover_heights: Option<Heights>,
    mover_still_since: i64,
    last_use: i64,
    speed_cap: f64,
    live: Vec<Heights>,
}

impl RouteFollower {
    /// `keys` held and `states` (the heights the world can take) at the start; `tic`: the game
    /// tic it starts at.
    pub fn new(view: Rc<View>, steps: Vec<Step>, keys: Keys, states: Option<States>, replan_level: bool, tic: i64) -> RouteFollower {
        let doors = view.local_doors(Key::ALL.into_iter().collect());
        let lift_sectors = view.triggers.iter().filter(|t| !t.permanent()).flat_map(|t| t.effects.iter().map(|(s, _)| *s)).collect();
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
            replans: 0,
            fired: BTreeSet::new(),
            missed: Vec::new(),
            failed_presses: Vec::new(),
            drops_ok: BTreeSet::new(),
            edge_wait: false,
            settle_logged: -1,
            cross_ready: false,
            cross_wait_logged: false,
            goal_tic: 0,
            last_pos: None,
            sprint: None,
            touchdown: None,
            rest_logged: -1,
            rest_tic: 0,
            seen_heights: Vec::new(),
            moved_at: BTreeMap::new(),
            run: None,
            barrel: None,
            barrel_shot: -1_000_000_000,
            sinking,
            lineup_tic: None,
            brake_until: -1,
            jump_wait: None,
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
            pinned_since: None,
            shot_floor: None,
            segments: Vec::new(),
            seg_i: 0,
            poly_i: 0,
            phase: Phase::Walk,
            path_polys: BTreeSet::new(),
            crossing_lines: BTreeSet::new(),
            off_since: None,
            unsticks: 0,
            unstick_until: -1,
            unstick_dir: 0.0,
            k: 0,
            s: 0.0,
            cross: 0.0,
            settle_until: 0,
            best_s: f64::NEG_INFINITY,
            progress_tic: 0,
            wait_tic: None,
            action_tic: None,
            mover_heights: None,
            mover_still_since: 0,
            last_use: -USE_EVERY,
            speed_cap: f64::INFINITY,
            live: Vec::new(),
        };
        f.next_step(tic);
        f
    }

    fn step(&self) -> &Step {
        &self.steps[self.step_i as usize]
    }

    fn trigger_line_no(&self) -> usize {
        self.step().trigger.as_ref().and_then(|t| t.linedef()).unwrap_or(0)
    }

    /// In the middle of a run-jump (lining up, sprinting, running over sinking floors).
    pub fn in_jump(&self) -> bool {
        self.sprint.is_some() || self.run.is_some() || self.lineup_tic.is_some()
    }

    /// Run-jumps still to come on the route.
    pub fn jumps_ahead(&self) -> bool {
        let from = usize::try_from(self.step_i).unwrap_or(0);
        self.steps.iter().skip(from).filter_map(|st| st.path.as_ref()).any(|p| p.steps.iter().any(|n| n.contains("run-jump")))
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
            Phase::Settle => self.settle.iter().map(|e| format!("wait for sector {}", e.sector)).collect::<Vec<_>>().join(", "),
            Phase::Walk => subs[0].clone(),
            _ => subs.get(1).cloned().unwrap_or_else(|| step.action.name().to_string()),
        };
        format!("step {}/{}: {sub} ({})", self.step_i + 1, self.steps.len(), step.label)
    }

    /// The rest of the current step's path from `pos` (None: a teleport or jump).
    pub fn planned_path(&self, pos: Option<Point>) -> Vec<Option<Point>> {
        if self.done || self.failed {
            return Vec::new();
        }
        let mut out: Vec<Option<Point>> = pos.into_iter().map(Some).collect();
        for i in self.seg_i..self.segments.len() {
            let pts = &self.segments[i].track.pts;
            let from = if i == self.seg_i { (self.k + 1).min(pts.len()) } else { 0 };
            out.extend(pts[from..].iter().map(|&p| Some(p)));
            if i + 1 < self.segments.len() {
                out.push(None);
            }
        }
        out
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
                    // (MAP29: one routed over a floor where it had risen from a minute before)
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
                    self.watches.push(Watch { expect: e.clone(), since: tic, line: t.line, reached: None, late: false, over: false, step: self.step_i, used: false });
                }
            }
            if let Some(k) = done.key {
                self.keys = self.keys.with(k);
            }
            // a quick permanent move the next path needs: let it finish first
            let settle: Vec<Expect> = done.expects.iter().filter(|e| e.needed && e.hold.is_none() && e.travel <= SETTLE_MAX).cloned().collect();
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
        self.log.add(tic, format!("step {}/{}: {subs}", self.step_i + 1, self.steps.len()));
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
        let path = self.step().path.clone().unwrap_or_else(|| Path { polys: Vec::new(), waypoints: Vec::new(), cost: 0.0, steps: Vec::new(), jumps: Vec::new(), squeezes: 0 });
        self.path_polys = path.polys.iter().copied().collect();
        // ledge edges the path means to cross (any others count as walls cutting corners)
        let mesh = &self.view.nav.mesh;
        self.crossing_lines = path
            .polys
            .windows(2)
            .flat_map(|w| {
                let (a, b) = (w[0], w[1]);
                mesh.poly(a).portals.iter().filter(move |&&pi| mesh.portals[pi].other(a) == b).filter_map(|&pi| mesh.portals[pi].linedef)
            })
            .collect();
        self.off_since = None;
        self.unsticks = 0;
        self.unstick_until = -1;
        self.enter_segment(tic);
    }

    fn enter_segment(&mut self, tic: i64) {
        self.k = 0;
        self.s = 0.0;
        self.cross = 0.0;
        self.settle_until = tic + OFF_COURSE_TICS;
        self.reset_progress(tic);
    }

    /// The waypoints in segments, split at teleports and jump run-ups.
    fn split(&self) -> Vec<Segment> {
        let step = self.step();
        let Some(path) = &step.path else { return Vec::new() };
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
                // itself is dropped; a nearby one can be where the step ends: E4M6's switch is
                // 26 past the landing, and a track of just that point is "already there")
                cur = vec![jump.dest];
                let land = (i + 2..pts.len()).find(|&k| pts[k].is_some_and(|q| dist(q, jump.dest) < 32.0));
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
                    seg_pts = vec![jump.as_ref().map_or_else(|| last_point(path).unwrap_or((0.0, 0.0)), |j| j.dest)];
                }
                let tail = self.look_tail(&seg_pts, end, jump.as_ref(), idx == n - 1);
                let mut look = seg_pts.clone();
                look.extend(tail);
                Segment { track: Track::new(seg_pts), end, jump, look: Track::new(look) }
            })
            .collect()
    }

    /// Where to look once the segment's own points run out.
    fn look_tail(&self, pts: &[Point], end: SegEnd, jump: Option<&Jump>, _last: bool) -> Vec<Point> {
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
                return p.waypoints.iter().skip(1).take(5).filter_map(|w| *w).take(3).collect();
            }
        }
        Vec::new()
    }

    /// Something else had the controls till now (the explorer looking round): that time isn't
    /// time stuck.
    pub fn resume(&mut self, tic: i64) {
        self.reset_progress(tic);
    }

    fn reset_progress(&mut self, tic: i64) {
        self.best_s = f64::NEG_INFINITY;
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
                    let f = nav.nav.trench_floor(t, |x| obs.heights.get(x).map_or(f64::INFINITY, |h| h.floor));
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
        for (sec, &h) in obs.heights.iter().enumerate() {
            if self.seen_heights[sec] != h {
                self.moved_at.insert(sec, obs.tic);
            }
            self.seen_heights[sec] = h;
        }
        self.live = obs.heights.clone();
        let pos = obs.pos();
        if self.last_pos.is_some_and(|l| dist(pos, l) > 64.0) {
            if self.seg_i + 1 < self.segments.len() && self.segments[self.seg_i].end == SegEnd::Teleport {
                self.log.add(obs.tic, "teleported");
                self.seg_i += 1;
                self.enter_segment(obs.tic);
            } else {
                self.log.add(obs.tic, format!("moved unexpectedly to ({:.0}, {:.0})", obs.x, obs.y));
                self.settle_until = obs.tic; // (judge where she is straight away)
            }
        }
        if let Some(last) = self.last_pos {
            if self.step().action == Action::Cross && !self.crossed {
                let (a, b) = nav.nav.map.line_seg(self.trigger_line_no());
                self.crossed = segments_cross(last, pos, a, b);
            }
        }
        self.last_pos = Some(pos);
        if obs.tic - self.step_tic > GIVE_UP_TICS {
            return self.fail(obs, &format!("step {} taking too long", self.step_i + 1));
        }
        self.check_watches(obs);
        if let Some(k) = obs.keys {
            self.keys = Keys(self.keys.0 | k.0);
        }
        match self.phase {
            Phase::Walk => self.walk(obs),
            Phase::Fetch => self.fetch(obs),
            Phase::Settle => self.settle_step(obs),
            Phase::Act => self.act_at_goal(obs),
        }
    }

    fn fail(&mut self, obs: &Observation, why: &str) -> Controls {
        self.failed = true;
        let label = self.step().label.clone();
        self.log.add(obs.tic, format!("FAILED at ({:.0}, {:.0}): {why} (step {}: {label})", obs.x, obs.y, self.step_i + 1));
        Controls::default()
    }

    // --- walking --------------------------------------------------------------------------

    fn walk(&mut self, obs: &Observation) -> Controls {
        if self.sprint.is_some() {
            return self.sprint_jump(obs);
        }
        if self.run.is_some() {
            return self.run_step(obs);
        }
        if self.barrel.is_some() {
            return self.shoot_barrel(obs);
        }
        let nav = Rc::clone(&self.view);
        let step = self.step().clone();
        if let Some(t) = step.trigger.as_ref().filter(|t| step.action == Action::Cross && t.info.trigger == Trigger::WR && !t.permanent()) {
            if !self.cross_ready {
                // A lift line to cross for its window: if the lift's away already (set off on the
                // way, MAP09), crossing now does nothing and the window is shorter than planned.
                let init = &nav.nav.movers.initial;
                let away: Vec<usize> = t.effects.iter().map(|(s, _)| *s).filter(|&x| obs.height(x).is_some_and(|live| (live.floor - init[x].floor).abs() > 0.5)).collect();
                if !away.is_empty() && obs.tic - self.step_tic < CROSS_REST_WAIT {
                    if !self.cross_wait_logged {
                        self.log.add(obs.tic, format!("sector {} already away: waiting for it to come back before crossing line {}", away[0], t.line));
                        self.cross_wait_logged = true;
                    }
                    self.progress_tic = obs.tic;
                    return self.motion.hold(obs, (0.0, 0.0), Some(obs.angle));
                }
                self.cross_ready = true;
            }
        }
        if obs.tic < self.brake_until {
            if self.motion.speed() > 1.0 {
                // just landed a jump: stop first
                self.progress_tic = obs.tic;
                return self.motion.hold(obs, (0.0, 0.0), self.look);
            }
            self.brake_until = -1;
        }
        let seg = self.segments[self.seg_i].clone();
        let track = &seg.track;
        let pos = obs.pos();
        (self.k, self.s, self.cross) = track.project(pos, self.s, 64.0);
        if self.lost(obs) {
            if !self.replan(obs, "off course", false) {
                return self.fail(obs, "off course and no way back");
            }
            return self.walk(obs);
        }
        self.speed_cap = f64::INFINITY;
        match self.unexpected_drop(obs) {
            Drop::None => {}
            Drop::Replanned => return self.walk(obs),
            Drop::GiveBack(why) => return self.fail(obs, &why),
        }
        self.creep_to_drop(obs);
        if let Some(c) = self.check_blocked(obs) {
            return c;
        }
        let end = track.pts[track.pts.len() - 1];
        let last = self.seg_i == self.segments.len() - 1;
        let stop = seg.end == SegEnd::Jump || (last && stops(step.action));
        // near the end only counts once she's come along the track (paths double back)
        let to_end = if track.length - self.s < 48.0 { dist(pos, end) } else { f64::INFINITY };
        if seg.end == SegEnd::Jump && to_end <= RUNUP_TOLERANCE {
            if let Some(j) = &seg.jump {
                return self.line_up(obs, j);
            }
        }
        if step.action == Action::Pickup && obs.keys.is_some_and(|k| step.key.is_some_and(|sk| k.has(sk))) {
            self.phase = Phase::Act; // (got it, maybe on the way)
            return self.act_at_goal(obs);
        }
        let arrive = if stop { FINAL_REACHED } else if step.action == Action::Pickup { PICKUP_REACHED } else { REACHED };
        if seg.end == SegEnd::Goal && (to_end < arrive || self.touching_item(pos)) {
            self.reset_progress(obs.tic);
            if step.action == Action::Pickup && obs.keys.is_some() && self.item.is_some() {
                self.phase = Phase::Fetch; // (at the spot but not holding it: go and get it)
                return self.fetch(obs);
            }
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        // progress, and getting unstuck
        if self.s > self.best_s + 2.0 {
            if self.best_s != f64::NEG_INFINITY {
                self.unsticks = 0; // (got past whatever snagged her)
            }
            self.best_s = self.s;
            self.progress_tic = obs.tic;
        }
        if obs.tic < self.unstick_until {
            return self.unstick(obs);
        }
        let trig_line = step.trigger.as_ref().and_then(|t| t.linedef());
        if obs.tic - self.progress_tic > STUCK_TICS && step.action == Action::Use && self.seg_i == self.segments.len() - 1 && trig_line.is_some_and(|li| self.line_distance(pos, li) <= PRESS_RANGE) {
            // can't get to the spot, but the switch is in reach from here (MAP27's, past a shut
            // door the press opens first): press from here
            self.reset_progress(obs.tic);
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        if let Some(c) = self.pinned_under_ceiling(obs) {
            return c;
        }
        if obs.tic - self.progress_tic > STUCK_TICS {
            self.unsticks += 1;
            if self.unsticks > UNSTICKS_BEFORE_REPLAN {
                // (with the heights as they are: the way the plan saw may be shut now; MAP23's
                // squeezed past a timed door that had shut, re-pathed the same way for ever)
                if !self.replan(obs, "stuck", true) {
                    return self.fail(obs, "stuck");
                }
                return self.walk(obs);
            }
            if let Some(b) = obs.barrels.iter().flatten().copied().find(|&b| dist(b, pos) < BARREL_IN_WAY) {
                // wedged against a barrel (MAP23 has rows of them across corridors): blow it up
                // from a safe distance, as a player would
                self.log.add(obs.tic, format!("barrel in the way at ({:.0}, {:.0}): shooting it", b.0, b.1));
                self.barrel = Some((b, obs.tic));
                return self.shoot_barrel(obs);
            }
            self.log.add(obs.tic, format!("stuck at ({:.0}, {:.0}), backing off", obs.x, obs.y));
            self.unstick_until = obs.tic + UNSTICK_TICS;
            self.progress_tic = obs.tic + UNSTICK_TICS;
            self.unstick_dir = self.away_from_walls(obs);
            return self.unstick(obs);
        }
        // follow the track: steer for a point a little ahead, look further ahead
        let mut carrot = self.carrot(pos, track);
        if seg.end == SegEnd::Teleport && to_end < REACHED {
            carrot = seg.look.pts[seg.look.pts.len() - 1]; // (keep going through the line)
        }
        let end_speed = if stop {
            0.0
        } else if seg.end == SegEnd::Teleport {
            FULL_SPEED
        } else {
            roll_speed(&seg)
        };
        // what's left of the track: off its end, as far as the end really is (MAP24: braked to a
        // stop 25 past a run-up the track ended at, and stood there "arrived")
        let rest = (track.length - self.s).max(dist(pos, end));
        let mut speed = FULL_SPEED.min(track.speed_limit(track.length - rest, end_speed)).min(self.speed_cap);
        if stop {
            speed = speed.min(0.3 * to_end + 0.5);
        }
        if seg.end == SegEnd::Jump {
            // run-ups are often right at a ledge: in gently, slowly over the last stretch
            // (braking does nothing between steps down: MAP17's run-up at the foot of a stair
            // was overshot into the pit beyond)
            if self.steps_down_to_end(obs, track) {
                speed = speed.min(0.06 * rest + 1.5).min(if rest < 128.0 { 6.0 } else { FULL_SPEED });
            } else {
                speed = speed.min(0.12 * rest + 1.5);
            }
        }
        speed = speed.min(self.caution_speed(obs, track));
        let look = self.look_angle(obs, &seg);
        if dist(pos, carrot) < 1.0 {
            return self.motion.hold(obs, (0.0, 0.0), Some(look));
        }
        self.motion.go(obs, heading(pos, carrot), speed, Some(look))
    }

    /// How fast to go for what's just ahead: down steps (braking does nothing dropping
    /// between them), and along a thin ledge with drops close by on both sides (walk it).
    fn caution_speed(&self, obs: &Observation, track: &Track) -> f64 {
        let nav = &self.view.nav;
        let (mesh, m) = (&nav.mesh, &nav.map);
        let drops = nav.drop_lines();
        let mut steps_down: Vec<f64> = Vec::new();
        let mut prev: Option<f64> = None;
        let ahead = CAUTION_AHEAD.min(track.length - self.s);
        let n = ((ahead / 16.0).floor() as i64).max(1);
        for i in 0..=n {
            let f = ahead * i as f64 / n as f64;
            let q = track.point_at(self.s + f);
            let t = track.point_at(self.s + f + 1.0);
            let (dx, dy) = (t.0 - q.0, t.1 - q.1);
            let Some(poly) = mesh.poly_at(q) else { continue };
            let floor = obs.height(mesh.poly(poly).sector).map_or(0.0, |h| h.floor);
            if prev.is_some_and(|p| floor < p) {
                steps_down.push(f);
            }
            prev = Some(floor);
            let mut sides: BTreeSet<bool> = BTreeSet::new();
            for cell in cells(q.0 - LEDGE_NEAR, q.1 - LEDGE_NEAR, q.0 + LEDGE_NEAR, q.1 + LEDGE_NEAR) {
                for &li in mesh.line_grid.get(&cell).into_iter().flatten() {
                    if !drops.contains(&li) {
                        continue;
                    }
                    let (Some(fs), Some(bs)) = m.sides(li) else { continue };
                    let (Some(hf), Some(hb)) = (obs.height(fs), obs.height(bs)) else { continue };
                    if (hf.floor - hb.floor).abs() <= MAX_STEP {
                        continue; // (not a drop at the heights now)
                    }
                    let (a, c) = m.line_seg(li);
                    if segment_distance(a, c, q, q) < LEDGE_NEAR {
                        let near = closest_point(q, a, c);
                        sides.insert(dx * (near.1 - q.1) - dy * (near.0 - q.0) > 0.0);
                    }
                }
            }
            if sides.len() == 2 {
                return LEDGE_SPEED;
            }
        }
        if steps_down.len() < 2 {
            return f64::INFINITY; // (one drop isn't a stair: MAP09's slowed for a 48 down missed a lift)
        }
        (STAIRS_DOWN_SPEED * STAIRS_DOWN_SPEED + 2.0 * BRAKE * steps_down[0]).sqrt()
    }

    /// Where the item a pickup step is for lies (the nearest such thing to its goal).
    fn item_of(&self) -> Option<Point> {
        let step = self.step();
        let key = step.key.filter(|_| step.action == Action::Pickup)?;
        let goal = last_point(step.path.as_ref()?)?;
        self.view.nav.map.things.iter().filter(|t| t.kind == key as u16).map(|t| (t.x, t.y)).min_by(|a, b| dist(*a, goal).total_cmp(&dist(*b, goal)))
    }

    /// Close enough that the boxes overlap: picked up, wherever the spot was.
    fn touching_item(&self, pos: Point) -> bool {
        self.item.is_some_and(|it| (pos.0 - it.0).abs().max((pos.1 - it.1).abs()) < PLAYER_RADIUS + ITEM_RADIUS - 1.0)
    }

    /// The point to steer for: as far ahead on the track as the straight line there keeps the
    /// box clear (corners rounded where there's room), else the next waypoint exactly.
    fn carrot(&self, pos: Point, track: &Track) -> Point {
        for ahead in CARROT_STEPS {
            let q = track.point_at(self.s + ahead);
            // (a path that doubles back can put a point far along it right next to her)
            let along = ahead.min(track.length - self.s);
            if dist(pos, q) < 0.6 * along {
                continue;
            }
            if dist(pos, q) < 1.0 || self.chord_clear(pos, q) {
                return q;
            }
        }
        track.carrot(self.k, self.s, TRACK_AHEAD)
    }

    /// Can the box move straight from p to q without touching a wall, solid thing or
    /// (uncrossed) ledge? Edges she's close to already only need not get closer.
    fn chord_clear(&self, p: Point, q: Point) -> bool {
        let nav = &self.view.nav;
        for (e1, e2) in nav.walls_near(p, q, 2.0 * PLAYER_RADIUS, Some(&self.crossing_lines), Some(&self.live)) {
            let length = dist(e1, e2).max(1e-6);
            let (c, s) = ((e2.0 - e1.0).abs() / length, (e2.1 - e1.1).abs() / length);
            let reach = PLAYER_RADIUS * (c + s) + 2.0; // (the box's extent across this edge)
            let d = segment_distance(p, q, e1, e2);
            if d < reach && d < segment_distance(p, p, e1, e2) - 0.5 {
                return false;
            }
        }
        // nor across a teleport line from its front (cutting a corner over one teleports her)
        let m = &nav.map;
        for cell in cells(p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)) {
            for &li in nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !TELEPORTERS_FOR_PLAYERS.contains(&m.linedefs[li].special) || self.crossing_lines.contains(&li) {
                    continue;
                }
                let (v1, v2) = m.line_seg(li);
                let in_front = (v2.0 - v1.0) * (p.1 - v1.1) - (v2.1 - v1.1) * (p.0 - v1.0) < 0.0;
                if in_front && segment_distance(p, q, v1, v2) < 1.0 {
                    return false;
                }
            }
        }
        true
    }

    /// The view angle: towards a point well ahead on the path, holding the last one when that
    /// point is too near to give a steady direction.
    fn look_angle(&mut self, obs: &Observation, seg: &Segment) -> f64 {
        let look = &seg.look;
        let s = if look.length > seg.track.length { look.project(obs.pos(), self.s, 64.0).1 } else { self.s };
        let point = look.point_at(s + LOOK_AHEAD);
        if dist(obs.pos(), point) >= 40.0 || self.look.is_none() {
            if dist(obs.pos(), point) > 1.0 {
                self.look = Some(heading(obs.pos(), point));
            } else if self.look.is_none() {
                self.look = Some(obs.angle);
            }
        }
        self.look.unwrap_or(obs.angle)
    }

    // --- recovering ---------------------------------------------------------------------------

    /// Under a ceiling too low for her over some of the box (a crusher coming down, a door
    /// shutting on her): Doom won't move her till it's risen, so that's waiting, not being stuck
    /// (E3M4's crusher room, the bonuses in it, a dozen lives given up there); up to PINNED_WAIT.
    fn pinned_under_ceiling(&mut self, obs: &Observation) -> Option<Controls> {
        let mesh = &self.view.nav.mesh;
        let r = PLAYER_RADIUS - 1.0;
        let secs: BTreeSet<usize> = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)].iter().filter_map(|(dx, dy)| mesh.poly_at((obs.x + dx, obs.y + dy))).map(|p| mesh.poly(p).sector).collect();
        let floor = secs.iter().filter_map(|&s| obs.height(s)).map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
        let ceiling = secs.iter().filter_map(|&s| obs.height(s)).map(|h| h.ceiling).fold(f64::INFINITY, f64::min);
        if ceiling - floor >= PLAYER_HEIGHT {
            self.pinned_since = None;
            return None;
        }
        let since = *self.pinned_since.get_or_insert_with(|| {
            self.log.add(obs.tic, format!("pinned under a ceiling at ({:.0}, {:.0}): waiting for it to rise", obs.x, obs.y));
            obs.tic
        });
        if obs.tic - since > PINNED_WAIT {
            return None;
        }
        self.progress_tic = obs.tic;
        Some(Controls::default())
    }

    /// Which way to back off: away from the walls and ledges nearest (MAP29: backing off a stair
    /// wall walked it off the other side into the pit), else backwards.
    fn away_from_walls(&self, obs: &Observation) -> f64 {
        let pos = obs.pos();
        let (mut ax, mut ay) = (0.0, 0.0);
        for (a, b) in self.view.nav.walls_near(pos, pos, 40.0, Some(&self.crossing_lines), Some(&obs.heights)) {
            let q = closest_point(pos, a, b);
            let d = dist(pos, q);
            if 1e-6 < d && d < 40.0 {
                ax += (pos.0 - q.0) / d * (40.0 - d);
                ay += (pos.1 - q.1) / d * (40.0 - d);
            }
        }
        let track = &self.segments[self.seg_i].track;
        let back = heading(track.point_at(self.s + TRACK_AHEAD), pos);
        if ax.hypot(ay) < 1e-6 {
            return back;
        }
        let away = ay.atan2(ax);
        let back_r = back.to_radians();
        // mostly sideways off what she's snagged on, a bit backwards
        heading((0.0, 0.0), (away.cos() * 2.0 + back_r.cos(), away.sin() * 2.0 + back_r.sin()))
    }

    fn unstick(&mut self, obs: &Observation) -> Controls {
        let seg = self.segments[self.seg_i].clone();
        let look = self.look_angle(obs, &seg);
        self.motion.go(obs, self.unstick_dir, 5.0, Some(look))
    }

    /// (poly, point in it) of the floor she's really standing on. Doom holds the box up on the
    /// highest floor under any of it, so her centre can be out over a pit beside a walkway
    /// narrower than the box (MAP24's) with nothing wrong.
    fn standing(&self, obs: &Observation) -> (Option<usize>, Point) {
        let mesh = &self.view.nav.mesh;
        let here = mesh.poly_at(obs.pos());
        let (Some(z), Some(h)) = (obs.z, here) else { return (here, obs.pos()) };
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
                    if (obs.heights[poly.sector].floor - z).abs() < 0.5 && inside(&poly.vertices, q.0, q.1) {
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
        let mut off = self.cross > OFF_COURSE;
        if !off && here.is_none_or(|h| !self.path_polys.contains(&h)) {
            if self.cross > STRAY {
                off = true;
            } else {
                // beside the path, but somewhere she can't step back up from
                let q = mesh.poly_at(self.segments[self.seg_i].track.point_at(self.s));
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
    /// from a landing was smoothed across the jump and can wander: MAP22's swung back off the
    /// ledge it lands on). Not a replan: the plan holds.
    fn path_on(&mut self, obs: &Observation) {
        let Some(goal) = self.step().path.as_ref().and_then(last_point) else { return };
        let doors = self.view.local_doors(self.keys);
        if let Some(path) = self.view.find_path(self.standing(obs).1, goal, &self.world_states(obs), Some(&doors)) {
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
        let Some(goal) = step.path.as_ref().and_then(last_point) else { return false };
        let states = if live_only { Self::live_states(obs) } else { self.world_states(obs) };
        let doors = view.local_doors(self.keys);
        if let Some(path) = view.find_path(pos, goal, &states, Some(&doors)) {
            self.steps[self.step_i as usize].path = Some(path);
            self.log.add(obs.tic, format!("{why} at ({:.0}, {:.0}): new path to ({:.0}, {:.0})", obs.x, obs.y, goal.0, goal.1));
        } else {
            let route = view.plan(pos, &[goal], &crate::navigator::Plan { keys: self.keys, states: Some(&states), max_expansions: 200, max_seconds: 600.0, fired: &self.fired, ..Default::default() });
            if let Some(route) = route {
                let new = steps_from_legs(&view, &route.legs, Some(step), Some(&states));
                let n = new.len();
                let i = self.step_i as usize;
                self.steps.splice(i..=i, new);
                self.log.add(obs.tic, format!("{why} at ({:.0}, {:.0}): back on track via {} extra step(s)", obs.x, obs.y, n as i64 - 1));
            } else if self.replan_level {
                let plan = plan_from(&view, pos, &Rest { keys: self.keys, states: Some(&states), reach: None, max_seconds: 600.0, max_expansions: 1000, fired: &self.fired });
                if !plan.complete {
                    self.log.add(obs.tic, format!("{why}: no way to the step goal or the exit"));
                    return false;
                }
                let n = plan.steps.len();
                self.steps.truncate(self.step_i as usize);
                self.steps.extend(plan.steps);
                self.log.add(obs.tic, format!("{why} at ({:.0}, {:.0}): replanned the rest of the level ({n} steps)", obs.x, obs.y));
            } else {
                self.log.add(obs.tic, format!("{why}: no way to the step goal"));
                return false;
            }
        }
        self.sprint = None;
        self.start_step(obs.tic);
        true
    }

    // --- run-off jumps ------------------------------------------------------------------------

    /// At the run-up: settle on the exact spot facing the takeoff, then sprint.
    fn line_up(&mut self, obs: &Observation, jump: &Jump) -> Controls {
        let hd = heading(jump.runup, jump.takeoff);
        let diff = norm(hd - obs.angle);
        // the heights the jump was found with must be there, and not still moving (if one has
        // stopped somewhere else it's not coming: jump from there anyway)
        for &(sector, h) in &jump.requires {
            let live = obs.height(sector);
            // (a ceiling a little lower is fine: MAP09's door just starting to close as the
            // lift brings her up for the jump through it)
            let off = live.is_none_or(|l| (l.floor - h.floor).abs() > 0.5 || l.ceiling > h.ceiling + 0.5 || l.ceiling < h.ceiling - JUMP_CEILING_SLACK);
            if !off {
                continue;
            }
            if self.jump_wait.is_none_or(|(s, l, _)| (s, l) != (sector, live)) {
                if self.jump_wait.is_none_or(|(s, _, _)| s != sector) {
                    self.log.add(obs.tic, format!("waiting for sector {sector} to reach {} before jumping (now {})", fmt_live(Some(h)), fmt_live(live)));
                }
                self.jump_wait = Some((sector, live, obs.tic));
            }
            // still moving lately (a lift waits ~3s at the bottom before coming back)
            let still_for = if self.lift_sectors.contains(&sector) { 5 * 35 } else { 35 };
            let since = self.jump_wait.map_or(obs.tic, |w| w.2);
            if obs.tic - since < still_for {
                self.lineup_tic = None;
                self.progress_tic = obs.tic;
                let mut spot = jump.runup;
                if let Some(here) = self.standing(obs).0 {
                    let poly = self.view.nav.mesh.poly(here);
                    if poly.sector == sector {
                        // riding it (set off on the way to the run-up: MAP06): the middle, clear
                        // of the edges, or it can't come back up with her
                        spot = poly.center;
                    }
                }
                return self.motion.hold(obs, ((spot.0 - obs.x) * 0.3, (spot.1 - obs.y) * 0.3), Some(hd));
            }
            if live.is_none_or(|l| l.floor < h.floor - 0.5) {
                // stopped lower than the jump needs: find another way with things as they are
                self.jump_wait = None;
                if self.replan(obs, &format!("sector {sector} at {}, too low for the jump", fmt_live(live)), true) {
                    return self.motion.hold(obs, (0.0, 0.0), Some(hd));
                }
                return self.fail(obs, "jump takeoff too low and no other way");
            }
        }
        // as exactly as the check did it: on the spot, square on, standing still
        let mut settled = dist(obs.pos(), jump.runup) < 1.5 && diff.abs() <= 0.75 && self.motion.speed() < 0.3 && self.motion.rate.abs() < 0.5;
        // something the jump needs is about to go (MAP09: the door it goes through starts
        // shutting a moment after the lift brings her up): no settling, go once facing
        let needs: BTreeSet<usize> = jump.requires.iter().map(|(s, _)| *s).collect();
        if self.watches.iter().any(|w| needs.contains(&w.expect.sector) && !w.over && w.expect.hold.is_some_and(|hold| w.reached.is_some_and(|r| r as f64 + hold - (obs.tic as f64) < JUMP_HURRY)))
            && dist(obs.pos(), jump.runup) < 6.0
            && diff.abs() <= 3.0
        {
            settled = true;
        }
        let lineup = *self.lineup_tic.get_or_insert(obs.tic);
        // (settling longer than a second: go anyway, if near enough on the spot; much longer:
        // from wherever)
        let near = dist(obs.pos(), jump.runup) < 6.0;
        if settled || (obs.tic - lineup > 35 && diff.abs() <= 1.5 && self.motion.speed() < 0.5 && (near || obs.tic - lineup > 3 * 35)) {
            self.lineup_tic = None;
            self.sprint = Some((jump.clone(), hd, obs.tic));
            self.log.add(obs.tic, format!("run-jump from ({:.0}, {:.0})", jump.takeoff.0, jump.takeoff.1));
            return self.sprint_jump(obs);
        }
        self.progress_tic = obs.tic;
        self.motion.hold(obs, ((jump.runup.0 - obs.x) * 0.3, (jump.runup.1 - obs.y) * 0.3), Some(hd))
    }

    /// Back out of its blast (128 units) and shoot the barrel in the way, then carry on.
    fn shoot_barrel(&mut self, obs: &Observation) -> Controls {
        let Some((at, since)) = self.barrel else { return self.walk(obs) };
        let pos = obs.pos();
        if !obs.barrels.iter().flatten().any(|&b| dist(b, at) < 8.0) || obs.tic - since > BARREL_GIVE_UP {
            self.barrel = None;
            self.reset_progress(obs.tic);
            self.log.add(obs.tic, if obs.tic - since <= BARREL_GIVE_UP { "barrel gone" } else { "couldn't shoot the barrel" });
            return self.walk(obs);
        }
        self.progress_tic = obs.tic;
        let look = heading(pos, at);
        if dist(pos, at) < BARREL_SAFE && obs.tic - since < 3 * 35 {
            let away = (pos.1 - at.1).atan2(pos.0 - at.0);
            return self.motion.hold(obs, (away.cos() * 8.0, away.sin() * 8.0), Some(look));
        }
        let mut c = self.motion.hold(obs, (0.0, 0.0), Some(look));
        // (single taps, the trigger let go between: the first shot of a burst goes straight,
        // only held ones spread)
        let gun = tap_gun(obs);
        if obs.weapon != Some(gun) {
            c.weapon = gun;
        } else if norm(look - obs.angle).abs() < 2.0 && obs.tic - self.barrel_shot >= BARREL_TAP {
            c.attack = true;
            self.barrel_shot = obs.tic;
        }
        c
    }

    /// Running over floors that sink once stepped on (MAP08's exit: platforms in nukage that go
    /// down as she lands). No stopping to line up a jump: keep running, and every few tics pick
    /// (with the movement simulator, from where she is and how fast she's going) the heading that
    /// comes down on the floor nearest the goal. Over once on a floor that stays put.
    fn run_step(&mut self, obs: &Observation) -> Controls {
        let mesh = Rc::clone(&self.view);
        let here = mesh.nav.mesh.poly_at(obs.pos());
        let here_sector = here.map(|h| mesh.nav.sector_of_poly(h));
        let on_ground = match (obs.z, here_sector) {
            (Some(z), Some(s)) => z <= obs.heights[s].floor + 0.5,
            _ => true,
        };
        if on_ground && here_sector.is_some_and(|s| !self.sinking.contains(&s)) {
            self.run = None;
            self.log.add(obs.tic, format!("ran across to ({:.0}, {:.0}) sector {}", obs.x, obs.y, here_sector.unwrap_or(0)));
            if !self.replan(obs, "after the run", false) {
                return self.fail(obs, "nowhere to go after the run");
            }
            return self.walk(obs);
        }
        let Some(run) = self.run else { return self.walk(obs) };
        if on_ground && (obs.tic >= run.next || here_sector != run.sector) {
            let Some(sector) = here_sector else { return self.walk(obs) };
            match self.run_heading(obs, sector) {
                None => {
                    self.run = None;
                    self.log.add(obs.tic, "no heading off the sinking floor lands anywhere");
                    if !self.replan(obs, "sinking floor", true) {
                        return self.fail(obs, "stranded on a sinking floor");
                    }
                    return self.walk(obs);
                }
                Some(h) => self.run = Some(Run { heading: h, sector: Some(sector), next: obs.tic + RUN_REAIM }),
            }
        }
        self.motion.rate = 0.0;
        run_controls(obs.angle, self.run.map_or(obs.angle, |r| r.heading))
    }

    /// The direction to push (from a fan all round: pushing back against her momentum may be
    /// what keeps her on) whose simulated run lands, off this floor, on the floor whose path to
    /// the step's goal is shortest.
    fn run_heading(&self, obs: &Observation, sector: usize) -> Option<f64> {
        let nav = &self.view.nav;
        let goal = self.step().path.as_ref().and_then(last_point)?;
        let mut heights: Vec<(f64, f64)> = nav.movers.initial.iter().enumerate().map(|(s, h)| obs.height(s).map_or((h.floor, h.ceiling), |l| (l.floor, l.ceiling))).collect();
        let (f, c) = heights[sector];
        heights[sector] = (f - RUN_AHEAD_SINK, c); // (it keeps going down meanwhile)
        let live = Self::live_states(obs);
        let doors = self.view.local_doors(self.keys);
        let d = self.motion.disp;
        let going = if d.0.hypot(d.1) > 2.0 { d.1.atan2(d.0).to_degrees() } else { obs.angle };
        let offs: Vec<f64> = (-12..12).map(|i| f64::from(i) * 15.0).collect();
        let start = RunStart { pos: obs.pos(), vel: d, z: obs.z.unwrap_or(f), angle: obs.angle };
        let headings: Vec<f64> = offs.iter().map(|o| going + o).collect();
        let landings = nav.jump_lab.run_landings(&start, &heights, sector, &headings, 70);
        let mut cost_from: BTreeMap<usize, Option<f64>> = BTreeMap::new();
        let mut best: Option<(f64, f64, f64)> = None;
        for (off, landing) in offs.iter().zip(landings) {
            let Some((at, land)) = landing else { continue };
            if nav.damage[land] != 0 || heights[land].0 < f - MAX_STEP {
                continue; // (into the nukage)
            }
            let cost = *cost_from.entry(land).or_insert_with(|| self.view.find_path(at, goal, &live, Some(&doors)).map(|p| p.cost));
            if let Some(cost) = cost {
                if best.is_none_or(|(bc, bo, _)| (cost, off.abs()) < (bc, bo)) {
                    best = Some((cost, off.abs(), going + off));
                }
            }
        }
        best.map(|b| b.2)
    }

    /// The sectors within BUSY_NEAR of a line (cached).
    fn sectors_near(&mut self, li: usize) -> BTreeSet<usize> {
        if let Some(s) = self.near_cache.get(&li) {
            return s.clone();
        }
        let nav = &self.view.nav;
        let (a, b) = nav.map.line_seg(li);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let out: BTreeSet<usize> = nav.mesh.live_polys().filter(|(_, p)| dist(p.center, mid) < BUSY_NEAR).map(|(_, p)| p.sector).collect();
        self.near_cache.insert(li, out.clone());
        out
    }

    /// A run-jump done: once she's on a floor again, did it come down on its landing?
    fn judge_landing(&mut self, obs: &Observation) {
        let Some((jump, tic)) = self.touchdown.clone() else { return };
        let nav = &self.view.nav;
        let here = nav.mesh.poly_at(obs.pos());
        let on_floor = match (here, obs.z) {
            (Some(h), Some(z)) => (z - obs.heights[nav.sector_of_poly(h)].floor).abs() < 1.0,
            _ => false,
        };
        if !on_floor {
            if obs.tic - tic > 2 * 35 {
                self.touchdown = None; // (still falling, or off the map: no telling)
            }
            return;
        }
        self.touchdown = None;
        let want = nav.sector_of_poly(jump.dest_poly);
        let got = here.map_or(0, |h| nav.sector_of_poly(h));
        if got != want {
            self.log.add(obs.tic, format!("missed the jump off ({:.0}, {:.0}): came down in sector {got}, not {want}", jump.takeoff.0, jump.takeoff.1));
            self.missed.push(jump);
        }
    }

    /// Full speed in a straight line, as the check did it: only to the edge for a soft jump
    /// (then coast: it lands slower, for narrow strips), else through the flight. Touching down
    /// on the landing's sector, stop sprinting so the landing is braked at once.
    fn sprint_jump(&mut self, obs: &Observation) -> Controls {
        let Some((jump, hd, since)) = self.sprint.clone() else { return self.walk(obs) };
        let nav = Rc::clone(&self.view);
        let (ux, uy) = (hd.to_radians().cos(), hd.to_radians().sin());
        let flown = (obs.x - jump.takeoff.0) * ux + (obs.y - jump.takeoff.1) * uy;
        let here = nav.nav.mesh.poly_at(obs.pos());
        // landed only on the sector the jump was checked to land on (a jump can skim another
        // floor on the way: E4M2's)
        let landed = flown > 0.0
            && here.is_some_and(|h| {
                let s = nav.nav.sector_of_poly(h);
                s == nav.nav.sector_of_poly(jump.dest_poly) && obs.z.is_some_and(|z| (z - obs.heights[s].floor).abs() < 1.0)
            });
        if landed || flown >= dist(jump.takeoff, jump.dest) - 8.0 || obs.tic - since > 4 * 35 {
            self.sprint = None;
            self.touchdown = Some((jump.clone(), obs.tic));
            let land = here.map(|h| nav.nav.sector_of_poly(h));
            self.log.add(obs.tic, format!("landed at ({:.0}, {:.0}) sector {}", obs.x, obs.y, land.map_or("None".to_string(), |s| s.to_string())));
            self.seg_i = (self.seg_i + 1).min(self.segments.len() - 1);
            self.enter_segment(obs.tic);
            self.look = Some(hd);
            if let Some(s) = land.filter(|s| self.sinking.contains(s)) {
                // going down under her: no settling, run on across
                self.log.add(obs.tic, format!("sector {s} sinking: running on"));
                self.run = Some(Run { heading: hd, sector: None, next: obs.tic });
                return self.run_step(obs);
            }
            self.path_on(obs);
            // (landing spots can be small: shed the sprint's speed before going on)
            self.brake_until = obs.tic + 2 * 35;
            return self.walk(obs);
        }
        let diff = norm(hd - obs.angle);
        self.motion.rate = 0.0;
        let push = flown < if jump.soft { jump.release } else { dist(jump.takeoff, jump.dest) - 8.0 };
        // keep on the jump's line till takeoff (MAP27's clears a ledge corner by nothing)
        let mut strafe = 0.0;
        if flown < 0.0 {
            let side = (obs.x - jump.takeoff.0) * uy - (obs.y - jump.takeoff.1) * ux;
            strafe = (-4.0 * side).clamp(-15.0, 15.0); // (side: how far right of the line)
        }
        Controls { forward: if push { 50.0 } else { 0.0 }, strafe, turn: (-diff).clamp(-3.0, 3.0), ..Controls::default() }
    }

    // --- doors and lifts in the way -----------------------------------------------------------

    /// A crossing just ahead that's become a one-way drop the plan didn't mean to take (MAP08:
    /// walking to the red key goes over a line that sinks the bridge back into the nukage).
    /// Stop and replan with the heights as they are: on if there's a way without such a drop;
    /// else the objective goes back to be judged again, the ground as it is now (E2M4: a floor
    /// raised on the way left the goal down in the blood pit, a drop into a trap).
    /// A drop just ahead onto a landing that's narrow the way she's going (a ledge over a pit):
    /// step off it slowly, as a falling player keeps her speed and slides on landing (MAP11's
    /// ledge with the blur sphere, 136 down and 48 across: she'd gone straight over it).
    fn creep_to_drop(&mut self, obs: &Observation) {
        let view = Rc::clone(&self.view);
        let mesh = &view.nav.mesh;
        let Some(path) = self.step().path.clone() else { return };
        let pos = obs.pos();
        let upto = (self.poly_i + 4).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            let (a, b) = (obs.heights[cs], obs.heights[ns]);
            if cs == ns || a.floor - b.floor <= MAX_STEP {
                continue;
            }
            let Some(portal) = mesh.poly(cur).portals.iter().map(|&pi| &mesh.portals[pi]).find(|p| p.other(cur) == nxt) else { return };
            let edge = closest_on_portal(pos, portal.p1, portal.p2);
            if dist(edge, pos) > CREEP_RANGE {
                return;
            }
            // (the way she'll go over: on from her to the edge, or, at it, the way the landing is)
            let to = if dist(edge, pos) > 4.0 { pos } else { mesh.poly(cur).center };
            let len = dist(edge, to).max(1e-6);
            let dir = ((edge.0 - to.0) / len, (edge.1 - to.1) / len);
            // (the floor she'd land on, how far on before it drops away again)
            let room = (1..)
                .map(|k| f64::from(k) * 8.0)
                .take_while(|&r| r < LANDING_ROOM)
                .find(|&r| mesh.poly_at((edge.0 + dir.0 * r, edge.1 + dir.1 * r)).is_none_or(|q| obs.heights[mesh.poly(q).sector].floor < b.floor - MAX_STEP));
            // (as fast as goes no further than that: the fall, about sqrt(2h) tics, and the
            // slide on landing, 1 / (1 - friction) tics' worth, at the speed she went over)
            if let Some(room) = room {
                let tics = (2.0 * (a.floor - b.floor)).sqrt() + 1.0 / (1.0 - crate::motor::FRICTION);
                self.speed_cap = self.speed_cap.min((room / tics).max(DROP_CREEP));
            }
            return;
        }
    }

    fn unexpected_drop(&mut self, obs: &Observation) -> Drop {
        let view = Rc::clone(&self.view);
        let mesh = &view.nav.mesh;
        let Some(path) = self.step().path.clone() else { return Drop::None };
        // (the drops the plan means to take, this step's and the rest's)
        let planned: BTreeSet<String> = self.steps[self.step_i as usize..].iter().filter_map(|st| st.path.as_ref()).flat_map(|p| p.steps.iter()).filter(|n| n.contains("one-way drop")).filter_map(|n| n.split("into sector ").nth(1).map(str::to_string)).collect();
        let upto = (self.poly_i + 4).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            if cs == ns {
                continue;
            }
            let Some(portal) = mesh.poly(cur).portals.iter().map(|&pi| &mesh.portals[pi]).find(|p| p.other(cur) == nxt) else { return Drop::None };
            if dist(closest_on_portal(obs.pos(), portal.p1, portal.p2), obs.pos()) > DROP_CHECK_DIST {
                return Drop::None;
            }
            let (a, b) = (obs.heights[cs], obs.heights[ns]);
            let key = (cs, ns, [a.floor.to_bits(), a.ceiling.to_bits(), b.floor.to_bits(), b.ceiling.to_bits()]);
            if planned.contains(&ns.to_string()) || self.drops_ok.contains(&key) || !can_cross(a, b) || can_cross(b, a) {
                continue;
            }
            if !self.states[cs].iter().any(|&ha| self.states[ns].iter().any(|&hb| can_cross(hb, ha))) {
                continue; // (one-way in the plan as well)
            }
            let coming = self.watches.iter().find(|w| w.expect.sector == ns && w.reached.is_none() && can_cross(a, w.expect.heights) && can_cross(w.expect.heights, a) && ((obs.tic - w.since) as f64) < 1.5 * w.expect.travel + 10.0 * TICRATE).map(|w| w.expect.heights);
            if let Some(h) = coming {
                // still on its way to where the plan has it (MAP29: a floor that takes half a
                // minute to rise to the walkway): wait at the edge
                if !self.edge_wait {
                    self.log.add(obs.tic, format!("waiting for sector {ns} to reach {}", fmt_heights(h)));
                    self.edge_wait = true;
                }
                self.speed_cap = 0.0;
                self.progress_tic = obs.tic;
                self.step_tic = obs.tic;
                return Drop::None;
            }
            self.drops_ok.insert(key);
            let why = format!("unexpected drop into sector {ns}");
            // (heights she allowed for, a lift down that she counted on coming up: only the timing
            // is off, on by the drop as before. Heights she didn't: what's beyond is for the
            // explorer to judge again)
            let allowed = |s: usize, h: Heights| self.states[s].iter().any(|x| (x.floor - h.floor).abs() < 1.0 && (x.ceiling - h.ceiling).abs() < 1.0);
            // (or one the plan is waiting to see move, caught on its way: MAP06's lift, pressed to
            // be got onto, sinking under her as she stood by its switch)
            let on_its_way = |s: usize| self.watches.iter().any(|w| w.expect.sector == s && w.reached.is_none());
            let foreseen = (allowed(cs, a) || on_its_way(cs)) && (allowed(ns, b) || on_its_way(ns));
            if !self.replan(obs, &why, true) {
                return if foreseen { Drop::None } else { Drop::GiveBack(format!("{why}, and no other way on")) };
            }
            if foreseen {
                return Drop::Replanned;
            }
            // (the new way: none of the drops it takes that the plan didn't mean to)
            let dropped: BTreeSet<String> = self.steps[self.step_i as usize..].iter().filter_map(|st| st.path.as_ref()).flat_map(|p| p.steps.iter()).filter(|n| n.contains("one-way drop")).filter_map(|n| n.split("into sector ").nth(1).map(str::to_string)).collect();
            if !dropped.is_subset(&planned) {
                return Drop::GiveBack(format!("{why}: the only way on drops where the plan didn't"));
            }
            return Drop::Replanned;
        }
        Drop::None
    }

    /// The first crossing ahead on the path, within reach, that the live heights don't allow.
    /// Looks past the next poly: doors often sit behind thin door-track sectors.
    fn next_blocked(&mut self, obs: &Observation) -> Option<Blocked> {
        let view = Rc::clone(&self.view);
        let nav = &view.nav;
        let mesh = &nav.mesh;
        let path = self.step().path.clone()?;
        let here = mesh.poly_at(obs.pos())?;
        let window: Vec<usize> = path.polys.iter().skip(self.poly_i).take(12).copied().collect();
        if let Some(i) = window.iter().position(|&p| p == here) {
            self.poly_i += i;
        }
        let upto = (self.poly_i + 8).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            let portal_i = mesh.poly(cur).portals.iter().copied().find(|&pi| mesh.portals[pi].other(cur) == nxt)?; // (none: a teleport)
            let portal = &mesh.portals[portal_i];
            let near = closest_on_portal(obs.pos(), portal.p1, portal.p2);
            let d = dist(near, obs.pos());
            if d > DOOR_CHECK_DIST {
                return None;
            }
            if ns != cs && !can_cross(obs.heights[cs], obs.heights[ns]) {
                // past the next crossing only for doors (they hide behind thin door tracks);
                // a lift: step on and ride, the crossing off it opens up on the way
                let door = self.doors.contains_key(&ns);
                if (j > self.poly_i && !door && d >= 2.0 * PLAYER_RADIUS) || (!door && d > BLOCK_CHECK_DIST) {
                    return None;
                }
                return Some((cur, ns, portal_i, None));
            }
            // the box reaches into sectors past a narrow strip before it can drop into the strip:
            // a low ceiling there must clear the floor she's on now
            let here_sector = mesh.poly(here).sector;
            let floor_now = obs.heights[here_sector].floor;
            let Heights { floor: f, ceiling: c } = obs.heights[ns];
            if j > self.poly_i && d < 2.0 * PLAYER_RADIUS && ns != here_sector && c - f.max(floor_now) < PLAYER_HEIGHT {
                return Some((cur, ns, portal_i, None));
            }
            // something beside the way that makes it too narrow now (key bars, a raised block):
            // open it if it's a door she can use, else wait as for a mover
            if nav.has_portal_gates(portal_i) {
                let live = Self::live_states(obs);
                let mut blockers = nav.gate_blockers(portal_i, cur, &live, None);
                blockers.sort_unstable();
                if !blockers.is_empty() && !nav.portal_fits_without(portal_i, &blockers) {
                    let near_polys: BTreeSet<usize> = [cur, here].into_iter().collect();
                    if let Some((door, point)) = self.gate_door(&blockers, &near_polys, obs.pos()) {
                        return Some((cur, door, portal_i, Some(point)));
                    }
                    if j == self.poly_i && d < BLOCK_CHECK_DIST {
                        let (f, b) = nav.map.sides(blockers[0]);
                        let other = [f, b].into_iter().flatten().find(|&s| s != cs).unwrap_or(cs);
                        return Some((cur, other, portal_i, None));
                    }
                }
            }
        }
        None
    }

    fn line_distance(&self, pos: Point, li: usize) -> f64 {
        let (a, b) = self.view.nav.map.line_seg(li);
        dist(pos, closest_point(pos, a, b))
    }

    /// Does the use trace from pos to target cross a line with a special on the way (a press
    /// would set that off instead)? Lines ending at target don't count.
    fn use_intercepted(&self, pos: Point, target: Point) -> bool {
        let nav = &self.view.nav;
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for cell in cells(pos.0, pos.1, target.0, target.1) {
            for &li in nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !seen.insert(li) || nav.map.linedefs[li].special == 0 {
                    continue;
                }
                let (a, b) = nav.map.line_seg(li);
                if dist(closest_point(target, a, b), target) < 1.0 {
                    continue; // (the line being pressed)
                }
                if segments_cross(pos, target, a, b) {
                    return true;
                }
            }
        }
        false
    }

    /// The nearest point (a little in from its ends) on a use line that opens this door.
    fn door_line_point(&self, sector: usize, pos: Point) -> Option<Point> {
        let nav = &self.view.nav;
        let mut best: Option<Point> = None;
        for t in &self.view.triggers {
            if !matches!(t.info.trigger, Trigger::DR | Trigger::D1) || t.effect(sector).is_none() {
                continue;
            }
            let Some(li) = t.linedef() else { continue };
            let (a, b) = nav.map.line_seg(li);
            let length = dist(a, b).max(1e-6);
            let k = 0.5f64.min(8.0 / length);
            let q = closest_point(pos, (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k), (b.0 + (a.0 - b.0) * k, b.1 + (a.1 - b.1) * k));
            if best.is_none_or(|bp| dist(pos, q) < dist(pos, bp)) {
                best = Some(q);
            }
        }
        best
    }

    /// Does the last 128 units of the track go down any steps (live floors)?
    fn steps_down_to_end(&self, obs: &Observation, track: &Track) -> bool {
        let mesh = &self.view.nav.mesh;
        let start = (track.length - 128.0).max(0.0);
        let floors: Vec<f64> = (0..9).filter_map(|k| mesh.poly_at(track.point_at(start + (track.length - start) * f64::from(k) / 8.0)).map(|q| obs.heights[mesh.poly(q).sector].floor)).collect();
        floors.windows(2).any(|w| w[1] < w[0])
    }

    /// Is `sector` shut by a "close, wait 30s, open" door (it opens again by itself)?
    fn reopens(&self, sector: usize, live: Heights) -> bool {
        self.view.triggers.iter().any(|t| [16, 76].contains(&t.info.special) && t.effect(sector).is_some_and(|hs| hs.iter().any(|h| (live.ceiling - h.ceiling).abs() < 1.0)))
    }

    /// (door sector, point to press) for a usable door among the sectors of these lines.
    fn gate_door(&self, lines: &[usize], near: &BTreeSet<usize>, pos: Point) -> Option<(usize, Point)> {
        let m = &self.view.nav.map;
        let sectors: BTreeSet<usize> = lines.iter().flat_map(|&li| { let (f, b) = m.sides(li); [f, b] }).flatten().collect();
        for t in &self.view.triggers {
            let effects: BTreeSet<usize> = t.effects.iter().map(|(s, _)| *s).collect();
            if matches!(t.info.trigger, Trigger::DR | Trigger::D1) && !sectors.is_disjoint(&effects) && !near.is_disjoint(&t.activators) && t.info.lock.satisfied(Key::ALL.into_iter().collect()) {
                let li = t.linedef()?;
                let (a, b) = m.line_seg(li);
                let s = *sectors.intersection(&effects).next()?;
                return Some((s, closest_point(pos, a, b)));
            }
        }
        None
    }

    /// A crossing just ahead the live heights don't allow: deal with it.
    fn check_blocked(&mut self, obs: &Observation) -> Option<Controls> {
        let Some((cur, sector, portal_i, use_point)) = self.next_blocked(obs) else {
            self.wait_tic = None;
            return None;
        };
        let view = Rc::clone(&self.view);
        let nav = &view.nav;
        let portal: Portal = nav.mesh.portals[portal_i].clone();
        let live = &obs.heights;
        let pos = obs.pos();
        let opener = self.doors.get(&sector).is_some_and(|d| use_point.is_some() || d.users.contains(&cur));
        // the part of the doorway in front of her, or the gate door's line; for a door, its use
        // line (the face with the special: MAP12's diagonal blue door isn't the side the path
        // goes in by)
        let door_point = use_point.or_else(|| if opener { self.door_line_point(sector, pos) } else { None }).unwrap_or_else(|| closest_on_portal(pos, portal.p1, portal.p2));
        if opener {
            // how far to the door, along the path where it goes round corners
            let along = self.segments[self.seg_i].track.project(door_point, self.s, DOOR_CHECK_DIST + 64.0).1 - self.s;
            let d = dist(pos, door_point).max(along);
            let here = nav.mesh.poly_at(pos);
            // (use reaches 64: close enough to press, whatever the door's user polys say)
            if use_point.is_none() && here.is_none_or(|h| !self.doors[&sector].users.contains(&h)) && dist(pos, door_point) > PRESS_RANGE {
                // not somewhere the door can be pressed from: keep walking up to it
                self.speed_cap = (0.3 * d).max(1.0);
                return None;
            }
            let mut standoff = DOOR_STANDOFF;
            if self.use_intercepted(pos, door_point) {
                // another switch line between her and the door would take the press (E3M9): go
                // right up to the door
                standoff = PLAYER_RADIUS + 2.0;
            }
            if d > standoff + 4.0 {
                // a shut door ahead: follow the path, slowing to stop in front of it
                self.speed_cap = (2.0 * BRAKE * (d - standoff)).sqrt().min(0.3 * (d - standoff) + 1.0).max(1.0);
                return None;
            }
        }
        let wait_tic = *self.wait_tic.get_or_insert_with(|| {
            let kind = if self.doors.contains_key(&sector) { "door" } else { "mover" };
            self.log.add(obs.tic, format!("waiting at {kind} (sector {sector})"));
            obs.tic
        });
        let waited = obs.tic - wait_tic;
        self.progress_tic = obs.tic; // (waiting isn't being stuck)
        // pressing use on a moving door reverses it: only press once things are still
        if Some(live[sector]) != self.mover_heights {
            self.mover_heights = Some(live[sector]);
            self.mover_still_since = obs.tic;
        }
        let still = obs.tic - self.mover_still_since >= USE_EVERY;
        let mut look = heading(pos, door_point);
        let mut want = (0.0, 0.0);
        let mut use_ = false;
        if !opener {
            // wait squarely at the edge she's crossing (half on something else, a rising lift
            // can't carry her up and goes back down)
            let length = dist(portal.p1, portal.p2).max(1e-6);
            let (mut nx, mut ny) = ((portal.p2.1 - portal.p1.1) / length, -(portal.p2.0 - portal.p1.0) / length);
            if (pos.0 - door_point.0) * nx + (pos.1 - door_point.1) * ny < 0.0 {
                (nx, ny) = (-nx, -ny); // (the normal back to her side)
            }
            let back = PLAYER_RADIUS + 2.0;
            let spot = (door_point.0 + nx * back, door_point.1 + ny * back);
            let gap = dist(pos, spot);
            if gap > 1.0 {
                let speed = 4.0f64.min(0.3 * gap);
                let h = heading(pos, spot).to_radians();
                want = (h.cos() * speed, h.sin() * speed);
            }
        }
        if opener {
            // use traces 64 straight ahead and stops at the first wall: face the door's middle,
            // then press
            let facing = norm(look - obs.angle).abs() < 10.0;
            use_ = facing && still && obs.tic - self.last_use >= USE_EVERY;
        }
        let mut switch_nearby = false;
        if !opener {
            // the lift or door isn't coming by itself: use a trigger for it she's next to
            let here = nav.mesh.poly_at(pos);
            let near: BTreeSet<usize> = std::iter::once(cur).chain(here).collect();
            let mut involved: BTreeSet<usize> = [nav.sector_of_poly(cur), sector].into_iter().collect();
            involved.extend(here.map(|h| nav.sector_of_poly(h)));
            for t in &view.triggers {
                if !matches!(t.info.trigger, Trigger::SR | Trigger::S1 | Trigger::DR | Trigger::D1) || !t.sectors.iter().any(|s| involved.contains(s)) || near.is_disjoint(&t.activators) {
                    continue;
                }
                switch_nearby = true;
                if waited > 3 * 35 && still {
                    // the nearest bit of it (a long lift edge's middle can be out of use range:
                    // MAP29), stepping up to it if need be
                    let Some(li) = t.linedef() else { break };
                    let (la, lb) = nav.map.line_seg(li);
                    let length = dist(la, lb).max(1e-6);
                    let k = ((pos.0 - la.0) * (lb.0 - la.0) + (pos.1 - la.1) * (lb.1 - la.1)) / (length * length);
                    let k = k.max(0.5f64.min(8.0 / length)).min(0.5f64.max(1.0 - 8.0 / length));
                    let point = (la.0 + (lb.0 - la.0) * k, la.1 + (lb.1 - la.1) * k);
                    look = heading(pos, point);
                    if dist(pos, point) > PRESS_RANGE {
                        let gap = dist(pos, point) - PRESS_RANGE + 4.0;
                        let h = look.to_radians();
                        let v = 4.0f64.min(0.3 * gap);
                        want = (h.cos() * v, h.sin() * v);
                    } else {
                        use_ = norm(look - obs.angle).abs() < 10.0 && obs.tic - self.last_use >= USE_EVERY;
                    }
                }
                break;
            }
        }
        let mut controls = self.motion.hold(obs, want, Some(look));
        controls.use_ = use_;
        if use_ {
            self.last_use = obs.tic;
        }
        // nothing to press here: it may need a walk-over line crossing again (a blazing lift
        // that went back up), so replan from the live heights after a few seconds; at once if
        // the window counted on has run out
        let missed = !opener && waited == 35 / 2 && self.window_over(sector);
        if waited == 0 {
            self.reopening = self.reopens(sector, live[sector]);
            if self.reopening {
                self.log.add(obs.tic, format!("sector {sector} shut for 30s: waiting for it to reopen"));
            }
        }
        if self.reopening {
            // a "close, wait 30s, open" door she walked into the trigger of (E1M6): nothing to
            // do but wait, as a player would; spin round lazily meanwhile
            if waited > 33 * 35 {
                self.reopening = false;
                if !self.replan(obs, &format!("sector {sector} not reopening"), false) {
                    return Some(self.fail(obs, &format!("waited too long for sector {sector} to reopen")));
                }
            }
            return Some(self.motion.hold(obs, want, Some((obs.angle + 10.0).rem_euclid(360.0))));
        }
        // still on its way to where the plan has it (MAP29's floor that takes half a minute to
        // rise to the walkway): nothing to replan, just wait
        let coming = self.watches.iter().any(|w| w.expect.sector == sector && w.reached.is_none() && ((obs.tic - w.since) as f64) < 1.5 * w.expect.travel + 10.0 * TICRATE);
        if coming && !opener {
            self.wait_tic = Some(obs.tic - waited.min(4 * 35)); // (keeps "waited" short)
            return Some(self.motion.hold(obs, want, Some(look)));
        }
        if missed || (!opener && waited == 5 * 35 && !switch_nearby) {
            let why = if missed { format!("missed sector {sector}'s window") } else { format!("sector {sector} not coming") };
            if self.replan(obs, &why, true) {
                return Some(self.motion.hold(obs, (0.0, 0.0), Some(look)));
            }
        }
        if waited > 15 * 35 {
            if !self.replan(obs, &format!("sector {sector} not opening"), false) {
                let cs = nav.sector_of_poly(cur);
                return Some(self.fail(obs, &format!("waited too long for sector {sector}: live {} -> {}", fmt_live(Some(live[cs])), fmt_live(Some(live[sector])))));
            }
            return Some(self.motion.hold(obs, (0.0, 0.0), Some(look)));
        }
        Some(controls)
    }

    // --- at the end of a step -----------------------------------------------------------------

    /// Wait (standing still) for the sectors the next path needs to finish moving.
    fn settle_step(&mut self, obs: &Observation) -> Controls {
        let at = |e: &Expect| obs.height(e.sector).is_some_and(|l| (l.floor - e.heights.floor).abs() < 1.0 && (l.ceiling - e.heights.ceiling).abs() < 1.0);
        let waiting: Vec<Expect> = self.settle.iter().filter(|e| !at(e)).cloned().collect();
        let took = (obs.tic - self.settle_tic) as f64 / TICRATE;
        if waiting.is_empty() {
            let secs: Vec<usize> = self.settle.iter().map(|e| e.sector).collect();
            self.log.add(obs.tic, format!("sectors {secs:?} in place after {took:.1}s"));
        } else if (obs.tic - self.settle_tic) as f64 > waiting.iter().map(|e| 1.5 * e.travel).fold(f64::NEG_INFINITY, f64::max) + TICRATE {
            let e = &waiting[0];
            self.log.add(obs.tic, format!("sector {} didn't reach {} in {took:.1}s (expected {:.1}s, it's at {}); going on", e.sector, fmt_heights(e.heights), e.travel / TICRATE, fmt_live(obs.height(e.sector))));
        } else {
            self.progress_tic = obs.tic;
            return self.motion.hold(obs, (0.0, 0.0), self.look);
        }
        self.settle.clear();
        self.advance(obs.tic);
        if self.done {
            return Controls::default();
        }
        self.walk(obs)
    }

    /// Check what fired triggers are doing against what the plan expected, and log it.
    fn check_watches(&mut self, obs: &Observation) {
        let mut log: Vec<String> = Vec::new();
        for w in &mut self.watches {
            let e = &w.expect;
            let Some(live) = obs.height(e.sector) else { continue };
            if w.over {
                continue;
            }
            let at = (live.floor - e.heights.floor).abs() < 1.0 && (live.ceiling - e.heights.ceiling).abs() < 1.0;
            match w.reached {
                None => {
                    if at {
                        w.reached = Some(obs.tic);
                        let took = (obs.tic - w.since) as f64 / TICRATE;
                        if e.needed || took > 1.5 * e.travel / TICRATE + 1.0 {
                            log.push(format!("sector {} reached {} after {took:.1}s (expected {:.1}s)", e.sector, fmt_heights(e.heights), e.travel / TICRATE));
                        }
                    } else if !w.late && ((obs.tic - w.since) as f64) > 1.5 * e.travel + 2.0 * TICRATE {
                        w.late = true;
                        log.push(format!(
                            "sector {} hasn't reached {} {:.1}s after line {} (expected {:.1}s); it's at {}",
                            e.sector,
                            fmt_heights(e.heights),
                            (obs.tic - w.since) as f64 / TICRATE,
                            w.line,
                            e.travel / TICRATE,
                            fmt_live(Some(live))
                        ));
                    }
                }
                Some(r) => {
                    if let Some(hold) = e.hold {
                        let since = (obs.tic - r) as f64;
                        if (!at && since > hold) || since > hold + e.travel {
                            w.over = true;
                            if e.needed {
                                log.push(format!("sector {}'s window is over (line {})", e.sector, w.line));
                            }
                        }
                    }
                }
            }
        }
        for m in log {
            self.log.add(obs.tic, m);
        }
    }

    /// Did the temporary state the step before set up for this sector (lift down, door open)
    /// run out? Each closing counts once.
    fn window_over(&mut self, sector: usize) -> bool {
        let prev = self.step_i - 1;
        if let Some(w) = self.watches.iter_mut().find(|w| w.over && !w.used && w.expect.sector == sector && w.step == prev) {
            w.used = true;
            return true;
        }
        false
    }

    /// Has the trigger done something? A sector it moves has moved since it fired, or is at a
    /// height it moves to already (a door open anyway).
    fn effect_started(&self, obs: &Observation, t: &crate::navigator::TriggerLine) -> bool {
        if t.effects.is_empty() {
            return true; // (nothing to check)
        }
        for (s, hs) in &t.effects {
            let Some(live) = obs.height(*s) else { continue };
            let was = self.before.as_ref().and_then(|b| b.iter().find(|(x, _)| x == s).and_then(|(_, h)| *h));
            if was.is_some_and(|w| (live.floor - w.floor).abs() > 0.5 || (live.ceiling - w.ceiling).abs() > 0.5) {
                return true;
            }
            if hs.iter().any(|h| (live.floor - h.floor).abs() < 1.0 && (live.ceiling - h.ceiling).abs() < 1.0) {
                return true;
            }
        }
        false
    }

    /// At the pickup spot without the item: walk straight at it a moment.
    fn fetch(&mut self, obs: &Observation) -> Controls {
        let key = self.step().key;
        if obs.keys.is_some_and(|k| key.is_some_and(|sk| k.has(sk))) {
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        let Some(item) = self.item else {
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        };
        if obs.tic - self.progress_tic > 3 * 35 {
            self.log.add(obs.tic, format!("couldn't pick up {} at ({:.0}, {:.0}); carrying on", key.map_or("?", Key::name), obs.x, obs.y));
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        let hd = heading(obs.pos(), item);
        let speed = 6.0f64.min(0.3 * dist(obs.pos(), item) + 1.0);
        self.motion.go(obs, hd, speed, Some(hd))
    }

    /// At the end of a walk-over step without having crossed its line: step over it.
    fn walk_across(&mut self, obs: &Observation) -> Controls {
        let pos = obs.pos();
        let (a, b) = self.view.nav.map.line_seg(self.trigger_line_no());
        let q = closest_point(pos, a, b);
        let length = dist(a, b).max(1e-6);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let side = (pos.0 - q.0) * nx + (pos.1 - q.1) * ny;
        let sign = if side > 0.0 { -1.0 } else { 1.0 };
        // aim 24 past the line, at the nearest point along it (not too near its ends)
        let t = if length > 64.0 { (((q.0 - a.0) * (b.0 - a.0) + (q.1 - a.1) * (b.1 - a.1)) / (length * length)).clamp(0.15, 0.85) } else { 0.5 };
        let on = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let target = (on.0 + sign * nx * 24.0, on.1 + sign * ny * 24.0);
        if obs.tic - self.progress_tic > 3 * 35 {
            self.log.add(obs.tic, format!("could not cross line {}", self.trigger_line_no()));
            self.crossed = true; // (give up on it and carry on; replanning catches problems)
        }
        let hd = heading(pos, target);
        self.motion.go(obs, hd, 8.0f64.min(0.3 * dist(pos, target) + 2.0), Some(hd))
    }

    /// Is her floor at a height where a level shot hits the gun line? (Shots go straight out;
    /// some switches are hit from a lift at the right height.) Waits a few seconds at most.
    fn at_shot_height(&mut self, obs: &Observation, li: usize) -> bool {
        let nav = &self.view.nav;
        let Some(here) = nav.mesh.poly_at(obs.pos()) else { return true };
        let Some(&(lo, hi)) = nav.shot_floors.get(&(li, here)) else { return true };
        let floor = obs.heights[nav.sector_of_poly(here)].floor;
        // (well in, and still: the shot goes a few tics after the trigger's pulled, and a lift
        // rising through the bottom of the range has gone on by then: MAP16's table)
        let still = self.shot_floor.replace(floor).is_some_and(|f| (f - floor).abs() < 0.5);
        if still && lo + SHOT_MARGIN <= floor && floor <= hi - SHOT_MARGIN {
            return true;
        }
        let since = *self.height_wait.get_or_insert_with(|| {
            self.log.add(obs.tic, format!("waiting to be at the right height to shoot line {li} (floor {floor:.0}, needs {lo:.0}..{hi:.0})"));
            obs.tic
        });
        obs.tic - since > 6 * 35
    }

    /// Right after finishing a step: whatever comes next.
    fn carry_on(&mut self, obs: &Observation) -> Controls {
        if self.done || self.failed {
            return Controls::default();
        }
        if self.phase == Phase::Settle {
            return self.settle_step(obs);
        }
        self.walk(obs)
    }

    fn act_at_goal(&mut self, obs: &Observation) -> Controls {
        let step = self.step().clone();
        let view = Rc::clone(&self.view);
        let nav = &view.nav;
        if step.action == Action::Endure {
            // an exit floor: stand on it while it hurts her down to 10 health, which ends the
            // level (E1M8)
            self.step_tic = obs.tic;
            self.progress_tic = obs.tic;
            return self.motion.hold(obs, (0.0, 0.0), self.look);
        }
        if step.action == Action::Cross && !self.crossed && step.trigger.is_some() {
            return self.walk_across(obs);
        }
        if let Some(t) = step.trigger.as_ref().filter(|t| step.action == Action::Cross && t.info.category != Category::Exit) {
            if self.cross_tic.is_none() {
                self.before = Some(t.effects.iter().map(|(s, _)| (*s, obs.height(*s))).collect());
                self.cross_tic = Some(obs.tic);
            }
            if !self.effect_started(obs, t) {
                if obs.tic - self.cross_tic.unwrap_or(obs.tic) < 35 {
                    return self.motion.hold(obs, (0.0, 0.0), self.look);
                }
                self.log.add(obs.tic, format!("crossing line {} didn't do anything", t.line));
            }
        }
        if matches!(step.action, Action::Pickup | Action::Cross | Action::Walk) {
            self.next_step(obs.tic);
            return self.carry_on(obs);
        }
        if step.action == Action::Kill {
            self.log.add(obs.tic, "skipping boss kill (no monsters in this run)");
            self.next_step(obs.tic);
            return Controls::default();
        }
        let Some(t) = step.trigger.clone() else {
            self.next_step(obs.tic);
            return self.carry_on(obs);
        };
        // aim at the nearest part of the line (a little in from its ends): the use trace is a
        // 64-unit ray, and from off to one side the line's middle can be out of its reach
        let li = t.linedef().unwrap_or(0);
        let (a, b) = nav.map.line_seg(li);
        let length = dist(a, b).max(1e-6);
        let pos = obs.pos();
        let mut tt = ((pos.0 - a.0) * (b.0 - a.0) + (pos.1 - a.1) * (b.1 - a.1)) / (length * length);
        tt = tt.max(0.5f64.min(8.0 / length)).min(0.5f64.max(1.0 - 8.0 / length));
        if step.action == Action::Shoot {
            tt = 0.5; // (a shot reaches: the middle, where it's widest to hit)
        }
        let aim = (a.0 + (b.0 - a.0) * tt, a.1 + (b.1 - a.1) * tt);
        let look = heading(pos, aim);
        let err = norm(look - obs.angle).abs();
        let span = norm(heading(pos, a) - heading(pos, b)).abs();
        // hold still (brake) while turning to face the line
        let last_seg = &self.segments[self.segments.len() - 1].track;
        let mut end = last_seg.pts[last_seg.pts.len() - 1];
        if self.press_tries > 0 {
            // missed: stand a bit closer each time
            let d = dist(end, aim);
            let k = (8.0 * f64::from(self.press_tries)).min((d - PLAYER_RADIUS - 4.0).max(0.0)) / d.max(1e-6);
            end = (end.0 + (aim.0 - end.0) * k, end.1 + (aim.1 - end.1) * k);
        }
        let mut controls = self.motion.hold(obs, ((end.0 - obs.x) * 0.2, (end.1 - obs.y) * 0.2), Some(look));
        let facing = err < if step.action == Action::Shoot { 4.0f64.min(span / 3.0) } else { 4.0 };
        if step.action == Action::Face {
            // walk up to it and look at it; pressing is left to a person
            if facing {
                self.log.add(obs.tic, format!("facing line {li}"));
                self.next_step(obs.tic);
            }
            return controls;
        }
        if step.action == Action::Use && self.action_tic.is_none() {
            if let Some((s0, _)) = t.shift_from {
                // stairs don't build while their first step is moving (MAP21: a lift as well):
                // wait till it's back up where the stairs left it
                let top = self.states[s0].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
                if obs.height(s0).is_some_and(|l| l.floor < top - 0.5) {
                    self.progress_tic = obs.tic;
                    self.step_tic = obs.tic;
                    return controls;
                }
            }
        }
        let pressing = matches!(step.action, Action::Use | Action::Shoot) && self.action_tic.is_none();
        if pressing {
            if let Some(relevant) = &t.relevant {
                // where it takes things depends on what's round it when pressed: wait for the
                // lifts and doors among those to be where the plan has them (MAP09)
                let init = &nav.movers.initial;
                let off: Vec<usize> = relevant
                    .iter()
                    .copied()
                    .filter(|&x| {
                        obs.height(x).is_some_and(|l| {
                            let h = rep(&self.states[x], init[x]);
                            (l.floor - h.floor).abs() > 0.5 || (l.ceiling - h.ceiling).abs() > 0.5
                        })
                    })
                    .collect();
                if !off.is_empty() && self.settle_logged != self.step_i {
                    self.log.add(obs.tic, format!("waiting for sector {} to settle before line {}", off[0], t.line));
                    self.settle_logged = self.step_i;
                    self.goal_tic = obs.tic;
                }
                if !off.is_empty() && obs.tic - self.goal_tic < SETTLE_WAIT {
                    self.progress_tic = obs.tic;
                    self.step_tic = obs.tic;
                    return controls;
                }
            }
        }
        if pressing && t.once() && t.shift_from.is_none() {
            // Doom ignores a switch on a sector that's busy (a door open on its timer), and a
            // once-only one is still there to press then: wait till what it works is at rest
            // (E3M4). What it works if she knows, and what's moving right by it, which she sees.
            let init = &nav.movers.initial;
            let mut cands: BTreeSet<usize> = t.effects.iter().map(|(s, _)| *s).collect();
            cands.extend(self.sectors_near(li));
            let busy: Vec<usize> = cands
                .into_iter()
                .filter(|&x| {
                    let Some(&m) = self.moved_at.get(&x) else { return false };
                    obs.tic - m < BUSY_TICS && !(obs.tic - m > REST_TICS && obs.height(x).is_some_and(|l| (l.floor - init[x].floor).abs() < 0.5 && (l.ceiling - init[x].ceiling).abs() < 0.5))
                })
                .collect();
            // (its own timer: MAP19's door switch, pressed with the lift set off on the way still
            // running, did nothing, forty-odd times)
            if !busy.is_empty() && self.rest_logged != self.step_i {
                self.log.add(obs.tic, format!("waiting for sector {} to come to rest before line {}", busy[0], t.line));
                self.rest_logged = self.step_i;
                self.rest_tic = obs.tic;
            }
            if !busy.is_empty() && obs.tic - self.rest_tic < SETTLE_WAIT {
                self.progress_tic = obs.tic;
                self.step_tic = obs.tic;
                return controls;
            }
        }
        if pressing {
            if let Some(nt) = self.steps.get(self.step_i as usize + 1).filter(|n| n.action == Action::Cross).and_then(|n| n.trigger.clone()).filter(|nt| nt.info.trigger == Trigger::WR && !nt.permanent()) {
                // the next step crosses a lift line for its window: if that lift is away (set
                // off on the way, MAP09), wait for it now, before this press starts a timer of
                // its own the wait would use up
                let init = &nav.movers.initial;
                let away: Vec<usize> = nt.effects.iter().map(|(s, _)| *s).filter(|&x| obs.height(x).is_some_and(|l| (l.floor - init[x].floor).abs() > 0.5)).collect();
                if !away.is_empty() && obs.tic - self.step_tic < CROSS_REST_WAIT {
                    if !self.cross_wait_logged {
                        self.log.add(obs.tic, format!("sector {} already away: waiting for it to come back before line {}", away[0], t.line));
                        self.cross_wait_logged = true;
                    }
                    self.progress_tic = obs.tic;
                    return controls;
                }
            }
        }
        if step.action == Action::Shoot && self.action_tic.is_none() && obs.weapon != Some(tap_gun(obs)) {
            // a fist or chainsaw (picked up on the way: the game switches to it) doesn't set
            // off a gun line (MAP19), nor anything that fires a missile; a shotgun's pellets
            // spread (MAP16's flame panel, 3 degrees across): out with the pistol first
            controls.weapon = tap_gun(obs);
            self.progress_tic = obs.tic;
            return controls;
        }
        if step.action == Action::Shoot && self.action_tic.is_none() && facing && !self.at_shot_height(obs, li) {
            self.progress_tic = obs.tic; // (waiting for the floor, a lift, to bring her level)
            return controls;
        }
        if self.action_tic.is_none() && facing {
            self.action_tic = Some(obs.tic);
        }
        if let Some(at) = self.action_tic {
            let since = obs.tic - at;
            match step.action {
                Action::Use | Action::Exit => controls.use_ = since == 0,
                Action::Shoot => controls.attack = since < 10,
                _ => {}
            }
            if since == 0 {
                self.before = Some(t.effects.iter().map(|(s, _)| (*s, obs.height(*s))).collect());
                let tries = if self.press_tries > 0 { format!(" (try {})", self.press_tries + 1) } else { String::new() };
                self.log.add(obs.tic, format!("{} line {li}{tries}", step.action.name()));
            }
            // did it do anything? (an exit ends the level; otherwise something should move)
            if since > 8 && step.action != Action::Exit && self.effect_started(obs, &t) {
                self.next_step(obs.tic);
            } else if since > if step.action == Action::Exit { 2 * 35 } else { 25 } {
                self.press_tries += 1;
                if self.press_tries < PRESS_TRIES {
                    self.log.add(obs.tic, format!("line {li} didn't do anything, trying again closer"));
                    self.action_tic = None;
                } else {
                    self.log.add(obs.tic, format!("line {li} didn't work after {PRESS_TRIES} tries; carrying on"));
                    self.failed_presses.push((obs.pos(), li));
                    self.next_step(obs.tic);
                }
            }
        } else if obs.tic - self.progress_tic > 3 * 35 {
            return self.fail(obs, &format!("could not face line {li}"));
        }
        controls
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
