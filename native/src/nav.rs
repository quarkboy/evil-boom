// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The navigator core over a navmesh: which sector heights let the player across which
//! portals, and the flood fill of where they can get to. Path-finding is in `path`, path
//! smoothing and fitting the player's box along it in `fit`.

mod fit;
mod path;

pub use path::{Found, Via};

use crate::det::{HashMap, HashSet};
use crate::geom::{seg_key, Cell, Point, Seg, SegKey, MAX_STEP, PLAYER_HEIGHT};
use crate::map::MapData;
use crate::mesh::Mesh;
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::Arc;

/// A sector's floor and ceiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heights {
    pub floor: f64,
    pub ceiling: f64,
}

/// A switch's face on a line with a way through it: its front's upper part (between the two
/// ceilings), its lower part (between the two floors), or both, showing a switch's texture.
#[derive(Clone, Copy, Debug)]
pub struct SwitchFace {
    pub upper: bool,
    pub lower: bool,
    pub front: usize,
    pub back: usize,
}

/// a switch's face this far over the floor she stands on is out of her sight
pub const FACE_REACH: f64 = 96.0;

impl SwitchFace {
    /// May it be pressed by someone standing on a floor at `floor`, the line's two sectors at
    /// `f` and `b`? Where a part with the switch's texture is showing, only from where one
    /// such part is within sight: not far over her head, nor under her feet. Where none is
    /// showing (the two ceilings level, or the floors), there's no face to be out of sight
    /// of, and it's as any line: the texture's a leftover on a part never seen (E4M3's lifts
    /// carry one on an upper part that doesn't exist; what shows is the lift's side below),
    /// or the thing hasn't moved to show it yet.
    pub fn seen(self, f: Heights, b: Heights, floor: f64) -> bool {
        let parts = [
            (self.upper, b.ceiling, f.ceiling),
            (self.lower, f.floor, b.floor),
        ];
        let mut showing = parts
            .iter()
            .filter(|(is, lo, hi)| *is && hi > lo)
            .peekable();
        showing.peek().is_none()
            || showing.any(|&(_, lo, hi)| lo - FACE_REACH <= floor && floor <= hi)
    }

    /// `seen`, with any of the heights the three sectors can be at together.
    pub fn seen_any(self, f: &[Heights], b: &[Heights], here: &[Heights]) -> bool {
        f.iter().any(|&f| {
            b.iter()
                .any(|&b| here.iter().any(|h| self.seen(f, b, h.floor)))
        })
    }
}

/// Heights by value, for hash sets and maps.
pub type HeightsKey = (u64, u64);

/// Do the heights `new`, from a move that stays put, take a sector's ceiling up from `old` and
/// leave its floor? Then they're instead of the old ones, not beside them: a door opened to
/// stay isn't shut any more, and what next moves its floor moves it under the open door.
/// (Memento Mori MAP06's way out: a slab a line opens long before a switch lowers its sill,
/// and with the slab still down as one of its states the sill came down under 40 of room.)
pub fn opened_for_good(old: &[Heights], new: &[Heights]) -> bool {
    let top = old
        .iter()
        .map(|h| h.ceiling)
        .fold(f64::NEG_INFINITY, f64::max);
    !new.is_empty()
        && new
            .iter()
            .all(|h| h.ceiling > top && old.iter().all(|o| o.floor == h.floor))
}

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
    b.floor - a.floor <= MAX_STEP
        && a.ceiling.min(b.ceiling) - a.floor.max(b.floor) >= PLAYER_HEIGHT
}

/// Standing at heights `here`, may her box be over a line between sectors at `a` and `b`? Not
/// where there's no opening over it she fits (a post, bars, a door shut), nor where it's a face
/// more than a step high and she's below its top by more than a step.
#[inline]
pub fn box_over(here: Heights, a: Heights, b: Heights) -> bool {
    let top = a.floor.max(b.floor);
    let under = a.ceiling.min(b.ceiling);
    under - top >= PLAYER_HEIGHT
        && under - here.floor >= PLAYER_HEIGHT
        && ((a.floor - b.floor).abs() <= MAX_STEP || here.floor >= top - MAX_STEP)
}

