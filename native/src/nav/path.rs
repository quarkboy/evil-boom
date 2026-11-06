//! Path-finding: A* over the navmesh's polys, then the path smoothed and fitted to the
//! player's box (`fit`), searched again round the polys where the box got squeezed.

use super::fit::Squeezes;
use super::{Doors, HeightSource, NavCore, Open};
use crate::geom::{closest_on_portal, dist, segment_distance, Point, PLAYER_RADIUS};
use crate::stats::{self, Phase};
use pyo3::prelude::*;
use std::collections::BinaryHeap;
use crate::det::{HashMap, HashSet};
use std::time::Instant;

/// Damaging floors cost this much more to walk over.
const DAMAGE_COST: f64 = 4.0;
/// Searches for a path the box fits along (each round avoiding where the last squeezed).
const FIT_ROUNDS: usize = 3;

/// How a path enters a poly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Portal(usize),
    /// index into the previous poly's teleports and jumps
    Tele(usize),
}

/// A path: the polys it goes through and how each is entered (None for the first), the
/// smoothed waypoints (None where it teleports), its cost, and how many spots along it the
/// box was found squeezed at.
pub struct Found {
    pub polys: Vec<usize>,
    pub waypoints: Vec<Option<Point>>,
    pub cost: f64,
    pub vias: Vec<Option<Via>>,
    pub squeezes: usize,
}

/// The polys of a path and how each is entered.
pub(super) type Chain = [(usize, Option<Via>)];

impl NavCore {
    /// A path from `start` to `goal` with these heights (and these doors openable). If the
    /// box gets squeezed along it, searched again avoiding the polys there; failing that the
    /// least squeezed path (with `squeezes` set), or None with `strict_fit`.
    pub fn find_path(&mut self, py: Python<'_>, start: Point, goal: Point, st: &mut dyn HeightSource, doors: Option<&Doors>, strict_fit: bool) -> PyResult<Option<Found>> {
        let mut best: Option<Found> = None;
        let mut banned: HashSet<usize> = HashSet::default();
        for _ in 0..FIT_ROUNDS {
            let Some((mut path, squeezed)) = self.find_path_once(py, start, goal, st, doors, &banned)? else { break };
            if squeezed.points.is_empty() {
                return Ok(Some(path));
            }
            let new = self.squeezed_polys(&path.polys, &squeezed.points, &banned);
            let n = squeezed.points.len();
            if !squeezed.hard && best.as_ref().is_none_or(|b| n < b.squeezes) {
                path.squeezes = n;
                best = Some(path);
            }
            if new.is_empty() {
                break;
            }
            banned.extend(new);
        }
        Ok(if strict_fit { None } else { best })
    }

    /// The polys along a path (not its ends, not banned already) the box got squeezed at.
    fn squeezed_polys(&self, polys: &[usize], squeezes: &[Point], banned: &HashSet<usize>) -> HashSet<usize> {
        let Some(inner) = polys.get(1..polys.len().saturating_sub(1)) else { return HashSet::default() };
        inner
            .iter()
            .copied()
            .filter(|pi| !banned.contains(pi))
            .filter(|&pi| {
                let poly = self.mesh.poly(pi);
                squeezes.iter().any(|&q| poly.edges().any(|(a, b)| segment_distance(a, b, q, q) <= PLAYER_RADIUS))
            })
            .collect()
    }

    /// A* from `start` to `goal`'s poly (not through `banned`), smoothed and fitted.
    fn find_path_once(&mut self, py: Python<'_>, start: Point, goal: Point, st: &mut dyn HeightSource, doors: Option<&Doors>, banned: &HashSet<usize>) -> PyResult<Option<(Found, Squeezes)>> {
        let (Some(sp), Some(gp)) = (self.mesh.poly_at(start), self.mesh.poly_at(goal)) else { return Ok(None) };
        // (no distance estimate with teleports about: they make it inadmissible)
        let estimate = |me: &Self, poly: usize| if me.has_teleports { 0.0 } else { dist(me.mesh.center(poly), goal) };
        let mut best: HashMap<usize, f64> = HashMap::from_iter([(sp, 0.0)]);
        let mut entry: HashMap<usize, Point> = HashMap::from_iter([(sp, start)]);
        let mut came: HashMap<usize, (usize, Via)> = HashMap::default();
        let t_astar = Instant::now();
        let mut heap = BinaryHeap::from([Open { f: estimate(self, sp), g: 0.0, id: sp }]);
        while let Some(Open { g, id: poly, .. }) = heap.pop() {
            if poly == gp {
                break;
            }
            if best.get(&poly).is_some_and(|&b| g > b) {
                continue;
            }
            let here = entry[&poly];
            let mult = if self.damage[self.poly_sector[poly]] { DAMAGE_COST } else { 1.0 };
            let mut moves: Vec<(usize, Point, Via, f64)> = Vec::new();
            for i in 0..self.adj[poly].len() {
                if self.enterable(self.adj[poly][i].to) && self.edge_passable(py, poly, i, st, doors)? {
                    let e = self.adj[poly][i];
                    let q = &self.mesh.portals[e.portal];
                    moves.push((e.to, closest_on_portal(here, q.p1, q.p2), Via::Portal(e.portal), 0.0));
                }
            }
            for ti in 0..self.tele[poly].len() {
                if self.tele_known(poly, ti) && self.tele_ok(poly, ti, st)? {
                    let t = &self.tele[poly][ti];
                    moves.push((t.dest, t.dest_pt, Via::Tele(ti), t.penalty));
                }
            }
            for (next, point, via, extra) in moves {
                if banned.contains(&next) {
                    continue;
                }
                let cost = g + dist(here, point) * mult + extra;
                if best.get(&next).is_none_or(|&b| cost < b) {
                    best.insert(next, cost);
                    entry.insert(next, point);
                    came.insert(next, (poly, via));
                    heap.push(Open { f: cost + estimate(self, next), g: cost, id: next });
                }
            }
        }
        let Some(&reached) = best.get(&gp) else { return Ok(None) };
        let mut chain: Vec<(usize, Option<Via>)> = Vec::new();
        let mut poly = gp;
        while poly != sp {
            let (prev, via) = came[&poly];
            chain.push((poly, Some(via)));
            poly = prev;
        }
        chain.push((sp, None));
        chain.reverse();
        let cost = reached + dist(entry[&gp], goal);
        stats::record(Phase::AStar, t_astar);
        let (waypoints, squeezes) = self.smooth(start, goal, &chain, st, doors)?;
        let path = Found {
            polys: chain.iter().map(|c| c.0).collect(),
            waypoints,
            cost,
            vias: chain.iter().map(|c| c.1).collect(),
            squeezes: 0,
        };
        Ok(Some((path, squeezes)))
    }
}
