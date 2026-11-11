//! The plan search's triggers: what firing each does with the world as it is, from where it
//! can be fired, and how long what it does lasts.

use super::world::{Pair, Timers, World};
use super::{KeySet, Planner};
use crate::det::{HashMap, HashSet};
use crate::geom::{Point, MAX_STEP};
use crate::nav::{can_cross, DoorOpening, Doors, Found, Heights};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// boss deaths, monster-only lines...
    Other,
    /// W1/WR
    Walk,
    /// S1/SR
    Switch,
    /// D1: a door used once, staying open
    DoorOnce,
    /// DR: a door used whenever
    DoorRepeat,
    /// G1/GR
    Gun,
}

impl Kind {
    pub fn is_door(self) -> bool {
        matches!(self, Kind::DoorOnce | Kind::DoorRepeat)
    }

    /// Pressed with the use key?
    pub fn is_used(self) -> bool {
        matches!(self, Kind::Switch | Kind::DoorOnce | Kind::DoorRepeat)
    }
}

pub use crate::movers::Effects;

/// A height-dependent trigger's effects with the sectors it depends on at these heights:
/// `effects_from_reps(trigger, [(sector, heights)])`.
pub type EffectsFromReps = Box<dyn Fn(usize, &[(usize, Heights)]) -> Effects>;

pub use crate::movers::Timing;

pub struct Trigger {
    /// its linedef (None: boss deaths and the like)
    pub line: Option<usize>,
    pub kind: Kind,
    pub effects: Effects,
    /// the polys it can be fired from
    pub activators: HashSet<usize>,
    /// its moves stay put (not lifts, crushers, doors that close again)
    pub permanent: bool,
    /// can it be used with these keys, by key bitmask
    pub lock_ok: Vec<bool>,
    pub timing: HashMap<Pair, Timing>,
    /// moves that stay put: sector -> travel to its first target
    pub perm_travel: HashMap<usize, f64>,
    /// stairs: (start sector, its first floor); the effects shift as it rises
    pub shift_from: Option<(usize, f64)>,
    /// W1/S1/G1/D1
    pub once: bool,
    /// effects worked out from the heights when fired: the sectors they depend on
    pub relevant: Option<Vec<usize>>,
}

impl Trigger {
    /// Firing it again can do something new (so one-shot ones are tracked as used).
    pub fn cumulative(&self) -> bool {
        self.shift_from.is_some() || self.relevant.is_some()
    }

    pub fn usable_with(&self, keys: KeySet) -> bool {
        self.lock_ok.get(keys.index()).copied().unwrap_or(false)
    }
}

/// The plan search's per-map facts.
pub struct PlanFacts {
    pub triggers: Vec<Trigger>,
    /// (line, activator poly) -> sectors that must be open to press it from there
    pub use_gates: HashMap<(usize, usize), Vec<usize>>,
    /// (line, activator poly) -> (front, back) sectors the press passes between, which must
    /// have a gap between them (a raised platform's floor at the ceiling of an alcove beyond)
    pub use_line_gates: HashMap<(usize, usize), Vec<(usize, usize)>>,
    /// (gun line, activator poly) -> the floors it can be shot from there
    pub shot_floors: HashMap<(usize, usize), (f64, f64)>,
    /// (walk-over line, activator poly) -> the sector across the line
    pub cross_sides: HashMap<(usize, usize), usize>,
    /// heights that don't last (lifts, doors that close again)
    pub temporary: HashSet<Pair>,
    /// key bit -> where that key is (skill 3)
    pub key_positions: Vec<(u8, Vec<Point>)>,
    /// `effects_from_reps(trigger, [(sector, heights)])`: a height-dependent trigger's effects
    /// with the sectors it depends on at those heights
    pub effects_from_reps: EffectsFromReps,
}

