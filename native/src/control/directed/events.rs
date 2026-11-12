//! What she's told, by kind: every piece of news carries what it is, and how much it matters
//! (`weight`) is this one table's, not each telling's. The words stay with whoever tells it
//! (they need what only it knows); the kind is for what decides by it: what's a moment, what's
//! urgent past the budget, and anything that wants only some kinds (a stream panel, a console).

/// How much a piece of news matters to her.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    /// to know, not to react to (told silently)
    Quiet,
    /// worth a reaction, within the moments a minute
    Moment,
    /// to be told whatever else has been (her life, her own action's answer, a question)
    Urgent,
}

/// What a piece of news is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    // Her actions.
    /// an action of hers begun ("Going to ...", "Exploring ...", paused for a fight)
    Begun,
    /// an action of hers done, or not, or dropped for another
    Outcome,
    /// a question for her
    Asked,
    /// a question's default taken, no answer having come
    Defaulted,
    /// her say in the fight taken ("Fighting bold, 30 s."), or what she'd asked of it over
    Tactic,
    TacticsOver,

    // Her life.
    Died,
    /// died, and the level begins again
    DiedAgain,
    HealthLow,
    HurtFast,
    Teleported,
    OutOfAmmo,
    Crushed,
    /// her own blast caught her (`big`: enough to matter)
    OwnBlast {
        big: bool,
    },
    /// on a floor that hurts with no known way off
    Trapped,

    // What she finds.
    Key,
    Weapon,
    Secret,
    ExitFound,
    SwitchFound,
    DoorFound,
    /// something of note seen
    Spotted,
    /// things seen she couldn't get to and now can (`notable`: one of note among them)
    Reachable {
        notable: bool,
    },
    /// the game's word on a lock tried without its key
    Lock,
    /// a powerup taken (what it is and does)
    Powerup,
    /// armor or a backpack taken
    Pickup,
    /// one that runs out about to (`vital`: one keeping her alive, invulnerability or the suit)
    PowerEnding {
        vital: bool,
    },
    PowerOver,

    // Monsters.
    /// monsters first seen (`dangerous`: a heavy one or a boss)
    Sighting {
        dangerous: bool,
    },
    /// a big one dead
    BigKill,
    /// several dead at once
    MultiKill,
    /// one dead, gibbed
    Gibbed,
    /// barrels gone off, and what they took
    Barrels,
    /// the level's last monster dead
    AllDead,
    Raised,
    /// a fight's summary (`notable`: it cost, or killed, enough)
    FightOver {
        notable: bool,
    },

    // Her moves.
    /// a big run-jump ahead
    JumpAhead,
    Landed,
    MissedJump,
    NotJumping,

    // The bot.
    /// what instinct's taken up
    Instinct,
    /// on by instinct from now
    OnInstinct,
    /// every kill, item and secret the level counts: done
    LevelComplete,
    /// her goal met, or nothing more to do for it
    GoalDone,
    /// an action of hers dropped for a newer one (told as an outcome is)
    Replaced,
    /// the bot's pick suggested (`loud`: the second time)
    Suggestion {
        loud: bool,
    },
    /// the restart hers now
    RestartOpen,

    // The game.
    LevelStart,
    LevelDone,
    /// a finale's text, as it's typed out
    OnScreen,
    /// a game hers to start, the next level hers to go on to
    GameOffer,
    GameOver,
    Restarted,
}

impl Event {
    /// The one table of how much each matters.
    pub fn weight(self) -> Weight {
        use Event as E;
        match self {
            E::Begun
            | E::TacticsOver
            | E::DoorFound
            | E::Gibbed
            | E::Instinct
            | E::OwnBlast { big: false }
            | E::Sighting { dangerous: false }
            | E::Reachable { notable: false }
            | E::FightOver { notable: false }
            | E::Suggestion { loud: false } => Weight::Quiet,
            E::Tactic
            | E::OwnBlast { big: true }
            | E::Key
            | E::Weapon
            | E::Secret
            | E::ExitFound
            | E::SwitchFound
            | E::Spotted
            | E::Reachable { notable: true }
            | E::Lock
            | E::Powerup
            | E::Pickup
            | E::PowerEnding { vital: false }
            | E::PowerOver
            | E::BigKill
            | E::MultiKill
            | E::Barrels
            | E::AllDead
            | E::Raised
            | E::FightOver { notable: true }
            | E::JumpAhead
            | E::Landed
            | E::MissedJump
            | E::NotJumping
            | E::Suggestion { loud: true }
            | E::RestartOpen
            | E::LevelStart => Weight::Moment,
            E::Outcome
            | E::Replaced
            | E::Asked
            | E::Defaulted
            | E::Died
            | E::DiedAgain
            | E::HealthLow
            | E::HurtFast
            | E::Teleported
            | E::OutOfAmmo
            | E::Crushed
            | E::Trapped
            | E::PowerEnding { vital: true }
            | E::Sighting { dangerous: true }
            | E::OnInstinct
            | E::LevelComplete
            | E::GoalDone
            | E::LevelDone
            | E::OnScreen
            | E::GameOffer
            | E::GameOver
            | E::Restarted => Weight::Urgent,
        }
    }
}
