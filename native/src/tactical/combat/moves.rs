//! What the fight decided, as a type: the tree's node (or what stands in for one with nothing in
//! sight). The label a decision logs is for reading; what the rest of her does about a decision
//! goes by its `Move`.

/// The fight's decision this tic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    // --- survive
    /// out of an arch-vile's sight
    OutOfVileSight,
    /// to where a hitscan heavy can't see her, before it fires
    Ducking,
    /// there: waiting out its fire
    Ducked,
    /// a missile on its way: out of its way
    Dodging,
    /// a homing missile going round her: to a corner it'll fly into
    ShakingOff,
    /// still for the BFG's spray
    BfgHold,
    /// out from under a crusher
    OffCrusher,
    /// away from a barrel going off
    OffLitBarrel,
    /// away from a barrel a shot could set off
    OffBarrel,
    // --- the fight
    /// closing in to fight up close
    MeleeClosing,
    /// at it, up close
    MeleeAt,
    /// back from what claws
    BackingOff,
    /// no shot fired: it'd wake a pack
    Sneaking,
    /// on to an errand (health, a gun, ammo), shooting on the way
    ToErrand,
    /// nothing to shoot with: on along the route
    Unarmed,
    /// closing in on a target out of the gun's reach
    ClosingOutOfReach,
    /// out of reach: on along the route
    OutOfReach,
    /// a step to where a shot gets to it
    SteppingForShot,
    /// outmatched: keeping her distance
    KeepingClear,
    /// outmatched by a hitscanner: out of its sight
    HidingFromOutmatcher,
    /// outmatched: on along the route, shooting on the way
    GettingPast,
    /// too near for the gun, or for them to miss
    KeepingOff,
    /// the route waits: standing and fighting
    Standing,
    /// standing's closing in to the gun's window
    ClosingIn,
    /// the route goes on, shooting as she goes
    ShootingOnTheWay,
    // --- nothing in sight, or after the tree
    /// what she was fighting just out of sight: fought where it was
    LostSight,
    /// a rocket about to leave: facing held the way it was judged safe
    RocketHeld,
    /// a rocket about to leave into something near: turned off it
    RocketTurnedOff,
}

impl Move {
    pub const ALL: [Move; 28] = [
        Move::OutOfVileSight,
        Move::Ducking,
        Move::Ducked,
        Move::Dodging,
        Move::ShakingOff,
        Move::BfgHold,
        Move::OffCrusher,
        Move::OffLitBarrel,
        Move::OffBarrel,
        Move::MeleeClosing,
        Move::MeleeAt,
        Move::BackingOff,
        Move::Sneaking,
        Move::ToErrand,
        Move::Unarmed,
        Move::ClosingOutOfReach,
        Move::OutOfReach,
        Move::SteppingForShot,
        Move::KeepingClear,
        Move::HidingFromOutmatcher,
        Move::GettingPast,
        Move::KeepingOff,
        Move::Standing,
        Move::ClosingIn,
        Move::ShootingOnTheWay,
        Move::LostSight,
        Move::RocketHeld,
        Move::RocketTurnedOff,
    ];

    /// The move of that name (as `{:?}` gives it).
    pub fn named(name: &str) -> Option<Move> {
        Move::ALL.into_iter().find(|m| format!("{m:?}") == name)
    }

    /// The route keeps her legs through it (unless a missile's coming or an arch-vile's fire).
    pub fn route_goes_on(self) -> bool {
        matches!(self, Move::ShootingOnTheWay | Move::ToErrand | Move::GettingPast | Move::OutOfVileSight | Move::OutOfReach | Move::Unarmed | Move::Sneaking)
    }

    /// Out of the way of something: where it takes her isn't second-guessed at a ledge's edge.
    pub fn escaping(self) -> bool {
        matches!(self, Move::Dodging | Move::Ducking | Move::Ducked | Move::OutOfVileSight | Move::OffCrusher | Move::OffLitBarrel | Move::OffBarrel)
    }
}

/// Why the trigger's held with a target in the sights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldFire {
    OutOfReach,
    RocketNotSafe,
    PlasmaBlocked,
    LeftToBoss,
    NoShot,
    BarrelInLine,
}

impl HeldFire {
    pub fn label(self) -> &'static str {
        match self {
            HeldFire::OutOfReach => "out of reach",
            HeldFire::RocketNotSafe => "rocket not safe",
            HeldFire::PlasmaBlocked => "plasma wouldn't get there",
            HeldFire::LeftToBoss => "left to the boss",
            HeldFire::NoShot => "no shot (too steep, or something in the way)",
            HeldFire::BarrelInLine => "a barrel in the line of fire, its chain by her",
        }
    }
}
