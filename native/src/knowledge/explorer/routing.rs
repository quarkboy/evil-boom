//! Planning objectives, caching failed searches and checking route risk.

use super::{Explorer, GoalKey, Kind, Objective, KEEP_REACH};
use crate::det::HashMap;
use crate::geom::{dist, segments_cross, Point};
use crate::knowledge::RISK_SHARE;
use crate::motor::Observation;
use crate::nav::Heights;
use crate::navigator::{add_height, Plan, Route, States, TriggerLine, View, MAX_STEP};
use crate::specials::Key;
use crate::walkthrough::{action_for, steps_from_legs, Action, Step};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;

/// a backstop only: the expansions decide (a clock limit made a run depend on the machine)
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

    /// The health going to obj by these legs and doing it would cost her: the way there on floors
    /// that hurt; ending on one, the way back off it too, and (but for picking something up) the
    /// time there doing it. (MAP08's pit under the door, 20 a second, a door pressed from it the
    /// last thing to do, dropped into at 54 health and died there, stuck, every time; E3M5, a wall
    /// to look at in a room whose floor hurt, counted the way in only: in and off and in again,
    /// each a tenth of her health or less, 47 s of it to death.) As she knows the floors: one
    /// changed to one that hurts since (E3M1's way over the blood gone down into it) at the worst
    /// floors' if the map has none for it.
    fn expected_damage(
        &self,
        legs: &[crate::navigator::Leg],
        obj: &Objective,
        states: &States,
        obs: &Observation,
    ) -> f64 {
        let way = self.route_damage(legs, states, obs);
        // (the worst only for one the map doesn't say, and none of its look felt: 5 a hit counted
        // as 20, E1M9's lip over the nukage at 68 health, every way on refused as costing too
        // much, stood there)
        let end = self
            .nav
            .poly_at(obj.goal)
            .map(|p| self.nav.sector_of_poly(p))
            .filter(|&s| self.knowledge.damaging(s))
            .map_or(0, |s| self.knowledge.floor_hit(s, WORST_FLOOR));
        // (through her armor, which takes its share of a floor's hits as of any: E2M4 HNTR, 80
        // health and blue armor, a secret door pressed from the lava counted 70, 35 it'd cost;
        // refused, nothing left to do)
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

    /// (mortal) Ways into a crusher she knows across a line that sets it off, stopped where she
    /// last saw it too low to get out from under once it starts (crush::trap): what getting
    /// through costs, on a path, is the follower setting it off from the edge and waiting a
    /// stroke for it to come up again; so much farther at her pace on a route. A way round
    /// shorter than that is better, none: through it so. (E2M4's, stopped 9 over her head, the
    /// way west across its own start line, walked in: set off, pinned, crushed.)
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
                    // (where she last saw it, and it's not known to be on the move)
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
        // (no way there last time, with nothing a plan goes by changed since: none now. What
        // she's offered comes from a rougher reckoning than the planner's, and a goal it can't
        // reach costs a whole search: E4M9's ledges and MAP15's secret exit ring, seconds each,
        // were planned for again every think)
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
        // (a search that uses up its budget: the next ones with half as much, down to a floor,
        // till a while goes by with none that does; the user's. MAP15's few goals behind the
        // block, thousands of steps each for none, every time she'd seen a little more)
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
        // (what she knows of the level as the view has it, but where she's walked only as far as
        // it's somewhere she knows no other way: the view takes the two together, and walking
        // over floor she'd seen is no news to a plan)
        let view = self.view_key(false);
        (
            view.sectors,
            self.knowledge.known_polys().len(),
            view.learned,
            view.teleports,
            view.spent_doors,
            view.spent_teleports,
            view.exits_open,
            view.blocked,
            view.difficult,
        )
            .hash(&mut h);
        self.banned_jumps().hash(&mut h);
        obs.keys.unwrap_or_default().0.hash(&mut h);
        self.spent_once().hash(&mut h);
        // (the heights as she has them: a sector's only where it's somewhere none of the triggers
        // she knows of puts it, nor where it starts; a lift or door at an end a switch she knows
        // gives is no news to a search that can press it. MAP15's goals behind the block,
        // searched again each time a lift went by, ten and twenty times; the user's rule)
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
        // (the once-only lines she's used this life are spent: E3M4's door)
        let fired = self.spent_once();
        let budget = (self.plan_expansions >> self.budget_cuts.min(MAX_BUDGET_CUTS))
            .max(MIN_EXPANSIONS.min(self.plan_expansions));
        // (from the floor she's on: on a lip, not the drop her middle's over)
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
        // no way to the nearest spot to press it from: the next nearest (MAP08's exit door)
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
        // (a plan sets its switches and lines off in the shortest order to walk them in, and
        // has what each does stay done. What a timed one does doesn't: a lift down a few
        // seconds, a door open a few. One that'll have lapsed before the plan gets to what
        // needs it is no plan: planned again without the jump that came too late; with no
        // jump in it to leave out, it's no route. MAP09's pass: a jump off a lift through a
        // door a switch opens for four seconds, the lift's ride after the switch; and the
        // lift's line crossed, then the switch, then back to the lift, long up again; ten
        // minutes of both, the stairs to the same door never taken)
        if let Some(want) = std::env::var("EVIL_DEBUG_JUMPS_INTO")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
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
        if route.is_none() && std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
            let sec = |p: Point| nav.poly_at(p).map(|q| (q, nav.sector_of_poly(q)));
            let direct = view.find_path(start, obj.goal, &states, None);
            self.log.push(format!("  no plan for '{}': from ({:.0}, {:.0}) {:?} to ({:.0}, {:.0}) {:?}; a path by itself: {:?}; believed heights there {:?}", obj.label, start.0, start.1, sec(start), obj.goal.0, obj.goal.1, sec(obj.goal), direct.map(|d| d.steps), sec(obj.goal).map(|(_, s)| states[s].clone())));
        }
        let Some(route) = route else {
            // (only the planner's none: not a way she won't take, which the risky twin may)
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
            // (towards it; the follower takes it on at once, from the nearest place out of its
            // chain's blasts)
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
        if self.knowledge.mortal && !self.on_hurting_floor(obs) {
            // (mortal: no way that costs more health than she has to spare, risky or not; noted,
            // for healing or a suit to go for. Not while down on a floor that hurts: staying
            // costs more than any way off it, the user's)
            // A tenth of her health or less is no risk, short or not (the user's rule for any
            // risk): nothing to wait for. (MAP12's caco closet, the fight that opened it leaving
            // her 13 under a suit, every way back over the nukage, half a point, refused for good)
            // Nothing else at all to do (`harm_ok`): the margin waits for healing or a suit
            // there's no way to, for ever (health doesn't come back by itself); only a way that'd
            // take all she has is refused: a line to set off (A nukage pit's island, health worn
            // to 37 by the wading for walls, the lifts' switches a wade of 15 off, refused at 22
            // against 25: stood there the hour), or a look into somewhere she hasn't seen, the
            // other way a pit opens (as `dead_end` has it; a dry island in a nukage pit at 28
            // health, the way on a room past a door the island's edge opens, a wade of 17 off,
            // refused against 25: stood there the hour); any other way, by the most it can take,
            // one hit of its hardest floor over the time's share. (E3M7 UV: at 18 after a fight,
            // the blue key's alcove ringed by the floor its pickup lowered into blood, every way
            // out 7 of it, the medikits 20: stood there the hour. HMP: at 41, a wade of 40 taken
            // with nothing over: dead in it)
            let cost = self.expected_damage(&route.legs, obj, &states, obs);
            let health = obs.health.unwrap_or(100.0);
            let way_on = obj.trigger.is_some() || obj.kind == Kind::Frontier;
            // (down to trying walls, a wall's weighed as with nothing else to do: there isn't.
            // E2M4: a ledge whose way off is a secret door at the back of a lava alcove; at 69
            // its wall, her best suspect, was refused, 22 left against 25, and walls with less
            // to go on over cheaper lava tried instead, down to 41, when it was refused for good)
            let walls = obj.kind == Kind::Wall && self.suspecting != super::Suspecting::No;
            let margin = match (self.harm_ok || walls, way_on) {
                (true, true) => 0.0,
                (true, false) => self.hardest_floor_hit(&route.legs, obj, &states),
                (false, _) => HEALTH_MARGIN,
            };
            if cost > RISK_SHARE * health && health - cost < margin {
                self.short_of_health = Some(obs.tic);
                if std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
                    self.log.push(format!("  no route for '{}': refusal 1: cost {cost:.0} (the way {:.0}) against health {health:.0}, armor {:?}, margin {margin:.0}, nothing else to do {}", obj.label, self.route_damage(&route.legs, &states, obs), obs.armor, self.harm_ok));
                }
                return None;
            }
        }
        if obj.kind != Kind::Exit && !obj.risky && !self.knowledge.protected(obs.tic) {
            // (nukage and the like only for what it costs a tenth of her health or less (the
            // user's: risk is the damage against what she has; E2M2's blue key, 385 of nukage,
            // a hit or two of 5, waited for as risky over 100 s while far-off things went
            // first), and never dropping into it; but where she's been down before and got out
            // again is all right: MAP24's)
            let k = &self.knowledge;
            let here = nav.poly_at(obs.pos()).map(|p| nav.sector_of_poly(p));
            // (not one that's gone down into what hurts since: crossing it before, it was a
            // walkway still, E3M1's, dropped into after to look at a wall, no way out)
            let escaped: BTreeSet<usize> = k
                .visited
                .iter()
                .map(|&p| nav.sector_of_poly(p))
                .filter(|s| Some(*s) != here && !k.changed_floor.get(s).is_some_and(|&h| h != 0))
                .collect();
            for leg in &route.legs {
                for n in &leg.path.steps {
                    if !n.contains("one-way drop") {
                        continue;
                    }
                    if let Some(s) = n.rsplit(' ').next().and_then(|x| x.parse::<usize>().ok()) {
                        if k.damaging(s) && !escaped.contains(&s) {
                            if std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
                                self.log
                                    .push(format!("  no route for '{}': refusal 2", obj.label));
                            }
                            return None;
                        }
                    }
                }
            }
            let damage = self.expected_damage(&route.legs, obj, &states, obs);
            // (not while shut in on floors that hurt, for a line to set off: staying costs all
            // she has, so a tenth of it is no measure, and a switch, a door, a line is the way a
            // pit opens; E4M2, a lava pit at 32 health, the door's line out refused at 23 against
            // 3.2, walls tried in the lava till dead. Not for walls or looking: lifted for every
            // way, she fought and wandered in the lava, worse)
            let shut_in = self.on_hurting_floor(obs)
                && self
                    .reach
                    .iter()
                    .all(|&p| self.knowledge.damaging(nav.sector_of_poly(p)));
            // (a wall tried over any floor that hurts is the risky twin's: a dip is no hit or a
            // whole one, and the walls with least to go on, a tenth of her health each as
            // reckoned, came before the one she most suspected, which stood on the worst of it.
            // E2M4 in an episode: 58 at the ledge, walls across the lava tried first, 41 by the
            // time the secret door's own was weighed, 12 short; an hour there, 8 of 12 episodes)
            let wall_over_harm = obj.kind == Kind::Wall && damage > 0.0;
            if (damage > RISK_SHARE * obs.health.unwrap_or(100.0) || wall_over_harm)
                && !(shut_in && obj.trigger.is_some())
            {
                if std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
                    self.log
                        .push(format!("  no route for '{}': refusal 3", obj.label));
                }
                return None;
            }
        }
        // (nor, with something safe still to do, by a run-jump over where she couldn't get back
        // from if it fell short: a jump can be knocked short, and a way round may be a room
        // off. MAP06: a shotgun across a lava trench, the bridge to it through a room not
        // looked into yet; the jump taken, a lost soul in its run, down in the trench, banks
        // of 72 all round, for good)
        if obj.kind != Kind::Exit
            && !obj.risky
            && route.legs.iter().any(|leg| {
                leg.path
                    .jumps
                    .iter()
                    .any(|j| self.falls_for_good(j, &view, &route.states))
            })
        {
            if std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
                self.log
                    .push(format!("  no route for '{}': refusal 4", obj.label));
            }
            return None;
        }
        if obj.kind != Kind::Exit && self.dead_end(&route, obj, obs, &view) {
            if std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
                self.log
                    .push(format!("  no route for '{}': refusal 5", obj.label));
            }
            return None;
        }
        if obj.kind != Kind::Exit && !obj.risky && self.shuts_in(&route, obj, obs, &view) {
            if std::env::var_os("EVIL_DEBUG_NO_ROUTE").is_some() {
                self.log
                    .push(format!("  no route for '{}': refusal 6", obj.label));
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

    /// Does the route count on something timed that will have lapsed by the time it's needed?
    /// The run-jumps (their keys) of the leg that needs it, to plan again without; none: a
    /// way on foot that comes too late, no route. Else None.
    /// A timed effect: what a line does that undoes itself (`hold`: a lift down and back, a
    /// door open and shut). Needed: by a later leg that walks into its sector and has no way
    /// with it lapsed, or by a jump that requires its height. When: the walk there at a run,
    /// a jump's lining up, and the wait for a floor the jump takes off from that another of
    /// the timed effects has away.
    fn lapses(&self, route: &Route, view: &View, states: &States) -> Option<BTreeSet<String>> {
        /// (units a tic, a run with its turns; tics to line a jump up; tics at a switch)
        const PACE: f64 = 10.0;
        const LINE_UP: f64 = 50.0;
        const PRESS: f64 = 18.0;
        let nav = &self.nav;
        let legs = &route.legs;
        // (how far along a leg's way till a poly that fits, by the polys' middles)
        let along = |leg: &crate::navigator::Leg, fits: &dyn Fn(usize) -> bool| -> Option<f64> {
            let mut far = 0.0;
            let mut last: Option<Point> = None;
            for &p in &leg.path.polys {
                let c = nav.poly_center(p);
                far += last.map_or(0.0, |l| dist(l, c));
                last = Some(c);
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
                .map(|w| dist(nav.poly_center(w[0]), nav.poly_center(w[1])))
                .sum::<f64>()
        };
        let mut running = states.clone();
        for i in 0..legs.len().saturating_sub(1) {
            let Some(id) = legs[i].trigger else { continue };
            let t = view.trigger(id).clone();
            let after = nav.apply_trigger(&running, &t);
            // (sector, the height it's at a while, tics from the trigger till it's left it)
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
                    // (a jump that needs it as it is meanwhile: its takeoff, lined up, and the
                    // wait for a floor it leaves from that's away meanwhile too)
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
                    // (a walk into it a leg or more on: is there a way without it?)
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
    fn falls_for_good(&self, jump: &crate::navigator::Jump, view: &View, states: &States) -> bool {
        const EVERY: f64 = 16.0;
        let nav = &self.nav;
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
        under.values().any(|&poly| {
            let back = view.reachable(poly, states, None);
            !back.contains(&from) && !back.contains(&jump.dest_poly)
        })
    }

    /// Does the route drop her somewhere she couldn't get back from, with nothing there to try
    /// (no part of the map she hasn't seen beyond it, no switch or door she hasn't tried)? A pit,
    /// risky or not: E3M3, everything else far off, the nearest a drop of 48 into a pool she'd
    /// seen all round, walls on every side, its floor hurting after all, died in it pressing walls.
    pub(super) fn dead_end(
        &mut self,
        route: &Route,
        obj: &Objective,
        obs: &Observation,
        view: &View,
    ) -> bool {
        // (a teleporter is a way on of itself: MAP09's, refused as a pit it's the way out of)
        if obj.kind == Kind::Teleporter
            || !route
                .legs
                .iter()
                .any(|leg| leg.path.steps.iter().any(|n| n.contains("one-way drop")))
        {
            return false;
        }
        let nav = Rc::clone(&self.nav);
        let (Some(end), Some(here)) = (nav.poly_at(obj.goal), nav.poly_at(obs.pos())) else {
            return false;
        };
        // (with the key it's for in hand: on a level she knows, the door it opens is the way on
        // from where it lies, and nothing unseen is left there to say so. MAP02 known
        // beforehand: its blue key refused 23 times, the level's hour stood out)
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
        // (nor is it a dead end with a way out of the level from it)
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
        // (nor with a teleporter there: a way out)
        if nav
            .teleports
            .iter()
            .any(|tp| !tp.src_polys.is_disjoint(&after))
        {
            return false;
        }
        // (nor with walls round it she hasn't seen yet: any of them could be the switch out.
        // E4M6's lava pit, seen from above all over, the lifts out of it worked by their sides
        // down in it, never looked at: refused as a pit with nothing in it)
        let sectors: BTreeSet<usize> = after.iter().map(|&p| nav.sector_of_poly(p)).collect();
        if k.unseen_walls().iter().any(|(s, _)| sectors.contains(s)) {
            return false;
        }
        // (nor when it's a wall well marked she's going down to try: that's what there is to
        // try there. MAP21, on an islet in the nukage sea, everything else across it: the lifts
        // out of the sea plain walls from below, marked as the one she'd found was, each
        // refused as a drop with nothing to try; "nothing left to do" for the last 860 s)
        if obj.kind == Kind::Wall && obj.suspicion >= super::objectives::NEAR_SUSPECT {
            return false;
        }
        if std::env::var_os("EVIL_DEBUG_DEAD_END").is_some() {
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
    /// be without them? (MAP08: the line by the red skull drops the floor she'd come back over.)
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
        // (doors gone through on the way: open on the way back too, as far as this goes)
        let opened = |st: &States| -> States {
            let mut out = st.clone();
            for (&d, o) in &doors {
                for &h in &o.heights {
                    add_height(&mut out[d], h);
                }
            }
            out
        };
        // (with what she knows how to work from there: MAP08's switch by the red skull)
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
        // (shut in with something left to try is the way on, maybe: unseen parts, switches not
        // pressed; MAP08's sinking floors to the exit door)
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
