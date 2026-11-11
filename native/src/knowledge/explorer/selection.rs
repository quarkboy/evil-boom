//! Suspecting walls and choosing the next objective when ordinary exploration stalls.

use super::{
    route_length, EndKind, Explorer, Hooks, Kind, Objective, Spots, Suspecting, GIVE_UP_TICS,
    JUMP_MISSES, STUCK_TIMES, WHILE_OPEN,
};
use crate::geom::{dist, Point};
use crate::knowledge::model::{Behind, Note, Wall, SUSPICION_MIN, THOROUGHNESS};
use crate::motor::follower::RouteFollower;
use crate::motor::Observation;
use crate::navigator::TriggerLine;
use crate::walkthrough::Step;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// objectives routed and compared by the way there, each pick
const COMPARED: usize = 4;
/// pickups nearer than a first-priority objective that are routed to see if they come first
const ON_THE_WAY: usize = 4;

/// a "close, wait 30s, open" door's wait, and a little (tics)
const REOPEN_TICS: i64 = 32 * 35;

/// ...or by the time it takes to walk its route at this (units a tic)
const ROUTE_SPEED: f64 = 8.0;

/// ...and this
const ROUTE_SLACK: i64 = 15 * 35;

/// suspected walls offered at a time (the most suspicious)
const SUSPECTS: usize = 6;

/// ...and in a panic, the nearest walls of all
const PANIC_WALLS: usize = 12;

/// ...and offered again after this
const RETRY_TICS: i64 = 30 * 35;

impl Explorer {
    /// Walls she suspects, to press use on: tried once nothing else is left (as a player humps
    /// walls for secrets).
    pub(super) fn walls(
        &mut self,
        obs: &Observation,
        reach: &BTreeSet<usize>,
        panic: bool,
    ) -> Vec<Objective> {
        let mut found = self.suspected(obs, reach, panic);
        if panic {
            // All of them, those with somewhere she can't get to behind them first, nearest first. But
            // before those, the ones she can get to without a floor that hurts (when unsuited): her
            // reach runs on over such floors, but once the health it takes is gone the routes there
            // are refused, and a round of far walls would leave the walls of the dry floor she stands
            // on never tried.
            let dry = self.dry_reach(obs, reach);
            let mesh = &self.nav.mesh;
            let wet = |p: Point| !mesh.poly_at(p).is_some_and(|pi| dry.contains(&pi));
            found.sort_by(|a, b| {
                wet(a.2)
                    .cmp(&wet(b.2))
                    .then((-a.0).total_cmp(&-b.0))
                    .then(dist(a.2, obs.pos()).total_cmp(&dist(b.2, obs.pos())))
            });
        }
        found
            .into_iter()
            // The ones with more to go on, then the nearest of them: not the nearest first and then
            // those with more to go on, or a round of weak walls near her comes before a strong one
            // further off.
            .filter(|(score, _, stand, _)| {
                !(self.strong_only && *score <= SUSPICION_MIN) && self.standable(*stand, false)
            })
            .take(if panic { PANIC_WALLS } else { SUSPECTS })
            .map(|(score, li, stand, mid)| wall_objective(score, li, stand, mid, true))
            .collect()
    }

    /// The part of her reach she can walk to from where she stands without setting foot on a
    /// floor that hurts (empty if she's on one, suited, or immortal: then every floor's alike).
    fn dry_reach(&self, obs: &Observation, reach: &BTreeSet<usize>) -> BTreeSet<usize> {
        let mesh = &self.nav.mesh;
        let k = &self.knowledge;
        let mut dry = BTreeSet::new();
        let Some(here) = mesh.poly_at(obs.pos()) else {
            return dry;
        };
        if !k.mortal || k.protected(obs.tic) || k.damaging(mesh.poly(here).sector) {
            return dry;
        }
        let mut todo = vec![here];
        dry.insert(here);
        while let Some(p) = todo.pop() {
            for &q in &mesh.poly(p).portals {
                let n = mesh.portals[q].other(p);
                if reach.contains(&n) && !k.damaging(mesh.poly(n).sector) && dry.insert(n) {
                    todo.push(n);
                }
            }
        }
        dry
    }

