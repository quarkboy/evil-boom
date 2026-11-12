// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What's what among the things in a level, by ZDoom class name: monsters (how big, how
//! tough, how they hurt you), missiles in flight (how fast, how wide), the player's weapons
//! and the items lying about.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attack {
    Hitscan,
    Missile,
    Melee,
    /// lost souls
    Charge,
    /// the arch-vile
    Fire,
    None,
}

#[derive(Clone, Copy, Debug)]
pub struct Monster {
    pub radius: f64,
    pub height: f64,
    pub health: i32,
    pub attack: Attack,
    /// also claws or bites up close
    pub melee: bool,
    /// how much it matters to deal with it first (damage a second, roughly)
    pub threat: f64,
    /// worth the big guns
    pub boss: bool,
}

const fn mon(
    radius: f64,
    height: f64,
    health: i32,
    attack: Attack,
    melee: bool,
    threat: f64,
) -> Monster {
    Monster {
        radius,
        height,
        health,
        attack,
        melee,
        threat,
        boss: false,
    }
}

pub fn monster(name: &str) -> Option<Monster> {
    use Attack as A;
    Some(match name {
        "Zombieman" => mon(20.0, 56.0, 20, A::Hitscan, false, 2.0),
        "ShotgunGuy" => mon(20.0, 56.0, 30, A::Hitscan, false, 4.0),
        "ChaingunGuy" => mon(20.0, 56.0, 70, A::Hitscan, false, 6.0),
        "WolfensteinSS" => mon(20.0, 56.0, 50, A::Hitscan, false, 4.0),
        "DoomImp" => mon(20.0, 56.0, 60, A::Missile, true, 2.0),
        "Demon" | "Spectre" => mon(30.0, 56.0, 150, A::Melee, false, 3.0),
        "LostSoul" => mon(16.0, 56.0, 100, A::Charge, false, 2.0),
        "Cacodemon" => mon(31.0, 56.0, 400, A::Missile, true, 3.0),
        "HellKnight" => mon(24.0, 64.0, 500, A::Missile, true, 4.0),
        "BaronOfHell" => mon(24.0, 64.0, 1000, A::Missile, true, 4.0),
        "Arachnotron" => mon(64.0, 64.0, 500, A::Missile, false, 6.0),
        "PainElemental" => mon(31.0, 56.0, 400, A::Missile, false, 4.0),
        "Revenant" => mon(20.0, 56.0, 300, A::Missile, true, 6.0),
        "Fatso" => mon(48.0, 64.0, 600, A::Missile, false, 7.0),
        "Archvile" => mon(20.0, 56.0, 700, A::Fire, false, 12.0),
        "Cyberdemon" => Monster {
            boss: true,
            ..mon(40.0, 110.0, 4000, A::Missile, false, 15.0)
        },
        "SpiderMastermind" => Monster {
            boss: true,
            ..mon(128.0, 100.0, 3000, A::Hitscan, false, 15.0)
        },
        "CommanderKeen" => mon(16.0, 72.0, 100, A::None, false, 0.0),
        _ => return None,
    })
}

/// The most one claw, bite or charge of it can take off her (p_enemy.c: the imp's and the lost
/// soul's 3-24, the demon's 4-40, the cacodemon's and the revenant's fist 10-60 and 6-60, the
/// baron's and the knight's 10-80); 0 for what doesn't hurt up close.
pub fn claw_max(name: &str) -> f64 {
    match name {
        "DoomImp" | "LostSoul" => 24.0,
        "Demon" | "Spectre" => 40.0,
        "Cacodemon" | "Revenant" => 60.0,
        "BaronOfHell" | "HellKnight" => 80.0,
        _ => 0.0,
    }
}

/// What a monster leaves where it dies (info.c's `droppeditem`: the zombies and the SS drop
/// what they carry, nothing else drops anything).
pub fn carries(name: &str) -> Option<&'static str> {
    match name {
        "Zombieman" | "WolfensteinSS" => Some("Clip"),
        "ShotgunGuy" => Some("Shotgun"),
        "ChaingunGuy" => Some("Chaingun"),
        _ => None,
    }
}

