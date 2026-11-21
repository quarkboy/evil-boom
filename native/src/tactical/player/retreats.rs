// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Her retreats: when one's due, which kind, where to, and when it's over (`Retreat`,
//! `RetreatKind`): falling back to cover, backing out of an ambush, out of the barrels,
//! baiting them round a corner.

use super::*;

impl Player {
    /// Is she falling back? A retreat under way goes on till it's there, it fails, or its own
    /// end comes; else, not too soon after the last, one may start.
    pub(super) fn retreating(&mut self, obs: &Observation) -> bool {
        if !self.fight.combat.now.shooters.is_empty() {
            self.shooters_tic = obs.tic;
        }
        // In among barrels with something about to set them off: out of them before anything else,
        // whatever she was at, another retreat included.
        if self
            .retreat
            .as_ref()
            .is_none_or(|r| r.kind != RetreatKind::OutOfBarrels)
            && obs.tic >= self.barrel_run_again
            && self.barrel_run_due(obs)
        {
            match self.out_of_barrels(obs) {
                Some(route) => {
                    self.barrel_runs.push(obs.pos());
                    self.retreat = Some(Retreat {
                        kind: RetreatKind::OutOfBarrels,
                        route,
                    });
                    self.retreat_tic = obs.tic;
                }
                None => self.barrel_run_again = obs.tic + 15,
            }
        }
        if let Some(r) = &self.retreat {
            let walled = r.kind == RetreatKind::ForAmmo && self.walled_off(obs, &r.route);
            if walled {
                self.walled_tic = obs.tic;
            }
            if r.route.done || r.route.failed || self.retreat_over(obs, r.kind) || walled {
                let msg = if r.route.done {
                    "got there".to_string()
                } else {
                    format!("{} over", r.kind.name())
                };
                let kind = r.kind;
                self.log(format!("tic {}: {msg}", obs.tic));
                if kind == RetreatKind::FallBack {
                    self.wait_until = obs.tic + self.fight.tune.retreat.fall_back_wait;
                }
                self.retreat = None;
                self.fight.combat.baiting = false;
                if let Some(f) = &mut self.explorer.follower {
                    f.resume(obs.tic);
                }
            }
            return self.retreat.is_some();
        }
        // A try at some other retreat that found no way doesn't hold off cover when it's due.
        if obs.tic - self.retreat_tic < self.fight.tune.retreat.again
            && (self.tried_cover || !self.cover_due())
        {
            return false;
        }
        self.retreat = self.start_retreat(obs);
        self.retreat.is_some()
    }

    /// In the blasts of a field of barrels (`barrels.field` of them or more in the chain she's
    /// in): out of it at once, whether or not something is there to set them off yet. With
    /// nothing to set them off (no monster that shoots awake and about, none going off), once
    /// only from about the same place: her way may lie through them, and run out of them every
    /// time she would never take it.
    pub(super) fn barrel_run_due(&self, obs: &Observation) -> bool {
        /// a run begun this near here before, with nothing having come of the barrels: not again
        const SAME_PLACE: f64 = 512.0;
        let c = &self.fight.combat;
        if c.now.invulnerable
            || crate::tactical::combat::Combat::barrel_field(obs) < self.fight.tune.barrels.field
        {
            return false;
        }
        !c.now.shooters.is_empty()
            || !obs.barrels_lit.is_empty()
            || !self
                .barrel_runs
                .iter()
                .any(|&p| dist(p, obs.pos()) < SAME_PLACE)
    }