    /// The walls worth pressing in a search for secrets that was asked for (an explore with
    /// secrets): every one with more to go on than one weak sign, not just the few best that a
    /// round of the walls takes when there's nothing else. Pressed; and bumped and shot too
    /// only where the wall's face stands out, as a mapper marks a switch or a gun line. Not
    /// every wall with a blank behind it. Her reach is as last brought up to date.
    /// At `thoroughness` 1-3 (0: none): pressed from a `secret_score` of THOROUGHNESS's first,
    /// shot too (a round's) from its second; the objective's suspicion is its secret score.
    pub fn suspected_walls(&mut self, obs: &Observation, thoroughness: u8) -> Vec<Objective> {
        let Some(&(press, shoot)) = usize::from(thoroughness)
            .checked_sub(1)
            .and_then(|i| THOROUGHNESS.get(i))
        else {
            return Vec::new();
        };
        let found: Vec<(f64, usize, Point, Point)> = self
            .secret_suspects(obs)
            .into_iter()
            .filter(|w| w.0 >= press && self.standable(w.2, false))
            .collect();
        found
            .into_iter()
            .map(|(score, li, stand, mid)| wall_objective(score, li, stand, mid, score >= shoot))
            .collect()
    }

    /// The walls a search for secrets might press, best first (one for each place behind):
    /// those with a `secret_score` the most thorough search goes to. This is the survey's
    /// score, not the rounds'. Her reach is as last brought up to date.
    fn secret_suspects(&mut self, obs: &Observation) -> Vec<(f64, usize, Point, Point)> {
        let reach = self.reach.clone();
        let known = self.knowledge.known_polys();
        let unreached = self.unreached_items(obs, &reach);
        let key = self.suspect_key(&reach, &known, &unreached, false);
        if let Some((_, found)) = self.secret_suspects.as_ref().filter(|(k, _)| *k == key) {
            return found.clone();
        }
        let mut best: BTreeMap<Behind, (f64, usize, Point, Point)> = BTreeMap::new();
        for (li, w) in self.wall_survey(obs) {
            let Wall::Suspect {
                signs,
                behind,
                stand,
                aim,
            } = w
            else {
                continue;
            };
            let score = signs.secret_score();
            let key = behind.unwrap_or(Behind::Line(li));
            if score >= THOROUGHNESS[2].0 && best.get(&key).is_none_or(|b| b.0 < score) {
                best.insert(key, (score, li, stand, aim));
            }
        }
        let mut found: Vec<(f64, usize, Point, Point)> = best.into_values().collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        self.secret_suspects = Some((key, found.clone()));
        found
    }

    /// What the walls' suspicion goes by, hashed: the same, the same walls suspected.
    fn suspect_key(
        &self,
        reach: &BTreeSet<usize>,
        known: &BTreeSet<usize>,
        unreached: &[Point],
        panic: bool,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let k = &self.knowledge;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        k.mapped.hash(&mut h);
        k.tried_walls.hash(&mut h);
        for (s, x) in &k.seen_heights {
            (s, x.key()).hash(&mut h);
        }
        k.last
            .as_ref()
            .and_then(|l| l.keys)
            .unwrap_or_default()
            .hash(&mut h);
        known.hash(&mut h);
        reach.hash(&mut h);
        for p in unreached {
            (p.0.to_bits(), p.1.to_bits()).hash(&mut h);
        }
        panic.hash(&mut h);
        h.finish()
    }

    /// Every wall she's mapped as the rubric sees it now, from where she can get to: its signs,
    /// or why it's none to try (`evil-boom secret-survey`).
    pub fn wall_survey(&self, obs: &Observation) -> Vec<(usize, crate::knowledge::model::Wall)> {
        let mesh = Arc::clone(&self.nav.mesh);
        let known = self.knowledge.known_polys();
        let unreached = self.unreached_items(obs, &self.reach);
        let places = Spots::new(known.difference(&self.reach).map(|&p| mesh.poly(p).center));
        let near = |q: Point, r: f64| places.near(q, r);
        self.knowledge
            .wall_survey(&self.reach, &unreached, Some(&near))
    }

