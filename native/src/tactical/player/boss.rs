// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The Icon of Sin, played: once the brain's awake and she has rockets, nothing else matters
//! but getting them into it. A post is chosen (`icon::lanes`), the floor it's on is got to
//! where a rocket from it goes in, and she fires from there till the brain's dead.
//!
//! This plans with the whole map known (`Navigator::full_view`) and the brain's place taken
//! from the map: an omniscient shortcut, the maintainer's call for this one kind of level
//! (docs/design/decisions, "The Icon of Sin"). The level is a race against what the brain
//! spawns, and a player comes to it knowing the trick.
//!
//! The ways a post's floor can be right:
//! - as it stands: walk there, face the brain, fire;
//! - a lift that passes through the heights that work: call it, board it at the bottom, and
//!   fire as it carries her through them (one rocket a ride: the launcher's slower than the
//!   window's long);
//! - either of those once something else has moved the floor for good (a switch that raises a
//!   pillar): that first.

use std::collections::BTreeSet;
use std::rc::Rc;

use super::Player;
use crate::domain::{heading, norm, Controls, Observation};
use crate::geom::{dist, Point};
use crate::motor::follower::RouteFollower;
use crate::motor::Motion;
use crate::nav::Heights;
use crate::navigator::{Plan, View};
use crate::navigator::{States, TriggerLine};
use crate::specials::Category;
use crate::tactical::icon::lanes::{self, Brain, Post, MUZZLE};
use crate::walkthrough::{action_for, steps_from_legs, Action, Step};

/// the launcher, as the game numbers its guns, and its slot
const LAUNCHER: i32 = 4;
const LAUNCHER_SLOT: i32 = 5;
/// from the press to the rocket leaving, with the launcher up and ready (its first frame's 8
/// tics)
const FIRE_DELAY: f64 = 8.0;
/// faced this near the way, the shot's good (degrees)
const AIMED: f64 = 0.3;
/// a floor still for this long has stopped (tics)
const STOPPED: i64 = 12;
/// boarding, or riding with no shot to be had, is given up after this long (tics)
const PATIENCE: i64 = 30 * 35;
/// a way that failed isn't planned again for this long (tics)
const AGAIN: i64 = 5 * 35;
/// what's in sight as the brain wakes is fought this long at most before she's off (tics)
const CLEARING: i64 = 12 * 35;
/// the BFG's slot
const BFG_SLOT: i32 = 7;
/// from a ride, what mends her is gone for if it's within this of the post (a jump off the
/// lift and a few steps, not a walk round the level)
const FETCH_WITHIN: f64 = 700.0;
/// ...a megasphere with health or armour under this
const FETCH_UNDER: f64 = 150.0;
/// a lift waits this long at the bottom (PLATWAIT, 3 s)
const LIFT_WAIT: i64 = 105;
/// she's back where she's needed this long before she's needed (tics)
const EARLY: i64 = 35;
/// her pace reckoned for getting back (units a tic: a run with a turn or two in it)
const PACE: f64 = 8.0;
/// no rocket with something in her aim this near: it would go off on it, by her
const TOO_NEAR: f64 = 176.0;

/// How a post's floor comes to be where a rocket from it goes in.
#[derive(Clone, Debug)]
enum How {
    /// it's there
    Stand,
    /// a lift carries her through: the triggers that call it
    Ride(Vec<usize>),
    /// once this trigger's moved it for good
    After(usize),
}

#[derive(Debug)]
enum Stage {
    /// the brain's just woken: what's shooting at her from where she stands is seen to first
    /// (since when)
    Clearing(i64),
    /// nothing in hand: a post is chosen
    Choosing,
    /// on the way to set off what moves the post's floor for good
    Setting(Post),
    /// set off: till it's stopped moving (since when)
    Settling(i64),
    /// on the way to call the lift
    Calling(Post),
    /// called: onto it at the bottom
    Boarding(Post, i64),
    /// on it, to fire as it carries her through
    Riding(Post, i64),
    /// the rocket's away: off the lift backwards over the line that calls it (its middle, and
    /// a spot just outside it), pressing it as she goes (since when)
    Leaving(Post, Point, Point, i64),
    /// on the way to a post good as it stands, then firing from it
    Standing(Post),
    /// a rocket's away and she's hurt: from where the ride has her, for what mends her (the
    /// things still to fetch, by their ids and places)
    Fetching(Vec<(i32, Point)>),
}

/// Whose the tic is, the Icon of Sin's business in hand.
pub(super) enum Turn {
    /// hers alone, these controls as they are: the shot and the moments before it
    Hers(Controls),
    /// a route's (`Boss::route`), the fight shooting over it at what's in the way
    Route,
    /// stood waiting, these controls, the fight shooting over them
    Wait(Controls),
    /// the same, and not a step from the spot whatever's coming: on a lift, off it is the ride
    /// lost
    Stay(Controls),
    /// time to spare (something of hers is moving and she's not needed yet): the fight's, its
    /// legs and all, these controls if it wants nothing
    Free(Controls),
}

