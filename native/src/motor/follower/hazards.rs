//! Waiting for crushers and escaping a ceiling that pins the player.

use super::RouteFollower;
use crate::geom::{dist, Point};
use crate::motor::{heading, Controls, Observation, FULL_SPEED};
use crate::navigator::{closest_on_portal, PLAYER_HEIGHT, PLAYER_RADIUS};
use std::collections::BTreeSet;

/// (mortal) a crusher this near ahead on the way is waited for at its edge...
const CRUSH_LOOK: f64 = 192.0;

/// ...till it's going up with this much more room than she needs
const CRUSH_ROOM: f64 = 8.0;

/// the longest wait under a ceiling too low for her before it counts as being stuck
const PINNED_WAIT: i64 = 10 * 35;

impl RouteFollower {
    /// Under a ceiling too low for her over some of the box (a crusher coming down, a door
    /// shutting on her): Doom won't move her till it's risen, so that's waiting, not being stuck
    /// (E3M4's crusher room, the bonuses in it, a dozen lives given up there); up to PINNED_WAIT.
    pub(super) fn pinned_under_ceiling(&mut self, obs: &Observation) -> Option<Controls> {
        let mesh = &self.view.nav.mesh;
        let r = PLAYER_RADIUS - 1.0;
        let secs: BTreeSet<usize> = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| mesh.poly_at((obs.x + dx, obs.y + dy)))
            .map(|p| mesh.poly(p).sector)
            .collect();
        let floor = secs.iter().filter_map(|&s| obs.height(s)).map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
        let ceiling = secs.iter().filter_map(|&s| obs.height(s)).map(|h| h.ceiling).fold(f64::INFINITY, f64::min);
        if ceiling - floor >= PLAYER_HEIGHT {
            self.pinned_since = None;
            return None;
        }
        if self.mortal {
            // (mortal: a ceiling on her may be a crusher, known or not; waiting is dying. Out from
            // under it to the nearest spot round her with room, straight there: E4M7's, waited
            // under 53 times; E3M4's, a route just begun backed off to where she stood)
            self.pinned_since = None;
            self.progress_tic = obs.tic;
            let pos = obs.pos();
            let r = PLAYER_RADIUS + 1.0;
            let roomy = |q: Point| {
                [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)].iter().all(|(dx, dy)| {
                    mesh.poly_at((q.0 + dx, q.1 + dy)).is_some_and(|p| {
                        let h = obs.heights[mesh.poly(p).sector];
                        h.ceiling - h.floor >= PLAYER_HEIGHT + 8.0
                    })
                })
            };
            let mut out = None;
            let mut ring = 16.0;
            while ring <= 96.0 && out.is_none() {
                for k in 0..16 {
                    let a = f64::from(k) * std::f64::consts::PI / 8.0;
                    let q = (pos.0 + a.cos() * ring, pos.1 + a.sin() * ring);
                    if roomy(q) && self.chord_clear(pos, q) {
                        out = Some(q);
                        break;
                    }
                }
                ring += 16.0;
            }
            let to = out.unwrap_or_else(|| self.segments[self.seg_i].track.point_at((self.s - 48.0).max(0.0)));
            let h = heading(pos, to);
            return Some(self.motion.go(obs, h, FULL_SPEED, None));
        }
        let since = *self.pinned_since.get_or_insert_with(|| {
            self.log.add(obs.tic, format!("pinned under a ceiling at ({:.0}, {:.0}): waiting for it to rise", obs.x, obs.y));
            obs.tic
        });
        if obs.tic - since > PINNED_WAIT {
            return None;
        }
        self.progress_tic = obs.tic;
        Some(Controls::default())
    }

    /// The sectors crushers she knows of move (not their stops).
    pub(super) fn crushers(&self) -> BTreeSet<usize> {
        self.view
            .triggers
            .iter()
            .filter(|t| t.info.category == crate::specials::Category::Crusher && !t.info.description.contains("stop"))
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .collect()
    }

    /// The sectors doors she knows of move (and nothing that crushes): a door coming down on her
    /// doesn't hurt (the game moves it with NO_CRUSH, and most go back up off her).
    pub(super) fn doors(&self) -> BTreeSet<usize> {
        let crushers = self.crushers();
        self.view.triggers.iter().filter(|t| t.info.category.is_door()).flat_map(|t| t.effects.iter().map(|(s, _)| *s)).filter(|s| !crushers.contains(s)).collect()
    }

    /// (mortal) A crusher just ahead on the way, coming down or too low to be under: wait at its
    /// edge till it's on its way up with room, then through without stopping (E4M7's, on the way
    /// to its teleporter: pinned under it, waiting for it to rise, killed 53 times).
    pub(super) fn crusher_ahead(&mut self, obs: &Observation) -> Option<Controls> {
        if !self.mortal || self.prev_live.len() != obs.heights.len() {
            return None;
        }
        let crushers = self.crushers();
        // (MAP02's first door, a blazing one shutting as she came back to it: waited at it as at a
        // crusher, eight seconds, every time)
        let doors = self.doors();
        let mesh = &self.view.nav.mesh;
        let here = mesh.poly_at(obs.pos()).map(|p| mesh.poly(p).sector);
        let path = self.step().path.clone()?;
        let pos = obs.pos();
        // (a crusher she knows of, or any ceiling she sees on its way down, or up without room
        // yet: one she doesn't know may be a crusher too; a shut door standing still is a door's)
        let moving = |s: usize| !doors.contains(&s) && (obs.heights[s].ceiling - self.prev_live[s].ceiling).abs() > 0.01;
        // (under one already, any of her: no stopping there to wait for the next; E4M7's two side
        // by side, waited at the second under the first)
        let r = PLAYER_RADIUS + 2.0;
        let under = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| mesh.poly_at((pos.0 + dx, pos.1 + dy)))
            .map(|p| mesh.poly(p).sector)
            .any(|s| crushers.contains(&s) || moving(s));
        if under {
            self.crusher_wait = None;
            return None;
        }
        // (by where the way goes into it, not its middle: E4M7's, its middle far off, run into)
        let mut ahead = None;
        let polys: Vec<usize> = path.polys.iter().skip(self.poly_i).take(16).copied().collect();
        for w in polys.windows(2) {
            let (cur, nxt) = (w[0], w[1]);
            let Some(pi) = mesh.poly(cur).portals.iter().copied().find(|&pi| mesh.portals[pi].other(cur) == nxt) else { break };
            let portal = &mesh.portals[pi];
            if dist(closest_on_portal(pos, portal.p1, portal.p2), pos) > CRUSH_LOOK {
                break;
            }
            let s = mesh.poly(nxt).sector;
            if Some(s) != here && (crushers.contains(&s) || moving(s)) {
                ahead = Some(s);
                break;
            }
        }
        let Some(s) = ahead else {
            self.crusher_wait = None;
            return None;
        };
        let (now, before) = (obs.heights[s], self.prev_live[s]);
        let rising = now.ceiling > before.ceiling + 0.01;
        let still = (now.ceiling - before.ceiling).abs() < 0.01;
        let room = now.ceiling - now.floor >= PLAYER_HEIGHT + CRUSH_ROOM;
        if (room && (rising || still)) || (still && !crushers.contains(&s)) {
            self.crusher_wait = None;
            return None;
        }
        if self.crusher_wait != Some(s) {
            self.log.add(obs.tic, format!("waiting for the crusher (sector {s}) to come up"));
            self.crusher_wait = Some(s);
        }
        self.progress_tic = obs.tic;
        self.step_tic = obs.tic;
        // (all of her out of it: a crusher crushes whatever's even partly under it)
        let r = PLAYER_RADIUS + 2.0;
        let over = [(-r, -r), (r, -r), (r, r), (-r, r)].iter().any(|(dx, dy)| mesh.poly_at((pos.0 + dx, pos.1 + dy)).is_some_and(|p| mesh.poly(p).sector == s));
        if over {
            let track = &self.segments[self.seg_i].track;
            let back = track.point_at((self.s - 32.0).max(0.0));
            let h = heading(pos, back);
            return Some(self.motion.go(obs, h, 6.0, None));
        }
        Some(self.motion.hold(obs, (0.0, 0.0), None))
    }
}
