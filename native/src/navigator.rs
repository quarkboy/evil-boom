//! The level's analysis for getting about, and the questions asked of it.
//!
//! Heights are kept apart from the mesh. A world (`States`) is, per sector, the heights it can
//! be at: at run time one, the live heights; for planning, also every height a mover she can
//! set off could put it at. A portal is passable if some pair allows it, by vanilla's rules:
//! a step up of at most 24, any drop, an opening of at least 56.
//!
//! - `analysis`: the triggers (switches, walk-over and gun lines, boss deaths, exit floors),
//!   from where each can be set off, teleports, trenches.
//! - `widths`: what the player's box fits through, and walls that never move.
//! - `jumps`: run-off jumps, found once (cached) and checked in the movement simulator.
//! - `view`: what she knows of it; flood fills, paths and plans through that.

mod analysis;
mod jumps;
mod view;
mod widths;

pub use jumps::{JumpChecks, JUMP_DOUBT};
pub use view::{Leg, Path, Plan, Route, View};

use crate::det::HashMap;
use crate::geom::{dist, Cell, Point, Seg};
use crate::jumps::JumpLab;
use crate::map::{MapData, ML_BLOCKING, ML_BLOCKPLAYERS};
use crate::mesh::{Mesh, Obstacle};
use crate::movers::{Effects, MoverModel};
use crate::nav::{can_cross, DoorOpening, Doors, Heights};
use crate::specials::{classify_sector, Category, Key, Keys, LineSpecial, Trigger};
use crate::things::{shootable_obstacle, solid_radius};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

pub use crate::geom::box_hits_segment;

pub use crate::geom::{MAX_STEP, PATH_CLEARANCE, PLAYER_HEIGHT, PLAYER_RADIUS};
pub const ITEM_RADIUS: f64 = 20.0;
/// an item this far under her feet is still hers to pick up (P_TouchSpecialThing)
pub const PICKUP_BELOW: f64 = 8.0;
/// how far inside touching range a pickup spot must be
pub const PICKUP_MARGIN: f64 = 2.0;
/// bullets leave the player at z + 28 + 8, and level: no aiming up or down
pub const SHOT_HEIGHT: f64 = 36.0;
/// narrower portals are nodebuilder slivers, never the real way through
pub const MIN_PORTAL: f64 = 4.0;
/// pseudo-trigger lines below this: exit floors
pub const EXIT_FLOOR: i64 = -1000;
/// degrees a gun line must span from where it's shot (aim isn't exact: she faces it to within a
/// third of that, and taps, the first shot going true; MAP16's flame panel, 3.3 across from the
/// table, hit from its edge at 3)
pub const MIN_SHOT_SPAN: f64 = 3.0;
/// the box must clear walls by this much: no one threads a gap exactly 32 wide
pub const BOX_SLACK: f64 = 0.25;
/// use traces 64 from the player's centre; keep a margin
pub const USE_REACH: f64 = 48.0;
pub const TELEPORT_DEST: u16 = 14;
pub const TELEPORTERS_FOR_PLAYERS: [u16; 8] = [39, 97, 174, 195, 207, 208, 209, 210];
/// walk-over teleporters monsters set off
pub const TELEPORTERS_FOR_MONSTERS: [u16; 6] = [39, 97, 125, 126, 207, 208];

/// Per sector: the heights it can be at.
pub type States = Vec<Vec<Heights>>;
/// key -> (poly, point) spots it can be picked up from (in the order the map has them)
pub type KeyItems = Vec<(Key, Vec<(usize, Point)>)>;

/// Add `h` to `hs` unless it's there.
pub fn add_height(hs: &mut Vec<Heights>, h: Heights) {
    if !hs.contains(&h) {
        hs.push(h);
    }
}

/// The highest of these heights, as Python's max() over (floor, ceiling) tuples.
pub fn max_heights(hs: &[Heights]) -> Option<Heights> {
    hs.iter().copied().max_by(|a, b| a.floor.total_cmp(&b.floor).then(a.ceiling.total_cmp(&b.ceiling)))
}

