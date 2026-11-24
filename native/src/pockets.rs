// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Pockets: the parts of the ground with one narrow way in. A small room off a corridor, an
//! alcove, a closet behind a door: somewhere a few monsters in the mouth shut her in.
//!
//! Over the navmesh's polys as a graph (its portals the edges), the pocket round a poly is
//! found by growing out from it, each time taking in the neighbour that leaves the least edge
//! open to the rest (so a room fills to its corners before the growth goes out by its door),
//! and keeping the stage at which that open edge, the mouth, was narrowest: one portal (a
//! door) or several side by side (an alcove's wide opening the mesh has cut in two). It's a
//! pocket if the mouth is narrow (`MOUTH`), the ground behind it deeper than she is wide for
//! that width (`DEEP`), small (`max_area`), and the lesser part of the ground: the far half of
//! a level beyond its one corridor has one way in too, and is no pocket.
//!
//! Only how the ground joins up, as given: not heights, doors or one-way drops (the caller's
//! polys are those she can walk between).

use crate::geom::{dist, Point};
use crate::mesh::Mesh;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Pocket {
    /// the portals that are its way in
    pub mouth: Vec<usize>,
    /// the middle of the mouth, and how wide it is in all
    pub at: Point,
    pub width: f64,
    pub polys: BTreeSet<usize>,
    /// square units
    pub area: f64,
}

/// a pocket's mouth is no wider than this in all
const MOUTH: f64 = 320.0;
/// a pocket's ground is at least this deep behind its mouth, for the mouth's width (its area
/// over that), and half as deep as the mouth is wide: a room's corner the mesh has cut off is
/// wide for what's behind it, and no pocket
const DEEP: f64 = 64.0;

