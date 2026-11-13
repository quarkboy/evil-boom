// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The game itself, as hers: which level to start, at which skill, and whether just that level
//! or on through its episode (Doom II: through the game), offered at the title and again once
//! what she chose is over; on through, the next level when she says so at the tally screen; the
//! level again from its start once she's made no progress for a while. What's hers is the
//! stream console's to say (`--choices`), and may be no more than one level at one skill.

/// What she may choose from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    /// the levels she may start at (none: she isn't offered a game)
    pub maps: Vec<String>,
    /// the skills, 1-5
    pub skills: Vec<i32>,
    /// just the level chosen
    pub single: bool,
    /// on from it through its episode (Doom II: through the game)
    pub continuous: bool,
    /// a game hers to start at all (off: only next and restart, as set)
    pub start: bool,
    /// on through, the next level hers to go on to from the tally screen
    pub next: bool,
    /// the level again from its start hers, once she's made no progress for a while
    pub restart: bool,
}

/// What she asks of the game (`doom_game`'s `do`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameAct {
    Start {
        map: Option<String>,
        skill: Option<i32>,
        play: Option<String>,
    },
    Next,
    Restart,
}

impl GameAct {
    /// From `doom_game`'s data.
    pub fn of_json(args: &serde_json::Value) -> Result<GameAct, String> {
        let s = |k: &str| {
            args.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        match s("do").as_deref() {
            Some("start") => Ok(GameAct::Start {
                map: s("map"),
                skill: args
                    .get("skill")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok()),
                play: s("play"),
            }),
            Some("next") => Ok(GameAct::Next),
            Some("restart") => Ok(GameAct::Restart),
            _ => Err("do: start, next or restart.".into()),
        }
    }

    /// From a script's words after `game`: `start [MAP] [SKILL] [single|continuous]`, `next`,
    /// `restart`.
    pub fn of_words(w: &[&str]) -> Result<GameAct, String> {
        match w.first().copied() {
            Some("start") => {
                let play = w
                    .iter()
                    .copied()
                    .find(|v| matches!(*v, "single" | "continuous"));
                Ok(GameAct::Start {
                    skill: w.iter().find_map(|v| v.parse::<i32>().ok()),
                    map: w[1..]
                        .iter()
                        .copied()
                        .find(|v| v.parse::<i32>().is_err() && Some(*v) != play)
                        .map(str::to_string),
                    play: play.map(str::to_string),
                })
            }
            Some("next") => Ok(GameAct::Next),
            Some("restart") => Ok(GameAct::Restart),
            _ => Err(
                "game start [MAP] [SKILL] [single|continuous] | game next | game restart".into(),
            ),
        }
    }
}

/// What she chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub map: String,
    pub skill: i32,
    pub continuous: bool,
}

impl Choices {
    /// None on offer.
    pub fn none() -> Choices {
        Choices {
            maps: Vec::new(),
            skills: Vec::new(),
            single: false,
            continuous: false,
            start: false,
            next: false,
            restart: false,
        }
    }

