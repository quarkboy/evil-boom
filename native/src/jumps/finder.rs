//! Finding run-off jumps across gaps. For every ledge edge (a drop of more than a step), take
//! the straight run-up behind it, the speed that run-up gives (vanilla thrust and friction),
//! and fly it: no air friction, gravity 1/tic^2, blocked by walls, by ledge faces more than a
//! step above the player and by low ceilings. Kept if it lands higher than the floor right
//! below the edge (plain drops are already edges).
//!
//! Sectors a trigger can move are allowed: each is given one of its possible heights (starting
//! heights first) that makes the jump work, and the moved heights it needed are recorded in
//! `requires`: the jump is only usable once the world has them.

use super::{Jump, JumpLab, STRAFE_SIDE};
use crate::det::{HashMap, HashSet};
use crate::geom::{
    box_hits_segment, cells, closest_point, dist, segments_cross, Point, MAX_STEP, PLAYER_HEIGHT,
    PLAYER_RADIUS,
};
use crate::motor::{FRICTION, RUN_THRUST};
use crate::nav::{can_cross, Heights};
use std::cell::Cell;

/// degrees off the edge normal
const JUMP_ANGLES: [f64; 7] = [0.0, -30.0, 30.0, -45.0, 45.0, -60.0, 60.0];
/// candidates kept per (takeoff poly, landing sector)
const JUMPS_PER_TARGET: usize = 4;
/// aiming a jump at a sector's nearest part: this far in from its rim
const AIM_INSIDE: f64 = 32.0;
const MAX_RUNUP: f64 = 256.0;
/// how far inside the box's edge the floors under it are found in flight (not on it: a point
/// on a line is in either sector)
const FLOOR_EDGE: f64 = 0.1;
/// run-ups tried: the longest, then shorter ones where it comes down nowhere
const RUNUP_CAPS: [f64; 3] = [MAX_RUNUP, 96.0, 48.0];
/// Map units between clearance checks behind the ledge.
const RUNUP_STEP: f64 = 8.0;
const MIN_RUNUP: f64 = 16.0;
const MAX_FLIGHT_TICS: usize = 120;
/// Map units between takeoff samples along an edge.
const TAKEOFF_SPACING: f64 = 16.0;
const MAX_AIM_ANGLE: f64 = 75.0;
const MIN_AIM_SEPARATION: f64 = 5.0;
const MIN_MARGIN_GAIN: f64 = 8.0;
const MIN_JUMP_SEPARATION: f64 = 8.0;
const GRAVITY: f64 = 1.0;
/// ceiling room to spare over the player during a jump
const FLIGHT_HEADROOM: f64 = 8.0;
/// tics a jump's box may stay over its ledge with a wall right beside it...
const LEDGE_TICS: usize = 4;
/// ...or at all (an edge walk, not a jump)
const LONG_LEDGE_TICS: usize = 30;
/// how near a wall counts as beside the box
const PIN_MARGIN: f64 = 3.0;
/// sectors within this of an edge are aimed at
const AIM_RANGE: f64 = 400.0;

/// Fractions along the ledge in trial order, nearest the midpoint first.
fn takeoff_samples(width: f64) -> Vec<f64> {
    let steps = ((width / TAKEOFF_SPACING).floor() as usize).max(1);
    let mut samples = vec![0.5];
    for i in 0..steps {
        let t = (i as f64 + 0.5) / steps as f64;
        if t != 0.5 {
            samples.push(t);
        }
    }
    samples.sort_by(|p, q| {
        (p - 0.5)
            .abs()
            .total_cmp(&(q - 0.5).abs())
            .then(p.total_cmp(q))
    });
    samples
}

/// The box's corners and edge middles, for what's under it.
fn box_points(r: f64) -> [Point; 8] {
    [
        (-r, -r),
        (0.0, -r),
        (r, -r),
        (r, 0.0),
        (r, r),
        (0.0, r),
        (-r, r),
        (-r, 0.0),
    ]
}

/// How a jump is tried: `high`, sectors met in flight at their highest floor that works; `cap`,
/// the longest run-up; `ceiling`, set if it came in too high for a ceiling it'd have come down
/// under (slower, from a shorter run-up, it may not); `strafe`, running strafing, faster.
#[derive(Clone, Copy)]
struct Flight<'a> {
    high: bool,
    cap: f64,
    ceiling: Option<&'a Cell<bool>>,
    strafe: bool,
}

