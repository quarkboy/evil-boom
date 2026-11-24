// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What the fight keeps from tic to tic, by what it's for: each a part of `Combat`.

use super::super::sound::Hearing;
use super::moves::HeldFire;
use super::Alert;
use crate::domain::Thing;
use crate::geom::Point;
use crate::specials::CrusherKind;
use std::collections::{BTreeMap, BTreeSet};

/// Floors and ceilings on the move, as she's seen them go, and which of them crush.
pub struct Movers {
    /// each sector's ceiling last tic, and when it was last seen coming down
    pub(super) ceilings: Vec<f64>,
    /// each sector's floor last tic, and when it was last seen coming up
    pub(super) floors: Vec<f64>,
    pub(super) floor_up: Vec<i64>,
    pub(super) ceiling_down: Vec<i64>,
    /// crusher sectors she knows of, by kind (their lines learned; kept from what she knows by
    /// the player)
    pub crushers: BTreeMap<usize, CrusherKind>,
    /// per crusher sector: tics to be out of it at the worst (`out_tics`)
    pub(super) crusher_out: BTreeMap<usize, f64>,
    /// ...and last came down lower than she stands (a crusher at work, between strokes too)
    pub(super) crushed_low: Vec<i64>,
    /// sectors only doors move: a door closing on her goes back up, or rests on her, and does
    /// no damage (vanilla T_VerticalDoor), so its ceiling coming down is no crusher
    pub(super) doors: BTreeSet<usize>,
    /// sectors only lifts move: a lift coming up on her goes back down (vanilla T_PlatRaise
    /// reverses a plat that doesn't crush, and none of the lifts do), so its floor coming up is no
    /// crusher either
    pub(super) lifts: BTreeSet<usize>,
    /// floors a walked-over line lowers into something that hurts (going down: as good as hurting)
    pub(super) sinks_into_harm: BTreeSet<usize>,
}

/// The gun's timing: when the trigger was down, when a shot left, what her legs wait for.
pub struct Trigger {
    /// the target she's shooting at with a missile gun: its id, its health, the gun's slot and
    /// her rounds for it, and where it stood, as when she began, it was last hurt, or it moved
    pub(super) shot_at: Option<(i32, i32, i32, i32, crate::geom::Point, i64)>,
    /// no more fired at this one from where she stood then, till this tic: her rounds weren't
    /// getting there (`aim.wasted`)
    pub(super) no_effect: Option<(i32, crate::geom::Point, i64)>,
    /// a shotgun's shots at one standing still, since she began at it: it, its health then,
    /// her shells then, where it stood (`aim_and_fire`, the pellets that aren't getting there)
    pub(super) spread_at: Option<(i32, i32, i32, crate::geom::Point)>,
    /// the target her plasma was last reckoned not to reach, and when
    pub(super) blocked_at: Option<(i32, i64)>,
    /// her rounds for the gun in hand, by the tic, a few seconds of it (for which of them have
    /// had time to land)
    pub(super) ammo_log: std::collections::VecDeque<(i64, i32, i32)>,
    /// the trigger was down last tic
    pub(super) held: bool,
    /// when a bullet or a shell last left the gun (by its ammo's count), and the slot's
    pub(super) shot_out: Option<(i32, i64)>,
    /// since when her legs have been held for a shot on its way out
    pub(super) line_held: Option<i64>,
    /// her legs her own till then, after a chaingun's burst they waited out
    pub(super) burst_rest: i64,
    /// when the first bullet of the press she's in left the gun (the chaingun's second goes 4 on)
    pub(super) burst_from: i64,
    /// the gun last fired and when (its reload)
    pub(super) last_shot: Option<(i32, i64)>,
    /// the way she faced as she last pressed the rocket launcher's trigger (the rocket leaves along
    /// the way she faces 8 tics later)
    pub(super) rocket_facing: Option<f64>,
    pub(super) held_fire: Option<HeldFire>,
}

