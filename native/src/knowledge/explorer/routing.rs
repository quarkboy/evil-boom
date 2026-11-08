//! Planning objectives, caching failed searches and checking route risk.

use super::{Explorer, GoalKey, Kind, Objective, KEEP_REACH};
use crate::geom::{dist, segments_cross, Point};
use crate::motor::Observation;
use crate::navigator::{add_height, Plan, Route, States, TriggerLine, View, MAX_STEP};
use crate::specials::Key;
use crate::walkthrough::{action_for, steps_from_legs, Action, Step};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;

/// a backstop only: the expansions decide (a clock limit made a run depend on the machine)
const PLAN_SECONDS: f64 = 600.0;

/// Minimum health a route must leave her with in mortal runs.
const HEALTH_MARGIN: f64 = 25.0;
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

/// how far over a hurting floor a route may go, normally...
const WADE_LIMIT: f64 = 256.0;

/// ...and not at all below this health
const WADE_HEALTH: f64 = 50.0;

/// Map units between midpoint samples of a route's exposure to damaging floors.
const DAMAGE_SAMPLE_STEP: f64 = 16.0;
/// Limit the shift used to reduce a repeatedly exhausted plan budget.
const MAX_BUDGET_CUTS: u32 = 16;
/// Distinct activator polygons tried before a trigger objective is considered unreachable.
const ALTERNATE_ACTIVATORS: usize = 3;

