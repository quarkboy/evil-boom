//! Path tracking, landing caution, pickup approach and recovery from obstacles.

use super::Trouble;
use super::{
    last_point, roll_speed, stops, Drop, Phase, RouteFollower, Run, SegEnd, Segment,
    CROSS_REST_WAIT, PRESS_RANGE,
};
use crate::geom::{cells, closest_point, dist, segment_distance, Point};
use crate::motor::{heading, Controls, Observation, Track, BRAKE, FULL_SPEED, TICRATE};
use crate::nav::{can_cross, Heights};
use crate::navigator::{
    closest_on_portal, PathNote, ITEM_RADIUS, MAX_STEP, PICKUP_BELOW, PLAYER_HEIGHT, PLAYER_RADIUS,
    TELEPORTERS_FOR_PLAYERS,
};
use crate::specials::Trigger;
use crate::walkthrough::{fmt_heights, Action, Step};
use std::collections::BTreeSet;
use std::rc::Rc;

/// arrival radius for points passed through
const REACHED: f64 = 12.0;

/// ...for points stopped at
const FINAL_REACHED: f64 = 10.0;

/// ...for pickup spots (they can be near the edge of touching range)
const PICKUP_REACHED: f64 = 6.0;

/// a barrel this close when stuck is what's in the way
const BARREL_IN_WAY: f64 = 40.0;

/// a drop this near ahead onto a narrow landing...
const CREEP_RANGE: f64 = 48.0;

/// ...narrower than this the way she's going...
const LANDING_ROOM: f64 = 192.0;

/// ...is stepped off no slower than this (units a tic)
const DROP_CREEP: f64 = 1.0;

/// how far ahead to look for crossings that became one-way drops
const DROP_CHECK_DIST: f64 = 48.0;

/// how exactly to hit a jump's run-up
const RUNUP_TOLERANCE: f64 = 6.0;

/// going down steps: in the air between them braking does nothing
const STAIRS_DOWN_SPEED: f64 = 8.0;

/// along a thin ledge (drops either side of the box): walk, don't run
const LEDGE_SPEED: f64 = 6.0;

const LEDGE_NEAR: f64 = PLAYER_RADIUS + 8.0;

const CAUTION_AHEAD: f64 = 160.0;

/// steer towards the path this far ahead
const TRACK_AHEAD: f64 = 32.0;

/// ...and look this far ahead
const LOOK_AHEAD: f64 = 128.0;

/// look-aheads to try steering for (longest first)
const CARROT_STEPS: [f64; 4] = [72.0, 56.0, 40.0, 28.0];

/// no progress this long while trying to move: back off and try again
const STUCK_TICS: i64 = 35;

const UNSTICK_TICS: i64 = 12;

const UNSTICKS_BEFORE_REPLAN: i32 = 2;

