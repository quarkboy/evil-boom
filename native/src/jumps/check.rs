// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Checking jumps in the simulator: stand at the run-up facing the takeoff, sprint (as the
//! follower does), brake on touchdown, and see whether the player stops on the landing sector
//! ("stuck"), touches it and slides off ("slid"), or misses it. A soft sprint (full speed only
//! to the edge, then coasting) is tried first: it lands slower, which narrow strips need; then
//! pushing all the way; then letting go somewhere in between (a run along a channel narrower
//! than the box, which holds the player up: coasting from the edge stops short, pushing on
//! overshoots the landing). Those, and shorter run-ups, aren't tried onto a floor that sinks
//! under the player: there's no stopping on that, only running on.
//!
//! A jump counts only if it has slack to it: made the one way that holds up, it still gets
//! there half a unit to either side and from a run-up a little longer or shorter. One that
//! comes in too fast to stop on its landing is tried from shorter run-ups (as a player would).

use super::physics::{Controls, PlayerSim};
use super::{Jump, JumpLab};
use crate::det::HashSet;
use crate::geom::{dist, Point, MAX_STEP};
use crate::motor::{FRICTION, MAX_MOVE, MAX_TURN, RUN_THRUST, TURN_ACCEL};
use crate::nav::Heights;

const TICRATE: usize = 35;
/// a jump has to work this far to either side of its line too (1 is too strict: it loses jumps
/// the game allows)
const ROBUST_SHIFT: f64 = 0.5;
/// ...and get there from a run-up this much longer or shorter (4 loses jumps onto ledges)
const ROBUST_RUNUP: f64 = 2.0;
/// How sure a jump is gets found making it: shifted this far to either side...
const SURE_SHIFTS: [f64; 2] = [2.0, 4.0];
/// ...turned this much either way about its takeoff (degrees)...
const SURE_TURNS: [f64; 2] = [1.5, 3.0];
/// ...and from a run-up this much longer or shorter (the follower's misses are 6-12 off)
const SURE_RUNUP: f64 = 8.0;
/// run-ups tried shorter by this much at a time, down to MIN_RUNUP, for a jump that won't stick
const RUNUP_STEP: f64 = 16.0;
const MIN_RUNUP: f64 = 16.0;
/// where to let go of forward is tried this far apart past the takeoff
const RELEASE_STEP: f64 = 16.0;

/// How long to push forward: until `Release(r)` units past the takeoff (0: a soft jump), or
/// `Full`, through the flight.
#[derive(Clone, Copy, PartialEq)]
enum Push {
    Release(f64),
    Full,
}

impl Push {
    fn label(self) -> String {
        match self {
            Push::Full => "ok".to_string(),
            Push::Release(0.0) => "soft".to_string(),
            Push::Release(r) => format!("soft+{r:.0}"),
        }
    }

    /// How far along from the run-up to push (as the follower's _sprint_jump does).
    fn until(self, edge: f64, flight: f64) -> f64 {
        match self {
            Push::Release(r) => edge + r,
            Push::Full => flight - 8.0,
        }
    }
}

/// What test_jump makes of a jump.
enum Test {
    Stuck(Push),
    /// it doesn't stop on the landing: it lands but slides off (`slid`), or not even that;
    /// and, run straight across the landing onto what's beyond, where it stops
    Off {
        slid: bool,
        through: Option<Point>,
    },
    Unverified,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flight {
    Stuck,
    Slid,
    Missed,
}

fn norm(deg: f64) -> f64 {
    (deg + 180.0).rem_euclid(360.0) - 180.0
}

/// The follower's Motion, as far as braking to a stop goes.
#[derive(Default)]
struct Motion {
    rate: f64,
    disp: Point,
    last: Option<Point>,
}

impl Motion {
    fn observe(&mut self, pos: Point) {
        if let Some(l) = self.last {
            let d = (pos.0 - l.0, pos.1 - l.1);
            self.disp = if d.0.hypot(d.1) < 64.0 { d } else { (0.0, 0.0) };
        }
        self.last = Some(pos);
    }

    fn speed(&self) -> f64 {
        self.disp.0.hypot(self.disp.1)
    }

