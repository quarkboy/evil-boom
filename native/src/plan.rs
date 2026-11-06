//! The plan search: best-first over worlds (the heights possible, keys held, where the player
//! stands), stepping by firing a trigger or picking up a key. Temporary heights (lifts down,
//! doors open for a while) carry timers, compared by dominance, and legs are timed against
//! them; routes where the box gets squeezed are kept as a fallback.

mod spots;
mod timing;
mod triggers;
mod world;

pub use triggers::{Kind, PlanCaches, PlanFacts, Timing, Trigger};
pub use world::World;

use crate::geom::{dist, midpoint, point_key, Point, PointKey};
use crate::nav::{Doors, Found, NavCore};
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};
use crate::det::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use world::{apply_effects, expire, without, Pair, Timers};

/// Lines with the same effect tried per node (the closest first).
const ALT_TRIGGERS: usize = 1;
/// Timer sets kept per world signature.
const TIMER_SETS: usize = 2;
/// Expansions to look for a route without squeezes once one with them is found.
const FALLBACK_GRACE: usize = 300;
/// Seconds to look for a route without squeezes once one with them is found.
const FALLBACK_SECONDS: f64 = 600.0;
/// Polys round an activator tried as where to fire it from.
const FIRING_SPOTS: usize = 8;

/// Keys held, as a bitmask by key (evildoom.nav.reach.KEY_ORDER).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct KeySet(pub u8);

impl KeySet {
    pub fn has(self, bit: u8) -> bool {
        self.0 & (1 << bit) != 0
    }

    pub fn with(self, bit: u8) -> KeySet {
        KeySet(self.0 | (1 << bit))
    }

    fn common(self, other: KeySet) -> u32 {
        (self.0 & other.0).count_ones()
    }

    fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// What to plan: from where to which goals, holding which keys, how hard to look, and what
/// the relaxed plan worked out is needed.
pub struct Query {
    pub start: Point,
    pub goals: Vec<Point>,
    pub keys: KeySet,
    /// key bit -> the (poly, point) spots it can be picked up from
    pub key_items: Vec<(u8, Vec<(usize, Point)>)>,
    pub max_expansions: usize,
    pub max_seconds: f64,
    /// goal point -> the switch used there (the goal only counts once it can be pressed)
    pub goal_triggers: HashMap<PointKey, usize>,
    /// lines (None: line-less triggers) and keys the relaxed plan needs
    pub relevant_lines: HashSet<Option<usize>>,
    pub relevant_keys: KeySet,
    /// no routes where the box gets squeezed
    pub strict_fit: bool,
    /// one-shot cumulative lines already used
    pub fired: BTreeSet<usize>,
}

/// One leg of a route: walking to fire a trigger or pick up a key (neither: to the goal),
/// with the heights and keys it was found with.
pub struct Leg {
    pub trigger: Option<usize>,
    pub key: Option<u8>,
    pub path: Rc<Found>,
    pub states: World,
    pub keys: KeySet,
}

pub struct Route {
    pub legs: Vec<Rc<Leg>>,
    pub states: World,
    pub keys: KeySet,
    pub goal: Point,
}

/// The leg into a node, found when the node comes up (most never do).
struct Pending {
    from: Point,
    states: World,
    keys: KeySet,
    trigger: Option<usize>,
    key: Option<u8>,
    timers: Rc<Timers>,
}

/// How promising a node is, best first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
struct Rank {
    /// needed keys held, most first
    needed_keys: Reverse<u32>,
    /// not what the relaxed plan uses
    irrelevant: bool,
    /// nothing new reachable
    no_progress: bool,
    /// most reachable first
    area: Reverse<usize>,
    steps: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Priority {
    no_goal: bool,
    squeezes: usize,
    rank: Rank,
}

struct Node {
    pos: Point,
    states: World,
    /// heights added along the way
    added: BTreeSet<Pair>,
    keys: KeySet,
    legs: Rc<Vec<Rc<Leg>>>,
    reach: Arc<HashSet<usize>>,
    /// reachable without the heights whose timers carried over from before
    stable: Arc<HashSet<usize>>,
    /// reachable without any temporary heights
    world: Arc<HashSet<usize>>,
    steps: usize,
    pending: Option<Pending>,
    timers: Rc<Timers>,
    squeezes: usize,
    rank: Rank,
    /// one-shot cumulative lines used
    fired: Rc<BTreeSet<usize>>,
}

/// What lasts of a node's world, for spotting the same one reached again.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Signature {
    lasting: Vec<Pair>,
    keys: KeySet,
    world: Vec<usize>,
    stairs_fired: Vec<usize>,
}

/// Per signature, the timer sets met (none dominating another), at most TIMER_SETS of them.
#[derive(Default)]
struct TimerSets(HashMap<Signature, Vec<Rc<Timers>>>);

fn timer_total(t: &Timers) -> f64 {
    t.values().sum::<f64>()
}

/// Does `a` have at least as long on every timer of `b`?
fn outlasts(a: &Timers, b: &Timers) -> bool {
    b.iter().all(|(k, v)| a.get(k).is_some_and(|av| av >= v))
}

impl TimerSets {
    /// Note timers `tm` under `sig`, unless timers met there already are as good (or there
    /// are enough of them, all with longer in total).
    fn admit(&mut self, sig: &Signature, tm: &Rc<Timers>) -> bool {
        let others = self.0.get(sig).map(Vec::as_slice).unwrap_or_default();
        let beaten = others.iter().any(|o| outlasts(o, tm));
        let crowded = others.len() >= TIMER_SETS && timer_total(tm) <= others.iter().map(|o| timer_total(o)).fold(f64::INFINITY, f64::min);
        if beaten || crowded {
            return false;
        }
        let mut keep: Vec<Rc<Timers>> = others.iter().filter(|o| !outlasts(tm, o)).cloned().collect();
        keep.push(Rc::clone(tm));
        keep.sort_by(|a, b| timer_total(b).total_cmp(&timer_total(a)));
        let in_top = keep.iter().take(TIMER_SETS).any(|o| **o == **tm);
        let kept = if in_top {
            keep.into_iter().take(TIMER_SETS).collect()
        } else {
            keep.into_iter().take(TIMER_SETS - 1).chain([Rc::clone(tm)]).collect()
        };
        self.0.insert(sig.clone(), kept);
        true
    }

    /// Take back what `admit` noted (the node turned out not to be reachable as it was).
    fn forget(&mut self, sig: &Signature, tm: &Rc<Timers>) {
        if let Some(v) = self.0.get_mut(sig) {
            v.retain(|o| !Rc::ptr_eq(o, tm));
        }
    }
}

/// The best route found where the box gets squeezed, while looking on for one without.
struct Fallback {
    squeezes: usize,
    route: Route,
    expansions: usize,
    found_at: Instant,
}

/// A step from a node: where it leaves the player, and the world after.
struct Step {
    target: Point,
    states: World,
    added: BTreeSet<Pair>,
    keys: KeySet,
    trigger: Option<usize>,
    key: Option<u8>,
    timers: Timers,
    /// what's reachable after, if worked out already
    reach: Option<Arc<HashSet<usize>>>,
    /// timers carried over from before the step
    carried: BTreeSet<Pair>,
}

/// A trigger that could be fired from a node.
struct Candidate {
    dist: f64,
    line: Option<usize>,
    trigger: usize,
    target: Point,
    usable: HashSet<usize>,
}

/// The search over one map's navigator core, plan facts and caches.
pub struct Planner<'a> {
    nav: &'a mut NavCore,
    facts: &'a PlanFacts,
    caches: &'a mut PlanCaches,
    strict_fit: bool,
    /// heights that triggers have started moving off for good
    moving_off: HashSet<Pair>,
}

