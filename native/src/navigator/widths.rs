//! What the player's box fits through, and the walls that never move.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use super::{box_hits_segment, Navigator, TriggerLine, BOX_SLACK, MIN_PORTAL, PLAYER_HEIGHT, PLAYER_RADIUS, TELEPORTERS_FOR_PLAYERS};
use crate::det::HashMap;
use crate::geom::{cells, dist, Cell, Point, Seg};
use crate::jumps::JumpLab;
use crate::map::{MapData, ML_BLOCKING, ML_BLOCKPLAYERS};
use crate::mesh::{Mesh, Portal};
use crate::movers::MoverModel;
use crate::nav::can_cross;
use crate::things::{shootable_obstacle, solid_radius};

/// The box edges of solid decorations (barrels, lamps, pillars...), which block like walls but
/// aren't lines, by grid cell. `shootable`: barrels too (they block till shot away; whether a way
/// is there at all goes without them, as the mesh does: MAP04's corridor to the armour room,
/// two barrels across it).
pub(super) fn obstacle_grid(m: &MapData, skill: i32, shootable: bool) -> HashMap<Cell, Vec<Seg>> {
    let mut out: HashMap<Cell, Vec<Seg>> = HashMap::default();
    for t in &m.things {
        let Some(r) = solid_radius(t.kind) else { continue };
        if !t.in_skill(skill) || (!shootable && shootable_obstacle(t.kind)) {
            continue;
        }
        let c = [(t.x - r, t.y - r), (t.x + r, t.y - r), (t.x + r, t.y + r), (t.x - r, t.y + r)];
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            for cell in cells(a.0, a.1, b.0, b.1) {
                out.entry(cell).or_default().push((a, b));
            }
        }
    }
    out
}

fn moving_sectors(triggers: &[TriggerLine]) -> BTreeSet<usize> {
    triggers.iter().flat_map(|t| t.effects.iter().map(|(s, _)| *s)).collect()
}

/// Two-sided lines that can only be crossed one way at the starting heights (a drop of more
/// than a step): falling over one by accident can strand her.
pub(crate) fn drop_lines(m: &MapData, movers: &MoverModel) -> BTreeSet<usize> {
    let init = &movers.initial;
    (0..m.linedefs.len())
        .filter(|&li| match m.sides(li) {
            (Some(f), Some(b)) => can_cross(init[f], init[b]) != can_cross(init[b], init[f]),
            _ => false,
        })
        .collect()
}

/// Lines she can't cross either way at the starting heights.
pub(crate) fn wall_lines(m: &MapData, movers: &MoverModel) -> BTreeSet<usize> {
    let init = &movers.initial;
    (0..m.linedefs.len())
        .filter(|&li| {
            let line = &m.linedefs[li];
            match m.sides(li) {
                (Some(f), Some(b)) => line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) != 0 || !(can_cross(init[f], init[b]) || can_cross(init[b], init[f])),
                _ => true,
            }
        })
        .collect()
}

/// Lines that teleport her when walked over (from the front).
pub(crate) fn teleport_lines(m: &MapData) -> BTreeSet<usize> {
    m.linedefs.iter().filter(|l| TELEPORTERS_FOR_PLAYERS.contains(&l.special)).map(|l| l.index).collect()
}

/// Walk-over lines that end the level.
pub(crate) fn exit_walk_lines(m: &MapData) -> BTreeSet<usize> {
    (0..m.linedefs.len()).filter(|&li| crate::specials::classify_line(m.linedefs[li].special).is_some_and(|i| i.category.is_exit() && i.trigger.walked())).collect()
}

/// Walk-over lines that shut (no headroom) a sector on either side of themselves.
pub(crate) fn shutting_lines(m: &MapData, triggers: &[TriggerLine]) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for t in triggers {
        let Some(li) = t.linedef() else { continue };
        if !t.info.trigger.walked() {
            continue;
        }
        let (f, b) = m.sides(li);
        if t.effects.iter().any(|(s, hs)| (Some(*s) == f || Some(*s) == b) && hs.iter().any(|h| h.ceiling - h.floor < PLAYER_HEIGHT)) {
            out.insert(li);
        }
    }
    out
}