    /// The shortest way to somewhere she knows of that is out of every barrel's blast, by the
    /// lanes between the barrels (`lanes::lane`). The run it makes has her legs and her eyes
    /// till she's there: nothing interrupts it.
    pub(super) fn out_of_barrels(&mut self, obs: &Observation) -> Option<RouteFollower> {
        /// out of a blast (128, and her radius, and a little)
        const CLEAR: f64 = 150.0;
        /// somewhere without barrels has none this near it: a gap in a field of them is no such
        /// place (the chain reaches it, and blasts throw barrels about)
        const AWAY: f64 = 384.0;
        /// the nearest this many places tried for a way to them
        const PLACES: usize = 24;
        /// ...and no more than this many of them searched for lanes between the barrels
        const LANES: usize = 4;
        let pos = obs.pos();
        let nav = Rc::clone(&self.explorer.nav);
        let k = &self.explorer.knowledge;
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        let mut places: Vec<Point> = k
            .known_polys()
            .into_iter()
            .filter(|&p| !k.damaging(nav.sector_of_poly(p)))
            .map(|p| nav.poly_center(p))
            .filter(|&q| {
                barrels
                    .iter()
                    .all(|&b| crate::motor::follower::blast_gap(q, b) >= AWAY)
            })
            .collect();
        places.sort_by(|a, b| dist(*a, pos).total_cmp(&dist(*b, pos)));
        let hurting: Vec<bool> = (0..obs.heights.len()).map(|s| k.damaging(s)).collect();
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        // Where she can stand, for the lanes between the barrels: all her box on floor she can step
        // between, with room overhead, none of it hurting.
        let floor = |p: Point| {
            let at = |q: Point| {
                nav.poly_at(q)
                    .map(|i| nav.sector_of_poly(i))
                    .filter(|&s| !hurting[s])
                    .map(|s| obs.heights[s])
                    .filter(|h| h.ceiling - h.floor >= crate::navigator::PLAYER_HEIGHT)
            };
            let r = crate::navigator::PLAYER_RADIUS + 1.0;
            let mid = at(p)?.floor;
            [(-r, -r), (r, -r), (r, r), (-r, r)]
                .into_iter()
                .all(|(dx, dy)| {
                    at((p.0 + dx, p.1 + dy))
                        .is_some_and(|h| (h.floor - mid).abs() <= crate::navigator::MAX_STEP)
                })
                .then_some(mid)
        };
        let length = |pts: &[Point]| pts.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>();
        let mut best: Option<(f64, crate::navigator::Path)> = None;
        let mut tried = 0;
        for q in places.into_iter().take(PLACES) {
            if tried >= LANES || best.as_ref().is_some_and(|b| dist(q, pos) >= b.0) {
                break; // (no shorter way to anywhere further off)
            }
            let Some(mut path) = view.find_path(pos, q, &live, None) else {
                continue;
            };
            if !path.jumps.is_empty() || path.waypoints.iter().any(Option::is_none) {
                continue;
            }
            // Not a way back through the ones going off, however near the place.
            if path
                .waypoints
                .windows(2)
                .filter_map(|w| Some((w[0]?, w[1]?)))
                .any(|(a, b)| {
                    obs.barrels_lit
                        .iter()
                        .any(|&l| segment_point_distance(a, b, l) < CLEAR)
                })
            {
                continue;
            }
            // The mesh's way has no barrels in it: the lanes between them, round about it.
            tried += 1;
            let along: Vec<Point> = path.waypoints.iter().flatten().copied().collect();
            let Some(lane) = crate::motor::follower::lanes::lane(
                pos,
                q,
                &along,
                &barrels,
                &floor,
                crate::navigator::MAX_STEP,
            ) else {
                if crate::debug::on().pace {
                    eprintln!("tic {}: no lane from ({:.0}, {:.0}) to ({:.0}, {:.0}) among {} barrels (floor here {:?}, there {:?})", obs.tic, pos.0, pos.1, q.0, q.1, barrels.len(), floor(pos), floor(q));
                }
                continue;
            };
            let way: Vec<Point> = std::iter::once(pos).chain(lane).collect();
            let l = length(&way);
            if best.as_ref().is_none_or(|b| l < b.0) {
                path.waypoints = way.into_iter().map(Some).collect();
                best = Some((l, path));
            }
        }
        let (length, path) = best?;
        if crate::debug::on().pace {
            eprintln!(
                "lane: {:?}",
                path.waypoints
                    .iter()
                    .flatten()
                    .map(|p| (p.0.round(), p.1.round()))
                    .collect::<Vec<_>>()
            );
        }
        let to = path
            .waypoints
            .iter()
            .flatten()
            .last()
            .copied()
            .unwrap_or(pos);
        self.log(format!("tic {}: in among barrels: running for ({:.0}, {:.0}), {length:.0} off between them, out of their blasts", obs.tic, to.0, to.1));
        let step = Step {
            path: Some(path),
            action: Action::Walk,
            trigger: None,
            key: None,
            barrel: None,
            label: "out of the barrels".to_string(),
            expects: Vec::new(),
        };
        let mut f = RouteFollower::new(
            view,
            vec![step],
            obs.keys.unwrap_or_default(),
            Some(live),
            false,
            obs.tic,
        );
        f.barrel_run = true;
        Some(f)
    }

