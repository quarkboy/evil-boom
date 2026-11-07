//! What she knows of the level (the polys she can go into, the teleports she knows, the
//! triggers as far as she's learned them, where she's seen keys), and the questions asked
//! through it: where she can get to, a path, a plan through triggers and keys.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::widths::{drop_lines, exit_walk_lines, shutting_lines, teleport_lines, wall_lines};
use super::{add_height, Jump, Navigator, States, TeleEdge, TriggerLine, JUMP_DOUBT, MAX_STEP, PLAYER_RADIUS, USE_REACH};
use crate::det::{HashMap, HashSet};
use crate::geom::{closest_point, dist, point_key, segment_distance, Point};
use crate::nav::{can_cross, AfterOnly, Callbacks, Doors, Edge, Found, Gate, HeightSource, Heights, HeightsKey, NavCore, NavFacts, Tele, TeleKind, Via};
use crate::plan::{KeySet, Kind, PlanCaches, PlanFacts, Planner, Query, Trigger as PlanTrigger, World};
use crate::specials::{Key, Keys, Lock, Trigger};

static NEXT_VIEW: AtomicU64 = AtomicU64::new(1);

/// A height-dependent trigger's effects with everything round it down, and the heights that
/// assumes.
type LowVariant = (crate::movers::Effects, Vec<(usize, Heights)>);

/// The flood fill, path-finding and plan core, set up for one view at a time.
pub(super) struct Core {
    nav: Option<NavCore>,
    caches: PlanCaches,
    /// the view it's set up for
    owner: u64,
}

impl Core {
    pub(super) fn placeholder() -> Core {
        Core { nav: None, caches: PlanCaches::default(), owner: 0 }
    }

    pub(super) fn new(n: &Navigator) -> Core {
        let mesh = &n.mesh;
        let adj: Vec<Vec<Edge>> = (0..mesh.polys.len())
            .map(|p| {
                let Some(poly) = &mesh.polys[p] else { return Vec::new() };
                poly.portals
                    .iter()
                    .filter(|&&pi| n.statically_passable(pi, p))
                    .map(|&pi| {
                        let to = mesh.portals[pi].other(p);
                        Edge { portal: pi, to, from_sector: poly.sector, to_sector: mesh.poly(to).sector, linedef: mesh.portals[pi].linedef }
                    })
                    .collect()
            })
            .collect();
        let gates: HashMap<usize, Vec<Gate>> = n.portal_gate_lines().into_iter().map(|(pi, g)| (pi, g.into_iter().map(|(line, sectors)| Gate { line, sectors }).collect())).collect();
        let tele: Vec<Vec<Tele>> = n
            .tele_from
            .iter()
            .map(|edges| {
                edges
                    .iter()
                    .map(|&e| match e {
                        TeleEdge::Jump(i) => {
                            let j = &n.jumps[i];
                            Tele { dest: j.dest_poly, dest_pt: j.dest, kind: TeleKind::Jump { runup: j.runup, takeoff: j.takeoff, requires: j.requires.clone() }, penalty: JUMP_DOUBT * (1.0 - j.sure) }
                        }
                        TeleEdge::Teleport(i) => {
                            let tp = &n.teleports[i];
                            let (front, back) = n.map.sides(tp.line);
                            Tele { dest: tp.dest_poly, dest_pt: tp.dest, kind: TeleKind::Line { line: tp.line, front, back }, penalty: 0.0 }
                        }
                    })
                    .collect()
            })
            .collect();
        let facts = NavFacts {
            adj,
            gates,
            tele,
            initial: n.movers.initial.clone(),
            damage: n.damage.iter().map(|&d| d != 0).collect(),
            has_teleports: !n.teleports.is_empty(),
            walls: wall_lines(&n.map, &n.movers).into_iter().collect(),
            drops: drop_lines(&n.map, &n.movers).into_iter().collect(),
            trip_lines: teleport_lines(&n.map).into_iter().chain(exit_walk_lines(&n.map)).collect(),
            shutting: shutting_lines(&n.map, &n.triggers).into_iter().collect(),
            static_grid: n.static_wall_grid.clone(),
            obstacle_grid: n.obstacle_grid.clone(),
        };
        let fit = Rc::clone(&n.fit);
        let (map, lab) = (Arc::clone(&n.map), Arc::clone(&n.jump_lab));
        let callbacks = Callbacks::new(Box::new(move |pi, blocking| fit.fits_without(pi, blocking)), Box::new(move |li, high, top| super::widths::landable(&map, &lab, li, high, top)));
        Core { nav: Some(NavCore::new(Arc::clone(&n.map), Arc::clone(&n.mesh), facts, callbacks)), caches: PlanCaches::default(), owner: 0 }
    }
}

/// Heights from a `States` for the core.
struct StatesSource(Vec<Rc<Vec<Heights>>>);

impl StatesSource {
    fn new(states: &States) -> Self {
        StatesSource(states.iter().map(|hs| Rc::new(hs.clone())).collect())
    }
}

impl HeightSource for StatesSource {
    fn heights(&mut self, sector: usize) -> Rc<Vec<Heights>> {
        self.0.get(sector).cloned().unwrap_or_default()
    }
}

fn world(states: &States) -> World {
    Rc::new(states.iter().map(|hs| Rc::new(hs.clone())).collect())
}

fn states_of(w: &World) -> States {
    w.iter().map(|hs| (**hs).clone()).collect()
}

/// A path: the polys, the points to walk through (None: a teleport), its cost, notes on the
/// way (teleports, drops, doors), its jumps in order, and the spots the box may not fit.
#[derive(Clone, Debug)]
pub struct Path {
    pub polys: Vec<usize>,
    pub waypoints: Vec<Option<Point>>,
    pub cost: f64,
    pub steps: Vec<String>,
    pub jumps: Vec<Jump>,
    pub squeezes: usize,
}

