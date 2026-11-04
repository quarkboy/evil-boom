//! Compiled navigation core for the evil-doom planner (being ported from evildoom.nav in
//! stages; the Python versions are the reference and the fallback when this isn't built).
//!
//! - `NativeMesh`: load a map from a WAD and build its navmesh (stage 1, for checking).
//! - `Nav`: the navigator core over a navmesh: flood fill (`reachable`) and path-finding
//!   (`find_path`, with funnel smoothing and box fitting), stage 2.

mod geom;
mod mesh;
mod nav;
mod plan;
mod wad;

use geom::{Point, Seg};
use nav::{read_doors, Edge, NavCore, States, Tele, TeleKind};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::{HashMap, HashSet};

type PolyOut = Option<(usize, Vec<Point>, Point, Vec<usize>, Vec<usize>, Vec<usize>)>;

/// A map loaded and its navmesh built in Rust, readable from Python for checking against
/// evildoom.nav.navmesh.
#[pyclass]
struct NativeMesh {
    map: wad::MapData,
    mesh: mesh::Mesh,
}

#[pymethods]
impl NativeMesh {
    #[new]
    #[pyo3(signature = (wad_path, map_name, obstacles, texture_wads=Vec::new()))]
    fn new(wad_path: &str, map_name: &str, obstacles: Vec<(f64, f64, f64)>, texture_wads: Vec<String>) -> PyResult<Self> {
        let err = |e: String| PyValueError::new_err(e);
        let w = wad::Wad::open(wad_path).map_err(err)?;
        let tw: Vec<wad::Wad> = texture_wads.iter().map(|p| wad::Wad::open(p)).collect::<Result<_, _>>().map_err(err)?;
        let refs: Vec<&wad::Wad> = tw.iter().collect();
        let map = wad::load_map(&w, map_name, &refs).map_err(err)?;
        let mesh = mesh::Mesh::build(&map, &obstacles).map_err(err)?;
        Ok(NativeMesh { map, mesh })
    }

    fn polys(&self) -> Vec<PolyOut> {
        self.mesh.polys.iter().map(|p| p.as_ref().map(|p| {
            let mut f: Vec<usize> = p.lines_front.iter().copied().collect();
            let mut b: Vec<usize> = p.lines_back.iter().copied().collect();
            f.sort_unstable();
            b.sort_unstable();
            (p.sector, p.vertices.clone(), p.center, f, b, p.portals.clone())
        })).collect()
    }