    /// Items she's seen and can't get to (what a wall in front of one is suspected for).
    fn unreached_items(&self, obs: &Observation, reach: &BTreeSet<usize>) -> Vec<Point> {
        let mesh = &self.nav.mesh;
        let known = self.knowledge.known_polys();
        obs.things
            .iter()
            .flatten()
            .filter(|t| {
                matches!(
                    t.category,
                    "Health" | "Armor" | "Ammo" | "Weapon" | "Powerup" | "Key"
                )
            })
            .filter(|t| {
                mesh.poly_at((t.x, t.y))
                    .is_some_and(|p| known.contains(&p) && !reach.contains(&p))
            })
            .map(|t| (t.x, t.y))
            .collect()
    }

    /// The walls she suspects from where she can get to now, best first: (score, line, where
    /// to stand, the wall's middle). (`drive`'s walls.)
    pub fn suspected(
        &mut self,
        obs: &Observation,
        reach: &BTreeSet<usize>,
        panic: bool,
    ) -> Vec<(f64, usize, Point, Point)> {
        let mesh = Arc::clone(&self.nav.mesh);
        let known = self.knowledge.known_polys();
        let unreached = self.unreached_items(obs, reach);
        // The same answer again while nothing it goes by has changed: it's asked for with every
        // list of what she could do, and weighing every wall of the level each time is costly.
        let key = self.suspect_key(reach, &known, &unreached, panic);
        if let Some((_, found)) = self.suspects.as_ref().filter(|(k, _)| *k == key) {
            return found.clone();
        }
        let places = Spots::new(known.difference(reach).map(|&p| mesh.poly(p).center));
        let near = |q: Point, r: f64| places.near(q, r);
        let found = self
            .knowledge
            .suspicious(reach, &unreached, panic, Some(&near));
        self.suspects = Some((key, found.clone()));
        found
    }

    /// Is a door she set off that shuts and opens again by itself (close, wait 30s, open) still
    /// within its wait?
    fn reopens_soon(&self, obs: &Observation) -> bool {
        let m = &self.nav.map;
        self.knowledge.fired.iter().any(|f| {
            [16, 76].contains(&m.linedefs[f.line].special) && obs.tic - f.tic < REOPEN_TICS
        })
    }

    /// Not dropped lately (the longer, the more often it has been).
    pub(super) fn fresh(&self, o: &Objective, tic: i64) -> bool {
        let id = o.ident();
        let times = self.drop_count.get(&id).copied().unwrap_or(0);
        tic - self.dropped.get(&id).copied().unwrap_or(-1_000_000_000)
            > (self.patience * (RETRY_TICS * 2i64.pow((times - 1).clamp(0, 5) as u32)) as f64)
                as i64
    }

