//! Missile avoidance, strafing and checking safe escape directions.

use super::{clearance, miss_by, Combat, DODGE_TICS};
use crate::geom::{dist, Point};
use crate::motor::{heading, norm, Observation, Thing, FRICTION, RUN_THRUST};
use crate::navigator::{MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::tactical::bestiary::{missile, weapon_in, Weapon};
use std::collections::BTreeSet;

const DODGE_WAYS: usize = 16;
/// a dodge along a missile's way (straight from it or at it) counts this much less than across it
const DODGE_ALONG: f64 = 24.0;

/// a clear dodge's worth for going the way she's going (an errand), all the way: as much as
/// room after it (24)
const W_TOWARD: f64 = 24.0;

/// going some way: pushing that way this long, then...
const PUSH_TICS: usize = 8;

/// ...letting go: where she'd slide to in this long must be safe too
const SLIDE_TICS: usize = 40;

/// a strafe one way lasts between these
const STRAFE_TICS: (i64, i64) = (18, 40);

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
                // (hers: her own plasma and rockets, dodged before, the fight given up; or gone
                // off already, standing in its blast frames, met as she runs at it)
                continue;
            }
            if t.name == "LostSoul" && t.vx.hypot(t.vy) < 8.0 {
                continue;
            }
            // (relative to her, as she's going, and were she to stop: aimed at where she was, it
            // hits if she turns back into it, as a route does, round a corner: Cyberdemon's
            // rockets met head on, 3 tics off)
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
                    if !(0.0 < when && when < DODGE_TICS as f64) {
                        return None;
                    }
                    let hz = t.z + t.vz * when;
                    if !(z - 24.0 < hz && hz < z + PLAYER_HEIGHT + 24.0) {
                        return None;
                    }
                    Some((when, (t.x + rvx * when - pos.0).hypot(t.y + rvy * when - pos.1)))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            if !self.dodging.contains(&t.id) && miss > clearance(&m) {
                continue;
            }
            if self.dodging.insert(t.id) {
                self.log.push(format!("tic {}: dodging {} {} (due in {when:.0} tics, {miss:.0} off)", obs.tic, t.name, t.id));
            }
            out.push((when, t.clone()));
        }
        let here: BTreeSet<i32> = out.iter().map(|(_, t)| t.id).collect();
        self.dodging.retain(|i| here.contains(i));
        if here.is_empty() {
            self.dodge_way = None;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.into_iter().map(|(_, t)| t).collect()
    }

    /// The way to run that keeps her furthest from all of them as they go by, where the floor's
    /// safe; the way she's going already when it's as good (no dithering).
    pub(super) fn dodge(&mut self, obs: &Observation, missiles: &[Thing]) -> Option<Point> {
        let pos = obs.pos();
        let mut ways: Vec<Point> = (0..DODGE_WAYS)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * i as f64 / DODGE_WAYS as f64;
                (a.cos(), a.sin())
            })
            .collect();
        if let Some(w) = self.dodge_way {
            ways.push(w);
        }
        let mut best: Option<(f64, Point)> = None;
        for way in ways {
            if !self.safe(obs, way, 64.0) {
                continue;
            }
            let room = self.clear_run(obs, way, 16.0 * DODGE_TICS as f64);
            let mut s = missiles.iter().filter_map(|t| missile(t.name).map(|m| miss_by(pos, self.velocity, way, room, t) - clearance(&m))).fold(f64::INFINITY, f64::min);
            s = s.min(48.0); // (clear is clear: then the rest decides)
            // (among those, where there's room after: dodged along a wall into a corner, the next
            // rocket had her)
            let ahead = (pos.0 + way.0 * room.min(160.0), pos.1 + way.1 * room.min(160.0));
            if s >= 0.0 {
                s += 24.0 * self.openness(obs, ahead, 192.0);
                // (across its way, not along it: a step aside clears its line for good; backing
                // straight off only puts it off, the faster she is the further, and it follows
                // her down the room. The user's: backing off along a fireball's path, far back,
                // when a strafe would do)
                let along = missiles
                    .iter()
                    .filter_map(|t| {
                        let v = t.vx.hypot(t.vy);
                        (v > 0.5).then(|| ((way.0 * t.vx + way.1 * t.vy) / v).abs())
                    })
                    .fold(0.0, f64::max);
                s -= DODGE_ALONG * along;
            }
            if let Some(dw) = self.dodge_way {
                s += 6.0 * (way.0 * dw.0 + way.1 * dw.1);
            }
            // (and, as clear, the way she's going: health, a gun, ammo across the room; dodging
            // where she stood, a rocket after a rocket, she never got there)
            if let Some(to) = self.toward.filter(|&to| s >= 0.0 && dist(to, pos) > 1.0) {
                let d = dist(to, pos);
                s += W_TOWARD * (way.0 * (to.0 - pos.0) + way.1 * (to.1 - pos.1)) / d;
            }
            if best.is_none_or(|(bs, _)| s > bs) {
                best = Some((s, way));
            }
        }
        self.dodge_way = best.map(|b| b.1);
        self.dodge_way
    }

    /// Back off from `what`: straight away if the floor's safe that way, else as near that as is.
    pub(super) fn away_from(&mut self, obs: &Observation, what: Point) -> Option<Point> {
        let away = (obs.y - what.1).atan2(obs.x - what.0);
        for off in [0.0f64, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0] {
            let a = away + off.to_radians();
            let d = (a.cos(), a.sin());
            if self.safe(obs, d, 64.0) {
                return Some(d);
            }
        }
        None
    }

    pub(super) fn gun(&self, obs: &Observation) -> Weapon {
        weapon_in(obs.weapon.filter(|w| (1..=7).contains(w)).unwrap_or(2), obs)
    }

    /// Away from t, aslant (the strafe's way and back), as far as it's safe.
    pub(super) fn keeping_off(&mut self, obs: &Observation, t: &Thing, aim: Option<f64>) -> Option<Point> {
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
    pub(super) fn closing_in(&mut self, obs: &Observation, t: &Thing, strafe: Option<Point>) -> Option<Point> {
        let a = (t.y - obs.y).atan2(t.x - obs.x);
        let ahead = (a.cos(), a.sin());
        let mut ways = vec![ahead];
        if let Some(s) = strafe {
            let mixed = (ahead.0 + s.0, ahead.1 + s.1);
            let n = mixed.0.hypot(mixed.1);
            if n > 0.1 {
                ways.insert(0, (mixed.0 / n, mixed.1 / n));
            }
        }
        ways.into_iter().find(|&w| self.safe(obs, w, 64.0)).or(strafe)
    }

    /// Side to side across the line of fire, a second or so each way, where it's safe.
    pub(super) fn strafe_dir(&mut self, obs: &Observation, aim: Option<f64>) -> Option<Point> {
        if obs.tic >= self.strafe_until {
            self.strafe = -self.strafe;
            let (lo, hi) = STRAFE_TICS;
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
        // (her middle and both sides: MAP07, the middle clear past a block's corner, the side
        // caught on it, pushing that way for ever)
        let side = (-d.1 * PLAYER_RADIUS, d.0 * PLAYER_RADIUS);
        [(0.0, 0.0), side, (-side.0, -side.1)].into_iter().map(|o| self.clear_line(obs, (obs.x + o.0, obs.y + o.1), d, length)).fold(length, f64::min)
    }

    /// Is she standing on a floor that hurts? Then that's no reason not to go a way (every way
    /// is over it; E1M5: stood still in the nukage, fighting, to death, 257 bullets left).
    pub(super) fn on_hurting_floor(&self, obs: &Observation) -> bool {
        self.nav.poly_at(obs.pos()).is_some_and(|p| self.hurts(obs, self.nav.sector_of_poly(p)))
    }

    /// Does sector s's floor hurt, or is it about to: by the map; as she knows it (seen turn to
    /// something that hurts since: `changed_floors`); or going down now to where it will (a
    /// walkway into the blood, E3M1's: backed off from imps on the far bank onto a stone
    /// just going down, not much of a step yet, in the blood with it a moment later).
    pub(super) fn hurts(&self, obs: &Observation, s: usize) -> bool {
        self.nav.damage[s] != 0
            || self.changed_floors.contains(&s)
            || (self.sinks_into_harm.contains(&s) && obs.height(s).is_some_and(|h| h.floor < self.nav.movers.initial[s].floor - 0.5))
    }

    /// Is she under a crusher at work?
    /// A crusher sector the fight's moves keep out of: on its way down, or at work at all.
    pub(super) fn no_go_crusher(&self, obs: &Observation, s: usize) -> bool {
        self.crushing(obs, s) || self.crusher_active(obs, s)
    }

    /// Is any of her in a crusher sector at work, coming down or not (`no_go_crusher`)?
    pub(super) fn in_crusher(&self, obs: &Observation) -> bool {
        let r = PLAYER_RADIUS + 2.0;
        [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| self.nav.poly_at((obs.x + dx, obs.y + dy)))
            .any(|p| self.no_go_crusher(obs, self.nav.sector_of_poly(p)))
    }

    /// Is any of her under a crusher at work? A crusher crushes whatever's even partly under it,
    /// and her middle out from under it isn't all of her (E3M5: her middle just out of a crusher's
    /// alcove, 14 units of her still under it, the fight let go, the route walked her back in).
    pub(super) fn under_crusher(&self, obs: &Observation) -> bool {
        let r = PLAYER_RADIUS + 2.0;
        [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| self.nav.poly_at((obs.x + dx, obs.y + dy)))
            .any(|p| self.crushing(obs, self.nav.sector_of_poly(p)))
    }

    /// The shortest straight way out from under a crusher at work (with no route to take her): of
    /// sixteen headings, the one that gets all of her to a floor with no crusher at work over it
    /// soonest, walking (no step up or drop she couldn't come back from, nothing that hurts).
    pub(super) fn way_out_of_crusher(&self, obs: &Observation) -> Option<Point> {
        let nav = &self.nav;
        let pos = obs.pos();
        let floor0 = nav.poly_at(pos).and_then(|p| obs.height(nav.sector_of_poly(p))).map(|h| h.floor)?;
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
                    if (h.floor - floor).abs() > MAX_STEP || (self.hurts(obs, s) && !self.on_hurting_floor(obs)) {
                        return None;
                    }
                    // (all of her out: her middle a radius and a bit past the last of it)
                    if !self.no_go_crusher(obs, s) && self.gets_out(obs, d, f + PLAYER_RADIUS + 2.0) {
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
        self.nav.poly_in(p).is_some_and(|q| !self.no_go_crusher(obs, self.nav.sector_of_poly(q)))
    }

    /// Does going `length` that way get her off a floor that hurts?
    pub(super) fn gets_off(&self, obs: &Observation, d: Point, length: f64) -> bool {
        let p = (obs.x + d.0 * length, obs.y + d.1 * length);
        self.nav.poly_in(p).is_some_and(|q| !self.hurts(obs, self.nav.sector_of_poly(q)))
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
        let Some(here) = nav.poly_at(from).or_else(|| nav.poly_at(obs.pos())) else { return 0.0 };
        let floor_of = |p: usize| obs.height(nav.sector_of_poly(p)).unwrap_or(crate::nav::Heights { floor: 0.0, ceiling: 0.0 });
        let mut floor = floor_of(here).floor;
        let n = ((length / 16.0).floor() as usize).max(2);
        for i in 0..=n {
            let f = length * i as f64 / n as f64;
            let q = (from.0 + d.0 * f, from.1 + d.1 * f);
            let back = length * (i.max(1) - 1) as f64 / n as f64;
            let Some(p) = nav.poly_in(q) else { return back };
            let h = floor_of(p);
            let s = nav.sector_of_poly(p);
            if (h.floor - floor).abs() > MAX_STEP || h.ceiling - h.floor < PLAYER_HEIGHT || (self.hurts(obs, s) && !self.on_hurting_floor(obs)) || (self.no_go_crusher(obs, s) && !self.in_crusher(obs)) {
                return back;
            }
            floor = h.floor;
        }
        length
    }

    /// A monster near and in front: walking on would walk into it.
    pub(super) fn blocking(&self, obs: &Observation, t: Option<&Thing>) -> bool {
        t.is_some_and(|t| dist((t.x, t.y), obs.pos()) < 320.0 && norm(heading(obs.pos(), (t.x, t.y)) - obs.angle).abs() < 60.0)
    }

    /// Is it all right to go `length` that way: floor all the way, no drop or step up she
    /// couldn't come back from, nothing that hurts, room to stand? And where her speed would
    /// carry her, pushing that way a moment and then letting go (E1M3: strafing by the start,
    /// slid into the nukage)?
    pub(super) fn safe(&self, obs: &Observation, d: Point, length: f64) -> bool {
        let nav = &self.nav;
        let Some(here) = nav.poly_at(obs.pos()) else { return false };
        let floor_of = |p: usize| obs.height(nav.sector_of_poly(p)).unwrap_or(crate::nav::Heights { floor: 0.0, ceiling: 0.0 });
        // (where she stands, not the floor under her middle: on a ledge's lip, her middle over the
        // drop, the game keeps her up at the highest floor her box is on; judged from the floor
        // below, the drop wasn't one. MAP06, walked off a crate top 552 down, then along the lip
        // of a nukage channel she couldn't climb out of, closing in on an imp)
        let floor0 = obs.z.map_or(floor_of(here).floor, |z| z.max(floor_of(here).floor));
        let mut floor;
        if self.clear_run(obs, d, length) < length {
            return false;
        }
        // (the slide: a wall or a step up stops her, which is all right; a drop or a hurting floor
        // isn't)
        let (mut x, mut y) = obs.pos();
        let (mut vx, mut vy) = self.velocity;
        floor = floor0;
        for k in 0..SLIDE_TICS {
            if k < PUSH_TICS {
                vx += RUN_THRUST * d.0;
                vy += RUN_THRUST * d.1;
            } else if vx * vx + vy * vy < 1.0 {
                break;
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
            if floor - h.floor > MAX_STEP || (self.hurts(obs, nav.sector_of_poly(p)) && !self.on_hurting_floor(obs) && !self.invulnerable(obs.tic)) {
                return false;
            }
            floor = h.floor;
        }
        true
    }
}