    /// A retreat's own end: getting out and baiting after a while (baiting too once they're at
    /// each other: leave them to it); for ammo once armed again, or after a while; backing out
    /// once nothing's shot at her for a moment, or after a while; cover once nothing's shot at
    /// her for a moment.
    pub(super) fn retreat_over(&self, obs: &Observation, kind: RetreatKind) -> bool {
        let since = obs.tic - self.retreat_tic;
        let calm = obs.tic - self.shooters_tic > self.fight.tune.retreat.clear_tics;
        match kind {
            RetreatKind::Out => since > self.fight.tune.retreat.out_tics,
            RetreatKind::Bait => {
                since > self.fight.tune.retreat.out_tics
                    || self
                        .fight
                        .combat
                        .visible
                        .iter()
                        .any(|t| self.fight.combat.infighting(t))
            }
            RetreatKind::ForAmmo => {
                self.fight.combat.armed(obs) || since > self.fight.tune.retreat.ammo_tics
            }
            RetreatKind::Back => calm || since > self.fight.tune.retreat.back_tics,
            RetreatKind::Cover => calm,
            RetreatKind::Post => since > self.fight.tune.retreat.post_tics,
            RetreatKind::FallBack => since > self.fight.tune.retreat.back_tics,
            RetreatKind::OutOfBarrels => false, // (till she's there: nothing ends it on the way)
        }
    }

    /// Unarmed, the rest of her way runs into one of them: it has moved onto her way since she
    /// chose it, and nothing moves it off. One beside her already walls off only a way that
    /// takes her nearer it, as `for_ammo` chose: a way leading off from beside it isn't into it.
    pub(super) fn walled_off(&self, obs: &Observation, route: &RouteFollower) -> bool {
        let pos = obs.pos();
        let path = route.planned_path(Some(pos));
        self.fight.combat.now.about.iter().any(|t| {
            let touch = (monster(t.name).map_or(20.0, |m| m.radius) + PLAYER_RADIUS)
                .min(dist((t.x, t.y), pos) - self.fight.tune.retreat.ammo_margin);
            path.windows(2)
                .filter_map(|w| Some((w[0]?, w[1]?)))
                .any(|(a, b)| segment_point_distance(a, b, (t.x, t.y)) < touch)
        })
    }