/// The Icon of Sin's state.
pub(super) struct Boss {
    brain: Option<Brain>,
    awake: bool,
    engaged: bool,
    view: Option<Rc<View>>,
    stage: Stage,
    pub(super) route: Option<RouteFollower>,
    motion: Motion,
    /// the floor under her last tic and how long it's been where it is
    floor: (f64, i64),
    /// its climb a tic, last tic
    rate: f64,
    /// a rocket's away this ride
    fired: Option<i64>,
    /// nothing to be done till this tic (a way failed, or none found)
    again: i64,
    rockets_fired: u32,
    /// times she's heard it hurt
    hurt: u32,
    /// the heights last tic, and when any of them last changed
    heights: (Vec<Heights>, i64),
    /// when each sector's last did
    moved: Vec<i64>,
    /// a lift's floor a tic ago, while she's leaving it
    before: std::collections::BTreeMap<usize, f64>,
    /// what she's set off to ready a post: not gone back to
    set_off: BTreeSet<usize>,
    /// ...and the one she's on the way to
    setting: Option<usize>,
    /// the brain woke then, and what was in sight hasn't been seen to yet
    clear_from: Option<i64>,
    /// the gun the fight was told to use before she took the BFG to what was in sight
    gun_before: Option<Option<i32>>,
    /// the lift she rides, and where it rests
    rest: Option<(usize, f64)>,
    /// what she's gone for from a ride already (ids): not again
    fetched: BTreeSet<i32>,
    /// what's settling: the sector, where its floor's going, and where she'll wait for it
    settling: Option<(usize, f64, Vec<Point>)>,
}

impl Boss {
    pub(super) fn new(nav: &crate::navigator::Navigator) -> Boss {
        Boss {
            brain: lanes::brain_of(&nav.map, &nav.mesh),
            awake: false,
            engaged: false,
            view: None,
            stage: Stage::Choosing,
            route: None,
            motion: Motion::default(),
            floor: (0.0, 0),
            rate: 0.0,
            fired: None,
            again: 0,
            rockets_fired: 0,
            hurt: 0,
            heights: (Vec::new(), 0),
            moved: Vec::new(),
            before: std::collections::BTreeMap::new(),
            set_off: BTreeSet::new(),
            setting: None,
            clear_from: None,
            gun_before: None,
            rest: None,
            fetched: BTreeSet::new(),
            settling: None,
        }
    }

    fn heights_before(&self, sector: usize) -> f64 {
        self.before
            .get(&sector)
            .copied()
            .unwrap_or(f64::NEG_INFINITY)
    }
}

fn floor_of(effects: &crate::movers::Effects, sector: usize) -> Option<f64> {
    effects
        .iter()
        .find(|(s, _)| *s == sector)
        .and_then(|(_, hs)| hs.iter().map(|h| h.floor).reduce(f64::min))
}

impl Player {
    /// The level's start, or a life's: nothing in hand, the brain as awake as it was (it
    /// doesn't sleep again).
    pub(super) fn boss_restart(&mut self) {
        self.boss.stage = Stage::Choosing;
        self.boss.route = None;
        self.boss.engaged = false;
        self.boss.again = 0;
    }

    /// Is the Icon of Sin's business what she's about?
    pub fn at_the_icon(&self) -> bool {
        self.boss.engaged
    }

    /// Rockets are for the brain, once it's awake: the fight has the other guns.
    pub(super) fn rockets_kept(&self) -> bool {
        self.boss.awake && self.boss.brain.is_some()
    }

    fn boss_log(&mut self, obs: &Observation, s: String) {
        self.explorer
            .log
            .push(format!("tic {}: [icon] {s}", obs.tic));
    }

    fn rockets(obs: &Observation) -> i32 {
        let has = obs.owned.as_ref().is_some_and(|o| o.contains(&LAUNCHER));
        if !has {
            return 0;
        }
        obs.ammo
            .as_ref()
            .and_then(|a| a.iter().find(|(slot, _)| *slot == LAUNCHER_SLOT))
            .map_or(0, |a| a.1)
    }

