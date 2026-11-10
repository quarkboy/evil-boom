//! The stream console (docs/CONSOLE_PLAN.md): the streamer's controls in a web page. `server`:
//! `evil-boom console`, the supervisor (the page, the settings, the game as a child process);
//! `game`: the game's end of the link (`watch --control ADDR`). Between them, JSON a line over a
//! local TCP connection: `FromGame` up, `Command` down.

pub mod game;
pub mod server;

use serde::{Deserialize, Serialize};

/// The protocol's version, in the game's hello: a supervisor and a game built apart may not agree.
pub const PROTOCOL: u32 = 2;

/// What the console asks of the running game, applied by it at the top of a frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Pause,
    Resume,
    /// times the game's own speed (the watcher's 0.25-16)
    Speed { x: f64 },
    Mute { on: bool },
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
    God { on: bool },
    /// lives left from now (a death with none left ends the game)
    Lives { n: i32 },
    /// a line from the streamer to Evil (Neuro's context; directed by Neuro only)
    Say { text: String },
    /// an action as if from Evil (`doom_go`, ...; `data` its JSON, as Neuro sends it); directed by
    /// Neuro only
    Order {
        action: String,
        #[serde(default)]
        data: Option<String>,
    },
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
    State(GameState),
    /// the log's new lines
    Log { lines: Vec<String> },
    /// something that happened: "died", "exited", "level MAP02", ...
    Event { what: String },
}

/// The game as the console shows it (what the streamer needs, not what the bot knows).
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
    pub lives_left: i32,
    pub dead: bool,
    pub exited: bool,
    pub paused: bool,
    pub speed: f64,
    pub muted: bool,
    /// damage does nothing to her
    pub god: bool,
    /// sfx, music (0-15), master (0-100); none from an engine without the control
    pub volume: Option<[i32; 3]>,
    /// who directs her: "bot" (on her own), "orders file", "neuro"
    pub mode: String,
    /// what she's doing, in a few words
    pub doing: String,
    pub neuro: Option<NeuroState>,
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
        assert_eq!(serde_json::from_str::<Command>(r#"{"cmd":"speed","x":2}"#).ok(), Some(Command::Speed { x: 2.0 }));
        assert_eq!(serde_json::from_str::<Command>(r#"{"cmd":"restart_level"}"#).ok(), Some(Command::RestartLevel));
        assert!(serde_json::from_str::<Command>(r#"{"cmd":"explode"}"#).is_err());
        assert!(serde_json::from_str::<Command>("not json").is_err());
        assert_eq!(serde_json::from_str::<Command>(r#"{"cmd":"goto_map","map":"MAP07"}"#).ok(), Some(Command::GotoMap { map: "MAP07".into(), carry: false }));
        let back = serde_json::to_string(&Command::Mute { on: true }).unwrap_or_default();
        assert_eq!(back, r#"{"cmd":"mute","on":true}"#);
    }

    #[test]
    fn game_lines_read_back() {
        let line = serde_json::to_string(&FromGame::State(GameState { map: "MAP01".into(), health: 100, ..GameState::default() })).unwrap_or_default();
        match serde_json::from_str::<FromGame>(&line) {
            Ok(FromGame::State(s)) => assert_eq!((s.map.as_str(), s.health), ("MAP01", 100)),
            other => panic!("{other:?}"),
        }
    }
}
