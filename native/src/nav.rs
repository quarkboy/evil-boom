//! The navigator core: flood fill (Navigator.reachable) and path-finding (Navigator.find_path
//! with its smoothing and box fitting), ported step for step from evildoom.nav.reach.

use crate::geom::*;
use crate::mesh::{cells, Mesh};
use crate::wad::MapData;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

pub type Heights = (f64, f64);

#[inline]
pub fn can_cross(a: Heights, b: Heights) -> bool {
    let ((fa, ca), (fb, cb)) = (a, b);
    fb - fa <= MAX_STEP && ca.min(cb) - fa.max(fb) >= PLAYER_HEIGHT
}

pub struct Edge {
    pub portal: usize,
    pub to: usize,
    pub sa: usize,
    pub sb: usize,
    pub linedef: i64,
}

pub enum TeleKind {
    Line { line: usize, front: i64, back: i64 },
    Jump { runup: Point, takeoff: Point, requires: Vec<(usize, Heights)> },
}

pub struct Tele {
    pub dest: usize,
    pub dest_pt: Point,
    pub kind: TeleKind,
}

pub type Doors = HashMap<usize, (Vec<Heights>, HashSet<usize>)>;

/// A sector -> set-of-(floor, ceiling) dict from Python, read lazily and cached.
pub struct States<'py> {
    pub dict: Bound<'py, PyDict>,
    pub cache: HashMap<usize, Vec<Heights>>,
}

impl<'py> States<'py> {
    pub fn new(dict: Bound<'py, PyDict>) -> Self {
        States { dict, cache: HashMap::new() }
    }
    pub fn get(&mut self, s: usize) -> PyResult<&Vec<Heights>> {
        if !self.cache.contains_key(&s) {
            let mut v = Vec::new();
            if let Some(hs) = self.dict.get_item(s)? {
                for h in hs.try_iter()? {
                    v.push(h?.extract::<Heights>()?);
                }
            }
            self.cache.insert(s, v);
        }
        Ok(&self.cache[&s])
    }
}

pub fn read_doors(d: &Bound<'_, PyDict>) -> PyResult<Doors> {
    let mut out = HashMap::new();
    for (k, v) in d.iter() {
        let (open, users): (Bound<'_, PyAny>, Bound<'_, PyAny>) = v.extract()?;
        let mut o = Vec::new();
        for h in open.try_iter()? {
            o.push(h?.extract::<Heights>()?);
        }
        let mut u = HashSet::new();
        for p in users.try_iter()? {
            u.insert(p?.extract::<usize>()?);
        }
        out.insert(k.extract::<usize>()?, (o, u));
    }
    Ok(out)
}

/// What a found path is made of: polys, waypoints (None = teleport), cost, how each poly was
/// entered (None, ("portal", i) or ("tele", index in the poly before's tele list)), squeezes.
pub struct Found {
    pub polys: Vec<usize>,
    pub waypoints: Vec<Option<Point>>,
    pub cost: f64,
    pub vias: Vec<Option<(bool, usize)>>, // (is tele, index)
    pub squeezes: usize,
}

#[derive(PartialEq)]
struct HeapItem(f64, f64, usize);
impl Eq for HeapItem {}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for HeapItem {
    // min-heap on (f, g, poly) like Python's heapq of tuples
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
            .then_with(|| o.1.total_cmp(&self.1))
            .then_with(|| o.2.cmp(&self.2))
    }
}

pub struct NavCore {
    pub map: MapData,
    pub mesh: Mesh,
    pub poly_sector: Vec<usize>,
    pub adj: Vec<Vec<Edge>>,
    pub gates: HashMap<usize, Vec<(i64, Vec<usize>)>>,
    pub tele: Vec<Vec<Tele>>,
    pub fits_cb: PyObject,
    pub landable_cb: PyObject,
    pub fits_cache: HashMap<(usize, Vec<i64>), bool>,
    pub landable_cache: HashMap<(i64, usize, i64), bool>,
    // path-finding facts
    pub initial: Vec<Heights>,
    pub damage: Vec<bool>,
    pub has_teleports: bool,
    pub walls: HashSet<usize>,
    pub drops: HashSet<usize>,
    pub teleport_lines: HashSet<usize>,
    pub shutting: HashSet<usize>,
    pub static_grid: HashMap<(i64, i64), Vec<Seg>>,
    pub obstacle_grid: HashMap<(i64, i64), Vec<Seg>>,
    pub obstacle_order: Vec<(i64, i64)>,
}

impl NavCore {
    fn line_seg(&self, li: usize) -> Seg {
        let l = &self.map.linedefs[li];
        let (a, b) = (self.map.vertexes[l.v1], self.map.vertexes[l.v2]);
        ((a.x, a.y), (b.x, b.y))
    }

    fn sector_at(&self, p: Point) -> Option<usize> {
        self.mesh.poly_at(p.0, p.1).map(|i| self.poly_sector[i])
    }

    // --- passability ------------------------------------------------------------------------