impl Explorer {
    /// How far a path goes on floors she takes to hurt: not over them (a hurting pit well below
    /// a walkway she runs along, or jumps across: MAP24).
    pub(super) fn wading(&self, waypoints: &[Option<Point>], heights: &States) -> f64 {
        self.wades(waypoints, heights).iter().map(|(_, d)| d).sum()
    }

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
                let Some(poly) = mesh.poly_at(q) else { continue };
                let s = mesh.poly(poly).sector;
                let floor = heights[s].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
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
    fn route_damage(&self, legs: &[crate::navigator::Leg], states: &States, obs: &Observation) -> f64 {
        let suit_left = self.knowledge.suit_until.map_or(0.0, |u| (u - obs.tic) as f64);
        let mut tics = 0.0;
        let mut total = 0.0;
        for leg in legs {
            for (s, len) in self.wades(&leg.path.waypoints, states) {
                let t = len / ROUTE_PACE;
                let felt = self.nav.damage[s];
                let per = f64::from(if felt > 0 { felt } else { WORST_FLOOR });
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
        if self.no_route.get(&goal).is_some_and(|&(s, tic)| s == sig && obs.tic - tic < NO_ROUTE_TICS) {
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
        let mut known: Vec<BTreeSet<crate::nav::HeightsKey>> = nav.movers.initial.iter().map(|h| BTreeSet::from([h.key()])).collect();
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

    fn plan_route(&mut self, obj: &Objective, obs: &Observation, failed: (GoalKey, u64)) -> Option<Vec<Step>> {
        let view = self.view();
        let nav = Rc::clone(&self.nav);
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
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
        let budget = (self.plan_expansions >> self.budget_cuts.min(MAX_BUDGET_CUTS)).max(MIN_EXPANSIONS.min(self.plan_expansions));
        let plan = |goal: Point| {
            view.plan(
                obs.pos(),
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
        while route.is_none() && matches!(obj.kind, Kind::Switch | Kind::Door | Kind::Exit) && tried.len() < ALTERNATE_ACTIVATORS {
            let (Some(t), Some(at)) = (obj.trigger.as_ref(), nav.poly_at(goal)) else { break };
            tried.insert(at);
            let rest: BTreeSet<usize> = t.activators.intersection(&self.reach).copied().filter(|p| !tried.contains(p)).collect();
            if rest.is_empty() {
                break;
            }
            goal = nav.activation_point(t, obs.pos(), &rest);
            route = plan(goal);
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
            Kind::Frontier | Kind::Recheck | Kind::Visit | Kind::Noise | Kind::Room | Kind::Item | Kind::Hunt | Kind::Wall | Kind::Secret => {
                step(Action::Walk, None, None)
            }
            Kind::Key => step(Action::Pickup, None, obj.key),
            // (towards it; the follower takes it on at once, from the nearest place out of its
            // chain's blasts)
            Kind::Barrel => Step { barrel: obj.look_at, ..step(Action::Walk, None, None) },
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
            let mut cost = self.route_damage(&route.legs, &states, obs);
            // (ending on a floor that hurts: the time there doing it, and the way back off it too,
            // not only the way in; MAP08's pit under the door, 20 a second, a door pressed from it
            // the last thing to do, dropped into at 54 health and died there, stuck, every time)
            // (as she knows it: a floor changed to one that hurts since, E3M1's way over the blood
            // gone down into it, at the worst floors' if the map has none for it)
            let end = self.nav.poly_at(obj.goal).map(|p| self.nav.sector_of_poly(p)).filter(|&s| self.knowledge.damaging(s)).map_or(0, |s| self.nav.damage[s].max(WORST_FLOOR));
            if end > 0 && obj.kind != Kind::Exit && self.knowledge.suit_until.is_none_or(|u| u < obs.tic) {
                cost = 2.0 * cost + f64::from(end) * END_STAY;
            }
            if cost > 0.0 && obs.health.unwrap_or(100.0) - cost < HEALTH_MARGIN {
                self.short_of_health = Some(obs.tic);
                return None;
            }
        }
        if obj.kind != Kind::Exit && !obj.risky && !self.knowledge.protected(obs.tic) {
            // (nukage and the like only for a few steps, and never dropping into it; but where
            // she's been down before and got out again is all right: MAP24's)
            let k = &self.knowledge;
            let here = nav.poly_at(obs.pos()).map(|p| nav.sector_of_poly(p));
            // (not one that's gone down into what hurts since: crossing it before, it was a
            // walkway still, E3M1's, dropped into after to look at a wall, no way out)
            let escaped: BTreeSet<usize> =
                k.visited.iter().map(|&p| nav.sector_of_poly(p)).filter(|s| Some(*s) != here && k.changed_floor.get(s) != Some(&true)).collect();
            let mut wade = 0.0;
            for leg in &route.legs {
                for n in &leg.path.steps {
                    if !n.contains("one-way drop") {
                        continue;
                    }
                    if let Some(s) = n.rsplit(' ').next().and_then(|x| x.parse::<usize>().ok()) {
                        if k.damaging(s) && !escaped.contains(&s) {
                            return None;
                        }
                    }
                }
                wade += self.wading(&leg.path.waypoints, &states);
            }
            if wade > WADE_LIMIT || (wade > 0.0 && obs.health.is_some_and(|h| h < WADE_HEALTH)) {
                return None;
            }
        }
        if obj.kind != Kind::Exit && self.dead_end(&route, obj, obs, &view) {
            return None;
        }
        if obj.kind != Kind::Exit && !obj.risky && self.shuts_in(&route, obj, obs, &view) {
            return None;
        }
        Some(steps_from_legs(&view, &route.legs, Some(last), Some(&states)))
    }

    /// Does the route drop her somewhere she couldn't get back from, with nothing there to try
    /// (no part of the map she hasn't seen beyond it, no switch or door she hasn't tried)? A pit,
    /// risky or not: E3M3, everything else far off, the nearest a drop of 48 into a pool she'd
    /// seen all round, walls on every side, its floor hurting after all, died in it pressing walls.
    pub(super) fn dead_end(&mut self, route: &Route, obj: &Objective, obs: &Observation, view: &View) -> bool {
        // (a teleporter is a way on of itself: MAP09's, refused as a pit it's the way out of)
        if obj.kind == Kind::Teleporter || !route.legs.iter().any(|leg| leg.path.steps.iter().any(|n| n.contains("one-way drop"))) {
            return false;
        }
        let nav = Rc::clone(&self.nav);
        let (Some(end), Some(here)) = (nav.poly_at(obj.goal), nav.poly_at(obs.pos())) else { return false };
        let doors = view.local_doors(route.keys);
        let mut states = route.states.clone();
        for (&d, o) in &doors {
            for &h in &o.heights {
                add_height(&mut states[d], h);
            }
        }
        let actions: Vec<&TriggerLine> = view.triggers.iter().filter(|t| !t.effects.is_empty()).collect();
        let after = view.saturate(end, &states, route.keys, &[], &actions).0;
        if after.contains(&here) {
            return false;
        }
        let k = &self.knowledge;
        let known = k.known_polys();
        if after.iter().any(|&pi| nav.mesh.poly(pi).portals.iter().any(|&q| !known.contains(&nav.mesh.portals[q].other(pi)))) {
            return false;
        }
        let by_line: BTreeMap<usize, &TriggerLine> = nav.triggers.iter().filter_map(|t| t.linedef().map(|li| (li, t))).collect();
        if k.interactables().iter().any(|li| by_line.get(li).is_some_and(|t| !t.activators.is_disjoint(&after))) {
            return false;
        }
        // (nor with a teleporter there: a way out)
        if nav.teleports.iter().any(|tp| !tp.src_polys.is_disjoint(&after)) {
            return false;
        }
        if self.current.as_ref().is_none_or(|c| c.ident() != obj.ident()) {
            self.log.push(format!("tic {}: '{}': a drop into somewhere with no way back and nothing there to try ({} polys)", obs.tic, obj.label, after.len()));
        }
        true
    }

    /// Would the walk-over lines the route goes over cut her off at its end, where she wouldn't
    /// be without them? (MAP08: the line by the red skull drops the floor she'd come back over.)
    pub(super) fn shuts_in(&mut self, route: &Route, obj: &Objective, obs: &Observation, view: &View) -> bool {
        let nav = Rc::clone(&self.nav);
        let (Some(end), Some(here)) = (nav.poly_at(obj.goal), nav.poly_at(obs.pos())) else { return false };
        if route.legs.is_empty() {
            return false;
        }
        let doors = view.local_doors(route.keys);
        let k = &self.knowledge;
        let spent: BTreeSet<usize> = k.fired.iter().filter(|f| f.life == k.lives).map(|f| f.line).collect();
        let legs: Vec<(Point, Point)> = route.legs.iter().flat_map(|leg| leg.path.waypoints.windows(2).filter_map(|w| Some((w[0]?, w[1]?)))).collect();
        let mut final_states = route.states.clone();
        let mut changed = false;
        for t in &view.triggers {
            let Some(li) = t.linedef() else { continue };
            if t.effects.is_empty() || !t.info.trigger.walked() || (t.once() && spent.contains(&li)) {
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
        let actions: Vec<&TriggerLine> = view.triggers.iter().filter(|t| !t.effects.is_empty()).collect();
        let before = view.saturate(end, &opened(&route.states), route.keys, &[], &actions).0;
        let after = view.saturate(end, &opened(&final_states), route.keys, &[], &actions).0;
        if after.contains(&here) || after.len() as f64 >= KEEP_REACH * before.len() as f64 {
            return false;
        }
        // (shut in with something left to try is the way on, maybe: unseen parts, switches not
        // pressed; MAP08's sinking floors to the exit door)
        let known = k.known_polys();
        if after.iter().any(|&pi| nav.mesh.poly(pi).portals.iter().any(|&q| !known.contains(&nav.mesh.portals[q].other(pi)))) {
            return false;
        }
        let by_line: BTreeMap<usize, &TriggerLine> = nav.triggers.iter().filter_map(|t| t.linedef().map(|li| (li, t))).collect();
        if k.interactables().iter().any(|li| by_line.get(li).is_some_and(|t| !t.activators.is_disjoint(&after))) {
            return false;
        }
        if self.current.as_ref().is_none_or(|c| c.ident() != obj.ident()) {
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
