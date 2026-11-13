// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The stream console (docs/CONSOLE.md): the controls for whoever runs the stream, in a web page. `server`:
//! `evil-boom console`, the supervisor (the page, the settings, the game as a child process);
//! `game`: the game's end of the link (`watch --control ADDR`). Between them, JSON a line over a
//! local TCP connection: `FromGame` up, `Command` down.

pub mod game;
pub mod server;

use serde::{Deserialize, Serialize};

/// The protocol's version, in the game's hello: a supervisor and a game built apart may not agree.
pub const PROTOCOL: u32 = 7;

/// What the console asks of the running game, applied by it at the top of a frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Pause,
    Resume,
    /// times the game's own speed (the watcher's 0.25-16)
    Speed {
        x: f64,
    },
    Mute {
        on: bool,
    },
    /// the level from its start again, no life spent (what she remembers of it kept, as a death
    /// keeps it)
    RestartLevel,
    /// the link to Neuro dropped and made again (her orders and names kept)
    NeuroReconnect,
    /// another level of the WAD, in the same game process; `carry`: what she has (else as the
    /// game was started: a pistol start)
    GotoMap {
        map: String,
        #[serde(default)]
        carry: bool,
    },
    /// how soon a decision's forced on her (Neuro's), in seconds: waiting with nothing to do
    /// (`idle`), a question unanswered (`ask`); 0 never; one left out is left as it is
    Forcing {
        #[serde(default)]
        idle: Option<f64>,
        #[serde(default)]
        ask: Option<f64>,
    },
    /// who's at the console, as Evil's told of what they do and say
    ConsoleName {
        name: String,
    },
    /// the sound's loudness: the game's own sfx and music volumes (0-15) and a master over the
    /// mix (0-100); one left out is left as it is
    Volume {
        #[serde(default)]
        sfx: Option<i32>,
        #[serde(default)]
        music: Option<i32>,
        #[serde(default)]
        master: Option<i32>,
    },
    /// damage does nothing to her (rehearsals), or does again
    God {
        on: bool,
    },
    /// a line from whoever's at the console to Evil (Neuro's context; directed by Neuro only)
    Say {
        text: String,
    },
    /// an action as if from Evil (`doom_go`, ...; `data` its JSON, as Neuro sends it); directed by
    /// Neuro only
    Order {
        action: String,
        #[serde(default)]
        data: Option<String>,
    },
    /// the level left by its exit (`secret`: the secret one), as a switch leaves it: the tally,
    /// the finale, the next level (with Doom's own screens and the episode on)
    ExitLevel {
        #[serde(default)]
        secret: bool,
    },
    /// the pictures a page is showing ("map", "game", "stream"), made for the next few seconds
    /// only: the console says again while a page goes on asking (none made otherwise, their
    /// drawing costing the window's frames)
    Views {
        names: Vec<String>,
    },
    /// the stream window's column to this side ("left", "right")
    Panel {
        side: String,
    },
    /// the games hers to start (`control::game::Choices::parse`'s words, every part given);
    /// none: none
    Choices {
        #[serde(default)]
        spec: Option<String>,
    },
    /// from the title card through the menu to the run's first level (Doom's own screens on);
    /// from anything else, the run again from the title card
    Begin,
    /// close the game cleanly
    Quit,
}

/// What the game tells the console.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FromGame {
    /// first, once: who it is
    Hello {
        protocol: u32,
        pid: u32,
        commit: String,
        wad: String,
        map: String,
        /// the WAD's levels, in its order
        #[serde(default)]
        maps: Vec<String>,
    },
    /// how things stand, a few times a second
    State(Box<GameState>),
    /// the log's new lines
    Log { lines: Vec<String> },
    /// something that happened: "died", "exited", "level MAP02", ...
    Event { what: String },
    /// a picture of part of the watcher's window, a PNG (base64): "map" (the map as she knows it,
    /// her route and objectives, the panel under it), "game" (the game's own view)
    View { name: String, png: String },
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Bytes as base64 (standard alphabet, padded): a picture in a JSON line.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 back to bytes; None if it isn't.
pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        let v = B64.iter().position(|&x| x == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// The game as the console shows it (what whoever runs it needs, not what the bot knows).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GameState {
    pub map: String,
    pub seconds: f64,
    pub health: i32,
    pub armor: i32,
    /// bullets, shells, rockets, cells
    pub ammo: [i32; 4],
    pub keys: Vec<String>,
    /// (got, of): kills, items, secrets
    pub kills: (i32, i32),
    pub items: (i32, i32),
    pub secrets: (i32, i32),
    pub dead: bool,
    pub exited: bool,
    pub paused: bool,
    pub speed: f64,
    pub muted: bool,
    /// damage does nothing to her
    pub god: bool,
    /// sfx, music (0-15), master (0-100); none from an engine without the control
    pub volume: Option<[i32; 3]>,
    /// what's on the screen: "title", "menu", "level", "dead", "between levels", "over", ...
    pub screen: String,
    /// who directs her: "bot" (on her own), "orders file", "neuro"
    pub mode: String,
    /// what she's doing, in a few words
    pub doing: String,
    pub neuro: Option<NeuroState>,
    /// Evil, if she's directing
    #[serde(default)]
    pub evil: Option<EvilState>,
}

