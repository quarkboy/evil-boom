// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Keeping count as she fights: the rounds she's spent and what they did, the health she's
//! lost and to what. Mostly for the logs and the tests; kept apart from what she sees
//! (`seeing.rs`) so the two aren't taken for each other.

use super::*;

impl Combat {
    /// The rounds gone from her guns since last tic. A bullet or a shell gone is a shot out:
    /// when, and from which pool, is kept (`shot_out`, and `burst_from` for the pistol's and
    /// the chaingun's presses), since the trigger's timing goes by it. The rest is counting,
    /// for the log and the run's statistics: rounds spent, shots, and shots with nothing on
    /// her line. Pools shared by two guns are counted once.
    pub(super) fn note_rounds_gone(&mut self, obs: &Observation, ammo: &[(i32, i32)]) {
        let pos = obs.pos();
        for &(s, n) in ammo {
            if matches!(s, 4 | 7) {
                continue;
            }
            if let Some(&(_, before)) = self.ammo_was.iter().find(|(x, _)| *x == s)
                && n < before
            {
                self.spent += f64::from(before - n);
                *self.tally.entry("rounds spent".to_string()).or_insert(0.0) +=
                    f64::from(before - n);
                // A bullet or shell gone with nothing it could hit on her line: into a wall, or the corner
                // she'd stepped back behind since the trigger.
                if matches!(s, 2 | 3) {
                    // The chaingun's second bullet, 4 tics on, is of the same press.
                    let second = obs.weapon == Some(4)
                        && self
                            .trigger
                            .shot_out
                            .is_some_and(|(p, t)| p == s && obs.tic - t <= CHAINGUN_SECOND);
                    if s == 2 && !second {
                        self.trigger.burst_from = obs.tic;
                    }
                    if crate::debug::on().shot {
                        let gap = self.trigger.shot_out.map_or(0, |(_, t)| obs.tic - t);
                        self.log.push(format!(
                            "tic {}: a shot out, gun {:?}, {gap} tics after the last",
                            obs.tic, obs.weapon
                        ));
                    }
                    self.trigger.shot_out = Some((s, obs.tic));
                    let seen = self.visible.clone();
                    let on = seen.iter().any(|t| {
                        let d = dist((t.x, t.y), pos).max(1.0);
                        let wide =
                            (mon(t.name).radius / d).atan().to_degrees() + aiming::AUTOAIM_SPREAD;
                        norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < wide
                            && self.shot_clear(obs, (t.x, t.y), t.z, mon(t.name).height)
                    });
                    *self
                        .tally
                        .entry("shots (bullets, shells)".to_string())
                        .or_insert(0.0) += 1.0;
                    if !on {
                        if crate::debug::on().shot {
                            let what: Vec<String> = seen
                                .iter()
                                .map(|t| {
                                    let clear =
                                        self.shot_clear(obs, (t.x, t.y), t.z, mon(t.name).height);
                                    format!(
                                        "{} {} {:.0} off, {:.1} degrees off her aim, {}",
                                        t.name,
                                        t.id,
                                        dist((t.x, t.y), pos),
                                        norm(heading(pos, (t.x, t.y)) - obs.angle),
                                        if clear { "clear" } else { "not clear" }
                                    )
                                })
                                .collect();
                            self.log.push(format!("tic {}: a shot (slot {s}) with nothing on her line; target {:?}, pressed {:?}, the move {:?}; in sight: {}", obs.tic, self.target, self.trigger.last_shot, self.last_move, what.join("; ")));
                        }
                        *self
                            .tally
                            .entry("shots with nothing on her line".to_string())
                            .or_insert(0.0) += 1.0;
                        let why = if seen.is_empty() {
                            "nothing in sight"
                        } else {
                            "something in sight"
                        };
                        *self
                            .tally
                            .entry(format!(
                                "shots with nothing on her line: {why}, the move {:?}",
                                self.last_move
                            ))
                            .or_insert(0.0) += 1.0;
                    }
                }
            }
        }
    }

