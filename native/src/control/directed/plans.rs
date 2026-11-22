// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What she's settled by herself at the level's scale, told to her (`tactical::player::Plan`):
//! the invulnerability fetched before she does again what she died after doing, where she's
//! off to with it, a gun before any key while she's only the pistol, and what's left alone
//! for the deaths it's cost. These are a player's choices of a level, and hers to have a say
//! in: each is told as it's taken up, the first as a question she can switch; all are in
//! `doom_why`; and an order of hers to do again what killed her is answered with a word of it.

use super::a;
use super::ask::{About, Choice, Then};
use super::{Director, Event};
use crate::domain::Observation;
use crate::knowledge::explorer::Objective;
use crate::play::Agent;
use crate::tactical::player::Plan;

/// Invulnerable and off with it: to what, as she'd say it.
fn charged(by: Option<&str>) -> String {
    match by {
        Some(by) => format!(
            "Invulnerable: straight to what killed you ({}).",
            a(by).to_lowercase()
        ),
        None => "Invulnerable: straight back to where you died.".into(),
    }
}

impl Director {
    /// Each tic: a plan newly taken up, told (once a life; a gun before any key, once a level).
    pub(super) fn plans(&mut self, agent: &mut Agent, obs: &Observation) {
        let now = agent.player().map(|p| p.plans()).unwrap_or_default();
        // (a key she's been told of: without one, a gun before any key is nothing to tell)
        let key_known = obs
            .things
            .iter()
            .flatten()
            .any(|t| t.category == "Key" && self.told_items.contains(&t.id));
        for p in now {
            if self.plans_told.contains(&p) || (p == Plan::GunBeforeKey && !key_known) {
                continue;
            }
            self.plans_told.push(p.clone());
            match p {
                Plan::SphereFirst { line, by } => {
                    let text = format!(
                        "You died to {} after {}: getting the invulnerability first.",
                        a(&by).to_lowercase(),
                        self.set_off(line)
                    );
                    // (done already, hers to switch: it's her life and her thirty seconds)
                    let choices = vec![
                        Choice {
                            label: "get it".into(),
                            then: Then::Nothing,
                        },
                        Choice {
                            label: "go without".into(),
                            then: Then::NoSphere,
                        },
                    ];
                    if !self.ask_now(
                        agent,
                        obs,
                        text.clone(),
                        choices,
                        0,
                        (About::Settle, "medium"),
                    ) {
                        self.tell(obs, Event::Plan, text);
                    }
                }
                Plan::Charged { by } => self.tell(obs, Event::Plan, charged(by.as_deref())),
                Plan::GunBeforeKey => self.tell(
                    obs,
                    Event::Plan,
                    "Only the pistol: a gun first, keys after.".into(),
                ),
            }
        }
    }

    /// A line she set off, as she'd know it: the switch or door she's been told of, else no
    /// more than that she set something off.
    fn set_off(&self, line: usize) -> String {
        for (kind, doing) in [("switch", "pressing"), ("door", "opening")] {
            let id = format!("{kind}-l{line}");
            if self.told_exits.contains(&id) {
                return format!("{doing} {id}");
            }
        }
        "something you set off".into()
    }

    /// Her plans as they stand, for `doom_why`.
    pub(super) fn plan_lines(&self, agent: &Agent) -> Vec<String> {
        agent
            .player()
            .map(|p| p.plans())
            .unwrap_or_default()
            .into_iter()
            .map(|p| match p {
                Plan::SphereFirst { line, by } => format!(
                    "Getting the invulnerability first: you died to {} after {}.",
                    a(&by).to_lowercase(),
                    self.set_off(line)
                ),
                Plan::Charged { by } => charged(by.as_deref()),
                Plan::GunBeforeKey => {
                    "Keys wait: you've only the pistol, and there's a gun to get.".into()
                }
            })
            .collect()
    }

    /// How many deaths `o` is left alone for on instinct (0: it isn't).
    pub(super) fn deaths_at(agent: &Agent, o: &Objective) -> usize {
        agent.player().map_or(0, |p| p.deaths_at(o))
    }

    /// An order of hers to go to `r`: a word of it if she died after setting that off in a
    /// life before (her order's carried out all the same: it's hers).
    pub fn death_note(&mut self, agent: &mut Agent, obs: &Observation, r: &str) -> Option<String> {
        let id = self.resolve(r);
        let obj = self.objective_for(agent, obs, &id).ok()?;
        let line = obj.trigger.as_ref()?.linedef()?;
        let by = agent.player()?.died_after(line)?;
        Some(format!(
            "You died to {} after this last time.",
            a(&by).to_lowercase()
        ))
    }
}
