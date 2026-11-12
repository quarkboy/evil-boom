// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Pickups as she takes them, by the game's own word for each ("Invulnerability!", "Picked up
//! the armor.", ...): the powerups, armor, the backpack, and ammo of a kind she had none of
//! (the user's: not health or armor bonuses, stimpacks, medikits, other ammo, nor a gun she has;
//! a new gun has its own line). A powerup: what it is and does, and for the ones that run out, a
//! word before they do and when they have (Doom's own times: invulnerability 30 s, invisibility
//! and the suit 60 s, the visor 120 s; another of the same starts its time again). Armor and a
//! backpack: what she has now.

use super::{Director, Event};
use crate::motor::Observation;

/// a power running out told this long (tics) before it does
const ENDING: i64 = 5 * 35;

/// The game's word for a powerup taken, what she's told, and how long it lasts (tics; None:
/// for good, or the level).
fn power(message: &str, obs: &Observation) -> Option<(&'static str, String, Option<i64>)> {
    let m = message.to_lowercase();
    let health = obs.health.unwrap_or(0.0);
    Some(if m.starts_with("invulnerability") {
        (
            "invulnerability",
            "You got invulnerability: nothing can hurt you for 30 s.".into(),
            Some(30 * 35),
        )
    } else if m.starts_with("supercharge") {
        (
            "soulsphere",
            format!("You got a soulsphere: health {health:.0}."),
            None,
        )
    } else if m.starts_with("megasphere") {
        (
            "megasphere",
            "You got a megasphere: health and armor 200.".into(),
            None,
        )
    } else if m.starts_with("berserk") {
        (
            "berserk",
            "You got a berserk pack: your fist hits ten times as hard till the level's end.".into(),
            None,
        )
    } else if m.starts_with("partial invisibility") {
        (
            "invisibility",
            "You got partial invisibility: monsters aim worse at you for 60 s.".into(),
            Some(60 * 35),
        )
    } else if m.starts_with("radiation shielding") {
        (
            "radiation suit",
            "You got a radiation suit: floors that hurt don't, for 60 s.".into(),
            Some(60 * 35),
        )
    } else if m.starts_with("computer area map") {
        (
            "map",
            "You got the computer map: the whole level's on your map.".into(),
            None,
        )
    } else if m.starts_with("light amplification") {
        (
            "visor",
            "You got a light amplification visor: everything lit for 120 s.".into(),
            Some(120 * 35),
        )
    } else {
        return None;
    })
}

/// The ammo pools by a gun's slot that uses each, and what its rounds are called.
const POOLS: [(i32, &str); 4] = [(2, "bullets"), (3, "shells"), (5, "rockets"), (6, "cells")];

/// How much of each pool she has.
fn pools(obs: &Observation) -> [i32; 4] {
    POOLS.map(|(slot, _)| {
        obs.ammo
            .iter()
            .flatten()
            .find(|(s, _)| *s == slot)
            .map_or(0, |(_, n)| *n)
    })
}

/// Armor, a backpack, or ammo of a kind she had none of (`had`, each pool before), taken (by the
/// game's word), as she's told it.
fn pickup(message: &str, obs: &Observation, had: [i32; 4]) -> Option<String> {
    let m = message.to_lowercase();
    let armor = obs.armor.unwrap_or(0.0);
    let thing = m.strip_prefix("picked up ")?.trim_end_matches(['.', '!']);
    Some(match thing {
        "the armor" => format!("You got green armor: armor {armor:.0}."),
        "the megaarmor" => format!("You got megaarmor: armor {armor:.0}."),
        t if t.starts_with("a backpack") => {
            "You got a backpack: you carry twice the ammo now.".to_string()
        }
        t => {
            let pool = if t.contains("bullets") || t == "a clip" {
                0
            } else if t.contains("shell") {
                1
            } else if t.contains("rocket") {
                2
            } else if t.contains("cell") {
                3
            } else {
                return None; // (health and armor bonuses, stimpacks, medikits, keys)
            };
            if had[pool] > 0 {
                return None; // (more of what she had: not worth a word)
            }
            format!(
                "You picked up {t}: {} {}, where you had none.",
                pools(obs)[pool],
                POOLS[pool].1
            )
        }
    })
}

impl Director {
    /// Each tic: a pickup told; a power running out told before it does, and when it has.
    pub(super) fn powers(&mut self, obs: &Observation) {
        let had = self.pools_had.unwrap_or_else(|| pools(obs));
        self.pools_had = Some(pools(obs));
        if let Some(said) = obs.message.as_deref().and_then(|m| pickup(m, obs, had)) {
            self.tell(obs, Event::Pickup, said);
        }
        if let Some((name, said, lasts)) = obs.message.as_deref().and_then(|m| power(m, obs)) {
            self.tell(obs, Event::Powerup, said);
            if let Some(t) = lasts {
                self.running.retain(|r| r.0 != name);
                self.running.push((name, obs.tic + t, false));
            }
        }
        let mut told = Vec::new();
        for r in &mut self.running {
            if !r.2 && obs.tic >= r.1 - ENDING {
                r.2 = true;
                let vital = matches!(r.0, "invulnerability" | "radiation suit");
                told.push((
                    Event::PowerEnding { vital },
                    format!("Your {} runs out in {} s.", r.0, ENDING / 35),
                ));
            }
            if obs.tic >= r.1 {
                told.push((Event::PowerOver, format!("Your {}'s worn off.", r.0)));
            }
        }
        self.running.retain(|r| obs.tic < r.1);
        for (e, t) in told {
            self.tell(obs, e, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pickup, power};
    use crate::motor::Observation;

    /// The game's pickup words: armor, the backpack, the powerups, and ammo she had none of told;
    /// the rest left out.
    #[test]
    fn pickups_told() {
        let obs = Observation {
            armor: Some(100.0),
            ammo: Some(vec![(2, 20), (3, 8)]),
            ..Observation::default()
        };
        let (none, some) = ([0; 4], [10, 4, 0, 0]);
        assert_eq!(
            pickup("Picked up the armor.", &obs, some).as_deref(),
            Some("You got green armor: armor 100.")
        );
        assert!(pickup("Picked up a backpack full of ammo!", &obs, some).is_some());
        assert!(power("Berserk!", &obs).is_some());
        assert_eq!(
            pickup("Picked up 4 shotgun shells.", &obs, none).as_deref(),
            Some("You picked up 4 shotgun shells: 8 shells, where you had none.")
        );
        for left in [
            "Picked up a health bonus.",
            "Picked up an armor bonus.",
            "Picked up a stimpack.",
            "Picked up a medikit that you REALLY need!",
            "Picked up a medikit.",
            "Picked up a clip.",
            "Picked up a box of shotgun shells.",
            "Picked up a blue keycard.",
        ] {
            assert!(
                pickup(left, &obs, some).is_none() && power(left, &obs).is_none(),
                "{left}"
            );
        }
    }
}