    /// A retreat to start, the first whose rule holds (a rule that holds with no way to go still
    /// counts as a try: not again for a while):
    /// - cover: hurt, with something that shoots in sight, or an arch-vile;
    /// - for ammo: nothing to shoot with and something after her (away, for ammo, not fists at
    ///   it);
    /// - back: ambushed in a bad place (out of the middle of them instead, surrounded, with no
    ///   way back);
    /// - out: surrounded;
    /// - bait: a heavy she can't kill soon, with others for its shots to land on.
    pub(super) fn start_retreat(&mut self, obs: &Observation) -> Option<Retreat> {
        if self.fight.combat.now.invulnerable || self.fight.combat.bfg_flying(obs) {
            return None; // (nothing to fall back from; or her BFG's spray to come, from where she is)
        }
        let unarmed = !self.fight.combat.armed(obs) && !self.fight.combat.now.about.is_empty();
        let on_way = unarmed && !self.fight.combat.now.vile && self.on_for_ammo(obs);
        let (kind, route) = if self.fight.combat.now.vile {
            self.retreat_tic = obs.tic;
            (RetreatKind::Cover, self.find_cover(obs))
        } else if self.cover_due() {
            self.retreat_tic = obs.tic;
            // Unarmed, cover doesn't end it: nothing she does from there kills what's after her, and
            // the cover's end is when they stop shooting. So the ammo first, and cover if there's no way
            // off for it. A way off for ammo that was just called off (one of them on it) counts as
            // tried: then cover, not the next way off past them.
            let walled = obs.tic - self.walled_tic < self.fight.tune.retreat.again;
            match (unarmed && !walled)
                .then(|| self.for_ammo(obs, on_way, false))
                .flatten()
            {
                Some(r) => (RetreatKind::ForAmmo, Some(r)),
                None if on_way => (RetreatKind::ForAmmo, None),
                None if self.cover_spent(obs) => return None,
                None => (RetreatKind::Cover, self.find_cover(obs)),
            }
        } else if self.fight.combat.in_the_open(obs.tic) {
            self.retreat_tic = obs.tic;
            (RetreatKind::Post, self.find_post(obs))
        } else if unarmed {
            self.retreat_tic = obs.tic;
            (RetreatKind::ForAmmo, self.for_ammo(obs, on_way, true))
        } else {
            match self.fall_back(obs) {
                Some(r) => {
                    self.retreat_tic = obs.tic;
                    (RetreatKind::FallBack, Some(r))
                }
                _ => {
                    if let Some(why) = self.ambushed(obs).filter(|_| !self.back_spent(obs)) {
                        self.retreat_tic = obs.tic;
                        match self.back_out(obs, why) {
                            Some(r) => (RetreatKind::Back, Some(r)),
                            None if self.fight.combat.now.surrounded => {
                                (RetreatKind::Out, self.out_of_it(obs))
                            }
                            None => (RetreatKind::Back, None),
                        }
                    } else if self.fight.combat.now.surrounded {
                        self.retreat_tic = obs.tic;
                        (RetreatKind::Out, self.out_of_it(obs))
                    } else if obs.tic - self.bait_tic > self.fight.tune.retreat.bait_again {
                        let b = self.baiting(obs)?;
                        self.retreat_tic = obs.tic;
                        self.bait_tic = obs.tic;
                        self.fight.combat.baiting = true;
                        (RetreatKind::Bait, Some(b))
                    } else {
                        return None;
                    }
                }
            }
        };
        self.tried_cover = kind == RetreatKind::Cover;
        if self.barred_retreats.contains(&kind) {
            return None;
        }
        if route.is_some() {
            self.explorer
                .journal
                .note(|| Record::new(obs.tic, "retreat", format!("{kind:?}")));
            self.fight.combat.count(&format!("retreat: {kind:?}"), 1.0);
        }
        route.map(|route| Retreat { kind, route })
    }

    /// Hurt, with something shooting at her in sight: under `retreat.health`, or as low as that
    /// by the time she'd get to cover as far off as it's looked for, at the rate she's being
    /// hit (what a floor takes isn't counted: cover from an imp does nothing about nukage, and
    /// the way off the floor was left for it).
    pub(super) fn cover_due(&mut self) -> bool {
        let (health, rate) = (self.fight.combat.now.health, self.fight.combat.now.hit_rate);
        (health < self.fight.tune.retreat.health
            || health - rate * self.fight.tune.retreat.cover_range / RUN_SPEED
                < self.fight.tune.retreat.health)
            && !self.fight.combat.now.shooters.is_empty()
    }

