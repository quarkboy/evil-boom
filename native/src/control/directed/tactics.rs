// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Her say in a fight: the stance, the target and the gun she orders, kept on her while they hold.

use super::*;

impl Director {
    /// A word on the fight (`what`: target, weapon, hold_fire, fire, bold, careful, steady, auto),
    /// about `r` where it takes one, for `seconds` (its own default if None). What it says back;
    /// or why not.
    pub fn fight(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        what: &str,
        r: Option<&str>,
        seconds: Option<f64>,
    ) -> Result<String, String> {
        self.said_in_fight();
        let (tactic, stance_secs) = (self.clocks.tactic, self.clocks.stance);
        let hold = |default: f64, most: f64| {
            let secs = seconds.unwrap_or(default).clamp(1.0, most);
            (secs, obs.tic + (secs * 35.0) as i64)
        };
        let known = agent
            .player()
            .map(|p| p.known_monsters())
            .unwrap_or_default();
        match what {
            "target" => {
                let r = r.ok_or("target what: a monster's id or a kind (\"archvile\")")?;
                let id = self.resolve(r);
                let (aim, called) = if let Some(n) = id
                    .strip_prefix("monster-")
                    .and_then(|n| n.parse::<i32>().ok())
                {
                    let Some((_, name, ..)) = known.iter().find(|m| m.0 == n) else {
                        return Err(format!("No {id} known (dead, or never seen)."));
                    };
                    (Aim::One(n), format!("{id} ({})", called(name)))
                } else if let Some(kind) = monster_kind(r) {
                    if !known.iter().any(|m| m.1 == kind) {
                        return Err(format!("No {kind} known of right now."));
                    }
                    (Aim::Kind(kind), kind.to_string())
                } else {
                    return Err(format!("\"{r}\" isn't a monster or a kind of one."));
                };
                let (secs, until) = hold(tactic, TACTIC_MOST);
                let said = format!("Targeting {called} first, {secs:.0} s.");
                self.tactics.target = Some((aim, called, until));
                Ok(said)
            }
            "weapon" | "gun" => {
                let r = r.ok_or("weapon which: a gun's name, or auto")?;
                if r == "auto" {
                    self.tactics.gun = None;
                    return Ok("Your gun by reflex again.".into());
                }
                let gun = gun_named(r).ok_or_else(|| format!("\"{r}\" isn't a gun."))?;
                // (the fist only with a berserk pack this level: without, no gun worth the name)
                if gun == 0 && obs.berserk != Some(true) {
                    return Err("Not the fist without a berserk pack this level.".into());
                }
                if gun != 0 && !obs.owned.as_ref().is_some_and(|o| o.contains(&gun)) {
                    return Err(format!("You haven't got the {}.", gun_called(gun)));
                }
                let (secs, until) = hold(tactic, TACTIC_MOST);
                self.tactics.gun = Some((gun, until));
                Ok(format!(
                    "Using the {} while it has a shot, {secs:.0} s.",
                    gun_called(gun)
                ))
            }
            "retreat" | "cover" => {
                agent
                    .player_mut()
                    .ok_or("no fight to retreat from")?
                    .order_retreat(obs)
                    .map_err(|why| format!("Not retreating: {why}. Still fighting."))?;
                Ok("Falling back to cover.".into())
            }
            "hold_fire" => {
                let (secs, until) = hold(tactic, TACTIC_MOST);
                self.tactics.hold_fire = Some(until);
                Ok(format!("Holding fire, {secs:.0} s."))
            }
            "fire" => {
                self.tactics.hold_fire = None;
                Ok("Firing at will.".into())
            }
            "steady" => {
                self.tactics.stance = None;
                if let Some(p) = agent.player_mut() {
                    p.steady();
                }
                Ok("Fighting as usual.".into())
            }
            "auto" => {
                // (nothing of hers holding: said, so it isn't sent again and again for nothing)
                if !self.tactics.any(obs.tic) {
                    return Ok("Nothing of yours to end: the fight's by reflex already.".into());
                }
                self.tactics = Tactics::default();
                if let Some(p) = agent.player_mut() {
                    p.steady();
                }
                Ok("The fight's all by reflex again.".into())
            }
            other => {
                let (name, over) = stance(other).ok_or(
                    "fight: target REF, weapon NAME|auto, retreat, hold_fire, fire, bold, careful, avoid, steady, auto",
                )?;
                let (secs, until) = hold(stance_secs, STANCE_MOST);
                agent
                    .player_mut()
                    .ok_or("no fight to take a stance in")?
                    .take_stance(name, &over)?;
                self.tactics.stance = Some((name, until));
                Ok(format!("Fighting {name}, {secs:.0} s."))
            }
        }
    }

    /// Her say in the fight put to the fight each tic; each given up, and said so, when its time
    /// is up or its subject's gone.
    pub(super) fn apply_tactics(&mut self, agent: &mut Agent, obs: &Observation) {
        let pos = obs.pos();
        let known = agent
            .player()
            .map(|p| p.known_monsters())
            .unwrap_or_default();
        let mut over = Vec::new();
        let target = match &self.tactics.target {
            Some((_, called, until)) if obs.tic >= *until => {
                over.push(format!("targeting {called}"));
                self.tactics.target = None;
                None
            }
            Some((Aim::One(id), called, _)) => {
                if known.iter().any(|m| m.0 == *id) {
                    Some(*id)
                } else {
                    over.push(format!("targeting {called} (gone)"));
                    self.tactics.target = None;
                    None
                }
            }
            Some((Aim::Kind(kind), called, _)) => {
                let near = known
                    .iter()
                    .filter(|m| m.1 == *kind)
                    .min_by(|a, b| dist(pos, a.2).total_cmp(&dist(pos, b.2)))
                    .map(|m| m.0);
                if near.is_none() {
                    over.push(format!("targeting {called} (none left)"));
                    self.tactics.target = None;
                }
                near
            }
            None => None,
        };
        if self.tactics.gun.is_some_and(|(_, until)| obs.tic >= until) {
            over.push("the gun chosen".to_string());
            self.tactics.gun = None;
        }
        if self.tactics.hold_fire.is_some_and(|until| obs.tic >= until) {
            over.push("holding fire".to_string());
            self.tactics.hold_fire = None;
        }
        let stance_over = self
            .tactics
            .stance
            .is_some_and(|(_, until)| obs.tic >= until);
        if stance_over && let Some((name, _)) = self.tactics.stance.take() {
            over.push(format!("fighting {name}"));
        }
        if let Some(p) = agent.player_mut() {
            if stance_over {
                p.steady();
            }
            let c = p.combat_mut();
            c.forced_target = target;
            c.ordered_gun = self
                .tactics
                .gun
                .map(|(gun, _)| crate::dsda::weapon_slot(gun));
            c.ordered_weapon = self.tactics.gun.map(|(gun, _)| gun);
            c.ordered_hold_fire = self.tactics.hold_fire.is_some();
        }
        if !over.is_empty() {
            self.tell(
                obs,
                Event::TacticsOver,
                format!("Over: {}.", over.join("; ")),
            );
        }
    }
}
