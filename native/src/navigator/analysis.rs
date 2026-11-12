// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The level's triggers and from where each can be set off; teleports; trenches.

use std::collections::{BTreeMap, BTreeSet};

use super::Teleport;
use super::{
    box_hits_segment, spot_in_poly, TriggerLine, EXIT_FLOOR, MAX_STEP, MIN_SHOT_SPAN,
    PLAYER_HEIGHT, PLAYER_RADIUS, SHOT_HEIGHT, TELEPORTERS_FOR_MONSTERS, TELEPORTERS_FOR_PLAYERS,
    TELEPORT_DEST, USE_REACH,
};
use crate::det::HashMap;
use crate::geom::{cells, closest_point, dist, segment_distance, segments_cross, Cell, Point, Seg};
use crate::map::{MapData, ML_BLOCKING, ML_BLOCKMONSTERS, ML_BLOCKPLAYERS, NO_SIDEDEF};
use crate::mesh::{inside, Mesh, Poly};
use crate::movers::{is_stairs, MoverModel};
use crate::nav::Heights;
use crate::specials::{classify_line, Category, LineSpecial, Lock, Trigger};

const BARON: u16 = 3003;
const CYBERDEMON: u16 = 16;
const SPIDER: u16 = 7;
const MANCUBUS: u16 = 67;
const ARACHNOTRON: u16 = 68;
const KEEN: u16 = 72;

fn monster_height(kind: u16) -> f64 {
    match kind {
        CYBERDEMON => 110.0,
        SPIDER => 100.0,
        KEEN => 72.0,
        _ => 64.0,
    }
}

fn monster_name(kind: u16) -> &'static str {
    match kind {
        BARON => "Barons of Hell",
        CYBERDEMON => "Cyberdemons",
        SPIDER => "Spider Masterminds",
        MANCUBUS => "Mancubi",
        ARACHNOTRON => "Arachnotrons",
        _ => "Commander Keens",
    }
}

/// A_BossDeath on these maps: (monster, tag, the special whose effect it has (0: none),
/// category).
fn boss_rules(map: &str) -> Vec<(u16, u16, u16, Category)> {
    match map {
        "E1M8" => vec![(BARON, 666, 38, Category::Floor)],
        "E2M8" => vec![(CYBERDEMON, 0, 0, Category::Exit)],
        "E3M8" => vec![(SPIDER, 0, 0, Category::Exit)],
        "E4M6" => vec![(CYBERDEMON, 666, 109, Category::Door)],
        "E4M8" => vec![(SPIDER, 666, 38, Category::Floor)],
        "MAP07" => vec![
            (MANCUBUS, 666, 38, Category::Floor),
            (ARACHNOTRON, 667, 30, Category::Floor),
        ],
        _ => Vec::new(),
    }
}

/// A sector lower than everything round it and too narrow anywhere for the player's box can't
/// be stood in: the box always rests on the floor beside it (an 8-unit slot in a floor, a
/// narrow channel round a bridge: no pits). The planner sees it at its lowest neighbour's
/// floor (and follows them as they move: with_trenches); the movers keep the real heights.
/// Returns trench -> its neighbours.
pub(super) fn fill_trenches(
    m: &MapData,
    mesh: &Mesh,
    movers: &mut MoverModel,
) -> BTreeMap<usize, Vec<usize>> {
    let mut moved: BTreeSet<usize> = BTreeSet::new();
    for l in m.linedefs.iter().filter(|l| l.special != 0) {
        moved.extend(m.tagged_sectors(l.tag));
        if l.two_sided() {
            moved.extend(m.back_sector(l));
        }
    }
    let r = PLAYER_RADIUS;
    let mut out = BTreeMap::new();
    for sec in &m.sectors {
        let si = sec.index;
        if moved.contains(&si) {
            continue;
        }
        let mut bounds: Vec<Seg> = Vec::new();
        let mut around: BTreeSet<usize> = BTreeSet::new();
        for (li, l) in m.linedefs.iter().enumerate() {
            let (f, b) = (m.front_sector(l), m.back_sector(l));
            if f != Some(si) && b != Some(si) || f == b {
                continue;
            }
            bounds.push(m.line_seg(li));
            if let Some(o) = if f == Some(si) { b } else { f } {
                around.insert(o);
            }
        }
        let Some(low) = around
            .iter()
            .map(|&o| movers.real[o].floor)
            .reduce(f64::min)
        else {
            continue;
        };
        if low <= sec.floor || sec.ceiling - low < PLAYER_HEIGHT {
            continue;
        }
        let fits = mesh
            .live_polys()
            .filter(|(_, p)| p.sector == si)
            .any(|(_, poly)| {
                let (x0, x1) = poly
                    .vertices
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                        (a.min(v.0), b.max(v.0))
                    });
                let (y0, y1) = poly
                    .vertices
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                        (a.min(v.1), b.max(v.1))
                    });
                let mut x = x0;
                while x <= x1 {
                    let mut y = y0;
                    while y <= y1 {
                        if inside(&poly.vertices, x, y)
                            && !bounds.iter().any(|&e| box_hits_segment(x, y, r, e))
                        {
                            return true;
                        }
                        y += 4.0;
                    }
                    x += 4.0;
                }
                false
            });
        if !fits {
            out.insert(si, around.iter().copied().collect());
            movers.initial[si] = Heights {
                floor: low,
                ceiling: sec.ceiling,
            };
        }
    }
    out
}

