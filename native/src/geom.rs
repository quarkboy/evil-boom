// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Plane geometry: points and segments, the player's dimensions, the line grid, and the
//! helpers path smoothing is built from (the funnel, portal clamping, the swept-box test).

/// A map position (x, y).
pub type Point = (f64, f64);
/// A line segment between two points.
pub type Seg = (Point, Point);
/// A point by value, for hash sets and maps (-0.0 and 0.0 are the same key).
pub type PointKey = (u64, u64);
/// A segment by value, for hash sets and maps.
pub type SegKey = (PointKey, PointKey);
/// A cell of the line grid.
pub type Cell = (i64, i64);

pub const PLAYER_RADIUS: f64 = 16.0;
pub const PLAYER_HEIGHT: f64 = 56.0;
/// The highest step the player walks up.
pub const MAX_STEP: f64 = 24.0;
/// How far paths keep from corners and portal ends.
pub const PATH_CLEARANCE: f64 = 24.0;
/// Size of the line grid's cells.
pub const GRID: f64 = 128.0;

/// Does the axis-aligned box of half-width r at (cx, cy) straddle the segment?
pub fn box_hits_segment(cx: f64, cy: f64, r: f64, seg: Seg) -> bool {
    let ((ax, ay), (bx, by)) = seg;
    if ax.max(bx) <= cx - r || ax.min(bx) >= cx + r || ay.max(by) <= cy - r || ay.min(by) >= cy + r
    {
        return false;
    }
    let (dx, dy) = (bx - ax, by - ay);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (px, py) in [
        (cx - r, cy - r),
        (cx + r, cy - r),
        (cx + r, cy + r),
        (cx - r, cy + r),
    ] {
        let s = dx * (py - ay) - dy * (px - ax);
        lo = lo.min(s);
        hi = hi.max(s);
    }
    lo < 0.0 && 0.0 < hi
}

#[inline]
pub fn dist(a: Point, b: Point) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[inline]
pub fn point_key(p: Point) -> PointKey {
    ((p.0 + 0.0).to_bits(), (p.1 + 0.0).to_bits())
}

#[inline]
pub fn seg_key(e: Seg) -> SegKey {
    (point_key(e.0), point_key(e.1))
}

pub fn midpoint(p1: Point, p2: Point) -> Point {
    ((p1.0 + p2.0) / 2.0, (p1.1 + p2.1) / 2.0)
}

/// Twice the signed area of the triangle a, b, c (positive when c is left of a->b).
fn tri(a: Point, b: Point, c: Point) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Do the segments cross properly (not just touch)?
pub fn segments_cross(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    let (d1, d2) = (tri(q1, q2, p1), tri(q1, q2, p2));
    let (d3, d4) = (tri(p1, p2, q1), tri(p1, p2, q2));
    (d1 > 0.0) != (d2 > 0.0)
        && (d3 > 0.0) != (d4 > 0.0)
        && d1 != 0.0
        && d2 != 0.0
        && d3 != 0.0
        && d4 != 0.0
}