/// Some heights of `a` and some of `b` the player can cross between, one way or the other.
fn crossable_either_way(a: &[Heights], b: &[Heights]) -> bool {
    a.iter()
        .any(|&ha| b.iter().any(|&hb| can_cross(ha, hb) || can_cross(hb, ha)))
}

/// The highest of these floors (-inf if none).
pub fn top_floor(hs: &[Heights]) -> f64 {
    hs.iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max)
}

/// Where flood fills and path-finding read the heights each sector can be at from.
pub trait HeightSource {
    fn heights(&mut self, sector: usize) -> Rc<Vec<Heights>>;
}

/// The plan search's heights: per sector, shared between the search's nodes.
pub struct Planned<'a>(pub &'a [Rc<Vec<Heights>>]);

impl HeightSource for Planned<'_> {
    fn heights(&mut self, sector: usize) -> Rc<Vec<Heights>> {
        Rc::clone(&self.0[sector])
    }
}

/// What a use-door can be opened to, and from which polys (its activators).
#[derive(Clone, Debug)]
pub struct DoorOpening {
    pub heights: Vec<Heights>,
    pub users: HashSet<usize>,
}

/// Doors that can be opened on the way, by sector.
pub type Doors = HashMap<usize, DoorOpening>;

/// The heights sectors take only once something has moved them for good, by sector: on one
/// visit she can go from its heights before to these (riding it as it moves), never back.
pub type AfterOnly = HashMap<usize, Vec<Heights>>;

/// A portal out of a poly that isn't ruled out whatever the heights.
#[derive(Clone, Copy)]
pub struct Edge {
    pub portal: usize,
    pub to: usize,
    pub from_sector: usize,
    pub to_sector: usize,
    pub linedef: Option<usize>,
    /// the teleport line it crosses from the front: open only once that's gone off for good
    pub teleport: Option<usize>,
}

/// A line near a portal that the box only gets past if the sectors round it are open.
pub struct Gate {
    pub line: usize,
    pub sectors: Vec<usize>,
}

pub enum TeleKind {
    /// a teleport line walked over from the front
    Line {
        line: usize,
        front: Option<usize>,
        back: Option<usize>,
    },
    /// a jump: run up from `runup`, take off at `takeoff`; only with those sectors at those
    /// heights
    Jump {
        runup: Point,
        takeoff: Point,
        requires: Vec<(usize, Heights)>,
        rising: Option<RisingTakeoff>,
    },
}

/// When a rising floor is close enough to the landing to plan the approach.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct RisingTakeoff {
    pub sector: usize,
    pub lineup_floor: f64,
    /// a lift works the floor (a line that takes it down and lets it come back): it goes by
    /// the takeoff's height each time it's called, whether or not she's seen it there
    #[serde(default)]
    pub lift: bool,
}

impl RisingTakeoff {
    /// Bound a detour and the time spent waiting on a platform. At Doom's
    /// ordinary rising-floor speed (one unit per tic) this is also the maximum
    /// distance below the landing at which it is worth lining up.
    pub(crate) const WAIT_TICS: i64 = 4 * 35;

    /// A flight across a gap needs the departure height the simulator checked.
    /// A fast floor may pass it by up to four units in one tic.
    pub fn at_edge(target: Heights, floor: f64, ceiling: f64) -> bool {
        floor >= target.floor
            && floor <= target.floor + 4.0
            && ceiling - floor.max(target.floor) >= crate::geom::PLAYER_HEIGHT
    }
}