impl RouteFollower {
    /// Walking the segment's track: what comes before the walking (a jump or a barrel in hand,
    /// a floor going down under her, a lift to wait for, a drop, a crusher, a door), getting
    /// there, getting unstuck, and then the steering and the pace.
    pub(super) fn walk(&mut self, obs: &Observation) -> Controls {
        if self.jump.sprint.is_some() {
            return self.sprint_jump(obs);
        }
        if self.jump.run.is_some() {
            return self.run_step(obs);
        }
        if self.shot.target.is_some() {
            return self.shoot_barrel(obs);
        }
        if let Some(c) = self.barrels_first(obs) {
            return c;
        }
        let step = self.step().clone();
        if let Some(c) = self.off_sinking_floor(obs, &step) {
            return c;
        }
        if let Some(c) = self.lift_window_wait(obs, &step) {
            return c;
        }
        if obs.tic < self.jump.brake_until {
            if self.motion.speed() > 1.0 {
                // just landed a jump: stop first
                self.progress_tic = obs.tic;
                return self.motion.hold(obs, (0.0, 0.0), self.look);
            }
            self.jump.brake_until = -1;
        }
        let seg = self.segments[self.seg_i].clone();
        let track = &seg.track;
        let pos = obs.pos();
        (self.at.index, self.at.along, self.at.off) = track.project(pos, self.at.along, 64.0);
        if self.lost(obs) {
            if !self.replan(obs, "off course", false) {
                return self.fail(obs, Trouble::OffCourse);
            }
            return self.walk(obs);
        }
        self.speed_cap = f64::INFINITY;
        match self.unexpected_drop(obs) {
            Drop::None => {}
            Drop::Replanned => return self.walk(obs),
            Drop::GiveBack(why) => return self.fail(obs, why),
        }
        self.creep_to_drop(obs);
        // (a crusher before a mover: what it's waited for differs, and where)
        if let Some(c) = self.crusher_ahead(obs) {
            return c;
        }
        if let Some(c) = self.check_blocked(obs) {
            return c;
        }
        let end = track.pts[track.pts.len() - 1];
        let last = self.seg_i == self.segments.len() - 1;
        let stop = seg.end == SegEnd::Jump || (last && stops(step.action));
        // near the end only counts once she's come along the track (paths double back)
        let to_end = if track.length - self.at.along < 48.0 {
            dist(pos, end)
        } else {
            f64::INFINITY
        };
        if seg.end == SegEnd::Jump && to_end <= RUNUP_TOLERANCE {
            if let Some(j) = &seg.jump {
                return self.line_up(obs, j);
            }
        }
        if step.action == Action::Pickup
            && obs
                .keys
                .is_some_and(|k| step.key.is_some_and(|sk| k.has(sk)))
        {
            self.phase = Phase::Act; // (got it, maybe on the way)
            return self.act_at_goal(obs);
        }
        let arrive = if stop {
            FINAL_REACHED
        } else if step.action == Action::Pickup {
            PICKUP_REACHED
        } else {
            REACHED
        };
        if seg.end == SegEnd::Goal && (to_end < arrive || self.touching_item(pos, obs)) {
            self.reset_progress(obs.tic);
            if step.action == Action::Pickup && obs.keys.is_some() && self.item.is_some() {
                self.phase = Phase::Fetch; // (at the spot but not holding it: go and get it)
                return self.fetch(obs);
            }
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        // progress, and getting unstuck
        if self.at.along > self.at.furthest + 2.0 {
            if self.at.furthest != f64::NEG_INFINITY {
                self.unstick.count = 0; // (got past whatever snagged her)
            }
            self.at.furthest = self.at.along;
            self.progress_tic = obs.tic;
        }
        if obs.tic < self.unstick.until {
            return self.unstick(obs);
        }
        let trig_line = step.trigger.as_ref().and_then(|t| t.linedef());
        if obs.tic - self.progress_tic > STUCK_TICS
            && step.action == Action::Use
            && self.seg_i == self.segments.len() - 1
            && trig_line.is_some_and(|li| self.line_distance(pos, li) <= PRESS_RANGE)
        {
            // can't get to the spot, but the switch is in reach from here (past a shut door the
            // press opens first, say): press from here
            self.reset_progress(obs.tic);
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        if let Some(c) = self.pinned_under_ceiling(obs) {
            return c;
        }
        if obs.tic - self.progress_tic > STUCK_TICS {
            return self.stuck(obs);
        }
        // follow the track: steer for a point a little ahead, look further ahead
        let mut carrot = self.carrot(pos, track);
        if seg.end == SegEnd::Teleport && to_end < REACHED {
            carrot = seg.look.pts[seg.look.pts.len() - 1]; // (keep going through the line)
        }
        // (barrels standing in the track: by a lane between them to where it's clear again)
        let on: Vec<Point> = [192.0, 320.0, 512.0, 96.0, 768.0]
            .into_iter()
            .map(|d| track.point_at((self.at.along + d).min(track.length)))
            .collect();
        carrot = self.lane_carrot(obs, carrot, &on);
        let end_speed = if stop {
            0.0
        } else if seg.end == SegEnd::Teleport {
            FULL_SPEED
        } else {
            roll_speed(&seg)
        };
        // what's left of the track: off its end, as far as the end really is (braked to a stop
        // past the end, she isn't there)
        let rest = (track.length - self.at.along).max(dist(pos, end));
        let mut speed = FULL_SPEED
            .min(track.speed_limit(track.length - rest, end_speed))
            .min(self.speed_cap);
        if stop {
            speed = speed.min(0.3 * to_end + 0.5);
            if self.by_a_drop(obs, end) {
                // (stopping at the brink of a drop: in gently, slowly over the last stretch, as for
                // a run-up; run at off a step down she's in the air, where braking does nothing, and
                // over the lip with no way back up)
                speed =
                    speed
                        .min(0.06 * rest + 1.5)
                        .min(if rest < 128.0 { 6.0 } else { FULL_SPEED });
            }
        }
        if seg.end == SegEnd::Jump {
            // run-ups are often right at a ledge: in gently, slowly over the last stretch
            // (braking does nothing between steps down: a run-up at the foot of a stair is
            // overshot into what's beyond)
            if self.steps_down_to_end(obs, track) {
                speed =
                    speed
                        .min(0.06 * rest + 1.5)
                        .min(if rest < 128.0 { 6.0 } else { FULL_SPEED });
            } else {
                speed = speed.min(0.12 * rest + 1.5);
            }
        }
        speed = speed.min(self.caution_speed(obs, track));
        if crate::debug::on().pace && obs.tic % 10 == 0 {
            eprintln!("tic {}: at ({:.0}, {:.0}) speed {speed:.1} (cap {:.1}, track {:.1}, caution {:.1}, stop {stop}, rest {rest:.0}), carrot ({:.0}, {:.0}), going {:?}", obs.tic, pos.0, pos.1, self.speed_cap, track.speed_limit(track.length - rest, end_speed), self.caution_speed(obs, track), carrot.0, carrot.1, obs.velocity);
        }
        let look = self.look_angle(obs, &seg);
        if dist(pos, carrot) < 1.0 {
            return self.motion.hold(obs, (0.0, 0.0), Some(look));
        }
        self.motion.go(obs, heading(pos, carrot), speed, Some(look))
    }

    /// The barrels she sees, for her steering; and those to deal with before walking on: a
    /// chain going off to keep out of, a barrel this step is to clear.
    fn barrels_first(&mut self, obs: &Observation) -> Option<Controls> {
        {
            // (but for one she's against already: the way leads off from it)
            let pos = obs.pos();
            self.weave.barrels = obs
                .barrels
                .iter()
                .flatten()
                .copied()
                .filter(|&b| {
                    !((pos.0 - b.0).abs() < super::lanes::ROOM
                        && (pos.1 - b.1).abs() < super::lanes::ROOM)
                })
                .collect();
        }
        if !self.barrel_run {
            if let Some(c) = self.keep_off_lit_chain(obs) {
                return Some(c);
            }
        }
        // (a barrel to clear on this step: taken on at once, shot from the nearest place out of
        // every blast of its chain, then on)
        let b = self
            .step()
            .barrel
            .filter(|_| self.shot.step != Some(self.step_i))
            .and_then(|b| super::barrel_now(obs, b))?;
        self.shot.step = Some(self.step_i);
        self.log.add(
            obs.tic,
            format!("clearing the barrel at ({:.0}, {:.0})", b.0, b.1),
        );
        self.shot.target = Some((b, obs.tic));
        Some(self.shoot_barrel(obs))
    }

    /// The floor she's walking on going down when nothing she's doing means it to (no move
    /// she's waiting on, not the step's own line): off it to the nearest floor that stays put,
    /// at once. Walking on to what she'd planned from it, a jump say, is from a floor that's no
    /// longer where the plan had it.
    fn off_sinking_floor(&mut self, obs: &Observation, step: &Step) -> Option<Controls> {
        let nav = Rc::clone(&self.view);
        let s = nav
            .nav
            .mesh
            .poly_at(obs.pos())
            .map(|p| nav.nav.sector_of_poly(p))?;
        let on_floor = obs.z.is_none_or(|z| z <= obs.heights[s].floor + 0.5);
        // (going down now, lower than a tic ago: not just lower than it started. A floor gone
        // down for good is a floor, nukage or not: a bridge lowered into it the first time
        // across is walked over after, not run off each time)
        let going_down = obs
            .height(s)
            .zip(self.prev_live.get(s))
            .is_some_and(|(l, p)| l.floor < p.floor - 0.01);
        let expected = self.watches.iter().any(|w| w.expect.sector == s && !w.over)
            || step.trigger.as_ref().is_some_and(|t| t.effect(s).is_some());
        // (and only with somewhere to run to: down in the pit already, it's for the explorer
        // to find the way out)
        // (and only a floor going down into what hurts, as the triggers that move it have it:
        // a lift going down under her is a lift)
        let into_harm = || {
            nav.nav.triggers.iter().any(|t| {
                t.info.trigger.walked()
                    && t.effect(s).is_some()
                    && nav.nav.floor_after(t, s) == Some(true)
            })
        };
        if on_floor
            && going_down
            && self.jump.sinking.contains(&s)
            && !expected
            && into_harm()
            && self.run_path(obs).is_some()
        {
            self.log.add(
                obs.tic,
                format!(
                    "sector {s} going down under her: running for the nearest floor that stays put"
                ),
            );
            self.jump.run = Some(Run {
                heading: obs.angle,
                sector: None,
                next: obs.tic,
            });
            return Some(self.run_step(obs));
        }
        None
    }

    /// A lift line to cross for its window: if the lift's away already (set off on the way),
    /// crossing now does nothing and the window is shorter than planned. Waited for, a while.
    fn lift_window_wait(&mut self, obs: &Observation, step: &Step) -> Option<Controls> {
        let t = step.trigger.as_ref().filter(|t| {
            step.action == Action::Cross && t.info.trigger == Trigger::WR && !t.permanent()
        })?;
        if self.cross_ready {
            return None;
        }
        let nav = Rc::clone(&self.view);
        let init = &nav.nav.movers.initial;
        let away: Vec<usize> = t
            .effects
            .iter()
            .map(|(s, _)| *s)
            .filter(|&x| {
                obs.height(x)
                    .is_some_and(|live| (live.floor - init[x].floor).abs() > 0.5)
            })
            .collect();
        if !away.is_empty() && obs.tic - self.step_tic < CROSS_REST_WAIT {
            if !self.cross_wait_logged {
                self.log.add(obs.tic, format!("sector {} already away: waiting for it to come back before crossing line {}", away[0], t.line));
                self.cross_wait_logged = true;
            }
            self.progress_tic = obs.tic;
            return Some(self.motion.hold(obs, (0.0, 0.0), Some(obs.angle)));
        }
        self.cross_ready = true;
        None
    }

    /// Nowhere for `STUCK_TICS`: back off and try again; after a couple of those (or at once
    /// on a floor that hurts), a new way; wedged against a barrel, shoot it.
    fn stuck(&mut self, obs: &Observation) -> Controls {
        let pos = obs.pos();
        self.unstick.count += 1;
        // (on a floor that hurts, a new way at once, no backing off first: each try costs her
        // health, and the way the plan saw may have gone with a floor that moved under her,
        // a ledge lowered by the line she crossed)
        let burning = {
            let nav = &self.view.nav;
            nav.poly_at(pos).is_some_and(|p| {
                nav.damage
                    .get(nav.sector_of_poly(p))
                    .is_some_and(|&d| d != 0)
            })
        };
        if self.unstick.count > UNSTICKS_BEFORE_REPLAN || burning {
            // (with the heights as they are: the way the plan saw may be shut now, a timed door
            // closed, and the same path again is stuck again)
            if !self.replan(obs, "stuck", true) {
                return self.fail(obs, Trouble::Stuck);
            }
            return self.walk(obs);
        }
        if let Some(b) = obs
            .barrels
            .iter()
            .flatten()
            .copied()
            .find(|&b| !self.barrel_run && dist(b, pos) < BARREL_IN_WAY)
        {
            // wedged against a barrel (they stand in rows across corridors): blow it up from a
            // safe distance, as a player would
            self.log.add(
                obs.tic,
                format!("barrel in the way at ({:.0}, {:.0}): shooting it", b.0, b.1),
            );
            self.shot.target = Some((b, obs.tic));
            return self.shoot_barrel(obs);
        }
        self.log.add(
            obs.tic,
            format!("stuck at ({:.0}, {:.0}), backing off", obs.x, obs.y),
        );
        self.unstick.until = obs.tic + UNSTICK_TICS;
        self.progress_tic = obs.tic + UNSTICK_TICS;
        self.unstick.dir = self.away_from_walls(obs);
        self.unstick(obs)
    }

    /// Where to steer for with barrels about: `carrot` if the line to it is clear of them; else
    /// by a lane between them (`lanes::lane`) to the first of `on` (points further on her
    /// track) that's clear and has one, kept to till she's through; no lane, the sidestep
    /// (`round_barrel`). The mesh's way has no barrels in it; she plans round the ones she sees
    /// as she sees them.
    fn lane_carrot(&mut self, obs: &Observation, carrot: Point, on: &[Point]) -> Point {
        use super::lanes::{chord_clear_of, clear_of, lane};
        let pos = obs.pos();
        while self
            .weave
            .detour
            .first()
            .is_some_and(|&q| dist(pos, q) < 12.0)
        {
            self.weave.detour.remove(0);
        }
        if let Some(&q) = self.weave.detour.first() {
            if chord_clear_of(&self.weave.barrels, pos, q) {
                return q;
            }
            self.weave.detour.clear(); // (one she hadn't seen, in the lane: again)
        }
        if chord_clear_of(&self.weave.barrels, pos, carrot) {
            return carrot;
        }
        if obs.tic - self.weave.detour_tic >= 8 {
            let nav = &self.view.nav;
            let floor = |p: Point| {
                let at = |q: Point| {
                    nav.poly_at(q)
                        .map(|i| nav.sector_of_poly(i))
                        .filter(|&s| nav.damage.get(s).is_none_or(|&d| d == 0))
                        .and_then(|s| obs.height(s))
                        .filter(|h| h.ceiling - h.floor >= crate::navigator::PLAYER_HEIGHT)
                };
                let r = PLAYER_RADIUS + 1.0;
                let mid = at(p)?.floor;
                [(-r, -r), (r, -r), (r, r), (-r, r)]
                    .into_iter()
                    .all(|(dx, dy)| {
                        at((p.0 + dx, p.1 + dy)).is_some_and(|h| (h.floor - mid).abs() <= MAX_STEP)
                    })
                    .then_some(mid)
            };
            let found = on
                .iter()
                .copied()
                .filter(|&q| dist(q, pos) > 32.0 && clear_of(&self.weave.barrels, q))
                .find_map(|q| lane(pos, q, &[pos, q], &self.weave.barrels, &floor, MAX_STEP));
            match found {
                Some(way) if !way.is_empty() => {
                    self.weave.detour = way;
                    return self.weave.detour[0];
                }
                _ => self.weave.detour_tic = obs.tic,
            }
        }
        self.round_barrel(obs, carrot)
    }

    /// A barrel standing in the way to `carrot`: round it, or between it and the next (a row of
    /// them 64 apart leaves her 12 to spare: through the middle of a gap), by whichever such way
    /// is shortest with floor there and no other barrel in it; none, `carrot` as it was (wedged
    /// against it, she shoots it). Weaving between them, as a player does: the route's way goes
    /// by the mesh, which has no barrels in it, and stood against a row of them she's there when
    /// something sets the lot off.
    fn round_barrel(&self, obs: &Observation, carrot: Point) -> Point {
        /// her box and a barrel's side by side (16 and 10), and a little
        const ROOM: f64 = PLAYER_RADIUS + 10.0 + 1.0;
        /// how far out from a barrel's middle she passes it with nothing the other side
        const PASS: f64 = ROOM + 10.0;
        /// another barrel this near one makes a gap to go through the middle of
        const ROW: f64 = 160.0;
        let pos = obs.pos();
        let d = dist(pos, carrot);
        if d < 1.0 {
            return carrot;
        }
        let u = ((carrot.0 - pos.0) / d, (carrot.1 - pos.1) / d);
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        // (one she's beside already, or as good as, isn't in her way)
        let ahead = |b: Point| (b.0 - pos.0) * u.0 + (b.1 - pos.1) * u.1 > 8.0;
        // (a way's clear of a barrel that stays its room off it; or that she's against now
        // and the way leads off from)
        let passes = |q: Point, o: Point| {
            segment_distance(pos, q, o, o) >= ROOM
                || (dist(pos, o) < ROOM + 4.0
                    && (q.0 - pos.0) * (o.0 - pos.0) + (q.1 - pos.1) * (o.1 - pos.1) <= 0.0)
        };
        let Some(&b) = barrels
            .iter()
            .filter(|&&b| ahead(b) && segment_distance(pos, carrot, b, b) < ROOM)
            .min_by(|x, y| dist(**x, pos).total_cmp(&dist(**y, pos)))
        else {
            return carrot;
        };
        let n = (-u.1, u.0);
        let sides = [1.0, -1.0]
            .into_iter()
            .map(|k| (b.0 + n.0 * PASS * k, b.1 + n.1 * PASS * k));
        let gaps = barrels
            .iter()
            .filter(|&&o| dist(o, b) > 2.0 * ROOM && dist(o, b) < ROW)
            .map(|&o| ((b.0 + o.0) / 2.0, (b.1 + o.1) / 2.0));
        let nav = &self.view.nav;
        sides
            .chain(gaps)
            .filter(|&q| {
                nav.poly_at(q).is_some_and(|p| {
                    nav.damage
                        .get(nav.sector_of_poly(p))
                        .is_none_or(|&h| h == 0)
                }) && dist(pos, q) > 16.0
                    && self.chord_clear(pos, q)
                    && barrels.iter().all(|&o| passes(q, o))
            })
            .min_by(|x, y| {
                (dist(pos, *x) + dist(*x, carrot)).total_cmp(&(dist(pos, *y) + dist(*y, carrot)))
            })
            .unwrap_or_else(|| {
                if crate::debug::on().pace && obs.tic % 10 == 0 {
                    eprintln!(
                        "  no way round the barrel at ({:.0}, {:.0}) from ({:.0}, {:.0})",
                        b.0, b.1, pos.0, pos.1
                    );
                }
                carrot
            })
    }

    /// How fast to go for what's just ahead: down steps (braking does nothing dropping
    /// between them), and along a thin ledge with drops close by on both sides (walk it).
    fn caution_speed(&self, obs: &Observation, track: &Track) -> f64 {
        let nav = &self.view.nav;
        let (mesh, m) = (&nav.mesh, &nav.map);
        let drops = nav.drop_lines();
        let mut steps_down: Vec<f64> = Vec::new();
        let mut prev: Option<f64> = None;
        let ahead = CAUTION_AHEAD.min(track.length - self.at.along);
        let n = ((ahead / 16.0).floor() as i64).max(1);
        for i in 0..=n {
            let f = ahead * i as f64 / n as f64;
            let q = track.point_at(self.at.along + f);
            let t = track.point_at(self.at.along + f + 1.0);
            let (dx, dy) = (t.0 - q.0, t.1 - q.1);
            let Some(poly) = mesh.poly_at(q) else {
                continue;
            };
            let floor = obs.height(mesh.poly(poly).sector).map_or(0.0, |h| h.floor);
            if prev.is_some_and(|p| floor < p) {
                steps_down.push(f);
            }
            prev = Some(floor);
            let mut sides: BTreeSet<bool> = BTreeSet::new();
            for cell in cells(
                q.0 - LEDGE_NEAR,
                q.1 - LEDGE_NEAR,
                q.0 + LEDGE_NEAR,
                q.1 + LEDGE_NEAR,
            ) {
                for &li in mesh.line_grid.get(&cell).into_iter().flatten() {
                    if !drops.contains(&li) {
                        continue;
                    }
                    let (Some(fs), Some(bs)) = m.sides(li) else {
                        continue;
                    };
                    let (Some(hf), Some(hb)) = (obs.height(fs), obs.height(bs)) else {
                        continue;
                    };
                    if (hf.floor - hb.floor).abs() <= MAX_STEP {
                        continue; // (not a drop at the heights now)
                    }
                    let (a, c) = m.line_seg(li);
                    if segment_distance(a, c, q, q) < LEDGE_NEAR {
                        let near = closest_point(q, a, c);
                        sides.insert(dx * (near.1 - q.1) - dy * (near.0 - q.0) > 0.0);
                    }
                }
            }
            if sides.len() == 2 {
                return LEDGE_SPEED;
            }
        }
        if steps_down.len() < 2 {
            return f64::INFINITY; // (one drop isn't a stair: slowing for it can miss a lift)
        }
        (STAIRS_DOWN_SPEED * STAIRS_DOWN_SPEED + 2.0 * BRAKE * steps_down[0]).sqrt()
    }

    /// Is p within reach of a drop (more than a step down, at the heights now)?
    fn by_a_drop(&self, obs: &Observation, p: Point) -> bool {
        let nav = &self.view.nav;
        let (mesh, m) = (&nav.mesh, &nav.map);
        let drops = nav.drop_lines();
        let near = 2.0 * LEDGE_NEAR;
        cells(p.0 - near, p.1 - near, p.0 + near, p.1 + near).any(|cell| {
            mesh.line_grid.get(&cell).into_iter().flatten().any(|&li| {
                let (Some(fs), Some(bs)) = (
                    drops.contains(&li).then_some(()).and(m.sides(li).0),
                    m.sides(li).1,
                ) else {
                    return false;
                };
                let (Some(hf), Some(hb)) = (obs.height(fs), obs.height(bs)) else {
                    return false;
                };
                let (a, c) = m.line_seg(li);
                (hf.floor - hb.floor).abs() > MAX_STEP && segment_distance(a, c, p, p) < near
            })
        })
    }

    /// Where the item a pickup step is for lies (the nearest such thing to its goal).
    pub(super) fn item_of(&self) -> Option<Point> {
        let step = self.step();
        let key = step.key.filter(|_| step.action == Action::Pickup)?;
        let goal = last_point(step.path.as_ref()?)?;
        self.view
            .nav
            .map
            .things
            .iter()
            .filter(|t| t.kind == key as u16)
            .map(|t| (t.x, t.y))
            .min_by(|a, b| dist(*a, goal).total_cmp(&dist(*b, goal)))
    }

    /// Close enough that the boxes overlap, and at a height the game lets her have it from:
    /// picked up, wherever the spot was. (Not from a floor well above an item in a sunken strip,
    /// the boxes overlapping over the edge: the game wants it no more than 8 under her feet.)
    fn touching_item(&self, pos: Point, obs: &Observation) -> bool {
        let Some(it) = self.item else { return false };
        if (pos.0 - it.0).abs().max((pos.1 - it.1).abs()) >= PLAYER_RADIUS + ITEM_RADIUS - 1.0 {
            return false;
        }
        let nav = &self.view.nav;
        match (obs.z, nav.poly_at(it)) {
            (Some(z), Some(p)) => (-PICKUP_BELOW..=PLAYER_HEIGHT)
                .contains(&(obs.heights[nav.sector_of_poly(p)].floor - z)),
            _ => true,
        }
    }

    /// The point to steer for: as far ahead on the track as the straight line there keeps the
    /// box clear (corners rounded where there's room), else the next waypoint exactly.
    fn carrot(&self, pos: Point, track: &Track) -> Point {
        for ahead in CARROT_STEPS {
            let q = track.point_at(self.at.along + ahead);
            // (a path that doubles back can put a point far along it right next to her)
            let along = ahead.min(track.length - self.at.along);
            if dist(pos, q) < 0.6 * along {
                continue;
            }
            // (no corner cut into a barrel)
            if dist(pos, q) < 1.0
                || (self.chord_clear(pos, q)
                    && super::lanes::chord_clear_of(&self.weave.barrels, pos, q))
            {
                return q;
            }
        }
        track.carrot(self.at.index, self.at.along, TRACK_AHEAD)
    }

    /// Can the box move straight from p to q without touching a wall, solid thing or
    /// (uncrossed) ledge? Edges she's close to already only need not get closer.
    pub(super) fn chord_clear(&self, p: Point, q: Point) -> bool {
        let nav = &self.view.nav;
        for (e1, e2) in nav.walls_near(
            p,
            q,
            2.0 * PLAYER_RADIUS,
            Some(&self.crossing_lines),
            Some(&self.live),
        ) {
            let length = dist(e1, e2).max(1e-6);
            let (c, s) = ((e2.0 - e1.0).abs() / length, (e2.1 - e1.1).abs() / length);
            let reach = PLAYER_RADIUS * (c + s) + 2.0; // (the box's extent across this edge)
            let d = segment_distance(p, q, e1, e2);
            if d < reach && d < segment_distance(p, p, e1, e2) - 0.5 {
                return false;
            }
        }
        // nor across a teleport line from its front (cutting a corner over one teleports her)
        let m = &nav.map;
        for cell in cells(p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)) {
            for &li in nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !TELEPORTERS_FOR_PLAYERS.contains(&m.linedefs[li].special)
                    || self.crossing_lines.contains(&li)
                {
                    continue;
                }
                let (v1, v2) = m.line_seg(li);
                let in_front = (v2.0 - v1.0) * (p.1 - v1.1) - (v2.1 - v1.1) * (p.0 - v1.0) < 0.0;
                if in_front && segment_distance(p, q, v1, v2) < 1.0 {
                    return false;
                }
            }
        }
        true
    }