/// One leg of a route: walk `path`, then set off `trigger` (its id) or pick up `key`
/// (neither: the last leg, to the goal).
#[derive(Clone, Debug)]
pub struct Leg {
    pub trigger: Option<usize>,
    pub path: Path,
    pub key: Option<Key>,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub legs: Vec<Leg>,
    /// the heights expected once it's done
    pub states: States,
    /// keys held at the end
    pub keys: Keys,
    /// the goal it reached
    pub goal: Option<Point>,
}

/// What to plan besides the start and the goals.
pub struct Plan<'a> {
    pub keys: Keys,
    pub states: Option<&'a States>,
    pub key_items: &'a [(Key, Vec<(usize, Point)>)],
    pub max_expansions: usize,
    pub max_seconds: f64,
    /// goal point -> the switch (trigger id) used there: the goal only counts once it can be
    /// pressed from there
    pub goal_triggers: &'a [(Point, usize)],
    /// once-only stair switches already used
    pub fired: &'a BTreeSet<usize>,
}

impl Default for Plan<'_> {
    fn default() -> Self {
        Plan { keys: Keys::default(), states: None, key_items: &[], max_expansions: 300, max_seconds: 5.0, goal_triggers: &[], fired: EMPTY }
    }
}

static EMPTY: &BTreeSet<usize> = &BTreeSet::new();

/// The navigator as she knows the level.
pub struct View {
    pub nav: Rc<Navigator>,
    pub triggers: Vec<TriggerLine>,
    /// (polys she may go into, teleport lines she knows); None: everything
    restriction: Option<(BTreeSet<usize>, BTreeSet<usize>)>,
    /// jumps (navigator.jumps indices) she won't take
    banned_jumps: BTreeSet<usize>,
    key_positions: Vec<(Key, Vec<Point>)>,
    id: u64,
    plan_facts: OnceCell<PlanFacts>,
    cross_sides: OnceCell<HashMap<(usize, usize), usize>>,
    lowest: OnceCell<Vec<Heights>>,
    low_variants: RefCell<BTreeMap<usize, LowVariant>>,
}

impl Navigator {
    /// The navigator knowing everything.
    pub fn full_view(self: &Rc<Self>) -> View {
        View::new(Rc::clone(self), self.triggers.clone(), None, self.key_positions(), BTreeSet::new())
    }

    /// A view for what she knows: only `polys` can be walked into, only `teleports` taken, only
    /// `triggers` (as far as she's learned their effects) used, and keys only where
    /// `key_positions` says; and not the jumps in `banned_jumps` (navigator.jumps indices).
    pub fn restricted_to(self: &Rc<Self>, polys: BTreeSet<usize>, teleports: BTreeSet<usize>, triggers: Vec<TriggerLine>, key_positions: Vec<(Key, Vec<Point>)>, banned_jumps: BTreeSet<usize>) -> View {
        View::new(Rc::clone(self), triggers, Some((polys, teleports)), key_positions, banned_jumps)
    }

    /// The gate lines at portal `pi` that block the box coming from poly `frm`: she can't get
    /// her box over the sector beyond them (too high a step, no headroom). Doors she can open
    /// from here (key bars beside a corridor) count as open.
    pub fn gate_blockers(&self, pi: usize, frm: usize, states: &States, doors: Option<&Doors>) -> Vec<usize> {
        let Some(gates) = self.fit.gates.get(&pi) else { return Vec::new() };
        let sector = self.mesh.poly(frm).sector;
        let mut here = states[sector].clone();
        if let Some(d) = doors.and_then(|d| d.get(&sector)) {
            for &h in &d.heights {
                add_height(&mut here, h);
            }
        }
        let mut out = Vec::new();
        for (li, _, (f, b)) in gates {
            for s in [*f, *b] {
                if s == sector {
                    continue;
                }
                let mut hs = states[s].clone();
                if let Some(d) = doors.and_then(|d| d.get(&s)).filter(|d| d.users.contains(&frm)) {
                    for &h in &d.heights {
                        add_height(&mut hs, h);
                    }
                }
                if !here.iter().any(|&ha| hs.iter().any(|&hb| can_cross(ha, hb))) {
                    out.push(*li);
                    break;
                }
            }
        }
        out
    }

    /// Can she cross this portal from poly `frm`? With `doors`, a door sector counts as open
    /// entered from a poly it can be used from, or standing in it.
    pub fn passable(&self, pi: usize, frm: usize, states: &States, doors: Option<&Doors>) -> bool {
        if !self.statically_passable(pi, frm) {
            return false;
        }
        let to = self.mesh.portals[pi].other(frm);
        let (sa, sb) = (self.mesh.poly(frm).sector, self.mesh.poly(to).sector);
        let mut blocking = self.gate_blockers(pi, frm, states, doors);
        blocking.sort_unstable();
        if !self.fit.fits_without(pi, &blocking) {
            return false;
        }
        sa == sb || self.heights_passable(pi, frm, sa, sb, states, doors)
    }

    fn heights_passable(&self, pi: usize, frm: usize, sa: usize, sb: usize, states: &States, doors: Option<&Doors>) -> bool {
        let mut a = states[sa].clone();
        let mut b = states[sb].clone();
        if let Some(d) = doors {
            if let Some(o) = d.get(&sa) {
                for &h in &o.heights {
                    add_height(&mut a, h);
                }
            }
            if let Some(o) = d.get(&sb).filter(|o| o.users.contains(&frm)) {
                for &h in &o.heights {
                    add_height(&mut b, h);
                }
            }
        }
        let linedef = self.mesh.portals[pi].linedef;
        a.iter().any(|&ha| b.iter().any(|&hb| can_cross(ha, hb) && (ha.floor - hb.floor <= MAX_STEP || linedef.is_none_or(|li| self.landable(li, sa, ha.floor)))))
    }

