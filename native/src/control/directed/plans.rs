// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What she's settled by herself at the level's scale, told to her (`tactical::player::Plan`):
//! the invulnerability fetched before she does again what she died after doing, where she's
//! off to with it, a gun before any key while she's only the pistol, and what's left alone
//! for the deaths it's cost. These are a player's choices of a level, and hers to have a say
//! in: each is told as it's taken up, the first as a question she can switch; all are in
//! her status while they hold; and an order of hers to do again what killed her is answered with a word of it.

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

/// fire held this long (tics) for a reason that lasts: told her
const HELD_TOLD_AFTER: i64 = 3 * 35;

impl Director {
    /// Each tic: a plan newly taken up, told (once a life; a gun before any key, once a level).
    pub(super) fn plans(&mut self, agent: &mut Agent, obs: &Observation) {
        let now = agent.player().map(|p| p.plans()).unwrap_or_default();
        let key_known = self.key_known(obs);
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

    /// A key she's been told of: without one, a gun before any key is nothing to tell.
    fn key_known(&self, obs: &Observation) -> bool {
        obs.things
            .iter()
            .flatten()
            .any(|t| t.category == "Key" && self.told_items.contains(&t.id))
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

    /// Her plans as they stand, in full (the console's "why").
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

    /// A plan in force, in a few words for her status (the first, if more than one).
    pub(super) fn plan_words(&self, agent: &Agent, obs: &Observation) -> Option<String> {
        let key_known = self.key_known(obs);
        let plans = agent.player()?.plans().into_iter();
        let mut plans = plans.filter(|p| *p != Plan::GunBeforeKey || key_known);
        plans.next().map(|p| match p {
            Plan::SphereFirst { .. } => "the invulnerability first".into(),
            Plan::Charged { .. } => "invulnerable: to what killed you".into(),
            Plan::GunBeforeKey => "a gun before keys".into(),
        })
    }

    /// Why she isn't shooting, when she isn't and it'll last (`HeldFire::lasting`), said to her.
    pub(super) fn not_shooting(&self, agent: &Agent) -> Option<String> {
        let why = agent.player()?.combat().holding_fire_long()?;
        Some(
            why.replace("by her", "near you")
                .replace("her shots", "your shots")
                // (no brackets in it: it goes inside lines that have their own)
                .replace(
                    " (too steep, or something in the way)",
                    ": too steep, or something in the way",
                ),
        )
    }

    /// Each tic of a fight: fire held a few seconds for a reason that lasts, told once a fight
    /// for each reason. She can't see that she isn't shooting, and it's hers to mend: another
    /// gun, somewhere else to stand, or leaving it.
    pub(super) fn held_fire_news(&mut self, agent: &Agent, obs: &Observation) {
        let now = agent.player().and_then(|p| p.combat().holding_fire_long());
        let Some(f) = self.fight.as_mut() else {
            return;
        };
        let Some(why) = now else {
            f.held = None;
            return;
        };
        match f.held {
            Some((was, _)) if was == why => {}
            _ => f.held = Some((why, obs.tic)),
        }
        let since = f.held.map_or(obs.tic, |h| h.1);
        if obs.tic - since < HELD_TOLD_AFTER || f.held_told.contains(&why) {
            return;
        }
        f.held_told.push(why);
        if let Some(said) = self.not_shooting(agent) {
            self.tell(obs, Event::HeldFire, format!("Not shooting: {said}."));
        }
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