    fn heights(states: &mut States<'_>, doors: &Option<Doors>, s: usize, frm: Option<usize>) -> PyResult<Vec<Heights>> {
        let mut out = states.get(s)?.clone();
        if let Some(d) = doors {
            if let Some((open, users)) = d.get(&s) {
                if frm.map_or(true, |p| users.contains(&p)) {
                    out.extend_from_slice(open);
                }
            }
        }
        Ok(out)
    }

    fn landable(&mut self, py: Python<'_>, li: i64, high: usize, top: f64) -> PyResult<bool> {
        let key = (li, high, (top * 16.0).round() as i64);
        if let Some(&v) = self.landable_cache.get(&key) {
            return Ok(v);
        }
        let v: bool = self.landable_cb.call1(py, (li, high, top))?.extract(py)?;
        self.landable_cache.insert(key, v);
        Ok(v)
    }

    fn fits(&mut self, py: Python<'_>, portal: usize, mut blocking: Vec<i64>) -> PyResult<bool> {
        if blocking.is_empty() {
            return Ok(true);
        }
        blocking.sort_unstable();
        let key = (portal, blocking);
        if let Some(&v) = self.fits_cache.get(&key) {
            return Ok(v);
        }
        let v: bool = self.fits_cb.call1(py, (portal, key.1.clone()))?.extract(py)?;
        self.fits_cache.insert(key, v);
        Ok(v)
    }

    fn fits_now(&mut self, py: Python<'_>, portal: usize, frm: usize, st: &mut States<'_>, doors: &Option<Doors>) -> PyResult<bool> {
        let Some(gates) = self.gates.get(&portal) else { return Ok(true) };
        let gates = gates.clone();
        let sector = self.poly_sector[frm];
        let here = Self::heights(st, doors, sector, None)?;
        let mut blocking = Vec::new();
        for (li, sectors) in &gates {
            for &s in sectors {
                if s == sector {
                    continue;
                }
                let hs = Self::heights(st, doors, s, Some(frm))?;
                if !here.iter().any(|&ha| hs.iter().any(|&hb| can_cross(ha, hb))) {
                    blocking.push(*li);
                    break;
                }
            }
        }
        self.fits(py, portal, blocking)
    }