impl<'a> Planner<'a> {
    pub fn new(nav: &'a mut NavCore, facts: &'a PlanFacts, caches: &'a mut PlanCaches) -> Self {
        Planner { nav, facts, caches, strict_fit: false, moving_off: HashSet::default() }
    }

    /// A route from `q.start` to one of `q.goals`, starting with the world at `states`.
    pub fn plan(&mut self, q: &Query, states: World) -> Option<Route> {
        let t0 = Instant::now();
        self.strict_fit = q.strict_fit;
        self.moving_off.clear();
        let mut goal_polys: Vec<(usize, Point)> = Vec::new();
        for &g in &q.goals {
            if let Some(gp) = self.nav.mesh.poly_at(g) {
                if !goal_polys.iter().any(|(p, _)| *p == gp) {
                    goal_polys.push((gp, g));
                }
            }
        }
        let start_poly = self.nav.mesh.poly_at(q.start)?;
        if goal_polys.is_empty() {
            return None;
        }
        let goal_set: HashSet<usize> = goal_polys.iter().map(|(p, _)| *p).collect();
        let triggers = &self.facts.triggers;
        let push = |heap: &mut BinaryHeap<Reverse<(Priority, usize)>>, nodes: &mut Vec<Option<Node>>, priority: Priority, node: Node| {
            heap.push(Reverse((priority, nodes.len())));
            nodes.push(Some(node));
        };
        let actions: Vec<usize> = (0..triggers.len()).filter(|&i| triggers[i].kind != Kind::DoorRepeat && !triggers[i].effects.is_empty()).collect();
        // (only used stair switches tell worlds apart)
        let stair_lines: HashSet<usize> = actions.iter().filter(|&&i| triggers[i].shift_from.is_some()).filter_map(|&i| triggers[i].line).collect();
        let reach = self.reach_from(start_poly, &states, q.keys);
        let root = Node {
            pos: q.start,
            states,
            added: BTreeSet::new(),
            keys: q.keys,
            legs: Rc::new(Vec::new()),
            stable: Arc::clone(&reach),
            world: Arc::clone(&reach),
            reach,
            steps: 0,
            pending: None,
            timers: Rc::new(Timers::new()),
            squeezes: 0,
            rank: Rank::default(),
            fired: Rc::new(q.fired.clone()),
        };
        let mut nodes: Vec<Option<Node>> = vec![Some(root)];
        let mut heap: BinaryHeap<Reverse<(Priority, usize)>> = BinaryHeap::from([Reverse((Priority { no_goal: false, squeezes: 0, rank: Rank::default() }, 0))]);
        let mut seen = TimerSets::default();
        let mut expansions = 0usize;
        let mut fallback: Option<Fallback> = None;
        let deadline = t0 + Duration::from_secs_f64(q.max_seconds);

        while let Some(Reverse((priority, id))) = heap.pop() {
            if expansions >= q.max_expansions || Instant::now() >= deadline {
                break;
            }
            let Some(mut node) = nodes[id].take() else { continue };
            if let Some(fb) = &fallback {
                if expansions - fb.expansions > FALLBACK_GRACE || fb.found_at.elapsed().as_secs_f64() > FALLBACK_SECONDS {
                    break;
                }
                if node.squeezes > 0 {
                    continue;
                }
            }
            // the same lasting world reached again: only with timers it doesn't beat
            let sig = Signature {
                lasting: node.added.iter().copied().filter(|k| !node.timers.contains_key(k)).collect(),
                keys: node.keys,
                world: sorted(&node.world),
                stairs_fired: node.fired.iter().copied().filter(|l| stair_lines.contains(l)).collect(),
            };
            let tm = Rc::clone(&node.timers);
            if !seen.admit(&sig, &tm) {
                continue;
            }
            if let Some(pend) = node.pending.take() {
                let doors = self.doors_for(pend.keys);
                let Some((path, used)) = self.timed_path(pend.from, node.pos, &pend.states, &doors, &pend.timers) else {
                    if !pend.timers.is_empty() {
                        seen.forget(&sig, &tm);
                    }
                    continue;
                };
                let squeezes = path.squeezes;
                let mut legs = (*node.legs).clone();
                legs.push(Rc::new(Leg { trigger: pend.trigger, key: pend.key, path: Rc::new(path), states: used, keys: pend.keys }));
                node.legs = Rc::new(legs);
                if squeezes > 0 {
                    // back in the queue, behind what isn't squeezed
                    node.squeezes += squeezes;
                    seen.forget(&sig, &tm);
                    let priority = Priority { squeezes: node.squeezes, rank: node.rank, ..priority };
                    push(&mut heap, &mut nodes, priority, node);
                    continue;
                }
            }
            expansions += 1;

            // goals
            let doors = self.doors_for(node.keys);
            let mut gps: Vec<(usize, Point)> = goal_polys.iter().copied().filter(|(p, _)| node.reach.contains(p)).collect();
            gps.sort_by(|a, b| dist(node.pos, a.1).total_cmp(&dist(node.pos, b.1)).then(a.0.cmp(&b.0)));
            for (gp, gpt) in gps {
                if let Some(&gt) = q.goal_triggers.get(&point_key(gpt)) {
                    if !self.activators_now(gt, &node.states, None).contains(&gp) {
                        continue;
                    }
                }
                let Some((path, used)) = self.timed_path(node.pos, gpt, &node.states, &doors, &node.timers) else { continue };
                let total = node.squeezes + path.squeezes;
                let mut legs = (*node.legs).clone();
                legs.push(Rc::new(Leg { trigger: None, key: None, path: Rc::new(path), states: used, keys: node.keys }));
                let route = Route { legs, states: node.states.clone(), keys: node.keys, goal: gpt };
                if total == 0 {
                    return Some(route);
                }
                if fallback.as_ref().is_none_or(|f| total < f.squeezes) {
                    fallback = Some(Fallback { squeezes: total, route, expansions, found_at: Instant::now() });
                }
            }

            let mut children: Vec<Node> = Vec::new();
            for step in self.trigger_steps(&node, &actions, &doors) {
                children.extend(self.make_child(&node, step, q));
            }
            for step in self.key_steps(&node, q) {
                children.extend(self.make_child(&node, step, q));
            }
            for c in children {
                let no_goal = !c.stable.iter().any(|p| goal_set.contains(p));
                push(&mut heap, &mut nodes, Priority { no_goal, squeezes: c.squeezes, rank: c.rank }, c);
            }
        }
        fallback.map(|f| f.route)
    }