/// A line that sets something off, or something like one (a boss's death, an exit floor).
#[derive(Clone, Debug)]
pub struct TriggerLine {
    /// its place in the navigator's triggers (the same in every view)
    pub id: usize,
    /// its linedef; negative for the pseudo ones (bosses: -1 - i; exit floors: EXIT_FLOOR - sector)
    pub line: i64,
    pub info: LineSpecial,
    pub sectors: Vec<usize>,
    pub effects: Effects,
    /// polys from which it can be set off (used, shot, crossed)
    pub activators: BTreeSet<usize>,
    /// Stairs build up from where the tagged sector is now: several switches for the same
    /// stairs each raise them another step (MAP21's four). (start sector, its first floor).
    pub shift_from: Option<(usize, f64)>,
    /// Its target is worked out from the heights round it when it fires (MAP27's "lower to
    /// highest neighbour + 8" once the platforms round it are down): the sectors it depends on.
    pub relevant: Option<Vec<usize>>,
    /// in a view: only its effects on these sectors are known (None: all)
    pub keep: Option<BTreeSet<usize>>,
}

impl TriggerLine {
    /// Its linedef, if it's a real line.
    pub fn linedef(&self) -> Option<usize> {
        usize::try_from(self.line).ok()
    }

    pub fn once(&self) -> bool {
        self.info.trigger.once()
    }

    /// Firing it again can do something new (so once-only ones are tracked as used).
    pub fn cumulative(&self) -> bool {
        self.shift_from.is_some() || self.relevant.is_some()
    }

    /// Its moves stay put (floors, stairs, doors that stay open), unlike lifts, crushers and
    /// doors that close again: once fired, the old heights are gone.
    pub fn permanent(&self) -> bool {
        !matches!(self.info.category, Category::Lift | Category::Crusher) && !self.info.description.contains("wait")
    }

    /// The heights it moves `s` to, if it moves it.
    pub fn effect(&self, s: usize) -> Option<&[Heights]> {
        self.effects.iter().find(|(x, _)| *x == s).map(|(_, hs)| hs.as_slice())
    }
}

/// A teleport line and where it goes.
#[derive(Clone, Debug)]
pub struct Teleport {
    pub line: usize,
    pub src_polys: BTreeSet<usize>,
    pub dest_poly: usize,
    pub dest: Point,
}

/// A run-off jump: sprint from `runup` through `takeoff` (a ledge's edge) and land at `dest`;
/// a one-way edge like a teleport.
#[derive(Clone, Debug)]
pub struct Jump {
    pub src_polys: BTreeSet<usize>,
    pub dest_poly: usize,
    pub dest: Point,
    pub runup: Point,
    pub takeoff: Point,
    pub speed: f64,
    /// (sector, heights) of movable sectors it depends on, sorted
    pub requires: Vec<(usize, Heights)>,
    /// sprint only to the edge, then coast (narrow landings)
    pub soft: bool,
    /// ...soft: let go of forward this far past the takeoff
    pub release: f64,
    /// sprint straight across a strip too narrow to stop on, to `dest`
    pub through: bool,
    /// how sure (0-1): made a little differently, the share that comes off
    pub sure: f64,
    /// run strafing, the faster (jumps::Jump::strafe)
    pub strafe: bool,
}

impl Jump {
    /// Which way to face to go along `heading`: straight on, or for a strafing run off it.
    pub fn facing(&self, heading: f64) -> f64 {
        if self.strafe {
            heading + crate::jumps::STRAFE_FACING
        } else {
            heading
        }
    }

    /// How hard to strafe (right) on the run: none, or a run key's sidestep.
    pub fn sidestep(&self) -> f64 {
        if self.strafe {
            crate::jumps::STRAFE_SIDE
        } else {
            0.0
        }
    }

    pub fn usable(&self, states: &States) -> bool {
        self.requires.iter().all(|(s, h)| states[*s].contains(h))
    }

    /// Its identity (for caching checks).
    pub fn key(&self) -> String {
        let (r, t, d) = (self.runup, self.takeoff, self.dest);
        format!("{:.0},{:.0}>{:.0},{:.0}>{:.0},{:.0}", r.0, r.1, t.0, t.1, d.0, d.1)
    }
}

/// A way from a poly that isn't a portal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeleEdge {
    /// navigator.teleports[i]
    Teleport(usize),
    /// navigator.jumps[i]
    Jump(usize),
}

/// (trigger id, [(sector, floor bits, ceiling bits)]): a trigger with the heights round it.
type DynKey = (usize, Vec<(usize, u64, u64)>);

