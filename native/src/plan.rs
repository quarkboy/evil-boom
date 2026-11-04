//! The plan search (Navigator.plan), step for step as evildoom.nav.reach: best-first over
//! worlds (heights possible, keys held, where we stand), with lasting lift/door timers
//! compared by dominance, legs timed against them, and squeezed routes kept as a fallback.

use crate::geom::*;
use crate::nav::{can_cross, Doors, Found, Heights, NavCore, Owned, Src, TeleKind};
use pyo3::prelude::*;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

thread_local! {
    /// (timed paths, their seconds, flood fills, their seconds): EVILDOOM_NATIVE_STATS=1
    pub static STATS: std::cell::RefCell<(usize, f64, usize, f64)> = const { std::cell::RefCell::new((0, 0.0, 0, 0.0)) };
}

thread_local! {
    /// seconds in find_path's phases: A*, push_from_corners, box_fits_path, straighten
    pub static PHASES: std::cell::RefCell<[f64; 4]> = const { std::cell::RefCell::new([0.0; 4]) };
}

pub fn phase(i: usize, t0: Instant) {
    PHASES.with(|p| p.borrow_mut()[i] += t0.elapsed().as_secs_f64());
}

pub const ITEM_RADIUS: f64 = 20.0;
pub const PICKUP_MARGIN: f64 = 2.0;
pub const LEG_SPEED: f64 = 12.0;
pub const USE_REACH: i64 = 48;
pub const TIMER_SETS: usize = 2;
pub const FALLBACK_GRACE: usize = 300;

/// (floor, ceiling) as bits, for sets and maps.
pub type HKey = (u64, u64);
#[inline]
pub fn hk(h: Heights) -> HKey {
    ((h.0 + 0.0).to_bits(), (h.1 + 0.0).to_bits())
}
#[inline]
fn unhk(k: HKey) -> Heights {
    (f64::from_bits(k.0), f64::from_bits(k.1))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Other,  // boss deaths, monster-only lines...
    Walk,   // W1/WR
    Switch, // S1/SR
    D1,
    DR,
    Gun, // G1/GR
}

pub struct Trig {
    pub line: i64,
    pub kind: Kind,
    pub effects: Vec<(usize, Vec<Heights>)>,
    pub activators: HashSet<usize>,
    pub permanent: bool,
    pub lock_ok: Vec<bool>, // by key bitmask (64)
    /// (sector, heights) -> (travel, hold; hold < 0 = stays)
    pub timing: HashMap<(usize, HKey), (f64, f64)>,
    /// permanent moves: sector -> travel to its first target
    pub perm_travel: HashMap<usize, f64>,
}

pub struct PlanData {
    pub triggers: Vec<Trig>,
    pub use_gates: HashMap<(i64, usize), Vec<usize>>,
    pub shot_floors: HashMap<(i64, usize), (f64, f64)>,
    /// (walk-over line, activator poly) -> the sector across the line (Navigator.cross_sides)
    pub cross_sides: HashMap<(i64, usize), usize>,
    pub temporary: HashSet<(usize, HKey)>,
    /// key bit -> where that key is (skill 3), in Navigator._key_positions order
    pub key_positions: Vec<(u8, Vec<Point>)>,
    pub door_cache: HashMap<u8, Arc<Doors>>,
    /// flood fills already done: (start poly, keys, hash of the heights) -> reach
    pub reach_cache: HashMap<(usize, u8, u64), Arc<HashSet<usize>>>,
}