/// The point of segment a-b closest to p.
pub fn closest_point(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-9 {
        return a;
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
    (a.0 + t * dx, a.1 + t * dy)
}

/// The distance between two segments (0 if they cross).
pub fn segment_distance(p1: Point, p2: Point, q1: Point, q2: Point) -> f64 {
    if segments_cross(p1, p2, q1, q2) {
        return 0.0;
    }
    dist(p1, closest_point(p1, q1, q2))
        .min(dist(p2, closest_point(p2, q1, q2)))
        .min(dist(q1, closest_point(q1, p1, p2)))
        .min(dist(q2, closest_point(q2, p1, p2)))
}

/// Separating-axis test: does a square of half-width r moved from p to q overlap `edge`?
pub fn swept_box_hits(p: Point, q: Point, r: f64, edge: Seg) -> bool {
    let (e1, e2) = edge;
    let mut axes = [(1.0, 0.0), (0.0, 1.0), (0.0, 0.0), (0.0, 0.0)];
    let mut n = 2;
    for ((x1, y1), (x2, y2)) in [(p, q), (e1, e2)] {
        let (dx, dy) = (x2 - x1, y2 - y1);
        let length = dx.hypot(dy);
        if length > 1e-9 {
            axes[n] = (-dy / length, dx / length);
            n += 1;
        }
    }
    axes[..n].iter().all(|&(ux, uy)| {
        let ext = r * (ux.abs() + uy.abs());
        let (a, b) = (p.0 * ux + p.1 * uy, q.0 * ux + q.1 * uy);
        let (c, d) = (e1.0 * ux + e1.1 * uy, e2.0 * ux + e2.1 * uy);
        !(a.max(b) + ext <= c.min(d) || c.max(d) <= a.min(b) - ext)
    })
}

/// Simple stupid funnel algorithm: the shortest path from `start` to `goal` through the
/// portals, given as (left, right) in travel order.
pub fn funnel(start: Point, goal: Point, portals: &[(Point, Point)]) -> Vec<Point> {
    let mut ps = Vec::with_capacity(portals.len() + 2);
    ps.push((start, start));
    ps.extend_from_slice(portals);
    ps.push((goal, goal));
    let mut path = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_i, mut right_i) = (0usize, 0usize);
    let mut i = 1;
    while i < ps.len() {
        let (pl, pr) = ps[i];
        if tri(apex, right, pr) >= 0.0 {
            if apex == right || tri(apex, left, pr) < 0.0 {
                right = pr;
                right_i = i;
            } else {
                // the right side crossed over the left: the left corner is on the path
                path.push(left);
                apex = left;
                (right, right_i) = (apex, left_i);
                i = left_i + 1;
                continue;
            }
        }
        if tri(apex, left, pl) <= 0.0 {
            if apex == left || tri(apex, right, pl) > 0.0 {
                left = pl;
                left_i = i;
            } else {
                path.push(right);
                apex = right;
                (left, left_i) = (apex, right_i);
                i = right_i + 1;
                continue;
            }
        }
        i += 1;
    }
    if path.last() != Some(&goal) {
        path.push(goal);
    }
    path
}

/// The point of the portal p1-p2 closest to p, PATH_CLEARANCE away from its ends if it's
/// wide enough (else its middle).
pub fn closest_on_portal(p: Point, p1: Point, p2: Point) -> Point {
    let ((x1, y1), (x2, y2)) = (p1, p2);
    let length = dist(p1, p2);
    if length <= 2.0 * PATH_CLEARANCE {
        return midpoint(p1, p2);
    }
    let t = ((p.0 - x1) * (x2 - x1) + (p.1 - y1) * (y2 - y1)) / (length * length);
    let margin = PATH_CLEARANCE / length;
    let t = t.max(margin).min(1.0 - margin);
    (x1 + t * (x2 - x1), y1 + t * (y2 - y1))
}

/// A portal's (left, right) ends crossing it from the side `from` is on (a point in the poly
/// it's crossed from), shrunk by PATH_CLEARANCE. (The side, not the way to the next poly's
/// centre: a long strip's centre can be off to one side, which flips the ends and lets the
/// funnel go straight through walls.)
pub fn oriented(p1: Point, p2: Point, from: Point) -> (Point, Point) {
    let (left, right) = if tri(p1, p2, from) > 0.0 {
        (p2, p1)
    } else {
        (p1, p2)
    };
    let length = dist(left, right);
    if length <= 2.0 * PATH_CLEARANCE {
        let m = midpoint(p1, p2);
        return (m, m);
    }
    let t = PATH_CLEARANCE / length;
    let shrink = |a: Point, b: Point| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    (shrink(left, right), shrink(right, left))
}

/// The grid cells covering the box with corners (x0, y0) and (x1, y1), in either order.
pub fn cells(x0: f64, y0: f64, x1: f64, y1: f64) -> impl Iterator<Item = Cell> {
    let (x0, x1) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (y0, y1) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    let (gx0, gx1) = ((x0 / GRID).floor() as i64, (x1 / GRID).floor() as i64);
    let (gy0, gy1) = ((y0 / GRID).floor() as i64, (y1 / GRID).floor() as i64);
    (gx0..=gx1).flat_map(move |gx| (gy0..=gy1).map(move |gy| (gx, gy)))
}

