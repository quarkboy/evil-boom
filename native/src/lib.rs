//! Compiled hot paths for the evil-doom planner. Optional: every function here has a Python
//! twin in `evildoom.nav` that is used when this module isn't built.
//!
//! `Graph::reachable` is `Navigator.reachable`: the flood fill over navmesh polys that the
//! plan search runs thousands of times per level (about 40% of planning time in Python).

mod mesh;
mod wad;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::{HashMap, HashSet};

const MAX_STEP: f64 = 24.0;
const PLAYER_HEIGHT: f64 = 56.0;

type Heights = (f64, f64); // (floor, ceiling)

#[inline]
fn can_cross(a: Heights, b: Heights) -> bool {
    let ((fa, ca), (fb, cb)) = (a, b);
    fb - fa <= MAX_STEP && ca.min(cb) - fa.max(fb) >= PLAYER_HEIGHT
}

struct Edge {
    portal: usize,
    to: usize,
    sa: usize,
    sb: usize,
    linedef: i64, // -1: none
}

enum Tele {
    /// a walk-over teleport line: fires only if its two sectors can be stepped between
    Line { dest: usize, front: i64, back: i64 },
    /// a run-off jump: needs these (sector, heights) to be possible
    Jump { dest: usize, requires: Vec<(usize, Heights)> },
}

type Doors = HashMap<usize, (Vec<Heights>, HashSet<usize>)>;

/// A sector -> set-of-(floor, ceiling) dict from Python, read lazily: a flood fill only looks
/// at the sectors it reaches, and converting all of them up front cost more than the fill.
struct States<'py> {
    dict: Bound<'py, PyDict>,
    cache: HashMap<usize, Vec<Heights>>,
}