    /// What has gone from the gun in her hands is put down to her target, as she'd reckon it:
    /// so many shots, at what such a shot does landing at that range (`put_in`).
    pub(super) fn reckon_put_in(&mut self, obs: &Observation, ammo: &[(i32, i32)]) {
        let pos = obs.pos();
        if let (Some(slot), Some(id)) = (obs.weapon, self.target) {
            let was = self
                .ammo_was
                .iter()
                .find(|(x, _)| *x == slot)
                .map(|&(_, n)| n);
            let now = ammo.iter().find(|(x, _)| *x == slot).map(|&(_, n)| n);
            if let (Some(was), Some(now)) = (was, now)
                && let Some(t) = obs
                    .things
                    .iter()
                    .flatten()
                    .find(|t| t.id == id)
                    .filter(|_| now < was)
                && let Some(m) = monster(t.name)
            {
                let w = weapon_in(slot, obs);
                let shots = f64::from(was - now) / f64::from(w.ammo_per_shot.max(1));
                *self.put_in.entry(id).or_insert(0.0) +=
                    shots * w.damage_on(&m) * w.hits(m.radius, dist((t.x, t.y), pos));
            }
        }
    }

    /// The health monsters have lost since last tic. What one in her sight lost is what she
    /// saw it lose (`seen_hurt`), whoever hurt it. The rest is counting: damage dealt, and
    /// shots that hurt nothing with a monster in sight.
    pub(super) fn note_health_lost(&mut self, obs: &Observation) {
        let pos = obs.pos();
        // (by id, in order: a list searched by halving. Every monster of the level is gone
        // through each tic, and on a level of thousands a map of them built and searched
        // each tic was a tenth of her thinking)
        let had = |list: &[(i32, i32)], id: i32| {
            list.binary_search_by_key(&id, |&(i, _)| i)
                .ok()
                .map(|at| list[at].1)
        };
        let mut now: Vec<(i32, i32)> = Vec::with_capacity(self.health_was.len());
        let mut hurt_any = false;
        for t in obs
            .things
            .iter()
            .flatten()
            .filter(|t| monster(t.name).is_some())
        {
            if let Some(h) = had(&self.health_was, t.id) {
                // Seen: in her sight the tic before, the hit landing in front of her.
                if t.health < h && self.visible.iter().any(|v| v.id == t.id) {
                    *self.seen_hurt.entry(t.id).or_insert(0) += h - t.health.max(0);
                }
                if t.health < h {
                    hurt_any = true;
                    self.dealt += f64::from(h - t.health.max(0));
                    *self.tally.entry("damage dealt".to_string()).or_insert(0.0) +=
                        f64::from(h - t.health.max(0));
                }
            }
            now.push((t.id, t.health));
        }
        now.sort_unstable_by_key(|&(id, _)| id);
        now.dedup_by_key(|&mut (id, _)| id);
        // One killed is gone from the list, and that counts as hurt.
        hurt_any |= self
            .visible
            .iter()
            .any(|v| had(&self.health_was, v.id).is_some() && had(&now, v.id).is_none());
        // A bullet or a shell gone this tic, a monster in sight, and nothing hurt by it.
        if self.trigger.shot_out.is_some_and(|(_, t)| t == obs.tic)
            && !self.visible.is_empty()
            && !hurt_any
        {
            *self
                .tally
                .entry("shots that hurt nothing, a monster in sight".to_string())
                .or_insert(0.0) += 1.0;
            *self
                .tally
                .entry(format!(
                    "shots that hurt nothing, a monster in sight: the move {:?}",
                    self.last_move
                ))
                .or_insert(0.0) += 1.0;
            let t = self
                .target
                .and_then(|id| self.visible.iter().find(|t| t.id == id).cloned());
            let how = match &t {
                None => "no target in sight".to_string(),
                Some(t) => {
                    let d = dist((t.x, t.y), pos);
                    let off = norm(heading(pos, (t.x, t.y)) - obs.angle).abs();
                    let wide = (mon(t.name).radius / d.max(1.0)).atan().to_degrees();
                    let clear = self.shot_clear(obs, (t.x, t.y), t.z, mon(t.name).height);
                    format!(
                        "{}, {}, {}",
                        if off < wide {
                            "aimed on it"
                        } else if off < wide + aiming::AUTOAIM_SPREAD {
                            "aimed near it"
                        } else {
                            "aimed off it"
                        },
                        if clear { "the line clear" } else { "no line" },
                        if d < 256.0 {
                            "under 256"
                        } else if d < 640.0 {
                            "256-640"
                        } else {
                            "over 640"
                        }
                    )
                }
            };
            *self
                .tally
                .entry(format!(
                    "shots that hurt nothing, gun {}: {how}",
                    obs.weapon.unwrap_or(0)
                ))
                .or_insert(0.0) += 1.0;
        }
        // Gone: killed, and what it had left was the last of it.
        self.dealt += self
            .health_was
            .iter()
            .filter(|&&(id, _)| had(&now, id).is_none())
            .map(|&(_, h)| f64::from(h.max(0)))
            .sum::<f64>();
        self.health_was = now;
    }
}
