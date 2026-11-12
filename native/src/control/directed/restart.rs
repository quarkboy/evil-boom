// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The level again from its start, hers once she's made no progress for a while: progress as
//! the score counts it (something collected or killed, a sector seen, an effect learned, a wall
//! tried, a line mapped or set off, a place walked). What she came in with is what she has
//! again, what she's learned of the level she keeps, and no life is spent.

use super::Event;
use super::{Director, Report};
use crate::motor::Observation;
use crate::play::Agent;

/// no progress this long (tics) and a restart is hers
const STALLED: i64 = 90 * 35;

impl Director {
    /// Each tic: progress noted; once she's gone long enough without, told (once) that the
    /// restart is hers.
    pub(super) fn watch_progress(&mut self, agent: &Agent, obs: &Observation) {
        let collected = usize::try_from(self.tally.items.0 + self.tally.kills.0).unwrap_or(0);
        let now = (collected, crate::sweep::known_progress(agent.x()));
        match &self.progress {
            Some((was, _)) if *was == now => {}
            _ => {
                self.progress = Some((now, obs.tic));
                self.told_restart = false;
            }
        }
        if self.restart_allowed && !self.told_restart && self.stalled_for(obs.tic) >= STALLED {
            self.told_restart = true;
            self.tell(obs, Event::RestartOpen, format!(
                    "Nothing new for {} s: doom_game restart is yours (the level from its start, with what you came in with; what you know of it kept).",
                    STALLED / 35
                ),);
        }
    }

    pub(super) fn stalled_for(&self, tic: i64) -> i64 {
        self.progress.as_ref().map_or(0, |(_, since)| tic - since)
    }

    /// She asks for the level again: granted (for the runner, `restart_wanted`) or why not.
    pub fn ask_restart(&mut self, tic: i64) -> Result<String, String> {
        if !self.restart_allowed {
            return Err("Restarting isn't yours on this stream.".into());
        }
        let left = STALLED - self.stalled_for(tic);
        if left > 0 {
            return Err(format!(
                "Not yet: it's yours after {} s with nothing new ({} s to go).",
                STALLED / 35,
                (left + 34) / 35
            ));
        }
        self.restart_wanted = true;
        Ok("Restarting the level.".into())
    }

    /// The level begun again on her asking (by the runner): told; her orders and her say in the
    /// fight are the attempt before's; names and what's been told of the level stand.
    pub fn restarted(&mut self, tic: i64) {
        self.begun_again(
            tic,
            "The level again from its start, with what you came in with.",
        );
    }

    /// The level begun again by the streamer (the console's): told her as theirs; the rest as
    /// when she asks.
    pub fn streamer_restarted(&mut self, tic: i64) {
        self.begun_again(
            tic,
            "The streamer started the level again, with what you came in with.",
        );
    }

    fn begun_again(&mut self, tic: i64, said: &str) {
        self.restart_wanted = false;
        self.reports.push(Report {
            tic,
            ok: true,
            text: said.into(),
            event: super::Event::Restarted,
        });
        self.new_life();
        self.progress = None;
        self.told_restart = false;
    }
}