    /// The Icon of Sin's turn, if it's her business now.
    pub(super) fn boss(&mut self, obs: &Observation) -> Option<Turn> {
        let brain = self.boss.brain?;
        if !self.boss.awake {
            // Its cry on seeing her is heard the level over.
            if !obs.sounds.iter().any(|s| s.name == "dsbossit") {
                return None;
            }
            self.boss.awake = true;
            self.boss.clear_from = Some(obs.tic);
            self.boss_log(obs, "the brain's awake".into());
        }
        // (its cry at each rocket that hurts it, heard the level over too)
        if obs.sounds.iter().any(|s| s.name == "dsbospn") {
            self.boss.hurt += 1;
            let n = self.boss.hurt;
            self.boss_log(obs, format!("the brain's hurt ({n})"));
        }
        if Self::rockets(obs) == 0 {
            if self.boss.engaged {
                self.boss_log(obs, "out of rockets: back to looking about".into());
                self.boss_restart();
            }
            return None;
        }
        if !self.boss.engaged {
            if obs.tic < self.boss.again {
                return None;
            }
            self.boss.engaged = true;
            self.boss.stage = match self.boss.clear_from.take() {
                Some(t) => Stage::Clearing(t),
                None => Stage::Choosing,
            };
        }
        self.boss.motion.observe(obs);
        if self.boss.heights.0 != obs.heights {
            self.boss.moved.resize(obs.heights.len(), i64::MIN / 2);
            for (s, h) in obs.heights.iter().enumerate() {
                if self.boss.heights.0.get(s) != Some(h) {
                    self.boss.moved[s] = obs.tic;
                }
            }
            self.boss.heights = (obs.heights.clone(), obs.tic);
        }
        let floor = self.floor_under(obs);
        self.boss.rate = floor - self.boss.floor.0;
        if (floor - self.boss.floor.0).abs() > 0.01 {
            self.boss.floor = (floor, obs.tic);
        }
        if let Some(r) = self.boss.route.as_mut() {
            for e in std::mem::take(&mut r.log.events) {
                self.explorer.log.push(format!("    [icon] {e}"));
            }
        }
        let c = match std::mem::replace(&mut self.boss.stage, Stage::Choosing) {
            Stage::Clearing(since) => {
                // What was standing there when she came in will be shooting at her the whole
                // way round: a few seconds for it now, with everything she has but the rockets.
                let shooter = self.fight.combat.visible.iter().any(|t| {
                    crate::tactical::bestiary::monster(t.name)
                        .is_some_and(|m| m.attack != crate::tactical::bestiary::Attack::None)
                });
                if shooter && obs.tic - since < CLEARING {
                    // (the BFG for it, while there's a shot in it)
                    if self.boss.gun_before.is_none() {
                        self.boss.gun_before = Some(self.fight.combat.ordered_gun);
                        self.fight.combat.ordered_gun = Some(BFG_SLOT);
                    }
                    self.boss.stage = Stage::Clearing(since);
                    Some(Turn::Free(Controls::default()))
                } else {
                    if let Some(before) = self.boss.gun_before.take() {
                        self.fight.combat.ordered_gun = before;
                    }
                    self.boss_choose(obs, brain)
                }
            }
            Stage::Choosing => self.boss_choose(obs, brain),
            Stage::Setting(p) => {
                let c = self.boss_follow(obs, Stage::Setting(p.clone()), Stage::Settling(obs.tic));
                if matches!(self.boss.stage, Stage::Settling(_)) {
                    let done = self.boss.setting.take();
                    self.boss.set_off.extend(done);
                    // What she set off takes its time: over to where the floor's lifts are
                    // called from meanwhile, not stood where she pressed it.
                    let lifts: Vec<usize> = self
                        .boss_view()
                        .triggers
                        .iter()
                        .filter(|t| {
                            t.info.category == Category::Lift && t.sectors.contains(&p.sector)
                        })
                        .map(|t| t.id)
                        .collect();
                    let by: Vec<(Point, Option<usize>)> = self
                        .boss_presses(obs, &lifts)
                        .into_iter()
                        .filter_map(|(at, _)| self.boss_dry_by(obs, at, p.sector))
                        .map(|at| (at, None))
                        .collect();
                    let to = done
                        .map(|id| self.boss_view().trigger(id).clone())
                        .and_then(|t| {
                            let states: States = obs.heights.iter().map(|&h| vec![h]).collect();
                            let view = self.boss_view();
                            floor_of(&view.nav.effects_now(&t, &states), p.sector)
                        })
                        .unwrap_or(obs.heights[p.sector].floor);
                    self.boss.route = None;
                    self.boss.settling =
                        Some((p.sector, to, by.into_iter().map(|g| g.0).collect()));
                }
                c
            }
            Stage::Settling(since) => Some(self.boss_settle(obs, since)),
            Stage::Calling(p) => {
                let next = Stage::Boarding(p.clone(), obs.tic);
                self.boss_follow(obs, Stage::Calling(p), next)
            }
            Stage::Boarding(p, since) => self.boss_board(obs, p, since),
            Stage::Riding(p, since) => Some(self.boss_ride(obs, brain, p, since)),
            Stage::Leaving(p, mid, out, since) => Some(self.boss_leave(obs, p, mid, out, since)),
            Stage::Standing(p) => Some(self.boss_stand(obs, brain, p)),
            Stage::Fetching(rest) => Some(self.boss_fetch(obs, rest)),
        };
        if c.is_none() {
            // Nothing she can do about it now: the exploring's, till something changes.
            self.boss.engaged = false;
            self.boss.route = None;
            self.boss.again = obs.tic + AGAIN;
        }
        c
    }

    fn floor_under(&self, obs: &Observation) -> f64 {
        let nav = &self.explorer.nav;
        nav.poly_at(obs.pos()).map_or(obs.z.unwrap_or(0.0), |p| {
            obs.heights[nav.sector_of_poly(p)].floor
        })
    }

    fn sector_under(&self, obs: &Observation) -> Option<usize> {
        let nav = &self.explorer.nav;
        nav.poly_at(obs.pos()).map(|p| nav.sector_of_poly(p))
    }

    fn boss_view(&mut self) -> Rc<View> {
        if self.boss.view.is_none() {
            self.boss.view = Some(Rc::new(self.explorer.nav.full_view()));
        }
        Rc::clone(self.boss.view.as_ref().expect("just made"))
    }

    /// A way to one of those spots (each with the trigger to set off there, if it's for one),
    /// through whatever it takes, the whole map known.
    fn boss_way(
        &mut self,
        obs: &Observation,
        goals: &[(Point, Option<usize>)],
        label: &str,
    ) -> Option<RouteFollower> {
        self.boss_way_at(obs, &obs.heights, goals, label)
    }

