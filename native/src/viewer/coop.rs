// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A co-op game watched (`watch --players N`, 2 to 4): one game with up to four players in it,
//! each a bot of her own, every one's view on the screen at once.
//!
//! Each player is an `Agent` as a single-player watch has one, with what she's seen and learned
//! her own: nothing's shared, and none of them knows the others are there yet (their bodies
//! aren't among the things she's given: not a monster, not something in her way). Doom's own
//! co-op rules play: each at their own start, the things only a netgame has, keys and weapons
//! left lying for the others, a death a rebirth at the start with a pistol (she presses on a
//! second later, and goes on knowing what she knew), and one of them out by the exit ends the
//! level for all.
//!
//! The window: two players, their views side by side and each one's log under it; three or four,
//! their views two by two and a column beside them with the whole map (where each of them is,
//! and the lines any of them has seen) and a card for each.
//!
//! Keys: space pause, N a tic while paused, +/- speed, M mute, Esc quit.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use super::canvas::*;
use super::{brightened, palettes, window, Rect, WatchOptions, H, W};
use crate::config::resolve_wad;
use crate::domain::{Controls, Observation, TICRATE};
use crate::dsda::WindowEvent;
use crate::engine::{Engine, Options};
use crate::knowledge::model::Knowledge;
use crate::map::{MapData, ML_DONTDRAW};
use crate::navigator::Navigator;
use crate::play::Agent;
use crate::tactical::tuning::Tuning;
use crate::wad::Wad;

/// The most players a game has (Doom's).
pub const MAX_PLAYERS: usize = 4;

/// A dead player presses on to be born again this long after dying (tics).
const REBIRTH_WAIT: i64 = 35;

/// Each player's colour, as Doom's co-op has them (green, indigo, brown, red).
const COLOURS: [Rgb; MAX_PLAYERS] = [
    (80, 220, 80),
    (150, 150, 255),
    (210, 160, 90),
    (240, 80, 80),
];

/// One of the players.
struct Seat {
    agent: Agent,
    /// the last tic's controls (for the agent's next) and observation
    controls: Option<Controls>,
    obs: Option<Observation>,
    /// secrets found so far, by the game's tally of hers
    secrets_seen: i32,
    /// dead since this tic (None: alive)
    dead_since: Option<i64>,
    deaths: i32,
    /// her lines set off so far that the others have been told of (`tell_used`)
    told: usize,
    /// her view as last drawn: (width, height, palette indices, the palette)
    frame: Option<(usize, usize, Vec<u8>, usize)>,
}

struct Coop {
    w: Wad,
    m: Arc<MapData>,
    nav: Rc<Navigator>,
    map_name: String,
    engine: Engine,
    seats: Vec<Seat>,
    /// PLAYPAL's palettes, made ours (brightened as the watch's are)
    palettes: Vec<Vec<u32>>,
    /// the tic the level began at (its time is from here)
    began: i64,
    /// the game's seed (the logs are named by it)
    seed: Option<u32>,
    speed: f64,
    paused: bool,
    sound: bool,
    muted: bool,
    sound_on: bool,
}

/// A co-op game of `o.players` watched in a window (or played through headless).
pub fn watch(wad: &str, map_name: &str, o: &WatchOptions) -> Result<(), String> {
    let unsupported = [
        (
            o.directed.is_some() || o.neuro.is_some() || o.gpt.is_some(),
            "directed play",
        ),
        (o.control.is_some(), "the stream console (--control)"),
        (o.present, "--present"),
        (o.stream.is_some(), "--stream"),
        (
            o.ultra || o.ultra_after.is_some() || o.tape.is_some(),
            "the ultra instinct or --tape",
        ),
        (o.practice, "--known"),
        (o.expert || o.secrets || o.complete, "the 100% runs"),
    ];
    if let Some((_, what)) = unsupported.iter().find(|(is, _)| *is) {
        return Err(format!("--players: not yet with {what}"));
    }
    let sound = o.sound && !o.headless;
    let mut coop = Coop::new(wad, map_name, o, sound)?;
    if o.headless {
        return coop.headless(o);
    }
    coop.show(o)
}