    /// Steps firing a trigger: one per distinct change to the world (by the closest line
    /// making it), from wherever round it leaves the most reachable.
    fn trigger_steps(&mut self, node: &Node, actions: &[usize], doors: &Arc<Doors>) -> Vec<Step> {
        let facts = self.facts;
        let mut options: Vec<(BTreeSet<Pair>, Vec<Candidate>)> = Vec::new();
        for &ti in actions {
            let t = &facts.triggers[ti];
            let mut usable: HashSet<usize> = self.activators_now(ti, &node.states, None).intersection(&node.reach).copied().collect();
            if usable.is_empty() {
                usable = self.activators_now(ti, &node.states, Some(doors)).intersection(&node.reach).copied().collect();
            }
            if usable.is_empty() || !t.usable_with(node.keys) || t.line.is_some_and(|l| node.fired.contains(&l)) {
                continue;
            }
            let new: BTreeSet<Pair> = self
                .effects_now(ti, &node.states)
                .iter()
                .flat_map(|(s, hs)| hs.iter().map(move |h| (*s, h.key())))
                .filter(|(s, h)| !node.states[*s].iter().any(|x| x.key() == *h))
                .collect();
            if new.is_empty() {
                continue;
            }
            let target = self.activation_point(ti, node.pos, &usable);
            let cand = Candidate { dist: dist(node.pos, target), line: t.line, trigger: ti, target, usable };
            match options.iter_mut().find(|o| o.0 == new) {
                Some(o) => o.1.push(cand),
                None => options.push((new, vec![cand])),
            }
        }
        let mut steps = Vec::new();
        for (new, mut cands) in options {
            cands.sort_by(|a, b| a.dist.total_cmp(&b.dist).then(a.line.cmp(&b.line)));
            let mut reaches: Vec<Arc<HashSet<usize>>> = Vec::new();
            for cand in cands.into_iter().take(ALT_TRIGGERS) {
                let (ti, target) = (cand.trigger, cand.target);
                let (base, left, gone) = expire(&node.states, &node.timers, node.pos, target, &self.moving_off);
                let effects = self.effects_now(ti, &base);
                let t = &facts.triggers[ti];
                let trial = apply_effects(&base, t.permanent, &effects);
                let mut timers = left.clone();
                let (deadlines, moved_off) = self.deadlines_of(ti, &effects, &base, &trial);
                timers.extend(deadlines);
                self.moving_off.extend(moved_off);
                // where to fire it from: the point found, or a spot in the nearest activator of
                // each sector round it, whichever leaves the most reachable
                let mut ordered: Vec<usize> = cand.usable.iter().copied().collect();
                ordered.sort_by(|&a, &b| dist(self.nav.mesh.center(a), target).total_cmp(&dist(self.nav.mesh.center(b), target)).then(a.cmp(&b)));
                let mut by_sector: Vec<(usize, usize)> = Vec::new();
                for p in ordered {
                    let s = self.nav.poly_sector[p];
                    if !by_sector.iter().any(|(bs, _)| *bs == s) {
                        by_sector.push((s, p));
                    }
                }
                let near = t.line.map_or(target, |line| {
                    let (a, b) = self.nav.map.line_seg(line);
                    midpoint(a, b)
                });
                let mut spots = vec![target];
                for &(_, p) in by_sector.iter().take(FIRING_SPOTS) {
                    spots.push(if t.line.is_some() { self.spot_in_poly(p, near) } else { self.nav.mesh.center(p) });
                }
                let mut best = target;
                let mut best_reach: Option<Arc<HashSet<usize>>> = None;
                let mut covered: HashSet<usize> = HashSet::default();
                for sp in spots {
                    let Some(p) = self.nav.mesh.poly_at(sp) else { continue };
                    if covered.contains(&p) {
                        continue;
                    }
                    let r = self.reach_from(p, &trial, node.keys);
                    covered.extend(r.iter().copied());
                    if best_reach.as_ref().is_none_or(|br| r.len() > br.len()) {
                        best = sp;
                        best_reach = Some(r);
                    }
                }
                if let Some(br) = &best_reach {
                    if reaches.contains(br) {
                        continue;
                    }
                    reaches.push(Arc::clone(br));
                }
                let mut added: BTreeSet<Pair> = node.added.iter().copied().filter(|k| !gone.contains(k)).collect();
                added.extend(new.iter().copied());
                let carried = left.keys().copied().collect();
                steps.push(Step { target: best, states: trial, added, keys: node.keys, trigger: Some(ti), key: None, timers, reach: best_reach, carried });
            }
        }
        steps
    }

