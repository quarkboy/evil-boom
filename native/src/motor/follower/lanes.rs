//! A way between barrels. The mesh knows walls and solid decorations, not barrels (they can be
//! blown up), so a path by it runs straight through a row of them. For a run out of a field of
//! barrels she needs the lanes there are between them: a search over a fine grid of the ground
//! round the mesh's path, a cell free where her box clears every barrel and stands on floor.
//! (The user's: "it's required to plan a route around the barrel obstacles for this". MAP23's
//! first field: rows of them 64 apart, 12 to spare for her between two, the gaps of one row not
//! in line with the next's; the mesh's path down its east side, 48 between a row's end and the
//! side column, wedged there till the chain came.)

use crate::geom::{dist, Point};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// the grid's cell: fine enough for two cells across the narrowest gap she fits
const CELL: f64 = 4.0;
/// her box and a barrel's side by side (16 and 10), and a little: the game's own test is by
/// the boxes (each of x and y), so this is by each too
pub const ROOM: f64 = 27.0;
/// how far round the mesh's path the lanes are looked for
const MARGIN: f64 = 384.0;
/// no more cells than this looked at
const MOST: usize = 400_000;

/// Is p clear of every barrel, her box against theirs?
pub fn clear_of(barrels: &[Point], p: Point) -> bool {
    barrels.iter().all(|b| (p.0 - b.0).abs() >= ROOM || (p.1 - b.1).abs() >= ROOM)
}

/// Is the straight line from a to b clear of every barrel all along?
pub fn chord_clear_of(barrels: &[Point], a: Point, b: Point) -> bool {
    let d = dist(a, b);
    let n = (d / CELL).ceil().max(1.0) as usize;
    (0..=n).all(|i| {
        let f = i as f64 / n as f64;
        clear_of(barrels, (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f))
    })
}

/// The way from `from` to `to` between `barrels`, near the way `along` (the mesh's): the floor
/// under a point where she can stand there (`floor`: None where she can't), a step between
/// neighbouring cells no more than `step`. The points to walk by, `from` left out; None: no way.
pub fn lane(from: Point, to: Point, along: &[Point], barrels: &[Point], floor: &dyn Fn(Point) -> Option<f64>, step: f64) -> Option<Vec<Point>> {
    let xs = along.iter().chain([&from, &to]).map(|p| p.0);
    let ys = along.iter().chain([&from, &to]).map(|p| p.1);
    let (x0, x1) = (xs.clone().fold(f64::INFINITY, f64::min) - MARGIN, xs.fold(f64::NEG_INFINITY, f64::max) + MARGIN);
    let (y0, y1) = (ys.clone().fold(f64::INFINITY, f64::min) - MARGIN, ys.fold(f64::NEG_INFINITY, f64::max) + MARGIN);
    let (w, h) = (((x1 - x0) / CELL) as i64 + 1, ((y1 - y0) / CELL) as i64 + 1);
    if w <= 0 || h <= 0 || (w * h) as usize > 4 * MOST {
        return None;
    }
    let at = |c: (i64, i64)| (x0 + c.0 as f64 * CELL, y0 + c.1 as f64 * CELL);
    let cell = |p: Point| (((p.0 - x0) / CELL).round() as i64, ((p.1 - y0) / CELL).round() as i64);
    let (start, goal) = (cell(from), cell(to));
    // (the floor at a cell, None where she can't be: looked up once)
    let mut floors: HashMap<(i64, i64), Option<f64>> = HashMap::new();
    let floor_at = |c: (i64, i64), floors: &mut HashMap<(i64, i64), Option<f64>>| {
        if c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
            return None;
        }
        *floors.entry(c).or_insert_with(|| {
            let p = at(c);
            // (where she starts she is, whatever's by her)
            if c != start && !clear_of(barrels, p) {
                return None;
            }
            floor(p)
        })
    };
    floor_at(goal, &mut floors)?;
    let goal_at = at(goal);
    let mut best: HashMap<(i64, i64), f64> = HashMap::from([(start, 0.0)]);
    let mut came: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
    let mut open = BinaryHeap::from([Reverse(((dist(at(start), goal_at) * 16.0) as u64, start))]);
    let mut looked = 0;
    while let Some(Reverse((_, c))) = open.pop() {
        if c == goal {
            let mut cells = vec![c];
            while let Some(&p) = came.get(cells.last()?) {
                cells.push(p);
            }
            cells.reverse();
            let pts: Vec<Point> = std::iter::once(from).chain(cells[1..].iter().map(|&c| at(c))).collect();
            return Some(straightened(&pts, barrels, floor, step));
        }
        looked += 1;
        if looked > MOST {
            return None;
        }
        let (Some(fc), Some(&g)) = (floor_at(c, &mut floors), best.get(&c)) else { continue };
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let n = (c.0 + dx, c.1 + dy);
            let Some(fne) = floor_at(n, &mut floors) else { continue };
            if (fne - fc).abs() > step {
                continue;
            }
            // (aslant only past two free corners)
            if dx != 0 && dy != 0 && (floor_at((c.0 + dx, c.1), &mut floors).is_none() || floor_at((c.0, c.1 + dy), &mut floors).is_none()) {
                continue;
            }
            let k = g + CELL * if dx != 0 && dy != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
            if best.get(&n).is_none_or(|&b| k < b) {
                best.insert(n, k);
                came.insert(n, c);
                open.push(Reverse((((k + dist(at(n), goal_at)) * 16.0) as u64, n)));
            }
        }
    }
    None
}

/// The grid's way with its corners taken out: each point the furthest on that a straight line
/// gets to, clear of barrels and on floor she can walk all along. Without `pts[0]`.
fn straightened(pts: &[Point], barrels: &[Point], floor: &dyn Fn(Point) -> Option<f64>, step: f64) -> Vec<Point> {
    let walkable = |a: Point, b: Point| {
        let n = (dist(a, b) / CELL).ceil().max(1.0) as usize;
        let mut last = floor(a);
        chord_clear_of(barrels, a, b)
            && (1..=n).all(|i| {
                let f = i as f64 / n as f64;
                let here = floor((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f));
                let ok = matches!((last, here), (Some(x), Some(y)) if (x - y).abs() <= step);
                last = here;
                ok
            })
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < pts.len() {
        // (the first stretch from where she stands, beside a barrel maybe: as the grid has it)
        let mut j = i + 1;
        let mut k = (i + 2).min(pts.len() - 1);
        let mut stride = 1;
        while k < pts.len() && k > j && walkable(pts[i], pts[k]) {
            j = k;
            stride *= 2;
            k = (k + stride).min(pts.len() - 1);
            if k == j {
                break;
            }
        }
        // (and back to the furthest that does, between the last that did and the first that didn't)
        while k > j + 1 {
            let mid = (j + k) / 2;
            if walkable(pts[i], pts[mid]) {
                j = mid;
            } else {
                k = mid;
            }
        }
        out.push(pts[j]);
        i = j;
    }
    out
}
