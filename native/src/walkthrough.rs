// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A level planned as steps the bot can carry out.

use std::collections::BTreeSet;

use crate::geom::Point;
use crate::movers::{Effects, TICRATE};
use crate::nav::Heights;
use crate::navigator::{
    add_height, KeyItems, Leg, Path, Plan, States, TriggerLine, View, EXIT_FLOOR,
};
use crate::specials::{Category, Key, Keys, Trigger};

/// What a trigger step should make happen, for checking as it runs: `sector` reaches `heights`
/// about `travel` tics after the action, then stays `hold` tics (None: for good). `needed`: the
/// next step's path relies on it (goes over or through the sector).
#[derive(Clone, Debug)]
pub struct Expect {
    pub sector: usize,
    pub heights: Heights,
    pub travel: f64,
    pub hold: Option<f64>,
    pub needed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Use,
    Shoot,
    Cross,
    Pickup,
    Exit,
    Kill,
    /// stand on an exit floor till it ends the level
    Endure,
    /// stop facing the trigger's line without pressing it
    Face,
    /// just get there
    Walk,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::Use => "use",
            Action::Shoot => "shoot",
            Action::Cross => "cross",
            Action::Pickup => "pickup",
            Action::Exit => "exit",
            Action::Kill => "kill",
            Action::Endure => "endure",
            Action::Face => "face",
            Action::Walk => "walk",
        }
    }
}

/// Walk `path`, then do `action`; `expects`: what the action should then do to the world.
#[derive(Clone, Debug)]
pub struct Step {
    pub path: Option<Path>,
    pub action: Action,
    pub trigger: Option<TriggerLine>,
    pub key: Option<Key>,
    pub label: String,
    pub expects: Vec<Expect>,
    /// a barrel to clear on the way: shot first, from somewhere out of every blast of its chain
    pub barrel: Option<Point>,
}

/// Heights as the logs show them (whole numbers without a point, as Python's ints).
pub fn fmt_heights(h: Heights) -> String {
    format!("({}, {})", fmt_num(h.floor), fmt_num(h.ceiling))
}

/// A number as Python prints it: an int if it's whole, else a float.
pub fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

