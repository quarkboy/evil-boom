// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What a tic's look tells the fight: the monsters, missiles and barrels in sight, who's hurt
//! her and what's died, the movers and the ground she's on.

use super::*;

impl Combat {
    /// Ceilings coming down and floors coming up, by sector, and when: what the crusher tests go
    /// by. Crushers are where a fight's moves don't go.
    pub(super) fn watch_movers(&mut self, obs: &Observation) {
        if self.movers.ceilings.len() != obs.heights.len() {
            self.movers.ceilings = obs.heights.iter().map(|h| h.ceiling).collect();
            self.movers.ceiling_down = vec![i64::MIN / 2; obs.heights.len()];
            self.movers.crushed_low = vec![i64::MIN / 2; obs.heights.len()];
        }
        if self.movers.floors.len() != obs.heights.len() {
            self.movers.floors = obs.heights.iter().map(|h| h.floor).collect();
            self.movers.floor_up = vec![i64::MIN / 2; obs.heights.len()];
        }
        for (i, h) in obs.heights.iter().enumerate() {
            if h.ceiling < self.movers.ceilings[i] - 0.01 {
                self.movers.ceiling_down[i] = obs.tic;
                if h.ceiling - h.floor < PLAYER_HEIGHT {
                    self.movers.crushed_low[i] = obs.tic;
                }
            }
            self.movers.ceilings[i] = h.ceiling;
            if h.floor > self.movers.floors[i] + 0.01 {
                self.movers.floor_up[i] = obs.tic;
            }
            self.movers.floors[i] = h.floor;
        }
        for &s in self.movers.crushers.keys() {
            if !self.movers.crusher_out.contains_key(&s) {
                self.movers
                    .crusher_out
                    .insert(s, crate::crush::out_tics(&self.nav, &obs.heights, s));
            }
        }
    }

    /// Health lost since last tic: kept (for the rate she's losing it at) and logged with what
    /// was by her when it went.
    pub(super) fn note_hurt(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if let Some(h) = obs.health {
            if let Some(before) = self.health.filter(|&before| h < before) {
                self.damage.push((obs.tic, before - h));
                // (the floor's: nothing dealt it, as the status bar's face has it, and she's on
                // a floor she knows hurts)
                let floor = obs.hit_sides.is_empty() && self.on_hurting_floor(obs);
                if floor {
                    self.floor_damage.push((obs.tic, before - h));
                }
                // What by: what has gone off by her, what's up against her.
                let by: Vec<String> = obs
                    .things
                    .iter()
                    .flatten()
                    .filter(|t| {
                        (missile(t.name).is_some() && dist((t.x, t.y), pos) < 256.0)
                            || (monster(t.name).is_some() && dist((t.x, t.y), pos) < 128.0)
                    })
                    .map(|t| format!("{} {:.0} off", t.name, dist((t.x, t.y), pos)))
                    .collect();
                self.log.push(format!(
                    "tic {}: hurt {:.0}, health {h:.0} ({})",
                    obs.tic,
                    before - h,
                    if floor {
                        "the floor".to_string()
                    } else if by.is_empty() {
                        "nothing near".to_string()
                    } else {
                        by.join(", ")
                    }
                ));
            }
            self.health = Some(h);
        }
    }