pub(crate) fn jump_height(
    rising: Option<RisingTakeoff>,
    sector: usize,
    target: Heights,
    states: &[Heights],
) -> bool {
    let Some(ride) = rising.filter(|r| r.sector == sector) else {
        return states.contains(&target);
    };
    states.iter().any(|h| {
        h.floor >= ride.lineup_floor
            && h.floor <= target.floor + 4.0
            && h.ceiling - h.floor.max(target.floor) >= crate::geom::PLAYER_HEIGHT
    })
    // Or a lift's floor with a height below the takeoff's and one at or above it, both now:
    // down and due back up. It goes by the takeoff's height between the two, with her on it
    // if she's there by then. (TNT MAP08: four
    // pillars, each raised to the ceiling by one switch and worked as a lift by another, are
    // the way onto the four ledges of the last hall, jumped off as they pass.)
        || (ride.lift
            && states.iter().any(|h| {
                h.floor < target.floor && h.ceiling - target.floor >= crate::geom::PLAYER_HEIGHT
            })
            && states.iter().any(|h| h.floor >= target.floor))
}

/// A way from a poly to a poly that isn't walking through a portal.
pub struct Tele {
    pub dest: usize,
    pub dest_pt: Point,
    pub kind: TeleKind,
    /// cost on top of the distance (a jump that doesn't always come off: less sure, dearer)
    pub penalty: f64,
}

/// The per-map facts the flood fill and path-finding work from (the navigator's analysis).
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
    /// lines that set something off walked over, a path mustn't so much as graze unless it means
    /// to cross them: teleports, and exits (a pad's edges can all be exits)
    pub trip_lines: HashSet<usize>,
    /// lines of sectors that shut by themselves (not to be counted as crossed)
    pub shutting: HashSet<usize>,
    /// cell -> one-sided walls (and the like) there
    pub static_grid: HashMap<Cell, Vec<Seg>>,
    /// cell -> the edges of solid things there
    pub obstacle_grid: HashMap<Cell, Vec<Seg>>,
}

/// Is there room for the box past these gate lines at this portal: `fits(portal, blocking
/// lines)` (the lines sorted, never none).
pub type FitsFn = Box<dyn Fn(usize, &[usize]) -> bool>;
/// Is there room to drop off line `line` from sector `high`, whose floor is at `top`:
/// `landable(line, high, top)`.
pub type LandableFn = Box<dyn Fn(usize, usize, f64) -> bool>;
/// Where along `line` there's room to drop off it from sector `high`, whose floor is at `top`:
/// `landing_span(line, high, top)`, two points on the line (none: nowhere).
pub type SpanFn = Box<dyn Fn(usize, usize, f64) -> Option<(Point, Point)>>;
/// A stretch of a line (none: no such stretch).
type Stretch = Option<(Point, Point)>;
/// Walked from a to b with the sectors at these (floor, ceiling) heights, as the game moves a
/// player: the share of tries that get there (`JumpLab::walks`).
pub type WalksFn = Box<dyn Fn(Point, Point, &[(f64, f64)]) -> f64>;
/// A stretch walked: its ends (bits) and a hash of the heights.
type WalkKey = (u64, u64, u64, u64, u64);

/// The geometry questions the flood fill asks of the level's analysis (navigator), and their
/// answers so far.
pub struct Callbacks {
    fits: FitsFn,
    landable: LandableFn,
    span: SpanFn,
    span_cache: std::cell::RefCell<HashMap<(usize, usize, i64), Stretch>>,
    walks: WalksFn,
    /// by the stretch's ends and a hash of the heights it was walked at
    walks_cache: std::cell::RefCell<HashMap<WalkKey, f64>>,
    fits_cache: HashMap<(usize, Vec<usize>), bool>,
    landable_cache: HashMap<(usize, usize, i64), bool>,
}

impl Callbacks {
    pub fn new(fits: FitsFn, landable: LandableFn, span: SpanFn, walks: WalksFn) -> Self {
        Callbacks {
            fits,
            landable,
            span,
            span_cache: std::cell::RefCell::new(HashMap::default()),
            walks,
            walks_cache: std::cell::RefCell::new(HashMap::default()),
            fits_cache: HashMap::default(),
            landable_cache: HashMap::default(),
        }
    }

