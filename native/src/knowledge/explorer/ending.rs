//! Why an objective ended short of done (or done some other way than doing it): as the log has
//! always said it, and plainly for whoever's playing.

use crate::motor::follower::Trouble;
use std::fmt;

#[derive(Clone, Debug)]
pub enum Why {
    /// what it was for seen on the way: done
    SeenOnTheWay,
    Gone,
    BeenThrough,
    ExitInView,
    HeardSomething,
    Interrupted,
    TookTooLong,
    PressedNothing,
    NothingToSee,
    NotInIt,
    TeleportedNowhere,
    NotThere,
    CouldntPickUp,
    NoRoute,
    NoWayNow,
    /// the route given up (the follower's line, as logged, and why)
    Route(String, Trouble),
    /// given up from outside, by the fight
    Fight(String),
    /// not taken: whoever directs her said not to (a jump)
    Declined,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Why::SeenOnTheWay => "seen it on the way",
            Why::Gone => "gone",
            Why::BeenThrough => "been through",
            Why::ExitInView => "an exit came into view",
            Why::HeardSomething => "something went off out of sight",
            Why::Interrupted => "interrupted",
            Why::TookTooLong => "took too long",
            Why::PressedNothing => "pressed, but nothing went off",
            Why::NothingToSee => "nothing to see from there",
            Why::NotInIt => "got there but not in it",
            Why::TeleportedNowhere => "didn't take me anywhere",
            Why::NotThere => "not there",
            Why::CouldntPickUp => "couldn't pick it up",
            Why::NoRoute => "no route",
            Why::NoWayNow => "no way there now",
            Why::Route(line, _) => return write!(f, "failed: {line}"),
            Why::Fight(why) => why,
            Why::Declined => "declined",
        };
        f.write_str(s)
    }
}

impl Why {
    /// In a few words for whoever's playing, as the reason something couldn't be done.
    pub fn plain(&self) -> String {
        match self {
            Why::SeenOnTheWay => "seen on the way",
            Why::Gone => "it's gone",
            Why::BeenThrough => "been through already",
            Why::ExitInView => "an exit came into view",
            Why::HeardSomething => "heard something",
            Why::Interrupted => "interrupted",
            Why::TookTooLong => "took too long",
            Why::PressedNothing => "pressed it, nothing seen to happen",
            Why::NothingToSee => "nothing to see from there",
            Why::NotInIt => "got near but couldn't get in",
            Why::TeleportedNowhere => "it didn't take you anywhere",
            Why::NotThere => "not there",
            Why::CouldntPickUp => "couldn't pick it up",
            Why::NoRoute | Why::NoWayNow => "no way there",
            Why::Route(_, t) => t.plain(),
            Why::Fight(_) => "a fight in the way",
            Why::Declined => "you said not to",
        }
        .to_string()
    }
}
