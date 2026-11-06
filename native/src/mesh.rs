//! The navigation mesh: one convex polygon per subsector (from the BSP), cut around solid
//! things, joined by portals where polygons share an edge.

use crate::geom::{cells, Cell, Point};
use crate::map::{MapData, Node, NF_SUBSECTOR};
use crate::det::{HashMap, HashSet};
use std::fmt;

const EPS: f64 = 0.01;

/// An obstacle cut out of the mesh: (x, y, half-width).
pub type Obstacle = (f64, f64, f64);

#[derive(Debug)]
pub enum MeshError {
    /// Subsectors but no nodes: the map was built with an extended node format.
    NoNodes,
    /// A subsector's segs run past the end of the seg list.
    BadSubsector(usize),
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshError::NoNodes => write!(f, "no usable BSP nodes (extended node formats unsupported)"),
            MeshError::BadSubsector(i) => write!(f, "subsector {i} refers to segs that aren't there"),
        }
    }
}

impl std::error::Error for MeshError {}

/// `(p - o) x d`: which side of the line through o along d the point p is on.
#[inline]
fn cross(o: Point, d: Point, p: Point) -> f64 {
    d.0 * (p.1 - o.1) - d.1 * (p.0 - o.0)
}

/// Sutherland-Hodgman: the part of a convex polygon in front of (or behind) the line through
/// `o` along `d`.
fn clip(poly: &[Point], o: Point, d: Point, front: bool) -> Vec<Point> {
    let sign = if front { -1.0 } else { 1.0 };
    let n = poly.len();
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let da = sign * cross(o, d, a);
        let db = sign * cross(o, d, b);
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

fn edges(vs: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    (0..vs.len()).map(move |i| (vs[i], vs[(i + 1) % vs.len()]))
}

fn area(poly: &[Point]) -> f64 {
    edges(poly).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum::<f64>().abs() / 2.0
}

fn centroid(poly: &[Point]) -> Point {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for (p, q) in edges(poly) {
        let c = p.0 * q.1 - q.0 * p.1;
        a += c;
        cx += (p.0 + q.0) * c;
        cy += (p.1 + q.1) * c;
    }
    if a.abs() < 1e-9 {
        let len = poly.len() as f64;
        return (poly.iter().map(|p| p.0).sum::<f64>() / len, poly.iter().map(|p| p.1).sum::<f64>() / len);
    }
    (cx / (3.0 * a), cy / (3.0 * a))
}

fn bounds(vs: &[Point]) -> (f64, f64, f64, f64) {
    vs.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |(x0, x1, y0, y1), p| {
        (x0.min(p.0), x1.max(p.0), y0.min(p.1), y1.max(p.1))
    })
}

/// The overlap of two collinear segments, or None (with some tolerance for nodebuilder
/// rounding).
fn shared_segment(a1: Point, a2: Point, b1: Point, b2: Point) -> Option<(Point, Point)> {
    let tol = 2.0;
    let d = (a2.0 - a1.0, a2.1 - a1.1);
    let length = d.0.hypot(d.1);
    if length < 1e-6 {
        return None;
    }
    if [b1, b2].iter().any(|&p| cross(a1, d, p).abs() / length > tol) {
        return None;
    }
    let (ux, uy) = (d.0 / length, d.1 / length);
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
    let (mut pos, mut neg) = (false, false);
    for (a, b) in edges(vs) {
        let c = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        if c.abs() > EPS {
            if c > 0.0 {
                pos = true;
            } else {
                neg = true;
            }
        }
    }
    !(pos && neg)
}

#[derive(Clone)]
pub struct Poly {
    pub sector: usize,
    pub vertices: Vec<Point>,
    pub center: Point,
    /// linedefs along its edges, by the side of them it's on
    pub lines_front: HashSet<usize>,
    pub lines_back: HashSet<usize>,
    pub portals: Vec<usize>,
}

impl Poly {
    fn new(sector: usize, vertices: Vec<Point>) -> Poly {
        let center = centroid(&vertices);
        Poly { sector, vertices, center, lines_front: HashSet::default(), lines_back: HashSet::default(), portals: Vec::new() }
    }

    pub fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        edges(&self.vertices)
    }
}

