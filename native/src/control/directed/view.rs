// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Her side of the game as the stream console shows it (`console::EvilState`): her settings and
//! goal, the stream column's calls, her open question with the time it has left, the danger, and
//! (when asked for: it costs a look at what's open) why the bot's doing what it's doing.

use super::{ask, Director};
use crate::console::{EvilState, QuestionState};
use crate::motor::Observation;
use crate::play::Agent;

impl Director {
    /// Her side, into `to` (the session's lines and counts already in it); `why` with it.
    pub fn console_view(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        why: bool,
        to: &mut EvilState,
    ) {
        to.goal = self.goal_label();
        to.policy = self.policy.line();
        to.idle = match self.idle_auto {
            Some(t) => format!("on by instinct after {} s", t / 35),
            None => "waits for her".into(),
        };
        to.calls = self.calls(obs.tic);
        to.danger = self.now.danger().to_string();
        to.question = self.question().map(|q| QuestionState {
            id: q.id,
            text: q.text.clone(),
            choices: q.choices.iter().map(|c| c.label.clone()).collect(),
            default: q.default,
            left: if q.now {
                Some((ask::NOW_OPEN - (obs.tic - q.opened)).max(0) as f64 / 35.0)
            } else {
                q.deadline.map(|d| (d - obs.tic).max(0) as f64 / 35.0)
            },
            now: q.now,
        });
        if why {
            to.why = self.why(agent, obs).lines().map(str::to_string).collect();
        }
    }
}