impl Coop {
    fn new(wad: &str, map_name: &str, o: &WatchOptions, sound: bool) -> Result<Coop, String> {
        let iwad = resolve_wad(wad);
        let w = Wad::open(&iwad)?;
        let map_name = map_name.to_uppercase();
        let (m, nav) = load(&w, &map_name, o)?;
        let skill = if o.fight > 0 { o.fight } else { 3 };
        let mut engine = Engine::new(
            &iwad,
            &map_name,
            &Options {
                skill,
                nomonsters: o.fight == 0,
                objects: true,
                drawn: !o.headless,
                sound,
                capture: false,
                seed: o.seed,
                resolution: o.render,
                soundfont: o.soundfont.clone(),
                music_wad: None,
                record: false,
            },
        )?;
        engine.set_players(o.players);
        engine.carry.clone_from(&o.carry);
        engine.start();
        if !o.mortal || o.charmed {
            engine.set_god(true);
        }
        let seats = seats(&nav, &map_name, o)?;
        let palettes = palettes(&w).iter().map(|p| brightened(p)).collect();
        let began = engine.tic;
        Ok(Coop {
            w,
            m,
            nav,
            map_name,
            engine,
            seats,
            palettes,
            began,
            seed: o.seed,
            speed: o.speed,
            paused: false,
            sound,
            muted: false,
            sound_on: sound,
        })
    }

    /// A tic: each player sees the level and decides (a dead one presses on to be born again),
    /// then the tic's played with all their controls.
    fn tic(&mut self) {
        let mut cs = Vec::with_capacity(self.seats.len());
        for (i, s) in self.seats.iter_mut().enumerate() {
            self.engine.as_player(i);
            let tic = self.engine.tic;
            if self.engine.dead() {
                let since = *s.dead_since.get_or_insert_with(|| {
                    s.deaths += 1;
                    s.agent
                        .x_mut()
                        .log
                        .push(format!("tic {tic}: died (death {})", s.deaths));
                    tic
                });
                // (the use key, pressed and let go: Doom wants it pressed again for each go)
                let press = tic - since >= REBIRTH_WAIT && (tic - since) % 2 == 0;
                cs.push(Controls {
                    use_: press,
                    ..Controls::default()
                });
                s.controls = None;
                continue;
            }
            if s.dead_since.take().is_some() {
                // (born again at her start: the level goes on as it is, she knowing what she
                // knew of it)
                s.agent.reborn();
                s.agent
                    .x_mut()
                    .log
                    .push(format!("tic {tic}: born again at the start"));
            }
            let obs = self.engine.observe();
            s.secrets_seen =
                crate::run::secret_found(&self.engine, &mut s.agent, &obs, s.secrets_seen);
            let c = s.agent.act(&obs, s.controls.as_ref());
            s.obs = Some(obs);
            cs.push(c);
        }
        self.engine.step_all(&cs);
        for (i, s) in self.seats.iter_mut().enumerate() {
            self.engine.as_player(i);
            for h in self.engine.take_hits() {
                let armor = if h.armor > 0 {
                    format!(" (and {} armor)", h.armor)
                } else {
                    String::new()
                };
                s.agent.x_mut().log.push(format!(
                    "tic {}: hit for {}{armor} by {}",
                    self.engine.tic,
                    h.health,
                    h.from()
                ));
            }
            if !self.engine.dead() {
                s.controls = Some(cs[i]);
            }
        }
        self.engine.as_player(0);
        self.tell_used();
    }

