// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Where a rocket can be fired from to hurt a boss brain: the game's own rules for a player's
//! missile, worked on the map's lines.
//!
//! A rocket leaves 32 over her feet and flies level (no monster in her aim to tilt it), a box
//! 22 across and 8 tall. It goes off where its box no longer fits: a wall, an opening whose
//! floor is not under it or whose ceiling is not 8 over it, a thing in its way. The blast hurts
//! what is within 128 of it (the further of the two map distances, less the thing's radius;
//! heights don't count) and in its sight, by 128 less that distance.
//!
//! So a post is a spot to stand, a way to face and the heights her feet may be at for the
//! rocket to go off near enough the brain. The floor she stands on needn't be at one of them
//! now: a lift passes through them (`Post::feet`), and whoever plans with a post says how the
//! floor gets there.

use crate::domain::heading;
use crate::geom::{dist, Point, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::map::MapData;
use crate::mesh::Mesh;
use crate::nav::Heights;

/// a player's missile leaves this far over her feet (P_SpawnPlayerMissile)
pub const MUZZLE: f64 = 32.0;
/// a rocket's box: half its width, and its height (MT_ROCKET)
const ROCKET_RADIUS: f64 = 11.0;
const ROCKET_HEIGHT: f64 = 8.0;
/// a rocket's blast (A_Explode: P_RadiusAttack 128)
const BLAST: f64 = 128.0;
/// it goes off about this far short of what stopped it (its box against the line, and a
/// half-step of its 20 a tic back)
const SHORT: f64 = 16.0;
/// a brain's box (MT_BOSSBRAIN)
pub const BRAIN_RADIUS: f64 = 16.0;
const BRAIN_HEIGHT: f64 = 16.0;
/// lanes are looked for this far out from the brain...
const REACH: f64 = 4096.0;
/// ...along lines through it this many degrees apart...
const FAN: f64 = 0.25;
/// ...at spots this far apart along each
const SPACING: f64 = 16.0;
/// a post has to work faced this many degrees to either side too (nobody aims to a hair)
const SLACK: f64 = 0.4;
/// ...and is no use hurting the brain less than this a rocket
const WORTH: f64 = 20.0;

/// A spot to fire rockets at the brain from.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub at: Point,
    pub sector: usize,
    /// the way to face (world degrees)
    pub heading: f64,
    /// the heights her feet may be at, lowest and highest
    pub feet: (f64, f64),
    /// what a rocket from the middle of them does to the brain (the blast alone)
    pub damage: f64,
}

/// A line a rocket's way crosses: how far along, and the opening there (none: a wall).
#[derive(Clone, Copy, Debug)]
struct Crossing {
    at: f64,
    gap: Option<(f64, f64)>,
}

/// The lines the segment from `from`, `length` long the way `dir` goes, crosses, nearest first,
/// each with its opening as the heights are.
fn crossings(
    map: &MapData,
    heights: &[Heights],
    from: Point,
    dir: Point,
    length: f64,
) -> Vec<Crossing> {
    let mut out = Vec::new();
    for li in 0..map.linedefs.len() {
        let (a, b) = map.line_seg(li);
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let det = dir.0 * ey - dir.1 * ex;
        if det.abs() < 1e-9 {
            continue;
        }
        let (wx, wy) = (a.0 - from.0, a.1 - from.1);
        let t = (wx * ey - wy * ex) / det;
        let u = (wx * dir.1 - wy * dir.0) / det;
        if !(0.0..=length).contains(&t) || !(0.0..=1.0).contains(&u) {
            continue;
        }
        let gap = match map.sides(li) {
            (Some(f), Some(b)) => {
                let (f, b) = (heights[f], heights[b]);
                Some((f.floor.max(b.floor), f.ceiling.min(b.ceiling)))
            }
            _ => None,
        };
        out.push(Crossing { at: t, gap });
    }
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    out
}

/// Does the rocket, its foot at `z`, get through that opening?
fn fits(gap: Option<(f64, f64)>, z: f64) -> bool {
    gap.is_some_and(|(floor, ceiling)| z > floor && z + ROCKET_HEIGHT <= ceiling)
}