    /// Stop where we are, facing `look`.
    fn brake(&mut self, angle: f64, look: f64) -> Controls {
        let err = norm(look - angle);
        let want = (MAX_TURN
            .min((2.0 * TURN_ACCEL * err.abs()).sqrt())
            .min(err.abs()))
        .copysign(err);
        self.rate = want.clamp(self.rate - TURN_ACCEL, self.rate + TURN_ACCEL);
        if err.abs() < 0.25 && self.rate.abs() <= TURN_ACCEL {
            self.rate = err;
        }
        let facing = (angle + self.rate).to_radians();
        let (ax, ay) = (-FRICTION * self.disp.0, -FRICTION * self.disp.1);
        let (mut cx, mut cy) = (ax / RUN_THRUST * MAX_MOVE, ay / RUN_THRUST * MAX_MOVE);
        let mag = cx.hypot(cy);
        if mag > MAX_MOVE {
            cx *= MAX_MOVE / mag;
            cy *= MAX_MOVE / mag;
        }
        let (sin, cos) = facing.sin_cos();
        Controls {
            forward: cx * cos + cy * sin,
            strafe: cx * sin - cy * cos,
            turn: -self.rate,
        }
    }
}

impl JumpLab {
    /// The verdict on every jump: "ok" (sprinted all the way), "soft" (coasting from the
    /// edge), "soft+N" (letting go N units past the takeoff), "slid" (lands, slides off),
    /// "fail"; "slid@x,y" / "fail@x,y" for one that works run straight across its landing
    /// strip onto what's beyond, to (x, y); any that sticks only from a shorter run-up with
    /// "~x,y", that run-up; "unverified" if it can't be set up.
    pub fn check_jumps(&self, jumps: &[Jump]) -> Vec<String> {
        let mut sim = PlayerSim::new(self, self.world());
        jumps
            .iter()
            .map(|j| {
                let v = self.verdict(&mut sim, j);
                if sticks(&v) || v == "unverified" || v.contains('@') || self.lands_sinking(j) {
                    return v;
                }
                self.shorter_runups(j)
                    .into_iter()
                    .find_map(|s| {
                        let w = self.verdict(&mut sim, &s);
                        sticks(&w).then(|| format!("{w}~{:.2},{:.2}", s.runup.0, s.runup.1))
                    })
                    .unwrap_or(v)
            })
            .collect()
    }

    /// One jump's verdict (without trying other run-ups): made the way that sticks, it has to
    /// stick from its own line and get there made a little off it (one that sticks coasting
    /// from the edge may fall back to pushing all the way for that).
    fn verdict(&self, sim: &mut PlayerSim<'_>, j: &Jump) -> String {
        match self.test_jump(sim, j) {
            Test::Stuck(push) => {
                let modes: &[Push] = if push == Push::Release(0.0) {
                    &[Push::Release(0.0), Push::Full]
                } else {
                    &[push]
                };
                modes
                    .iter()
                    .copied()
                    .find(|&m| {
                        self.works(sim, j, m, false)
                            && self.variants(j).iter().all(|v| self.works(sim, v, m, true))
                    })
                    .map_or_else(|| "fail".to_string(), Push::label)
            }
            Test::Off { slid, through } => {
                let r = if slid { "slid" } else { "fail" };
                if (slid || through.is_some()) && !self.robust(sim, j) {
                    "fail".to_string()
                } else if let Some(at) = through {
                    format!("{r}@{:.0},{:.0}", at.0, at.1)
                } else {
                    r.to_string()
                }
            }
            Test::Unverified => "unverified".to_string(),
        }
    }

    /// Does the jump land on a floor that sinks once stepped on?
    fn lands_sinking(&self, j: &Jump) -> bool {
        self.sinking.contains(&self.mesh.poly(j.dest_poly).sector)
    }

    /// The jump from run-ups RUNUP_STEP shorter at a time, longest first, down to MIN_RUNUP.
    fn shorter_runups(&self, j: &Jump) -> Vec<Jump> {
        let length = dist(j.runup, j.takeoff);
        if length < 1e-6 {
            return Vec::new();
        }
        let (ux, uy) = (
            (j.takeoff.0 - j.runup.0) / length,
            (j.takeoff.1 - j.runup.1) / length,
        );
        (1..)
            .map(|k| length - RUNUP_STEP * f64::from(k))
            .take_while(|&l| l >= MIN_RUNUP)
            .map(|l| Jump {
                runup: (j.takeoff.0 - ux * l, j.takeoff.1 - uy * l),
                ..j.clone()
            })
            .collect()
    }

