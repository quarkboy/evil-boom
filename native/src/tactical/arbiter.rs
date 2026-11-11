//! Who has her legs this tic. Three would walk her: the explorer's route, a retreat's route, and
//! the fight's move; only one does, and the others go on over it where they can (the fight
//! still aims and fires over a route's legs). This is the one place that says which.
//!
//! As it stands (the order the code had grown, said in one place; not yet a considered rule):
//!
//! 1. Falling back, a dodge has the legs only with a way clear of the hit; a dodge with nowhere
//!    to step leaves them to the retreat (stood on a ledge every step off it a drop, fireballs
//!    from all round, she stood dodging nowhere, the way back out never walked).
//! 2. Any other move of the fight's that wants the legs has them, falling back or not: the
//!    retreat's way is then only where the fight's steering leans (`Combat::toward`).
//! 3. Else a retreat under way has them.
//! 4. Else the explorer's route.
//!
//! What a move wants is `Decision::wants_legs`: not if the node said the route keeps them; in a
//! jump or a timed crossing (`busy`) only pressed (a missile coming, an arch-vile's fire); else
//! unless it's one of the moves the route goes on through (`Move::route_goes_on`).

use super::combat::{Decision, Move};

/// Whose legs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legs {
    /// the fight's move
    Fight,
    /// a retreat's route (the fight aiming and firing over it)
    Retreat,
    /// the explorer's route (the fight, if there's one, aiming and firing over it)
    Route,
}

/// Who has her legs: `decision` the fight's this tic, `busy` in a jump or a timed crossing,
/// `retreating` a retreat under way, `dodge_clear` the dodge's way is clear of the hit.
pub fn arbitrate(
    decision: Option<&Decision>,
    busy: bool,
    retreating: bool,
    dodge_clear: bool,
) -> Legs {
    let wants = decision.is_some_and(|d| d.wants_legs(busy));
    match decision {
        Some(d) if retreating && d.kind == Move::Dodging => {
            if d.moving.is_some() && dodge_clear {
                Legs::Fight
            } else {
                Legs::Retreat
            }
        }
        _ if wants => Legs::Fight,
        _ if retreating => Legs::Retreat,
        _ => Legs::Route,
    }
}
