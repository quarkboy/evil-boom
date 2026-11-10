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

impl Combat {
    /// An arch-vile in sight winding up its fire: the way to the nearest place it can't see her
    /// that she can get to before it goes off (behind a pillar), as (its name, the way). None if
    /// there's none (then she shoots: nothing else helps).
    pub(super) fn vile_fire(&mut self, obs: &Observation) -> Option<(&'static str, Option<Point>)> {
        let pos = obs.pos();
        let Some(vile) = self.now.against_her.iter().filter(|t| (0..VILE_BLAST).contains(&t.attack) && mon(t.name).attack == Attack::Fire).min_by_key(|t| t.attack).cloned() else {
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
        let best = self.shadow_spot(obs, eye(&vile), left * RUN);
        let z = self.z(obs);
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
                if far <= self.tune.hide.shelter_near && hug.is_none_or(|(hf, _)| far < hf) {
                    let to = dist(p, (vile.x, vile.y)).max(1.0);
                    let blast = Body { x: p.0 + (vile.x - p.0) / to * FIRE_SHORT, y: p.1 + (vile.y - p.1) / to * FIRE_SHORT, z, height: FIRE_HEIGHT };
                    if !self.los.clear(blast, me, &self.heights_now) {
                        hug = Some((far, p));
                    }
                }
                far += 16.0;
            }
        }
        // (the blast shelter only at the last moment and close by: going for it sooner, or
        // further, cost more shooting than it saved, as Astra's branch measured)
        let hug = hug.filter(|&(far, _)| left <= self.tune.hide.shelter_late + far / RUN);
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