    /// The same, the heights taken to be `heights` (a lift she's on, where it's going).
    fn boss_way_at(
        &mut self,
        obs: &Observation,
        heights: &[Heights],
        goals: &[(Point, Option<usize>)],
        label: &str,
    ) -> Option<RouteFollower> {
        let view = self.boss_view();
        let live: States = heights.iter().map(|&h| vec![h]).collect();
        let keys = obs.keys.unwrap_or_default();
        let key_items = view.nav.key_items(None);
        let points: Vec<Point> = goals.iter().map(|g| g.0).collect();
        let triggers: Vec<(Point, usize)> =
            goals.iter().filter_map(|g| g.1.map(|t| (g.0, t))).collect();
        let route = view.plan(
            obs.pos(),
            &points,
            &Plan {
                keys,
                states: Some(&live),
                key_items: &key_items,
                max_expansions: 20000,
                max_seconds: 5.0,
                goal_triggers: &triggers,
                fired: &BTreeSet::new(),
            },
        )?;
        let trigger = route
            .goal
            .and_then(|g| triggers.iter().find(|(p, _)| *p == g))
            .map(|(_, id)| view.trigger(*id).clone());
        let goal = Step {
            path: None,
            action: trigger.as_ref().map_or(Action::Walk, action_for),
            trigger,
            key: None,
            label: label.to_string(),
            expects: Vec::new(),
            barrel: None,
        };
        let steps = steps_from_legs(&view, &route.legs, Some(goal), Some(&live));
        Some(RouteFollower::new(
            view,
            steps,
            keys,
            Some(live),
            false,
            obs.tic,
        ))
    }