/// The first teleport destination in each sector, and the teleport lines that go there.
pub(super) fn find_teleports(m: &MapData, mesh: &Mesh) -> Vec<Teleport> {
    let mut dests: BTreeMap<usize, (usize, Point)> = BTreeMap::new();
    for t in m.things.iter().filter(|t| t.kind == TELEPORT_DEST) {
        if let Some(p) = mesh.poly_at((t.x, t.y)) {
            dests.entry(mesh.poly(p).sector).or_insert((p, (t.x, t.y)));
        }
    }
    let mut out = Vec::new();
    for line in m
        .linedefs
        .iter()
        .filter(|l| TELEPORTERS_FOR_PLAYERS.contains(&l.special))
    {
        for s in m.tagged_sectors(line.tag) {
            if let Some(&(poly, point)) = dests.get(&s) {
                let src_polys = mesh
                    .live_polys()
                    .filter(|(_, p)| p.lines_front.contains(&line.index))
                    .map(|(i, _)| i)
                    .collect();
                out.push(Teleport {
                    line: line.index,
                    src_polys,
                    dest_poly: poly,
                    dest: point,
                });
                break;
            }
        }
    }
    out
}

/// Lines that block sight and hitscans: one-sided, or shut at the start; by grid cell.
pub(super) fn sight_blockers(
    m: &MapData,
    movers: &MoverModel,
) -> HashMap<Cell, Vec<(Point, Point, usize)>> {
    let init = &movers.initial;
    let mut out: HashMap<Cell, Vec<(Point, Point, usize)>> = HashMap::default();
    for (li, l) in m.linedefs.iter().enumerate() {
        let (f, b) = (m.front_sector(l), m.back_sector(l));
        let shut = match (f, b) {
            (Some(f), Some(b)) => {
                init[f].ceiling.min(init[b].ceiling) - init[f].floor.max(init[b].floor) <= 0.0
            }
            _ => true,
        };
        if shut {
            let (a, c) = m.line_seg(li);
            for cell in cells(a.0, a.1, c.0, c.1) {
                out.entry(cell).or_default().push((a, c, li));
            }
        }
    }
    out
}

/// What's shut in the way of a press: sectors shut in themselves, and pairs of sectors with no
/// gap between them.
type UseGates = (BTreeSet<usize>, BTreeSet<(usize, usize)>);

/// Gates on presses found while working out the triggers.
type Gates = (
    HashMap<(usize, usize), BTreeSet<usize>>,
    HashMap<(usize, usize), BTreeSet<(usize, usize)>>,
    HashMap<(usize, usize), (f64, f64)>,
);

pub(super) struct Analysis<'a> {
    m: &'a MapData,
    mesh: &'a Mesh,
    movers: &'a MoverModel,
    obstacle_grid: &'a HashMap<Cell, Vec<Seg>>,
    skill: i32,
    sight: HashMap<Cell, Vec<(Point, Point, usize)>>,
    /// tagged sectors and door sectors
    may_move: BTreeSet<usize>,
    use_gates: HashMap<(usize, usize), BTreeSet<usize>>,
    use_line_gates: HashMap<(usize, usize), BTreeSet<(usize, usize)>>,
    shot_floors: HashMap<(usize, usize), (f64, f64)>,
}