    /// The flight of one jump, tic by tic (x, y, z, angle), for comparing with the engine.
    pub fn trace(&self, j: &Jump, soft: bool) -> Vec<(f64, f64, f64, f64)> {
        let push = if soft { Push::Release(0.0) } else { Push::Full };
        let mut sim = PlayerSim::new(self, self.world());
        let Some(heading) = self.prepare(&mut sim, j) else {
            return Vec::new();
        };
        let edge = dist(j.runup, j.takeoff);
        let flight = edge + dist(j.takeoff, j.dest);
        let (uy, ux) = heading.to_radians().sin_cos();
        let mut out = Vec::new();
        for _ in 0..80 {
            let o = sim.pos();
            out.push((o.0, o.1, sim.z_units(), sim.angle_degrees()));
            let flown = (o.0 - j.runup.0) * ux + (o.1 - j.runup.1) * uy;
            let pushing = flown < push.until(edge, flight);
            let turn = (-norm(j.facing(heading) - sim.angle_degrees())).clamp(-3.0, 3.0);
            sim.step(Controls {
                forward: if pushing { 50.0 } else { 0.0 },
                strafe: if pushing { j.sidestep() } else { 0.0 },
                turn,
            });
        }
        out
    }

    /// Does the jump stick, pushing as `push` says? With `reach`: does it get there at all
    /// (sliding on after touching down is all right, not falling short); a variant that can't
    /// be set up or can't get going (its run-up backed into a wall) doesn't count against it.
    fn works(&self, sim: &mut PlayerSim<'_>, j: &Jump, push: Push, reach: bool) -> bool {
        let Some(heading) = self.prepare(sim, j) else {
            return reach;
        };
        let (result, at) = self.fly(sim, j, heading, push, false);
        if reach && result == Flight::Missed && dist(at, j.runup) < 4.0 {
            return true;
        }
        result == Flight::Stuck || (reach && result == Flight::Slid)
    }

    /// The jump a little off: half a unit to either side of its line, and from a run-up
    /// ROBUST_RUNUP longer or shorter.
    fn variants(&self, j: &Jump) -> Vec<Jump> {
        let ((rx, ry), (tx, ty)) = (j.runup, j.takeoff);
        let length = (tx - rx).hypot(ty - ry);
        if length < 1e-6 {
            return Vec::new();
        }
        let (ux, uy) = ((tx - rx) / length, (ty - ry) / length);
        let (px, py) = (-uy, ux);
        let mut out = Vec::new();
        for s in [ROBUST_SHIFT, -ROBUST_SHIFT] {
            out.push(Jump {
                runup: (rx + px * s, ry + py * s),
                takeoff: (tx + px * s, ty + py * s),
                dest: (j.dest.0 + px * s, j.dest.1 + py * s),
                ..j.clone()
            });
        }
        for d in [-ROBUST_RUNUP, ROBUST_RUNUP] {
            if length - d > 1.0 {
                out.push(Jump {
                    runup: (rx + ux * d, ry + uy * d),
                    ..j.clone()
                });
            }
        }
        out.retain(|v| self.mesh.poly_at(v.runup).is_some());
        out
    }

    /// How sure each jump is (0-1): of it made shifted to either side, turned about its
    /// takeoff and from a longer or shorter run-up (SURE_*), the share that still gets onto
    /// its landing (stops there, or slides or runs across it as it does). 1 for one that
    /// can't be varied (no room to).
    pub fn robustness(&self, jumps: &[Jump]) -> Vec<f64> {
        let mut sim = PlayerSim::new(self, self.world());
        jumps
            .iter()
            .map(|j| {
                let variants: Vec<Jump> = self
                    .sure_variants(j)
                    .into_iter()
                    .filter(|v| self.mesh.poly_at(v.runup).is_some())
                    .collect();
                let mut tried = 0usize;
                let mut landed = 0usize;
                for v in &variants {
                    match self.test_jump(&mut sim, v) {
                        Test::Unverified => {}
                        Test::Off {
                            slid: false,
                            through: None,
                        } => tried += 1,
                        _ => {
                            tried += 1;
                            landed += 1;
                        }
                    }
                }
                if tried == 0 {
                    1.0
                } else {
                    landed as f64 / tried as f64
                }
            })
            .collect()
    }