    /// The nearest spot to `at` to wait at on a floor that doesn't hurt (`at` itself, if its
    /// floor doesn't).
    fn boss_dry_by(&self, obs: &Observation, at: Point, not: usize) -> Option<Point> {
        let nav = &self.explorer.nav;
        // (not on the floor that's moving itself)
        let dry = |q: Point| {
            lanes::standable(&nav.map, &nav.mesh, &obs.heights, q)
                .is_some_and(|s| s != not && nav.damage[s] == 0)
        };
        if dry(at) {
            return Some(at);
        }
        let mut best: Option<(f64, Point)> = None;
        for i in -16..=16 {
            for j in -16..=16 {
                let q = (at.0 + 32.0 * f64::from(i), at.1 + 32.0 * f64::from(j));
                let d = dist(q, at);
                if best.is_none_or(|b| d < b.0) && dry(q) {
                    best = Some((d, q));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Where to stand to set each of those triggers off.
    fn boss_presses(&mut self, obs: &Observation, ids: &[usize]) -> Vec<(Point, Option<usize>)> {
        let view = self.boss_view();
        let all: BTreeSet<usize> = view.nav.mesh.live_polys().map(|(i, _)| i).collect();
        ids.iter()
            .map(|&id| {
                let t: &TriggerLine = view.trigger(id);
                (view.nav.activation_point(t, obs.pos(), &all), Some(id))
            })
            .collect()
    }

    /// The posts there are, each with how its floor comes right, the best first: one good as
    /// it stands before a ride, a ride before something to set off first; then the most hurt a
    /// rocket, then the nearest the middle of its floor (a lift's edge is no place to ride).
    fn boss_posts(&mut self, obs: &Observation, brain: Brain) -> Vec<(Post, How)> {
        let view = self.boss_view();
        let nav = Rc::clone(&self.explorer.nav);
        let all = lanes::posts(&nav.map, &nav.mesh, &obs.heights, brain);
        let states: States = obs.heights.iter().map(|&h| vec![h]).collect();
        let lifts = |sector: usize, rest: f64, feet: (f64, f64)| -> Vec<usize> {
            view.triggers
                .iter()
                .filter(|t| t.info.category == Category::Lift && t.sectors.contains(&sector))
                .filter(|t| {
                    floor_of(&view.nav.effects_now(t, &states), sector)
                        .is_some_and(|low| low < feet.0 && rest >= feet.1)
                })
                .map(|t| t.id)
                .collect()
        };
        let mut out: Vec<(u8, Post, How)> = Vec::new();
        for p in all {
            let floor = obs.heights[p.sector].floor;
            // (a lift she's ridden, down or on its way: where it rests is where it'll be)
            let rest = self
                .boss
                .rest
                .filter(|r| r.0 == p.sector)
                .map_or(floor, |r| r.1.max(floor));
            // (as it stands: stood, not a lift passing through)
            let still = self
                .boss
                .moved
                .get(p.sector)
                .is_none_or(|&t| obs.tic - t > STOPPED);
            if still && p.feet.0 <= floor && floor <= p.feet.1 {
                out.push((0, p, How::Stand));
                continue;
            }
            let ride = lifts(p.sector, rest, p.feet);
            if !ride.is_empty() {
                out.push((1, p, How::Ride(ride)));
                continue;
            }
            // Something that moves the floor for good, to where it's right or a lift makes it so.
            let first = view
                .triggers
                .iter()
                .filter(|t| t.permanent() && t.sectors.contains(&p.sector))
                .filter(|t| !self.boss.set_off.contains(&t.id))
                .find(|t| {
                    floor_of(&view.nav.effects_now(t, &states), p.sector).is_some_and(|to| {
                        (to - floor).abs() > 0.5
                            && ((p.feet.0 <= to && to <= p.feet.1)
                                || !lifts(p.sector, to, p.feet).is_empty())
                    })
                })
                .map(|t| t.id);
            if let Some(id) = first {
                out.push((2, p, How::After(id)));
            }
        }
        let middle = |p: &Post| {
            nav.poly_at(p.at)
                .map_or(0.0, |i| dist(nav.mesh.poly(i).center, p.at))
        };
        out.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.damage.round().total_cmp(&a.1.damage.round()))
                .then(middle(&a.1).total_cmp(&middle(&b.1)))
        });
        out.into_iter().map(|(_, p, how)| (p, how)).collect()
    }

    fn boss_choose(&mut self, obs: &Observation, brain: Brain) -> Option<Turn> {
        let posts = self.boss_posts(obs, brain);
        if posts.is_empty() {
            self.boss_log(
                obs,
                "no place to fire a rocket into it from, as things are".into(),
            );
            return None;
        }
        // One sector's best post is as good as its others for finding a way: the first of each
        // sector and kind that a way's found to.
        let mut tried: BTreeSet<(usize, u8)> = BTreeSet::new();
        for (p, how) in posts {
            let kind = match how {
                How::Stand => 0,
                How::Ride(_) => 1,
                How::After(_) => 2,
            };
            if !tried.insert((p.sector, kind)) {
                continue;
            }
            match how {
                How::Stand => {
                    if let Some(route) = self.boss_way(obs, &[(p.at, None)], "to fire at the brain")
                    {
                        self.boss_log(
                            obs,
                            format!(
                                "a place to fire from as it stands: ({:.0}, {:.0}), {:.0} a rocket",
                                p.at.0, p.at.1, p.damage
                            ),
                        );
                        self.boss.route = Some(route);
                        self.boss.stage = Stage::Standing(p);
                        return Some(Turn::Wait(Controls::default()));
                    }
                }
                How::Ride(lifts) => {
                    // On it already, under where it works: ride.
                    if self.sector_under(obs) == Some(p.sector) && self.boss.floor.0 < p.feet.0 {
                        self.boss.fired = None;
                        self.boss.stage = Stage::Riding(p, obs.tic);
                        return Some(Turn::Wait(Controls::default()));
                    }
                    // (the nearest place to call it from, if there's a way to that; else any)
                    let mut goals = self.boss_presses(obs, &lifts);
                    let pos = obs.pos();
                    goals.sort_by(|a, b| dist(a.0, pos).total_cmp(&dist(b.0, pos)));
                    let route = self
                        .boss_way(obs, &goals[..1], "call the lift")
                        .or_else(|| self.boss_way(obs, &goals, "call the lift"));
                    if let Some(route) = route {
                        self.boss.rest = Some((p.sector, obs.heights[p.sector].floor));
                        self.boss_log(
                            obs,
                            format!(
                                "a lift to fire from on its way up: sector {}, feet {:.0} to {:.0}, {:.0} a rocket",
                                p.sector, p.feet.0, p.feet.1, p.damage
                            ),
                        );
                        self.boss.route = Some(route);
                        self.boss.stage = Stage::Calling(p);
                        return Some(Turn::Wait(Controls::default()));
                    }
                }
                How::After(id) => {
                    let goals = self.boss_presses(obs, &[id]);
                    if let Some(route) = self.boss_way(obs, &goals, "ready the place to fire from")
                    {
                        let line = self.boss_view().trigger(id).line;
                        self.boss_log(
                            obs,
                            format!("sector {} has to be moved first: line {line}", p.sector),
                        );
                        self.boss.route = Some(route);
                        self.boss.setting = Some(id);
                        self.boss.stage = Stage::Setting(p);
                        return Some(Turn::Wait(Controls::default()));
                    }
                }
            }
        }
        self.boss_log(obs, "no way to a place to fire from".into());
        None
    }

    /// What she set off is on its way: the fight's time, till she's needed where the floor's
    /// lifts are called from; over there as it stops.
    fn boss_settle(&mut self, obs: &Observation, since: i64) -> Turn {
        let moving = obs.tic - self.boss.heights.1 <= STOPPED;
        if !((moving || obs.tic - since <= STOPPED) && obs.tic - since < 2 * PATIENCE) {
            self.boss.route = None;
            self.boss.settling = None;
            return Turn::Wait(Controls::default());
        }
        self.boss.stage = Stage::Settling(since);
        let Some((sector, to, goals)) = self.boss.settling.clone() else {
            return Turn::Free(Controls::default());
        };
        let floor = obs.heights[sector].floor;
        let rate = (floor - self.boss.heights_before(sector)).abs();
        self.boss.before.insert(sector, floor);
        let left = if rate > 0.01 && rate.is_finite() {
            (to - floor).abs() / rate
        } else {
            0.0
        };
        let pos = obs.pos();
        let near = goals
            .iter()
            .map(|&g| dist(g, pos))
            .fold(f64::INFINITY, f64::min);
        if near < 48.0 {
            self.boss.route = None;
            return Turn::Free(self.boss.motion.hold(obs, (0.0, 0.0), None));
        }
        // Over there, the fight's to stop her for while there's time to spare, hers alone once
        // there isn't.
        if self.boss.route.as_ref().is_none_or(|r| r.done || r.failed) {
            let by: Vec<(Point, Option<usize>)> = goals.iter().map(|&g| (g, None)).collect();
            self.boss.route = self.boss_way(obs, &by, "over to the lift");
        }
        // (the first tics, before its pace is known: time to spare. None on a floor that
        // hurts: off it first)
        let hurting = self
            .sector_under(obs)
            .is_some_and(|s| self.explorer.nav.damage[s] != 0);
        if self.boss.route.is_some()
            && (hurting || (obs.tic - since > 2 && left <= near / PACE + EARLY as f64))
        {
            return Turn::Route;
        }
        Turn::Free(Controls::default())
    }

    /// The route in hand: `now` while it goes on, `then` once it's done.
    fn boss_follow(&mut self, obs: &Observation, now: Stage, then: Stage) -> Option<Turn> {
        let route = self.boss.route.as_ref()?;
        if route.failed {
            self.boss_log(obs, "that way failed".into());
            return None;
        }
        if route.done {
            self.boss.route = None;
            self.boss.stage = then;
            return Some(Turn::Wait(Controls::default()));
        }
        self.boss.stage = now;
        Some(Turn::Route)
    }

    /// The lift's called: onto it once it's down.
    fn boss_board(&mut self, obs: &Observation, p: Post, since: i64) -> Option<Turn> {
        if self.sector_under(obs) == Some(p.sector) && dist(obs.pos(), p.at) < 24.0 {
            self.boss.route = None;
            self.boss.fired = None;
            self.boss.stage = Stage::Riding(p, obs.tic);
            return Some(Turn::Wait(Controls::default()));
        }
        // Up where it rests and still: the call didn't take (pressed while it was on its way).
        let rest = obs.heights[p.sector].floor > p.feet.1;
        if obs.tic - since > PATIENCE
            || (rest && obs.tic - self.boss.heights.1 > 3 * 35 && obs.tic - since > 3 * 35)
        {
            self.boss_log(obs, "the lift didn't come: called again".into());
            self.boss.route = None;
            self.boss.stage = Stage::Choosing;
            return Some(Turn::Wait(Controls::default()));
        }
        self.boss.stage = Stage::Boarding(p.clone(), since);
        if self
            .boss
            .route
            .as_ref()
            .is_some_and(|r| !r.done && !r.failed)
        {
            return Some(Turn::Route);
        }
        // Till it's down the time's the fight's.
        let floor = obs.heights[p.sector].floor;
        let moved = self.boss.moved.get(p.sector).copied().unwrap_or(0);
        let down = obs.tic - moved > 1 && floor < p.feet.0;
        let rate = (floor - self.boss.heights_before(p.sector)).abs().max(1.0);
        self.boss.before.insert(p.sector, floor);
        let low = self.boss_low(obs, p.sector);
        let left = if down {
            (LIFT_WAIT - (obs.tic - moved)) as f64
        } else {
            (floor - low).max(0.0) / rate + LIFT_WAIT as f64
        };
        // (stood by it till it's down, the fight shooting from there: its legs would have her
        // off across the slime and the lift gone up without her)
        let _ = left;
        if !down {
            let face = heading(obs.pos(), p.at);
            return Some(Turn::Wait(self.boss.motion.hold(
                obs,
                (0.0, 0.0),
                Some(face),
            )));
        }
        self.boss.route = self.go_to_point(obs, p.at, &[], "onto the lift", 0.0, false);
        if self.boss.route.is_some() {
            return Some(Turn::Route);
        }
        // (no way on yet: towards it)
        let face = heading(obs.pos(), p.at);
        let want = ((p.at.0 - obs.x) * 0.3, (p.at.1 - obs.y) * 0.3);
        Some(Turn::Wait(self.boss.motion.hold(obs, want, Some(face))))
    }

    /// Where the sector's lifts take its floor down to.
    fn boss_low(&mut self, obs: &Observation, sector: usize) -> f64 {
        let view = self.boss_view();
        let states: States = obs.heights.iter().map(|&h| vec![h]).collect();
        view.triggers
            .iter()
            .filter(|t| t.info.category == Category::Lift && t.sectors.contains(&sector))
            .filter_map(|t| floor_of(&view.nav.effects_now(t, &states), sector))
            .fold(obs.heights[sector].floor, f64::min)
    }

    /// Would a rocket fired from where she is, her feet at `feet`, hurt the brain, faced as
    /// she'd face it and a little to either side? The floor under her is taken to be at her
    /// feet.
    fn boss_shot(&self, obs: &Observation, brain: Brain, feet: f64) -> bool {
        let nav = &self.explorer.nav;
        let mut heights: Vec<Heights> = obs.heights.clone();
        if let Some(s) = self.sector_under(obs) {
            heights[s].floor = feet;
        }
        let way = heading(obs.pos(), brain.at);
        [-0.4, 0.0, 0.4].into_iter().all(|off| {
            lanes::fly(
                &nav.map,
                &heights,
                brain,
                obs.pos(),
                way + off,
                feet + MUZZLE,
            )
            .0
            .is_some()
        })
    }

    /// What her aim would take a rocket to, fired the way `way`: a player's missile goes at the
    /// first thing it could hurt along the way she faces or 5.6 degrees to either side, within
    /// 1024 (P_SpawnPlayerMissile), and only level with nothing there.
    fn boss_aim_taken(obs: &Observation, way: f64) -> Vec<(&'static str, f64)> {
        let mut out = Vec::new();
        for t in obs.things.iter().flatten() {
            let Some(m) = crate::tactical::bestiary::monster(t.name) else {
                continue;
            };
            if t.health <= 0 {
                continue;
            }
            let (dx, dy) = (t.x - obs.x, t.y - obs.y);
            let hit = [-5.625, 0.0, 5.625].into_iter().any(|off| {
                let (sin, cos) = (way + off).to_radians().sin_cos();
                let (along, across) = (dx * cos + dy * sin, dy * cos - dx * sin);
                along > 0.0 && along < 1024.0 + m.radius && across.abs() < m.radius
            });
            if hit {
                out.push((t.name, dx.hypot(dy)));
            }
        }
        out
    }

    /// Held on the spot, faced to the brain, the launcher up; `fire`: and the trigger pressed,
    /// if she's faced right.
    fn boss_aim(&mut self, obs: &Observation, brain: Brain, at: Point, fire: bool) -> Controls {
        let way = heading(obs.pos(), brain.at);
        let want = ((at.0 - obs.x) * 0.3, (at.1 - obs.y) * 0.3);
        let mut c = self.boss.motion.hold(obs, want, Some(way));
        if obs.ready != Some(LAUNCHER) {
            c.weapon = LAUNCHER_SLOT;
            c.exact = Some(LAUNCHER);
        } else if fire && norm(way - obs.angle).abs() < AIMED {
            c.attack = true;
        }
        c
    }

    /// On the lift: a rocket as it carries her through the heights that work, pressed early
    /// by the launcher's delay. At the top, or stopped short with no shot: round again.
    fn boss_ride(&mut self, obs: &Observation, brain: Brain, p: Post, since: i64) -> Turn {
        let floor = self.boss.floor.0;
        let still = obs.tic - self.boss.floor.1 > STOPPED;
        let on = self.sector_under(obs) == Some(p.sector);
        // (a rocket pressed leaves 8 tics on: held as she was till it has, then off to call
        // the lift again, without riding it to the top first)
        let away = self
            .boss
            .fired
            .is_some_and(|t| obs.tic - t > FIRE_DELAY as i64 + 1);
        let over = !on || (still && floor > p.feet.1) || away || obs.tic - since > PATIENCE;
        if over {
            if self.boss.fired.is_none() {
                self.boss_log(obs, format!("no shot that ride (floor {floor:.0})"));
            }
            // Hurt, and something that mends her to be had from up here: that, before the next
            // ride.
            if on && away {
                if let Some(rest) = self.boss_fetch_from(obs, &p) {
                    self.boss.stage = Stage::Fetching(rest);
                    return Turn::Hers(self.boss_aim(obs, brain, p.at, false));
                }
            }
            self.boss.stage = match self.boss_call_line(obs, &p).filter(|_| on) {
                Some((mid, out)) => Stage::Leaving(p.clone(), mid, out, obs.tic),
                None => Stage::Choosing,
            };
            return Turn::Hers(self.boss_aim(obs, brain, p.at, false));
        }
        let rising = self.boss.rate > 0.0;
        let p_at = p.at;
        let near = Self::boss_aim_taken(obs, heading(obs.pos(), brain.at))
            .iter()
            .any(|t| t.1 < TOO_NEAR);
        let fire = self.boss.fired.is_none()
            && rising
            && !near
            && self.boss_shot(obs, brain, floor + self.boss.rate * FIRE_DELAY);
        let c = self.boss_aim(obs, brain, p.at, fire);
        if c.attack {
            self.boss.fired = Some(obs.tic);
            self.boss.rockets_fired += 1;
            self.boss_log(
                obs,
                format!(
                    "rocket {} away from the lift at floor {floor:.0}, climbing {:.0} a tic; in her aim: {:?}",
                    self.boss.rockets_fired,
                    self.boss.rate,
                    Self::boss_aim_taken(obs, heading(obs.pos(), brain.at))
                ),
            );
        }
        self.boss.stage = Stage::Riding(p, since);
        // On its way up she's the brain's alone; waiting at the bottom, the fight may shoot.
        if rising || c.attack {
            Turn::Hers(c)
        } else {
            let at = p_at;
            let face = heading(obs.pos(), brain.at);
            let want = ((at.0 - obs.x) * 0.3, (at.1 - obs.y) * 0.3);
            Turn::Stay(self.boss.motion.hold(obs, want, Some(face)))
        }
    }

    /// The nearest line that calls the post's lift: its middle, and a spot just outside it.
    fn boss_call_line(&mut self, obs: &Observation, p: &Post) -> Option<(Point, Point)> {
        let view = self.boss_view();
        let nav = Rc::clone(&self.explorer.nav);
        let pos = obs.pos();
        view.triggers
            .iter()
            .filter(|t| t.info.category == Category::Lift && t.sectors.contains(&p.sector))
            .filter_map(|t| t.linedef())
            .filter_map(|li| {
                let (a, b) = nav.map.line_seg(li);
                let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                let len = dist(a, b).max(1e-9);
                let n = (-(b.1 - a.1) / len, (b.0 - a.0) / len);
                // (the side that isn't the lift's)
                [1.0, -1.0].into_iter().find_map(|s| {
                    let out = (mid.0 + n.0 * s * 22.0, mid.1 + n.1 * s * 22.0);
                    let sector = nav.poly_at(out).map(|i| nav.sector_of_poly(i))?;
                    (sector != p.sector).then_some((mid, out))
                })
            })
            .min_by(|x, y| dist(x.1, pos).total_cmp(&dist(y.1, pos)))
    }

    /// Off the lift backwards over the line that calls it, faced to the line and pressing it
    /// once she's over: use has no height to it, and the lift is called before she's landed.
    fn boss_leave(
        &mut self,
        obs: &Observation,
        p: Post,
        mid: Point,
        out: Point,
        since: i64,
    ) -> Turn {
        let off = self.sector_under(obs) != Some(p.sector);
        let moved = self.boss.moved.get(p.sector).copied().unwrap_or(0);
        // Called: its floor's going down. Or it never took: the long way.
        if off && moved == obs.tic && self.boss.rate <= 0.0 && obs.tic - since > 4 {
            let down = obs.heights[p.sector].floor < self.boss.heights_before(p.sector);
            if down {
                self.boss.stage = Stage::Boarding(p, obs.tic);
                return Turn::Wait(Controls::default());
            }
        }
        if obs.tic - since > 4 * 35 {
            self.boss.stage = Stage::Choosing;
            return Turn::Wait(Controls::default());
        }
        let face = heading(out, mid);
        let want = ((out.0 - obs.x) * 0.5, (out.1 - obs.y) * 0.5);
        let mut c = self.boss.motion.hold(obs, want, Some(face));
        // (a press is the button going down: every other tic, once the lift's at rest)
        let rested = obs.tic - moved > 1;
        c.use_ = off && rested && obs.tic % 2 == 0;
        self.boss
            .before
            .insert(p.sector, obs.heights[p.sector].floor);
        self.boss.stage = Stage::Leaving(p, mid, out, since);
        Turn::Hers(c)
    }

    /// What's worth going for from the lift, a rocket just away, and a way to the first of it
    /// in hand: a megasphere she's seen near the post, with her health or armour down, and a
    /// radiation suit by it if the floor the lift's called from hurts. The way is planned from
    /// where the lift's taking her (its rest).
    fn boss_fetch_from(&mut self, obs: &Observation, p: &Post) -> Option<Vec<(i32, Point)>> {
        let hurt =
            obs.health.unwrap_or(100.0) < FETCH_UNDER || obs.armor.unwrap_or(0.0) < FETCH_UNDER;
        if !hurt {
            return None;
        }
        let near = |t: &&crate::domain::Thing| {
            dist((t.x, t.y), p.at) < FETCH_WITHIN && !self.boss.fetched.contains(&t.id)
        };
        let things: Vec<&crate::domain::Thing> = obs.things.iter().flatten().filter(near).collect();
        let mega = things
            .iter()
            .find(|t| t.name == "Megasphere" || t.name == "Soulsphere")?;
        let mut want = vec![(mega.id, (mega.x, mega.y))];
        let slime = self
            .boss_call_line(obs, p)
            .and_then(|(_, out)| self.explorer.nav.poly_at(out))
            .is_some_and(|i| self.explorer.nav.damage[self.explorer.nav.sector_of_poly(i)] != 0);
        if slime {
            let from = want[0].1;
            if let Some(suit) = things
                .iter()
                .filter(|t| t.name == "RadSuit")
                .min_by(|a, b| dist((a.x, a.y), from).total_cmp(&dist((b.x, b.y), from)))
            {
                want.push((suit.id, (suit.x, suit.y)));
            }
        }
        let mut heights = obs.heights.clone();
        if let Some((_, rest)) = self.boss.rest.filter(|r| r.0 == p.sector) {
            heights[p.sector].floor = rest;
        }
        let route = self.boss_way_at(obs, &heights, &[(want[0].1, None)], "for what mends her")?;
        self.boss_log(
            obs,
            format!(
                "hurt ({:.0} health, {:.0} armour): off the lift for what mends her ({} things)",
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0),
                want.len()
            ),
        );
        self.boss.fetched.extend(want.iter().map(|w| w.0));
        self.boss.route = Some(route);
        Some(want)
    }