    /// Where to stand to use a switch, or to walk to so a walk-over line is crossed. A switch
    /// spot the box can't be at (barrels before E1M9's switch) is moved straight back from the
    /// line, as far as use range allows.
    pub fn activation_point(&self, t: &TriggerLine, pos: Point, reach: &BTreeSet<usize>) -> Point {
        let spot = self.activation_spot(t, pos, reach);
        if t.line < 0 || !matches!(t.info.trigger, Trigger::S1 | Trigger::SR | Trigger::D1 | Trigger::DR) || !self.box_blocked(spot) {
            return spot;
        }
        let (a, b) = self.map.line_seg(t.line as usize);
        let q = closest_point(spot, a, b);
        let d = dist(spot, q);
        if d < 1.0 {
            return spot;
        }
        let (ux, uy) = ((spot.0 - q.0) / d, (spot.1 - q.1) / d);
        let mut back = d.floor() as i64 + 4;
        while back < (USE_REACH + 13.0) as i64 {
            let p = (q.0 + ux * back as f64, q.1 + uy * back as f64);
            if self.mesh.poly_at(p).is_some_and(|pi| reach.contains(&pi)) && !self.box_blocked(p) {
                return p;
            }
            back += 4;
        }
        spot
    }

    fn activation_spot(&self, t: &TriggerLine, pos: Point, reach: &BTreeSet<usize>) -> Point {
        let mesh = &self.mesh;
        let in_reach: Vec<usize> = t.activators.intersection(reach).copied().collect();
        let first = || mesh.poly(in_reach.first().copied().or_else(|| t.activators.first().copied()).unwrap_or(0)).center;
        let Some(li) = t.linedef() else { return first() };
        let (a, b) = self.map.line_seg(li);
        let length = dist(a, b).max(1.0);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length); // (to the front)
        let front = (mid.0 + nx * 20.0, mid.1 + ny * 20.0);
        let back = (mid.0 - nx * 20.0, mid.1 - ny * 20.0);
        let ok: Vec<Point> = [front, back].into_iter().filter(|&p| mesh.poly_at(p).is_some_and(|pi| reach.contains(&pi))).collect();
        if t.info.trigger.walked() && !ok.is_empty() {
            // far side: cross the line
            return ok.iter().copied().reduce(|x, y| if dist(y, pos) > dist(x, pos) { y } else { x }).unwrap_or(front);
        }
        if t.info.trigger.shot() {
            // anywhere with a view: the shooting spot nearest
            if let Some(c) = in_reach.iter().map(|&p| mesh.poly(p).center).reduce(|x, y| if dist(y, pos) < dist(x, pos) { y } else { x }) {
                return c;
            }
        }
        if ok.contains(&front) {
            return front;
        }
        // the spot nearest the line in any poly it can be used from...
        let mut best: Option<(f64, Point)> = None;
        for &pi in &in_reach {
            let poly = mesh.poly(pi);
            let spot = super::spot_in_poly(&poly.vertices, poly.center, mid, PLAYER_RADIUS + 2.0);
            if mesh.poly_at(spot) == Some(pi) {
                let d = dist(spot, closest_point(spot, a, b));
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, spot));
                }
            }
        }
        if let Some((d, spot)) = best {
            if d <= USE_REACH + 12.0 {
                return spot;
            }
        }
        // ...unless that's out of reach: then a spot in reach the box can stand at, nearest the
        // line, along those polys' edges (MAP14's door, pressed from a floor 48 below)
        let mut near: Option<(f64, Point)> = None;
        for &pi in &in_reach {
            let poly = mesh.poly(pi);
            let vs = &poly.vertices;
            let n = vs.len();
            for i in 0..n {
                let (p1, p2) = (vs[i], vs[(i + 1) % n]);
                for k in 0..9 {
                    let k = f64::from(k);
                    let e = (p1.0 + (p2.0 - p1.0) * k / 8.0, p1.1 + (p2.1 - p1.1) * k / 8.0);
                    let d = dist(e, poly.center);
                    if d < 1e-6 {
                        continue;
                    }
                    let f = 1.0f64.min((PLAYER_RADIUS + 1.0) / d);
                    let q = (e.0 + (poly.center.0 - e.0) * f, e.1 + (poly.center.1 - e.1) * f);
                    let r = segment_distance(a, b, q, q);
                    if r > USE_REACH + 12.0 || near.is_some_and(|(nd, _)| r >= nd) {
                        continue;
                    }
                    if mesh.poly_at(q) == Some(pi) && !self.box_blocked(q) {
                        near = Some((r, q));
                    }
                }
            }
        }
        if let Some((_, q)) = near {
            return q;
        }
        best.map_or_else(first, |(_, s)| s)
    }
}

impl View {
    fn new(nav: Rc<Navigator>, triggers: Vec<TriggerLine>, restriction: Option<(BTreeSet<usize>, BTreeSet<usize>)>, key_positions: Vec<(Key, Vec<Point>)>, banned_jumps: BTreeSet<usize>) -> View {
        View {
            nav,
            triggers,
            restriction,
            banned_jumps,
            key_positions,
            id: NEXT_VIEW.fetch_add(1, Ordering::Relaxed),
            plan_facts: OnceCell::new(),
            cross_sides: OnceCell::new(),
            lowest: OnceCell::new(),
            low_variants: RefCell::new(BTreeMap::new()),
        }
    }

    /// The core, set up for this view.
    fn with_core<T>(&self, f: impl FnOnce(&mut NavCore, &mut PlanCaches) -> T) -> T {
        let mut core = self.nav.core.borrow_mut();
        let core = &mut *core;
        if core.owner != self.id {
            let nav = core.nav.as_mut().expect("the navigator's core is set up");
            let banned: HashSet<(usize, usize)> = self
                .nav
                .tele_from
                .iter()
                .enumerate()
                .flat_map(|(p, edges)| edges.iter().enumerate().filter(|(_, e)| matches!(e, TeleEdge::Jump(j) if self.banned_jumps.contains(j))).map(move |(ti, _)| (p, ti)))
                .collect();
            match &self.restriction {
                Some((polys, teleports)) => nav.restrict(Some(polys.iter().copied().collect::<HashSet<usize>>()), Some(teleports.iter().copied().collect()), banned),
                None => nav.restrict(None, None, banned),
            }
            core.caches = PlanCaches::default();
            core.owner = self.id;
        }
        f(core.nav.as_mut().expect("the navigator's core is set up"), &mut core.caches)
    }