/// The grid cells covering the bounding box of a-b grown by `pad`.
pub fn cells_around(a: Point, b: Point, pad: f64) -> impl Iterator<Item = Cell> {
    cells(
        a.0.min(b.0) - pad,
        a.1.min(b.1) - pad,
        a.0.max(b.0) + pad,
        a.1.max(b.1) + pad,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn funnel_straight_corridor_is_one_segment() {
        let portals: Vec<_> = [10.0, 20.0, 30.0]
            .iter()
            .map(|&x| ((x, 10.0), (x, -10.0)))
            .collect();
        assert_eq!(
            funnel((0.0, 0.0), (40.0, 0.0), &portals),
            vec![(0.0, 0.0), (40.0, 0.0)]
        );
    }

    #[test]
    fn funnel_bends_around_corner() {
        // the corridor goes +x then turns +y; the inside corner is at (10, 10)
        let portals = [((10.0, 10.0), (10.0, -10.0)), ((10.0, 10.0), (30.0, 10.0))];
        assert_eq!(
            funnel((0.0, 0.0), (20.0, 40.0), &portals),
            vec![(0.0, 0.0), (10.0, 10.0), (20.0, 40.0)]
        );
    }

    #[test]
    fn funnel_without_portals_goes_straight() {
        assert_eq!(
            funnel((0.0, 0.0), (5.0, 5.0), &[]),
            vec![(0.0, 0.0), (5.0, 5.0)]
        );
        assert_eq!(funnel((1.0, 1.0), (1.0, 1.0), &[]), vec![(1.0, 1.0)]);
    }

    #[test]
    fn touching_segments_do_not_cross() {
        assert!(segments_cross(
            (0.0, 0.0),
            (2.0, 2.0),
            (0.0, 2.0),
            (2.0, 0.0)
        ));
        assert!(!segments_cross(
            (0.0, 0.0),
            (1.0, 1.0),
            (1.0, 1.0),
            (2.0, 0.0)
        ));
        assert!(!segments_cross(
            (0.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
            (1.0, 1.0)
        ));
    }

    #[test]
    fn closest_point_clamps_to_the_ends() {
        assert_eq!(
            closest_point((5.0, 3.0), (0.0, 0.0), (10.0, 0.0)),
            (5.0, 0.0)
        );
        assert_eq!(
            closest_point((-4.0, 3.0), (0.0, 0.0), (10.0, 0.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            closest_point((3.0, 3.0), (2.0, 2.0), (2.0, 2.0)),
            (2.0, 2.0)
        );
        assert_eq!(
            segment_distance((0.0, 1.0), (10.0, 1.0), (0.0, -2.0), (10.0, -2.0)),
            3.0
        );
    }

    #[test]
    fn swept_box_hits_edges_it_passes() {
        let wall = ((10.0, -50.0), (10.0, 50.0));
        assert!(swept_box_hits((0.0, 0.0), (20.0, 0.0), 1.0, wall));
        assert!(swept_box_hits((0.0, 0.0), (0.0, 0.0), 11.0, wall));
        assert!(!swept_box_hits((0.0, 0.0), (0.0, 30.0), 9.0, wall));
        // diagonal move past the end of a short edge
        assert!(!swept_box_hits(
            (0.0, 0.0),
            (20.0, 20.0),
            1.0,
            ((15.0, 0.0), (20.0, 0.0))
        ));
    }

    #[test]
    fn narrow_portals_are_crossed_in_the_middle() {
        assert_eq!(
            closest_on_portal((100.0, 0.0), (0.0, 0.0), (40.0, 0.0)),
            (20.0, 0.0)
        );
        assert_eq!(
            closest_on_portal((100.0, 5.0), (0.0, 0.0), (200.0, 0.0)),
            (100.0, 0.0)
        );
        let end = closest_on_portal((0.0, 5.0), (0.0, 0.0), (200.0, 0.0));
        assert!((end.0 - PATH_CLEARANCE).abs() < 1e-9 && end.1 == 0.0);
        // walking +x through a portal along x = 0: the left end is the one at +y
        let (l, r) = oriented((0.0, 0.0), (0.0, 100.0), (-10.0, 50.0));
        assert!(
            dist(l, (0.0, 100.0 - PATH_CLEARANCE)) < 1e-9 && dist(r, (0.0, PATH_CLEARANCE)) < 1e-9
        );
        // ...and walking -x, whichever way the end points are given
        let (l, r) = oriented((0.0, 100.0), (0.0, 0.0), (10.0, -500.0));
        assert!(
            dist(l, (0.0, PATH_CLEARANCE)) < 1e-9 && dist(r, (0.0, 100.0 - PATH_CLEARANCE)) < 1e-9
        );
    }

    #[test]
    fn cells_cover_the_box() {
        let cs: Vec<Cell> = cells(130.0, -1.0, 0.0, 10.0).collect();
        assert_eq!(cs, vec![(0, -1), (0, 0), (1, -1), (1, 0)]);
        assert_eq!(point_key((-0.0, 1.0)), point_key((0.0, 1.0)));
    }
}
