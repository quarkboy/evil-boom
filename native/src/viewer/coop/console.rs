// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A co-op or deathmatch game under the stream console (`watch --players N --control ADDR`,
//! docs/CONSOLE.md): the console's commands that mean something with more than one player
//! carried out at the top of a frame (pause, speed, sound, the picture, god mode, the exit, and
//! to every player's Neuro what's said and set), the rest told as not here; the game's state,
//! each player's in it, her log's lines by player, and what's happened sent back.

use super::Coop;
use crate::console::{Command, FromGame, GameState, NeuroState, PlayerState};
use crate::viewer::console::Console;

/// a line to the Neuros from the console: at most this long (their context is precious)
const SAY_MAX: usize = 300;

/// The link and what's been sent over it.
pub(super) struct Link {
    pub(super) c: Console,
    /// each player's log lines sent so far
    shown: Vec<usize>,
    /// (map, exited, each one dead) as last told: what's changed is an event
    told: (String, bool, Vec<bool>),
    /// orders given before the level had begun (what the console starts each with): given
    /// once every one has looked
    held: Vec<(String, Option<String>)>,
}

impl Link {
    pub(super) fn new(c: Console) -> Link {
        Link {
            c,
            shown: Vec::new(),
            told: (String::new(), false, Vec::new()),
            held: Vec::new(),
        }
    }

    /// A new level: each one's log starts again (a new agent's).
    pub(super) fn new_level(&mut self) {
        self.shown.clear();
    }
}

impl Coop {
    /// The console's commands carried out; what's new sent to it. True: it's said to quit.
    pub(super) fn console_frame(&mut self) -> bool {
        let Some(mut k) = self.console.take() else {
            return false;
        };
        let mut quit = false;
        if self.seats.iter().all(|s| s.obs.is_some()) {
            for (action, data) in std::mem::take(&mut k.held) {
                self.order(&action, data.as_deref());
            }
        }
        while !quit {
            let Some(cmd) = k.c.link().command() else {
                break;
            };
            let line = self.command(cmd, &mut k, &mut quit);
            if let Some(l) = line {
                self.seats[0].agent.x_mut().log.push(l);
            }
        }
        if let Some(why) = k.c.link().lost() {
            self.seats[0]
                .agent
                .x_mut()
                .log
                .push(format!("console: the link's gone ({why}); playing on"));
        }
        self.send_news(&mut k);
        if k.c.link().state_due() {
            let st = self.console_state();
            k.c.link().send(FromGame::State(Box::new(st)));
        }
        self.console = Some(k);
        quit
    }

    /// One command: what's to be logged of it, if anything.
    fn command(&mut self, cmd: Command, k: &mut Link, quit: &mut bool) -> Option<String> {
        match cmd {
            Command::Pause => self.paused = true,
            Command::Resume => self.paused = false,
            Command::Speed { x } => self.speed = x.clamp(0.25, 16.0),
            Command::Mute { on } => self.muted = on,
            Command::Views { names } => k.c.want(names),
            Command::Volume { sfx, music, master } => self.engine.set_volume(sfx, music, master),
            Command::Picture {
                brightness,
                fov,
                flash_red,
                flash_gold,
                flash_green,
            } => {
                if let Some(b) = brightness {
                    crate::viewer::set_brightness(b);
                    self.palettes = crate::viewer::palettes(&self.w)
                        .iter()
                        .map(|p| crate::viewer::brightened(p))
                        .collect();
                }
                crate::viewer::set_flashes([flash_red, flash_gold, flash_green]);
                if let Some(f) = fov {
                    self.engine.set_fov(f);
                }
            }
            Command::God { on } => {
                self.engine.set_god(on);
                return Some(format!(
                    "console: god mode {}",
                    if on { "on" } else { "off" }
                ));
            }
            Command::ExitLevel { secret } => {
                return Some(if self.engine.exit_level(secret) {
                    format!(
                        "console: out by the {}exit",
                        if secret { "secret " } else { "" }
                    )
                } else {
                    "console: this engine can't leave a level by itself".into()
                });
            }
            Command::Quit => *quit = true,
            // (to every player's Neuro: what's said, the link again, what's locked and forced)
            _ if self.neuro.is_empty() && neuros_only(&cmd) => {
                return Some("console: not directed by Neuro".into());
            }
            Command::NeuroReconnect => {
                self.neuro.iter_mut().for_each(|n| n.reconnect());
                return Some("console: reconnecting to each Neuro".into());
            }
            Command::Say { text } => {
                let text: String = text.trim().chars().take(SAY_MAX).collect();
                if text.is_empty() {
                    return Some("console: nothing to say".into());
                }
                self.neuro.iter_mut().for_each(|n| n.say(&text));
                return Some(format!("console to every Evil: {text}"));
            }
            Command::Lock { settings } => {
                self.neuro.iter_mut().for_each(|n| n.set_locked(&settings))
            }
            Command::ConsoleName { name } => {
                for n in &mut self.neuro {
                    n.director.console_name.clone_from(&name);
                }
            }
            Command::Forcing {
                idle,
                ask,
                next,
                instinct,
                answers,
                quiet,
            } => {
                for n in &mut self.neuro {
                    n.set_forcing(idle, ask, next);
                    if instinct == Some(false) {
                        let mut l = n.locked();
                        l.push("instinct".into());
                        n.set_locked(&l);
                    }
                    if let Some(secs) = answers {
                        n.set_answer_wait(secs);
                    }
                    if let Some(q) = quiet {
                        n.set_next_silent(q);
                    }
                }
            }
            Command::Order { action, data } => {
                if self.seats.iter().all(|s| s.obs.is_some()) {
                    self.order(&action, data.as_deref());
                } else {
                    k.held.push((action, data));
                }
            }
            // (the stream window's, and a game hers to choose: this one has neither)
            Command::Sizes { .. }
            | Command::Column { .. }
            | Command::Panel { .. }
            | Command::Choices { .. } => {}
            Command::RestartLevel | Command::GotoMap { .. } | Command::Begin => {
                return Some(format!("console: not in a {} game yet", self.kind()));
            }
        }
        None
    }