    pub fn initial_states(&self) -> States {
        self.nav.initial_states()
    }

    /// The jumps (navigator.jumps indices) left out.
    pub fn banned_jumps(&self) -> &BTreeSet<usize> {
        &self.banned_jumps
    }

    pub fn local_doors(&self, keys: Keys) -> Doors {
        Navigator::local_doors(&self.triggers, keys)
    }

    /// How the flood fill gets from `start` to `goal` with these heights, if it does: a line a
    /// poly on the way, with its sector and how it was entered.
    pub fn reach_chain(&self, start: usize, goal: usize, states: &States, doors: Option<&Doors>) -> Option<Vec<String>> {
        let doors = doors.filter(|d| !d.is_empty());
        let chain = self.with_core(|nav, _| nav.reach_chain(start, goal, &mut StatesSource::new(states), doors))?;
        let n = &self.nav;
        Some(
            chain
                .iter()
                .map(|&(p, via)| {
                    let how = match via {
                        None => "start".to_string(),
                        Some(Via::Portal(pi)) => format!("portal {pi} (line {:?})", n.mesh.portals[pi].linedef),
                        Some(Via::Tele(ti)) => {
                            let from = chain.iter().position(|c| c.0 == p).and_then(|i| i.checked_sub(1)).map_or(p, |i| chain[i].0);
                            match n.tele_from[from].get(ti) {
                                Some(TeleEdge::Jump(j)) => format!("run-jump {j} off ({:.0}, {:.0}), requires {:?}", n.jumps[*j].takeoff.0, n.jumps[*j].takeoff.1, n.jumps[*j].requires),
                                Some(TeleEdge::Teleport(i)) => format!("teleport via line {}", n.teleports[*i].line),
                                None => format!("tele {ti}"),
                            }
                        }
                    };
                    format!("poly {p} sector {} ({:.0}, {:.0}): {how}", n.sector_of_poly(p), n.mesh.center(p).0, n.mesh.center(p).1)
                })
                .collect(),
        )
    }

    /// The same, a sector moved only for good one way on a visit (see `saturate`).
    fn reachable_phased(&self, start: usize, states: &States, doors: Option<&Doors>, after: &AfterOnly) -> BTreeSet<usize> {
        let doors = doors.filter(|d| !d.is_empty());
        self.with_core(|nav, _| nav.reachable_phased(start, &mut StatesSource::new(states), doors, after)).into_iter().collect()
    }

    /// Polys she can get to from `start` with these heights (and use-doors she can open).
    pub fn reachable(&self, start: usize, states: &States, doors: Option<&Doors>) -> BTreeSet<usize> {
        let doors = doors.filter(|d| !d.is_empty());
        self.with_core(|nav, _| nav.reachable(start, &mut StatesSource::new(states), doors)).into_iter().collect()
    }

    /// A path from `start` to `goal` (A* over polys; a stretch the box can't get through is
    /// searched round; if every way squeezes, the least squeezed, or None with strict_fit).
    pub fn find_path(&self, start: Point, goal: Point, states: &States, doors: Option<&Doors>) -> Option<Path> {
        let doors = doors.filter(|d| !d.is_empty());
        let strict = self.nav.strict_fit;
        let found = self.with_core(|nav, _| nav.find_path(start, goal, &mut StatesSource::new(states), doors, strict))?;
        Some(self.path_of(&found, states, doors))
    }

    fn path_of(&self, f: &Found, states: &States, doors: Option<&Doors>) -> Path {
        let n = &self.nav;
        // (poly, how it was entered)
        let mut chain: Vec<(usize, Option<Step>)> = Vec::with_capacity(f.polys.len());
        for (i, (&poly, via)) in f.polys.iter().zip(&f.vias).enumerate() {
            chain.push((
                poly,
                via.map(|v| match v {
                    Via::Portal(pi) => Step::Portal(pi),
                    Via::Tele(ti) => Step::Tele(n.tele_from[f.polys[i - 1]][ti]),
                }),
            ));
        }
        let jumps = chain.iter().filter_map(|(_, s)| if let Some(Step::Tele(TeleEdge::Jump(j))) = s { Some(n.jumps[*j].clone()) } else { None }).collect();
        Path { polys: f.polys.clone(), waypoints: f.waypoints.clone(), cost: f.cost, steps: self.annotate(&chain, states, doors), jumps, squeezes: f.squeezes }
    }

    fn annotate(&self, chain: &[(usize, Option<Step>)], states: &States, doors: Option<&Doors>) -> Vec<String> {
        let n = &self.nav;
        let mut notes = Vec::new();
        for i in 1..chain.len() {
            let frm = chain[i - 1].0;
            let (poly, via) = chain[i];
            match via {
                Some(Step::Tele(TeleEdge::Jump(j))) => {
                    let j = &n.jumps[j];
                    notes.push(format!("run-jump off ({:.0}, {:.0}) to ({:.0}, {:.0})", j.takeoff.0, j.takeoff.1, j.dest.0, j.dest.1));
                }
                Some(Step::Tele(TeleEdge::Teleport(t))) => notes.push(format!("teleport via line {}", n.teleports[t].line)),
                Some(Step::Portal(pi)) => {
                    let (sa, sb) = (n.sector_of_poly(frm), n.sector_of_poly(poly));
                    if sa == sb {
                        continue;
                    }
                    if doors.is_some_and(|d| d.contains_key(&sb)) && !n.passable(pi, frm, states, None) {
                        notes.push(format!("open door (sector {sb})"));
                    } else if !n.passable(pi, poly, states, doors) {
                        let low = |s: usize| states[s].iter().map(|h| h.floor).fold(f64::INFINITY, f64::min);
                        let drop = low(sa) - low(sb);
                        if drop > MAX_STEP {
                            notes.push(format!("one-way drop of {drop} into sector {sb}"));
                        }
                    }
                }
                None => {}
            }
        }
        notes
    }