/// The missiles she's getting out of the way of, and the way.
pub struct Dodging {
    /// missiles being dodged (till they've gone by)
    pub(super) missiles: BTreeSet<i32>,
    /// homing missiles being dodged: since when; and where out of the sight of the one being
    /// shaken off she's going
    pub(super) homing_since: BTreeMap<i32, i64>,
    pub(super) shake_spot: Option<(i32, Option<Point>)>,
    pub(super) way: Option<Point>,
    /// does that way take her out of every missile's way (its hit; a blast may still reach her)?
    pub(super) clear: bool,
}

/// Her stepping about a target: the way last taken, and what came of it.
pub struct Stepping {
    /// the way fighting moved her last
    pub(super) way: Option<Point>,
    /// where she took on the target she's stepping about (it, there, the last tic of it)
    pub(super) from: Option<(i32, Point, i64)>,
    pub(super) tic: i64,
    pub(super) still: i64,
    /// a way she got nowhere pushing, and till when it's left be
    pub(super) stuck: Option<(Point, i64)>,
    /// when she last turned about (took a way more or less against the one she was on)
    pub(super) turned_about: i64,
    /// the way the fight's legs last went, and when; and when they last turned about
    pub(super) legs_way: Option<(Point, i64)>,
    pub(super) legs_turned: i64,
    /// stepped for a shot at these for long with them none the worse: each not stepped for
    /// again till its tic
    pub(super) no_step: BTreeMap<i32, (i64, u32)>,
    /// the spot she's stepping to for plasma to get to this one (`stepping_for_plasma`)
    pub(super) plasma_to: Option<(i32, Point)>,
    /// backed off from these for long with them none the worse: each not backed off from
    /// again till its tic. (Each, not the last only: with a pit of demons under her ledge
    /// she left one, backed off from the next for its twenty seconds, and by then the first
    /// was hers to back off from again; ten minutes at the ledge.)
    /// With the tic, how many times it's been left so: twice as long each time (`left_till`).
    pub(super) no_back: BTreeMap<i32, (i64, u32)>,
    /// kept off from these for long with them none the worse: each not kept off from again
    /// till its tic (and how many times, as `no_back`)
    pub(super) no_keep: BTreeMap<i32, (i64, u32)>,
}

/// Getting out of fire there's no dodging: an arch-vile's, a hitscan shooter's.
pub struct Cover {
    /// running from an arch-vile's fire: when the way was chosen, from which, the way
    pub(super) vile_way: Option<(i64, i32, Point)>,
    /// ...and till when its fire might still go off
    pub(super) vile_until: i64,
    /// ducking from a hitscan shooter's fire: from which, to where
    pub(super) duck_way: Option<(i32, &'static str, Point)>,
    /// ...and till when it might still be firing
    pub(super) duck_until: i64,
    /// when there was last nowhere to duck from one
    pub(super) no_duck_tic: i64,
}

/// Keeping off the barrels by her while there's shooting.
pub struct OffBarrels {
    pub(super) way: Option<Point>,
    /// when this bout of it began: the stepping away and the coming back between
    pub(super) since: i64,
    /// when she was last stepping away
    pub(super) last: i64,
    pub(super) rest_until: i64,
}

/// The works of her senses: the game's monsters they read, what's been heard, when she last looked where.
pub struct Senses {
    /// every monster in the level as it is now: her senses' only (`observe` makes what she
    /// sees and hears of them out of it); nothing decides by it
    pub(super) game_monsters: BTreeMap<i32, Thing>,
    pub(super) hearing: Hearing,
    /// deaths she's seen whose cries are still to come: (when, the kind)
    pub(super) cries_due: Vec<(i64, &'static str)>,
    /// attacks heard lately: (when, the kinds that sound like it)
    pub(super) attacks_heard: Vec<(i64, &'static [&'static str])>,
    /// something to turn and look at: a sound from out of her view, a hit, a bump
    pub(super) alert: Option<Alert>,
    /// hit from a side with nothing she can see there: away from it (the way, a unit) till
    /// this tic, before whatever she was doing
    pub(super) flinch: Option<(i64, crate::geom::Point)>,
    /// when she last looked about as she walked (`senses.glance_every`)
    pub(super) glanced: i64,
    /// when she last looked round for one she remembers (`senses.check_every`)
    pub(super) checked: i64,
    /// when each way round her (the steering's sixteen) was last in her view
    pub(super) looked: [i64; 16],
}
