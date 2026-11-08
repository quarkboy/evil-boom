//! Lining up, executing and judging jumps and runs over sinking floors.

use super::{run_controls, RouteFollower, Run};
use crate::geom::{closest_on_portal, closest_point, dist, funnel, oriented, Point, Seg, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::jumps::RunStart;
use crate::motor::{fmt_live, heading, norm, Controls, Observation, TICRATE};
use crate::navigator::{Jump, MAX_STEP};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, BTreeSet};
use std::rc::Rc;

/// tics between re-aims running over sinking floors
const RUN_REAIM: i64 = 4;

/// how much lower a sinking floor is taken to be by the time she's off it
const RUN_AHEAD_SINK: f64 = 4.0;
/// how far round to look for solid ground off a sinking floor
const RUN_LOOK: f64 = 1024.0;
/// how far along a way over sinking floors to look for one that stays put
const RUN_PATH_LOOK: f64 = 2048.0;
/// how far onto the floor that stays put the run aims
const RUN_INTO: f64 = 48.0;
/// units a tic she covers running across (near full speed)
const RUN_SPEED: f64 = 16.0;
/// units a tic a floor goes down (Doom's lowering floors, FLOORSPEED)
const SINK_RATE: f64 = 1.0;

/// a jump goes without settling if what it needs runs out sooner
const JUMP_HURRY: f64 = 2.0 * TICRATE;

/// how much lower than planned a ceiling can be for a jump
const JUMP_CEILING_SLACK: f64 = 16.0;

impl RouteFollower {
    /// At the run-up: settle on the exact spot facing the takeoff, then sprint.
    pub(super) fn line_up(&mut self, obs: &Observation, jump: &Jump) -> Controls {
        let hd = heading(jump.runup, jump.takeoff);
        // (faced off the way for a strafing run: MAP03's well)
        let face = jump.facing(hd);
        let diff = norm(face - obs.angle);
        // the heights the jump was found with must be there, and not still moving (if one has
        // stopped somewhere else it's not coming: jump from there anyway)
        for &(sector, h) in &jump.requires {
            let live = obs.height(sector);
            // (a ceiling a little lower is fine: MAP09's door just starting to close as the
            // lift brings her up for the jump through it)
            let off = live.is_none_or(|l| (l.floor - h.floor).abs() > 0.5 || l.ceiling > h.ceiling + 0.5 || l.ceiling < h.ceiling - JUMP_CEILING_SLACK);
            if !off {
                continue;
            }
            if self.jump_wait.is_none_or(|(s, l, _)| (s, l) != (sector, live)) {
                if self.jump_wait.is_none_or(|(s, _, _)| s != sector) {
                    self.log.add(obs.tic, format!("waiting for sector {sector} to reach {} before jumping (now {})", fmt_live(Some(h)), fmt_live(live)));
                }
                self.jump_wait = Some((sector, live, obs.tic));
            }
            // still moving lately (a lift waits ~3s at the bottom before coming back)
            let still_for = if self.lift_sectors.contains(&sector) { 5 * 35 } else { 35 };
            let since = self.jump_wait.map_or(obs.tic, |w| w.2);
            if obs.tic - since < still_for {
                self.lineup_tic = None;
                self.progress_tic = obs.tic;
                let mut spot = jump.runup;
                if let Some(here) = self.standing(obs).0 {
                    let poly = self.view.nav.mesh.poly(here);
                    if poly.sector == sector {
                        // riding it (set off on the way to the run-up: MAP06): the middle, clear
                        // of the edges, or it can't come back up with her
                        spot = poly.center;
                    }
                }
                return self.motion.hold(obs, ((spot.0 - obs.x) * 0.3, (spot.1 - obs.y) * 0.3), Some(face));
            }
            if live.is_none_or(|l| l.floor < h.floor - 0.5) {
                // stopped lower than the jump needs: find another way with things as they are
                self.jump_wait = None;
                if self.replan(obs, &format!("sector {sector} at {}, too low for the jump", fmt_live(live)), true) {
                    return self.motion.hold(obs, (0.0, 0.0), Some(face));
                }
                return self.fail(obs, "jump takeoff too low and no other way");
            }
        }
        // as exactly as the check did it: on the spot, square on, standing still
        let mut settled = dist(obs.pos(), jump.runup) < 1.5 && diff.abs() <= 0.75 && self.motion.speed() < 0.3 && self.motion.rate.abs() < 0.5;
        // something the jump needs is about to go (MAP09: the door it goes through starts
        // shutting a moment after the lift brings her up): no settling, go once facing
        let needs: BTreeSet<usize> = jump.requires.iter().map(|(s, _)| *s).collect();
        if self.watches.iter().any(|w| {
            needs.contains(&w.expect.sector)
                && !w.over
                && w.expect.hold.is_some_and(|hold| w.reached.is_some_and(|r| r as f64 + hold - (obs.tic as f64) < JUMP_HURRY))
        }) && dist(obs.pos(), jump.runup) < 6.0
            && diff.abs() <= 3.0
        {
            settled = true;
        }
        let lineup = *self.lineup_tic.get_or_insert(obs.tic);
        // (settling longer than a second: go anyway, if near enough on the spot; much longer:
        // from wherever)
        let near = dist(obs.pos(), jump.runup) < 6.0;
        if settled || (obs.tic - lineup > 35 && diff.abs() <= 1.5 && self.motion.speed() < 0.5 && (near || obs.tic - lineup > 3 * 35)) {
            self.lineup_tic = None;
            self.sprint = Some((jump.clone(), hd, obs.tic));
            self.log.add(obs.tic, format!("run-jump from ({:.0}, {:.0}){}", jump.takeoff.0, jump.takeoff.1, if jump.strafe { ", strafing" } else { "" }));
            return self.sprint_jump(obs);
        }
        self.progress_tic = obs.tic;
        self.motion.hold(obs, ((jump.runup.0 - obs.x) * 0.3, (jump.runup.1 - obs.y) * 0.3), Some(face))
    }

