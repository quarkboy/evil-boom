//! What the fight keeps from tic to tic, by what it's for: each a part of `Combat`.

use super::super::sound::Hearing;
use super::moves::HeldFire;
use super::Alert;
use crate::geom::Point;
use crate::motor::Thing;
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
    pub(super) since: i64,
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
    /// when she last looked about as she walked (`senses.glance_every`)
    pub(super) glanced: i64,
    /// when she last looked round for one she remembers (`senses.check_every`)
    pub(super) checked: i64,
    /// when each way round her (the steering's sixteen) was last in her view
    pub(super) looked: [i64; 16],
}
