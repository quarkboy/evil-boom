// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Moving the player: the path geometry she follows, and the low-level controller that turns
//! a wanted movement into controls the way a person playing would (smooth turns, no
//! straferun, working against momentum). What she observes and the controls themselves are
//! `crate::domain`'s.

pub mod follower;
pub mod keys;

use crate::domain::{
    heading, norm, Controls, Observation, FRICTION, FULL_SPEED, MAX_MOVE, MAX_TURN, RUN_THRUST,
    TURN_ACCEL,
};
use crate::geom::{closest_point, dist, Point};
use crate::nav::Heights;

/// A drop this near ahead onto a narrow landing...
pub const CREEP_RANGE: f64 = 48.0;
/// ...narrower than this the way she's going...
pub const LANDING_ROOM: f64 = 192.0;
/// ...is stepped off no slower than this (units a tic).
pub const DROP_CREEP: f64 = 1.0;

/// The pace to step off a drop of `fall` at, to go no further than `room` after: the fall,
/// about sqrt(2h) tics, and the slide on landing, 1 / (1 - friction) tics' worth, at the
/// speed she went over.
pub fn creep_pace(room: f64, fall: f64) -> f64 {
    let tics = (2.0 * fall).sqrt() + 1.0 / (1.0 - FRICTION);
    (room / tics).max(DROP_CREEP)
}

/// deceleration assumed when planning speed (units a tic, per tic)
pub const BRAKE: f64 = 1.5;
/// moving up to this far off the facing direction costs no speed
const STRAFE_FREE: f64 = 40.0;
/// (turn in degrees, speed to take it at)
const CORNER_SPEEDS: [(f64, f64); 6] = [
    (0.0, FULL_SPEED),
    (20.0, FULL_SPEED),
    (45.0, 11.0),
    (90.0, 6.5),
    (135.0, 4.5),
    (180.0, 3.0),
];
/// turns within this distance count as one corner
const CORNER_SPAN: f64 = 48.0;
/// how close to a waypoint before steering on past it
const CORNER_CUT: f64 = 6.0;

pub(crate) fn corner_speed(turn: f64) -> f64 {
    for w in CORNER_SPEEDS.windows(2) {
        let ((a0, v0), (a1, v1)) = (w[0], w[1]);
        if turn <= a1 {
            return v0 + (v1 - v0) * (turn - a0) / (a1 - a0);
        }
    }
    CORNER_SPEEDS[CORNER_SPEEDS.len() - 1].1
}

/// A waypoint polyline with arc-length lookups and a speed profile.
#[derive(Clone, Debug)]
pub struct Track {
    pub pts: Vec<Point>,
    pub cum: Vec<f64>,
    pub length: f64,
    /// how sharp the corner starting at each vertex is (several small turns close together are
    /// one big one)
    corner: Vec<f64>,
}

