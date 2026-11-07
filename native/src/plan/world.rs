//! The plan search's worlds: per sector, the heights it can be at, shared between nodes
//! (only the sectors a step changes are copied).

use super::triggers::Effects;
use crate::det::HashSet;
use crate::geom::{dist, Point};
use crate::nav::{Heights, HeightsKey};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

pub type World = Rc<Vec<Rc<Vec<Heights>>>>;
/// A sector at some heights.
pub type Pair = (usize, HeightsKey);
/// Temporary heights (lifts down, doors open for a while) -> tics left of them.
pub type Timers = BTreeMap<Pair, f64>;

/// Assumed walking speed, units a tic.
pub const LEG_SPEED: f64 = 12.0;
/// Time allowed to activate a trigger before walking the next leg.
pub(super) const ACTIVATION_TICS: f64 = 10.0;

/// A hash of every sector's heights (what a flood fill depends on, with the keys).
pub fn world_hash(states: &World) -> u64 {
    let mut h = DefaultHasher::new();
    for hs in states.iter() {
        hs.len().hash(&mut h);
        for x in hs.iter() {
            x.floor.to_bits().hash(&mut h);
            x.ceiling.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// The world with these heights gone (in order; never a sector's last one).
pub fn without(states: &World, pairs: impl IntoIterator<Item = Pair>) -> World {
    let mut sorted: Vec<Pair> = pairs.into_iter().collect();
    sorted.sort_unstable();
    let mut out: Vec<Rc<Vec<Heights>>> = (**states).clone();
    for (s, h) in sorted {
        if out[s].len() > 1 && out[s].iter().any(|x| x.key() == h) {
            out[s] = Rc::new(out[s].iter().copied().filter(|x| x.key() != h).collect());
        }
    }
    Rc::new(out)
}

fn push_new(hs: &mut Vec<Heights>, new: &[Heights]) {
    for h in new {
        if !hs.iter().any(|x| x.key() == h.key()) {
            hs.push(*h);
        }
    }
}

/// The world after a trigger with these effects: the heights added, or for one whose moves
/// stay put and that only takes a sector down, instead of the old ones.
pub fn apply_effects(states: &World, permanent: bool, effects: &Effects) -> World {
    let mut out: Vec<Rc<Vec<Heights>>> = (**states).clone();
    for (s, hs) in effects {
        let cur_min = out[*s].iter().map(|h| h.floor).fold(f64::INFINITY, f64::min);
        let hs_max = hs.iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
        let mut new = if permanent && hs_max < cur_min { Vec::new() } else { (*out[*s]).clone() };
        push_new(&mut new, hs);
        out[*s] = Rc::new(new);
    }
    Rc::new(out)
}

/// Walking from `from` to `to`: the timers that are still running when there, the world
/// without the heights whose timers ran out (or that are on their way off for good), and
/// those heights.
pub fn expire(states: &World, timers: &Timers, from: Point, to: Point, moving_off: &HashSet<Pair>) -> (World, Timers, BTreeSet<Pair>) {
    if timers.is_empty() {
        return (states.clone(), Timers::new(), BTreeSet::new());
    }
    let took = dist(from, to) / LEG_SPEED + ACTIVATION_TICS;
    let left: Timers = timers.iter().filter(|(k, v)| **v - took > 0.0 && !moving_off.contains(k)).map(|(k, v)| (*k, *v - took)).collect();
    let gone: BTreeSet<Pair> = timers.keys().copied().filter(|k| !left.contains_key(k)).collect();
    if gone.is_empty() {
        return (states.clone(), left, gone);
    }
    (without(states, gone.iter().copied()), left, gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(floor: f64, ceiling: f64) -> Heights {
        Heights { floor, ceiling }
    }

    fn world(sectors: Vec<Vec<Heights>>) -> World {
        Rc::new(sectors.into_iter().map(Rc::new).collect())
    }

    #[test]
    fn without_keeps_a_sector_at_some_height() {
        let w = world(vec![vec![h(0.0, 128.0), h(64.0, 128.0)], vec![h(8.0, 72.0)]]);
        let out = without(&w, [(0, h(0.0, 128.0).key()), (1, h(8.0, 72.0).key())]);
        assert_eq!(*out[0], vec![h(64.0, 128.0)]);
        assert_eq!(*out[1], vec![h(8.0, 72.0)]);
    }

    #[test]
    fn lowering_for_good_replaces_the_heights() {
        let w = world(vec![vec![h(64.0, 128.0)]]);
        let effects = vec![(0, vec![h(0.0, 128.0)])];
        assert_eq!(*apply_effects(&w, true, &effects)[0], vec![h(0.0, 128.0)]);
        assert_eq!(*apply_effects(&w, false, &effects)[0], vec![h(64.0, 128.0), h(0.0, 128.0)]);
        // raising: the old heights stay possible (it may not have got there yet)
        let up = vec![(0, vec![h(96.0, 128.0)])];
        assert_eq!(apply_effects(&w, true, &up)[0].len(), 2);
    }

    #[test]
    fn timers_run_out_on_the_way() {
        let w = world(vec![vec![h(0.0, 128.0), h(64.0, 128.0)]]);
        let pair = (0, h(0.0, 128.0).key());
        let timers = Timers::from([(pair, 20.0)]);
        // 120 units: 10 tics of walking, plus 10
        let (_, left, gone) = expire(&w, &timers, (0.0, 0.0), (119.0, 0.0), &HashSet::default());
        assert!(gone.is_empty() && left[&pair] > 0.0);
        let (after, left, gone) = expire(&w, &timers, (0.0, 0.0), (121.0, 0.0), &HashSet::default());
        assert!(left.is_empty() && gone.contains(&pair));
        assert_eq!(*after[0], vec![h(64.0, 128.0)]);
    }
}
