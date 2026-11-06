//! The navigator core over a navmesh: which sector heights let the player across which
//! portals, and the flood fill of where they can get to. Path-finding is in `path`, path
//! smoothing and fitting the player's box along it in `fit`.

mod fit;
mod path;

pub use path::{Found, Via};

use crate::geom::{seg_key, Cell, Point, Seg, SegKey, MAX_STEP, PLAYER_HEIGHT};
use crate::map::MapData;
use crate::mesh::Mesh;
use pyo3::prelude::*;
use std::cmp::Ordering;
use crate::det::{HashMap, HashSet};
use std::rc::Rc;

/// A sector's floor and ceiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heights {
    pub floor: f64,
    pub ceiling: f64,
}

/// Heights by value, for hash sets and maps.
pub type HeightsKey = (u64, u64);

impl Heights {
    pub fn key(self) -> HeightsKey {
        ((self.floor + 0.0).to_bits(), (self.ceiling + 0.0).to_bits())
    }

    pub fn tuple(self) -> (f64, f64) {
        (self.floor, self.ceiling)
    }
}

/// Can the player get from a sector at heights `a` to one at `b` (step up, fit under)?
#[inline]
pub fn can_cross(a: Heights, b: Heights) -> bool {
    b.floor - a.floor <= MAX_STEP && a.ceiling.min(b.ceiling) - a.floor.max(b.floor) >= PLAYER_HEIGHT
}

/// Some heights of `a` and some of `b` the player can cross between, one way or the other.
fn crossable_either_way(a: &[Heights], b: &[Heights]) -> bool {
    a.iter().any(|&ha| b.iter().any(|&hb| can_cross(ha, hb) || can_cross(hb, ha)))
}

/// The highest of these floors (-inf if none).
pub fn top_floor(hs: &[Heights]) -> f64 {
    hs.iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max)
}

/// Where flood fills and path-finding read the heights each sector can be at from.
pub trait HeightSource {
    fn heights(&mut self, sector: usize) -> PyResult<Rc<Vec<Heights>>>;
}

/// The plan search's heights: per sector, shared between the search's nodes.
pub struct Planned<'a>(pub &'a [Rc<Vec<Heights>>]);

impl HeightSource for Planned<'_> {
    fn heights(&mut self, sector: usize) -> PyResult<Rc<Vec<Heights>>> {
        Ok(Rc::clone(&self.0[sector]))
    }
}

/// What a use-door can be opened to, and from which polys (its activators).
pub struct DoorOpening {
    pub heights: Vec<Heights>,
    pub users: HashSet<usize>,
}

/// Doors that can be opened on the way, by sector.
pub type Doors = HashMap<usize, DoorOpening>;

/// A portal out of a poly that isn't ruled out whatever the heights.
#[derive(Clone, Copy)]
pub struct Edge {
    pub portal: usize,
    pub to: usize,
    pub from_sector: usize,
    pub to_sector: usize,
    pub linedef: Option<usize>,
}

/// A line near a portal that the box only gets past if the sectors round it are open.
pub struct Gate {
    pub line: usize,
    pub sectors: Vec<usize>,
}

pub enum TeleKind {
    /// a teleport line walked over from the front
    Line { line: usize, front: Option<usize>, back: Option<usize> },
    /// a jump: run up from `runup`, take off at `takeoff`; only with those sectors at those
    /// heights
    Jump { runup: Point, takeoff: Point, requires: Vec<(usize, Heights)> },
}

/// A way from a poly to a poly that isn't walking through a portal.
pub struct Tele {
    pub dest: usize,
    pub dest_pt: Point,
    pub kind: TeleKind,
    /// cost on top of the distance (a jump that doesn't always come off: less sure, dearer)
    pub penalty: f64,
}

/// The per-map facts the navigator works from, worked out by Python (evildoom.nav.reach).
#[derive(FromPyObject)]
#[pyo3(from_item_all)]
pub struct NavFacts {
    /// per poly: its statically passable portals
    pub adj: Vec<Vec<Edge>>,
    /// portal -> the lines near it in the box's way
    pub gates: HashMap<usize, Vec<Gate>>,
    /// per poly: teleports and jumps from it
    pub tele: Vec<Vec<Tele>>,
    /// per sector: its heights at the start
    pub initial: Vec<Heights>,
    /// per sector: does it hurt
    pub damage: Vec<bool>,
    pub has_teleports: bool,
    /// lines the box can't cross when shut (two-sided walls, doors)
    pub walls: HashSet<usize>,
    /// lines with a drop the player can't climb back up
    pub drops: HashSet<usize>,
    pub teleport_lines: HashSet<usize>,
    /// lines of sectors that shut by themselves (not to be counted as crossed)
    pub shutting: HashSet<usize>,
    /// cell -> one-sided walls (and the like) there
    pub static_grid: HashMap<Cell, Vec<Seg>>,
    /// cell -> the edges of solid things there
    pub obstacle_grid: HashMap<Cell, Vec<Seg>>,
}