    fn portals(&self) -> Vec<(usize, usize, Point, Point, Option<usize>)> {
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

/// tele entries: (kind "line"|"jump", dest poly, dest point, line, front, back, runup, takeoff,
/// requires)
type TeleIn = (String, usize, Point, i64, i64, i64, Point, Point, Vec<(usize, (f64, f64))>);

/// The navigator core (see nav.rs), built from Python's map data and per-map facts.
#[pyclass]
struct Nav {
    core: NavCore,
}

#[pymethods]
impl Nav {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn new(
        verts: Vec<(f64, f64)>,
        lines: Vec<(usize, usize, u16, u16, u16, u16, u16)>,
        side_sectors: Vec<usize>,
        segs: Vec<(usize, usize, usize, i16)>,
        subsectors: Vec<(usize, usize)>,
        nodes: Vec<(f64, f64, f64, f64, u16, u16)>,
        obstacles: Vec<(f64, f64, f64)>,
        adj: Vec<Vec<(usize, usize, usize, usize, i64)>>,
        gates: HashMap<usize, Vec<(i64, Vec<usize>)>>,
        tele: Vec<Vec<TeleIn>>,
        initial: Vec<(f64, f64)>,
        damage: Vec<bool>,
        has_teleports: bool,
        walls: HashSet<usize>,
        drops: HashSet<usize>,
        teleport_lines: HashSet<usize>,
        shutting: HashSet<usize>,
        static_grid: HashMap<(i64, i64), Vec<Seg>>,
        obstacle_grid: HashMap<(i64, i64), Vec<Seg>>,
        fits_cb: PyObject,
        landable_cb: PyObject,
    ) -> PyResult<Self> {
        let map = wad::MapData::from_parts(verts, lines, side_sectors, segs, subsectors, nodes);
        let mesh = mesh::Mesh::build(&map, &obstacles).map_err(PyValueError::new_err)?;
        let poly_sector = mesh.polys.iter().map(|p| p.as_ref().map_or(0, |p| p.sector)).collect();
        let adj = adj.into_iter().map(|es| es.into_iter().map(|(portal, to, sa, sb, linedef)| Edge { portal, to, sa, sb, linedef }).collect()).collect();
        let tele = tele.into_iter().map(|ts| ts.into_iter().map(|(kind, dest, dest_pt, line, front, back, runup, takeoff, requires)| Tele {
            dest,
            dest_pt,
            kind: if kind == "jump" { TeleKind::Jump { runup, takeoff, requires } } else { TeleKind::Line { line: line as usize, front, back } },
        }).collect()).collect();
        let mut seg_lines = HashMap::new();
        for (li, l) in map.linedefs.iter().enumerate() {
            let (va, vb) = (map.vertexes[l.v1], map.vertexes[l.v2]);
            seg_lines.insert(geom::skey(((va.x, va.y), (vb.x, vb.y))), li);
        }
        Ok(Nav {
            core: NavCore {
                map,
                mesh,
                poly_sector,
                adj,
                gates,
                tele,
                fits_cb,
                landable_cb,
                fits_cache: HashMap::new(),
                landable_cache: HashMap::new(),
                initial,
                damage,
                has_teleports,
                walls,
                drops,
                teleport_lines,
                shutting,
                static_grid,
                obstacle_grid,
                obstacle_order: Vec::new(),
                plan_data: None,
                seg_lines,
                smooth_doors: None,
                hard_squeeze: std::sync::atomic::AtomicBool::new(false),
            },
        })
    }

    /// Navigator.reachable
    #[pyo3(signature = (start, states, doors=None))]
    fn reachable(&mut self, py: Python<'_>, start: usize, states: Bound<'_, PyDict>, doors: Option<Bound<'_, PyDict>>) -> PyResult<HashSet<usize>> {
        let mut st = States::new(states);
        let doors = match doors {
            Some(d) if !d.is_empty() => Some(read_doors(&d)?),
            _ => None,
        };
        self.core.reachable(py, start, &mut st, doors.as_ref())
    }

    /// Navigator.find_path: None, or (polys, waypoints (None = teleport), cost, vias (None |
    /// (is_tele, portal or index into the previous poly's tele list)), squeezes).
    #[pyo3(signature = (start, goal, states, doors=None, strict_fit=false))]
    #[allow(clippy::type_complexity)]
    fn find_path(&mut self, py: Python<'_>, start: Point, goal: Point, states: Bound<'_, PyDict>, doors: Option<Bound<'_, PyDict>>, strict_fit: bool)
        -> PyResult<Option<(Vec<usize>, Vec<Option<Point>>, f64, Vec<Option<(bool, usize)>>, usize)>> {
        let mut st = States::new(states);
        let doors = match doors {
            Some(d) if !d.is_empty() => Some(read_doors(&d)?),
            _ => None,
        };
        Ok(self.core.find_path(py, start, goal, &mut st, doors.as_ref(), strict_fit)?.map(|f| (f.polys, f.waypoints, f.cost, f.vias, f.squeezes)))
    }

    fn poly_at(&self, x: f64, y: f64) -> Option<usize> {
        self.core.mesh.poly_at(x, y)
    }

    /// The plan search's per-map facts. `triggers`: (line, kind, effects [(sector,
    /// [heights])], activators, permanent, lock_ok per key bitmask, timing [((sector, heights),
    /// (travel, hold or -1))], perm_travel [(sector, travel)]); `key_positions`: [(key bit,
    /// [(x, y)])].
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn set_plan_data(
        &mut self,
        triggers: Vec<(i64, String, Vec<(usize, Vec<(f64, f64)>)>, HashSet<usize>, bool, Vec<bool>, Vec<((usize, (f64, f64)), (f64, f64))>, Vec<(usize, f64)>)>,
        use_gates: HashMap<(i64, usize), Vec<usize>>,
        shot_floors: HashMap<(i64, usize), (f64, f64)>,
        cross_sides: HashMap<(i64, usize), usize>,
        temporary: Vec<(usize, (f64, f64))>,
        key_positions: Vec<(u8, Vec<Point>)>,
    ) {
        use plan::{hk, Kind, PlanData, Trig};
        let triggers = triggers.into_iter().map(|(line, kind, effects, activators, permanent, lock_ok, timing, perm)| Trig {
            line,
            kind: match kind.as_str() { "W" => Kind::Walk, "S" => Kind::Switch, "D1" => Kind::D1, "DR" => Kind::DR, "G" => Kind::Gun, _ => Kind::Other },
            effects,
            activators,
            permanent,
            lock_ok,
            timing: timing.into_iter().map(|((s, h), v)| ((s, hk(h)), v)).collect(),
            perm_travel: perm.into_iter().collect(),
        }).collect();
        self.core.plan_data = Some(PlanData {
            triggers,
            use_gates,
            shot_floors,
            cross_sides,
            temporary: temporary.into_iter().map(|(s, h)| (s, hk(h))).collect(),
            key_positions,
            door_cache: HashMap::new(),
            reach_cache: HashMap::new(),
        });
    }

    /// Navigator.plan. Returns None or (legs [(trigger index or None, key bit or None, path
    /// (as find_path), leg states {sector: [heights]}, leg keys bitmask)], final states, final
    /// keys bitmask, goal point).
    #[pyo3(signature = (start, goals, keys, states, key_items, max_expansions, max_seconds, goal_triggers, rel_t, rel_k, strict_fit=false))]
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn plan(
        &mut self,
        py: Python<'_>,
        start: Point,
        goals: Vec<Point>,
        keys: u8,
        states: Bound<'_, PyDict>,
        key_items: Vec<(u8, Vec<(usize, Point)>)>,
        max_expansions: usize,
        max_seconds: f64,
        goal_triggers: Vec<(Point, usize)>,
        rel_t: HashSet<i64>,
        rel_k: u8,
        strict_fit: bool,
    ) -> PyResult<Option<PyObject>> {
        use std::rc::Rc;
        let n = self.core.poly_sector.iter().copied().max().unwrap_or(0) + 1;
        let n = n.max(states.len());
        let mut st = States::new(states);
        let mut world: Vec<Rc<Vec<(f64, f64)>>> = Vec::with_capacity(n);
        for s in 0..n {
            world.push(Rc::new(st.get_owned(s)?));
        }
        let gt: HashMap<(u64, u64), usize> = goal_triggers.into_iter().map(|(p, t)| (geom::pkey(p), t)).collect();
        plan::STATS.with(|s| *s.borrow_mut() = (0, 0.0, 0, 0.0));
        plan::PHASES.with(|p| *p.borrow_mut() = [0.0; 4]);
        let t0 = std::time::Instant::now();
        let found = self.core.plan(py, start, &goals, keys, Rc::new(world), &key_items, max_expansions, max_seconds, &gt, &rel_t, rel_k, strict_fit)?;
        if std::env::var_os("EVILDOOM_NATIVE_STATS").is_some() {
            plan::STATS.with(|s| {
                let s = s.borrow();
                let p = plan::PHASES.with(|p| *p.borrow());
                eprintln!("native plan {:.2}s: {} timed paths {:.2}s (A* {:.2} corners {:.2} fit {:.2} straighten {:.2}), {} flood fills {:.2}s", t0.elapsed().as_secs_f64(), s.0, s.1, p[0], p[1], p[2], p[3], s.2, s.3);
            });
        }
        let Some(route) = found else { return Ok(None) };
        let to_dict = |w: &Rc<Vec<Rc<Vec<(f64, f64)>>>>| -> PyResult<PyObject> {
            let d = PyDict::new(py);
            for (s, hs) in w.iter().enumerate() {
                d.set_item(s, hs.as_ref().clone())?;
            }
            Ok(d.into_any().unbind())
        };
        let mut legs = Vec::new();
        for leg in &route.legs {
            let f = &leg.path;
            let path = (f.polys.clone(), f.waypoints.clone(), f.cost, f.vias.clone(), f.squeezes);
            legs.push((leg.trig, leg.key, path, to_dict(&leg.states)?, leg.keys).into_pyobject(py)?.into_any().unbind());
        }
        let out = (legs, to_dict(&route.states)?, route.keys, route.goal).into_pyobject(py)?.into_any().unbind();
        Ok(Some(out))
    }
}

#[pymodule]
fn evildoom_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeMesh>()?;
    m.add_class::<Nav>()?;
    Ok(())
}