    /// `SpanFn`.
    pub(crate) fn landing_span(
        &self,
        line: usize,
        high: usize,
        top: f64,
    ) -> Option<(Point, Point)> {
        let key = (line, high, (top * 16.0).round() as i64);
        if let Some(&v) = self.span_cache.borrow().get(&key) {
            return v;
        }
        let v = (self.span)(line, high, top);
        self.span_cache.borrow_mut().insert(key, v);
        v
    }

    /// `WalksFn`.
    pub(crate) fn walks(&self, a: Point, b: Point, heights: &[(f64, f64)]) -> f64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (f, c) in heights {
            (f.to_bits(), c.to_bits()).hash(&mut h);
        }
        let key = (
            a.0.to_bits(),
            a.1.to_bits(),
            b.0.to_bits(),
            b.1.to_bits(),
            h.finish(),
        );
        if let Some(&v) = self.walks_cache.borrow().get(&key) {
            return v;
        }
        let v = (self.walks)(a, b, heights);
        self.walks_cache.borrow_mut().insert(key, v);
        v
    }

    fn landable(&mut self, line: usize, high: usize, top: f64) -> bool {
        let key = (line, high, (top * 16.0).round() as i64);
        if let Some(&v) = self.landable_cache.get(&key) {
            return v;
        }
        let v = (self.landable)(line, high, top);
        self.landable_cache.insert(key, v);
        v
    }

    fn fits(&mut self, portal: usize, mut blocking: Vec<usize>) -> bool {
        if blocking.is_empty() {
            return true;
        }
        blocking.sort_unstable();
        let key = (portal, blocking);
        if let Some(&v) = self.fits_cache.get(&key) {
            return v;
        }
        let v = (self.fits)(portal, &key.1);
        self.fits_cache.insert(key, v);
        v
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
        o.f.total_cmp(&self.f)
            .then_with(|| o.g.total_cmp(&self.g))
            .then_with(|| o.id.cmp(&self.id))
    }
}

pub struct NavCore {
    pub map: Arc<MapData>,
    pub mesh: Arc<Mesh>,
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
    pub trip_lines: HashSet<usize>,
    pub shutting: HashSet<usize>,
    pub static_grid: HashMap<Cell, Vec<Seg>>,
    pub obstacle_grid: HashMap<Cell, Vec<Seg>>,
    /// a linedef's segment -> the linedef
    pub seg_lines: HashMap<SegKey, usize>,
    pub callbacks: Callbacks,
    /// what the player knows of the map, if not everything (`restrict`)
    pub known: Option<Known>,
    /// per poly: how many times its length a way across it counts for (in sight of something
    /// she'd rather not be seen by); empty: none
    pub exposure: Vec<f64>,
    /// line -> (sector, units): crossing the line into the sector costs that much more (a
    /// crusher she'd set off going in, stopped too low to be under when it starts)
    pub entry_cost: HashMap<usize, (usize, f64)>,
    /// sector -> does its floor hurt now: those she's seen change since the level began (a
    /// pit risen into plain floor, a floor lowered into slime), for which `damage` is out of date
    pub floor_changed: HashMap<usize, bool>,
}

/// The part of the map the player knows: the polys that can be walked into, and the
/// teleport lines whose destination is known.
pub struct Known {
    pub polys: Vec<bool>,
    pub teleports: HashSet<usize>,
    /// teleport lines that no longer teleport (walked over like any other)
    pub spent: HashSet<usize>,
    /// (poly, teleport index) edges not to take
    pub banned: HashSet<(usize, usize)>,
}

