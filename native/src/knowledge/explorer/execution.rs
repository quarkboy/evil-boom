//! Following objectives, looking around, and recording arrival or failure.

use super::{turn_to, Explorer, Hooks, Kind, Objective, ARRIVE_SLACK, JUMP_MISSES, LOOK_TICS, STUCK_TIMES};
use crate::geom::dist;
use crate::knowledge::model::{Heard, How, Note};
use crate::motor::follower::RouteFollower;
use crate::motor::{Controls, Observation};
use std::collections::BTreeSet;

/// a suspected wall pressed: watched this long for something to happen
const WALL_TICS: i64 = 25;

/// a wall tried: walked up to till this near (past the box's edge), at this speed, giving up
/// on it this many tics before the press is due
const WALL_BUMP_GAP: f64 = 3.0;

const WALL_BUMP_SPEED: f64 = 25.0;

const WALL_BUMP_LAST: i64 = 4;

/// stuck this near the last time, so many objectives running, with nothing new known: stranded
const STUCK_SAME: f64 = 32.0;

const STUCK_RUN: usize = 3;

/// an item still there on arrival: shuffled over this long
const SHUFFLE_TICS: i64 = 35;

/// sounds from about the same place...
const AMBIENT_RADIUS: f64 = 96.0;

/// ...heard more often than this are background
const AMBIENT_REPEATS: usize = 4;

/// at most one look round this often (stairs build in many steps)
const GLANCE_GAP: i64 = 5 * 35;

/// tics after a walk-over line goes off unseen, before looking
const GLANCE_AFTER: i64 = 8;

/// a wall-looking trip whose wall's been seen ends this near its spot
const ROOM_NEAR: f64 = 160.0;

impl Explorer {
    /// Stop and look: towards a sound she couldn't see the source of, or round about when
    /// something went off underfoot and nothing's been seen or heard to happen.
    pub(super) fn glance(&mut self, obs: &Observation) -> Option<Controls> {
        if let Some(until) = self.glance_until {
            if obs.tic < until {
                return Some(turn_to(obs, self.glance_at));
            }
            self.glance_until = None;
            if let Some(f) = &mut self.follower {
                f.resume(obs.tic); // (MAP24: "stuck" after looking round off a teleporter)
            }
        }
        while self.heard_n < self.knowledge.heard.len() {
            let h = self.knowledge.heard[self.heard_n];
            self.heard_n += 1;
            if self.worth_a_look(&h) && obs.tic - self.last_glance >= GLANCE_GAP {
                return Some(self.start_glance(obs, Some(h.bearing), &format!("heard something, bearing {:.0}", h.bearing)));
            }
        }
        let learned: BTreeSet<i64> = self.knowledge.learned.iter().filter(|(_, s)| !s.is_empty()).map(|(&i, _)| self.nav.triggers[i].line).collect();
        while self.glanced < self.knowledge.fired.len() {
            let f = self.knowledge.fired[self.glanced];
            // (not a teleporter she knows where goes: nothing to look for, and a stop on the way
            // through, MAP21's to the megasphere's lift, a second over its window every time)
            let unknown = f.how == How::Walk && !learned.contains(&(f.line as i64)) && !self.knowledge.teleports.contains_key(&f.line);
            if unknown && obs.tic - f.tic < GLANCE_AFTER {
                return None; // (give it a moment to be seen first)
            }
            self.glanced += 1;
            if unknown {
                return Some(self.start_glance(obs, None, &format!("set off line {} walking", f.line)));
            }
        }
        None
    }

    /// A sound she couldn't see the source of, not one she'd expect, not background.
    pub(super) fn worth_a_look(&self, h: &Heard) -> bool {
        !h.seen && !self.knowledge.expected(h) && !self.ambient(h)
    }

    fn start_glance(&mut self, obs: &Observation, bearing: Option<f64>, why: &str) -> Controls {
        self.last_glance = obs.tic;
        self.log.push(format!("tic {}: {why}: looking {}", obs.tic, if bearing.is_none() { "round" } else { "that way" }));
        self.glance_until = Some(obs.tic + LOOK_TICS);
        self.glance_at = bearing;
        turn_to(obs, bearing)
    }

    /// A sound heard again and again from the same place (a lift going up and down for ever):
    /// tuned out.
    fn ambient(&self, h: &Heard) -> bool {
        self.knowledge.heard.iter().filter(|o| dist(o.origin, h.origin) < AMBIENT_RADIUS && o.tic <= h.tic).count() > AMBIENT_REPEATS
    }