    /// What each has set off of the level's once-only lines, told the others (as players say
    /// "I hit the switch"): used up for them too.
    fn tell_used(&mut self) {
        let mut said: Vec<(usize, usize)> = Vec::new();
        for (i, s) in self.seats.iter_mut().enumerate() {
            let k = &s.agent.x().knowledge;
            for f in &k.fired[s.told.min(k.fired.len())..] {
                if k.once_only(f.line) {
                    said.push((i, f.line));
                }
            }
            s.told = k.fired.len();
        }
        for (from, line) in said {
            for (i, s) in self.seats.iter_mut().enumerate() {
                if i != from && s.agent.x_mut().knowledge.mates_used.insert(line) {
                    s.agent.x_mut().log.push(format!(
                        "tic {}: player {} set off line {line}: used",
                        self.engine.tic,
                        from + 1
                    ));
                }
            }
        }
    }

    /// The level's out by its exit: on to the next one (the episode played on), each player
    /// with what she got out with. False: no next level, or not playing on.
    fn next_level(&mut self, o: &WatchOptions) -> Result<bool, String> {
        if !o.episode {
            return Ok(false);
        }
        let Some(next) = crate::run::next_map(&self.map_name, self.engine.secret_exit()) else {
            return Ok(false);
        };
        let (m, nav) = load(&self.w, &next, o)?;
        let carried: Vec<_> = (0..self.seats.len())
            .map(|i| {
                self.engine.as_player(i);
                self.engine.loadout()
            })
            .collect();
        self.engine.as_player(0);
        self.engine.carry = None;
        self.engine.goto(&next)?;
        for (i, l) in carried.iter().enumerate() {
            self.engine.as_player(i);
            self.engine.game.give(l);
        }
        self.engine.as_player(0);
        let deaths: Vec<i32> = self.seats.iter().map(|s| s.deaths).collect();
        self.seats = seats(&nav, &next, o)?;
        for (s, d) in self.seats.iter_mut().zip(deaths) {
            s.deaths = d;
        }
        println!("{} done: on to {next}", self.map_name);
        (self.m, self.nav, self.map_name) = (m, nav, next);
        self.began = self.engine.tic;
        Ok(true)
    }

    /// Played through without a window, as fast as it goes: a line when the level's over (or
    /// the time's up), each player's part in it.
    fn headless(&mut self, o: &WatchOptions) -> Result<(), String> {
        let start = Instant::now();
        loop {
            let cap = (o.max_seconds * TICRATE) as i64;
            while !self.engine.exited() && self.engine.tic - self.began < cap {
                self.tic();
            }
            self.report(start);
            if !self.engine.exited() || !self.next_level(o)? {
                return Ok(());
            }
        }
    }

    fn report(&mut self, start: Instant) {
        let secs = (self.engine.tic - self.began) as f64 / TICRATE;
        let outcome = if self.engine.exited() {
            "EXITED"
        } else {
            "TIMEOUT"
        };
        println!(
            "COOP {} {outcome} {:.0}s game, {:.0}s real, {} players",
            self.map_name,
            secs,
            start.elapsed().as_secs_f64(),
            self.seats.len()
        );
        for i in 0..self.seats.len() {
            self.engine.as_player(i);
            let t = self.engine.tally();
            let p = self.engine.game.player();
            let s = &self.seats[i];
            println!(
                "  player {}: kills {}/{} items {}/{} secrets {}/{} health {} armor {} deaths {}",
                i + 1,
                t[1].0,
                t[1].1,
                t[0].0,
                t[0].1,
                t[2].0,
                t[2].1,
                p.health,
                p.armor,
                s.deaths
            );
        }
        self.engine.as_player(0);
        // (each one's log, to read afterwards: out/watch/coop/MAP[-sSEED]-pN.log)
        let dir = crate::config::out_dir().join("watch").join("coop");
        let _ = std::fs::create_dir_all(&dir);
        for (i, s) in self.seats.iter().enumerate() {
            let seed = self.seed.map_or_else(String::new, |s| format!("-s{s}"));
            let path = dir.join(format!("{}{seed}-p{}.log", self.map_name, i + 1));
            if let Err(e) = std::fs::write(&path, s.agent.x().log.join("\n") + "\n") {
                println!("{}: {e}", path.display());
            }
        }
    }

