// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Legs of a plan timed against the temporary heights they rely on: a lift that will have
//! gone back up by the time the player gets there can't be used.

use super::triggers::{LegKey, TimedKey};
use super::world::{without, world_hash, Pair, Timers, World, ACTIVATION_TICS, LEG_SPEED};
use super::{KeySet, PlanCaches, Planner};
use crate::det::{HashMap, HashSet};
use crate::geom::{dist, Point, MAX_STEP};
use crate::motor::{creep_pace, CREEP_RANGE, LANDING_ROOM};
use crate::nav::{Doors, Found, HeightSource, Heights, Planned, TeleKind, Via};
use crate::stats::{self, Phase};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Tics a lift takes to come back up to be jumped off again.
const LIFT_BACK: f64 = 60.0;
/// Map units between samples used to estimate arrival at a sector along a path.
const ARRIVAL_SAMPLE_STEP: f64 = 8.0;

/// A world's heights, noting which sectors were asked for and what was answered.
struct Asked<'a> {
    states: &'a World,
    of: Vec<(usize, Rc<Vec<Heights>>)>,
    seen: HashSet<usize>,
}

impl HeightSource for Asked<'_> {
    fn heights(&mut self, sector: usize) -> Rc<Vec<Heights>> {
        let hs = Rc::clone(&self.states[sector]);
        if self.seen.insert(sector) {
            self.of.push((sector, Rc::clone(&hs)));
        }
        hs
    }
}

impl Planner<'_> {
    /// When the player gets to each sector along a path (tics, from 10 for setting off).
    ///
    /// A drop onto a landing narrow the way she's going is crept up to and stepped off
    /// (`creep_to_drop`): the creep and the fall, with no ground made in it, are counted, as
    /// the follower will take them (MAP06: a blazing lift's 4.4 s, a drop of 320 onto 45 of
    /// floor on the way, planned at 3.4 s and walked in 5.4, 23 times).
    fn arrival_tics(&self, path: &Found, states: &World) -> HashMap<usize, f64> {
        let mut out: HashMap<usize, f64> = HashMap::default();
        // (a floor that's one height in this world; a lift's two say nothing of a drop)
        let floor = |s: usize| match states[s].as_slice() {
            [h] => Some(h.floor),
            _ => None,
        };
        let floor_at = |q: Point| self.nav.sector_at(q).and_then(floor);
        let mut last: Option<(usize, Point)> = None;
        let mut crept = 0.0;
        let mut note = |q: Point, d: f64| {
            let Some(s) = self.nav.sector_at(q) else {
                return;
            };
            if let Some((ls, lq)) = last.filter(|(ls, _)| *ls != s) {
                let step = dist(lq, q);
                if let (Some(a), Some(b), true) = (floor(ls), floor(s), step > 0.0) {
                    let dir = ((q.0 - lq.0) / step, (q.1 - lq.1) / step);
                    let room = (1..)
                        .map(|k| f64::from(k) * 8.0)
                        .take_while(|&r| r < LANDING_ROOM)
                        .find(|&r| {
                            floor_at((q.0 + dir.0 * r, q.1 + dir.1 * r))
                                .is_none_or(|f| f < b - MAX_STEP)
                        });
                    if let (Some(room), true) = (room, a - b > MAX_STEP) {
                        crept += CREEP_RANGE / creep_pace(room, a - b) + (2.0 * (a - b)).sqrt()
                            - CREEP_RANGE / LEG_SPEED;
                    }
                }
            }
            last = Some((s, q));
            out.entry(s)
                .or_insert(ACTIVATION_TICS + d / LEG_SPEED + crept);
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
                .or_insert(ACTIVATION_TICS + path.cost / LEG_SPEED + crept);
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
        let out = self.timed_path_inner(start, goal, states, keys, &doors, deadlines);
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
        keys: KeySet,
        doors: &Arc<Doors>,
        deadlines: &Timers,
    ) -> Option<(Found, World)> {
        let doors = (!doors.is_empty()).then_some(&**doors);
        let path = self.leg(start, goal, states, keys, doors)?;
        if deadlines.is_empty() {
            return Some((path, states.clone()));
        }
        let mut arrive = self.arrival_tics(&path, states);
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
            .filter(|&(&(s, _), &left)| arrive.get(&s).is_some_and(|&a| a > left))
            .map(|(k, _)| *k)
            .collect();
        if late.is_empty() {
            return Some((path, states.clone()));
        }
        let slower = without(states, late);
        self.leg(start, goal, &slower, keys, doors)
            .map(|p| (p, slower))
    }

    /// The way from `start` to `goal` in this world (`NavCore::find_path`), kept by the heights
    /// it was found by: the sectors the search asked the heights of, and what it was told. In
    /// another world that has those sectors the same, the search would go the same way step
    /// for step, so its answer's the same. A plan's worlds differ mostly somewhere else in the
    /// level, and the same few legs come up in thousands of them.
    fn leg(
        &mut self,
        start: Point,
        goal: Point,
        states: &World,
        keys: KeySet,
        doors: Option<&Doors>,
    ) -> Option<Found> {
        let key: LegKey = (
            start.0.to_bits(),
            start.1.to_bits(),
            goal.0.to_bits(),
            goal.1.to_bits(),
            keys,
            self.strict_fit,
        );
        if let Some(known) = self.caches.legs.get(&key) {
            let same = known.iter().find(|(asked, _)| {
                asked
                    .iter()
                    .all(|(s, hs)| Rc::ptr_eq(&states[*s], hs) || *states[*s] == **hs)
            });
            if let Some((_, found)) = same {
                return found.clone();
            }
        }
        let mut asked = Asked {
            states,
            of: Vec::new(),
            seen: HashSet::default(),
        };
        let found = self
            .nav
            .find_path(start, goal, &mut asked, doors, self.strict_fit);
        if self.caches.legs_kept > PlanCaches::MAX_LEGS {
            self.caches.legs.clear();
            self.caches.legs_kept = 0;
        }
        self.caches.legs_kept += 1;
        self.caches
            .legs
            .entry(key)
            .or_default()
            .push((asked.of, found.clone()));
        found
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
