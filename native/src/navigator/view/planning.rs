//! Preparing planner facts and queries from a restricted view.

use super::{key_bit, states_of, world, Leg, LowVariant, Plan, Route, Step, View};
use crate::det::HashMap;
use crate::geom::{point_key, Point};
use crate::nav::{can_cross, Heights, HeightsKey};
use crate::navigator::{States, TeleEdge, TriggerLine};
use crate::plan::{KeySet, Kind, PlanFacts, Planner, Query, Trigger as PlanTrigger};
use crate::specials::{Key, Keys, Lock, Trigger};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// a plan search with every switch, after one with only those the relaxed plan needs found
/// none (more than the most steps a route has been seen to take)
const WIDER_EXPANSIONS: usize = 2000;

/// Bound the relaxed dependency walk independently of the concrete plan search budget.
const RELAXED_GOAL_LIMIT: usize = 200;
/// Share of the full budget used after the narrow search proves its choices insufficient.
const WIDER_BUDGET_DIVISOR: usize = 10;

impl View {
    /// A height-dependent trigger's effects with the sectors round it at the lowest any trigger
    /// puts them, and the (sector, heights) that assumes (a pedestal that comes down further
    /// once the platforms round it are down).
    pub(super) fn low_variant(&self, t: &TriggerLine) -> LowVariant {
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
        let reps: Vec<(usize, Heights)> = relevant
            .iter()
            .map(|&s| {
                (
                    s,
                    if t.sectors.contains(&s) {
                        init[s]
                    } else {
                        lowest[s]
                    },
                )
            })
            .collect();
        let assumes: Vec<(usize, Heights)> = reps
            .iter()
            .copied()
            .filter(|(s, h)| *h != init[*s])
            .collect();
        let v = (
            if assumes.is_empty() {
                Vec::new()
            } else {
                self.nav.effects_from_reps(t, &reps)
            },
            assumes,
        );
        self.low_variants.borrow_mut().insert(t.id, v.clone());
        v
    }