impl<'a> Analysis<'a> {
    pub(super) fn new(
        m: &'a MapData,
        mesh: &'a Mesh,
        movers: &'a MoverModel,
        obstacle_grid: &'a HashMap<Cell, Vec<Seg>>,
        skill: i32,
    ) -> Self {
        let mut may_move: BTreeSet<usize> = m
            .sectors
            .iter()
            .filter(|s| s.tag != 0)
            .map(|s| s.index)
            .collect();
        for l in &m.linedefs {
            if classify_line(l.special)
                .is_some_and(|i| matches!(i.trigger, Trigger::D1 | Trigger::DR))
            {
                may_move.extend(m.back_sector(l));
            }
        }
        Analysis {
            m,
            mesh,
            movers,
            obstacle_grid,
            skill,
            sight: sight_blockers(m, movers),
            may_move,
            use_gates: HashMap::default(),
            use_line_gates: HashMap::default(),
            shot_floors: HashMap::default(),
        }
    }

    pub(super) fn take_gates(&mut self) -> Gates {
        (
            std::mem::take(&mut self.use_gates),
            std::mem::take(&mut self.use_line_gates),
            std::mem::take(&mut self.shot_floors),
        )
    }

    fn affected(&self, li: usize, info: &LineSpecial) -> Vec<usize> {
        let l = &self.m.linedefs[li];
        if matches!(info.trigger, Trigger::D1 | Trigger::DR) {
            return self.m.back_sector(l).into_iter().collect();
        }
        self.m.tagged_sectors(l.tag)
    }

    pub(super) fn find_triggers(&mut self) -> Vec<TriggerLine> {
        let (m, mesh) = (self.m, self.mesh);
        let mut touching_front: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut touching_any: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut crossing: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for (i, poly) in mesh.live_polys() {
            for &li in &poly.lines_front {
                touching_front.entry(li).or_default().insert(i);
                touching_any.entry(li).or_default().insert(i);
            }
            for &li in &poly.lines_back {
                touching_any.entry(li).or_default().insert(i);
            }
        }
        for portal in &mesh.portals {
            if let Some(li) = portal.linedef {
                crossing.entry(li).or_default().extend([portal.a, portal.b]);
            }
        }
        let mut out: Vec<TriggerLine> = Vec::new();
        for line in &m.linedefs {
            let li = line.index;
            let Some(info) = classify_line(line.special) else {
                continue;
            };
            if !info.players
                || info.trigger == Trigger::None
                || matches!(
                    info.category,
                    Category::Light | Category::Other | Category::Teleport
                )
            {
                continue;
            }
            let activators: BTreeSet<usize> = match info.trigger {
                Trigger::S1 | Trigger::SR | Trigger::D1 | Trigger::DR => {
                    let mut a = touching_front.get(&li).cloned().unwrap_or_default();
                    a.extend(self.polys_in_use_range(li));
                    a
                }
                Trigger::W1 | Trigger::WR => {
                    let mut a = crossing.get(&li).cloned().unwrap_or_default();
                    a.extend(touching_any.get(&li).into_iter().flatten().copied());
                    a
                }
                _ => self.gun_activators(li, touching_any.get(&li).cloned().unwrap_or_default()),
            };
            let sectors = self.affected(li, &info);
            let shift_from = (is_stairs(line.special) && sectors.len() == 1)
                .then(|| (sectors[0], self.movers.initial[sectors[0]].floor));
            let effects = self.movers.effects(line.special, &sectors, None);
            out.push(TriggerLine {
                id: 0,
                line: li as i64,
                info,
                sectors,
                effects,
                activators,
                shift_from,
                relevant: None,
                keep: None,
            });
        }
        // (a gun line shot only from where the floor can be at a height the shot hits it: from a
        // raised floor at its top, say, never from the floor of the room round it)
        let mut floors: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
        for t in &out {
            for (s, hs) in &t.effects {
                floors
                    .entry(*s)
                    .or_default()
                    .extend(hs.iter().map(|h| h.floor));
            }
        }
        for t in out.iter_mut().filter(|t| t.info.trigger.shot()) {
            let li = t.line as usize;
            t.activators.retain(|&p| {
                let s = mesh.poly(p).sector;
                let Some(&(lo, hi)) = self.shot_floors.get(&(li, p)) else {
                    return false;
                };
                std::iter::once(self.movers.initial[s].floor)
                    .chain(floors.get(&s).into_iter().flatten().copied())
                    .any(|f| (lo..=hi).contains(&f))
            });
        }
        let bosses = self.boss_triggers(&out);
        out.extend(bosses);
        out.extend(self.exit_floors());
        for (i, t) in out.iter_mut().enumerate() {
            t.id = i;
        }
        out
    }