/// Evil as the console shows her (directed by Neuro or a file): her standing settings and goal,
/// her open question, why the bot's doing what it's doing, what she's been told and what she's
/// done lately, how much of it a minute; the console's last action's answer in full.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvilState {
    pub goal: Option<String>,
    /// her settings in a line ("stance careful, save cells, ...")
    pub policy: String,
    /// with no action: "on by instinct after 3 s", or "waits"
    pub idle: String,
    /// the stream column's lines (stance, what she's said in the fight, a goal)
    pub calls: Vec<String>,
    pub question: Option<QuestionState>,
    /// `doom_why`, a second old at most
    pub why: Vec<String>,
    /// how dangerous it is now ("safe for now", "dangerous", "deadly")
    pub danger: String,
    /// what she's been told lately, newest last
    pub told: Vec<ToldLine>,
    /// her actions lately and what came back, newest last
    pub actions: Vec<ActionLine>,
    /// a minute's worth: her actions, words told her, moments (not silent)
    pub per_minute: [u32; 3],
    /// how soon a decision's forced on her, in seconds: waiting with nothing to do, a question
    /// unanswered (0: never)
    #[serde(default)]
    pub forcing: [f64; 2],
    /// the console's last action's answer: (its number, the action, done, the answer)
    pub reply: Option<(u64, String, bool, String)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuestionState {
    pub id: u32,
    pub text: String,
    pub choices: Vec<String>,
    pub default: usize,
    /// seconds till the default (None: it waits for her)
    pub left: Option<f64>,
    /// one that can't wait: the default's already being done
    pub now: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToldLine {
    pub seconds: f64,
    pub text: String,
    /// told to be heard (not silent)
    pub loud: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActionLine {
    pub seconds: f64,
    pub name: String,
    pub data: String,
    pub ok: bool,
    pub reply: String,
    /// given from the console, as hers
    pub by_console: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NeuroState {
    pub url: String,
    pub connected: bool,
    /// who's playing, from the startup's acknowledgement ("Evil Neuro (evil)")
    pub who: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_read_as_sent() {
        assert_eq!(
            serde_json::from_str::<Command>(r#"{"cmd":"speed","x":2}"#).ok(),
            Some(Command::Speed { x: 2.0 })
        );
        assert_eq!(
            serde_json::from_str::<Command>(r#"{"cmd":"restart_level"}"#).ok(),
            Some(Command::RestartLevel)
        );
        assert!(serde_json::from_str::<Command>(r#"{"cmd":"explode"}"#).is_err());
        assert!(serde_json::from_str::<Command>("not json").is_err());
        assert_eq!(
            serde_json::from_str::<Command>(r#"{"cmd":"goto_map","map":"MAP07"}"#).ok(),
            Some(Command::GotoMap {
                map: "MAP07".into(),
                carry: false
            })
        );
        let back = serde_json::to_string(&Command::Mute { on: true }).unwrap_or_default();
        assert_eq!(back, r#"{"cmd":"mute","on":true}"#);
    }

    #[test]
    fn base64_both_ways() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        for n in 0..40u8 {
            let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(unbase64(&base64(&bytes)).as_deref(), Some(&bytes[..]));
        }
        assert_eq!(unbase64("T!=="), None);
    }

    #[test]
    fn game_lines_read_back() {
        let line = serde_json::to_string(&FromGame::State(Box::new(GameState {
            map: "MAP01".into(),
            health: 100,
            ..GameState::default()
        })))
        .unwrap_or_default();
        match serde_json::from_str::<FromGame>(&line) {
            Ok(FromGame::State(s)) => assert_eq!((s.map.as_str(), s.health), ("MAP01", 100)),
            other => panic!("{other:?}"),
        }
    }
}
