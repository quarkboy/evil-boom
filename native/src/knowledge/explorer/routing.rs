// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Planning objectives, caching failed searches and checking route risk.

use super::{Explorer, GoalKey, Kind, Objective, KEEP_REACH};
use crate::det::HashMap;
use crate::domain::Observation;
use crate::geom::{dist, segments_cross, Point};
use crate::knowledge::RISK_SHARE;
use crate::nav::Heights;
use crate::navigator::{
    add_height, PathNote, Plan, Route, States, TriggerLine, View, MAX_STEP, PLAYER_RADIUS,
};
use crate::specials::Key;
use crate::walkthrough::{action_for, steps_from_legs, Action, Step};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;

/// a backstop only: the expansions decide (a clock limit would make a run depend on the machine)
const PLAN_SECONDS: f64 = 600.0;

/// Minimum health a route must leave her with in mortal runs.
pub(super) const HEALTH_MARGIN: f64 = 25.0;
/// a route ending on a floor that hurts counts this many hurts there (doing it, getting off)
const END_STAY: f64 = 2.0;
/// what a floor that hurts does at the worst (20 a hit), for one she knows hurts and the map
/// doesn't say how much
const WORST_FLOOR: i32 = 20;

/// her pace on a route, units a tic (a little under running, for the turns)
const ROUTE_PACE: f64 = 12.0;

/// a hurting floor does its damage once in this many tics
const HURT_PERIOD: f64 = 32.0;

/// a goal found no way to is taken to have none for this long at most, nothing changed
const NO_ROUTE_TICS: i64 = 20 * 35;

/// a plan search's budget, halved each time one uses it up, goes no lower than this...
const MIN_EXPANSIONS: usize = 200;

/// ...and is whole again after this long with none that did
const MAXED_RESET: i64 = 20 * 35;

/// Map units between midpoint samples of a route's exposure to damaging floors.
const DAMAGE_SAMPLE_STEP: f64 = 16.0;
/// Limit the shift used to reduce a repeatedly exhausted plan budget.
const MAX_BUDGET_CUTS: u32 = 16;
/// Distinct activator polygons tried before a trigger objective is considered unreachable.
const ALTERNATE_ACTIVATORS: usize = 3;

/// Why a way the planner found to an objective isn't taken (`Explorer::refusal`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Refusal {
    /// Over floors that hurt it would cost more health than she has to spare: `cost` of her
    /// `health`, leaving less than `margin`.
    ShortOfHealth { cost: f64, health: f64, margin: f64 },
    /// It drops her onto a floor that hurts (`sector`) that she has never got out of.
    DropOntoHarm { sector: usize },
    /// It costs more than a tenth of her health over floors that hurt (`damage`) while this
    /// isn't the risky pass; or it's a wall tried over such a floor, which is the risky
    /// pass's.
    HarmWhileSafeLeft { damage: f64 },
    /// It has a run-jump over somewhere she couldn't get back from if it fell short.
    JumpOverNoWayBack,
    /// It drops her somewhere with no way back and nothing there to try.
    DeadEnd,
    /// The lines it walks over would shut her in at its end.
    ShutsHerIn,
    /// It has a run-jump that comes down among barrels standing on its landing (the one at
    /// `at`): cleared first, or not jumped.
    BarrelsOnLanding { at: Point },
}

impl Explorer {
    /// The stretches of a path down on floors she takes to hurt, in order: (sector, length).
    fn wades(&self, waypoints: &[Option<Point>], heights: &States) -> Vec<(usize, f64)> {
        let (mesh, k) = (&self.nav.mesh, &self.knowledge);
        let mut out: Vec<(usize, f64)> = Vec::new();
        let mut prev: Option<Point> = None;
        let mut on: Option<f64> = None;
        for &p in waypoints {
            let (Some(p), Some(pr)) = (p, prev) else {
                prev = p;
                continue;
            };
            let n = ((dist(pr, p) / DAMAGE_SAMPLE_STEP).floor() as i64).max(1);
            let step = dist(pr, p) / n as f64;
            for i in 0..n {
                let f = (i as f64 + 0.5) / n as f64;
                let q = (pr.0 + (p.0 - pr.0) * f, pr.1 + (p.1 - pr.1) * f);
                let Some(poly) = mesh.poly_at(q) else {
                    continue;
                };
                let s = mesh.poly(poly).sector;
                let floor = heights[s]
                    .iter()
                    .map(|h| h.floor)
                    .fold(f64::NEG_INFINITY, f64::max);
                if !k.damaging(s) {
                    on = Some(floor);
                } else if on.is_none_or(|o| floor >= o - MAX_STEP) {
                    out.push((s, step)); // (down in it, not passing over it)
                }
            }
            prev = Some(p);
        }
        out
    }

    /// (mortal runs) The health a route's legs would cost her: her time down on floors that
    /// hurt, at what each does (one that only looks it: at the worst); a suit on, nothing for
    /// the time it has left, but the worst floors' now and then.
    fn route_damage(
        &self,
        legs: &[crate::navigator::Leg],
        states: &States,
        obs: &Observation,
    ) -> f64 {
        let suit_left = self
            .knowledge
            .suit_until
            .map_or(0.0, |u| (u - obs.tic) as f64);
        let mut tics = 0.0;
        let mut total = 0.0;
        for leg in legs {
            for (s, len) in self.wades(&leg.path.waypoints, states) {
                let t = len / ROUTE_PACE;
                let per = f64::from(self.knowledge.floor_hit(s, WORST_FLOOR));
                let rate = if tics + t < suit_left {
                    if per >= 20.0 {
                        per * 5.0 / 256.0
                    } else {
                        0.0
                    }
                } else {
                    per
                };
                total += t / HURT_PERIOD * rate;
                tics += t;
            }
        }
        total
    }

    /// The hardest hit of any floor that hurts on the way to obj or where it ends: what the
    /// game's clock can add to the damage `expected_damage` spreads over the time (a floor hits
    /// once every 32 tics of the level's clock, so a wade can take one more hit than its time's
    /// share, by when it starts).
    fn hardest_floor_hit(
        &self,
        legs: &[crate::navigator::Leg],
        obj: &Objective,
        states: &States,
    ) -> f64 {
        let end = self
            .nav
            .poly_at(obj.goal)
            .map(|p| self.nav.sector_of_poly(p))
            .filter(|&s| self.knowledge.damaging(s));
        let way = legs
            .iter()
            .flat_map(|leg| self.wades(&leg.path.waypoints, states))
            .map(|(s, _)| s);
        way.chain(end)
            .map(|s| self.knowledge.floor_hit(s, WORST_FLOOR))
            .max()
            .map_or(0.0, f64::from)
    }

