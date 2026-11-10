//! The watcher under the stream console (`watch --control ADDR`, docs/CONSOLE_PLAN.md): the
//! console's commands applied at the top of a frame, as the window's own keys are; the game's
//! state, log and events sent back. A child of the viewer for `Watch`'s insides.

use super::{enter_level, load_level, Watch, WatchOptions};
use crate::console::game::Link;
use crate::wad::Wad;
use crate::console::{Command, FromGame, GameState, NeuroState};

/// a streamer's line to Evil: at most this long (her context is precious)
const SAY_MAX: usize = 300;

/// The link and what's been sent over it.
pub(super) struct Console {
    link: Link,
    /// her log's lines sent so far
    shown: usize,
    /// (map, dead, exited) as last told: what's changed is an event
    told: (String, bool, bool),
}

impl Console {
    pub(super) fn connect(addr: &str, wad: &str, map: &str, w: &Wad) -> Result<Console, String> {
        Ok(Console { link: Link::connect(addr, wad, map, maps(w))?, shown: 0, told: (String::new(), false, false) })
    }

    /// A new level: her log starts again (a new agent's).
    pub(super) fn new_level(&mut self) {
        self.shown = 0;
    }
}

/// What the window's loop is to do after the console's commands.
pub(super) enum Frame {
    Go,
    Quit,
    /// another level of the WAD (`carry`: what she has, else as the run started)
    Goto { map: String, carry: bool },
}

