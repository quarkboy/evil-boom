//! Clearing barrels, choosing a firing position and escaping chain explosions.

use super::{along, barrel_gun, barrel_now, blast_gap, tap_rounds, RouteFollower, BARREL_TAP};
use crate::geom::{dist, segment_distance, Point};
use crate::motor::{heading, norm, Controls, Observation, FULL_SPEED};
use crate::navigator::{States, PLAYER_RADIUS};

/// back off this far before shooting one (blast radius 128)
const BARREL_SAFE: f64 = 150.0;

/// Give up after this many tics trying to clear a barrel from nearby.
const BARREL_GIVE_UP: i64 = 8 * 35;

/// Mortal runs allow longer to walk outside the blasts and shoot from there.
const BARREL_GIVE_UP_FAR: i64 = 20 * 35;

/// a barrel blown up sets off another this near, by `blast_gap` (its blast, 128, and the other's
/// radius, 10)
const BARREL_CHAIN: f64 = 138.0;

/// a barrel hit goes off this many tics later (its death frames, 5 + 5 + 5); so does each one a
/// blast sets off
const BARREL_DELAY: f64 = 15.0;

/// (mortal) no nearer to where to shoot a barrel from in this long: given up
const BARREL_STALL: i64 = 2 * 35;

/// (mortal) shot, no barrel gone this long: the chain's over
const BARREL_SETTLE: i64 = 35;

/// (mortal) shot, she runs off this long...
const BARREL_FLEE_TICS: i64 = 2 * 35;

/// ...at least this fast, counted on (units a tic: well under running, for the turn first)
const BARREL_FLEE_PACE: f64 = 8.0;

/// (mortal) somewhere to shoot a barrel from, looked for this near it...
const BARREL_LOOK: f64 = 1024.0;

/// ...the nearest this many of them, by the way there
const BARREL_POSTS: usize = 12;

/// a barrel's radius (a shot at another that passes this near it hits it instead)
const BARREL_RADIUS: f64 = 10.0;
/// a barrel's height (what a shot at it has to get to)
const BARREL_HEIGHT: f64 = 42.0;

/// Candidate retreat positions sampled backwards along the incoming path.
const BACKTRACK_POINTS: usize = 64;
const BACKTRACK_SPACING: f64 = 16.0;
/// Extra candidates considered because walls, barrels and crushers can reject nearby posts.
const POST_CANDIDATE_FACTOR: usize = 3;

/// Somewhere to shoot a barrel from: how far to go there, where, the barrel to aim at (the one in
/// the way, or another that sets it off), the way there and the way off after.
struct Post {
    length: f64,
    q: Point,
    aim: Point,
    way: Vec<Point>,
    flee: Vec<Point>,
    /// out of every blast of the chain where she shoots from: stay there, no running off
    stay: bool,
}

/// The barrel nearest `p` of a chain going off (each barrel hit, and every one its blast sets
/// off in turn) whose blast reaches `p`.
fn lit_chain_by(obs: &Observation, p: Point) -> Option<Point> {
    let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
    let mut chain: Vec<Point> = obs.barrels_lit.clone();
    let mut i = 0;
    while i < chain.len() {
        let c = chain[i];
        for &x in &barrels {
            if blast_gap(c, x) < BARREL_CHAIN && !chain.iter().any(|&y| dist(x, y) < 1.0) {
                chain.push(x);
            }
        }
        i += 1;
    }
    chain.into_iter().filter(|&b| blast_gap(p, b) < BARREL_SAFE).min_by(|a, b| blast_gap(*a, p).total_cmp(&blast_gap(*b, p)))
}

