//! What the player sees: the linedefs the renderer draws from a viewpoint. A line is
//! revealed on the automap (ML_MAPPED) as soon as any part of one of its segs is drawn, so
//! this follows ZDoom's R_RenderBSPNode / R_AddLine / R_ClipWallSegment: segs front to back,
//! projected to screen columns, drawn where no solid wall covers them yet.
//!
//! Only horizontal occlusion counts (walls cover whole columns, windows none), as in the
//! renderer: a step seen through a window is mapped even if it's hidden behind the sill.
//!
//! Also which subsectors are seen into: one is when any of its segs (from either side) is in
//! columns no solid wall covers yet, as the walk reaches it.

use crate::map::{MapData, NF_SUBSECTOR, NO_SIDEDEF};

/// The flat that marks a sky ceiling.
const SKY_FLAT: &str = "F_SKY1";
/// Nearer than this (in view depth) a wall end isn't projected.
const TOO_CLOSE: f64 = 1.0 / 2048.0;

/// A seg as the renderer needs it.
struct ViewSeg {
    v1: (f64, f64),
    v2: (f64, f64),
    line: usize,
    /// the sidedef on the seg's side, and the sector behind it if any
    side: usize,
    front: usize,
    back: Option<usize>,
}

/// The screen columns covered by solid walls so far: sorted, disjoint, inclusive ranges.
struct Covered {
    ranges: Vec<(i32, i32)>,
    width: i32,
}

impl Covered {
    fn new(width: i32) -> Self {
        Covered { ranges: Vec::new(), width }
    }

    fn full(&self) -> bool {
        matches!(self.ranges.as_slice(), [(a, b)] if *a <= 0 && *b >= self.width - 1)
    }

    /// Is any column of first..=last not covered?
    fn any_open(&self, first: i32, last: i32) -> bool {
        let i = self.ranges.partition_point(|&(_, end)| end < first);
        self.ranges.get(i).is_none_or(|&(start, end)| start > first || end < last)
    }

    /// Cover first..=last (merging with ranges it touches).
    fn cover(&mut self, first: i32, last: i32) {
        let start = self.ranges.partition_point(|&(_, end)| end < first.saturating_sub(1));
        let overlaps = self.ranges[start..].partition_point(|&(begin, _)| begin <= last.saturating_add(1));
        if overlaps == 0 {
            self.ranges.insert(start, (first, last));
        } else {
            let end = start + overlaps;
            self.ranges[start] = (first.min(self.ranges[start].0), last.max(self.ranges[end - 1].1));
            self.ranges.drain(start + 1..end);
        }
    }
}

/// The renderer's view of a map: which lines get drawn from where.
pub struct Sight {
    line_count: usize,
    segs: Vec<ViewSeg>,
    /// per subsector: its segs
    subsectors: Vec<std::ops::Range<usize>>,
    nodes: Vec<crate::map::Node>,
    /// per sector: (floor flat, ceiling flat, light)
    looks: Vec<(String, String, i32)>,
    /// per sidedef: (has an upper texture, has a lower texture, has a middle texture)
    textured: Vec<(bool, bool, bool)>,
}

/// What a view shows: the linedefs drawn (each once, in the order first drawn), those in
/// view but not drawn at all (two-sided lines alike on both sides: the floor across them is
/// still seen), and the subsectors seen into.
pub struct Seen {
    pub drawn: Vec<usize>,
    pub through: Vec<usize>,
    pub subsectors: Vec<usize>,
}

/// A view: where from, which way, how many screen columns, and the sectors' heights now.
pub struct View<'a> {
    pub x: f64,
    pub y: f64,
    /// degrees, anticlockwise from east
    pub angle: f64,
    pub width: i32,
    /// per sector: (floor, ceiling)
    pub heights: &'a [(f64, f64)],
}

