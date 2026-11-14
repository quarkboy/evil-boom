// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Thing types that matter for getting about.

/// Solid decorations from the vanilla mobjinfo table (MF_SOLID, not shootable or a monster):
/// thing type -> collision radius. They block like a square box of that half-width, as the
/// player does. Hanging bodies are solid too (types 49-53 and 73-78; 59-63 are the
/// non-blocking copies). Explosive barrels are solid and shootable, and don't move.
pub fn solid_radius(kind: u16) -> Option<f64> {
    match kind {
        2035 => Some(10.0),      // explosive barrel
        54 => Some(32.0),        // large brown tree
        70 | 2028 | 85 | 86 | 48 // burning barrel, floor lamps, tall techno column
        | 30..=33 | 36 | 37 | 35 | 41 | 42 | 43 | 47 // pillars, candelabra, eye, rock, tree, stump
        | 44..=46 | 55..=57      // firesticks
        | 25..=29                // impaled bodies, skulls on poles, skull pile
        | 49..=53 | 73..=78 => Some(16.0), // hanging victims (solid)
        _ => None,
    }
}

/// Solid things that can be destroyed (shot): they don't block a way for good.
pub fn shootable_obstacle(kind: u16) -> bool {
    kind == 2035
}

pub const BARREL: u16 = 2035;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupKind {
    Key,
    Weapon,
    Ammo,
    Health,
    Armor,
    Powerup,
}

/// Things that can be picked up (Doom, Doom II): type -> (name, kind).
pub fn pickup(kind: u16) -> Option<(&'static str, PickupKind)> {
    use PickupKind as P;
    Some(match kind {
        5 => ("blue keycard", P::Key),
        6 => ("yellow keycard", P::Key),
        13 => ("red keycard", P::Key),
        38 => ("red skull key", P::Key),
        39 => ("yellow skull key", P::Key),
        40 => ("blue skull key", P::Key),
        2001 => ("shotgun", P::Weapon),
        82 => ("super shotgun", P::Weapon),
        2002 => ("chaingun", P::Weapon),
        2003 => ("rocket launcher", P::Weapon),
        2004 => ("plasma rifle", P::Weapon),
        2005 => ("chainsaw", P::Weapon),
        2006 => ("BFG9000", P::Weapon),
        2007 => ("clip", P::Ammo),
        2048 => ("box of bullets", P::Ammo),
        2008 => ("shells", P::Ammo),
        2049 => ("box of shells", P::Ammo),
        2010 => ("rocket", P::Ammo),
        2046 => ("box of rockets", P::Ammo),
        2047 => ("cell", P::Ammo),
        17 => ("cell pack", P::Ammo),
        8 => ("backpack", P::Ammo),
        2011 => ("stimpack", P::Health),
        2012 => ("medikit", P::Health),
        2014 => ("health bonus", P::Health),
        2013 => ("soulsphere", P::Health),
        83 => ("megasphere", P::Health),
        2018 => ("green armor", P::Armor),
        2019 => ("blue armor", P::Armor),
        2015 => ("armor bonus", P::Armor),
        2022 => ("invulnerability", P::Powerup),
        2023 => ("berserk", P::Powerup),
        2024 => ("partial invisibility", P::Powerup),
        2025 => ("radiation suit", P::Powerup),
        2026 => ("computer area map", P::Powerup),
        2045 => ("light amplification visor", P::Powerup),
        _ => return None,
    })
}
