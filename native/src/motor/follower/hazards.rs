//! Waiting for crushers and escaping a ceiling that pins the player.

use super::{run_controls, RouteFollower};
use crate::geom::{dist, Point};
use crate::mesh::Mesh;
use crate::motor::{heading, Controls, Observation, FULL_SPEED};
use crate::navigator::{closest_on_portal, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

/// (mortal) a crusher this near ahead on the way is waited for at its edge...
const CRUSH_LOOK: f64 = 192.0;

/// ...till it's going up with this much more room than she needs
const CRUSH_ROOM: f64 = 8.0;

/// coming down on her with less room than this left over her head: out from under it, not on
const CRUSH_ESCAPE: f64 = 64.0;

/// (mortal, at a goal) a ceiling coming down this near her box (a step on) is waited out
const CRUSH_STEP: f64 = 24.0;

/// (mortal) a crusher's crossing: room kept over her head the whole way under it (a player can't
/// move under less than her height)...
const CROSS_HEADROOM: f64 = 2.0;
/// ...counting on getting along this much slower than she could, and out this many tics later
const CROSS_PACE: f64 = 0.85;
const CROSS_SLACK: f64 = 4.0;
/// a crossing begun is gone through, not waited out again halfway, for this long
const CROSS_COMMIT: i64 = 3 * 35;

/// Tics to go `d` from `v0` units a tic, running straight on, at `CROSS_PACE` of her speed.
fn run_tics(v0: f64, d: f64) -> f64 {
    crate::motor::run_tics(v0, d, CROSS_PACE)
}

/// (mortal) a crusher ahead still this long, too low to get under, has stopped: no waiting for it
const CRUSH_STOPPED: i64 = 3 * 35;

/// (mortal) a stopped crusher set off from its edge: she steps this far over its line...
const KICK_PAST: f64 = 2.0;
/// ...going no faster than this at the line (units a tic)...
const KICK_SPEED: f64 = 2.0;
/// ...and gives up on it after this long
const KICK_TICS: i64 = 4 * 35;
/// a crusher stands still a tic as it turns at each end: still longer than this is stopped
const CRUSH_TURN: i64 = 2;

/// how far a way out from under a ceiling coming down is looked for
const ESCAPE_LOOK: f64 = 512.0;

/// how far into the floor out from under it she aims
const ESCAPE_INTO: f64 = 32.0;

/// the longest wait under a ceiling too low for her before it counts as being stuck
const PINNED_WAIT: i64 = 10 * 35;

impl RouteFollower {
    /// Under a ceiling too low for her over some of the box (a crusher coming down, a door
    /// shutting on her): Doom won't move her till it's risen, so that's waiting, not being stuck;
    /// up to PINNED_WAIT.
    pub(super) fn pinned_under_ceiling(&mut self, obs: &Observation) -> Option<Controls> {
        let mesh = &self.view.nav.mesh;
        let r = PLAYER_RADIUS - 1.0;
        let secs: BTreeSet<usize> = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| mesh.poly_at((obs.x + dx, obs.y + dy)))
            .map(|p| mesh.poly(p).sector)
            .collect();
        let floor = secs
            .iter()
            .filter_map(|&s| obs.height(s))
            .map(|h| h.floor)
            .fold(f64::NEG_INFINITY, f64::max);
        let ceiling = secs
            .iter()
            .filter_map(|&s| obs.height(s))
            .map(|h| h.ceiling)
            .fold(f64::INFINITY, f64::min);
        if ceiling - floor >= PLAYER_HEIGHT {
            self.crusher.pinned_since = None;
            return None;
        }
        if self.mortal {
            // (mortal: a ceiling on her may be a crusher, known or not; waiting is dying. Out from
            // under it to the nearest spot round her with room, straight there, whatever route she
            // was on)
            self.crusher.pinned_since = None;
            self.progress_tic = obs.tic;
            let pos = obs.pos();
            let r = PLAYER_RADIUS + 1.0;
            let roomy = |q: Point| {
                [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
                    .iter()
                    .all(|(dx, dy)| {
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
            let to = out.unwrap_or_else(|| {
                self.segments[self.seg_i]
                    .track
                    .point_at((self.at.along - 48.0).max(0.0))
            });
            let h = heading(pos, to);
            return Some(self.motion.go(obs, h, FULL_SPEED, None));
        }
        let since = *self.crusher.pinned_since.get_or_insert_with(|| {
            self.log.add(
                obs.tic,
                format!(
                    "pinned under a ceiling at ({:.0}, {:.0}): waiting for it to rise",
                    obs.x, obs.y
                ),
            );
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
            .filter(|t| {
                t.info.category == crate::specials::Category::Crusher
                    && !t.info.description.contains("stop")
            })
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .collect()
    }

    /// The sectors doors she knows of move (and nothing that crushes): a door coming down on her
    /// doesn't hurt (the game moves it with NO_CRUSH, and most go back up off her).
    pub(super) fn doors(&self) -> BTreeSet<usize> {
        let crushers = self.crushers();
        self.view
            .triggers
            .iter()
            .filter(|t| t.info.category.is_door())
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .filter(|s| !crushers.contains(s))
            .collect()
    }

    /// The heading along the shortest way out from under ceilings on the move (`moving`) to the
    /// nearest floor with room for her under a ceiling that stays put.
    fn out_from_under(&self, obs: &Observation, moving: &dyn Fn(usize) -> bool) -> Option<f64> {
        escape_heading(&self.view.nav.mesh, obs, moving)
    }

    /// Out from under sector s's ceiling coming down on her, `offset` the part of her under it:
    /// her middle under it, by the way out; only an edge of her, straight away from that edge
    /// (a wait at a crusher's edge with a corner of her over it holds her there otherwise). Full
    /// speed whichever way she's facing: backing out at a walk is caught.
    fn escape_crusher(
        &mut self,
        obs: &Observation,
        s: usize,
        (dx, dy): Point,
        doors: &BTreeSet<usize>,
    ) -> Option<Controls> {
        let pos = obs.pos();
        // (by the way to somewhere all of her is out; else, an edge of her under, straight away from it)
        let way = self
            .out_from_under(obs, &|q| {
                !doors.contains(&q)
                    && (obs.heights[q].ceiling - self.prev_live[q].ceiling).abs() > 0.01
            })
            .or_else(|| (dx != 0.0 || dy != 0.0).then(|| heading((pos.0 + dx, pos.1 + dy), pos)));
        let h = way?;
        if self.crusher.wait != Some(s) {
            self.log.add(
                obs.tic,
                format!("the ceiling of sector {s} coming down on her: out from under it"),
            );
            self.crusher.wait = Some(s);
        }
        self.progress_tic = obs.tic;
        self.step_tic = obs.tic;
        self.motion.rate = 0.0;
        Some(run_controls(obs.angle, h))
    }

    /// (mortal) The crusher rules for every phase but walking (`crusher_ahead` has those): at a
    /// goal, settling, fetching. Partly under a ceiling coming down with little room left: out
    /// from under it first. About to touch one (her box a step on): held where she is till it's
    /// going up. Under a ceiling lower than she's tall a player can't move at all (P_TryMove
    /// takes the lowest ceiling over her box), so going in on its way down is being pinned till
    /// it rises: a switch in a crusher's alcove is walked up to on the ceiling's way up only.
    pub(super) fn crusher_guard(&mut self, obs: &Observation) -> Option<Controls> {
        if !self.mortal || self.prev_live.len() != obs.heights.len() {
            return None;
        }
        let doors = self.doors();
        let mesh = &self.view.nav.mesh;
        let pos = obs.pos();
        let coming_down = |s: usize| {
            let (now, before) = (obs.heights[s], self.prev_live[s]);
            !doors.contains(&s)
                && now.ceiling < before.ceiling - 0.01
                && now.ceiling - now.floor - PLAYER_HEIGHT < CRUSH_ESCAPE
        };
        let r = PLAYER_RADIUS + 2.0;
        let box_at = |dx: f64, dy: f64| {
            mesh.poly_at((pos.0 + dx, pos.1 + dy))
                .map(|p| mesh.poly(p).sector)
        };
        let under: Vec<(usize, Point)> = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|&(dx, dy)| box_at(dx, dy).map(|s| (s, (dx, dy))))
            .collect();
        let near = PLAYER_RADIUS + CRUSH_STEP;
        let next: Option<usize> = (0..8)
            .filter_map(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                box_at(a.cos() * near, a.sin() * near)
            })
            .find(|&s| !under.iter().any(|&(u, _)| u == s) && coming_down(s));
        if let Some((s, offset)) = under.iter().copied().find(|&(s, _)| coming_down(s)) {
            if let Some(c) = self.escape_crusher(obs, s, offset, &doors) {
                return Some(c);
            }
        }
        let Some(s) = next else {
            if self.crusher.wait.is_some() {
                self.crusher.wait = None;
            }
            return None;
        };
        if self.crusher.wait != Some(s) {
            self.log.add(
                obs.tic,
                format!(
                    "the ceiling of sector {s} coming down beside her: waiting for it to go up"
                ),
            );
            self.crusher.wait = Some(s);
        }
        self.progress_tic = obs.tic;
        self.step_tic = obs.tic;
        Some(self.motion.hold(obs, (0.0, 0.0), self.look))
    }

    /// Sector s's crossing on the way from here (`polys`, hers first): how far her front is from
    /// getting under it (0: under it already) and how far her back from being out the far side.
    /// None: the way doesn't come out of it in the look ahead (it ends in there: a switch in a
    /// crusher's alcove).
    fn crossing(&self, obs: &Observation, polys: &[usize], s: usize) -> Option<(f64, f64)> {
        let mesh = &self.view.nav.mesh;
        let pos = obs.pos();
        // (the way's length portal to portal, not as the crow flies: a way that goes half round a
        // pillar under the crusher is no straight dash)
        let (mut at, mut along) = (pos, 0.0);
        let mut d_in = (mesh.poly_at(pos).map(|p| mesh.poly(p).sector) == Some(s)).then_some(0.0);
        for w in polys.windows(2) {
            let (cur, nxt) = (w[0], w[1]);
            let pi = mesh
                .poly(cur)
                .portals
                .iter()
                .copied()
                .find(|&pi| mesh.portals[pi].other(cur) == nxt)?;
            let portal = &mesh.portals[pi];
            let p = closest_on_portal(at, portal.p1, portal.p2);
            along += dist(at, p);
            at = p;
            let (a, b) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            match d_in {
                None if b == s => d_in = Some((along - PLAYER_RADIUS).max(0.0)),
                Some(d) if a == s && b != s => return Some((d, along + PLAYER_RADIUS)),
                _ => {}
            }
        }
        None
    }

    /// From `from` tics on to `to`, does sector s's ceiling leave her room (her height and
    /// `CROSS_HEADROOM`)? It's taken to go on as a crusher does, at its pace last seen, between
    /// its floor + 8 and its top, turning at each.
    fn room_through(&self, obs: &Observation, s: usize, from: f64, to: f64) -> bool {
        let (now, before) = (obs.heights[s], self.prev_live[s]);
        let need = now.floor + PLAYER_HEIGHT + CROSS_HEADROOM;
        let top = self.view.nav.movers.initial[s].ceiling.max(now.ceiling);
        let bottom = now.floor + 8.0;
        let pace = self.crusher.ceiling_pace.get(&s).copied().unwrap_or(0.0);
        let mut dir = if now.ceiling > before.ceiling + 0.01 {
            1.0
        } else if now.ceiling < before.ceiling - 0.01 || top - now.ceiling < 0.5 {
            -1.0
        } else if now.ceiling - bottom < 0.5 {
            1.0
        } else {
            0.0 // (stopped where it is)
        };
        let mut h = now.ceiling;
        let mut k = 0.0;
        while k <= to {
            if k >= from && h < need {
                return false;
            }
            h += dir * pace;
            if h >= top {
                (h, dir) = (top, -1.0);
            } else if h <= bottom {
                (h, dir) = (bottom, 1.0);
            }
            k += 1.0;
        }
        true
    }

    /// Would she be through sector s's crossing (from where she is on the way) with room over her
    /// all the way, setting off (or going on) now? None: no crossing out of it known.
    fn through_in_time(&self, obs: &Observation, polys: &[usize], s: usize) -> Option<bool> {
        let (d_in, d_out) = self.crossing(obs, polys, s)?;
        let v0 = self.motion.speed();
        Some(self.room_through(
            obs,
            s,
            run_tics(v0, d_in),
            run_tics(v0, d_out) + CROSS_SLACK,
        ))
    }

    /// Could she go into sector s now, on the way (`polys`, hers first)? Through it in time, and
    /// only while it's going up or still with room; a way that ends in there, in once it's rising
    /// with room (or still with room; or still and no crusher). With the crossing's timing (None:
    /// none out of it known).
    fn crossable(
        &self,
        obs: &Observation,
        polys: &[usize],
        s: usize,
        crushers: &BTreeSet<usize>,
    ) -> (bool, Option<bool>) {
        let (now, before) = (obs.heights[s], self.prev_live[s]);
        let rising = now.ceiling > before.ceiling + 0.01;
        let still = (now.ceiling - before.ceiling).abs() < 0.01;
        let room = now.ceiling - now.floor >= PLAYER_HEIGHT + CRUSH_ROOM;
        let timed = self.through_in_time(obs, polys, s);
        let go = match timed {
            Some(t) => t && (rising || (still && room)),
            None => (room && (rising || still)) || (still && !crushers.contains(&s)),
        };
        (go, timed)
    }

    /// Could she go into crusher s now, and on into the one straight after it, if any?
    fn open_now(
        &self,
        obs: &Observation,
        polys: &[usize],
        s: usize,
        crushers: &BTreeSet<usize>,
        doors: &BTreeSet<usize>,
    ) -> bool {
        self.crossable(obs, polys, s, crushers).0
            && self
                .crusher_after(obs, polys, s, crushers, doors)
                .is_none_or(|b| self.crossable(obs, polys, b, crushers).0)
    }

    /// The crusher (or ceiling on the move) the way goes into straight out of sector s, if it
    /// does: none between, so no waiting for it but under s.
    fn crusher_after(
        &self,
        obs: &Observation,
        polys: &[usize],
        s: usize,
        crushers: &BTreeSet<usize>,
        doors: &BTreeSet<usize>,
    ) -> Option<usize> {
        let mesh = &self.view.nav.mesh;
        let (_, b) = polys
            .windows(2)
            .map(|w| (mesh.poly(w[0]).sector, mesh.poly(w[1]).sector))
            .find(|&(a, b)| a == s && b != s)?;
        let moving = !doors.contains(&b)
            && (obs.heights[b].ceiling - self.prev_live[b].ceiling).abs() > 0.01;
        (crushers.contains(&b) || moving).then_some(b)
    }

    /// (mortal) A crusher just ahead on the way, coming down or too low to be under: wait at its
    /// edge till it's on its way up with room, then through without stopping (stopping under it
    /// to wait for it to rise is being pinned).
    pub(super) fn crusher_ahead(&mut self, obs: &Observation) -> Option<Controls> {
        if !self.mortal || self.prev_live.len() != obs.heights.len() {
            return None;
        }
        if let Some(c) = self.kicking(obs) {
            return Some(c);
        }
        let crushers = self.crushers();
        // (a door shutting as she comes to it is no crusher, blazing or not: not waited at as one)
        let doors = self.doors();
        let nav = std::rc::Rc::clone(&self.view.nav);
        let mesh = &nav.mesh;
        let here = mesh.poly_at(obs.pos()).map(|p| mesh.poly(p).sector);
        let path = self.step().path.clone()?;
        let pos = obs.pos();
        // (a crusher she knows of, or any ceiling she sees on its way down, or up without room
        // yet: one she doesn't know may be a crusher too; a shut door standing still is a door's)
        let moving = |s: usize| {
            !doors.contains(&s) && (obs.heights[s].ceiling - self.prev_live[s].ceiling).abs() > 0.01
        };
        // (under one already, any of her: no stopping there to wait for the next, of two side by
        // side)
        let r = PLAYER_RADIUS + 2.0;
        let under = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| mesh.poly_at((pos.0 + dx, pos.1 + dy)))
            .map(|p| mesh.poly(p).sector)
            .any(|s| crushers.contains(&s) || moving(s));
        let polys: Vec<usize> = path
            .polys
            .iter()
            .skip(self.poly_i)
            .take(32)
            .copied()
            .collect();
        if under {
            // (but with it coming down on her and not much room left, out from under it now, by
            // the nearest way to a ceiling that isn't: the way on may be a long one inside it, and
            // caught at head height she's pinned, Doom moving her nowhere, till dead)
            let moving_down = |s: usize| {
                obs.heights[s].ceiling < self.prev_live[s].ceiling - 0.01
                    && obs.heights[s].ceiling - obs.heights[s].floor - PLAYER_HEIGHT < CRUSH_ESCAPE
            };
            // (any of her under it: a crusher crushes whatever's even partly under it)
            // (on through, if she'll be out the far side with room over her all the way: back
            // out of a fast one she's halfway under is caught as surely. Every one coming down on
            // her, not the first found: of two side by side, through the one over her middle in
            // time is still under the edge of the other)
            let pressing = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
                .iter()
                .filter_map(|&(dx, dy)| {
                    mesh.poly_at((pos.0 + dx, pos.1 + dy))
                        .map(|p| (mesh.poly(p).sector, (dx, dy)))
                })
                .find(|&(s, _)| {
                    moving_down(s) && self.through_in_time(obs, &polys, s) != Some(true)
                });
            // (the way on out of where her middle is into a crusher she couldn't go into now (the
            // walking rule's, the one after it too): out from under all of them, not on into it,
            // nor a wait at its edge under one, as with a fast crusher and a slow one beyond it. A
            // crossing begun is gone through)
            let committed = |b: usize| {
                self.crusher
                    .cross
                    .is_some_and(|(c, since)| c == b && obs.tic - since < CROSS_COMMIT)
            };
            let next_closed = here
                .and_then(|h| self.crusher_after(obs, &polys, h, &crushers, &doors))
                .filter(|&b| !committed(b) && !self.open_now(obs, &polys, b, &crushers, &doors));
            if let Some((s, offset)) =
                pressing.or_else(|| here.zip(next_closed).map(|(h, _)| (h, (0.0, 0.0))))
            {
                self.crusher.cross = None;
                if let Some(c) = self.escape_crusher(obs, s, offset, &doors) {
                    return Some(c);
                }
            }
            self.crusher.wait = None;
            return None;
        }
        // (by where the way goes into it, not its middle, which can be far off)
        let mut ahead = None;
        for w in polys.windows(2).take(15) {
            let (cur, nxt) = (w[0], w[1]);
            let Some(pi) = mesh
                .poly(cur)
                .portals
                .iter()
                .copied()
                .find(|&pi| mesh.portals[pi].other(cur) == nxt)
            else {
                break;
            };
            let portal = &mesh.portals[pi];
            if dist(closest_on_portal(pos, portal.p1, portal.p2), pos) > CRUSH_LOOK {
                break;
            }
            let s = mesh.poly(nxt).sector;
            if Some(s) != here && (crushers.contains(&s) || moving(s)) {
                ahead = Some((s, pi));
                break;
            }
        }
        // (out from under it and none ahead: a crossing begun is over, the legs the fight's again;
        // kept, the fight would leave her to the route with a monster at her)
        let Some((s, entry)) = ahead else {
            self.crusher.wait = None;
            self.crusher.cross = None;
            return None;
        };
        // (a crossing timed and begun: gone through, not waited out halfway)
        if self
            .crusher
            .cross
            .is_some_and(|(c, since)| c == s && obs.tic - since < CROSS_COMMIT)
        {
            return None;
        }
        let (now, before) = (obs.heights[s], self.prev_live[s]);
        let still = (now.ceiling - before.ceiling).abs() < 0.01;
        let room = now.ceiling - now.floor >= PLAYER_HEIGHT + CRUSH_ROOM;
        // (by time, where the way comes out of it: set off when she'd be under it only while it
        // leaves her room, from her front getting there to her back out of it, at her pace from
        // where she is; rising with room isn't enough for a fast one, open some 16 tics with 96
        // units to cross from a standstill. A way that ends in there, as before: in once it's
        // rising with room)
        // (and only while it's going up, or still with room: in on its way down, however the sums
        // came out, is a dash to beat it, and a slow one on a long way through wins)
        // (stopped, and going in sets it off, with too little room to be through before it has her:
        // set off from the edge, out again, and crossed when it comes up; a crusher stopped just
        // over her head, its own start line at its edge, crushes her otherwise)
        if still
            && obs.tic
                - self
                    .crusher
                    .moved_at
                    .get(&s)
                    .copied()
                    .unwrap_or(i64::MIN / 2)
                > CRUSH_TURN
        {
            if let Some(c) = self.set_off_first(obs, &polys, s, entry) {
                return Some(c);
            }
        }
        let (go, timed) = self.crossable(obs, &polys, s, &crushers);
        // (and out of it into another she couldn't go into now: not in, a wait under the first is
        // no wait)
        let go = go
            && self
                .crusher_after(obs, &polys, s, &crushers, &doors)
                .is_none_or(|b| self.crossable(obs, &polys, b, &crushers).0);
        if go {
            // (committed to only a crossing that comes out the far side: one into an alcove under
            // it is waited at again when a replan sends her back in)
            self.crusher.cross = timed.is_some().then_some((s, obs.tic));
            self.crusher.wait = None;
            return None;
        }
        // (waiting is no crossing)
        self.crusher.cross = None;
        if self.crusher.wait != Some(s) {
            self.log.add(
                obs.tic,
                format!("waiting for the crusher (sector {s}) to come up"),
            );
            self.crusher.wait = Some(s);
        }
        // (one that's stopped, too low to be under, isn't coming up: a crusher turns at each end at
        // once, so still for a while is stopped (a crusher-stop line). The way's no way while it's
        // there, and waiting at it is waiting for good)
        if still
            && !room
            && obs.tic
                - self
                    .crusher
                    .moved_at
                    .get(&s)
                    .copied()
                    .unwrap_or(i64::MIN / 2)
                > CRUSH_STOPPED
        {
            // (stopped with her height under it, if no more: it does nothing while it's stopped,
            // and the room to spare was for one coming down. Through, then; its start line
            // crossed on the way out sets it going behind her. Lower than her height, there's no
            // going in)
            let gap = now.ceiling - now.floor;
            if gap >= PLAYER_HEIGHT {
                self.log.add(
                    obs.tic,
                    format!("the crusher (sector {s}) has stopped {gap:.0} up: through under it"),
                );
                self.crusher.cross = Some((s, obs.tic));
                self.crusher.wait = None;
                return None;
            }
            return Some(self.fail(
                obs,
                &format!("the crusher (sector {s}) has stopped too low to get under ({gap:.0} up)"),
            ));
        }
        self.progress_tic = obs.tic;
        self.step_tic = obs.tic;
        // (all of her out of it: a crusher crushes whatever's even partly under it)
        let r = PLAYER_RADIUS + 2.0;
        let over = [(-r, -r), (r, -r), (r, r), (-r, r)].iter().any(|(dx, dy)| {
            mesh.poly_at((pos.0 + dx, pos.1 + dy))
                .is_some_and(|p| mesh.poly(p).sector == s)
        });
        if over {
            let track = &self.segments[self.seg_i].track;
            let back = track.point_at((self.at.along - 32.0).max(0.0));
            let h = heading(pos, back);
            return Some(self.motion.go(obs, h, 6.0, None));
        }
        Some(self.motion.hold(obs, (0.0, 0.0), None))
    }
}

impl RouteFollower {
    /// The crusher that line `li` sets off going over it into sector s, as she knows it.
    fn starts_crusher(&self, li: usize, s: usize) -> Option<crate::specials::CrusherKind> {
        self.view
            .triggers
            .iter()
            .filter(|t| {
                t.linedef() == Some(li)
                    && t.info.trigger.walked()
                    && t.effects.iter().any(|(x, _)| *x == s)
            })
            .find_map(|t| crate::specials::crusher_kind(t.info.special))
    }

    /// (mortal) Going into stopped crusher s across portal `entry`: if a line on the way in sets it
    /// off (its edge, or one crossed just before it), and it'd have her before she's out the far
    /// side (and its stroke's more than she'd take on), step just over that line to set it off and
    /// let it come up first. None: on as it is. Fails the step if even backing out from the line
    /// wouldn't beat it.
    fn set_off_first(
        &mut self,
        obs: &Observation,
        polys: &[usize],
        s: usize,
        entry: usize,
    ) -> Option<Controls> {
        let nav = std::rc::Rc::clone(&self.view.nav);
        let mesh = &nav.mesh;
        // (the last line on the way that sets it off, up to and over its edge: start lines can
        // ring a crusher a few units out from its edge, and over one at a run she's under it as
        // it starts. Where her middle crosses it, how far along, and the way over it)
        let (mut at, mut along) = (obs.pos(), 0.0);
        let mut found = None;
        let mut entry_along = None;
        for w in polys.windows(2) {
            let (cur, nxt) = (w[0], w[1]);
            let pi = mesh
                .poly(cur)
                .portals
                .iter()
                .copied()
                .find(|&pi| mesh.portals[pi].other(cur) == nxt)?;
            let portal = &mesh.portals[pi];
            let p = closest_on_portal(at, portal.p1, portal.p2);
            along += dist(at, p);
            at = p;
            if let Some(kind) = portal
                .linedef
                .and_then(|li| self.starts_crusher(li, s).map(|k| (li, k)))
            {
                found = Some((kind, pi, at, along, mesh.poly(nxt).center));
            }
            if pi == entry {
                entry_along = Some(along);
                break;
            }
        }
        let ((li, kind), pi, at, trig_along, beyond) = found?;
        let gap = entry_along? - trig_along;
        let portal = &mesh.portals[pi];
        let h = obs.heights[s];
        let room = crate::crush::room_tics(h, kind);
        if crate::crush::stroke_cost(kind, obs)
            <= crate::knowledge::RISK_SHARE * obs.health.unwrap_or(100.0)
        {
            return None;
        }
        // (it starts as her middle crosses the line; out the far side then, from where she is at
        // her pace, or not)
        if let Some((_, d_out)) = self.crossing(obs, polys, s) {
            let v0 = self.motion.speed();
            if room >= run_tics(v0, d_out) - run_tics(v0, trig_along) + CROSS_SLACK {
                return None;
            }
        }
        // (stepped over at a crawl and straight back: all of her out before it's at her head; a
        // line out from its edge, as much less back as it's out)
        let (a, b) = nav.map.line_seg(li);
        let length = dist(a, b).max(1e-6);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let at = closest_on_portal(at, portal.p1, portal.p2);
        let sign = if (beyond.0 - at.0) * nx + (beyond.1 - at.1) * ny > 0.0 {
            1.0
        } else {
            -1.0
        };
        let clear = (PLAYER_RADIUS * (nx.abs() + ny.abs()) + KICK_PAST - gap).max(0.0) + 1.0;
        if room < run_tics(-KICK_SPEED, clear) + CROSS_SLACK {
            return Some(self.fail(obs, &format!("the crusher (sector {s}) has stopped too low to get through or set off from its edge")));
        }
        let spot = (at.0 + sign * nx * KICK_PAST, at.1 + sign * ny * KICK_PAST);
        self.log.add(obs.tic, format!("setting off the crusher (sector {s}) from its edge (line {li}), to cross it when it comes up"));
        self.crusher.kick = Some((s, obs.tic, spot));
        self.kicking(obs)
    }

    /// (mortal) Setting off a crusher from its edge: on to the spot just over its line at a crawl
    /// till it moves; then it's for the crusher rules (out from under it, waited at, crossed).
    fn kicking(&mut self, obs: &Observation) -> Option<Controls> {
        let (s, since, spot) = self.crusher.kick?;
        if (obs.heights[s].ceiling - self.prev_live[s].ceiling).abs() > 0.01 {
            self.crusher.kick = None;
            return None;
        }
        if obs.tic - since > KICK_TICS {
            self.crusher.kick = None;
            return Some(self.fail(
                obs,
                &format!("the crusher (sector {s}) didn't start from its edge"),
            ));
        }
        self.progress_tic = obs.tic;
        self.step_tic = obs.tic;
        let pos = obs.pos();
        let hd = heading(pos, spot);
        Some(self.motion.go(
            obs,
            hd,
            (0.3 * dist(pos, spot) + KICK_SPEED).min(8.0),
            Some(hd),
        ))
    }
}

/// The heading out from under ceilings `moving` (judged by sector), by the mesh: the first portal
/// on the way to the nearest poly not under one, steps she can take, room to stand, aimed well
/// into the poly beyond.
fn escape_heading(mesh: &Mesh, obs: &Observation, moving: &dyn Fn(usize) -> bool) -> Option<f64> {
    // (somewhere all of her is out from under: her box, at a poly's middle, every corner on floor
    // with no ceiling moving over it; not her middle out with an edge still under, which a
    // crusher beside a wall she slides along still catches)
    let r = PLAYER_RADIUS + 2.0;
    let clear = |c: Point| {
        [(-r, -r), (r, -r), (r, r), (-r, r)].iter().all(|(dx, dy)| {
            mesh.poly_at((c.0 + dx, c.1 + dy))
                .is_some_and(|q| !moving(mesh.poly(q).sector))
        })
    };
    nearest_heading(
        mesh,
        obs,
        &|p| !moving(mesh.poly(p).sector) && clear(mesh.poly(p).center),
        ESCAPE_LOOK,
    )
}

/// The heading to the nearest poly that's a `goal`, by the mesh, no further than `look` along the
/// way: the first portal on the way, steps she can take, room to stand, aimed well into the poly
/// beyond (her own poly a goal: its middle).
pub fn nearest_heading(
    mesh: &Mesh,
    obs: &Observation,
    goal_poly: &dyn Fn(usize) -> bool,
    look: f64,
) -> Option<f64> {
    let start = mesh.poly_at(obs.pos())?;
    let n = mesh.polys.len();
    let mut cost = vec![f64::INFINITY; n];
    let mut entry = vec![obs.pos(); n];
    let mut came: Vec<Option<(usize, usize)>> = vec![None; n];
    cost[start] = 0.0;
    let mut open = BinaryHeap::new();
    open.push(Reverse((0u64, start)));
    let mut goal = None;
    while let Some(Reverse((_, p))) = open.pop() {
        let s = mesh.poly(p).sector;
        if goal_poly(p) {
            goal = Some(p);
            break;
        }
        if cost[p] > look {
            continue;
        }
        let h = obs.heights[s];
        for &qi in &mesh.poly(p).portals {
            let q = &mesh.portals[qi];
            let o = q.other(p);
            let ho = obs.heights[mesh.poly(o).sector];
            if ho.floor > h.floor + MAX_STEP
                || ho.ceiling - ho.floor < PLAYER_HEIGHT
                || dist(q.p1, q.p2) < 2.0 * PLAYER_RADIUS
            {
                continue;
            }
            let at = closest_on_portal(entry[p], q.p1, q.p2);
            let k = cost[p] + dist(entry[p], at);
            if k < cost[o] {
                (cost[o], entry[o], came[o]) = (k, at, Some((p, qi)));
                open.push(Reverse(((k * 16.0) as u64, o)));
            }
        }
    }
    // (the first portal of the way, crossed well into the next poly; or the middle of hers)
    let mut p = goal?;
    if p == start {
        return Some(heading(obs.pos(), mesh.poly(p).center));
    }
    let mut first = None;
    while let Some((from, qi)) = came[p] {
        first = Some((qi, p));
        p = from;
    }
    let (qi, beyond) = first?;
    let q = &mesh.portals[qi];
    let at = closest_on_portal(obs.pos(), q.p1, q.p2);
    let c = mesh.poly(beyond).center;
    let d = dist(at, c).max(1.0);
    let aim = (
        at.0 + (c.0 - at.0) / d * d.min(ESCAPE_INTO),
        at.1 + (c.1 - at.1) / d * d.min(ESCAPE_INTO),
    );
    Some(heading(obs.pos(), aim))
}