/// What the map says of the brain: where it is and the sector it's in.
#[derive(Clone, Copy, Debug)]
pub struct Brain {
    pub at: Point,
    pub sector: usize,
}

/// A rocket fired level from `from` the way `way` (degrees), its foot at `z`: what its blast
/// does to the brain (none: nothing), and how far it flew. Its box is three lines abreast: the
/// first of them stopped stops it.
pub fn fly(
    map: &MapData,
    heights: &[Heights],
    brain: Brain,
    from: Point,
    way: f64,
    z: f64,
) -> (Option<f64>, f64) {
    let dir = (way.to_radians().cos(), way.to_radians().sin());
    let side = (-dir.1, dir.0);
    let mut stop = REACH + 2048.0;
    for off in [-ROCKET_RADIUS, 0.0, ROCKET_RADIUS] {
        let start = (from.0 + side.0 * off, from.1 + side.1 * off);
        if let Some(c) = crossings(map, heights, start, dir, stop)
            .into_iter()
            .find(|c| !fits(c.gap, z))
        {
            stop = stop.min(c.at);
        }
    }
    // The brain's own box in the way, at a height the rocket meets it.
    let floor = heights[brain.sector].floor;
    let (bx, by) = (brain.at.0 - from.0, brain.at.1 - from.1);
    let (along, across) = (bx * dir.0 + by * dir.1, bx * side.0 + by * side.1);
    let reach = BRAIN_RADIUS + ROCKET_RADIUS;
    if across.abs() < reach && along > 0.0 && z < floor + BRAIN_HEIGHT && z + ROCKET_HEIGHT > floor
    {
        stop = stop.min(along - reach + SHORT);
    }
    let flown = (stop - SHORT).max(0.0);
    let burst = (from.0 + dir.0 * flown, from.1 + dir.1 * flown);
    let away = (burst.0 - brain.at.0)
        .abs()
        .max((burst.1 - brain.at.1).abs())
        - BRAIN_RADIUS;
    let hurt = BLAST - away.max(0.0);
    if hurt <= 0.0 {
        return (None, flown);
    }
    // In the blast's sight: no wall, and no shut opening, between the two.
    let to = (brain.at.0 - burst.0, brain.at.1 - burst.1);
    let far = to.0.hypot(to.1);
    let seen = far < 1.0
        || crossings(map, heights, burst, (to.0 / far, to.1 / far), far)
            .iter()
            .all(|c| c.gap.is_some_and(|(floor, ceiling)| ceiling > floor));
    (seen.then_some(hurt), flown)
}

/// The heights a rocket's foot may be at, fired from `from` the way `way`, to hurt the brain:
/// each run of them with what the middle of it does. Its fate changes only at an opening's
/// floor or ceiling on the way, so the heights between those are tried, one for each.
fn windows(
    map: &MapData,
    heights: &[Heights],
    brain: Brain,
    from: Point,
    way: f64,
) -> Vec<(f64, f64, f64)> {
    let dir = (way.to_radians().cos(), way.to_radians().sin());
    let side = (-dir.1, dir.0);
    let mut marks: Vec<f64> = Vec::new();
    for off in [-ROCKET_RADIUS, 0.0, ROCKET_RADIUS] {
        let start = (from.0 + side.0 * off, from.1 + side.1 * off);
        for c in crossings(map, heights, start, dir, REACH + 2048.0) {
            let Some((floor, ceiling)) = c.gap else {
                break;
            };
            marks.push(floor);
            marks.push(ceiling - ROCKET_HEIGHT);
        }
    }
    let floor = heights[brain.sector].floor;
    marks.push(floor - ROCKET_HEIGHT);
    marks.push(floor + BRAIN_HEIGHT);
    marks.retain(|m| m.is_finite());
    marks.sort_by(f64::total_cmp);
    marks.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    let mut out: Vec<(f64, f64, f64)> = Vec::new();
    for w in marks.windows(2) {
        // (a foot's whole units over the lower mark, up to the upper)
        let (lo, hi) = (w[0] + 1.0, w[1]);
        if hi < lo {
            continue;
        }
        let Some(hurt) = fly(map, heights, brain, from, way, (lo + hi) / 2.0).0 else {
            continue;
        };
        if hurt < WORTH {
            continue;
        }
        match out.last_mut() {
            Some(last) if (last.1 - w[0]).abs() < 0.5 && (last.2 - hurt).abs() < 1.0 => last.1 = hi,
            _ => out.push((lo, hi, hurt)),
        }
    }
    out
}