/// What the navigator holds for the questions asked of it that change as it's used.
#[derive(Default)]
struct Caches {
    /// (line, high sector, top) -> room to drop
    landable: HashMap<(usize, usize, u64), bool>,
    /// (trigger, reps) -> its effects with the world like that
    dyn_effects: HashMap<DynKey, Effects>,
}

pub struct Navigator {
    pub map: Arc<MapData>,
    pub mesh: Arc<Mesh>,
    /// 1-5: which of the map's things are there
    pub skill: i32,
    /// solid decorations cut out of the mesh, grown by the player's half-width
    pub mesh_obstacles: Vec<Obstacle>,
    pub movers: Arc<MoverModel>,
    /// trench sector -> the sectors round it
    pub trenches: BTreeMap<usize, Vec<usize>>,
    /// per sector: the damage its floor does
    pub damage: Vec<i32>,
    /// find_path refuses paths the box (maybe) can't get through (else the planner prefers
    /// paths with fewer squeezes)
    pub strict_fit: bool,
    /// (switch line, poly) -> sectors that must be open for the use trace
    pub use_gates: HashMap<(usize, usize), BTreeSet<usize>>,
    /// (switch line, poly) -> (front, back) sectors the use trace passes between, shut at the
    /// start by their heights, that must have a gap between them for the press
    pub use_line_gates: HashMap<(usize, usize), BTreeSet<(usize, usize)>>,
    /// (gun line, poly) -> floors to stand on there for a level shot to hit it
    pub shot_floors: HashMap<(usize, usize), (f64, f64)>,
    pub triggers: Vec<TriggerLine>,
    pub teleports: Vec<Teleport>,
    pub jumps: Vec<Jump>,
    /// per poly: its teleports, then its jumps
    pub tele_from: Vec<Vec<TeleEdge>>,
    /// per portal: does the box fit through somewhere
    pub portal_fits: Vec<bool>,
    /// what the box gets through when lines of moving sectors near portals block
    fit: Rc<widths::PortalFit>,
    line_sets: widths::LineSets,
    /// cell -> static walls (sorted, unique)
    static_wall_grid: HashMap<Cell, Vec<Seg>>,
    /// cell -> the box edges of solid things
    obstacle_grid: HashMap<Cell, Vec<Seg>>,
    /// lines that block sight (one-sided, shut at the start), by cell
    sight_grid: HashMap<Cell, Vec<(Point, Point, usize)>>,
    /// sectors moving under the player once stepped on
    pub sinking: BTreeSet<usize>,
    pub jump_lab: Arc<JumpLab>,
    caches: RefCell<Caches>,
    /// the flood fill, path-finding and plan core, set up for one view at a time
    core: RefCell<view::Core>,
}