    /// (walk-over line, activator poly) -> the sector across the line from it. A walk-over
    /// line only goes off when her centre gets across it, so from below a ledge it can't be set
    /// off (MAP09's lift line on a 48-unit step: only from the top, dropping over).
    pub fn cross_sides(&self) -> &HashMap<(usize, usize), usize> {
        self.cross_sides.get_or_init(|| {
            let m = &self.nav.map;
            let mut out = HashMap::default();
            for t in &self.triggers {
                let Some(li) = t.linedef() else { continue };
                if !t.info.trigger.walked() {
                    continue;
                }
                let (Some(f), Some(b)) = m.sides(li) else { continue };
                if f == b {
                    continue;
                }
                for &p in &t.activators {
                    let sec = self.nav.sector_of_poly(p);
                    if sec == f || sec == b {
                        out.insert((li, p), if sec == f { b } else { f });
                    }
                }
            }
            out
        })
    }

    /// The trigger's activator polys whose use trace isn't blocked by something shut with these
    /// heights (with `doors`, ones she can open count as open), and for gun lines, where the
    /// floor can be at a height a level shot hits the line from.
    pub fn activators_now(&self, t: &TriggerLine, states: &States, doors: Option<&Doors>) -> BTreeSet<usize> {
        let Some(li) = t.linedef() else { return t.activators.clone() };
        let n = &self.nav;
        let mut out = BTreeSet::new();
        for &p in &t.activators {
            if let Some(gates) = n.use_gates.get(&(li, p)) {
                if !gates.iter().all(|&s| states[s].iter().any(|h| h.ceiling - h.floor > 0.0) || doors.is_some_and(|d| d.contains_key(&s))) {
                    continue;
                }
            }
            if let Some(pairs) = n.use_line_gates.get(&(li, p)) {
                if !pairs.iter().all(|&(f, b)| states[f].iter().any(|a| states[b].iter().any(|c| a.ceiling.min(c.ceiling) - a.floor.max(c.floor) > 0.0))) {
                    continue;
                }
            }
            let sector = n.sector_of_poly(p);
            if let Some(&(lo, hi)) = n.shot_floors.get(&(li, p)) {
                if !states[sector].iter().any(|h| lo <= h.floor && h.floor <= hi) {
                    continue;
                }
            }
            if let Some(&far) = self.cross_sides().get(&(li, p)) {
                if !doors.is_some_and(|d| d.contains_key(&far)) && !states[sector].iter().any(|&a| states[far].iter().any(|&b| can_cross(a, b))) {
                    continue;
                }
            }
            out.insert(p);
        }
        out
    }

    pub fn pickup_spots(&self, x: f64, y: f64, states: &States) -> Vec<(usize, Point)> {
        self.nav.pickup_spots(x, y, states)
    }

    /// Fire every reachable trigger and take every reachable key until nothing changes:
    /// (reach, heights, keys) then. Every height a trigger gives is kept (it needn't be fired:
    /// this is the planner's first check, and mustn't come up short), but a sector moved only
    /// for good is one way on a visit, from before to after (E4M9's lifts 151 and 153, lowered
    /// for good to the room's floor, were walked onto from it at 0 and stepped off at 64 onto
    /// the ledges beside: two dozen goals out of reach, planned for in vain every think).
    pub fn saturate(&self, start: usize, states: &States, keys: Keys, key_items: &[(Key, Vec<(usize, Point)>)], triggers: &[&TriggerLine]) -> (BTreeSet<usize>, States, Keys) {
        let mut states = states.clone();
        let mut keys = keys;
        let mut fired: BTreeSet<i64> = BTreeSet::new();
        // (the heights only a move for good gives, and the sectors something else moves too)
        let mut after = AfterOnly::default();
        let mut mixed: BTreeSet<usize> = BTreeSet::new();
        loop {
            let doors = self.local_doors(keys);
            after.retain(|s, _| !mixed.contains(s));
            let reach = self.reachable_phased(start, &states, Some(&doors), &after);
            let new: Vec<&TriggerLine> = triggers.iter().copied().filter(|t| !fired.contains(&t.line) && t.activators.iter().any(|p| reach.contains(p)) && t.info.lock.satisfied(keys)).collect();
            let new_keys: Vec<Key> = key_items.iter().filter(|(k, spots)| !keys.has(*k) && spots.iter().any(|(p, _)| reach.contains(p))).map(|(k, _)| *k).collect();
            if new.is_empty() && new_keys.is_empty() {
                return (reach, states, keys);
            }
            for k in new_keys {
                keys = keys.with(k);
            }
            for t in new {
                fired.insert(t.line);
                for (s, hs) in self.nav.effects_now(t, &states) {
                    if !t.permanent() {
                        mixed.insert(s);
                    }
                    for h in hs {
                        if t.permanent() && !states[s].contains(&h) {
                            add_height(after.entry(s).or_default(), h);
                        }
                        add_height(&mut states[s], h);
                    }
                }
                if t.relevant.is_some() {
                    // (and as it'd be with everything round it down)
                    for (s, hs) in self.low_variant(t).0 {
                        mixed.insert(s);
                        for h in hs {
                            add_height(&mut states[s], h);
                        }
                    }
                }
            }
        }
    }