/// A hash of every sector's heights (the world a flood fill depends on, with the keys).
fn world_hash(states: &World) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for hs in states.iter() {
        hs.len().hash(&mut h);
        for &(f, c) in hs.iter() {
            f.to_bits().hash(&mut h);
            c.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

type World = Rc<Vec<Rc<Vec<Heights>>>>;
type Temp = BTreeMap<(usize, HKey), f64>;
type Pair = (usize, HKey);

pub struct LegOut {
    pub trig: Option<usize>,
    pub key: Option<u8>,
    pub path: Rc<Found>,
    pub states: World,
    pub keys: u8,
}

pub struct RouteOut {
    pub legs: Vec<Rc<LegOut>>,
    pub states: World,
    pub keys: u8,
    pub goal: Point,
}

struct Pending {
    pos: Point,
    states: World,
    keys: u8,
    trig: Option<usize>,
    key: Option<u8>,
    temp: Rc<Temp>,
}

struct Node {
    pos: Point,
    states: World,
    added: BTreeSet<Pair>,
    keys: u8,
    legs: Rc<Vec<Rc<LegOut>>>,
    reach: Rc<HashSet<usize>>,
    steps: usize,
    pending: Option<Pending>,
    temp: Rc<Temp>,
    squeezes: usize,
    world: Option<Rc<HashSet<usize>>>,
    stable: Option<Rc<HashSet<usize>>>,
    rank: (i64, bool, bool, i64, usize),
}

/// (can't reach a goal, squeezes, not relevant, not progress, -len(stable), steps), counter
#[derive(PartialEq, Eq)]
struct Prio((bool, usize, i64, bool, bool, i64, usize), u64);
impl PartialOrd for Prio {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Prio {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.cmp(&self.0).then_with(|| o.1.cmp(&self.1)) // min-heap
    }
}

fn without(states: &World, pairs: &[Pair]) -> World {
    let mut sorted: Vec<Pair> = pairs.to_vec();
    sorted.sort();
    let mut out: Vec<Rc<Vec<Heights>>> = (**states).clone();
    for (s, h) in sorted {
        let hv = unhk(h);
        if out[s].len() > 1 && out[s].iter().any(|x| hk(*x) == h) {
            out[s] = Rc::new(out[s].iter().copied().filter(|x| hk(*x) != hk(hv)).collect());
        }
    }
    Rc::new(out)
}

fn sorted_reach(r: &HashSet<usize>) -> Vec<usize> {
    let mut v: Vec<usize> = r.iter().copied().collect();
    v.sort_unstable();
    v
}

impl NavCore {
    fn pd(&self) -> &PlanData {
        self.plan_data.as_ref().expect("plan data not set")
    }

    fn doors_for(&mut self, keys: u8) -> Arc<Doors> {
        if let Some(d) = self.plan_data.as_ref().unwrap().door_cache.get(&keys) {
            return d.clone();
        }
        let pd = self.plan_data.as_ref().unwrap();
        let mut out: Doors = HashMap::new();
        for t in &pd.triggers {
            if !(t.kind == Kind::DR || t.kind == Kind::D1) || !t.lock_ok[keys as usize] {
                continue;
            }
            for (s, hs) in &t.effects {
                let e = out.entry(*s).or_insert_with(|| (Vec::new(), HashSet::new()));
                for h in hs {
                    if !e.0.iter().any(|x| hk(*x) == hk(*h)) {
                        e.0.push(*h);
                    }
                }
                e.1.extend(t.activators.iter().copied());
            }
        }
        let rc = Arc::new(out);
        self.plan_data.as_mut().unwrap().door_cache.insert(keys, rc.clone());
        rc
    }

    /// Navigator.activators_now
    fn activators_now(&self, ti: usize, states: &World, doors: Option<&Doors>) -> HashSet<usize> {
        let pd = self.pd();
        let t = &pd.triggers[ti];
        if t.line < 0 {
            return t.activators.clone();
        }
        let mut out = HashSet::new();
        for &p in &t.activators {
            if let Some(gs) = pd.use_gates.get(&(t.line, p)) {
                let ok = gs.iter().all(|&s| states[s].iter().any(|h| h.1 - h.0 > 0.0) || doors.map_or(false, |d| d.contains_key(&s)));
                if !ok {
                    continue;
                }
            }
            if let Some(&(lo, hi)) = pd.shot_floors.get(&(t.line, p)) {
                if !states[self.poly_sector[p]].iter().any(|h| lo <= h.0 && h.0 <= hi) {
                    continue;
                }
            }
            if let Some(&far) = pd.cross_sides.get(&(t.line, p)) {
                let open = doors.map_or(false, |d| d.contains_key(&far));
                if !open && !states[self.poly_sector[p]].iter().any(|&a| states[far].iter().any(|&b| can_cross(a, b))) {
                    continue;
                }
            }
            out.insert(p);
        }
        out
    }

    fn spot_in_poly(&self, poly: usize, target: Point) -> Point {
        let inset = PLAYER_RADIUS + 2.0;
        let p = self.mesh.polys[poly].as_ref().unwrap();
        let vs = &p.vertices;
        let mut best: Option<Point> = None;
        for i in 0..vs.len() {
            let q = closest_point(target, vs[i], vs[(i + 1) % vs.len()]);
            if best.map_or(true, |b| dist(q, target) < dist(b, target)) {
                best = Some(q);
            }
        }
        let best = best.unwrap();
        let (cx, cy) = p.center;
        let d = dist(best, (cx, cy));
        if d <= inset {
            return p.center;
        }
        (best.0 + (cx - best.0) * inset / d, best.1 + (cy - best.1) * inset / d)
    }

    fn center(&self, poly: usize) -> Point {
        self.mesh.polys[poly].as_ref().unwrap().center
    }

    /// Navigator._activation_spot
    fn activation_spot(&self, ti: usize, pos: Point, reach: &HashSet<usize>) -> Point {
        let t = &self.pd().triggers[ti];
        let inter: Vec<usize> = {
            let mut v: Vec<usize> = t.activators.iter().copied().filter(|p| reach.contains(p)).collect();
            v.sort_unstable();
            v
        };
        if t.line < 0 {
            let p = inter.first().copied().unwrap_or_else(|| *t.activators.iter().min().unwrap());
            return self.center(p);
        }
        let (a, b) = self.line_seg(t.line as usize);
        let length = dist(a, b).max(1.0);
        let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let front = (mx + nx * 20.0, my + ny * 20.0);
        let back = (mx - nx * 20.0, my - ny * 20.0);
        let ok: Vec<Point> = [front, back].into_iter().filter(|p| self.mesh.poly_at(p.0, p.1).map_or(false, |q| reach.contains(&q))).collect();
        if t.kind == Kind::Walk && !ok.is_empty() {
            // max by distance (first of equals)
            let mut best = ok[0];
            for &p in &ok[1..] {
                if dist(p, pos) > dist(best, pos) {
                    best = p;
                }
            }
            return best;
        }
        if t.kind == Kind::Gun {
            let spots: Vec<Point> = inter.iter().map(|&p| self.center(p)).collect();
            if !spots.is_empty() {
                let mut best = spots[0];
                for &c in &spots[1..] {
                    if dist(c, pos) < dist(best, pos) {
                        best = c;
                    }
                }
                return best;
            }
        }
        if ok.contains(&front) {
            return front;
        }
        let mut best: Option<(f64, Point)> = None;
        for &pi in &inter {
            let spot = self.spot_in_poly(pi, (mx, my));
            if self.mesh.poly_at(spot.0, spot.1) == Some(pi) {
                let d = dist(spot, closest_point(spot, a, b));
                if best.map_or(true, |bb| d < bb.0) {
                    best = Some((d, spot));
                }
            }
        }
        match best {
            Some((_, s)) => s,
            None => self.center(*inter.iter().min().unwrap()),
        }
    }

    fn box_hits_static(&self, p: Point) -> PyResult<bool> {
        let none = HashSet::new();
        let mut dummy = Owned(&[]);
        Ok(!self.box_hits(p, p, &none, None, &mut dummy)?.is_empty())
    }

    /// Navigator._activation_point
    fn activation_point(&self, ti: usize, pos: Point, reach: &HashSet<usize>) -> PyResult<Point> {
        let spot = self.activation_spot(ti, pos, reach);
        let t = &self.pd().triggers[ti];
        if t.line < 0 || !matches!(t.kind, Kind::Switch | Kind::D1 | Kind::DR) {
            return Ok(spot);
        }
        if !self.box_hits_static(spot)? {
            return Ok(spot);
        }
        let (a, b) = self.line_seg(t.line as usize);
        let q = closest_point(spot, a, b);
        let d = dist(spot, q);
        if d < 1.0 {
            return Ok(spot);
        }
        let (ux, uy) = ((spot.0 - q.0) / d, (spot.1 - q.1) / d);
        let mut back = d as i64 + 4;
        while back < USE_REACH + 13 {
            let p = (q.0 + ux * back as f64, q.1 + uy * back as f64);
            if let Some(poly) = self.mesh.poly_at(p.0, p.1) {
                if reach.contains(&poly) && !self.box_hits_static(p)? {
                    return Ok(p);
                }
            }
            back += 4;
        }
        Ok(spot)
    }

    /// Navigator.pickup_spots
    fn pickup_spots(&self, x: f64, y: f64, states: &World) -> Vec<(usize, Point)> {
        let Some(home) = self.mesh.poly_at(x, y) else { return Vec::new() };
        let hs = self.poly_sector[home];
        let mut spots = vec![(home, (x, y))];
        let item_floors: Vec<f64> = states[hs].iter().map(|h| h.0).collect();
        let reach = PLAYER_RADIUS + ITEM_RADIUS - PICKUP_MARGIN;
        for (pi_poly, poly) in self.mesh.polys.iter().enumerate() {
            let Some(poly) = poly else { continue };
            if poly.sector != hs {
                continue;
            }
            for &pi in &poly.portals {
                let portal = &self.mesh.portals[pi];
                let other_i = if portal.a == pi_poly { portal.b } else { portal.a };
                let other = self.mesh.polys[other_i].as_ref().unwrap();
                if other.sector == hs {
                    continue;
                }
                let near = closest_on_segment((x, y), portal.p1, portal.p2);
                if !item_floors.iter().any(|&fi| states[other.sector].iter().any(|h| -8.0 <= fi - h.0 && fi - h.0 <= PLAYER_HEIGHT)) {
                    continue;
                }
                let blocks = portal.linedef.map_or(false, |li| self.map.linedefs[li].flags & (0x0001 | 0x2000) != 0);
                let steppable = states[other.sector].iter().any(|&ho| states[hs].iter().any(|&hh| can_cross(ho, hh)));
                let back = if blocks || !steppable { PLAYER_RADIUS + 1.0 } else { 8.0 };
                let (cx, cy) = other.center;
                let d = dist(near, (cx, cy)).max(1.0);
                let spot = (near.0 + (cx - near.0) * back / d, near.1 + (cy - near.1) * back / d);
                if (spot.0 - x).abs() < reach && (spot.1 - y).abs() < reach && self.mesh.poly_at(spot.0, spot.1) == Some(other_i) {
                    spots.push((other_i, spot));
                }
            }
        }
        spots
    }

    fn apply_trigger(&self, states: &World, ti: usize) -> World {
        let t = &self.pd().triggers[ti];
        let mut out: Vec<Rc<Vec<Heights>>> = (**states).clone();
        for (s, hs) in &t.effects {
            let cur_min = out[*s].iter().map(|h| h.0).fold(f64::INFINITY, f64::min);
            let hs_max = hs.iter().map(|h| h.0).fold(f64::NEG_INFINITY, f64::max);
            let mut dedup: Vec<Heights> = Vec::new();
            if t.permanent && hs_max < cur_min {
                for h in hs {
                    if !dedup.iter().any(|x| hk(*x) == hk(*h)) {
                        dedup.push(*h);
                    }
                }
            } else {
                dedup = (*out[*s]).clone();
                for h in hs {
                    if !dedup.iter().any(|x| hk(*x) == hk(*h)) {
                        dedup.push(*h);
                    }
                }
            }
            out[*s] = Rc::new(dedup);
        }
        Rc::new(out)
    }

    /// Navigator._arrival_tics
    fn arrival_tics(&self, path: &Found) -> HashMap<usize, f64> {
        let mut out: HashMap<usize, f64> = HashMap::new();
        let mut dist_so_far = 0.0;
        let mut prev: Option<Point> = None;
        let note = |me: &Self, out: &mut HashMap<usize, f64>, q: Point, d: f64| {
            if let Some(s) = me.sector_at(q) {
                out.entry(s).or_insert(10.0 + d / LEG_SPEED);
            }
        };
        for p in &path.waypoints {
            let Some(p) = *p else {
                prev = None;
                continue;
            };
            match prev {
                None => note(self, &mut out, p, dist_so_far),
                Some(pr) => {
                    let seg = dist(pr, p);
                    let n = ((seg / 8.0).floor() as i64).max(1);
                    for i in 1..=n {
                        let fi = i as f64;
                        let nf = n as f64;
                        note(self, &mut out, (pr.0 + (p.0 - pr.0) * fi / nf, pr.1 + (p.1 - pr.1) * fi / nf), dist_so_far + seg * fi / nf);
                    }
                    dist_so_far += seg;
                }
            }
            prev = Some(p);
        }
        for &poly in &path.polys {
            out.entry(self.poly_sector[poly]).or_insert(10.0 + path.cost / LEG_SPEED);
        }
        out
    }

    /// Navigator._timed_path in deadline mode; also the heights the path was found with
    /// (those of temporary heights it'd be too late for left out), for the leg's notes.
    fn timed_path(&mut self, py: Python<'_>, start: Point, goal: Point, states: &World, doors: &Arc<Doors>, deadlines: &Temp, strict: bool) -> PyResult<Option<(Found, World)>> {
        let t0 = Instant::now();
        let out = self.timed_path_(py, start, goal, states, doors, deadlines, strict);
        STATS.with(|s| { let mut s = s.borrow_mut(); s.0 += 1; s.1 += t0.elapsed().as_secs_f64(); });
        out
    }

    fn timed_path_(&mut self, py: Python<'_>, start: Point, goal: Point, states: &World, doors: &Arc<Doors>, deadlines: &Temp, strict: bool) -> PyResult<Option<(Found, World)>> {
        let doors_opt: Option<&Doors> = if doors.is_empty() { None } else { Some(&**doors) };
        let path = self.find_path(py, start, goal, &mut Owned(states), doors_opt, strict)?;
        let Some(path) = path else { return Ok(None) };
        if deadlines.is_empty() {
            return Ok(Some((path, states.clone())));
        }
        let mut arrive = self.arrival_tics(&path);
        for (i, via) in path.vias.iter().enumerate() {
            if let Some((true, ti)) = via {
                let t = &self.tele[path.polys[i - 1]][*ti];
                if let TeleKind::Jump { takeoff, requires, .. } = &t.kind {
                    if let Some(src) = self.sector_at(*takeoff) {
                        if let Some(&at) = arrive.get(&src) {
                            for (s, _h) in requires {
                                let cur = *arrive.get(s).unwrap_or(&f64::INFINITY);
                                arrive.insert(*s, cur.min(at));
                            }
                        }
                    }
                }
            }
        }
        let mut late: Vec<Pair> = deadlines.iter().filter(|(&(s, _), &left)| arrive.get(&s).map_or(false, |&a| a > left)).map(|(k, _)| *k).collect();
        if late.is_empty() {
            return Ok(Some((path, states.clone())));
        }
        late.sort();
        let slower = without(states, &late);
        Ok(self.find_path(py, start, goal, &mut Owned(&slower), doors_opt, strict)?.map(|p| (p, slower)))
    }

    fn reach_from(&mut self, py: Python<'_>, poly: usize, states: &World, keys: u8) -> PyResult<HashSet<usize>> {
        let t0 = Instant::now();
        let out = self.reach_from_(py, poly, states, keys);
        STATS.with(|s| { let mut s = s.borrow_mut(); s.2 += 1; s.3 += t0.elapsed().as_secs_f64(); });
        out
    }

    fn reach_from_(&mut self, py: Python<'_>, poly: usize, states: &World, keys: u8) -> PyResult<HashSet<usize>> {
        let key = (poly, keys, world_hash(states));
        if let Some(r) = self.pd().reach_cache.get(&key) {
            return Ok((**r).clone());
        }
        let doors = self.doors_for(keys);
        let d: Option<&Doors> = if doors.is_empty() { None } else { Some(&*doors) };
        let r = self.reachable(py, poly, &mut Owned(states), d)?;
        let pd = self.plan_data.as_mut().unwrap();
        if pd.reach_cache.len() > 200_000 {
            pd.reach_cache.clear();
        }
        pd.reach_cache.insert(key, Arc::new(r.clone()));
        Ok(r)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        &mut self,
        py: Python<'_>,
        start: Point,
        goals: &[Point],
        keys0: u8,
        states0: World,
        key_items: &[(u8, Vec<(usize, Point)>)],
        max_expansions: usize,
        max_seconds: f64,
        goal_triggers: &HashMap<(u64, u64), usize>,
        rel_t: &HashSet<i64>,
        rel_k: u8,
        strict: bool,
    ) -> PyResult<Option<RouteOut>> {
        let t0 = Instant::now();
        let mut goal_polys: Vec<(usize, Point)> = Vec::new();
        for &g in goals {
            if let Some(gp) = self.mesh.poly_at(g.0, g.1) {
                if !goal_polys.iter().any(|(p, _)| *p == gp) {
                    goal_polys.push((gp, g));
                }
            }
        }
        let Some(start_poly) = self.mesh.poly_at(start.0, start.1) else { return Ok(None) };
        if goal_polys.is_empty() {
            return Ok(None);
        }
        let goal_set: HashSet<usize> = goal_polys.iter().map(|(p, _)| *p).collect();
        let actions: Vec<usize> = (0..self.pd().triggers.len()).filter(|&i| {
            let t = &self.pd().triggers[i];
            t.kind != Kind::DR && !t.effects.is_empty()
        }).collect();
        let mut moving_off: HashSet<Pair> = HashSet::new();
        let root_reach = Rc::new(self.reach_from(py, start_poly, &states0, keys0)?);
        let root = Node {
            pos: start, states: states0, added: BTreeSet::new(), keys: keys0, legs: Rc::new(Vec::new()),
            reach: root_reach, steps: 0, pending: None, temp: Rc::new(BTreeMap::new()), squeezes: 0,
            world: None, stable: None, rank: (0, false, false, 0, 0),
        };
        let mut counter: u64 = 0;
        let mut heap: BinaryHeap<(Prio, std::cmp::Reverse<u64>)> = BinaryHeap::new();
        let mut nodes: HashMap<u64, Node> = HashMap::new();
        nodes.insert(counter, root);
        heap.push((Prio((false, 0, 0, false, false, 0, 0), counter), std::cmp::Reverse(counter)));
        counter += 1;
        type SigKey = (Vec<Pair>, u8, Vec<usize>);
        let mut seen: HashMap<SigKey, Vec<Rc<Temp>>> = HashMap::new();
        let mut expansions = 0usize;
        let mut fallback: Option<(usize, RouteOut, usize, Instant)> = None;
        let deadline = t0 + std::time::Duration::from_secs_f64(max_seconds);

        let tsum = |t: &Temp| t.values().sum::<f64>();
        while let Some((Prio(priority, _), std::cmp::Reverse(id))) = heap.pop() {
            if expansions >= max_expansions || Instant::now() >= deadline {
                break;
            }
            let mut node = nodes.remove(&id).unwrap();
            if let Some(fb) = &fallback {
                if expansions - fb.2 > FALLBACK_GRACE || fb.3.elapsed().as_secs_f64() > 10.0 {
                    return Ok(fallback.map(|f| f.1));
                }
                if node.squeezes > 0 {
                    continue;
                }
            }
            // world signature (what lasts) + timers by dominance
            let sig: SigKey = {
                let added: Vec<Pair> = node.added.iter().copied().filter(|k| !node.temp.contains_key(k)).collect();
                let w = node.world.as_ref().unwrap_or(&node.reach);
                (added, node.keys, sorted_reach(w))
            };
            let tm = node.temp.clone();
            {
                let others = seen.get(&sig).map(|v| v.as_slice()).unwrap_or(&[]);
                let beaten = others.iter().any(|o| tm.iter().all(|(k, v)| o.get(k).map_or(false, |ov| ov >= v)));
                let crowded = others.len() >= TIMER_SETS && tsum(&tm) <= others.iter().map(|o| tsum(o)).fold(f64::INFINITY, f64::min);
                if beaten || crowded {
                    continue;
                }
                // mark
                let mut keep: Vec<Rc<Temp>> = others.iter().filter(|o| !o.iter().all(|(k, v)| tm.get(k).map_or(false, |tv| tv >= v))).cloned().collect();
                keep.push(tm.clone());
                keep.sort_by(|a, b| tsum(b).total_cmp(&tsum(a)));
                let in_top = keep.iter().take(TIMER_SETS).any(|o| **o == *tm);
                let new_list: Vec<Rc<Temp>> = if in_top {
                    keep.into_iter().take(TIMER_SETS).collect()
                } else {
                    let mut v: Vec<Rc<Temp>> = keep.into_iter().take(TIMER_SETS - 1).collect();
                    v.push(tm.clone());
                    v
                };
                seen.insert(sig.clone(), new_list);
            }
            let unmark = |seen: &mut HashMap<SigKey, Vec<Rc<Temp>>>, sig: &SigKey, tm: &Rc<Temp>| {
                if let Some(v) = seen.get_mut(sig) {
                    v.retain(|o| !Rc::ptr_eq(o, tm));
                }
            };
            if let Some(pend) = node.pending.take() {
                let doors = self.doors_for(pend.keys);
                let path = self.timed_path(py, pend.pos, node.pos, &pend.states, &doors, &pend.temp, strict)?;
                let Some((path, used)) = path else {
                    if !pend.temp.is_empty() {
                        unmark(&mut seen, &sig, &tm);
                    }
                    continue;
                };
                let sq = path.squeezes;
                let mut legs = (*node.legs).clone();
                legs.push(Rc::new(LegOut { trig: pend.trig, key: pend.key, path: Rc::new(path), states: used, keys: pend.keys }));
                node.legs = Rc::new(legs);
                if sq > 0 {
                    node.squeezes += sq;
                    unmark(&mut seen, &sig, &tm);
                    let r = node.rank;
                    let id2 = counter;
                    counter += 1;
                    heap.push((Prio((priority.0, node.squeezes, r.0, r.1, r.2, r.3, r.4), id2), std::cmp::Reverse(id2)));
                    nodes.insert(id2, node);
                    continue;
                }
            }
            expansions += 1;
            let doors = self.doors_for(node.keys);
            // goals
            let mut gps: Vec<(usize, Point)> = goal_polys.iter().copied().filter(|(p, _)| node.reach.contains(p)).collect();
            gps.sort_by(|a, b| dist(node.pos, a.1).total_cmp(&dist(node.pos, b.1)).then(a.0.cmp(&b.0)));
            for (gp, gpt) in gps {
                if let Some(&gt) = goal_triggers.get(&crate::geom::pkey(gpt)) {
                    if !self.activators_now(gt, &node.states, None).contains(&gp) {
                        continue;
                    }
                }
                let path = self.timed_path(py, node.pos, gpt, &node.states, &doors, &node.temp, strict)?;
                if let Some((path, used)) = path {
                    let total = node.squeezes + path.squeezes;
                    let mut legs = (*node.legs).clone();
                    legs.push(Rc::new(LegOut { trig: None, key: None, path: Rc::new(path), states: used, keys: node.keys }));
                    let route = RouteOut { legs, states: node.states.clone(), keys: node.keys, goal: gpt };
                    if total == 0 {
                        return Ok(Some(route));
                    }
                    if fallback.as_ref().map_or(true, |f| total < f.0) {
                        fallback = Some((total, route, expansions, Instant::now()));
                    }
                }
            }
            let mut children: Vec<Node> = Vec::new();
            // triggers: one candidate per distinct change, the closest line producing it
            let mut options: Vec<(BTreeSet<Pair>, f64, usize, Point, HashSet<usize>)> = Vec::new();
            for &ti in &actions {
                let mut usable: HashSet<usize> = self.activators_now(ti, &node.states, None).intersection(&node.reach).copied().collect();
                if usable.is_empty() {
                    usable = self.activators_now(ti, &node.states, Some(&doors)).intersection(&node.reach).copied().collect();
                }
                if usable.is_empty() || !self.pd().triggers[ti].lock_ok[node.keys as usize] {
                    continue;
                }
                let new: BTreeSet<Pair> = self.pd().triggers[ti].effects.iter()
                    .flat_map(|(s, hs)| hs.iter().map(move |h| (*s, hk(*h))))
                    .filter(|(s, h)| !node.states[*s].iter().any(|x| hk(*x) == *h))
                    .collect();
                if new.is_empty() {
                    continue;
                }
                let target = self.activation_point(ti, node.pos, &usable)?;
                let d = dist(node.pos, target);
                match options.iter_mut().find(|o| o.0 == new) {
                    Some(o) => {
                        if d < o.1 {
                            o.1 = d;
                            o.2 = ti;
                            o.3 = target;
                            o.4 = usable;
                        }
                    }
                    None => options.push((new, d, ti, target, usable)),
                }
            }
            for (new, _, ti, target, usable) in options {
                let (base, left, gone) = self.expire(&node.states, &node.temp, target, node.pos, &moving_off);
                let trial = self.apply_trigger(&base, ti);
                let mut temp = left.clone();
                for (k, v) in self.deadlines_of(ti, &base, &trial, &mut moving_off) {
                    temp.insert(k, v);
                }
                let mut ordered: Vec<usize> = usable.iter().copied().collect();
                ordered.sort_by(|&a, &b| dist(self.center(a), target).total_cmp(&dist(self.center(b), target)).then(a.cmp(&b)));
                let mut by_sector: Vec<(usize, usize)> = Vec::new();
                for p in ordered {
                    let s = self.poly_sector[p];
                    if !by_sector.iter().any(|(bs, _)| *bs == s) {
                        by_sector.push((s, p));
                    }
                }
                let line = self.pd().triggers[ti].line;
                let near = if line >= 0 { let (a, b) = self.line_seg(line as usize); portal_mid(a, b) } else { target };
                let mut spots = vec![target];
                for &(_, p) in by_sector.iter().take(8) {
                    spots.push(if line >= 0 { self.spot_in_poly(p, near) } else { self.center(p) });
                }
                let mut best = spots[0];
                let mut best_reach: Option<HashSet<usize>> = None;
                let mut covered: HashSet<usize> = HashSet::new();
                for sp in spots {
                    let Some(p) = self.mesh.poly_at(sp.0, sp.1) else { continue };
                    if covered.contains(&p) {
                        continue;
                    }
                    let r = self.reach_from(py, p, &trial, node.keys)?;
                    covered.extend(r.iter().copied());
                    if best_reach.as_ref().map_or(true, |br| r.len() > br.len()) {
                        best = sp;
                        best_reach = Some(r);
                    }
                }
                let mut added: BTreeSet<Pair> = node.added.iter().copied().filter(|k| !gone.contains(k)).collect();
                added.extend(new.iter().copied());
                let carried: BTreeSet<Pair> = left.keys().copied().collect();
                if let Some(c) = self.make_child(py, &node, best, trial, added, node.keys, Some(ti), None, temp, best_reach, carried, &goal_set, rel_t, rel_k)? {
                    children.push(c);
                }
            }
            // keys
            for (kbit, spots0) in key_items {
                if node.keys & (1 << kbit) != 0 {
                    continue;
                }
                let mut spots: Vec<(usize, Point)> = spots0.clone();
                if let Some((_, poss)) = self.pd().key_positions.iter().find(|(b, _)| b == kbit) {
                    let poss = poss.clone();
                    spots = poss.iter().flat_map(|&(x, y)| self.pickup_spots(x, y, &node.states)).collect();
                }
                let here: Vec<(usize, Point)> = spots.into_iter().filter(|(p, _)| node.reach.contains(p)).collect();
                if here.is_empty() {
                    continue;
                }
                let ks = node.keys | (1 << kbit);
                let mut best: Option<((usize, f64), Point)> = None;
                for (p, pt) in here {
                    let n = self.reach_from(py, p, &node.states, ks)?.len();
                    let score = (n, -dist(node.pos, pt));
                    if best.map_or(true, |(b, _)| score.0 > b.0 || (score.0 == b.0 && score.1 > b.1)) {
                        best = Some((score, pt));
                    }
                }
                let spot = best.unwrap().1;
                let (after_states, left, gone) = self.expire(&node.states, &node.temp, spot, node.pos, &moving_off);
                let added: BTreeSet<Pair> = node.added.iter().copied().filter(|k| !gone.contains(k)).collect();
                let carried: BTreeSet<Pair> = left.keys().copied().collect();
                if let Some(c) = self.make_child(py, &node, spot, after_states, added, ks, None, Some(*kbit), left, None, carried, &goal_set, rel_t, rel_k)? {
                    children.push(c);
                }
            }
            for c in children {
                let r = c.rank;
                let goal_unreach = !c.stable.as_ref().unwrap().iter().any(|p| goal_set.contains(p));
                let id2 = counter;
                counter += 1;
                heap.push((Prio((goal_unreach, c.squeezes, r.0, r.1, r.2, r.3, r.4), id2), std::cmp::Reverse(id2)));
                nodes.insert(id2, c);
            }
        }
        Ok(fallback.map(|f| f.1))
    }

    fn expire(&self, states: &World, temp: &Temp, target: Point, frm: Point, moving_off: &HashSet<Pair>) -> (World, Temp, HashSet<Pair>) {
        if temp.is_empty() {
            return (states.clone(), BTreeMap::new(), HashSet::new());
        }
        let took = dist(frm, target) / LEG_SPEED + 10.0;
        let left: Temp = temp.iter().filter(|(k, v)| **v - took > 0.0 && !moving_off.contains(k)).map(|(k, v)| (*k, *v - took)).collect();
        let gone: HashSet<Pair> = temp.keys().copied().filter(|k| !left.contains_key(k)).collect();
        if gone.is_empty() {
            return (states.clone(), left, gone);
        }
        let g: Vec<Pair> = gone.iter().copied().collect();
        (without(states, &g), left, gone)
    }

    fn deadlines_of(&self, ti: usize, base: &World, trial: &World, moving_off: &mut HashSet<Pair>) -> Temp {
        let pd = self.pd();
        let t = &pd.triggers[ti];
        let mut out: Temp = BTreeMap::new();
        for (s, hs) in &t.effects {
            if t.permanent {
                let travel = if t.line >= 0 { *t.perm_travel.get(s).unwrap_or(&0.0) } else { 0.0 };
                let mut olds: Vec<Heights> = trial[*s].iter().copied().filter(|h| !hs.iter().any(|x| hk(*x) == hk(*h))).collect();
                olds.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                let rise = hs[0].0;
                for h in olds {
                    moving_off.insert((*s, hk(h)));
                    // (a rising floor: only until it's a step above, see Navigator.plan)
                    let last = if rise <= h.0 { travel } else { travel.min((travel - 1.0) * MAX_STEP / (rise - h.0) + 1.0) };
                    out.insert((*s, hk(h)), last);
                }
                continue;
            }
            for h in hs {
                if pd.temporary.contains(&(*s, hk(*h))) {
                    let v = if t.line >= 0 {
                        let (travel, hold) = *t.timing.get(&(*s, hk(*h))).unwrap_or(&(0.0, -1.0));
                        travel + if hold >= 0.0 { hold } else { 0.0 }
                    } else {
                        0.0
                    };
                    out.insert((*s, hk(*h)), v);
                }
            }
        }
        let _ = base;
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn make_child(&mut self, py: Python<'_>, node: &Node, target: Point, mut trial: World, mut added: BTreeSet<Pair>, ks: u8, ti: Option<usize>, key: Option<u8>, mut temp: Temp, after: Option<HashSet<usize>>, mut carried: BTreeSet<Pair>, goal_set: &HashSet<usize>, rel_t: &HashSet<i64>, rel_k: u8) -> PyResult<Option<Node>> {
        let _ = goal_set;
        let Some(target_poly) = self.mesh.poly_at(target.0, target.1) else { return Ok(None) };
        let after = match after {
            Some(a) => a,
            None => self.reach_from(py, target_poly, &trial, ks)?,
        };
        let mut stable = after.clone();
        if !carried.is_empty() {
            let c: Vec<Pair> = carried.iter().copied().collect();
            let plain = without(&trial, &c);
            stable = self.reach_from(py, target_poly, &plain, ks)?;
            if stable == after {
                trial = plain;
                temp.retain(|k, _| !carried.contains(k));
                added.retain(|k| !carried.contains(k));
                carried.clear();
            }
        }
        let mut world = stable.clone();
        let fresh: Vec<Pair> = temp.keys().copied().filter(|k| !carried.contains(k)).collect();
        if !fresh.is_empty() {
            let all: Vec<Pair> = temp.keys().copied().collect();
            let plain = without(&trial, &all);
            world = self.reach_from(py, target_poly, &plain, ks)?;
        }
        let prev_stable = node.stable.as_ref().unwrap_or(&node.reach);
        let progress = stable.iter().any(|p| !prev_stable.contains(p)) || key.is_some();
        let relevant = ti.map_or(false, |i| rel_t.contains(&self.pd().triggers[i].line)) || key.map_or(false, |k| rel_k & (1 << k) != 0);
        let steps = node.steps + 1;
        // (needed keys held first: see Navigator.plan)
        let rank = (-((ks & rel_k).count_ones() as i64), !relevant, !progress, -(stable.len() as i64), steps);
        Ok(Some(Node {
            pos: target,
            states: trial,
            added,
            keys: ks,
            legs: node.legs.clone(),
            reach: Rc::new(after),
            steps,
            pending: Some(Pending { pos: node.pos, states: node.states.clone(), keys: node.keys, trig: ti, key, temp: node.temp.clone() }),
            temp: Rc::new(temp),
            squeezes: node.squeezes,
            world: Some(Rc::new(world)),
            stable: Some(Rc::new(stable)),
            rank,
        }))
    }
}