impl Navigator {
    /// The level's analysis (`keep_verified` then drops the jumps that don't come off; share
    /// it in an Arc after).
    pub fn new(map: Arc<MapData>, skill: i32) -> Result<Navigator, String> {
        if map.nodes.is_empty() && map.subsectors.len() > 1 {
            return Err(format!("{}: no usable BSP nodes (extended node formats aren't supported)", map.name));
        }
        // Solid decorations are cut out of the mesh, grown by the player's half-width: the mesh
        // is where her centre can be. Barrels aren't cut: they can be shot away.
        let mesh_obstacles: Vec<Obstacle> = map
            .things
            .iter()
            .filter(|t| !shootable_obstacle(t.kind) && t.in_skill(skill))
            .filter_map(|t| solid_radius(t.kind).map(|r| (t.x, t.y, r + PLAYER_RADIUS)))
            .collect();
        let mesh = Arc::new(Mesh::build(&map, &mesh_obstacles).map_err(|e| e.to_string())?);
        let neighbors = (0..map.sectors.len()).map(|s| neighbors_of_sector(&map, s)).collect();
        let mut movers = MoverModel::new(Arc::clone(&map), neighbors);
        let trenches = analysis::fill_trenches(&map, &mesh, &mut movers);
        let movers = Arc::new(movers);
        let damage = map.sectors.iter().map(|s| classify_sector(s.special).damage).collect();
        let obstacle_grid = widths::obstacle_grid(&map, skill, true);
        let mut a = analysis::Analysis::new(&map, &mesh, &movers, &obstacle_grid, skill);
        let mut triggers = a.find_triggers();
        let (use_gates, use_line_gates, shot_floors) = a.take_gates();
        for t in &mut triggers {
            t.effects = with_trenches(&movers, &trenches, &t.effects);
        }
        for t in &mut triggers {
            if t.line < 0 || !crate::movers::depends_on_heights(t.info.special) || t.sectors.is_empty() || t.shift_from.is_some() {
                continue;
            }
            let mut relevant: BTreeSet<usize> = t.sectors.iter().copied().collect();
            for &s in &t.sectors {
                relevant.extend(movers.neighbors(s).iter().copied());
            }
            t.relevant = Some(relevant.into_iter().collect());
        }
        let teleports = analysis::find_teleports(&map, &mesh);
        let sight_grid = analysis::sight_blockers(&map, &movers);
        let w = widths::Widths::new(&map, &mesh, &movers, &triggers, &widths::obstacle_grid(&map, skill, false));
        let sinking = sinking_sectors(&map, &movers, &triggers);
        let jump_lab = Arc::new(JumpLab::new(
            Arc::clone(&map),
            Arc::clone(&mesh),
            crate::jumps::JumpFacts {
                static_grid: w.static_wall_grid.clone(),
                obstacle_grid: obstacle_grid.clone(),
                initial: movers.initial.iter().map(|h| (h.floor, h.ceiling)).collect(),
                teleports: map
                    .linedefs
                    .iter()
                    .filter(|l| crate::specials::classify_line(l.special).is_some_and(|i| i.category == Category::Teleport))
                    .map(|l| l.index)
                    .collect(),
                start_angle: map.things.iter().find(|t| t.kind == 1).map_or(90.0, |t| f64::from(t.angle)),
                permanent: triggers.iter().filter(|t| t.line >= 0 && t.permanent()).map(|t| t.effects.clone()).collect(),
                sinking: sinking.iter().copied().collect(),
            },
        ));
        let core = view::Core::placeholder();
        let line_sets = widths::LineSets::new(&map, &movers);
        let mut nav = Navigator {
            map,
            mesh,
            skill,
            mesh_obstacles,
            movers,
            trenches,
            damage,
            strict_fit: false,
            use_gates,
            use_line_gates,
            shot_floors,
            triggers,
            teleports,
            jumps: Vec::new(),
            tele_from: Vec::new(),
            portal_fits: w.fits,
            fit: Rc::new(w.fit),
            line_sets,
            static_wall_grid: w.static_wall_grid,
            obstacle_grid,
            sight_grid,
            sinking,
            jump_lab,
            caches: RefCell::new(Caches::default()),
            core: RefCell::new(core),
        };
        let found = jumps::find_jumps_cached(&nav);
        nav.set_jumps(found);
        Ok(nav)
    }

    /// Replace the jump edges (with only the checked ones, say). Jumps that need a lift or a
    /// timed door at a height it only passes through or waits at are left out.
    pub fn set_jumps(&mut self, jumps: Vec<Jump>) {
        let temporary = temporary_heights(&self.triggers, &self.movers);
        self.jumps = jumps.into_iter().filter(|j| !j.requires.iter().any(|r| temporary.contains(&(r.0, r.1.key())))).collect();
        let mut tele_from: Vec<Vec<TeleEdge>> = vec![Vec::new(); self.mesh.polys.len()];
        for (i, tp) in self.teleports.iter().enumerate() {
            for &p in &tp.src_polys {
                tele_from[p].push(TeleEdge::Teleport(i));
            }
        }
        for (i, j) in self.jumps.iter().enumerate() {
            for &p in &j.src_polys {
                tele_from[p].push(TeleEdge::Jump(i));
            }
        }
        self.tele_from = tele_from;
        *self.core.get_mut() = view::Core::new(self);
    }

    pub fn initial_states(&self) -> States {
        self.movers.initial.iter().map(|&h| vec![h]).collect()
    }

    pub fn poly_at(&self, p: Point) -> Option<usize> {
        self.mesh.poly_at(p)
    }

    pub fn sector_of_poly(&self, poly: usize) -> usize {
        self.mesh.poly(poly).sector
    }

    pub fn poly_center(&self, poly: usize) -> Point {
        self.mesh.poly(poly).center
    }

    pub fn line_ends(&self, li: usize) -> (Point, Point) {
        self.map.line_seg(li)
    }