    /// A height-dependent trigger's effects with the sectors round it at the lowest any trigger
    /// puts them, and the (sector, heights) that assumes (MAP27: the red key's pedestal comes
    /// down to 8 once the platforms round it are down).
    fn low_variant(&self, t: &TriggerLine) -> LowVariant {
        let init = &self.nav.movers.initial;
        let lowest = self.lowest.get_or_init(|| {
            let mut low = init.clone();
            for u in &self.triggers {
                for (s, hs) in &u.effects {
                    for h in hs {
                        if h.floor < low[*s].floor {
                            low[*s] = *h;
                        }
                    }
                }
            }
            low
        });
        if let Some(v) = self.low_variants.borrow().get(&t.id) {
            return v.clone();
        }
        let relevant = t.relevant.clone().unwrap_or_default();
        let reps: Vec<(usize, Heights)> = relevant.iter().map(|&s| (s, if t.sectors.contains(&s) { init[s] } else { lowest[s] })).collect();
        let assumes: Vec<(usize, Heights)> = reps.iter().copied().filter(|(s, h)| *h != init[*s]).collect();
        let v = (if assumes.is_empty() { Vec::new() } else { self.nav.effects_from_reps(t, &reps) }, assumes);
        self.low_variants.borrow_mut().insert(t.id, v.clone());
        v
    }

    /// The plan search's facts for this view's triggers.
    fn plan_facts(&self) -> &PlanFacts {
        self.plan_facts.get_or_init(|| {
            let n = &self.nav;
            let triggers = self
                .triggers
                .iter()
                .map(|t| {
                    let line = t.linedef();
                    let mut timing = HashMap::default();
                    let mut perm_travel = HashMap::default();
                    if line.is_some() {
                        for (s, hs) in &t.effects {
                            for h in hs {
                                timing.insert((*s, h.key()), n.movers.timing(t.info.special, &t.info.description, *s, *h));
                            }
                            if let Some(h) = hs.first() {
                                perm_travel.insert(*s, n.movers.timing(t.info.special, &t.info.description, *s, *h).travel);
                            }
                        }
                    }
                    let kind = match (line, t.info.trigger) {
                        (None, _) => Kind::Other,
                        (_, Trigger::W1 | Trigger::WR) => Kind::Walk,
                        (_, Trigger::S1 | Trigger::SR) => Kind::Switch,
                        (_, Trigger::D1) => Kind::DoorOnce,
                        (_, Trigger::DR) => Kind::DoorRepeat,
                        (_, Trigger::G1 | Trigger::GR) => Kind::Gun,
                        _ => Kind::Other,
                    };
                    PlanTrigger {
                        line,
                        kind,
                        effects: t.effects.clone(),
                        activators: t.activators.iter().copied().collect(),
                        permanent: t.permanent(),
                        lock_ok: (0..64u8).map(|m| t.info.lock.satisfied(Keys(m))).collect(),
                        timing,
                        perm_travel,
                        shift_from: t.shift_from,
                        once: t.once(),
                        relevant: t.relevant.clone(),
                    }
                })
                .collect();
            let specials: Vec<(u16, Vec<usize>)> = self.triggers.iter().map(|t| (t.info.special, t.sectors.clone())).collect();
            let (movers, trenches) = (Arc::clone(&n.movers), n.trenches.clone());
            PlanFacts {
                triggers,
                use_gates: n.use_gates.iter().map(|(k, v)| (*k, v.iter().copied().collect())).collect(),
                use_line_gates: n.use_line_gates.iter().map(|(k, v)| (*k, v.iter().copied().collect())).collect(),
                shot_floors: n.shot_floors.clone(),
                cross_sides: self.cross_sides().clone(),
                temporary: super::temporary_heights(&self.triggers, &n.movers).into_iter().collect(),
                key_positions: self.key_positions.iter().map(|(k, ps)| (key_bit(*k), ps.clone())).collect(),
                effects_from_reps: Box::new(move |ti, reps| {
                    let (special, sectors) = &specials[ti];
                    super::effects_from_reps(&movers, &trenches, *special, sectors, reps)
                }),
            }
        })
    }

    /// A route from `start` to any of `goals`, using triggers and picking up keys (with
    /// `key_items`) on the way, in whatever order works. Best-first over (the world's heights,
    /// keys held, where she stands); see the plan module. The one-off analysis (the upper bound,
    /// what the relaxed plan needs) is done here first.
    pub fn plan(&self, start: Point, goals: &[Point], q: &Plan<'_>) -> Option<Route> {
        let n = &self.nav;
        let goal_polys: BTreeSet<usize> = goals.iter().filter_map(|&g| n.poly_at(g)).collect();
        let start_poly = n.poly_at(start)?;
        if goal_polys.is_empty() {
            return None;
        }
        let actions: Vec<&TriggerLine> = self.triggers.iter().filter(|t| t.info.trigger != Trigger::DR && !t.effects.is_empty()).collect();
        let states0 = q.states.cloned().unwrap_or_else(|| self.initial_states());
        let (reach_all, states_all, keys_all) = self.saturate(start_poly, &states0, q.keys, q.key_items, &actions);
        if goal_polys.is_disjoint(&reach_all) {
            return None;
        }
        let (rel_t, rel_k) = self.relaxed_needs(start_poly, &goal_polys, &states0, q.key_items, &actions, (&reach_all, &states_all, keys_all));
        if std::env::var_os("EVIL_DEBUG_RELAXED").is_some() {
            eprintln!("relaxed: lines {:?} keys {:?}", rel_t, rel_k.iter().map(Key::name).collect::<Vec<_>>());
        }
        let index: BTreeMap<usize, usize> = self.triggers.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        let query = Query {
            start,
            goals: goals.to_vec(),
            keys: KeySet(q.keys.0),
            key_items: q.key_items.iter().map(|(k, spots)| (key_bit(*k), spots.clone())).collect(),
            max_expansions: q.max_expansions,
            max_seconds: q.max_seconds,
            goal_triggers: q.goal_triggers.iter().filter_map(|(p, id)| index.get(id).map(|&i| (point_key(*p), i))).collect(),
            relevant_lines: rel_t.iter().map(|&l| usize::try_from(l).ok()).collect(),
            relevant_keys: KeySet(rel_k.0),
            strict_fit: n.strict_fit,
            fired: q.fired.clone(),
        };
        let facts = self.plan_facts();
        crate::stats::reset();
        let t0 = std::time::Instant::now();
        let found = self.with_core(|nav, caches| Planner::new(nav, facts, caches).plan(&query, world(&states0)));
        if crate::stats::enabled() {
            eprintln!("{}", crate::stats::report(t0.elapsed().as_secs_f64()));
        }
        let route = found?;
        let legs = route
            .legs
            .iter()
            .map(|leg| {
                let states = states_of(&leg.states);
                let doors = self.local_doors(Keys(leg.keys.0));
                Leg { trigger: leg.trigger.map(|i| self.triggers[i].id), path: self.path_of(&leg.path, &states, Some(&doors).filter(|d| !d.is_empty())), key: leg.key.map(|b| Key::ALL[usize::from(b)]) }
            })
            .collect();
        Some(Route { legs, states: states_of(&route.states), keys: Keys(route.keys.0), goal: Some(route.goal) })
    }

