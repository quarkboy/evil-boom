//! Navigation mesh from the map's BSP (evildoom.nav.navmesh), operation for operation so the
//! polygons, portals and their numbering come out identical to the Python version.

use crate::wad::{MapData, NF_SUBSECTOR};
use std::collections::{HashMap, HashSet};

pub type Point = (f64, f64);
const EPS: f64 = 0.01;
pub const GRID: f64 = 128.0;

#[inline]
pub fn cross(ox: f64, oy: f64, dx: f64, dy: f64, px: f64, py: f64) -> f64 {
    dx * (py - oy) - dy * (px - ox)
}

/// Sutherland-Hodgman: keep the part of a convex polygon on one side of a line.
pub fn clip_front(poly: &[Point], ox: f64, oy: f64, dx: f64, dy: f64, front: bool) -> Vec<Point> {
    let sign = if front { -1.0 } else { 1.0 };
    let n = poly.len();
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let da = sign * cross(ox, oy, dx, dy, a.0, a.1);
        let db = sign * cross(ox, oy, dx, dy, b.0, b.1);
        if da >= -EPS {
            out.push(a);
        }
        if (da > EPS && db < -EPS) || (da < -EPS && db > EPS) {
            let t = da / (da - db);
            out.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
        }
    }
    out
}

pub fn area(poly: &[Point]) -> f64 {
    let n = poly.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        s += a.0 * b.1 - b.0 * a.1;
    }
    s.abs() / 2.0
}

pub fn centroid(poly: &[Point]) -> Point {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let c = p.0 * q.1 - q.0 * p.1;
        a += c;
        cx += (p.0 + q.0) * c;
        cy += (p.1 + q.1) * c;
    }
    if a.abs() < 1e-9 {
        let len = n as f64;
        return (poly.iter().map(|p| p.0).sum::<f64>() / len, poly.iter().map(|p| p.1).sum::<f64>() / len);
    }
    (cx / (3.0 * a), cy / (3.0 * a))
}

/// Grid cells covered by a segment's bounding box.
pub fn cells(x0: f64, y0: f64, x1: f64, y1: f64) -> impl Iterator<Item = (i64, i64)> {
    let (x0, x1) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (y0, y1) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    let (gx0, gx1) = ((x0 / GRID).floor() as i64, (x1 / GRID).floor() as i64);
    let (gy0, gy1) = ((y0 / GRID).floor() as i64, (y1 / GRID).floor() as i64);
    (gx0..=gx1).flat_map(move |gx| (gy0..=gy1).map(move |gy| (gx, gy)))
}

/// Overlap of two collinear segments, or None (tolerance for nodebuilder rounding).
pub fn shared_segment(a1: Point, a2: Point, b1: Point, b2: Point) -> Option<(Point, Point)> {
    let tol = 2.0;
    let (dx, dy) = (a2.0 - a1.0, a2.1 - a1.1);
    let length = dx.hypot(dy);
    if length < 1e-6 {
        return None;
    }
    for p in [b1, b2] {
        if cross(a1.0, a1.1, dx, dy, p.0, p.1).abs() / length > tol {
            return None;
        }
    }
    let (ux, uy) = (dx / length, dy / length);
    let t1 = (b1.0 - a1.0) * ux + (b1.1 - a1.1) * uy;
    let t2 = (b2.0 - a1.0) * ux + (b2.1 - a1.1) * uy;
    let lo = 0.0f64.max(t1.min(t2));
    let hi = length.min(t1.max(t2));
    if hi - lo < 1.0 {
        return None;
    }
    Some(((a1.0 + ux * lo, a1.1 + uy * lo), (a1.0 + ux * hi, a1.1 + uy * hi)))
}

/// Is the point inside (or on) the convex polygon?
pub fn inside(vs: &[Point], x: f64, y: f64) -> bool {
    let n = vs.len();
    let (mut pos, mut neg) = (false, false);
    for i in 0..n {
        let (a, b) = (vs[i], vs[(i + 1) % n]);
        let c = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        if c.abs() > EPS {
            if c > 0.0 { pos = true } else { neg = true }
        }
    }
    !(pos && neg)
}

#[derive(Clone)]
pub struct Poly {
    pub sector: usize,
    pub vertices: Vec<Point>,
    pub center: Point,
    pub lines_front: HashSet<usize>,
    pub lines_back: HashSet<usize>,
    pub portals: Vec<usize>,
}

