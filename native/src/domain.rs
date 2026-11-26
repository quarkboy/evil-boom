// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What every part of her shares: a tic's observation (what she sees and hears of the game)
//! and the controls she gives back, the things and sounds in it, and the game's own
//! constants of time and movement. A leaf: it depends on the map's and the mesh's types and
//! on nothing of hers, so the knowledge, the fight and the follower can all read it without
//! reading each other.

use crate::geom::Point;
use crate::nav::Heights;
use crate::specials::Keys;

pub const TICRATE: f64 = 35.0;
/// forwardmove 50 << 11, per tic (vanilla)
pub const RUN_THRUST: f64 = 1.5625;
/// ORIG_FRICTION, on the ground only
pub const FRICTION: f64 = 0.90625;
/// forward/strafe input magnitude: plain running, no straferun boost
pub const MAX_MOVE: f64 = 50.0;
/// ~16 units a tic, with a little in reserve
pub const FULL_SPEED: f64 = RUN_THRUST * 0.97 / (1.0 - FRICTION);

/// What a sound is, as anyone who's played knows it by ear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// a monster's cry on seeing her
    Sight,
    /// a monster attacking: a gun, a claw, a wind-up
    Attack,
    Pain,
    Death,
    /// a monster about, awake
    Active,
    /// a monster's own steps, for the few that make any (a cyberdemon's hoof and metal, a
    /// spider mastermind's legs, an arachnotron's): one a step, as it walks
    Walking,
    /// a missile fired
    Launch,
    /// a missile going off
    Impact,
    /// anything else: a door, a lift, a switch, her own
    Other,
}

/// A sound the game started this tic, as she hears it: Doom's rule (within 1200 units, walls or
/// none; louder the nearer; its side by which ear, never front or back).
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    /// the sound's name in the game ("dspistol", "dsfirsht", "dspodth1", "dsdorcls", ...)
    pub name: &'static str,
    /// what it is
    pub what: Heard,
    /// the kinds of thing that make this sound for that (several share some: the three former
    /// humans' death, the fireball of an imp, a cacodemon and a baron); none for `Other`
    pub kinds: &'static [&'static str],
    /// hers: her own gun, her own grunt, a pickup
    pub own: bool,
    /// 1-127: 127 within 160 units, falling to nothing at 1200
    pub volume: i32,
    /// 0-255, 128 the middle: less to her left, more to her right
    pub separation: i32,
    /// what made it, if a thing did (its id and class name), and where it came from: the game's
    /// own, for the log and for checking; not something heard
    pub from: Option<(i32, &'static str)>,
    pub at: Point,
}

/// Something in the level as it is now: a monster, a missile in flight, an item.
#[derive(Clone, Debug, PartialEq)]
pub struct Thing {
    pub id: i32,
    /// its class name (ZDoom's: "DoomImp", "BaronBall", "Shotgun", ...)
    pub name: &'static str,
    /// Monster, Projectile, Weapon, Ammo, Health, Armor, Key, Powerup, ...
    pub category: &'static str,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub angle: f64,
    pub health: i32,
    /// a missile: the type of what fired it (0 the player; -1 none)
    pub owner: i32,
    /// its state now (what it's doing: an arch-vile starting a fire, a cyberdemon aiming)
    pub state: i32,
    /// a monster: it's after her
    pub after_her: bool,
    /// a monster: tics into its attack, from its first frame (as she'd see it winding up: an
    /// arch-vile raising its hands, a cyberdemon aiming); -1 not attacking
    pub attack: i32,
}

/// What she sees of the game a tic.
#[derive(Clone, Debug, Default)]
pub struct Observation {
    pub x: f64,
    pub y: f64,
    /// degrees, 0 = east, anticlockwise
    pub angle: f64,
    /// per sector: (floor, ceiling) now
    pub heights: Vec<Heights>,
    pub tic: i64,
    pub keys: Option<Keys>,
    /// her feet's height
    pub z: Option<f64>,
    /// the weapon slot held
    pub weapon: Option<i32>,
    /// the weapon held, as the game numbers them (0 fist, 1 pistol, 2 shotgun, 3 chaingun,
    /// 4 rocket launcher, 5 plasma rifle, 6 BFG, 7 chainsaw, 8 super shotgun): two share a slot
    /// twice over, and which of them is up only this says
    pub ready: Option<i32>,
    /// the weapons she has, as the game numbers them
    pub owned: Option<Vec<i32>>,
    /// where explosive barrels are now
    pub barrels: Option<Vec<Point>>,
    /// those of them going off: hit, blowing up in their death frames (15 tics), still there
    pub barrels_lit: Vec<Point>,
    pub health: Option<f64>,
    /// radiation suits picked up so far
    pub suits: Option<i32>,
    /// what's about now
    pub things: Option<Vec<Thing>>,
    /// weapon slots held (1-7)
    pub weapons: Option<Vec<i32>>,
    /// weapon slot -> ammo for it
    pub ammo: Option<Vec<(i32, i32)>>,
    pub armor: Option<f64>,
    /// 1 green (takes a third of each hit), 2 blue (half); 0 none
    pub armor_type: Option<i32>,
    /// has the super shotgun (slot 3 then brings it up)
    pub super_shotgun: Option<bool>,
    /// has the chainsaw (slot 1 then brings it up, unless she's berserk)
    pub chainsaw: Option<bool>,
    /// berserk (till the level's end: the fist's punch ten times as hard)
    pub berserk: Option<bool>,
    /// her (x, y) a tic
    pub velocity: Option<Point>,
    /// has the computer area map
    pub allmap: Option<bool>,
    /// has a backpack (twice the ammo carried)
    pub backpack: Option<bool>,
    /// what the game just told her
    pub message: Option<String>,
    /// the sounds started since she last looked
    pub sounds: Vec<Sound>,
    /// each hit on her since she last looked that something dealt: which way the status bar's
    /// face looked for it (-1 her left, 0 straight at it, 1 her right; behind is a side)
    pub hit_sides: Vec<i32>,
    /// the tic of the last such hit, if she's had one
    pub hit_tic: Option<i64>,
    /// in a co-op game, the other players (as the automap shows them all to each): none alone
    pub mates: Vec<Mate>,
    /// in a co-op game, which player she is (0 the first; it settles who gives way)
    pub me: usize,
}

