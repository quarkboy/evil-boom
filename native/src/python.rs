//! The Python side: the classes evildoom.nav uses, and how values cross over.

use crate::geom::{point_key, Point};
use crate::map::MapData;
use crate::mesh::{Mesh, Obstacle};
use crate::nav::{Callbacks, DoorOpening, Doors, Edge, Found, Gate, HeightSource, Heights, NavCore, NavFacts, Tele, TeleKind, Via};
use crate::plan::{KeySet, Kind, PlanCaches, PlanFacts, Planner, Query, Timing, Trigger, World};
use crate::sight;
use crate::stats;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

// --- values from Python -----------------------------------------------------------------------

/// `(floor, ceiling)`
impl<'py> FromPyObject<'py> for Heights {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let (floor, ceiling) = ob.extract()?;
        Ok(Heights { floor, ceiling })
    }
}

/// `(portal, poly beyond, its sector, the far sector, linedef or None)`
impl<'py> FromPyObject<'py> for Edge {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let (portal, to, from_sector, to_sector, linedef) = ob.extract()?;
        Ok(Edge { portal, to, from_sector, to_sector, linedef })
    }
}

/// `(line, [sectors])`
impl<'py> FromPyObject<'py> for Gate {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let (line, sectors) = ob.extract()?;
        Ok(Gate { line, sectors })
    }
}

/// `("line", dest poly, dest point, line, front sector or None, back sector or None)` or
/// `("jump", dest poly, dest point, runup, takeoff, [(sector, heights)])`
impl<'py> FromPyObject<'py> for Tele {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let kind: String = ob.get_item(0)?.extract()?;
        match kind.as_str() {
            "line" => {
                let (_, dest, dest_pt, line, front, back): (String, usize, Point, usize, Option<usize>, Option<usize>) = ob.extract()?;
                Ok(Tele { dest, dest_pt, kind: TeleKind::Line { line, front, back } })
            }
            "jump" => {
                let (_, dest, dest_pt, runup, takeoff, requires): (String, usize, Point, Point, Point, Vec<(usize, Heights)>) = ob.extract()?;
                Ok(Tele { dest, dest_pt, kind: TeleKind::Jump { runup, takeoff, requires } })
            }
            other => Err(PyValueError::new_err(format!("unknown teleport kind {other:?}"))),
        }
    }
}

/// `(travel, hold or None)`
impl<'py> FromPyObject<'py> for Timing {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let (travel, hold) = ob.extract()?;
        Ok(Timing { travel, hold })
    }
}

/// "W", "S", "D1", "DR", "G" or anything else for other triggers.
impl<'py> FromPyObject<'py> for Kind {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        Ok(match ob.extract::<String>()?.as_str() {
            "W" => Kind::Walk,
            "S" => Kind::Switch,
            "D1" => Kind::DoorOnce,
            "DR" => Kind::DoorRepeat,
            "G" => Kind::Gun,
            _ => Kind::Other,
        })
    }
}

/// The items of an iterable (a set, say: what `Vec`'s extraction won't take).
fn extract_all<'py, T: FromPyObject<'py>>(ob: &Bound<'py, PyAny>) -> PyResult<Vec<T>> {
    ob.try_iter()?.map(|x| x?.extract()).collect()
}

/// A `{sector: {heights}}` dict, read as asked for.
struct PyHeights<'py> {
    dict: Bound<'py, PyDict>,
    cache: HashMap<usize, Rc<Vec<Heights>>>,
}

impl<'py> PyHeights<'py> {
    fn new(dict: Bound<'py, PyDict>) -> Self {
        PyHeights { dict, cache: HashMap::new() }
    }
}

impl HeightSource for PyHeights<'_> {
    fn heights(&mut self, sector: usize) -> PyResult<Rc<Vec<Heights>>> {
        if let Some(hs) = self.cache.get(&sector) {
            return Ok(Rc::clone(hs));
        }
        let hs = Rc::new(match self.dict.get_item(sector)? {
            Some(hs) => extract_all(&hs)?,
            None => Vec::new(),
        });
        self.cache.insert(sector, Rc::clone(&hs));
        Ok(hs)
    }
}