    /// A monster gone from the game is dead. It is hers to know if she saw it go; else by its
    /// cry, if she hears it and can tell whose it was (`hear`); else it's still about, for all
    /// she knows, where she last saw it.
    pub(super) fn note_deaths(&mut self, obs: &Observation) {
        let monsters: BTreeMap<i32, Thing> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| monster(t.name).is_some())
            .map(|t| (t.id, t.clone()))
            .collect();
        let was_visible: BTreeSet<i32> = self.visible.iter().map(|t| t.id).collect();
        for i in self.senses.game_monsters.keys() {
            if monsters.contains_key(i) || self.dead.contains(i) || !was_visible.contains(i) {
                continue;
            }
            if let Some(s) = self.known.get(i) {
                self.dead.insert(*i);
                self.kills += 1;
                self.kind_died.insert(s.name, obs.tic);
                self.senses.cries_due.push((obs.tic, s.name));
                *self.tally.entry("deaths seen".to_string()).or_insert(0.0) += 1.0;
                self.log
                    .push(format!("tic {}: {} {i} dead", obs.tic, s.name));
            }
        }
        self.senses.game_monsters = monsters;
    }

    /// The monsters in her sight this tic (`visible`), and what she keeps of each: where and
    /// how she last saw it, whether it's awake, whom it's after, its health as she reckons it.
    pub(super) fn see_monsters(&mut self, obs: &Observation, me: Body) {
        let _timed = crate::debug::Timed::new("fight.see_monsters");
        let pos = obs.pos();
        self.visible.clear();
        for t in self.senses.game_monsters.values() {
            let d = dist((t.x, t.y), pos);
            if !(0.0..self.tune.engage.range).contains(&d) {
                continue;
            }
            let recent = self
                .known
                .get(&t.id)
                .is_some_and(|s| obs.tic - s.tic < self.tune.senses.recent_sight_tics);
            // In her view, or seen a moment ago in the corner of her eye. Not for being near: one behind
            // her that makes no sound she doesn't know of, till it does or she backs into it.
            if !(recent
                || d < self.tune.senses.see_near
                || norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < self.tune.senses.fov)
            {
                continue;
            }
            let target = Body {
                x: t.x,
                y: t.y,
                z: t.z,
                height: mon(t.name).height,
            };
            // Nothing between them; or, by instinct, something come between only a moment ago.
            let sees = self.los.clear(me, target, &self.heights_now);
            if !sees
                && !self
                    .known
                    .get(&t.id)
                    .is_some_and(|s| obs.tic - s.sighted <= self.tune.senses.instinct_tics)
            {
                continue;
            }
            let seen = self.known.entry(t.id).or_insert_with(|| {
                self.log.push(format!(
                    "tic {}: saw {} {} at ({:.0}, {:.0})",
                    obs.tic, t.name, t.id, t.x, t.y
                ));
                Seen {
                    id: t.id,
                    name: t.name,
                    x: t.x,
                    y: t.y,
                    z: t.z,
                    tic: obs.tic,
                    first: obs.tic,
                    x0: t.x,
                    y0: t.y,
                    angle0: t.angle,
                    awake: false,
                    lost: false,
                    going: (0.0, 0.0),
                    sighted: obs.tic,
                    foreknown: false,
                }
            });
            seen.x = t.x;
            seen.y = t.y;
            seen.z = t.z;
            seen.tic = obs.tic;
            seen.lost = false;
            seen.foreknown = false;
            if sees {
                seen.sighted = obs.tic;
            }
            if self.dead.remove(&t.id) {
                self.seen_hurt.remove(&t.id); // (raised again, whole: an arch-vile's work)
            }
            seen.awake = seen.awake
                || dist(seen.pos(), (seen.x0, seen.y0)) > self.tune.senses.wake_distance
                || t.vx.hypot(t.vy) > self.tune.senses.wake_speed
                || norm(t.angle - seen.angle0).abs() > self.tune.senses.wake_turn;
            let track = self.tracks.entry(t.id).or_default();
            if track
                .back()
                .is_some_and(|&(tic, _, _)| obs.tic - tic > self.tune.senses.track_tics)
            {
                track.clear(); // (out of sight a while: where it was then says nothing)
            }
            track.push_back((obs.tic, t.x, t.y));
            while track
                .front()
                .is_some_and(|&(tic, _, _)| obs.tic - tic > self.tune.senses.track_tics)
            {
                track.pop_front();
            }
            self.visible.push(t.clone());
        }
        // Each as she can know it. Its health is her reckoning (its kind's less what she's seen it
        // lose), never the game's own number. Whom it's after is what she can see of it: her, unless
        // it's turned to another monster in sight and not to her; never the game's own word for its
        // target.
        let facing = self.tune.senses.facing;
        let after_her: Vec<bool> = self
            .visible
            .iter()
            .map(|t| {
                let off = |p: Point| norm(heading((t.x, t.y), p) - t.angle).abs();
                off(pos) < facing
                    || !self
                        .visible
                        .iter()
                        .any(|o| o.id != t.id && off((o.x, o.y)) < facing)
            })
            .collect();
        for (t, after) in self.visible.iter_mut().zip(after_her) {
            t.after_her = after;
        }
        for t in &mut self.visible {
            // One going down she sees going down.
            if t.health > 0 {
                t.health =
                    (mon(t.name).health - self.seen_hurt.get(&t.id).copied().unwrap_or(0)).max(1);
            }
        }
        for t in &self.visible {
            self.last_seen.insert(t.id, t.clone());
        }
        let going: Vec<(i32, Point)> = self
            .visible
            .iter()
            .map(|t| (t.id, self.moving(t)))
            .collect();
        for (id, v) in going {
            if let Some(s) = self.known.get_mut(&id) {
                s.going = v;
            }
        }
        // One not where she last saw it, with the place in her sight again and a while gone by: she
        // doesn't know where it is.
        let in_sight: BTreeSet<i32> = self.visible.iter().map(|t| t.id).collect();
        let looked: Vec<i32> = self
            .known
            .values()
            .filter(|s| {
                !s.lost
                    && !in_sight.contains(&s.id)
                    && !self.dead.contains(&s.id)
                    && obs.tic - s.tic > self.tune.engage.lost_tics
            })
            .filter(|s| {
                let d = dist(s.pos(), pos);
                d < self.tune.engage.range
                    && norm(heading(pos, s.pos()) - obs.angle).abs() < self.tune.senses.fov
                    && self.los.clear(me, sight::eye_of(s), &self.heights_now)
            })
            .map(|s| s.id)
            .collect();
        for id in looked {
            if let Some(s) = self.known.get_mut(&id) {
                s.lost = true;
                self.log.push(format!(
                    "tic {}: {} {id} isn't where she last saw it",
                    obs.tic, s.name
                ));
                *self.tally.entry("lost".to_string()).or_insert(0.0) += 1.0;
            }
        }
    }

    /// Missiles: noticed in her view with nothing in the way, then followed while they fly. A
    /// missile in flight makes no sound, only its launch, which turns her to look. Her own she
    /// knows of.
    pub(super) fn see_missiles(&mut self, obs: &Observation, me: Body) {
        let _timed = crate::debug::Timed::new("fight.see_missiles");
        let pos = obs.pos();
        let flying: BTreeMap<i32, &Thing> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| missile(t.name).is_some() && monster(t.name).is_none())
            .map(|t| (t.id, t))
            .collect();
        self.missiles_seen.retain(|i| flying.contains_key(i));
        for (&id, t) in &flying {
            if self.missiles_seen.contains(&id) {
                continue;
            }
            let d = dist((t.x, t.y), pos);
            let noticed = t.owner == 0
                || (d < self.tune.engage.range
                    && norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < self.tune.senses.fov
                    && self.los.clear(
                        me,
                        Body {
                            x: t.x,
                            y: t.y,
                            z: t.z,
                            height: MISSILE_HEIGHT,
                        },
                        &self.heights_now,
                    ));
            if noticed {
                self.missiles_seen.insert(id);
            }
        }
    }

    /// Barrels are seen as monsters are: in her view (or near), nothing in the way; kept while
    /// they're there.
    pub(super) fn see_barrels(&mut self, obs: &Observation, me: Body) {
        let pos = obs.pos();
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        // The same barrel pushed by shots is still the one she saw.
        self.barrels_seen = self
            .barrels_seen
            .iter()
            .filter_map(|&b| crate::motor::follower::barrel_now(obs, b))
            .collect();
        self.barrels_seen.dedup_by(|a, b| dist(*a, *b) < 1.0);
        for &b in &barrels {
            let d = dist(b, pos);
            if d >= self.tune.engage.range || self.barrels_seen.iter().any(|&x| dist(x, b) < 1.0) {
                continue;
            }
            if !(d < self.tune.senses.near
                || norm(heading(pos, b) - obs.angle).abs() < self.tune.senses.fov)
            {
                continue;
            }
            let floor = self.nav.poly_at(b).map_or(0.0, |p| {
                self.heights_now
                    .get(self.nav.sector_of_poly(p))
                    .map_or(0.0, |h| h.0)
            });
            if self.los.clear(
                me,
                Body {
                    x: b.0,
                    y: b.1,
                    z: floor,
                    height: BARREL_HEIGHT,
                },
                &self.heights_now,
            ) {
                self.barrels_seen.push(b);
            }
        }
    }

    /// What only claws or charges, awake and in her sight, is watched for whether it gets any
    /// nearer while she stays put (`cant_reach`); the watch begins again when she moves off.
    pub(super) fn watch_reach(&mut self, obs: &Observation) {
        let pos = obs.pos();
        let watched: Vec<(i32, f64)> = self
            .visible
            .iter()
            .filter(|t| {
                matches!(mon(t.name).attack, Attack::Melee | Attack::Charge)
                    && self.known.get(&t.id).is_some_and(|k| k.awake)
            })
            .map(|t| (t.id, dist((t.x, t.y), pos)))
            .collect();
        self.reach_watch
            .retain(|id, w| watched.iter().any(|(i, _)| i == id) && dist(w.2, pos) < 128.0);
        for (id, d) in watched {
            self.reach_watch.entry(id).or_insert((obs.tic, d, pos));
        }
    }

    /// The ground she's on counts as been on, from now; and the ways round her that are in her
    /// view this tic are noted as looked along.
    pub(super) fn note_ground_and_view(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if self.been.is_empty() {
            self.been = vec![false; self.nav.mesh.polys.len()];
        }
        if let Some(p) = self.nav.poly_at(pos) {
            self.been[p] = true;
        }
        // The ways round her that are in her view this tic.
        for (i, at) in self.senses.looked.iter_mut().enumerate() {
            if norm(22.5 * i as f64 - obs.angle).abs() < self.tune.senses.fov {
                *at = obs.tic;
            }
        }
    }
}
