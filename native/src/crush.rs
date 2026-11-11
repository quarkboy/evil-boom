//! What a crusher does to her, by DSDA's p_ceilng.c and p_map.c: it comes down to 8 over its
//! floor and back up, and while its ceiling is lower than she's tall anywhere over her box she
//! can't move at all (P_TryMove takes the lowest ceiling over the box) and takes 10 every fourth
//! tic. A fast one keeps its speed on through her: one stroke, 120 at the worst. A slow one
//! slows to an eighth of its speed on her: held there, some hundred hits, dead.

use crate::geom::Point;
use crate::motor::{run_tics, Observation, FULL_SPEED};
use crate::nav::Heights;
use crate::navigator::{Navigator, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::specials::CrusherKind;
use std::collections::BTreeSet;

/// a crusher comes down this far over its floor
pub const BOTTOM: f64 = 8.0;
/// what a crusher does a hit...
const HIT: f64 = 10.0;
/// ...once in this many tics
const HIT_EVERY: f64 = 4.0;

/// Tics, once it's on its way down, before a crusher at heights `h` has her pinned.
pub fn room_tics(h: Heights, kind: CrusherKind) -> f64 {
    (h.ceiling - h.floor - PLAYER_HEIGHT) / kind.speed
}

/// The health one stroke of a crusher that keeps on through her costs at the worst (all of the
/// stroke lower than she's tall, down and back up), through her armor. A crusher that holds her
/// costs all she has.
pub fn stroke_cost(kind: CrusherKind, obs: &Observation) -> f64 {
    let health = obs.health.unwrap_or(100.0);
    if kind.holds {
        return health;
    }
    let tics = 2.0 * (PLAYER_HEIGHT - BOTTOM) / kind.speed;
    obs.through_armor((tics / HIT_EVERY).ceil() * HIT)
}

/// Tics to get all of her out of sector s at the worst: from its point farthest from a way out
/// (onto a floor beside it she can step to, with room for her, at `heights`), starting off at
/// full speed the other way. 0 with no way out of it.
pub fn out_tics(nav: &Navigator, heights: &[Heights], s: usize) -> f64 {
    let mesh = &nav.mesh;
    let Some(fh) = heights.get(s) else { return 0.0 };
    let polys: Vec<usize> = (0..mesh.polys.len())
        .filter(|&p| mesh.polys[p].as_ref().is_some_and(|q| q.sector == s))
        .collect();
    let mut ways: Vec<(Point, Point)> = Vec::new();
    for &p in &polys {
        for &pi in &mesh.poly(p).portals {
            let q = &mesh.portals[pi];
            let o = mesh.poly(q.other(p)).sector;
            if o != s
                && nav.statically_passable(pi, p, &BTreeSet::new())
                && heights.get(o).is_some_and(|oh| {
                    oh.floor <= fh.floor + MAX_STEP && oh.ceiling - oh.floor >= PLAYER_HEIGHT
                })
            {
                ways.push((q.p1, q.p2));
            }
        }
    }
    if ways.is_empty() {
        return 0.0;
    }
    let far = polys
        .iter()
        .flat_map(|&p| {
            mesh.poly(p)
                .vertices
                .iter()
                .copied()
                .chain([mesh.poly(p).center])
        })
        .map(|pt| {
            ways.iter()
                .map(|&(a, b)| crate::tactical::combat::segment_point_distance(a, b, pt))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max);
    run_tics(-FULL_SPEED, far + PLAYER_RADIUS, 1.0)
}

/// Is a crusher of `kind`, at heights `h` (stopped, or anywhere in its stroke), a trap for
/// whoever's in it should it start again: less room over her head, in tics, than getting out
/// takes at the worst (`out`), and its stroke more than she'd take on (`afford`)?
pub fn trap(h: Heights, kind: CrusherKind, out: f64, afford: f64, obs: &Observation) -> bool {
    room_tics(h, kind) < out && stroke_cost(kind, obs) > afford
}