/// The WAD's levels, in its order (E1M1 ..., MAP01 ...).
fn maps(w: &Wad) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in &w.lumps {
        let n = l.name.to_uppercase();
        let shaped = (n.len() == 4 && n.starts_with('E') && n.as_bytes()[2] == b'M') || (n.len() == 5 && n.starts_with("MAP"));
        if shaped && crate::engine::episode_map(&n).is_ok() && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// The console's other level, in the same game process: the map made ready first, so one that
/// can't be (no such level in the WAD) leaves her playing on where she is, told why.
pub(super) fn goto(mut watch: Watch, game: Option<(&Wad, &WatchOptions)>, map: &str, carry: bool) -> Result<Watch, String> {
    let map = map.trim().to_uppercase();
    let Some((w, o)) = game else {
        watch.agent.x_mut().log.push("console: no other level here".into());
        return Ok(watch);
    };
    match load_level(w, &map, o) {
        Ok(loaded) => {
            let carried = carry.then(|| watch.engine.loadout());
            let mut next = enter_level(watch, w, o, &map, loaded, carried)?;
            next.agent.x_mut().log.push(format!("console: on to {map}{}", if carry { ", what she had carried" } else { "" }));
            Ok(next)
        }
        Err(e) => {
            watch.agent.x_mut().log.push(format!("console: not to {map}: {e}"));
            Ok(watch)
        }
    }
}

impl Watch {
    /// The console's commands carried out; what's new sent to it; what the loop's to do.
    pub(super) fn console_frame(&mut self) -> Frame {
        let Some(mut c) = self.console.take() else { return Frame::Go };
        let mut frame = Frame::Go;
        // (till one that ends the frame: another level or quitting; what's after it waits for the
        // next frame, the level it's for then the one there)
        while matches!(frame, Frame::Go) {
            let Some(cmd) = c.link.command() else { break };
            match cmd {
                Command::Pause => self.paused = true,
                Command::Resume => self.paused = false,
                Command::Speed { x } => self.speed = x.clamp(0.25, 16.0),
                Command::Mute { on } => self.muted = on,
                Command::RestartLevel => {
                    // (as a death restarts it, but no life spent: the streamer's call)
                    self.engine.restart(true);
                    self.agent.restart();
                    self.controls = None;
                    self.agent.x_mut().log.push("console: the level restarted".into());
                }
                Command::NeuroReconnect => match self.neuro.as_mut() {
                    Some(n) => {
                        n.reconnect();
                        self.agent.x_mut().log.push("console: reconnecting to Neuro".into());
                    }
                    None => self.agent.x_mut().log.push("console: not directed by Neuro: nothing to reconnect".into()),
                },
                Command::GotoMap { map, carry } => frame = Frame::Goto { map, carry },
                Command::God { on } => {
                    self.engine.set_god(on);
                    self.agent.x_mut().log.push(format!("console: god mode {}", if on { "on" } else { "off" }));
                }
                Command::Lives { n } => {
                    self.lives_left = n.clamp(0, 99);
                    self.agent.x_mut().log.push(format!("console: {} lives left", self.lives_left));
                }
                Command::Say { text } => {
                    let text: String = text.trim().chars().take(SAY_MAX).collect();
                    let line = match self.neuro.as_ref() {
                        Some(n) if !text.is_empty() => {
                            n.say(&text);
                            format!("streamer to Evil: {text}")
                        }
                        Some(_) => "console: nothing to say".into(),
                        None => "console: not directed by Neuro: no one to tell".into(),
                    };
                    self.agent.x_mut().log.push(line);
                }
                Command::Order { action, data } => {
                    let lines = match (self.neuro.as_mut(), self.obs.as_ref()) {
                        (Some(n), Some(obs)) => {
                            let (ok, msg) = n.streamer_action(&mut self.agent, obs, &action, data.as_deref());
                            vec![format!(">> (streamer) {action} {}", data.as_deref().unwrap_or("")), format!("<< {}{}", if ok { "" } else { "FAILED: " }, msg.lines().next().unwrap_or(""))]
                        }
                        (None, _) => vec!["console: not directed by Neuro: no orders to give".into()],
                        (_, None) => vec!["console: the game hasn't started yet".into()],
                    };
                    self.agent.x_mut().log.extend(lines);
                }
                Command::Quit => frame = Frame::Quit,
            }
        }
        if let Some(why) = c.link.lost() {
            self.agent.x_mut().log.push(format!("console: the link's gone ({why}); playing on"));
        }
        // (her log since: a new agent, a new level's or a restart's, starts it again)
        let log = &self.agent.x().log;
        if log.len() < c.shown {
            c.shown = 0;
        }
        if log.len() > c.shown {
            c.link.send(FromGame::Log { lines: log[c.shown..].to_vec() });
            c.shown = log.len();
        }
        let now = (self.map_name.clone(), self.engine.dead(), self.engine.exited());
        if now.0 != c.told.0 {
            c.link.send(FromGame::Event { what: format!("level {}", now.0) });
        }
        if now.1 && !c.told.1 {
            c.link.send(FromGame::Event { what: "died".into() });
        }
        if now.2 && !c.told.2 {
            c.link.send(FromGame::Event { what: "exited".into() });
        }
        c.told = now;
        if c.link.state_due() {
            c.link.send(FromGame::State(self.console_state()));
        }
        self.console = Some(c);
        frame
    }

    fn console_state(&self) -> GameState {
        let t = self.engine.tally();
        let mut s = GameState {
            map: self.map_name.clone(),
            seconds: self.engine.tic as f64 / crate::motor::TICRATE,
            items: t[0],
            kills: t[1],
            secrets: t[2],
            lives_left: self.lives_left,
            dead: self.engine.dead(),
            exited: self.engine.exited(),
            paused: self.paused,
            speed: self.speed,
            // (no sound at all, as a headless or --mute game has, is muted too)
            muted: self.muted || !self.sound,
            god: self.engine.god(),
            ..GameState::default()
        };
        if let Some(o) = &self.obs {
            s.health = o.health.unwrap_or(0.0).round() as i32;
            s.armor = o.armor.unwrap_or(0.0).round() as i32;
            let ammo = |slot: i32| o.ammo.iter().flatten().find(|(k, _)| *k == slot).map_or(0, |(_, n)| *n);
            s.ammo = [ammo(2), ammo(3), ammo(5), ammo(6)];
            s.keys = o.keys.unwrap_or_default().iter().map(|k| format!("{k:?}")).collect();
        }
        (s.mode, s.doing) = if let Some(n) = &self.neuro {
            let (url, connected, who) = n.link_state();
            s.neuro = Some(NeuroState { url: url.to_string(), connected, who: who.to_string() });
            ("neuro".into(), n.director.doing(&self.agent))
        } else if let Some(f) = &self.feed {
            ("orders file".into(), f.director.doing(&self.agent))
        } else {
            ("bot".into(), self.agent.x().current.as_ref().map_or_else(|| "-".into(), |o| o.label.clone()))
        };
        s
    }
}
