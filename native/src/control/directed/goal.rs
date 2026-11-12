//! `doom_goal`: a standing aim of hers (the SDK plan's 4.2): the bot works toward it by itself
//! whenever no action of hers is in hand, at once rather than after the idle wait, till it's met,
//! nothing more is known to do for it, or she says `none`. An action of hers comes first; the
//! goal goes on after. On instinct for her as the idle fallback is, with what the goal needs:
//! what it's after first (`Explorer::focus`), last resorts let go of where it's a thorough one,
//! the exit given back where it's leaving.

use super::{Director, Doing, Event};
use crate::knowledge::explorer::{Focus, GoingBack, Resort};
use crate::motor::Observation;
use crate::play::Agent;
use crate::specials::{Color, Keys};
use std::collections::BTreeSet;

/// What she's set the bot to.
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// out by the first exit she knows of
    Out,
    /// every kill, item and secret, then out
    Everything,
    /// a key of that colour, or any (and the keys she held when it was set)
    Key(Option<Color>, Keys),
    /// every monster the level counts dead
    Kills,
    /// everything she knows of to pick up
    Supplies,
    /// every secret the level counts found
    Secrets,
}

impl Goal {
    /// The goal, as she's told it.
    fn label(&self) -> String {
        match self {
            Goal::Out => "leave the level".into(),
            Goal::Everything => "everything (100%), then leave".into(),
            Goal::Key(Some(c), _) => format!("get the {} key", color_name(*c)),
            Goal::Key(None, _) => "get a key".into(),
            Goal::Kills => "kill everything".into(),
            Goal::Supplies => "pick up everything you know of".into(),
            Goal::Secrets => "find every secret".into(),
        }
    }
}

fn color_name(c: Color) -> &'static str {
    match c {
        Color::Red => "red",
        Color::Blue => "blue",
        Color::Yellow => "yellow",
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
    /// Her goal set (`what`, and for a key which: a colour, or a key's id), or cleared (`none`):
    /// what she's told.
    pub fn set_goal(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        what: &str,
        r: Option<&str>,
    ) -> Result<String, String> {
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
            "everything" => Goal::Everything,
            "kills" => Goal::Kills,
            "supplies" => Goal::Supplies,
            "secrets" => Goal::Secrets,
            "key" => Goal::Key(r.and_then(|r| key_color(r, obs)), obs.keys.unwrap_or_default()),
            other => {
                return Err(format!(
                    "goal: no \"{other}\". out, everything, key (ref: a colour or a key's id), kills, supplies, secrets, none."
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
        Ok(said)
    }

    /// The goal, worked toward with nothing of hers in hand; met, or with nothing more to do for
    /// it, ended and said. True if it took the bot up this tic.
    pub(super) fn goal_tick(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
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
        let waiting = self.doing.is_none()
            && self.queue.is_empty()
            && self.resume.is_none()
            && self.pressed.is_none()
            && agent.x().current.is_none();
        if !waiting || obs.health.unwrap_or(0.0) <= 0.0 {
            return false;
        }
        self.auto_spent = false;
        self.go_instinct(agent);
        self.steer_goal(agent, &goal);
        self.tell(
            obs,
            Event::Instinct,
            format!("Going on with your goal: {}.", goal.label()),
        );
        true
    }

    /// What the goal asks of the bot, on instinct for it.
    fn steer_goal(&self, agent: &mut Agent, goal: &Goal) {
        let x = agent.x_mut();
        x.focus = match goal {
            Goal::Key(..) => Focus::Keys,
            Goal::Kills => Focus::Kills,
            Goal::Secrets => Focus::Secrets,
            Goal::Supplies => Focus::Supplies,
            Goal::Out | Goal::Everything => Focus::None,
        };
        if matches!(
            goal,
            Goal::Everything | Goal::Kills | Goal::Secrets | Goal::Supplies
        ) {
            x.going_back = GoingBack::AsItComes;
        }
        if matches!(goal, Goal::Everything | Goal::Kills | Goal::Secrets) {
            x.resorts = Some(thorough_resorts());
        }
        x.hold_exits(*goal != Goal::Out);
    }

    /// The goal let go of: the bot's own way again, if it's still on instinct.
    fn end_goal(&mut self, agent: &mut Agent) {
        let x = agent.x_mut();
        x.focus = Focus::None;
        if matches!(self.doing, Some(Doing::Auto)) {
            x.resorts = Some(self.resorts.clone());
            x.hold_exits(true);
        }
    }

    /// Met: what she's told (None: not yet). Everything done turns into leaving.
    fn goal_met(&mut self, goal: &Goal, agent: &mut Agent, obs: &Observation) -> Option<String> {
        let keys = obs.keys.unwrap_or_default();
        match goal {
            Goal::Out => None,
            Goal::Everything => {
                if self.tally.complete() {
                    self.goal = Some(Goal::Out);
                    self.tell(
                        obs,
                        Event::GoalDone,
                        "Everything's done here (100%): leaving.".into(),
                    );
                }
                None
            }
            Goal::Key(Some(c), _) => keys
                .has_color(*c)
                .then(|| format!("Goal done: you have the {} key.", color_name(*c))),
            Goal::Key(None, had) => keys
                .iter()
                .any(|k| !had.has(k))
                .then(|| "Goal done: you have a key.".to_string()),
            Goal::Kills => (self.tally.kills.0 >= self.tally.kills.1)
                .then(|| "Goal done: everything here is dead.".to_string()),
            Goal::Secrets => {
                (self.secrets_done()).then(|| "Goal done: every secret here is found.".to_string())
            }
            Goal::Supplies => (obs.tic % 35 == 0
                && !self
                    .open_objectives(agent, obs)
                    .iter()
                    .any(|o| o.kind == crate::knowledge::explorer::Kind::Item))
            .then(|| "Goal done: nothing more to pick up that you know of.".to_string()),
        }
    }

    /// The goal in a few words, for her status and the stream's column (None: none set).
    pub fn goal_label(&self) -> Option<String> {
        self.goal.as_ref().map(Goal::label)
    }

    /// A new level: a key goal was this one's.
    pub(super) fn goal_new_map(&mut self) {
        if matches!(self.goal, Some(Goal::Key(..))) {
            self.goal = None;
        }
    }
}

/// A key she names: a colour ("blue", "the red skull"), or a key's id from the lists.
fn key_color(r: &str, obs: &Observation) -> Option<Color> {
    let named = |s: &str| {
        let s = s.to_lowercase();
        [Color::Red, Color::Blue, Color::Yellow]
            .into_iter()
            .find(|&c| s.contains(color_name(c)))
    };
    named(r).or_else(|| {
        let n: i32 = r.strip_prefix("key-t")?.parse().ok()?;
        obs.things
            .iter()
            .flatten()
            .find(|t| t.id == n)
            .and_then(|t| named(t.name))
    })
}