/// A `{sector: ({heights}, {activator polys})}` dict of the use-doors that can be opened.
fn read_doors(d: Option<Bound<'_, PyDict>>) -> PyResult<Option<Doors>> {
    let Some(d) = d.filter(|d| !d.is_empty()) else { return Ok(None) };
    let mut out = Doors::new();
    for (k, v) in d.iter() {
        let (heights, users): (Bound<'_, PyAny>, Bound<'_, PyAny>) = v.extract()?;
        out.insert(k.extract()?, DoorOpening { heights: extract_all(&heights)?, users: extract_all(&users)?.into_iter().collect() });
    }
    Ok(Some(out))
}

/// A trigger, as a dict.
#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct TriggerIn {
    line: Option<usize>,
    kind: Kind,
    effects: Vec<(usize, Vec<Heights>)>,
    activators: HashSet<usize>,
    permanent: bool,
    lock_ok: Vec<bool>,
    /// [((sector, heights), timing)]
    timing: Vec<((usize, Heights), Timing)>,
    perm_travel: Vec<(usize, f64)>,
    shift_from: Option<(usize, f64)>,
    once: bool,
    relevant: Option<Vec<usize>>,
}

impl From<TriggerIn> for Trigger {
    fn from(t: TriggerIn) -> Self {
        Trigger {
            line: t.line,
            kind: t.kind,
            effects: t.effects,
            activators: t.activators,
            permanent: t.permanent,
            lock_ok: t.lock_ok,
            timing: t.timing.into_iter().map(|((s, h), tm)| ((s, h.key()), tm)).collect(),
            perm_travel: t.perm_travel.into_iter().collect(),
            shift_from: t.shift_from,
            once: t.once,
            relevant: t.relevant,
        }
    }
}

/// The plan search's per-map facts, as a dict.
#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct PlanFactsIn {
    triggers: Vec<TriggerIn>,
    use_gates: HashMap<(usize, usize), Vec<usize>>,
    shot_floors: HashMap<(usize, usize), (f64, f64)>,
    cross_sides: HashMap<(usize, usize), usize>,
    temporary: Vec<(usize, Heights)>,
    key_positions: Vec<(u8, Vec<Point>)>,
    effects_from_reps: PyObject,
}

impl From<PlanFactsIn> for PlanFacts {
    fn from(f: PlanFactsIn) -> Self {
        PlanFacts {
            triggers: f.triggers.into_iter().map(Trigger::from).collect(),
            use_gates: f.use_gates,
            shot_floors: f.shot_floors,
            cross_sides: f.cross_sides,
            temporary: f.temporary.into_iter().map(|(s, h)| (s, h.key())).collect(),
            key_positions: f.key_positions,
            effects_from_reps: f.effects_from_reps,
        }
    }
}

/// What to plan, as a dict (besides the start, goals and heights).
#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct QueryIn {
    keys: u8,
    key_items: Vec<(u8, Vec<(usize, Point)>)>,
    max_expansions: usize,
    max_seconds: f64,
    goal_triggers: Vec<(Point, usize)>,
    relevant_lines: HashSet<Option<usize>>,
    relevant_keys: u8,
    strict_fit: bool,
    fired: Vec<usize>,
}

impl QueryIn {
    fn into_query(self, start: Point, goals: Vec<Point>) -> Query {
        Query {
            start,
            goals,
            keys: KeySet(self.keys),
            key_items: self.key_items,
            max_expansions: self.max_expansions,
            max_seconds: self.max_seconds,
            goal_triggers: self.goal_triggers.into_iter().map(|(p, t)| (point_key(p), t)).collect(),
            relevant_lines: self.relevant_lines,
            relevant_keys: KeySet(self.relevant_keys),
            strict_fit: self.strict_fit,
            fired: self.fired.into_iter().collect::<BTreeSet<usize>>(),
        }
    }
}

