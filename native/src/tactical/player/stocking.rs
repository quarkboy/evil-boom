// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Stocking up: what a pickup would gain her, and the supplies worth a detour.

use super::*;

impl Fight {
    /// What a pickup would add to what she starts the next level with, as a share: health (of
    /// 100), armor (of 100), a gun's ammo she has (of what it can be), a backpack's room; 0 once
    /// she's no need of it (full health, that ammo full, armor no better than hers). None of the
    /// bonuses (a point each), nor the powers (they don't last into the next level), nor guns she
    /// hasn't (had as they are).
    pub(super) fn stock_gain(
        &self,
        obs: &Observation,
        name: &str,
        health: f64,
        held: &[i32],
    ) -> f64 {
        if matches!(name, "HealthBonus" | "ArmorBonus") {
            return 0.0;
        }
        if let Some((amount, cap)) = health_item(name) {
            return (f64::from(amount).min(f64::from(cap) - health) / 100.0).max(0.0);
        }
        if armor_item(name).is_some() {
            return (Combat::pickup_gain(obs, name) / 100.0).max(0.0);
        }
        // (a backpack doubles what she can carry: the first is all room; another, its ammo)
        let packed = obs.backpack == Some(true);
        if name == "Backpack" && !packed {
            return 1.0;
        }
        let most = |s: i32| ammo_max(s) * if packed { 2.0 } else { 1.0 };
        let have = |s: i32| {
            f64::from(
                obs.ammo
                    .iter()
                    .flatten()
                    .find(|(x, _)| *x == s)
                    .map_or(0, |(_, n)| *n),
            )
        };
        let gain = |s: i32, amount: f64| (amount.min(most(s) - have(s)) / most(s)).max(0.0);
        if let Some((slots, n)) = ammo_item(name) {
            return slots
                .iter()
                .filter(|s| held.contains(s))
                .map(|&s| gain(s, f64::from(n)))
                .fold(0.0, f64::max);
        }
        // (a gun she has already is the ammo it comes with)
        match weapon_item(name).filter(|s| held.contains(s)) {
            Some(3) => gain(3, 8.0),
            Some(4) => gain(4, 20.0),
            Some(5) => gain(5, 2.0),
            Some(s @ (6 | 7)) => gain(s, 40.0),
            _ => 0.0,
        }
    }