/// Where two polys meet: the shared stretch p1-p2, and the two-sided linedef on it if any.
#[derive(Clone)]
pub struct Portal {
    pub a: usize,
    pub b: usize,
    pub p1: Point,
    pub p2: Point,
    pub linedef: Option<usize>,
}

impl Portal {
    pub fn other(&self, poly: usize) -> usize {
        if poly == self.a { self.b } else { self.a }
    }
}

pub struct Mesh {
    /// by subsector (None where it has no area), then the extra pieces obstacles cut
    pub polys: Vec<Option<Poly>>,
    pub portals: Vec<Portal>,
    /// subsector -> the polys it was cut into, for those obstacles cut
    pub pieces: HashMap<usize, Vec<usize>>,
    /// cell -> the linedefs crossing it
    pub line_grid: HashMap<Cell, Vec<usize>>,
    nodes: Vec<Node>,
}

impl Mesh {
    /// The navmesh of map `m`, with `obstacles` cut out of it.
    pub fn build(m: &MapData, obstacles: &[Obstacle]) -> Result<Mesh, MeshError> {
        if m.nodes.is_empty() && m.subsectors.len() > 1 {
            return Err(MeshError::NoNodes);
        }
        let mut line_grid: HashMap<Cell, Vec<usize>> = HashMap::default();
        for li in 0..m.linedefs.len() {
            let (a, b) = m.line_seg(li);
            for c in cells(a.0, a.1, b.0, b.1) {
                line_grid.entry(c).or_default().push(li);
            }
        }
        let mut mesh = Mesh { polys: Vec::new(), portals: Vec::new(), pieces: HashMap::default(), line_grid, nodes: m.nodes.clone() };
        mesh.build_polys(m)?;
        if !obstacles.is_empty() {
            mesh.cut_obstacles(obstacles);
        }
        mesh.attach_lines(m);
        mesh.build_portals(m);
        Ok(mesh)
    }

    /// Poly `i`, which must be one that exists (polys are only ever reached through the
    /// mesh's own lists, which skip the dropped ones).
    pub fn poly(&self, i: usize) -> &Poly {
        self.polys[i].as_ref().expect("a dropped poly was referred to")
    }

    fn poly_mut(&mut self, i: usize) -> &mut Poly {
        self.polys[i].as_mut().expect("a dropped poly was referred to")
    }

    pub fn center(&self, i: usize) -> Point {
        self.poly(i).center
    }

    /// The polys that exist, with their indices.
    /// The polys subsector `ss` became (one, or the pieces obstacles cut it into).
    pub fn subsector_polys(&self, ss: usize) -> Vec<usize> {
        if let Some(group) = self.pieces.get(&ss) {
            return group.clone();
        }
        if self.polys.get(ss).is_some_and(Option::is_some) {
            vec![ss]
        } else {
            Vec::new()
        }
    }

    pub fn live_polys(&self) -> impl Iterator<Item = (usize, &Poly)> {
        self.polys.iter().enumerate().filter_map(|(i, p)| p.as_ref().map(|p| (i, p)))
    }

