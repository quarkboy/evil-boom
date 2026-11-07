//! Where to move while fighting: every way round scored for room to run, being at the right
//! distance from each monster (her target's window, the others' standoffs), going across their
//! line of fire, not ending up between them, and keeping on the way she's going.

use super::aiming::{standoff, window};
use super::{mon, spread, Combat};
use crate::geom::{dist, Point};
use crate::los::Body;
use crate::motor::{Observation, Thing};
use crate::navigator::{MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
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
const W_EXPOSED: f64 = 1.5;

/// still this long pushing one way: not that way for a while
const STUCK_TICS: i64 = 6;

/// an arch-vile's fire goes off this long into its attack (A_VileAttack, S_VILE_ATK10)
const VILE_BLAST: i32 = 66;

/// its blast goes off this far short of her, towards it...
const FIRE_SHORT: f64 = 24.0;
/// ...seeing from this high (the fire's)
const FIRE_HEIGHT: f64 = 16.0;

/// a hiding place this deep in an arch-vile's shadow, along the way there
const SHADOW: f64 = 48.0;

/// she covers about this much a tic, running
const RUN: f64 = 14.0;

impl Combat {
    /// An arch-vile in sight winding up its fire: the way to the nearest place it can't see her
    /// that she can get to before it goes off (behind a pillar), as (its name, the way). None if
    /// there's none (then she shoots: nothing else helps).
    pub(super) fn vile_fire(&mut self, obs: &Observation) -> Option<(&'static str, Option<Point>)> {
        let pos = obs.pos();
        let Some(vile) = self.against_her().into_iter().filter(|t| (0..VILE_BLAST).contains(&t.attack) && mon(t.name).attack == Attack::Fire).min_by_key(|t| t.attack) else {
            // (out of its sight now: stay there till it's gone off)
            if obs.tic >= self.vile_until {
                self.vile_way = None;
                return None;
            }
            return Some(("Archvile", self.vile_way.and_then(|(_, _, spot)| self.towards(obs, spot))));
        };
        let left = f64::from(VILE_BLAST - vile.attack - 2);
        self.vile_until = obs.tic + VILE_BLAST as i64 - vile.attack as i64 + 4;
        if let Some((_, id, spot)) = self.vile_way {
            if id == vile.id {
                return Some((vile.name, self.towards(obs, spot)));
            }
        }
        let eye = Body { x: vile.x, y: vile.y, z: vile.z, height: mon(vile.name).height };
        self.refresh_tic_cache(obs);
        let z = self.z(obs);
        let mut best: Option<(f64, Point)> = None;
        // (failing that, somewhere its blast can't reach her: it goes off 24 short of her
        // towards the arch-vile, and hurts only what it can see from there: pressed against
        // something low between them, the blast's on the far side of it; only the 20 of the
        // hit itself lands)
        let mut hug: Option<(f64, Point)> = None;
        for i in 0..WAYS {
            let a = 2.0 * std::f64::consts::PI * i as f64 / WAYS as f64;
            let d = (a.cos(), a.sin());
            // (her middle stops her radius short of what ends the run)
            let room = self.clear_run(obs, d, left * RUN + PLAYER_RADIUS) - PLAYER_RADIUS;
            let mut far = 0.0;
            while far <= room {
                let p = (pos.0 + d.0 * far, pos.1 + d.1 * far);
                let me = Body { x: p.0, y: p.1, z, height: PLAYER_HEIGHT };
                if !self.fits(obs, p) {
                    far += 16.0;
                    continue;
                }
                if hug.is_none_or(|(hf, _)| far < hf) {
                    let to = dist(p, (vile.x, vile.y)).max(1.0);
                    let blast = Body { x: p.0 + (vile.x - p.0) / to * FIRE_SHORT, y: p.1 + (vile.y - p.1) / to * FIRE_SHORT, z, height: FIRE_HEIGHT };
                    if !self.los.clear(blast, me, &self.heights_now) {
                        hug = Some((far, p));
                    }
                }
                if far > 0.0 && !self.los.clear(eye, me, &self.heights_now) {
                    // (well into its shadow, not on the edge of it: stopped just past a
                    // pillar's corner, it still saw her)
                    let deep = far + SHADOW;
                    let inside = deep <= room && {
                        let q = (pos.0 + d.0 * deep, pos.1 + d.1 * deep);
                        !self.los.clear(eye, Body { x: q.0, y: q.1, z, height: PLAYER_HEIGHT }, &self.heights_now)
                    };
                    if inside {
                        let spot = (pos.0 + d.0 * (far + SHADOW / 2.0), pos.1 + d.1 * (far + SHADOW / 2.0));
                        if best.is_none_or(|(bf, _)| far < bf) {
                            best = Some((far, spot));
                        }
                        break;
                    }
                }
                far += 16.0;
            }
        }
        let Some((_, spot)) = best.or(hug) else {
            self.vile_until = 0; // (nowhere to hide: fight on)
            return None;
        };
        self.vile_way = Some((obs.tic, vile.id, spot));
        self.log.push(format!(
            "tic {}: hiding from {} {} at ({:.0}, {:.0}), {} tics in: to ({:.0}, {:.0}){}",
            obs.tic,
            vile.name,
            vile.id,
            vile.x,
            vile.y,
            vile.attack,
            spot.0,
            spot.1,
            if best.is_none() { " (its blast blocked there)" } else { "" }
        ));
        Some((vile.name, self.towards(obs, spot)))
    }

    /// Is there room for her at p (all of her, not only her middle)?
    fn fits(&self, obs: &Observation, p: Point) -> bool {
        let nav = &self.nav;
        let Some(at) = nav.poly_in(p) else { return false };
        let floor = obs.height(nav.sector_of_poly(at)).map_or(0.0, |h| h.floor);
        // (her box is square, the game's way: its corners and sides)
        let r = PLAYER_RADIUS + 1.0;
        [(-r, -r), (0.0, -r), (r, -r), (r, 0.0), (r, r), (0.0, r), (-r, r), (-r, 0.0)].into_iter().all(|(dx, dy)| {
            let q = (p.0 + dx, p.1 + dy);
            nav.poly_in(q).and_then(|r| obs.height(nav.sector_of_poly(r))).is_some_and(|h| (h.floor - floor).abs() <= MAX_STEP && h.ceiling - h.floor >= PLAYER_HEIGHT)
        })
    }

    /// The way to `spot` (None once there: stopping).
    fn towards(&self, obs: &Observation, spot: Point) -> Option<Point> {
        let d = dist(spot, obs.pos());
        (d > 12.0).then(|| ((spot.0 - obs.x) / d, (spot.1 - obs.y) / d))
    }

    /// The way to move fighting `target` with `gun` (None: nowhere safe).
    pub(super) fn steer(&mut self, obs: &Observation, target: Option<&Thing>, gun: &Weapon) -> Option<Point> {
        let pos = obs.pos();
        let mut them = self.against_her();
        if let Some(t) = target.filter(|t| !them.iter().any(|m| m.id == t.id)) {
            them.push(t.clone()); // (out of sight just now: where it was)
        }
        let shooting = target.is_some_and(|t| matches!(mon(t.name).attack, Attack::Missile | Attack::Hitscan));
        let spread_now = spread(pos, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
        let hitscanners: Vec<Thing> = them.iter().filter(|m| mon(m.name).attack == Attack::Hitscan).cloned().collect();
        self.refresh_tic_cache(obs);
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
            // (hitscanners besides her target that'd see her there: one at a time from cover, not
            // all of them at once)
            let ahead_body = Body { x: ahead.0, y: ahead.1, z: self.z(obs), height: PLAYER_HEIGHT };
            for m in hitscanners.iter().filter(|m| Some(m.id) != target.map(|t| t.id)) {
                let eye = Body { x: m.x, y: m.y, z: m.z, height: mon(m.name).height };
                if self.los.clear(eye, ahead_body, &self.heights_now) {
                    s -= W_EXPOSED * mon(m.name).threat / 4.0;
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
