//! Climbs: up a face more than a step high, by way of something lower beside it that the box
//! stands on as it goes (the player stands on the highest floor under the box, so a crate a
//! step up, off to one side of a taller one, is a stair to it: E2M7's crates, the
//! invulnerability on top). The navmesh joins only sectors that touch; these are found by
//! running at every such face in the simulator, and checked after like the jumps.

use super::physics::{Controls, PlayerSim};
use super::{Jump, JumpLab};
use crate::det::HashSet;
use crate::geom::{dist, Point, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::motor::MAX_MOVE;

/// degrees off the face's normal
const CLIMB_ANGLES: [f64; 3] = [0.0, -30.0, 30.0];
/// run-ups tried, longest first (the first the box can stand at)
const CLIMB_RUNUPS: [f64; 3] = [64.0, 32.0, 16.0];
/// map units between takeoff samples along a face
const CLIMB_SPACING: f64 = 16.0;
/// tics of running at it
const CLIMB_TICS: usize = 70;
/// a climb is up the face run at: no further past it than this...
const CLIMB_PAST: f64 = 64.0;
/// how far round a climb's run-up the navmesh is searched for a way up already
const CLIMB_WALK: f64 = 512.0;
/// ...nor pushed off the line run along by more than this (sliding along a wall: somewhere
/// else, up the stairs)
const CLIMB_OFF_LINE: f64 = 8.0;

impl JumpLab {
    /// The climbs up the map's faces too high to step, with the sectors at their starting
    /// heights (those a trigger moves left out): one per (run-up poly, sector reached).
    pub fn find_climbs(&self, moving: &HashSet<usize>, portal_fits: &[bool]) -> Vec<Jump> {
        let mesh = &self.mesh;
        let mut sim = PlayerSim::new(self, self.world());
        let mut found: HashSet<(usize, usize)> = HashSet::default();
        let mut out = Vec::new();
        for portal in &mesh.portals {
            if portal.linedef.is_some_and(|li| self.map.linedefs[li].blocks_player()) {
                continue;
            }
            let ((x1, y1), (x2, y2)) = (portal.p1, portal.p2);
            let width = dist(portal.p1, portal.p2);
            if width < 1.0 {
                continue;
            }
            let mid = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
            for (a, b) in [(portal.a, portal.b), (portal.b, portal.a)] {
                let (sa, sb) = (mesh.poly(a).sector, mesh.poly(b).sector);
                if moving.contains(&sa) || moving.contains(&sb) {
                    continue;
                }
                let ((fa, _), (fb, cb)) = (self.initial[sa], self.initial[sb]);
                if fb - fa <= MAX_STEP || cb - fb < PLAYER_HEIGHT {
                    continue;
                }
                // (the normal into b)
                let (mut nx, mut ny) = (-(y2 - y1) / width, (x2 - x1) / width);
                let ca = mesh.center(a);
                if nx * (mid.0 - ca.0) + ny * (mid.1 - ca.1) < 0.0 {
                    (nx, ny) = (-nx, -ny);
                }
                let samples = ((width / CLIMB_SPACING).floor() as usize).max(1);
                for k in 0..samples {
                    let t = (k as f64 + 0.5) / samples as f64;
                    let takeoff = (x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
                    for ang in CLIMB_ANGLES {
                        let (s, c) = ang.to_radians().sin_cos();
                        let d = (nx * c - ny * s, nx * s + ny * c);
                        let Some(j) = self.climb(&mut sim, takeoff, d, fa, moving) else { continue };
                        let key = (j.src_polys[0], mesh.poly(j.dest_poly).sector);
                        if !found.contains(&key) && !self.walks_to(j.src_polys[0], key.1, portal_fits) {
                            out.push(j);
                        }
                        found.insert(key);
                    }
                }
            }
        }
        out
    }

    /// Is there a way from `src` to `sector` near it on foot, up nothing more than steps, at the
    /// starting heights? (Then a run that gets there up the side of the stairs is no climb:
    /// MAP16's, the box over a step with her centre beside it.)
    pub(super) fn walks_to(&self, src: usize, sector: usize, portal_fits: &[bool]) -> bool {
        let mesh = &self.mesh;
        let origin = mesh.center(src);
        let mut seen: HashSet<usize> = HashSet::default();
        let mut todo = vec![src];
        seen.insert(src);
        while let Some(p) = todo.pop() {
            let s = mesh.poly(p).sector;
            if s == sector {
                return true;
            }
            for &pi in &mesh.poly(p).portals {
                let q = mesh.portals[pi].other(p);
                let t = mesh.poly(q).sector;
                let (f, (g, c)) = (self.initial[s].0, self.initial[t]);
                if !portal_fits.get(pi).copied().unwrap_or(false) || g - f > MAX_STEP || c - g.max(f) < PLAYER_HEIGHT || dist(origin, mesh.center(q)) > CLIMB_WALK {
                    continue;
                }
                if seen.insert(q) {
                    todo.push(q);
                }
            }
        }
        false
    }

    /// Running at the face through `takeoff` along `d` from the first run-up the box can stand
    /// at, on a floor of `floor` or a step above: where she first stands more than a step
    /// higher, if anywhere, with the box over nothing a trigger moves on the way (MAP16's run
    /// past a lift, up a step only at the lift's starting height).
    fn climb(&self, sim: &mut PlayerSim<'_>, takeoff: Point, d: Point, floor: f64, moving: &HashSet<usize>) -> Option<Jump> {
        let mesh = &self.mesh;
        let runup = CLIMB_RUNUPS.iter().map(|r| (takeoff.0 - d.0 * r, takeoff.1 - d.1 * r)).find(|&p| {
            let (clear, f, c) = self.fits_at(p);
            // (a step up already, the box over something low beside the face, will do; but the
            // centre over the floor run from, where she can be got to: not a crate's edge with
            // the room floor under it, filed as a climb from the room)
            let centre = mesh.poly_at(p).map(|q| self.initial[mesh.poly(q).sector].0);
            clear && (-0.5..=MAX_STEP).contains(&(f - floor)) && c - f >= PLAYER_HEIGHT && centre.is_some_and(|g| (-0.5..=MAX_STEP).contains(&(g - floor)))
        })?;
        let src = mesh.poly_at(runup)?;
        sim.place(runup, d.1.atan2(d.0).to_degrees());
        let mut speed = 0.0;
        // (the floor under her centre last, and whether it ever rose more than a step at once:
        // else it's stairs, the navmesh's already)
        let mut under = floor;
        let mut lifted = false;
        for _ in 0..CLIMB_TICS {
            sim.step(Controls { forward: MAX_MOVE, strafe: 0.0, turn: 0.0 });
            if sim.teleported {
                return None;
            }
            let pos = sim.pos();
            let r = PLAYER_RADIUS;
            if [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)].iter().any(|(dx, dy)| mesh.poly_at((pos.0 + dx, pos.1 + dy)).is_some_and(|q| moving.contains(&mesh.poly(q).sector))) {
                return None;
            }
            let (rx, ry) = (pos.0 - runup.0, pos.1 - runup.1);
            if (rx * d.1 - ry * d.0).abs() > CLIMB_OFF_LINE || rx * d.0 + ry * d.1 > dist(runup, takeoff) + CLIMB_PAST {
                return None;
            }
            let (vx, vy) = sim.velocity();
            speed = f64::max(speed, vx.hypot(vy));
            if vx.hypot(vy) < 0.1 && speed > 1.0 {
                return None; // (stopped against it)
            }
            let p = mesh.poly_at(pos)?;
            let high = self.initial[mesh.poly(p).sector].0;
            lifted = lifted || high - under > MAX_STEP;
            under = high;
            // (stood on, the centre over it: not the box over its edge with the centre lower)
            if lifted && sim.on_floor() && (sim.floorz_units() - high).abs() < 0.5 && high > floor + MAX_STEP {
                return Some(Jump::new(src, p, pos, runup, takeoff, speed, Vec::new()));
            }
        }
        None
    }
}