    /// The ways of making a jump a little differently that `robustness` tries.
    fn sure_variants(&self, j: &Jump) -> Vec<Jump> {
        let ((rx, ry), (tx, ty)) = (j.runup, j.takeoff);
        let length = (tx - rx).hypot(ty - ry);
        if length < 1e-6 {
            return Vec::new();
        }
        let (ux, uy) = ((tx - rx) / length, (ty - ry) / length);
        let (px, py) = (-uy, ux);
        let mut out = Vec::new();
        for s in SURE_SHIFTS.iter().flat_map(|&s| [s, -s]) {
            out.push(Jump {
                runup: (rx + px * s, ry + py * s),
                takeoff: (tx + px * s, ty + py * s),
                dest: (j.dest.0 + px * s, j.dest.1 + py * s),
                ..j.clone()
            });
        }
        for a in SURE_TURNS.iter().flat_map(|&a| [a, -a]) {
            let (sin, cos) = a.to_radians().sin_cos();
            let (dx, dy) = (rx - tx, ry - ty);
            out.push(Jump {
                runup: (tx + dx * cos - dy * sin, ty + dx * sin + dy * cos),
                ..j.clone()
            });
        }
        for d in [SURE_RUNUP, -SURE_RUNUP] {
            if length + d > 1.0 {
                out.push(Jump {
                    runup: (tx - ux * (length + d), ty - uy * (length + d)),
                    ..j.clone()
                });
            }
        }
        out
    }

    /// Does a jump that only slides still work half a unit to either side?
    fn robust(&self, sim: &mut PlayerSim<'_>, j: &Jump) -> bool {
        let ((rx, ry), (tx, ty)) = (j.runup, j.takeoff);
        let length = (tx - rx).hypot(ty - ry);
        if length < 1e-6 {
            return true;
        }
        let (px, py) = (-(ty - ry) / length, (tx - rx) / length);
        [ROBUST_SHIFT, -ROBUST_SHIFT].iter().all(|&s| {
            let shifted = Jump {
                runup: (rx + px * s, ry + py * s),
                takeoff: (tx + px * s, ty + py * s),
                dest: (j.dest.0 + px * s, j.dest.1 + py * s),
                ..j.clone()
            };
            self.mesh.poly_at(shifted.runup).is_some()
                && !matches!(
                    self.test_jump(sim, &shifted),
                    Test::Off {
                        slid: false,
                        through: None
                    }
                )
        })
    }

    /// Soft, then pushing all the way, then letting go RELEASE_STEP further past the takeoff
    /// at a time; if none stops on the landing, run straight across it onto what's beyond
    /// (narrow strips).
    fn test_jump(&self, sim: &mut PlayerSim<'_>, j: &Jump) -> Test {
        let edge = dist(j.runup, j.takeoff);
        let flight = edge + dist(j.takeoff, j.dest);
        let releases = if self.lands_sinking(j) { 0 } else { usize::MAX };
        let partial = (1..)
            .map(|k| RELEASE_STEP * f64::from(k))
            .take_while(|&r| edge + r < flight - 8.0)
            .map(Push::Release)
            .take(releases);
        let mut slid = false;
        for push in [Push::Release(0.0), Push::Full].into_iter().chain(partial) {
            let Some(heading) = self.prepare(sim, j) else {
                return Test::Unverified;
            };
            match self.fly(sim, j, heading, push, false).0 {
                Flight::Stuck => return Test::Stuck(push),
                Flight::Slid if push == Push::Full => slid = true,
                _ => {}
            }
        }
        let through = self.prepare(sim, j).and_then(|heading| {
            match self.fly(sim, j, heading, Push::Full, true) {
                (Flight::Stuck, at) => Some(at),
                _ => None,
            }
        });
        Test::Off { slid, through }
    }

