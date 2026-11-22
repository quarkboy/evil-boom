// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! `doom_goal`, which does two things. `finish`: when the level's done (out by the first exit;
//! or every kill, every secret, or everything first, and then out), standing across levels.
//! `prefer`: what comes first meanwhile (health, ammo, the hunt, new ground), till she changes
//! it or the level ends. Neither plays for her: each puts what it's after first in what's
//! suggested to her (`Director::pick`) and in what she does on instinct
//! (`Explorer::focus`); a thorough finish lets go of the last resorts, and `out` gives instinct
//! the exit. How she explores meanwhile stays her `explore` setting's.

use super::{Director, Doing, Event};
use crate::domain::Observation;
use crate::knowledge::explorer::{Focus, Resort};
use crate::play::Agent;
use std::collections::BTreeSet;

/// When the level's done, as she's set it (`finish`).
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// out by the first exit she knows of
    Out,
    /// every kill, item and secret, then out
    Completionist,
    /// every monster the level counts dead, then out
    Kills,
    /// every secret the level counts found, then out
    Secrets,
    /// everything she knows of to pick up (not hers to set: the answer to "stock up first?"
    /// on her way out to another level)
    Supplies,
}

impl Goal {
    /// The goal, as she's told it.
    fn label(&self) -> String {
        match self {
            Goal::Out => "leave the level".into(),
            Goal::Completionist => "completionist (100%), then leave".into(),
            Goal::Kills => "kill everything, then leave".into(),
            Goal::Supplies => "pick up everything you know of".into(),
            Goal::Secrets => "find every secret, then leave".into(),
        }
    }

    /// What the goal puts first, on instinct and in what's suggested to her.
    pub(super) fn focus(&self) -> Focus {
        match self {
            Goal::Kills => Focus::Kills,
            Goal::Secrets => Focus::Secrets,
            Goal::Supplies => Focus::Supplies,
            Goal::Out | Goal::Completionist => Focus::None,
        }
    }
}

/// What comes first meanwhile, as she's set it (`prefer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefer {
    /// health that would add anything, as when she's hurt
    Health,
    /// ammo she has room for, as when she's short
    Ammo,
    /// the monsters she knows of
    Hunt,
    /// ground not yet seen, before pickups, doors and switches
    Unexplored,
}

impl Prefer {
    fn parse(s: &str) -> Option<Prefer> {
        Some(match s {
            "health" => Prefer::Health,
            "ammo" => Prefer::Ammo,
            "hunt" => Prefer::Hunt,
            "unexplored" => Prefer::Unexplored,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Prefer::Health => "health first",
            Prefer::Ammo => "ammo first",
            Prefer::Hunt => "the hunt first",
            Prefer::Unexplored => "new ground first",
        }
    }

    fn focus(self) -> Focus {
        match self {
            Prefer::Health | Prefer::Ammo => Focus::Things,
            Prefer::Hunt => Focus::Kills,
            Prefer::Unexplored => Focus::Unexplored,
        }
    }

    /// A pickup it's after: one of its kind that would add anything (the big ones, spheres
    /// and the berserk pack, stay her `powerups` setting's).
    fn takes(self, t: &crate::domain::Thing, obs: &Observation) -> bool {
        use crate::bestiary::{ammo_item, ammo_max, health_item};
        match self {
            Prefer::Health => {
                matches!(t.name, "Stimpack" | "Medikit" | "HealthBonus")
                    && health_item(t.name)
                        .is_some_and(|(_, cap)| obs.health.unwrap_or(0.0) < f64::from(cap))
            }
            Prefer::Ammo => ammo_item(t.name).is_some_and(|(slots, _)| {
                let have = obs
                    .ammo
                    .iter()
                    .flatten()
                    .find(|(s, _)| *s == slots[0])
                    .map_or(0, |(_, n)| *n);
                let max = ammo_max(slots[0]) * if obs.backpack == Some(true) { 2.0 } else { 1.0 };
                f64::from(have) < max
            }),
            Prefer::Hunt | Prefer::Unexplored => false,
        }
    }
}

/// every last resort but leaving: what a thorough goal takes without asking her
fn thorough_resorts() -> BTreeSet<Resort> {
    [
        Resort::Strafing,
        Resort::StuckAgain,
        Resort::EveryWall,
        Resort::MissedJumps,
        Resort::Harm,
    ]
    .into_iter()
    .collect()
}

impl Director {
    /// When the level's done set (`what`), or cleared (`none`): what she's told. Completionist
    /// asked first (a question: she's sure, or not).
    pub fn set_goal(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        what: &str,
    ) -> Result<String, String> {
        if what.trim().eq_ignore_ascii_case("completionist")
            && self.goal != Some(Goal::Completionist)
        {
            let asked = self.ask(
                obs,
                "Completionist: every kill, item and secret before you leave. Slow, and dull to watch. Sure?".into(),
                vec![
                    super::ask::Choice {
                        label: "yes, completionist".into(),
                        then: super::ask::Then::Goal("completionist"),
                    },
                    super::ask::Choice {
                        label: "no".into(),
                        then: super::ask::Then::Nothing,
                    },
                ],
                1,
                true,
            );
            return asked.ok_or_else(|| "Not now: answer what's asked first.".into());
        }
        self.goal_to(agent, what)
    }

