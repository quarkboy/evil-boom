// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Getting out of a fight's sight: out of it altogether, held at an edge, kept out of
//! sight, a post to shoot from, cover to fall back to.

use super::*;

impl Player {
    /// To somewhere she's been nearby where fewer of them can see her, they're further, and all
    /// on one side of her, by a way that doesn't go through them.
    pub(super) fn out_of_it(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let them: Vec<Thing> = self
            .fight
            .combat
            .visible
            .iter()
            .filter(|t| monster(t.name).is_some_and(|m| m.attack != Attack::None))
            .cloned()
            .collect();
        let spots = self.visited_spots(
            obs,
            96.0,
            self.fight.tune.retreat.spot_range,
            COVER_SPOTS * 2,
        );
        if spots.is_empty() || them.is_empty() {
            return None;
        }
        let seen = self.seen_from(obs, &spots, &them, true);
        let nav = Rc::clone(&self.explorer.nav);
        let mut scored: Vec<(f64, usize)> = spots
            .iter()
            .zip(&seen)
            .map(|(&(d, pi), &sees)| {
                let c = nav.poly_center(pi);
                let nearest = them
                    .iter()
                    .map(|t| dist(c, (t.x, t.y)))
                    .fold(f64::INFINITY, f64::min);
                let pts: Vec<Point> = them.iter().map(|t| (t.x, t.y)).collect();
                (
                    sees + spread(c, &pts) / 45.0 - nearest.min(640.0) / 96.0 + d / 256.0,
                    pi,
                )
            })
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (_, pi) in scored.into_iter().take(6) {
            if let Some(f) = self.go_to(obs, pi, &them, "out of it") {
                let g = nav.poly_center(pi);
                self.log(format!(
                    "tic {}: surrounded by {}: getting out to ({:.0}, {:.0})",
                    obs.tic,
                    them.len(),
                    g.0,
                    g.1
                ));
                return Some(f);
            }
        }
        self.log(format!(
            "tic {}: surrounded, nowhere better in reach",
            obs.tic
        ));
        None
    }

    /// On her way, hidden, about to step into the sight of a sniper that's firing (heard: its
    /// guns; at her, as far as she knows, unless she can see it's at something else): not yet.
    /// Once it stops, it winds up again before its next burst, which is the time to get across
    /// to the next cover. A player hears when its attack is over.
    pub(super) fn hold_at_edge(&mut self, obs: &Observation, c: &Controls) -> bool {
        if c.forward.hypot(c.strafe) < 1.0 || self.fight.combat.now.invulnerable {
            return false;
        }
        let firing: Vec<Seen> = self
            .fight
            .combat
            .remaining()
            .into_iter()
            .filter(|s| self.fight.combat.sniper(obs, s.name))
            .filter(|s| {
                let seen = self.fight.combat.visible.iter().find(|t| t.id == s.id);
                self.fight.combat.firing(obs, s) && seen.is_none_or(|t| t.after_her)
            })
            .collect();
        if firing.is_empty() {
            return false;
        }
        let pos = obs.pos();
        let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
        let ahead = (
            pos.0 + way.cos() * self.fight.tune.retreat.edge_look,
            pos.1 + way.sin() * self.fight.tune.retreat.edge_look,
        );
        let hidden_now = !firing
            .iter()
            .any(|s| self.fight.combat.shot_from(obs, s, pos));
        let seen_ahead = firing
            .iter()
            .any(|s| self.fight.combat.shot_from(obs, s, ahead));
        if hidden_now && seen_ahead {
            if obs.tic - self.edge_log > 35 {
                self.edge_log = obs.tic;
                self.log(format!(
                    "tic {}: holding at the edge of cover till the {} stops firing",
                    obs.tic, firing[0].name
                ));
            }
            return true;
        }
        false
    }