impl Step {
    /// The step spelled out, for the log: walk, act, then what to wait for (moves that stay put
    /// the next path needs) or beat (ones that go back).
    pub fn substeps(&self) -> Vec<String> {
        let goal = self
            .path
            .as_ref()
            .and_then(|p| p.waypoints.iter().rev().find_map(|w| *w));
        let notes = match &self.path {
            Some(p) if !p.notes.is_empty() => {
                let notes: Vec<String> = p.notes.iter().map(ToString::to_string).collect();
                format!(" ({})", notes.join("; "))
            }
            _ => String::new(),
        };
        let mut out = vec![match goal {
            Some(g) => format!("walk to ({:.0}, {:.0}){notes}", g.0, g.1),
            None => "walk".to_string(),
        }];
        if self.action == Action::Pickup {
            out.push(format!("pick up {}", self.key.map_or("?", Key::name)));
        } else if let Some(li) = self.trigger.as_ref().and_then(TriggerLine::linedef) {
            let verb = match self.action {
                Action::Use => "press",
                Action::Exit => "press exit",
                other => other.name(),
            };
            out.push(format!("{verb} line {li}"));
        } else if self.action == Action::Kill {
            out.push(self.label.clone());
        }
        for e in self.expects.iter().filter(|e| e.needed) {
            match e.hold {
                None => out.push(format!(
                    "wait for sector {} to reach {} (~{:.1}s)",
                    e.sector,
                    fmt_heights(e.heights),
                    e.travel / TICRATE
                )),
                Some(hold) => out.push(format!(
                    "get to sector {} within {:.1}s, before it moves back",
                    e.sector,
                    (e.travel + hold) / TICRATE
                )),
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
pub struct LevelPlan {
    pub steps: Vec<Step>,
    pub complete: bool,
    pub problem: String,
}

pub fn action_for(t: &TriggerLine) -> Action {
    if t.line <= EXIT_FLOOR {
        return Action::Endure;
    }
    if t.line < 0 {
        return Action::Kill;
    }
    match t.info.trigger {
        Trigger::S1 | Trigger::SR | Trigger::D1 | Trigger::DR => Action::Use,
        Trigger::G1 | Trigger::GR => Action::Shoot,
        _ => Action::Cross,
    }
}

/// What firing `t` should do (`effects`: as fired, by default its plain effects), with
/// timings; `needed` where the path `after` depends on it, or for sectors in `also`.
pub fn expects_of(
    view: &View,
    t: &TriggerLine,
    after: Option<&Path>,
    effects: Option<&Effects>,
    also: &BTreeSet<usize>,
) -> Vec<Expect> {
    let nav = &view.nav;
    let mut on_path: BTreeSet<usize> = after
        .map(|p| p.polys.iter().map(|&q| nav.sector_of_poly(q)).collect())
        .unwrap_or_default();
    on_path.extend(also.iter().copied());
    if t.info.description.contains("perpetual") {
        return Vec::new(); // (they keep going: nothing to wait for or beat)
    }
    let mut out = Vec::new();
    for (s, hs) in effects.unwrap_or(&t.effects) {
        let h = hs[0];
        if h == nav.movers.initial[*s] && hs.len() == 1 {
            continue; // (already like that)
        }
        let timing = (t.line >= 0).then(|| {
            nav.movers
                .timing(t.info.special, &t.info.description, *s, h)
        });
        out.push(Expect {
            sector: *s,
            heights: h,
            travel: timing.map_or(0.0, |tm| tm.travel),
            hold: timing.and_then(|tm| tm.hold),
            needed: on_path.contains(s),
        });
    }
    out
}

/// Steps for a route's legs. The last (goal) leg becomes `goal_step` with that leg's path, or
/// is dropped without one. Trigger steps get `expects` (from `states`, the heights at the
/// start: stairs build on from where they are).
pub fn steps_from_legs(
    view: &View,
    legs: &[Leg],
    goal_step: Option<Step>,
    states: Option<&States>,
) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut running: States = states.cloned().unwrap_or_else(|| view.initial_states());
    for (i, leg) in legs.iter().enumerate().take(legs.len().saturating_sub(1)) {
        if let Some(id) = leg.trigger {
            let t = view.trigger(id).clone();
            let effects = view.nav.effects_now(&t, &running);
            // another switch for the same stairs next: wait for these to stop first (a press
            // while they're still moving does nothing)
            let next = legs[i + 1].trigger.map(|n| view.trigger(n));
            let also: BTreeSet<usize> =
                if next.is_some_and(|n| t.shift_from.is_some() && n.shift_from == t.shift_from) {
                    effects.iter().map(|(s, _)| *s).collect()
                } else {
                    BTreeSet::new()
                };
            let expects = expects_of(view, &t, Some(&legs[i + 1].path), Some(&effects), &also);
            running = view.nav.apply_trigger(&running, &t);
            steps.push(Step {
                path: Some(leg.path.clone()),
                action: action_for(&t),
                label: format!("line {}: {}", t.line, t.info.description),
                trigger: Some(t),
                key: None,
                expects,
                barrel: None,
            });
        } else if let Some(k) = leg.key {
            steps.push(Step {
                path: Some(leg.path.clone()),
                action: Action::Pickup,
                trigger: None,
                key: Some(k),
                label: format!("pick up {}", k.name()),
                expects: Vec::new(),
                barrel: None,
            });
        }
    }
    if let (Some(mut g), Some(last)) = (goal_step, legs.last()) {
        g.path = Some(last.path.clone());
        steps.push(g);
    }
    steps
}

/// Where to stand to use each exit line that can be set off from `reach`: (spot, trigger id).
pub fn exit_goals(view: &View, pos: Point, reach: &BTreeSet<usize>) -> Vec<(Point, usize)> {
    view.triggers
        .iter()
        .filter(|t| t.info.category == Category::Exit && !t.activators.is_disjoint(reach))
        .map(|t| (view.nav.activation_point(t, pos, reach), t.id))
        .collect()
}

fn exit_step(t: &TriggerLine) -> Step {
    let action = match action_for(t) {
        Action::Use => Action::Exit,
        a => a,
    };
    Step {
        path: None,
        action,
        trigger: Some(t.clone()),
        key: None,
        label: format!("exit via line {}", t.line),
        expects: Vec::new(),
        barrel: None,
    }
}

/// From the map's starting heights, finish the level: one search whose goal is any exit,
/// using switches and picking up keys in whatever order works.
pub fn plan_level(view: &View, start: Point, max_seconds: f64, max_expansions: usize) -> LevelPlan {
    let Some(start_poly) = view.nav.poly_at(start) else {
        return LevelPlan {
            problem: "start off the map".into(),
            ..LevelPlan::default()
        };
    };
    let stages = view.progression(start_poly);
    if !stages.iter().any(|s| {
        s.exits
            .iter()
            .any(|&id| view.trigger(id).info.category == Category::Exit)
    }) {
        return LevelPlan {
            problem: "no route to the exit (not reachable even in principle)".into(),
            ..LevelPlan::default()
        };
    }
    let reach = stages.last().map(|s| s.reached.clone());
    plan_from(
        view,
        start,
        &Rest {
            keys: Keys::default(),
            states: None,
            reach: reach.as_ref(),
            max_seconds,
            max_expansions,
            fired: &BTreeSet::new(),
        },
    )
}

/// What `plan_from` plans with: the keys held, the heights the world can take now (default:
/// the map's), which polys' exit spots count (default: all), how hard to look, and the
/// once-only stair switches already used.
pub struct Rest<'a> {
    pub keys: Keys,
    pub states: Option<&'a States>,
    pub reach: Option<&'a BTreeSet<usize>>,
    pub max_seconds: f64,
    pub max_expansions: usize,
    pub fired: &'a BTreeSet<usize>,
}

/// Plan the rest of the level from `pos`.
pub fn plan_from(view: &View, pos: Point, rest: &Rest<'_>) -> LevelPlan {
    let Rest {
        keys,
        states,
        reach,
        max_seconds,
        max_expansions,
        fired,
    } = *rest;
    let all: BTreeSet<usize>;
    let reach = match reach {
        Some(r) => r,
        None => {
            all = view.nav.mesh.live_polys().map(|(i, _)| i).collect();
            &all
        }
    };
    let exits = exit_goals(view, pos, reach);
    if exits.is_empty() {
        return LevelPlan {
            problem: "no exit lines".into(),
            ..LevelPlan::default()
        };
    }
    let goals: Vec<Point> = exits.iter().map(|(p, _)| *p).collect();
    let key_items: KeyItems = view.nav.key_items(None);
    let t0 = std::time::Instant::now();
    let route = view.plan(
        pos,
        &goals,
        &Plan {
            keys,
            states,
            key_items: &key_items,
            max_expansions,
            max_seconds,
            goal_triggers: &exits,
            fired,
        },
    );
    let Some(route) = route else {
        let timed_out = t0.elapsed().as_secs_f64() >= max_seconds;
        return LevelPlan {
            problem: format!(
                "no route to the exit{}",
                if timed_out {
                    format!(" (search gave up after {max_seconds:.0}s)")
                } else {
                    String::new()
                }
            ),
            ..LevelPlan::default()
        };
    };
    let goal = route
        .goal
        .and_then(|g| exits.iter().find(|(p, _)| *p == g))
        .map(|(_, id)| exit_step(view.trigger(*id)));
    LevelPlan {
        steps: steps_from_legs(view, &route.legs, goal, states),
        complete: true,
        problem: String::new(),
    }
}

/// One stage of `progression`: the keys held, where she gets to, the triggers fired then, the
/// keys there for the taking, and the exits fired by then.
pub struct Stage {
    pub keys: Keys,
    pub reached: BTreeSet<usize>,
    pub fired: Vec<usize>,
    pub keys_available: Keys,
    pub exits: Vec<usize>,
}

impl View {
    /// The trigger with this id, as this view has it.
    pub fn trigger(&self, id: usize) -> &TriggerLine {
        self.triggers
            .iter()
            .find(|t| t.id == id)
            .unwrap_or(&self.nav.triggers[id])
    }

    /// Alternate: fire every usable trigger till nothing changes, then pick up every key there
    /// is to get to; each key set is a stage.
    pub fn progression(&self, start: usize) -> Vec<Stage> {
        let nav = &self.nav;
        let mut states = self.initial_states();
        let mut keys = Keys::default();
        let mut fired: BTreeSet<i64> = BTreeSet::new();
        let key_things: Vec<(Key, Point)> = nav
            .map
            .things
            .iter()
            .filter(|t| t.in_skill(nav.skill))
            .filter_map(|t| Key::from_thing(t.kind).map(|k| (k, (t.x, t.y))))
            .collect();
        let mut stages = Vec::new();
        loop {
            let mut fired_now = Vec::new();
            let reached = loop {
                let reached = self.reachable(start, &states, None);
                let usable: Vec<&TriggerLine> = self
                    .triggers
                    .iter()
                    .filter(|t| {
                        !fired.contains(&t.line)
                            && !t.activators.is_disjoint(&reached)
                            && t.info.lock.satisfied(keys)
                    })
                    .collect();
                if usable.is_empty() {
                    break reached;
                }
                for t in usable {
                    fired.insert(t.line);
                    fired_now.push(t.id);
                    for (s, hs) in nav.effects_now(t, &states) {
                        for h in hs {
                            add_height(&mut states[s], h);
                        }
                    }
                }
            };
            let available: Keys = key_things
                .iter()
                .filter(|(_, (x, y))| {
                    self.pickup_spots(*x, *y, &states)
                        .iter()
                        .any(|(p, _)| reached.contains(p))
                })
                .map(|(k, _)| *k)
                .collect();
            let exits = self
                .triggers
                .iter()
                .filter(|t| t.info.category.is_exit() && fired.contains(&t.line))
                .map(|t| t.id)
                .collect();
            let done = keys.covers(available);
            stages.push(Stage {
                keys,
                reached,
                fired: fired_now,
                keys_available: available,
                exits,
            });
            if done {
                break;
            }
            keys = Keys(keys.0 | available.0);
        }
        stages
    }
}