    /// Running over floors that sink once stepped on (MAP08's exit: platforms in nukage that go
    /// down as she lands). No stopping to line up a jump: keep running, and every few tics pick
    /// (with the movement simulator, from where she is and how fast she's going) the heading that
    /// comes down on the floor nearest the goal. Over once on a floor that stays put.
    pub(super) fn run_step(&mut self, obs: &Observation) -> Controls {
        let mesh = Rc::clone(&self.view);
        let here = mesh.nav.mesh.poly_at(obs.pos());
        let here_sector = here.map(|h| mesh.nav.sector_of_poly(h));
        let on_ground = match (obs.z, here_sector) {
            (Some(z), Some(s)) => z <= obs.heights[s].floor + 0.5,
            _ => true,
        };
        if on_ground && here_sector.is_some_and(|s| !self.sinking.contains(&s)) {
            self.run = None;
            self.run_for = None;
            self.log.add(obs.tic, format!("ran across to ({:.0}, {:.0}) sector {}", obs.x, obs.y, here_sector.unwrap_or(0)));
            // (where she was going is forgotten, the user's rule: maybe gone down too, the way
            // back to it surely; the explorer chooses again from here. E3M1: across to the far
            // bank, then back for the old goal on the first stone, now in the blood, dropped in)
            return self.fail(obs, "ran off a floor going down: what she was going for is for choosing again");
        }
        let Some(run) = self.run else { return self.walk(obs) };
        // (the way to the nearest floor that stays put, along the floors she can still step
        // onto, steered along every tic: a winding way of them is followed round its bends)
        if let (true, Some(sector)) = (on_ground, here_sector) {
            if let Some((h, target)) = self.run_path(obs) {
                if self.run_for != Some(target) {
                    self.log.add(obs.tic, format!("running off sector {sector} for sector {target}, the nearest floor that stays put"));
                    self.run_for = Some(target);
                }
                self.run = Some(Run { heading: h, sector: Some(sector), next: obs.tic + RUN_REAIM });
                self.motion.rate = 0.0;
                return run_controls(obs.angle, h);
            }
        }
        if on_ground && (obs.tic >= run.next || here_sector != run.sector) {
            let Some(sector) = here_sector else { return self.walk(obs) };
            // (on the heading she's on if it's still as good: turning about on every look
            // loses the speed the next gap needs; MAP08's, swung from one platform to the next
            // and back, off the edge between them)
            let keep = run.sector.map(|_| run.heading);
            match self.run_heading(obs, sector, keep) {
                None => {
                    self.run = None;
                    self.log.add(obs.tic, "no heading off the sinking floor lands anywhere");
                    if !self.replan(obs, "sinking floor", true) {
                        return self.fail(obs, "stranded on a sinking floor");
                    }
                    return self.walk(obs);
                }
                Some((h, why, land)) => {
                    if self.run.is_none_or(|r| norm(r.heading - h).abs() > 20.0) {
                        self.log.add(obs.tic, format!("running at {h:.0} degrees off sector {sector}, for sector {land} ({why})"));
                    }
                    self.run = Some(Run { heading: h, sector: Some(sector), next: obs.tic + RUN_REAIM });
                }
            }
        }
        self.motion.rate = 0.0;
        run_controls(obs.angle, self.run.map_or(obs.angle, |r| r.heading))
    }