    /// Steps picking up a key not held yet: from the spot it can be picked up from that
    /// leaves the most reachable (the nearest of those).
    fn key_steps(&mut self, node: &Node, q: &Query) -> Vec<Step> {
        let mut steps = Vec::new();
        for (kbit, item_spots) in &q.key_items {
            if node.keys.has(*kbit) {
                continue;
            }
            // (at the heights now, if it's known where the key is)
            let spots: Vec<(usize, Point)> = match self.facts.key_positions.iter().find(|(b, _)| b == kbit) {
                Some((_, at)) => at.iter().flat_map(|&(x, y)| self.pickup_spots(x, y, &node.states)).collect(),
                None => item_spots.clone(),
            };
            let keys = node.keys.with(*kbit);
            let mut best: Option<((usize, f64), Point)> = None;
            for (p, pt) in spots.into_iter().filter(|(p, _)| node.reach.contains(p)) {
                let score = (self.reach_from(p, &node.states, keys).len(), -dist(node.pos, pt));
                if best.is_none_or(|(b, _)| score.0 > b.0 || (score.0 == b.0 && score.1 > b.1)) {
                    best = Some((score, pt));
                }
            }
            let Some((_, spot)) = best else { continue };
            let (states, left, gone) = expire(&node.states, &node.timers, node.pos, spot, &self.moving_off);
            let added = node.added.iter().copied().filter(|k| !gone.contains(k)).collect();
            let carried = left.keys().copied().collect();
            steps.push(Step { target: spot, states, added, keys, trigger: None, key: Some(*kbit), timers: left, reach: None, carried });
        }
        steps
    }