/// Can she stand there: her box on the map, all of it over the sector's floor or lower, with
/// headroom.
pub fn standable(map: &MapData, mesh: &Mesh, heights: &[Heights], at: Point) -> Option<usize> {
    let sector = mesh.poly(mesh.poly_at(at)?).sector;
    let here = heights[sector];
    let r = PLAYER_RADIUS;
    for (dx, dy) in [(-r, -r), (-r, r), (r, -r), (r, r)] {
        let s = mesh.poly(mesh.poly_at((at.0 + dx, at.1 + dy))?).sector;
        let h = heights[s];
        if h.floor > here.floor || h.ceiling - here.floor < PLAYER_HEIGHT {
            return None;
        }
    }
    // (and no wall through her box: a spot in a corner the mesh's piece still covers)
    for li in 0..map.linedefs.len() {
        if map.linedefs[li].two_sided() {
            continue;
        }
        if crate::geom::box_hits_segment(at.0, at.1, r, map.line_seg(li)) {
            return None;
        }
    }
    Some(sector)
}

/// Every post there is, the heights as given: spots along lines through the brain, each with
/// the heights her feet may be at. A spot's own floor needn't be among them (see the module's
/// header); `feet` is narrowed to what works faced a little to either side too.
pub fn posts(map: &MapData, mesh: &Mesh, heights: &[Heights], brain: Brain) -> Vec<Post> {
    let mut out = Vec::new();
    let lines = (360.0 / FAN) as usize;
    for i in 0..lines {
        let back = FAN * i as f64;
        let dir = (back.to_radians().cos(), back.to_radians().sin());
        // Out from the brain as far as the map goes: a wall ends it (nothing fires through one).
        let wall = crossings(map, heights, brain.at, dir, REACH)
            .into_iter()
            .find(|c| c.gap.is_none())
            .map_or(REACH, |c| c.at);
        let mut d = 2.0 * SPACING;
        while d < wall {
            let at = (brain.at.0 + dir.0 * d, brain.at.1 + dir.1 * d);
            d += SPACING;
            let Some(sector) = standable(map, mesh, heights, at) else {
                continue;
            };
            let way = heading(at, brain.at);
            // (her own floor is under the rocket wherever it is: a lift's, at any height)
            let mut under = heights.to_vec();
            under[sector].floor = f64::NEG_INFINITY;
            let heights = &under[..];
            let all = windows(map, heights, brain, at, way);
            for (lo, hi, damage) in all {
                // (faced a little off either way: the heights that still do)
                let mut feet = (lo, hi);
                for off in [-SLACK, SLACK] {
                    let fit = windows(map, heights, brain, at, way + off)
                        .into_iter()
                        .filter(|w| w.0 <= feet.1 && w.1 >= feet.0)
                        .map(|w| (w.0.max(feet.0), w.1.min(feet.1)))
                        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)));
                    feet = fit.unwrap_or((1.0, 0.0));
                }
                if feet.1 < feet.0 {
                    continue;
                }
                out.push(Post {
                    at,
                    sector,
                    heading: way,
                    feet: (feet.0 - MUZZLE, feet.1 - MUZZLE),
                    damage,
                });
            }
        }
    }
    out
}

/// The brain the map has, if it has one.
pub fn brain_of(map: &MapData, mesh: &Mesh) -> Option<Brain> {
    const BOSS_BRAIN: u16 = 88;
    let t = map.things.iter().find(|t| t.kind == BOSS_BRAIN)?;
    let at = (t.x, t.y);
    Some(Brain {
        at,
        sector: mesh.poly(mesh.poly_at(at)?).sector,
    })
}

/// How far a post is from the brain.
pub fn range(p: &Post, brain: Brain) -> f64 {
    dist(p.at, brain.at)
}
