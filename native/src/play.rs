//! Who plays a level: the explorer alone, the explorer after every item (the expert's run), or
//! the player with monsters about (the fight).

use std::rc::Rc;

use crate::knowledge::completion::Completionist;
use crate::knowledge::explorer::{Explorer, NoHooks, Objective};
use crate::knowledge::model::Knowledge;
use crate::motor::{Controls, Observation};
use crate::navigator::Navigator;
use crate::tactical::player::Player;
use crate::tactical::tuning::Tuning;

pub enum Agent {
    Plain(Box<Explorer>),
    Expert(Box<Explorer>, Completionist),
    Fight(Box<Player>),
}

impl Agent {
    /// `fight`: monsters in at that skill (0: none); `expert`: every item, then out (`lives - 1`
    /// levels again she may ask for); `tune`: the numbers she judges a fight by.
    pub fn new(nav: &Rc<Navigator>, knowledge: Knowledge, fight: i32, expert: bool, lives: i32, tune: Rc<Tuning>) -> Agent {
        if fight > 0 {
            let mut p = Player::new(Rc::clone(nav), knowledge, tune);
            if expert {
                p.complete(lives - 1); // (every item as well as fighting: --complete)
            }
            Agent::Fight(Box::new(p))
        } else if expert {
            let mut x = Explorer::new(Rc::clone(nav), knowledge);
            let c = Completionist::new(&mut x, lives - 1);
            Agent::Expert(Box::new(x), c)
        } else {
            Agent::Plain(Box::new(Explorer::new(Rc::clone(nav), knowledge)))
        }
    }

    pub fn x(&self) -> &Explorer {
        match self {
            Agent::Plain(x) | Agent::Expert(x, _) => x,
            Agent::Fight(p) => &p.explorer,
        }
    }

    pub fn x_mut(&mut self) -> &mut Explorer {
        match self {
            Agent::Plain(x) | Agent::Expert(x, _) => x,
            Agent::Fight(p) => &mut p.explorer,
        }
    }

    pub fn act(&mut self, obs: &Observation, last: Option<&Controls>) -> Controls {
        match self {
            Agent::Plain(x) => x.act(obs, last, &mut NoHooks),
            Agent::Expert(x, c) => x.act(obs, last, c),
            Agent::Fight(p) => p.act(obs, last),
        }
    }

    /// The objectives open now (for showing).
    pub fn objectives(&mut self, obs: &Observation) -> Vec<Objective> {
        match self {
            Agent::Plain(x) => x.objectives(obs, false, &mut NoHooks),
            Agent::Expert(x, c) => x.objectives(obs, false, c),
            Agent::Fight(p) => p.explorer.objectives(obs, false, &mut NoHooks),
        }
    }

    /// Her options now, best first, with whoever's playing's own wants in (evil-boom drive).
    pub fn ranked(&mut self, obs: &Observation, risky: bool) -> Vec<Objective> {
        match self {
            Agent::Plain(x) => x.ranked(obs, risky, &mut NoHooks),
            Agent::Expert(x, c) => x.ranked(obs, risky, c),
            Agent::Fight(p) => p.ranked(obs, risky),
        }
    }

    /// The level begun again with no death to note.
    pub fn start_over(&mut self) {
        match self {
            Agent::Plain(x) | Agent::Expert(x, _) => x.restart(),
            Agent::Fight(p) => p.start_over(),
        }
    }

    pub fn restart(&mut self) {
        match self {
            Agent::Plain(x) | Agent::Expert(x, _) => x.restart(),
            Agent::Fight(p) => p.restart(),
        }
    }

    /// (a level again, asked for by the expert's run, counted: a run played back)
    pub fn used_restart(&mut self) {
        if let Agent::Expert(_, c) = self {
            c.restarts -= 1;
        }
    }

    /// (no level again to be had: out with what she has)
    pub fn no_more_restarts(&mut self) {
        if let Agent::Expert(_, c) = self {
            c.restarts = 0;
        }
    }

    pub fn player(&self) -> Option<&Player> {
        match self {
            Agent::Fight(p) => Some(p),
            _ => None,
        }
    }

    pub fn player_mut(&mut self) -> Option<&mut Player> {
        match self {
            Agent::Fight(p) => Some(p),
            _ => None,
        }
    }
}