/// What the plan search has worked out so far, kept between searches on the same map.
#[derive(Default)]
pub struct PlanCaches {
    doors: HashMap<KeySet, Arc<Doors>>,
    pub(super) reach: HashMap<(usize, KeySet, u64), Arc<HashSet<usize>>>,
    effects: HashMap<(usize, Vec<Pair>), Effects>,
    /// legs found (or not) between two points, by (from, to, heights hash, keys, timers hash,
    /// strict fit): the same from one search to the next for the same view (several items
    /// compared are each searched for through the same lifts and teleports)
    pub(super) timed: HashMap<TimedKey, Option<(Found, World)>>,
}

/// A leg asked for: from, to (bits), a hash of the heights, the keys, a hash of the timers,
/// whether the box must fit all the way.
pub(super) type TimedKey = (u64, u64, u64, u64, u64, KeySet, u64, bool);

impl PlanCaches {
    /// Flood fills cached before the cache is started afresh.
    pub(super) const MAX_REACH: usize = 200_000;
    /// Legs remembered before the cache is started afresh.
    pub(super) const MAX_TIMED: usize = 50_000;
}

impl Planner<'_> {
    /// The use-doors that can be opened with these keys.
    pub(super) fn doors_for(&mut self, keys: KeySet) -> Arc<Doors> {
        if let Some(d) = self.caches.doors.get(&keys) {
            return Arc::clone(d);
        }
        let mut out: Doors = HashMap::default();
        for t in self
            .facts
            .triggers
            .iter()
            .filter(|t| t.kind.is_door() && t.usable_with(keys))
        {
            for (s, hs) in &t.effects {
                let door = out.entry(*s).or_insert_with(|| DoorOpening {
                    heights: Vec::new(),
                    users: HashSet::default(),
                });
                for h in hs {
                    if !door.heights.iter().any(|x| x.key() == h.key()) {
                        door.heights.push(*h);
                    }
                }
                door.users.extend(t.activators.iter().copied());
            }
        }
        let out = Arc::new(out);
        self.caches.doors.insert(keys, Arc::clone(&out));
        out
    }

    /// The polys trigger `ti` can be fired from now: those whose use gates are open, that
    /// are at a height a gun line can be shot from, and (walk-over lines) from which the
    /// far side can be walked onto.
    pub(super) fn activators_now(
        &self,
        ti: usize,
        states: &World,
        doors: Option<&Doors>,
    ) -> HashSet<usize> {
        let t = &self.facts.triggers[ti];
        let Some(line) = t.line else {
            return t.activators.clone();
        };
        let openable = |s: usize| doors.is_some_and(|d| d.contains_key(&s));
        t.activators
            .iter()
            .copied()
            .filter(|&p| {
                if let Some(gates) = self.facts.use_gates.get(&(line, p)) {
                    if !gates.iter().all(|&s| {
                        states[s].iter().any(|h| h.ceiling - h.floor > 0.0) || openable(s)
                    }) {
                        return false;
                    }
                }
                if let Some(pairs) = self.facts.use_line_gates.get(&(line, p)) {
                    let gap = |f: usize, b: usize| {
                        states[f].iter().any(|a| {
                            states[b]
                                .iter()
                                .any(|c| a.ceiling.min(c.ceiling) - a.floor.max(c.floor) > 0.0)
                        })
                    };
                    if !pairs.iter().all(|&(f, b)| gap(f, b)) {
                        return false;
                    }
                }
                let here = &states[self.nav.poly_sector[p]];
                if let Some(&(lo, hi)) = self.facts.shot_floors.get(&(line, p)) {
                    if !here.iter().any(|h| lo <= h.floor && h.floor <= hi) {
                        return false;
                    }
                }
                if let Some(&far) = self.facts.cross_sides.get(&(line, p)) {
                    if !openable(far)
                        && !here
                            .iter()
                            .any(|&a| states[far].iter().any(|&b| can_cross(a, b)))
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// What firing trigger `ti` does with the world at these heights.
    pub(super) fn effects_now(&mut self, ti: usize, states: &World) -> Effects {
        let facts = self.facts;
        let t = &facts.triggers[ti];
        if let Some(relevant) = &t.relevant {
            // one height per sector it depends on: the only one, the starting one if still
            // possible, else the highest; as at the start: the plain effects
            let mut reps: Vec<(usize, Heights)> = Vec::with_capacity(relevant.len());
            for &s in relevant {
                let hs = &states[s];
                let init = self.nav.initial[s];
                let rep = match hs.as_slice() {
                    [only] => *only,
                    _ if hs
                        .iter()
                        .any(|h| h.floor == init.floor && h.ceiling == init.ceiling) =>
                    {
                        init
                    }
                    _ => hs
                        .iter()
                        .copied()
                        .max_by(|a, b| {
                            a.floor
                                .total_cmp(&b.floor)
                                .then(a.ceiling.total_cmp(&b.ceiling))
                        })
                        .unwrap_or(init),
                };
                reps.push((s, rep));
            }
            if reps.iter().all(|&(s, rep)| rep == self.nav.initial[s]) {
                return t.effects.clone();
            }
            let key = (
                ti,
                reps.iter().map(|&(s, h)| (s, h.key())).collect::<Vec<_>>(),
            );
            if let Some(v) = self.caches.effects.get(&key) {
                return v.clone();
            }
            let effects = (facts.effects_from_reps)(ti, &reps);
            self.caches.effects.insert(key, effects.clone());
            return effects;
        }
        let Some((s0, f0)) = t.shift_from else {
            return t.effects.clone();
        };
        // stairs: as far up as the start has risen
        let shift = states[s0]
            .iter()
            .map(|h| h.floor)
            .fold(f64::NEG_INFINITY, f64::max)
            - f0;
        if shift == 0.0 {
            return t.effects.clone();
        }
        t.effects
            .iter()
            .map(|(s, hs)| {
                (
                    *s,
                    hs.iter()
                        .map(|h| Heights {
                            floor: h.floor + shift,
                            ceiling: h.ceiling,
                        })
                        .collect(),
                )
            })
            .collect()
    }

    /// How long the heights trigger `ti` makes (`effects`, fired with the world at `base`,
    /// making it `trial`) last: for moves that stay put the old heights until it's moved off
    /// them (those returned too), else the temporary ones until they go back.
    pub(super) fn deadlines_of(
        &self,
        ti: usize,
        effects: &Effects,
        base: &World,
        trial: &World,
    ) -> (Timers, Vec<Pair>) {
        let t = &self.facts.triggers[ti];
        let mut out = Timers::new();
        let mut moving_off = Vec::new();
        for (s, hs) in effects {
            if t.permanent {
                let travel = if t.line.is_some() {
                    t.perm_travel.get(s).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
                let Some(target) = hs.first() else { continue };
                let mut olds: Vec<Heights> = trial[*s]
                    .iter()
                    .copied()
                    .filter(|h| !hs.iter().any(|x| x.key() == h.key()))
                    .collect();
                olds.sort_by(|a, b| {
                    a.floor
                        .total_cmp(&b.floor)
                        .then(a.ceiling.total_cmp(&b.ceiling))
                });
                let rise = target.floor;
                for h in olds {
                    moving_off.push((*s, h.key()));
                    // a rising floor: only until it's a step above
                    let last = if rise <= h.floor {
                        travel
                    } else {
                        travel.min((travel - 1.0) * MAX_STEP / (rise - h.floor) + 1.0)
                    };
                    out.insert((*s, h.key()), last);
                }
                continue;
            }
            for h in hs {
                let pair = (*s, h.key());
                let timing = if t.line.is_some() {
                    t.timing.get(&pair).copied()
                } else {
                    None
                };
                // also a height another line makes for good, if not there yet: this one takes
                // it away again
                let held = timing.is_some_and(|tm| tm.hold.is_some())
                    && !base[*s].iter().any(|x| x.key() == h.key());
                if self.facts.temporary.contains(&pair) || held {
                    let tm = timing.unwrap_or(Timing {
                        travel: 0.0,
                        hold: None,
                    });
                    let lasts = if t.line.is_some() {
                        tm.travel + tm.hold.unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    out.insert(pair, lasts);
                }
            }
        }
        (out, moving_off)
    }
}
