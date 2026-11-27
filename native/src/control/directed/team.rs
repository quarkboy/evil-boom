// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Co-op: the others in the game, as she's told of them. Each player is directed by her own
//! Neuro (`viewer/coop.rs`); what she's told of the others is what players in a game tell each
//! other: where each is, how they're doing, a death, a key found, a switch hit, a way out found.

use super::{compass, Director, Entry, Event, Report};
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
    /// the player who's a person, not a bot, if one is
    pub human: Option<usize>,
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
        let person = self.human.map_or(String::new(), |h| {
            format!(
                " Player{} is a person, playing with you.",
                &who(h)["player".len()..]
            )
        });
        format!(
            "Co-op: you're {} of {}. The others are players like you; their status is in yours.{person} \
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
    /// it is: told so, with what she had (`before`, her last look alive) that she knows where to
    /// take again: each key (the level leaves them all lying), and each gun, the level's own
    /// copy if she knows one (it stays where it lies, as a key does), else the nearest dropped.
    pub fn reborn(&mut self, agent: &Agent, obs: &Observation, before: Option<&Observation>) {
        let kills = std::mem::take(&mut self.kills);
        self.new_life();
        self.kills = kills.reborn();
        let mut again = Vec::new();
        if let Some(b) = before {
            let x = agent.x();
            let known = x.knowledge.known_polys();
            let pos = obs.pos();
            let placed = |t: &crate::domain::Thing| {
                x.nav.map.things.iter().any(|m| {
                    m.x.round() as i32 == t.x.round() as i32
                        && m.y.round() as i32 == t.y.round() as i32
                })
            };
            let had = |name: &str| match name {
                "Chainsaw" => b.chainsaw == Some(true),
                "SuperShotgun" => b.super_shotgun == Some(true),
                _ if crate::specials::Key::named(name).is_some() => {
                    crate::specials::Key::had_already(name, b.keys)
                }
                _ => crate::bestiary::weapon_item(name)
                    .is_some_and(|w| b.weapons.as_ref().is_some_and(|ws| ws.contains(&w))),
            };
            let mut names: Vec<&'static str> = Vec::new();
            for t in obs.things.iter().flatten() {
                if (t.category == "Key" || t.category == "Weapon")
                    && !names.contains(&t.name)
                    && had(t.name)
                {
                    names.push(t.name);
                }
            }
            for name in names {
                let best = obs
                    .things
                    .iter()
                    .flatten()
                    .filter(|t| t.name == name)
                    .filter(|t| {
                        x.nav
                            .poly_at((t.x, t.y))
                            .is_some_and(|p| known.contains(&p))
                    })
                    .min_by(|a, c| {
                        (!placed(a), dist(pos, (a.x, a.y)))
                            .partial_cmp(&(!placed(c), dist(pos, (c.x, c.y))))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                if let Some(t) = best {
                    let id = format!(
                        "{}-t{}",
                        if t.category == "Key" { "key" } else { "item" },
                        t.id
                    );
                    again.push(Entry {
                        id,
                        what: crate::control::names::called(t.name).to_string(),
                        at: (t.x, t.y),
                    });
                }
            }
        }
        // (and what she knows of besides isn't news on her way about: a gun seen while she had
        // it, "spotted" now she hasn't. Every shotgun of the level's dead was, at once)
        {
            let x = agent.x();
            let known = x.knowledge.known_polys();
            for t in obs.things.iter().flatten() {
                if (t.category == "Key" || t.category == "Weapon")
                    && x.nav
                        .poly_at((t.x, t.y))
                        .is_some_and(|p| known.contains(&p))
                {
                    self.told_items.insert(t.id);
                }
            }
        }
        let known = if again.is_empty() {
            String::new()
        } else {
            let lines: Vec<String> = again.iter().map(|e| self.describe(e, obs)).collect();
            format!(" Known, to take again: {}.", lines.join("; "))
        };
        self.reports.push(Report {
            tic: obs.tic,
            ok: true,
            text: format!("You're back at your start with a pistol. The level's as it was.{known}"),
            event: Event::Reborn,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Team;

    /// Told who she is, and of a person among them if there is one.
    #[test]
    fn brief_names_the_person() {
        let mut t = Team {
            me: 1,
            players: 2,
            ..Team::default()
        };
        assert!(!t.brief().contains("person"));
        t.human = Some(0);
        let b = t.brief();
        assert!(
            b.contains("you're player 2 (indigo) of 2")
                && b.contains("Player 1 (green) is a person, playing with you."),
            "{b}"
        );
    }
}
