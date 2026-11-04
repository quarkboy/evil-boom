//! Geometry helpers shared by path-finding (evildoom.nav.reach's module-level functions).

pub type Point = (f64, f64);
pub type Seg = (Point, Point);

pub const PLAYER_RADIUS: f64 = 16.0;
pub const PLAYER_HEIGHT: f64 = 56.0;
pub const MAX_STEP: f64 = 24.0;
pub const PATH_CLEARANCE: f64 = 24.0;
pub const WAYPOINT_CLEARANCE: f64 = 18.0;
pub const THING_CLEARANCE: f64 = 6.0;
pub const BOX_SLACK: f64 = 0.25;
pub const DAMAGE_COST: f64 = 4.0;

#[inline]
pub fn dist(a: Point, b: Point) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// A key for points and segments in hash sets (by value, like Python tuples).
#[inline]
pub fn pkey(p: Point) -> (u64, u64) {
    ((p.0 + 0.0).to_bits(), (p.1 + 0.0).to_bits())
}
#[inline]
pub fn skey(e: Seg) -> (u64, u64, u64, u64) {
    let (a, b) = (pkey(e.0), pkey(e.1));
    (a.0, a.1, b.0, b.1)
}

pub fn segments_cross(p1: Point, p2: Point, q1: Point, q2: Point) -> bool {
    let side = |a: Point, b: Point, c: Point| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let (d1, d2) = (side(q1, q2, p1), side(q1, q2, p2));
    let (d3, d4) = (side(p1, p2, q1), side(p1, p2, q2));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) && d1 != 0.0 && d2 != 0.0 && d3 != 0.0 && d4 != 0.0
}

pub fn closest_point(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-9 {
        return a;
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
    (a.0 + t * dx, a.1 + t * dy)
}

pub fn segment_distance(p1: Point, p2: Point, q1: Point, q2: Point) -> f64 {
    if segments_cross(p1, p2, q1, q2) {
        return 0.0;
    }
    dist(p1, closest_point(p1, q1, q2))
        .min(dist(p2, closest_point(p2, q1, q2)))
        .min(dist(q1, closest_point(q1, p1, p2)))
        .min(dist(q2, closest_point(q2, p1, p2)))
}

pub fn box_clearance(a: Point, b: Point) -> f64 {
    let length = dist(a, b).max(1e-6);
    let (c, s) = ((b.0 - a.0).abs() / length, (b.1 - a.1).abs() / length);
    WAYPOINT_CLEARANCE.max(PLAYER_RADIUS * (c + s) + 1.0)
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
    for &(ux, uy) in &axes[..n] {
        let ext = r * (ux.abs() + uy.abs());
        let (a, b) = (p.0 * ux + p.1 * uy, q.0 * ux + q.1 * uy);
        let (c, d) = (e1.0 * ux + e1.1 * uy, e2.0 * ux + e2.1 * uy);
        if a.max(b) + ext <= c.min(d) || c.max(d) <= a.min(b) - ext {
            return false;
        }
    }
    true
}

pub fn tri(a: Point, b: Point, c: Point) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Simple stupid funnel algorithm; portals are (left, right) in travel order.
pub fn funnel(start: Point, goal: Point, portals: &[(Point, Point)]) -> Vec<Point> {
    let mut ps = Vec::with_capacity(portals.len() + 2);
    ps.push((start, start));
    ps.extend_from_slice(portals);
    ps.push((goal, goal));
    let mut path = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut apex_i, mut left_i, mut right_i) = (0usize, 0usize, 0usize);
    let mut i = 1;
    while i < ps.len() {
        let (pl, pr) = ps[i];
        if tri(apex, right, pr) >= 0.0 {
            if apex == right || tri(apex, left, pr) < 0.0 {
                right = pr;
                right_i = i;
            } else {
                path.push(left);
                apex = left;
                apex_i = left_i;
                left = apex;
                right = apex;
                left_i = apex_i;
                right_i = apex_i;
                i = apex_i + 1;
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
                apex_i = right_i;
                left = apex;
                right = apex;
                left_i = apex_i;
                right_i = apex_i;
                i = apex_i + 1;
                continue;
            }
        }
        i += 1;
    }
    if *path.last().unwrap() != goal {
        path.push(goal);
    }
    path
}

pub fn portal_mid(p1: Point, p2: Point) -> Point {
    ((p1.0 + p2.0) / 2.0, (p1.1 + p2.1) / 2.0)
}

/// Closest point to p on the portal, PATH_CLEARANCE away from its ends if wide enough.
pub fn closest_on_segment(p: Point, p1: Point, p2: Point) -> Point {
    let ((x1, y1), (x2, y2)) = (p1, p2);
    let length = dist(p1, p2);
    if length <= 2.0 * PATH_CLEARANCE {
        return portal_mid(p1, p2);
    }
    let t = ((p.0 - x1) * (x2 - x1) + (p.1 - y1) * (y2 - y1)) / (length * length);
    let margin = PATH_CLEARANCE / length;
    let t = t.max(margin).min(1.0 - margin);
    (x1 + t * (x2 - x1), y1 + t * (y2 - y1))
}

/// (left, right) endpoints walking from one poly to the next, shrunk by PATH_CLEARANCE.
pub fn oriented(p1: Point, p2: Point, from: Point, to: Point) -> (Point, Point) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (left, right) = if dx * (p1.1 - from.1) - dy * (p1.0 - from.0) > 0.0 { (p1, p2) } else { (p2, p1) };
    let length = dist(left, right);
    if length <= 2.0 * PATH_CLEARANCE {
        let m = portal_mid(p1, p2);
        return (m, m);
    }
    let t = PATH_CLEARANCE / length;
    let shrink = |a: Point, b: Point| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    (shrink(left, right), shrink(right, left))
}