/// The heights each sector has been given for the jump being tried, in the order given.
#[derive(Clone)]
struct Chosen(Vec<(usize, Heights)>);

impl Chosen {
    fn get(&self, s: usize) -> Option<Heights> {
        self.0.iter().find(|(t, _)| *t == s).map(|(_, h)| *h)
    }
}

/// What the finder works from: per sector the heights it can take (starting ones first), the
/// sectors triggers move, and which portals the box fits through.
pub struct FinderInput {
    pub options: Vec<Vec<Heights>>,
    pub moving: HashSet<usize>,
    pub portal_fits: Vec<bool>,
}

impl JumpLab {
    pub fn find_jumps(&self, input: &FinderInput) -> Vec<Jump> {
        let mesh = &self.mesh;
        // Keep groups in first-discovery order; use the map only to locate them.
        let mut groups: Vec<Vec<Jump>> = Vec::new();
        let mut group_index = HashMap::default();
        for (pi, portal) in mesh.portals.iter().enumerate() {
            if !input.portal_fits.get(pi).copied().unwrap_or(false) {
                continue;
            }
            if portal
                .linedef
                .is_some_and(|li| self.map.linedefs[li].blocks_player())
            {
                continue;
            }
            let width = dist(portal.p1, portal.p2);
            let mut samples = None;
            for (a, b) in [(portal.a, portal.b), (portal.b, portal.a)] {
                let (sa, sb) = (mesh.poly(a).sector, mesh.poly(b).sector);
                let ((x1, y1), (x2, y2)) = (portal.p1, portal.p2);
                let length = width.max(1.0);
                let (mut nx, mut ny) = (-(y2 - y1) / length, (x2 - x1) / length);
                let mid = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
                let ca = mesh.center(a);
                if nx * (mid.0 - ca.0) + ny * (mid.1 - ca.1) < 0.0 {
                    (nx, ny) = (-nx, -ny);
                }
                for &ha in &input.options[sa] {
                    for &hb in &input.options[sb] {
                        if ha.floor - hb.floor <= MAX_STEP {
                            continue;
                        }
                        // fixed angles, plus aiming at nearby sectors high enough to land on (an
                        // island in a pit isn't at any fixed angle)
                        let aimed =
                            self.aim_angles(mid, (nx, ny), hb.floor + MAX_STEP, &input.options);
                        let samples = samples.get_or_insert_with(|| takeoff_samples(width));
                        for ang in JUMP_ANGLES.into_iter().chain(aimed) {
                            let (s, c) = ang.to_radians().sin_cos();
                            let d = (nx * c - ny * s, nx * s + ny * c);
                            for &t in samples.iter() {
                                let takeoff = (x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
                                let chosen = Chosen(if sa == sb {
                                    vec![(sa, ha)]
                                } else {
                                    vec![(sa, ha), (sb, hb)]
                                });
                                let ceiling = Cell::new(false);
                                let jump = self.try_jump(
                                    a,
                                    takeoff,
                                    d,
                                    chosen.clone(),
                                    input,
                                    Flight {
                                        high: false,
                                        cap: MAX_RUNUP,
                                        ceiling: Some(&ceiling),
                                        strafe: false,
                                    },
                                );
                                // A raised-floor landing is an alternative. Only a normal
                                // landing ends the takeoff search for this direction.
                                let normal_landing = jump.is_some();
                                // (none, and it came in too high for a ceiling: from a shorter
                                // run-up, slower: MAP06's ledge behind the fake wall, its ceiling
                                // 88 over the floor, too fast from far back)
                                let found = jump
                                    .or_else(|| {
                                        self.try_jump(
                                            a,
                                            takeoff,
                                            d,
                                            chosen.clone(),
                                            input,
                                            Flight {
                                                high: true,
                                                cap: MAX_RUNUP,
                                                ceiling: None,
                                                strafe: false,
                                            },
                                        )
                                    })
                                    .or_else(|| {
                                        ceiling
                                            .get()
                                            .then(|| {
                                                RUNUP_CAPS[1..].iter().find_map(|&cap| {
                                                    self.try_jump(
                                                        a,
                                                        takeoff,
                                                        d,
                                                        chosen.clone(),
                                                        input,
                                                        Flight {
                                                            high: false,
                                                            cap,
                                                            ceiling: None,
                                                            strafe: false,
                                                        },
                                                    )
                                                })
                                            })
                                            .flatten()
                                    })
                                    // (still nowhere: strafing, faster, as a player runs for a far
                                    // ledge: MAP03's well, over its rim from the walkway)
                                    .or_else(|| {
                                        self.try_jump(
                                            a,
                                            takeoff,
                                            d,
                                            chosen.clone(),
                                            input,
                                            Flight {
                                                high: false,
                                                cap: MAX_RUNUP,
                                                ceiling: None,
                                                strafe: true,
                                            },
                                        )
                                        .map(|mut j| {
                                            j.strafe = true;
                                            j
                                        })
                                    });
                                if let Some(j) = found {
                                    // (strafing runs apart: not taking the places of the jumps
                                    // without, E3M6's off the lift to the soulsphere)
                                    let key = (a, mesh.poly(j.dest_poly).sector, j.strafe);
                                    let index = group_index.entry(key).or_insert_with(|| {
                                        groups.push(Vec::new());
                                        groups.len() - 1
                                    });
                                    groups[*index].push(j);
                                }
                                if normal_landing {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        groups
            .into_iter()
            .flat_map(|group| self.select_landings(group))
            .collect()
    }

    /// Keep the first jumps for a takeoff poly and target sector, plus alternatives
    /// landing farther from a drop (a slightly short jump still lands on MAP24).
    fn select_landings(&self, candidates: Vec<Jump>) -> Vec<Jump> {
        let mut cands = candidates.into_iter();
        let mut kept: Vec<Jump> = cands.by_ref().take(JUMPS_PER_TARGET).collect();
        let mut rest: Vec<(f64, Jump)> = cands.map(|j| (self.landing_margin(&j), j)).collect();
        rest.sort_by(|(ma, a), (mb, b)| {
            mb.total_cmp(ma)
                .then_with(|| dist(a.takeoff, a.dest).total_cmp(&dist(b.takeoff, b.dest)))
                .then_with(|| a.key().cmp(&b.key()))
        });
        let best = kept
            .iter()
            .map(|j| self.landing_margin(j))
            .fold(f64::NEG_INFINITY, f64::max);
        for (m, j) in rest.into_iter().take(JUMPS_PER_TARGET) {
            if m > best + MIN_MARGIN_GAIN
                && kept.iter().all(|k| {
                    dist(j.takeoff, k.takeoff) > MIN_JUMP_SEPARATION
                        || dist(j.dest, k.dest) > MIN_JUMP_SEPARATION
                })
            {
                kept.push(j);
            }
        }
        kept
    }

    /// Angles (degrees off normal n, within +-75) towards nearby sectors whose floor could be
    /// landed on (above min_floor): their middles, then their nearest parts (a little in).
    fn aim_angles(
        &self,
        origin: Point,
        n: Point,
        min_floor: f64,
        options: &[Vec<Heights>],
    ) -> Vec<f64> {
        let base = n.1.atan2(n.0);
        let mut out: Vec<f64> = Vec::new();
        let add = |out: &mut Vec<f64>, p: Point| {
            let ang = ((p.1 - origin.1).atan2(p.0 - origin.0) - base).to_degrees();
            let ang = (ang + 180.0).rem_euclid(360.0) - 180.0;
            if ang.abs() <= MAX_AIM_ANGLE
                && JUMP_ANGLES
                    .iter()
                    .chain(out.iter())
                    .all(|a| (ang - a).abs() > MIN_AIM_SEPARATION)
            {
                out.push(ang.round_ties_even());
            }
        };
        let mut nearby = Vec::new();
        for &(s, c) in &self.sector_centres {
            let d = dist(origin, c);
            if d > AIM_RANGE
                || d < 16.0
                || options[s]
                    .iter()
                    .map(|h| h.floor)
                    .fold(f64::NEG_INFINITY, f64::max)
                    <= min_floor
            {
                continue;
            }
            add(&mut out, c);
            nearby.push((s, c));
        }
        for (s, c) in nearby {
            let near = self
                .sector_edges
                .get(&s)
                .into_iter()
                .flatten()
                .map(|&(a, b)| closest_point(origin, a, b))
                .fold(None, |best: Option<Point>, q| match best {
                    Some(b) if dist(b, origin) <= dist(q, origin) => Some(b),
                    _ => Some(q),
                });
            if let Some(near) = near.filter(|&q| dist(q, origin) > 1.0) {
                let k = (AIM_INSIDE / dist(near, c).max(1.0)).min(1.0);
                add(
                    &mut out,
                    (near.0 + (c.0 - near.0) * k, near.1 + (c.1 - near.1) * k),
                );
            }
        }
        out
    }

    /// How far a jump's landing is from the nearest edge of its landing sector with a drop of
    /// more than a step beyond it (walls don't count: landing by one is fine).
    fn landing_margin(&self, j: &Jump) -> f64 {
        let s = self.mesh.poly(j.dest_poly).sector;
        let floor = self.initial[s].0;
        let mut best = f64::INFINITY;
        for &li in self.sector_lines.get(&s).into_iter().flatten() {
            let (f, b) = self.map.sides(li);
            let other = if f == Some(s) { b } else { f };
            let Some(other) = other else { continue };
            if self.initial[other].0 >= floor - MAX_STEP {
                continue;
            }
            let (a, c) = self.map.line_seg(li);
            best = best.min(dist(j.dest, closest_point(j.dest, a, c)));
        }
        best
    }

    /// Does the box at p straddle a static wall or a solid thing's edge?
    fn box_hits_static_wall(&self, p: Point) -> bool {
        let r = PLAYER_RADIUS - 0.1;
        cells(p.0 - r, p.1 - r, p.0 + r, p.1 + r).any(|c| {
            self.static_grid
                .get(&c)
                .into_iter()
                .flatten()
                .any(|&e| box_hits_segment(p.0, p.1, r, e))
        })
    }

    /// A wall (static, a solid thing, or a floor more than a step above z at the heights being
    /// assumed) within PIN_MARGIN of the box at p?
    fn pinned(&self, p: Point, z: f64, chosen: &Chosen) -> bool {
        let r = PLAYER_RADIUS + PIN_MARGIN;
        if cells(p.0 - r, p.1 - r, p.0 + r, p.1 + r).any(|c| {
            self.static_grid
                .get(&c)
                .into_iter()
                .flatten()
                .any(|&e| box_hits_segment(p.0, p.1, r, e))
        }) {
            return true;
        }
        box_points(r).iter().any(|&(dx, dy)| {
            let q = (p.0 + dx, p.1 + dy);
            self.mesh.poly_at(q).is_some_and(|pi| {
                let poly = self.mesh.poly(pi);
                crate::mesh::inside(&poly.vertices, q.0, q.1)
                    && chosen
                        .get(poly.sector)
                        .unwrap_or(self.initial_heights(poly.sector))
                        .floor
                        > z + MAX_STEP
            })
        })
    }

    fn initial_heights(&self, s: usize) -> Heights {
        let (floor, ceiling) = self.initial[s];
        Heights { floor, ceiling }
    }

    /// The linedefs the segment a-b properly crosses (ascending).
    fn crossed(&self, a: Point, b: Point) -> Vec<usize> {
        let mut cand: Vec<usize> = cells(a.0, a.1, b.0, b.1)
            .flat_map(|c| self.mesh.line_grid.get(&c).into_iter().flatten().copied())
            .collect();
        cand.sort_unstable();
        cand.dedup();
        cand.retain(|&li| {
            let (v1, v2) = self.map.line_seg(li);
            segments_cross(a, b, v1, v2)
        });
        cand
    }

    /// The jump off the edge at `takeoff` from poly `src` heading `n`, if there is one, from a
    /// run-up no longer than `how.cap`. `how.high`: sectors met in flight are taken at their
    /// highest possible floor that works, rather than the first (a platform raised to land on:
    /// MAP09's bridge).
    fn try_jump(
        &self,
        src: usize,
        takeoff: Point,
        mut n: Point,
        mut chosen: Chosen,
        input: &FinderInput,
        how: Flight,
    ) -> Option<Jump> {
        let Flight {
            high,
            cap,
            ceiling,
            strafe,
        } = how;
        // (running and strafing at once: forward and sideways thrust together)
        let thrust = if strafe {
            RUN_THRUST * (1.0 + (STRAFE_SIDE / 50.0).powi(2)).sqrt()
        } else {
            RUN_THRUST
        };
        let mesh = &self.mesh;
        let src_sector = mesh.poly(src).sector;
        let floor = chosen.get(src_sector)?.floor;
        let below = chosen
            .0
            .iter()
            .find(|(s, _)| *s != src_sector)
            .map_or(floor, |(_, h)| h.floor);
        let pick = |chosen: &mut Chosen,
                    sector: usize,
                    ok: &dyn Fn(Heights) -> bool,
                    flight: bool|
         -> Option<Heights> {
            if let Some(h) = chosen.get(sector) {
                return ok(h).then_some(h);
            }
            let mut order = input.options[sector].clone();
            if flight && high {
                order.sort_by(|a, b| b.floor.total_cmp(&a.floor));
            }
            // (what one line moves it moves together: a sector isn't given the height the
            // line leaves it at beside another of the line's left where it began. MAP09: the
            // run off the top of three steps a line builds, kept with the top step built and
            // the one below it not: a jump the world never has the heights for, the stairs to
            // a door never taken, the door jumped at from a lift ten minutes)
            let moved = |effects: &[(usize, Vec<Heights>)], s: usize, h: Heights| -> Option<bool> {
                let hs = &effects.iter().find(|(e, _)| *e == s)?.1;
                let start = self.initial[s];
                hs.iter()
                    .any(|x| (x.floor, x.ceiling) != start)
                    .then(|| (h.floor, h.ceiling) != start && hs.contains(&h))
            };
            let together = |h: Heights| {
                self.permanent.iter().all(|effects| {
                    let Some(mine) = moved(effects, sector, h) else {
                        return true;
                    };
                    chosen.0.iter().all(|&(other, oh)| {
                        moved(effects, other, oh).is_none_or(|theirs| theirs == mine)
                    })
                })
            };
            let h = order.into_iter().find(|&h| ok(h) && together(h))?;
            chosen.0.push((sector, h));
            Some(h)
        };
        // Run-up: back off along -n while every line crossed running forward is crossable (in
        // the running direction) and the box stays clear of walls.
        let (mut runup, mut back) = (0.0, takeoff);
        let r = PLAYER_RADIUS - 1.0;
        let bx = box_points(r);
        'run: while runup < cap {
            let nxt = (
                takeoff.0 - n.0 * (runup + RUNUP_STEP),
                takeoff.1 - n.1 * (runup + RUNUP_STEP),
            );
            let Some(here) = mesh.poly_at(nxt) else { break };
            if self.box_hits_static_wall(nxt) {
                break;
            }
            // the whole box has to fit: the player stands on the highest floor under it
            let Some(hc) = pick(
                &mut chosen,
                mesh.poly(here).sector,
                &|h: Heights| h.ceiling - h.floor >= PLAYER_HEIGHT,
                false,
            ) else {
                break;
            };
            for &(dx, dy) in &bx {
                let Some(q) = mesh.poly_at((nxt.0 + dx, nxt.1 + dy)) else {
                    break 'run;
                };
                let ok = |h: Heights| {
                    h.floor <= hc.floor + MAX_STEP
                        && h.ceiling - h.floor.max(hc.floor) >= PLAYER_HEIGHT
                };
                if pick(&mut chosen, mesh.poly(q).sector, &ok, false).is_none() {
                    break 'run;
                }
            }
            for li in self.crossed(nxt, back) {
                let l = &self.map.linedefs[li];
                let (f, b) = self.map.sides(li);
                let (Some(f), Some(b)) = (f, b) else {
                    break 'run;
                };
                if l.blocks_player() {
                    break 'run;
                }
                // which side is behind (farther from the takeoff)?
                let (v1, v2) = self.map.line_seg(li);
                let behind_front =
                    (v2.0 - v1.0) * (nxt.1 - v1.1) - (v2.1 - v1.1) * (nxt.0 - v1.0) < 0.0;
                let (far, near) = if behind_front { (f, b) } else { (b, f) };
                let hf = pick(&mut chosen, far, &|_| true, false)?;
                if pick(&mut chosen, near, &|h| can_cross(hf, h), false).is_none() {
                    break 'run;
                }
            }
            runup += RUNUP_STEP;
            back = nxt;
        }
        if runup < MIN_RUNUP {
            return None;
        }
        let (mut v, mut d) = (0.0, 0.0);
        while d < runup {
            v = v * FRICTION + thrust;
            d += v;
        }
        // Flight: the player stands on the highest floor under the box, so keeps ledge height
        // till the box has left the ledge, and lands (or steps up) as soon as the front of the
        // box reaches something no more than a step above. Per tic: move, check the floors and
        // ceilings under the box, then fall (first falling tic -2, then -1 per tic).
        let (mut x, mut y) = takeoff;
        // (a landing no higher than the floor below the edge is only an edge, a plain drop, if
        // that floor walks to it: not MAP11's run off the walkway over the nukage, the lip past it
        // and down into the room beyond)
        let below_poly = mesh.poly_at((takeoff.0 + n.0 * 2.0, takeoff.1 + n.1 * 2.0));
        let plain = |s: usize| {
            below_poly.is_none_or(|bp| {
                mesh.poly(bp).sector == s || self.walks_to(bp, s, &input.portal_fits)
            })
        };
        let (mut z, mut vz) = (floor, 0.0);
        let mut airborne = false;
        let mut held = 0;
        let mut pinned = false;
        // (the floors under the box at its full size, as the game finds them: the highest any of
        // it is over. A unit in, she's off the ledge a tic early and a tic lower at the far side:
        // MAP15's run across the pit into the megasphere's door, 20 under its floor in the game)
        let all: Vec<Point> = box_points(PLAYER_RADIUS - FLOOR_EDGE)
            .iter()
            .copied()
            .chain([(0.0, 0.0)])
            .collect();
        let requires = |chosen: &Chosen| -> Vec<(usize, Heights)> {
            chosen
                .0
                .iter()
                .copied()
                .filter(|&(s, h)| {
                    (h.floor, h.ceiling) != self.initial[s] || input.moving.contains(&s)
                })
                .collect()
        };
        for _ in 0..MAX_FLIGHT_TICS {
            let mut nn = (x + n.0 * v, y + n.1 * v);
            if self.box_hits_static_wall(nn) {
                // (a wall in the way: she slides along it, as the game has her, one way or the
                // other (P_SlideMove's stairstep): MAP06's jump along the pit's far wall into the
                // ledge behind the fake wall. Nowhere to slide, something solid in the way, a
                // barrel on the island: the run's over and she comes down where she is, a landing
                // if that's on it: MAP25's stepping stones)
                let (vx, vy) = (n.0 * v, n.1 * v);
                let slide = [(vx, 0.0), (0.0, vy)].into_iter().find(|&(dx, dy)| {
                    dx.hypot(dy) > 0.5 && !self.box_hits_static_wall((x + dx, y + dy))
                });
                if let Some((dx, dy)) = slide {
                    v = dx.hypot(dy);
                    n = (dx / v, dy / v);
                    nn = (x + dx, y + dy);
                } else {
                    v = 0.0;
                    nn = (x, y);
                }
            }
            for li in self.crossed((x, y), nn) {
                let l = &self.map.linedefs[li];
                if !l.two_sided() || l.blocks_player() {
                    return None;
                }
            }
            let mut under: Vec<(usize, Heights)> = Vec::new();
            for &(dx, dy) in &all {
                let q = mesh.poly_at((nn.0 + dx, nn.1 + dy))?;
                let s = mesh.poly(q).sector;
                if under.iter().any(|(t, _)| *t == s) {
                    continue;
                }
                // (a little headroom to spare in the air: MAP09's jump through a door)
                let zz = z;
                let ok = move |h: Heights| {
                    h.floor <= zz + MAX_STEP
                        && h.ceiling - zz.max(h.floor) >= PLAYER_HEIGHT + FLIGHT_HEADROOM
                };
                let Some(h) = pick(&mut chosen, s, &ok, true) else {
                    // (a floor she'd come down on, but no room under its ceiling this high)
                    if let Some(c) = ceiling.filter(|_| {
                        input.options[s].iter().any(|h| {
                            h.floor <= zz + MAX_STEP && h.ceiling - h.floor >= PLAYER_HEIGHT
                        })
                    }) {
                        c.set(true);
                    }
                    return None;
                };
                under.push((s, h));
            }
            let fmax = under
                .iter()
                .map(|(_, h)| h.floor)
                .fold(f64::NEG_INFINITY, f64::max);
            let floor_of = |s: usize| under.iter().find(|(t, _)| *t == s).map(|(_, h)| h.floor);
            (x, y) = nn;
            z += vz;
            if z <= fmax {
                z = fmax;
                vz = 0.0;
                let ahead = mesh.poly_at((
                    x + n.0 * (PLAYER_RADIUS - 1.0),
                    y + n.1 * (PLAYER_RADIUS - 1.0),
                ));
                if let Some(ahead) = ahead.filter(|&p| {
                    mesh.poly(p).sector != src_sector && floor_of(mesh.poly(p).sector) == Some(fmax)
                }) {
                    if fmax <= below + MAX_STEP && plain(mesh.poly(ahead).sector) {
                        return None; // just dropped to the floor below: an ordinary edge
                    }
                    let mut on = (
                        x + n.0 * (PLAYER_RADIUS + 8.0),
                        y + n.1 * (PLAYER_RADIUS + 8.0),
                    );
                    let mut land = mesh.poly_at(on);
                    // (further on: the same sector, or one at the same height past a strip too
                    // narrow to stop in: MAP06's, 8 wide, behind the fake wall)
                    let level = |l: usize| {
                        let s = mesh.poly(l).sector;
                        s == mesh.poly(ahead).sector
                            || chosen
                                .get(s)
                                .or_else(|| input.options[s].first().copied())
                                .is_some_and(|h| h.floor == fmax)
                    };
                    if land.is_none_or(|l| !level(l)) {
                        land = Some(ahead);
                        on = (
                            x + n.0 * (PLAYER_RADIUS - 1.0),
                            y + n.1 * (PLAYER_RADIUS - 1.0),
                        );
                    }
                    return Some(Jump::new(
                        src,
                        land?,
                        on,
                        back,
                        takeoff,
                        v,
                        requires(&chosen),
                    ));
                }
                if !airborne {
                    // still on the ledge, the box hanging over the drop: fine for a few tics,
                    // but a strip narrower than the box by a wall is down to the last unit
                    // (MAP27), and a long one is no jump
                    held += 1;
                    pinned = pinned || self.pinned((x, y), z, &chosen);
                    if held > LONG_LEDGE_TICS || (held > LEDGE_TICS && pinned) {
                        return None;
                    }
                }
                if airborne {
                    // came down supported by something under the box that isn't ahead (a
                    // narrow bridge under the back half): a landing on it all the same
                    let support: Vec<Point> = all
                        .iter()
                        .copied()
                        .filter(|&(dx, dy)| {
                            mesh.poly_at((x + dx, y + dy)).is_some_and(|q| {
                                mesh.poly(q).sector != src_sector
                                    && floor_of(mesh.poly(q).sector) == Some(fmax)
                            })
                        })
                        .collect();
                    let &(dx, dy) = support.iter().min_by(|a, b| {
                        (a.0.abs() + a.1.abs()).total_cmp(&(b.0.abs() + b.1.abs()))
                    })?;
                    let on = (x + dx * 0.8, y + dy * 0.8);
                    let land = mesh.poly_at(on)?;
                    if floor_of(mesh.poly(land).sector) != Some(fmax)
                        || fmax <= below + MAX_STEP && plain(mesh.poly(land).sector)
                    {
                        return None;
                    }
                    return Some(Jump::new(
                        src,
                        land,
                        on,
                        back,
                        takeoff,
                        v,
                        requires(&chosen),
                    ));
                }
            } else {
                airborne = true;
                vz = if vz == 0.0 {
                    -2.0 * GRAVITY
                } else {
                    vz - GRAVITY
                };
            }
        }
        None
    }
}