    /// Is cover no answer just now? Back to it `retreat.cover_runs` times running with nothing
    /// killed and nothing new learned between (each within three minutes of the last), it has got
    /// her nothing and will again: out, set off for the same thing, the same ones in sight,
    /// back. None for `retreat.cover_rest`, twice as long each time it comes to that, and she
    /// goes on with what she was about. Counts this one.
    pub(super) fn cover_spent(&mut self, obs: &Observation) -> bool {
        /// retreats to cover are one run of them with no longer than this between (tics: a
        /// retreat, the wait out of sight and the walk back to the same ones is a minute or
        /// more by itself)
        const COVER_RUN: i64 = 180 * 35;
        if obs.tic < self.cover_off.0 {
            return true;
        }
        let now = (self.fight.combat.kills, self.explorer.knowledge.known());
        let (n, last, then) = &self.cover_runs;
        let same = *then == now && obs.tic - last < COVER_RUN;
        let n = if same { n + 1 } else { 1 };
        self.cover_runs = (n, obs.tic, now);
        if n <= self.fight.tune.retreat.cover_runs {
            return false;
        }
        let times = self.cover_off.1;
        let rest = self.fight.tune.retreat.cover_rest << times.min(5);
        self.cover_off = (obs.tic + rest, times + 1);
        self.cover_runs.0 = 0;
        self.log(format!(
            "tic {}: cover's got her nothing {n} times running: none for {:.0}s",
            obs.tic,
            rest as f64 / 35.0
        ));
        true
    }

    /// Is backing out of an ambush no answer just now? As cover (`cover_spent`, and by its
    /// numbers): backed out `retreat.cover_runs` times running with nothing killed and nothing
    /// new learned between, and the way she was going takes her back among the same ones, who
    /// are no fewer for it. None for `retreat.cover_rest`, twice as long each time, and she goes
    /// on through them. Doom II MAP13's key room, the key up among thirteen imps on ledges she
    /// has no shot at from the floor she has to cross: backed out every three seconds, over the
    /// doorway's line that stops the lift she'd just started. Counts this one.
    pub(super) fn back_spent(&mut self, obs: &Observation) -> bool {
        /// back-outs are one run of them with no longer than this between
        const BACK_RUN: i64 = 180 * 35;
        if obs.tic < self.back_off.0 {
            return true;
        }
        let now = (self.fight.combat.kills, self.explorer.knowledge.known());
        let (n, last, then) = &self.back_runs;
        let same = *then == now && obs.tic - last < BACK_RUN;
        let n = if same { n + 1 } else { 1 };
        self.back_runs = (n, obs.tic, now);
        if n <= self.fight.tune.retreat.cover_runs {
            return false;
        }
        let times = self.back_off.1;
        let rest = self.fight.tune.retreat.cover_rest << times.min(5);
        self.back_off = (obs.tic + rest, times + 1);
        self.back_runs.0 = 0;
        self.log(format!(
            "tic {}: backing out's got her nothing {n} times running: none for {:.0}s",
            obs.tic,
            rest as f64 / 35.0
        ));
        true
    }

    /// Where she stands now, while nothing's shooting at her and nothing's hurting her: on the
    /// trail back.
    pub(super) fn keep_trail(&mut self, obs: &Observation) {
        let calm = self.fight.combat.now.shooters.is_empty()
            && self.fight.combat.now.damage_rate == 0.0
            && !self.plight.hurting_floor;
        if !calm {
            return;
        }
        let pos = obs.pos();
        if self.trail.last().is_none_or(|&p| dist(p, pos) > TRAIL_STEP) {
            self.trail.push(pos);
            if self.trail.len() > TRAIL_LEN {
                self.trail.remove(0);
            }
        }
    }

    /// Monsters woken on ground she's never been on, that she hasn't fallen back from yet: back a
    /// little the way she came, onto ground she knows, to let them come to her.
    pub(super) fn fall_back(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let back = self.fight.tune.retreat.fall_back;
        if back <= 0.0 {
            return None;
        }
        let woken = self.fight.combat.woken_ahead();
        if !woken.iter().any(|t| !self.fallen_for.contains(&t.id)) {
            return None;
        }
        self.fallen_for.extend(woken.iter().map(|t| t.id));
        let pos = obs.pos();
        let nav = Rc::clone(&self.explorer.nav);
        let k = &self.explorer.knowledge;
        // The nearest place on her trail that far back, not on a floor that hurts.
        let spot = self.trail.iter().rev().copied().find(|&p| {
            dist(p, pos) >= back
                && nav
                    .poly_at(p)
                    .is_some_and(|pi| !k.damaging(nav.sector_of_poly(pi)))
        })?;
        let pi = nav.poly_at(spot)?;
        let them = self.fight.combat.now.about.clone();
        let f = self.go_to(obs, pi, &them, "fall back")?;
        self.fight.combat.count("fall backs", 1.0);
        self.log(format!(
            "tic {}: {} woken on ground she's not been on: back to ({:.0}, {:.0}) to let them come",
            obs.tic,
            woken.len(),
            spot.0,
            spot.1
        ));
        Some(f)
    }

