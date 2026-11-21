// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Who plays a level: the explorer alone, the explorer after every item (the expert's run), or
//! the player with monsters about (the fight).

use std::rc::Rc;

use crate::domain::{Controls, Observation};
use crate::knowledge::completion::Completionist;
use crate::knowledge::explorer::{Explorer, NoHooks, Objective};
use crate::knowledge::model::Knowledge;
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
    pub fn new(
        nav: &Rc<Navigator>,
        knowledge: Knowledge,
        fight: i32,
        expert: bool,
        lives: i32,
        tune: Rc<Tuning>,
    ) -> Agent {
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
        let c = match self {
            Agent::Plain(x) => x.act(obs, last, &mut NoHooks),
            Agent::Expert(x, c) => x.act(obs, last, c),
            Agent::Fight(p) => p.act(obs, last),
        };
        // (`--keys`: whatever she'd do, done with the keys a keyboard has)
        match self.x_mut().keys.as_mut() {
            Some(k) => {
                let got = k.press(obs.tic, c);
                if crate::debug::on().keys {
                    eprintln!(
                        "tic {}: at ({:.0}, {:.0}) facing {:.0} going {:?}: asked ({:.0}, {:.0}) turn {:.1}, keys ({:.0}, {:.0})",
                        obs.tic, obs.x, obs.y, obs.angle,
                        obs.velocity.map(|v| (v.0.round(), v.1.round())),
                        c.forward * if c.run { 1.0 } else { 0.5 }, c.strafe * if c.run { 1.0 } else { 0.5 }, c.turn, got.forward, got.strafe
                    );
                }
                got
            }
            None => c,
        }
    }

    /// Her movement by keys from here on (`--keys`, `motor::keys`): her hand as fast as her
    /// tuning has it.
    pub fn keys_only(&mut self) {
        let hand = match self {
            Agent::Fight(p) => p.hand(),
            _ => crate::tactical::tuning::Hand::default(),
        };
        crate::motor::keys::set_on();
        self.x_mut().keys = Some(crate::motor::keys::Keys::new(hand));
    }

    /// The objectives she last chose from, for showing (`Explorer::objectives_shown`: asking
    /// changes nothing she does).
    pub fn objectives(&mut self) -> Vec<Objective> {
        self.x_mut().objectives_shown()
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