/// The geometry questions the navigator asks back of Python, and their answers so far.
pub struct Callbacks {
    /// `portal_fits_without(portal, blocking lines)`: does the box get through the portal
    /// with those lines' sectors shut?
    pub fits: PyObject,
    /// `_landable(line, high sector, top)`: is there room to drop off the line from there?
    pub landable: PyObject,
    fits_cache: HashMap<(usize, Vec<usize>), bool>,
    landable_cache: HashMap<(usize, usize, i64), bool>,
}

impl Callbacks {
    pub fn new(fits: PyObject, landable: PyObject) -> Self {
        Callbacks { fits, landable, fits_cache: HashMap::default(), landable_cache: HashMap::default() }
    }

    fn landable(&mut self, py: Python<'_>, line: usize, high: usize, top: f64) -> PyResult<bool> {
        let key = (line, high, (top * 16.0).round() as i64);
        if let Some(&v) = self.landable_cache.get(&key) {
            return Ok(v);
        }
        let v: bool = self.landable.call1(py, (line, high, top))?.extract(py)?;
        self.landable_cache.insert(key, v);
        Ok(v)
    }

    fn fits(&mut self, py: Python<'_>, portal: usize, mut blocking: Vec<usize>) -> PyResult<bool> {
        if blocking.is_empty() {
            return Ok(true);
        }
        blocking.sort_unstable();
        let key = (portal, blocking);
        if let Some(&v) = self.fits_cache.get(&key) {
            return Ok(v);
        }
        let v: bool = self.fits.call1(py, (portal, key.1.clone()))?.extract(py)?;
        self.fits_cache.insert(key, v);
        Ok(v)
    }
}

/// A min-heap entry for best-first searches: (estimate, cost so far, node), lowest first.
#[derive(PartialEq)]
pub(crate) struct Open {
    pub f: f64,
    pub g: f64,
    pub id: usize,
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        // reversed: BinaryHeap pops the greatest
        o.f.total_cmp(&self.f).then_with(|| o.g.total_cmp(&self.g)).then_with(|| o.id.cmp(&self.id))
    }
}

pub struct NavCore {
    pub map: MapData,
    pub mesh: Mesh,
    /// per poly: its sector
    pub poly_sector: Vec<usize>,
    pub adj: Vec<Vec<Edge>>,
    pub gates: HashMap<usize, Vec<Gate>>,
    pub tele: Vec<Vec<Tele>>,
    pub initial: Vec<Heights>,
    pub damage: Vec<bool>,
    pub has_teleports: bool,
    pub walls: HashSet<usize>,
    pub drops: HashSet<usize>,
    pub teleport_lines: HashSet<usize>,
    pub shutting: HashSet<usize>,
    pub static_grid: HashMap<Cell, Vec<Seg>>,
    pub obstacle_grid: HashMap<Cell, Vec<Seg>>,
    /// a linedef's segment -> the linedef
    pub seg_lines: HashMap<SegKey, usize>,
    pub callbacks: Callbacks,
    /// what the player knows of the map, if not everything (`restrict`)
    pub known: Option<Known>,
}

/// The part of the map the player knows: the polys that can be walked into, and the
/// teleport lines whose destination is known.
pub struct Known {
    pub polys: Vec<bool>,
    pub teleports: HashSet<usize>,
}

impl NavCore {
    pub fn new(map: MapData, mesh: Mesh, facts: NavFacts, callbacks: Callbacks) -> Self {
        let poly_sector = mesh.polys.iter().map(|p| p.as_ref().map_or(0, |p| p.sector)).collect();
        let seg_lines = (0..map.linedefs.len()).map(|li| (seg_key(map.line_seg(li)), li)).collect();
        let NavFacts { adj, gates, tele, initial, damage, has_teleports, walls, drops, teleport_lines, shutting, static_grid, obstacle_grid } = facts;
        NavCore {
            map,
            mesh,
            poly_sector,
            adj,
            gates,
            tele,
            initial,
            damage,
            has_teleports,
            walls,
            drops,
            teleport_lines,
            shutting,
            static_grid,
            obstacle_grid,
            seg_lines,
            callbacks,
            known: None,
        }
    }

    /// Limit flood fills and path-finding to these polys and teleport lines (None: all).
    pub fn restrict(&mut self, polys: Option<HashSet<usize>>, teleports: Option<HashSet<usize>>) {
        self.known = polys.map(|ps| {
            let mut known = vec![false; self.mesh.polys.len()];
            for p in ps {
                if let Some(k) = known.get_mut(p) {
                    *k = true;
                }
            }
            Known { polys: known, teleports: teleports.unwrap_or_default() }
        });
    }

    /// May the player go into `poly` (does she know it's there)?
    fn enterable(&self, poly: usize) -> bool {
        self.known.as_ref().is_none_or(|k| k.polys.get(poly).copied().unwrap_or(false))
    }

