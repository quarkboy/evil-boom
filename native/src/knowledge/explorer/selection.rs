//! Suspecting walls and choosing the next objective when ordinary exploration stalls.

use super::{route_length, Explorer, Hooks, Kind, Objective, Spots, Suspecting, GIVE_UP_TICS, JUMP_MISSES, STUCK_TIMES, WHILE_OPEN};
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
    pub(super) fn walls(&mut self, obs: &Observation, reach: &BTreeSet<usize>, panic: bool) -> Vec<Objective> {
        let mut found = self.suspected(obs, reach, panic);
        if panic {
            // (all of them: those with somewhere she can't get to behind first, nearest first;
            // but before them those she can get to without a floor that hurts, unsuited: her
            // reach runs on over such floors, but with the health it takes gone, routes there
            // refuse them, and a round of far walls there left the walls of the dry floor she
            // was stranded on, a hidden door among them, never tried. E4M6 UV: health 9, a dry
            // ledge in a lava field, 12 far walls dropped "no route" every round for 300 s)
            let dry = self.dry_reach(obs, reach);
            let mesh = &self.nav.mesh;
            let wet = |p: Point| !mesh.poly_at(p).is_some_and(|pi| dry.contains(&pi));
            found.sort_by(|a, b| wet(a.2).cmp(&wet(b.2)).then((-a.0).total_cmp(&-b.0)).then(dist(a.2, obs.pos()).total_cmp(&dist(b.2, obs.pos()))));
        }
        found
            .into_iter()
            // (the ones with more to go on, then the nearest of them: not the nearest, then those
            // with more to go on, none of a round of weak ones near her while E1M6's door 1119,
            // further off, waited)
            .filter(|(score, _, stand, _)| !(self.strong_only && *score <= SUSPICION_MIN) && self.standable(*stand, false))
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
        let Some(here) = mesh.poly_at(obs.pos()) else { return dry };
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

    /// The walls worth a search for secrets asked for (Evil's explore with secrets): every one with
    /// more to go on than one weak sign, not just the few best a round of the walls takes when
    /// there's nothing else; pressed, and bumped and shot too only where the wall's face stands
    /// out (as a mapper marks a switch or a gun line). Not every wall with a blank behind it,
    /// pressed and shot: the user's, watching E1M1's start room searched wall by wall, sixteen of
    /// them. Her reach as last brought up to date.
    /// At `thoroughness` 1-3 (0: none): pressed from a `secret_score` of THOROUGHNESS's first,
    /// shot too (a round's) from its second; the objective's suspicion its secret score.
    pub fn suspected_walls(&mut self, obs: &Observation, thoroughness: u8) -> Vec<Objective> {
        let Some(&(press, shoot)) = usize::from(thoroughness).checked_sub(1).and_then(|i| THOROUGHNESS.get(i)) else { return Vec::new() };
        let found: Vec<(f64, usize, Point, Point)> = self.secret_suspects(obs).into_iter().filter(|w| w.0 >= press && self.standable(w.2, false)).collect();
        found.into_iter().map(|(score, li, stand, mid)| wall_objective(score, li, stand, mid, score >= shoot)).collect()
    }

    /// The walls a search for secrets might press, best first (one for each place behind): those
    /// with a `secret_score` the most thorough search goes to (the survey's weights, not the
    /// rounds' score: with the rounds' over 2, the same secrets with a quarter more walls). Her
    /// reach as last brought up to date.
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
            let Wall::Suspect { signs, behind, stand, aim } = w else { continue };
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
    fn suspect_key(&self, reach: &BTreeSet<usize>, known: &BTreeSet<usize>, unreached: &[Point], panic: bool) -> u64 {
        use std::hash::{Hash, Hasher};
        let k = &self.knowledge;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        k.mapped.hash(&mut h);
        k.tried_walls.hash(&mut h);
        for (s, x) in &k.seen_heights {
            (s, x.key()).hash(&mut h);
        }
        k.last.as_ref().and_then(|l| l.keys).unwrap_or_default().hash(&mut h);
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
        self.knowledge.wall_survey(&self.reach, &unreached, Some(&near))
    }

    /// Items she's seen and can't get to (what a wall in front of one is suspected for).
    fn unreached_items(&self, obs: &Observation, reach: &BTreeSet<usize>) -> Vec<Point> {
        let mesh = &self.nav.mesh;
        let known = self.knowledge.known_polys();
        obs.things
            .iter()
            .flatten()
            .filter(|t| matches!(t.category, "Health" | "Armor" | "Ammo" | "Weapon" | "Powerup" | "Key"))
            .filter(|t| mesh.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p) && !reach.contains(&p)))
            .map(|t| (t.x, t.y))
            .collect()
    }

    /// The walls she suspects from where she can get to now, best first: (score, line, where
    /// to stand, the wall's middle). (`drive`'s walls.)
    pub fn suspected(&mut self, obs: &Observation, reach: &BTreeSet<usize>, panic: bool) -> Vec<(f64, usize, Point, Point)> {
        let mesh = Arc::clone(&self.nav.mesh);
        let known = self.knowledge.known_polys();
        let unreached = self.unreached_items(obs, reach);
        // (the same again while nothing it goes by has changed: asked for every list of what she
        // could do, for the strong suspects by her, every wall of the level weighed each time)
        let key = self.suspect_key(reach, &known, &unreached, panic);
        if let Some((_, found)) = self.suspects.as_ref().filter(|(k, _)| *k == key) {
            return found.clone();
        }
        let places = Spots::new(known.difference(reach).map(|&p| mesh.poly(p).center));
        let near = |q: Point, r: f64| places.near(q, r);
        let found = self.knowledge.suspicious(reach, &unreached, panic, Some(&near));
        self.suspects = Some((key, found.clone()));
        found
    }

    /// Is a door she set off that shuts and opens again by itself (close, wait 30s, open) still
    /// within its wait?
    fn reopens_soon(&self, obs: &Observation) -> bool {
        let m = &self.nav.map;
        self.knowledge.fired.iter().any(|f| [16, 76].contains(&m.linedefs[f.line].special) && obs.tic - f.tic < REOPEN_TICS)
    }

    /// Not dropped lately (the longer, the more often it has been).
    pub(super) fn fresh(&self, o: &Objective, tic: i64) -> bool {
        let id = o.ident();
        let times = self.drop_count.get(&id).copied().unwrap_or(0);
        tic - self.dropped.get(&id).copied().unwrap_or(-1_000_000_000) > (self.patience * (RETRY_TICS * 2i64.pow((times - 1).clamp(0, 5) as u32)) as f64) as i64
    }

    pub(super) fn next(&mut self, obs: &Observation, hooks: &mut dyn Hooks) {
        self.follower = None;
        let suited = self.knowledge.protected(obs.tic);
        if suited && !self.suited && self.short_of_health.is_some() {
            // (a suit on: what cost too much a moment ago may not now, and its minute is short)
            self.log.push(format!("tic {}: suit on: what cost too much gets another go", obs.tic));
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
        let mut found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        if !found && self.nav.jumps.iter().any(|j| j.strafe) {
            // (nothing else: what a strafing run gets to, MAP03's well)
            self.strafe_ok = true;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                self.log.push(format!("tic {}: nothing else to do: strafing runs too", obs.tic));
            } else {
                self.strafe_ok = false;
            }
        }
        if found {
            self.panic_tries = 0;
        }
        if !found && perceive && self.suspecting == Suspecting::No {
            self.out_of_options.get_or_insert(obs.tic);
            // (nothing else: walls that might hide something, pressed in turn)
            self.suspecting = Suspecting::Walls;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                self.panic_tries = 0;
                self.log.push(format!("tic {}: nothing else to do: trying walls", obs.tic));
            }
        }
        if !found && !self.dropped.is_empty() && self.retried.as_deref() != Some(self.knowledge.known_summary().as_str()) {
            // (nothing else: what was given up on lately gets another go, its wait cut short,
            // once till there's something else to do: dropped when the way was shut a while,
            // E1M4's bonus behind the door a walk-over line opens, it waited out her whole life)
            // (once till she knows something more: not each wall pressed that did nothing, which
            // threw MAP15's and E4M9's goals out of reach back in, seconds of planning each)
            self.retried = Some(self.knowledge.known_summary());
            self.log.push(format!("tic {}: nothing else to do: the {} things given up on lately get another go", obs.tic, self.dropped.len()));
            self.dropped.clear();
            self.suspecting = Suspecting::No;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        // (with the exit there, held back for what's left: one round of the walls with little to
        // go on, then out; a press that turns up something to do starts another. E2M5 went round
        // every wall of the level)
        self.strong_only = self.exit_last && self.panic_tries >= PANIC_WALLS;
        if !found && perceive && (!self.exit_last || !self.exits_open) {
            // (and nothing to try either: is there a way out of here at all? Every wall round,
            // nearest first, as a player in a pit with no way out would)
            self.suspecting = Suspecting::Panic;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                // (counted: walls with no more to go on than one weak sign; those with more are
                // all pressed: E4M7's line 1283, E4M9's 1637)
                if self.current.as_ref().is_some_and(|o| o.suspicion <= SUSPICION_MIN) {
                    self.panic_tries += 1;
                }
                self.log.push(format!("tic {}: nothing at all to do: pressing every wall round here", obs.tic));
            }
        }
        self.suspecting = Suspecting::No;
        self.strong_only = false;
        if !found && self.exit_last && hooks.before_exit(self, obs) {
            self.log.push(format!("tic {}: nothing else to do here: the level again, for what's left", obs.tic));
            self.restart_wanted = true;
            self.current = None;
            return;
        }
        // (not out while a door she set off that shuts for a while and opens again by itself is
        // still shut: what's past it is still to do, as a player waits for it; E1M6's door 37,
        // shut on the way to a wall, the level left with a blur sphere behind it)
        if !found && self.exit_last && self.reopens_soon(obs) {
            if self.current.is_some() || !self.waiting_reopen {
                self.log.push(format!("tic {}: nothing else to do till a door opens again", obs.tic));
            }
            self.waiting_reopen = true;
            self.current = None;
            return;
        }
        if !found && self.waiting_reopen {
            // (open again: what was given up on while it was shut gets another go, the way
            // through it back)
            self.waiting_reopen = false;
            self.retried = None;
            self.current = None;
            return;
        }
        if !found && self.exit_last {
            self.log.push(format!("tic {}: nothing else to do: for the exit", obs.tic));
            self.exits_open = true;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        if !found && self.knowledge.missed_jumps.values().any(|&n| n < JUMP_MISSES) {
            self.log.push(format!("tic {}: nothing else to do: the jumps missed once get another go", obs.tic));
            self.set_jumps(true);
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        let stuck: Vec<usize> = self.knowledge.stuck_at.iter().filter(|(_, &n)| n >= STUCK_TIMES).map(|(&p, _)| p).collect();
        if !found && !stuck.is_empty() {
            // (the places she got stuck, given another go: two goes can be bad luck, or a goal that
            // was the trouble, not the way: E3M4's, MAP05's, MAP27's)
            self.log.push(format!("tic {}: nothing else to do: the {} places she got stuck get another go", obs.tic, stuck.len()));
            for p in stuck {
                self.knowledge.note(Note::Stuck(p, STUCK_TIMES - 1));
            }
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        if !found {
            // (nothing at all else, walls and all: a switch pressed from a floor that hurts, the
            // only way on left; shut in already, what it might shut is no worse than staying.
            // MAP09's start, an island in a nukage pit, its lifts out switches on the pit's walls)
            // What the passes before refused this very think gets the pass too: refused under
            // the margin it lifts. (A dry island in a nukage pit at 28 health, the walk-over line
            // that opens the pit's door a wade of 7 off, refused at 21 against 25 and so left
            // out of this pass: stood there the hour)
            let tic = obs.tic;
            self.dropped.retain(|_, &mut t| t != tic);
            self.harm_ok = true;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                self.log.push(format!("tic {}: nothing else to do: switches from a floor that hurts", obs.tic));
            }
            self.from_dead_end = found;
            if !found {
                self.harm_ok = false;
            }
        }
        if !found {
            if self.current.is_some() {
                self.log.push(format!("tic {}: nothing left to do", obs.tic));
            }
            self.current = None;
            // (the exit out of reach too: where she can get from here is a trap, if it's where
            // she's started again from; known for the next go)
            self.trapped = Some(self.reach.clone());
        }
    }

    /// Take up the best objective there's a route to: of the nearest few as the crow flies, the
    /// one nearest by the way there (teleporters and lifts make those very different: MAP24).
    fn try_objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> bool {
        for _ in 0..3 {
            let ranked = self.ranked(obs, risky, hooks);
            if self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic)) {
                for o in ranked.iter().skip(COMPARED) {
                    self.log.push(format!("  (not compared) '{}' at ({:.0}, {:.0}), priority {}", o.label, o.goal.0, o.goal.1, o.priority()));
                }
            }
            let options: Vec<Objective> = ranked.into_iter().take(COMPARED).collect();
            if options.is_empty() {
                return false;
            }
            let mut routed: Vec<(f64, Objective, Vec<Step>)> = Vec::new();
            let debug = self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic));
            for obj in options {
                let route = self.route_to(&obj, obs);
                if debug {
                    let how = route.as_ref().map_or_else(|| "no route".to_string(), |st| format!("{:.0} ({} steps)", route_length(st), st.len()));
                    self.log.push(format!("  option '{}' at ({:.0}, {:.0}), priority {}: {how}", obj.label, obj.goal.0, obj.goal.1, obj.priority()));
                }
                match route {
                    Some(steps) if !steps.is_empty() => {
                        let length = route_length(&steps);
                        let mut obj = obj;
                        if let Some(w) = obj.worth.filter(|&w| length > w) {
                            self.log.push(format!("tic {}: '{}' not worth {length:.0} to get to (at most {w:.0}): in its turn", obs.tic, obj.label));
                            obj.urgent = false;
                            obj.worth = None;
                            obj.label = obj.label.trim_end_matches(WHILE_OPEN).trim_end_matches(" right away").to_string();
                        }
                        // (a wall given up on before: as if twice as far for each time)
                        let tries = obj.wall.map_or(1.0, |w| 2f64.powi(self.wall_drops.get(&w).copied().unwrap_or(0)));
                        let prio = obj.priority();
                        let weighed = hooks.weigh(obs, &obj);
                        routed.push((length * obj.kind.weight() * tries * weighed, obj, steps));
                        if prio < 2 {
                            break; // (nothing further down the list comes first anyway)
                        }
                    }
                    _ => self.drop_objective(&obj, obs, "no route"),
                }
            }
            if routed.is_empty() {
                continue;
            }
            routed.sort_by(|a, b| (a.1.priority(), a.0).partial_cmp(&(b.1.priority(), b.0)).unwrap_or(std::cmp::Ordering::Equal));
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
        // (time to do it: walking the route, and some)
        self.allowed = GIVE_UP_TICS.max((route_length(&steps) / ROUTE_SPEED) as i64 + ROUTE_SLACK);
        self.exits_seen = self.knowledge.exits().len();
        let n = steps.len();
        // (how far it is, as the crow flies and along the way: for reading a run's ordering)
        let (along, straight) = (route_length(&steps), dist(obs.pos(), obj.goal));
        let states = self.states();
        let view = self.view();
        let mut follower = RouteFollower::new(view, steps, obs.keys.unwrap_or_default(), Some(states), false, obs.tic);
        follower.fired.extend(self.spent_once()); // (its replans: not by them either)
        follower.mortal = self.knowledge.mortal;
        self.follower = Some(follower);
        self.follower_seen = 0;
        self.log.push(format!("tic {}: {} ({n} steps, {along:.0} along, {straight:.0} straight) | {}", obs.tic, obj.label, self.knowledge.summary()));
        if let Some(li) = obj.trigger.as_ref().filter(|_| obj.kind == Kind::Switch).and_then(TriggerLine::linedef) {
            self.pressed_knowing.insert(li, self.knowledge.known_summary());
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
pub(super) fn wall_objective(score: f64, li: usize, stand: Point, mid: Point, round: bool) -> Objective {
    let mut o = Objective::new(Kind::Wall, stand, format!("try the wall (line {li}, {score:.1})"));
    o.look_at = Some(mid);
    o.wall = Some(li);
    o.suspicion = score;
    o.round = round;
    o
}