    /// An action as if each one's own, from the console (`Command::Order`: what the console
    /// starts them with, her settings and her goal), and what came of it in each one's log.
    fn order(&mut self, action: &str, data: Option<&str>) {
        for (n, s) in self.neuro.iter_mut().zip(self.seats.iter_mut()) {
            let Some(obs) = s.obs.as_ref() else { continue };
            let (ok, msg) = n.console_action(&mut s.agent, obs, action, data);
            let lines = [
                format!(
                    ">> ({}) {action} {}",
                    n.director.console_name,
                    data.unwrap_or("")
                ),
                format!(
                    "<< {}{}",
                    if ok { "" } else { "FAILED: " },
                    msg.lines().next().unwrap_or("")
                ),
            ];
            s.agent.x_mut().log.extend(lines);
        }
    }

    /// Each one's log since, by player; the level, its end and each death, as events.
    fn send_news(&mut self, k: &mut Link) {
        k.shown.resize(self.seats.len(), 0);
        let mut lines = Vec::new();
        for (i, s) in self.seats.iter().enumerate() {
            let log = &s.agent.x().log;
            if log.len() < k.shown[i] {
                k.shown[i] = 0;
            }
            lines.extend(log[k.shown[i]..].iter().map(|l| format!("P{}: {l}", i + 1)));
            k.shown[i] = log.len();
        }
        if !lines.is_empty() {
            k.c.link().send(FromGame::Log { lines });
        }
        let dead: Vec<bool> = self.seats.iter().map(|s| s.dead_since.is_some()).collect();
        let now = (self.map_name.clone(), self.engine.exited(), dead);
        let mut events = Vec::new();
        if now.0 != k.told.0 {
            events.push(format!("level {}", now.0));
        }
        if now.1 && !k.told.1 {
            events.push("exited".into());
        }
        for (i, &d) in now.2.iter().enumerate() {
            if d && !k.told.2.get(i).copied().unwrap_or(false) {
                events.push(format!("player {} died", i + 1));
            }
        }
        for what in events {
            k.c.link().send(FromGame::Event { what });
        }
        k.told = now;
    }