    /// The node a step from `node` leads to, its leg to be found when it comes up.
    fn make_child(&mut self, node: &Node, step: Step, q: &Query) -> Option<Node> {
        let Step { target, mut states, mut added, keys, trigger, key, mut timers, reach, mut carried } = step;
        let target_poly = self.nav.mesh.poly_at(target)?;
        let reach = match reach {
            Some(r) => r,
            None => self.reach_from(target_poly, &states, keys),
        };
        let mut stable = Arc::clone(&reach);
        if !carried.is_empty() {
            // the timers from before not needed for what's reachable: let them go
            let plain = without(&states, carried.iter().copied());
            stable = self.reach_from(target_poly, &plain, keys);
            if stable == reach {
                states = plain;
                timers.retain(|k, _| !carried.contains(k));
                added.retain(|k| !carried.contains(k));
                carried.clear();
            }
        }
        let mut world = Arc::clone(&stable);
        if timers.keys().any(|k| !carried.contains(k)) {
            let plain = without(&states, timers.keys().copied());
            world = self.reach_from(target_poly, &plain, keys);
        }
        let progress = stable.iter().any(|p| !node.stable.contains(p)) || key.is_some();
        let relevant = trigger.is_some_and(|i| q.relevant_lines.contains(&self.facts.triggers[i].line)) || key.is_some_and(|k| q.relevant_keys.has(k));
        let steps = node.steps + 1;
        let rank = Rank {
            // (needed keys held first)
            needed_keys: Reverse(keys.common(q.relevant_keys)),
            irrelevant: !relevant,
            no_progress: !progress,
            area: Reverse(stable.len()),
            steps,
        };
        let facts = self.facts;
        let fired = match trigger.map(|i| &facts.triggers[i]) {
            Some(t) if t.once && t.cumulative() => match t.line {
                Some(line) => {
                    let mut f = (*node.fired).clone();
                    f.insert(line);
                    Rc::new(f)
                }
                None => Rc::clone(&node.fired),
            },
            _ => Rc::clone(&node.fired),
        };
        Some(Node {
            pos: target,
            states,
            added,
            keys,
            legs: Rc::clone(&node.legs),
            reach,
            stable,
            world,
            steps,
            pending: Some(Pending { from: node.pos, states: node.states.clone(), keys: node.keys, trigger, key, timers: Rc::clone(&node.timers) }),
            timers: Rc::new(timers),
            squeezes: node.squeezes,
            rank,
            fired,
        })
    }
}

fn sorted(r: &HashSet<usize>) -> Vec<usize> {
    let mut v: Vec<usize> = r.iter().copied().collect();
    v.sort_unstable();
    v
}