/// Another player in a co-op game, as she knows of them (the automap shows every player).
#[derive(Clone, Copy, Debug, Default)]
pub struct Mate {
    /// which player (0 the first)
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// degrees, 0 = east, anticlockwise
    pub angle: f64,
    /// moving, per tic
    pub vx: f64,
    pub vy: f64,
    pub alive: bool,
}

impl Mate {
    pub fn pos(&self) -> Point {
        (self.x, self.y)
    }
}

impl Observation {
    pub fn pos(&self) -> Point {
        (self.x, self.y)
    }

    /// The health `raw` damage costs her through her armor: green takes a third of each hit,
    /// blue half, while it lasts (vanilla `P_DamageMobj`; a hurting floor's hits too).
    pub fn through_armor(&self, raw: f64) -> f64 {
        let share = match self.armor_type {
            Some(1) => 1.0 / 3.0,
            Some(2) => 0.5,
            _ => 0.0,
        };
        raw - (raw * share).min(self.armor.unwrap_or(0.0))
    }

    pub fn height(&self, s: usize) -> Option<Heights> {
        self.heights.get(s).copied()
    }
}

/// A tic's controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    /// -50..50
    pub forward: f64,
    /// positive: right
    pub strafe: f64,
    /// degrees, positive: right (clockwise)
    pub turn: f64,
    pub use_: bool,
    pub attack: bool,
    pub run: bool,
    /// switch to the weapon in this slot (1-7; 0: don't)
    pub weapon: i32,
    /// the very gun the press is for (the game's number: the plain shotgun 2, the super 8, the
    /// fist 0, the chainsaw 7), not the slot's best one
    pub exact: Option<i32>,
}

impl Default for Controls {
    fn default() -> Self {
        Controls {
            forward: 0.0,
            strafe: 0.0,
            turn: 0.0,
            use_: false,
            attack: false,
            run: true,
            weapon: 0,
            exact: None,
        }
    }
}

/// (deg + 180) mod 360 - 180, as Python's % has it.
pub fn norm(deg: f64) -> f64 {
    (deg + 180.0).rem_euclid(360.0) - 180.0
}

/// The direction from a to b, degrees.
pub fn heading(a: Point, b: Point) -> f64 {
    (b.1 - a.1).atan2(b.0 - a.0).to_degrees()
}

/// degrees a tic she turns running across sinking floors
pub(crate) const RUN_TURN: f64 = 10.0;

/// Turning, roughly a keyboard player's (vanilla's fast keyboard turn is 7 degrees a tic).
pub(crate) const MAX_TURN: f64 = 8.0;
pub(crate) const TURN_ACCEL: f64 = 1.0;

/// Tics to go `d` from `v0` units a tic (negative: going the other way), running straight on
/// (thrust and friction as Doom has them), at `pace` of it.
pub fn run_tics(v0: f64, d: f64, pace: f64) -> f64 {
    let (mut v, mut x, mut k) = (v0, 0.0, 0.0);
    while x < d && k < 400.0 {
        v = (v + RUN_THRUST) * FRICTION;
        x += v * pace;
        k += 1.0;
    }
    k
}

/// (hers, not the game's, but read at every level of her: the follower's, the explorer's, the
/// fight's)
/// the share of her health something on the way may cost her and still not be a risk (the
/// user's: "10% or less of current health = not risky"): floors that hurt, a fast crusher's stroke
pub const RISK_SHARE: f64 = 0.1;

/// A barrel, as the game has it (`mobjinfo`: radius 10, height 42), and its blast (128, as a
/// rocket's: `P_RadiusAttack`, measured by the greater of the gaps along x and y).
pub const BARREL_RADIUS: f64 = 10.0;
/// (what a shot at one has to get to; seen over a ledge this high or not)
pub const BARREL_HEIGHT: f64 = 42.0;
/// a barrel blown up sets off another this near, by that measure (its blast, 128, and the
/// other's radius)
pub const BARREL_CHAIN: f64 = 138.0;
/// ...and is clear of her this far off (its 128 and her radius, and a little): where she
/// backs off to before shooting one
pub const BARREL_SAFE: f64 = 150.0;