// --- values to Python -------------------------------------------------------------------------

/// A poly: (sector, vertices, centre, lines it's in front of, lines it's behind, portals).
type PolyOut = (usize, Vec<Point>, Point, Vec<usize>, Vec<usize>, Vec<usize>);
/// A portal: (poly a, poly b, p1, p2, linedef or None).
type PortalOut = (usize, usize, Point, Point, Option<usize>);
/// How a path enters a poly: (is a teleport, portal or index into the previous poly's
/// teleports).
type ViaOut = Option<(bool, usize)>;
/// A path: (polys, waypoints (None = teleport), cost, vias, squeezes).
type PathOut = (Vec<usize>, Vec<Option<Point>>, f64, Vec<ViaOut>, usize);

fn via_out(via: Option<Via>) -> ViaOut {
    via.map(|v| match v {
        Via::Portal(pi) => (false, pi),
        Via::Tele(ti) => (true, ti),
    })
}

fn path_out(f: &Found) -> PathOut {
    (f.polys.clone(), f.waypoints.clone(), f.cost, f.vias.iter().copied().map(via_out).collect(), f.squeezes)
}

fn world_out<'py>(py: Python<'py>, w: &World) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    for (s, hs) in w.iter().enumerate() {
        d.set_item(s, hs.iter().map(|h| h.tuple()).collect::<Vec<_>>())?;
    }
    Ok(d)
}

fn sorted(set: &HashSet<usize>) -> Vec<usize> {
    let mut v: Vec<usize> = set.iter().copied().collect();
    v.sort_unstable();
    v
}

// --- classes ----------------------------------------------------------------------------------

/// A map's navmesh, built from `evildoom.wad.mapdata.MapData` with `obstacles` (x, y,
/// half-width) cut out of it.
#[pyclass]
pub struct NativeMesh {
    mesh: Mesh,
}

#[pymethods]
impl NativeMesh {
    #[new]
    fn new(map: MapData, obstacles: Vec<Obstacle>) -> PyResult<Self> {
        let mesh = Mesh::build(&map, &obstacles).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(NativeMesh { mesh })
    }

    fn polys(&self) -> Vec<Option<PolyOut>> {
        self.mesh
            .polys
            .iter()
            .map(|p| p.as_ref().map(|p| (p.sector, p.vertices.clone(), p.center, sorted(&p.lines_front), sorted(&p.lines_back), p.portals.clone())))
            .collect()
    }

    fn portals(&self) -> Vec<PortalOut> {
        self.mesh.portals.iter().map(|q| (q.a, q.b, q.p1, q.p2, q.linedef)).collect()
    }

    fn pieces(&self) -> HashMap<usize, Vec<usize>> {
        self.mesh.pieces.clone()
    }

    fn poly_at(&self, x: f64, y: f64) -> Option<usize> {
        self.mesh.poly_at((x, y))
    }
}

/// The navigator core for a map: flood fill, path-finding and the plan search.
#[pyclass]
pub struct Nav {
    core: NavCore,
    plan_facts: Option<PlanFacts>,
    plan_caches: PlanCaches,
}

#[pymethods]
impl Nav {
    /// `facts`: a dict of NavFacts' fields; `fits` and `landable`: the geometry questions
    /// asked back (Navigator.portal_fits_without and _landable).
    #[new]
    fn new(map: MapData, obstacles: Vec<Obstacle>, facts: NavFacts, fits: PyObject, landable: PyObject) -> PyResult<Self> {
        let mesh = Mesh::build(&map, &obstacles).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let core = NavCore::new(map, mesh, facts, Callbacks::new(fits, landable));
        Ok(Nav { core, plan_facts: None, plan_caches: PlanCaches::default() })
    }