    /// From the console's words: "maps=E1M1,E1M2;skills=3,4;play=single,continuous" (`all`
    /// for maps: every one in `wad_maps`). What's left out of it: the game as started
    /// (`map`, `skill`, `episode`), the only choice.
    pub fn parse(
        s: &str,
        wad_maps: &[String],
        (map, skill, episode): (&str, i32, bool),
    ) -> Result<Choices, String> {
        let mut c = Choices {
            maps: vec![map.to_uppercase()],
            skills: vec![skill],
            single: !episode,
            continuous: episode,
            start: true,
            next: false,
            restart: false,
        };
        for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| format!("--choices: \"{part}\" isn't key=value"))?;
            let list: Vec<String> = value
                .split(',')
                .map(|v| v.trim().to_uppercase())
                .filter(|v| !v.is_empty())
                .collect();
            match key.trim() {
                "maps" if list == ["ALL"] => c.maps = wad_maps.to_vec(),
                "maps" => {
                    if let Some(m) = list.iter().find(|m| !wad_maps.contains(m)) {
                        return Err(format!("--choices: no level {m} in this WAD"));
                    }
                    c.maps = list;
                }
                "skills" => {
                    c.skills = list
                        .iter()
                        .map(|v| v.parse::<i32>().ok().filter(|n| (1..=4).contains(n)))
                        .collect::<Option<Vec<i32>>>()
                        .ok_or("--choices: skills are 1-4 (nightmare is never hers)")?;
                }
                "start" | "next" | "restart" => {
                    let on = match list.as_slice() {
                        [v] if v == "ON" => true,
                        [v] if v == "OFF" => false,
                        _ => return Err(format!("--choices: {} is on or off", key.trim())),
                    };
                    match key.trim() {
                        "start" => c.start = on,
                        "next" => c.next = on,
                        _ => c.restart = on,
                    }
                }
                "play" => {
                    c.single = list.iter().any(|p| p == "SINGLE");
                    c.continuous = list.iter().any(|p| p == "CONTINUOUS");
                    if !c.single && !c.continuous {
                        return Err("--choices: play is single, continuous or both".into());
                    }
                }
                other => {
                    return Err(format!(
                        "--choices: no \"{other}\" (maps, skills, play, start, next, restart)"
                    ))
                }
            }
        }
        Ok(c)
    }

    /// Anything to choose from at all.
    pub fn offered(&self) -> bool {
        self.start
            && !self.maps.is_empty()
            && !self.skills.is_empty()
            && (self.single || self.continuous)
    }

    /// Her choice checked against what's offered (what she leaves out: the one on offer, when
    /// there's only one).
    pub fn check(
        &self,
        map: Option<&str>,
        skill: Option<i32>,
        play: Option<&str>,
    ) -> Result<Start, String> {
        if !self.offered() {
            return Err("Starting a game isn't yours on this stream.".into());
        }
        let map = match map.map(|m| m.trim().to_uppercase()) {
            Some(m) if self.maps.contains(&m) => m,
            Some(m) => return Err(format!("{m} isn't on offer: {}.", self.maps_line())),
            None if self.maps.len() == 1 => self.maps[0].clone(),
            None => return Err(format!("Which level? {}.", self.maps_line())),
        };
        let skill = match skill {
            Some(s) if self.skills.contains(&s) => s,
            Some(s) => return Err(format!("Skill {s} isn't on offer: {}.", self.skills_line())),
            None if self.skills.len() == 1 => self.skills[0],
            None => return Err(format!("Which skill? {}.", self.skills_line())),
        };
        let continuous = match play.map(|p| p.trim().to_lowercase()).as_deref() {
            Some("continuous") if self.continuous => true,
            Some("single") if self.single => false,
            Some(p @ ("continuous" | "single")) => {
                return Err(format!("{p} isn't on offer: {}.", self.play_line()))
            }
            Some(p) => return Err(format!("play: single or continuous, not \"{p}\".")),
            None if self.single != self.continuous => self.continuous,
            None => return Err(format!("Which? {}.", self.play_line())),
        };
        Ok(Start {
            map,
            skill,
            continuous,
        })
    }

    /// What's on offer, for her to choose from (short: she reads it).
    pub fn offer(&self) -> String {
        format!(
            "Start a game with doom_game. Level: {}. Skill: {}. Play: {}.",
            self.maps_line(),
            self.skills_line(),
            self.play_line()
        )
    }

    fn maps_line(&self) -> String {
        match self.maps.as_slice() {
            [one] => one.clone(),
            many if many.len() > 12 => format!(
                "any of {} ({} to {})",
                many.len(),
                many[0],
                many[many.len() - 1]
            ),
            many => many.join(", "),
        }
    }

    fn skills_line(&self) -> String {
        self.skills
            .iter()
            .map(|&s| format!("{s} ({})", crate::control::directed::skill_name(s)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn play_line(&self) -> String {
        match (self.single, self.continuous) {
            (true, true) => "single (just that level) or continuous (on through its episode)",
            (true, false) => "single (just that level)",
            _ => "continuous (on through its episode)",
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{Choices, Start};

    #[test]
    fn choices_offered_and_checked() {
        let wad: Vec<String> = ["E1M1", "E1M2", "E2M1"].map(String::from).to_vec();
        let only = Choices::parse("", &wad, ("e1m1", 3, false)).unwrap();
        assert_eq!(
            only.check(None, None, None),
            Ok(Start {
                map: "E1M1".into(),
                skill: 3,
                continuous: false
            })
        );
        assert!(only.check(Some("E1M2"), None, None).is_err());
        let wide = Choices::parse(
            "maps=all; skills=3,4; play=single,continuous",
            &wad,
            ("E1M1", 3, false),
        )
        .unwrap();
        assert_eq!(wide.maps.len(), 3);
        assert!(wide.check(Some("e2m1"), Some(4), None).is_err());
        assert_eq!(
            wide.check(Some("e2m1"), Some(4), Some("continuous")),
            Ok(Start {
                map: "E2M1".into(),
                skill: 4,
                continuous: true
            })
        );
        assert!(Choices::parse("maps=E9M9", &wad, ("E1M1", 3, false)).is_err());
        assert!(!Choices::none().offered());
    }
}