    /// The view angle: towards a point well ahead on the path, holding the last one when that
    /// point is too near to give a steady direction.
    fn look_angle(&mut self, obs: &Observation, seg: &Segment) -> f64 {
        let look = &seg.look;
        let s = if look.length > seg.track.length {
            look.project(obs.pos(), self.at.along, 64.0).1
        } else {
            self.at.along
        };
        let point = look.point_at(s + LOOK_AHEAD);
        if dist(obs.pos(), point) >= 40.0 || self.look.is_none() {
            if dist(obs.pos(), point) > 1.0 {
                self.look = Some(heading(obs.pos(), point));
            } else if self.look.is_none() {
                self.look = Some(obs.angle);
            }
        }
        self.look.unwrap_or(obs.angle)
    }

    /// Which way to back off: away from the walls and ledges nearest (backing straight off a
    /// stair's wall can walk her off its other side), else backwards.
    fn away_from_walls(&self, obs: &Observation) -> f64 {
        let pos = obs.pos();
        let (mut ax, mut ay) = (0.0, 0.0);
        for (a, b) in self.view.nav.walls_near(
            pos,
            pos,
            40.0,
            Some(&self.crossing_lines),
            Some(&obs.heights),
        ) {
            let q = closest_point(pos, a, b);
            let d = dist(pos, q);
            if 1e-6 < d && d < 40.0 {
                ax += (pos.0 - q.0) / d * (40.0 - d);
                ay += (pos.1 - q.1) / d * (40.0 - d);
            }
        }
        let track = &self.segments[self.seg_i].track;
        let back = heading(track.point_at(self.at.along + TRACK_AHEAD), pos);
        if ax.hypot(ay) < 1e-6 {
            return back;
        }
        let away = ay.atan2(ax);
        let back_r = back.to_radians();
        // mostly sideways off what she's snagged on, a bit backwards
        heading(
            (0.0, 0.0),
            (
                away.cos() * 2.0 + back_r.cos(),
                away.sin() * 2.0 + back_r.sin(),
            ),
        )
    }

