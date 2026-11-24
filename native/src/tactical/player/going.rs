// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Going somewhere in a fight: for ammo, to a point, by a way that's clear of them.

use super::*;

impl Player {
    /// Out of ammo with something after her: to the nearest ammo or gun she knows of that she can
    /// use, by a way not past them; else to the spot she's been that's furthest from them.
    /// Through doors if need be. With no plain way to ammo but on her way for some already
    /// (`on_way`): on with that, not away. With no way to ammo at all, not hurt (`keep_on`), and
    /// her way on clear of them: on with it.
    pub(super) fn for_ammo(
        &mut self,
        obs: &Observation,
        on_way: bool,
        keep_on: bool,
    ) -> Option<RouteFollower> {
        let them = self.fight.combat.now.about.clone();
        let pos = obs.pos();
        // No nearer to any of them than she is: with one on her already every way off starts by
        // it, and away is still away.
        let nearest = them
            .iter()
            .map(|m| dist((m.x, m.y), pos))
            .fold(f64::INFINITY, f64::min);
        let clear = self
            .fight
            .tune
            .errands
            .past_clear
            .min(nearest - self.fight.tune.retreat.ammo_margin)
            .max(0.0);
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        let known = self.explorer.knowledge.known_polys();
        let nav = Rc::clone(&self.explorer.nav);
        let floors = self.explorer.roomiest();
        // To the thing itself, not the middle of the floor it's on: a big floor's middle can be
        // further from the thing than her reach, and she'd stop there none the richer.
        let mut ammo: Vec<(f64, Point, &'static str, i32)> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| {
                weapon_item(t.name).is_some_and(|s| s != 1 && s != 8)
                    || ammo_item(t.name)
                        .is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s)))
            })
            .filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
            // (not one lying out of her reach over the floor under it: at a ledge's edge, the
            // floor beside sunk away. She'd stand under it none the richer)
            .filter(|t| {
                nav.poly_at((t.x, t.y)).is_none_or(|p| {
                    t.z - floors[nav.sector_of_poly(p)].0 <= crate::geom::PLAYER_HEIGHT
                })
            })
            .map(|t| (dist((t.x, t.y), pos), (t.x, t.y), t.name, t.id))
            .collect();
        ammo.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Not for one she has gone for twice already and come away from no richer: there's a
        // way to it by the map and she doesn't get there (run off it each time by what's
        // after her, or it's nowhere she can stand), and gone for a third time it's the same
        // twenty seconds again, for ever. (Memento Mori MAP22: a Shell, a Spider Mastermind
        // about; "away for the Shell" 151 times, fifty minutes, to the end of the hour.) With
        // a round gained since, she's got to something: all to go for again.
        const AMMO_TRIES: u32 = 2;
        let rounds = obs
            .ammo
            .as_ref()
            .map_or(0, |a| a.iter().map(|&(_, n)| n).sum::<i32>());
        self.ammo_gone_for.retain(|&(_, _, had)| rounds <= had);
        let mut refused: Vec<i32> = Vec::new();
        for (_, at, name, id) in ammo.into_iter().take(6) {
            if self
                .ammo_gone_for
                .iter()
                .any(|&(t, n, _)| t == id && n >= AMMO_TRIES)
            {
                refused.push(id);
                continue;
            }
            if let Some(f) = self.go_to_point(obs, at, &them, "for ammo", clear, true) {
                self.log(format!("tic {}: out of ammo: away for the {name}", obs.tic));
                match self.ammo_gone_for.iter_mut().find(|e| e.0 == id) {
                    Some(e) => e.1 += 1,
                    None => self.ammo_gone_for.push((id, 1, rounds)),
                }
                return Some(f);
            }
            refused.push(id);
        }
        // And the errand for one of those is given up: unarmed, with no way to it clear of them, it
        // would only take her back into them once she's away. It is chosen afresh later, with them
        // known.
        if self
            .explorer
            .current
            .as_ref()
            .is_some_and(|c| c.kind == Kind::Item && c.thing.is_some_and(|t| refused.contains(&t)))
        {
            self.explorer
                .give_up(obs, "out of ammo, no way to it clear of them");
        }
        if on_way {
            self.log(format!(
                "tic {}: out of ammo: on her way for it already",
                obs.tic
            ));
            return None;
        }
        // No way to ammo clear of them, and her own way on keeps clear of them: on with it, not
        // away. Away finds no ammo; her way on is where new ammo turns up, and every flight drags
        // her off it. Clear by the full margin, not the nearer one `clear` allows: with one on her,
        // away gains distance that her way on doesn't. And none of them a hitscanner: no way is
        // clear of its shots.
        let hitscan = them
            .iter()
            .any(|t| monster(t.name).is_some_and(|m| matches!(m.attack, Attack::Hitscan)));
        if keep_on && !hitscan && self.route_clear(obs, &them, self.fight.tune.errands.past_clear) {
            self.log(format!(
                "tic {}: out of ammo, none to get to: on her way, clear of them",
                obs.tic
            ));
            return None;
        }
        // Away isn't for ever. Away finds no ammo and kills nothing; what's left to do is past
        // them, and each time she set off for it she was run back from it, the same errand
        // chosen again once she was away: every 17 seconds till the clock ran out (Doom II
        // MAP14, seven runs of sixteen, two chaingunners between her and the rest of the
        // level). Fled so for `away_give_up` with no round gained and nothing new seen, she's
        // done with fleeing till she has or knows more: on with her way, past them.
        // (New ground seen sets the clock back, though it's seen by going on, never by going
        // away, and she does go on a little and get run back again: with only a round gained
        // setting it back the flights are a tenth as many and the score no better, two
        // more runs stalled. See the decision log.)
        let k = &self.explorer.knowledge;
        let has = (
            obs.ammo
                .as_ref()
                .map_or(0, |a| a.iter().map(|&(_, n)| n).sum::<i32>()),
            k.mapped.len() + k.sectors.len(),
        );
        match self.away_since {
            Some((_, had)) if has.0 > had.0 || has.1 > had.1 => self.away_since = None,
            Some((since, _)) if obs.tic - since > self.fight.tune.retreat.away_give_up => {
                self.log(format!(
                    "tic {}: out of ammo, none known: away has got her nothing: on with her way",
                    obs.tic
                ));
                return None;
            }
            _ => {}
        }
        let mut spots: Vec<(f64, usize)> = self
            .visited_spots(obs, 128.0, self.fight.tune.retreat.ammo_away, COVER_SPOTS)
            .into_iter()
            .map(|(_, pi)| {
                (
                    them.iter()
                        .map(|m| dist((m.x, m.y), nav.poly_center(pi)))
                        .fold(f64::INFINITY, f64::min),
                    pi,
                )
            })
            .collect();
        spots.sort_by(|a, b| b.0.total_cmp(&a.0));
        // Only somewhere further from them than where she stands: where she is is never one of
        // the spots (they're a way off), so the best of them from each of two places was the
        // other, and she ran between the two every second for five minutes (MAP06).
        let here = them
            .iter()
            .map(|m| dist((m.x, m.y), pos))
            .fold(f64::INFINITY, f64::min);
        spots.retain(|&(d, _)| d >= here + self.fight.tune.retreat.ammo_gain);
        // All of them, furthest first: the furthest may be beyond a lift, with no plain way there.
        for (_, pi) in spots {
            if let Some(f) = self.go_to_clear(obs, pi, &them, "for ammo", clear, true) {
                self.away_since.get_or_insert((obs.tic, has));
                self.log(format!(
                    "tic {}: out of ammo, none known: away from them",
                    obs.tic
                ));
                return Some(f);
            }
        }
        self.log(format!(
            "tic {}: out of ammo: no way off for ammo or away that isn't past them",
            obs.tic
        ));
        None
    }

    /// Unarmed, the explorer is already on her way to ammo or a gun she can use, by a way not
    /// past them. When `for_ammo` finds no plain way to any (on the heights as they are: no
    /// lifts, drops or jumps), that route is the way off for ammo. Neither away nor to cover
    /// then: nothing she does there kills what's after her, and the route would only take her
    /// back the way she came.
    pub(super) fn on_for_ammo(&self, obs: &Observation) -> bool {
        let Some(cur) = self
            .explorer
            .current
            .as_ref()
            .filter(|c| c.kind == Kind::Item)
        else {
            return false;
        };
        let Some(f) = self.explorer.follower.as_ref() else {
            return false;
        };
        let name = cur.label.trim_prefix("pick up ");
        let name = name.split(' ').next().unwrap_or(name);
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        if !(weapon_item(name).is_some_and(|s| s != 1 && s != 8)
            || ammo_item(name).is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s))))
        {
            return false;
        }
        let them = self.fight.combat.now.about.clone();
        let nearest = them
            .iter()
            .map(|m| dist((m.x, m.y), obs.pos()))
            .fold(f64::INFINITY, f64::min);
        let clear = self
            .fight
            .tune
            .errands
            .past_clear
            .min(nearest - self.fight.tune.retreat.ammo_margin)
            .max(0.0);
        keeps_clear(obs, f, &them, clear)
    }

    /// The explorer's way on, the rest of it, passes none of them nearer than `clear`.
    pub(super) fn route_clear(&self, obs: &Observation, them: &[Thing], clear: f64) -> bool {
        self.explorer.current.is_some()
            && self
                .explorer
                .follower
                .as_ref()
                .is_some_and(|f| keeps_clear(obs, f, them, clear))
    }

    /// A way to that poly (not past monsters), if there is one.
    pub(super) fn go_to(
        &mut self,
        obs: &Observation,
        pi: usize,
        them: &[Thing],
        label: &str,
    ) -> Option<RouteFollower> {
        self.go_to_clear(
            obs,
            pi,
            them,
            label,
            self.fight.tune.errands.past_clear,
            false,
        )
    }

    /// A way to that poly passing no monster closer than `clear` (`doors`: through doors she
    /// can open, else none).
    pub(super) fn go_to_clear(
        &mut self,
        obs: &Observation,
        pi: usize,
        them: &[Thing],
        label: &str,
        clear: f64,
        doors: bool,
    ) -> Option<RouteFollower> {
        let goal = self.explorer.nav.poly_center(pi);
        self.go_to_point(obs, goal, them, label, clear, doors)
    }

    /// The same, to a point.
    pub(super) fn go_to_point(
        &mut self,
        obs: &Observation,
        goal: Point,
        them: &[Thing],
        label: &str,
        clear: f64,
        doors: bool,
    ) -> Option<RouteFollower> {
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        let openable = doors.then(|| view.local_doors(obs.keys.unwrap_or_default()));
        let path = view.find_path(obs.pos(), goal, &live, openable.as_ref())?;
        let through = path
            .waypoints
            .windows(2)
            .filter_map(|w| Some((w[0]?, w[1]?)))
            .flat_map(|(a, b)| {
                them.iter()
                    .map(move |t| segment_point_distance(a, b, (t.x, t.y)))
            })
            .fold(f64::INFINITY, f64::min);
        if through < clear {
            return None; // (not past them)
        }
        let step = Step {
            path: Some(path),
            action: Action::Walk,
            trigger: None,
            key: None,
            barrel: None,
            label: label.to_string(),
            expects: Vec::new(),
        };
        Some(RouteFollower::new(
            view,
            vec![step],
            obs.keys.unwrap_or_default(),
            Some(live),
            false,
            obs.tic,
        ))
    }

    /// Seen-by score of spots, by the monsters' eyes.
    pub(super) fn seen_from(
        &mut self,
        obs: &Observation,
        spots: &[(f64, usize)],
        them: &[Thing],
        weigh: bool,
    ) -> Vec<f64> {
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        let places: Vec<(Point, f64)> = spots
            .iter()
            .map(|&(_, pi)| (nav.poly_center(pi), heights[nav.sector_of_poly(pi)].0))
            .collect();
        let mut seen = vec![0.0; spots.len()];
        for t in them {
            let Some(m) = monster(t.name) else { continue };
            for (i, &(p, z)) in places.iter().enumerate() {
                if self.fight.combat.sees_her(eye(t), p, z, &heights) {
                    seen[i] += if weigh { m.threat } else { 1.0 };
                }
            }
        }
        seen
    }
}