/// Is there a spot along the portal where the box of half-width r touches none of `walls`?
fn box_fits_somewhere(portal: &Portal, r: f64, walls: &[Seg]) -> bool {
    let ((x1, y1), (x2, y2)) = (portal.p1, portal.p2);
    let n = ((dist(portal.p1, portal.p2) / 4.0).floor() as usize).max(1);
    (0..=n).any(|i| {
        let t = i as f64 / n as f64;
        let (x, y) = (x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
        !walls.iter().any(|&w| box_hits_segment(x, y, r, w))
    })
}

pub(super) struct Widths {
    pub fits: Vec<bool>,
    pub fit: PortalFit,
    pub static_wall_grid: HashMap<Cell, Vec<Seg>>,
}

/// Lines near a portal that narrow it when they block: (line, its segment, (front, back)).
pub type Gates = Vec<(usize, Seg, (usize, usize))>;

/// Which portals the box gets through when lines of moving sectors near them block.
pub struct PortalFit {
    mesh: Arc<Mesh>,
    /// portal -> lines near it of moving sectors (or ledges) that narrow it when they block:
    /// (line, its segment, (front, back))
    pub gates: HashMap<usize, Gates>,
    /// cell -> (wall, its line or None for a thing's edge)
    grid: HashMap<Cell, Vec<(Seg, Option<usize>)>>,
    cache: RefCell<HashMap<(usize, Vec<usize>), bool>>,
}

impl PortalFit {
    /// Does the box fit through the portal with these gate lines (sorted) as walls?
    pub fn fits_without(&self, portal: usize, blocking: &[usize]) -> bool {
        if blocking.is_empty() {
            return true;
        }
        let key = (portal, blocking.to_vec());
        if let Some(&v) = self.cache.borrow().get(&key) {
            return v;
        }
        let gates = self.gates.get(&portal).map_or(&[][..], Vec::as_slice);
        let p = &self.mesh.portals[portal];
        let ((x1, y1), (x2, y2)) = (p.p1, p.p2);
        let r = PLAYER_RADIUS + BOX_SLACK;
        let mut walls: Vec<Seg> = Vec::new();
        for cell in cells(x1.min(x2) - r, y1.min(y2) - r, x1.max(x2) + r, y1.max(y2) + r) {
            for &(e, line) in self.grid.get(&cell).into_iter().flatten() {
                if (line.is_none() || line != p.linedef) && !walls.contains(&e) {
                    walls.push(e);
                }
            }
        }
        walls.extend(gates.iter().filter(|g| blocking.contains(&g.0)).map(|g| g.1));
        let v = box_fits_somewhere(p, r, &walls);
        self.cache.borrow_mut().insert(key, v);
        v
    }
}

impl Widths {
    pub(super) fn new(m: &MapData, mesh: &Arc<Mesh>, movers: &MoverModel, triggers: &[TriggerLine], obstacles: &HashMap<Cell, Vec<Seg>>) -> Widths {
        let moving = moving_sectors(triggers);
        let walls = static_walls(m, movers, &moving, obstacles);
        let mut grid: HashMap<Cell, Vec<(Seg, Option<usize>)>> = HashMap::default();
        for &(e, line) in &walls {
            for cell in cells(e.0 .0, e.0 .1, e.1 .0, e.1 .1) {
                grid.entry(cell).or_default().push((e, line));
            }
        }
        // (sorted, not by hashing: what's checked first mustn't vary)
        let mut static_wall_grid: HashMap<Cell, Vec<Seg>> = HashMap::default();
        for &(e, _) in &walls {
            for cell in cells(e.0 .0, e.0 .1, e.1 .0, e.1 .1) {
                let v = static_wall_grid.entry(cell).or_default();
                if !v.contains(&e) {
                    v.push(e);
                }
            }
        }
        // Whether the box fits through each portal somewhere along it (PIT_CheckLine: a wall
        // blocks a box overlapping its bounding box with corners on both sides of its line).
        let r = PLAYER_RADIUS + BOX_SLACK; // (a gap exactly 32 wide can't be threaded)
        let ledges = drop_lines(m, movers); // (walls only from below: judged per approach)
        let mut fits = Vec::with_capacity(mesh.portals.len());
        let mut gates_out = HashMap::default();
        for (pi, portal) in mesh.portals.iter().enumerate() {
            let ((x1, y1), (x2, y2)) = (portal.p1, portal.p2);
            let bx = (x1.min(x2) - r, y1.min(y2) - r, x1.max(x2) + r, y1.max(y2) + r);
            // (the portal's own line is judged by flags and heights, not here)
            let mut near: Vec<Seg> = Vec::new();
            for cell in cells(bx.0, bx.1, bx.2, bx.3) {
                for &(e, line) in grid.get(&cell).into_iter().flatten() {
                    if (line.is_none() || line != portal.linedef) && !near.contains(&e) {
                        near.push(e);
                    }
                }
            }
            // (slivers, BSP leftovers a unit or so wide, are never the real way through)
            let ok = dist(portal.p1, portal.p2) >= MIN_PORTAL && box_fits_somewhere(portal, r, &near);
            fits.push(ok);
            if !ok {
                continue;
            }
            // lines of moving sectors that would make the portal too narrow when they block
            let mut gates: Gates = Vec::new();
            for cell in cells(bx.0, bx.1, bx.2, bx.3) {
                for &li in mesh.line_grid.get(&cell).into_iter().flatten() {
                    let (Some(f), Some(b)) = m.sides(li) else { continue };
                    if Some(li) == portal.linedef || !(moving.contains(&f) || moving.contains(&b) || ledges.contains(&li)) || gates.iter().any(|g| g.0 == li) {
                        continue;
                    }
                    // only lines across the portal narrow it (one alongside, a thin BSP strip by
                    // a door, matters for the next crossing instead)
                    let (v1, v2) = m.line_seg(li);
                    let (lx, ly) = (v2.0 - v1.0, v2.1 - v1.1);
                    let cos = (lx * (x2 - x1) + ly * (y2 - y1)).abs() / (lx.hypot(ly) * (x2 - x1).hypot(y2 - y1)).max(1e-9);
                    if cos < 0.87 {
                        gates.push((li, (v1, v2), (f, b)));
                    }
                }
            }
            if !gates.is_empty() {
                near.extend(gates.iter().map(|g| g.1));
                if !box_fits_somewhere(portal, r, &near) {
                    gates_out.insert(pi, gates);
                }
            }
        }
        Widths { fits, fit: PortalFit { mesh: Arc::clone(mesh), gates: gates_out, grid, cache: RefCell::new(HashMap::default()) }, static_wall_grid }
    }
}

/// (segment, linedef or None) for everything that blocks her and never changes: one-sided,
/// impassable-flagged, or a height difference that can't be crossed either way between
/// sectors no mover touches; plus solid decorations' box edges.
fn static_walls(m: &MapData, movers: &MoverModel, moving: &BTreeSet<usize>, obstacles: &HashMap<Cell, Vec<Seg>>) -> Vec<(Seg, Option<usize>)> {
    let init = &movers.initial;
    let mut out: Vec<(Seg, Option<usize>)> = Vec::new();
    for (li, line) in m.linedefs.iter().enumerate() {
        let wall = match m.sides(li) {
            (Some(f), Some(b)) => {
                line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) != 0
                    || (!moving.contains(&f) && !moving.contains(&b) && !(can_cross(init[f], init[b]) || can_cross(init[b], init[f])))
            }
            _ => true,
        };
        if wall {
            out.push((m.line_seg(li), Some(li)));
        }
    }
    let mut cells_sorted: Vec<&Cell> = obstacles.keys().collect();
    cells_sorted.sort_unstable();
    let mut edges: Vec<Seg> = Vec::new();
    for c in cells_sorted {
        for &e in &obstacles[c] {
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    out.extend(edges.into_iter().map(|e| (e, None)));
    // (sorted, unique: as the Python had them)
    out.sort_by(|a, b| a.0 .0 .0.total_cmp(&b.0 .0 .0).then(a.0 .0 .1.total_cmp(&b.0 .0 .1)).then(a.0 .1 .0.total_cmp(&b.0 .1 .0)).then(a.0 .1 .1.total_cmp(&b.0 .1 .1)).then(a.1.map_or(-1, |l| l as i64).cmp(&b.1.map_or(-1, |l| l as i64))));
    out.dedup();
    out
}

impl Navigator {
    /// Does the box fit through the portal with these gate lines (sorted) as walls?
    pub fn portal_fits_without(&self, portal: usize, blocking: &[usize]) -> bool {
        self.fit.fits_without(portal, blocking)
    }

    /// The gate lines at each portal: (line, sectors either side).
    pub fn portal_gate_lines(&self) -> HashMap<usize, Vec<(usize, Vec<usize>)>> {
        self.fit.gates.iter().map(|(&pi, g)| (pi, g.iter().map(|(li, _, (f, b))| (*li, vec![*f, *b])).collect())).collect()
    }

    /// Can she drop off line `li` from sector `high`, whose floor is at `top`? (`landable`)
    pub fn landable(&self, li: usize, high: usize, top: f64) -> bool {
        let key = (li, high, top.to_bits());
        if let Some(&v) = self.caches.borrow().landable.get(&key) {
            return v;
        }
        let v = landable(&self.map, &self.jump_lab, li, high, top);
        self.caches.borrow_mut().landable.insert(key, v);
        v
    }

    /// Is the portal ruled out whatever the heights (a blocking line, a teleport line walked
    /// from its front: that's the teleport, not a walk; a gap the box never fits)?
    pub fn statically_passable(&self, portal: usize, frm: usize) -> bool {
        let p = &self.mesh.portals[portal];
        if let Some(li) = p.linedef {
            let line = &self.map.linedefs[li];
            if line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) != 0 {
                return false;
            }
            if TELEPORTERS_FOR_PLAYERS.contains(&line.special) && self.mesh.poly(frm).lines_front.contains(&li) {
                return false;
            }
        }
        self.portal_fits[portal]
    }
}

/// Can she drop off line `li` from sector `high`, whose floor is at `top`? Doom keeps you on
/// the highest floor anywhere under your box, so you only fall once the whole box is past the
/// ledge, and up there it must fit (not straddle a wall, nor an opening too low for someone at
/// `top`). A gap exactly as wide as the box can't be dropped into (MAP15's 32-wide strip).
/// Only walls and ceilings are checked (the map's): floors past the ledge may have moved, and
/// a teleport line just past it means she never lands (MAP13).
pub(super) fn landable(m: &MapData, lab: &JumpLab, li: usize, high: usize, top: f64) -> bool {
    let (a, b) = m.line_seg(li);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    let length = if length == 0.0 { 1.0 } else { length };
    let (mut nx, mut ny) = (-dy / length, dx / length); // (left of v1->v2: the back side)
    if m.back_sector(&m.linedefs[li]) == Some(high) {
        (nx, ny) = (-nx, -ny);
    }
    let clear = PLAYER_RADIUS * (nx.abs() + ny.abs()) + 0.5; // (the box wholly past the line)
    let steps = ((length / 8.0).floor() as usize).max(2);
    for i in 1..steps {
        let t = i as f64 / steps as f64;
        for d in [clear, clear + 4.0, clear + 8.0, clear + 16.0, clear + 32.0] {
            let p: Point = (a.0 + dx * t + nx * d, a.1 + dy * t + ny * d);
            let (fits, _floor, ceil) = lab.fits_at(p);
            if fits && ceil - top >= PLAYER_HEIGHT {
                return true;
            }
        }
    }
    false
}

/// The line sets `walls_near` and friends go by.
pub(super) struct LineSets {
    walls: BTreeSet<usize>,
    drops: BTreeSet<usize>,
    teleports: BTreeSet<usize>,
}

impl LineSets {
    pub(super) fn new(m: &MapData, movers: &MoverModel) -> LineSets {
        LineSets { walls: wall_lines(m, movers), drops: drop_lines(m, movers), teleports: teleport_lines(m) }
    }
}

impl Navigator {
    /// Two-sided lines with a drop she can't climb back up, at the starting heights.
    pub fn drop_lines(&self) -> &BTreeSet<usize> {
        &self.line_sets.drops
    }

    /// Does a portal have lines of moving sectors beside it that can narrow it?
    pub fn has_portal_gates(&self, portal: usize) -> bool {
        self.fit.gates.contains_key(&portal)
    }

    /// Wall lines and solid things' box edges near the segment a-b. With `exempt` (the lines a
    /// path crosses: doors it opens, ledges it drops off), ledge edges and teleport lines count
    /// as walls too, and the exempt ones don't. With `live` heights, lines that are walls or
    /// ledges now count as well (a floor that has risen makes new ledges: E4M2).
    pub fn walls_near(&self, a: Point, b: Point, pad: f64, exempt: Option<&BTreeSet<usize>>, live: Option<&[crate::nav::Heights]>) -> Vec<Seg> {
        let ls = &self.line_sets;
        let is_wall = |li: usize| match exempt {
            Some(ex) => (ls.walls.contains(&li) || ls.drops.contains(&li) || ls.teleports.contains(&li)) && !ex.contains(&li),
            None => ls.walls.contains(&li),
        };
        let mut seen_lines: BTreeSet<usize> = BTreeSet::new();
        let mut seen_edges: Vec<Seg> = Vec::new();
        let mut out: Vec<Seg> = Vec::new();
        for cell in cells(a.0.min(b.0) - pad, a.1.min(b.1) - pad, a.0.max(b.0) + pad, a.1.max(b.1) + pad) {
            for &li in self.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !seen_lines.insert(li) {
                    continue;
                }
                let wall = is_wall(li);
                if let Some(live) = live {
                    if !wall && exempt.is_none_or(|ex| !ex.contains(&li)) {
                        if let (Some(f), Some(bk)) = self.map.sides(li) {
                            if let (Some(&hf), Some(&hb)) = (live.get(f), live.get(bk)) {
                                if !(can_cross(hf, hb) && can_cross(hb, hf)) {
                                    out.push(self.map.line_seg(li));
                                    continue;
                                }
                            }
                        }
                    }
                }
                if wall {
                    out.push(self.map.line_seg(li));
                }
            }
            for &e in self.obstacle_grid.get(&cell).into_iter().flatten() {
                if !seen_edges.contains(&e) {
                    seen_edges.push(e);
                    out.push(e);
                }
            }
        }
        out
    }
}
