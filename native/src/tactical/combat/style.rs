//! How to fight with it: the style a weapon's fought with at a monster (up close or from a
//! distance), the distance she'd fight it from (its window), and how far the weapon reaches.

use super::{mon, Combat, ENGAGE_RANGE};
use crate::motor::Thing;
use crate::tactical::bestiary::{melee_at, Attack, Weapon};

/// How she fights a monster with a weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// straight up to it, then standing at it hitting: the fist berserk at what it'd punch, the
    /// chainsaw at what it'd keep flinching (`bestiary::melee_at`)
    Melee,
    /// from the gun's window, the steering keeping her there (and the fist or chainsaw at what's
    /// come right up to her)
    Ranged,
}

impl Style {
    pub fn of(w: &Weapon, t: &Thing) -> Style {
        if melee_at(w, t.name) {
            Style::Melee
        } else {
            Style::Ranged
        }
    }
}

/// something that shoots back from anywhere: shot at past a gun's reach while this much lands
const FAR_HITS: f64 = 0.2;

/// How near to t she'd fight it from with w, and how far: the gun's own (its spread, its blast)
/// and what t does up close (claws, bites; a cyberdemon's rockets too quick to dodge). Up close
/// (`Style::Melee`): right up to it, whatever its claws.
pub fn window(w: &Weapon, t: &Thing) -> (f64, f64) {
    if Style::of(w, t) == Style::Melee {
        return w.keep;
    }
    let s = standoff(t.name);
    (w.keep.0.max(s), w.keep.1.max(s + 256.0))
}

/// The nearest she'd fight a monster from, whatever the gun.
pub fn standoff(name: &str) -> f64 {
    match name {
        "Cyberdemon" | "SpiderMastermind" => 640.0,
        "Fatso" => 400.0,
        "Arachnotron" | "Revenant" | "PainElemental" => 256.0,
        "Demon" | "Spectre" | "HellKnight" | "BaronOfHell" | "Cacodemon" => 160.0,
        "DoomImp" | "LostSoul" => 128.0,
        _ => 0.0,
    }
}

impl Combat {
    /// Close enough to shoot at with w: inside its reach, or (at something that hurts her from
    /// anywhere) as far as enough of a shot still lands.
    pub fn in_reach(w: &Weapon, t: &Thing, d: f64) -> bool {
        let info = mon(t.name);
        if d >= ENGAGE_RANGE {
            return false;
        }
        if d < w.reach {
            return true;
        }
        info.attack == Attack::Hitscan && w.slot != 1 && w.hits(info.radius, d) >= FAR_HITS
    }
}