    /// The polys reachable from `start` with these heights ({sector: {(floor, ceiling)}})
    /// and use-doors ({sector: ({heights}, {activator polys})}).
    #[pyo3(signature = (start, states, doors=None))]
    fn reachable(&mut self, py: Python<'_>, start: usize, states: Bound<'_, PyDict>, doors: Option<Bound<'_, PyDict>>) -> PyResult<HashSet<usize>> {
        let doors = read_doors(doors)?;
        self.core.reachable(py, start, &mut PyHeights::new(states), doors.as_ref())
    }

    /// A path from `start` to `goal`, or None.
    #[pyo3(signature = (start, goal, states, doors=None, strict_fit=false))]
    fn find_path(&mut self, py: Python<'_>, start: Point, goal: Point, states: Bound<'_, PyDict>, doors: Option<Bound<'_, PyDict>>, strict_fit: bool) -> PyResult<Option<PathOut>> {
        let doors = read_doors(doors)?;
        let found = self.core.find_path(py, start, goal, &mut PyHeights::new(states), doors.as_ref(), strict_fit)?;
        Ok(found.as_ref().map(path_out))
    }

    fn poly_at(&self, x: f64, y: f64) -> Option<usize> {
        self.core.mesh.poly_at((x, y))
    }

    /// The plan search's per-map facts (a dict of PlanFacts' fields), needed before `plan`.
    fn set_plan_data(&mut self, facts: PlanFactsIn) {
        self.plan_facts = Some(facts.into());
    }

    /// A route from `start` to one of `goals` with the world at `states`, or None:
    /// (legs [(trigger index or None, key bit or None, path, the leg's heights, its keys
    /// bitmask)], final heights, final keys bitmask, goal point).
    fn plan(&mut self, py: Python<'_>, start: Point, goals: Vec<Point>, states: Bound<'_, PyDict>, query: QueryIn) -> PyResult<Option<PyObject>> {
        let facts = self.plan_facts.as_ref().ok_or_else(|| PyRuntimeError::new_err("set_plan_data must be called before plan"))?;
        let sectors = (self.core.poly_sector.iter().copied().max().unwrap_or(0) + 1).max(states.len());
        let mut st = PyHeights::new(states);
        let world: World = Rc::new((0..sectors).map(|s| st.heights(s)).collect::<PyResult<_>>()?);
        let query = query.into_query(start, goals);
        stats::reset();
        let t0 = Instant::now();
        let found = Planner::new(py, &mut self.core, facts, &mut self.plan_caches).plan(&query, world)?;
        if stats::enabled() {
            eprintln!("{}", stats::report(t0.elapsed().as_secs_f64()));
        }
        let Some(route) = found else { return Ok(None) };
        let legs = route
            .legs
            .iter()
            .map(|leg| Ok((leg.trigger, leg.key, path_out(&leg.path), world_out(py, &leg.states)?, leg.keys.0)))
            .collect::<PyResult<Vec<_>>>()?;
        let out = (legs, world_out(py, &route.states)?, route.keys.0, route.goal);
        Ok(Some(out.into_pyobject(py)?.into_any().unbind()))
    }
}

/// What the renderer draws of a map from a viewpoint (the lines the automap reveals).
#[pyclass]
pub struct Sight {
    sight: sight::Sight,
}

#[pymethods]
impl Sight {
    #[new]
    fn new(map: MapData) -> Self {
        Sight { sight: sight::Sight::new(&map) }
    }

    /// The linedefs drawn looking from (x, y) at `angle` (degrees, anticlockwise from east)
    /// on a screen `width` columns wide, with the sectors at `heights` [(floor, ceiling)].
    fn drawn(&self, x: f64, y: f64, angle: f64, width: i32, heights: Vec<(f64, f64)>) -> Vec<usize> {
        self.sight.drawn(&sight::View { x, y, angle, width, heights: &heights })
    }
}
