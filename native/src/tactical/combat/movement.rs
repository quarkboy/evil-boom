// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Missile avoidance, strafing and checking safe escape directions.

use super::{clearance, hit_clearance, miss_by, Combat};
use crate::geom::{dist, segments_cross, Point};
use crate::motor::{heading, norm, Observation, Thing, FRICTION, RUN_THRUST};
use crate::navigator::{MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS, TELEPORTERS_FOR_PLAYERS};
use crate::tactical::bestiary::{missile, weapon_in, Weapon};
use std::collections::BTreeSet;

const DODGE_WAYS: usize = 16;

/// going some way: pushing that way this long, then...
const PUSH_TICS: usize = 8;

/// ...letting go: where she'd slide to in this long must be safe too
const SLIDE_TICS: usize = 40;

impl Combat {
    /// The missiles that'd hit her, soonest first: any that would if she went on as she's going,
    /// and any she's getting out of the way of that haven't gone by yet.
    pub(super) fn incoming(&mut self, obs: &Observation) -> Vec<Thing> {
        let pos = obs.pos();
        let z = self.z(obs);
        let vel = self.velocity;
        let mut out: Vec<(f64, Thing)> = Vec::new();
        for t in obs.things.iter().flatten() {
            let Some(m) = missile(t.name) else { continue };
            if t.owner == 0 || t.vx.hypot(t.vy) < 1.0 {
                // Not her own: her plasma and rockets; nor one gone off already, standing in its blast
                // frames.
                continue;
            }
            if t.name == "LostSoul" && t.vx.hypot(t.vy) < 8.0 {
                continue;
            }
            // Relative to her as she's going, and as if she stopped: one aimed at where she was still
            // hits if she turns back into it, as a route does round a corner.
            let Some((when, miss)) = [vel, (0.0, 0.0)]
                .into_iter()
                .filter_map(|v| {
                    let (rvx, rvy) = (t.vx - v.0, t.vy - v.1);
                    let v2 = rvx * rvx + rvy * rvy;
                    if v2 < 1.0 {
                        return None;
                    }
                    let (rx, ry) = (pos.0 - t.x, pos.1 - t.y);
                    let when = (rx * rvx + ry * rvy) / v2;
                    if !(0.0 < when && when < self.tune.dodge.within_tics as f64) {
                        return None;
                    }
                    let hz = t.z + t.vz * when;
                    if !(z - 24.0 < hz && hz < z + PLAYER_HEIGHT + 24.0) {
                        return None;
                    }
                    Some((
                        when,
                        (t.x + rvx * when - pos.0).hypot(t.y + rvy * when - pos.1),
                    ))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            if !self.dodge.missiles.contains(&t.id) && miss > clearance(&self.tune.dodge, &m) {
                continue;
            }
            if self.dodge.missiles.insert(t.id) {
                self.log.push(format!(
                    "tic {}: dodging {} {} (due in {when:.0} tics, {miss:.0} off)",
                    obs.tic, t.name, t.id
                ));
            }
            out.push((when, t.clone()));
        }
        let here: BTreeSet<i32> = out.iter().map(|(_, t)| t.id).collect();
        self.dodge.missiles.retain(|i| here.contains(i));
        for (_, t) in out
            .iter()
            .filter(|(_, t)| missile(t.name).is_some_and(|m| m.homing))
        {
            self.dodge.homing_since.entry(t.id).or_insert(obs.tic);
        }
        // One that has gone by and come round again is still the same one: kept while it flies.
        let flying: BTreeSet<i32> = obs.things.iter().flatten().map(|t| t.id).collect();
        self.dodge.homing_since.retain(|i, _| flying.contains(i));
        if self
            .dodge
            .shake_spot
            .is_some_and(|(i, _)| !self.dodge.homing_since.contains_key(&i))
        {
            self.dodge.shake_spot = None;
        }
        if here.is_empty() {
            self.dodge.way = None;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.into_iter().map(|(_, t)| t).collect()
    }

    /// The way to run that keeps her furthest from all of them as they go by, where the floor's
    /// safe; the way she's going already when it's as good (no dithering).
    pub(super) fn dodge(
        &mut self,
        obs: &Observation,
        missiles: &[Thing],
        prefer: Option<Point>,
    ) -> Option<Point> {
        let pos = obs.pos();
        let mut ways: Vec<Point> = (0..DODGE_WAYS)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * i as f64 / DODGE_WAYS as f64;
                (a.cos(), a.sin())
            })
            .collect();
        if let Some(w) = self.dodge.way {
            ways.push(w);
        }
        let mut best: Option<(f64, Point, f64)> = None;
        for way in ways {
            if !self.safe(obs, way, 64.0) {
                continue;
            }
            let room = self.clear_run(obs, way, 16.0 * self.tune.dodge.within_tics as f64);
            let misses: Vec<(f64, f64)> = missiles
                .iter()
                .filter_map(|t| {
                    missile(t.name).map(|m| {
                        (
                            miss_by(&self.tune.dodge, pos, self.velocity, way, room, t),
                            m,
                        )
                    })
                })
                .map(|(d, m)| {
                    (
                        d - clearance(&self.tune.dodge, &m),
                        d - hit_clearance(&self.tune.dodge, &m),
                    )
                })
                .collect();
            let mut s = misses.iter().map(|m| m.0).fold(f64::INFINITY, f64::min);
            // Clear of the missile itself, its blast or not.
            let hit = misses.iter().map(|m| m.1).fold(f64::INFINITY, f64::min);
            s = s.min(48.0); // (clear is clear: then the rest decides)
                             // Among those, where there's room after: a dodge along a wall into a corner leaves no way
                             // out of the next one.
            let ahead = (
                pos.0 + way.0 * room.min(160.0),
                pos.1 + way.1 * room.min(160.0),
            );
            if s >= 0.0 {
                s += 24.0 * self.openness(obs, ahead, 192.0);
                // Across its way, not along it: a step aside clears its line for good. Backing straight off
                // only puts it off, and it follows her down the room.
                let along = missiles
                    .iter()
                    .filter_map(|t| {
                        let v = t.vx.hypot(t.vy);
                        (v > 0.5).then(|| ((way.0 * t.vx + way.1 * t.vy) / v).abs())
                    })
                    .fold(0.0, f64::max);
                s -= self.tune.dodge.along * along;
            }
            if let Some(dw) = self.dodge.way {
                s += 6.0 * (way.0 * dw.0 + way.1 * dw.1);
            }
            // As clear, a way that stays on ground she's been on: sideways where she is, not back
            // through a door into a room she's never been in. This is the steering's weight for new
            // ground, in the dodge's units.
            if s >= 0.0 && self.tune.steer.w_new_ground > 0.0 && self.off_known(ahead) {
                s -= 6.0 * self.tune.steer.w_new_ground;
            }
            // And, as clear, the way she's going (health, a gun, ammo across the room): dodging where
            // she stands, missile after missile, she never gets there.
            if let Some(to) = self.going_to.filter(|&to| s >= 0.0 && dist(to, pos) > 1.0) {
                let d = dist(to, pos);
                s += self.tune.dodge.toward * (way.0 * (to.0 - pos.0) + way.1 * (to.1 - pos.1)) / d;
            }
            // And, as clear, a way she'd rather go: round a corner from a homing one.
            if let Some(p) = prefer.filter(|_| s >= 0.0) {
                s += 2.0 * self.tune.dodge.toward * (way.0 * p.0 + way.1 * p.1);
            }
            if best.is_none_or(|(bs, _, _)| s > bs) {
                best = Some((s, way, hit));
            }
        }
        self.dodge.way = best.map(|b| b.1);
        self.dodge.clear = best.is_some_and(|b| b.2 >= 0.0);
        self.dodge.way
    }

    /// Would going `way` (none: coasting where she is) keep her clear of every one of these
    /// missiles' own hit, blasts or not?
    pub(super) fn clear_of_hits(
        &self,
        obs: &Observation,
        missiles: &[Thing],
        way: Option<Point>,
    ) -> bool {
        let (way, room) = way.map_or(((0.0, 0.0), f64::INFINITY), |w| {
            (
                w,
                self.clear_run(obs, w, 16.0 * self.tune.dodge.within_tics as f64),
            )
        });
        missiles.iter().all(|t| {
            missile(t.name).is_none_or(|m| {
                miss_by(&self.tune.dodge, obs.pos(), self.velocity, way, room, t)
                    >= hit_clearance(&self.tune.dodge, &m)
            })
        })
    }

    /// Did the last dodge find a way clear of every missile coming? Clear of a missile's own hit,
    /// that is, whatever its blast: the hit is the blast and a good deal more.
    pub fn dodge_clear(&self) -> bool {
        self.dodge.clear
    }

    /// Back off from `what`: straight away if the floor's safe that way, else as near that as is.
    pub(super) fn away_from(&mut self, obs: &Observation, what: Point) -> Option<Point> {
        let away = (obs.y - what.1).atan2(obs.x - what.0);
        let pos = obs.pos();
        // First a way that stays on ground she's been on, if that's counted; then any.
        for known_only in [self.tune.steer.w_new_ground > 0.0, false] {
            for off in [0.0f64, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0] {
                let a = away + off.to_radians();
                let d = (a.cos(), a.sin());
                if known_only && self.off_known((pos.0 + d.0 * 96.0, pos.1 + d.1 * 96.0)) {
                    continue;
                }
                if self.safe(obs, d, 64.0) {
                    return Some(d);
                }
            }
            if !known_only {
                break;
            }
        }
        None
    }

    pub(super) fn gun(&self, obs: &Observation) -> Weapon {
        weapon_in(obs.weapon.filter(|w| (1..=7).contains(w)).unwrap_or(2), obs)
    }

    /// Away from t, aslant (the strafe's way and back), as far as it's safe.
    pub(super) fn keeping_off(
        &mut self,
        obs: &Observation,
        t: &Thing,
        aim: Option<f64>,
    ) -> Option<Point> {
        let a = (obs.y - t.y).atan2(obs.x - t.x);
        let back = (a.cos(), a.sin());
        let mut ways = vec![back];
        if let Some(s) = self.strafe_dir(obs, aim) {
            let mixed = (back.0 + s.0, back.1 + s.1);
            let n = mixed.0.hypot(mixed.1);
            if n > 0.1 {
                ways.insert(0, (mixed.0 / n, mixed.1 / n));
            }
        }
        if let Some(w) = ways.into_iter().find(|&w| self.safe(obs, w, 64.0)) {
            return Some(w);
        }
        self.away_from(obs, (t.x, t.y))
    }

    /// Towards t, zig-zagging (the strafe's way and forwards), as far as it's safe.
    pub(super) fn closing_in(
        &mut self,
        obs: &Observation,
        t: &Thing,
        strafe: Option<Point>,
    ) -> Option<Point> {
        let a = (t.y - obs.y).atan2(t.x - obs.x);
        let ahead = (a.cos(), a.sin());
        // Straight on not clear, a little to either side of it: with a side of her caught on a
        // doorway's jamb, straight on is no way, and a few degrees off slides her past it.
        let mut ways = vec![ahead];
        ways.extend(
            self.tune
                .dodge
                .close_veer
                .iter()
                .flat_map(|&v| [a + v, a - v])
                .map(|b| (b.cos(), b.sin())),
        );
        if let Some(s) = strafe {
            let mixed = (ahead.0 + s.0, ahead.1 + s.1);
            let n = mixed.0.hypot(mixed.1);
            if n > 0.1 {
                ways.insert(0, (mixed.0 / n, mixed.1 / n));
            }
        }
        ways.into_iter()
            .find(|&w| self.safe(obs, w, 64.0))
            .or(strafe)
    }

    /// Side to side across the line of fire, a second or so each way, where it's safe.
    pub(super) fn strafe_dir(&mut self, obs: &Observation, aim: Option<f64>) -> Option<Point> {
        if obs.tic >= self.strafe_until {
            self.strafe = -self.strafe;
            let (lo, hi) = self.tune.dodge.strafe_tics;
            self.strafe_until = obs.tic + lo + (obs.tic * 7919).rem_euclid(hi - lo);
        }
        let aim = aim?;
        let a = (aim + 90.0 * self.strafe).to_radians();
        let d = (a.cos(), a.sin());
        if self.safe(obs, d, 48.0) {
            return Some(d);
        }
        self.strafe = -self.strafe;
        let d = (-d.0, -d.1);
        self.safe(obs, d, 48.0).then_some(d)
    }

    /// How far she could go that way (up to `length`): floor all the way, no step up or drop she
    /// couldn't come back from, room to stand, nothing that hurts.
    pub(super) fn clear_run(&self, obs: &Observation, d: Point, length: f64) -> f64 {
        // Her middle and both sides: the middle can be clear past a corner that catches a side.
        let side = (-d.1 * PLAYER_RADIUS, d.0 * PLAYER_RADIUS);
        [(0.0, 0.0), side, (-side.0, -side.1)]
            .into_iter()
            .map(|o| self.clear_line(obs, (obs.x + o.0, obs.y + o.1), d, length))
            .fold(length, f64::min)
    }

    /// Is she standing on a floor that hurts? Then that is no reason not to go a way: every way
    /// is over it, and standing still on it is the worst of them.
    pub(super) fn on_hurting_floor(&self, obs: &Observation) -> bool {
        self.nav
            .poly_at(obs.pos())
            .is_some_and(|p| self.hurts(obs, self.nav.sector_of_poly(p)))
    }

    /// Does sector s's floor hurt, or is it about to? As she takes it (`hazards`: felt, else by
    /// its look, or seen to turn into something that hurts; not under a suit), or going down now
    /// to where it will (a walkway sinking into what's under it).
    pub(super) fn hurts(&self, obs: &Observation, s: usize) -> bool {
        self.hazards.get(s).copied().unwrap_or(false)
            || (self.movers.sinks_into_harm.contains(&s)
                && obs
                    .height(s)
                    .is_some_and(|h| h.floor < self.nav.movers.initial[s].floor - 0.5))
    }

    /// A crusher sector the fight's moves keep out of: on its way down, at work at all, or one
    /// she knows stopped too low to get out from under if it starts again.
    pub(super) fn no_go_crusher(&self, obs: &Observation, s: usize) -> bool {
        self.crushing(obs, s) || self.crusher_active(obs, s) || self.crusher_trap(obs, s)
    }

    /// Is any of her in a crusher sector at work, coming down or not (`no_go_crusher`)?
    pub(super) fn in_crusher(&self, obs: &Observation) -> bool {
        self.box_in_crusher(obs, obs.pos())
    }

    /// Would any of her, stood at p, be in a crusher sector at work (`no_go_crusher`)?
    fn box_in_crusher(&self, obs: &Observation, p: Point) -> bool {
        let r = PLAYER_RADIUS + 2.0;
        [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| self.nav.poly_at((p.0 + dx, p.1 + dy)))
            .any(|q| self.no_go_crusher(obs, self.nav.sector_of_poly(q)))
    }

    /// Is any of her under a crusher at work? A crusher crushes whatever is even partly under it,
    /// and her middle being out from under it isn't all of her.
    pub(super) fn under_crusher(&self, obs: &Observation) -> bool {
        let r = PLAYER_RADIUS + 2.0;
        [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| self.nav.poly_at((obs.x + dx, obs.y + dy)))
            .any(|p| self.crushing(obs, self.nav.sector_of_poly(p)))
    }

    /// The shortest straight way out from under a crusher at work, when there's no route to take
    /// her: of sixteen headings, the one that gets all of her to a floor with no crusher at work
    /// over it soonest, walking (no step up or drop she couldn't come back from, nothing that
    /// hurts, room for her box all the way). All of her means her box's corners, not her middle
    /// a radius on along the heading: along a crusher's edge that test never gets her out.
    pub(super) fn way_out_of_crusher(&self, obs: &Observation) -> Option<Point> {
        let nav = &self.nav;
        let pos = obs.pos();
        let floor0 = nav
            .poly_at(pos)
            .and_then(|p| obs.height(nav.sector_of_poly(p)))
            .map(|h| h.floor)?;
        (0..16)
            .filter_map(|i| {
                let a = f64::from(i) * std::f64::consts::PI / 8.0;
                let d = (a.cos(), a.sin());
                let mut floor = floor0;
                for k in 1..=48 {
                    let f = f64::from(k) * 8.0;
                    let q = (pos.0 + d.0 * f, pos.1 + d.1 * f);
                    let s = nav.sector_of_poly(nav.poly_in(q)?);
                    let h = obs.height(s)?;
                    if (h.floor - floor).abs() > MAX_STEP
                        || (self.hurts(obs, s) && !self.on_hurting_floor(obs))
                    {
                        return None;
                    }
                    // A corner of her in the solid: a wall or a pillar's corner in the way.
                    let r = PLAYER_RADIUS - 1.0;
                    if [(-r, -r), (r, -r), (r, r), (-r, r)]
                        .iter()
                        .any(|(dx, dy)| nav.poly_in((q.0 + dx, q.1 + dy)).is_none())
                    {
                        return None;
                    }
                    if !self.box_in_crusher(obs, q) {
                        return Some((f, d));
                    }
                    floor = h.floor;
                }
                None
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, d)| d)
    }

    /// Does going `length` that way get her out from under a crusher?
    pub(super) fn gets_out(&self, obs: &Observation, d: Point, length: f64) -> bool {
        let p = (obs.x + d.0 * length, obs.y + d.1 * length);
        self.nav
            .poly_in(p)
            .is_some_and(|q| !self.no_go_crusher(obs, self.nav.sector_of_poly(q)))
    }

    /// Does going `length` that way get her off a floor that hurts?
    pub(super) fn gets_off(&self, obs: &Observation, d: Point, length: f64) -> bool {
        let p = (obs.x + d.0 * length, obs.y + d.1 * length);
        self.nav
            .poly_in(p)
            .is_some_and(|q| !self.hurts(obs, self.nav.sector_of_poly(q)))
    }

    /// How open it is round p: the mean of how far she could go from there each way, up to
    /// `length` (a corner's little; the middle of a hall all of it).
    pub fn openness(&self, obs: &Observation, p: Point, length: f64) -> f64 {
        let n = 8;
        (0..n)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * f64::from(i) / f64::from(n);
                self.clear_line(obs, p, (a.cos(), a.sin()), length)
            })
            .sum::<f64>()
            / f64::from(n)
            / length
    }

    fn clear_line(&self, obs: &Observation, from: Point, d: Point, length: f64) -> f64 {
        let nav = &self.nav;
        let Some(here) = nav.poly_at(from).or_else(|| nav.poly_at(obs.pos())) else {
            return 0.0;
        };
        let floor_of = |p: usize| {
            obs.height(nav.sector_of_poly(p))
                .unwrap_or(crate::nav::Heights {
                    floor: 0.0,
                    ceiling: 0.0,
                })
        };
        let mut floor = floor_of(here).floor;
        let n = ((length / 16.0).floor() as usize).max(2);
        for i in 0..=n {
            let f = length * i as f64 / n as f64;
            let q = (from.0 + d.0 * f, from.1 + d.1 * f);
            let back = length * (i.max(1) - 1) as f64 / n as f64;
            let Some(p) = nav.poly_in(q) else { return back };
            let h = floor_of(p);
            let s = nav.sector_of_poly(p);
            if (h.floor - floor).abs() > MAX_STEP
                || h.ceiling - h.floor < PLAYER_HEIGHT
                || (self.hurts(obs, s) && !self.on_hurting_floor(obs))
                || (self.no_go_crusher(obs, s) && !self.in_crusher(obs))
            {
                return back;
            }
            floor = h.floor;
        }
        length
    }

    /// A monster near and in front of her (as she faces: whatever she's fighting close, the route
    /// waits for).
    pub(super) fn blocking(&self, obs: &Observation, t: Option<&Thing>) -> bool {
        t.is_some_and(|t| {
            dist((t.x, t.y), obs.pos()) < 320.0
                && norm(heading(obs.pos(), (t.x, t.y)) - obs.angle).abs() < 60.0
        })
    }

    /// A monster near and ahead the way the route walks her: walking on would walk into it. By
    /// the route's way, not her facing: she faces what she aims at, so by facing anything near
    /// that she was shooting at would count as in her way.
    pub(super) fn in_her_way(&self, obs: &Observation, t: &Thing) -> bool {
        self.route_way.is_some_and(|way| {
            dist((t.x, t.y), obs.pos()) < 320.0
                && norm(heading(obs.pos(), (t.x, t.y)) - way).abs() < 60.0
        })
    }

    /// Is it all right to go `length` that way: floor all the way, no drop or step up she
    /// couldn't come back from, nothing that hurts, room to stand? And likewise where her speed
    /// would carry her, pushing that way a moment and then letting go?
    pub(super) fn safe(&self, obs: &Observation, d: Point, length: f64) -> bool {
        let nav = &self.nav;
        let Some(here) = nav.poly_at(obs.pos()) else {
            return false;
        };
        let floor_of = |p: usize| {
            obs.height(nav.sector_of_poly(p))
                .unwrap_or(crate::nav::Heights {
                    floor: 0.0,
                    ceiling: 0.0,
                })
        };
        // From where she stands, not the floor under her middle: on a ledge's lip with her middle
        // over the drop, the game keeps her up at the highest floor her box is on. Judged from the
        // floor below, the drop would not look like one.
        let floor0 = obs
            .z
            .map_or(floor_of(here).floor, |z| z.max(floor_of(here).floor));
        if self.clear_run(obs, d, length) < length {
            return false;
        }
        self.slide_ok(obs, floor0, Some(d))
    }

    /// Would letting go of the controls now leave her where she stands, or near it? Or does the
    /// speed she has carry her over a drop she couldn't come back from, or onto a floor that
    /// hurts (landed from a jump on a narrow strip, say, with the fight wanting her nowhere)?
    pub(super) fn coasting_off(&self, obs: &Observation) -> bool {
        let nav = &self.nav;
        let Some(here) = nav.poly_at(obs.pos()) else {
            return false;
        };
        let floor = obs
            .height(nav.sector_of_poly(here))
            .map_or(0.0, |h| h.floor);
        !self.slide_ok(obs, obs.z.map_or(floor, |z| z.max(floor)), None)
    }

    /// The slide from where she is, pushing `push` for the first `PUSH_TICS` (`None`: not at
    /// all), from standing at `floor0`. A wall or a step up stops her, which is all right; a
    /// drop, a hurting floor or a teleporter's front isn't (her middle over the pad from that
    /// side and she is somewhere else).
    fn slide_ok(&self, obs: &Observation, floor0: f64, push: Option<Point>) -> bool {
        let nav = &self.nav;
        let floor_of = |p: usize| {
            obs.height(nav.sector_of_poly(p))
                .unwrap_or(crate::nav::Heights {
                    floor: 0.0,
                    ceiling: 0.0,
                })
        };
        let teleports: Vec<(Point, Point)> = nav
            .map
            .linedefs
            .iter()
            .filter(|l| TELEPORTERS_FOR_PLAYERS.contains(&l.special))
            .map(|l| nav.map.line_seg(l.index))
            .collect();
        // Crossing (a, b) from its front, the right of a->b, as the game's P_CrossSpecialLine and
        // EV_Teleport take it.
        let side = |(a, b): (Point, Point), p: Point| {
            (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
        };
        let teleported = |from: Point, to: Point| {
            teleports.iter().any(|&l| {
                side(l, from) <= 0.0 && side(l, to) > 0.0 && segments_cross(from, to, l.0, l.1)
            })
        };
        let (mut x, mut y) = obs.pos();
        let (mut vx, mut vy) = self.velocity;
        let mut floor = floor0;
        for k in 0..SLIDE_TICS {
            if let Some(d) = push.filter(|_| k < PUSH_TICS) {
                vx += RUN_THRUST * d.0;
                vy += RUN_THRUST * d.1;
            } else if vx * vx + vy * vy < 1.0 {
                break;
            }
            if teleported((x, y), (x + vx, y + vy)) {
                return false;
            }
            x += vx;
            y += vy;
            vx *= FRICTION;
            vy *= FRICTION;
            let Some(p) = nav.poly_in((x, y)) else { break };
            let h = floor_of(p);
            if h.floor - floor > MAX_STEP || h.ceiling - h.floor < PLAYER_HEIGHT {
                break;
            }
            if floor - h.floor > MAX_STEP
                || (self.hurts(obs, nav.sector_of_poly(p))
                    && !self.on_hurting_floor(obs)
                    && !self.now.invulnerable)
            {
                return false;
            }
            floor = h.floor;
        }
        true
    }
}