    /// Can this teleport or jump be taken with these heights? A walk-over teleport only goes
    /// off if the player can step over its line (MAP08 has one before a shut niche); a jump
    /// needs the moved heights it was found with.
    pub fn edge_ok(&self, e: TeleEdge, states: &States) -> bool {
        match e {
            TeleEdge::Jump(i) => self.jumps[i].usable(states),
            TeleEdge::Teleport(i) => {
                let (f, b) = self.map.sides(self.teleports[i].line);
                let (Some(f), Some(b)) = (f, b) else { return false };
                states[f].iter().any(|&fh| states[b].iter().any(|&bh| can_cross(fh, bh)))
            }
        }
    }

    pub fn edge_dest(&self, e: TeleEdge) -> (usize, Point) {
        match e {
            TeleEdge::Teleport(i) => (self.teleports[i].dest_poly, self.teleports[i].dest),
            TeleEdge::Jump(i) => (self.jumps[i].dest_poly, self.jumps[i].dest),
        }
    }

    /// Doors she can open herself (DR/D1 lines) with these keys: sector -> (open heights,
    /// polys it can be used from).
    pub fn local_doors(triggers: &[TriggerLine], keys: Keys) -> Doors {
        let mut out = Doors::default();
        for t in triggers {
            if !matches!(t.info.trigger, Trigger::DR | Trigger::D1) || !t.info.lock.satisfied(keys) {
                continue;
            }
            for (s, hs) in &t.effects {
                let door = out.entry(*s).or_insert_with(|| DoorOpening { heights: Vec::new(), users: Default::default() });
                for &h in hs {
                    add_height(&mut door.heights, h);
                }
                door.users.extend(t.activators.iter().copied());
            }
        }
        out
    }

    /// Does sector s's floor hurt once trigger t has moved it, if t changes its type (None:
    /// it doesn't)? Lowering to the lowest neighbour and changing texture takes that
    /// neighbour's flat and type (MAP08's trap drops a walkway into the nukage); raising to
    /// the next higher one and changing texture clears the type.
    pub fn floor_after(&self, t: &TriggerLine, s: usize) -> Option<bool> {
        let special = if t.line >= 0 { Some(t.info.special) } else { None };
        match special {
            Some(37) => {
                let around = self.movers.neighbors(s);
                // (by the map's floors: a trench's initial height is its sides')
                let model = around.iter().copied().min_by(|&a, &b| self.map.sectors[a].floor.total_cmp(&self.map.sectors[b].floor))?;
                Some(self.damage[model] != 0)
            }
            Some(20 | 22 | 47 | 68) => Some(false),
            _ => None,
        }
    }

    /// Where the player stands over trench t, given each sector's floor now.
    pub fn trench_floor(&self, t: usize, floor_of: impl Fn(usize) -> f64) -> f64 {
        trench_floor(&self.movers, &self.trenches, t, floor_of)
    }

    /// Trigger `id`'s effects with these (sector, heights) as they are now.
    pub fn effects_from_reps(&self, t: &TriggerLine, reps: &[(usize, Heights)]) -> Effects {
        let key = (t.id, reps.iter().map(|(s, h)| (*s, h.floor.to_bits(), h.ceiling.to_bits())).collect::<Vec<_>>());
        if let Some(e) = self.caches.borrow().dyn_effects.get(&key) {
            return e.clone();
        }
        let out = effects_from_reps(&self.movers, &self.trenches, t.info.special, &t.sectors, reps);
        self.caches.borrow_mut().dyn_effects.insert(key, out.clone());
        out
    }

