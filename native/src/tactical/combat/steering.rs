//! Where to move while fighting: every way round scored for room to run, being at the right
//! distance from each monster (her target's window, the others' standoffs), going across their
//! line of fire, not ending up between them, and keeping on the way she's going.

use super::aiming::{standoff, window};
use super::{mon, spread, Combat};
use crate::geom::{dist, Point};
use crate::motor::{Observation, Thing};
use crate::tactical::bestiary::{Attack, Weapon};

const WAYS: usize = 16;

/// where she'd be, about, after half a second running a way
const LOOK: f64 = 160.0;

/// room to run looked for this far
const ROOM: f64 = 384.0;

/// walls looked for round where she'd be this far
const OPEN: f64 = 192.0;

/// what each counts for
const W_OPEN: f64 = 2.0;
const W_ROOM: f64 = 2.0;
const W_NEAR: f64 = 1.0;
const W_FAR: f64 = 0.6;
const W_ACROSS: f64 = 1.2;
const W_SURROUND: f64 = 1.5;
const W_KEEP: f64 = 0.8;

/// still this long pushing one way: not that way for a while
const STUCK_TICS: i64 = 6;

impl Combat {
    /// The way to move fighting `target` with `gun` (None: nowhere safe).
    pub(super) fn steer(&mut self, obs: &Observation, target: Option<&Thing>, gun: &Weapon) -> Option<Point> {
        let pos = obs.pos();
        let mut them = self.against_her();
        if let Some(t) = target.filter(|t| !them.iter().any(|m| m.id == t.id)) {
            them.push(t.clone()); // (out of sight just now: where it was)
        }
        let shooting = target.is_some_and(|t| matches!(mon(t.name).attack, Attack::Missile | Attack::Hitscan));
        let spread_now = spread(pos, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
        let mut best: Option<(f64, Point)> = None;
        for i in 0..WAYS {
            let a = 2.0 * std::f64::consts::PI * i as f64 / WAYS as f64;
            let d = (a.cos(), a.sin());
            if !self.safe(obs, d, 64.0) {
                continue;
            }
            if self.steer_stuck.is_some_and(|(w, until)| obs.tic < until && d.0 * w.0 + d.1 * w.1 > 0.7) {
                continue; // (pushed that way and got nowhere)
            }
            let room = self.clear_run(obs, d, ROOM);
            let ahead = (pos.0 + d.0 * LOOK.min(room), pos.1 + d.1 * LOOK.min(room));
            let mut s = W_ROOM * room / ROOM + W_OPEN * self.openness(obs, ahead, OPEN);
            for m in &them {
                let info = mon(m.name);
                let w = (info.threat + 1.0).sqrt();
                let at = dist(ahead, (m.x, m.y));
                let (near, far) = if Some(m.id) == target.map(|t| t.id) { window(gun, m) } else { (standoff(m.name), f64::INFINITY) };
                if at < near {
                    s -= W_NEAR * w * (near - at) / LOOK;
                }
                if at > far {
                    s -= W_FAR * (at - far) / LOOK;
                }
            }
            if let Some(t) = target.filter(|_| shooting) {
                let to = ((t.x - pos.0) / dist((t.x, t.y), pos).max(1.0), (t.y - pos.1) / dist((t.x, t.y), pos).max(1.0));
                s += W_ACROSS * (d.0 * to.1 - d.1 * to.0).abs();
            }
            if them.len() >= 2 {
                let after = spread(ahead, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
                s -= W_SURROUND * ((after - spread_now) / 90.0).max(0.0) + W_SURROUND * (after / 360.0);
            }
            if let Some(k) = self.steer_way {
                s += W_KEEP * (d.0 * k.0 + d.1 * k.1);
            }
            if best.is_none_or(|(bs, _)| s > bs) {
                best = Some((s, d));
            }
        }
        // (pushing a way and not moving: caught on something the floor doesn't show)
        if let Some(k) = self.steer_way {
            if self.velocity.0.hypot(self.velocity.1) < 1.0 && self.steer_tic == obs.tic - 1 {
                self.steer_still += 1;
            } else {
                self.steer_still = 0;
            }
            if self.steer_still > STUCK_TICS {
                self.steer_stuck = Some((k, obs.tic + STUCK_TICS * 4));
                self.steer_still = 0;
            }
        }
        self.steer_tic = obs.tic;
        self.steer_way = best.map(|b| b.1);
        self.steer_way
    }
}