impl Sight {
    pub fn new(m: &MapData) -> Self {
        let segs = m
            .segs
            .iter()
            .map(|s| {
                let l = &m.linedefs[s.linedef];
                let (side, other) = if s.direction == 0 { (l.front, l.back) } else { (l.back, l.front) };
                let side = usize::from(side);
                ViewSeg {
                    v1: m.vertexes[s.v1].point(),
                    v2: m.vertexes[s.v2].point(),
                    line: s.linedef,
                    side,
                    front: m.sidedefs[side].sector,
                    back: (other != NO_SIDEDEF).then(|| m.sidedefs[usize::from(other)].sector),
                }
            })
            .collect();
        let has = |t: &str| t != "-" && !t.is_empty();
        Sight {
            line_count: m.linedefs.len(),
            segs,
            subsectors: m.subsectors.iter().map(|ss| ss.first_seg..ss.first_seg + ss.seg_count).collect(),
            nodes: m.nodes.clone(),
            looks: m.sectors.iter().map(|s| (s.floor_tex.clone(), s.ceiling_tex.clone(), s.light)).collect(),
            textured: m.sidedefs.iter().map(|sd| (has(&sd.upper), has(&sd.lower), has(&sd.middle))).collect(),
        }
    }

    /// What this view shows.
    pub fn view(&self, view: &View<'_>) -> Seen {
        let (sin, cos) = view.angle.to_radians().sin_cos();
        let mut walk = Walk { sight: self, view, covered: Covered::new(view.width), seen: vec![LineSeen::default(); self.line_count], out: Seen { drawn: Vec::new(), through: Vec::new(), subsectors: Vec::new() }, sin, cos };
        match self.nodes.len().checked_sub(1) {
            None => walk.subsector(0),
            Some(root) => walk.node(u16::try_from(root).unwrap_or(0)),
        }
        walk.out
    }
}

/// A linedef can be reported from either side. Keep the first occurrence in each list.
#[derive(Clone, Copy, Default)]
struct LineSeen {
    drawn: bool,
    through: bool,
}

/// One BSP walk for a view.
struct Walk<'a> {
    sight: &'a Sight,
    view: &'a View<'a>,
    covered: Covered,
    seen: Vec<LineSeen>,
    out: Seen,
    sin: f64,
    cos: f64,
}