    /// Has what the objective was to look at been seen?
    fn seen_to(&self, obj: &Objective) -> bool {
        let k = &self.knowledge;
        match obj.kind {
            Kind::Frontier => {
                let known = k.known_polys();
                obj.reveals.iter().any(|&p| known.contains(&(p as usize)))
            }
            Kind::Recheck => obj.reveals.iter().all(|&s| k.seen_tic.get(&(s as usize)).copied().unwrap_or(-1) >= self.since),
            Kind::Noise => obj.heard.as_ref().is_some_and(|h| self.noise_seen(h)),
            Kind::Room => obj.reveals.iter().all(|&l| k.mapped.contains(&(l as usize))),
            Kind::Visit => obj.reveals.iter().all(|&p| k.visited.contains(&(p as usize))),
            Kind::Secret => obj.reveals.iter().all(|&s| self.secrets_had.contains(&(s as usize))),
            _ => false,
        }
    }

    /// The route to the objective is done: check a press went off, and look round where there's
    /// something to look for. Controls while still looking, else None (a new objective's wanted).
    pub(super) fn arrived(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> Option<Controls> {
        let obj = self.current.clone()?;
        let k = &self.knowledge;
        // (this time: one pressed long ago doesn't count; MAP08's switch)
        let pressed_now = |li: i64| usize::try_from(li).is_ok_and(|li| k.fired.iter().any(|f| k.tried_key(f.line) == k.tried_key(li) && f.tic >= self.since));
        if matches!(obj.kind, Kind::Switch | Kind::Door) && !obj.trigger.as_ref().is_some_and(|t| pressed_now(t.line)) {
            self.drop_objective(&obj, obs, "pressed, but nothing went off");
        } else if matches!(obj.kind, Kind::Frontier | Kind::Switch | Kind::Recheck | Kind::Noise | Kind::Room) && !self.seen_to(&obj) {
            let until = *self.look_until.get_or_insert(obs.tic + LOOK_TICS);
            if obs.tic < until {
                let at = if obj.kind == Kind::Noise { obj.heard.map(|h| h.origin) } else { obj.look_at };
                return Some(match at {
                    Some(at) => turn_to(obs, Some((at.1 - obs.y).atan2(at.0 - obs.x).to_degrees())),
                    None => Controls { turn: 360.0 / LOOK_TICS as f64, ..Controls::default() },
                });
            }
            if obj.kind != Kind::Switch && !self.seen_to(&obj) {
                self.drop_objective(&obj, obs, "nothing to see from there");
            }
        } else if obj.kind == Kind::Visit && !self.seen_to(&obj) {
            self.drop_objective(&obj, obs, "got there but not in it");
        } else if obj.kind == Kind::Teleporter
            && !obj.trigger.as_ref().is_some_and(|t| usize::try_from(t.line).is_ok_and(|li| self.knowledge.teleports.contains_key(&li)))
        {
            self.drop_objective(&obj, obs, "didn't take me anywhere");
        } else if obj.kind == Kind::Wall {
            // face it, press use, watch a moment (what it does, if anything, the knowledge sees)
            let wall = obj.wall.unwrap_or(0);
            if self.look_until.is_none() {
                self.look_until = Some(obs.tic + WALL_TICS + 20);
                self.knowledge.note(Note::TriedWall(wall, obs.keys.unwrap_or_default()));
            }
            let at = obj.look_at.unwrap_or(obj.goal);
            let bearing = (at.1 - obs.y).atan2(at.0 - obs.x).to_degrees();
            let off = crate::motor::norm(bearing - obs.angle).abs();
            let until = self.look_until.unwrap_or(obs.tic);
            if obs.tic < until {
                let mut c = turn_to(obs, Some(bearing));
                // (not where the press would reach an exit behind it, with the exit held back:
                // MAP22's wall in front of the exit switch, the level ended an item short)
                let exit = self.exit_last && self.knowledge.use_hits(obs.pos(), obs.angle, &obs.heights).is_some_and(|li| self.knowledge.is_exit(li));
                // (a wall that stands out, up against it first, as a player goes right up to a mark:
                // over anything on the floor in front, MAP10's triangle of walk-over lines before
                // the marked wall that opens the way to the megasphere, stopped short of at the stand
                // spot; not every wall, nor one by the way, the last steps over a trap's line: E2M4,
                // MAP14)
                let (wa, wb) = self.nav.map.line_seg(wall);
                let gap = crate::geom::segment_distance(wa, wb, obs.pos(), obs.pos());
                if off < 4.0
                    && obs.tic < until - WALL_TICS - WALL_BUMP_LAST
                    && gap > crate::geom::PLAYER_RADIUS + WALL_BUMP_GAP
                    && obj.round
                    && self.knowledge.marked(wall)
                {
                    c.forward = WALL_BUMP_SPEED;
                } else if off < 4.0 && obs.tic < until - WALL_TICS && !exit {
                    c.use_ = true;
                    self.look_until = Some(obs.tic + WALL_TICS); // (pressed: now watch)
                } else if off < 4.0 && obs.tic > until - WALL_TICS + 4 && obs.tic < until - 8 && obj.round {
                    // (and a few shots: some walls answer to a gun, not a hand; with the pistol,
                    // single taps: a rocket sets off no wall, and blows up in her face; MAP11. Only
                    // in a round of walls, coming back to search, not one tried on the way: a gun
                    // line's rare, and a shot wakes what hears it; the user's)
                    let gun = crate::motor::follower::tap_gun(obs);
                    if obs.weapon != Some(gun) {
                        c.weapon = gun;
                    } else if obs.tic - self.wall_shot >= crate::motor::follower::BARREL_TAP {
                        c.attack = true;
                        self.wall_shot = obs.tic;
                    }
                }
                if c.use_ && obj.round {
                    // (out with the pistol as she presses, ready to shoot after)
                    c.weapon = crate::motor::follower::tap_gun(obs);
                }
                return Some(c);
            }
        } else if matches!(obj.kind, Kind::Item | Kind::Hunt) && !hooks.gone(obs, &obj) {
            if obj.kind == Kind::Item {
                // Doom picks things up only moving onto them: come down on one from a ledge and
                // stand still, and it stays (MAP10's bonuses in the nukage pits). A shuffle first;
                // back to the spot first if she's off it (E2M7's armor bonus in a sunken strip: the
                // route over as she dropped into it, her run carrying her past and up the step beyond)
                let until = *self.look_until.get_or_insert(obs.tic + SHUFFLE_TICS);
                if obs.tic < until {
                    let a = (obj.goal.1 - obs.y).atan2(obj.goal.0 - obs.x) - obs.angle.to_radians();
                    let off = dist(obs.pos(), obj.goal) > ARRIVE_SLACK;
                    let push = if off || (obs.tic / 4) % 2 == 0 { 20.0 } else { -20.0 };
                    return Some(Controls { forward: push * a.cos(), strafe: -push * a.sin(), ..Controls::default() });
                }
            }
            self.drop_objective(&obj, obs, if obj.kind == Kind::Hunt { "not there" } else { "couldn't pick it up" });
        }
        self.look_until = None;
        self.follower = None;
        None
    }

    /// Presses that did nothing: what's in their way is worth trying.
    fn note_presses(&mut self, obs: &Observation) {
        let Some(f) = &mut self.follower else { return };
        let presses = std::mem::take(&mut f.failed_presses);
        for (at, li) in presses {
            for other in self.knowledge.note(Note::FailedPress(at, li)) {
                self.log.push(format!("tic {}: line {li} did nothing: line {other} is in the way of it", obs.tic));
            }
        }
    }

    /// Run-jumps the follower missed: counted (for the level, across lives), and left out from
    /// now on (E4M2). True if one was.
    fn note_misses(&mut self, obs: &Observation) -> bool {
        let Some(f) = &mut self.follower else { return false };
        if f.missed.is_empty() {
            return false;
        }
        let missed = std::mem::take(&mut f.missed);
        for j in missed {
            let n = self.knowledge.missed_jumps.get(&j.key()).copied().unwrap_or(0) + 1;
            self.knowledge.note(Note::MissedJump(j.key(), n));
            self.log.push(format!(
                "tic {}: missed the jump off ({:.0}, {:.0}) {}",
                obs.tic,
                j.takeoff.0,
                j.takeoff.1,
                if n >= JUMP_MISSES { "for good: not trying it again" } else { "once: another way first" }
            ));
        }
        self.set_jumps(false);
        true
    }

    /// A route failed stuck: where is counted, for the level across lives; stuck there
    /// STUCK_TIMES times, it's left out of the way (MAP18).
    fn note_stuck(&mut self, obs: &Observation) {
        let Some(p) = self.nav.poly_at(obs.pos()) else { return };
        let n = self.knowledge.stuck_at.get(&p).copied().unwrap_or(0) + 1;
        self.knowledge.note(Note::Stuck(p, n));
        if n == STUCK_TIMES {
            self.log.push(format!("tic {}: stuck at ({:.0}, {:.0}) {STUCK_TIMES} times: not going that way again", obs.tic, obs.x, obs.y));
        }
    }

    pub(super) fn need_new(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        self.note_presses(obs);
        if self.note_misses(obs) {
            return true; // (the way it was going may take that jump again: plan anew)
        }
        let Some(obj) = self.current.clone() else {
            return true;
        };
        let (done, failed) = match &self.follower {
            None => return true,
            Some(f) => (f.done, f.failed),
        };
        if done || failed {
            if failed {
                let why = self.follower.as_ref().and_then(|f| f.log.events.iter().rev().find(|e| e.contains("FAILED")).cloned()).unwrap_or_default();
                let why = why.split_once("] ").map_or(why.as_str(), |(_, r)| r).to_string();
                self.drop_objective(&obj, obs, &format!("failed: {why}"));
                if why.contains("stuck") {
                    self.note_stuck(obs);
                    // (stuck at one spot whatever she sets out for: wedged, a trap she couldn't
                    // know of, as stranded as with nothing to do; MAP18's block, risen under the
                    // edge of her box beside door 91, holding her up where the door's too low)
                    let here = obs.pos();
                    let same = self.stuck_run.is_some_and(|(at, _)| dist(at, here) < STUCK_SAME);
                    let n = if same { self.stuck_run.map_or(0, |(_, n)| n) + 1 } else { 1 };
                    self.stuck_run = Some((here, n));
                    if n >= STUCK_RUN && self.knowledge.summary() == self.stuck_known {
                        self.log.push(format!("tic {}: stuck here {n} times over, whatever for: stranded", obs.tic));
                        self.restart_wanted = true;
                    }
                    self.stuck_known = self.knowledge.summary();
                } else {
                    self.stuck_run = None;
                }
            } else {
                self.stuck_run = None;
            }
            return true;
        }
        let k = &self.knowledge;
        if obj.kind == Kind::Frontier {
            let known = k.known_polys();
            if obj.reveals.iter().any(|&p| known.contains(&(p as usize))) {
                return self.ends(obs, "seen it on the way");
            }
        }
        if obj.kind == Kind::Room && self.seen_to(&obj) && dist(obs.pos(), obj.goal) < ROOM_NEAR {
            // (the wall seen already, nearly there: on, not up to the spot to stop and look; E2M2's
            // crates, a stop at each face. Only nearly there: the way there shows more, E2M1's
            // way on, missed ending them as soon as seen)
            return self.ends(obs, "seen it on the way");
        }
        if obj.kind == Kind::Teleporter && obj.trigger.as_ref().is_some_and(|t| usize::try_from(t.line).is_ok_and(|li| k.teleports.contains_key(&li))) {
            return self.ends(obs, "been through");
        }
        if obj.kind != Kind::Exit && !self.exit_last && k.exits().len() > self.exits_seen {
            return self.ends(obs, "an exit came into view");
        }
        if hooks.gone(obs, &obj) {
            return self.ends(obs, "gone");
        }
        let heard: Vec<Heard> = k.heard[self.urgent_n.min(k.heard.len())..].to_vec();
        self.urgent_n = k.heard.len();
        // (something just went off out of sight: go and look first; not half way through a run
        // of jumps: MAP08's)
        let jumping = self.follower.as_ref().is_some_and(RouteFollower::jumps_ahead);
        if !(obj.kind == Kind::Noise && obj.urgent) && !jumping && heard.iter().any(|h| self.urgent(h, obs.tic) && self.worth_a_look(h)) {
            return self.ends(obs, "something went off out of sight");
        }
        if !jumping && hooks.interrupt(self, obs, &obj) {
            return self.ends(obs, "interrupted");
        }
        if obs.tic - self.since > self.allowed {
            self.drop_objective(&obj, obs, "took too long");
            let stuck = self.follower.as_ref().is_some_and(|f| f.log.events.iter().rev().take(6).any(|e| e.contains("stuck at")));
            if stuck {
                self.note_stuck(obs); // (timed out stuck: the same as failing stuck)
            }
            return true;
        }
        false
    }

    /// (an objective given up mid-way, for whoever's tracing: why)
    pub(super) fn ends(&mut self, obs: &Observation, why: &str) -> bool {
        if self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic)) {
            self.log.push(format!("  (objective ends at tic {}: {why})", obs.tic));
        }
        true
    }

    pub(super) fn drop_objective(&mut self, obj: &Objective, obs: &Observation, why: &str) {
        let id = obj.ident();
        self.dropped.insert(id.clone(), obs.tic);
        *self.drop_count.entry(id).or_insert(0) += 1;
        if let Some(w) = obj.wall {
            *self.wall_drops.entry(w).or_insert(0) += 1;
        }
        self.log.push(format!("tic {}: dropped '{}' ({why})", obs.tic, obj.label));
    }
}