    /// The plan search's facts for this view's triggers.
    pub(super) fn plan_facts(&self) -> &PlanFacts {
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
                                timing.insert(
                                    (*s, h.key()),
                                    n.movers.timing(t.info.special, &t.info.description, *s, *h),
                                );
                            }
                            if let Some(h) = hs.first() {
                                perm_travel.insert(
                                    *s,
                                    n.movers
                                        .timing(t.info.special, &t.info.description, *s, *h)
                                        .travel,
                                );
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
            let specials: Vec<(u16, Vec<usize>)> = self
                .triggers
                .iter()
                .map(|t| (t.info.special, t.sectors.clone()))
                .collect();
            let (movers, trenches) = (Arc::clone(&n.movers), n.trenches.clone());
            PlanFacts {
                triggers,
                use_gates: n
                    .use_gates
                    .iter()
                    .map(|(k, v)| (*k, v.iter().copied().collect()))
                    .collect(),
                use_line_gates: n
                    .use_line_gates
                    .iter()
                    .map(|(k, v)| (*k, v.iter().copied().collect()))
                    .collect(),
                shot_floors: n.shot_floors.clone(),
                cross_sides: self.cross_sides().clone(),
                temporary: crate::navigator::temporary_heights(&self.triggers, &n.movers)
                    .into_iter()
                    .collect(),
                key_positions: self
                    .key_positions
                    .iter()
                    .map(|(k, ps)| (key_bit(*k), ps.clone()))
                    .collect(),
                effects_from_reps: Box::new(move |ti, reps| {
                    let (special, sectors) = &specials[ti];
                    crate::navigator::effects_from_reps(&movers, &trenches, *special, sectors, reps)
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
        let actions: Vec<&TriggerLine> = self
            .triggers
            .iter()
            .filter(|t| t.info.trigger != Trigger::DR && !t.effects.is_empty())
            .collect();
        let states0 = q.states.cloned().unwrap_or_else(|| self.initial_states());
        let (reach_all, states_all, keys_all) =
            self.saturate(start_poly, &states0, q.keys, q.key_items, &actions);
        if goal_polys.is_disjoint(&reach_all) {
            return None;
        }
        let (rel_t, rel_k) = self.relaxed_needs(
            start_poly,
            &goal_polys,
            &states0,
            q.key_items,
            &actions,
            (&reach_all, &states_all, keys_all),
        );
        if crate::debug::on().relaxed {
            eprintln!(
                "relaxed: lines {:?} keys {:?}",
                rel_t,
                rel_k.iter().map(Key::name).collect::<Vec<_>>()
            );
        }
        let index: BTreeMap<usize, usize> = self
            .triggers
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();
        let query = Query {
            start,
            goals: goals.to_vec(),
            keys: KeySet(q.keys.0),
            key_items: q
                .key_items
                .iter()
                .map(|(k, spots)| (key_bit(*k), spots.clone()))
                .collect(),
            max_expansions: q.max_expansions,
            max_seconds: q.max_seconds,
            goal_triggers: q
                .goal_triggers
                .iter()
                .filter_map(|(p, id)| index.get(id).map(|&i| (point_key(*p), i)))
                .collect(),
            relevant_lines: rel_t.iter().map(|&l| usize::try_from(l).ok()).collect(),
            relevant_keys: KeySet(rel_k.0),
            useful_lines: Some(rel_t.iter().map(|&l| usize::try_from(l).ok()).collect()),
            strict_fit: n.strict_fit,
            fired: q.fired.clone(),
        };
        let facts = self.plan_facts();
        crate::stats::reset();
        let t0 = std::time::Instant::now();
        // (first with only the lines the relaxed plan needs, to see if there's any way at all:
        // none, every way with those tried, then with them all as far as the search went before
        // routes needed more. Not every set of every switch in the level first, to find there's
        // no way: with a dozen switches that's thousands of steps and half a minute. A way with
        // those: the route as ever, from them all; the few lines' route can be another, worse)
        let narrow = self.with_core(|nav, caches| {
            Planner::new(nav, facts, caches).plan(&query, world(&states0))
        });
        let none =
            narrow.is_none() && crate::stats::last_ended() == Some(crate::stats::Ended::Exhausted);
        // (a tenth of the budget, which comes down as searches use theirs up: the explorer's)
        let max_expansions = if none {
            (q.max_expansions / WIDER_BUDGET_DIVISOR).min(WIDER_EXPANSIONS)
        } else {
            q.max_expansions
        };
        let wider = Query {
            useful_lines: None,
            max_expansions,
            ..query
        };
        let found = self.with_core(|nav, caches| {
            Planner::new(nav, facts, caches).plan(&wider, world(&states0))
        });
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
                Leg {
                    trigger: leg.trigger.map(|i| self.triggers[i].id),
                    path: self.path_of(&leg.path, &states, Some(&doors).filter(|d| !d.is_empty())),
                    key: leg.key.map(|b| Key::ALL[usize::from(b)]),
                }
            })
            .collect();
        Some(Route {
            legs,
            states: states_of(&route.states),
            keys: Keys(route.keys.0),
            goal: Some(route.goal),
        })
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
                        if let std::collections::btree_map::Entry::Vacant(e) =
                            producers.entry((s, h.key()))
                        {
                            e.insert(vec![t]);
                            assumed.insert((s, h.key()), assumes.clone());
                        }
                    }
                }
            }
        }
        let key_for = |lock: Lock| {
            key_items
                .iter()
                .map(|(k, _)| *k)
                .find(|k| lock.satisfied(Keys::default().with(*k)))
        };
        let spots_of = |k: Key| -> BTreeSet<usize> {
            key_items
                .iter()
                .filter(|(x, _)| *x == k)
                .flat_map(|(_, s)| s.iter().map(|(p, _)| *p))
                .collect()
        };
        let mut need_t: BTreeSet<i64> = BTreeSet::new();
        let mut need_k = Keys::default();
        let mut without: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut relied: BTreeSet<(usize, HeightsKey)> = BTreeSet::new();
        let mut queue: Vec<BTreeSet<usize>> = vec![goals.clone()];
        let mut seen_goals: BTreeSet<BTreeSet<usize>> = BTreeSet::new();
        while let Some(target) = queue.pop() {
            if seen_goals.len() >= RELAXED_GOAL_LIMIT {
                break;
            }
            if seen_goals.contains(&target) || target.is_disjoint(reach_all) {
                continue;
            }
            seen_goals.insert(target.clone());
            // breadth first with parents, in the saturated world
            let mut parent: BTreeMap<usize, (Option<usize>, Option<Step>)> =
                BTreeMap::from([(start, (None, None))]);
            let mut todo = std::collections::VecDeque::from([start]);
            let mut end = None;
            while let Some(p) = todo.pop_front() {
                if target.contains(&p) {
                    end = Some(p);
                    break;
                }
                let mut moves: Vec<(usize, Step)> = mesh
                    .poly(p)
                    .portals
                    .iter()
                    .filter(|&&pi| n.passable(pi, p, states_all, Some(&doors_all), self.spent()))
                    .map(|&pi| (mesh.portals[pi].other(p), Step::Portal(pi)))
                    .collect();
                moves.extend(
                    n.tele_from[p]
                        .iter()
                        .filter(|&&e| {
                            !matches!(e, TeleEdge::Jump(j) if self.banned_jumps.contains(&j))
                                && n.edge_ok(e, states_all)
                        })
                        .map(|&e| (n.edge_dest(e).0, Step::Tele(e))),
                );
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
                    Some(Step::Portal(pi))
                        if !n.passable(pi, prev, states0, None, self.spent()) =>
                    {
                        let (sa, sb) = (n.sector_of_poly(prev), n.sector_of_poly(p));
                        let pairs: Vec<(Heights, Heights)> = states_all[sa]
                            .iter()
                            .flat_map(|&ha| {
                                states_all[sb]
                                    .iter()
                                    .filter(move |&&hb| can_cross(ha, hb))
                                    .map(move |&hb| (ha, hb))
                            })
                            .collect();
                        // (fewest heights not there now; of those, the lowest floors: what's been
                        // brought down is the likelier way through)
                        let new = |ha: &Heights, hb: &Heights| {
                            usize::from(!states0[sa].contains(ha))
                                + usize::from(!states0[sb].contains(hb))
                        };
                        if let Some(&(ha, hb)) = pairs.iter().min_by(|(a1, b1), (a2, b2)| {
                            new(a1, b1)
                                .cmp(&new(a2, b2))
                                .then((a1.floor + b1.floor).total_cmp(&(a2.floor + b2.floor)))
                        }) {
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
                                    if let Some(k) =
                                        key_for(t.info.lock).filter(|k| !need_k.has(*k))
                                    {
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
                // A producer that can be got to without what it makes (a switch that opens a door
                // from inside isn't one); if there's none, and the sector is a locked door as well,
                // that door's key is what's really needed.
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
                // (not from where the press itself waits on that sector: a lift's switch pressed
                // from the floor below across the lift's edge only once it's down)
                let free = |t: &TriggerLine| -> BTreeSet<usize> {
                    let li = t.linedef().unwrap_or(usize::MAX);
                    t.activators
                        .iter()
                        .copied()
                        .filter(|&p| {
                            !n.use_gates.get(&(li, p)).is_some_and(|g| g.contains(&sh.0))
                                && !n.use_line_gates.get(&(li, p)).is_some_and(|g| {
                                    g.iter().any(|pr| pr.0 == sh.0 || pr.1 == sh.0)
                                })
                        })
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
                    // of the press, if anywhere (a switch behind a keyed door: the path there
                    // names the key)
                    let li = t.linedef().unwrap_or(usize::MAX);
                    let clear: BTreeSet<usize> = t
                        .activators
                        .iter()
                        .copied()
                        .filter(|&p| {
                            !n.use_gates.contains_key(&(li, p))
                                && !n.use_line_gates.contains_key(&(li, p))
                        })
                        .collect();
                    let clear_reach: BTreeSet<usize> =
                        clear.intersection(reach_all).copied().collect();
                    queue.push(if clear_reach.is_empty() {
                        t.activators.clone()
                    } else {
                        clear_reach.clone()
                    });
                    if clear_reach.is_empty() {
                        // pressed through a door: its key
                        let gates: BTreeSet<usize> = t
                            .activators
                            .iter()
                            .flat_map(|&p| n.use_gates.get(&(li, p)).into_iter().flatten().copied())
                            .collect();
                        for dt in &self.triggers {
                            if dt.sectors.iter().any(|s| gates.contains(s))
                                && dt.info.lock != Lock::None
                            {
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
        // too (platforms can be lowered from several places); only for heights a
        // height-dependent trigger relies on round it (elsewhere the extra lines only widen the
        // search).
        for &t in triggers {
            if t.effects
                .iter()
                .any(|(s, hs)| hs.iter().any(|h| relied.contains(&(*s, h.key()))))
            {
                need_t.insert(t.line);
            }
        }
        (need_t, need_k)
    }
}
