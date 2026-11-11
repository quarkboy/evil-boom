//! The rest of the moments: a level begun and done; a big jump ahead, and how it came down; a
//! teleport into a crowd; out of ammo in a fight.

use super::{compass, engaged, kit, some, Director};
use crate::geom::{dist, Point};
use crate::motor::follower::JumpNews;
use crate::motor::{heading, Observation};
use crate::navigator::Jump;
use crate::play::Agent;
use std::collections::BTreeMap;

/// a run-jump told before she runs at it: this far across...
const BIG_ACROSS: f64 = 128.0;
/// ...or with this far to fall if it's missed, or a floor below that hurts
const BIG_DROP: f64 = 64.0;
/// a jump this sure or less is "a long shot"
const LONG_SHOT: f64 = 0.8;
/// the same jump lined up for again within this (tics) isn't told again
const JUMP_AGAIN: i64 = 20 * 35;
/// moved this far in a tic: a teleport
const TELEPORT: f64 = 64.0;
/// ...and this many fighting her within this long (tics) of it: teleported into them
const CROWD: usize = 3;
const CROWD_WITHIN: i64 = 35;
/// a level's title waited for this long (tics) at its start, then begun without it
const TITLE_WAIT: i64 = 35;

/// What the runner tells the director of the level, as it tells it the tally.
#[derive(Clone, Debug, Default)]
pub struct Level {
    /// "E1M2", "MAP07"
    pub map: String,
    /// the automap's title ("level 7: dead simple"), when the game has it
    pub title: Option<String>,
    /// 1-5
    pub skill: i32,
}

#[derive(Default)]
pub(super) struct Moments {
    /// when this level began (its first tic seen), and its start told
    began: Option<i64>,
    told_start: bool,
    /// the big jumps told (by key), and when
    jumps_told: BTreeMap<String, i64>,
    /// where she was last tic, and when she last teleported
    was_at: Option<Point>,
    teleported: Option<i64>,
    /// out of ammo told, this fight
    told_dry: bool,
    /// her kit, as it stands (for the level's end, told after the last tic)
    kit: String,
    last_tic: i64,
    /// the level's end told (it's told from wherever the level's left: once)
    told_done: bool,
}

impl Director {
    /// What the game's screen says, between levels (a finale's story, an episode's end): told
    /// as it is, its lines run together.
    pub fn on_screen(&mut self, text: &str) {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let tic = self.moments.last_tic;
        self.reports.push(super::Report {
            tic,
            ok: true,
            text: format!("On screen: \"{text}\""),
            notable: true,
            urgent: true,
        });
    }

    /// The game over: no lives left.
    pub fn game_over(&mut self) {
        let tic = self.moments.last_tic;
        self.reports.push(super::Report {
            tic,
            ok: true,
            text: "Game over: no lives left.".into(),
            notable: true,
            urgent: true,
        });
    }
}

/// A skill as the game's menu has it.
fn skill_name(skill: i32) -> &'static str {
    match skill {
        1 => "I'm too young to die",
        2 => "Hey, not too rough",
        3 => "Hurt me plenty",
        4 => "Ultra-Violence",
        _ => "Nightmare!",
    }
}