/// A hitscan shooter's wind-up: the tics from the first frame of its attack (turning to face
/// her, as she'd see it) to its first shot (info.c: the spider mastermind's 20 tics of
/// `A_FaceTarget`, then a burst every 4 for as long as it sees her; the zombies' 10).
pub fn first_shot(name: &str) -> Option<i32> {
    match name {
        "Zombieman" | "ShotgunGuy" | "ChaingunGuy" => Some(10),
        "WolfensteinSS" | "SpiderMastermind" => Some(20),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Missile {
    pub radius: f64,
    pub speed: f64,
    /// roughly, on a direct hit
    pub damage: f64,
    pub splash: bool,
    /// steers after her as it flies (A_Tracer): dodged, it comes round again, till it hits
    /// something
    pub homing: bool,
}

pub fn missile(name: &str) -> Option<Missile> {
    let m = |radius, speed, damage| Missile {
        radius,
        speed,
        damage,
        splash: false,
        homing: false,
    };
    Some(match name {
        "DoomImpBall" => m(6.0, 10.0, 12.0),
        "CacodemonBall" => m(6.0, 10.0, 20.0),
        "BaronBall" => m(6.0, 15.0, 32.0),
        "ArachnotronPlasma" | "PlasmaBall" => m(13.0, 25.0, 20.0),
        "FatShot" => m(6.0, 20.0, 32.0),
        "RevenantTracer" => Missile {
            homing: true,
            ..m(11.0, 10.0, 40.0)
        },
        "Rocket" => Missile {
            splash: true,
            ..m(11.0, 20.0, 80.0)
        },
        "BFGBall" => m(13.0, 25.0, 400.0),
        "SpawnShot" => m(6.0, 10.0, 0.0), // (the Icon of Sin's cubes)
        "LostSoul" => m(16.0, 20.0, 12.0), // (charging; it's a monster too)
        _ => return None,
    })
}

pub const TICS_PER_SECOND: f64 = 35.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weapon {
    pub slot: i32,
    pub ammo_per_shot: i32,
    /// a shot's, on average, every pellet hitting (a rocket's with its blast)
    pub damage: f64,
    /// tics from one shot to the next, holding the trigger
    pub cycle: f64,
    /// hitscan: pellets land up to this far off (degrees, most near the middle)
    pub spread: f64,
    /// ...and up and down, where the gun spreads that way too (the super shotgun's: its
    /// pellets' slope is off by up to (P_Random - P_Random) << 5, 7.1 degrees)
    pub rise: f64,
    pub hitscan: bool,
    /// its missile's, if not hitscan
    pub speed: f64,
    /// keep at least this far from what it hits
    pub splash: f64,
    /// no shooting at anything further (the spread wastes it)
    pub reach: f64,
    /// fighting with it: keep this far off (from, to)
    pub keep: (f64, f64),
    /// hits a shot lands, each a roll of the target's pain chance (the super shotgun's 20
    /// pellets; a rocket's hit and its blast)
    pub pellets: f64,
}

impl Weapon {
    const fn new(slot: i32, ammo_per_shot: i32, damage: f64, cycle: f64) -> Weapon {
        Weapon {
            slot,
            ammo_per_shot,
            damage,
            cycle,
            spread: 0.0,
            rise: 0.0,
            hitscan: true,
            speed: 0.0,
            splash: 0.0,
            reach: 2048.0,
            keep: (0.0, 2048.0),
            pellets: 1.0,
        }
    }

    /// Damage a second, most of it hitting (what she can do with it, roughly).
    /// Pain rolls a second on something this wide this far off (each hit's a chance to make it
    /// flinch out of what it's doing).
    pub fn flinch_rolls(&self, radius: f64, d: f64) -> f64 {
        self.pellets * self.hits(radius, d) * TICS_PER_SECOND / self.cycle
    }

    pub fn dps(&self) -> f64 {
        0.8 * self.damage * TICS_PER_SECOND / self.cycle
    }

    /// A shot's damage on m (the cyberdemon and the spider mastermind take nothing from a
    /// rocket's blast, only the hit).
    pub fn damage_on(&self, m: &Monster) -> f64 {
        if m.boss && self.slot == 5 {
            self.damage / 2.0
        } else {
            self.damage
        }
    }

    /// The share of a shot that lands on something this wide and this tall this far off: across
    /// (`hits`), and up and down where the gun spreads that way.
    pub fn lands(&self, radius: f64, height: f64, d: f64) -> f64 {
        let across = self.hits(radius, d);
        if !self.hitscan || self.rise == 0.0 {
            return across;
        }
        let a = (height / 2.0).atan2(d.max(1.0)).to_degrees();
        across
            * if a >= self.rise {
                1.0
            } else {
                1.0 - (1.0 - a / self.rise).powi(2)
            }
    }

    /// The share of a shot's damage that lands on something this wide this far off.
    pub fn hits(&self, radius: f64, d: f64) -> f64 {
        if !self.hitscan {
            // (it may step out of the way, the more so the longer it's in the air: twenty rockets
            // at cacodemons 1300 off, two hit)
            return if d < 768.0 {
                0.85
            } else {
                0.85 * (768.0 / d).powi(2)
            };
        }
        if self.spread == 0.0 {
            return 1.0;
        }
        let a = radius.atan2(d.max(1.0)).to_degrees();
        if a >= self.spread {
            1.0
        } else {
            1.0 - (1.0 - a / self.spread).powi(2)
        }
    }
}

/// By slot (slot 1's fist and chainsaw share, as do slot 3's shotguns: SUPER_SHOTGUN).
pub fn weapon(slot: i32) -> Weapon {
    let w = Weapon::new;
    match slot {
        1 => weapon_const(1),
        2 => Weapon {
            spread: 5.6,
            reach: 800.0,
            keep: (192.0, 640.0),
            ..w(2, 1, 10.0, 14.0)
        },
        3 => Weapon {
            spread: 5.6,
            reach: 640.0,
            keep: (128.0, 448.0),
            pellets: 7.0,
            ..w(3, 1, 70.0, 37.0)
        },
        4 => Weapon {
            spread: 5.6,
            reach: 1100.0,
            keep: (192.0, 768.0),
            ..w(4, 1, 10.0, 4.0)
        },
        5 => Weapon {
            hitscan: false,
            speed: 20.0,
            splash: 160.0,
            keep: (320.0, 1024.0),
            pellets: 2.0,
            ..w(5, 1, 180.0, 20.0)
        },
        6 => Weapon {
            hitscan: false,
            speed: 25.0,
            keep: (192.0, 768.0),
            ..w(6, 1, 22.0, 3.0)
        },
        _ => Weapon {
            hitscan: false,
            speed: 25.0,
            keep: (320.0, 1024.0),
            ..w(7, 40, 700.0, 60.0)
        },
    }
}

pub const SUPER_SHOTGUN: Weapon = Weapon {
    spread: 11.2,
    rise: 7.1,
    reach: 480.0,
    keep: (96.0, 320.0),
    pellets: 20.0,
    ..Weapon::new(3, 2, 200.0, 57.0)
};

/// The fist berserk: A_Punch's 2-20 times ten.
pub const BERSERK_FIST: Weapon = Weapon {
    damage: 110.0,
    ..weapon_const(1)
};

/// The chainsaw: A_Saw's 2-20 (berserk or not) a 4-tic frame, the trigger held.
pub const CHAINSAW: Weapon = Weapon {
    cycle: 4.0,
    ..weapon_const(1)
};

/// (the fist, as a constant to build the others on. Its reach is MELEERANGE from her centre to
/// the target's: the punch's trace starts at her centre and meets a body on its diagonal, which
/// runs through its centre (p_maputl.c PIT_AddThingIntercepts), so its radius adds nothing;
/// A_Saw's is one more)
const fn weapon_const(slot: i32) -> Weapon {
    Weapon {
        reach: 64.0,
        keep: (0.0, 64.0),
        ..Weapon::new(slot, 0, 11.0, 22.0)
    }
}

/// The most health a monster may have for her to go at it with her fists, berserk. A punch is
/// 110 on average; what she'd take meanwhile is about the punches it needs, by the chance it
/// isn't left flinching by each, by its swipe: an imp or a demon next to nothing, a revenant or a
/// cacodemon some 55-60 (3-4 punches, half or more flinching, 6-60 a swipe), a hell knight 160
/// and a baron 330 (5 and 9 punches, a flinch one in five, 10-80 a swipe); the mancubus,
/// arachnotron and arch-vile, 500 and more, fire at any distance.
const PUNCH_HEALTH: i32 = 400;

/// Would she go at it with her fists, berserk?
pub fn punchable(name: &str) -> bool {
    monster(name).is_some_and(|m| !m.boss && m.health <= PUNCH_HEALTH)
}

/// The most health a monster may have for her to go at it with the chainsaw. A hit every 4 tics,
/// 11 on average, each a roll of its pain chance: a demon (150, 180/256) takes some 14, and to
/// bite needs two in a row it doesn't flinch at (one pair in eleven); a cacodemon (400, 128/256)
/// takes 36, and gets its bites in.
const SAW_HEALTH: i32 = 150;

/// Is w a melee weapon she'd go up to t with (closing in, not keeping her distance): the fist
/// berserk at what it'd punch, the chainsaw at what it'd keep flinching?
pub fn melee_at(w: &Weapon, name: &str) -> bool {
    (*w == BERSERK_FIST && punchable(name))
        || (*w == CHAINSAW && monster(name).is_some_and(|m| !m.boss && m.health <= SAW_HEALTH))
}

/// What a slot brings up, as the game picks it for her (dsda/evil_api.c): slot 3 the super
/// shotgun once she has it; slot 1 the fist while she's berserk, else the chainsaw once she has
/// it.
pub fn weapon_in(slot: i32, obs: &crate::motor::Observation) -> Weapon {
    match slot {
        // (the plain shotgun up, the super had: slot 3 is the one in her hands)
        3 if obs.super_shotgun == Some(true) && obs.ready != Some(2) => SUPER_SHOTGUN,
        1 if obs.berserk == Some(true) => BERSERK_FIST,
        1 if obs.chainsaw == Some(true) => CHAINSAW,
        _ => weapon(slot),
    }
}

/// An item's category.
pub fn item(name: &str) -> Option<&'static str> {
    Some(match name {
        "Shotgun" | "SuperShotgun" | "Chaingun" | "RocketLauncher" | "PlasmaRifle" | "Chainsaw"
        | "BFG9000" => "Weapon",
        "Clip" | "ClipBox" | "Shell" | "ShellBox" | "RocketAmmo" | "RocketBox" | "Cell"
        | "CellPack" | "Backpack" => "Ammo",
        "Stimpack" | "Medikit" | "HealthBonus" | "Soulsphere" | "Megasphere" | "Berserk" => {
            "Health"
        }
        "GreenArmor" | "BlueArmor" | "ArmorBonus" => "Armor",
        "InvulnerabilitySphere" | "BlurSphere" | "RadSuit" | "Allmap" | "Infrared" => "Powerup",
        "BlueCard" | "YellowCard" | "RedCard" | "BlueSkull" | "YellowSkull" | "RedSkull" => "Key",
        _ => return None,
    })
}

/// Weapon pickups -> the slot they fill.
pub fn weapon_item(name: &str) -> Option<i32> {
    Some(match name {
        "Chainsaw" => 1,
        "Shotgun" | "SuperShotgun" => 3,
        "Chaingun" => 4,
        "RocketLauncher" => 5,
        "PlasmaRifle" => 6,
        "BFG9000" => 7,
        _ => return None,
    })
}

/// Ammo pickups -> (the slots it's for, how much).
/// The most of a weapon slot's ammo she can carry, without a backpack (twice it with one).
pub fn ammo_max(slot: i32) -> f64 {
    match slot {
        2 | 4 => 200.0,
        3 | 5 => 50.0,
        _ => 300.0,
    }
}

pub fn ammo_item(name: &str) -> Option<(&'static [i32], i32)> {
    Some(match name {
        "Clip" => (&[2, 4], 10),
        "ClipBox" => (&[2, 4], 50),
        "Shell" => (&[3], 4),
        "ShellBox" => (&[3], 20),
        "RocketAmmo" => (&[5], 1),
        "RocketBox" => (&[5], 5),
        "Cell" => (&[6, 7], 20),
        "CellPack" => (&[6, 7], 100),
        "Backpack" => (&[2, 3, 4, 5, 6, 7], 10),
        _ => return None,
    })
}

/// Health pickups -> (how much, up to what).
pub fn health_item(name: &str) -> Option<(i32, i32)> {
    Some(match name {
        "Stimpack" => (10, 100),
        "Medikit" => (25, 100),
        "HealthBonus" => (1, 200),
        "Soulsphere" => (100, 200),
        "Megasphere" => (200, 200),
        "Berserk" => (100, 100),
        _ => return None,
    })
}

/// Armor pickups -> (how much, up to what).
pub fn armor_item(name: &str) -> Option<(i32, i32)> {
    Some(match name {
        "GreenArmor" => (100, 100),
        "BlueArmor" | "Megasphere" => (200, 200),
        "ArmorBonus" => (1, 200),
        _ => return None,
    })
}

pub fn category(name: &str) -> &'static str {
    if monster(name).is_some() {
        return "Monster";
    }
    if missile(name).is_some() {
        return "Projectile";
    }
    item(name).unwrap_or("Other")
}