    /// Fetching: the way to the first of them, then the next; all had, or a way failed, back to
    /// the brain.
    fn boss_fetch(&mut self, obs: &Observation, mut rest: Vec<(i32, Point)>) -> Turn {
        if self
            .boss
            .route
            .as_ref()
            .is_some_and(|r| !r.done && !r.failed)
        {
            self.boss.stage = Stage::Fetching(rest);
            return Turn::Route;
        }
        let failed = self.boss.route.as_ref().is_some_and(|r| r.failed);
        self.boss.route = None;
        if !rest.is_empty() {
            rest.remove(0);
        }
        if failed || rest.is_empty() {
            self.boss.stage = Stage::Choosing;
            return Turn::Wait(Controls::default());
        }
        self.boss.route = self.boss_way(obs, &[(rest[0].1, None)], "for what mends her");
        if self.boss.route.is_some() {
            self.boss.stage = Stage::Fetching(rest);
            return Turn::Route;
        }
        self.boss.stage = Stage::Choosing;
        Turn::Wait(Controls::default())
    }

    /// To the post, then rockets from it for as long as they go in.
    fn boss_stand(&mut self, obs: &Observation, brain: Brain, p: Post) -> Turn {
        if let Some(route) = self.boss.route.as_ref() {
            if !route.done && !route.failed {
                self.boss.stage = Stage::Standing(p);
                return Turn::Route;
            }
            if route.failed {
                self.boss_log(obs, "the way to the place failed".into());
                self.boss.route = None;
                self.boss.stage = Stage::Choosing;
                return Turn::Wait(Controls::default());
            }
            self.boss.route = None;
        }
        let near = dist(obs.pos(), p.at) < 8.0;
        let fire = near && self.boss_shot(obs, brain, self.boss.floor.0);
        if near && !fire && obs.tic - self.boss.floor.1 > STOPPED {
            // (the floor's moved, or something's shut: no shot from here any more)
            self.boss_log(obs, "no shot from the place any more".into());
            self.boss.stage = Stage::Choosing;
            return Turn::Wait(self.boss_aim(obs, brain, p.at, false));
        }
        let c = self.boss_aim(obs, brain, p.at, fire);
        if c.attack {
            self.boss.rockets_fired += 1;
        }
        self.boss.stage = Stage::Standing(p);
        Turn::Hers(c)
    }
}