fn area(vertices: &[Point]) -> f64 {
    let n = vertices.len();
    (0..n)
        .map(|i| {
            let (a, b) = (vertices[i], vertices[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

/// The ground's measures: its polys' areas, the area of the connected piece each is in, and
/// which of the mesh's polys are of it. Made once for a ground and held (`Ground::measured`):
/// the fight asks about one spot at a time, and making them for each spot asked about was a
/// fifth of her thinking on a big map.
#[derive(Clone)]
pub struct Measures {
    areas: BTreeMap<usize, f64>,
    piece: BTreeMap<usize, f64>,
    /// by poly: is it of the ground?
    member: Vec<bool>,
}

impl Measures {
    pub fn new(mesh: &Mesh, polys: &BTreeSet<usize>) -> Measures {
        let areas: BTreeMap<usize, f64> = polys
            .iter()
            .map(|&p| (p, area(&mesh.poly(p).vertices)))
            .collect();
        let mut member = vec![false; polys.last().map_or(0, |&p| p + 1)];
        for &p in polys {
            member[p] = true;
        }
        let mut piece: BTreeMap<usize, f64> = BTreeMap::new();
        for &root in polys {
            if piece.contains_key(&root) {
                continue;
            }
            let (mut seen, mut todo) = (BTreeSet::from([root]), vec![root]);
            while let Some(p) = todo.pop() {
                for &q in &mesh.poly(p).portals {
                    let o = mesh.portals[q].other(p);
                    if member.get(o) == Some(&true) && seen.insert(o) {
                        todo.push(o);
                    }
                }
            }
            let whole: f64 = seen.iter().map(|p| areas[p]).sum();
            piece.extend(seen.into_iter().map(|p| (p, whole)));
        }
        Measures {
            areas,
            piece,
            member,
        }
    }

    fn has(&self, p: usize) -> bool {
        self.member.get(p) == Some(&true)
    }
}

/// A ground (some of the mesh's polys) and its measures.
pub struct Ground<'a> {
    mesh: &'a Mesh,
    polys: &'a BTreeSet<usize>,
    measures: std::borrow::Cow<'a, Measures>,
}

impl<'a> Ground<'a> {
    pub fn new(mesh: &'a Mesh, polys: &'a BTreeSet<usize>) -> Ground<'a> {
        Ground {
            mesh,
            polys,
            measures: std::borrow::Cow::Owned(Measures::new(mesh, polys)),
        }
    }

    /// The ground of `polys`, its measures made already (`Measures::new`, of the same polys).
    pub fn measured(
        mesh: &'a Mesh,
        polys: &'a BTreeSet<usize>,
        measures: &'a Measures,
    ) -> Ground<'a> {
        Ground {
            mesh,
            polys,
            measures: std::borrow::Cow::Borrowed(measures),
        }
    }

    /// The portals of `p` that lead to ground outside `inside`.
    fn out<'b>(
        &'b self,
        p: usize,
        inside: &'b BTreeSet<usize>,
    ) -> impl Iterator<Item = usize> + 'b {
        self.mesh.poly(p).portals.iter().copied().filter(move |&q| {
            let o = self.mesh.portals[q].other(p);
            self.measures.has(o) && !inside.contains(&o)
        })
    }

    fn wide(&self, q: usize) -> f64 {
        let portal = &self.mesh.portals[q];
        dist(portal.p1, portal.p2)
    }

    /// The pocket round poly `p`, no bigger than `max_area`, if it's in one: the biggest such
    /// with the narrowest mouth.
    pub fn around(&self, p: usize, max_area: f64) -> Option<Pocket> {
        let m = &*self.measures;
        let whole = *m.piece.get(&p)?;
        let mut a = m.areas[&p];
        // (the narrowest mouth so far: its width, and how many polys were in then)
        let mut best: Option<(f64, usize)> = None;
        let mut taken = vec![p];
        // What's in, by a flag a poly; of those, the ones with ground outside them still (the
        // edge), in order; and the ground just outside (the next to take in). Kept as the
        // pocket grows: gone through anew at each step, every poly taken in so far was asked
        // about again for each one added, and a pocket of a thousand polys round a spot in
        // an open hall was a third of her thinking on Sunder MAP12. The mouth is summed over
        // the edge in the order it was over the whole inside (what's not on the edge added
        // nothing), so to the same width, to the bit.
        let mut is_in = vec![false; m.member.len()];
        is_in[p] = true;
        let outside = |i: usize, is_in: &[bool]| {
            let mesh = self.mesh;
            mesh.poly(i)
                .portals
                .iter()
                .map(move |&q| (q, mesh.portals[q].other(i)))
                .filter(|&(_, o)| m.has(o) && !is_in[o])
                .collect::<Vec<(usize, usize)>>()
        };
        let mut edge: BTreeSet<usize> = BTreeSet::new();
        let mut next_to: BTreeSet<usize> = BTreeSet::new();
        let first = outside(p, &is_in);
        if !first.is_empty() {
            edge.insert(p);
        }
        next_to.extend(first.into_iter().map(|(_, o)| o));
        loop {
            let open: f64 = edge
                .iter()
                .flat_map(|&i| outside(i, &is_in))
                .map(|(q, _)| self.wide(q))
                .sum();
            if a > max_area || a > whole - a || open <= 0.0 {
                break;
            }
            if open <= MOUTH
                && a / open >= DEEP.max(open / 2.0)
                && best.is_none_or(|(w, _)| open <= w)
            {
                best = Some((open, taken.len()));
            }
            // The neighbour that leaves the least open: what it shares with the inside closes,
            // what else it has opens.
            let next = next_to
                .iter()
                .map(|&o| {
                    let (mut shut, mut opens) = (0.0, 0.0);
                    for &q in &self.mesh.poly(o).portals {
                        let other = self.mesh.portals[q].other(o);
                        if m.member.get(other) == Some(&true) && is_in[other] {
                            shut += self.wide(q);
                        } else if m.has(other) {
                            opens += self.wide(q);
                        }
                    }
                    (opens - shut, o)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            let Some((_, o)) = next else { break };
            is_in[o] = true;
            next_to.remove(&o);
            taken.push(o);
            a += m.areas[&o];
            // (its neighbours inside may have no ground outside them now; those outside are
            // next to the pocket)
            for &q in &self.mesh.poly(o).portals {
                let other = self.mesh.portals[q].other(o);
                if !m.has(other) {
                    continue;
                }
                if is_in[other] {
                    if outside(other, &is_in).is_empty() {
                        edge.remove(&other);
                    }
                } else {
                    next_to.insert(other);
                }
            }
            if !outside(o, &is_in).is_empty() {
                edge.insert(o);
            }
        }
        let (width, n) = best?;
        let polys: BTreeSet<usize> = taken[..n].iter().copied().collect();
        let mouth: Vec<usize> = polys.iter().flat_map(|&i| self.out(i, &polys)).collect();
        let at = mouth.iter().fold((0.0, 0.0), |at, &q| {
            let (portal, w) = (&self.mesh.portals[q], self.wide(q) / width);
            (
                at.0 + (portal.p1.0 + portal.p2.0) / 2.0 * w,
                at.1 + (portal.p1.1 + portal.p2.1) / 2.0 * w,
            )
        });
        Some(Pocket {
            mouth,
            at,
            width,
            area: polys.iter().map(|p| self.measures.areas[p]).sum(),
            polys,
        })
    }

    /// Every pocket of the ground no bigger than `max_area`, the smallest first (one for each
    /// set of polys, whichever of them it was found from).
    pub fn all(&self, max_area: f64) -> Vec<Pocket> {
        let mut found: BTreeMap<BTreeSet<usize>, Pocket> = BTreeMap::new();
        for &p in self.polys {
            if let Some(k) = self.around(p, max_area) {
                found.entry(k.polys.clone()).or_insert(k);
            }
        }
        let mut out: Vec<Pocket> = found.into_values().collect();
        out.sort_by(|a, b| a.area.total_cmp(&b.area).then(a.mouth.cmp(&b.mouth)));
        out
    }
}