    /// Choose what to do next. The ordinary objectives first; then, with nothing found, one last
    /// resort after another, in this order, each tried only if the ones before found nothing:
    ///
    /// 1. the ordinary objectives, the safe ones and then the risky ones (`ordinary`);
    /// 2. what only a strafing run-jump gets to (`strafing_runs`);
    /// 3. the places she got stuck, another go (`stuck_places_again`);
    /// 4. walls that might hide something (`suspect_walls`);
    /// 5. what was given up on lately, another go (`given_up_again`);
    /// 6. every wall round her (`every_wall`);
    /// 7. the level again, if whoever plays it would rather (`Hooks::before_exit`); a wait for
    ///    a door that opens again by itself;
    /// 8. the exit, where it was being held back (`held_back_exit`);
    /// 9. the run-jumps missed once (`missed_jumps_again`);
    /// 10. switches and walls from a floor that hurts, the health margin lifted
    ///     (`with_harm_allowed`).
    ///
    /// With nothing at all, she stands, and where she can get to is noted as a trap.
    pub(super) fn next(&mut self, obs: &Observation, hooks: &mut dyn Hooks) {
        self.follower = None;
        let suited = self.knowledge.protected(obs.tic);
        if suited && !self.suited && self.short_of_health.is_some() {
            // A suit on: what cost too much a moment ago may not now, and its minute is short.
            self.log.push(format!(
                "tic {}: suit on: what cost too much gets another go",
                obs.tic
            ));
            self.dropped.clear();
            self.no_route.clear();
            self.short_of_health = None;
        }
        self.suited = suited;
        if self.difficult {
            self.set_jumps(false); // (the jumps missed once: out again, other ways first)
        }
        self.exits_open = !self.exit_last;
        let perceive = self.knowledge.perceive;
        self.strafe_ok = false;
        self.harm_ok = false;
        self.from_dead_end = false;
        let mut found = self.ordinary(obs, hooks);
        if !found {
            found = self.strafing_runs(obs, hooks);
        }
        if !found {
            found = self.stuck_places_again(obs, hooks);
        }
        if found {
            self.panic_tries = 0;
        }
        if !found && perceive && self.suspecting == Suspecting::No {
            found = self.suspect_walls(obs, hooks);
        }
        if !found {
            found = self.given_up_again(obs, hooks);
        }
        // With the exit there, held back for what's left: one round of the walls with little to go
        // on, then out. A press that turns up something to do starts another round.
        self.strong_only = self.exit_last && self.panic_tries >= PANIC_WALLS;
        if !found && perceive && (!self.exit_last || !self.exits_open) {
            found = self.every_wall(obs, hooks);
        }
        self.suspecting = Suspecting::No;
        self.strong_only = false;
        if !found && self.exit_last && hooks.before_exit(self, obs) {
            self.log.push(format!(
                "tic {}: nothing else to do here: the level again, for what's left",
                obs.tic
            ));
            self.restart_wanted = true;
            self.current = None;
            return;
        }
        // Not out while a door she set off that shuts for a while and opens again by itself is
        // still shut: what's past it is still to do, and a player waits for it.
        if !found && self.exit_last && self.reopens_soon(obs) {
            if self.current.is_some() || !self.waiting_reopen {
                self.log.push(format!(
                    "tic {}: nothing else to do till a door opens again",
                    obs.tic
                ));
            }
            self.waiting_reopen = true;
            self.current = None;
            return;
        }
        if !found && self.waiting_reopen {
            // Open again: what was given up on while it was shut gets another go, the way through it
            // being back.
            self.waiting_reopen = false;
            self.retried = None;
            self.current = None;
            return;
        }
        if !found && self.exit_last {
            found = self.held_back_exit(obs, hooks);
        }
        if !found {
            found = self.missed_jumps_again(obs, hooks);
        }
        if !found {
            found = self.with_harm_allowed(obs, hooks);
        }
        if !found {
            if self.current.is_some() {
                self.log
                    .push(format!("tic {}: nothing left to do", obs.tic));
            }
            self.current = None;
            // The exit out of reach too: where she can get from here is a trap, if it's where she has
            // started again from. Known for the next go.
            self.trapped = Some(self.reach.clone());
        }
    }

