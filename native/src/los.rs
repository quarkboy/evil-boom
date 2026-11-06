//! Line of sight between two things, as ZDoom's P_CheckSight works it out: a trace from the
//! looker's eyes to the target, crossing lines in order; a one-sided line blocks, and each
//! two-sided one narrows the window of heights the target can be seen through (its opening
//! seen from the eyes, as slopes over the distance along the trace). The REJECT table is
//! left out (it only ever says no early, and maps rarely lie with it).

use crate::geom::{cells, Cell, Point, GRID};
use crate::map::MapData;
use crate::det::HashMap;

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
                if at.last() != Some(&li) {
                    at.push(li);
                }
            }
        }
        for at in grid.values_mut() {
            at.sort_unstable();
            at.dedup();
        }
        LineOfSight { lines, grid }
    }

    /// Can `from` see `to`, with the sectors at `heights` [(floor, ceiling)]? (From its eyes,
    /// three quarters of the way up, as ZDoom's sightzstart.)
    pub fn clear(&self, from: Body, to: Body, heights: &[(f64, f64)]) -> bool {
        let eye = from.z + from.height * 0.75;
        // the window the target is seen through, as height per whole trace length
        let mut top = to.z + to.height - eye;
        let mut bottom = to.z - eye;
        let (a, b) = ((from.x, from.y), (to.x, to.y));
        for (frac, li) in self.crossings(a, b) {
            let l = &self.lines[li];
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

    /// The lines the segment a-b crosses, as (how far along, 0..1, line), nearest first.
    fn crossings(&self, a: Point, b: Point) -> Vec<(f64, usize)> {
        let mut candidates: Vec<usize> = segment_cells(a, b)
            .filter_map(|c| self.grid.get(&c))
            .flatten()
            .copied()
            .collect();
        candidates.sort_unstable();
        candidates.dedup();
        let mut out: Vec<(f64, usize)> = candidates
            .into_iter()
            .filter_map(|li| {
                let l = &self.lines[li];
                crossing(a, b, l.a, l.b).map(|t| (t, li))
            })
            .collect();
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        out
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
        assert_eq!(crossing((0.0, 0.0), (4.0, 0.0), (5.0, -1.0), (5.0, 1.0)), None);
    }
}
