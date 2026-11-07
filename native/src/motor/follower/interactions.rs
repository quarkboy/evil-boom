//! Using doors and switches, shooting lines, and observing their effects.

use super::{tap_gun, Blocked, Phase, RouteFollower, CROSS_REST_WAIT, PRESS_RANGE, USE_EVERY};
use crate::geom::{cells, closest_point, dist, segments_cross, Point};
use crate::mesh::Portal;
use crate::motor::{fmt_live, heading, norm, Controls, Observation, BRAKE, TICRATE};
use crate::nav::{can_cross, Heights};
use crate::navigator::{closest_on_portal, rep, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::specials::{Category, Key, Trigger};
use crate::walkthrough::{fmt_heights, Action, Expect};
use std::collections::BTreeSet;
use std::rc::Rc;

/// where along a gun line to aim, the first in sight: the middle, then out towards its ends
const SHOT_AIMS: [f64; 7] = [0.5, 0.35, 0.65, 0.2, 0.8, 0.08, 0.92];

/// how far into its range of floors a gun line is shot from
const SHOT_MARGIN: f64 = 2.0;

/// a sector that moved this lately may still be busy (Doom ignores a switch on it)...
const BUSY_TICS: i64 = 200;

/// ...what moves this near a switch counts (she can see it moving)
const BUSY_NEAR: f64 = 384.0;

/// ...unless it's back where it rests and has been still this long
const REST_TICS: i64 = 12;

/// the longest wait for things round a switch to settle before pressing it
const SETTLE_WAIT: i64 = 10 * 35;

/// how close to a lift or mover before live heights are checked
const BLOCK_CHECK_DIST: f64 = 48.0;

/// ...and to a door (so she slows and walks up to it)
const DOOR_CHECK_DIST: f64 = 128.0;

/// where to stand to press a door
const DOOR_STANDOFF: f64 = 36.0;

/// presses (creeping closer) before giving up on a switch
const PRESS_TRIES: i32 = 3;

impl RouteFollower {
    /// Does a shot from `from` at `to` (on line `target`) get there: no wall, nor a sector shut
    /// now, in the way before it?
    fn shot_clear(&self, obs: &Observation, from: Point, to: Point, target: usize) -> bool {
        let nav = &self.view.nav;
        let m = &nav.map;
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for cell in cells(from.0, from.1, to.0, to.1) {
            for &li in nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if li == target || !seen.insert(li) {
                    continue;
                }
                let (p, q) = m.line_seg(li);
                if !segments_cross(from, to, p, q) {
                    continue;
                }
                match m.sides(li) {
                    (Some(f), Some(bk)) => {
                        let (hf, hb) = (obs.heights[f], obs.heights[bk]);
                        if hf.ceiling.min(hb.ceiling) <= hf.floor.max(hb.floor) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    /// The sectors within BUSY_NEAR of a line (cached).
    fn sectors_near(&mut self, li: usize) -> BTreeSet<usize> {
        if let Some(s) = self.near_cache.get(&li) {
            return s.clone();
        }
        let nav = &self.view.nav;
        let (a, b) = nav.map.line_seg(li);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let out: BTreeSet<usize> = nav.mesh.live_polys().filter(|(_, p)| dist(p.center, mid) < BUSY_NEAR).map(|(_, p)| p.sector).collect();
        self.near_cache.insert(li, out.clone());
        out
    }

    /// The first crossing ahead on the path, within reach, that the live heights don't allow.
    /// Looks past the next poly: doors often sit behind thin door-track sectors.
    fn next_blocked(&mut self, obs: &Observation) -> Option<Blocked> {
        let view = Rc::clone(&self.view);
        let nav = &view.nav;
        let mesh = &nav.mesh;
        let path = self.step().path.clone()?;
        let here = mesh.poly_at(obs.pos())?;
        let window: Vec<usize> = path.polys.iter().skip(self.poly_i).take(12).copied().collect();
        if let Some(i) = window.iter().position(|&p| p == here) {
            self.poly_i += i;
        }
        let upto = (self.poly_i + 8).min(path.polys.len().saturating_sub(1));
        for j in self.poly_i..upto {
            let (cur, nxt) = (path.polys[j], path.polys[j + 1]);
            let (cs, ns) = (mesh.poly(cur).sector, mesh.poly(nxt).sector);
            // (a jump or a climb onto the poly beside: the jump's to do, not a walk the heights
            // don't allow; MAP12's climb up the stair's side, waited at "for the mover" for good)
            if path.jumps.iter().any(|jp| jp.dest_poly == nxt && jp.src_polys.contains(&cur)) {
                return None;
            }
            let portal_i = mesh.poly(cur).portals.iter().copied().find(|&pi| mesh.portals[pi].other(cur) == nxt)?; // (none: a teleport)
            let portal = &mesh.portals[portal_i];
            let near = closest_on_portal(obs.pos(), portal.p1, portal.p2);
            let d = dist(near, obs.pos());
            if d > DOOR_CHECK_DIST {
                return None;
            }
            if ns != cs && !can_cross(obs.heights[cs], obs.heights[ns]) {
                // past the next crossing only for doors (they hide behind thin door tracks);
                // a lift: step on and ride, the crossing off it opens up on the way
                let door = self.doors.contains_key(&ns);
                if (j > self.poly_i && !door && d >= 2.0 * PLAYER_RADIUS) || (!door && d > BLOCK_CHECK_DIST) {
                    return None;
                }
                return Some((cur, ns, portal_i, None));
            }
            // the box reaches into sectors past a narrow strip before it can drop into the strip:
            // a low ceiling there must clear the floor she's on now
            let here_sector = mesh.poly(here).sector;
            let floor_now = obs.heights[here_sector].floor;
            let Heights { floor: f, ceiling: c } = obs.heights[ns];
            if j > self.poly_i && d < 2.0 * PLAYER_RADIUS && ns != here_sector && c - f.max(floor_now) < PLAYER_HEIGHT {
                return Some((cur, ns, portal_i, None));
            }
            // something beside the way that makes it too narrow now (key bars, a raised block):
            // open it if it's a door she can use, else wait as for a mover
            if nav.has_portal_gates(portal_i) {
                let live = Self::live_states(obs);
                let mut blockers = nav.gate_blockers(portal_i, cur, &live, None);
                blockers.sort_unstable();
                if !blockers.is_empty() && !nav.portal_fits_without(portal_i, &blockers) {
                    let near_polys: BTreeSet<usize> = [cur, here].into_iter().collect();
                    if let Some((door, point)) = self.gate_door(&blockers, &near_polys, obs.pos()) {
                        return Some((cur, door, portal_i, Some(point)));
                    }
                    if j == self.poly_i && d < BLOCK_CHECK_DIST {
                        let (f, b) = nav.map.sides(blockers[0]);
                        let other = [f, b].into_iter().flatten().find(|&s| s != cs).unwrap_or(cs);
                        return Some((cur, other, portal_i, None));
                    }
                }
            }
        }
        None
    }

    pub(super) fn line_distance(&self, pos: Point, li: usize) -> f64 {
        let (a, b) = self.view.nav.map.line_seg(li);
        dist(pos, closest_point(pos, a, b))
    }

    /// Does the use trace from pos to target cross a line with a special on the way (a press
    /// would set that off instead)? Lines ending at target don't count.
    fn use_intercepted(&self, pos: Point, target: Point) -> bool {
        let nav = &self.view.nav;
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for cell in cells(pos.0, pos.1, target.0, target.1) {
            for &li in nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !seen.insert(li) || nav.map.linedefs[li].special == 0 {
                    continue;
                }
                let (a, b) = nav.map.line_seg(li);
                if dist(closest_point(target, a, b), target) < 1.0 {
                    continue; // (the line being pressed)
                }
                if segments_cross(pos, target, a, b) {
                    return true;
                }
            }
        }
        false
    }

    /// Would a wall stop a press from `pos` at `target` (the use trace ends at a one-sided line or
    /// a shut opening, P_UseLines)? Not the line being pressed.
    pub(super) fn use_walled(&self, pos: Point, target: Point) -> bool {
        let nav = &self.view.nav;
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for cell in cells(pos.0, pos.1, target.0, target.1) {
            for &li in nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !seen.insert(li) {
                    continue;
                }
                let (a, b) = nav.map.line_seg(li);
                if dist(closest_point(target, a, b), target) < 1.0 || !segments_cross(pos, target, a, b) {
                    continue;
                }
                let open = match nav.map.sides(li) {
                    (Some(f), Some(bk)) => self.live.get(f).zip(self.live.get(bk)).is_none_or(|(hf, hb)| hf.ceiling.min(hb.ceiling) > hf.floor.max(hb.floor)),
                    _ => false,
                };
                if !open {
                    return true;
                }
            }
        }
        false
    }

    /// The nearest point (a little in from its ends) on a use line that opens this door.
    fn door_line_point(&self, sector: usize, pos: Point) -> Option<Point> {
        let nav = &self.view.nav;
        let mut best: Option<Point> = None;
        for t in &self.view.triggers {
            if !matches!(t.info.trigger, Trigger::DR | Trigger::D1) || t.effect(sector).is_none() {
                continue;
            }
            let Some(li) = t.linedef() else { continue };
            let (a, b) = nav.map.line_seg(li);
            let length = dist(a, b).max(1e-6);
            let k = 0.5f64.min(8.0 / length);
            let q = closest_point(pos, (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k), (b.0 + (a.0 - b.0) * k, b.1 + (a.1 - b.1) * k));
            if best.is_none_or(|bp| dist(pos, q) < dist(pos, bp)) {
                best = Some(q);
            }
        }
        best
    }

    /// Is `sector` shut by a "close, wait 30s, open" door (it opens again by itself)?
    fn reopens(&self, sector: usize, live: Heights) -> bool {
        self.view
            .triggers
            .iter()
            .any(|t| [16, 76].contains(&t.info.special) && t.effect(sector).is_some_and(|hs| hs.iter().any(|h| (live.ceiling - h.ceiling).abs() < 1.0)))
    }

    /// (door sector, point to press) for a usable door among the sectors of these lines.
    fn gate_door(&self, lines: &[usize], near: &BTreeSet<usize>, pos: Point) -> Option<(usize, Point)> {
        let m = &self.view.nav.map;
        let sectors: BTreeSet<usize> = lines
            .iter()
            .flat_map(|&li| {
                let (f, b) = m.sides(li);
                [f, b]
            })
            .flatten()
            .collect();
        for t in &self.view.triggers {
            let effects: BTreeSet<usize> = t.effects.iter().map(|(s, _)| *s).collect();
            if matches!(t.info.trigger, Trigger::DR | Trigger::D1)
                && !sectors.is_disjoint(&effects)
                && !near.is_disjoint(&t.activators)
                && t.info.lock.satisfied(Key::ALL.into_iter().collect())
            {
                let li = t.linedef()?;
                let (a, b) = m.line_seg(li);
                let s = *sectors.intersection(&effects).next()?;
                return Some((s, closest_point(pos, a, b)));
            }
        }
        None
    }

    /// A crossing just ahead the live heights don't allow: deal with it.
    pub(super) fn check_blocked(&mut self, obs: &Observation) -> Option<Controls> {
        let Some((cur, sector, portal_i, use_point)) = self.next_blocked(obs) else {
            self.wait_tic = None;
            return None;
        };
        let view = Rc::clone(&self.view);
        let nav = &view.nav;
        let portal: Portal = nav.mesh.portals[portal_i].clone();
        let live = &obs.heights;
        let pos = obs.pos();
        let opener = self.doors.get(&sector).is_some_and(|d| use_point.is_some() || d.users.contains(&cur));
        // the part of the doorway in front of her, or the gate door's line; for a door, its use
        // line (the face with the special: MAP12's diagonal blue door isn't the side the path
        // goes in by)
        let door_point =
            use_point.or_else(|| if opener { self.door_line_point(sector, pos) } else { None }).unwrap_or_else(|| closest_on_portal(pos, portal.p1, portal.p2));
        if opener {
            // how far to the door, along the path where it goes round corners
            let along = self.segments[self.seg_i].track.project(door_point, self.s, DOOR_CHECK_DIST + 64.0).1 - self.s;
            let d = dist(pos, door_point).max(along);
            let here = nav.mesh.poly_at(pos);
            // (use reaches 64: close enough to press, whatever the door's user polys say)
            if use_point.is_none() && here.is_none_or(|h| !self.doors[&sector].users.contains(&h)) && dist(pos, door_point) > PRESS_RANGE {
                // not somewhere the door can be pressed from: keep walking up to it
                self.speed_cap = (0.3 * d).max(1.0);
                return None;
            }
            let mut standoff = DOOR_STANDOFF;
            if self.use_intercepted(pos, door_point) || self.use_walled(pos, door_point) {
                // another switch line between her and the door would take the press (E3M9), or a
                // wall stop it (E3M4's exit door, set back in its frame, pressed from off to the
                // side across the frame's corner): go right up to the door
                standoff = PLAYER_RADIUS + 2.0;
            }
            if d > standoff + 4.0 {
                // a shut door ahead: follow the path, slowing to stop in front of it
                self.speed_cap = (2.0 * BRAKE * (d - standoff)).sqrt().min(0.3 * (d - standoff) + 1.0).max(1.0);
                return None;
            }
        }
        let wait_tic = *self.wait_tic.get_or_insert_with(|| {
            let kind = if self.doors.contains_key(&sector) { "door" } else { "mover" };
            self.log.add(obs.tic, format!("waiting at {kind} (sector {sector})"));
            obs.tic
        });
        let waited = obs.tic - wait_tic;
        self.progress_tic = obs.tic; // (waiting isn't being stuck)
                                     // pressing use on a moving door reverses it: only press once things are still
        if Some(live[sector]) != self.mover_heights {
            self.mover_heights = Some(live[sector]);
            self.mover_still_since = obs.tic;
        }
        let still = obs.tic - self.mover_still_since >= USE_EVERY;
        let mut look = heading(pos, door_point);
        let mut want = (0.0, 0.0);
        let mut use_ = false;
        if !opener {
            // wait squarely at the edge she's crossing (half on something else, a rising lift
            // can't carry her up and goes back down)
            let length = dist(portal.p1, portal.p2).max(1e-6);
            let (mut nx, mut ny) = ((portal.p2.1 - portal.p1.1) / length, -(portal.p2.0 - portal.p1.0) / length);
            if (pos.0 - door_point.0) * nx + (pos.1 - door_point.1) * ny < 0.0 {
                (nx, ny) = (-nx, -ny); // (the normal back to her side)
            }
            let back = PLAYER_RADIUS + 2.0;
            let spot = (door_point.0 + nx * back, door_point.1 + ny * back);
            let gap = dist(pos, spot);
            if gap > 1.0 {
                let speed = 4.0f64.min(0.3 * gap);
                let h = heading(pos, spot).to_radians();
                want = (h.cos() * speed, h.sin() * speed);
            }
        }
        if opener {
            // use traces 64 straight ahead and stops at the first wall: face the door's middle,
            // then press
            let facing = norm(look - obs.angle).abs() < 10.0;
            use_ = facing && still && obs.tic - self.last_use >= USE_EVERY;
        }
        let mut switch_nearby = false;
        if !opener {
            // the lift or door isn't coming by itself: use a trigger for it she's next to
            let here = nav.mesh.poly_at(pos);
            let near: BTreeSet<usize> = std::iter::once(cur).chain(here).collect();
            let mut involved: BTreeSet<usize> = [nav.sector_of_poly(cur), sector].into_iter().collect();
            involved.extend(here.map(|h| nav.sector_of_poly(h)));
            for t in &view.triggers {
                if !matches!(t.info.trigger, Trigger::SR | Trigger::S1 | Trigger::DR | Trigger::D1)
                    || !t.sectors.iter().any(|s| involved.contains(s))
                    || near.is_disjoint(&t.activators)
                {
                    continue;
                }
                switch_nearby = true;
                if waited > 3 * 35 && still {
                    // the nearest bit of it (a long lift edge's middle can be out of use range:
                    // MAP29), stepping up to it if need be
                    let Some(li) = t.linedef() else { break };
                    let (la, lb) = nav.map.line_seg(li);
                    let length = dist(la, lb).max(1e-6);
                    let k = ((pos.0 - la.0) * (lb.0 - la.0) + (pos.1 - la.1) * (lb.1 - la.1)) / (length * length);
                    let k = k.max(0.5f64.min(8.0 / length)).min(0.5f64.max(1.0 - 8.0 / length));
                    let point = (la.0 + (lb.0 - la.0) * k, la.1 + (lb.1 - la.1) * k);
                    look = heading(pos, point);
                    if dist(pos, point) > PRESS_RANGE {
                        let gap = dist(pos, point) - PRESS_RANGE + 4.0;
                        let h = look.to_radians();
                        let v = 4.0f64.min(0.3 * gap);
                        want = (h.cos() * v, h.sin() * v);
                    } else {
                        use_ = norm(look - obs.angle).abs() < 10.0 && obs.tic - self.last_use >= USE_EVERY;
                    }
                }
                break;
            }
        }
        let mut controls = self.motion.hold(obs, want, Some(look));
        controls.use_ = use_;
        if use_ {
            self.last_use = obs.tic;
        }
        // nothing to press here: it may need a walk-over line crossing again (a blazing lift
        // that went back up), so replan from the live heights after a few seconds; at once if
        // the window counted on has run out
        let missed = !opener && waited == 35 / 2 && self.window_over(sector);
        if waited == 0 {
            self.reopening = self.reopens(sector, live[sector]);
            if self.reopening {
                self.log.add(obs.tic, format!("sector {sector} shut for 30s: waiting for it to reopen"));
            }
        }
        if self.reopening {
            // a "close, wait 30s, open" door she walked into the trigger of (E1M6): nothing to
            // do but wait, as a player would; spin round lazily meanwhile
            if waited > 33 * 35 {
                self.reopening = false;
                if !self.replan(obs, &format!("sector {sector} not reopening"), false) {
                    return Some(self.fail(obs, &format!("waited too long for sector {sector} to reopen")));
                }
            }
            return Some(self.motion.hold(obs, want, Some((obs.angle + 10.0).rem_euclid(360.0))));
        }
        // still on its way to where the plan has it (MAP29's floor that takes half a minute to
        // rise to the walkway): nothing to replan, just wait
        let coming = self
            .watches
            .iter()
            .any(|w| w.expect.sector == sector && w.reached.is_none() && ((obs.tic - w.since) as f64) < 1.5 * w.expect.travel + 10.0 * TICRATE);
        if coming && !opener {
            self.wait_tic = Some(obs.tic - waited.min(4 * 35)); // (keeps "waited" short)
            return Some(self.motion.hold(obs, want, Some(look)));
        }
        if missed || (!opener && waited == 5 * 35 && !switch_nearby) {
            let why = if missed { format!("missed sector {sector}'s window") } else { format!("sector {sector} not coming") };
            if self.replan(obs, &why, true) {
                return Some(self.motion.hold(obs, (0.0, 0.0), Some(look)));
            }
        }
        if waited > 15 * 35 {
            if !self.replan(obs, &format!("sector {sector} not opening"), false) {
                let cs = nav.sector_of_poly(cur);
                return Some(
                    self.fail(obs, &format!("waited too long for sector {sector}: live {} -> {}", fmt_live(Some(live[cs])), fmt_live(Some(live[sector])))),
                );
            }
            return Some(self.motion.hold(obs, (0.0, 0.0), Some(look)));
        }
        Some(controls)
    }

    /// Wait (standing still) for the sectors the next path needs to finish moving.
    pub(super) fn settle_step(&mut self, obs: &Observation) -> Controls {
        let at = |e: &Expect| obs.height(e.sector).is_some_and(|l| (l.floor - e.heights.floor).abs() < 1.0 && (l.ceiling - e.heights.ceiling).abs() < 1.0);
        let waiting: Vec<Expect> = self.settle.iter().filter(|e| !at(e)).cloned().collect();
        let took = (obs.tic - self.settle_tic) as f64 / TICRATE;
        if waiting.is_empty() {
            let secs: Vec<usize> = self.settle.iter().map(|e| e.sector).collect();
            self.log.add(obs.tic, format!("sectors {secs:?} in place after {took:.1}s"));
        } else if (obs.tic - self.settle_tic) as f64 > waiting.iter().map(|e| 1.5 * e.travel).fold(f64::NEG_INFINITY, f64::max) + TICRATE {
            let e = &waiting[0];
            self.log.add(
                obs.tic,
                format!(
                    "sector {} didn't reach {} in {took:.1}s (expected {:.1}s, it's at {}); going on",
                    e.sector,
                    fmt_heights(e.heights),
                    e.travel / TICRATE,
                    fmt_live(obs.height(e.sector))
                ),
            );
        } else {
            self.progress_tic = obs.tic;
            return self.motion.hold(obs, (0.0, 0.0), self.look);
        }
        self.settle.clear();
        self.advance(obs.tic);
        if self.done {
            return Controls::default();
        }
        self.walk(obs)
    }

    /// Check what fired triggers are doing against what the plan expected, and log it.
    pub(super) fn check_watches(&mut self, obs: &Observation) {
        let mut log: Vec<String> = Vec::new();
        for w in &mut self.watches {
            let e = &w.expect;
            let Some(live) = obs.height(e.sector) else { continue };
            if w.over {
                continue;
            }
            let at = (live.floor - e.heights.floor).abs() < 1.0 && (live.ceiling - e.heights.ceiling).abs() < 1.0;
            match w.reached {
                None => {
                    if at {
                        w.reached = Some(obs.tic);
                        let took = (obs.tic - w.since) as f64 / TICRATE;
                        if e.needed || took > 1.5 * e.travel / TICRATE + 1.0 {
                            log.push(format!("sector {} reached {} after {took:.1}s (expected {:.1}s)", e.sector, fmt_heights(e.heights), e.travel / TICRATE));
                        }
                    } else if !w.late && ((obs.tic - w.since) as f64) > 1.5 * e.travel + 2.0 * TICRATE {
                        w.late = true;
                        log.push(format!(
                            "sector {} hasn't reached {} {:.1}s after line {} (expected {:.1}s); it's at {}",
                            e.sector,
                            fmt_heights(e.heights),
                            (obs.tic - w.since) as f64 / TICRATE,
                            w.line,
                            e.travel / TICRATE,
                            fmt_live(Some(live))
                        ));
                    }
                }
                Some(r) => {
                    if let Some(hold) = e.hold {
                        let since = (obs.tic - r) as f64;
                        if (!at && since > hold) || since > hold + e.travel {
                            w.over = true;
                            if e.needed {
                                log.push(format!("sector {}'s window is over (line {})", e.sector, w.line));
                            }
                        }
                    }
                }
            }
        }
        for m in log {
            self.log.add(obs.tic, m);
        }
    }

    /// Did the temporary state the step before set up for this sector (lift down, door open)
    /// run out? Each closing counts once.
    fn window_over(&mut self, sector: usize) -> bool {
        let prev = self.step_i - 1;
        if let Some(w) = self.watches.iter_mut().find(|w| w.over && !w.used && w.expect.sector == sector && w.step == prev) {
            w.used = true;
            return true;
        }
        false
    }

    /// Has the trigger done something? A sector it moves has moved since it fired, or is at a
    /// height it moves to already (a door open anyway).
    fn effect_started(&self, obs: &Observation, t: &crate::navigator::TriggerLine) -> bool {
        if t.effects.is_empty() {
            return true; // (nothing to check)
        }
        for (s, hs) in &t.effects {
            let Some(live) = obs.height(*s) else { continue };
            let was = self.before.as_ref().and_then(|b| b.iter().find(|(x, _)| x == s).and_then(|(_, h)| *h));
            if was.is_some_and(|w| (live.floor - w.floor).abs() > 0.5 || (live.ceiling - w.ceiling).abs() > 0.5) {
                return true;
            }
            if hs.iter().any(|h| (live.floor - h.floor).abs() < 1.0 && (live.ceiling - h.ceiling).abs() < 1.0) {
                return true;
            }
        }
        false
    }

    /// At the pickup spot without the item: walk straight at it a moment.
    pub(super) fn fetch(&mut self, obs: &Observation) -> Controls {
        let key = self.step().key;
        if obs.keys.is_some_and(|k| key.is_some_and(|sk| k.has(sk))) {
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        let Some(item) = self.item else {
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        };
        if obs.tic - self.progress_tic > 3 * 35 {
            self.log.add(obs.tic, format!("couldn't pick up {} at ({:.0}, {:.0}); carrying on", key.map_or("?", Key::name), obs.x, obs.y));
            self.phase = Phase::Act;
            return self.act_at_goal(obs);
        }
        let hd = heading(obs.pos(), item);
        let speed = 6.0f64.min(0.3 * dist(obs.pos(), item) + 1.0);
        self.motion.go(obs, hd, speed, Some(hd))
    }

    /// At the end of a walk-over step without having crossed its line: step over it.
    fn walk_across(&mut self, obs: &Observation) -> Controls {
        let pos = obs.pos();
        let (a, b) = self.view.nav.map.line_seg(self.trigger_line_no());
        let q = closest_point(pos, a, b);
        let length = dist(a, b).max(1e-6);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let side = (pos.0 - q.0) * nx + (pos.1 - q.1) * ny;
        let sign = if side > 0.0 { -1.0 } else { 1.0 };
        // aim 24 past the line, at the nearest point along it (not too near its ends)
        let t = if length > 64.0 { (((q.0 - a.0) * (b.0 - a.0) + (q.1 - a.1) * (b.1 - a.1)) / (length * length)).clamp(0.15, 0.85) } else { 0.5 };
        let on = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let target = (on.0 + sign * nx * 24.0, on.1 + sign * ny * 24.0);
        if obs.tic - self.progress_tic > 3 * 35 {
            self.log.add(obs.tic, format!("could not cross line {}", self.trigger_line_no()));
            self.crossed = true; // (give up on it and carry on; replanning catches problems)
        }
        let hd = heading(pos, target);
        self.motion.go(obs, hd, 8.0f64.min(0.3 * dist(pos, target) + 2.0), Some(hd))
    }

    /// Is her floor at a height where a level shot hits the gun line? (Shots go straight out;
    /// some switches are hit from a lift at the right height.) Waits a few seconds at most.
    fn at_shot_height(&mut self, obs: &Observation, li: usize) -> bool {
        let nav = &self.view.nav;
        let Some(here) = nav.mesh.poly_at(obs.pos()) else { return true };
        let Some(&(lo, hi)) = nav.shot_floors.get(&(li, here)) else { return true };
        let floor = obs.heights[nav.sector_of_poly(here)].floor;
        // (well in, and still: the shot goes a few tics after the trigger's pulled, and a lift
        // rising through the bottom of the range has gone on by then: MAP16's table)
        let still = self.shot_floor.replace(floor).is_some_and(|f| (f - floor).abs() < 0.5);
        if still && lo + SHOT_MARGIN <= floor && floor <= hi - SHOT_MARGIN {
            return true;
        }
        let since = *self.height_wait.get_or_insert_with(|| {
            self.log.add(obs.tic, format!("waiting to be at the right height to shoot line {li} (floor {floor:.0}, needs {lo:.0}..{hi:.0})"));
            obs.tic
        });
        obs.tic - since > 6 * 35
    }

    /// Right after finishing a step: whatever comes next.
    fn carry_on(&mut self, obs: &Observation) -> Controls {
        if self.done || self.failed {
            return Controls::default();
        }
        if self.phase == Phase::Settle {
            return self.settle_step(obs);
        }
        self.walk(obs)
    }

    pub(super) fn act_at_goal(&mut self, obs: &Observation) -> Controls {
        let step = self.step().clone();
        let view = Rc::clone(&self.view);
        let nav = &view.nav;
        if step.action == Action::Endure {
            // an exit floor: stand on it while it hurts her down to 10 health, which ends the
            // level (E1M8)
            self.step_tic = obs.tic;
            self.progress_tic = obs.tic;
            return self.motion.hold(obs, (0.0, 0.0), self.look);
        }
        if step.action == Action::Cross && !self.crossed && step.trigger.is_some() {
            return self.walk_across(obs);
        }
        if let Some(t) = step.trigger.as_ref().filter(|t| step.action == Action::Cross && t.info.category != Category::Exit) {
            if self.cross_tic.is_none() {
                self.before = Some(t.effects.iter().map(|(s, _)| (*s, obs.height(*s))).collect());
                self.cross_tic = Some(obs.tic);
            }
            if !self.effect_started(obs, t) {
                if obs.tic - self.cross_tic.unwrap_or(obs.tic) < 35 {
                    return self.motion.hold(obs, (0.0, 0.0), self.look);
                }
                self.log.add(obs.tic, format!("crossing line {} didn't do anything", t.line));
            }
        }
        if matches!(step.action, Action::Pickup | Action::Cross | Action::Walk) {
            self.next_step(obs.tic);
            return self.carry_on(obs);
        }
        if step.action == Action::Kill {
            self.log.add(obs.tic, "skipping boss kill (no monsters in this run)");
            self.next_step(obs.tic);
            return Controls::default();
        }
        let Some(t) = step.trigger.clone() else {
            self.next_step(obs.tic);
            return self.carry_on(obs);
        };
        // aim at the nearest part of the line (a little in from its ends): the use trace is a
        // 64-unit ray, and from off to one side the line's middle can be out of its reach
        let li = t.linedef().unwrap_or(0);
        let (a, b) = nav.map.line_seg(li);
        let length = dist(a, b).max(1e-6);
        let pos = obs.pos();
        let mut tt = ((pos.0 - a.0) * (b.0 - a.0) + (pos.1 - a.1) * (b.1 - a.1)) / (length * length);
        tt = tt.max(0.5f64.min(8.0 / length)).min(0.5f64.max(1.0 - 8.0 / length));
        if step.action == Action::Shoot {
            // (a shot reaches: the middle, where it's widest to hit; else the part of it in
            // sight: MAP10's door 296 from the step down the room, its middle behind the alcove's
            // corner, shot at 18 times)
            tt = SHOT_AIMS.into_iter().find(|&t| self.shot_clear(obs, pos, (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), li)).unwrap_or(0.5);
        }
        let aim = (a.0 + (b.0 - a.0) * tt, a.1 + (b.1 - a.1) * tt);
        let look = heading(pos, aim);
        let err = norm(look - obs.angle).abs();
        let span = norm(heading(pos, a) - heading(pos, b)).abs();
        // hold still (brake) while turning to face the line
        let last_seg = &self.segments[self.segments.len() - 1].track;
        let mut end = last_seg.pts[last_seg.pts.len() - 1];
        if self.press_tries > 0 {
            // missed: stand a bit closer each time
            let d = dist(end, aim);
            let k = (8.0 * f64::from(self.press_tries)).min((d - PLAYER_RADIUS - 4.0).max(0.0)) / d.max(1e-6);
            end = (end.0 + (aim.0 - end.0) * k, end.1 + (aim.1 - end.1) * k);
        }
        let mut controls = self.motion.hold(obs, ((end.0 - obs.x) * 0.2, (end.1 - obs.y) * 0.2), Some(look));
        let facing = err < if step.action == Action::Shoot { 4.0f64.min(span / 3.0) } else { 4.0 };
        if step.action == Action::Face {
            // walk up to it and look at it; pressing is left to a person
            if facing {
                self.log.add(obs.tic, format!("facing line {li}"));
                self.next_step(obs.tic);
            }
            return controls;
        }
        if step.action == Action::Use && self.action_tic.is_none() {
            if let Some((s0, _)) = t.shift_from {
                // stairs don't build while their first step is moving (MAP21: a lift as well):
                // wait till it's back up where the stairs left it
                let top = self.states[s0].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
                if obs.height(s0).is_some_and(|l| l.floor < top - 0.5) {
                    self.progress_tic = obs.tic;
                    self.step_tic = obs.tic;
                    return controls;
                }
            }
        }
        let pressing = matches!(step.action, Action::Use | Action::Shoot) && self.action_tic.is_none();
        if pressing {
            if let Some(relevant) = &t.relevant {
                // where it takes things depends on what's round it when pressed: wait for the
                // lifts and doors among those to be where the plan has them (MAP09)
                let init = &nav.movers.initial;
                let off: Vec<usize> = relevant
                    .iter()
                    .copied()
                    .filter(|&x| {
                        obs.height(x).is_some_and(|l| {
                            let h = rep(&self.states[x], init[x]);
                            (l.floor - h.floor).abs() > 0.5 || (l.ceiling - h.ceiling).abs() > 0.5
                        })
                    })
                    .collect();
                if !off.is_empty() && self.settle_logged != self.step_i {
                    self.log.add(obs.tic, format!("waiting for sector {} to settle before line {}", off[0], t.line));
                    self.settle_logged = self.step_i;
                    self.goal_tic = obs.tic;
                }
                if !off.is_empty() && obs.tic - self.goal_tic < SETTLE_WAIT {
                    self.progress_tic = obs.tic;
                    self.step_tic = obs.tic;
                    return controls;
                }
            }
        }
        if pressing && t.once() && t.shift_from.is_none() {
            // Doom ignores a switch on a sector that's busy (a door open on its timer), and a
            // once-only one is still there to press then: wait till what it works is at rest
            // (E3M4). What it works if she knows, and what's moving right by it, which she sees.
            let init = &nav.movers.initial;
            let mut cands: BTreeSet<usize> = t.effects.iter().map(|(s, _)| *s).collect();
            cands.extend(self.sectors_near(li));
            let busy: Vec<usize> = cands
                .into_iter()
                .filter(|&x| {
                    let Some(&m) = self.moved_at.get(&x) else { return false };
                    obs.tic - m < BUSY_TICS
                        && !(obs.tic - m > REST_TICS
                            && obs.height(x).is_some_and(|l| (l.floor - init[x].floor).abs() < 0.5 && (l.ceiling - init[x].ceiling).abs() < 0.5))
                })
                .collect();
            // (its own timer: MAP19's door switch, pressed with the lift set off on the way still
            // running, did nothing, forty-odd times)
            if !busy.is_empty() && self.rest_logged != self.step_i {
                self.log.add(obs.tic, format!("waiting for sector {} to come to rest before line {}", busy[0], t.line));
                self.rest_logged = self.step_i;
                self.rest_tic = obs.tic;
            }
            if !busy.is_empty() && obs.tic - self.rest_tic < SETTLE_WAIT {
                self.progress_tic = obs.tic;
                self.step_tic = obs.tic;
                return controls;
            }
        }
        if pressing {
            if let Some(nt) = self
                .steps
                .get(self.step_i as usize + 1)
                .filter(|n| n.action == Action::Cross)
                .and_then(|n| n.trigger.clone())
                .filter(|nt| nt.info.trigger == Trigger::WR && !nt.permanent())
            {
                // the next step crosses a lift line for its window: if that lift is away (set
                // off on the way, MAP09), wait for it now, before this press starts a timer of
                // its own the wait would use up
                let init = &nav.movers.initial;
                let away: Vec<usize> =
                    nt.effects.iter().map(|(s, _)| *s).filter(|&x| obs.height(x).is_some_and(|l| (l.floor - init[x].floor).abs() > 0.5)).collect();
                if !away.is_empty() && obs.tic - self.step_tic < CROSS_REST_WAIT {
                    if !self.cross_wait_logged {
                        self.log.add(obs.tic, format!("sector {} already away: waiting for it to come back before line {}", away[0], t.line));
                        self.cross_wait_logged = true;
                    }
                    self.progress_tic = obs.tic;
                    return controls;
                }
            }
        }
        if step.action == Action::Shoot && self.action_tic.is_none() && obs.weapon != Some(tap_gun(obs)) {
            // a fist or chainsaw (picked up on the way: the game switches to it) doesn't set
            // off a gun line (MAP19), nor anything that fires a missile; a shotgun's pellets
            // spread (MAP16's flame panel, 3 degrees across): out with the pistol first
            controls.weapon = tap_gun(obs);
            self.progress_tic = obs.tic;
            return controls;
        }
        if step.action == Action::Shoot && self.action_tic.is_none() && facing && !self.at_shot_height(obs, li) {
            self.progress_tic = obs.tic; // (waiting for the floor, a lift, to bring her level)
            return controls;
        }
        if self.action_tic.is_none() && facing {
            self.action_tic = Some(obs.tic);
        }
        if let Some(at) = self.action_tic {
            let since = obs.tic - at;
            match step.action {
                Action::Use | Action::Exit => controls.use_ = since == 0,
                Action::Shoot => controls.attack = since < 10,
                _ => {}
            }
            if since == 0 {
                self.before = Some(t.effects.iter().map(|(s, _)| (*s, obs.height(*s))).collect());
                let tries = if self.press_tries > 0 { format!(" (try {})", self.press_tries + 1) } else { String::new() };
                self.log.add(obs.tic, format!("{} line {li}{tries}", step.action.name()));
            }
            // did it do anything? (an exit ends the level; otherwise something should move)
            if since > 8 && step.action != Action::Exit && self.effect_started(obs, &t) {
                self.next_step(obs.tic);
            } else if since > if step.action == Action::Exit { 2 * 35 } else { 25 } {
                self.press_tries += 1;
                if self.press_tries < PRESS_TRIES {
                    self.log.add(obs.tic, format!("line {li} didn't do anything, trying again closer"));
                    self.action_tic = None;
                } else {
                    self.log.add(obs.tic, format!("line {li} didn't work after {PRESS_TRIES} tries; carrying on"));
                    self.failed_presses.push((obs.pos(), li));
                    self.next_step(obs.tic);
                }
            }
        } else if obs.tic - self.progress_tic > 3 * 35 {
            return self.fail(obs, &format!("could not face line {li}"));
        }
        controls
    }
}