    fn build_polys(&mut self, m: &MapData) -> Result<(), MeshError> {
        let (minx, maxx, miny, maxy) = bounds(&m.vertexes.iter().map(|v| v.point()).collect::<Vec<_>>());
        let pad = 64.0;
        let world = vec![(minx - pad, miny - pad), (maxx + pad, miny - pad), (maxx + pad, maxy + pad), (minx - pad, maxy + pad)];
        fn descend(m: &MapData, child: u16, poly: Vec<Point>, out: &mut HashMap<usize, Vec<Point>>) {
            if child & NF_SUBSECTOR != 0 {
                out.insert(usize::from(child & !NF_SUBSECTOR), poly);
                return;
            }
            let n = m.nodes[usize::from(child)];
            let (o, d) = ((n.x, n.y), (n.dx, n.dy));
            let front = clip(&poly, o, d, true);
            let back = clip(&poly, o, d, false);
            descend(m, n.front, front, out);
            descend(m, n.back, back, out);
        }
        let mut by_subsector: HashMap<usize, Vec<Point>> = HashMap::default();
        match u16::try_from(m.nodes.len()) {
            Ok(0) => {
                by_subsector.insert(0, world);
            }
            Ok(n) => descend(m, n - 1, world, &mut by_subsector),
            Err(_) => return Err(MeshError::NoNodes),
        }
        for (i, ss) in m.subsectors.iter().enumerate() {
            let segs = m.segs.get(ss.first_seg..ss.first_seg + ss.seg_count).ok_or(MeshError::BadSubsector(i))?;
            let mut poly = by_subsector.remove(&i).unwrap_or_default();
            for seg in segs {
                let (a, b) = (m.vertexes[seg.v1], m.vertexes[seg.v2]);
                if poly.len() >= 3 {
                    poly = clip(&poly, a.point(), (b.x - a.x, b.y - a.y), true);
                }
            }
            let Some(first) = segs.first() else {
                self.polys.push(None);
                continue;
            };
            if poly.len() < 3 || area(&poly) < 1.0 {
                self.polys.push(None);
                continue;
            }
            let line = &m.linedefs[first.linedef];
            let side = if first.direction != 0 { line.back } else { line.front };
            let mut p = Poly::new(m.sidedefs[usize::from(side)].sector, poly);
            for seg in segs {
                if seg.direction != 0 {
                    p.lines_back.insert(seg.linedef);
                } else {
                    p.lines_front.insert(seg.linedef);
                }
            }
            self.polys.push(Some(p));
        }
        Ok(())
    }

    fn cut_obstacles(&mut self, obstacles: &[Obstacle]) {
        /// What's left of a convex polygon outside the box (x0, x1, y0, y1).
        fn outside(vs: &[Point], (x0, x1, y0, y1): (f64, f64, f64, f64)) -> Vec<Vec<Point>> {
            let left = clip(vs, (x0, 0.0), (0.0, 1.0), false);
            let rest = clip(vs, (x0, 0.0), (0.0, 1.0), true);
            let right = clip(&rest, (x1, 0.0), (0.0, 1.0), true);
            let middle = clip(&rest, (x1, 0.0), (0.0, 1.0), false);
            let below = clip(&middle, (0.0, y0), (1.0, 0.0), true);
            let above = clip(&middle, (0.0, y1), (1.0, 0.0), false);
            [left, right, below, above].into_iter().filter(|p| p.len() >= 3 && area(p) >= 1.0).collect()
        }
        let rects: Vec<(f64, f64, f64, f64)> = obstacles.iter().map(|&(x, y, r)| (x - r, x + r, y - r, y + r)).collect();
        for ss in 0..self.polys.len() {
            let Some(poly) = &self.polys[ss] else { continue };
            let sector = poly.sector;
            let (minx, maxx, miny, maxy) = bounds(&poly.vertices);
            let hits: Vec<_> = rects.iter().copied().filter(|b| b.0 < maxx && b.1 > minx && b.2 < maxy && b.3 > miny).collect();
            if hits.is_empty() {
                continue;
            }
            let mut pieces = vec![poly.vertices.clone()];
            for b in hits {
                pieces = pieces.iter().flat_map(|p| outside(p, b)).collect();
            }
            if pieces.is_empty() {
                self.polys[ss] = None;
                self.pieces.insert(ss, Vec::new());
                continue;
            }
            let mut group = vec![ss];
            let mut pieces = pieces.into_iter();
            if let Some(first) = pieces.next() {
                self.polys[ss] = Some(Poly::new(sector, first));
            }
            for p in pieces {
                group.push(self.polys.len());
                self.polys.push(Some(Poly::new(sector, p)));
            }
            self.pieces.insert(ss, group);
        }
    }

    /// Linedefs collinear with and overlapping the segment p-q, ascending.
    fn lines_on_edge(&self, m: &MapData, p: Point, q: Point) -> Vec<usize> {
        let mut found: Vec<usize> = cells(p.0, p.1, q.0, q.1).flat_map(|c| self.line_grid.get(&c).into_iter().flatten().copied()).collect();
        found.sort_unstable();
        found.dedup();
        found.retain(|&li| {
            let (a, b) = m.line_seg(li);
            shared_segment(a, b, p, q).is_some()
        });
        found
    }