    /// A hitscan shooter in sight winding up at her, or firing, in a fight that'll take a while
    /// (not over in `DUCK_FIGHT` seconds with the gun up): the way to the nearest place it can't
    /// see her that she can get to before its first shot (or soon, already firing), as (its name,
    /// the way); and held there till its fire would have broken off (it fires on only while it
    /// sees her). A player's peek and duck at a pillar's corner: E3M8, the spider mastermind's
    /// 20 tics of turning to her, then bursts at any range, stood up to in the open; 4/4 seeds
    /// dead with it at 600-2000 of its 3000. Then the fight's own "keeping at it" brings her out.
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
            if obs.tic >= self.duck_until {
                self.duck_way = None;
                return None;
            }
            return self.duck_way.map(|(_, name, spot)| (name, self.towards(obs, spot)));
        };
        let first = first_shot(t.name).unwrap_or(0);
        let to_shot = (first - t.attack).max(0);
        let left = f64::from(to_shot).max(self.tune.hide.duck_firing) - 2.0;
        let spot = match self.duck_way {
            Some((id, _, spot)) if id == t.id => spot,
            _ => {
                let Some((_, spot)) = self.shadow_spot(obs, eye(&t), left * RUN) else {
                    self.duck_way = None;
                    self.no_duck_tic = obs.tic;
                    if t.attack == 0 || t.attack == first {
                        self.log.push(format!("tic {}: nowhere to duck from {} {} ({} tics in, {:.0} to get there)", obs.tic, t.name, t.id, t.attack, left * RUN));
                    }
                    return None; // (nowhere to duck: fight on)
                };
                self.duck_way = Some((t.id, t.name, spot));
                spot
            }
        };
        // (not till the last moment: its wind-up's the time to shoot it, out in the open; off
        // to the spot with just the time to get there before its first shot. Off at once, it
        // got next to no shooting done: MAP16's cover, 6/8 lost or drawn, all won before)
        let going = obs.tic < self.duck_until || to_shot == 0 || f64::from(to_shot) <= dist(spot, pos) / RUN + self.tune.hide.duck_late;
        if !going {
            return None;
        }
        if obs.tic >= self.duck_until {
            self.log.push(format!("tic {}: ducking from {} {}, {} tics in: to ({:.0}, {:.0})", obs.tic, t.name, t.id, t.attack, spot.0, spot.1));
        }
        self.duck_until = obs.tic + i64::from(to_shot) + self.tune.hide.duck_hold;
        Some((t.name, self.towards(obs, spot)))
    }

    /// A homing missile dodged so long it's going round her (dodged, it comes round again, and
    /// again: MAP10, four revenant tracers bunched into one, dodged for the rest of the hour):
    /// the way to the nearest place it can't see her, round a corner it flies into on its way
    /// after her, as a player shakes one off, once nothing else in sight is after her. None: none,
    /// or nowhere out of its sight in reach.
    pub(super) fn shake_off(&mut self, obs: &Observation, missiles: &[Thing]) -> Option<(&'static str, Option<Point>)> {
        // (with nothing else in sight after her: in a fight, dodged as it comes, running off round
        // a corner from the revenant still shooting walked her into the arachnotron's plasma)
        if !self.now.against_her.is_empty() {
            return None;
        }
        let t = missiles.iter().find(|t| self.homing_since.get(&t.id).is_some_and(|&since| obs.tic - since >= self.tune.hide.shake_after))?;
        let eye = Body { x: t.x, y: t.y, z: t.z, height: 8.0 };
        let held = self.shake_spot.filter(|&(id, _)| id == t.id).map(|(_, spot)| spot);
        let spot = match held {
            Some(spot) => spot,
            None => {
                let spot = self.shadow_spot(obs, eye, self.tune.hide.shake_reach).map(|(_, spot)| spot);
                match spot {
                    Some(p) => self.log.push(format!("tic {}: shaking off {} {}: to ({:.0}, {:.0})", obs.tic, t.name, t.id, p.0, p.1)),
                    None => self.log.push(format!("tic {}: shaking off {} {} round a corner", obs.tic, t.name, t.id)),
                }
                self.shake_spot = Some((t.id, spot));
                spot
            }
        };
        if let Some(spot) = spot {
            return Some((t.name, self.towards(obs, spot)));
        }
        // (nowhere straight off: round a corner by the mesh; a big open room's way out, worked
        // out afresh as it comes after her)
        Some((t.name, Some(self.hidden_way(obs, eye)?)))
    }

    /// Sheltering from an arch-vile's blast rather than out of its sight: where it is, to face
    /// (the fire's put by her facing; turned away, the shelter fails).
    pub(super) fn vile_face(&self) -> Option<Point> {
        let (_, id, _) = self.vile_way?;
        // (still in its sight: sheltering from the blast, not hidden)
        self.visible.iter().find(|t| t.id == id).map(|v| (v.x, v.y))
    }

    /// Is there room for her at p (all of her, not only her middle)?
    pub(super) fn fits(&self, obs: &Observation, p: Point) -> bool {
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
        let mut them = self.now.against_her.clone();
        if let Some(t) = target.filter(|t| !them.iter().any(|m| m.id == t.id)) {
            them.push(t.clone()); // (out of sight just now: where it was)
        }
        let shooting = target.is_some_and(|t| matches!(mon(t.name).attack, Attack::Missile | Attack::Hitscan));
        let spread_now = spread(pos, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
        let hurting = self.on_hurting_floor(obs);
        let shooters = them.iter().any(|m| matches!(mon(m.name).attack, Attack::Hitscan | Attack::Missile));
        let crushed = self.under_crusher(obs);
        let hitscanners: Vec<Thing> = them.iter().filter(|m| mon(m.name).attack == Attack::Hitscan).cloned().collect();
        // (a shotgun reloading: out of her target's sight too till it's ready, then back; Astra's
        // branch's reload cover)
        let reloading = matches!(obs.weapon, Some(3)) && self.last_shot.is_some_and(|(s, tic)| s == 3 && ((obs.tic - tic) as f64) < gun.cycle - self.tune.steer.reload_back);
        self.refresh_tic_cache(obs);
        // (not towards an errand in the claws' reach of something one hit of which could kill her:
        // E2M3, 35 health, out of ammo, the shell she backed off towards 50 from a baron, 10-80 a
        // hit, kept her by it till it hit)
        let health = self.now.health;
        let errand_way = self.toward.filter(|&to| dist(to, pos) > 1.0 && !them.iter().any(|m| claw_max(m.name) >= health && dist(to, (m.x, m.y)) < CLAW_REACH));
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
            let step = (pos.0 + d.0 * self.tune.steer.body_look, pos.1 + d.1 * self.tune.steer.body_look);
            if them.iter().any(|m| segment_point_distance(pos, step, (m.x, m.y)) < mon(m.name).radius + PLAYER_RADIUS && dist((m.x, m.y), step) < dist((m.x, m.y), pos)) {
                continue; // (a monster's body in the way: they're solid)
            }
            let room = self.clear_run(obs, d, self.tune.steer.room);
            let ahead = (pos.0 + d.0 * self.tune.steer.look.min(room), pos.1 + d.1 * self.tune.steer.look.min(room));
            let mut s = self.tune.steer.w_room * room / self.tune.steer.room + self.tune.steer.w_open * self.openness(obs, ahead, self.tune.steer.open);
            if hurting && self.gets_off(obs, d, self.tune.steer.look.min(room)) {
                s += self.tune.steer.w_off_floor; // (off the floor that's hurting her, before anything)
            }
            // (not by a barrel with anything shooting in sight: a shot at her, or past her, sets it
            // off; MAP02, fighting shotgun guys 35 from one, 76 off her in one go)
            // (and with her shooting at something: her shots set them off as well, E1M8's demons,
            // biters only, fought among its barrels)
            if shooters || target.is_some() {
                for b in obs.barrels.iter().flatten() {
                    let gap = crate::motor::follower::blast_gap(ahead, *b);
                    if gap < self.tune.steer.barrel_keep {
                        s -= self.tune.steer.w_barrel * (self.tune.steer.barrel_keep - gap) / self.tune.steer.barrel_keep;
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
                let (near, far) = if Some(m.id) == target.map(|t| t.id) { window(gun, m) } else { (standoff(m.name), f64::INFINITY) };
                if at < near {
                    s -= self.tune.steer.w_near * w * (near - at) / self.tune.steer.look;
                }
                if at > far {
                    s -= self.tune.steer.w_far * (at - far) / self.tune.steer.look;
                }
            }
            // (hitscanners besides her target that'd see her there: one at a time from cover, not
            // all of them at once)
            let z = self.z(obs);
            for m in hitscanners.iter().filter(|m| reloading || Some(m.id) != target.map(|t| t.id)) {
                if self.sees(eye(m), ahead, z, &self.heights_now) {
                    s -= self.tune.steer.w_exposed * mon(m.name).threat / 4.0;
                }
            }
            if let Some(t) = target.filter(|_| shooting) {
                let to = ((t.x - pos.0) / dist((t.x, t.y), pos).max(1.0), (t.y - pos.1) / dist((t.x, t.y), pos).max(1.0));
                s += self.tune.steer.w_across * (d.0 * to.1 - d.1 * to.0).abs();
            }
            if them.len() >= 2 {
                let after = spread(ahead, &them.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>());
                s -= self.tune.steer.w_surround * ((after - spread_now) / 90.0).max(0.0) + self.tune.steer.w_surround * (after / 360.0);
            }
            if let Some(k) = self.steer_way {
                s += self.tune.steer.w_keep * (d.0 * k.0 + d.1 * k.1);
            }
            // (the way she's going, an errand (health, a gun, ammo): backing off towards it, not
            // round and round where she is; the user's: 1% health, a cacodemon on her, the
            // medikit in the room never got to)
            if let Some(to) = errand_way {
                s += self.tune.steer.w_toward * (d.0 * (to.0 - pos.0) + d.1 * (to.1 - pos.1)) / dist(to, pos);
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
            if self.steer_still > self.tune.steer.stuck_tics {
                self.steer_stuck = Some((k, obs.tic + self.tune.steer.stuck_tics * 4));
                self.steer_still = 0;
                self.bumped(obs, k.1.atan2(k.0).to_degrees());
            }
        }
        self.steer_tic = obs.tic;
        self.steer_way = best.map(|b| b.1);
        self.steer_way
    }
}