    /// Ways across ground that a hitscan shooter she can't take on would see her on (from where
    /// she last saw it) count for `EXPOSED` times their length: she goes round the back of
    /// what's between them rather than across the open. Worked out again once one has moved, or
    /// after a while.
    pub(super) fn keep_out_of_sight(&mut self, obs: &Observation) {
        let unhurt = self.fight.combat.now.invulnerable;
        let them: Vec<Seen> = self
            .fight
            .combat
            .remaining()
            .into_iter()
            .filter(|s| !unhurt && s.awake && self.fight.combat.sniper(obs, s.name))
            // Not one she can see fighting something else: it won't shoot her till it's done or she
            // hurts it, which is the time to cross. One out of sight is taken to be after her. A
            // monster sees all round once awake (A_Chase): its target is what counts, not its facing.
            .filter(|s| {
                self.fight
                    .combat
                    .visible
                    .iter()
                    .find(|t| t.id == s.id)
                    .is_none_or(|t| t.after_her)
            })
            .collect();
        let at: Vec<Point> = them.iter().map(Seen::pos).collect();
        if at.is_empty() {
            if self.exposure.take().is_some() {
                self.explorer.view().set_exposure(Vec::new());
            }
            return;
        }
        let fresh = self.exposure.as_ref().is_some_and(|(tic, was)| {
            obs.tic - tic < self.fight.tune.retreat.exposure_again
                && was.len() == at.len()
                && was
                    .iter()
                    .zip(&at)
                    .all(|(&a, &b)| dist(a, b) < self.fight.tune.retreat.exposure_moved)
        });
        if fresh {
            return;
        }
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        let combat = &self.fight.combat;
        let exposure: Vec<f64> = (0..nav.mesh.polys.len())
            .map(|pi| {
                if nav.mesh.polys[pi].is_none() {
                    return 1.0;
                }
                let c = nav.poly_center(pi);
                let z = heights.get(nav.sector_of_poly(pi)).map_or(0.0, |h| h.0);
                let seen = them.iter().any(|s| {
                    dist(c, s.pos()) < self.fight.tune.retreat.exposure_range
                        && combat.sees_her(eye_of(s), c, z, &heights)
                });
                if seen {
                    self.fight.tune.retreat.exposed
                } else {
                    1.0
                }
            })
            .collect();
        // First seen: the way she's on was chosen without it, so it's chosen again.
        self.fight.rethink |= self.exposure.is_none();
        self.explorer.view().set_exposure(exposure);
        self.exposure = Some((obs.tic, at));
    }

