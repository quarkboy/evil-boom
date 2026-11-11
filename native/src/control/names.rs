//! What things are called, to her: the names a Doom player knows them by ("mancubus", "box of
//! shells", "partial invisibility"), for everything written for her to read. Inside, a thing
//! goes by its class name (`dsda::type_name`: "Fatso", "ShellBox", "BlurSphere"), and the
//! tables that judge by kind are keyed by that.

/// (class name, what it's called, several of them)
const NAMES: [(&str, &str, &str); 61] = [
    ("Zombieman", "zombieman", "zombiemen"),
    ("ShotgunGuy", "shotgun guy", "shotgun guys"),
    ("ChaingunGuy", "chaingunner", "chaingunners"),
    ("DoomImp", "imp", "imps"),
    ("Demon", "demon", "demons"),
    ("Spectre", "spectre", "spectres"),
    ("LostSoul", "lost soul", "lost souls"),
    ("Cacodemon", "cacodemon", "cacodemons"),
    ("HellKnight", "hell knight", "hell knights"),
    ("BaronOfHell", "baron of Hell", "barons of Hell"),
    ("Arachnotron", "arachnotron", "arachnotrons"),
    ("PainElemental", "pain elemental", "pain elementals"),
    ("Revenant", "revenant", "revenants"),
    ("Fatso", "mancubus", "mancubi"),
    ("Archvile", "arch-vile", "arch-viles"),
    (
        "SpiderMastermind",
        "spider mastermind",
        "spider masterminds",
    ),
    ("Cyberdemon", "cyberdemon", "cyberdemons"),
    ("WolfensteinSS", "Wolfenstein SS", "Wolfenstein SS"),
    ("CommanderKeen", "Commander Keen", "Commander Keens"),
    ("BossBrain", "Icon of Sin", "Icons of Sin"),
    ("ExplosiveBarrel", "barrel", "barrels"),
    ("GreenArmor", "green armor", "green armors"),
    ("BlueArmor", "megaarmor", "megaarmors"),
    ("HealthBonus", "health bonus", "health bonuses"),
    ("ArmorBonus", "armor bonus", "armor bonuses"),
    ("BlueCard", "blue keycard", "blue keycards"),
    ("RedCard", "red keycard", "red keycards"),
    ("YellowCard", "yellow keycard", "yellow keycards"),
    ("BlueSkull", "blue skull key", "blue skull keys"),
    ("RedSkull", "red skull key", "red skull keys"),
    ("YellowSkull", "yellow skull key", "yellow skull keys"),
    ("Stimpack", "stimpack", "stimpacks"),
    ("Medikit", "medikit", "medikits"),
    ("Soulsphere", "soulsphere", "soulspheres"),
    ("Megasphere", "megasphere", "megaspheres"),
    (
        "InvulnerabilitySphere",
        "invulnerability",
        "invulnerabilities",
    ),
    ("Berserk", "berserk pack", "berserk packs"),
    (
        "BlurSphere",
        "partial invisibility",
        "partial invisibilities",
    ),
    ("RadSuit", "radiation suit", "radiation suits"),
    ("Allmap", "computer area map", "computer area maps"),
    (
        "Infrared",
        "light amplification visor",
        "light amplification visors",
    ),
    ("Clip", "clip", "clips"),
    ("ClipBox", "box of bullets", "boxes of bullets"),
    ("Shell", "shells", "shells"),
    ("ShellBox", "box of shells", "boxes of shells"),
    ("RocketAmmo", "rocket", "rockets"),
    ("RocketBox", "box of rockets", "boxes of rockets"),
    ("Cell", "energy cell", "energy cells"),
    ("CellPack", "energy cell pack", "energy cell packs"),
    ("Backpack", "backpack", "backpacks"),
    ("Chainsaw", "chainsaw", "chainsaws"),
    ("Shotgun", "shotgun", "shotguns"),
    ("SuperShotgun", "super shotgun", "super shotguns"),
    ("Chaingun", "chaingun", "chainguns"),
    ("RocketLauncher", "rocket launcher", "rocket launchers"),
    ("PlasmaRifle", "plasma rifle", "plasma rifles"),
    ("BFG9000", "BFG9000", "BFG9000s"),
    ("Rocket", "rocket", "rockets"),
    ("PlasmaBall", "plasma", "plasma"),
    ("BFGBall", "BFG blast", "BFG blasts"),
    ("DoomPlayer", "you", "you"),
];

/// What a thing's called, by its class name (one with no name here: as it is).
pub fn called(name: &str) -> &str {
    NAMES.iter().find(|n| n.0 == name).map_or(name, |n| n.1)
}

/// So many of a thing: "1 imp", "3 mancubi".
pub fn some(n: usize, name: &str) -> String {
    match NAMES.iter().find(|e| e.0 == name) {
        Some(e) if n == 1 => format!("1 {}", e.1),
        Some(e) => format!("{n} {}", e.2),
        None => format!("{n} {name}"),
    }
}

/// The one of its kind, at a sentence's start: "The cyberdemon".
pub fn the(name: &str) -> String {
    format!("The {}", called(name))
}

/// One of several of its kind, at a sentence's start: "A mancubus", "An arachnotron".
pub fn a(name: &str) -> String {
    let called = called(name);
    let an = called.starts_with(['a', 'e', 'i', 'o', 'u', 'A', 'E', 'I', 'O', 'U']);
    format!("{} {called}", if an { "An" } else { "A" })
}

#[cfg(test)]
mod tests {
    use super::{called, some, NAMES};
    use crate::dsda::type_name;

    /// Every monster and pickup the game has a class name for has a plain one.
    #[test]
    fn every_kind_is_named() {
        for name in (0..200).filter_map(type_name) {
            let kind = crate::tactical::bestiary::category(name);
            if matches!(
                kind,
                "Monster" | "Weapon" | "Ammo" | "Health" | "Armor" | "Key" | "Powerup"
            ) {
                assert!(NAMES.iter().any(|n| n.0 == name), "{name} ({kind})");
            }
        }
        assert_eq!(called("Fatso"), "mancubus");
        assert_eq!(some(2, "Fatso"), "2 mancubi");
        assert_eq!(some(1, "DoomImp"), "1 imp");
    }
}
