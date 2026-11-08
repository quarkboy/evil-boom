//! Path tracking, landing caution, pickup approach and recovery from obstacles.

use super::{last_point, roll_speed, stops, Drop, Phase, RouteFollower, Run, SegEnd, Segment, CROSS_REST_WAIT, PRESS_RANGE};
use crate::geom::{cells, closest_point, dist, segment_distance, Point};
use crate::motor::{heading, Controls, Observation, Track, BRAKE, FULL_SPEED, TICRATE};
use crate::nav::{can_cross, Heights};
use crate::navigator::{closest_on_portal, ITEM_RADIUS, MAX_STEP, PICKUP_BELOW, PLAYER_HEIGHT, PLAYER_RADIUS, TELEPORTERS_FOR_PLAYERS};
use crate::specials::Trigger;
use crate::walkthrough::{fmt_heights, Action};
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
    pub(super) fn walk(&mut self, obs: &Observation) -> Controls {
        if self.sprint.is_some() {
            return self.sprint_jump(obs);
        }
        if self.run.is_some() {
            return self.run_step(obs);
        }
        if self.barrel.is_some() {
            return self.shoot_barrel(obs);
        }
        let nav = Rc::clone(&self.view);
        let step = self.step().clone();
        // (the floor she's walking on going down when nothing she's doing means it to (no move
        // she's waiting on, not the step's own line): the user's rule, for the nearest floor
        // that stays put at once. E3M1: walked over the way over the blood, its first stone
        // went down under her, walked on to the jump she'd planned, off its corner into the blood)
        if let Some(s) = nav.nav.mesh.poly_at(obs.pos()).map(|p| nav.nav.sector_of_poly(p)) {
            let on_floor = obs.z.is_none_or(|z| z <= obs.heights[s].floor + 0.5);
            let going_down = obs.height(s).is_some_and(|l| l.floor < nav.nav.movers.initial[s].floor - 0.5);
            let expected = self.watches.iter().any(|w| w.expect.sector == s && !w.over) || step.trigger.as_ref().is_some_and(|t| t.effect(s).is_some());
            // (and only with somewhere to run to: down in the pit already, it's for the explorer
            // to find the way out)
            // (and only a floor going down into what hurts, as the triggers that move it have it:
            // a lift going down under her is a lift, E1M2's, run off two thousand times)
            let into_harm = || nav.nav.triggers.iter().any(|t| t.info.trigger.walked() && t.effect(s).is_some() && nav.nav.floor_after(t, s) == Some(true));
            if on_floor && going_down && self.sinking.contains(&s) && !expected && into_harm() && self.run_path(obs).is_some() {
                self.log.add(obs.tic, format!("sector {s} going down under her: running for the nearest floor that stays put"));
                self.run = Some(Run { heading: obs.angle, sector: None, next: obs.tic });
                return self.run_step(obs);
            }
        }
        if let Some(t) = step.trigger.as_ref().filter(|t| step.action == Action::Cross && t.info.trigger == Trigger::WR && !t.permanent()) {
            if !self.cross_ready {
                // A lift line to cross for its window: if the lift's away already (set off on the
                // way, MAP09), crossing now does nothing and the window is shorter than planned.
                let init = &nav.nav.movers.initial;
                let away: Vec<usize> =
                    t.effects.iter().map(|(s, _)| *s).filter(|&x| obs.height(x).is_some_and(|live| (live.floor - init[x].floor).abs() > 0.5)).collect();
                if !away.is_empty() && obs.tic - self.step_tic < CROSS_REST_WAIT {
                    if !self.cross_wait_logged {
                        self.log.add(obs.tic, format!("sector {} already away: waiting for it to come back before crossing line {}", away[0], t.line));
                        self.cross_wait_logged = true;
                    }
                    self.progress_tic = obs.tic;
                    return self.motion.hold(obs, (0.0, 0.0), Some(obs.angle));
                }
                self.cross_ready = true;
            }
        }
        if obs.tic < self.brake_until {
            if self.motion.speed() > 1.0 {
                // just landed a jump: stop first
                self.progress_tic = obs.tic;
                return self.motion.hold(obs, (0.0, 0.0), self.look);
            }
            self.brake_until = -1;
        }
        let seg = self.segments[self.seg_i].clone();
        let track = &seg.track;
        let pos = obs.pos();
        (self.k, self.s, self.cross) = track.project(pos, self.s, 64.0);
        if self.lost(obs) {
            if !self.replan(obs, "off course", false) {
                return self.fail(obs, "off course and no way back");
            }
            return self.walk(obs);
        }
        self.speed_cap = f64::INFINITY;
        match self.unexpected_drop(obs) {
            Drop::None => {}
            Drop::Replanned => return self.walk(obs),
            Drop::GiveBack(why) => return self.fail(obs, &why),
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
        let to_end = if track.length - self.s < 48.0 { dist(pos, end) } else { f64::INFINITY };
        if seg.end == SegEnd::Jump && to_end <= RUNUP_TOLERANCE {
            if let Some(j) = &seg.jump {
                return self.line_up(obs, j);
            }
        }
        if step.action == Action::Pickup && obs.keys.is_some_and(|k| step.key.is_some_and(|sk| k.has(sk))) {
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
        if self.s > self.best_s + 2.0 {
            if self.best_s != f64::NEG_INFINITY {
                self.unsticks = 0; // (got past whatever snagged her)
            }
            self.best_s = self.s;
            self.progress_tic = obs.tic;
        }
        if obs.tic < self.unstick_until {
            return self.unstick(obs);
        }
        let trig_line = step.trigger.as_ref().and_then(|t| t.linedef());
        if obs.tic - self.progress_tic > STUCK_TICS
            && step.action == Action::Use
            && self.seg_i == self.segments.len() - 1
            && trig_line.is_some_and(|li| self.line_distance(pos, li) <= PRESS_RANGE)
        {
            // can't get to the spot, but the switch is in reach from here (MAP27's, past a shut
            // door the press opens first): press from here
            self.reset_progress(obs.tic);
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        if let Some(c) = self.pinned_under_ceiling(obs) {
            return c;
        }
        if obs.tic - self.progress_tic > STUCK_TICS {
            self.unsticks += 1;
            if self.unsticks > UNSTICKS_BEFORE_REPLAN {
                // (with the heights as they are: the way the plan saw may be shut now; MAP23's
                // squeezed past a timed door that had shut, re-pathed the same way for ever)
                if !self.replan(obs, "stuck", true) {
                    return self.fail(obs, "stuck");
                }
                return self.walk(obs);
            }
            if let Some(b) = obs.barrels.iter().flatten().copied().find(|&b| dist(b, pos) < BARREL_IN_WAY) {
                // wedged against a barrel (MAP23 has rows of them across corridors): blow it up
                // from a safe distance, as a player would
                self.log.add(obs.tic, format!("barrel in the way at ({:.0}, {:.0}): shooting it", b.0, b.1));
                self.barrel = Some((b, obs.tic));
                return self.shoot_barrel(obs);
            }
            self.log.add(obs.tic, format!("stuck at ({:.0}, {:.0}), backing off", obs.x, obs.y));
            self.unstick_until = obs.tic + UNSTICK_TICS;
            self.progress_tic = obs.tic + UNSTICK_TICS;
            self.unstick_dir = self.away_from_walls(obs);
            return self.unstick(obs);
        }
        // follow the track: steer for a point a little ahead, look further ahead
        let mut carrot = self.carrot(pos, track);
        if seg.end == SegEnd::Teleport && to_end < REACHED {
            carrot = seg.look.pts[seg.look.pts.len() - 1]; // (keep going through the line)
        }
        let end_speed = if stop {
            0.0
        } else if seg.end == SegEnd::Teleport {
            FULL_SPEED
        } else {
            roll_speed(&seg)
        };
        // what's left of the track: off its end, as far as the end really is (MAP24: braked to a
        // stop 25 past a run-up the track ended at, and stood there "arrived")
        let rest = (track.length - self.s).max(dist(pos, end));
        let mut speed = FULL_SPEED.min(track.speed_limit(track.length - rest, end_speed)).min(self.speed_cap);
        if stop {
            speed = speed.min(0.3 * to_end + 0.5);
        }
        if seg.end == SegEnd::Jump {
            // run-ups are often right at a ledge: in gently, slowly over the last stretch
            // (braking does nothing between steps down: MAP17's run-up at the foot of a stair
            // was overshot into the pit beyond)
            if self.steps_down_to_end(obs, track) {
                speed = speed.min(0.06 * rest + 1.5).min(if rest < 128.0 { 6.0 } else { FULL_SPEED });
            } else {
                speed = speed.min(0.12 * rest + 1.5);
            }
        }
        speed = speed.min(self.caution_speed(obs, track));
        let look = self.look_angle(obs, &seg);
        if dist(pos, carrot) < 1.0 {
            return self.motion.hold(obs, (0.0, 0.0), Some(look));
        }
        self.motion.go(obs, heading(pos, carrot), speed, Some(look))
    }

    /// How fast to go for what's just ahead: down steps (braking does nothing dropping
    /// between them), and along a thin ledge with drops close by on both sides (walk it).
    fn caution_speed(&self, obs: &Observation, track: &Track) -> f64 {
        let nav = &self.view.nav;
        let (mesh, m) = (&nav.mesh, &nav.map);
        let drops = nav.drop_lines();
        let mut steps_down: Vec<f64> = Vec::new();
        let mut prev: Option<f64> = None;
        let ahead = CAUTION_AHEAD.min(track.length - self.s);
        let n = ((ahead / 16.0).floor() as i64).max(1);
        for i in 0..=n {
            let f = ahead * i as f64 / n as f64;
            let q = track.point_at(self.s + f);
            let t = track.point_at(self.s + f + 1.0);
            let (dx, dy) = (t.0 - q.0, t.1 - q.1);
            let Some(poly) = mesh.poly_at(q) else { continue };
            let floor = obs.height(mesh.poly(poly).sector).map_or(0.0, |h| h.floor);
            if prev.is_some_and(|p| floor < p) {
                steps_down.push(f);
            }
            prev = Some(floor);
            let mut sides: BTreeSet<bool> = BTreeSet::new();
            for cell in cells(q.0 - LEDGE_NEAR, q.1 - LEDGE_NEAR, q.0 + LEDGE_NEAR, q.1 + LEDGE_NEAR) {
                for &li in mesh.line_grid.get(&cell).into_iter().flatten() {
                    if !drops.contains(&li) {
                        continue;
                    }
                    let (Some(fs), Some(bs)) = m.sides(li) else { continue };
                    let (Some(hf), Some(hb)) = (obs.height(fs), obs.height(bs)) else { continue };
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
            return f64::INFINITY; // (one drop isn't a stair: MAP09's slowed for a 48 down missed a lift)
        }
        (STAIRS_DOWN_SPEED * STAIRS_DOWN_SPEED + 2.0 * BRAKE * steps_down[0]).sqrt()
    }

    /// Where the item a pickup step is for lies (the nearest such thing to its goal).
    pub(super) fn item_of(&self) -> Option<Point> {
        let step = self.step();
        let key = step.key.filter(|_| step.action == Action::Pickup)?;
        let goal = last_point(step.path.as_ref()?)?;
        self.view.nav.map.things.iter().filter(|t| t.kind == key as u16).map(|t| (t.x, t.y)).min_by(|a, b| dist(*a, goal).total_cmp(&dist(*b, goal)))
    }

    /// Close enough that the boxes overlap, and at a height the game lets her have it from:
    /// picked up, wherever the spot was. (Not from the floor 24 above E2M7's armor bonus in its
    /// sunken strip, the boxes overlapping over the edge: the item more than 8 under her feet.)
    fn touching_item(&self, pos: Point, obs: &Observation) -> bool {
        let Some(it) = self.item else { return false };
        if (pos.0 - it.0).abs().max((pos.1 - it.1).abs()) >= PLAYER_RADIUS + ITEM_RADIUS - 1.0 {
            return false;
        }
        let nav = &self.view.nav;
        match (obs.z, nav.poly_at(it)) {
            (Some(z), Some(p)) => (-PICKUP_BELOW..=PLAYER_HEIGHT).contains(&(obs.heights[nav.sector_of_poly(p)].floor - z)),
            _ => true,
        }
    }

    /// The point to steer for: as far ahead on the track as the straight line there keeps the
    /// box clear (corners rounded where there's room), else the next waypoint exactly.
    fn carrot(&self, pos: Point, track: &Track) -> Point {
        for ahead in CARROT_STEPS {
            let q = track.point_at(self.s + ahead);
            // (a path that doubles back can put a point far along it right next to her)
            let along = ahead.min(track.length - self.s);
            if dist(pos, q) < 0.6 * along {
                continue;
            }
            if dist(pos, q) < 1.0 || self.chord_clear(pos, q) {
                return q;
            }
        }
        track.carrot(self.k, self.s, TRACK_AHEAD)
    }

    /// Can the box move straight from p to q without touching a wall, solid thing or
    /// (uncrossed) ledge? Edges she's close to already only need not get closer.
    pub(super) fn chord_clear(&self, p: Point, q: Point) -> bool {
        let nav = &self.view.nav;
        for (e1, e2) in nav.walls_near(p, q, 2.0 * PLAYER_RADIUS, Some(&self.crossing_lines), Some(&self.live)) {
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
                if !TELEPORTERS_FOR_PLAYERS.contains(&m.linedefs[li].special) || self.crossing_lines.contains(&li) {
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
        let s = if look.length > seg.track.length { look.project(obs.pos(), self.s, 64.0).1 } else { self.s };
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

    /// Which way to back off: away from the walls and ledges nearest (MAP29: backing off a stair
    /// wall walked it off the other side into the pit), else backwards.
    fn away_from_walls(&self, obs: &Observation) -> f64 {
        let pos = obs.pos();
        let (mut ax, mut ay) = (0.0, 0.0);
        for (a, b) in self.view.nav.walls_near(pos, pos, 40.0, Some(&self.crossing_lines), Some(&obs.heights)) {
            let q = closest_point(pos, a, b);
            let d = dist(pos, q);
            if 1e-6 < d && d < 40.0 {
                ax += (pos.0 - q.0) / d * (40.0 - d);
                ay += (pos.1 - q.1) / d * (40.0 - d);
            }
        }
        let track = &self.segments[self.seg_i].track;
        let back = heading(track.point_at(self.s + TRACK_AHEAD), pos);
        if ax.hypot(ay) < 1e-6 {
            return back;
        }
        let away = ay.atan2(ax);
        let back_r = back.to_radians();
        // mostly sideways off what she's snagged on, a bit backwards
        heading((0.0, 0.0), (away.cos() * 2.0 + back_r.cos(), away.sin() * 2.0 + back_r.sin()))
    }

    fn unstick(&mut self, obs: &Observation) -> Controls {
        let seg = self.segments[self.seg_i].clone();
        let look = self.look_angle(obs, &seg);
        self.motion.go(obs, self.unstick_dir, 5.0, Some(look))
    }

    /// Does the last 128 units of the track go down any steps (live floors)?
    fn steps_down_to_end(&self, obs: &Observation, track: &Track) -> bool {
        let mesh = &self.view.nav.mesh;
        let start = (track.length - 128.0).max(0.0);
        let floors: Vec<f64> = (0..9)
            .filter_map(|k| mesh.poly_at(track.point_at(start + (track.length - start) * f64::from(k) / 8.0)).map(|q| obs.heights[mesh.poly(q).sector].floor))
            .collect();
        floors.windows(2).any(|w| w[1] < w[0])
    }

    /// A crossing just ahead that's become a one-way drop the plan didn't mean to take (MAP08:
    /// walking to the red key goes over a line that sinks the bridge back into the nukage).
    /// Stop and replan with the heights as they are: on if there's a way without such a drop;
    /// else the objective goes back to be judged again, the ground as it is now (E2M4: a floor
    /// raised on the way left the goal down in the blood pit, a drop into a trap).
    /// A drop just ahead onto a landing that's narrow the way she's going (a ledge over a pit):
    /// step off it slowly, as a falling player keeps her speed and slides on landing (MAP11's
    /// ledge with the blur sphere, 136 down and 48 across: she'd gone straight over it).
    fn creep_to_drop(&mut self, obs: &Observation) {
        let view = Rc::clone(&self.view);
        let mesh = &view.nav.mesh;
        let Some(path) = self.step().path.clone() else { return };
        let pos = obs.pos();
        let upto = (self.poly_i + 4).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            let (a, b) = (obs.heights[cs], obs.heights[ns]);
            if cs == ns || a.floor - b.floor <= MAX_STEP {
                continue;
            }
            let Some(portal) = mesh.poly(cur).portals.iter().map(|&pi| &mesh.portals[pi]).find(|p| p.other(cur) == nxt) else { return };
            let edge = closest_on_portal(pos, portal.p1, portal.p2);
            if dist(edge, pos) > CREEP_RANGE {
                return;
            }
            // (the way she'll go over: on from her to the edge, or, at it, the way the landing is)
            let to = if dist(edge, pos) > 4.0 { pos } else { mesh.poly(cur).center };
            let len = dist(edge, to).max(1e-6);
            let dir = ((edge.0 - to.0) / len, (edge.1 - to.1) / len);
            // (the floor she'd land on, how far on before it drops away again)
            let room = (1..)
                .map(|k| f64::from(k) * 8.0)
                .take_while(|&r| r < LANDING_ROOM)
                .find(|&r| mesh.poly_at((edge.0 + dir.0 * r, edge.1 + dir.1 * r)).is_none_or(|q| obs.heights[mesh.poly(q).sector].floor < b.floor - MAX_STEP));
            // (as fast as goes no further than that: the fall, about sqrt(2h) tics, and the
            // slide on landing, 1 / (1 - friction) tics' worth, at the speed she went over)
            if let Some(room) = room {
                let tics = (2.0 * (a.floor - b.floor)).sqrt() + 1.0 / (1.0 - crate::motor::FRICTION);
                self.speed_cap = self.speed_cap.min((room / tics).max(DROP_CREEP));
            }
            return;
        }
    }

    fn unexpected_drop(&mut self, obs: &Observation) -> Drop {
        let view = Rc::clone(&self.view);
        let mesh = &view.nav.mesh;
        let Some(path) = self.step().path.clone() else { return Drop::None };
        // (the drops the plan means to take, this step's and the rest's)
        let planned: BTreeSet<String> = self.steps[self.step_i as usize..]
            .iter()
            .filter_map(|st| st.path.as_ref())
            .flat_map(|p| p.steps.iter())
            .filter(|n| n.contains("one-way drop"))
            .filter_map(|n| n.split("into sector ").nth(1).map(str::to_string))
            .collect();
        let upto = (self.poly_i + 4).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            if cs == ns {
                continue;
            }
            let Some(portal) = mesh.poly(cur).portals.iter().map(|&pi| &mesh.portals[pi]).find(|p| p.other(cur) == nxt) else { return Drop::None };
            if dist(closest_on_portal(obs.pos(), portal.p1, portal.p2), obs.pos()) > DROP_CHECK_DIST {
                return Drop::None;
            }
            let (a, b) = (obs.heights[cs], obs.heights[ns]);
            let key = (cs, ns, [a.floor.to_bits(), a.ceiling.to_bits(), b.floor.to_bits(), b.ceiling.to_bits()]);
            if planned.contains(&ns.to_string()) || self.drops_ok.contains(&key) || !can_cross(a, b) || can_cross(b, a) {
                continue;
            }
            if !self.states[cs].iter().any(|&ha| self.states[ns].iter().any(|&hb| can_cross(hb, ha))) {
                continue; // (one-way in the plan as well)
            }
            // (or the lift it'd drop off, on its way down to level: MAP05's, pressed to be ridden,
            // its far side a drop till it's arrived)
            let coming = self
                .watches
                .iter()
                .find(|w| {
                    let fits = if w.expect.sector == ns {
                        can_cross(a, w.expect.heights) && can_cross(w.expect.heights, a)
                    } else {
                        w.expect.sector == cs && can_cross(w.expect.heights, b) && can_cross(b, w.expect.heights)
                    };
                    fits && w.reached.is_none() && ((obs.tic - w.since) as f64) < 1.5 * w.expect.travel + 10.0 * TICRATE
                })
                .map(|w| w.expect.heights);
            if let Some(h) = coming {
                // still on its way to where the plan has it (MAP29: a floor that takes half a
                // minute to rise to the walkway): wait at the edge
                if !self.edge_wait {
                    self.log.add(obs.tic, format!("waiting for sector {ns} to reach {}", fmt_heights(h)));
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
            let allowed = |s: usize, h: Heights| self.states[s].iter().any(|x| (x.floor - h.floor).abs() < 1.0 && (x.ceiling - h.ceiling).abs() < 1.0);
            // (or one the plan is waiting to see move, caught on its way: MAP06's lift, pressed to
            // be got onto, sinking under her as she stood by its switch)
            let on_its_way = |s: usize| self.watches.iter().any(|w| w.expect.sector == s && w.reached.is_none());
            let foreseen = (allowed(cs, a) || on_its_way(cs)) && (allowed(ns, b) || on_its_way(ns));
            if !self.replan(obs, &why, true) {
                return if foreseen { Drop::None } else { Drop::GiveBack(format!("{why}, and no other way on")) };
            }
            if foreseen {
                return Drop::Replanned;
            }
            // (the new way: none of the drops it takes that the plan didn't mean to)
            let dropped: BTreeSet<String> = self.steps[self.step_i as usize..]
                .iter()
                .filter_map(|st| st.path.as_ref())
                .flat_map(|p| p.steps.iter())
                .filter(|n| n.contains("one-way drop"))
                .filter_map(|n| n.split("into sector ").nth(1).map(str::to_string))
                .collect();
            if !dropped.is_subset(&planned) {
                return Drop::GiveBack(format!("{why}: the only way on drops where the plan didn't"));
            }
            return Drop::Replanned;
        }
        Drop::None
    }
}