    /// Gun lines: shot from anywhere with a view of the line where a level shot (no aiming up
    /// or down) actually hits it, and not too edge-on (a line seen at 3 degrees from 340 away
    /// is 4 units wide).
    fn gun_activators(&mut self, li: usize, mut candidates: BTreeSet<usize>) -> BTreeSet<usize> {
        let (a, b) = self.m.line_seg(li);
        let points: Vec<Point> = [0.25, 0.5, 0.75]
            .iter()
            .map(|t| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
            .collect();
        for &p in &points {
            candidates.extend(self.line_of_sight_polys(p, 2048.0, Some(li)));
        }
        let mut out = BTreeSet::new();
        for pi in candidates {
            let c = self.mesh.poly(pi).center;
            let span = (((a.1 - c.1).atan2(a.0 - c.0).to_degrees()
                - (b.1 - c.1).atan2(b.0 - c.0).to_degrees())
                + 180.0)
                .rem_euclid(360.0)
                - 180.0;
            if span.abs() < MIN_SHOT_SPAN {
                continue;
            }
            let ranges: Vec<(f64, f64)> = points
                .iter()
                .filter_map(|&p| self.shot_floors_at(c, p, li))
                .collect();
            if let Some(best) =
                ranges
                    .iter()
                    .copied()
                    .reduce(|x, y| if y.1 - y.0 > x.1 - x.0 { y } else { x })
            {
                out.insert(pi);
                self.shot_floors.insert((li, pi), best);
            }
        }
        out
    }

    /// Sectors that end the level once the floor has hurt the player down to 10 health
    /// (special 11): pseudo-triggers, line EXIT_FLOOR - sector, set off by standing on them.
    fn exit_floors(&self) -> Vec<TriggerLine> {
        self.m
            .sectors
            .iter()
            .filter(|s| s.special == 11)
            .map(|sec| TriggerLine {
                id: 0,
                line: EXIT_FLOOR - sec.index as i64,
                info: LineSpecial {
                    special: 0,
                    trigger: Trigger::None,
                    category: Category::Exit,
                    description: format!("stand on sector {} till it ends the level", sec.index),
                    lock: Lock::None,
                    monsters: false,
                    players: true,
                    generalized: false,
                },
                sectors: Vec::new(),
                effects: Vec::new(),
                activators: self
                    .mesh
                    .live_polys()
                    .filter(|(_, p)| p.sector == sec.index)
                    .map(|(i, _)| i)
                    .collect(),
                shift_from: None,
                relevant: None,
                keep: None,
            })
            .collect()
    }

    /// Polys that come within USE_REACH of the line on its front side. Use traces 64 from the
    /// player's centre, so a switch at the back of an alcove too shallow to stand in is
    /// pressed from the room in front.
    fn polys_in_use_range(&mut self, li: usize) -> BTreeSet<usize> {
        let (a, b) = self.m.line_seg(li);
        let mut out = BTreeSet::new();
        for (pi, poly) in self.mesh.live_polys() {
            let (x0, x1) = poly
                .vertices
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v.0), hi.max(v.0))
                });
            let (y0, y1) = poly
                .vertices
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v.1), hi.max(v.1))
                });
            if x0 > a.0.max(b.0) + USE_REACH
                || x1 < a.0.min(b.0) - USE_REACH
                || y0 > a.1.max(b.1) + USE_REACH
                || y1 < a.1.min(b.1) - USE_REACH
            {
                continue;
            }
            // the front is right of v1->v2: the poly's centre must be in front
            let c = poly.center;
            if (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0) >= 0.0 {
                continue;
            }
            let vs = &poly.vertices;
            let n = vs.len();
            if !(0..n).any(|i| segment_distance(vs[i], vs[(i + 1) % n], a, b) <= USE_REACH) {
                continue;
            }
            // ...and somewhere in it the box can stand within use range (a floor below the one in
            // front of a door can be out of reach of it)
            if !self.press_point_in(pi, poly, a, b) {
                continue;
            }
            // The use trace stops at the first line with no opening: a switch behind shut bars
            // can only be pressed through them once they're open.
            let near = spot_in_poly(vs, c, crate::geom::midpoint(a, b), 1.0);
            let length = dist(a, b).max(1e-6);
            let f = 0.45f64.min(2.0 / length);
            let inner = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            let inner2 = (b.0 + (a.0 - b.0) * f, b.1 + (a.1 - b.1) * f);
            let gates: Vec<UseGates> = [near, c]
                .iter()
                .filter_map(|&p| self.use_gates_on(p, closest_point(p, inner, inner2), li))
                .collect();
            let Some((sectors, pairs)) = gates.into_iter().reduce(|x, y| {
                if y.0.len() + y.1.len() < x.0.len() + x.1.len() {
                    y
                } else {
                    x
                }
            }) else {
                continue;
            };
            out.insert(pi);
            if !sectors.is_empty() {
                self.use_gates.insert((li, pi), sectors);
            }
            if !pairs.is_empty() {
                self.use_line_gates.insert((li, pi), pairs);
            }
        }
        out
    }

    /// Is there a spot in the poly the box can stand at within use range of the line a-b
    /// (along its edges, moved in by the box)?
    fn press_point_in(&self, pi: usize, poly: &Poly, a: Point, b: Point) -> bool {
        let vs = &poly.vertices;
        let n = vs.len();
        for i in 0..n {
            let (p1, p2) = (vs[i], vs[(i + 1) % n]);
            for k in 0..9 {
                let k = f64::from(k);
                let e = (
                    p1.0 + (p2.0 - p1.0) * k / 8.0,
                    p1.1 + (p2.1 - p1.1) * k / 8.0,
                );
                let d = dist(e, poly.center);
                let f = if d > 1e-6 {
                    1.0f64.min((PLAYER_RADIUS + 1.0) / d)
                } else {
                    0.0
                };
                let q = (
                    e.0 + (poly.center.0 - e.0) * f,
                    e.1 + (poly.center.1 - e.1) * f,
                );
                // (the trace's whole 64)
                if segment_distance(a, b, q, q) <= USE_REACH + 16.0
                    && self.mesh.poly_at(q) == Some(pi)
                    && !self.box_on_wall(q)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Does the box at q overlap a one-sided or impassable line, or a solid thing?
    fn box_on_wall(&self, q: Point) -> bool {
        let r = PLAYER_RADIUS - 0.5;
        for cell in cells(q.0 - r, q.1 - r, q.0 + r, q.1 + r) {
            for &li in self.mesh.line_grid.get(&cell).into_iter().flatten() {
                let line = &self.m.linedefs[li];
                if (line.back == NO_SIDEDEF || line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) != 0)
                    && box_hits_segment(q.0, q.1, r, self.m.line_seg(li))
                {
                    return true;
                }
            }
            if self
                .obstacle_grid
                .get(&cell)
                .into_iter()
                .flatten()
                .any(|&e| box_hits_segment(q.0, q.1, r, e))
            {
                return true;
            }
        }
        false
    }

    /// What's shut at the map's starting heights that the use trace from p to q (on the target
    /// line) passes: sectors shut in themselves (doors), and pairs of sectors with no gap
    /// between them where one can move (a switch beside a raised platform); None if a line
    /// that never opens is in the way.
    fn use_gates_on(&self, p: Point, q: Point, target: usize) -> Option<UseGates> {
        let (m, init) = (self.m, &self.movers.initial);
        let (mut gates, mut pairs) = (BTreeSet::new(), BTreeSet::new());
        for cell in cells(p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)) {
            for &li in self.mesh.line_grid.get(&cell).into_iter().flatten() {
                if li == target {
                    continue;
                }
                let (f, b) = m.sides(li);
                let closed = match (f, b) {
                    (Some(f), Some(b)) => {
                        init[f].ceiling.min(init[b].ceiling) - init[f].floor.max(init[b].floor)
                            <= 0.0
                    }
                    _ => true,
                };
                // The trace also stops at the first other line with any special (vanilla: a
                // walk-over line too, so a door pressed from beyond one does nothing). Once-only
                // switches lose their special once used.
                let usable = classify_line(m.linedefs[li].special)
                    .is_some_and(|i| !matches!(i.trigger, Trigger::S1 | Trigger::D1));
                if !closed && !usable {
                    continue;
                }
                let (v1, v2) = m.line_seg(li);
                // touching counts: slipping exactly through a corner isn't a clear view
                if segment_distance(p, q, v1, v2) >= 0.5 {
                    continue;
                }
                if usable {
                    return None;
                }
                let (Some(f), Some(b)) = (f, b) else {
                    return None;
                };
                let movable: Vec<usize> = [f, b]
                    .into_iter()
                    .filter(|s| {
                        self.may_move.contains(s) && init[*s].ceiling - init[*s].floor <= 0.0
                    })
                    .collect();
                if !movable.is_empty() {
                    gates.extend(movable);
                } else if self.may_move.contains(&f) || self.may_move.contains(&b) {
                    pairs.insert((f, b));
                } else {
                    return None;
                }
            }
        }
        Some((gates, pairs))
    }

    /// Vanilla A_BossDeath / A_KeenDie: killing every monster of a type sets off a tagged
    /// action on certain maps. Pseudo-triggers (lines -1 - i), set off from where the monsters
    /// can be killed.
    fn boss_triggers(&self, line_triggers: &[TriggerLine]) -> Vec<TriggerLine> {
        let (m, mesh) = (self.m, self.mesh);
        let mut rules = boss_rules(&m.name);
        rules.push((KEEN, 666, 2, Category::Door)); // (Keens open tag 666 on any map)
        let mut out = Vec::new();
        for (i, (monster, tag, special, category)) in rules.into_iter().enumerate() {
            let homes: Vec<Point> = m
                .things
                .iter()
                .filter(|t| t.kind == monster && t.in_skill(self.skill))
                .map(|t| (t.x, t.y))
                .collect();
            if homes.is_empty() {
                continue;
            }
            // killable from anywhere the boss can walk or teleport to, or anywhere with a view
            // of where it starts
            let mut polys = BTreeSet::new();
            for &h in &homes {
                if let Some(home) = mesh.poly_at(h) {
                    polys.extend(self.monster_reach(home, monster_height(monster), line_triggers));
                }
                polys.extend(self.line_of_sight_polys(h, 2048.0, None));
            }
            let sectors = if tag != 0 {
                m.tagged_sectors(tag)
            } else {
                Vec::new()
            };
            let info = LineSpecial {
                special,
                trigger: Trigger::None,
                category,
                description: format!("kill all {}", monster_name(monster)),
                lock: Lock::None,
                monsters: false,
                players: true,
                generalized: false,
            };
            let effects = if sectors.is_empty() {
                Vec::new()
            } else {
                self.movers.effects(special, &sectors, None)
            };
            out.push(TriggerLine {
                id: 0,
                line: -1 - i as i64,
                info,
                sectors,
                effects,
                activators: polys,
                shift_from: None,
                relevant: None,
                keep: None,
            });
        }
        out
    }

    /// Polys a monster can get to from `start`: steps of at most 24 both ways (monsters don't
    /// walk off ledges), its own height of headroom, not through impassable or block-monster
    /// lines, plus the teleporters monsters use; doors and lifts monsters set off taken as open.
    fn monster_reach(
        &self,
        start: usize,
        height: f64,
        line_triggers: &[TriggerLine],
    ) -> BTreeSet<usize> {
        let (m, mesh) = (self.m, self.mesh);
        let mut states: Vec<Vec<Heights>> = self.movers.initial.iter().map(|&h| vec![h]).collect();
        for t in line_triggers.iter().filter(|t| t.info.monsters) {
            for (s, hs) in &t.effects {
                for &h in hs {
                    super::add_height(&mut states[*s], h);
                }
            }
        }
        let mut dests: BTreeMap<usize, usize> = BTreeMap::new();
        for t in m.things.iter().filter(|t| t.kind == TELEPORT_DEST) {
            if let Some(p) = mesh.poly_at((t.x, t.y)) {
                dests.entry(mesh.poly(p).sector).or_insert(p);
            }
        }
        let mut tele: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for line in m
            .linedefs
            .iter()
            .filter(|l| TELEPORTERS_FOR_MONSTERS.contains(&l.special))
        {
            let Some(dest) = m
                .tagged_sectors(line.tag)
                .into_iter()
                .find_map(|s| dests.get(&s).copied())
            else {
                continue;
            };
            for (pi, p) in mesh.live_polys() {
                if p.lines_front.contains(&line.index) {
                    tele.entry(pi).or_default().push(dest);
                }
            }
        }
        let ok = |a: Heights, b: Heights| {
            (b.floor - a.floor).abs() <= MAX_STEP
                && a.ceiling.min(b.ceiling) - a.floor.max(b.floor) >= height
        };
        let mut seen = BTreeSet::from([start]);
        let mut todo = vec![start];
        while let Some(poly) = todo.pop() {
            let mut next: Vec<usize> = tele.get(&poly).cloned().unwrap_or_default();
            for &pi in &mesh.poly(poly).portals {
                let portal = &mesh.portals[pi];
                let o = portal.other(poly);
                if portal
                    .linedef
                    .is_some_and(|li| m.linedefs[li].flags & (ML_BLOCKING | ML_BLOCKMONSTERS) != 0)
                {
                    continue;
                }
                let (sa, sb) = (mesh.poly(poly).sector, mesh.poly(o).sector);
                if sa == sb
                    || states[sa]
                        .iter()
                        .any(|&ha| states[sb].iter().any(|&hb| ok(ha, hb)))
                {
                    next.push(o);
                }
            }
            for n in next {
                if seen.insert(n) {
                    todo.push(n);
                }
            }
        }
        seen
    }

    fn clear_sight(&self, a: Point, b: Point, ignore: Option<usize>) -> bool {
        let mut seen: Vec<usize> = Vec::new();
        for cell in cells(a.0, a.1, b.0, b.1) {
            for &(p, q, li) in self.sight.get(&cell).into_iter().flatten() {
                if seen.contains(&li) {
                    continue;
                }
                seen.push(li);
                if Some(li) != ignore && segments_cross(a, b, p, q) {
                    return false;
                }
            }
        }
        true
    }

    /// Floors to stand on at p so a level shot hits line `target` at q: bullets leave at
    /// SHOT_HEIGHT above the feet and go straight, so every two-sided line on the way must be
    /// open at that height, and the target hit where it's solid. (The map's starting heights.)
    fn shot_floors_at(&self, p: Point, q: Point, target: usize) -> Option<(f64, f64)> {
        let (m, init) = (self.m, &self.movers.initial);
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        let mut seen: Vec<usize> = Vec::new();
        for cell in cells(p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)) {
            for &li in self.mesh.line_grid.get(&cell).into_iter().flatten() {
                if li == target || seen.contains(&li) {
                    continue;
                }
                seen.push(li);
                let (v1, v2) = m.line_seg(li);
                if !segments_cross(p, q, v1, v2) {
                    continue;
                }
                let (Some(f), Some(b)) = m.sides(li) else {
                    return None;
                };
                lo = lo.max(init[f].floor).max(init[b].floor);
                hi = hi.min(init[f].ceiling).min(init[b].ceiling);
            }
        }
        let (f, b) = m.sides(target);
        let f = f?;
        match b {
            None => {
                lo = lo.max(init[f].floor);
                hi = hi.min(init[f].ceiling);
            }
            Some(b) => {
                // two-sided: only its upper or lower part is solid
                let opening = (
                    init[f].floor.max(init[b].floor),
                    init[f].ceiling.min(init[b].ceiling),
                );
                let below = (lo, hi.min(opening.0));
                let above = (lo.max(opening.1), hi);
                (lo, hi) = if below.1 - below.0 >= above.1 - above.0 {
                    below
                } else {
                    above
                };
            }
        }
        if hi - lo < 1.0 {
            return None;
        }
        Some((lo - SHOT_HEIGHT, hi - SHOT_HEIGHT))
    }

    /// Polys whose centre has a clear view of `target` within range (sight blocked by
    /// one-sided lines and sectors shut at the start, not ledges), but for line `ignore`.
    fn line_of_sight_polys(
        &self,
        target: Point,
        max_range: f64,
        ignore: Option<usize>,
    ) -> BTreeSet<usize> {
        self.mesh
            .live_polys()
            .filter(|(_, p)| {
                dist(p.center, target) <= max_range && self.clear_sight(p.center, target, ignore)
            })
            .map(|(i, _)| i)
            .collect()
    }
}
