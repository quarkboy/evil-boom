// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What's worth going for in a fight: a better gun, what she's short of, the errands open.

use super::*;
use crate::bestiary::Monster;

impl Fight {
    pub(super) fn new_gun(obs: &Observation, name: &str) -> bool {
        if name == "SuperShotgun" {
            return obs.super_shotgun != Some(true);
        }
        if name == "Chainsaw" {
            return obs.chainsaw != Some(true);
        }
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        weapon_item(name).is_some_and(|s| !held.contains(&s))
    }

    pub(super) fn gun(obs: &Observation, name: &str) -> crate::bestiary::Weapon {
        if name == "SuperShotgun" {
            SUPER_SHOTGUN
        } else if name == "Chainsaw" {
            CHAINSAW
        } else {
            weapon_in(weapon_item(name).unwrap_or(2), obs)
        }
    }

    /// What she'd have with that gun or ammo picked up: the ammo, or the gun and the ammo a gun
    /// lying about comes with (twice its clip: two rockets, eight shells, twenty bullets, forty
    /// cells).
    pub(super) fn armed_with(obs: &Observation, name: &str) -> Observation {
        let mut o = obs.clone();
        let (pool, comes): (&[i32], i32) = if let Some(slot) = weapon_item(name).filter(|&s| s != 1)
        {
            let held = o.weapons.get_or_insert_with(|| vec![1, 2]);
            if !held.contains(&slot) {
                held.push(slot);
            }
            let clip = match slot {
                3 => "Shell",
                5 => "RocketAmmo",
                6 | 7 => "Cell",
                _ => "Clip",
            };
            ammo_item(clip).map_or((&[], 0), |(pool, n)| (pool, 2 * n))
        } else if let Some((pool, n)) = ammo_item(name).filter(|_| name != "Backpack") {
            (pool, n)
        } else {
            return o;
        };
        let ammo = o.ammo.get_or_insert_with(Vec::new);
        for &s in pool {
            match ammo.iter_mut().find(|(k, _)| *k == s) {
                Some((_, n)) => *n += comes,
                None => ammo.push((s, comes)),
            }
        }
        o
    }

    /// Would that gun do more than the best she has (worth dropping what she's doing for)?
    pub(super) fn better_gun(&self, obs: &Observation, name: &str) -> bool {
        // (one she has, the commonest answer, needs no look through what's about)
        name != "Chainsaw" && Self::new_gun(obs, name) && self.better_on(obs, name, &self.boss())
    }

    /// A boss she knows of still about, if any: what a gun is judged on. That is what the
    /// level's fight comes down to, and it takes no rocket's blast.
    fn boss(&self) -> Option<Monster> {
        self.combat
            .about()
            .filter_map(|s| monster(s.name))
            .find(|m| m.boss)
    }

    /// `better_gun`, the boss it's judged on in hand (`boss`).
    fn better_on(&self, obs: &Observation, name: &str, boss: &Option<Monster>) -> bool {
        name != "Chainsaw"
            && Self::new_gun(obs, name)
            && Combat::dps_on(&Self::gun(obs, name), boss.as_ref())
                > self.combat.firepower_on(obs, boss.as_ref())
    }

    /// Seconds of walking a gun is worth: what it'd save killing what she knows is still about
    /// (and some, for what's to come).
    pub(super) fn gun_worth(&self, obs: &Observation, name: &str) -> f64 {
        self.worth_against(obs, name, self.met())
    }

    /// The health of what she's met this life and is still about. With a level known beforehand
    /// "still about" would be everything in it, and every gun in it worth any walk.
    fn met(&self) -> f64 {
        self.combat
            .about()
            .filter(|s| !s.foreknown)
            .map(|s| mhealth(s.name))
            .sum()
    }

    /// `gun_worth`, the health it's to save killing in hand (`met`).
    fn worth_against(&self, obs: &Observation, name: &str, hp: f64) -> f64 {
        let w = Self::gun(obs, name);
        let now = self.combat.now.firepower;
        self.tune.errands.gun_worth + (hp / now - hp / w.dps().max(now)).max(0.0)
    }