    /// The goal taken (asked about already where it's asked), or cleared: what she's told.
    pub(super) fn goal_to(&mut self, agent: &mut Agent, what: &str) -> Result<String, String> {
        let goal = match what.trim().to_lowercase().as_str() {
            "none" => {
                return Ok(match self.goal.take() {
                    Some(g) => {
                        self.end_goal(agent);
                        format!("Goal dropped: {}.", g.label())
                    }
                    None => "No goal set.".into(),
                });
            }
            "out" => Goal::Out,
            "completionist" => Goal::Completionist,
            "kills" => Goal::Kills,
            "supplies" => Goal::Supplies,
            "secrets" => Goal::Secrets,
            other => {
                return Err(format!(
                    "finish: no \"{other}\". out, kills, secrets, completionist or none."
                ))
            }
        };
        // (it stands in for her queued actions; what's in hand is stopped for it)
        self.queue.clear();
        self.resume = None;
        if matches!(
            self.doing,
            Some(Doing::Once(..) | Doing::Area(..) | Doing::Way(..))
        ) {
            self.doing = None;
            let x = agent.x_mut();
            x.current = None;
            x.follower = None;
        }
        let said = format!("Goal: {}.", goal.label());
        self.goal = Some(goal);
        // (an action of hers: her wait for the next starts afresh, and instinct may take up
        // again after it, with the goal's things first)
        self.auto_spent = false;
        self.idle_since = None;
        Ok(said)
    }

    /// The goal: met, or with nothing more to do for it on instinct, ended and said; on
    /// instinct, what it needs set. It never starts her off (always false: the tic goes on).
    pub(super) fn goal_tick(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        // (what's first on instinct: hers to prefer, else her goal's)
        let (focus, things) = (self.focus_now(), self.preferred_things(obs));
        let x = agent.x_mut();
        x.focus = focus;
        x.focus_things = things;
        // (what she prefers, had: it's over, and said)
        if obs.tic % 35 == 0
            && let Some(p) = self.prefer
            && let Some(said) = self.prefer_met(p, agent, obs)
        {
            self.prefer = None;
            self.tell(obs, Event::GoalDone, said);
        }
        let Some(goal) = self.goal.clone() else {
            return false;
        };
        if let Some(done) = self.goal_met(&goal, agent, obs) {
            self.tell(obs, Event::GoalDone, done);
            self.end_goal(agent);
            self.goal = None;
            return false;
        }
        // (everything done turns the goal into leaving: worked from that from this tic on)
        let Some(goal) = self.goal.clone() else {
            return false;
        };
        if matches!(self.doing, Some(Doing::Auto)) {
            if agent.x().out_of_options.is_some() {
                self.tell(
                    obs,
                    Event::GoalDone,
                    format!(
                        "Nothing more you know of to do for your goal ({}).",
                        goal.label()
                    ),
                );
                self.end_goal(agent);
                self.goal = None;
                return false;
            }
            self.steer_goal(agent, &goal);
            return false;
        }
        false
    }

    /// What the goal asks of the bot, on instinct for it.
    fn steer_goal(&self, agent: &mut Agent, goal: &Goal) {
        let x = agent.x_mut();
        if matches!(goal, Goal::Completionist | Goal::Kills | Goal::Secrets) {
            x.resorts = Some(thorough_resorts());
        }
        x.hold_exits(*goal != Goal::Out);
    }

    /// The goal let go of: the bot's own way again, if it's still on instinct.
    fn end_goal(&mut self, agent: &mut Agent) {
        let x = agent.x_mut();
        if matches!(self.doing, Some(Doing::Auto)) {
            x.resorts = Some(self.resorts.clone());
            x.hold_exits(true);
        }
    }

    /// Met: what she's told (None: not yet). Everything done turns into leaving.
    fn goal_met(&mut self, goal: &Goal, agent: &mut Agent, obs: &Observation) -> Option<String> {
        // (a count complete: what's left of the goal is leaving)
        let done = |d: &mut Self, said: &str| {
            d.goal = Some(Goal::Out);
            d.tell(obs, Event::GoalDone, said.to_string());
        };
        match goal {
            Goal::Out => None,
            Goal::Completionist => {
                if self.tally.complete() {
                    done(self, "Everything's done here (100%): leaving.");
                }
                None
            }
            Goal::Kills => {
                if self.tally.kills.0 >= self.tally.kills.1 {
                    done(self, "Everything here is dead: leaving.");
                }
                None
            }
            Goal::Secrets => {
                if self.secrets_done() {
                    done(self, "Every secret here is found: leaving.");
                }
                None
            }
            Goal::Supplies => (obs.tic % 35 == 0
                && !self
                    .open_objectives(agent, obs)
                    .iter()
                    .any(|o| o.kind == crate::knowledge::explorer::Kind::Item))
            .then(|| "Goal done: nothing more to pick up that you know of.".to_string()),
        }
    }