impl<'py> States<'py> {
    fn get(&mut self, s: usize) -> PyResult<&Vec<Heights>> {
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

fn read_doors(d: &Bound<'_, PyDict>) -> PyResult<Doors> {
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

/// The navmesh's static connectivity for flood fills (see Navigator._static_adjacency).
#[pyclass]
struct Graph {
    poly_sector: Vec<usize>,
    adj: Vec<Vec<Edge>>,
    /// per portal: (gate line, the moving sectors on its far side)
    gates: HashMap<usize, Vec<(i64, Vec<usize>)>>,
    tele: Vec<Vec<Tele>>,
    /// Navigator.portal_fits_without(portal, blocking lines) and ._landable(line, sector,
    /// floor): geometry questions answered in Python, cached here
    fits_cb: PyObject,
    landable_cb: PyObject,
    fits_cache: HashMap<(usize, Vec<i64>), bool>,
    landable_cache: HashMap<(i64, usize, i64), bool>,
}

impl Graph {
    /// Heights of `s`, plus a door's open heights when it can be used from `frm` (always, if
    /// `frm` is None: standing in it).
    fn heights(
        states: &mut States<'_>,
        doors: &Option<Doors>,
        s: usize,
        frm: Option<usize>,
    ) -> PyResult<Vec<Heights>> {
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

    /// Navigator._fits_now: does the box fit through `portal` from poly `frm`, given which
    /// nearby moving-sector lines block (gate_blockers)?
    fn fits_now(
        &mut self,
        py: Python<'_>,
        portal: usize,
        frm: usize,
        states: &mut States<'_>,
        doors: &Option<Doors>,
    ) -> PyResult<bool> {
        let Some(gates) = self.gates.get(&portal) else { return Ok(true) };
        let gates = gates.clone();
        let sector = self.poly_sector[frm];
        let here = Self::heights(states, doors, sector, None)?;
        let mut blocking = Vec::new();
        for (li, sectors) in &gates {
            for &s in sectors {
                if s == sector {
                    continue;
                }
                let hs = Self::heights(states, doors, s, Some(frm))?;
                if !here.iter().any(|&ha| hs.iter().any(|&hb| can_cross(ha, hb))) {
                    blocking.push(*li);
                    break;
                }
            }
        }
        self.fits(py, portal, blocking)
    }

    /// Navigator._heights_passable: a step or drop we can make, with headroom.
    fn heights_passable(
        &mut self,
        py: Python<'_>,
        sa: usize,
        sb: usize,
        linedef: i64,
        frm: usize,
        states: &mut States<'_>,
        doors: &Option<Doors>,
    ) -> PyResult<bool> {
        let hs_a = Self::heights(states, doors, sa, None)?;
        let hs_b = Self::heights(states, doors, sb, Some(frm))?;
        for &ha in &hs_a {
            for &hb in &hs_b {
                if !can_cross(ha, hb) {
                    continue;
                }
                if ha.0 - hb.0 <= MAX_STEP || linedef < 0 || self.landable(py, linedef, sa, ha.0)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[pymethods]
impl Graph {
    /// `adj[poly]` = [(portal, to poly, its sector, far sector, linedef or -1)] (the portals
    /// that aren't ruled out whatever the heights); `gates[portal]` = [(line, [sectors])];
    /// `tele[poly]` = [("line", dest, front, back, []) | ("jump", dest, -1, -1, requires)].
    #[new]
    fn new(
        poly_sector: Vec<usize>,
        adj: Vec<Vec<(usize, usize, usize, usize, i64)>>,
        gates: HashMap<usize, Vec<(i64, Vec<usize>)>>,
        tele: Vec<Vec<(String, usize, i64, i64, Vec<(usize, Heights)>)>>,
        fits_cb: PyObject,
        landable_cb: PyObject,
    ) -> Self {
        let adj = adj
            .into_iter()
            .map(|es| {
                es.into_iter()
                    .map(|(portal, to, sa, sb, linedef)| Edge { portal, to, sa, sb, linedef })
                    .collect()
            })
            .collect();
        let tele = tele
            .into_iter()
            .map(|ts| {
                ts.into_iter()
                    .map(|(kind, dest, front, back, requires)| {
                        if kind == "jump" {
                            Tele::Jump { dest, requires }
                        } else {
                            Tele::Line { dest, front, back }
                        }
                    })
                    .collect()
            })
            .collect();
        Graph {
            poly_sector,
            adj,
            gates,
            tele,
            fits_cb,
            landable_cb,
            fits_cache: HashMap::new(),
            landable_cache: HashMap::new(),
        }
    }

    /// Navigator.reachable: every poly reachable from `start` with these heights (`states`:
    /// sector -> the heights it can take) and use-doors (`doors`: sector -> (open heights,
    /// polys it can be opened from)).
    #[pyo3(signature = (start, states, doors=None))]
    fn reachable(
        &mut self,
        py: Python<'_>,
        start: usize,
        states: Bound<'_, PyDict>,
        doors: Option<Bound<'_, PyDict>>,
    ) -> PyResult<HashSet<usize>> {
        let mut states = States { dict: states, cache: HashMap::new() };
        let doors = match doors {
            Some(d) if !d.is_empty() => Some(read_doors(&d)?),
            _ => None,
        };
        let mut seen: HashSet<usize> = HashSet::new();
        seen.insert(start);
        let mut todo = vec![start];
        while let Some(poly) = todo.pop() {
            for i in 0..self.adj[poly].len() {
                let (portal, to, sa, sb, linedef) = {
                    let e = &self.adj[poly][i];
                    (e.portal, e.to, e.sa, e.sb, e.linedef)
                };
                if seen.contains(&to) {
                    continue;
                }
                if self.gates.contains_key(&portal)
                    && !self.fits_now(py, portal, poly, &mut states, &doors)?
                {
                    continue;
                }
                if sa != sb
                    && !self.heights_passable(py, sa, sb, linedef, poly, &mut states, &doors)?
                {
                    continue;
                }
                seen.insert(to);
                todo.push(to);
            }
            for ti in 0..self.tele[poly].len() {
                let (dest, ok) = match &self.tele[poly][ti] {
                    Tele::Jump { dest, requires } => {
                        let (dest, requires) = (*dest, requires.clone());
                        let mut ok = true;
                        for (s, h) in &requires {
                            if !states.get(*s)?.iter().any(|x| x == h) {
                                ok = false;
                                break;
                            }
                        }
                        (dest, ok)
                    }
                    Tele::Line { dest, front, back } => {
                        let (dest, front, back) = (*dest, *front, *back);
                        let ok = if front >= 0 && back >= 0 {
                            let fs = states.get(front as usize)?.clone();
                            let bs = states.get(back as usize)?;
                            fs.iter().any(|&fh| bs.iter().any(|&bh| can_cross(fh, bh)))
                        } else {
                            false
                        };
                        (dest, ok)
                    }
                };
                if ok && !seen.contains(&dest) {
                    seen.insert(dest);
                    todo.push(dest);
                }
            }
        }
        Ok(seen)
    }
}

type PolyOut = Option<(usize, Vec<mesh::Point>, mesh::Point, Vec<usize>, Vec<usize>, Vec<usize>)>;

/// A map loaded and its navmesh built in Rust (stage 1 of the port: owned here, readable from
/// Python for checking against evildoom.nav.navmesh).
#[pyclass]
struct NativeMesh {
    map: wad::MapData,
    mesh: mesh::Mesh,
}

#[pymethods]
impl NativeMesh {
    /// `wad_path`'s `map_name`, texture sizes from `texture_wads` then the map's own WAD;
    /// `obstacles`: (x, y, half-width) boxes cut out (solid things, grown by the player).
    #[new]
    #[pyo3(signature = (wad_path, map_name, obstacles, texture_wads=Vec::new()))]
    fn new(wad_path: &str, map_name: &str, obstacles: Vec<(f64, f64, f64)>,
           texture_wads: Vec<String>) -> PyResult<Self> {
        let err = |e: String| PyValueError::new_err(e);
        let w = wad::Wad::open(wad_path).map_err(err)?;
        let tw: Vec<wad::Wad> = texture_wads.iter().map(|p| wad::Wad::open(p)).collect::<Result<_, _>>().map_err(err)?;
        let refs: Vec<&wad::Wad> = tw.iter().collect();
        let map = wad::load_map(&w, map_name, &refs).map_err(err)?;
        let mesh = mesh::Mesh::build(&map, &obstacles).map_err(err)?;
        Ok(NativeMesh { map, mesh })
    }

    /// per poly: None or (sector, vertices, center, lines_front, lines_back, portals)
    fn polys(&self) -> Vec<PolyOut> {
        self.mesh.polys.iter().map(|p| p.as_ref().map(|p| {
            let mut f: Vec<usize> = p.lines_front.iter().copied().collect();
            let mut b: Vec<usize> = p.lines_back.iter().copied().collect();
            f.sort_unstable();
            b.sort_unstable();
            (p.sector, p.vertices.clone(), p.center, f, b, p.portals.clone())
        })).collect()
    }

    /// per portal: (a, b, p1, p2, linedef or None)
    fn portals(&self) -> Vec<(usize, usize, mesh::Point, mesh::Point, Option<usize>)> {
        self.mesh.portals.iter().map(|q| (q.a, q.b, q.p1, q.p2, q.linedef)).collect()
    }

    fn pieces(&self) -> HashMap<usize, Vec<usize>> {
        self.mesh.pieces.clone()
    }

    fn poly_at(&self, x: f64, y: f64) -> Option<usize> {
        self.mesh.poly_at(x, y)
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        (self.map.vertexes.len(), self.map.linedefs.len(), self.map.sectors.len(), self.map.things.len())
    }
}

#[pymodule]
fn evildoom_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Graph>()?;
    m.add_class::<NativeMesh>()?;
    Ok(())
}
