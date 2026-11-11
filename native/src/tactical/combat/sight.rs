//! Who can see her where: the one question behind every way she has of getting out of something's
//! sight (an arch-vile's fire, a hitscanner's burst, a homing missile, what outmatches her; cover,
//! a post, backing out, the ground a sniper watches). `sees` is the question; `shadow_spot` and
//! `hidden_way` are the two searches the fight makes with it from where she stands (straight
//! runs; round a corner by the mesh). The player's searches over the places she knows (cover, a
//! post, the way back) ask the same `sees`.

use super::steering::WAYS;
use super::{mon, Combat, Seen};
use crate::geom::{dist, Point};
use crate::los::Body;
use crate::motor::{Observation, Thing};
use crate::navigator::{PLAYER_HEIGHT, PLAYER_RADIUS};

/// Where a monster sees from: all of it.
pub fn eye(t: &Thing) -> Body {
    Body { x: t.x, y: t.y, z: t.z, height: mon(t.name).height }
}

/// ...one out of sight, from where she last saw it.
pub fn eye_of(s: &Seen) -> Body {
    Body { x: s.x, y: s.y, z: s.z, height: mon(s.name).height }
}

/// Her, standing at p on a floor at z.
pub fn standing(p: Point, z: f64) -> Body {
    Body { x: p.0, y: p.1, z, height: PLAYER_HEIGHT }
}

impl Combat {
    /// Would something seeing from `eye` see her standing at p, on a floor at z (the sectors'
    /// heights as given: this tic's)?
    pub fn sees(&self, eye: Body, p: Point, z: f64, heights: &[(f64, f64)]) -> bool {
        self.los.clear(eye, standing(p, z), heights)
    }

    /// The nearest place, `reach` or less away along one of the ways round, that something seeing
    /// from `eye` can't see her (well into its shadow: the spot half `SHADOW` past where it loses
    /// her, and all of `SHADOW` past still hidden), as (how far till hidden, the spot).
    pub(super) fn shadow_spot(&mut self, obs: &Observation, eye: Body, reach: f64) -> Option<(f64, Point)> {
        let pos = obs.pos();
        self.refresh_tic_cache(obs);
        let z = self.z(obs);
        let mut best: Option<(f64, Point)> = None;
        for i in 0..WAYS {
            let a = 2.0 * std::f64::consts::PI * i as f64 / WAYS as f64;
            let d = (a.cos(), a.sin());
            // (her middle stops her radius short of what ends the run)
            let room = self.clear_run(obs, d, reach + PLAYER_RADIUS) - PLAYER_RADIUS;
            let mut far = 16.0;
            while far <= room && best.is_none_or(|(bf, _)| far < bf) {
                let p = (pos.0 + d.0 * far, pos.1 + d.1 * far);
                let hidden = |c: &Self, q: Point| !c.sees(eye, q, z, &c.heights_now);
                if self.fits(obs, p) && hidden(self, p) {
                    // (well into its shadow, not on the edge of it: stopped just past a
                    // pillar's corner, it still saw her)
                    let deep = far + self.tune.hide.shadow;
                    if deep <= room && hidden(self, (pos.0 + d.0 * deep, pos.1 + d.1 * deep)) {
                        best = Some((far, (pos.0 + d.0 * (far + self.tune.hide.shadow / 2.0), pos.1 + d.1 * (far + self.tune.hide.shadow / 2.0))));
                        break;
                    }
                }
                far += 16.0;
            }
        }
        best
    }

    /// The way round a corner by the mesh, `SHAKE_FAR` or less along it, to the nearest place
    /// whose middle something seeing from `eye` can't see.
    pub(super) fn hidden_way(&mut self, obs: &Observation, eye: Body) -> Option<Point> {
        self.refresh_tic_cache(obs);
        let z = self.z(obs);
        let mesh = std::sync::Arc::clone(&self.nav.mesh);
        let hidden = |p: usize| {
            let c = mesh.poly(p).center;
            self.fits(obs, c) && !self.sees(eye, c, z, &self.heights_now)
        };
        let h = crate::motor::follower::nearest_heading(&mesh, obs, &hidden, self.tune.hide.shake_far)?.to_radians();
        Some((h.cos(), h.sin()))
    }

