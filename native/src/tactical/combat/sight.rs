//! Who can see her where: the one question behind every way she has of getting out of something's
//! sight (an arch-vile's fire, a hitscanner's burst, a homing missile, what outmatches her; cover,
//! a post, backing out, the ground a sniper watches). `sees` is the question; `shadow_spot` and
//! `hidden_way` are the two searches the fight makes with it from where she stands (straight
//! runs; round a corner by the mesh). The player's searches over the places she knows (cover, a
//! post, the way back) ask the same `sees`.

use super::steering::WAYS;
use super::{mon, Combat, Seen};
use crate::geom::Point;
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

    /// Out of the sight of a hitscan shooter she's no shot of hers to spend on: the way round a
    /// corner to where it can't see her (None: nowhere in reach). Its bullets land the same at any
    /// range, so more distance in its sight keeps her no safer.
    pub(super) fn out_of_sight_of(&mut self, obs: &Observation, t: &Thing) -> Option<Point> {
        self.hidden_way(obs, eye(t))
    }

}
