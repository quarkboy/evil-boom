// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Co-op: the others in the game, as she's told of them. Each player is directed by her own
//! Neuro (`viewer/coop.rs`); what she's told of the others is what players in a game tell each
//! other: where each is, how they're doing, a death, a key found, a switch hit, a way out found.

use super::{compass, Director, Event, Report};
use crate::domain::{heading, Observation};
use crate::geom::{dist, Point};
use crate::play::Agent;

/// Each player's colour's name, as Doom's co-op has them.
pub const COLOUR_NAMES: [&str; 4] = ["green", "indigo", "brown", "red"];

/// The game she's one of the players of.
#[derive(Clone, Debug, Default)]
pub struct Team {
    /// her own number (0-based), and how many there are
    pub me: usize,
    pub players: usize,
    pub mates: Vec<Mate>,
}

/// Another player, as she'd be told.
#[derive(Clone, Debug)]
pub struct Mate {
    /// 0-based
    pub player: usize,
    pub pos: Point,
    pub alive: bool,
    pub health: i32,
    pub fighting: bool,
}

/// "player 2 (indigo)"
pub fn who(player: usize) -> String {
    format!(
        "player {} ({})",
        player + 1,
        COLOUR_NAMES.get(player).copied().unwrap_or("?")
    )
}

impl Team {
    /// Told her with the rules: who she is in this game.
    pub fn brief(&self) -> String {
        format!(
            "Co-op: you're {} of {}. The others are players like you; their status is in yours. \
             Your bullets hurt them.",
            who(self.me),
            self.players
        )
    }
}

impl Director {
    /// The others as they are this tic (the watch's to set): the line in her status made again
    /// once a second.
    pub fn set_team(&mut self, agent: &Agent, obs: &Observation, team: Team) {
        if self.team.is_none() || obs.tic % 35 == 0 {
            let pos = obs.pos();
            let mut parts = Vec::new();
            for m in &team.mates {
                if !m.alive {
                    parts.push(format!("{} dead", who(m.player)));
                    continue;
                }
                let area = self
                    .area_at(agent, m.pos)
                    .map_or(String::new(), |a| format!(" in {a}"));
                let fighting = if m.fighting { ", fighting" } else { "" };
                parts.push(format!(
                    "{}{area} {} {:.0}, health {}{fighting}",
                    who(m.player),
                    compass(heading(pos, m.pos)),
                    dist(pos, m.pos),
                    m.health
                ));
            }
            self.team_line = if parts.is_empty() {
                String::new()
            } else {
                format!("team: {}", parts.join("; "))
            };
        }
        self.team = Some(team);
    }

    /// Something another player's done, told her as they'd say it ("player 2 (indigo) got the
    /// blue key"): a moment, by its kind's weight.
    pub fn mate_news(&mut self, tic: i64, text: String) {
        self.reports.push(Report {
            tic,
            ok: true,
            text,
            event: Event::Team,
        });
    }

    /// Born again at her start in a co-op game (a pistol and 100 health), the level going on as
    /// it is.
    pub fn reborn(&mut self, tic: i64) {
        self.new_life();
        self.reports.push(Report {
            tic,
            ok: true,
            text: "You're back at your start with a pistol. The level's as it was.".into(),
            event: Event::Reborn,
        });
    }
}