    /// The relaxed plan (order ignored): which triggers and keys a route to `goals` depends on.
    /// A path to the goal in the saturated world; every crossing that needs a moved height names
    /// the trigger(s) making it, every locked door a key; then the same for those triggers'
    /// activators and those keys' spots, till nothing new is needed. It points the search at
    /// the level's critical chain.
    fn relaxed_needs(
        &self,
        start: usize,
        goals: &BTreeSet<usize>,
        states0: &States,
        key_items: &[(Key, Vec<(usize, Point)>)],
        triggers: &[&TriggerLine],
        (reach_all, states_all, keys_all): (&BTreeSet<usize>, &States, Keys),
    ) -> (BTreeSet<i64>, Keys) {
        let n = &self.nav;
        let mesh = &n.mesh;
        let doors_all = self.local_doors(keys_all);
        let mut producers: BTreeMap<(usize, HeightsKey), Vec<&TriggerLine>> = BTreeMap::new();
        let mut assumed: BTreeMap<(usize, HeightsKey), Vec<(usize, Heights)>> = BTreeMap::new();
        for &t in triggers {
            if t.activators.is_disjoint(reach_all) {
                continue;
            }
            for (s, hs) in &t.effects {
                for h in hs {
                    producers.entry((*s, h.key())).or_default().push(t);
                }
            }
            if t.relevant.is_some() {
                let (effects, assumes) = self.low_variant(t);
                for (s, hs) in effects {
                    for h in hs {
                        if let std::collections::btree_map::Entry::Vacant(e) = producers.entry((s, h.key())) {
                            e.insert(vec![t]);
                            assumed.insert((s, h.key()), assumes.clone());
                        }
                    }
                }
            }
        }
        let key_for = |lock: Lock| key_items.iter().map(|(k, _)| *k).find(|k| lock.satisfied(Keys::default().with(*k)));
        let spots_of = |k: Key| -> BTreeSet<usize> { key_items.iter().filter(|(x, _)| *x == k).flat_map(|(_, s)| s.iter().map(|(p, _)| *p)).collect() };
        let mut need_t: BTreeSet<i64> = BTreeSet::new();
        let mut need_k = Keys::default();
        let mut without: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut relied: BTreeSet<(usize, HeightsKey)> = BTreeSet::new();
        let mut queue: Vec<BTreeSet<usize>> = vec![goals.clone()];
        let mut seen_goals: BTreeSet<BTreeSet<usize>> = BTreeSet::new();
        while let Some(target) = queue.pop() {
            if seen_goals.len() >= 200 {
                break;
            }
            if seen_goals.contains(&target) || target.is_disjoint(reach_all) {
                continue;
            }
            seen_goals.insert(target.clone());
            // breadth first with parents, in the saturated world
            let mut parent: BTreeMap<usize, (Option<usize>, Option<Step>)> = BTreeMap::from([(start, (None, None))]);
            let mut todo = std::collections::VecDeque::from([start]);
            let mut end = None;
            while let Some(p) = todo.pop_front() {
                if target.contains(&p) {
                    end = Some(p);
                    break;
                }
                let mut moves: Vec<(usize, Step)> =
                    mesh.poly(p).portals.iter().filter(|&&pi| n.passable(pi, p, states_all, Some(&doors_all))).map(|&pi| (mesh.portals[pi].other(p), Step::Portal(pi))).collect();
                moves.extend(n.tele_from[p].iter().filter(|&&e| !matches!(e, TeleEdge::Jump(j) if self.banned_jumps.contains(&j)) && n.edge_ok(e, states_all)).map(|&e| (n.edge_dest(e).0, Step::Tele(e))));
                for (to, via) in moves {
                    if let std::collections::btree_map::Entry::Vacant(e) = parent.entry(to) {
                        e.insert((Some(p), Some(via)));
                        todo.push_back(to);
                    }
                }
            }
            let Some(end) = end else { continue };
            let mut needed: BTreeSet<(usize, HeightsKey)> = BTreeSet::new();
            let mut needed_h: BTreeMap<(usize, HeightsKey), Heights> = BTreeMap::new();
            let mut p = end;
            while let (Some(prev), via) = parent[&p] {
                match via {
                    Some(Step::Tele(TeleEdge::Jump(j))) => {
                        for &(s, h) in &n.jumps[j].requires {
                            needed.insert((s, h.key()));
                            needed_h.insert((s, h.key()), h);
                        }
                    }
                    Some(Step::Portal(pi)) if !n.passable(pi, prev, states0, None) => {
                        let (sa, sb) = (n.sector_of_poly(prev), n.sector_of_poly(p));
                        let pairs: Vec<(Heights, Heights)> = states_all[sa].iter().flat_map(|&ha| states_all[sb].iter().filter(move |&&hb| can_cross(ha, hb)).map(move |&hb| (ha, hb))).collect();
                        // (fewest heights not there now; of those, the lowest floors: what's been
                        // brought down is the likelier way through, MAP27's pedestal at 8)
                        let new = |ha: &Heights, hb: &Heights| usize::from(!states0[sa].contains(ha)) + usize::from(!states0[sb].contains(hb));
                        if let Some(&(ha, hb)) = pairs.iter().min_by(|(a1, b1), (a2, b2)| new(a1, b1).cmp(&new(a2, b2)).then((a1.floor + b1.floor).total_cmp(&(a2.floor + b2.floor)))) {
                            for (s, h) in [(sa, ha), (sb, hb)] {
                                if !states0[s].contains(&h) {
                                    needed.insert((s, h.key()));
                                    needed_h.insert((s, h.key()), h);
                                }
                            }
                        } else if doors_all.contains_key(&sb) {
                            // a use-door: which key does it need?
                            for t in &self.triggers {
                                if t.sectors.contains(&sb) && t.info.lock != Lock::None {
                                    if let Some(k) = key_for(t.info.lock).filter(|k| !need_k.has(*k)) {
                                        need_k = need_k.with(k);
                                        queue.push(spots_of(k));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                p = prev;
            }
            for sh in needed {
                let cands: Vec<&TriggerLine> = producers.get(&sh).cloned().unwrap_or_default();
                // A producer that can be got to without what it makes (MAP27: the switch that
                // opens a door from inside); if there's none, and the sector is a locked door as
                // well, that door's key is what's really needed (the yellow door before MAP27's
                // blue key).
                let w = without.entry(sh.0).or_insert_with(|| {
                    let mut st2 = states_all.clone();
                    if st2[sh.0].len() > 1 {
                        let h = needed_h[&sh];
                        st2[sh.0].retain(|x| *x != h);
                    }
                    let mut d2 = doors_all.clone();
                    d2.remove(&sh.0);
                    self.reachable(start, &st2, Some(&d2))
                });
                // (not from where the press itself waits on that sector: MAP27's lift switch,
                // pressed from the floor below across the lift's edge only once it's down)
                let free = |t: &TriggerLine| -> BTreeSet<usize> {
                    let li = t.linedef().unwrap_or(usize::MAX);
                    t.activators
                        .iter()
                        .copied()
                        .filter(|&p| !n.use_gates.get(&(li, p)).is_some_and(|g| g.contains(&sh.0)) && !n.use_line_gates.get(&(li, p)).is_some_and(|g| g.iter().any(|pr| pr.0 == sh.0 || pr.1 == sh.0)))
                        .collect()
                };
                let first = cands.iter().copied().find(|t| !free(t).is_disjoint(w));
                if first.is_none() {
                    for dt in &self.triggers {
                        if dt.sectors.contains(&sh.0) && dt.info.lock != Lock::None {
                            if let Some(k) = key_for(dt.info.lock).filter(|k| !need_k.has(*k)) {
                                need_k = need_k.with(k);
                                queue.push(spots_of(k));
                            }
                        }
                    }
                }
                for &(s2, h2) in assumed.get(&sh).into_iter().flatten() {
                    relied.insert((s2, h2.key()));
                    if let Some(t2) = producers.get(&(s2, h2.key())).and_then(|v| v.first()) {
                        if need_t.insert(t2.line) {
                            queue.push(t2.activators.clone());
                        }
                    }
                }
                let chosen: Vec<&TriggerLine> = match first {
                    Some(t) => vec![t],
                    None => cands.iter().take(1).copied().collect(),
                };
                for t in chosen {
                    if !need_t.insert(t.line) {
                        continue;
                    }
                    // where it can be used from without something having to be open in the way
                    // of the press, if anywhere (MAP27's switch for the red key's pedestal,
                    // behind a blue door: the path there names the blue key)
                    let li = t.linedef().unwrap_or(usize::MAX);
                    let clear: BTreeSet<usize> = t.activators.iter().copied().filter(|&p| !n.use_gates.contains_key(&(li, p)) && !n.use_line_gates.contains_key(&(li, p))).collect();
                    let clear_reach: BTreeSet<usize> = clear.intersection(reach_all).copied().collect();
                    queue.push(if clear_reach.is_empty() { t.activators.clone() } else { clear_reach.clone() });
                    if clear_reach.is_empty() {
                        // pressed through a door: its key
                        let gates: BTreeSet<usize> = t.activators.iter().flat_map(|&p| n.use_gates.get(&(li, p)).into_iter().flatten().copied()).collect();
                        for dt in &self.triggers {
                            if dt.sectors.iter().any(|s| gates.contains(s)) && dt.info.lock != Lock::None {
                                if let Some(k) = key_for(dt.info.lock).filter(|k| !need_k.has(*k)) {
                                    need_k = need_k.with(k);
                                    queue.push(spots_of(k));
                                }
                            }
                        }
                    }
                    if t.info.lock != Lock::None {
                        if let Some(k) = key_for(t.info.lock).filter(|k| !need_k.has(*k)) {
                            need_k = need_k.with(k);
                            queue.push(spots_of(k));
                        }
                    }
                }
            }
        }
        // For those the change is what's needed, not the line: any other line making it counts
        // too (MAP27: the platforms can be lowered from several places); only for heights a
        // height-dependent trigger relies on round it (elsewhere the extra lines only widen the
        // search, MAP15).
        for &t in triggers {
            if t.effects.iter().any(|(s, hs)| hs.iter().any(|h| relied.contains(&(*s, h.key())))) {
                need_t.insert(t.line);
            }
        }
        (need_t, need_k)
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Portal(usize),
    Tele(TeleEdge),
}

/// A key's bit in the plan's key masks (Key::ALL's order).
fn key_bit(k: Key) -> u8 {
    Key::ALL.iter().position(|&x| x == k).map_or(0, |i| i as u8)
}