    /// Items she has seen that she could use now.
    pub(super) fn supplies(
        &mut self,
        x: &Explorer,
        obs: &Observation,
        reach: &BTreeSet<usize>,
        pos: Point,
    ) -> Vec<Objective> {
        let known = x.knowledge.known_polys();
        let health = self.combat.now.health;
        let held = obs.weapons.clone().unwrap_or_default();
        let ammo = |s: i32| {
            f64::from(
                obs.ammo
                    .iter()
                    .flatten()
                    .find(|(x, _)| *x == s)
                    .map_or(0, |(_, n)| *n),
            )
        };
        // Then ammo and guns before anything else. Not sooner for having died for want of it: made
        // urgent, it's walked for past the dangers.
        let short = self.combat.now.short_of_ammo;
        let mut out = Vec::new();
        // A better gun than she has that something she knows of carries: killing it is the gun,
        // and when it's nearer than one lying about, it's the one to go for.
        let mut carried: Vec<(&'static str, Seen)> = Vec::new();
        for s in self.combat.remaining() {
            let Some(gun) =
                carries(s.name).filter(|g| weapon_item(g).is_some() && self.better_gun(obs, g))
            else {
                continue;
            };
            if !x
                .nav
                .poly_at(s.pos())
                .is_some_and(|p| known.contains(&p) && reach.contains(&p))
            {
                continue;
            }
            match carried.iter_mut().find(|(g, _)| *g == gun) {
                Some((_, nearest)) if dist(nearest.pos(), pos) <= dist(s.pos(), pos) => {}
                Some((_, nearest)) => *nearest = s,
                None => carried.push((gun, s)),
            }
        }
        self.combat.wanted_dead = carried.iter().map(|(_, s)| s.id).collect();
        for (gun, s) in &carried {
            let mut o = Objective::new(
                Kind::Hunt,
                x.stand_spot_for(s.pos(), reach),
                format!("hunt {} {} (for its {gun})", s.name, s.id),
            );
            o.thing = Some(s.id);
            // As a better gun lying about is: first, and for all that's by it.
            o.needed = true;
            o.urgent = true;
            out.push(o);
        }
        for t in obs.things.iter().flatten() {
            let Some(p) = x.nav.poly_at((t.x, t.y)) else {
                continue;
            };
            if !known.contains(&p) || !reach.contains(&p) {
                continue;
            }
            let d = dist((t.x, t.y), pos);
            // The same gun carried by something nearer: this one in its turn.
            let carried_nearer = carried
                .iter()
                .any(|(gun, s)| *gun == t.name && dist(s.pos(), pos) < d);
            // Going on to another level: what she'd start it short of, had before she leaves
            // (and only by a way clear of what she knows is about).
            let stock = self.going_on
                && self.stock_gain(obs, t.name, health, &held) > 0.0
                && self.clear_way_to(obs, (t.x, t.y));
            // The big spheres only when they'd add enough (a megasphere 100 to what she
            // can take, a soulsphere 50 health); short of that, left where they lie to come back for.
            let sphere_later = (t.name == "Soulsphere"
                && health > 200.0 - self.tune.errands.soul_gain)
                || (t.name == "Megasphere"
                    && Combat::pickup_gain(obs, "Megasphere") < self.tune.errands.mega_gain);
            if sphere_later && !stock {
                continue;
            }
            // How much it's needed (0: not at all, 1: badly).
            let mut need = 0.0;
            let invulnerability = t.name == "InvulnerabilitySphere"
                && self.wants_invulnerability(obs)
                && self.clear_way_to(obs, (t.x, t.y));
            if invulnerability || (weapon_item(t.name).is_some() && Self::new_gun(obs, t.name)) {
                need = 1.0;
            } else if let Some(slot) = weapon_item(t.name).filter(|&s| s != 1) {
                // A gun she has already is its ammo (a dropped shotgun is 4 shells): not left lying about.
                need = (1.0 - ammo(slot) / ammo_max(slot) * 1.5).max(0.0);
            } else if let Some((slots, _)) = ammo_item(t.name) {
                // Ammo for a gun she hasn't yet is wanted too, less (`spare_ammo`): the gun's to
                // come, here or a level on, and it's no good found with nothing to put in it. (A
                // rocket at her feet was left lying, with only the pistol.)
                let has_gun = slots.iter().any(|s| held.contains(s));
                let min = slots
                    .iter()
                    .filter(|s| !has_gun || held.contains(s))
                    .map(|&s| ammo(s) / ammo_max(s))
                    .reduce(f64::min);
                if t.name == "Backpack" {
                    need = 0.8;
                } else if let Some(min) = min {
                    need = (1.0 - min * 1.5).max(0.0);
                    if !has_gun {
                        need *= self.tune.errands.spare_ammo;
                    }
                }
            } else if let Some((amount, cap)) = health_item(t.name)
                && amount >= 10
                && health < f64::from(cap)
            {
                need = ((f64::from(cap) - health) / 60.0).min(1.0);
            }
            // Armor, and what brings armor with it, by what it adds to how much she can take. A
            // megaarmor on no armor is as much again as her health; a green over a few blue points is
            // less than nothing.
            if armor_item(t.name).is_some() {
                need = need.max((Combat::pickup_gain(obs, t.name) / 100.0).min(1.0));
            }
            let not_now = need <= 0.1 || (d > self.tune.errands.item_range && need < 0.8);
            if not_now && !stock {
                continue;
            }
            let Some(goal) = x.touch_spot((t.x, t.y), t.z, reach) else {
                continue;
            };
            let mut why = "";
            if invulnerability {
                // Invulnerability with something about she can't take on: gone for at once.
                why = " (something about she can't take on)";
            } else if (weapon_item(t.name).is_some() || ammo_item(t.name).is_some())
                && !self.clear_way_armed(&Self::armed_with(obs, t.name), (t.x, t.y))
            {
                // Something to fight with, but on the way to it something she couldn't take on even with
                // it: not worth dropping everything for.
            } else if armor_item(t.name).is_some()
                && self.combat.now.outmatched
                && d < self.tune.errands.heal_near
                && Combat::pickup_gain(obs, t.name) >= self.tune.errands.armor_outmatched
            {
                // Armor close by, with more in sight than she can kill: on before a gun is gone
                // for among them. (A megaarmor at her feet was left for a chaingun at a baron's.)
                why = " (outmatched)";
            } else if carried_nearer {
            } else if weapon_item(t.name).is_some() && need > 0.0 && self.better_gun(obs, t.name) {
                why = " (a better gun)";
            } else if short && (weapon_item(t.name).is_some() || ammo_item(t.name).is_some()) {
                why = " (out of ammo)";
            } else if health < self.tune.errands.hurt
                && health_item(t.name).is_some_and(|(a, _)| a >= 10)
            {
                why = " (hurt)";
            } else if health < self.tune.errands.survive_health
                && health_item(t.name).is_some_and(|(a, _)| a >= 10)
                && self.combat.now.time_to_death < self.tune.errands.survive_secs
            {
                // Losing health fast, not yet down to the rest: off for it now, while she can.
                why = " (to survive)";
            }
            let worth = (why == " (a better gun)").then(|| self.gun_worth(obs, t.name) * RUN_SPEED);
            if worth.is_some_and(|w| d > w) {
                why = ""; // (too far to drop everything for, as the crow flies even)
            }
            let to_take = if stock && why.is_empty() {
                " (to take on)"
            } else {
                ""
            };
            let mut o = Objective::new(
                Kind::Item,
                goal,
                format!("pick up {}{why}{to_take}", t.name),
            );
            o.thing = Some(t.id);
            o.stock = stock;
            o.stock_only = stock && (sphere_later || not_now);
            o.urgent = !why.is_empty();
            o.needed = need >= self.tune.errands.needed && !carried_nearer;
            o.worth = if why.is_empty() { None } else { worth };
            out.push(o);
        }
        out
    }
}