    fn show(&mut self, o: &WatchOptions) -> Result<(), String> {
        let title = format!("evil-boom co-op: {}", self.map_name);
        let mut window = window::Window::open(&self.engine.game, &title, (W, H), o.window)?;
        let mut canvas = Canvas {
            px: vec![0; W * H],
            width: W,
            clip: (0, 0, W, H),
            text_scale: 1.0,
        };
        let mut frames = super::frames::Frames::from_env();
        let (mut last, mut owed) = (Instant::now(), 0.0);
        let real = Instant::now();
        let mut reported = false;
        'frames: loop {
            for event in window::Window::events(&self.engine.game) {
                use window::key;
                let WindowEvent::Key(k) = event else {
                    if matches!(event, WindowEvent::Close) {
                        break 'frames;
                    }
                    continue;
                };
                match k {
                    key::ESCAPE => break 'frames,
                    key::SPACE => self.paused = !self.paused,
                    key::N | key::RIGHT if self.paused && !self.engine.exited() => self.tic(),
                    key::EQUALS | key::KP_PLUS => self.speed = (self.speed * 2.0).min(16.0),
                    key::MINUS | key::KP_MINUS => self.speed = (self.speed / 2.0).max(0.25),
                    key::M => self.muted = !self.muted,
                    _ => {}
                }
            }
            let sound =
                self.sound && !self.muted && !self.paused && (self.speed - 1.0).abs() < 1e-9;
            if sound != self.sound_on {
                self.engine.sound(sound);
                self.sound_on = sound;
            }
            let (now, tic0) = (Instant::now(), self.engine.tic);
            owed += (now - last).as_secs_f64() * TICRATE * self.speed;
            last = now;
            owed = f64::min(owed, 8.0); // (don't race to catch up after a slow plan)
            let cap = (o.max_seconds * TICRATE) as i64;
            while owed >= 1.0 && !self.paused && !self.engine.exited() {
                if self.engine.tic - self.began >= cap {
                    break;
                }
                self.tic();
                owed -= 1.0;
            }
            if self.engine.exited() {
                if !reported {
                    self.report(real);
                    reported = true;
                }
                if self.next_level(o)? {
                    reported = false;
                    window::Window::set_title(
                        &self.engine.game,
                        &format!("evil-boom co-op: {}", self.map_name),
                    );
                }
            }
            if self.paused {
                owed = 0.0;
            }
            let between = if self.paused || self.engine.exited() {
                1.0
            } else {
                owed.clamp(0.0, 1.0)
            };
            if self.engine.tic != tic0 || !self.paused || self.seats[0].frame.is_none() {
                for i in 0..self.seats.len() {
                    self.seats[i].frame = self.engine.frame_of(i, between);
                }
            }
            let stepped = Instant::now();
            self.draw(&mut canvas);
            let painted = Instant::now();
            window.show(&self.engine.game, &canvas.px, W, H);
            if let Some(f) = frames.as_mut() {
                let picture = self.seats[0].frame.as_ref().map_or(&[][..], |f| &f.2[..]);
                f.frame(now, stepped, painted, self.engine.tic - tic0, picture);
                if f.done() {
                    let png = crate::config::out_dir().join("watch").join("frames.png");
                    if let Err(e) = super::save_png_sized(&png, &canvas.px, W, H) {
                        println!("{}: {e}", png.display());
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    // --- The picture ---------------------------------------------------------------------------

    /// The window: the views, and the logs (two players) or the map and the cards (three, four).
    fn draw(&self, c: &mut Canvas) {
        c.clip = (0, 0, W, H);
        c.fill((0, 0, W, H), BG);
        let n = self.seats.len();
        if n <= 2 {
            for i in 0..n {
                let x = i * (W / 2);
                self.draw_view(c, i, (x, 0, W / 2, 600));
                self.draw_card(c, i, (x, 600, W / 2, H - 600), true);
            }
        } else {
            let (vw, vh) = (600, 450);
            for i in 0..n {
                self.draw_view(c, i, ((i % 2) * vw, (i / 2) * vh, vw, vh));
            }
            let col = (2 * vw, 0, W - 2 * vw, H);
            let map = (col.0, 0, col.2, 300);
            self.draw_overview(c, map);
            let card_h = (H - map.3) / n;
            for i in 0..n {
                self.draw_card(c, i, (col.0, map.3 + i * card_h, col.2, card_h), false);
            }
        }
    }

    /// Player i's view, scaled into r, her name over it.
    fn draw_view(&self, c: &mut Canvas, i: usize, r: Rect) {
        c.clip = r;
        let s = &self.seats[i];
        match &s.frame {
            Some((fw, fh, px, pal)) => {
                let palette = self.palettes.get(*pal).unwrap_or(&self.palettes[0]);
                for y in 0..r.3 {
                    let sy = y * fh / r.3;
                    let row = (r.1 + y) * W + r.0;
                    for x in 0..r.2 {
                        let sx = x * fw / r.2;
                        if let Some(&p) = px.get(sy * fw + sx) {
                            c.px[row + x] = palette[usize::from(p)];
                        }
                    }
                }
            }
            None => {
                c.text("starting...", (r.0 + 20, r.1 + 20), TEXT, 2);
            }
        }
        // (her name, in her colour, and a frame round the view)
        let name = format!("Player {}", i + 1);
        c.fill((r.0, r.1, 8 * name.len() * 2 + 12, 22), (0, 0, 0));
        c.text(&name, (r.0 + 6, r.1 + 3), COLOURS[i], 2);
        if s.dead_since.is_some() {
            c.text(
                "DEAD",
                (r.0 + r.2 / 2 - 32, r.1 + r.3 / 3),
                (255, 60, 60),
                4,
            );
        } else if self.engine.exited() {
            c.text("EXITED", (r.0 + r.2 / 2 - 48, r.1 + r.3 / 3), HEAD, 4);
        }
        for (x0, y0, w, h) in [
            (r.0, r.1, r.2, 2),
            (r.0, r.1 + r.3 - 2, r.2, 2),
            (r.0, r.1, 2, r.3),
            (r.0 + r.2 - 2, r.1, 2, r.3),
        ] {
            c.fill((x0, y0, w, h), COLOURS[i]);
        }
        c.clip = (0, 0, W, H);
    }

    /// What player i is doing, and (`log`) the end of her log, into r.
    fn draw_card(&self, c: &mut Canvas, i: usize, r: Rect, log: bool) {
        c.clip = r;
        c.fill(r, (20, 20, 26));
        c.fill((r.0, r.1, 4, r.3), COLOURS[i]);
        let s = &self.seats[i];
        let x = r.0 + 12;
        let mut y = c.text(&format!("Player {}", i + 1), (x, r.1 + 6), COLOURS[i], 2);
        if let Some(obs) = &s.obs {
            let gun = obs.weapon.map_or(String::new(), |w| format!(", gun {w}"));
            let line = format!(
                "health {:.0}  armor {:.0}{gun}  deaths {}",
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0),
                s.deaths
            );
            y = c.text(&line, (x, y + 2), TEXT, 1);
        }
        let doing = if s.dead_since.is_some() {
            "dead: back at the start in a moment".to_string()
        } else {
            s.agent
                .x()
                .current
                .as_ref()
                .map_or_else(|| "looking round".to_string(), |o| o.label.clone())
        };
        let (width, _) = c.fits(1, r.2 - 24, r.3);
        y = c.text(
            &clip(&format!("doing: {doing}"), width),
            (x, y + 2),
            HEAD,
            1,
        );
        // (the end of her log, as much as fits: a few lines on a card, more under a view)
        let room = (r.1 + r.3).saturating_sub(y + 4) / c.glyph(1).max(1);
        let rows = if log { room } else { room.min(4) };
        let lines: Vec<&String> = s.agent.x().log.iter().rev().take(rows).collect();
        for l in lines.into_iter().rev() {
            y = c.text(&clip(l, width), (x, y), DIM, 1);
        }
        c.clip = (0, 0, W, H);
    }

    /// The whole map in r: the lines any of them has seen bright, the rest faint, and each
    /// player where she is, facing the way she faces.
    fn draw_overview(&self, c: &mut Canvas, r: Rect) {
        c.clip = r;
        c.fill(r, (8, 8, 12));
        let m = &self.m;
        let xs = m.vertexes.iter().map(|v| v.x);
        let ys = m.vertexes.iter().map(|v| v.y);
        let (x0, x1) = (
            xs.clone().fold(f64::INFINITY, f64::min),
            xs.fold(f64::NEG_INFINITY, f64::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f64::INFINITY, f64::min),
            ys.fold(f64::NEG_INFINITY, f64::max),
        );
        let pad = 10.0;
        let scale = ((r.2 as f64 - 2.0 * pad) / (x1 - x0).max(1.0))
            .min((r.3 as f64 - 2.0 * pad) / (y1 - y0).max(1.0));
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let at = |x: f64, y: f64| {
            (
                r.0 as f64 + r.2 as f64 / 2.0 + (x - cx) * scale,
                r.1 as f64 + r.3 as f64 / 2.0 - (y - cy) * scale,
            )
        };
        for (li, l) in m.linedefs.iter().enumerate() {
            if l.flags & ML_DONTDRAW != 0 {
                continue;
            }
            let (a, b) = (&m.vertexes[l.v1], &m.vertexes[l.v2]);
            let seen = self
                .seats
                .iter()
                .any(|s| s.agent.x().knowledge.mapped.contains(&li));
            let colour = if seen { (170, 170, 180) } else { (45, 45, 55) };
            c.line(at(a.x, a.y), at(b.x, b.y), colour, 1);
        }
        for (i, s) in self.seats.iter().enumerate() {
            let Some(obs) = &s.obs else { continue };
            let p = at(obs.x, obs.y);
            let a = obs.angle.to_radians();
            c.circle(p, 4.0, COLOURS[i], None);
            c.line(p, (p.0 + 9.0 * a.cos(), p.1 - 9.0 * a.sin()), COLOURS[i], 2);
        }
        c.text(&self.map_name, (r.0 + 6, r.1 + 4), HEAD, 1);
        c.clip = (0, 0, W, H);
    }
}

/// A level's map and its analysis.
fn load(w: &Wad, name: &str, o: &WatchOptions) -> Result<(Arc<MapData>, Rc<Navigator>), String> {
    crate::engine::episode_map(name)?;
    let m = Arc::new(crate::wad::load_level(w, name)?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    Ok((m, Rc::new(nav)))
}

/// The players for a level, each in it afresh as a watch starts her.
fn seats(nav: &Rc<Navigator>, map_name: &str, o: &WatchOptions) -> Result<Vec<Seat>, String> {
    let tune = Tuning::from(o.tuning.as_deref())?;
    let going_on = o.episode
        && (crate::run::next_map(map_name, false).is_some()
            || crate::run::next_map(map_name, true).is_some());
    Ok((0..o.players)
        .map(|_| {
            let mut knowledge = Knowledge::new(Rc::clone(nav), o.mortal, !o.peek);
            knowledge.charmed = o.charmed;
            let mut agent = Agent::new(nav, knowledge, o.fight, false, o.lives, Rc::clone(&tune));
            if let Some(p) = agent.player_mut() {
                p.set_going_on(going_on);
                p.set_coop();
            }
            if o.keys {
                agent.keys_only();
            }
            Seat {
                agent,
                controls: None,
                obs: None,
                secrets_seen: 0,
                dead_since: None,
                deaths: 0,
                told: 0,
                frame: None,
            }
        })
        .collect())
}

/// s cut to `width` characters.
fn clip(s: &str, width: usize) -> String {
    s.chars().take(width).collect()
}