    /// Caught in a bad place: a trap sprung (several of them seen all at once), or a fight that's
    /// going badly (losing health fast, or them on more than one side). Why, if so. In a tight
    /// ambush the choices are out, or kill fast.
    pub(super) fn ambushed(&mut self, obs: &Observation) -> Option<String> {
        // (a trap sprung, or a room walked in on: several of them seen all at once)
        let (count, tics) = (
            self.fight.tune.retreat.trap_count,
            self.fight.tune.retreat.trap_tics,
        );
        if count > 0
            && self.fight.combat.newly_seen(obs.tic, tics) >= count
            && !self.fight.combat.now.about.is_empty()
        {
            return Some(format!(
                "a trap: {} of them all at once",
                self.fight.combat.newly_seen(obs.tic, tics)
            ));
        }
        let them = self.fight.combat.now.shooters.clone();
        if them.is_empty() {
            return None;
        }
        let pos = obs.pos();
        let close: Vec<Point> = self
            .fight
            .combat
            .now
            .about
            .iter()
            .filter(|t| dist((t.x, t.y), pos) < self.fight.tune.retreat.ambush_near)
            .map(|t| (t.x, t.y))
            .collect();
        let losing = self.fight.combat.now.damage_rate > self.fight.tune.retreat.ambush_rate;
        if !losing && close.len() < self.fight.tune.retreat.ambush_count {
            return None;
        }
        let open = self
            .fight
            .combat
            .openness(obs, pos, self.fight.tune.retreat.open_look);
        let sides = spread(pos, &close);
        let bad = if self.plight.hurting_floor {
            "on a floor that hurts"
        } else if open < self.fight.tune.retreat.tight {
            "a tight spot"
        } else if sides > self.fight.tune.retreat.sides {
            "them on several sides"
        } else {
            return None;
        };
        Some(format!(
            "{bad}, {}",
            if losing {
                "losing health fast"
            } else {
                "several close"
            }
        ))
    }

    /// Back the way she came, to the nearest place on it none of them can see her (or well clear
    /// of them all), by a way not through them.
    pub(super) fn back_out(&mut self, obs: &Observation, why: String) -> Option<RouteFollower> {
        let them: Vec<Thing> = self.fight.combat.now.about.clone();
        let pos = obs.pos();
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        // Not back onto a floor that hurts now: the way she came may since have gone down into
        // what was under it.
        let k = &self.explorer.knowledge;
        let trail: Vec<Point> = self
            .trail
            .iter()
            .rev()
            .copied()
            .filter(|&p| {
                dist(p, pos) > self.fight.tune.retreat.back_min
                    && nav
                        .poly_at(p)
                        .is_none_or(|pi| !k.damaging(nav.sector_of_poly(pi)))
            })
            .collect();
        for p in trail {
            let Some(pi) = nav.poly_at(p) else { continue };
            let z = heights[nav.sector_of_poly(pi)].0;
            let seen = them
                .iter()
                .any(|t| self.fight.combat.sees_her(eye(t), p, z, &heights));
            let clear = them
                .iter()
                .all(|t| dist((t.x, t.y), p) > self.fight.tune.retreat.back_clear);
            if (!seen || clear)
                && let Some(f) = self.go_to(obs, pi, &them, "back out")
            {
                self.log(format!(
                    "tic {}: caught in {why}: backing out the way she came to ({:.0}, {:.0})",
                    obs.tic, p.0, p.1
                ));
                return Some(f);
            }
        }
        None
    }