    fn console_state(&mut self) -> GameState {
        // (the level's tally, all of theirs together: each kill or item is one player's)
        let mut tally = [(0, 0); 3];
        let n = self.seats.len();
        let mut kills = vec![0; n];
        for (i, k) in kills.iter_mut().enumerate() {
            self.engine.as_player(i);
            let t = self.engine.tally();
            for (sum, one) in tally.iter_mut().zip(t) {
                *sum = (sum.0 + one.0, one.1);
            }
            *k = t[1].0;
        }
        self.engine.as_player(0);
        let frags = (self.deathmatch > 0).then(|| self.frags());
        let players: Vec<PlayerState> = self
            .seats
            .iter()
            .enumerate()
            .map(|(i, s)| PlayerState {
                player: i + 1,
                health: s.obs.as_ref().and_then(|o| o.health).unwrap_or(0.0).round() as i32,
                armor: s.obs.as_ref().and_then(|o| o.armor).unwrap_or(0.0).round() as i32,
                dead: s.dead_since.is_some(),
                deaths: s.deaths,
                frags: frags.as_ref().map(|f| f[i]),
                kills: kills[i],
                doing: if s.dead_since.is_some() {
                    "dead: back in a moment".into()
                } else if let Some(nr) = self.neuro.get(i) {
                    nr.director.doing(&s.agent)
                } else {
                    s.agent
                        .x()
                        .current
                        .as_ref()
                        .map_or_else(|| "-".into(), |o| o.label.clone())
                },
                neuro: self.neuro.get(i).map(|nr| {
                    let (url, connected, who) = nr.link_state();
                    NeuroState {
                        url: url.to_string(),
                        connected,
                        who: who.to_string(),
                    }
                }),
            })
            .collect();
        let mut s = GameState {
            map: self.map_name.clone(),
            seconds: (self.engine.tic - self.began) as f64 / crate::domain::TICRATE,
            items: tally[0],
            kills: tally[1],
            secrets: tally[2],
            dead: players.iter().all(|p| p.dead),
            exited: self.engine.exited(),
            paused: self.paused,
            speed: self.speed,
            muted: self.muted || !self.sound,
            god: self.engine.god(),
            screen: if self.engine.exited() {
                "exited".into()
            } else {
                "level".into()
            },
            volume: self.engine.volume(),
            brightness: crate::viewer::brightness(),
            fov: self.engine.fov(),
            flashes: crate::viewer::flashes(),
            mode: format!(
                "{}, {} {}",
                self.kind(),
                n,
                if self.neuro.is_empty() {
                    "bots"
                } else {
                    "Neuros"
                }
            ),
            doing: players
                .first()
                .map_or_else(String::new, |p| p.doing.clone()),
            ..GameState::default()
        };
        // (the top bar's player: the first)
        if let Some(o) = &self.seats[0].obs {
            s.health = o.health.unwrap_or(0.0).round() as i32;
            s.armor = o.armor.unwrap_or(0.0).round() as i32;
            let ammo = |slot: i32| {
                o.ammo
                    .iter()
                    .flatten()
                    .find(|(k, _)| *k == slot)
                    .map_or(0, |(_, n)| *n)
            };
            s.ammo = [ammo(2), ammo(3), ammo(5), ammo(6)];
            s.keys = o
                .keys
                .unwrap_or_default()
                .iter()
                .map(|k| format!("{k:?}"))
                .collect();
        }
        // (the top bar's Neuro: all of them, connected only if every one is)
        if !self.neuro.is_empty() {
            let each: Vec<&NeuroState> = players.iter().filter_map(|p| p.neuro.as_ref()).collect();
            s.neuro = Some(NeuroState {
                url: each
                    .iter()
                    .map(|e| e.url.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                connected: each.iter().all(|e| e.connected),
                who: format!(
                    "{}/{} connected",
                    each.iter().filter(|e| e.connected).count(),
                    each.len()
                ),
            });
        }
        s.players = players;
        s
    }

    /// The window's pictures for a page showing them: the whole window (as "stream", half
    /// size) and player 1's view (as "game").
    pub(super) fn console_views(&mut self, px: &[u32]) {
        let Some(k) = self.console.as_mut() else {
            return;
        };
        if !k.c.views_due() {
            return;
        }
        k.c.viewed();
        if k.c.wants("stream") {
            k.c.stream_view(px, crate::viewer::W, crate::viewer::H);
        }
        if let Some((w, h, f, pal)) = &self.seats[0].frame {
            let palette = self.palettes.get(*pal).unwrap_or(&self.palettes[0]);
            k.c.game_view(px, Some(&(*w, *h, f.clone())), palette);
        }
    }
}

/// A command for Neuro's ear alone (what's said, the link, her settings): told as nothing
/// to do without one. (Who's at the console isn't: it's sent to every game.)
fn neuros_only(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::NeuroReconnect
            | Command::Say { .. }
            | Command::Lock { .. }
            | Command::Forcing { .. }
    )
}
