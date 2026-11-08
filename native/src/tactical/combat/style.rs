//! How to fight with it: the distance she'd fight a monster from with a weapon (its window),
//! and how far it reaches.

use super::{mon, Combat, ENGAGE_RANGE};
use crate::motor::Thing;
use crate::tactical::bestiary::{Attack, Weapon};

/// something that shoots back from anywhere: shot at past a gun's reach while this much lands
const FAR_HITS: f64 = 0.2;

/// How near to t she'd fight it from with w, and how far: the gun's own (its spread, its blast)
/// and what t does up close (claws, bites; a cyberdemon's rockets too quick to dodge).
pub fn window(w: &Weapon, t: &Thing) -> (f64, f64) {
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
        if d < w.reach + if w.slot == 1 { info.radius } else { 0.0 } {
            return true;
        }
        info.attack == Attack::Hitscan && w.slot != 1 && w.hits(info.radius, d) >= FAR_HITS
    }
}