    /// Is poly `p` in a pocket of the ground she knows: somewhere small with one narrow way in
    /// (`pockets`), that a few of them in its mouth would shut her in?
    pub(super) fn in_pocket(&self, p: usize) -> bool {
        let known = self.explorer.knowledge.known_polys();
        let mut memo = self.pocketed.borrow_mut();
        if memo.0 != known.len() {
            *memo = (known.len(), Default::default());
        }
        *memo.1.entry(p).or_insert_with(|| {
            crate::pockets::Ground::new(&self.explorer.nav.mesh, &known)
                .around(p, self.fight.tune.retreat.pocket_area)
                .is_some()
        })
    }

    /// The places she's been, not on hurting floors, between `near` and `far` from her: nearest
    /// first, at most `n`. Outmatched, not those in a pocket, while there's anywhere else: a
    /// small room with one way in is no retreat from more than she can kill.
    pub(super) fn visited_spots(
        &self,
        obs: &Observation,
        near: f64,
        far: f64,
        n: usize,
    ) -> Vec<(f64, usize)> {
        let k = &self.explorer.knowledge;
        let nav = &self.explorer.nav;
        let mut spots: Vec<(f64, usize)> = k
            .visited
            .iter()
            .filter(|&&p| !k.damaging(nav.sector_of_poly(p)))
            .map(|&p| (dist(nav.poly_center(p), obs.pos()), p))
            .filter(|&(d, _)| near < d && d < far)
            .collect();
        spots.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        // Only where she can stand: a spot is walked to at its poly's middle, and the middle of
        // a sliver along a wall is nearer the wall than her half-width. Sent to one for cover
        // she pressed on the wall 12 short of it till the retreat was over, ten seconds, the
        // maintainer watching (Doom II MAP14, 1,212 s).
        let stands = |&(_, p): &(f64, usize)| self.explorer.standable(nav.poly_center(p), false);
        if self.fight.combat.now.outmatched {
            let open: Vec<(f64, usize)> = spots
                .iter()
                .copied()
                .filter(|&(_, p)| !self.in_pocket(p))
                .filter(stands)
                .take(n)
                .collect();
            if !open.is_empty() {
                return open;
            }
        }
        spots.into_iter().filter(stands).take(n).collect()
    }

    /// With a heavy after her she can't see off quickly and monsters of another kind near it:
    /// to a place she's been where its shots at her go through them, holding fire.
    pub(super) fn baiting(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let them = self.fight.combat.now.about.clone();
        let fp = self.fight.combat.now.firepower;
        let heavies: Vec<&Thing> = them
            .iter()
            .filter(|h| {
                HEAVIES.contains(&h.name)
                    && mhealth(h.name) / fp > self.fight.tune.retreat.bait_heavy
            })
            .collect();
        if heavies.is_empty() {
            return None;
        }
        let spots = self.visited_spots(
            obs,
            64.0,
            self.fight.tune.retreat.bait_range,
            COVER_SPOTS * 2,
        );
        let nav = Rc::clone(&self.explorer.nav);
        let mut good: Vec<(f64, usize, Thing, Thing)> = Vec::new();
        for h in &heavies {
            let kind = species(h.name);
            let shields: Vec<&Thing> = self
                .fight
                .combat
                .visible
                .iter()
                .filter(|b| {
                    b.id != h.id && species(b.name) != kind && dist((b.x, b.y), (h.x, h.y)) < 1024.0
                })
                .collect();
            for &(d, pi) in &spots {
                let c = nav.poly_center(pi);
                if let Some(b) = shields.iter().find(|b| {
                    segment_point_distance((h.x, h.y), c, (b.x, b.y))
                        < monster(b.name).map_or(20.0, |m| m.radius) + 8.0
                        && 64.0 < dist((h.x, h.y), (b.x, b.y))
                        && dist((h.x, h.y), (b.x, b.y)) < dist((h.x, h.y), c) - 128.0
                }) {
                    good.push((d, pi, (*h).clone(), (*b).clone()));
                }
            }
        }
        good.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, pi, h, b) in good.into_iter().take(6) {
            if let Some(f) = self.go_to(obs, pi, &them, "bait") {
                self.log(format!(
                    "tic {}: baiting {} {} into {} {}",
                    obs.tic, h.name, h.id, b.name, b.id
                ));
                return Some(f);
            }
        }
        None
    }
}