    /// What she's going for to fight with (and where it is), if that's what the route is for;
    /// or, mid-fight, with only the pistol or next to no ammo, something to fight with that she
    /// hasn't turned aside for yet.
    pub(super) fn errand(&self, x: &Explorer, obs: &Observation) -> Option<(String, Point)> {
        // Only what's near: a gun far off isn't walked for through a fight. Near by the way there,
        // where it's what her route is for: one close as the crow flies can be a long way round.
        let reach = self.tune.errands.press_reach;
        let far = |at: Point| {
            let straight = dist(at, obs.pos());
            match (&x.current, &x.follower) {
                (Some(c), Some(f)) if dist(c.goal, at) < 1.0 => straight.max(f.way_left(obs.pos())),
                _ => straight,
            }
        };
        self.errand_any(x, obs)
            .filter(|(_, at)| x.press_on || far(*at) < reach)
    }

    pub(super) fn errand_any(&self, x: &Explorer, obs: &Observation) -> Option<(String, Point)> {
        // An order to press on with, mid-fight: the objective in hand is the errand.
        if let Some(cur) = x.current.as_ref().filter(|_| x.press_on) {
            return Some((cur.label.clone(), cur.goal));
        }
        if let Some(cur) = x.current.as_ref().filter(|c| c.kind == Kind::Item) {
            // The item's class name, before what the label adds ("pick up Medikit (to heal)").
            let name = cur.label.trim_prefix("pick up ");
            let name = name.split(' ').next().unwrap_or(name);
            // Health she's on her way to and can use, close by: on to it, shooting on the way. Not one
            // far off, however low she is: the fight's own retreats and cover are what keep her alive
            // there, and a small pickup doesn't pay for the way.
            let heals = health_item(name).is_some_and(|(amount, cap)| {
                amount >= 10 && self.combat.now.health < f64::from(cap)
            });
            // Ammo, urgent, only within the reach that ammo is turned aside for mid-fight.
            let near = dist(cur.goal, obs.pos()) < self.tune.errands.item_range;
            if cur.urgent && health_item(name).is_none() && (near || ammo_item(name).is_none()) {
                return Some((name.to_string(), cur.goal));
            }
            if heals && dist(cur.goal, obs.pos()) < self.tune.errands.heal_near {
                return Some((name.to_string(), cur.goal));
            }
        }
        if self.combat.now.firepower > self.tune.errands.pistol_only
            && !self.combat.now.short_of_ammo
        {
            return None;
        }
        self.errands(x, obs)
            .first()
            .map(|t| (t.name.to_string(), (t.x, t.y)))
    }

    /// Guns and ammo she knows where they are and hasn't turned aside for yet: better guns worth
    /// the walk, and, short of ammo, ammo for what she has (or any gun).
    pub(super) fn errands(&self, x: &Explorer, obs: &Observation) -> Vec<Thing> {
        let known = x.knowledge.known_polys();
        let pos = obs.pos();
        let short = self.combat.now.short_of_ammo;
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        // What the guns are judged on: looked up once, and only if there's a gun to judge (a
        // level may have a thousand lying about, and ten thousand monsters to look through).
        let boss = std::cell::LazyCell::new(|| self.boss());
        let met = std::cell::LazyCell::new(|| self.met());
        obs.things
            .iter()
            .flatten()
            .filter(|t| !self.offered.contains(&t.id) && !self.offered_spheres.contains(&t.id))
            .filter(|t| {
                if t.name == "InvulnerabilitySphere" {
                    // In the middle of a fight she can't win as she is: what wins it.
                    return dist((t.x, t.y), pos) < 2.0 * self.tune.errands.item_range
                        && self.wants_invulnerability(obs, (t.x, t.y))
                        && self.clear_dash_to(obs, (t.x, t.y));
                }
                if let Some(slot) = weapon_item(t.name)
                    .filter(|&s| s != 1 && held.contains(&s) && !Self::new_gun(obs, t.name))
                {
                    // One she has: its ammo.
                    short
                        && held.contains(&slot)
                        && dist((t.x, t.y), pos) < self.tune.errands.item_range
                } else if weapon_item(t.name).is_some() {
                    ((Self::new_gun(obs, t.name) && self.better_on(obs, t.name, &boss))
                        || (short && Self::new_gun(obs, t.name) && t.name != "Chainsaw"))
                        && dist((t.x, t.y), pos)
                            <= self.worth_against(obs, t.name, *met) * RUN_SPEED
                } else {
                    short
                        && ammo_item(t.name)
                            .is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s)))
                        && dist((t.x, t.y), pos) < self.tune.errands.item_range
                }
            })
            .filter(|t| {
                x.nav
                    .poly_at((t.x, t.y))
                    .is_some_and(|p| known.contains(&p))
            })
            .cloned()
            .collect()
    }
}