/// "Dead Simple" from "level 7: dead simple".
fn title_of(t: &str) -> String {
    let name = t.split_once(':').map_or(t, |(_, n)| n).trim();
    name.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Director {
    /// Each tic: the level begun, jumps, teleports, a fight gone dry.
    pub(super) fn more_moments(&mut self, agent: &mut Agent, obs: &Observation) {
        let mut m = std::mem::take(&mut self.moments);
        let pos = obs.pos();
        m.last_tic = obs.tic;
        m.kit = kit(obs);

        // The level begun: once its title's to be had (or a moment's gone without).
        let began = *m.began.get_or_insert(obs.tic);
        if !m.told_start {
            let title = self.level.title.clone();
            if title.is_some() || obs.tic - began >= TITLE_WAIT {
                m.told_start = true;
                let name = title.map_or(String::new(), |t| format!(": {}", title_of(&t)));
                self.event(
                    obs,
                    true,
                    format!(
                        "{}{name}. Skill: {}. You come in with: {}.",
                        self.level.map,
                        skill_name(self.level.skill),
                        m.kit
                    ),
                );
            }
        }

        // Jumps: a big one before she runs at it, and how it came down.
        let news = std::mem::take(&mut agent.x_mut().jumps);
        for n in news {
            match n {
                JumpNews::Going(j) => {
                    let key = j.key();
                    if m.jumps_told
                        .get(&key)
                        .is_some_and(|&t| obs.tic - t < JUMP_AGAIN)
                    {
                        continue;
                    }
                    if let Some(line) = self.big_jump(agent, obs, &j) {
                        m.jumps_told.insert(key, obs.tic);
                        self.event(obs, true, line);
                    }
                }
                JumpNews::Made(j) => {
                    if m.jumps_told.contains_key(&j.key()) {
                        self.event(obs, true, "Made the jump.".into());
                    }
                }
                JumpNews::Missed(j, sector) => {
                    let nav = std::rc::Rc::clone(&agent.x().nav);
                    let want = nav.sector_of_poly(j.dest_poly);
                    let places = self.places(agent);
                    let area = places.area_of(sector).clone();
                    // (down off the planned spot but in the same room, at heights she walks
                    // between: made, for anyone watching)
                    if places.area_of(want).id == area.id {
                        if m.jumps_told.contains_key(&j.key()) {
                            self.event(obs, true, "Made the jump.".into());
                        }
                        continue;
                    }
                    let hurts = if area.hurts { ", the floor hurts" } else { "" };
                    let what = self.tag(&area.id, obs);
                    self.event(
                        obs,
                        true,
                        format!("Missed the jump: came down in {what}{hurts}."),
                    );
                }
            }
        }

        // A teleport into a crowd.
        if m.was_at.is_some_and(|p| dist(p, pos) > TELEPORT) {
            m.teleported = Some(obs.tic);
        }
        m.was_at = Some(pos);
        if let Some(t) = m.teleported {
            let near = engaged(agent, obs);
            if near.len() >= CROWD {
                m.teleported = None;
                let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
                for t in &near {
                    *counts.entry(t.name).or_insert(0) += 1;
                }
                let them: Vec<String> = counts.iter().map(|(n, c)| some(*c, n)).collect();
                self.alarm(obs, format!("Teleported right into {}!", them.join(", ")));
            } else if obs.tic - t > CROWD_WITHIN {
                m.teleported = None;
            }
        }

        // Out of ammo in a fight: down to the fist (or the chainsaw).
        if self.fight.is_none() {
            m.told_dry = false;
        } else if !m.told_dry
            && agent
                .player()
                .is_some_and(|p| !p.combat().now.guns_armed && !p.combat().now.engaged.is_empty())
        {
            m.told_dry = true;
            let what = if obs.chainsaw == Some(true) {
                "the chainsaw"
            } else {
                "your fists"
            };
            self.alarm(obs, format!("Out of ammo mid-fight: down to {what}."));
        }
        self.moments = m;
    }

    /// A run-jump worth telling before she runs at it, as a line: far across, a long way down if
    /// it's missed, or onto a floor that hurts. None for a hop.
    fn big_jump(&mut self, agent: &Agent, obs: &Observation, j: &Jump) -> Option<String> {
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let floor = |p: Point| {
            nav.mesh
                .poly_at(p)
                .map(|i| nav.sector_of_poly(i))
                .map(|s| (s, obs.heights[s].floor))
        };
        let across = dist(j.takeoff, j.dest);
        let mid = (
            (j.takeoff.0 + j.dest.0) / 2.0,
            (j.takeoff.1 + j.dest.1) / 2.0,
        );
        let (_, from) = floor(j.takeoff)?;
        let below = floor(mid);
        let drop = below.map_or(0.0, |(_, f)| from - f);
        let hurts = below.is_some_and(|(s, _)| nav.damage[s] != 0);
        if across < BIG_ACROSS && drop < BIG_DROP && !hurts {
            return None;
        }
        let to = {
            let s = nav.sector_of_poly(j.dest_poly);
            let area = self.places(agent).area_of(s).clone();
            self.tag(&area.id, obs)
        };
        let under = if hurts {
            ", a floor that hurts below".to_string()
        } else if drop >= BIG_DROP {
            format!(", a {drop:.0} drop below")
        } else {
            String::new()
        };
        let odds = if j.sure <= LONG_SHOT {
            ", a long shot"
        } else {
            ""
        };
        Some(format!(
            "Jumping for it: {across:.0} across to {to} ({}){under}{odds}.",
            compass(heading(obs.pos(), j.dest))
        ))
    }

    /// The level done (by its exit): told, with the time it took, the tally and what she
    /// leaves with. From the runner, before the next level's begun.
    pub fn level_done(&mut self) {
        let m = &mut self.moments;
        let Some(began) = m.began.filter(|_| !m.told_done) else {
            return;
        };
        m.told_done = true;
        let secs = (m.last_tic - began) / 35;
        let text = format!(
            "{} done in {}:{:02}: {}. You leave with: {}.",
            self.level.map,
            secs / 60,
            secs % 60,
            self.tally.line(),
            m.kit
        );
        let tic = m.last_tic;
        self.reports.push(super::Report {
            tic,
            ok: true,
            text,
            notable: true,
            urgent: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::title_of;

    #[test]
    fn titles() {
        assert_eq!(title_of("level 7: dead simple"), "Dead Simple");
        assert_eq!(title_of("E1M1: Hangar"), "Hangar");
    }
}
