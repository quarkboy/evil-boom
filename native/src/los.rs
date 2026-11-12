// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Line of sight between two things, as Doom's P_CheckSight works it out (id's, GPL: DSDA-Doom's
//! p_sight.c, GZDoom's p_sight.cpp): a trace from the
//! looker's eyes to the target. A one-sided line blocks, and each two-sided one narrows
//! the window of heights the target can be seen through (its opening
//! seen from the eyes, as slopes over the distance along the trace). The REJECT table is
//! left out (it only ever says no early, and maps rarely lie with it).

use crate::det::HashMap;
use crate::geom::{cells, Cell, Point, GRID};
use crate::map::MapData;

/// A line as the trace needs it.
struct SightLine {
    a: Point,
    b: Point,
    /// (front, back) sectors; no back for a one-sided line
    front: Option<usize>,
    back: Option<usize>,
}

/// One end of a sight check: a thing at (x, y) whose feet are at `z`, `height` tall.
#[derive(Clone, Copy)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub height: f64,
}

pub struct LineOfSight {
    lines: Vec<SightLine>,
    grid: HashMap<Cell, Vec<usize>>,
}

impl LineOfSight {
    pub fn new(m: &MapData) -> Self {
        let lines: Vec<SightLine> = (0..m.linedefs.len())
            .map(|li| {
                let (a, b) = m.line_seg(li);
                let (front, back) = m.sides(li);
                SightLine { a, b, front, back }
            })
            .collect();
        let mut grid: HashMap<Cell, Vec<usize>> = HashMap::default();
        for (li, l) in lines.iter().enumerate() {
            for c in segment_cells(l.a, l.b) {
                let at = grid.entry(c).or_default();
                // Lines arrive in index order, even when a segment revisits this cell.
                if at.last() != Some(&li) {
                    at.push(li);
                }
            }
        }
        LineOfSight { lines, grid }
    }

    /// Can `from` see `to`, with the sectors at `heights` [(floor, ceiling)]? (From its eyes,
    /// three quarters of the way up, as Doom's sightzstart.)
    pub fn clear(&self, from: Body, to: Body, heights: &[(f64, f64)]) -> bool {
        let eye = from.z + from.height * 0.75;
        // the window the target is seen through, as height per whole trace length
        let mut top = to.z + to.height - eye;
        let mut bottom = to.z - eye;
        let (a, b) = ((from.x, from.y), (to.x, to.y));
        // Each opening only narrows the same slope interval. Intersecting the windows in
        // grid order (even visiting a line twice) gives the same answer and lets a wall
        // stop the trace before we visit the remaining cells.
        for &li in segment_cells(a, b)
            .filter_map(|c| self.grid.get(&c))
            .flatten()
        {
            let l = &self.lines[li];
            let Some(frac) = crossing(a, b, l.a, l.b) else {
                continue;
            };
            let (Some(f), Some(k)) = (l.front, l.back) else {
                return false;
            };
            let (Some(&(ff, fc)), Some(&(bf, bc))) = (heights.get(f), heights.get(k)) else {
                return false;
            };
            if ff == bf && fc == bc {
                continue;
            }
            let (open_top, open_bottom) = (fc.min(bc), ff.max(bf));
            if open_bottom >= open_top {
                return false;
            }
            if frac <= 0.0 {
                continue;
            }
            if ff != bf {
                bottom = bottom.max((open_bottom - eye) / frac);
            }
            if fc != bc {
                top = top.min((open_top - eye) / frac);
            }
            if top <= bottom {
                return false;
            }
        }
        true
    }
}

/// Where along a-b (0..1) it crosses the segment c-d, if it does.
fn crossing(a: Point, b: Point, c: Point, d: Point) -> Option<f64> {
    let r = (b.0 - a.0, b.1 - a.1);
    let s = (d.0 - c.0, d.1 - c.1);
    let den = r.0 * s.1 - r.1 * s.0;
    if den.abs() < 1e-12 {
        return None;
    }
    let q = (c.0 - a.0, c.1 - a.1);
    let t = (q.0 * s.1 - q.1 * s.0) / den;
    let u = (q.0 * r.1 - q.1 * r.0) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// The grid cells a segment passes through (and maybe a few beside it): its bounding box a
/// grid step of it at a time.
fn segment_cells(a: Point, b: Point) -> impl Iterator<Item = Cell> {
    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let n = (len / GRID).ceil().max(1.0) as usize;
    (0..n).flat_map(move |i| {
        let t0 = i as f64 / n as f64;
        let t1 = (i + 1) as f64 / n as f64;
        let p = (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0);
        let q = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
        cells(p.0, p.1, q.0, q.1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_finds_the_fraction() {
        let t = crossing((0.0, 0.0), (10.0, 0.0), (5.0, -1.0), (5.0, 1.0));
        assert_eq!(t, Some(0.5));
        assert_eq!(
            crossing((0.0, 0.0), (4.0, 0.0), (5.0, -1.0), (5.0, 1.0)),
            None
        );
    }

    #[test]
    fn openings_share_a_slope_window_in_any_visit_order() {
        let opening = |x, back| SightLine {
            a: (x, 0.0),
            b: (x, 10.0),
            front: Some(0),
            back: Some(back),
        };
        let from = Body {
            x: 0.0,
            y: 5.0,
            z: -3.0,
            height: 4.0,
        }; // eye at zero
        let to = Body {
            x: 100.0,
            y: 5.0,
            z: 0.0,
            height: 100.0,
        };
        for visits in [vec![0, 1], vec![1, 0, 1]] {
            let sight = LineOfSight {
                lines: vec![opening(25.0, 1), opening(75.0, 2)],
                grid: HashMap::from_iter([((0, 0), visits)]),
            };
            // A floor of 10 at 1/4 blocks the bottom 40 of the target. A ceiling
            // of 30 at 3/4 closes the window exactly; raising it leaves a sliver.
            assert!(!sight.clear(from, to, &[(0.0, 100.0), (10.0, 100.0), (0.0, 30.0)]));
            assert!(sight.clear(from, to, &[(0.0, 100.0), (10.0, 100.0), (0.0, 31.0)]));
        }
    }
}
