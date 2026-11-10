//! The watcher under the stream console (`watch --control ADDR`, docs/CONSOLE_PLAN.md): the
//! console's commands applied at the top of a frame, as the window's own keys are; the game's
//! state, log and events sent back. A child of the viewer for `Watch`'s insides.

use super::Watch;
use crate::console::game::Link;
use crate::console::{Command, FromGame, GameState, NeuroState};

/// The link and what's been sent over it.
pub(super) struct Console {
    link: Link,
    /// her log's lines sent so far
    shown: usize,
    /// (map, dead, exited) as last told: what's changed is an event
    told: (String, bool, bool),
}

impl Console {
    pub(super) fn connect(addr: &str, wad: &str, map: &str) -> Result<Console, String> {
        Ok(Console { link: Link::connect(addr, wad, map)?, shown: 0, told: (String::new(), false, false) })
    }
}

impl Watch {
    /// The console's commands carried out; what's new sent to it. False: it asked the game to
    /// quit.
    pub(super) fn console_frame(&mut self) -> bool {
        let Some(mut c) = self.console.take() else { return true };
        let mut go_on = true;
        for cmd in c.link.commands() {
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
                Command::Quit => go_on = false,
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
        go_on
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
