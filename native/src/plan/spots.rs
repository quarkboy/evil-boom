// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Where the plan search has the player stand: to fire a trigger, to pick up a key.

use super::world::World;
use super::{Kind, Planner};
use crate::det::HashSet;
use crate::geom::{
    closest_on_portal, closest_point, dist, midpoint, segment_distance, Point, PLAYER_HEIGHT,
    PLAYER_RADIUS,
};
use crate::nav::can_cross;

/// Items' half-width.
const ITEM_RADIUS: f64 = 20.0;
/// Kept off the edge of touching range, for where the player stops.
const PICKUP_MARGIN: f64 = 2.0;
/// How far the use key reaches.
const USE_REACH: i64 = 48;

/// The first of these points farthest from `p` (or nearest, with `nearest`).
fn first_extreme(
    points: impl IntoIterator<Item = Point>,
    p: Point,
    nearest: bool,
) -> Option<Point> {
    points.into_iter().reduce(|best, q| {
        let better = if nearest {
            dist(q, p) < dist(best, p)
        } else {
            dist(q, p) > dist(best, p)
        };
        if better {
            q
        } else {
            best
        }
    })
}

impl Planner<'_> {
    /// A point in `poly` as near `target` as can be, PLAYER_RADIUS + 2 in from its edge (or
    /// its centre if it's that small).
    pub(super) fn spot_in_poly(&self, poly: usize, target: Point) -> Point {
        let inset = PLAYER_RADIUS + 2.0;
        let p = self.nav.mesh.poly(poly);
        let edge_points = p.edges().map(|(a, b)| closest_point(target, a, b));
        let Some(best) = first_extreme(edge_points, target, true) else {
            return p.center;
        };
        let (cx, cy) = p.center;
        let d = dist(best, (cx, cy));
        if d <= inset {
            return p.center;
        }
        (
            best.0 + (cx - best.0) * inset / d,
            best.1 + (cy - best.1) * inset / d,
        )
    }

    /// A spot the box can stand at within use range of the line a-b, nearest it, along the
    /// edges of these polys (moved in by the box): a door can be pressed from a floor below
    /// the one in front of it, where the spot nearest the line's middle is out of range.
    fn press_spot_in_reach(&self, polys: &[usize], a: Point, b: Point) -> Option<Point> {
        let mesh = &self.nav.mesh;
        let reach = USE_REACH as f64 + 12.0;
        let mut best: Option<(f64, Point)> = None;
        for &pi in polys {
            let poly = mesh.poly(pi);
            for (p1, p2) in poly.edges() {
                for k in 0..=8 {
                    let t = f64::from(k) / 8.0;
                    let e = (p1.0 + (p2.0 - p1.0) * t, p1.1 + (p2.1 - p1.1) * t);
                    let d = dist(e, poly.center);
                    if d < 1e-6 {
                        continue;
                    }
                    let f = ((PLAYER_RADIUS + 1.0) / d).min(1.0);
                    let q = (
                        e.0 + (poly.center.0 - e.0) * f,
                        e.1 + (poly.center.1 - e.1) * f,
                    );
                    let r = segment_distance(a, b, q, q);
                    if r > reach || best.is_some_and(|(br, _)| r >= br) {
                        continue;
                    }
                    if mesh.poly_at(q) == Some(pi) && !self.nav.box_blocked_static(q) {
                        best = Some((r, q));
                    }
                }
            }
        }
        best.map(|(_, q)| q)
    }

    /// Where to stand to fire trigger `ti` from one of the polys in `reach`, coming from `pos`.
    fn activation_spot(&self, ti: usize, pos: Point, reach: &HashSet<usize>) -> Point {
        let mesh = &self.nav.mesh;
        let t = &self.facts.triggers[ti];
        let mut inter: Vec<usize> = t
            .activators
            .iter()
            .copied()
            .filter(|p| reach.contains(p))
            .collect();
        inter.sort_unstable();
        let Some(&first) = inter.first().or_else(|| t.activators.iter().min()) else {
            return pos;
        };
        let Some(line) = t.line else {
            return mesh.center(first);
        };
        let (a, b) = self.nav.map.line_seg(line);
        let length = dist(a, b).max(1.0);
        let (mx, my) = midpoint(a, b);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let front = (mx + nx * 20.0, my + ny * 20.0);
        let back = (mx - nx * 20.0, my - ny * 20.0);
        let ok: Vec<Point> = [front, back]
            .into_iter()
            .filter(|&p| mesh.poly_at(p).is_some_and(|q| reach.contains(&q)))
            .collect();
        if t.kind == Kind::Walk {
            // over the line: to its far side
            if let Some(far) = first_extreme(ok.iter().copied(), pos, false) {
                return far;
            }
        }
        if t.kind == Kind::Gun {
            if let Some(near) = first_extreme(inter.iter().map(|&p| mesh.center(p)), pos, true) {
                return near;
            }
        }
        if ok.contains(&front) {
            return front;
        }
        let mut best: Option<(f64, Point)> = None;
        for &pi in &inter {
            let spot = self.spot_in_poly(pi, (mx, my));
            if mesh.poly_at(spot) == Some(pi) {
                let d = dist(spot, closest_point(spot, a, b));
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, spot));
                }
            }
        }
        if let Some((d, spot)) = best {
            if d <= USE_REACH as f64 + 12.0 {
                return spot;
            }
        }
        // (out of reach: somewhere else in those polys that isn't)
        if let Some(spot) = self.press_spot_in_reach(&inter, a, b) {
            return spot;
        }
        best.map_or_else(|| mesh.center(first), |(_, s)| s)
    }

    /// `activation_spot`, but for lines pressed with the use key, stepped back from the line
    /// (within reach) if the box doesn't fit there.
    pub(super) fn activation_point(&self, ti: usize, pos: Point, reach: &HashSet<usize>) -> Point {
        let spot = self.activation_spot(ti, pos, reach);
        let t = &self.facts.triggers[ti];
        let Some(line) = t.line.filter(|_| t.kind.is_used()) else {
            return spot;
        };
        if !self.nav.box_blocked_static(spot) {
            return spot;
        }
        let (a, b) = self.nav.map.line_seg(line);
        let q = closest_point(spot, a, b);
        let d = dist(spot, q);
        if d < 1.0 {
            return spot;
        }
        let (ux, uy) = ((spot.0 - q.0) / d, (spot.1 - q.1) / d);
        ((d as i64 + 4)..USE_REACH + 13)
            .step_by(4)
            .map(|back| (q.0 + ux * back as f64, q.1 + uy * back as f64))
            .find(|&p| {
                self.nav
                    .mesh
                    .poly_at(p)
                    .is_some_and(|poly| reach.contains(&poly))
                    && !self.nav.box_blocked_static(p)
            })
            .unwrap_or(spot)
    }

    /// Polys (and a point in each) from which an item at (x, y) can be picked up: the boxes
    /// only need to overlap, with the item between 8 below and 56 above the player's feet, so
    /// items on ledges too high to step onto can be grabbed from next to them.
    pub(super) fn pickup_spots(&self, x: f64, y: f64, states: &World) -> Vec<(usize, Point)> {
        let (mesh, map) = (&self.nav.mesh, &self.nav.map);
        let Some(home) = mesh.poly_at((x, y)) else {
            return Vec::new();
        };
        let hs = self.nav.poly_sector[home];
        let mut spots = vec![(home, (x, y))];
        let item_floors: Vec<f64> = states[hs].iter().map(|h| h.floor).collect();
        let reach = PLAYER_RADIUS + ITEM_RADIUS - PICKUP_MARGIN;
        for (own, poly) in mesh.live_polys().filter(|(_, p)| p.sector == hs) {
            for &pi in &poly.portals {
                let portal = &mesh.portals[pi];
                let other_i = portal.other(own);
                let other = mesh.poly(other_i);
                if other.sector == hs {
                    continue;
                }
                let near = closest_on_portal((x, y), portal.p1, portal.p2);
                if !item_floors.iter().any(|&fi| {
                    states[other.sector]
                        .iter()
                        .any(|h| -8.0 <= fi - h.floor && fi - h.floor <= PLAYER_HEIGHT)
                }) {
                    continue;
                }
                // far enough back that the box fits on the other side of the line: 17 if the
                // line blocks or the item's side is too high to step onto, else a little past
                let blocks = portal
                    .linedef
                    .is_some_and(|li| map.linedefs[li].blocks_player());
                let steppable = states[other.sector]
                    .iter()
                    .any(|&ho| states[hs].iter().any(|&hh| can_cross(ho, hh)));
                let back = if blocks || !steppable {
                    PLAYER_RADIUS + 1.0
                } else {
                    8.0
                };
                let (cx, cy) = other.center;
                let d = dist(near, (cx, cy)).max(1.0);
                let spot = (
                    near.0 + (cx - near.0) * back / d,
                    near.1 + (cy - near.1) * back / d,
                );
                if (spot.0 - x).abs() < reach
                    && (spot.1 - y).abs() < reach
                    && mesh.poly_at(spot) == Some(other_i)
                {
                    spots.push((other_i, spot));
                }
            }
        }
        spots
    }
}
