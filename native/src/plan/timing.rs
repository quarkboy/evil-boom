//! Legs of a plan timed against the temporary heights they rely on: a lift that will have
//! gone back up by the time the player gets there can't be used.

use super::world::{without, world_hash, Pair, Timers, World, LEG_SPEED};
use super::{KeySet, PlanCaches, Planner};
use crate::geom::{dist, Point};
use crate::nav::{Doors, Found, Heights, Planned, TeleKind, Via};
use crate::stats::{self, Phase};
use pyo3::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

/// Tics a lift takes to come back up to be jumped off again.
const LIFT_BACK: f64 = 60.0;

impl Planner<'_, '_> {
    /// When the player gets to each sector along a path (tics, from 10 for setting off).
    fn arrival_tics(&self, path: &Found) -> HashMap<usize, f64> {
        let mut out: HashMap<usize, f64> = HashMap::new();
        let mut note = |q: Point, d: f64| {
            if let Some(s) = self.nav.sector_at(q) {
                out.entry(s).or_insert(10.0 + d / LEG_SPEED);
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
                    let n = ((seg / 8.0).floor() as i64).max(1);
                    for i in 1..=n {
                        let (fi, nf) = (i as f64, n as f64);
                        note((pr.0 + (p.0 - pr.0) * fi / nf, pr.1 + (p.1 - pr.1) * fi / nf), so_far + seg * fi / nf);
                    }
                    so_far += seg;
                }
            }
            prev = Some(p);
        }
        for &poly in &path.polys {
            out.entry(self.nav.poly_sector[poly]).or_insert(10.0 + path.cost / LEG_SPEED);
        }
        out
    }

    /// The jumps along a path: (takeoff, the sectors and heights each needs).
    fn jumps<'p>(&'p self, path: &'p Found) -> impl Iterator<Item = (Point, &'p [(usize, Heights)])> + 'p {
        path.vias.iter().enumerate().filter_map(move |(i, via)| match via {
            Some(Via::Tele(ti)) => match &self.nav.tele[path.polys[i - 1]][*ti].kind {
                TeleKind::Jump { takeoff, requires, .. } => Some((*takeoff, requires.as_slice())),
                TeleKind::Line { .. } => None,
            },
            _ => None,
        })
    }

    /// A path from `start` to `goal`, not relying on temporary heights (`deadlines`) it would
    /// be too late for; and the heights it was found with.
    pub(super) fn timed_path(&mut self, start: Point, goal: Point, states: &World, doors: &Arc<Doors>, deadlines: &Timers) -> PyResult<Option<(Found, World)>> {
        let t0 = Instant::now();
        let out = self.timed_path_inner(start, goal, states, doors, deadlines);
        stats::record(Phase::TimedPath, t0);
        out
    }

    fn timed_path_inner(&mut self, start: Point, goal: Point, states: &World, doors: &Arc<Doors>, deadlines: &Timers) -> PyResult<Option<(Found, World)>> {
        let doors = (!doors.is_empty()).then_some(&**doors);
        let Some(path) = self.nav.find_path(self.py, start, goal, &mut Planned(states), doors, self.strict_fit)? else { return Ok(None) };
        if deadlines.is_empty() {
            return Ok(Some((path, states.clone())));
        }
        let mut arrive = self.arrival_tics(&path);
        // what a jump needs, it needs when taking off
        for (takeoff, requires) in self.jumps(&path) {
            let Some(&at) = self.nav.sector_at(takeoff).and_then(|src| arrive.get(&src)) else { continue };
            for (s, _) in requires {
                let cur = arrive.get(s).copied().unwrap_or(f64::INFINITY);
                arrive.insert(*s, cur.min(at));
            }
        }
        // a jump off a lift that's still down waits for it to come back (and everything after
        // the takeoff is that much later)
        for (takeoff, requires) in self.jumps(&path) {
            let Some(&t_off) = self.nav.sector_at(takeoff).and_then(|src| arrive.get(&src)) else { continue };
            let mut lifts: HashMap<usize, f64> = HashMap::new();
            for (&(s2, h2), &left) in deadlines {
                if self.facts.temporary.contains(&(s2, h2)) && requires.iter().any(|(s, h)| *s == s2 && h.key() != h2) {
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
        let late: Vec<Pair> = deadlines.iter().filter(|(&(s, _), &left)| arrive.get(&s).is_some_and(|&a| a > left)).map(|(k, _)| *k).collect();
        if late.is_empty() {
            return Ok(Some((path, states.clone())));
        }
        let slower = without(states, late);
        Ok(self.nav.find_path(self.py, start, goal, &mut Planned(&slower), doors, self.strict_fit)?.map(|p| (p, slower)))
    }

    /// The polys reachable from `poly` in this world with these keys (cached).
    pub(super) fn reach_from(&mut self, poly: usize, states: &World, keys: KeySet) -> PyResult<Arc<HashSet<usize>>> {
        let t0 = Instant::now();
        let key = (poly, keys, world_hash(states));
        if let Some(r) = self.caches.reach.get(&key) {
            let r = Arc::clone(r);
            stats::record(Phase::FloodFill, t0);
            return Ok(r);
        }
        let doors = self.doors_for(keys);
        let doors = (!doors.is_empty()).then_some(&*doors);
        let r = Arc::new(self.nav.reachable(self.py, poly, &mut Planned(states), doors)?);
        if self.caches.reach.len() > PlanCaches::MAX_REACH {
            self.caches.reach.clear();
        }
        self.caches.reach.insert(key, Arc::clone(&r));
        stats::record(Phase::FloodFill, t0);
        Ok(r)
    }
}