impl Track {
    pub fn new(pts: Vec<Point>) -> Track {
        let n = pts.len();
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            cum.push(cum[cum.len() - 1] + dist(w[0], w[1]));
        }
        let length = cum[cum.len() - 1];
        let mut turns = vec![0.0; n];
        for i in 1..n.saturating_sub(1) {
            if dist(pts[i - 1], pts[i]) > 1e-6 && dist(pts[i], pts[i + 1]) > 1e-6 {
                turns[i] = norm(heading(pts[i], pts[i + 1]) - heading(pts[i - 1], pts[i]));
            }
        }
        let mut corner = vec![0.0; n];
        for i in 1..n.saturating_sub(1) {
            let (mut total, mut j) = (0.0, i);
            while j < n - 1 && cum[j] - cum[i] <= CORNER_SPAN {
                total += turns[j];
                j += 1;
            }
            corner[i] = f64::max(f64::abs(total), turns[i].abs());
        }
        Track {
            pts,
            cum,
            length,
            corner,
        }
    }

    /// (segment, arc length, distance) of the closest point on the track between a little
    /// behind `s_now` and `ahead` past it (so progress can't skip to a later part of a track
    /// that passes nearby).
    pub fn project(&self, p: Point, s_now: f64, ahead: f64) -> (usize, f64, f64) {
        let n = self.pts.len();
        if n == 1 {
            return (0, 0.0, dist(p, self.pts[0]));
        }
        let (lo, hi) = (s_now - 32.0, s_now + ahead);
        let mut best: Option<(usize, f64, f64)> = None;
        for k in 0..n - 1 {
            if self.cum[k + 1] < lo {
                continue;
            }
            if self.cum[k] > hi {
                break;
            }
            let (a, b) = (self.pts[k], self.pts[k + 1]);
            let seg = self.cum[k + 1] - self.cum[k];
            let mut q = closest_point(p, a, b);
            let mut s = self.cum[k] + dist(a, q);
            if s > hi || s < lo {
                s = s.max(lo).max(self.cum[k]).min(hi).min(self.cum[k + 1]);
                let t = if seg > 1e-9 {
                    (s - self.cum[k]) / seg
                } else {
                    0.0
                };
                q = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
            let d = dist(p, q);
            if best.is_none_or(|(_, _, bd)| d < bd - 1e-6) {
                best = Some((k, s, d));
            }
        }
        best.unwrap_or((n - 2, self.length, dist(p, self.pts[n - 1])))
    }

    /// A point to steer for: `ahead` along the track, but never past the next waypoint till
    /// nearly there (cutting a corner's inside snags the box on it).
    pub fn carrot(&self, k: usize, s: f64, ahead: f64) -> Point {
        let n = self.pts.len();
        let mut next = k + 1;
        while next < n - 1 && self.cum[next] - s < CORNER_CUT {
            next += 1;
        }
        self.point_at((s + ahead).min(self.cum[next.min(n - 1)]))
    }

    pub fn point_at(&self, s: f64) -> Point {
        let n = self.pts.len();
        if s <= 0.0 || n == 1 {
            return self.pts[0];
        }
        if s >= self.length {
            return self.pts[n - 1];
        }
        let k = (self.cum.partition_point(|&c| c <= s).saturating_sub(1)).min(n - 2);
        let (a, b) = (self.pts[k], self.pts[k + 1]);
        let seg = (self.cum[k + 1] - self.cum[k]).max(1e-9);
        let t = (s - self.cum[k]) / seg;
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    }

    /// The fastest she can go at arc length `s` and still slow down for what's ahead.
    pub fn speed_limit(&self, s: f64, end_speed: f64) -> f64 {
        let rest = (self.length - s).max(0.0);
        let mut limit = (end_speed * end_speed + 2.0 * BRAKE * rest).sqrt();
        if end_speed == 0.0 {
            limit = limit.min(0.3 * rest + 0.5); // (creep the last bit to stop exactly)
        }
        for i in 1..self.pts.len().saturating_sub(1) {
            let ahead = self.cum[i] - s;
            if ahead < -CORNER_SPAN / 2.0 {
                continue;
            }
            if ahead > 320.0 {
                break;
            }
            let v = corner_speed(self.corner[i]);
            limit = limit.min((v * v + 2.0 * BRAKE * ahead.max(0.0)).sqrt());
        }
        limit
    }
}

/// Turns a wanted movement (a tic's displacement) and view angle into controls, turning
/// smoothly and working against momentum.
#[derive(Clone, Debug, Default)]
pub struct Motion {
    /// turn rate now, degrees a tic anticlockwise
    pub rate: f64,
    /// last tic's displacement
    pub disp: Point,
    last: Option<Point>,
    /// at full pace whichever way she faces (`go`): on a floor that's hurting her, the way off
    /// isn't gone slowly for having her eyes on what she's shooting
    pub any_way: bool,
}

impl Motion {
    pub fn observe(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if let Some(last) = self.last {
            let d = (pos.0 - last.0, pos.1 - last.1);
            self.disp = if d.0.hypot(d.1) < 64.0 { d } else { (0.0, 0.0) }; // (teleports kill momentum)
        }
        self.last = Some(pos);
    }

    pub fn speed(&self) -> f64 {
        self.disp.0.hypot(self.disp.1)
    }