    /// Her goal and what she prefers, in a few words, for her status and the stream's column
    /// (None: neither set).
    pub fn goal_label(&self) -> Option<String> {
        let parts: Vec<String> = self
            .goal
            .as_ref()
            .map(Goal::label)
            .into_iter()
            .chain(self.prefer.map(|p| p.label().to_string()))
            .collect();
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    /// What she'd have first meanwhile set (`what`), or cleared (`none`): what she's told.
    pub fn set_prefer(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        what: &str,
    ) -> Result<String, String> {
        let what = what.trim().to_lowercase();
        if what == "none" {
            return Ok(match self.prefer.take() {
                Some(p) => format!("No longer {}.", p.label()),
                None => "Nothing preferred.".into(),
            });
        }
        let p = Prefer::parse(&what).ok_or_else(|| {
            format!("prefer: no \"{what}\". health, ammo, hunt, unexplored or none.")
        })?;
        // (had already: said, and not set to be dropped a moment after)
        if self.prefer_met(p, agent, obs).is_some() {
            return Ok(match p {
                Prefer::Health => "Not set: your health's 100 already.",
                Prefer::Ammo => "Not set: you've ammo enough for a cyberdemon already.",
                Prefer::Hunt => "Not set: nothing you know of is alive.",
                Prefer::Unexplored => "Not set: no new ground you know a way to.",
            }
            .into());
        }
        self.prefer = Some(p);
        // (an action of hers: instinct may take up again after it, with that first)
        self.auto_spent = false;
        self.idle_since = None;
        Ok(format!("From now, {}.", p.label()))
    }

    /// What she prefers, had (None: not yet). Health: 100 of it. Ammo: enough, with the guns
    /// she holds and the BFG left out, to kill a cyberdemon (the fight's own reckoning of what
    /// her ammo's worth, `Combat::ammo_short_for`). The hunt: nothing she knows of left alive.
    /// New ground: none she knows a way to left unseen.
    fn prefer_met(&self, p: Prefer, agent: &mut Agent, obs: &Observation) -> Option<String> {
        let done = match p {
            Prefer::Health => obs.health.unwrap_or(0.0) >= 100.0,
            Prefer::Ammo => agent.player().is_some_and(|pl| {
                let mut o = obs.clone();
                o.weapons = obs
                    .weapons
                    .as_ref()
                    .map(|w| w.iter().copied().filter(|g| *g != 7).collect());
                !pl.combat().ammo_short_for(&o, &["Cyberdemon"])
            }),
            Prefer::Hunt => agent
                .player()
                .is_some_and(|pl| pl.known_monsters().is_empty()),
            Prefer::Unexplored => {
                let none = std::collections::BTreeSet::new();
                !self
                    .open_objectives(agent, obs)
                    .iter()
                    .any(|o| Focus::Unexplored.wants(o, &none))
            }
        };
        done.then(|| {
            match p {
                Prefer::Health => "Health 100: no longer health first.",
                Prefer::Ammo => "Ammo enough for a cyberdemon: no longer ammo first.",
                Prefer::Hunt => "Nothing you know of left alive: the hunt's over.",
                Prefer::Unexplored => "No new ground left that you know a way to.",
            }
            .to_string()
        })
    }

    /// She's said what she'd have first.
    pub(super) fn prefers(&self) -> bool {
        self.prefer.is_some()
    }

    /// What's put first now: what she prefers, else what her goal's after.
    pub(super) fn focus_now(&self) -> Focus {
        self.prefer
            .map(Prefer::focus)
            .or_else(|| self.goal.as_ref().map(Goal::focus))
            .unwrap_or(Focus::None)
    }

    /// The pickups she prefers now (health, or ammo, that would add anything): for the
    /// explorer to offer and put first.
    pub(super) fn preferred_things(&self, obs: &Observation) -> BTreeSet<i32> {
        let Some(p) = self.prefer else {
            return BTreeSet::new();
        };
        obs.things
            .iter()
            .flatten()
            .filter(|t| p.takes(t, obs))
            .map(|t| t.id)
            .collect()
    }

    /// A new level: what she preferred was the last one's.
    pub(super) fn goal_new_map(&mut self) {
        self.prefer = None;
    }
}