    /// The health that going to obj by these legs and doing it would cost her: the way there on
    /// floors that hurt; and, ending on one, the way back off it too and (except for picking
    /// something up) the time there doing it. As she knows the floors: one that has changed to
    /// something that hurts since counts at the worst floors' rate if the map has none for it.
    fn expected_damage(
        &self,
        legs: &[crate::navigator::Leg],
        obj: &Objective,
        states: &States,
        obs: &Observation,
    ) -> f64 {
        let way = self.route_damage(legs, states, obs);
        // The worst rate only for a floor the map doesn't give one for and none of whose look she
        // has felt: counting a mild floor as the worst refuses every way on over it.
        let end = self
            .nav
            .poly_at(obj.goal)
            .map(|p| self.nav.sector_of_poly(p))
            .filter(|&s| self.knowledge.damaging(s))
            .map_or(0, |s| self.knowledge.floor_hit(s, WORST_FLOOR));
        // Through her armor, which takes its share of a floor's hits as of any other.
        if end == 0
            || obj.kind == Kind::Exit
            || self.knowledge.suit_until.is_some_and(|u| u >= obs.tic)
        {
            return obs.through_armor(way);
        }
        let stay = if matches!(obj.kind, Kind::Item | Kind::Key) {
            0.0
        } else {
            f64::from(end) * END_STAY
        };
        obs.through_armor(2.0 * way + stay)
    }

    /// (mortal) Ways into a crusher she knows across a line that sets it off, where it stopped,
    /// as she last saw it, too low to get out from under once it starts (`crush::trap`). Getting
    /// through means the follower setting it off from the edge and waiting a stroke for it to
    /// come up again, so on a route that way counts as so much farther at her pace. A way round
    /// shorter than that is better; with none, it's through.
    fn crusher_entry_costs(&mut self, view: &View, states: &States, obs: &Observation) {
        let mut costs: HashMap<usize, (usize, f64)> = HashMap::default();
        if self.knowledge.mortal {
            let nav = Rc::clone(&self.nav);
            let rest: Vec<Heights> = states
                .iter()
                .enumerate()
                .map(|(s, hs)| hs.first().copied().unwrap_or(nav.movers.initial[s]))
                .collect();
            let afford = RISK_SHARE * obs.health.unwrap_or(100.0);
            for (t, sectors) in self.knowledge.known_triggers() {
                let (Some(kind), Some(li)) =
                    (crate::specials::crusher_kind(t.info.special), t.linedef())
                else {
                    continue;
                };
                if !t.info.trigger.walked() {
                    continue;
                }
                for s in sectors {
                    // Where she last saw it, and it's not known to be on the move.
                    let [h] = states[s].as_slice() else { continue };
                    let out = *self
                        .crusher_out
                        .entry(s)
                        .or_insert_with(|| crate::crush::out_tics(&nav, &rest, s));
                    if crate::crush::trap(*h, kind, out, afford, obs) {
                        let top = nav.movers.initial[s].ceiling.max(h.ceiling);
                        let stroke = 2.0 * (top - h.floor - crate::crush::BOTTOM) / kind.speed;
                        costs.insert(li, (s, stroke * ROUTE_PACE));
                    }
                }
            }
        }
        view.set_entry_costs(costs);
    }

    /// Steps to get to an objective and do it, through what's known (using switches she has
    /// learned on the way if need be).
    pub fn route_to(&mut self, obj: &Objective, obs: &Observation) -> Option<Vec<Step>> {
        // No way there last time, with nothing a plan goes by changed since: none now. What she's
        // offered comes from a rougher reckoning than the planner's, and a goal the planner can't
        // reach costs a whole search each time.
        let goal = GoalKey {
            x_bits: obj.goal.0.to_bits(),
            y_bits: obj.goal.1.to_bits(),
            trigger_line: obj.trigger.as_ref().map(|t| t.line),
            try_other_activators: matches!(obj.kind, Kind::Switch | Kind::Door | Kind::Exit),
        };
        let sig = self.plan_signature(obs);
        if self
            .no_route
            .get(&goal)
            .is_some_and(|&(s, tic)| s == sig && obs.tic - tic < NO_ROUTE_TICS)
        {
            self.stats.known_none += 1;
            return None;
        }
        let t0 = Instant::now();
        // A search that uses up its budget: the next ones get half as much, down to a floor, till a
        // while goes by with none that does.
        if obs.tic - self.maxed_tic > MAXED_RESET {
            self.budget_cuts = 0;
        }
        crate::stats::take_maxed();
        let route = self.plan_route(obj, obs, (goal, sig));
        if crate::stats::take_maxed() {
            self.budget_cuts += 1;
            self.maxed_tic = obs.tic;
        }
        let took = t0.elapsed().as_secs_f64();
        let s = &mut self.stats;
        if route.is_some() {
            s.routed += 1;
            s.routed_secs += took;
        } else {
            s.failed += 1;
            s.failed_secs += took;
        }
        route
    }

    /// Forget the goals there was no route to (`drive budget`: searched for again).
    pub fn forget_no_routes(&mut self) {
        self.no_route.clear();
    }

