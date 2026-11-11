//! How things stand this tic, as she'd be told it: worked out once a tic and read by everything
//! that tells her (her status's gist, the look's danger line, the fight's questions), so they
//! can't disagree. From the fight's own assessment (`Combat::now`) and its move, and her health.

use std::collections::BTreeMap;

use super::{engaged, Director};
use crate::motor::Observation;
use crate::play::Agent;
use crate::tactical::combat::Move;

#[derive(Clone, Debug, Default)]
pub struct Situation {
    pub tic: i64,
    pub health: f64,
    pub armor: f64,
    /// what's fighting her (`Combat::engaged`), by kind: how many
    pub against: BTreeMap<&'static str, usize>,
    /// the odds too long (the assessment's)
    pub outmatched: bool,
    pub surrounded: bool,
    pub short_of_ammo: bool,
    /// an arch-vile in sight she can't kill in a moment
    pub vile: bool,
    /// a boss among what's fighting her
    pub boss: bool,
    /// the fight's move this tic; and falling back (a retreat under way)
    pub mv: Option<Move>,
    pub retreating: bool,
}

impl Situation {
    pub fn fighting(&self) -> bool {
        !self.against.is_empty()
    }

    /// The danger in a word, from the assessment: deadly (outmatched), dangerous (something's
    /// fighting her), safe.
    pub fn danger(&self) -> &'static str {
        if self.outmatched {
            "deadly"
        } else if self.fighting() {
            "dangerous"
        } else {
            "safe for now"
        }
    }

    /// What the fight's doing, in a few words ("falling back", "dodging", "fighting it out").
    pub fn doing(&self) -> &'static str {
        if self.retreating {
            return "falling back";
        }
        match self.mv {
            Some(m) if m.escaping() => "getting out of the way",
            Some(Move::KeepingClear | Move::HidingFromOutmatcher | Move::BackingOff) => {
                "keeping clear"
            }
            Some(Move::KeepingOff) => "keeping off them",
            Some(Move::GettingPast | Move::ShootingOnTheWay | Move::ToErrand) => {
                "pushing on, shooting on the way"
            }
            Some(Move::Sneaking | Move::LeavingToIt | Move::SettingOn) => "letting them be",
            Some(Move::MeleeClosing | Move::ClosingIn | Move::ClosingOutOfReach) => "closing in",
            Some(Move::OutOfReach | Move::Unarmed) => "going on past",
            _ => "fighting it out",
        }
    }
}

impl Director {
    /// How things stand now (worked out once a tic, in `tick`).
    pub(super) fn take_stock(&mut self, agent: &Agent, obs: &Observation) {
        let mut against: BTreeMap<&'static str, usize> = BTreeMap::new();
        for t in engaged(agent, obs) {
            *against.entry(t.name).or_insert(0) += 1;
        }
        let boss = against
            .keys()
            .any(|n| crate::tactical::bestiary::monster(n).is_some_and(|m| m.boss));
        let p = agent.player();
        let now = p.map(|p| &p.combat().now);
        self.now = Situation {
            tic: obs.tic,
            health: obs.health.unwrap_or(0.0),
            armor: obs.armor.unwrap_or(0.0),
            against,
            outmatched: now.is_some_and(|n| n.outmatched),
            surrounded: now.is_some_and(|n| n.surrounded),
            short_of_ammo: now.is_some_and(|n| n.short_of_ammo),
            vile: now.is_some_and(|n| n.vile),
            boss,
            mv: p.and_then(|p| p.combat().last_move),
            retreating: p.is_some_and(crate::tactical::player::Player::falling_back),
        };
    }
}