    /// The heights the jump needs (and whatever else the trigger making them moves: two posts
    /// lowering together, say), standing at the run-up facing the takeoff; the heading, or
    /// None if it can't be stood at.
    fn prepare(&self, sim: &mut PlayerSim<'_>, j: &Jump) -> Option<f64> {
        let mut heights: Vec<Heights> = self
            .initial
            .iter()
            .map(|&(floor, ceiling)| Heights { floor, ceiling })
            .collect();
        for &(s, h) in &j.requires {
            let init = self.initial[s];
            if (h.floor, h.ceiling) == init {
                continue;
            }
            if let Some(t) = self
                .permanent
                .iter()
                .find(|t| t.iter().any(|(ts, hs)| *ts == s && hs.contains(&h)))
            {
                for (ts, hs) in t {
                    if let Some(first) = hs.first() {
                        heights[*ts] = *first;
                    }
                }
            }
        }
        for &(s, h) in &j.requires {
            heights[s] = h;
        }
        sim.heights = heights
            .iter()
            .map(|h| {
                (
                    super::physics::to_fixed(h.floor),
                    super::physics::to_fixed(h.ceiling),
                )
            })
            .collect();
        sim.place((j.runup.0.round(), j.runup.1.round()), self.start_angle);
        for _ in 0..4 {
            sim.step(Controls::default());
        }
        if dist(sim.pos(), j.runup) > 4.0 {
            return None;
        }
        let heading = (j.takeoff.1 - j.runup.1)
            .atan2(j.takeoff.0 - j.runup.0)
            .to_degrees();
        // (faced off the way, strafing: the way's still `heading`)
        for _ in 0..20 {
            let diff = norm(j.facing(heading) - sim.angle_degrees());
            if diff.abs() < 1.0 {
                break;
            }
            sim.step(Controls {
                turn: (-diff).clamp(-25.0, 25.0),
                ..Controls::default()
            });
        }
        Some(heading)
    }

    /// Sprint, then brake on touchdown: and where we ended up. `through`: keep sprinting
    /// across the landing sector and land on whatever comes next (no lower than a step below
    /// it).
    fn fly(
        &self,
        sim: &mut PlayerSim<'_>,
        j: &Jump,
        heading: f64,
        push: Push,
        through: bool,
    ) -> (Flight, Point) {
        let edge = dist(j.runup, j.takeoff);
        let flight = edge + dist(j.takeoff, j.dest);
        let (uy, ux) = heading.to_radians().sin_cos();
        let mut want = self.mesh.poly(j.dest_poly).sector;
        let src: HashSet<usize> = j
            .src_polys
            .iter()
            .map(|&p| self.mesh.poly(p).sector)
            .collect();
        let mut motion = Motion::default();
        let mut last = sim.pos();
        let mut touched: Option<usize> = None;
        let mut crossed = false;
        for i in 0..6 * TICRATE {
            let o = sim.pos();
            motion.observe(o);
            if dist(o, last) > 64.0 || sim.teleported {
                return (Flight::Missed, last);
            }
            last = o;
            let here = self.mesh.poly_at(o).map(|p| self.mesh.poly(p).sector);
            // only past the takeoff: the landing sector can be the run-up's own, across a gap
            let past = (o.0 - j.takeoff.0) * ux + (o.1 - j.takeoff.1) * uy > 0.0;
            crossed = crossed || (past && here == Some(want));
            if let (None, true, Some(s)) = (touched, past, here) {
                if (sim.z_units() - sim.floor_of(s)).abs() < 1.0 {
                    if !through && s == want {
                        touched = Some(i);
                    } else if through
                        && crossed
                        && !src.contains(&s)
                        && s != want
                        && sim.floor_of(s) >= sim.floor_of(want) - MAX_STEP
                    {
                        touched = Some(i);
                        want = s;
                    }
                }
            }
            if let Some(t) = touched {
                if motion.speed() < 0.3 || i - t > 2 * TICRATE {
                    return (
                        if here == Some(want) {
                            Flight::Stuck
                        } else {
                            Flight::Slid
                        },
                        last,
                    );
                }
                let c = motion.brake(sim.angle_degrees(), heading);
                sim.step(c);
                continue;
            }
            let flown = (o.0 - j.runup.0) * ux + (o.1 - j.runup.1) * uy;
            if flown >= flight - 8.0 && i > 4 * TICRATE {
                break;
            }
            let pushing = through || flown < push.until(edge, flight);
            let turn = (-norm(j.facing(heading) - sim.angle_degrees())).clamp(-3.0, 3.0);
            sim.step(Controls {
                forward: if pushing { 50.0 } else { 0.0 },
                strafe: if pushing { j.sidestep() } else { 0.0 },
                turn,
            });
        }
        (Flight::Missed, last)
    }
}

/// A verdict that stops on the landing ("ok", "soft", "soft+N").
fn sticks(v: &str) -> bool {
    v == "ok" || v.starts_with("soft")
}