    /// This tic's turn rate: easing in and out to land on `angle`.
    pub fn turn_towards(&mut self, obs: &Observation, angle: Option<f64>) -> f64 {
        let err = angle.map_or(0.0, |a| norm(a - obs.angle));
        let want = MAX_TURN
            .min((2.0 * TURN_ACCEL * err.abs()).sqrt())
            .min(err.abs())
            .copysign(err);
        self.rate = (self.rate - TURN_ACCEL).max((self.rate + TURN_ACCEL).min(want));
        if err.abs() < 0.25 && self.rate.abs() <= TURN_ACCEL {
            self.rate = err;
        }
        self.rate
    }

    pub fn drive(
        &mut self,
        obs: &Observation,
        want: Point,
        look: Option<f64>,
        cap: f64,
    ) -> Controls {
        let rate = self.turn_towards(obs, look);
        let facing = (obs.angle + rate).to_radians(); // (thrust goes by the angle after this tic's turn)
                                                      // next tic's displacement is FRICTION * this one's + thrust (on the ground)
        let (ax, ay) = (
            want.0 - FRICTION * self.disp.0,
            want.1 - FRICTION * self.disp.1,
        );
        let (mut cx, mut cy) = (ax / RUN_THRUST * MAX_MOVE, ay / RUN_THRUST * MAX_MOVE);
        let mag = cx.hypot(cy);
        if mag > cap {
            (cx, cy) = (cx * cap / mag, cy * cap / mag);
        }
        let (sin, cos) = facing.sin_cos();
        Controls {
            forward: cx * cos + cy * sin,
            strafe: cx * sin - cy * cos,
            turn: -rate,
            ..Controls::default()
        }
    }

    /// Go `speed` units a tic towards world angle `direction`, slower when that's far off where
    /// she's looking (a little strafing is fine; running sideways isn't).
    pub fn go(
        &mut self,
        obs: &Observation,
        direction: f64,
        speed: f64,
        look: Option<f64>,
    ) -> Controls {
        let off = norm(direction - obs.angle).abs();
        let mut speed = speed;
        // (slower the further the way is from her facing, to 15% backwards. With the fight's aim
        // over a route's legs that's a crawl, and it's kept on purpose: the route at full pace
        // whichever way she's turned, and the same for retreats only, each lost on the score
        // (-0.017, -0.016; 2026-11-11). Going on slowly while she's shooting is a caution)
        // (but not on a floor that's hurting her: `any_way`)
        let off = if self.any_way { 0.0 } else { off };
        if off > 90.0 {
            speed *= (0.45 - (off - 90.0) / 60.0 * 0.3).max(0.15);
        } else if off > STRAFE_FREE {
            speed *= 1.0 - (off - STRAFE_FREE) / (90.0 - STRAFE_FREE) * 0.55;
        }
        let rad = direction.to_radians();
        self.drive(obs, (rad.cos() * speed, rad.sin() * speed), look, MAX_MOVE)
    }

    pub fn hold(&mut self, obs: &Observation, want: Point, look: Option<f64>) -> Controls {
        self.drive(obs, want, look, MAX_MOVE)
    }
}

/// (forward, strafe) that take away her momentum this tic, facing `facing` (degrees) after the
/// tic's turn: turning on the spot is standing still while she turns, not sliding on (a look
/// round just after landing on a small ledge would slide her off it).
pub fn brake(obs: &Observation, facing: f64) -> (f64, f64) {
    let Some((vx, vy)) = obs.velocity else {
        return (0.0, 0.0);
    };
    let (ax, ay) = (-FRICTION * vx, -FRICTION * vy);
    let (mut cx, mut cy) = (ax / RUN_THRUST * MAX_MOVE, ay / RUN_THRUST * MAX_MOVE);
    let mag = cx.hypot(cy);
    if mag > MAX_MOVE {
        (cx, cy) = (cx * MAX_MOVE / mag, cy * MAX_MOVE / mag);
    }
    let (sin, cos) = facing.to_radians().sin_cos();
    (cx * cos + cy * sin, cx * sin - cy * cos)
}

/// Heights as Python printed a tuple of floats ("(48.0, 176.0)").
pub fn fmt_live(h: Option<Heights>) -> String {
    match h {
        Some(h) => format!("({:?}, {:?})", h.floor, h.ceiling),
        None => "None".to_string(),
    }
}