impl RouteFollower {
    /// Barrels going off that she didn't set off (a monster's shot, a blast): off out of their
    /// chain's blasts if they reach her, else no further on while they'd reach where she'll be
    /// as they go (MAP23: walked on after a chain going off down the field ahead of her, into
    /// it, 24 health). The chain burns out a barrel a blast (15 tics) at a time: then on.
    pub(super) fn keep_off_lit_chain(&mut self, obs: &Observation) -> Option<Controls> {
        let pos = obs.pos();
        let v = obs.velocity.unwrap_or((0.0, 0.0));
        let by = lit_chain_by(obs, pos);
        let ahead = by.is_none()
            && (1..=4).any(|k| {
                let f = BARREL_DELAY * f64::from(k) / 4.0;
                lit_chain_by(obs, (pos.0 + v.0 * f, pos.1 + v.1 * f)).is_some()
            });
        if by.is_none() && !ahead {
            self.chain_wait = false;
            return None;
        }
        if !self.chain_wait {
            self.log.add(obs.tic, "barrels going off ahead: keeping out of their blasts till they're done");
            self.chain_wait = true;
        }
        self.progress_tic = obs.tic;
        Some(match by {
            Some(b) => {
                let h = heading(b, pos);
                self.motion.go(obs, h, FULL_SPEED, Some(h))
            }
            None => self.motion.hold(obs, (0.0, 0.0), None),
        })
    }

