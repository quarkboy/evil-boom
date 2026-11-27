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

/// A deathmatch she's in (the watch's to set, each tic): the score and the clock, as Doom's own
/// messages and its frag counts tell every player.
#[derive(Clone, Debug, Default)]
pub struct Match {
    /// 1 Doom's classic (weapons stay), 2 deathmatch 2.0 (what's taken comes back)
    pub mode: i32,
    /// over at this many frags (0: none), after this long (s; 0: none)
    pub frag_limit: i32,
    pub seconds: f64,
    /// the game's time so far (s), and each player's frags
    pub played: f64,
    pub frags: Vec<i32>,
    /// gearing up or hunting, and the best word of the others she has (`Player::hunt_state`)
    pub hunt: Option<String>,
}

/// The game she's one of the players of.
#[derive(Clone, Debug, Default)]
pub struct Team {
    /// her own number (0-based), and how many there are
    pub me: usize,
    /// a deathmatch, not co-op: the others are her rivals
    pub rivals: Option<Match>,
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
        if let Some(m) = &self.rivals {
            let limit = match (m.frag_limit, m.seconds) {
                (0, s) if s <= 0.0 => "no limit".to_string(),
                (0, s) => format!("after {:.0} minutes", s / 60.0),
                (f, s) if s <= 0.0 => format!("to {f} frags"),
                (f, s) => format!("to {f} frags or {:.0} minutes", s / 60.0),
            };
            let items = if m.mode == 2 {
                "Everything taken comes back 30 s later."
            } else {
                "Weapons stay where they lie; the rest doesn't come back."
            };
            return format!(
                "Deathmatch: you're {} of {}; the others are your rivals. Most frags wins ({limit}). \
                 Killed, you're back at once at a random start with a pistol. {items} No exit. \
                 You hear rivals: their guns, grunts, doors; you see them only in sight. \
                 Doing nothing by instinct: doom_hunt to hunt them or gear up.",
                who(self.me),
                self.players
            );
        }
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
        if let Some(m) = &team.rivals {
            // (each tic: the score as she's just been told it)
            self.team_line = standings(&team, m);
            self.team = Some(team);
            return;
        }
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

    /// A deathmatch: a frag, as Doom tells every player (`by` fragged `of`; the same, a
    /// suicide), with the score after.
    pub fn frag_news(&mut self, tic: i64, by: usize, of: usize, frags: &[i32]) {
        let Some(team) = &self.team else { return };
        let me = team.me;
        let name = |p: usize| if p == me { "you".to_string() } else { who(p) };
        let text = if by == of {
            format!(
                "{} died by {} own hand.",
                cap(&name(by)),
                if by == me { "your" } else { "their" }
            )
        } else if by == me {
            format!("You fragged {}.", who(of))
        } else if of == me {
            format!("{} fragged you.", cap(&who(by)))
        } else {
            format!("{} fragged {}.", cap(&who(by)), who(of))
        };
        let mine = frags.get(me).copied().unwrap_or(0);
        let best = (0..frags.len())
            .filter(|&p| p != me)
            .map(|p| frags[p])
            .max()
            .unwrap_or(0);
        let score = match mine.cmp(&best) {
            std::cmp::Ordering::Greater => format!(" You lead, {mine} to {best}."),
            std::cmp::Ordering::Equal => format!(" Level at {mine}."),
            std::cmp::Ordering::Less => format!(" You trail, {mine} to {best}."),
        };
        self.reports.push(Report {
            tic,
            ok: true,
            text: format!("{text}{score}"),
            event: if by == me && by != of {
                Event::Frag
            } else {
                Event::Team
            },
        });
    }

    /// A deathmatch over (the frags or the time): who won, as the game's end says.
    pub fn match_over(&mut self, tic: i64, frags: &[i32]) {
        let Some(team) = &self.team else { return };
        let me = team.me;
        let top = frags.iter().copied().max().unwrap_or(0);
        let winners: Vec<usize> = (0..frags.len()).filter(|&p| frags[p] == top).collect();
        let mine = frags.get(me).copied().unwrap_or(0);
        let text = if winners == [me] {
            format!("Match over: you won, with {mine} frags.")
        } else if winners.contains(&me) {
            format!("Match over: a draw at {mine} frags.")
        } else {
            format!(
                "Match over: {} won with {top}; you had {mine}.",
                who(winners[0])
            )
        };
        self.reports.push(Report {
            tic,
            ok: true,
            text,
            event: Event::Team,
        });
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
            text: if self.team.as_ref().is_some_and(|t| t.rivals.is_some()) {
                format!("You're back, at a random start, with a pistol.{known}")
            } else {
                format!("You're back at your start with a pistol. The level's as it was.{known}")
            },
            event: Event::Reborn,
        });
    }
}

/// A deathmatch's line in her status: the frags, best first, her own "you", the clock, and her
/// hunting ("frags: player 1 (green) 7, you 5, player 3 (brown) 2; 4:12 left; hunting: a
/// shotgun 600 off, 3s ago").
fn standings(team: &Team, m: &Match) -> String {
    let mut order: Vec<usize> = (0..m.frags.len()).collect();
    order.sort_by_key(|&p| std::cmp::Reverse(m.frags[p]));
    let parts: Vec<String> = order
        .iter()
        .map(|&p| {
            let name = if p == team.me {
                "you".to_string()
            } else {
                who(p)
            };
            format!("{name} {}", m.frags[p])
        })
        .collect();
    let mut line = format!("frags: {}", parts.join(", "));
    if m.frag_limit > 0 {
        line.push_str(&format!(" (to {})", m.frag_limit));
    }
    if m.seconds > 0.0 {
        let left = (m.seconds - m.played).max(0.0) as i64;
        line.push_str(&format!("; {}:{:02} left", left / 60, left % 60));
    }
    if let Some(h) = &m.hunt {
        line.push_str(&format!("; {h}"));
    }
    line
}

/// "Player 2 (indigo)": a name to begin a sentence with.
fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| {
        f.to_uppercase().collect::<String>() + c.as_str()
    })
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