    /// The best objective of those offered now: of the safe ones, else of the risky ones.
    fn safe_then_risky(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks)
    }

    /// The ordinary objectives: the safe ones; then, where the exit is being held back but not
    /// for anything risky (`exit_before_risks`), the exit; then the risky ones.
    fn ordinary(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        let mut found = self.try_objectives(obs, false, hooks);
        if !found && self.exit_last && self.exit_before_risks {
            // The exit is held back for everything else, but not for what she mightn't come back from:
            // with nothing safe left, it's out.
            self.exit_last = false;
            self.exits_open = true;
            self.log.push(format!(
                "tic {}: nothing safe left to do: for the exit",
                obs.tic
            ));
            found = self.try_objectives(obs, false, hooks);
        }
        found || self.try_objectives(obs, true, hooks)
    }

    /// Nothing else: what a strafing run-jump gets to, if the level has any.
    fn strafing_runs(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        if !self.nav.jumps.iter().any(|j| j.strafe) {
            return false;
        }
        self.strafe_ok = true;
        let found = self.safe_then_risky(obs, hooks);
        if found {
            self.log.push(format!(
                "tic {}: nothing else to do: strafing runs too",
                obs.tic
            ));
        } else {
            self.strafe_ok = false;
        }
        found
    }

    /// The places she got stuck are given another go: two failures can be bad luck, or a goal
    /// that was the trouble rather than the way. Before the walls: a way she knows is a better
    /// bet than a guess at one. Once till she knows something more: stuck there again, it's the
    /// walls' turn.
    fn stuck_places_again(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        let stuck: Vec<usize> = self
            .knowledge
            .stuck_at
            .iter()
            .filter(|(_, &n)| n >= STUCK_TIMES)
            .map(|(&p, _)| p)
            .collect();
        if stuck.is_empty()
            || self.stuck_retried.as_deref() == Some(self.knowledge.known_summary().as_str())
        {
            return false;
        }
        self.stuck_retried = Some(self.knowledge.known_summary());
        self.log.push(format!(
            "tic {}: nothing else to do: the {} places she got stuck get another go",
            obs.tic,
            stuck.len()
        ));
        for p in stuck {
            self.knowledge.note(Note::Stuck(p, STUCK_TIMES - 1));
        }
        self.safe_then_risky(obs, hooks)
    }

    /// Nothing else: walls that might hide something, pressed in turn. Leaves `suspecting` at
    /// `Walls` for the tiers after it, as `next` expects.
    fn suspect_walls(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        self.out_of_options.get_or_insert(obs.tic);
        self.suspecting = Suspecting::Walls;
        let found = self.safe_then_risky(obs, hooks);
        if found {
            self.panic_tries = 0;
            self.log
                .push(format!("tic {}: nothing else to do: trying walls", obs.tic));
        }
        found
    }

    /// Nothing else: what was given up on lately gets another go with its wait cut short (it may
    /// have been dropped only because the way was shut a while). Once till she knows something
    /// more, not after each wall pressed that did nothing: that would throw goals out of reach
    /// back in every time, seconds of planning each.
    fn given_up_again(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        if self.dropped.is_empty()
            || self.retried.as_deref() == Some(self.knowledge.known_summary().as_str())
        {
            return false;
        }
        self.retried = Some(self.knowledge.known_summary());
        self.log.push(format!(
            "tic {}: nothing else to do: the {} things given up on lately get another go",
            obs.tic,
            self.dropped.len()
        ));
        self.dropped.clear();
        self.suspecting = Suspecting::No;
        self.safe_then_risky(obs, hooks)
    }

    /// And nothing to try either: is there a way out of here at all? Every wall round her,
    /// nearest first, as a player in a pit with no way out would.
    fn every_wall(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        self.suspecting = Suspecting::Panic;
        let found = self.safe_then_risky(obs, hooks);
        if found {
            // Counted: walls with no more to go on than one weak sign. Those with more are all pressed.
            if self
                .current
                .as_ref()
                .is_some_and(|o| o.suspicion <= SUSPICION_MIN)
            {
                self.panic_tries += 1;
            }
            self.log.push(format!(
                "tic {}: nothing at all to do: pressing every wall round here",
                obs.tic
            ));
        }
        found
    }

    /// The exit, which was being held back for everything else.
    fn held_back_exit(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        self.log
            .push(format!("tic {}: nothing else to do: for the exit", obs.tic));
        self.exits_open = true;
        self.safe_then_risky(obs, hooks)
    }

    /// The run-jumps missed once (left out since) get another go, if there are any.
    fn missed_jumps_again(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        if !self
            .knowledge
            .missed_jumps
            .values()
            .any(|&n| n < JUMP_MISSES)
        {
            return false;
        }
        self.log.push(format!(
            "tic {}: nothing else to do: the jumps missed once get another go",
            obs.tic
        ));
        self.set_jumps(true);
        self.safe_then_risky(obs, hooks)
    }

    /// Nothing at all else, walls and all: a switch pressed from a floor that hurts is the only
    /// way on left, and shut in already, what it might shut is no worse than staying. The health
    /// margin is lifted for this pass (`harm_ok`).
    ///
    /// What the passes before refused this very think gets this pass too: it was refused under
    /// the margin that this pass lifts. Only what had no route, though, not what was dropped as
    /// gone (a monster hunted and not there): that would be taken up and dropped again every
    /// tic, each think routing every wall afresh.
    fn with_harm_allowed(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        let tic = obs.tic;
        let refused = std::mem::take(&mut self.refused_now.1);
        self.dropped
            .retain(|id, &mut t| t != tic || !refused.contains(id));
        self.refused_now.1 = refused;
        self.harm_ok = true;
        // The walls too, every one round her: left out of this pass, a wall refused under the
        // margin would never be tried with the margin lifted.
        if self.knowledge.perceive {
            self.suspecting = Suspecting::Panic;
        }
        let found = self.safe_then_risky(obs, hooks);
        self.suspecting = Suspecting::No;
        if found {
            self.log.push(format!(
                "tic {}: nothing else to do: switches and walls from a floor that hurts",
                obs.tic
            ));
        }
        self.from_dead_end = found;
        if !found {
            self.harm_ok = false;
        }
        found
    }

    /// Take up the best objective there's a route to: of the nearest few as the crow flies, the
    /// one nearest by the way there (teleporters and lifts make those very different).
    fn try_objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> bool {
        for _ in 0..3 {
            let ranked = self.ranked(obs, risky, hooks);
            if self
                .debug_tics
                .is_some_and(|(a, b)| (a..=b).contains(&obs.tic))
            {
                for o in ranked.iter().skip(COMPARED) {
                    self.log.push(format!(
                        "  (not compared) '{}' at ({:.0}, {:.0}), priority {}",
                        o.label,
                        o.goal.0,
                        o.goal.1,
                        o.priority()
                    ));
                }
            }
            // The exit first of all, except for something she needs that's nearer by the way than it
            // is: those are compared with it too, by their ways. A level she knows the whole of is
            // otherwise the exit from its first tic, walked for with what she started with.
            let exit_first = ranked.first().is_some_and(|o| o.kind == Kind::Exit);
            let before_exit = |o: &Objective| o.kind == Kind::Item && (o.needed || o.urgent);
            let mut rest = ranked;
            let mut options: Vec<Objective> = rest.drain(..COMPARED.min(rest.len())).collect();
            if exit_first {
                let (keep, put_back): (Vec<Objective>, Vec<Objective>) = options
                    .into_iter()
                    .partition(|o| o.kind == Kind::Exit || before_exit(o));
                options = keep;
                rest.splice(0..0, put_back);
                let more = COMPARED + 1 - options.len().min(COMPARED);
                for _ in 0..more {
                    let Some(i) = rest.iter().position(before_exit) else {
                        break;
                    };
                    options.push(rest.remove(i));
                }
            }
            if options.is_empty() {
                return false;
            }
            let mut routed: Vec<(f64, Objective, Vec<Step>)> = Vec::new();
            let debug = self
                .debug_tics
                .is_some_and(|(a, b)| (a..=b).contains(&obs.tic));
            for obj in options {
                let route = self.route_to(&obj, obs);
                if debug {
                    let how = route.as_ref().map_or_else(
                        || "no route".to_string(),
                        |st| format!("{:.0} ({} steps)", route_length(st), st.len()),
                    );
                    self.log.push(format!(
                        "  option '{}' at ({:.0}, {:.0}), priority {}: {how}",
                        obj.label,
                        obj.goal.0,
                        obj.goal.1,
                        obj.priority()
                    ));
                }
                match route {
                    Some(steps) if !steps.is_empty() => {
                        let length = route_length(&steps);
                        let mut obj = obj;
                        if let Some(w) = obj.worth.filter(|&w| length > w) {
                            self.log.push(format!("tic {}: '{}' not worth {length:.0} to get to (at most {w:.0}): in its turn", obs.tic, obj.label));
                            obj.urgent = false;
                            obj.needed = false;
                            obj.worth = None;
                            obj.label = obj
                                .label
                                .trim_end_matches(WHILE_OPEN)
                                .trim_end_matches(" right away")
                                .to_string();
                        }
                        // A wall given up on before counts as twice as far for each time.
                        let tries = obj.wall.map_or(1.0, |w| {
                            2f64.powi(self.wall_drops.get(&w).copied().unwrap_or(0))
                        });
                        let prio = obj.priority();
                        let weighed = hooks.weigh(obs, &obj);
                        routed.push((length * obj.kind.weight() * tries * weighed, obj, steps));
                        if prio < 2 && !exit_first {
                            break; // (nothing further down the list comes first anyway)
                        }
                    }
                    _ => self.drop_no_route(&obj, obs),
                }
            }
            if routed.is_empty() {
                continue;
            }
            routed.sort_by(|a, b| {
                (a.1.priority(), a.0)
                    .partial_cmp(&(b.1.priority(), b.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            // And on the way to anything that comes first for what it is (the exit, a gun, a keyed
            // door), what she'd pick up that's nearer by the way than it is had first. Blind, she has
            // what lies about by the time she knows of anything far; knowing the level, she'd walk past
            // it for the far thing. Not before something urgent.
            if routed[0].1.priority() < 2 && !routed[0].1.urgent {
                let far = routed[0].0;
                let pos = obs.pos();
                let mut near: Vec<usize> = (0..rest.len())
                    .filter(|&i| rest[i].kind == Kind::Item && dist(rest[i].goal, pos) < far)
                    .collect();
                near.sort_by(|&a, &b| dist(rest[a].goal, pos).total_cmp(&dist(rest[b].goal, pos)));
                for i in near.into_iter().take(ON_THE_WAY) {
                    let obj = rest[i].clone();
                    if let Some(steps) = self.route_to(&obj, obs).filter(|st| !st.is_empty()) {
                        let weighed =
                            route_length(&steps) * obj.kind.weight() * hooks.weigh(obs, &obj);
                        routed.push((weighed, obj, steps));
                    }
                }
                if let Some(i) = (1..routed.len())
                    .filter(|&i| routed[i].1.kind == Kind::Item && routed[i].0 < far)
                    .min_by(|&a, &b| routed[a].0.total_cmp(&routed[b].0))
                {
                    routed.swap(0, i);
                }
            }
            self.journal.note(|| crate::decisions::Record {
                options: routed
                    .iter()
                    .map(|(weighed, o, _)| (o.label.clone(), *weighed))
                    .collect(),
                ..crate::decisions::Record::new(obs.tic, "objective", routed[0].1.label.clone())
            });
            let (_, obj, steps) = routed.swap_remove(0);
            self.start(obs, obj, steps);
            return true;
        }
        false
    }

    /// Set about `obj` by `steps` (its route).
    pub(super) fn start(&mut self, obs: &Observation, obj: Objective, steps: Vec<Step>) {
        self.since = obs.tic;
        self.end_why = None;
        self.end_kind = EndKind::Plain;
        // Time to do it: walking the route, and some.
        self.allowed = GIVE_UP_TICS.max((route_length(&steps) / ROUTE_SPEED) as i64 + ROUTE_SLACK);
        self.exits_seen = self.knowledge.exits().len();
        let n = steps.len();
        // How far it is, as the crow flies and along the way: for reading a run's ordering.
        let (along, straight) = (route_length(&steps), dist(obs.pos(), obj.goal));
        let states = self.states();
        let view = self.view();
        let mut follower = RouteFollower::new(
            view,
            steps,
            obs.keys.unwrap_or_default(),
            Some(states),
            false,
            obs.tic,
        );
        follower.fired.extend(self.spent_once()); // (its replans: not by them either)
                                                  // The practice pass: nothing can hurt her, a barrel's blast neither.
        follower.mortal = self.knowledge.mortal && !self.knowledge.immune;
        self.follower = Some(follower);
        self.follower_seen = 0;
        self.log.push(format!(
            "tic {}: {} ({n} steps, {along:.0} along, {straight:.0} straight) | {}",
            obs.tic,
            obj.label,
            self.knowledge.summary()
        ));
        if let Some(li) = obj
            .trigger
            .as_ref()
            .filter(|_| obj.kind == Kind::Switch)
            .and_then(TriggerLine::linedef)
        {
            self.pressed_knowing
                .insert(li, self.knowledge.known_summary());
        }
        self.current = Some(obj);
    }

    /// Set about this objective now, whatever she'd have chosen (evil-boom drive: what if she had?).
    /// False if she has no route to it.
    pub fn take(&mut self, obs: &Observation, obj: Objective) -> bool {
        match self.route_to(&obj, obs) {
            Some(steps) if !steps.is_empty() => {
                self.start(obs, obj, steps);
                true
            }
            _ => false,
        }
    }
}

/// Pressing a suspected wall (line `li`, `score` its suspicion) from `stand`, facing `mid`; `round`:
/// searched for (bumped, shot too), not just tried on the way.
pub(super) fn wall_objective(
    score: f64,
    li: usize,
    stand: Point,
    mid: Point,
    round: bool,
) -> Objective {
    let mut o = Objective::new(
        Kind::Wall,
        stand,
        format!("try the wall (line {li}, {score:.1})"),
    );
    o.look_at = Some(mid);
    o.wall = Some(li);
    o.suspicion = score;
    o.round = round;
    o
}