    fn unstick(&mut self, obs: &Observation) -> Controls {
        let seg = self.segments[self.seg_i].clone();
        let look = self.look_angle(obs, &seg);
        self.motion.go(obs, self.unstick.dir, 5.0, Some(look))
    }

    /// Does the last 128 units of the track go down any steps (live floors)?
    fn steps_down_to_end(&self, obs: &Observation, track: &Track) -> bool {
        let mesh = &self.view.nav.mesh;
        let start = (track.length - 128.0).max(0.0);
        let floors: Vec<f64> = (0..9)
            .filter_map(|k| {
                mesh.poly_at(track.point_at(start + (track.length - start) * f64::from(k) / 8.0))
                    .map(|q| obs.heights[mesh.poly(q).sector].floor)
            })
            .collect();
        floors.windows(2).any(|w| w[1] < w[0])
    }

    /// A crossing just ahead that's become a one-way drop the plan didn't mean to take (a line
    /// walked over on the way can sink the bridge she's on). Stop and replan with the heights as
    /// they are: on if there's a way without such a drop; else the objective goes back to be
    /// judged again, the ground as it is now (a floor raised on the way can leave the goal down
    /// in a pit, a drop into a trap).
    /// A drop just ahead onto a landing that's narrow the way she's going (a ledge over a pit):
    /// step off it slowly, as a falling player keeps her speed and slides on landing, straight
    /// over a ledge 48 across.
    fn creep_to_drop(&mut self, obs: &Observation) {
        let view = Rc::clone(&self.view);
        let mesh = &view.nav.mesh;
        let Some(path) = self.step().path.clone() else {
            return;
        };
        let pos = obs.pos();
        let upto = (self.poly_i + 4).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            let (a, b) = (obs.heights[cs], obs.heights[ns]);
            if cs == ns || a.floor - b.floor <= MAX_STEP {
                continue;
            }
            let Some(portal) = mesh
                .poly(cur)
                .portals
                .iter()
                .map(|&pi| &mesh.portals[pi])
                .find(|p| p.other(cur) == nxt)
            else {
                return;
            };
            let edge = closest_on_portal(pos, portal.p1, portal.p2);
            if dist(edge, pos) > CREEP_RANGE {
                return;
            }
            // (the way she'll go over: on from her to the edge, or, at it, the way the landing is)
            let to = if dist(edge, pos) > 4.0 {
                pos
            } else {
                mesh.poly(cur).center
            };
            let len = dist(edge, to).max(1e-6);
            let dir = ((edge.0 - to.0) / len, (edge.1 - to.1) / len);
            // (the floor she'd land on, how far on before it drops away again)
            let room = (1..)
                .map(|k| f64::from(k) * 8.0)
                .take_while(|&r| r < LANDING_ROOM)
                .find(|&r| {
                    mesh.poly_at((edge.0 + dir.0 * r, edge.1 + dir.1 * r))
                        .is_none_or(|q| obs.heights[mesh.poly(q).sector].floor < b.floor - MAX_STEP)
                });
            // (as fast as goes no further than that: the fall, about sqrt(2h) tics, and the
            // slide on landing, 1 / (1 - friction) tics' worth, at the speed she went over)
            if let Some(room) = room {
                let tics =
                    (2.0 * (a.floor - b.floor)).sqrt() + 1.0 / (1.0 - crate::motor::FRICTION);
                self.speed_cap = self.speed_cap.min((room / tics).max(DROP_CREEP));
            }
            return;
        }
    }

    fn unexpected_drop(&mut self, obs: &Observation) -> Drop {
        let view = Rc::clone(&self.view);
        let mesh = &view.nav.mesh;
        let Some(path) = self.step().path.clone() else {
            return Drop::None;
        };
        // (the drops the plan means to take, this step's and the rest's)
        let planned: BTreeSet<usize> = self.steps[self.step_i as usize..]
            .iter()
            .filter_map(|st| st.path.as_ref())
            .flat_map(|p| p.notes.iter())
            .filter_map(|n| match n {
                PathNote::Drop { sector, .. } => Some(*sector),
                _ => None,
            })
            .collect();
        let upto = (self.poly_i + 4).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            if cs == ns {
                continue;
            }
            let Some(portal) = mesh
                .poly(cur)
                .portals
                .iter()
                .map(|&pi| &mesh.portals[pi])
                .find(|p| p.other(cur) == nxt)
            else {
                return Drop::None;
            };
            if dist(
                closest_on_portal(obs.pos(), portal.p1, portal.p2),
                obs.pos(),
            ) > DROP_CHECK_DIST
            {
                return Drop::None;
            }
            let (a, b) = (obs.heights[cs], obs.heights[ns]);
            let key = (
                cs,
                ns,
                [
                    a.floor.to_bits(),
                    a.ceiling.to_bits(),
                    b.floor.to_bits(),
                    b.ceiling.to_bits(),
                ],
            );
            if planned.contains(&ns)
                || self.drops_ok.contains(&key)
                || !can_cross(a, b)
                || can_cross(b, a)
            {
                continue;
            }
            if !self.states[cs]
                .iter()
                .any(|&ha| self.states[ns].iter().any(|&hb| can_cross(hb, ha)))
            {
                continue; // (one-way in the plan as well)
            }
            // (or the lift it'd drop off, on its way down to level: pressed to be ridden, its far
            // side is a drop till it's arrived)
            let coming = self
                .watches
                .iter()
                .find(|w| {
                    let fits = if w.expect.sector == ns {
                        can_cross(a, w.expect.heights) && can_cross(w.expect.heights, a)
                    } else {
                        w.expect.sector == cs
                            && can_cross(w.expect.heights, b)
                            && can_cross(b, w.expect.heights)
                    };
                    fits && w.reached.is_none()
                        && ((obs.tic - w.since) as f64) < 1.5 * w.expect.travel + 10.0 * TICRATE
                })
                .map(|w| w.expect.heights);
            if let Some(h) = coming {
                // still on its way to where the plan has it (a floor can take half a minute to rise
                // to a walkway): wait at the edge
                if !self.edge_wait {
                    self.log.add(
                        obs.tic,
                        format!("waiting for sector {ns} to reach {}", fmt_heights(h)),
                    );
                    self.edge_wait = true;
                }
                self.speed_cap = 0.0;
                self.progress_tic = obs.tic;
                self.step_tic = obs.tic;
                return Drop::None;
            }
            self.drops_ok.insert(key);
            let why = format!("unexpected drop into sector {ns}");
            // (heights she allowed for, a lift down that she counted on coming up: only the timing
            // is off, on by the drop as before. Heights she didn't: what's beyond is for the
            // explorer to judge again)
            let allowed = |s: usize, h: Heights| {
                self.states[s]
                    .iter()
                    .any(|x| (x.floor - h.floor).abs() < 1.0 && (x.ceiling - h.ceiling).abs() < 1.0)
            };
            // (or one the plan is waiting to see move, caught on its way: a lift pressed to be got
            // onto, sinking under her as she stands by its switch)
            let on_its_way = |s: usize| {
                self.watches
                    .iter()
                    .any(|w| w.expect.sector == s && w.reached.is_none())
            };
            let foreseen = (allowed(cs, a) || on_its_way(cs)) && (allowed(ns, b) || on_its_way(ns));
            if !self.replan(obs, &why, true) {
                return if foreseen {
                    Drop::None
                } else {
                    Drop::GiveBack(Trouble::Drop(format!("{why}, and no other way on")))
                };
            }
            if foreseen {
                return Drop::Replanned;
            }
            // (the new way: none of the drops it takes that the plan didn't mean to)
            let dropped: BTreeSet<usize> = self.steps[self.step_i as usize..]
                .iter()
                .filter_map(|st| st.path.as_ref())
                .flat_map(|p| p.notes.iter())
                .filter_map(|n| match n {
                    PathNote::Drop { sector, .. } => Some(*sector),
                    _ => None,
                })
                .collect();
            if !dropped.is_subset(&planned) {
                return Drop::GiveBack(Trouble::Drop(format!(
                    "{why}: the only way on drops where the plan didn't"
                )));
            }
            return Drop::Replanned;
        }
        Drop::None
    }
}
