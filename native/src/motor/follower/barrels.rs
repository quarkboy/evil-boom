//! Clearing barrels, choosing a firing position and escaping chain explosions.

use super::Trouble;
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

/// a shot leaves from this far up her: half her height and 8 (P_AimLineAttack's shootz)
const GUN_HEIGHT: f64 = 36.0;

/// a tap's shot has landed, or hasn't, this many tics after the trigger (it leaves the gun 3 or
/// 4 tics after, and a barrel hit is seen lit the tic after that)
const TAP_SEEN: i64 = 8;

/// (mortal) shot, she runs off this long...
const BARREL_FLEE_TICS: i64 = 2 * 35;

/// ...at least this fast, counted on (units a tic: well under running, for the turn first)
const BARREL_FLEE_PACE: f64 = 8.0;

/// (mortal) somewhere to shoot a barrel from, looked for this near it...
const BARREL_LOOK: f64 = 1024.0;

/// ...the nearest this many of them, by the way there
const BARREL_POSTS: usize = 12;

/// a spot this near one she had no shot from isn't chosen again for that barrel
const FAILED_POST_NEAR: f64 = 16.0;
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
    chain
        .into_iter()
        .filter(|&b| blast_gap(p, b) < BARREL_SAFE)
        .min_by(|a, b| blast_gap(*a, p).total_cmp(&blast_gap(*b, p)))
}

impl RouteFollower {
    /// Barrels going off that she didn't set off (a monster's shot, a blast): off out of their
    /// chain's blasts if they reach her, else no further on while they'd reach where she'll be
    /// as they go (walking on after a chain going off ahead of her is walking into it). The
    /// chain burns out a barrel a blast (15 tics) at a time: then on.
    pub(super) fn keep_off_lit_chain(&mut self, obs: &Observation) -> Option<Controls> {
        let pos = obs.pos();
        let v = obs.velocity.unwrap_or((0.0, 0.0));
        let by = lit_chain_by(obs, pos);
        let ahead = by.is_none()
            && (1..=4).any(|k| {
                let f = BARREL_DELAY * f64::from(k) / 4.0;
                lit_chain_by(obs, (pos.0 + v.0 * f, pos.1 + v.1 * f)).is_some()
            });
        if crate::debug::on().barrels && !obs.barrels_lit.is_empty() {
            self.log.add(obs.tic, format!("lit {:?}; she's at ({:.0}, {:.0}) going ({:.1}, {:.1}); in the blast of the chain's {by:?}, ahead {ahead}; health {:?}", obs.barrels_lit, pos.0, pos.1, v.0, v.1, obs.health));
        }
        if by.is_none() {
            self.chain.flee.clear();
        }
        if by.is_none() && !ahead {
            self.chain.waiting = false;
            return None;
        }
        if !self.chain.waiting {
            self.log.add(
                obs.tic,
                "barrels going off ahead: keeping out of their blasts till they're done",
            );
            self.chain.waiting = true;
        }
        Some(match by {
            Some(b) => {
                // (by a way out of all of them, kept to till she's out; none found, looked for
                // again a blast on)
                if self.chain.flee.is_empty()
                    && obs.tic - self.chain.flee_tic >= BARREL_DELAY as i64
                {
                    self.chain.flee = self.way_off_chain(obs);
                    self.chain.flee_tic = obs.tic;
                    if let Some(&to) = self.chain.flee.last() {
                        self.log.add(
                            obs.tic,
                            format!("off to ({:.0}, {:.0}), out of their blasts", to.0, to.1),
                        );
                    }
                }
                while self.chain.flee.len() > 1 && dist(pos, self.chain.flee[0]) < 16.0 {
                    self.chain.flee.remove(0);
                }
                // (no way found: on along her route, running, not stopped among them; with no
                // route, straight away from the nearest of them)
                let Some(&to) = self.chain.flee.first() else {
                    if self.planned_path(None).is_empty() {
                        self.progress_tic = obs.tic;
                        let h = heading(b, pos);
                        return Some(self.motion.go(obs, h, FULL_SPEED, Some(h)));
                    }
                    // (the route's own walking, and its own count of getting nowhere: wedged
                    // against a barrel before a door, it's pressed from there)
                    return None;
                };
                self.progress_tic = obs.tic;
                let h = heading(pos, to);
                self.motion.go(obs, h, FULL_SPEED, Some(h))
            }
            None => {
                self.progress_tic = obs.tic;
                self.motion.hold(obs, (0.0, 0.0), None)
            }
        })
    }