    /// Out in the open in a long fight with a hitscan shooter: to the nearest place she knows of
    /// (seen or been) that none of them can see, with somewhere near it they can (a corner to
    /// step out from and back behind), by a way that doesn't go through them. Crossing the open
    /// to get there beats standing in it under the bursts.
    pub(super) fn find_post(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let shooters = self.fight.combat.now.shooters.clone();
        if shooters.is_empty() {
            return None;
        }
        let nav = Rc::clone(&self.explorer.nav);
        let known = self.explorer.knowledge.known_polys();
        let heights = self.fight.combat.heights(obs).to_vec();
        let pos = obs.pos();
        // Places on a grid, not the nav's polys: a hall is a few big polys, and the shadow behind a
        // pillar lies inside one whose middle is in plain sight.
        let on_known = |p: Point| -> Option<f64> {
            let pi = nav.poly_in(p)?;
            let s = nav.sector_of_poly(pi);
            (known.contains(&pi) && !self.explorer.knowledge.damaging(s)).then(|| heights[s].0)
        };
        let fits = |p: Point| {
            let r = PLAYER_RADIUS;
            [(-r, -r), (r, -r), (r, r), (-r, r)]
                .into_iter()
                .all(|(dx, dy)| on_known((p.0 + dx, p.1 + dy)).is_some())
        };
        let n = (self.fight.tune.retreat.post_range / POST_GRID) as i32;
        let mut grid: Vec<(f64, Point, f64)> = (-n..=n)
            .flat_map(|i| {
                (-n..=n).map(move |j| {
                    (
                        pos.0 + f64::from(i) * POST_GRID,
                        pos.1 + f64::from(j) * POST_GRID,
                    )
                })
            })
            .filter_map(|p| {
                let d = dist(p, pos);
                (32.0 < d && d < self.fight.tune.retreat.post_range && fits(p)).then_some((
                    d,
                    p,
                    on_known(p)?,
                ))
            })
            .collect();
        grid.sort_by(|a, b| a.0.total_cmp(&b.0));
        let combat = &self.fight.combat;
        let seen = |p: Point, z: f64| {
            shooters
                .iter()
                .any(|t| combat.sees_her(eye(t), p, z, &heights))
        };
        let from_them = |p: Point| {
            shooters
                .iter()
                .map(|t| dist(p, (t.x, t.y)))
                .fold(f64::INFINITY, f64::min)
        };
        let mut posts: Vec<(f64, Point)> = Vec::new();
        for &(d, p, z) in grid.iter().take(POST_LOOKS) {
            if seen(p, z) {
                continue;
            }
            let peek = (0..8).any(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                [
                    self.fight.tune.retreat.post_peek / 2.0,
                    self.fight.tune.retreat.post_peek,
                ]
                .into_iter()
                .any(|r| {
                    let q = (p.0 + a.cos() * r, p.1 + a.sin() * r);
                    on_known(q).is_some_and(|qz| seen(q, qz))
                })
            });
            if peek {
                // The far side of something, and the further from them the better, not only the nearest:
                // bullets spread, so the further off, the fewer land.
                posts.push((d - self.fight.tune.retreat.post_far * from_them(p), p));
            }
        }
        posts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let found = posts.len();
        for g in posts.into_iter().take(4).map(|(_, p)| p) {
            if let Some(f) = self.go_to_point(
                obs,
                g,
                &shooters,
                "post",
                self.fight.tune.errands.past_clear,
                false,
            ) {
                self.log(format!(
                    "tic {}: in the open: to a post at ({:.0}, {:.0}), health {:.0}",
                    obs.tic,
                    g.0,
                    g.1,
                    obs.health.unwrap_or(0.0)
                ));
                return Some(f);
            }
        }
        self.log(format!(
            "tic {}: in the open, no post in reach ({} places, {found} posts)",
            obs.tic,
            grid.len()
        ));
        None
    }

    pub(super) fn find_cover(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let shooters = self.fight.combat.now.shooters.clone();
        let spots = self.visited_spots(
            obs,
            f64::NEG_INFINITY,
            self.fight.tune.retreat.cover_range,
            COVER_SPOTS,
        );
        if spots.is_empty() {
            return None;
        }
        let seen = self.seen_from(obs, &spots, &shooters, false);
        // Not in a door that's shut now: hidden only while it's shut, and it opens with her in it.
        let open = |pi: usize| {
            obs.heights
                .get(self.explorer.nav.sector_of_poly(pi))
                .is_some_and(|h| h.ceiling - h.floor >= PLAYER_HEIGHT)
        };
        let hidden: Vec<usize> = spots
            .iter()
            .zip(&seen)
            .filter(|(&(d, pi), &v)| v == 0.0 && d > 32.0 && open(pi))
            .map(|(&(_, pi), _)| pi)
            .collect();
        if hidden.is_empty() {
            self.log(format!("tic {}: no cover in reach", obs.tic));
            return None;
        }
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        // Open now first; else through a door she can open: the one she came in by may have shut
        // behind her.
        let doors = view.local_doors(obs.keys.unwrap_or_default());
        let hidden: Vec<usize> = hidden.into_iter().take(4).collect();
        let found = [None, Some(&doors)].into_iter().find_map(|d| {
            hidden.iter().find_map(|&pi| {
                let goal = view.nav.poly_center(pi);
                view.find_path(obs.pos(), goal, &live, d)
                    .map(|path| (goal, path))
            })
        });
        if let Some((goal, path)) = found {
            self.log(format!(
                "tic {}: falling back to cover at ({:.0}, {:.0}), health {:.0}",
                obs.tic,
                goal.0,
                goal.1,
                obs.health.unwrap_or(0.0)
            ));
            let step = Step {
                path: Some(path),
                action: Action::Walk,
                trigger: None,
                key: None,
                barrel: None,
                label: "cover".into(),
                expects: Vec::new(),
            };
            return Some(RouteFollower::new(
                Rc::clone(&view),
                vec![step],
                obs.keys.unwrap_or_default(),
                Some(live),
                false,
                obs.tic,
            ));
        }
        None
    }
}