impl NavCore {
    pub fn new(map: Arc<MapData>, mesh: Arc<Mesh>, facts: NavFacts, callbacks: Callbacks) -> Self {
        let poly_sector = mesh
            .polys
            .iter()
            .map(|p| p.as_ref().map_or(0, |p| p.sector))
            .collect();
        let seg_lines = (0..map.linedefs.len())
            .map(|li| (seg_key(map.line_seg(li)), li))
            .collect();
        let NavFacts {
            adj,
            gates,
            tele,
            initial,
            damage,
            has_teleports,
            walls,
            drops,
            trip_lines,
            shutting,
            static_grid,
            obstacle_grid,
        } = facts;
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
            trip_lines,
            shutting,
            static_grid,
            obstacle_grid,
            seg_lines,
            callbacks,
            known: None,
            exposure: Vec::new(),
            entry_cost: HashMap::default(),
            floor_changed: HashMap::default(),
        }
    }

    /// Limit flood fills and path-finding to these polys and teleport lines (None: all), walking
    /// over the `spent` teleport lines. `banned`: (poly, teleport index) edges she won't take
    /// (jumps she keeps missing).
    pub fn restrict(
        &mut self,
        polys: Option<HashSet<usize>>,
        teleports: Option<HashSet<usize>>,
        spent: HashSet<usize>,
        banned: HashSet<(usize, usize)>,
    ) {
        self.known = polys.map(|ps| {
            let mut known = vec![false; self.mesh.polys.len()];
            for p in ps {
                if let Some(k) = known.get_mut(p) {
                    *k = true;
                }
            }
            Known {
                polys: known,
                teleports: teleports.unwrap_or_default(),
                spent,
                banned,
            }
        });
    }

    /// May the player go into `poly` (does she know it's there)?
    fn enterable(&self, poly: usize) -> bool {
        self.known
            .as_ref()
            .is_none_or(|k| k.polys.get(poly).copied().unwrap_or(false))
    }

    /// Does the player know teleport/jump `ti` from `poly` is there and where it goes?
    fn tele_known(&self, poly: usize, ti: usize) -> bool {
        let Some(known) = &self.known else {
            return true;
        };
        let t = &self.tele[poly][ti];
        !known.banned.contains(&(poly, ti))
            && self.enterable(t.dest)
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
    fn heights_with_doors(
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
        s: usize,
        frm: Option<usize>,
    ) -> Rc<Vec<Heights>> {
        // (the sector's own list, shared, where no door adds to it: this is asked at every
        // edge of every flood, and a copy made each time was a tenth of a flood's cost)
        let own = st.heights(s);
        match doors.and_then(|d| d.get(&s)) {
            Some(door) if frm.is_none_or(|p| door.users.contains(&p)) => {
                let mut out = own.to_vec();
                out.extend_from_slice(&door.heights);
                Rc::new(out)
            }
            _ => own,
        }
    }

    /// Does the box get through `portal` from poly `frm` with the gate lines round it as the
    /// heights have them?
    fn fits_now(
        &mut self,
        portal: usize,
        frm: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
    ) -> bool {
        let Some(gates) = self.gates.get(&portal) else {
            return true;
        };
        let sector = self.poly_sector[frm];
        let here = Self::heights_with_doors(st, doors, sector, None);
        let mut blocking = Vec::new();
        for gate in gates {
            for &s in &gate.sectors {
                if s == sector {
                    continue;
                }
                let there = Self::heights_with_doors(st, doors, s, Some(frm));
                if !here
                    .iter()
                    .any(|&ha| there.iter().any(|&hb| can_cross(ha, hb)))
                {
                    blocking.push(gate.line);
                    break;
                }
            }
        }
        self.callbacks.fits(portal, blocking)
    }

    /// The height part of passability: a step or a drop we can make, with headroom.
    fn heights_passable(
        &mut self,
        e: Edge,
        frm: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
    ) -> bool {
        let hs_a = Self::heights_with_doors(st, doors, e.from_sector, None);
        let hs_b = Self::heights_with_doors(st, doors, e.to_sector, Some(frm));
        for &ha in hs_a.iter() {
            for &hb in hs_b.iter() {
                if self.lands(e, ha, hb) {
                    return true;
                }
            }
        }
        false
    }

    /// From heights `ha` on edge `e`'s near side to `hb` on its far one: through, and onto it.
    fn lands(&mut self, e: Edge, ha: Heights, hb: Heights) -> bool {
        if !can_cross(ha, hb) {
            return false;
        }
        match e.linedef {
            _ if ha.floor - hb.floor <= MAX_STEP => true,
            None => true,
            Some(li) => {
                self.callbacks.landable(li, e.from_sector, ha.floor)
                    && self.drop_reaches(e, li, ha.floor)
            }
        }
    }

    /// Is any of edge `e`'s portal within the stretch of its line she can drop off from a floor
    /// at `top` (`Callbacks::landing_span`)? A long ledge is several portals, and the pieces
    /// of it under a low ceiling beyond are no way down at all.
    fn drop_reaches(&self, e: Edge, li: usize, top: f64) -> bool {
        let Some((a, b)) = self.callbacks.landing_span(li, e.from_sector, top) else {
            return true;
        };
        let q = &self.mesh.portals[e.portal];
        let (dx, dy) = (q.p2.0 - q.p1.0, q.p2.1 - q.p1.1);
        let len = dx.hypot(dy).max(1e-9);
        let along = |p: Point| ((p.0 - q.p1.0) * dx + (p.1 - q.p1.1) * dy) / len;
        let (ta, tb) = (along(a), along(b));
        ta.max(tb).min(len) - ta.min(tb).max(0.0) > 0.5
    }

    /// The ways edge `e` from `frm` can be crossed, coming from its sector at its heights after
    /// a move for good (`late`) or at any: for each, whether she's at the far sector's after it.
    fn phased_crossings(
        &mut self,
        e: Edge,
        frm: usize,
        late: bool,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
        after: &AfterOnly,
    ) -> [bool; 2] {
        let mut out = [false; 2];
        if !self.open(e)
            || (self.gates.contains_key(&e.portal) && !self.fits_now(e.portal, frm, st, doors))
        {
            return out;
        }
        if e.from_sector == e.to_sector {
            out[usize::from(late)] = true;
            return out;
        }
        let late_a = after.get(&e.from_sector);
        let hs_a: Vec<Heights> = Self::heights_with_doors(st, doors, e.from_sector, None)
            .iter()
            .copied()
            .filter(|h| !late || late_a.is_none_or(|v| v.contains(h)))
            .collect();
        let hs_b = Self::heights_with_doors(st, doors, e.to_sector, Some(frm));
        let late_b = after.get(&e.to_sector);
        for &ha in &hs_a {
            for &hb in hs_b.iter() {
                if self.lands(e, ha, hb) {
                    out[usize::from(late_b.is_some_and(|v| v.contains(&hb)))] = true;
                }
            }
        }
        out
    }

    /// Is edge `e` there to cross at all: not over a teleport line's front, unless she knows it
    /// no longer teleports?
    fn open(&self, e: Edge) -> bool {
        e.teleport
            .is_none_or(|t| self.known.as_ref().is_some_and(|k| k.spent.contains(&t)))
    }

    /// Can edge `i` of `poly`'s adjacency be crossed now?
    fn edge_passable(
        &mut self,
        poly: usize,
        i: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
    ) -> bool {
        let e = self.adj[poly][i];
        if !self.open(e) {
            return false;
        }
        if self.gates.contains_key(&e.portal) && !self.fits_now(e.portal, poly, st, doors) {
            return false;
        }
        e.from_sector == e.to_sector || self.heights_passable(e, poly, st, doors)
    }

    /// Can teleport/jump `ti` from `poly` be taken now?
    fn tele_ok(&self, poly: usize, ti: usize, st: &mut dyn HeightSource) -> bool {
        match &self.tele[poly][ti].kind {
            TeleKind::Jump {
                requires, rising, ..
            } => {
                for (s, h) in requires {
                    if !jump_height(*rising, *s, *h, &st.heights(*s)) {
                        return false;
                    }
                }
                true
            }
            TeleKind::Line { front, back, .. } => {
                let (Some(front), Some(back)) = (*front, *back) else {
                    return false;
                };
                let fs = st.heights(front);
                let bs = st.heights(back);
                fs.iter().any(|&fh| bs.iter().any(|&bh| can_cross(fh, bh)))
            }
        }
    }

    /// The polys reachable from `start` (flood fill).
    pub fn reachable(
        &mut self,
        start: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
    ) -> HashSet<usize> {
        let mut seen: HashSet<usize> = HashSet::from_iter([start]);
        let mut todo = vec![start];
        while let Some(poly) = todo.pop() {
            for i in 0..self.adj[poly].len() {
                let to = self.adj[poly][i].to;
                if !seen.contains(&to)
                    && self.enterable(to)
                    && self.edge_passable(poly, i, st, doors)
                {
                    seen.insert(to);
                    todo.push(to);
                }
            }
            for ti in 0..self.tele[poly].len() {
                let dest = self.tele[poly][ti].dest;
                if !seen.contains(&dest) && self.tele_known(poly, ti) && self.tele_ok(poly, ti, st)
                {
                    seen.insert(dest);
                    todo.push(dest);
                }
            }
        }
        seen
    }

    /// Does the flood fill from `start` get to a poly that `stop` picks (`start` among them)?
    /// As `reachable`, but done as soon as it does: asking whether somewhere can be got back
    /// from needs no more than the first way back. (Gone on from nearest the likely way back
    /// first, a queue by distance, was tried: no quicker on E2M7, MAP14 or MAP15, the first
    /// way back being a few polys off as a rule; left plain.)
    pub fn reaches(
        &mut self,
        start: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
        stop: &dyn Fn(usize) -> bool,
    ) -> bool {
        if stop(start) {
            return true;
        }
        let mut seen: HashSet<usize> = HashSet::from_iter([start]);
        let mut todo = vec![start];
        while let Some(poly) = todo.pop() {
            for i in 0..self.adj[poly].len() {
                let to = self.adj[poly][i].to;
                if !seen.contains(&to)
                    && self.enterable(to)
                    && self.edge_passable(poly, i, st, doors)
                {
                    if stop(to) {
                        return true;
                    }
                    seen.insert(to);
                    todo.push(to);
                }
            }
            for ti in 0..self.tele[poly].len() {
                let dest = self.tele[poly][ti].dest;
                if !seen.contains(&dest) && self.tele_known(poly, ti) && self.tele_ok(poly, ti, st)
                {
                    if stop(dest) {
                        return true;
                    }
                    seen.insert(dest);
                    todo.push(dest);
                }
            }
        }
        false
    }

    /// How the flood fill gets from `start` to `goal`, if it does: the polys on the way, each with
    /// how it was entered (a portal, a teleport or jump), for tracing what it takes for reachable.
    pub fn reach_chain(
        &mut self,
        start: usize,
        goal: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
    ) -> Option<Vec<(usize, Option<Via>)>> {
        let mut came: HashMap<usize, (usize, Via)> = HashMap::default();
        let mut seen: HashSet<usize> = HashSet::from_iter([start]);
        let mut todo = std::collections::VecDeque::from([start]);
        while let Some(poly) = todo.pop_front() {
            if poly == goal {
                let mut chain = vec![(goal, came.get(&goal).map(|c| c.1))];
                let mut at = goal;
                while let Some(&(from, _)) = came.get(&at) {
                    chain.push((from, came.get(&from).map(|c| c.1)));
                    at = from;
                }
                chain.reverse();
                return Some(chain);
            }
            for i in 0..self.adj[poly].len() {
                let to = self.adj[poly][i].to;
                if !seen.contains(&to)
                    && self.enterable(to)
                    && self.edge_passable(poly, i, st, doors)
                {
                    seen.insert(to);
                    came.insert(to, (poly, Via::Portal(self.adj[poly][i].portal)));
                    todo.push_back(to);
                }
            }
            for ti in 0..self.tele[poly].len() {
                let dest = self.tele[poly][ti].dest;
                if !seen.contains(&dest) && self.tele_known(poly, ti) && self.tele_ok(poly, ti, st)
                {
                    seen.insert(dest);
                    came.insert(dest, (poly, Via::Tele(ti)));
                    todo.push_back(dest);
                }
            }
        }
        None
    }

    /// The polys reachable from `start`, no visit to a sector moved for good going from its
    /// heights after back to those before (a lift lowered for good to a room's floor is got on
    /// from the room, not off at its old height onto the ledge beside).
    pub fn reachable_phased(
        &mut self,
        start: usize,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
        after: &AfterOnly,
    ) -> HashSet<usize> {
        if after.is_empty() {
            return self.reachable(start, st, doors);
        }
        let mut seen: HashSet<(usize, bool)> = HashSet::from_iter([(start, false)]);
        let mut out: HashSet<usize> = HashSet::from_iter([start]);
        let mut todo = vec![(start, false)];
        while let Some((poly, late)) = todo.pop() {
            for i in 0..self.adj[poly].len() {
                let e = self.adj[poly][i];
                if !self.enterable(e.to) {
                    continue;
                }
                for (phase, ok) in self
                    .phased_crossings(e, poly, late, st, doors, after)
                    .into_iter()
                    .enumerate()
                {
                    if ok && seen.insert((e.to, phase == 1)) {
                        out.insert(e.to);
                        todo.push((e.to, phase == 1));
                    }
                }
            }
            // (on a sector at its heights after a move for good: only a jump found from those, not
            // one from its heights before, as all the rest were)
            let late_here = if late {
                after.get(&self.poly_sector[poly])
            } else {
                None
            };
            for ti in 0..self.tele[poly].len() {
                let dest = self.tele[poly][ti].dest;
                if let Some(hs) = late_here {
                    let from_after = match &self.tele[poly][ti].kind {
                        TeleKind::Jump {
                            requires, rising, ..
                        } => requires.iter().any(|(s, h)| {
                            *s == self.poly_sector[poly] && jump_height(*rising, *s, *h, hs)
                        }),
                        TeleKind::Line { .. } => true,
                    };
                    if !from_after {
                        continue;
                    }
                }
                // (landed: at whatever heights, as far as this goes)
                if !seen.contains(&(dest, false))
                    && self.tele_known(poly, ti)
                    && self.tele_ok(poly, ti, st)
                {
                    seen.insert((dest, false));
                    out.insert(dest);
                    todo.push((dest, false));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod rising_takeoff_tests {
    use super::*;

    #[test]
    fn the_rider_can_line_up_before_it_is_time_to_leave() {
        let target = Heights {
            floor: 64.0,
            ceiling: 136.0,
        };
        let ride = Some(RisingTakeoff {
            lift: false,
            sector: 1,
            lineup_floor: -55.0,
        });
        let observed = |floor| Heights { floor, ..target };
        assert!(!jump_height(ride, 1, target, &[observed(-56.0)]));
        assert!(jump_height(ride, 1, target, &[observed(-55.0)]));
        assert!(!jump_height(ride, 0, target, &[observed(-55.0)]));
        assert!(!RisingTakeoff::at_edge(target, 40.0, 136.0));
        assert!(RisingTakeoff::at_edge(target, 64.0, 136.0));
        assert!(!RisingTakeoff::at_edge(target, 64.0, 100.0));
        assert!(!RisingTakeoff::at_edge(target, 72.0, 136.0));
    }
}