    fn attach_lines(&mut self, m: &MapData) {
        for i in 0..self.polys.len() {
            let Some(poly) = &self.polys[i] else { continue };
            let center = poly.center;
            let sides: Vec<(usize, bool)> = poly
                .edges()
                .flat_map(|(p, q)| self.lines_on_edge(m, p, q))
                .map(|li| {
                    let (a, b) = m.line_seg(li);
                    (li, cross(a, (b.0 - a.0, b.1 - a.1), center) < 0.0)
                })
                .collect();
            let poly = self.poly_mut(i);
            for (li, front) in sides {
                if front {
                    poly.lines_front.insert(li);
                } else {
                    poly.lines_back.insert(li);
                }
            }
        }
    }

    fn build_portals(&mut self, m: &MapData) {
        // the grid's cells in the order first filled, so portals are numbered by that
        let mut order: Vec<Cell> = Vec::new();
        let mut grid: HashMap<Cell, Vec<(usize, Point, Point)>> = HashMap::default();
        for (i, poly) in self.live_polys() {
            for (p, q) in poly.edges() {
                if (p.0 - q.0).hypot(p.1 - q.1) < 1.0 {
                    continue;
                }
                for c in cells(p.0, p.1, q.0, q.1) {
                    grid.entry(c)
                        .or_insert_with(|| {
                            order.push(c);
                            Vec::new()
                        })
                        .push((i, p, q));
                }
            }
        }
        let mut seen: HashSet<(usize, usize)> = HashSet::default();
        for c in &order {
            let bucket = &grid[c];
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

    /// The portal(s) between polys a and b along p1-p2: one per two-sided linedef between
    /// their sectors there, else one without a linedef.
    fn add_portal(&mut self, m: &MapData, a: usize, b: usize, p1: Point, p2: Point) {
        let sectors: HashSet<usize> = [self.poly(a).sector, self.poly(b).sector].into_iter().collect();
        let mut pieces: Vec<(Point, Point, Option<usize>)> = Vec::new();
        for li in self.lines_on_edge(m, p1, p2) {
            let l = &m.linedefs[li];
            if !l.two_sided() {
                continue;
            }
            let ls: HashSet<usize> = [m.front_sector(l), m.back_sector(l)].into_iter().flatten().collect();
            if ls == sectors {
                let (v1, v2) = m.line_seg(li);
                if let Some((q1, q2)) = shared_segment(p1, p2, v1, v2) {
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
            self.poly_mut(a).portals.push(idx);
            self.poly_mut(b).portals.push(idx);
        }
    }

    /// The poly at (x, y): walk the BSP like R_PointInSubsector; among obstacle pieces the
    /// one holding the point, else the nearest.
    /// The poly the point is really in (on its floor, not beyond its edges): None for a point
    /// in a wall or off the map (poly_at goes by the BSP, which gives a poly for any point).
    pub fn poly_inside(&self, p: Point) -> Option<usize> {
        let i = self.poly_at(p)?;
        let v = &self.poly(i).vertices;
        let n = v.len();
        // (convex: on the same side of every edge, give or take a unit)
        let side = |k: usize| {
            let (a, b) = (v[k], v[(k + 1) % n]);
            let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-9);
            ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / len
        };
        let sides: Vec<f64> = (0..n).map(side).collect();
        let inside = sides.iter().all(|&d| d >= -1.0) || sides.iter().all(|&d| d <= 1.0);
        inside.then_some(i)
    }

    pub fn poly_at(&self, (x, y): Point) -> Option<usize> {
        let Some(last) = self.nodes.len().checked_sub(1) else {
            return self.polys.first().and_then(|p| p.as_ref()).map(|_| 0);
        };
        let mut child = self.nodes[last].child_toward(x, y);
        while child & NF_SUBSECTOR == 0 {
            child = self.nodes[usize::from(child)].child_toward(x, y);
        }
        let ss = usize::from(child & !NF_SUBSECTOR);
        let Some(group) = self.pieces.get(&ss) else {
            return self.polys.get(ss).and_then(|p| p.as_ref()).map(|_| ss);
        };
        let mut best: Option<(f64, usize)> = None;
        for &i in group {
            let vs = &self.poly(i).vertices;
            if inside(vs, x, y) {
                return Some(i);
            }
            let d = vs.iter().map(|q| (x - q.0).hypot(y - q.1)).fold(f64::INFINITY, f64::min);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }
}
