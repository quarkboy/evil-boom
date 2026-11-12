// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Where to move while fighting: every way round scored for room to run, being at the right
//! distance from each monster (her target's window, the others' standoffs), going across their
//! line of fire, not ending up between them, and keeping on the way she's going.

use super::sight::eye;
use super::style::{standoff, window};
use super::{mon, segment_point_distance, spread, Combat};
use crate::geom::{dist, Point};
use crate::los::Body;
use crate::motor::{Observation, Thing};
use crate::navigator::{MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::tactical::bestiary::{claw_max, first_shot, weapon_in, Attack, Weapon};

/// the ways round her looked along
pub(super) const WAYS: usize = 16;

/// a claw's reach, centre to centre: MELEERANGE (64) less 20 plus her radius (p_enemy.c
/// `P_CheckMeleeRange`), and a stride of it (8-10) on top: 60 + 20
pub(super) const CLAW_REACH: f64 = 80.0;

/// an arch-vile's fire goes off this long into its attack (A_VileAttack, S_VILE_ATK10)
pub(super) const VILE_BLAST: i32 = 66;

/// its blast goes off this far short of her, towards it...
const FIRE_SHORT: f64 = 24.0;
/// ...seeing from this high (the fire's)
const FIRE_HEIGHT: f64 = 16.0;

/// she covers about this much a tic, running
const RUN: f64 = 14.0;

/// two ways are against each other when the cosine between them is under this (over about 107
/// degrees apart)
const ABOUT: f64 = -0.3;

impl Combat {
    /// An arch-vile in sight winding up its fire: the way to the nearest place it can't see her
    /// that she can get to before it goes off (behind a pillar), as (its name, the way). None if
    /// there's none (then she shoots: nothing else helps).
    pub(super) fn vile_fire(&mut self, obs: &Observation) -> Option<(&'static str, Option<Point>)> {
        let pos = obs.pos();
        let Some(vile) = self
            .now
            .against_her
            .iter()
            .filter(|t| (0..VILE_BLAST).contains(&t.attack) && mon(t.name).attack == Attack::Fire)
            .min_by_key(|t| t.attack)
            .cloned()
        else {
            // Out of its sight now: stay there till it has gone off.
            if obs.tic >= self.cover.vile_until {
                self.cover.vile_way = None;
                return None;
            }
            return Some((
                "Archvile",
                self.cover
                    .vile_way
                    .and_then(|(_, _, spot)| self.towards(obs, spot)),
            ));
        };
        let left = f64::from(VILE_BLAST - vile.attack - 2);
        self.cover.vile_until = obs.tic + VILE_BLAST as i64 - vile.attack as i64 + 4;
        if let Some((_, id, spot)) = self.cover.vile_way {
            if id == vile.id {
                return Some((vile.name, self.towards(obs, spot)));
            }
        }
        let best = self.shadow_spot(obs, eye(&vile), left * RUN);
        let z = self.z(obs);
        // Failing that, somewhere its blast can't reach her. The blast goes off 24 short of her
        // towards the arch-vile and hurts only what it can see from there, so pressed against
        // something low between them she takes only the 20 of the hit itself.
        let mut hug: Option<(f64, Point)> = None;
        for i in 0..WAYS {
            let a = 2.0 * std::f64::consts::PI * i as f64 / WAYS as f64;
            let d = (a.cos(), a.sin());
            // Her middle stops her radius short of what ends the run.
            let room = self.clear_run(obs, d, left * RUN + PLAYER_RADIUS) - PLAYER_RADIUS;
            let mut far = 0.0;
            while far <= room {
                let p = (pos.0 + d.0 * far, pos.1 + d.1 * far);
                let me = Body {
                    x: p.0,
                    y: p.1,
                    z,
                    height: PLAYER_HEIGHT,
                };
                if !self.fits(obs, p) {
                    far += 16.0;
                    continue;
                }
                if far <= self.tune.hide.shelter_near && hug.is_none_or(|(hf, _)| far < hf) {
                    let to = dist(p, (vile.x, vile.y)).max(1.0);
                    let blast = Body {
                        x: p.0 + (vile.x - p.0) / to * FIRE_SHORT,
                        y: p.1 + (vile.y - p.1) / to * FIRE_SHORT,
                        z,
                        height: FIRE_HEIGHT,
                    };
                    if !self.los.clear(blast, me, &self.heights_now) {
                        hug = Some((far, p));
                    }
                }
                far += 16.0;
            }
        }
        // The blast shelter is taken only at the last moment and close by: going for it sooner, or
        // further, costs more shooting than it saves.
        let hug = hug.filter(|&(far, _)| left <= self.tune.hide.shelter_late + far / RUN);
        let Some((_, spot)) = best.or(hug) else {
            self.cover.vile_until = 0; // (nowhere to hide: fight on)
            return None;
        };
        self.cover.vile_way = Some((obs.tic, vile.id, spot));
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
            if best.is_none() {
                " (its blast blocked there)"
            } else {
                ""
            }
        ));
        Some((vile.name, self.towards(obs, spot)))
    }

    /// A hitscan shooter in sight winding up at her, or firing, in a fight that will take a while
    /// (not over in `DUCK_FIGHT` seconds with the gun up): the way to the nearest place it can't
    /// see her that she can get to before its first shot (or soon, if it's firing already), as
    /// (its name, the way). She is held there till its fire would have broken off: it fires on
    /// only while it sees her. This is a player's peek and duck at a corner; the fight's own
    /// "keeping at it" brings her out again.
    pub(super) fn duck(&mut self, obs: &Observation) -> Option<(&'static str, Option<Point>)> {
        let w = weapon_in(obs.weapon.unwrap_or(2), obs);
        let pos = obs.pos();
        let shooter = self
            .now
            .against_her
            .iter()
            .filter(|t| first_shot(t.name).is_some() && t.attack >= 0)
            .filter(|t| self.time_to_kill(&w, t, dist((t.x, t.y), pos)) > self.tune.hide.duck_fight)
            .min_by_key(|t| t.attack)
            .cloned();
        let Some(t) = shooter else {
            if obs.tic >= self.cover.duck_until {
                self.cover.duck_way = None;
                return None;
            }
            return self
                .cover
                .duck_way
                .map(|(_, name, spot)| (name, self.towards(obs, spot)));
        };
        let first = first_shot(t.name).unwrap_or(0);
        let to_shot = (first - t.attack).max(0);
        let left = f64::from(to_shot).max(self.tune.hide.duck_firing) - 2.0;
        let spot = match self.cover.duck_way {
            Some((id, _, spot)) if id == t.id => spot,
            _ => {
                let Some((_, spot)) = self.shadow_spot(obs, eye(&t), left * RUN) else {
                    self.cover.duck_way = None;
                    self.cover.no_duck_tic = obs.tic;
                    if t.attack == 0 || t.attack == first {
                        self.log.push(format!(
                            "tic {}: nowhere to duck from {} {} ({} tics in, {:.0} to get there)",
                            obs.tic,
                            t.name,
                            t.id,
                            t.attack,
                            left * RUN
                        ));
                    }
                    return None; // (nowhere to duck: fight on)
                };
                self.cover.duck_way = Some((t.id, t.name, spot));
                spot
            }
        };
        // Not till the last moment: its wind-up is the time to shoot it, out in the open. She goes
        // with just the time to get to the spot before its first shot.
        let going = obs.tic < self.cover.duck_until
            || to_shot == 0
            || f64::from(to_shot) <= dist(spot, pos) / RUN + self.tune.hide.duck_late;
        if !going {
            return None;
        }
        if obs.tic >= self.cover.duck_until {
            self.log.push(format!(
                "tic {}: ducking from {} {}, {} tics in: to ({:.0}, {:.0})",
                obs.tic, t.name, t.id, t.attack, spot.0, spot.1
            ));
        }
        self.cover.duck_until = obs.tic + i64::from(to_shot) + self.tune.hide.duck_hold;
        Some((t.name, self.towards(obs, spot)))
    }

    /// A homing missile dodged so long that it is going round her (dodged, it comes round again):
    /// the way to the nearest place it can't see her, round a corner it will fly into on its way
    /// after her. Only once nothing else in sight is after her. `None`: no such missile, or
    /// nowhere out of its sight in reach.
    pub(super) fn shake_off(
        &mut self,
        obs: &Observation,
        missiles: &[Thing],
    ) -> Option<(&'static str, Option<Point>)> {
        // Only with nothing else in sight after her. In a fight it is dodged as it comes: running
        // off round a corner would take her into the others' fire.
        if !self.now.about.is_empty() {
            return None;
        }
        let t = missiles.iter().find(|t| {
            self.dodge
                .homing_since
                .get(&t.id)
                .is_some_and(|&since| obs.tic - since >= self.tune.hide.shake_after)
        })?;
        let eye = Body {
            x: t.x,
            y: t.y,
            z: t.z,
            height: 8.0,
        };
        let held = self
            .dodge
            .shake_spot
            .filter(|&(id, _)| id == t.id)
            .map(|(_, spot)| spot);
        let spot = match held {
            Some(spot) => spot,
            None => {
                let spot = self
                    .shadow_spot(obs, eye, self.tune.hide.shake_reach)
                    .map(|(_, spot)| spot);
                match spot {
                    Some(p) => self.log.push(format!(
                        "tic {}: shaking off {} {}: to ({:.0}, {:.0})",
                        obs.tic, t.name, t.id, p.0, p.1
                    )),
                    None => self.log.push(format!(
                        "tic {}: shaking off {} {} round a corner",
                        obs.tic, t.name, t.id
                    )),
                }
                self.dodge.shake_spot = Some((t.id, spot));
                spot
            }
        };
        if let Some(spot) = spot {
            return Some((t.name, self.towards(obs, spot)));
        }
        // Nowhere straight off: round a corner by the mesh, worked out afresh as it comes after her.
        Some((t.name, Some(self.hidden_way(obs, eye)?)))
    }

    /// Sheltering from an arch-vile's blast rather than out of its sight: where it is, to face
    /// (the fire's put by her facing; turned away, the shelter fails).
    pub(super) fn vile_face(&self) -> Option<Point> {
        let (_, id, _) = self.cover.vile_way?;
        // Still in its sight: sheltering from the blast, not hidden.
        self.visible.iter().find(|t| t.id == id).map(|v| (v.x, v.y))
    }

    /// Is there room for her at p (all of her, not only her middle)?
    pub(super) fn fits(&self, obs: &Observation, p: Point) -> bool {
        let nav = &self.nav;
        let Some(at) = nav.poly_in(p) else {
            return false;
        };
        let floor = obs.height(nav.sector_of_poly(at)).map_or(0.0, |h| h.floor);
        // Her box is square, as the game has it: its corners and sides.
        let r = PLAYER_RADIUS + 1.0;
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
        .into_iter()
        .all(|(dx, dy)| {
            let q = (p.0 + dx, p.1 + dy);
            nav.poly_in(q)
                .and_then(|r| obs.height(nav.sector_of_poly(r)))
                .is_some_and(|h| {
                    (h.floor - floor).abs() <= MAX_STEP && h.ceiling - h.floor >= PLAYER_HEIGHT
                })
        })
    }

    /// The way to `spot` (None once there: stopping).
    fn towards(&self, obs: &Observation, spot: Point) -> Option<Point> {
        let d = dist(spot, obs.pos());
        (d > 12.0).then(|| ((spot.0 - obs.x) / d, (spot.1 - obs.y) / d))
    }

    /// The way to move fighting `target` with `gun` (None: nowhere safe).
    pub(super) fn steer(
        &mut self,
        obs: &Observation,
        target: Option<&Thing>,
        gun: &Weapon,
    ) -> Option<Point> {
        let pos = obs.pos();
        let mut them = self.now.about.clone();
        if let Some(t) = target.filter(|t| !them.iter().any(|m| m.id == t.id)) {
            them.push(t.clone()); // (out of sight just now: where it was)
        }
        let shooting =
            target.is_some_and(|t| matches!(mon(t.name).attack, Attack::Missile | Attack::Hitscan));
        let spread_now = spread(pos, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
        let hurting = self.on_hurting_floor(obs);
        let shooters = them
            .iter()
            .any(|m| matches!(mon(m.name).attack, Attack::Hitscan | Attack::Missile));
        let crushed = self.under_crusher(obs);
        let hitscanners: Vec<Thing> = them
            .iter()
            .filter(|m| mon(m.name).attack == Attack::Hitscan)
            .cloned()
            .collect();
        // A shotgun reloading: out of her target's sight too till it's ready, then back.
        let reloading = matches!(obs.weapon, Some(3))
            && self.trigger.last_shot.is_some_and(|(s, tic)| {
                s == 3 && ((obs.tic - tic) as f64) < gun.cycle - self.tune.steer.reload_back
            });
        self.refresh_tic_cache(obs);
        // Not towards an errand that lies in the claws' reach of something one hit of which could
        // kill her.
        let health = self.now.health;
        let errand_way = self.going_to.filter(|&to| {
            dist(to, pos) > 1.0
                && !them
                    .iter()
                    .any(|m| claw_max(m.name) >= health && dist(to, (m.x, m.y)) < CLAW_REACH)
        });
        // Where she took this target on: her steps to the side are kept about there. Begun afresh
        // for another target, or after a second of anything else.
        let from = target
            .filter(|_| shooting && self.tune.steer.side_span > 0.0)
            .map(|t| {
                let from = self
                    .steer
                    .from
                    .filter(|&(id, _, tic)| id == t.id && obs.tic - tic <= 35)
                    .map_or(pos, |f| f.1);
                self.steer.from = Some((t.id, from, obs.tic));
                let far = dist((t.x, t.y), from).max(1.0);
                (from, ((t.x - from.0) / far, (t.y - from.1) / far))
            });
        let mut best: Option<(f64, Point)> = None;
        // (the way she's on, if it's still one she could take, and what it scores now)
        let mut kept: Option<(f64, Point)> = None;
        for i in 0..WAYS {
            let a = 2.0 * std::f64::consts::PI * i as f64 / WAYS as f64;
            let d = (a.cos(), a.sin());
            if !self.safe(obs, d, 64.0) {
                continue;
            }
            if self
                .steer
                .stuck
                .is_some_and(|(w, until)| obs.tic < until && d.0 * w.0 + d.1 * w.1 > 0.7)
            {
                continue; // (pushed that way and got nowhere)
            }
            let step = (
                pos.0 + d.0 * self.tune.steer.body_look,
                pos.1 + d.1 * self.tune.steer.body_look,
            );
            if them.iter().any(|m| {
                segment_point_distance(pos, step, (m.x, m.y)) < mon(m.name).radius + PLAYER_RADIUS
                    && dist((m.x, m.y), step) < dist((m.x, m.y), pos)
            }) {
                continue; // (a monster's body in the way: they're solid)
            }
            let room = self.clear_run(obs, d, self.tune.steer.room);
            let ahead = (
                pos.0 + d.0 * self.tune.steer.look.min(room),
                pos.1 + d.1 * self.tune.steer.look.min(room),
            );
            let mut s = self.tune.steer.w_room * room / self.tune.steer.room
                + self.tune.steer.w_open * self.openness(obs, ahead, self.tune.steer.open);
            if hurting && self.gets_off(obs, d, self.tune.steer.look.min(room)) {
                s += self.tune.steer.w_off_floor; // (off the floor that's hurting her, before anything)
            }
            // Not by a barrel with anything shooting in sight, or with her shooting at something: a
            // shot at her, past her, or one of her own sets it off.
            if shooters || target.is_some() {
                for b in obs.barrels.iter().flatten() {
                    let gap = crate::motor::follower::blast_gap(ahead, *b);
                    if gap < self.tune.steer.barrel_keep {
                        s -= self.tune.steer.w_barrel * (self.tune.steer.barrel_keep - gap)
                            / self.tune.steer.barrel_keep;
                    }
                }
            }
            if crushed && self.gets_out(obs, d, self.tune.steer.look.min(room).max(48.0)) {
                s += 2.0 * self.tune.steer.w_off_floor; // (out from under a crusher, before even that)
            }
            for m in &them {
                let info = mon(m.name);
                let w = (info.threat + 1.0).sqrt();
                let at = dist(ahead, (m.x, m.y));
                let (near, far) = if Some(m.id) == target.map(|t| t.id) {
                    window(gun, m, self.tune.steer.near_by_gun)
                } else {
                    (standoff(m.name), f64::INFINITY)
                };
                if at < near {
                    s -= self.tune.steer.w_near * w * (near - at) / self.tune.steer.look;
                }
                if at > far {
                    s -= self.tune.steer.w_far * (at - far) / self.tune.steer.look;
                }
            }
            // Hitscanners besides her target that would see her there: one at a time from cover, not
            // all of them at once.
            let z = self.z(obs);
            for m in hitscanners
                .iter()
                .filter(|m| reloading || Some(m.id) != target.map(|t| t.id))
            {
                if self.sees(eye(m), ahead, z, &self.heights_now) {
                    s -= self.tune.steer.w_exposed * mon(m.name).threat / 4.0;
                }
            }
            if let Some(t) = target.filter(|_| shooting) {
                let to = (
                    (t.x - pos.0) / dist((t.x, t.y), pos).max(1.0),
                    (t.y - pos.1) / dist((t.x, t.y), pos).max(1.0),
                );
                s += self.tune.steer.w_across * (d.0 * to.1 - d.1 * to.0).abs();
            }
            if them.len() >= 2 {
                let after = spread(ahead, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
                s -= self.tune.steer.w_surround * ((after - spread_now) / 90.0).max(0.0)
                    + self.tune.steer.w_surround * (after / 360.0);
            }
            if let Some(k) = self.steer.way {
                s += self.tune.steer.w_keep * (d.0 * k.0 + d.1 * k.1);
            }
            if let Some((from, to)) = from {
                let aside = |p: Point| ((p.0 - from.0) * to.1 - (p.1 - from.1) * to.0).abs();
                // Only a way that takes her further out: back in costs nothing.
                let over = aside(step) - self.tune.steer.side_span;
                if over > 0.0 && aside(step) > aside(pos) {
                    s -= self.tune.steer.w_span * over / self.tune.steer.look;
                }
            }
            // Onto ground she's never been on: what's there she doesn't know, and it wakes.
            if self.tune.steer.w_new_ground > 0.0 && (self.off_known(step) || self.off_known(ahead))
            {
                s -= self.tune.steer.w_new_ground;
            }
            // A way she hasn't looked along lately: what's there she doesn't know.
            if self.tune.steer.w_unlooked > 0.0
                && obs.tic - self.senses.looked[i] > self.tune.steer.unlooked_tics
            {
                s -= self.tune.steer.w_unlooked;
            }
            // From there, facing her target: what she knows is about and would have behind her.
            if let Some(t) = target.filter(|_| self.tune.steer.w_out_of_view > 0.0) {
                let facing = crate::motor::heading(ahead, (t.x, t.y));
                let hidden: f64 = them
                    .iter()
                    .filter(|m| {
                        m.id != t.id
                            && crate::motor::norm(crate::motor::heading(ahead, (m.x, m.y)) - facing)
                                .abs()
                                > self.tune.senses.fov
                    })
                    .map(|m| mon(m.name).threat)
                    .sum();
                s -= self.tune.steer.w_out_of_view * hidden / 4.0;
            }
            // The way she's going on an errand (health, a gun, ammo): backing off towards it, not round
            // and round where she is.
            if let Some(to) = errand_way {
                s += self.tune.steer.w_toward * (d.0 * (to.0 - pos.0) + d.1 * (to.1 - pos.1))
                    / dist(to, pos);
            }
            if self.steer.way.is_some_and(|k| d.0 * k.0 + d.1 * k.1 > 0.99) {
                kept = Some((s, d));
            }
            if best.is_none_or(|(bs, _)| s > bs) {
                best = Some((s, d));
            }
        }
        // Turning about (the best way more or less against the one she's on) so soon after the
        // last time, for a way only a little better: she keeps the way she's on. A player's
        // steps to the side run a while each way; the best of sixteen ways scored afresh every
        // tic flips on a hair, and darts her to and fro.
        if let (Some((bs, b)), Some((ks, k))) = (best, kept) {
            if b.0 * k.0 + b.1 * k.1 < ABOUT {
                if obs.tic - self.steer.turned_about < self.tune.steer.about_tics
                    && bs - ks < self.tune.steer.about_margin
                {
                    best = kept;
                } else {
                    self.steer.turned_about = obs.tic;
                }
            }
        }
        // Pushing a way and not moving: caught on something the floor doesn't show.
        if let Some(k) = self.steer.way {
            if self.velocity.0.hypot(self.velocity.1) < 1.0 && self.steer.tic == obs.tic - 1 {
                self.steer.still += 1;
            } else {
                self.steer.still = 0;
            }
            if self.steer.still > self.tune.steer.stuck_tics {
                self.steer.stuck = Some((k, obs.tic + self.tune.steer.stuck_tics * 4));
                self.steer.still = 0;
                self.bumped(obs, k.1.atan2(k.0).to_degrees());
            }
        }
        self.steer.tic = obs.tic;
        self.steer.way = best.map(|b| b.1);
        self.steer.way
    }
}