impl Walk<'_> {
    fn node(&mut self, child: u16) {
        if self.covered.full() {
            return;
        }
        if child & NF_SUBSECTOR != 0 {
            self.subsector(usize::from(child & !NF_SUBSECTOR));
            return;
        }
        let n = self.sight.nodes[usize::from(child)];
        let near = n.child_toward(self.view.x, self.view.y);
        let far = if near == n.front { n.back } else { n.front };
        self.node(near);
        self.node(far);
    }

    fn subsector(&mut self, ss: usize) {
        let Some(range) = self.sight.subsectors.get(ss).cloned() else { return };
        if range.clone().any(|i| self.open_either_side(i)) {
            self.out.subsectors.push(ss);
        }
        for i in range {
            self.add_line(i);
        }
    }

    /// Is any of seg i in columns not covered yet, whichever side of it we're on?
    fn open_either_side(&self, i: usize) -> bool {
        let s = &self.sight.segs[i];
        let (a, b) = (self.to_view(s.v1), self.to_view(s.v2));
        [(a, b), (b, a)].into_iter().any(|(l, r)| self.columns(l, r).is_some_and(|(x1, x2)| self.covered.any_open(x1, x2 - 1)))
    }

    /// A point in view space: (to the right, depth).
    fn to_view(&self, p: (f64, f64)) -> (f64, f64) {
        let (tx, ty) = (p.0 - self.view.x, p.1 - self.view.y);
        (tx * self.sin - ty * self.cos, tx * self.cos + ty * self.sin)
    }

    /// The screen columns [x1, x2) a wall from l to r (view space, l on the left) covers,
    /// clipped to the 90-degree view (ZDoom's FWallCoords::Init).
    fn columns(&self, (lx, ly): (f64, f64), (rx, ry): (f64, f64)) -> Option<(i32, i32)> {
        let width = f64::from(self.view.width);
        let cx = width / 2.0;
        let (x1, z1) = if lx >= -ly {
            // the left end is in view (or off the right: then all of it is)
            if lx > ly || ly == 0.0 {
                return None;
            }
            ((cx + lx * cx / ly).floor().max(0.0), ly)
        } else {
            // clipped at the left edge of the view
            if rx < -ry {
                return None;
            }
            let den = lx - rx - ry + ly;
            if den == 0.0 {
                return None;
            }
            (0.0, ly + (ry - ly) * (lx + ly) / den)
        };
        if z1 < TOO_CLOSE {
            return None;
        }
        let (x2, z2) = if rx <= ry {
            if rx < -ry || ry == 0.0 {
                return None;
            }
            ((cx + rx * cx / ry).floor().min(width), ry)
        } else {
            // clipped at the right edge
            if lx > ly {
                return None;
            }
            let den = ry - ly - rx + lx;
            if den == 0.0 {
                return None;
            }
            (width, ly + (ry - ly) * (lx - ly) / den)
        };
        if z2 < TOO_CLOSE || x2 <= x1 {
            return None;
        }
        // (whole columns within the screen here)
        Some((x1 as i32, x2 as i32))
    }

    /// Does the seg block the view behind it (a wall, or a closed door), let it through (a
    /// window: heights or looks differ), or is it not drawn at all (an invisible line)?
    fn classify(&self, s: &ViewSeg) -> Option<bool> {
        let Some(back) = s.back else { return Some(true) };
        let hs = self.view.heights;
        let ((ff, fc), (bf, bc)) = (hs[s.front], hs[back]);
        if bc <= ff || bf >= fc {
            return Some(true);
        }
        let (front_look, back_look) = (&self.sight.looks[s.front], &self.sight.looks[back]);
        let (top, bottom, middle) = self.sight.textured[s.side];
        // (a door shut from behind, killough's automap fix: ZDoom's doorclosed)
        if (back_look.1 != SKY_FLAT || front_look.1 != SKY_FLAT) && bc <= bf && (bc >= fc || top) && (bf <= ff || bottom) {
            return Some(true);
        }
        if bf != ff || bc != fc || front_look != back_look || middle {
            return Some(false);
        }
        None
    }

    fn add_line(&mut self, i: usize) {
        let s = &self.sight.segs[i];
        let (dx, dy) = (s.v2.0 - s.v1.0, s.v2.1 - s.v1.1);
        // only the side facing us is drawn
        if dx * (self.view.y - s.v1.1) - dy * (self.view.x - s.v1.0) >= 0.0 {
            return;
        }
        let Some((x1, x2)) = self.columns(self.to_view(s.v1), self.to_view(s.v2)) else { return };
        let solid = self.classify(s);
        if !self.covered.any_open(x1, x2 - 1) {
            return;
        }
        let Some(solid) = solid else {
            if !self.seen[s.line].through {
                self.seen[s.line].through = true;
                self.out.through.push(s.line);
            }
            return;
        };
        if !self.seen[s.line].drawn {
            self.seen[s.line].drawn = true;
            self.out.drawn.push(s.line);
        }
        if solid {
            self.covered.cover(x1, x2 - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Covered;

    #[test]
    fn covered_ranges_merge() {
        let mut c = Covered::new(320);
        c.cover(10, 20);
        c.cover(30, 40);
        assert!(c.any_open(0, 9) && c.any_open(15, 25) && !c.any_open(12, 18));
        c.cover(21, 29);
        assert_eq!(c.ranges, vec![(10, 40)]);
        assert!(!c.any_open(10, 40));
        c.cover(0, 9);
        c.cover(41, 319);
        assert!(c.full());
    }
}