    /// What firing `t` does with the world at these heights.
    pub fn effects_now(&self, t: &TriggerLine, states: &States) -> Effects {
        if let Some(relevant) = &t.relevant {
            let init = &self.movers.initial;
            let reps: Vec<(usize, Heights)> = relevant.iter().map(|&s| (s, rep(&states[s], init[s]))).collect();
            if reps.iter().all(|(s, h)| *h == init[*s]) {
                return t.effects.clone();
            }
            let mut out = self.effects_from_reps(t, &reps);
            if let Some(keep) = &t.keep {
                out.retain(|(s, _)| keep.contains(s));
            }
            return out;
        }
        let Some((s0, f0)) = t.shift_from else { return t.effects.clone() };
        let shift = states[s0].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max) - f0;
        if shift == 0.0 {
            return t.effects.clone();
        }
        t.effects.iter().map(|(s, hs)| (*s, hs.iter().map(|h| Heights { floor: h.floor + shift, ceiling: h.ceiling }).collect())).collect()
    }

    /// The heights the world can take after `t` fires: its targets added (lifts and timed
    /// doors come back; a rising floor can be stepped onto on its way), except that a floor
    /// that drops for good replaces the old height (MAP07 can't be jumped from the lowered
    /// ledge).
    pub fn apply_trigger(&self, states: &States, t: &TriggerLine) -> States {
        let mut out = states.clone();
        for (s, hs) in self.effects_now(t, states) {
            let new_max = hs.iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
            let old_min = out[s].iter().map(|h| h.floor).fold(f64::INFINITY, f64::min);
            if t.permanent() && new_max < old_min {
                out[s] = hs;
            } else {
                for h in hs {
                    add_height(&mut out[s], h);
                }
            }
        }
        out
    }

    /// Polys (and a point in each) from which an item at (x, y) can be picked up.
    /// P_TouchSpecialThing only needs the boxes to overlap (16 + 20 each way) and the item to
    /// be between 8 below and 56 above her feet: items on pedestals too high to step onto can
    /// be had from the floor beside them, even across a railing if close enough.
    pub fn pickup_spots(&self, x: f64, y: f64, states: &States) -> Vec<(usize, Point)> {
        let mesh = &self.mesh;
        let Some(home) = mesh.poly_at((x, y)) else { return Vec::new() };
        let home_sector = mesh.poly(home).sector;
        let mut spots = vec![(home, (x, y))];
        let item_floors: Vec<f64> = states[home_sector].iter().map(|h| h.floor).collect();
        let reach = PLAYER_RADIUS + ITEM_RADIUS - PICKUP_MARGIN;
        for (own, poly) in mesh.live_polys().filter(|(_, p)| p.sector == home_sector) {
            for &pi in &poly.portals {
                let portal = &mesh.portals[pi];
                let other = portal.other(own);
                let other_sector = mesh.poly(other).sector;
                if other_sector == home_sector {
                    continue;
                }
                let near = closest_on_portal((x, y), portal.p1, portal.p2);
                if !item_floors.iter().any(|&fi| states[other_sector].iter().any(|ho| (-8.0..=PLAYER_HEIGHT).contains(&(fi - ho.floor)))) {
                    continue;
                }
                // stand far enough back for the box to fit on this side of the line (17 if the
                // line blocks or the item's side is too high to step onto, else a little past)
                let blocks = portal.linedef.is_some_and(|li| self.map.linedefs[li].flags & (ML_BLOCKING | ML_BLOCKPLAYERS) != 0);
                let steppable = states[other_sector].iter().any(|&ho| states[home_sector].iter().any(|&hh| can_cross(ho, hh)));
                let back = if blocks || !steppable { PLAYER_RADIUS + 1.0 } else { 8.0 };
                let c = mesh.poly(other).center;
                let d = dist(near, c).max(1.0);
                let spot = (near.0 + (c.0 - near.0) * back / d, near.1 + (c.1 - near.1) * back / d);
                if (spot.0 - x).abs() < reach && (spot.1 - y).abs() < reach && mesh.poly_at(spot) == Some(other) {
                    spots.push((other, spot));
                }
            }
        }
        spots
    }

    /// Every key on the map with the spots it can be picked up from (by default judged against
    /// every height a sector can ever take: keys on pedestals may only be had once something
    /// moves).
    pub fn key_items(&self, states: Option<&States>) -> KeyItems {
        let all;
        let states = match states {
            Some(s) => s,
            None => {
                let mut s = self.initial_states();
                for t in &self.triggers {
                    for (sec, hs) in &t.effects {
                        for &h in hs {
                            add_height(&mut s[*sec], h);
                        }
                    }
                }
                all = s;
                &all
            }
        };
        let mut out: KeyItems = Vec::new();
        for t in &self.map.things {
            let Some(k) = Key::from_thing(t.kind) else { continue };
            if !t.in_skill(self.skill) {
                continue;
            }
            let spots = self.pickup_spots(t.x, t.y, states);
            match out.iter_mut().find(|(x, _)| *x == k) {
                Some((_, v)) => v.extend(spots),
                None => out.push((k, spots)),
            }
        }
        out
    }

    /// Where each key (at the skill played) is.
    pub fn key_positions(&self) -> Vec<(Key, Vec<Point>)> {
        let mut out: Vec<(Key, Vec<Point>)> = Vec::new();
        for t in &self.map.things {
            let Some(k) = Key::from_thing(t.kind) else { continue };
            if !t.in_skill(self.skill) {
                continue;
            }
            match out.iter_mut().find(|(x, _)| *x == k) {
                Some((_, v)) => v.push((t.x, t.y)),
                None => out.push((k, vec![(t.x, t.y)])),
            }
        }
        out
    }

    /// Floors that sink once stepped on (a walk-over line on their own edge lowers them:
    /// MAP08's exit run over the nukage).
    pub fn sinking_sectors(&self) -> &BTreeSet<usize> {
        &self.sinking
    }

    /// Does the box at p touch a static wall or solid thing?
    pub fn box_blocked(&self, p: Point) -> bool {
        let r = PLAYER_RADIUS + BOX_SLACK;
        let pad = PLAYER_RADIUS + 1.0;
        let mut seen: Vec<Seg> = Vec::new();
        for cell in crate::geom::cells(p.0 - pad, p.1 - pad, p.0 + pad, p.1 + pad) {
            for &e in self.static_wall_grid.get(&cell).into_iter().flatten() {
                if !seen.contains(&e) {
                    seen.push(e);
                    if box_overlaps(p, r, e) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The lines that block sight (one-sided, or shut at the start), by grid cell.
    pub fn sight_blocking_grid(&self) -> &HashMap<Cell, Vec<(Point, Point, usize)>> {
        &self.sight_grid
    }

    /// cell -> the box edges of solid things.
    pub fn obstacle_grid(&self) -> &HashMap<Cell, Vec<Seg>> {
        &self.obstacle_grid
    }

    /// cell -> walls that never change (and solid things' edges).
    pub fn static_wall_grid(&self) -> &HashMap<Cell, Vec<Seg>> {
        &self.static_wall_grid
    }
}

/// The sectors on the other side of the lines round `sector`.
pub fn neighbors_of_sector(m: &MapData, sector: usize) -> Vec<usize> {
    let mut out = BTreeSet::new();
    for l in &m.linedefs {
        let (f, b) = (m.front_sector(l), m.back_sector(l));
        if f == Some(sector) || b == Some(sector) {
            for s in [f, b].into_iter().flatten() {
                if s != sector {
                    out.insert(s);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// One height to work a target out from: the only one, the starting one if it's still
/// possible, else the highest.
pub fn rep(hs: &[Heights], initial: Heights) -> Heights {
    if hs.len() == 1 {
        return hs[0];
    }
    if hs.contains(&initial) {
        initial
    } else {
        max_heights(hs).unwrap_or(initial)
    }
}

fn trench_floor(movers: &MoverModel, trenches: &BTreeMap<usize, Vec<usize>>, t: usize, floor_of: impl Fn(usize) -> f64) -> f64 {
    let low = trenches[&t].iter().map(|&o| floor_of(o)).fold(f64::INFINITY, f64::min);
    movers.real[t].floor.max(low)
}

/// `effects` and what they do to the trenches beside the sectors they move.
pub(crate) fn with_trenches(movers: &MoverModel, trenches: &BTreeMap<usize, Vec<usize>>, effects: &Effects) -> Effects {
    let mut out = effects.clone();
    for (&tr, around) in trenches {
        let moving: Vec<usize> = around.iter().copied().filter(|o| effects.iter().any(|(s, _)| s == o)).collect();
        if moving.is_empty() {
            continue;
        }
        let ceil = movers.real[tr].ceiling;
        let mut hs: Vec<Heights> = Vec::new();
        for &n in &moving {
            let Some((_, nhs)) = effects.iter().find(|(s, _)| *s == n) else { continue };
            for h in nhs {
                let f = trench_floor(movers, trenches, tr, |o| if o == n { h.floor } else { movers.initial[o].floor });
                let new = Heights { floor: f, ceiling: ceil };
                if ceil - f >= PLAYER_HEIGHT && !hs.contains(&new) {
                    hs.push(new);
                }
            }
        }
        if !hs.is_empty() {
            match out.iter_mut().find(|(s, _)| *s == tr) {
                Some(e) => e.1 = hs,
                None => out.push((tr, hs)),
            }
        }
    }
    out
}

/// A trigger's effects (special on sectors) with these (sector, heights) as they are now.
pub(crate) fn effects_from_reps(
    movers: &MoverModel,
    trenches: &BTreeMap<usize, Vec<usize>>,
    special: u16,
    sectors: &[usize],
    reps: &[(usize, Heights)],
) -> Effects {
    let now: Vec<(usize, Heights)> = reps.iter().copied().filter(|(s, h)| *h != movers.initial[*s]).collect();
    let lookup = |s: usize| now.iter().find(|(x, _)| *x == s).map(|(_, h)| *h);
    with_trenches(movers, trenches, &movers.effects(special, sectors, Some(&lookup)))
}

/// (sector, heights) that only movers which come back (lifts, timed doors, crushers) produce:
/// the sector won't stay like that.
pub(crate) fn temporary_heights(triggers: &[TriggerLine], movers: &MoverModel) -> std::collections::BTreeSet<(usize, crate::nav::HeightsKey)> {
    let mut lasting = std::collections::BTreeSet::new();
    for t in triggers.iter().filter(|t| t.permanent() || t.info.description.contains("perpetual")) {
        for (s, hs) in &t.effects {
            for h in hs {
                lasting.insert((*s, h.key()));
            }
        }
    }
    for (s, h) in movers.initial.iter().enumerate() {
        lasting.insert((s, h.key()));
    }
    let mut out = std::collections::BTreeSet::new();
    for t in triggers.iter().filter(|t| !t.permanent()) {
        for (s, hs) in &t.effects {
            for h in hs {
                if !lasting.contains(&(*s, h.key())) {
                    out.insert((*s, h.key()));
                }
            }
        }
    }
    out
}

fn sinking_sectors(m: &MapData, movers: &MoverModel, triggers: &[TriggerLine]) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for t in triggers {
        let Some(li) = t.linedef() else { continue };
        if !t.info.trigger.walked() {
            continue;
        }
        let (f, b) = m.sides(li);
        for sec in [f, b].into_iter().flatten() {
            if t.effect(sec).is_some_and(|hs| hs.iter().any(|h| h.floor < movers.real[sec].floor)) {
                out.insert(sec);
            }
        }
    }
    out
}

/// Does the axis-aligned square of half-width r at p overlap the segment (separating axes:
/// x, y and the segment's normal)?
pub fn box_overlaps(p: Point, r: f64, edge: Seg) -> bool {
    let (e1, e2) = edge;
    let mut axes = vec![(1.0, 0.0), (0.0, 1.0)];
    let (dx, dy) = (e2.0 - e1.0, e2.1 - e1.1);
    let length = dx.hypot(dy);
    if length > 1e-9 {
        axes.push((-dy / length, dx / length));
    }
    for (ux, uy) in axes {
        let ext = r * (ux.abs() + uy.abs());
        let a = p.0 * ux + p.1 * uy;
        let (c, d) = (e1.0 * ux + e1.1 * uy, e2.0 * ux + e2.1 * uy);
        if a + ext <= c.min(d) || c.max(d) <= a - ext {
            return false;
        }
    }
    true
}

/// The point of the poly nearest `target`, moved `inset` towards the poly's centre (or the
/// centre, if the poly is smaller than that).
pub fn spot_in_poly(vertices: &[Point], center: Point, target: Point, inset: f64) -> Point {
    let n = vertices.len();
    let best = (0..n)
        .map(|i| crate::geom::closest_point(target, vertices[i], vertices[(i + 1) % n]))
        .min_by(|a, b| dist(*a, target).total_cmp(&dist(*b, target)))
        .unwrap_or(center);
    let d = dist(best, center);
    if d <= inset {
        return center;
    }
    (best.0 + (center.0 - best.0) * inset / d, best.1 + (center.1 - best.1) * inset / d)
}

/// The point of the portal nearest p, kept PATH_CLEARANCE off its ends if it's wide enough.
pub fn closest_on_portal(p: Point, p1: Point, p2: Point) -> Point {
    let length = dist(p1, p2);
    if length <= 2.0 * PATH_CLEARANCE {
        return crate::geom::midpoint(p1, p2);
    }
    let t = ((p.0 - p1.0) * (p2.0 - p1.0) + (p.1 - p1.1) * (p2.1 - p1.1)) / (length * length);
    let margin = PATH_CLEARANCE / length;
    let t = t.max(margin).min(1.0 - margin);
    (p1.0 + t * (p2.0 - p1.0), p1.1 + t * (p2.1 - p1.1))
}
