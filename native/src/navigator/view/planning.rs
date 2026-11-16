// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Preparing planner facts and queries from a restricted view.

use super::{key_bit, states_of, world, Leg, LowVariant, Plan, Route, Step, View};
use crate::det::HashMap;
use crate::geom::{point_key, Point};
use crate::nav::{can_cross, Heights, HeightsKey};
use crate::navigator::{Effects, States, TeleEdge, TriggerLine};
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
                stand_gates: n.stand_gates.clone(),
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
        // (and every line that does just what one of those does: a lift's called from its top
        // by one line and from its foot by another, and the relaxed plan names the one)
        let same: Vec<&Effects> = actions
            .iter()
            .filter(|t| rel_t.contains(&t.line))
            .map(|t| &t.effects)
            .collect();
        let rel_same: BTreeSet<i64> = actions
            .iter()
            .filter(|t| rel_t.contains(&t.line) || same.contains(&&t.effects))
            .map(|t| t.line)
            .collect();
        if crate::debug::on().relaxed {
            self.relaxed_gaps(
                start_poly,
                &states0,
                q,
                &actions,
                &rel_t,
                (&reach_all, &states_all, keys_all),
            );
        }
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
            useful_lines: Some(rel_same.iter().map(|&l| usize::try_from(l).ok()).collect()),
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
        if none && crate::debug::on().relaxed {
            let furthest = self.with_core(|_, caches| caches.furthest.take());
            if let Some((reach, w, lines)) = furthest {
                let reach: BTreeSet<usize> = reach.into_iter().collect();
                eprintln!(
                    "relaxed: the search's furthest: {} polys in reach, lines set off {:?}",
                    reach.len(),
                    lines
                );
                self.relaxed_stops(
                    start_poly,
                    &actions,
                    &rel_t,
                    (&reach, &states_of(&w)),
                    (&reach_all, &states_all, keys_all),
                    &lines,
                );
            }
        }
        // (a way with the few lines and the box fitting all along it: that's the route. The
        // search from them all after it found the same way or one no better by anything it
        // measures, in as many seconds as the level has switches)
        let clean =
            narrow.is_some() && crate::stats::last_ended() == Some(crate::stats::Ended::Found);
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
        let found = if clean {
            None
        } else {
            self.with_core(|nav, caches| {
                Planner::new(nav, facts, caches).plan(&wider, world(&states0))
            })
        };
        if crate::stats::enabled() {
            eprintln!("{}", crate::stats::report(t0.elapsed().as_secs_f64()));
        }
        // (none from them all in its budget, and one with the few lines: that one)
        let route = found.or(narrow)?;
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

    /// For EVIL_DEBUG_RELAXED: with only the relaxed plan's lines set off (all that can be,
    /// in any order, nothing lost), which of them are never got to, and for each the first
    /// crossing on the way there, as the level is with every line set off, that isn't made.
    fn relaxed_gaps(
        &self,
        start: usize,
        states0: &States,
        q: &Plan<'_>,
        actions: &[&TriggerLine],
        rel_t: &BTreeSet<i64>,
        (reach_all, states_all, keys_all): (&BTreeSet<usize>, &States, Keys),
    ) {
        let rel: Vec<&TriggerLine> = actions
            .iter()
            .copied()
            .filter(|t| rel_t.contains(&t.line))
            .collect();
        let (reach_r, states_r, keys_r) = self.saturate(start, states0, q.keys, q.key_items, &rel);
        eprintln!(
            "relaxed: with its lines alone: {} polys of {} in reach, keys {:?} of {:?}",
            reach_r.len(),
            reach_all.len(),
            keys_r,
            keys_all
        );
        self.relaxed_stops(
            start,
            actions,
            rel_t,
            (&reach_r, &states_r),
            (reach_all, states_all, keys_all),
            &[],
        );
    }

    /// For EVIL_DEBUG_RELAXED: of the relaxed plan's lines not in `done`, those not to be set
    /// off from `reach_r`, and for each the first crossing on the way there (as the level is
    /// with every line set off) that goes out of `reach_r`.
    fn relaxed_stops(
        &self,
        start: usize,
        actions: &[&TriggerLine],
        rel_t: &BTreeSet<i64>,
        (reach_r, states_r): (&BTreeSet<usize>, &States),
        (reach_all, states_all, keys_all): (&BTreeSet<usize>, &States, Keys),
        done: &[usize],
    ) {
        let n = &self.nav;
        let mesh = &n.mesh;
        let rel: Vec<&TriggerLine> = actions
            .iter()
            .copied()
            .filter(|t| rel_t.contains(&t.line))
            .collect();
        let doors_all = self.local_doors(keys_all);
        // (what's wanted: each line, by its polys and what it makes; each key, by where it's
        // picked up from)
        let mut wanted: Vec<(String, BTreeSet<usize>, Effects)> = Vec::new();
        for t in &rel {
            if t.linedef().is_some_and(|l| done.contains(&l)) {
                continue;
            }
            if !t.activators.is_disjoint(reach_r) || t.activators.is_disjoint(reach_all) {
                if !t.activators.is_disjoint(reach_r) && !done.is_empty() {
                    eprintln!(
                        "relaxed: line {} ({}): in reach there and not set off (usable from {:?} of its polys there)",
                        t.line,
                        t.info.description,
                        self.activators_now(t, states_r, None).intersection(reach_r).count()
                    );
                }
                continue;
            }
            wanted.push((
                format!("line {} ({})", t.line, t.info.description),
                t.activators.clone(),
                t.effects.clone(),
            ));
        }
        for (k, spots) in n.key_items(None) {
            let polys: BTreeSet<usize> = spots.iter().map(|(p, _)| *p).collect();
            if polys.is_disjoint(reach_r) && !polys.is_disjoint(reach_all) {
                wanted.push((format!("key {}", k.name()), polys, Vec::new()));
            }
        }
        for (what, target, effects) in &wanted {
            // (the way there before it has done what it does)
            let mut before = states_all.clone();
            for (es, hs) in effects {
                if before[*es].len() > 1 {
                    before[*es].retain(|x| !hs.contains(x));
                }
            }
            let states_all = &before;
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
            let Some(end) = end else {
                eprintln!("relaxed: {what}: no way to it at all");
                continue;
            };
            let mut chain = vec![end];
            let mut p = end;
            while let (Some(prev), _) = parent[&p] {
                chain.push(prev);
                p = prev;
            }
            chain.reverse();
            let Some(i) = chain.iter().position(|p| !reach_r.contains(p)) else {
                continue;
            };
            let (from, to) = (chain[i.saturating_sub(1)], chain[i]);
            let (sa, sb) = (n.sector_of_poly(from), n.sector_of_poly(to));
            let via = match parent[&to].1 {
                Some(Step::Portal(pi)) => {
                    format!("portal {pi} (line {:?})", mesh.portals[pi].linedef)
                }
                Some(Step::Tele(TeleEdge::Jump(j))) => {
                    format!("a jump needing {:?}", n.jumps[j].requires)
                }
                Some(Step::Tele(_)) => "a teleport".to_string(),
                None => String::new(),
            };
            let c = mesh.poly(to).center;
            eprintln!(
                "relaxed: {what}: stops going from poly {from} (sector {sa}, heights {:?}) to poly {to} at ({:.0}, {:.0}) (sector {sb}: {:?}, in the end {:?}) by {via}",
                states_r[sa].iter().map(|h| (h.floor, h.ceiling)).collect::<Vec<_>>(),
                c.0,
                c.1,
                states_r[sb].iter().map(|h| (h.floor, h.ceiling)).collect::<Vec<_>>(),
                states_all[sb].iter().map(|h| (h.floor, h.ceiling)).collect::<Vec<_>>(),
            );
        }
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
        // (a trigger's polys, wanted for a height it makes: the way to them is looked for in the
        // level without that height. The shortest way to a switch can run through the very
        // wall it lowers, and what's needed is the way there before it has)
        let mut before: BTreeMap<BTreeSet<usize>, (usize, Heights)> = BTreeMap::new();
        let mut seen_goals: BTreeSet<BTreeSet<usize>> = BTreeSet::new();
        while let Some(target) = queue.pop() {
            if seen_goals.len() >= RELAXED_GOAL_LIMIT {
                break;
            }
            if seen_goals.contains(&target) || target.is_disjoint(reach_all) {
                continue;
            }
            seen_goals.insert(target.clone());
            let earlier: Option<(States, crate::navigator::Doors)> = before
                .get(&target)
                .filter(|(s, _)| states_all[*s].len() > 1)
                .map(|&(s, h)| {
                    let mut st = states_all.clone();
                    st[s].retain(|x| *x != h);
                    let mut d = doors_all.clone();
                    d.remove(&s);
                    (st, d)
                });
            // breadth first with parents, in the saturated world (without the height the
            // target's wanted for, if there's a way then)
            let mut found = None;
            for world in [
                earlier.as_ref().map(|(st, d)| (st, d)),
                Some((states_all, &doors_all)),
            ]
            .into_iter()
            .flatten()
            {
                let (st, doors_t) = world;
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
                        .filter(|&&pi| n.passable(pi, p, st, Some(doors_t), self.spent()))
                        .map(|&pi| (mesh.portals[pi].other(p), Step::Portal(pi)))
                        .collect();
                    moves.extend(
                        n.tele_from[p]
                            .iter()
                            .filter(|&&e| {
                                !matches!(e, TeleEdge::Jump(j) if self.banned_jumps.contains(&j))
                                    && n.edge_ok(e, st)
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
                if let Some(end) = end {
                    found = Some((end, parent, st));
                    break;
                }
            }
            let Some((end, parent, st)) = found else {
                continue;
            };
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
                        // (the lines of moving sectors in the box's way at the portal now and
                        // not in the end, a fence's posts: what brings them down is needed)
                        let now = n.gate_blockers(pi, prev, states0, None);
                        let then = n.gate_blockers(pi, prev, states_all, Some(&doors_all));
                        for li in now.into_iter().filter(|l| !then.contains(l)) {
                            let (f, b) = n.map.sides(li);
                            for s in [f, b].into_iter().flatten() {
                                let moved = states_all[s]
                                    .iter()
                                    .filter(|h| !states0[s].contains(h))
                                    .min_by(|x, y| x.floor.total_cmp(&y.floor));
                                if let Some(&h) = moved {
                                    needed.insert((s, h.key()));
                                    needed_h.insert((s, h.key()), h);
                                }
                                // (bars she opens herself, with a key: the key)
                                if doors_all.contains_key(&s) {
                                    for t in &self.triggers {
                                        if t.sectors.contains(&s) && t.info.lock != Lock::None {
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
                        }
                        let (sa, sb) = (n.sector_of_poly(prev), n.sector_of_poly(p));
                        let pairs: Vec<(Heights, Heights)> = st[sa]
                            .iter()
                            .flat_map(|&ha| {
                                st[sb]
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
            if crate::debug::on().relaxed {
                eprintln!(
                    "relaxed: to polys {:?}... needs {:?}",
                    target.iter().take(3).collect::<Vec<_>>(),
                    needed
                        .iter()
                        .map(|sh| (
                            sh.0,
                            needed_h[sh].floor,
                            needed_h[sh].ceiling,
                            producers
                                .get(sh)
                                .map(|v| v.iter().map(|t| t.line).collect::<Vec<_>>())
                        ))
                        .collect::<Vec<_>>()
                );
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
                                && !n.stand_gates.get(&(li, p)).is_some_and(|places| {
                                    places
                                        .iter()
                                        .flatten()
                                        .any(|pr| pr.0 == sh.0 || pr.1 == sh.0)
                                })
                        })
                        .collect()
                };
                let first = cands.iter().copied().find(|t| !free(t).is_disjoint(w));
                if crate::debug::on().relaxed && first.is_none() {
                    eprintln!(
                        "relaxed: sector {} at {:?}: no maker got to without it (makers {:?})",
                        sh.0,
                        needed_h.get(&sh).map(|h| (h.floor, h.ceiling)),
                        cands.iter().map(|t| t.line).collect::<Vec<_>>()
                    );
                }
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
                                && !n.stand_gates.contains_key(&(li, p))
                        })
                        .collect();
                    let clear_reach: BTreeSet<usize> =
                        clear.intersection(reach_all).copied().collect();
                    let wanted = if clear_reach.is_empty() {
                        t.activators.clone()
                    } else {
                        clear_reach.clone()
                    };
                    before
                        .entry(wanted.clone())
                        .or_insert((sh.0, needed_h[&sh]));
                    queue.push(wanted);
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
                    // walked over, and from nowhere can she walk onto its far side as the level
                    // starts (a line round a pedestal with no headroom till it's lowered): what
                    // makes the room there
                    let sides: Vec<(usize, usize)> = t
                        .activators
                        .intersection(reach_all)
                        .filter_map(|&p| {
                            self.cross_sides()
                                .get(&(li, p))
                                .map(|&far| (n.sector_of_poly(p), far))
                        })
                        .collect();
                    let walks = |st: &States, here: usize, far: usize| {
                        st[here]
                            .iter()
                            .any(|&a| st[far].iter().any(|&b| can_cross(a, b)))
                    };
                    if !sides.is_empty() && !sides.iter().any(|&(h, f)| walks(states0, h, f)) {
                        let made = sides.iter().find_map(|&(here, far)| {
                            states_all[far]
                                .iter()
                                .filter(|b| !states0[far].contains(b))
                                .find(|&&b| states_all[here].iter().any(|&a| can_cross(a, b)))
                                .map(|&b| (far, b))
                        });
                        if let Some((far, h)) = made {
                            relied.insert((far, h.key()));
                            if let Some(t2) = producers.get(&(far, h.key())).and_then(|v| v.first())
                            {
                                if need_t.insert(t2.line) {
                                    queue.push(t2.activators.clone());
                                }
                            }
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