#[derive(Clone)]
pub struct Portal {
    pub a: usize,
    pub b: usize,
    pub p1: Point,
    pub p2: Point,
    pub linedef: Option<usize>,
}

pub struct Mesh {
    pub polys: Vec<Option<Poly>>,
    pub portals: Vec<Portal>,
    pub pieces: HashMap<usize, Vec<usize>>,
    pub line_grid: HashMap<(i64, i64), Vec<usize>>,
    nodes: Vec<(f64, f64, f64, f64, u16, u16)>,
}

impl Mesh {
    /// NavMesh(m, obstacles): `obstacles` are (x, y, half-width) boxes cut out of the mesh.
    pub fn build(m: &MapData, obstacles: &[(f64, f64, f64)]) -> Result<Mesh, String> {
        if m.nodes.is_empty() && m.subsectors.len() > 1 {
            return Err("no usable BSP nodes (extended node formats unsupported)".into());
        }
        let mut line_grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, l) in m.linedefs.iter().enumerate() {
            let (a, b) = (m.vertexes[l.v1], m.vertexes[l.v2]);
            for c in cells(a.x, a.y, b.x, b.y) {
                line_grid.entry(c).or_default().push(i);
            }
        }
        let nodes = m.nodes.iter().map(|n| (n.x, n.y, n.dx, n.dy, n.front, n.back)).collect();
        let mut mesh = Mesh { polys: Vec::new(), portals: Vec::new(), pieces: HashMap::new(), line_grid, nodes };
        mesh.build_polys(m);
        if !obstacles.is_empty() {
            mesh.cut_obstacles(obstacles);
        }
        mesh.attach_lines(m);
        mesh.build_portals(m);
        Ok(mesh)
    }

    fn build_polys(&mut self, m: &MapData) {
        let xs = m.vertexes.iter().map(|v| v.x);
        let ys = m.vertexes.iter().map(|v| v.y);
        let (minx, maxx) = (xs.clone().fold(f64::INFINITY, f64::min), xs.fold(f64::NEG_INFINITY, f64::max));
        let (miny, maxy) = (ys.clone().fold(f64::INFINITY, f64::min), ys.fold(f64::NEG_INFINITY, f64::max));
        let pad = 64.0;
        let bounds = vec![(minx - pad, miny - pad), (maxx + pad, miny - pad), (maxx + pad, maxy + pad), (minx - pad, maxy + pad)];
        let mut cells_by_ss: HashMap<usize, Vec<Point>> = HashMap::new();
        fn descend(m: &MapData, child: u16, poly: Vec<Point>, out: &mut HashMap<usize, Vec<Point>>) {
            if child & NF_SUBSECTOR != 0 {
                out.insert((child & !NF_SUBSECTOR) as usize, poly);
                return;
            }
            let n = m.nodes[child as usize];
            let f = clip_front(&poly, n.x, n.y, n.dx, n.dy, true);
            let b = clip_front(&poly, n.x, n.y, n.dx, n.dy, false);
            descend(m, n.front, f, out);
            descend(m, n.back, b, out);
        }
        if !m.nodes.is_empty() {
            descend(m, (m.nodes.len() - 1) as u16, bounds, &mut cells_by_ss);
        } else {
            cells_by_ss.insert(0, bounds);
        }
        for (i, ss) in m.subsectors.iter().enumerate() {
            let mut poly = cells_by_ss.get(&i).cloned().unwrap_or_default();
            let segs = &m.segs[ss.first_seg..ss.first_seg + ss.seg_count];
            for seg in segs {
                let (a, b) = (m.vertexes[seg.v1], m.vertexes[seg.v2]);
                if poly.len() >= 3 {
                    poly = clip_front(&poly, a.x, a.y, b.x - a.x, b.y - a.y, true);
                }
            }
            if poly.len() < 3 || area(&poly) < 1.0 {
                self.polys.push(None);
                continue;
            }
            let line = &m.linedefs[segs[0].linedef];
            let side = if segs[0].direction != 0 { line.back } else { line.front };
            let center = centroid(&poly);
            let mut p = Poly {
                sector: m.sidedefs[side as usize].sector,
                vertices: poly,
                center,
                lines_front: HashSet::new(),
                lines_back: HashSet::new(),
                portals: Vec::new(),
            };
            for seg in segs {
                if seg.direction != 0 { p.lines_back.insert(seg.linedef); } else { p.lines_front.insert(seg.linedef); }
            }
            self.polys.push(Some(p));
        }
    }

    fn cut_obstacles(&mut self, boxes: &[(f64, f64, f64)]) {
        fn outside(vs: &[Point], b: (f64, f64, f64, f64)) -> Vec<Vec<Point>> {
            let (x0, x1, y0, y1) = b;
            let left = clip_front(vs, x0, 0.0, 0.0, 1.0, false);
            let rest = clip_front(vs, x0, 0.0, 0.0, 1.0, true);
            let right = clip_front(&rest, x1, 0.0, 0.0, 1.0, true);
            let middle = clip_front(&rest, x1, 0.0, 0.0, 1.0, false);
            let below = clip_front(&middle, 0.0, y0, 1.0, 0.0, true);
            let above = clip_front(&middle, 0.0, y1, 1.0, 0.0, false);
            [left, right, below, above].into_iter().filter(|p| p.len() >= 3 && area(p) >= 1.0).collect()
        }
        let rects: Vec<(f64, f64, f64, f64)> = boxes.iter().map(|&(x, y, r)| (x - r, x + r, y - r, y + r)).collect();
        let n = self.polys.len();
        for ss in 0..n {
            let Some(poly) = self.polys[ss].clone() else { continue };
            let xs = poly.vertices.iter().map(|v| v.0);
            let ys = poly.vertices.iter().map(|v| v.1);
            let (minx, maxx) = (xs.clone().fold(f64::INFINITY, f64::min), xs.fold(f64::NEG_INFINITY, f64::max));
            let (miny, maxy) = (ys.clone().fold(f64::INFINITY, f64::min), ys.fold(f64::NEG_INFINITY, f64::max));
            let hits: Vec<_> = rects.iter().copied().filter(|b| b.0 < maxx && b.1 > minx && b.2 < maxy && b.3 > miny).collect();
            if hits.is_empty() {
                continue;
            }
            let mut pieces = vec![poly.vertices.clone()];
            for b in hits {
                pieces = pieces.iter().flat_map(|p| outside(p, b)).collect();
            }
            let mut group = Vec::new();
            for (k, p) in pieces.iter().enumerate() {
                let i = if k == 0 { ss } else { self.polys.len() };
                let piece = Poly {
                    sector: poly.sector,
                    vertices: p.clone(),
                    center: centroid(p),
                    lines_front: HashSet::new(),
                    lines_back: HashSet::new(),
                    portals: Vec::new(),
                };
                if k == 0 { self.polys[ss] = Some(piece) } else { self.polys.push(Some(piece)) }
                group.push(i);
            }
            if pieces.is_empty() {
                self.polys[ss] = None;
            }
            self.pieces.insert(ss, group);
        }
    }

    /// Linedefs collinear with and overlapping the segment p-q, ascending.
    pub fn lines_on_edge(&self, m: &MapData, p: Point, q: Point) -> Vec<usize> {
        let mut cand: Vec<usize> = cells(p.0, p.1, q.0, q.1)
            .flat_map(|c| self.line_grid.get(&c).into_iter().flatten().copied())
            .collect();
        cand.sort_unstable();
        cand.dedup();
        cand.into_iter()
            .filter(|&li| {
                let l = &m.linedefs[li];
                let (a, b) = (m.vertexes[l.v1], m.vertexes[l.v2]);
                shared_segment((a.x, a.y), (b.x, b.y), p, q).is_some()
            })
            .collect()
    }

    fn attach_lines(&mut self, m: &MapData) {
        for i in 0..self.polys.len() {
            let Some(poly) = &self.polys[i] else { continue };
            let vs = poly.vertices.clone();
            let center = poly.center;
            let mut add = Vec::new();
            for k in 0..vs.len() {
                let (p, q) = (vs[k], vs[(k + 1) % vs.len()]);
                for li in self.lines_on_edge(m, p, q) {
                    let l = &m.linedefs[li];
                    let (a, b) = (m.vertexes[l.v1], m.vertexes[l.v2]);
                    add.push((li, cross(a.x, a.y, b.x - a.x, b.y - a.y, center.0, center.1) < 0.0));
                }
            }
            let poly = self.polys[i].as_mut().unwrap();
            for (li, front) in add {
                if front { poly.lines_front.insert(li); } else { poly.lines_back.insert(li); }
            }
        }
    }

    fn build_portals(&mut self, m: &MapData) {
        // insertion-ordered grid (Python dicts keep insertion order)
        let mut order: Vec<(i64, i64)> = Vec::new();
        let mut grid: HashMap<(i64, i64), Vec<(usize, Point, Point)>> = HashMap::new();
        for (i, poly) in self.polys.iter().enumerate() {
            let Some(poly) = poly else { continue };
            let vs = &poly.vertices;
            for k in 0..vs.len() {
                let (p, q) = (vs[k], vs[(k + 1) % vs.len()]);
                if (p.0 - q.0).hypot(p.1 - q.1) < 1.0 {
                    continue;
                }
                for c in cells(p.0, p.1, q.0, q.1) {
                    grid.entry(c).or_insert_with(|| { order.push(c); Vec::new() }).push((i, p, q));
                }
            }
        }
        let mut seen: HashSet<(usize, usize)> = HashSet::new();
        for c in &order {
            let bucket = grid[c].clone();
            for (i, &(pa, a1, a2)) in bucket.iter().enumerate() {
                for &(pb, b1, b2) in &bucket[i + 1..] {
                    let key = (pa.min(pb), pa.max(pb));
                    if pa == pb || seen.contains(&key) {
                        continue;
                    }
                    let Some((o1, o2)) = shared_segment(a1, a2, b1, b2) else { continue };
                    seen.insert(key);
                    self.add_portal(m, pa, pb, o1, o2);
                }
            }
        }
    }

    fn add_portal(&mut self, m: &MapData, a: usize, b: usize, p1: Point, p2: Point) {
        let (sa, sb) = (self.polys[a].as_ref().unwrap().sector, self.polys[b].as_ref().unwrap().sector);
        let sectors: HashSet<usize> = [sa, sb].into_iter().collect();
        let mut pieces: Vec<(Point, Point, Option<usize>)> = Vec::new();
        for li in self.lines_on_edge(m, p1, p2) {
            let l = &m.linedefs[li];
            if !l.two_sided() {
                continue;
            }
            let ls: HashSet<usize> = [m.front_sector(l), m.back_sector(l)].into_iter().flatten().collect();
            if ls == sectors {
                let (v1, v2) = (m.vertexes[l.v1], m.vertexes[l.v2]);
                if let Some((q1, q2)) = shared_segment(p1, p2, (v1.x, v1.y), (v2.x, v2.y)) {
                    pieces.push((q1, q2, Some(li)));
                }
            }
        }
        if pieces.is_empty() {
            pieces.push((p1, p2, None));
        }
        for (q1, q2, linedef) in pieces {
            let idx = self.portals.len();
            self.portals.push(Portal { a, b, p1: q1, p2: q2, linedef });
            self.polys[a].as_mut().unwrap().portals.push(idx);
            self.polys[b].as_mut().unwrap().portals.push(idx);
        }
    }

    /// NavMesh.poly_at: walk the BSP; among obstacle pieces the one holding the point, else
    /// the nearest.
    pub fn poly_at(&self, x: f64, y: f64) -> Option<usize> {
        if self.nodes.is_empty() {
            return if self.polys.first().map_or(false, |p| p.is_some()) { Some(0) } else { None };
        }
        let mut child = (self.nodes.len() - 1) as u16;
        while child & NF_SUBSECTOR == 0 {
            let (nx, ny, ndx, ndy, front, back) = self.nodes[child as usize];
            child = if ndx * (y - ny) - ndy * (x - nx) < 0.0 { front } else { back };
        }
        let ss = (child & !NF_SUBSECTOR) as usize;
        let Some(group) = self.pieces.get(&ss) else {
            return self.polys.get(ss).and_then(|p| p.as_ref().map(|_| ss));
        };
        let mut best: Option<(f64, usize)> = None;
        for &i in group {
            let vs = &self.polys[i].as_ref().unwrap().vertices;
            if inside(vs, x, y) {
                return Some(i);
            }
            let d = vs.iter().map(|q| (x - q.0).hypot(y - q.1)).fold(f64::INFINITY, f64::min);
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }
}