    fn heights_passable(&mut self, py: Python<'_>, sa: usize, sb: usize, linedef: i64, frm: usize, st: &mut States<'_>, doors: &Option<Doors>) -> PyResult<bool> {
        let hs_a = Self::heights(st, doors, sa, None)?;
        let hs_b = Self::heights(st, doors, sb, Some(frm))?;
        for &ha in &hs_a {
            for &hb in &hs_b {
                if can_cross(ha, hb) && (ha.0 - hb.0 <= MAX_STEP || linedef < 0 || self.landable(py, linedef, sa, ha.0)?) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Edge `i` of `poly`'s adjacency crossable now?
    fn edge_passable(&mut self, py: Python<'_>, poly: usize, i: usize, st: &mut States<'_>, doors: &Option<Doors>) -> PyResult<bool> {
        let (portal, sa, sb, linedef) = {
            let e = &self.adj[poly][i];
            (e.portal, e.sa, e.sb, e.linedef)
        };
        if self.gates.contains_key(&portal) && !self.fits_now(py, portal, poly, st, doors)? {
            return Ok(false);
        }
        Ok(sa == sb || self.heights_passable(py, sa, sb, linedef, poly, st, doors)?)
    }

    fn tele_ok(&self, poly: usize, ti: usize, st: &mut States<'_>) -> PyResult<bool> {
        match &self.tele[poly][ti].kind {
            TeleKind::Jump { requires, .. } => {
                for (s, h) in requires.clone() {
                    if !st.get(s)?.iter().any(|x| *x == h) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            TeleKind::Line { front, back, .. } => {
                let (front, back) = (*front, *back);
                if front < 0 || back < 0 {
                    return Ok(false);
                }
                let fs = st.get(front as usize)?.clone();
                let bs = st.get(back as usize)?;
                Ok(fs.iter().any(|&fh| bs.iter().any(|&bh| can_cross(fh, bh))))
            }
        }
    }

    pub fn reachable(&mut self, py: Python<'_>, start: usize, st: &mut States<'_>, doors: &Option<Doors>) -> PyResult<HashSet<usize>> {
        let mut seen: HashSet<usize> = HashSet::new();
        seen.insert(start);
        let mut todo = vec![start];
        while let Some(poly) = todo.pop() {
            for i in 0..self.adj[poly].len() {
                let to = self.adj[poly][i].to;
                if seen.contains(&to) || !self.edge_passable(py, poly, i, st, doors)? {
                    continue;
                }
                seen.insert(to);
                todo.push(to);
            }
            for ti in 0..self.tele[poly].len() {
                let dest = self.tele[poly][ti].dest;
                if !seen.contains(&dest) && self.tele_ok(poly, ti, st)? {
                    seen.insert(dest);
                    todo.push(dest);
                }
            }
        }
        Ok(seen)
    }

    // --- path-finding -------------------------------------------------------------------------

    pub fn find_path(&mut self, py: Python<'_>, start: Point, goal: Point, st: &mut States<'_>, doors: &Option<Doors>, strict_fit: bool) -> PyResult<Option<Found>> {
        let mut best: Option<Found> = None;
        let mut banned: HashSet<usize> = HashSet::new();
        for _ in 0..3 {
            let mut squeezes: Vec<Point> = Vec::new();
            let Some(mut path) = self.find_path_once(py, start, goal, st, doors, &banned, &mut squeezes)? else { break };
            if squeezes.is_empty() {
                return Ok(Some(path));
            }
            let n = squeezes.len();
            let polys = path.polys.clone();
            if best.as_ref().map_or(true, |b| n < b.squeezes) {
                path.squeezes = n;
                best = Some(path);
            }
            let mut new = HashSet::new();
            if polys.len() >= 2 {
                for &pi in &polys[1..polys.len() - 1] {
                    let vs = &self.mesh.polys[pi].as_ref().unwrap().vertices;
                    let hit = squeezes.iter().any(|&q| (0..vs.len()).any(|i| segment_distance(vs[i], vs[(i + 1) % vs.len()], q, q) <= PLAYER_RADIUS));
                    if hit && !banned.contains(&pi) {
                        new.insert(pi);
                    }
                }
            }
            if new.is_empty() {
                break;
            }
            banned.extend(new);
        }
        if strict_fit {
            return Ok(None);
        }
        Ok(best)
    }

    fn find_path_once(&mut self, py: Python<'_>, start: Point, goal: Point, st: &mut States<'_>, doors: &Option<Doors>, banned: &HashSet<usize>, squeezes: &mut Vec<Point>) -> PyResult<Option<Found>> {
        let (Some(sp), Some(gp)) = (self.mesh.poly_at(start.0, start.1), self.mesh.poly_at(goal.0, goal.1)) else { return Ok(None) };
        let use_h = !self.has_teleports;
        let h = |me: &Self, poly: usize| if use_h { dist(me.mesh.polys[poly].as_ref().unwrap().center, goal) } else { 0.0 };
        let mut best: HashMap<usize, f64> = HashMap::new();
        let mut entry: HashMap<usize, Point> = HashMap::new();
        let mut came: HashMap<usize, (usize, (bool, usize))> = HashMap::new();
        best.insert(sp, 0.0);
        entry.insert(sp, start);
        let mut heap = BinaryHeap::new();
        heap.push(HeapItem(h(self, sp), 0.0, sp));
        while let Some(HeapItem(_, g, poly)) = heap.pop() {
            if poly == gp {
                break;
            }
            if g > best[&poly] {
                continue;
            }
            let here = entry[&poly];
            let mult = if self.damage[self.poly_sector[poly]] { DAMAGE_COST } else { 1.0 };
            let mut moves: Vec<(usize, Point, (bool, usize))> = Vec::new();
            for i in 0..self.adj[poly].len() {
                if self.edge_passable(py, poly, i, st, doors)? {
                    let (pi, to) = (self.adj[poly][i].portal, self.adj[poly][i].to);
                    let q = &self.mesh.portals[pi];
                    moves.push((to, closest_on_segment(here, q.p1, q.p2), (false, pi)));
                }
            }
            for ti in 0..self.tele[poly].len() {
                if self.tele_ok(poly, ti, st)? {
                    let t = &self.tele[poly][ti];
                    moves.push((t.dest, t.dest_pt, (true, ti)));
                }
            }
            for (nxt, point, via) in moves {
                if banned.contains(&nxt) {
                    continue;
                }
                let cost = g + dist(here, point) * mult;
                if best.get(&nxt).map_or(true, |&b| cost < b) {
                    best.insert(nxt, cost);
                    entry.insert(nxt, point);
                    came.insert(nxt, (poly, via));
                    heap.push(HeapItem(cost + h(self, nxt), cost, nxt));
                }
            }
        }
        if !best.contains_key(&gp) {
            return Ok(None);
        }
        // chain of (poly, via into it)
        let mut chain: Vec<(usize, Option<(bool, usize)>)> = vec![(gp, None)];
        while chain.last().unwrap().0 != sp {
            let cur = chain.last().unwrap().0;
            let (prev, via) = came[&cur];
            chain.last_mut().unwrap().1 = Some(via);
            chain.push((prev, None));
        }
        chain.reverse();
        let cost = best[&gp] + dist(entry[&gp], goal);
        let points = self.smooth(py, start, goal, &chain, st, squeezes)?;
        Ok(Some(Found {
            polys: chain.iter().map(|c| c.0).collect(),
            waypoints: points,
            cost,
            vias: chain.iter().map(|c| c.1).collect(),
            squeezes: 0,
        }))
    }

    fn tele_of(&self, chain: &[(usize, Option<(bool, usize)>)], i: usize) -> Option<&Tele> {
        match chain[i].1 {
            Some((true, ti)) => Some(&self.tele[chain[i - 1].0][ti]),
            _ => None,
        }
    }

    fn smooth(&mut self, _py: Python<'_>, start: Point, goal: Point, chain: &[(usize, Option<(bool, usize)>)], st: &mut States<'_>, squeezes: &mut Vec<Point>) -> PyResult<Vec<Option<Point>>> {
        let mut points: Vec<Option<Point>> = Vec::new();
        let mut seg_start = start;
        let mut portals: Vec<(Point, Point)> = Vec::new();
        for i in 1..chain.len() {
            let prev_poly = chain[i - 1].0;
            let poly = chain[i].0;
            if let Some(t) = self.tele_of(chain, i) {
                match &t.kind {
                    TeleKind::Jump { runup, takeoff, .. } => {
                        points.extend(funnel(seg_start, *runup, &portals).into_iter().map(Some));
                        points.push(Some(*takeoff));
                        seg_start = t.dest_pt;
                        portals.clear();
                    }
                    TeleKind::Line { line, .. } => {
                        let (a, b) = self.line_seg(*line);
                        let mid = portal_mid(a, b);
                        let length = dist(a, b).max(1.0);
                        let approach = (mid.0 + (b.1 - a.1) / length * 24.0, mid.1 - (b.0 - a.0) / length * 24.0);
                        points.extend(funnel(seg_start, approach, &portals).into_iter().map(Some));
                        points.push(Some(mid));
                        points.push(None);
                        seg_start = t.dest_pt;
                        portals.clear();
                    }
                }
                continue;
            }
            if let Some((false, pi)) = chain[i].1 {
                let q = &self.mesh.portals[pi];
                let fc = self.mesh.polys[prev_poly].as_ref().unwrap().center;
                let tc = self.mesh.polys[poly].as_ref().unwrap().center;
                portals.push(oriented(q.p1, q.p2, fc, tc));
            }
        }
        points.extend(funnel(seg_start, goal, &portals).into_iter().map(Some));
        let mut crossed: HashSet<usize> = HashSet::new();
        for (i, c) in chain.iter().enumerate() {
            match c.1 {
                Some((false, pi)) => {
                    if let Some(li) = self.mesh.portals[pi].linedef {
                        crossed.insert(li);
                    }
                }
                Some((true, _)) => {
                    if let Some(Tele { kind: TeleKind::Line { line, .. }, .. }) = self.tele_of(chain, i) {
                        crossed.insert(*line);
                    }
                }
                None => {}
            }
        }
        for li in &self.shutting {
            crossed.remove(li);
        }
        let mut pinned: HashSet<(u64, u64)> = HashSet::new();
        for i in 1..chain.len() {
            if let Some(Tele { kind: TeleKind::Jump { runup, takeoff, .. }, .. }) = self.tele_of(chain, i) {
                pinned.insert(pkey(*runup));
                pinned.insert(pkey(*takeoff));
            }
        }
        let sectors: HashSet<usize> = chain.iter().map(|c| self.poly_sector[c.0]).collect();
        let mut out: Vec<Option<Point>> = Vec::new();
        let mut run: Vec<Point> = Vec::new();
        points.push(None);
        for p in points {
            match p {
                None => {
                    if !run.is_empty() {
                        let a = self.push_from_corners(&run, &crossed, &pinned, st)?;
                        let b = self.box_fits_path(&a, &crossed, &pinned, st, squeezes)?;
                        let c = self.straighten(&b, &crossed, &pinned, Some(&sectors), st)?;
                        out.extend(c.into_iter().map(Some));
                        run.clear();
                    }
                    out.push(None);
                }
                Some(p) => run.push(p),
            }
        }
        out.pop();
        Ok(out)
    }

    // --- box fitting -------------------------------------------------------------------------

    /// Navigator._static_edges(a, b, pad, exempt) at the planned heights.
    fn static_edges(&self, a: Point, b: Point, pad: f64, exempt: Option<&HashSet<usize>>, st: &mut States<'_>) -> PyResult<Vec<Seg>> {
        let cs: Vec<(i64, i64)> = cells(a.0.min(b.0) - pad, a.1.min(b.1) - pad, a.0.max(b.0) + pad, a.1.max(b.1) + pad).collect();
        let mut seen_e: HashSet<(u64, u64, u64, u64)> = HashSet::new();
        let mut out = Vec::new();
        for c in &cs {
            if let Some(es) = self.static_grid.get(c) {
                for &e in es {
                    if seen_e.insert(skey(e)) {
                        out.push(e);
                    }
                }
            }
        }
        let Some(exempt) = exempt else { return Ok(out) };
        let here = self.mesh.poly_at(a.0, a.1);
        let ours: Option<f64> = match here {
            None => None,
            Some(hp) => {
                let hs = self.poly_sector[hp];
                let reach_up = st.get(hs)?.iter().map(|h| h.0).fold(f64::NEG_INFINITY, f64::max) + MAX_STEP;
                let mut grid_f: HashMap<(i32, i32), f64> = HashMap::new();
                let mut order = Vec::new();
                for i in [-1i32, 0, 1] {
                    for j in [-1i32, 0, 1] {
                        let f = match self.sector_at((a.0 + i as f64 * PLAYER_RADIUS, a.1 + j as f64 * PLAYER_RADIUS)) {
                            Some(s) => st.get(s)?.iter().map(|h| h.0).fold(f64::NEG_INFINITY, f64::max),
                            None => f64::NEG_INFINITY,
                        };
                        grid_f.insert((i, j), f);
                        order.push((i, j));
                    }
                }
                let high: HashSet<(i32, i32)> = grid_f.iter().filter(|(_, &f)| f > reach_up).map(|(&c, _)| c).collect();
                let wedged = [-1i32, 0, 1].iter().any(|&k| {
                    (high.contains(&(-1, k)) && high.contains(&(1, k)) && !high.contains(&(0, k)))
                        || (high.contains(&(k, -1)) && high.contains(&(k, 1)) && !high.contains(&(k, 0)))
                });
                order.iter().map(|c| grid_f[c]).filter(|&f| f > f64::NEG_INFINITY && (!wedged || f <= reach_up)).fold(None, |acc: Option<f64>, f| Some(acc.map_or(f, |a| a.max(f))))
            }
        };
        let mut seen_l: HashSet<usize> = HashSet::new();
        for c in &cs {
            let Some(lis) = self.mesh.line_grid.get(c) else { continue };
            for &li in lis {
                if seen_l.contains(&li) || exempt.contains(&li) {
                    continue;
                }
                let l = &self.map.linedefs[li];
                let (f, bk) = (self.map.front_sector(l), self.map.back_sector(l));
                if self.teleport_lines.contains(&li) {
                    seen_l.insert(li);
                    out.push(self.line_seg(li));
                    continue;
                }
                if !self.walls.contains(&li) && !self.drops.contains(&li) {
                    let (Some(f), Some(bk), Some(ours)) = (f, bk, ours) else { continue };
                    let fs = st.get(f)?.clone();
                    let bs = st.get(bk)?.clone();
                    let mut top = f64::NEG_INFINITY;
                    for hf in &fs {
                        for hb in &bs {
                            top = top.max(hf.1.min(hb.1));
                        }
                    }
                    if top - ours >= PLAYER_HEIGHT {
                        continue;
                    }
                    seen_l.insert(li);
                    out.push(self.line_seg(li));
                    continue;
                }
                seen_l.insert(li);
                if self.walls.contains(&li) {
                    if let (Some(f), Some(bk)) = (f, bk) {
                        let fs = st.get(f)?.clone();
                        let bs = st.get(bk)?.clone();
                        if fs.iter().any(|&hf| bs.iter().any(|&hb| can_cross(hf, hb) || can_cross(hb, hf))) {
                            continue;
                        }
                    }
                }
                if !self.walls.contains(&li) {
                    let (f, bk) = (f.unwrap(), bk.unwrap());
                    let fs = st.get(f)?.clone();
                    let bs = st.get(bk)?.clone();
                    let mut top = f64::INFINITY;
                    for hf in &fs {
                        for hb in &bs {
                            top = top.min(hf.0.max(hb.0));
                        }
                    }
                    match ours {
                        Some(o) if o + MAX_STEP < top => {}
                        _ => continue,
                    }
                }
                out.push(self.line_seg(li));
            }
        }
        Ok(out)
    }

    fn box_hits(&self, p: Point, q: Point, ignore: &HashSet<(u64, u64, u64, u64)>, exempt: Option<&HashSet<usize>>, st: &mut States<'_>) -> PyResult<HashSet<(u64, u64, u64, u64)>> {
        let r = PLAYER_RADIUS + BOX_SLACK;
        Ok(self.static_edges(p, q, PLAYER_RADIUS + 1.0, exempt, st)?
            .into_iter()
            .filter(|&e| !ignore.contains(&skey(e)) && swept_box_hits(p, q, r, e))
            .map(skey)
            .collect())
    }

    fn ledges_crossed(&self, a: Point, b: Point, exempt: &HashSet<usize>) -> bool {
        for c in cells(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)) {
            if let Some(lis) = self.mesh.line_grid.get(&c) {
                for &li in lis {
                    if self.drops.contains(&li) && !exempt.contains(&li) {
                        let (v1, v2) = self.line_seg(li);
                        if segments_cross(a, b, v1, v2) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn box_fits_path(&self, pts: &[Point], exempt: &HashSet<usize>, pinned: &HashSet<(u64, u64)>, st: &mut States<'_>, squeezes: &mut Vec<Point>) -> PyResult<Vec<Point>> {
        if pts.len() < 2 {
            return Ok(pts.to_vec());
        }
        let none = HashSet::new();
        let mut kept = Vec::with_capacity(pts.len());
        for (i, &p) in pts.iter().enumerate() {
            if i == 0 || i == pts.len() - 1 || pinned.contains(&pkey(p)) || self.box_hits(p, p, &none, Some(exempt), st)?.is_empty() {
                kept.push(p);
            }
        }
        let mut out = vec![kept[0]];
        for &b in &kept[1..] {
            let a = *out.last().unwrap();
            let mut ignore = self.box_hits(a, a, &none, Some(exempt), st)?;
            ignore.extend(self.box_hits(b, b, &none, Some(exempt), st)?);
            if !self.box_hits(a, b, &ignore, Some(exempt), st)?.is_empty() {
                if let Some(d) = self.detour(a, b, exempt, &ignore, st)? {
                    out.extend(d);
                } else {
                    squeezes.extend(self.squeeze_points(a, b, &ignore, exempt, st)?);
                }
            }
            out.push(b);
        }
        Ok(out)
    }

    fn sectors_along(&self, a: Point, b: Point) -> HashSet<usize> {
        let n = ((dist(a, b) / 8.0).floor() as i64).max(1);
        let mut out = HashSet::new();
        for i in 0..=n {
            let t = i as f64 / n as f64;
            if let Some(s) = self.sector_at((a.0 + (b.0 - a.0) * i as f64 / n as f64, a.1 + (b.1 - a.1) * i as f64 / n as f64)) {
                let _ = t;
                out.insert(s);
            }
        }
        out
    }

    fn within_sectors(&self, a: Point, b: Point, sectors: &HashSet<usize>) -> bool {
        let n = ((dist(a, b) / 8.0).floor() as i64).max(1);
        for i in 0..=n {
            match self.sector_at((a.0 + (b.0 - a.0) * i as f64 / n as f64, a.1 + (b.1 - a.1) * i as f64 / n as f64)) {
                Some(s) if sectors.contains(&s) => {}
                _ => return false,
            }
        }
        true
    }

    fn straighten(&self, pts: &[Point], exempt: &HashSet<usize>, pinned: &HashSet<(u64, u64)>, sectors: Option<&HashSet<usize>>, st: &mut States<'_>) -> PyResult<Vec<Point>> {
        if pts.len() < 3 {
            return Ok(pts.to_vec());
        }
        let over: Vec<HashSet<usize>> = (0..pts.len() - 1).map(|k| self.sectors_along(pts[k], pts[k + 1])).collect();
        let none = HashSet::new();
        let mut out = vec![pts[0]];
        let mut i = 0;
        while i < pts.len() - 1 {
            let a = pts[i];
            let mut best = i + 1;
            let hi = (pts.len() - 1).min(i + 10);
            let mut j = hi;
            while j > i + 1 {
                if pts[i + 1..j].iter().any(|p| pinned.contains(&pkey(*p))) {
                    j -= 1;
                    continue;
                }
                let b = pts[j];
                let mut allowed: HashSet<usize> = over[i..j].iter().flatten().copied().collect();
                if let Some(s) = sectors {
                    allowed.retain(|x| s.contains(x));
                }
                if !self.within_sectors(a, b, &allowed) {
                    j -= 1;
                    continue;
                }
                let mut ignore = self.box_hits(a, a, &none, Some(exempt), st)?;
                ignore.extend(self.box_hits(b, b, &none, Some(exempt), st)?);
                if self.box_hits(a, b, &ignore, Some(exempt), st)?.is_empty() && !self.ledges_crossed(a, b, exempt) {
                    best = j;
                    break;
                }
                j -= 1;
            }
            out.push(pts[best]);
            i = best;
        }
        Ok(out)
    }

    fn squeeze_points(&self, a: Point, b: Point, ignore: &HashSet<(u64, u64, u64, u64)>, exempt: &HashSet<usize>, st: &mut States<'_>) -> PyResult<Vec<Point>> {
        let length = dist(a, b);
        if length < 1.0 {
            return Ok(Vec::new());
        }
        let (ux, uy) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
        let mut out = Vec::new();
        let mut t = 0i64;
        while t <= length.floor() as i64 {
            let p = (a.0 + ux * t as f64, a.1 + uy * t as f64);
            if !self.box_hits(p, p, ignore, Some(exempt), st)?.is_empty() {
                out.push(p);
            }
            t += 8;
        }
        let step = (out.len() / 6).max(1);
        Ok(out.into_iter().step_by(step).collect())
    }

    fn detour(&self, a: Point, b: Point, exempt: &HashSet<usize>, ignore: &HashSet<(u64, u64, u64, u64)>, st: &mut States<'_>) -> PyResult<Option<Vec<Point>>> {
        let length = dist(a, b);
        if length < 1.0 {
            return Ok(None);
        }
        let (ux, uy) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
        let none = HashSet::new();
        let mut bad = Vec::new();
        let mut t = 0i64;
        while t <= length.floor() as i64 {
            let tf = t as f64;
            let p = (a.0 + ux * tf, a.1 + uy * tf);
            let q = (a.0 + ux * (tf + 8.0).min(length), a.1 + uy * (tf + 8.0).min(length));
            if !self.box_hits(p, q, ignore, Some(exempt), st)?.is_empty() {
                bad.push(tf);
            }
            t += 8;
        }
        if bad.is_empty() {
            return Ok(None);
        }
        let (t0, t1) = (bad[0] - 40.0, bad[bad.len() - 1] + 48.0);
        let mut a2 = if t0 > 0.0 { (a.0 + ux * t0, a.1 + uy * t0) } else { a };
        let mut b2 = if t1 < length { (a.0 + ux * t1, a.1 + uy * t1) } else { b };
        if (a2 != a && !self.box_hits(a2, a2, &none, Some(exempt), st)?.is_empty())
            || (b2 != b && !self.box_hits(b2, b2, &none, Some(exempt), st)?.is_empty())
        {
            a2 = a;
            b2 = b;
        }
        let mut nodes = vec![a2, b2];
        let mut seen: HashSet<(u64, u64)> = HashSet::new();
        for e in self.static_edges(a2, b2, 96.0, Some(exempt), st)? {
            for v in [e.0, e.1] {
                if seen.contains(&pkey(v)) || segment_distance(a2, b2, v, v) > 96.0 {
                    continue;
                }
                seen.insert(pkey(v));
                for d in [PLAYER_RADIUS + THING_CLEARANCE, PLAYER_RADIUS + 2.0 * BOX_SLACK] {
                    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                        let c = (v.0 + sx * d, v.1 + sy * d);
                        if self.mesh.poly_at(c.0, c.1).is_some() && self.box_hits(c, c, &none, Some(exempt), st)?.is_empty() {
                            nodes.push(c);
                        }
                    }
                }
            }
        }
        let limit = 3.0 * dist(a2, b2) + 192.0;
        let mut best: HashMap<usize, f64> = HashMap::new();
        best.insert(0, 0.0);
        let mut prev: HashMap<usize, usize> = HashMap::new();
        let mut todo = BinaryHeap::new();
        todo.push(HeapItem(dist(a2, b2), 0.0, 0));
        let mut expansions = 0;
        while let Some(HeapItem(_, cost, i)) = todo.pop() {
            if expansions >= 150 {
                break;
            }
            if i == 1 {
                break;
            }
            if cost > best[&i] {
                continue;
            }
            expansions += 1;
            for j in 1..nodes.len() {
                let c = cost + dist(nodes[i], nodes[j]);
                if j != i && c < *best.get(&j).unwrap_or(&f64::INFINITY) && c + dist(nodes[j], b2) <= limit {
                    // ok(nodes[i], nodes[j])
                    let (p, q) = (nodes[i], nodes[j]);
                    let skip = if p == a || p == a2 || q == b || q == b2 { ignore.clone() } else { HashSet::new() };
                    if self.box_hits(p, q, &skip, Some(exempt), st)?.is_empty() && !self.ledges_crossed(p, q, exempt) {
                        best.insert(j, c);
                        prev.insert(j, i);
                        todo.push(HeapItem(c + dist(nodes[j], b2), c, j));
                    }
                }
            }
        }
        if !prev.contains_key(&1) {
            return Ok(None);
        }
        let mut chain = Vec::new();
        let mut i = prev[&1];
        while i != 0 {
            chain.push(nodes[i]);
            i = prev[&i];
        }
        chain.reverse();
        let mut out = Vec::new();
        if a2 != a {
            out.push(a2);
        }
        out.extend(chain);
        if b2 != b {
            out.push(b2);
        }
        Ok(Some(out))
    }

    /// Navigator._walls_near(a, b, pad, exempt) at the planned heights (live=None).
    fn walls_near(&self, a: Point, b: Point, pad: f64, exempt: Option<&HashSet<usize>>, st: &mut States<'_>) -> PyResult<Vec<Seg>> {
        let mut out = Vec::new();
        let mut seen_l: HashSet<usize> = HashSet::new();
        let mut seen_e: HashSet<(u64, u64, u64, u64)> = HashSet::new();
        let is_wall = |li: &usize| match exempt {
            None => self.walls.contains(li),
            Some(ex) => (self.walls.contains(li) || self.drops.contains(li) || self.teleport_lines.contains(li)) && !ex.contains(li),
        };
        for c in cells(a.0.min(b.0) - pad, a.1.min(b.1) - pad, a.0.max(b.0) + pad, a.1.max(b.1) + pad) {
            if let Some(lis) = self.mesh.line_grid.get(&c) {
                for &li in lis {
                    if is_wall(&li) && !seen_l.contains(&li) {
                        seen_l.insert(li);
                        let l = &self.map.linedefs[li];
                        let (f, bk) = (self.map.front_sector(l), self.map.back_sector(l));
                        if let (Some(f), Some(bk)) = (f, bk) {
                            if self.walls.contains(&li) {
                                let fs = st.get(f)?.clone();
                                let bs = st.get(bk)?.clone();
                                if fs.iter().any(|&hf| bs.iter().any(|&hb| can_cross(hf, hb) || can_cross(hb, hf))) {
                                    continue;
                                }
                            }
                        }
                        out.push(self.line_seg(li));
                    }
                }
            }
            if let Some(es) = self.obstacle_grid.get(&c) {
                for &e in es {
                    if seen_e.insert(skey(e)) {
                        out.push(e);
                    }
                }
            }
        }
        Ok(out)
    }

    fn wall_clearance(&self, p: Point, exempt: &HashSet<usize>, st: &mut States<'_>) -> PyResult<f64> {
        Ok(self.walls_near(p, p, PATH_CLEARANCE, Some(exempt), st)?.into_iter().map(|(a, b)| dist(p, closest_point(p, a, b))).fold(f64::INFINITY, f64::min))
    }

    fn nudge_off_walls(&self, mut p: Point, exempt: &HashSet<usize>, st: &mut States<'_>) -> PyResult<Point> {
        for _ in 0..4 {
            let mut worst: Option<(f64, Point, Point, Point, f64)> = None;
            for (a, b) in self.walls_near(p, p, WAYPOINT_CLEARANCE + 6.0, Some(exempt), st)? {
                let q = closest_point(p, a, b);
                let d = dist(p, q);
                let need = box_clearance(a, b);
                if d < need && worst.map_or(true, |w| d - need < w.0) {
                    worst = Some((d - need, q, a, b, need));
                }
            }
            let Some((_, q, a, b, need)) = worst else { return Ok(p) };
            let d = dist(p, q);
            let (mut ux, mut uy);
            if d > 1e-6 {
                ux = (p.0 - q.0) / d;
                uy = (p.1 - q.1) / d;
            } else {
                let length = dist(a, b).max(1e-6);
                ux = -(b.1 - a.1) / length;
                uy = (b.0 - a.0) / length;
                if self.mesh.poly_at(p.0 + ux, p.1 + uy).is_none() {
                    ux = -ux;
                    uy = -uy;
                }
            }
            let moved = (q.0 + ux * need, q.1 + uy * need);
            let here = self.sector_at(p);
            let there = self.sector_at(moved);
            let crosses = self.walls_near(p, moved, 0.0, Some(exempt), st)?.into_iter().any(|(e1, e2)| segments_cross(p, moved, e1, e2));
            if there.is_none() || (here.is_some() && there != here) || crosses {
                return Ok(p);
            }
            p = moved;
        }
        Ok(p)
    }

    fn push_from_corners(&self, pts: &[Point], exempt: &HashSet<usize>, pinned: &HashSet<(u64, u64)>, st: &mut States<'_>) -> PyResult<Vec<Point>> {
        let mut out = vec![pts[0]];
        for &b in &pts[1..] {
            let a = *out.last().unwrap();
            self.segment(a, b, 0, exempt, st, &mut out)?;
        }
        let n = out.len();
        for i in 1..n.saturating_sub(1) {
            if !pinned.contains(&pkey(out[i])) {
                out[i] = self.nudge_off_walls(out[i], exempt, st)?;
            }
        }
        Ok(out)
    }

    fn segment(&self, a: Point, b: Point, depth: u32, exempt: &HashSet<usize>, st: &mut States<'_>, out: &mut Vec<Point>) -> PyResult<()> {
        let mut worst: Option<(f64, Point, Point)> = None;
        if depth < 4 {
            let mut corners: Vec<Point> = Vec::new();
            let mut seen: HashSet<(u64, u64)> = HashSet::new();
            for (p1, p2) in self.walls_near(a, b, PATH_CLEARANCE, Some(exempt), st)? {
                for v in [p1, p2] {
                    if seen.insert(pkey(v)) {
                        corners.push(v);
                    }
                }
            }
            corners.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
            for v in corners {
                let q = closest_point(v, a, b);
                let d = dist(v, q);
                if d < PATH_CLEARANCE - 1.0 && dist(v, a) > 1.0 && dist(v, b) > 1.0 && worst.map_or(true, |w| d < w.0) {
                    worst = Some((d, v, q));
                }
            }
        }
        if let Some((d, v, q)) = worst {
            let (ux, uy) = if d < 1e-6 {
                let length = dist(a, b).max(1e-6);
                (-(b.1 - a.1) / length, (b.0 - a.0) / length)
            } else {
                ((q.0 - v.0) / d, (q.1 - v.1) / d)
            };
            let w = (v.0 + ux * PATH_CLEARANCE, v.1 + uy * PATH_CLEARANCE);
            let (wp, qp) = (self.sector_at(w), self.sector_at(q));
            if wp.is_some() && qp.is_some() && wp == qp && dist(w, a) > 4.0 && dist(w, b) > 4.0
                && self.wall_clearance(w, exempt, st)? >= WAYPOINT_CLEARANCE
            {
                self.segment(a, w, depth + 1, exempt, st, out)?;
                self.segment(w, b, depth + 1, exempt, st, out)?;
                return Ok(());
            }
        }
        out.push(b);
        Ok(())
    }
}