    /// Does the player know teleport/jump `ti` from `poly` is there and where it goes?
    fn tele_known(&self, poly: usize, ti: usize) -> bool {
        let Some(known) = &self.known else { return true };
        let t = &self.tele[poly][ti];
        self.enterable(t.dest)
            && match t.kind {
                TeleKind::Line { line, .. } => known.teleports.contains(&line),
                TeleKind::Jump { .. } => true,
            }
    }

    pub fn sector_at(&self, p: Point) -> Option<usize> {
        self.mesh.poly_at(p).map(|i| self.poly_sector[i])
    }

    // --- passability ------------------------------------------------------------------------

    /// Sector `s`'s heights, and those its door opens to if it's one we can open (from `frm`,
    /// if given).
    fn heights_with_doors(st: &mut dyn HeightSource, doors: Option<&Doors>, s: usize, frm: Option<usize>) -> PyResult<Vec<Heights>> {
        let mut out = st.heights(s)?.to_vec();
        if let Some(door) = doors.and_then(|d| d.get(&s)) {
            if frm.is_none_or(|p| door.users.contains(&p)) {
                out.extend_from_slice(&door.heights);
            }
        }
        Ok(out)
    }

    /// Does the box get through `portal` from poly `frm` with the gate lines round it as the
    /// heights have them?
    fn fits_now(&mut self, py: Python<'_>, portal: usize, frm: usize, st: &mut dyn HeightSource, doors: Option<&Doors>) -> PyResult<bool> {
        let Some(gates) = self.gates.get(&portal) else { return Ok(true) };
        let sector = self.poly_sector[frm];
        let here = Self::heights_with_doors(st, doors, sector, None)?;
        let mut blocking = Vec::new();
        for gate in gates {
            for &s in &gate.sectors {
                if s == sector {
                    continue;
                }
                let there = Self::heights_with_doors(st, doors, s, Some(frm))?;
                if !here.iter().any(|&ha| there.iter().any(|&hb| can_cross(ha, hb))) {
                    blocking.push(gate.line);
                    break;
                }
            }
        }
        self.callbacks.fits(py, portal, blocking)
    }

    /// The height part of passability: a step or a drop we can make, with headroom.
    fn heights_passable(&mut self, py: Python<'_>, e: Edge, frm: usize, st: &mut dyn HeightSource, doors: Option<&Doors>) -> PyResult<bool> {
        let hs_a = Self::heights_with_doors(st, doors, e.from_sector, None)?;
        let hs_b = Self::heights_with_doors(st, doors, e.to_sector, Some(frm))?;
        for &ha in &hs_a {
            for &hb in &hs_b {
                if !can_cross(ha, hb) {
                    continue;
                }
                let landable = match e.linedef {
                    _ if ha.floor - hb.floor <= MAX_STEP => true,
                    None => true,
                    Some(li) => self.callbacks.landable(py, li, e.from_sector, ha.floor)?,
                };
                if landable {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Can edge `i` of `poly`'s adjacency be crossed now?
    fn edge_passable(&mut self, py: Python<'_>, poly: usize, i: usize, st: &mut dyn HeightSource, doors: Option<&Doors>) -> PyResult<bool> {
        let e = self.adj[poly][i];
        if self.gates.contains_key(&e.portal) && !self.fits_now(py, e.portal, poly, st, doors)? {
            return Ok(false);
        }
        Ok(e.from_sector == e.to_sector || self.heights_passable(py, e, poly, st, doors)?)
    }

    /// Can teleport/jump `ti` from `poly` be taken now?
    fn tele_ok(&self, poly: usize, ti: usize, st: &mut dyn HeightSource) -> PyResult<bool> {
        match &self.tele[poly][ti].kind {
            TeleKind::Jump { requires, .. } => {
                for (s, h) in requires {
                    if !st.heights(*s)?.contains(h) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            TeleKind::Line { front, back, .. } => {
                let (Some(front), Some(back)) = (*front, *back) else { return Ok(false) };
                let fs = st.heights(front)?;
                let bs = st.heights(back)?;
                Ok(fs.iter().any(|&fh| bs.iter().any(|&bh| can_cross(fh, bh))))
            }
        }
    }

    /// The polys reachable from `start` (flood fill).
    pub fn reachable(&mut self, py: Python<'_>, start: usize, st: &mut dyn HeightSource, doors: Option<&Doors>) -> PyResult<HashSet<usize>> {
        let mut seen: HashSet<usize> = HashSet::from_iter([start]);
        let mut todo = vec![start];
        while let Some(poly) = todo.pop() {
            for i in 0..self.adj[poly].len() {
                let to = self.adj[poly][i].to;
                if !seen.contains(&to) && self.enterable(to) && self.edge_passable(py, poly, i, st, doors)? {
                    seen.insert(to);
                    todo.push(to);
                }
            }
            for ti in 0..self.tele[poly].len() {
                let dest = self.tele[poly][ti].dest;
                if !seen.contains(&dest) && self.tele_known(poly, ti) && self.tele_ok(poly, ti, st)? {
                    seen.insert(dest);
                    todo.push(dest);
                }
            }
        }
        Ok(seen)
    }
}
