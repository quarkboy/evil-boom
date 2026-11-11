//! Powerups as she takes them, by the game's own word for each ("Invulnerability!",
//! "Supercharge!", ...): what it is and does, and for the ones that run out, a word before they
//! do and when they have (Doom's own times: invulnerability 30 s, invisibility and the suit 60 s,
//! the visor 120 s; another of the same starts its time again).

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

impl Director {
    /// Each tic: a powerup taken told; one running out told before it does, and when it has.
    pub(super) fn powers(&mut self, obs: &Observation) {
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