    /// The heading along the shortest way (string-pulled through the navmesh) to the nearest
    /// floor that stays put, over floors she can step onto as they are now (no hurting floors;
    /// sinking ones not gone too low yet), and the sector it gets to. The user's rule: the
    /// floor going down, where she was going is forgotten, for the nearest ground that holds.
    /// (E3M1's way over the blood winds: a heading held for the fan's run went off a bend into
    /// the blood, or braked her to a stop on a floor going down.)
    pub(super) fn run_path(&self, obs: &Observation) -> Option<(f64, usize)> {
        let nav = &self.view.nav;
        let mesh = &nav.mesh;
        let start = mesh.poly_at(obs.pos())?;
        let height = |s: usize| obs.height(s).map_or((nav.movers.initial[s].floor, nav.movers.initial[s].ceiling), |l| (l.floor, l.ceiling));
        let solid = |s: usize| nav.damage[s] == 0 && !self.sinking.contains(&s);
        let n = mesh.polys.len();
        let mut cost = vec![f64::INFINITY; n];
        let mut entry = vec![obs.pos(); n];
        let mut came: Vec<Option<(usize, usize)>> = vec![None; n];
        cost[start] = 0.0;
        let mut open = BinaryHeap::new();
        open.push(Reverse((0u64, start)));
        // (the floors that stay put she can get to, nearest first: the first poly of each)
        let mut found: Vec<usize> = Vec::new();
        while let Some(Reverse((_, p))) = open.pop() {
            let s = mesh.poly(p).sector;
            if p != start && solid(s) {
                if !found.iter().any(|&g| mesh.poly(g).sector == s) {
                    found.push(p);
                }
                continue;
            }
            if cost[p] > RUN_PATH_LOOK {
                continue;
            }
            let (f, c) = height(s);
            for &qi in &mesh.poly(p).portals {
                let q = &mesh.portals[qi];
                let o = q.other(p);
                let so = mesh.poly(o).sector;
                let (fo, co) = height(so);
                if nav.damage[so] != 0 || fo > f + MAX_STEP || co.min(c) - fo.max(f) < PLAYER_HEIGHT || dist(q.p1, q.p2) < 2.0 * PLAYER_RADIUS {
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
        // (the way to one, and where it heads first)
        let way = |goal: usize| {
            let mut portals = Vec::new();
            let mut p = goal;
            while let Some((from, qi)) = came[p] {
                let q = &mesh.portals[qi];
                portals.push(oriented(q.p1, q.p2, mesh.poly(from).center));
                p = from;
            }
            portals.reverse();
            // (well onto it, not its edge)
            let (into, centre) = (entry[goal], mesh.poly(goal).center);
            let d = dist(into, centre).max(1.0);
            let end = (into.0 + (centre.0 - into.0) / d * d.min(RUN_INTO), into.1 + (centre.1 - into.1) / d * d.min(RUN_INTO));
            let next = funnel(obs.pos(), end, &portals).get(1).copied().unwrap_or(end);
            heading(obs.pos(), next)
        };
        // (the one she's making for while she still can, bends and all; else the nearest on
        // ahead of her, the way she was going (back is the way she came, a floor gone down
        // behind her maybe, and on is the way on: E3M1's way over the blood, its trigger once
        // only, back to where she'd started was the level lost); else the nearest)
        let d = self.motion.disp;
        let going = (d.0.hypot(d.1) > 2.0).then(|| d.1.atan2(d.0).to_degrees());
        let goal = found
            .iter()
            .copied()
            .find(|&g| Some(mesh.poly(g).sector) == self.run_for)
            .or_else(|| found.iter().copied().find(|&g| going.is_none_or(|a| norm(way(g) - a).abs() < 90.0)))
            .or_else(|| found.first().copied())?;
        Some((way(goal), mesh.poly(goal).sector))
    }

    /// The direction to push (from a fan all round: pushing back against her momentum may be
    /// what keeps her on) whose simulated run lands, off this floor, on the nearest floor that
    /// stays put; else on the sinking floor nearest one, to run on from. Where she was going
    /// doesn't come into it: the floor going down under her, wherever it was is going too, maybe
    /// (MAP08's: a look at a wall from the platform she was running over, every way off judged
    /// by the way back to it, none, stood there and went down with it). The user's rule.
    fn run_heading(&self, obs: &Observation, sector: usize, keep: Option<f64>) -> Option<(f64, &'static str, usize)> {
        let nav = &self.view.nav;
        let mut heights: Vec<(f64, f64)> =
            nav.movers.initial.iter().enumerate().map(|(s, h)| obs.height(s).map_or((h.floor, h.ceiling), |l| (l.floor, l.ceiling))).collect();
        let (f, c) = heights[sector];
        heights[sector] = (f - RUN_AHEAD_SINK, c); // (it keeps going down meanwhile)
        let d = self.motion.disp;
        let going = if d.0.hypot(d.1) > 2.0 { d.1.atan2(d.0).to_degrees() } else { obs.angle };
        let mut offs: Vec<f64> = (-12..12).map(|i| f64::from(i) * 15.0).collect();
        if let Some(k) = keep {
            offs.push(norm(k - going));
        }
        let kept = keep.map(|k| norm(k - going));
        let start = RunStart { pos: obs.pos(), vel: d, z: obs.z.unwrap_or(f), angle: obs.angle };
        let headings: Vec<f64> = offs.iter().map(|o| going + o).collect();
        let landings = nav.jump_lab.run_landings(&start, &heights, sector, &headings, 70);
        // (one not yet going down holds a while once she's on it; one going down already is
        // lower every tic)
        let moving = |s: usize| heights[s].0 < nav.movers.initial[s].floor - 0.5;
        let solid = |s: usize| nav.damage[s] == 0 && !self.sinking.contains(&s);
        // (footing that holds: solid ground, and a sinking floor not going down yet, which goes
        // only once she's on it: a path of them is ground till she's along it, E3M1's way over
        // the blood, the far bank further than a floor sinks in the time to run there)
        let holds = |s: usize| solid(s) || (s != sector && nav.damage[s] == 0 && !moving(s));
        // (the banks: the edges where footing that holds meets the nukage or a floor going down,
        // near enough to matter; a room behind the pit's wall is no nearer for being close)
        let near_solid: Vec<(Seg, f64)> = nav
            .mesh
            .portals
            .iter()
            .filter_map(|q| {
                let (sa, sb) = (nav.sector_of_poly(q.a), nav.sector_of_poly(q.b));
                let bank = match (holds(sa), holds(sb)) {
                    (true, false) => sa,
                    (false, true) => sb,
                    _ => return None,
                };
                (dist(closest_point(obs.pos(), q.p1, q.p2), obs.pos()) < RUN_LOOK).then_some(((q.p1, q.p2), heights[bank].0))
            })
            .collect();
        // (how far a sinking landing leaves her from footing that holds she could still step up onto
        // when she got there, the floor under her going down all the while)
        let to_solid = |at: Point, floor: f64| {
            near_solid
                .iter()
                .map(|((a, b), h)| (dist(at, closest_point(at, *a, *b)), *h))
                .filter(|(d, h)| *h <= floor - d / RUN_SPEED * SINK_RATE + MAX_STEP)
                .map(|(d, _)| d)
                .fold(f64::INFINITY, f64::min)
        };
        // (best first: the heading she's on while it still lands somewhere she'd get off from;
        // else the one nearest straight on, her momentum kept (the user's: turning about loses
        // the speed the gaps need, and a long run aimed at the far bank slid off the side of the
        // platforms, MAP08); then solid ground, a floor not going down yet, one going down)
        let mut best: Option<(bool, f64, u8, f64, f64, usize)> = None;
        for (off, landing) in offs.iter().zip(landings) {
            let Some((at, land)) = landing else { continue };
            if nav.damage[land] != 0 || heights[land].0 < f - MAX_STEP {
                continue; // (into the nukage)
            }
            let (tier, cost) = if solid(land) {
                (0, dist(obs.pos(), at))
            } else {
                (if moving(land) { 2 } else { 1 }, to_solid(at, heights[land].0))
            };
            if cost.is_infinite() {
                continue; // (no solid ground she'd make from there)
            }
            let new = Some(*off) != kept;
            if best.is_none_or(|(bn, bo, bt, bc, _, _)| (new, off.abs(), tier, cost) < (bn, bo, bt, bc)) {
                best = Some((new, off.abs(), tier, cost, going + off, land));
            }
        }
        best.map(|(_, _, tier, _, h, land)| (h, ["solid ground", "a sinking floor not going down yet", "a sinking floor going down"][usize::from(tier)], land))
    }

    /// A run-jump done: once she's on a floor again, did it come down on its landing?
    pub(super) fn judge_landing(&mut self, obs: &Observation) {
        let Some((jump, tic)) = self.touchdown.clone() else { return };
        let nav = &self.view.nav;
        let here = nav.mesh.poly_at(obs.pos());
        let on_floor = match (here, obs.z) {
            (Some(h), Some(z)) => (z - obs.heights[nav.sector_of_poly(h)].floor).abs() < 1.0,
            _ => false,
        };
        if !on_floor {
            if obs.tic - tic > 2 * 35 {
                self.touchdown = None; // (still falling, or off the map: no telling)
            }
            return;
        }
        self.touchdown = None;
        let want = nav.sector_of_poly(jump.dest_poly);
        let got = here.map_or(0, |h| nav.sector_of_poly(h));
        if got != want {
            self.log.add(obs.tic, format!("missed the jump off ({:.0}, {:.0}): came down in sector {got}, not {want}", jump.takeoff.0, jump.takeoff.1));
            self.missed.push(jump);
        }
    }

    /// Full speed in a straight line, as the check did it: only to the edge for a soft jump
    /// (then coast: it lands slower, for narrow strips), else through the flight. Touching down
    /// on the landing's sector, stop sprinting so the landing is braked at once.
    pub(super) fn sprint_jump(&mut self, obs: &Observation) -> Controls {
        let Some((jump, hd, since)) = self.sprint.clone() else { return self.walk(obs) };
        let nav = Rc::clone(&self.view);
        let (ux, uy) = (hd.to_radians().cos(), hd.to_radians().sin());
        let flown = (obs.x - jump.takeoff.0) * ux + (obs.y - jump.takeoff.1) * uy;
        let here = nav.nav.mesh.poly_at(obs.pos());
        // landed only on the sector the jump was checked to land on (a jump can skim another
        // floor on the way: E4M2's)
        let landed = flown > 0.0
            && here.is_some_and(|h| {
                let s = nav.nav.sector_of_poly(h);
                s == nav.nav.sector_of_poly(jump.dest_poly) && obs.z.is_some_and(|z| (z - obs.heights[s].floor).abs() < 1.0)
            });
        if landed || flown >= dist(jump.takeoff, jump.dest) - 8.0 || obs.tic - since > 4 * 35 {
            self.sprint = None;
            self.touchdown = Some((jump.clone(), obs.tic));
            let land = here.map(|h| nav.nav.sector_of_poly(h));
            self.log.add(obs.tic, format!("landed at ({:.0}, {:.0}) sector {}", obs.x, obs.y, land.map_or("None".to_string(), |s| s.to_string())));
            self.seg_i = (self.seg_i + 1).min(self.segments.len() - 1);
            self.enter_segment(obs.tic);
            self.look = Some(hd);
            if let Some(s) = land.filter(|s| self.sinking.contains(s)) {
                // going down under her: no settling, run on across
                self.log.add(obs.tic, format!("sector {s} sinking: running on"));
                self.run = Some(Run { heading: hd, sector: None, next: obs.tic });
                return self.run_step(obs);
            }
            self.path_on(obs);
            // (landing spots can be small: shed the sprint's speed before going on)
            self.brake_until = obs.tic + 2 * 35;
            return self.walk(obs);
        }
        let diff = norm(jump.facing(hd) - obs.angle);
        self.motion.rate = 0.0;
        let push = flown < if jump.soft { jump.release } else { dist(jump.takeoff, jump.dest) - 8.0 };
        // keep on the jump's line till takeoff (MAP27's clears a ledge corner by nothing)
        let mut strafe = 0.0;
        if flown < 0.0 {
            let side = (obs.x - jump.takeoff.0) * uy - (obs.y - jump.takeoff.1) * ux;
            strafe = (-4.0 * side).clamp(-15.0, 15.0); // (side: how far right of the line)
        }
        if push {
            strafe += jump.sidestep();
        }
        Controls { forward: if push { 50.0 } else { 0.0 }, strafe, turn: (-diff).clamp(-3.0, 3.0), ..Controls::default() }
    }
}
