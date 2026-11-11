//! Legs of a plan timed against the temporary heights they rely on: a lift that will have
//! gone back up by the time the player gets there can't be used.

use super::triggers::TimedKey;
use super::world::{without, world_hash, Pair, Timers, World, ACTIVATION_TICS, LEG_SPEED};
use super::{KeySet, PlanCaches, Planner};
use crate::det::{HashMap, HashSet};
use crate::geom::{dist, Point};
use crate::nav::{Doors, Found, Heights, Planned, TeleKind, Via};
use crate::stats::{self, Phase};
use std::sync::Arc;
use std::time::Instant;

/// Tics a lift takes to come back up to be jumped off again.
const LIFT_BACK: f64 = 60.0;
/// Map units between samples used to estimate arrival at a sector along a path.
const ARRIVAL_SAMPLE_STEP: f64 = 8.0;

impl Planner<'_> {
    /// When the player gets to each sector along a path (tics, from 10 for setting off).
    fn arrival_tics(&self, path: &Found) -> HashMap<usize, f64> {
        let mut out: HashMap<usize, f64> = HashMap::default();
        let mut note = |q: Point, d: f64| {
            if let Some(s) = self.nav.sector_at(q) {
                out.entry(s).or_insert(ACTIVATION_TICS + d / LEG_SPEED);
            }
        };
        let mut so_far = 0.0;
        let mut prev: Option<Point> = None;
        for &p in &path.waypoints {
            let Some(p) = p else {
                prev = None;
                continue;
            };
            match prev {
                None => note(p, so_far),
                Some(pr) => {
                    let seg = dist(pr, p);
                    let n = ((seg / ARRIVAL_SAMPLE_STEP).floor() as i64).max(1);
                    for i in 1..=n {
                        let (fi, nf) = (i as f64, n as f64);
                        note(
                            (pr.0 + (p.0 - pr.0) * fi / nf, pr.1 + (p.1 - pr.1) * fi / nf),
                            so_far + seg * fi / nf,
                        );
                    }
                    so_far += seg;
                }
            }
            prev = Some(p);
        }
        for &poly in &path.polys {
            out.entry(self.nav.poly_sector[poly])
                .or_insert(ACTIVATION_TICS + path.cost / LEG_SPEED);
        }
        out
    }

    /// The jumps along a path: (takeoff, the sectors and heights each needs).
    fn jumps<'p>(
        &'p self,
        path: &'p Found,
    ) -> impl Iterator<Item = (Point, &'p [(usize, Heights)])> + 'p {
        path.vias
            .iter()
            .enumerate()
            .filter_map(move |(i, via)| match via {
                Some(Via::Tele(ti)) => match &self.nav.tele[path.polys[i - 1]][*ti].kind {
                    TeleKind::Jump {
                        takeoff, requires, ..
                    } => Some((*takeoff, requires.as_slice())),
                    TeleKind::Line { .. } => None,
                },
                _ => None,
            })
    }

    /// A path from `start` to `goal`, not relying on temporary heights (`deadlines`) it would
    /// be too late for; and the heights it was found with.
    pub(super) fn timed_path(
        &mut self,
        start: Point,
        goal: Point,
        states: &World,
        keys: KeySet,
        deadlines: &Timers,
    ) -> Option<(Found, World)> {
        let t0 = Instant::now();
        let timers = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for (k, v) in deadlines {
                (k, v.to_bits()).hash(&mut h);
            }
            h.finish()
        };
        let key: TimedKey = (
            start.0.to_bits(),
            start.1.to_bits(),
            goal.0.to_bits(),
            goal.1.to_bits(),
            world_hash(states),
            keys,
            timers,
            self.strict_fit,
        );
        if let Some(found) = self.caches.timed.get(&key) {
            let out = found.clone();
            stats::record(Phase::TimedPath, t0);
            return out;
        }
        let doors = self.doors_for(keys);
        let out = self.timed_path_inner(start, goal, states, &doors, deadlines);
        if self.caches.timed.len() > PlanCaches::MAX_TIMED {
            self.caches.timed.clear();
        }
        self.caches.timed.insert(key, out.clone());
        stats::record(Phase::TimedPath, t0);
        out
    }

    fn timed_path_inner(
        &mut self,
        start: Point,
        goal: Point,
        states: &World,
        doors: &Arc<Doors>,
        deadlines: &Timers,
    ) -> Option<(Found, World)> {
        let doors = (!doors.is_empty()).then_some(&**doors);
        let path = self
            .nav
            .find_path(start, goal, &mut Planned(states), doors, self.strict_fit)?;
        if deadlines.is_empty() {
            return Some((path, states.clone()));
        }
        let mut arrive = self.arrival_tics(&path);
        // what a jump needs, it needs when taking off
        for (takeoff, requires) in self.jumps(&path) {
            let Some(&at) = self.nav.sector_at(takeoff).and_then(|src| arrive.get(&src)) else {
                continue;
            };
            for (s, _) in requires {
                let cur = arrive.get(s).copied().unwrap_or(f64::INFINITY);
                arrive.insert(*s, cur.min(at));
            }
        }
        // a jump off a lift that's still down waits for it to come back (and everything after
        // the takeoff is that much later)
        for (takeoff, requires) in self.jumps(&path) {
            let Some(&t_off) = self.nav.sector_at(takeoff).and_then(|src| arrive.get(&src)) else {
                continue;
            };
            let mut lifts: HashMap<usize, f64> = HashMap::default();
            for (&(s2, h2), &left) in deadlines {
                if self.facts.temporary.contains(&(s2, h2))
                    && requires.iter().any(|(s, h)| *s == s2 && h.key() != h2)
                {
                    lifts.insert(s2, left);
                }
            }
            if lifts.is_empty() {
                continue;
            }
            let ready = lifts.values().copied().fold(f64::NEG_INFINITY, f64::max) + LIFT_BACK;
            if ready > t_off {
                for (x, a) in arrive.iter_mut() {
                    if *a >= t_off && !lifts.contains_key(x) {
                        *a += ready - t_off;
                    }
                }
            }
        }
        let late: Vec<Pair> = deadlines
            .iter()
            .filter(|(&(s, _), &left)| arrive.get(&s).is_some_and(|&a| a > left))
            .map(|(k, _)| *k)
            .collect();
        if late.is_empty() {
            return Some((path, states.clone()));
        }
        let slower = without(states, late);
        self.nav
            .find_path(start, goal, &mut Planned(&slower), doors, self.strict_fit)
            .map(|p| (p, slower))
    }

    /// The polys reachable from `poly` in this world with these keys (cached).
    pub(super) fn reach_from(
        &mut self,
        poly: usize,
        states: &World,
        keys: KeySet,
    ) -> Arc<HashSet<usize>> {
        let t0 = Instant::now();
        let key = (poly, keys, world_hash(states));
        if let Some(r) = self.caches.reach.get(&key) {
            let r = Arc::clone(r);
            stats::record(Phase::FloodFill, t0);
            return r;
        }
        let doors = self.doors_for(keys);
        let doors = (!doors.is_empty()).then_some(&*doors);
        let r = Arc::new(self.nav.reachable(poly, &mut Planned(states), doors));
        if self.caches.reach.len() > PlanCaches::MAX_REACH {
            self.caches.reach.clear();
        }
        self.caches.reach.insert(key, Arc::clone(&r));
        stats::record(Phase::FloodFill, t0);
        r
    }
}
