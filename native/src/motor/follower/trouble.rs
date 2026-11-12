// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Why a route was given up: the follower's own account (exactly as its log has it, for
//! whoever traces a run) and the same in a few plain words for whoever's playing, with no
//! sectors, lines or coordinates in it.

use std::fmt;

#[derive(Clone, Debug)]
pub enum Trouble {
    /// pushed off the way, and none back to it
    OffCourse,
    Stuck,
    /// a drop ahead the plan didn't mean to take, and no way on without one like it
    Drop(String),
    /// a crusher stopped too low to get under (sector, the gap)
    CrusherLow(usize, f64),
    /// ...too low to get through or to set off from its edge
    CrusherStuck(usize),
    /// set off from its edge, and it didn't start
    CrusherNoStart(usize),
    /// a barrel in the way and nothing to shoot it with
    BarrelNoGun,
    /// ...nowhere to shoot it from and get out of the blasts
    BarrelNoPost,
    /// ...and no getting to the place to shoot it from
    BarrelNoReach,
    /// a door or lift closed on the way that didn't open again (sector)
    NoReopen(usize),
    /// a lift to jump off that didn't come (sector)
    LiftNotComing(usize),
    /// waited too long for a mover (sector, how it was and how its neighbour was)
    MoverSlow(usize, String, String),
    /// couldn't turn to face a line to press
    CantFace(usize),
    TakeoffLow,
    RanOffLowering,
    Sinking,
    JumpBlocked,
    JumpShort,
    /// a step of the route taking too long (its number)
    StepSlow(usize),
}

impl fmt::Display for Trouble {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trouble::OffCourse => write!(f, "off course and no way back"),
            Trouble::Stuck => write!(f, "stuck"),
            Trouble::Drop(why) => write!(f, "{why}"),
            Trouble::CrusherLow(s, gap) => write!(
                f,
                "the crusher (sector {s}) has stopped too low to get under ({gap:.0} up)"
            ),
            Trouble::CrusherStuck(s) => write!(
                f,
                "the crusher (sector {s}) has stopped too low to get through or set off from its edge"
            ),
            Trouble::CrusherNoStart(s) => {
                write!(f, "the crusher (sector {s}) didn't start from its edge")
            }
            Trouble::BarrelNoGun => write!(f, "a barrel in the way, and nothing to shoot it with"),
            Trouble::BarrelNoPost => write!(
                f,
                "a barrel in the way, nowhere to shoot it from and run off out of the blasts"
            ),
            Trouble::BarrelNoReach => write!(
                f,
                "a barrel in the way, and no getting to somewhere to shoot it from"
            ),
            Trouble::NoReopen(s) => write!(f, "waited too long for sector {s} to reopen"),
            Trouble::LiftNotComing(s) => write!(f, "sector {s} not coming for the jump off it"),
            Trouble::MoverSlow(s, from, to) => {
                write!(f, "waited too long for sector {s}: live {from} -> {to}")
            }
            Trouble::CantFace(li) => write!(f, "could not face line {li}"),
            Trouble::TakeoffLow => write!(f, "jump takeoff too low and no other way"),
            Trouble::RanOffLowering => write!(
                f,
                "ran off a floor going down: what she was going for is for choosing again"
            ),
            Trouble::Sinking => write!(f, "stranded on a sinking floor"),
            Trouble::JumpBlocked => write!(f, "run-jump blocked by a monster"),
            Trouble::JumpShort => write!(f, "run-jump knocked short of its speed, time after time"),
            Trouble::StepSlow(n) => write!(f, "step {n} taking too long"),
        }
    }
}

impl Trouble {
    /// In a few words for whoever's playing (it follows "couldn't get there: ").
    pub fn plain(&self) -> &'static str {
        match self {
            Trouble::OffCourse => "knocked off the way, and no way back to it",
            Trouble::Stuck => "stuck",
            Trouble::Drop(_) => "the only way on is a drop you didn't mean to take",
            Trouble::CrusherLow(..) | Trouble::CrusherStuck(_) => {
                "a crusher stopped too low to get past"
            }
            Trouble::CrusherNoStart(_) => "a crusher that wouldn't start",
            Trouble::BarrelNoGun => "a barrel in the way, and nothing to shoot it with",
            Trouble::BarrelNoPost | Trouble::BarrelNoReach => {
                "a barrel in the way, and nowhere safe to shoot it from"
            }
            Trouble::NoReopen(_) => "a door or lift on the way didn't open again",
            Trouble::LiftNotComing(_) => "a lift on the way didn't come",
            Trouble::MoverSlow(..) => "waited too long for a door or lift",
            Trouble::CantFace(_) => "couldn't line up to press it",
            Trouble::TakeoffLow | Trouble::JumpShort => "couldn't make a jump on the way",
            Trouble::RanOffLowering | Trouble::Sinking => "a floor on the way went down",
            Trouble::JumpBlocked => "a monster in the way of a jump",
            Trouble::StepSlow(_) => "took too long",
        }
    }
}