    /// Back out of its blast (128 units) and shoot the barrel in the way, then carry on.
    pub(super) fn shoot_barrel(&mut self, obs: &Observation) -> Controls {
        if let Some(until) = self.barrel_flee {
            if obs.tic < until && !self.barrel_flee_way.is_empty() {
                self.progress_tic = obs.tic;
                let pos = obs.pos();
                while self.barrel_flee_way.len() > 1 && dist(pos, self.barrel_flee_way[0]) < 16.0 {
                    self.barrel_flee_way.remove(0);
                }
                let h = heading(pos, self.barrel_flee_way[0]);
                return self.motion.go(obs, h, FULL_SPEED, Some(h));
            }
            self.barrel_flee = None;
            self.barrel_settle = Some((obs.barrels.as_ref().map_or(0, Vec::len), obs.tic));
        }
        if let Some((n, since)) = self.barrel_settle {
            // (the rest of the chain going off, a blast a while after the last: MAP23's field,
            // gone back into as it went on, five times)
            let now = obs.barrels.as_ref().map_or(0, Vec::len);
            if now != n {
                self.barrel_settle = Some((now, obs.tic));
            }
            if obs.tic - self.barrel_settle.map_or(since, |x| x.1) < BARREL_SETTLE {
                // (on away from the nearest barrel still there, while they go off; shot from out
                // of every blast, where she is, as planned: away from one there would be towards
                // another, E1M5's, back across the hall into the one going off)
                self.progress_tic = obs.tic;
                if self.barrel_stay {
                    return self.motion.hold(obs, (0.0, 0.0), None);
                }
                let pos = obs.pos();
                if let Some(b) = obs
                    .barrels
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|&b| blast_gap(pos, b) < BARREL_SAFE * 1.5)
                    .min_by(|a, b| blast_gap(*a, pos).total_cmp(&blast_gap(*b, pos)))
                {
                    let h = heading(b, pos);
                    return self.motion.go(obs, h, FULL_SPEED, Some(h));
                }
                return self.motion.hold(obs, (0.0, 0.0), None);
            }
            self.barrel_settle = None;
        }
        let Some((was, since)) = self.barrel else { return self.walk(obs) };
        let pos = obs.pos();
        // (mortal: time to walk off to somewhere outside the blasts first)
        let give_up = if self.mortal { BARREL_GIVE_UP_FAR } else { BARREL_GIVE_UP };
        // (the same barrel, pushed by the shots at it: E1M1's, a pistol tap moving it 8 units,
        // taken for gone, shot at again as another, 16 times)
        let now = barrel_now(obs, was);
        let at = now.unwrap_or(was);
        self.barrel = Some((at, since));
        if now.is_none() || obs.tic - since > give_up {
            self.barrel = None;
            self.barrel_post = None;
            self.barrel_way.clear();
            self.barrel_best = None;
            self.reset_progress(obs.tic);
            self.log.add(obs.tic, if obs.tic - since <= give_up { "barrel gone" } else { "couldn't shoot the barrel" });
            return self.walk(obs);
        }
        self.progress_tic = obs.tic;
        let look = heading(pos, at);
        // (nothing to shoot it with: not stood before it for the 20 s all the same, the trigger
        // pulled on nothing; MAP23, her rounds gone on lost souls, a barrel before the switch,
        // seven times over. Able to be hurt, the step fails, and what she's out of is what's
        // gone for; not able to be, she walks up and punches it. The user's)
        if ![2, 3, 4].into_iter().any(|slot| tap_rounds(obs, slot) > 0) {
            if self.mortal {
                self.barrel = None;
                self.barrel_post = None;
                self.barrel_way.clear();
                self.barrel_best = None;
                return self.fail(obs, "a barrel in the way, and nothing to shoot it with");
            }
            /// (the fist's reach is 64 from her middle to its; a little inside it)
            const PUNCH_FROM: f64 = 56.0;
            if dist(pos, at) > PUNCH_FROM {
                return self.motion.go(obs, look, FULL_SPEED, Some(look));
            }
            let mut c = self.motion.hold(obs, (0.0, 0.0), Some(look));
            if obs.weapon != Some(1) {
                c.weapon = 1;
            } else if norm(look - obs.angle).abs() < 10.0 {
                c.attack = true;
            }
            return c;
        }
        // (mortal: outside the blast of every barrel it sets off in turn, however long backing off
        // takes; E1M4's, shot from 70 away with a wall behind her, killed her. The user's: a
        // barrel blown up sets off those beside it)
        if self.mortal {
            // (somewhere to shoot it from that she can see it from, and a way off from there she
            // can run along that keeps her out of each blast of the chain as it comes: back the way
            // she came to it, then back along her route; the user's: the chain takes time, she
            // runs as it goes. E3M4's, shot from 31 away, the run off it into a wall)
            let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
            if self.barrel_post.is_none() {
                let track = &self.segments[self.seg_i].track;
                let mut behind: Vec<Point> = Vec::new();
                let mut back = self.s;
                while back > 0.0 && behind.len() < BACKTRACK_POINTS {
                    behind.push(track.point_at(back));
                    back -= BACKTRACK_SPACING;
                }
                let mut cands: Vec<Point> = behind.clone();
                let mesh = &self.view.nav.mesh;
                for pi in 0..mesh.polys.len() {
                    let Some(poly) = mesh.polys[pi].as_ref() else { continue };
                    if dist(poly.center, at) < BARREL_LOOK {
                        cands.push(poly.center);
                    }
                }
                let states: States = obs.heights.iter().map(|h| vec![*h]).collect();
                // (the barrel in the way, or any it sets off: shot from where one of those can be
                // seen, the chain takes the rest; MAP02's corridor of them, the one in the way round
                // a corner from anywhere clear of the others)
                let group: Vec<Point> = self.barrel_chain(&barrels, at, at).into_iter().map(|(b, _)| b).collect();
                // (not on a floor that hurts: she stays where she shoots from till the chain's done;
                // E1M3, a post on nukage, the barrel not to be had from there, dead standing on it)
                let nav = &self.view.nav;
                cands.retain(|&q| nav.poly_at(q).is_none_or(|p| nav.damage.get(nav.sector_of_poly(p)).is_none_or(|&d| d == 0)));
                // (and from where a shot would get to it, heights and all: a ledge, a step, a low
                // ceiling between take the shot as surely as a wall; the user's, posts chosen the
                // barrel couldn't be hit from, the shots into the floor or a lip)
                let heights: Vec<(f64, f64)> = obs.heights.iter().map(|h| (h.floor, h.ceiling)).collect();
                let floor_at = |p: Point| nav.poly_at(p).map(|i| obs.heights[nav.sector_of_poly(i)].floor);
                let shot = |q: Point, b: Point| match (floor_at(q), floor_at(b)) {
                    (Some(fq), Some(fb)) => nav.sight().clear(
                        crate::los::Body { x: q.0, y: q.1, z: fq, height: crate::navigator::PLAYER_HEIGHT },
                        crate::los::Body { x: b.0, y: b.1, z: fb, height: BARREL_HEIGHT },
                        &heights,
                    ),
                    _ => false,
                };
                let mut seen: Vec<(Point, Point)> = cands
                    .into_iter()
                    .filter_map(|q| group.iter().copied().filter(|&b| !self.use_walled(q, b) && shot(q, b)).min_by(|a, b| dist(*a, q).total_cmp(&dist(*b, q))).map(|b| (q, b)))
                    .collect();
                seen.sort_by(|a, b| dist(a.0, pos).total_cmp(&dist(b.0, pos)));
                let mut best: Option<Post> = None;
                let crushers = self.crushers();
                for (q, aim) in seen.into_iter().take(BARREL_POSTS * POST_CANDIDATE_FACTOR) {
                    let way: Vec<Point> = if dist(q, pos) < 8.0 {
                        vec![pos]
                    } else {
                        let Some(path) = self.view.find_path(pos, q, &states, None) else { continue };
                        path.waypoints.iter().flatten().copied().collect()
                    };
                    // (not a way past a barrel: E4M5's, a spot found beyond it, walked into it; nor
                    // one under a crusher: E3M4's, gone under the one she'd just set off)
                    if way.windows(2).any(|w| barrels.iter().any(|&b| segment_distance(w[0], w[1], b, b) < PLAYER_RADIUS + BARREL_RADIUS)) {
                        continue;
                    }
                    if way.iter().any(|&w| mesh.poly_at(w).is_some_and(|p| crushers.contains(&mesh.poly(p).sector))) {
                        continue;
                    }
                    let flee: Vec<Point> = way.iter().rev().copied().chain(behind.iter().copied()).collect();
                    let Some(stay) = self.flee_safe(&barrels, aim, q, &flee) else { continue };
                    // (out of every blast where she shoots from: she stays there; the way back she
                    // came may go back by the barrel, E1M5's, 27 off it when the post was chosen,
                    // run back past it as it went off)
                    let flee = if stay { Vec::new() } else { flee };
                    let length: f64 = way.windows(2).map(|w| dist(w[0], w[1])).sum();
                    if best.as_ref().is_none_or(|b| length < b.length) {
                        best = Some(Post { length, q, aim, way, flee, stay });
                    }
                }
                if let Some(Post { q, aim, way, flee, stay, .. }) = best {
                    let then = if stay { "staying there, out of every blast" } else { "then off back the way she came" };
                    self.log.add(obs.tic, format!("shooting it from ({:.0}, {:.0}), {then}", q.0, q.1));
                    self.barrel_stay = stay;
                    if dist(aim, at) > 1.0 {
                        // (another of them, the chain to take it)
                        self.log.add(obs.tic, format!("(at the one at ({:.0}, {:.0}), that sets it off)", aim.0, aim.1));
                        self.barrel = self.barrel.map(|(_, since)| (aim, since));
                    }
                    self.barrel_post = Some(q);
                    self.barrel_way = way;
                    self.barrel_flee_way = flee;
                }
            }
            let Some(post) = self.barrel_post else {
                self.barrel = None;
                return self.fail(obs, "a barrel in the way, nowhere to shoot it from and run off out of the blasts");
            };
            if dist(pos, post) > 8.0 {
                // (getting no nearer for a while: give it up)
                let d = dist(pos, post);
                match self.barrel_best {
                    Some((best, _)) if d < best - 2.0 => self.barrel_best = Some((d, obs.tic)),
                    Some((_, at_tic)) if obs.tic - at_tic > BARREL_STALL => {
                        self.barrel = None;
                        self.barrel_post = None;
                        self.barrel_way.clear();
                        self.barrel_best = None;
                        return self.fail(obs, "a barrel in the way, and no getting to somewhere to shoot it from");
                    }
                    None => self.barrel_best = Some((d, obs.tic)),
                    _ => {}
                }
                while self.barrel_way.len() > 1 && dist(pos, self.barrel_way[0]) < 12.0 {
                    self.barrel_way.remove(0);
                }
                let to = self.barrel_way.first().copied().unwrap_or(post);
                let h = heading(pos, to);
                let v = 12.0f64.min(0.3 * dist(pos, post) + 2.0);
                return self.motion.go(obs, h, v, Some(h));
            }
        } else if dist(pos, at) < BARREL_SAFE && obs.tic - since < 3 * 35 {
            let away = (pos.1 - at.1).atan2(pos.0 - at.0);
            return self.motion.hold(obs, (away.cos() * 8.0, away.sin() * 8.0), Some(look));
        }
        let mut c = self.motion.hold(obs, (0.0, 0.0), Some(look));
        // (single taps, the trigger let go between: the first shot of a burst goes straight,
        // only held ones spread)
        let gun = barrel_gun(obs, dist(pos, at));
        if obs.weapon != Some(gun) {
            c.weapon = gun;
        } else if norm(look - obs.angle).abs() < 2.0 && obs.tic - self.barrel_shot >= BARREL_TAP {
            c.attack = true;
            self.barrel_shot = obs.tic;
            if self.mortal {
                // (and off, the way worked out)
                self.barrel_flee = Some(obs.tic + BARREL_FLEE_TICS);
            }
        }
        c
    }

    /// The barrels shooting the one at `at` from `pos` sets off, in turn: a shot hits the first
    /// barrel in its way, not the one aimed at, and each blast sets off those beside it that it
    /// can see (MAP23's rows, through walls, were nearly all of its 208).
    fn barrel_chain(&self, barrels: &[Point], at: Point, pos: Point) -> Vec<(Point, u32)> {
        // (each with how many blasts in turn it goes off after: one goes off a while after it's
        // hit, so the chain takes time to come her way)
        let mut chain: Vec<(Point, u32)> =
            std::iter::once(at).chain(barrels.iter().copied().filter(|&b| segment_distance(pos, at, b, b) < BARREL_RADIUS + 2.0)).map(|b| (b, 0)).collect();
        let mut todo: std::collections::VecDeque<(Point, u32)> = chain.iter().copied().collect();
        while let Some((b, hops)) = todo.pop_front() {
            for &c in barrels {
                if blast_gap(b, c) < BARREL_CHAIN && !chain.iter().any(|&(x, _)| dist(x, c) < 1.0) && !self.use_walled(b, c) {
                    chain.push((c, hops + 1));
                    todo.push_back((c, hops + 1));
                }
            }
        }
        chain
    }

    /// Shot from `q`: out of every blast of the chain there already (Some(true): she stays), or
    /// off along `flee` at her pace counted on, out of each blast when it comes (Some(false)), or
    /// neither (None). A blast reaches only what it can see.
    fn flee_safe(&self, barrels: &[Point], at: Point, q: Point, flee: &[Point]) -> Option<bool> {
        let chain = self.barrel_chain(barrels, at, q);
        // (shot from somewhere already out of every blast of it: no running needed; MAP02's
        // barrels before the way to the exit, none of the spots with a run off safe, given up on
        // for the hour)
        if chain.iter().all(|&(b, _)| self.use_walled(b, q) || blast_gap(q, b) >= BARREL_SAFE) {
            return Some(true);
        }
        chain
            .iter()
            .all(|&(b, hops)| {
                let p = along(flee, BARREL_FLEE_PACE * BARREL_DELAY * f64::from(hops + 1));
                self.use_walled(b, p) || blast_gap(p, b) >= BARREL_SAFE
            })
            .then_some(false)
    }
}