    /// Two bosses awake and about her, of two kinds, one with a gun (a spider mastermind and a
    /// cyberdemon): its bullets hit whatever's between it and her, and what's hit turns on what
    /// hit it. Some((the one with the gun, the other, where to)): where to, None with the two at
    /// each other already (neither turned to her); else Some(the way) to the nearest place in a
    /// line with the two, one of them between her and the other, in the far one's sight but for
    /// it (Some(None): there).
    /// None: no two such, or no such place.
    /// (MAP20 on the easiest skill, 16 runs of 16 dead in a minute at the start's porch: both
    /// let loose on her, kept clear of in their sight, the rockets' blasts off the walls by her;
    /// the two at 3500 of 4000 and 2800 of 3000 at her death, never set on each other. The
    /// user's: the level's made for it.)
    pub(super) fn set_on(&mut self, obs: &Observation) -> Option<(Thing, Thing, Option<Option<Point>>)> {
        /// how far past the one between she'd stand, nearest first
        const PAST: [f64; 5] = [256.0, 384.0, 512.0, 640.0, 768.0];
        /// how far a way to there is looked for
        const LOOK: f64 = 4096.0;
        let pos = obs.pos();
        let (gun, other) = self.boss_pair(obs)?;
        if !gun.after_her && !other.after_her {
            return Some((gun, other, None));
        }
        self.refresh_tic_cache(obs);
        let z = self.z(obs);
        // (in a line with the two, past either: past the other, the gun's bullets at her hit
        // it; past the gun, what the other throws at her hits that)
        let apart = dist((gun.x, gun.y), (other.x, other.y)).max(1.0);
        let u = ((other.x - gun.x) / apart, (other.y - gun.y) / apart);
        let thrower = matches!(mon(other.name).attack, super::Attack::Missile | super::Attack::Hitscan);
        let past_other = PAST.iter().map(|k| (other.x + u.0 * k, other.y + u.1 * k)).map(|p| (p, eye(&gun)));
        let past_gun = PAST.iter().filter(|_| thrower).map(|k| (gun.x - u.0 * k, gun.y - u.1 * k)).map(|p| (p, eye(&other)));
        let spot = past_other
            .chain(past_gun)
            .filter(|&(p, far)| self.fits(obs, p) && self.sees(far, p, z, &self.heights_now))
            .map(|(p, _)| p)
            .min_by(|a, b| dist(*a, pos).total_cmp(&dist(*b, pos)))?;
        if std::env::var_os("EVIL_DEBUG_SET_ON").is_some() && obs.tic % 35 == 0 {
            self.log.push(format!("tic {}: (she's at ({:.0}, {:.0}, {:.0}); the {} at ({:.0}, {:.0}, {:.0}) after her {}, the {} at ({:.0}, {:.0}, {:.0}) after her {}; the spot ({:.0}, {:.0}), {:.0} off)", obs.tic, pos.0, pos.1, z, gun.name, gun.x, gun.y, gun.z, gun.after_her, other.name, other.x, other.y, other.z, other.after_her, spot.0, spot.1, dist(spot, pos)));
        }
        let mesh = std::sync::Arc::clone(&self.nav.mesh);
        let (here, there) = (mesh.poly_at(pos), mesh.poly_at(spot)?);
        let d = dist(spot, pos);
        let way = if d < 16.0 {
            None
        } else if here == Some(there) {
            Some(((spot.0 - pos.0) / d, (spot.1 - pos.1) / d))
        } else {
            let h = crate::motor::follower::nearest_heading(&mesh, obs, &|p| p == there, LOOK)?.to_radians();
            Some((h.cos(), h.sin()))
        };
        Some((gun, other, Some(way)))
    }

    /// Two bosses awake and about her, of two kinds, one with a gun: (the one with the gun, the
    /// other, the nearest such).
    pub(super) fn boss_pair(&self, obs: &Observation) -> Option<(Thing, Thing)> {
        let pos = obs.pos();
        let bosses: Vec<&Thing> = self.now.about.iter().filter(|t| t.health > 0 && mon(t.name).boss && self.known.get(&t.id).is_some_and(|k| k.awake)).collect();
        let gun = *bosses.iter().find(|t| mon(t.name).attack == super::Attack::Hitscan)?;
        let other = *bosses.iter().filter(|t| t.name != gun.name).min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos)))?;
        Some((gun.clone(), other.clone()))
    }

    /// Out of the sight of a hitscan shooter she's no shot of hers to spend on: the way round a
    /// corner to where it can't see her (None: nowhere in reach). Its bullets land the same at any
    /// range, so more distance in its sight keeps her no safer.
    pub(super) fn out_of_sight_of(&mut self, obs: &Observation, t: &Thing) -> Option<Point> {
        self.hidden_way(obs, eye(t))
    }

}