    /// All a plan goes by, hashed: what she knows (the view), the jumps she won't try, her keys,
    /// the lines spent this life, the heights as she has them, and where she can get to from
    /// here (for where she's starting from).
    fn plan_signature(&mut self, obs: &Observation) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // What she knows of the level as the view has it, but where she's walked only as far as it's
        // somewhere she knows no other way: the view takes the two together, and walking over floor
        // she had seen is no news to a plan.
        let view = self.view_key(false);
        (
            view.sectors,
            self.knowledge.known_polys().len(),
            view.learned,
            view.teleports,
            view.spent_doors,
            view.spent_teleports,
            view.exits_offered,
            view.blocked,
            view.missed_jumps_in,
        )
            .hash(&mut h);
        self.banned_jumps().hash(&mut h);
        obs.keys.unwrap_or_default().0.hash(&mut h);
        self.spent_once().hash(&mut h);
        // The heights as she has them: a sector's only where it's somewhere that none of the
        // triggers she knows puts it, nor where it starts. A lift or door at an end that a switch she
        // knows gives is no news to a search that can press it, and must not start a new search.
        let states = self.states();
        let nav = &self.nav;
        let mut known: Vec<BTreeSet<crate::nav::HeightsKey>> = nav
            .movers
            .initial
            .iter()
            .map(|h| BTreeSet::from([h.key()]))
            .collect();
        for (t, sectors) in self.knowledge.known_triggers() {
            for (sec, hs) in nav.effects_now(&t, &states) {
                if sectors.contains(&sec) {
                    known[sec].extend(hs.iter().map(|x| x.key()));
                }
            }
        }
        for (sec, hs) in states.iter().enumerate() {
            if hs.iter().any(|x| !known[sec].contains(&x.key())) {
                sec.hash(&mut h);
                for x in hs {
                    x.key().hash(&mut h);
                }
            }
        }
        self.reach.hash(&mut h);
        h.finish()
    }

    fn plan_route(
        &mut self,
        obj: &Objective,
        obs: &Observation,
        failed: (GoalKey, u64),
    ) -> Option<Vec<Step>> {
        let view = self.view();
        let nav = Rc::clone(&self.nav);
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        self.crusher_entry_costs(&view, &states, obs);
        let k = &self.knowledge;
        let mut key_items: Vec<(Key, Vec<(usize, Point)>)> = Vec::new();
        for th in k.known_things() {
            if let Some(kk) = Key::from_thing(th.kind) {
                let spots = view.pickup_spots(th.x, th.y, &states);
                match key_items.iter_mut().find(|(x, _)| *x == kk) {
                    Some((_, v)) => v.extend(spots),
                    None => key_items.push((kk, spots)),
                }
            }
        }
        // The once-only lines she has used this life are spent.
        let fired = self.spent_once();
        let budget = (self.plan_expansions >> self.budget_cuts.min(MAX_BUDGET_CUTS))
            .max(MIN_EXPANSIONS.min(self.plan_expansions));
        // From the floor she's on: on a lip, not the drop her middle is over.
        let start = self.standing_point(obs);
        let plan = |goal: Point| {
            view.plan(
                start,
                &[goal],
                &Plan {
                    keys,
                    states: Some(&states),
                    key_items: &key_items,
                    max_expansions: budget,
                    max_seconds: PLAN_SECONDS,
                    goal_triggers: &[],
                    fired: &fired,
                },
            )
        };
        let mut route = plan(obj.goal);
        // No way to the nearest spot to press it from: the next nearest.
        let mut goal = obj.goal;
        let mut tried: BTreeSet<usize> = BTreeSet::new();
        while route.is_none()
            && matches!(obj.kind, Kind::Switch | Kind::Door | Kind::Exit)
            && tried.len() < ALTERNATE_ACTIVATORS
        {
            let (Some(t), Some(at)) = (obj.trigger.as_ref(), nav.poly_at(goal)) else {
                break;
            };
            tried.insert(at);
            let rest: BTreeSet<usize> = t
                .activators
                .intersection(&self.reach)
                .copied()
                .filter(|p| !tried.contains(p))
                .collect();
            if rest.is_empty() {
                break;
            }
            goal = nav.activation_point(t, obs.pos(), &rest);
            route = plan(goal);
        }
        // A plan sets its switches and lines off in the shortest order to walk them in, and has
        // what each does stay done. What a timed one does doesn't stay: a lift is down a few
        // seconds, a door open a few. A plan that will have lapsed before it gets to what needs it
        // is no plan: it's planned again without the jump that came too late, and with no jump in
        // it to leave out, it's no route.
        if let Some(want) = crate::debug::on().jumps_into {
            let banned = self.banned_jumps();
            for (ji, j) in nav
                .jumps
                .iter()
                .enumerate()
                .filter(|(_, j)| nav.sector_of_poly(j.dest_poly) == want)
            {
                let from: BTreeSet<usize> =
                    j.src_polys.iter().map(|&p| nav.sector_of_poly(p)).collect();
                let needs: Vec<String> = j
                    .requires
                    .iter()
                    .map(|(rs, rh)| {
                        format!(
                            "{rs} at ({}, {}): believed {}",
                            rh.floor,
                            rh.ceiling,
                            states[*rs].iter().any(|h| h.key() == rh.key())
                        )
                    })
                    .collect();
                eprintln!("JUMP {ji} from {from:?} off ({:.0}, {:.0}): banned {}, strafe {}, soft {}, sure {:.2}, through {}; needs {needs:?}", j.takeoff.0, j.takeoff.1, banned.contains(&ji), j.strafe, j.soft, j.sure, j.through);
            }
        }
        const REPLANS: usize = 6;
        for _ in 0..REPLANS {
            let Some(late) = route.as_ref().and_then(|r| self.lapses(r, &view, &states)) else {
                break;
            };
            if late.is_empty() {
                route = None;
                break;
            }
            let learned = self
                .knowledge
                .learned
                .values()
                .map(BTreeSet::len)
                .sum::<usize>();
            if self.lapsing_jumps.1 != learned {
                self.lapsing_jumps = (BTreeSet::new(), learned);
            }
            self.log.push(format!(
                "tic {}: '{}': a way with a jump too late for what's timed before it: another",
                obs.tic, obj.label
            ));
            self.lapsing_jumps.0.extend(late);
            let without = self.view();
            route = without.plan(
                start,
                &[goal],
                &Plan {
                    keys,
                    states: Some(&states),
                    key_items: &key_items,
                    max_expansions: budget,
                    max_seconds: PLAN_SECONDS,
                    goal_triggers: &[],
                    fired: &fired,
                },
            );
        }
        // A way by a run-jump over somewhere there's no getting back from, with something safe
        // still to do, isn't taken (`refusal`): another way without that jump, if there is one,
        // before it's given up as no way. (A shortcut across the corner of a pit the stairs go
        // round: the stairs.) None: the way she had, for `refusal` to turn down as before, and
        // the risky twin to take.
        /// a way round a jump over a fall for good is taken if no longer than the jump's way by
        /// this factor, and this much (map units, as the planner costs them)
        const WAY_ROUND: f64 = 1.5;
        const WAY_ROUND_MORE: f64 = 256.0;
        if obj.kind != Kind::Exit && !obj.risky {
            let first = route.clone();
            for _ in 0..REPLANS {
                let Some(r) = route.as_ref() else { break };
                let over: Vec<crate::navigator::Jump> = r
                    .legs
                    .iter()
                    .flat_map(|leg| leg.path.jumps.iter())
                    .filter(|j| self.falls_for_good(j, &view, &r.states, keys))
                    .cloned()
                    .collect();
                if over.is_empty() {
                    break;
                }
                // ...nor by its twins, between the same two floors: over the same fall, most
                // likely, whether or not each is told to be (one that takes off from the very
                // lip of a pit isn't, the pit taken for where it starts).
                let twins: Vec<String> = nav
                    .jumps
                    .iter()
                    .filter(|j| {
                        over.iter().any(|o| {
                            nav.sector_of_poly(o.dest_poly) == nav.sector_of_poly(j.dest_poly)
                                && o.src_polys.iter().any(|&a| {
                                    j.src_polys
                                        .iter()
                                        .any(|&b| nav.sector_of_poly(a) == nav.sector_of_poly(b))
                                })
                        })
                    })
                    .map(|j| j.key())
                    .collect();
                self.unsafe_jumps.extend(twins);
                let without = self.view();
                route = without.plan(
                    start,
                    &[goal],
                    &Plan {
                        keys,
                        states: Some(&states),
                        key_items: &key_items,
                        max_expansions: budget,
                        max_seconds: PLAN_SECONDS,
                        goal_triggers: &[],
                        fired: &fired,
                    },
                );
            }
            if !self.unsafe_jumps.is_empty() {
                // (Not a way round much longer than the jump's, though: that's the fights and the
                // floors on a long walk for a risk the risky twin takes once, later.)
                let length =
                    |r: &Route| -> f64 { r.legs.iter().map(|leg| leg.path.cost).sum::<f64>() };
                let near = match (&route, &first) {
                    (Some(r), Some(f)) => length(r) <= length(f) * WAY_ROUND + WAY_ROUND_MORE,
                    _ => false,
                };
                if near {
                    self.log.push(format!(
                        "tic {}: '{}': not by a jump over where there's no getting back from: another way",
                        obs.tic, obj.label
                    ));
                } else {
                    route = first;
                }
                self.unsafe_jumps.clear();
            }
        }
        if route.is_none() && crate::debug::on().no_route {
            let sec = |p: Point| nav.poly_at(p).map(|q| (q, nav.sector_of_poly(q)));
            let direct = view.find_path(start, obj.goal, &states, None);
            self.log.push(format!("  no plan for '{}': from ({:.0}, {:.0}) {:?} to ({:.0}, {:.0}) {:?}; a path by itself: {:?}; believed heights there {:?}", obj.label, start.0, start.1, sec(start), obj.goal.0, obj.goal.1, sec(obj.goal), direct.map(|d| d.notes.iter().map(ToString::to_string).collect::<Vec<_>>()), sec(obj.goal).map(|(_, s)| states[s].clone())));
        }
        let Some(route) = route else {
            // Only the planner's "none": not a way she won't take, which the risky twin may.
            self.no_route.insert(failed.0, (failed.1, obs.tic));
            return None;
        };
        let step = |action: Action, trigger: Option<TriggerLine>, key: Option<Key>| Step {
            path: None,
            action,
            trigger,
            key,
            label: obj.label.clone(),
            expects: Vec::new(),
            barrel: None,
        };
        let last = match obj.kind {
            Kind::Frontier
            | Kind::Recheck
            | Kind::Visit
            | Kind::Noise
            | Kind::Room
            | Kind::Item
            | Kind::Hunt
            | Kind::Wall
            | Kind::Secret => step(Action::Walk, None, None),
            Kind::Key => step(Action::Pickup, None, obj.key),
            // Towards it; the follower takes it on at once, from the nearest place out of its chain's
            // blasts.
            Kind::Barrel => Step {
                barrel: obj.look_at,
                ..step(Action::Walk, None, None)
            },
            Kind::Teleporter => step(Action::Cross, obj.trigger.clone(), None),
            _ => {
                let t = obj.trigger.clone()?;
                let action = match action_for(&t) {
                    Action::Use if obj.kind == Kind::Exit => Action::Exit,
                    a => a,
                };
                step(action, Some(t), None)
            }
        };
        if let Some(refusal) = self.refusal(&route, obj, obs, &view, &states) {
            if crate::debug::on().no_route {
                self.log
                    .push(format!("  no route for '{}': {refusal:?}", obj.label));
                // (and what each leg of it costs her: a way of several legs is refused whole)
                for (i, leg) in route.legs.iter().enumerate() {
                    let cost = self.route_damage(std::slice::from_ref(leg), &states, obs);
                    self.log.push(format!(
                        "    leg {i}: trigger {:?}, {:.0} long, {cost:.0} of her health",
                        leg.trigger.map(|id| view.trigger(id).line),
                        leg.path.cost
                    ));
                }
            }
            // A way of several legs, too much for her health as a whole: the switch its legs
            // get her to with enough left is kept, to be gone for by itself if there's nothing
            // else (`way_on_by_legs`). The furthest such: the first is often only the lift
            // out of where she is, which comes back up behind her every time. And none she's
            // gone for this way in this life already.
            if matches!(refusal, Refusal::ShortOfHealth { .. }) && route.legs.len() > 1 {
                let health = obs.health.unwrap_or(100.0);
                let life = self.knowledge.lives;
                let far = (1..route.legs.len())
                    .rev()
                    .filter_map(|k| {
                        let t = view.trigger(route.legs[k - 1].trigger?);
                        let cost =
                            obs.through_armor(self.route_damage(&route.legs[..k], &states, obs));
                        (health - cost >= HEALTH_MARGIN
                            && !self.legs_taken.contains(&(life, t.line)))
                        .then(|| t.clone())
                    })
                    .next();
                if let Some(t) = far {
                    if !self.short_legs.iter().any(|x| x.line == t.line) {
                        self.short_legs.push(t);
                    }
                }
            }
            return None;
        }
        Some(steps_from_legs(
            &view,
            &route.legs,
            Some(last),
            Some(&states),
        ))
    }

    /// Why a way the planner found isn't to be taken, if it isn't: the checks in the order they
    /// are made, the first that fails.
    fn refusal(
        &mut self,
        route: &Route,
        obj: &Objective,
        obs: &Observation,
        view: &View,
        states: &States,
    ) -> Option<Refusal> {
        let nav = Rc::clone(&self.nav);
        if self.knowledge.mortal && !self.on_hurting_floor(obs) {
            // Mortal: no way that costs more health than she has to spare, risky or not; noted, so
            // that healing or a suit is gone for. Not while she's down on a floor that hurts: staying
            // costs more than any way off it.
            // A tenth of her health or less is no risk, short or not: nothing to wait for.
            // With nothing else at all to do (`harm_ok`), waiting for healing or a suit there's no way
            // to would be waiting for ever, so the margin is lifted: for a line to set off, or a look
            // into somewhere she hasn't seen (the two ways a pit opens), only a way that would take all
            // she has is refused; for any other way, the margin is the most the game's clock can add,
            // one hit of its hardest floor.
            let cost = self.expected_damage(&route.legs, obj, states, obs);
            let health = obs.health.unwrap_or(100.0);
            // (a key she hasn't got is a way on as much as a line to set off: E4M1, the blue
            // skull 7 of her health off with 5 left and nothing else on the level to do)
            let way_on = obj.trigger.is_some() || matches!(obj.kind, Kind::Frontier | Kind::Key);
            // Down to trying walls, a wall is weighed as with nothing else to do, because there is
            // nothing else. Otherwise her best suspect can be refused by a few points while weaker
            // walls over cheaper floors are tried and wear her down past it.
            let walls = obj.kind == Kind::Wall && self.suspecting != super::Suspecting::No;
            let lifted = self.harm_allowed && (way_on || !self.harm_ways_only);
            let margin = match (lifted || walls, way_on) {
                (true, true) => 0.0,
                (true, false) => self.hardest_floor_hit(&route.legs, obj, states),
                (false, _) => HEALTH_MARGIN,
            };
            // (and not at all with nothing whatever left to do: `way_on_whatever`)
            if cost > RISK_SHARE * health
                && health - cost < margin
                && !(self.harm_whatever && (way_on || self.harm_anything))
            {
                self.short_of_health = Some(obs.tic);
                self.health_refused.insert(obj.ident());
                return Some(Refusal::ShortOfHealth {
                    cost,
                    health,
                    margin,
                });
            }
        }
        // Mortal, and risky or not: no drop onto a floor that hurts with no way off it she
        // knows. That's no risk taken but the end, short of a secret found in ten seconds; a
        // jump over the same floor, which the risky pass does take, is a way on if it's made.
        // Left for when there's nothing else at all (`with_harm_allowed`).
        if self.knowledge.mortal
            && obj.kind != Kind::Exit
            && obj.risky
            && !self.harm_allowed
            && !self.on_hurting_floor(obs)
            && !self.knowledge.protected(obs.tic)
        {
            for leg in &route.legs {
                for n in &leg.path.notes {
                    let PathNote::Drop { sector: s, .. } = *n else {
                        continue;
                    };
                    // (nor for a teleporter she has seen there and is going for: the way off is
                    // what she's dropping in for. E2M4: the pad in the blood before the exit,
                    // turned down at full health while sixty went on walls over other floors)
                    if !self.knowledge.damaging(s) || obj.kind == Kind::Teleporter {
                        continue;
                    }
                    let landing = leg
                        .path
                        .polys
                        .iter()
                        .copied()
                        .find(|&p| nav.sector_of_poly(p) == s);
                    let way_off = landing.is_some_and(|p| {
                        view.reachable(p, &route.states, None)
                            .iter()
                            .any(|&q| !self.knowledge.damaging(nav.sector_of_poly(q)))
                    });
                    if !way_off {
                        return Some(Refusal::DropOntoHarm { sector: s });
                    }
                }
            }
            // (not for a switch or a door to try there: what it does isn't known, and it's
            // the one thing on such a perch that could be the way off it)
            if obj.trigger.is_none() && obj.kind != Kind::Teleporter {
                if let Some(s) = self.marooned(route, obj, obs, view) {
                    return Some(Refusal::DropOntoHarm { sector: s });
                }
            }
        }
        if obj.kind != Kind::Exit && !obj.risky && !self.knowledge.protected(obs.tic) {
            // Nukage and the like only for what costs a tenth of her health or less, and never
            // dropping into it; but where she's been down before and got out again is all right.
            let k = &self.knowledge;
            let here = nav.poly_at(obs.pos()).map(|p| nav.sector_of_poly(p));
            // Not one that has gone down into what hurts since: crossing it before, it was a walkway
            // still.
            let escaped: BTreeSet<usize> = k
                .visited
                .iter()
                .map(|&p| nav.sector_of_poly(p))
                .filter(|s| Some(*s) != here && !k.changed_floor.get(s).is_some_and(|&h| h != 0))
                .collect();
            for leg in &route.legs {
                for n in &leg.path.notes {
                    let PathNote::Drop { sector: s, .. } = *n else {
                        continue;
                    };
                    // (a teleporter to try there is the way off it: as above)
                    if k.damaging(s) && !escaped.contains(&s) && obj.kind != Kind::Teleporter {
                        return Some(Refusal::DropOntoHarm { sector: s });
                    }
                }
            }
            let damage = self.expected_damage(&route.legs, obj, states, obs);
            // Not while shut in on floors that hurt, for a line to set off: staying costs all she has,
            // so a tenth of it is no measure, and a switch, a door or a line is the way a pit opens.
            // Not for walls or looking: lifted for every way, she wanders in it, which is worse.
            let shut_in = self.on_hurting_floor(obs)
                && self
                    .reach
                    .iter()
                    .all(|&p| self.knowledge.damaging(nav.sector_of_poly(p)));
            // A wall tried over any floor that hurts is left to the risky twin. A dip is no hit or a
            // whole one, never the fraction it's reckoned at, so walls that look cheap aren't; in the
            // risky pass her best suspect goes first.
            let wall_over_harm = obj.kind == Kind::Wall && damage > 0.0;
            if (damage > RISK_SHARE * obs.health.unwrap_or(100.0) || wall_over_harm)
                && !(shut_in && obj.trigger.is_some())
            {
                return Some(Refusal::HarmWhileSafeLeft { damage });
            }
        }
        // Nor, with something safe still to do, by a run-jump over where she couldn't get back from
        // if it fell short: a jump can be knocked short, and a way round may be a room off.
        if obj.kind != Kind::Exit
            && !obj.risky
            && route.legs.iter().any(|leg| {
                leg.path
                    .jumps
                    .iter()
                    .any(|j| self.falls_for_good(j, view, &route.states, route.keys))
            })
        {
            return Some(Refusal::JumpOverNoWayBack);
        }
        if let Some(at) = self.barrels_on_landing(route, obs) {
            return Some(Refusal::BarrelsOnLanding { at });
        }
        if obj.kind != Kind::Exit && self.dead_end(route, obj, obs, view) {
            return Some(Refusal::DeadEnd);
        }
        if obj.kind != Kind::Exit && !obj.risky && self.shuts_in(route, obj, obs, view) {
            return Some(Refusal::ShutsHerIn);
        }
        None
    }

    /// A barrel she has seen, still standing, that a run-jump of the route would bring her
    /// down against: on the landing's floor, in the way of her box from where the flight
    /// comes over that floor to where the jump ends. Among barrels on a landing she's wedged:
    /// none to be shot from so near, and every way on a straight line into one. (Doom II
    /// MAP25: the pillar of five on the way to the megasphere; every run that lived to
    /// jump onto it stalled there.) Whatever the route's for, risky or not: cleared first.
    fn barrels_on_landing(&self, route: &Route, obs: &Observation) -> Option<Point> {
        /// a barrel's thing type, its half-width, and room to spare
        const BARREL: u16 = 2035;
        const BARREL_RADIUS: f64 = 10.0;
        const SPARE: f64 = 6.0;
        const EVERY: f64 = 8.0;
        let nav = &self.nav;
        let standing = obs.barrels.as_ref()?;
        let jumps: Vec<&crate::navigator::Jump> = route
            .legs
            .iter()
            .flat_map(|leg| leg.path.jumps.iter())
            .collect();
        if jumps.is_empty() || standing.is_empty() {
            return None;
        }
        // (those she has seen the floor under: as the map has them, where one stands still)
        let seen: Vec<Point> = self
            .knowledge
            .known_things()
            .iter()
            .filter(|t| t.kind == BARREL)
            .map(|t| (t.x, t.y))
            .filter(|&p| standing.iter().any(|&b| dist(b, p) < 2.0 * BARREL_RADIUS))
            .collect();
        for j in jumps {
            let landing = nav.sector_of_poly(j.dest_poly);
            let length = dist(j.takeoff, j.dest).max(1e-6);
            let mut at = 0.0;
            while at <= length {
                let p = (
                    j.takeoff.0 + (j.dest.0 - j.takeoff.0) * at / length,
                    j.takeoff.1 + (j.dest.1 - j.takeoff.1) * at / length,
                );
                if nav.poly_at(p).map(|q| nav.sector_of_poly(q)) == Some(landing) {
                    if let Some(&b) = seen
                        .iter()
                        .find(|&&b| dist(b, p) < PLAYER_RADIUS + BARREL_RADIUS + SPARE)
                    {
                        return Some(b);
                    }
                }
                at += EVERY;
            }
        }
        None
    }

    /// A route that ends, by a jump or a drop, on ground she can't come back from, off which
    /// the only way is onto a floor that hurts, with no other ground that doesn't to be got to
    /// from there: marooned, the same end as the drop into the pit itself, a jump later. The
    /// sector she'd be marooned on. (Plutonia MAP09: the low span of the exit bridge, jumped
    /// down onto from the pit's rim before the switch that raises it.)
    fn marooned(
        &self,
        route: &Route,
        obj: &Objective,
        obs: &Observation,
        view: &View,
    ) -> Option<usize> {
        let nav = &self.nav;
        let down = route.legs.iter().any(|leg| {
            leg.path
                .notes
                .iter()
                .any(|n| matches!(n, PathNote::Drop { .. } | PathNote::RunJump { .. }))
        });
        let end = route.legs.last()?.path.polys.last().copied()?;
        let here = nav.poly_at(self.standing_point(obs))?;
        if !down || self.knowledge.damaging(nav.sector_of_poly(end)) {
            return None;
        }
        // (everywhere she could get to from there, with the doors she can open and the lines
        // she knows, and the key she'd have if it's a key she's going for: as `dead_end` has
        // it. A skull key on its island in the lava opens the door that's the way on)
        let keys = obj.key.map_or(route.keys, |kk| route.keys.with(kk));
        let doors = view.local_doors(keys);
        let mut states = route.states.clone();
        for (&d, o) in &doors {
            for &h in &o.heights {
                add_height(&mut states[d], h);
            }
        }
        let actions: Vec<&TriggerLine> = view
            .triggers
            .iter()
            .filter(|t| !t.effects.is_empty())
            .collect();
        let after = view.saturate(end, &states, keys, &[], &actions).0;
        if after.contains(&here)
            || !after
                .iter()
                .any(|&q| self.knowledge.damaging(nav.sector_of_poly(q)))
        {
            return None;
        }
        // (nor with somewhere there she hasn't seen into, or a teleporter: either may be the
        // way on. E4M4's chaingun on its island in the lava, the level going on beyond it;
        // a teleporter pad out in the slime)
        let known = self.knowledge.known_polys();
        let unseen_beyond = after.iter().any(|&q| {
            nav.mesh
                .poly(q)
                .portals
                .iter()
                .any(|&pi| !known.contains(&nav.mesh.portals[pi].other(q)))
        });
        if unseen_beyond
            || nav
                .teleports
                .iter()
                .any(|tp| !tp.src_polys.is_disjoint(&after))
        {
            return None;
        }
        // the ground that doesn't hurt she can walk to from where she ends without crossing
        // any that does
        let safe = |q: usize| after.contains(&q) && !self.knowledge.damaging(nav.sector_of_poly(q));
        let mut island: BTreeSet<usize> = BTreeSet::from([end]);
        let mut queue = vec![end];
        while let Some(p) = queue.pop() {
            for &pi in &nav.mesh.poly(p).portals {
                let q = nav.mesh.portals[pi].other(p);
                if safe(q) && island.insert(q) {
                    queue.push(q);
                }
            }
        }
        after
            .iter()
            .all(|&q| island.contains(&q) || self.knowledge.damaging(nav.sector_of_poly(q)))
            .then(|| nav.sector_of_poly(end))
    }

    /// Does the route count on something timed that will have lapsed by the time it's needed?
    /// The run-jumps (their keys) of the leg that needs it, to plan again without; none: a
    /// way on foot that comes too late, no route. Else None.
    /// A timed effect: what a line does that undoes itself (`hold`: a lift down and back, a
    /// door open and shut). Needed: by a later leg that walks into its sector and has no way
    /// with it lapsed, or by a jump that requires its height. When: the walk there at a run,
    /// a jump's lining up, and the wait for a floor the jump takes off from that another of
    /// the timed effects has away.
    fn lapses(&self, route: &Route, view: &View, states: &States) -> Option<BTreeSet<String>> {
        /// units a tic, a run with its turns; tics to line a jump up; tics at a switch
        const PACE: f64 = 10.0;
        const LINE_UP: f64 = 50.0;
        const PRESS: f64 = 18.0;
        let nav = &self.nav;
        let legs = &route.legs;
        // The walk from one piece of the mesh to the next on a way, by their middles: none
        // where they don't join (a teleporter's two ends can be the level apart, and she's
        // across in a tic: counted as a walk, a way by two teleporters never got back to
        // its timed door in time. TNT MAP32's exit).
        let hop = |a: usize, b: usize| {
            let joined = nav
                .mesh
                .poly(a)
                .portals
                .iter()
                .any(|&pi| nav.mesh.portals[pi].other(a) == b);
            if joined {
                dist(nav.poly_center(a), nav.poly_center(b))
            } else {
                0.0
            }
        };
        // How far along a leg's way till a poly that fits.
        let along = |leg: &crate::navigator::Leg, fits: &dyn Fn(usize) -> bool| -> Option<f64> {
            let mut far = 0.0;
            let mut last: Option<usize> = None;
            for &p in &leg.path.polys {
                far += last.map_or(0.0, |l| hop(l, p));
                last = Some(p);
                if fits(p) {
                    return Some(far);
                }
            }
            None
        };
        let length = |leg: &crate::navigator::Leg| {
            leg.path
                .polys
                .windows(2)
                .map(|w| hop(w[0], w[1]))
                .sum::<f64>()
        };
        let mut running = states.clone();
        for i in 0..legs.len().saturating_sub(1) {
            let Some(id) = legs[i].trigger else { continue };
            let t = view.trigger(id).clone();
            let after = nav.apply_trigger(&running, &t);
            // (sector, the height it's at a while, tics from the trigger till it has left it)
            let timed: Vec<(usize, Heights, f64)> = nav
                .effects_now(&t, &running)
                .iter()
                .filter(|(s, hs)| {
                    !hs.is_empty() && !running[*s].iter().any(|h| h.key() == hs[0].key())
                })
                .filter_map(|(s, hs)| {
                    let tm = nav
                        .movers
                        .timing(t.info.special, &t.info.description, *s, hs[0]);
                    tm.hold
                        .filter(|_| t.line >= 0)
                        .map(|hold| (*s, hs[0], tm.travel + hold))
                })
                .collect();
            if timed.is_empty() {
                running = after;
                continue;
            }
            let mut lapsed = after.clone();
            for (s, _, _) in &timed {
                lapsed[*s].clone_from(&running[*s]);
            }
            let mut elapsed = 0.0;
            for (j, leg) in legs.iter().enumerate().skip(i + 1) {
                let jumps = || {
                    leg.path
                        .jumps
                        .iter()
                        .map(crate::navigator::Jump::key)
                        .collect::<BTreeSet<String>>()
                };
                for (s, h, window) in &timed {
                    // A jump that needs it as it is meanwhile: its takeoff, lined up, and the wait for a floor
                    // it leaves from that's away meanwhile too.
                    for jump in leg.path.jumps.iter().filter(|jump| {
                        jump.requires
                            .iter()
                            .any(|(rs, rh)| rs == s && rh.key() == h.key())
                    }) {
                        let to = along(leg, &|p| jump.src_polys.contains(&p)).unwrap_or(0.0);
                        let ride = jump
                            .requires
                            .iter()
                            .filter_map(|(rs, rh)| {
                                timed
                                    .iter()
                                    .find(|(os, oh, _)| os == rs && oh.key() != rh.key())
                                    .map(|o| o.2)
                            })
                            .fold(0.0, f64::max);
                        if elapsed + to / PACE + LINE_UP + ride > *window {
                            return Some(jumps());
                        }
                    }
                    // A walk into it a leg or more on: is there a way without it?
                    if j > i + 1 {
                        if let Some(to) = along(leg, &|p| nav.sector_of_poly(p) == *s) {
                            let (Some(&a), Some(&b)) =
                                (leg.path.polys.first(), leg.path.polys.last())
                            else {
                                continue;
                            };
                            if elapsed + to / PACE > *window
                                && view
                                    .find_path(
                                        nav.poly_center(a),
                                        nav.poly_center(b),
                                        &lapsed,
                                        None,
                                    )
                                    .is_none()
                            {
                                return Some(jumps());
                            }
                        }
                    }
                }
                elapsed += length(leg) / PACE + leg.path.jumps.len() as f64 * LINE_UP + PRESS;
                if let Some(next) = leg.trigger {
                    lapsed = nav.apply_trigger(&lapsed, view.trigger(next));
                }
            }
            running = after;
        }
        None
    }

    /// A run-jump that, fallen short of, leaves her somewhere she knows no way back from to
    /// either end of it: what's under its flight, lower than she could step back up from.
    ///
    /// No way back by walking, that is, and none she'd plan either: by a lift's switch, a
    /// door she has the key to, the jumps that brought her. (MAP14: the gap before the exit's
    /// door, 88 of water under it; the water's the moat she has been round all level, a lift
    /// out of it, but the way back up to the window is through the blue door and two jumps,
    /// so the door past the gap was "no route" till everything safe in the level had been
    /// gone over again, five minutes and more, and an hour's cap hit.)
    fn falls_for_good(
        &mut self,
        jump: &crate::navigator::Jump,
        view: &View,
        states: &States,
        keys: crate::specials::Keys,
    ) -> bool {
        const EVERY: f64 = 16.0;
        let nav = Rc::clone(&self.nav);
        // (A takeoff on the very lip of a pit can be in the pit's own poly, the pit then taken
        // for an end of the jump and not looked under. Left so: told right, the jumps she makes
        // from such lips are refused, and the score is the worse for it; see the decision log.)
        let Some(from) = nav.poly_at(jump.takeoff) else {
            return false;
        };
        let ends = [nav.sector_of_poly(from), nav.sector_of_poly(jump.dest_poly)];
        let length = dist(jump.takeoff, jump.dest);
        let mut under: BTreeMap<usize, usize> = BTreeMap::new();
        let mut at = EVERY;
        while at < length {
            let p = (
                jump.takeoff.0 + (jump.dest.0 - jump.takeoff.0) * at / length,
                jump.takeoff.1 + (jump.dest.1 - jump.takeoff.1) * at / length,
            );
            if let Some(poly) = nav.poly_at(p) {
                let sector = nav.sector_of_poly(poly);
                if !ends.contains(&sector) {
                    under.entry(sector).or_insert(poly);
                }
            }
            at += EVERY;
        }
        let stuck: Vec<usize> = under
            .into_values()
            .filter(|&poly| {
                let back = view.reachable(poly, states, None);
                !back.contains(&from) && !back.contains(&jump.dest_poly)
            })
            .collect();
        stuck
            .into_iter()
            .any(|poly| !self.back_up(jump, poly, view, states, keys))
    }

    /// Is there a way she'd plan from poly `under`, below a jump's flight, back to its
    /// takeoff? Worked out once for what she knows.
    fn back_up(
        &mut self,
        jump: &crate::navigator::Jump,
        under: usize,
        view: &View,
        states: &States,
        keys: crate::specials::Keys,
    ) -> bool {
        let known = self.knowledge.known_summary();
        let id = (jump.key(), under);
        if let Some((_, back)) = self.ways_back.get(&id).filter(|(k, _)| *k == known) {
            return *back;
        }
        let fired = self.spent_once();
        let back = view
            .plan(
                self.nav.mesh.center(under),
                &[jump.takeoff],
                &Plan {
                    keys,
                    states: Some(states),
                    key_items: &[],
                    max_expansions: self.plan_expansions,
                    max_seconds: PLAN_SECONDS,
                    goal_triggers: &[],
                    fired: &fired,
                },
            )
            .is_some();
        self.ways_back.insert(id, (known, back));
        back
    }

    /// Does the route drop her somewhere she couldn't get back from, with nothing there to try
    /// (no part of the map she hasn't seen beyond it, no switch or door she hasn't tried)? A
    /// pit, risky or not.
    pub(super) fn dead_end(
        &mut self,
        route: &Route,
        obj: &Objective,
        obs: &Observation,
        view: &View,
    ) -> bool {
        // A teleporter is a way on of itself.
        if obj.kind == Kind::Teleporter
            || !route.legs.iter().any(|leg| {
                leg.path
                    .notes
                    .iter()
                    .any(|n| matches!(n, PathNote::Drop { .. }))
            })
        {
            return false;
        }
        let nav = Rc::clone(&self.nav);
        let (Some(end), Some(here)) = (nav.poly_at(obj.goal), nav.poly_at(obs.pos())) else {
            return false;
        };
        // With the key it's for in hand: on a level she knows, the door it opens is the way on from
        // where it lies, and nothing unseen is left there to say so.
        let keys = obj.key.map_or(route.keys, |kk| route.keys.with(kk));
        let doors = view.local_doors(keys);
        let mut states = route.states.clone();
        for (&d, o) in &doors {
            for &h in &o.heights {
                add_height(&mut states[d], h);
            }
        }
        let actions: Vec<&TriggerLine> = view
            .triggers
            .iter()
            .filter(|t| !t.effects.is_empty())
            .collect();
        let after = view.saturate(end, &states, keys, &[], &actions).0;
        if after.contains(&here) {
            return false;
        }
        let k = &self.knowledge;
        // Nor is it a dead end with a way out of the level from it.
        if view.triggers.iter().any(|t| {
            t.linedef().is_some_and(|li| k.is_exit(li)) && !t.activators.is_disjoint(&after)
        }) {
            return false;
        }
        let known = k.known_polys();
        if after.iter().any(|&pi| {
            nav.mesh
                .poly(pi)
                .portals
                .iter()
                .any(|&q| !known.contains(&nav.mesh.portals[q].other(pi)))
        }) {
            return false;
        }
        let by_line: BTreeMap<usize, &TriggerLine> = nav
            .triggers
            .iter()
            .filter_map(|t| t.linedef().map(|li| (li, t)))
            .collect();
        if k.interactables().iter().any(|li| {
            by_line
                .get(li)
                .is_some_and(|t| !t.activators.is_disjoint(&after))
        }) {
            return false;
        }
        // Nor with a teleporter there: a way out.
        if nav
            .teleports
            .iter()
            .any(|tp| !tp.src_polys.is_disjoint(&after))
        {
            return false;
        }
        // Nor with walls round it she hasn't seen yet: any of them could be the switch out.
        let sectors: BTreeSet<usize> = after.iter().map(|&p| nav.sector_of_poly(p)).collect();
        if k.unseen_walls().iter().any(|(s, _)| sectors.contains(s)) {
            return false;
        }
        // Nor when it's a well-marked wall she's going down to try: that is what there is to try
        // there.
        if obj.kind == Kind::Wall && obj.suspicion >= super::objectives::NEAR_SUSPECT {
            return false;
        }
        if crate::debug::on().dead_end {
            let inter: Vec<String> = k
                .interactables()
                .iter()
                .map(|li| {
                    format!(
                        "{li}: activators in sectors {:?}",
                        by_line.get(li).map(|t| t
                            .activators
                            .iter()
                            .map(|&p| nav.sector_of_poly(p))
                            .collect::<BTreeSet<usize>>())
                    )
                })
                .collect();
            self.log.push(format!("  dead end from poly {end} (sector {}), she's at poly {here} (sector {}): after it sectors {sectors:?}; to try: {inter:?}", nav.sector_of_poly(end), nav.sector_of_poly(here)));
        }
        if self
            .current
            .as_ref()
            .is_none_or(|c| c.ident() != obj.ident())
        {
            self.log.push(format!("tic {}: '{}': a drop into somewhere with no way back and nothing there to try ({} polys)", obs.tic, obj.label, after.len()));
        }
        true
    }

    /// Would the walk-over lines the route goes over cut her off at its end, where she wouldn't
    /// be without them (a line that drops the floor she'd come back over)?
    pub(super) fn shuts_in(
        &mut self,
        route: &Route,
        obj: &Objective,
        obs: &Observation,
        view: &View,
    ) -> bool {
        let nav = Rc::clone(&self.nav);
        let (Some(end), Some(here)) = (nav.poly_at(obj.goal), nav.poly_at(obs.pos())) else {
            return false;
        };
        if route.legs.is_empty() {
            return false;
        }
        let doors = view.local_doors(route.keys);
        let k = &self.knowledge;
        let spent: BTreeSet<usize> = k
            .fired
            .iter()
            .filter(|f| f.life == k.lives)
            .map(|f| f.line)
            .collect();
        let legs: Vec<(Point, Point)> = route
            .legs
            .iter()
            .flat_map(|leg| {
                leg.path
                    .waypoints
                    .windows(2)
                    .filter_map(|w| Some((w[0]?, w[1]?)))
            })
            .collect();
        let mut final_states = route.states.clone();
        let mut changed = false;
        for t in &view.triggers {
            let Some(li) = t.linedef() else { continue };
            if t.effects.is_empty() || !t.info.trigger.walked() || (t.once() && spent.contains(&li))
            {
                continue;
            }
            let (a, b) = nav.map.line_seg(li);
            if legs.iter().any(|&(p, q)| segments_cross(p, q, a, b)) {
                final_states = nav.apply_trigger(&final_states, t);
                changed = true;
            }
        }
        if !changed {
            return false;
        }
        // Doors gone through on the way: open on the way back too, as far as this goes.
        let opened = |st: &States| -> States {
            let mut out = st.clone();
            for (&d, o) in &doors {
                for &h in &o.heights {
                    add_height(&mut out[d], h);
                }
            }
            out
        };
        // With what she knows how to work from there.
        let actions: Vec<&TriggerLine> = view
            .triggers
            .iter()
            .filter(|t| !t.effects.is_empty())
            .collect();
        let before = view
            .saturate(end, &opened(&route.states), route.keys, &[], &actions)
            .0;
        let after = view
            .saturate(end, &opened(&final_states), route.keys, &[], &actions)
            .0;
        if after.contains(&here) || after.len() as f64 >= KEEP_REACH * before.len() as f64 {
            return false;
        }
        // Shut in with something left to try may be the way on: unseen parts, switches not pressed.
        let known = k.known_polys();
        if after.iter().any(|&pi| {
            nav.mesh
                .poly(pi)
                .portals
                .iter()
                .any(|&q| !known.contains(&nav.mesh.portals[q].other(pi)))
        }) {
            return false;
        }
        let by_line: BTreeMap<usize, &TriggerLine> = nav
            .triggers
            .iter()
            .filter_map(|t| t.linedef().map(|li| (li, t)))
            .collect();
        if k.interactables().iter().any(|li| {
            by_line
                .get(li)
                .is_some_and(|t| !t.activators.is_disjoint(&after))
        }) {
            return false;
        }
        if self
            .current
            .as_ref()
            .is_none_or(|c| c.ident() != obj.ident())
        {
            self.log.push(format!(
                "tic {}: '{}': what the way there sets off would shut her in ({} polys to get to after, {} before)",
                obs.tic,
                obj.label,
                after.len(),
                before.len()
            ));
        }
        true
    }
}