    /// In the blasts of a chain of barrels going off: the way to run, to the nearest place out
    /// of every barrel's blast. The shortest way to one of the nearest such places; empty when
    /// there's none, or her route's out of them as soon or nearly: then it's on along the route.
    /// (Not straight away from the nearest of them: between two, away from one is towards the
    /// other, to and fro on the spot as the chain comes.)
    fn way_off_chain(&self, obs: &Observation) -> Vec<Point> {
        /// the nearest this many places out of the blasts tried for a way to them
        const PLACES: usize = 24;
        /// the route's taken when it's out of them within this much of the shortest way out
        const ROUTE_WITHIN: f64 = 1.5;
        let pos = obs.pos();
        // (the ones going off and every one they set off in turn)
        let all: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        let mut barrels: Vec<Point> = obs.barrels_lit.clone();
        let mut i = 0;
        while i < barrels.len() {
            let c = barrels[i];
            for &x in &all {
                if blast_gap(c, x) < BARREL_CHAIN
                    && !barrels.iter().any(|&y| dist(x, y) < 1.0)
                    && !self.use_walled(c, x)
                {
                    barrels.push(x);
                }
            }
            i += 1;
        }
        let clear = |p: Point| {
            barrels
                .iter()
                .all(|&b| self.use_walled(b, p) || blast_gap(p, b) >= BARREL_SAFE)
        };
        // (how far along a way till it's out of every blast, and stays out the next stretch)
        let out_at = |way: &[Point]| {
            let length: f64 = way.windows(2).map(|w| dist(w[0], w[1])).sum();
            let mut d = 0.0;
            while d <= length {
                if clear(along(way, d)) && clear(along(way, (d + 64.0).min(length))) {
                    return Some(d);
                }
                d += 16.0;
            }
            None
        };
        let route: Vec<Point> = std::iter::once(pos)
            .chain(self.planned_path(None).into_iter().map_while(|p| p))
            .collect();
        let by_route = if route.len() > 1 {
            out_at(&route)
        } else {
            None
        };
        let nav = &self.view.nav;
        let mesh = &nav.mesh;
        let mut places: Vec<Point> = (0..mesh.polys.len())
            .filter_map(|pi| mesh.polys[pi].as_ref().map(|poly| (pi, poly.center)))
            .filter(|&(pi, q)| {
                nav.damage
                    .get(nav.sector_of_poly(pi))
                    .is_none_or(|&d| d == 0)
                    && clear(q)
            })
            .map(|(_, q)| q)
            .collect();
        places.sort_by(|a, b| dist(*a, pos).total_cmp(&dist(*b, pos)));
        let states: States = obs.heights.iter().map(|h| vec![*h]).collect();
        let mut best: Option<(f64, Vec<Point>)> = None;
        for q in places.into_iter().take(PLACES) {
            if best.as_ref().is_some_and(|b| dist(q, pos) >= b.0) {
                break; // (no shorter way to anywhere further off)
            }
            let Some(path) = self.view.find_path(pos, q, &states, None) else {
                continue;
            };
            if !path.jumps.is_empty() {
                continue;
            }
            let way: Vec<Point> = std::iter::once(pos)
                .chain(path.waypoints.iter().flatten().copied())
                .collect();
            let Some(d) = out_at(&way) else { continue };
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, way));
            }
        }
        if crate::debug::on().barrels {
            eprintln!("tic {}: at ({:.0}, {:.0}), {} barrels: out of them by the route in {by_route:?}, by the shortest way in {:?} to {:?}", obs.tic, pos.0, pos.1, barrels.len(), best.as_ref().map(|b| b.0), best.as_ref().and_then(|b| b.1.last()));
        }
        // (the route itself: nothing for her to follow but it, its presses and all; a door at
        // the far end walked to as a way out is stood before, never pressed)
        match (by_route, best) {
            (Some(r), Some((d, _))) if r <= d * ROUTE_WITHIN => Vec::new(),
            (_, Some((_, way))) => way[1..].to_vec(),
            _ => Vec::new(),
        }
    }

    /// Back out of its blast (128 units) and shoot the barrel in the way, then carry on.
    pub(super) fn shoot_barrel(&mut self, obs: &Observation) -> Controls {
        // (a tap that set nothing off: the run off and the wait for the chain to burn out are for
        // barrels going off. Once the shot's had time to land with no barrel lit and none gone,
        // she's back to tapping, at the pistol's own pace, not a run and a wait a tap)
        if let Some((n, at)) = self.shot.tapped {
            let going_off =
                !obs.barrels_lit.is_empty() || obs.barrels.as_ref().map_or(0, Vec::len) != n;
            if going_off {
                self.shot.tapped = None;
            } else if obs.tic - at >= TAP_SEEN {
                self.shot.tapped = None;
                self.shot.flee_until = None;
                self.shot.settle = None;
            }
        }
        if let Some(until) = self.shot.flee_until {
            if obs.tic < until && !self.shot.flee_way.is_empty() {
                self.progress_tic = obs.tic;
                let pos = obs.pos();
                while self.shot.flee_way.len() > 1 && dist(pos, self.shot.flee_way[0]) < 16.0 {
                    self.shot.flee_way.remove(0);
                }
                let h = heading(pos, self.shot.flee_way[0]);
                return self.motion.go(obs, h, FULL_SPEED, Some(h));
            }
            self.shot.flee_until = None;
            self.shot.settle = Some((obs.barrels.as_ref().map_or(0, Vec::len), obs.tic));
        }
        if let Some((n, since)) = self.shot.settle {
            // (the rest of the chain going off, a blast a while after the last: not gone back into
            // as it goes on)
            let now = obs.barrels.as_ref().map_or(0, Vec::len);
            if now != n {
                self.shot.settle = Some((now, obs.tic));
            }
            if obs.tic - self.shot.settle.map_or(since, |x| x.1) < BARREL_SETTLE {
                // (on away from the nearest barrel still there, while they go off; shot from out
                // of every blast, where she is, as planned: away from one there would be towards
                // another, maybe the one going off)
                self.progress_tic = obs.tic;
                if self.shot.stay {
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
            self.shot.settle = None;
        }
        let Some((was, since)) = self.shot.target else {
            return self.walk(obs);
        };
        let pos = obs.pos();
        // (mortal: time to walk off to somewhere outside the blasts first)
        let give_up = if self.mortal {
            BARREL_GIVE_UP_FAR
        } else {
            BARREL_GIVE_UP
        };
        // (the same barrel, pushed by the shots at it: a pistol tap moves it some 8 units, and
        // it isn't gone, nor another to shoot afresh)
        let now = barrel_now(obs, was);
        let at = now.unwrap_or(was);
        self.shot.target = Some((at, since));
        if now.is_none() || obs.tic - since > give_up {
            self.shot.failed_posts.clear();
            self.shot.target = None;
            self.shot.post = None;
            self.shot.way.clear();
            self.shot.best = None;
            self.reset_progress(obs.tic);
            self.log.add(
                obs.tic,
                if obs.tic - since <= give_up {
                    "barrel gone"
                } else {
                    "couldn't shoot the barrel"
                },
            );
            return self.walk(obs);
        }
        self.progress_tic = obs.tic;
        let look = heading(pos, at);
        // (nothing to shoot it with: not stood before it all the same, the trigger pulled on
        // nothing. Able to be hurt, the step fails, and what she's out of is what's gone for;
        // not able to be, she walks up and punches it)
        if ![2, 3, 4].into_iter().any(|slot| tap_rounds(obs, slot) > 0) {
            if self.mortal {
                self.shot.target = None;
                self.shot.post = None;
                self.shot.way.clear();
                self.shot.best = None;
                return self.fail(obs, Trouble::BarrelNoGun);
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
        // takes; shot from 70 away with a wall behind her, it kills her. A barrel blown up sets
        // off those beside it)
        if self.mortal {
            // (somewhere to shoot it from that she can see it from, and a way off from there she
            // can run along that keeps her out of each blast of the chain as it comes: back the way
            // she came to it, then back along her route; the chain takes time, she runs as it goes,
            // and the run mustn't end at a wall)
            let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
            if self.shot.post.is_none() {
                let track = &self.segments[self.seg_i].track;
                let mut behind: Vec<Point> = Vec::new();
                let mut back = self.at.along;
                while back > 0.0 && behind.len() < BACKTRACK_POINTS {
                    behind.push(track.point_at(back));
                    back -= BACKTRACK_SPACING;
                }
                let mut cands: Vec<Point> = behind.clone();
                let mesh = &self.view.nav.mesh;
                for pi in 0..mesh.polys.len() {
                    let Some(poly) = mesh.polys[pi].as_ref() else {
                        continue;
                    };
                    if dist(poly.center, at) < BARREL_LOOK {
                        cands.push(poly.center);
                    }
                }
                let states: States = obs.heights.iter().map(|h| vec![*h]).collect();
                // (the barrel in the way, or any it sets off: shot from where one of those can be
                // seen, the chain takes the rest; in a corridor of them the one in the way can be round
                // a corner from anywhere clear of the others)
                let group: Vec<Point> = self
                    .barrel_chain(&barrels, at, at)
                    .into_iter()
                    .map(|(b, _)| b)
                    .collect();
                // (not where she's had no shot from already; nor on a floor that hurts: she stays
                // where she shoots from till the chain's done)
                let failed = &self.shot.failed_posts;
                cands.retain(|&q| failed.iter().all(|&f| dist(f, q) > FAILED_POST_NEAR));
                let nav = &self.view.nav;
                cands.retain(|&q| {
                    nav.poly_at(q).is_none_or(|p| {
                        nav.damage
                            .get(nav.sector_of_poly(p))
                            .is_none_or(|&d| d == 0)
                    })
                });
                // (and from where a shot would get to it, heights and all: a ledge, a step, a low
                // ceiling between take the shot as surely as a wall, into the floor or a lip)
                let mut seen: Vec<(Point, Point)> = cands
                    .into_iter()
                    .filter_map(|q| {
                        group
                            .iter()
                            .copied()
                            .filter(|&b| {
                                !self.use_walled(q, b) && self.shot_reaches(obs, q, b, false)
                            })
                            .min_by(|a, b| dist(*a, q).total_cmp(&dist(*b, q)))
                            .map(|b| (q, b))
                    })
                    .collect();
                seen.sort_by(|a, b| dist(a.0, pos).total_cmp(&dist(b.0, pos)));
                let mut best: Option<Post> = None;
                let crushers = self.crushers();
                for (q, aim) in seen.into_iter().take(BARREL_POSTS * POST_CANDIDATE_FACTOR) {
                    let way: Vec<Point> = if dist(q, pos) < 8.0 {
                        vec![pos]
                    } else {
                        let Some(path) = self.view.find_path(pos, q, &states, None) else {
                            continue;
                        };
                        path.waypoints.iter().flatten().copied().collect()
                    };
                    // (not a way past a barrel, which would walk her into it; nor one under a crusher)
                    if way.windows(2).any(|w| {
                        barrels.iter().any(|&b| {
                            segment_distance(w[0], w[1], b, b) < PLAYER_RADIUS + BARREL_RADIUS
                        })
                    }) {
                        continue;
                    }
                    if way.iter().any(|&w| {
                        mesh.poly_at(w)
                            .is_some_and(|p| crushers.contains(&mesh.poly(p).sector))
                    }) {
                        continue;
                    }
                    let flee: Vec<Point> = way
                        .iter()
                        .rev()
                        .copied()
                        .chain(behind.iter().copied())
                        .collect();
                    let Some(stay) = self.flee_safe(&barrels, aim, q, &flee) else {
                        continue;
                    };
                    // (a monster about that shoots: from out of every blast or not at all; shot
                    // from beside the barrel to run after, its shot can set it off first)
                    if self.shooters_about && !stay {
                        continue;
                    }
                    // (out of every blast where she shoots from: she stays there; the way back she
                    // came may go back by the barrel as it goes off)
                    let flee = if stay { Vec::new() } else { flee };
                    let length: f64 = way.windows(2).map(|w| dist(w[0], w[1])).sum();
                    if best.as_ref().is_none_or(|b| length < b.length) {
                        best = Some(Post {
                            length,
                            q,
                            aim,
                            way,
                            flee,
                            stay,
                        });
                    }
                }
                if let Some(Post {
                    q,
                    aim,
                    way,
                    flee,
                    stay,
                    ..
                }) = best
                {
                    let then = if stay {
                        "staying there, out of every blast"
                    } else {
                        "then off back the way she came"
                    };
                    self.log.add(
                        obs.tic,
                        format!("shooting it from ({:.0}, {:.0}), {then}", q.0, q.1),
                    );
                    self.shot.stay = stay;
                    if dist(aim, at) > 1.0 {
                        // (another of them, the chain to take it)
                        self.log.add(
                            obs.tic,
                            format!(
                                "(at the one at ({:.0}, {:.0}), that sets it off)",
                                aim.0, aim.1
                            ),
                        );
                        self.shot.target = self.shot.target.map(|(_, since)| (aim, since));
                    }
                    self.shot.post = Some(q);
                    self.shot.way = way;
                    self.shot.flee_way = flee;
                }
            }
            let Some(post) = self.shot.post else {
                self.shot.target = None;
                return self.fail(obs, Trouble::BarrelNoPost);
            };
            if dist(pos, post) > 8.0 {
                // (getting no nearer for a while: give it up)
                let d = dist(pos, post);
                match self.shot.best {
                    Some((best, _)) if d < best - 2.0 => self.shot.best = Some((d, obs.tic)),
                    Some((_, at_tic)) if obs.tic - at_tic > BARREL_STALL => {
                        self.shot.target = None;
                        self.shot.post = None;
                        self.shot.way.clear();
                        self.shot.best = None;
                        return self.fail(obs, Trouble::BarrelNoReach);
                    }
                    None => self.shot.best = Some((d, obs.tic)),
                    _ => {}
                }
                while self.shot.way.len() > 1 && dist(pos, self.shot.way[0]) < 12.0 {
                    self.shot.way.remove(0);
                }
                let to = self.shot.way.first().copied().unwrap_or(post);
                let h = heading(pos, to);
                let v = 12.0f64.min(0.3 * dist(pos, post) + 2.0);
                return self.motion.go(obs, h, v, Some(h));
            }
        } else if dist(pos, at) < BARREL_SAFE && obs.tic - since < 3 * 35 {
            let away = (pos.1 - at.1).atan2(pos.0 - at.0);
            return self
                .motion
                .hold(obs, (away.cos() * 8.0, away.sin() * 8.0), Some(look));
        }
        let mut c = self.motion.hold(obs, (0.0, 0.0), Some(look));
        // (single taps, the trigger let go between: the first shot of a burst goes straight,
        // only held ones spread)
        // (the shotgun only with all of the barrel in the gun's view: its pellets spread up and
        // down too, and at a barrel mostly behind a lip nearly all of a blast goes into the lip)
        let gun = match barrel_gun(obs, dist(pos, at)) {
            3 if !self.shot_reaches(obs, pos, at, true) => super::tap_gun(obs),
            gun => gun,
        };
        if obs.weapon != Some(gun) {
            c.weapon = gun;
        } else if norm(look - obs.angle).abs() < 2.0 && obs.tic - self.shot.last_shot >= BARREL_TAP
        {
            if let Some(post) = self
                .shot
                .post
                .filter(|_| self.mortal && !self.shot_reaches(obs, pos, at, false))
            {
                // (no shot from where she stands: a step short of the spot the shot was found
                // from, right onto it; on it, the heights have changed since it was chosen (the
                // floor she's on, or one between), and it's somewhere else to shoot from. Not
                // the trigger pulled on a lip till the step's time is up)
                if dist(pos, post) > 1.5 {
                    return self
                        .motion
                        .hold(obs, (post.0 - pos.0, post.1 - pos.1), Some(look));
                }
                self.log.add(
                    obs.tic,
                    "no shot at the barrel from here now: somewhere else to shoot it from",
                );
                self.shot.failed_posts.push(post);
                self.shot.post = None;
                self.shot.way.clear();
                self.shot.best = None;
                return c;
            }
            c.attack = true;
            self.shot.last_shot = obs.tic;
            self.shot.tapped = Some((obs.barrels.as_ref().map_or(0, Vec::len), obs.tic));
            if crate::debug::on().barrels {
                eprintln!("tic {}: tap at the barrel at ({:.0}, {:.0}) from ({:.0}, {:.0}), {:.0} off, gun {gun}, ammo {:?}", obs.tic, at.0, at.1, pos.0, pos.1, dist(pos, at), obs.ammo);
            }
            if self.mortal {
                // (and off, the way worked out)
                self.shot.flee_until = Some(obs.tic + BARREL_FLEE_TICS);
            }
        }
        c
    }

    /// Would a shot from `q` get to the barrel at `b`, the heights as they are: a ledge, a step,
    /// a low ceiling between take the shot as surely as a wall. By the gun's own line: a shot
    /// leaves from `GUN_HEIGHT` up her (P_AimLineAttack's shootz), lower than her eyes, and what
    /// she can just see over a lip she can't always hit. `whole`: all of the barrel in the gun's
    /// view, top to bottom (a gun that spreads wants that: aimed at a sliver, most of it goes
    /// into the lip); else any of it (a bullet goes where it's aimed).
    fn shot_reaches(&self, obs: &Observation, q: Point, b: Point, whole: bool) -> bool {
        let nav = &self.view.nav;
        let heights: Vec<(f64, f64)> = obs.heights.iter().map(|h| (h.floor, h.ceiling)).collect();
        let floor_at = |p: Point| {
            nav.poly_at(p)
                .map(|i| obs.heights[nav.sector_of_poly(i)].floor)
        };
        let (Some(fq), Some(fb)) = (floor_at(q), floor_at(b)) else {
            return false;
        };
        let gun = crate::los::Body {
            x: q.0,
            y: q.1,
            z: fq + GUN_HEIGHT,
            height: 0.0,
        };
        let part = |z: f64, height: f64| {
            nav.sight().clear(
                gun,
                crate::los::Body {
                    x: b.0,
                    y: b.1,
                    z,
                    height,
                },
                &heights,
            )
        };
        if whole {
            part(fb, 2.0) && part(fb + BARREL_HEIGHT - 2.0, 2.0)
        } else {
            part(fb, BARREL_HEIGHT)
        }
    }

    /// The barrels shooting the one at `at` from `pos` sets off, in turn: a shot hits the first
    /// barrel in its way, not the one aimed at, and each blast sets off those beside it that it
    /// can see (rows of them, through walls, can be nearly all a level has).
    fn barrel_chain(&self, barrels: &[Point], at: Point, pos: Point) -> Vec<(Point, u32)> {
        // (each with how many blasts in turn it goes off after: one goes off a while after it's
        // hit, so the chain takes time to come her way)
        let mut chain: Vec<(Point, u32)> = std::iter::once(at)
            .chain(
                barrels
                    .iter()
                    .copied()
                    .filter(|&b| segment_distance(pos, at, b, b) < BARREL_RADIUS + 2.0),
            )
            .map(|b| (b, 0))
            .collect();
        let mut todo: std::collections::VecDeque<(Point, u32)> = chain.iter().copied().collect();
        while let Some((b, hops)) = todo.pop_front() {
            for &c in barrels {
                if blast_gap(b, c) < BARREL_CHAIN
                    && !chain.iter().any(|&(x, _)| dist(x, c) < 1.0)
                    && !self.use_walled(b, c)
                {
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
        // (shot from somewhere already out of every blast of it: no running needed, where none
        // of the spots has a safe run off)
        if chain
            .iter()
            .all(|&(b, _)| self.use_walled(b, q) || blast_gap(q, b) >= BARREL_SAFE)
        {
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
