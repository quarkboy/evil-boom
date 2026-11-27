// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Watching Evil play a map in real time, with what she knows beside the game.
//!
//! Left: the game as the player sees it, and the explorer's log. Right: the map as she knows it
//! (mapped lines, known things, the route being followed, the objectives open to her), and the
//! status panel: what she's doing, the objectives she could pick, what she has found.
//!
//! Keys: space pause, +/- speed, A the whole map (faint: what she doesn't know), F follow her
//! on the map, M mute the game's sound, L the log panel: the whole log or only what passes
//! between the game and Evil, T the text size, mouse wheel zoom the map, Esc quit.

use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use crate::dsda::WindowEvent;

use crate::config::{out_dir, resolve_wad};
use crate::domain::{Controls, Observation, TICRATE};
use crate::engine::{Engine, Options};
use crate::geom::{dist, Point};
use crate::knowledge::completion::most_items;
use crate::knowledge::explorer::{Kind, Objective};
use crate::knowledge::model::Knowledge;
use crate::map::{MapData, ML_DONTDRAW};
use crate::navigator::Navigator;
use crate::play::Agent;
use crate::run::{broken_secrets, hold_exit, progress, RunOptions};
use crate::specials::Lock;
use crate::tactical::tuning::Tuning;
use crate::things::{pickup, PickupKind};

mod canvas;
mod console;
mod coop;
mod draw;
mod frames;
mod instinct;
mod keys;
mod present;
mod record;
mod stream;
mod window;

use crate::wad::Wad;
use canvas::*;
pub use record::{find_ffmpeg, record, RecordOptions, RecordView};
pub use stream::Side as StreamSide;

const W: usize = 1600;
const H: usize = 900;
/// (x, y, w, h)
type Rect = (usize, usize, usize, usize);
/// The ultra instinct's glow as last worked out: the picture's box and the canvas's width it
/// was for, and each pixel it reddens (its place on the canvas, how much).
type UltraMask = ((Rect, usize), Vec<(u32, u8)>);
const GAME: Rect = (0, 0, 800, 600);
const LOG: Rect = (0, 600, 800, 300);
const MAP: Rect = (800, 0, 800, 520);
const PANEL: Rect = (800, 520, 800, 380);

/// How to watch.
#[derive(Clone)]
pub struct WatchOptions {
    /// (`--tape FILE`) a recorded run's controls, played in place of hers
    pub tape: Option<std::path::PathBuf>,
    /// (`--ultra`) the ultra instinct on from the start, and kept on
    pub ultra: bool,
    /// (`--workers N`) its looks by N worker processes, started with the window (0: its own)
    pub workers: usize,
    /// (`--ultra-speed X`) the window's speed while its search has her, of what it is otherwise
    pub ultra_speed: f64,
    /// (`--ultra-after N`) hers on a level once she's died N times there
    pub ultra_after: Option<i32>,
    /// (`--ultra-back S`) how many seconds back a death is undone to
    pub ultra_back: f64,
    /// she plays as mortal and nothing hurts her (`--charmed`)
    pub charmed: bool,
    /// the chaingun and bullets for ever (`--endless-bullets`)
    pub endless_bullets: bool,
    /// her movement as keys (`--keys`)
    pub keys: bool,
    /// her key presses drawn over the game (`--show-keys`)
    pub show_keys: bool,
    pub speed: f64,
    pub show_all: bool,
    pub mortal: bool,
    /// the pass before a level known beforehand (`--known`), watched: the level as it is,
    /// nothing able to hurt her, everything first and then out. Not the run after it.
    pub practice: bool,
    pub lives: i32,
    pub fight: i32,
    pub peek: bool,
    pub expert: bool,
    /// every secret before the exit
    pub secrets: bool,
    /// every kill, item and secret before the exit (the exit last while any's short)
    pub complete: bool,
    /// a 100% run gives up on what's left (for the exit) after this long with nothing more
    /// collected or learned
    pub stall_seconds: f64,
    /// the most game time a level's given (headless)
    pub max_seconds: f64,
    /// what she starts with (an episode's level: what she brought from the one before)
    pub carry: Option<crate::dsda::Loadout>,
    /// on through the episode, level after level, what she gets out with carried (one life)
    pub episode: bool,
    /// the numbers she judges by, changed by this file
    pub tuning: Option<std::path::PathBuf>,
    /// no window: played through as fast as it goes, a line a level (checking a watch plays as
    /// the run did); its log written to out/watch/live.log as it plays
    pub headless: bool,
    /// --speed given: headless keeps to it in real time (a language model playing Evil, watched
    /// through the logs) rather than flat out
    pub paced: bool,
    pub seed: Option<u32>,
    /// how much of the log is kept (`--log`; unless told, `LogLevel::usual`)
    pub log: Option<crate::config::LogLevel>,
    /// the map view centred on a point, zoomed in this much (for looking at one place)
    pub focus: Option<(f64, f64, f64)>,
    /// the game's sound and music out loud (the window only; off while paused or not at x1)
    pub sound: bool,
    /// directed play: orders read from this file as they're added to it (control::script's
    /// commands, a line each), what comes back written beside it (FILE.out) and into the log
    pub directed: Option<std::path::PathBuf>,
    /// directed play by Neuro: the WebSocket URL of Neuro (or `evil-boom neuro-mock`)
    pub neuro: Option<String>,
    /// directed by the GPT stand-in, started in this program (its config; `neuro` set to it)
    pub gpt: Option<std::path::PathBuf>,
    /// the game's sound taken a tic at a time, not played (a recording at x1)
    pub capture_sound: bool,
    /// run under the stream console (`evil-boom console`): its address, connected back to
    pub control: Option<String>,
    /// Doom's own screens (the stream's): the title card, the menu, the melt, the tally, the
    /// finales, death as the game shows it (see viewer::present)
    pub present: bool,
    /// with them: held at the title card till the console begins the game
    pub title_wait: bool,
    /// the window for the stream: 16:9, the game on one side and a column for the viewers on
    /// the other, the column's side this (see viewer::stream)
    pub stream: Option<StreamSide>,
    /// the games hers to start (`control::game::Choices::parse`'s words); none: none
    pub choices: Option<String>,
    /// her lives are for all the levels played on through (`--episode-lives`), not each one's
    pub lives_shared: bool,
    /// settings not hers to change (`--lock`; of `neuro::session::LOCKABLE`): each stays as
    /// it is ("instinct": on) and is out of her actions
    pub locked: Vec<String>,
    /// (`--instinct off`) she waits for each action from the start, instinct off
    pub instinct_off: bool,
    /// the window's size, vsync, full screen
    pub window: crate::dsda::WindowSpec,
    /// the game drawn at this size (width, height); none: its own (640x480)
    pub render: Option<(usize, usize)>,
    /// the music played with this SoundFont (FluidSynth); none: by the OPL synth
    pub soundfont: Option<std::path::PathBuf>,
    /// the songs played as recorded (the Steam re-release's), the rest as MIDI
    pub music: Option<crate::music::Recordings>,
    /// (`--players N`) a co-op game of N players (2 to 4), each a bot of her own (see
    /// viewer::coop); 1: the single-player watch
    pub players: usize,
    /// (`--human`) with more than one player: the last is a person at the window's keyboard
    /// and mouse (viewer::coop::human), not a bot
    pub human: bool,
    /// (`--mouse DEG`) degrees a mouse pixel turns a person playing
    pub mouse_sens: f64,
    /// (`--hunt all|none|N,M`) in a deathmatch, the bots that hunt the others once geared up
    /// (0-based; None: all of them), the rest as before: for setting one against the other
    pub hunt: Option<Vec<usize>>,
    /// (`--hunt-style`, `--fight-style`) a deathmatch's styles (`tactical::styles`): one for all,
    /// or one a player in order (the last for the rest); empty: the tuning's own
    pub hunt_style: Vec<String>,
    pub fight_style: Vec<String>,
    /// (`--camera N`) a co-op or deathmatch game begun with the camera on player N (0-based):
    /// their view filling the window, the sound theirs; None: every view at once
    pub camera: Option<usize>,
    /// (`--human-window`) a person playing has a window of their own, the watcher's window the
    /// stream's (every view, or the camera)
    pub human_window: bool,
    /// (`--human-window-size`, `--human-fullscreen`) that window's size (None: 960x720, or the
    /// render's if bigger), or the whole screen it's on; the picture in it is `render`'s
    pub human_window_size: Option<(usize, usize)>,
    pub human_fullscreen: bool,
    /// (`--deathmatch`, `--altdeath`) with more than one player: 0 co-op, 1 deathmatch, 2
    /// altdeath
    pub deathmatch: i32,
    /// (`--frags N`) a deathmatch over at this many frags (0: no limit)
    pub frag_limit: i32,
    /// (`--minutes M`) a deathmatch over after this much game time (0: no limit)
    pub minutes: f64,
}

impl Watch {
    /// She's died: whoever directs her told, as it came (with the hits the game hasn't passed
    /// on yet, the last blow among them), and with `lives_left` after this one.
    pub(super) fn tell_death(&mut self, again: bool) {
        let hits = std::mem::take(&mut self.hits);
        let tic = self.engine.tic;
        // (passed on at once: no tic of the level may come to pass it on)
        self.tell_screens(|d| d.died(tic, again, &hits));
    }

    /// Off a level with a game hers to start (the title, a game over, the end of what she
    /// chose): what's on offer told her (again whenever it changes), and her choice taken,
    /// checked, for the window's loop to begin.
    pub(super) fn serve_choices(&mut self) {
        if self.start_wanted.is_some() {
            return; // (chosen: begun at the window's next turn)
        }
        if self.offered.as_ref() != Some(&self.choices) {
            self.offered = Some(self.choices.clone());
            if self.choices.offered() {
                let (tic, offer) = (self.engine.tic, self.choices.offer());
                self.tell_screens(|d| d.offer_game(tic, &offer));
            }
        }
        self.serve_game(true, false);
    }

    /// Off a level: what she asks of the game taken (a game to start, if `start`; the next
    /// level, if `next`), checked; the rest answered that it's not for now. A game chosen is
    /// begun by the window's loop (`start_wanted`); the next level, by the tally screen's
    /// (`next_wanted`).
    pub(super) fn serve_game(&mut self, start: bool, next: bool) {
        self.serve_off_level(start, next, None);
    }

    /// The window's own game over (a level or episode done, or lives out, without the game's
    /// screens): what she asks answered that it's over (Neuro waits on an answer to every
    /// action: unanswered, each hung till its timeout).
    pub(super) fn serve_over(&mut self) {
        self.serve_off_level(false, false, Some("The game's over: nothing more to play."));
    }

    /// A screen that isn't hers to act on (the tally counting, a death, the menu walked to a
    /// game): what she asks answered that it's not now, `why`.
    pub(super) fn serve_wait(&mut self, why: &str) {
        self.serve_off_level(false, false, Some(why));
    }

    /// `serve_game`'s work; `not_now`: why nothing's hers to do now (else, that there's no
    /// level, and a game's hers to start if it is).
    fn serve_off_level(&mut self, start: bool, next: bool, not_now: Option<&str>) {
        use crate::control::game::GameAct;
        let choices = self.choices.clone();
        let (mut wanted, mut go_on) = (None, false);
        let mut take = |act: GameAct| match act {
            GameAct::Start { map, skill, play } if start => {
                match choices.check(map.as_deref(), skill, play.as_deref()) {
                    Ok(s) => {
                        let msg = format!(
                            "Starting {} at skill {}, {}.",
                            s.map,
                            s.skill,
                            if s.continuous {
                                "on through its episode"
                            } else {
                                "just that level"
                            }
                        );
                        wanted = Some(s);
                        (true, msg)
                    }
                    Err(e) => (false, e),
                }
            }
            GameAct::Next if next => {
                go_on = true;
                (true, "On to the next level.".to_string())
            }
            GameAct::Start { .. } => (
                false,
                not_now
                    .unwrap_or("Not now: this game's still on.")
                    .to_string(),
            ),
            GameAct::Next => (false, "Not now: no level to go on to.".to_string()),
            GameAct::Restart => (false, "Not in a level now.".to_string()),
        };
        let mut lines = Vec::new();
        if let Some(feed) = self.feed.as_mut() {
            lines.extend(feed.off_level(&mut take));
        }
        if let Some(neuro) = self.neuro.as_mut() {
            let tic = self.engine.tic;
            neuro.set_choices(&choices);
            // (told to start a game only when one's hers to start)
            let idle = not_now.unwrap_or(if start && choices.start {
                "Not in a level now: start a game first (doom_game)."
            } else {
                "Not in a level now."
            });
            // (a game hers to start: nothing happens till she does)
            let offer = (start && not_now.is_none() && choices.offered()).then(|| choices.offer());
            lines.extend(neuro.off_level(
                tic,
                idle,
                offer.as_deref(),
                |args| match GameAct::of_json(args) {
                    Ok(act) => take(act),
                    Err(e) => (false, e),
                },
            ));
        }
        self.agent.x_mut().log.extend(lines);
        // (offered again once what she's chosen is over: a level begun is a Watch afresh, with
        // nothing offered)
        if wanted.is_some() {
            self.start_wanted = wanted;
        }
        self.next_wanted |= go_on;
    }

    /// The game's screens between levels told to whoever's directing (`f` on each director),
    /// and passed on at once: no level's tics run then to pass them on.
    pub(super) fn tell_screens(&mut self, f: impl Fn(&mut crate::control::directed::Director)) {
        let mut lines = Vec::new();
        if let Some(feed) = self.feed.as_mut() {
            f(&mut feed.director);
            lines.extend(feed.between());
        }
        if let Some(neuro) = self.neuro.as_mut() {
            f(&mut neuro.director);
            lines.extend(neuro.between(self.engine.tic));
        }
        self.agent.x_mut().log.extend(lines);
    }
}

/// A watched game's orders, from a file added to as it plays.
struct Feed {
    director: crate::control::directed::Director,
    input: std::path::PathBuf,
    /// how much of it's been read
    read: usize,
    out: std::path::PathBuf,
    /// reports shown so far
    shown: usize,
}

impl Feed {
    fn new(input: &Path) -> Feed {
        if let Some(dir) = input.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        if !input.exists() {
            let _ = std::fs::write(input, "");
        }
        let out = input.with_extension("out");
        let _ = std::fs::write(&out, "");
        // (from its start: the file is this game's orders, and a saved one plays back)
        Feed {
            director: crate::control::directed::Director::new(),
            input: input.to_path_buf(),
            read: 0,
            out,
            shown: 0,
        }
    }

    /// The tic's orders read and carried out, the reports since; the lines for the log.
    fn tic(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        let mut lines = Vec::new();
        if obs.tic % 5 == 0 {
            for cmd in self.new_commands() {
                lines.push(format!(">> {cmd}"));
                match crate::control::script::command(&mut self.director, agent, obs, &cmd) {
                    Ok(out) => lines.extend(out.into_iter().map(|l| format!("<< {l}"))),
                    Err(e) => lines.push(format!("<< refused: {e}")),
                }
            }
        }
        self.director.tick(agent, obs);
        self.write_news(&mut lines);
        lines
    }

    /// The commands added to the file since it was last read (whole lines only: one still
    /// being written waits).
    fn new_commands(&mut self) -> Vec<String> {
        let Ok(bytes) = std::fs::read(&self.input) else {
            return Vec::new();
        };
        if bytes.len() <= self.read {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&bytes[self.read..]).to_string();
        let upto = text.rfind('\n').map_or(0, |i| i + 1);
        self.read += upto;
        text[..upto]
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect()
    }

    /// Off a level: `game ...` (start, next, restart) by `act`; anything else refused, there
    /// being no level to do it in. Written out, with what's been told since.
    fn off_level(
        &mut self,
        mut act: impl FnMut(crate::control::game::GameAct) -> (bool, String),
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for cmd in self.new_commands() {
            lines.push(format!(">> {cmd}"));
            let w: Vec<&str> = cmd.split_whitespace().collect();
            let said = match w.split_first() {
                Some((&"game", rest)) => match crate::control::game::GameAct::of_words(rest) {
                    Ok(a) => match act(a) {
                        (true, msg) => msg,
                        (false, msg) => format!("refused: {msg}"),
                    },
                    Err(e) => format!("refused: {e}"),
                },
                _ => "refused: not in a level now (game start|next|restart)".to_string(),
            };
            lines.push(format!("<< {said}"));
        }
        self.write_news(&mut lines);
        lines
    }

    /// Between levels: what the director's been told of the screens, written out.
    fn between(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        self.write_news(&mut lines);
        lines
    }

    /// The reports since last time, added to `lines`, and `lines` written to the out file.
    fn write_news(&mut self, lines: &mut Vec<String>) {
        for r in &self.director.reports[self.shown..] {
            lines.push(format!(
                "<< [{:.1}s] {}{}{}",
                r.tic as f64 / 35.0,
                if r.notable() { "(!) " } else { "" },
                if r.ok { "" } else { "FAILED: " },
                r.text
            ));
        }
        self.shown = self.director.reports.len();
        if !lines.is_empty() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&self.out) {
                for l in lines.iter() {
                    let _ = writeln!(f, "{l}");
                }
            }
        }
    }
}

/// A 100% run's exit held back (see sweep::hold_exit), and its stall clock.
struct Hold {
    o: RunOptions,
    secrets_short: i32,
    items_short: i32,
    best: [usize; 7],
    best_tic: i64,
    given_up: bool,
}

struct Watch {
    /// the stream console's link (--control)
    console: Option<console::Console>,
    /// Doom's own screens (--present)
    present: Option<present::Present>,
    /// the stream's window (--stream)
    stream: Option<stream::Stream>,
    hold: Option<Hold>,
    /// directed play's orders, if it's directed
    feed: Option<Feed>,
    /// directed by Neuro
    neuro: Option<crate::neuro::session::Session>,
    /// the games hers to start (the console's to say); the choices last offered her; and the
    /// one she's chosen, for the window's loop to begin
    choices: crate::control::game::Choices,
    /// the WAD's levels (what may be on offer)
    wad_maps: Vec<String>,
    offered: Option<crate::control::game::Choices>,
    start_wanted: Option<crate::control::game::Start>,
    /// on to the next level, she's said, at the tally screen
    next_wanted: bool,
    /// no switches or doors (an arena's fight)
    no_use: bool,
    /// secrets found so far (as the tally has them)
    secrets_seen: i32,
    /// `--endless-bullets`
    endless_bullets: bool,
    /// (`--tape`) a run's controls by its tics, played in place of hers while there are any
    pub tape: std::collections::BTreeMap<i64, Controls>,
    /// the ultra instinct, while it's on (`U`, or `--ultra`): the fight searched in the game
    /// itself, at half speed while it searches (`crate::ultra`)
    ultra: Option<crate::ultra::Session>,
    /// (`--ultra`) kept on: not dropped when the fight's over
    ultra_stays: bool,
    /// its workers, while no session has them (`--workers N`)
    ultra_pool: Option<crate::ultra::Pool>,
    /// its workers asked for (they're started, or started again on another level, when it's
    /// next turned on)
    ultra_workers: usize,
    /// the tic it's waiting on its workers for: what she saw and what she'd have done, kept
    /// till they answer (she's asked once a tic)
    ultra_wait: Option<(Observation, Controls)>,
    /// ...and that the tic asked for just now wasn't played
    ultra_waiting: bool,
    /// its looks waited for (the headless watcher), not come back for (the window)
    ultra_block: bool,
    /// (`--ultra-speed`) the window's speed while its search has her, of what it is otherwise
    ultra_speed: f64,
    /// (`--ultra-after N`) hers by dying: on a level once she's died N times there (see
    /// `ultra::LIVE_CAP`)
    ultra_after: Option<i32>,
    /// (`--ultra-back S`) how far back a death is undone to, in tics
    ultra_back: i64,
    /// her deaths on this level that stood
    ultra_deaths: i32,
    /// what she has of it, in tics
    ultra_charge: f64,
    /// the session that's on is one a death began: it spends what she has
    ultra_earned: bool,
    /// games kept as she plays, for a death to be undone to
    ultra_rolling: crate::ultra::Rolling,
    /// the tics games have been put back over, sessions past: her own clock only goes on
    clock_ahead: i64,
    /// her clock over the game's, when she last looked (`tic_see`)
    ahead_seen: i64,
    /// its glow: the pixels it reddens and how much each, for the picture's box it was last
    /// drawn in (the box and the canvas's width)
    ultra_mask: std::cell::RefCell<UltraMask>,
    /// ...and how hot it glows (0: on and idle, 1: the search has her), eased; when last drawn
    ultra_heat: std::cell::Cell<(f64, f64)>,
    /// the hits on her last tic, for whoever's directing to be told of
    hits: Vec<crate::dsda::Hit>,
    m: Arc<MapData>,
    map_name: String,
    engine: Engine,
    agent: Agent,
    /// the game's colors as shown (`brightened`), and as the WAD has them
    palette: Vec<u32>,
    /// PLAYPAL's palettes as the WAD has them, and which of them `palette` is (the game's
    /// own choice a frame: the red of a hit, a pickup's gold, the radiation suit's green)
    palettes: Vec<Vec<u32>>,
    palette_now: usize,
    frame: Option<(usize, usize, Vec<u8>)>,
    /// the frame drawn as each tic's played (a window draws its own, between tics, instead)
    frame_per_tic: bool,
    obs: Option<Observation>,
    controls: Option<Controls>,
    /// her keys drawn over the game (K), and the last tics' presses and turns for it
    /// (`viewer/keys.rs`)
    show_keys: bool,
    pressed: std::collections::VecDeque<(u8, f64)>,
    options: Vec<Objective>,
    options_tic: i64,
    speed: f64,
    paused: bool,
    /// the game's sound up (a window's), muted by M, and whether it's on now
    sound: bool,
    muted: bool,
    sound_on: bool,
    show_all: bool,
    follow: bool,
    /// the log panel showing only what passes between the game and Evil (L)
    evil_log: bool,
    /// which of TEXT_SIZES the text's drawn at (T)
    text_size: usize,
    zoom: f64,
    center: Option<Point>,
    bounds: (f64, f64, f64, f64),
    lives_left: i32,
}

pub fn watch(wad: &str, map_name: &str, o: &WatchOptions) -> Result<(), String> {
    if o.players > 1 || o.deathmatch > 0 {
        return coop::watch(wad, map_name, o);
    }
    let sound = o.sound && !o.headless;
    let (w, mut watch) = first_watch(wad, map_name, o, sound)?;
    (watch.sound, watch.sound_on) = (sound, sound);
    if let Some(addr) = &o.control {
        watch.console = Some(console::Console::connect(addr, wad, map_name, &w)?);
    }
    watch.wad_maps = console::maps(&w);
    if let Some(c) = &o.choices {
        let skill = if o.fight > 0 { o.fight } else { 3 };
        watch.choices =
            crate::control::game::Choices::parse(c, &watch.wad_maps, (map_name, skill, o.episode))?;
    }
    if let Some(side) = o.stream {
        watch.stream = Some(stream::Stream::new(side, o.fight));
    }
    if o.present && !o.headless {
        watch.present = present::Present::start(&watch, o, o.title_wait);
        if watch.present.is_none() {
            watch.agent.x_mut().log.push("present: this engine hasn't Doom's own screens (rebuild it: tools/build-engine.sh)".into());
        }
    }
    if o.headless {
        return headless(watch, &w, o);
    }
    let title = format!("evil-boom explorer: {}", watch.map_name);
    show(watch, &title, Some((&w, o)))
}

/// The WAD and the watcher for the first level.
fn first_watch(
    wad: &str,
    map_name: &str,
    o: &WatchOptions,
    sound: bool,
) -> Result<(Wad, Watch), String> {
    let iwad = resolve_wad(wad);
    let w = Wad::open(&iwad)?;
    let map_name = map_name.to_uppercase();
    let m = Arc::new(crate::wad::load_level(&w, &map_name)?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    // (the songs as recorded, if asked for and the re-release is there: made once, in out/music)
    let music_wad = match o.music.filter(|_| sound || o.capture_sound) {
        Some(kind) => {
            let made = crate::music::music_wad(&iwad, kind)?;
            if made.is_none() {
                println!(
                    "music: no {} recordings for {} (the Steam re-release's rerelease/extras.wad \
                     not found, or not this IWAD's): MIDI",
                    kind.name(),
                    iwad.display()
                );
            }
            made
        }
        None => None,
    };
    let mut engine = Engine::new(
        &iwad,
        &map_name,
        &Options {
            skill,
            nomonsters: o.fight == 0,
            objects: o.fight > 0 || o.expert,
            drawn: true,
            sound,
            capture: o.capture_sound,
            seed: o.seed,
            resolution: o.render,
            soundfont: o.soundfont.clone(),
            music_wad,
            record: false,
        },
    )?;
    engine.carry.clone_from(&o.carry);
    engine.start();
    if !o.mortal || o.practice || o.charmed {
        engine.set_god(true);
    }
    if o.practice {
        // (the level begun again, as the pass begins it: the same run, by its random numbers)
        engine.restart(true);
    }
    let mut watch = level(&w, m, nav, engine, o)?;
    watch.feed = o.directed.as_deref().map(Feed::new);
    // (the stand-in's thread, if there's one waiting: the engine is up, the environment set)
    crate::neuro::gpt::go();
    let clocks = Tuning::from(o.tuning.as_deref())?.directed.clone();
    if let Some(f) = watch.feed.as_mut() {
        f.director.set_clocks(&clocks);
        if o.instinct_off {
            f.director.idle_auto = None;
        }
    }
    watch.neuro = o
        .neuro
        .as_deref()
        .map(|url| crate::neuro::session::Session::new(url, &clocks));
    if let Some(n) = watch.neuro.as_mut() {
        n.set_locked(&o.locked);
        if o.instinct_off {
            n.set_instinct(false);
        }
    }
    Ok((w, watch))
}

/// The watch played through without a window, as fast as it goes (the time cap as a run's):
/// a line as each level ends.
fn headless(mut watch: Watch, w: &Wad, o: &WatchOptions) -> Result<(), String> {
    use std::io::Write;
    let mut start = watch.engine.tic;
    // (the log as it's written, to follow live: tail -f)
    let live = out_dir().join("watch").join("live.log");
    std::fs::create_dir_all(live.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let level = o.log.unwrap_or_else(crate::config::LogLevel::usual);
    let mut log = crate::config::Log::create_at(
        &live,
        &o.seed.map_or_else(String::new, |s| format!("s{s}")),
        level,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "log: {}{}",
        live.display(),
        if o.paced {
            format!(" (at x{})", o.speed)
        } else {
            String::new()
        }
    );
    loop {
        let mut shown = 0;
        let (mut began, mut first) = (Instant::now(), watch.engine.tic);
        let _ = writeln!(log, "== {}", watch.map_name);
        while (!watch.engine.finished()
            || (watch.engine.dead()
                && (watch.lives_left > 0 || watch.ultra.is_some() || watch.ultra_unlocked())))
            && ((watch.engine.tic - start) as f64) < o.max_seconds * TICRATE
        {
            if watch.engine.dead() {
                match watch.died(o.directed.is_some() || o.neuro.is_some()) {
                    Died::Undone => continue,
                    Died::Over => break,
                    Died::Again(at) => {
                        let left = if o.lives == i32::MAX {
                            "no limit to the lives".to_string()
                        } else {
                            format!("{} lives left", watch.lives_left)
                        };
                        let line = format!(
                            "{}: died at {:.0}s, {left}",
                            watch.map_name,
                            (at - start) as f64 / TICRATE
                        );
                        println!("{line}");
                        let _ = writeln!(log, "{line}");
                    }
                }
            }
            // (paused: what she sends answered, the game held; kept to time afresh after)
            if watch.serve_paused() {
                std::thread::sleep(std::time::Duration::from_secs_f64(1.0 / TICRATE));
                (began, first) = (Instant::now(), watch.engine.tic);
                continue;
            }
            watch.tic();
            let lines = &watch.agent.x().log;
            let at = watch.engine.tic as f64 / TICRATE;
            for l in lines[shown.min(lines.len())..]
                .iter()
                .filter(|l| level.keeps(l))
            {
                let _ = writeln!(log, "[{at:7.1}s] {l}");
            }
            shown = lines.len();
            if o.paced && o.speed > 0.0 {
                let due = began
                    + std::time::Duration::from_secs_f64(
                        (watch.engine.tic - first) as f64 / (TICRATE * o.speed),
                    );
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        }
        // (dead for good: told, as any death is)
        if watch.engine.dead() {
            watch.tell_death(false);
        }
        let t = watch.engine.tally();
        let how = if watch.engine.exited() {
            "EXITED"
        } else if watch.engine.dead() {
            "DIED"
        } else {
            "TIME"
        };
        let line = format!(
            "{}: {how} kills {}/{}, items {}/{}, secrets {}/{}, {:.0}s game",
            watch.map_name,
            t[1].0,
            t[1].1,
            t[0].0,
            t[0].1,
            t[2].0,
            t[2].1,
            (watch.engine.tic - start) as f64 / TICRATE
        );
        println!("{line}");
        let _ = writeln!(log, "{line}");
        if !(o.episode && watch.engine.exited()) {
            return Ok(());
        }
        match next_level(watch, w, o)? {
            Some(next) => watch = next,
            None => return Ok(()),
        }
        start = watch.engine.tic;
    }
}

/// What came of a death (`Watch::died`).
pub(super) enum Died {
    /// the ultra instinct put the game back: she's alive
    Undone,
    /// a life spent, the level begun again; the tic she died at
    Again(i64),
    /// no life left
    Over,
}

/// The watcher for the level the engine's on: Evil in it afresh, as a run starts her.
fn level(
    w: &Wad,
    m: Arc<MapData>,
    nav: Rc<Navigator>,
    engine: Engine,
    o: &WatchOptions,
) -> Result<Watch, String> {
    let map_name = engine.map_name.clone();
    let skill = if o.fight > 0 { o.fight } else { 3 };
    // (seeing as a run does: watched, a run plays as it did)
    let mut knowledge = Knowledge::new(Rc::clone(&nav), o.mortal, !o.peek);
    knowledge.charmed = o.charmed;
    let mut agent = Agent::new(
        &nav,
        knowledge,
        o.fight,
        o.expert,
        o.lives,
        Tuning::from(o.tuning.as_deref())?,
    );
    // (on through the episode, a level after this one: what she'd take with her had first)
    if let Some(p) = agent.player_mut() {
        p.set_going_on(
            o.episode
                && (crate::run::next_map(&map_name, false).is_some()
                    || crate::run::next_map(&map_name, true).is_some()),
        );
    }
    // (a 100% run as a sweep plays it: the exit held back while what it's after is short, given
    // up on after a stall)
    let hold = (o.expert || o.secrets || o.complete).then(|| {
        let ro = RunOptions {
            expert: o.expert,
            secrets: o.secrets,
            complete: o.complete,
            fight: o.fight,
            stall_seconds: o.stall_seconds,
            ..RunOptions::default()
        };
        Hold {
            secrets_short: broken_secrets(&map_name),
            items_short: most_items(&m, skill),
            o: ro,
            best: [0; 7],
            best_tic: 0,
            given_up: false,
        }
    });
    if o.secrets {
        agent.x_mut().exit_held_back = true;
        agent.x_mut().secrets_mode = true;
    }
    if o.practice {
        // (as `sweep::scout` sets her up)
        agent.x_mut().knowledge.immune = true;
        agent.x_mut().exit_held_back = true;
        agent.x_mut().exit_before_risks = true;
        if let Some(p) = agent.player_mut() {
            p.combat_mut().immune = true;
        }
        agent.start_over();
    }
    if o.directed.is_some() || o.neuro.is_some() {
        crate::control::directed::Director::take_over(&mut agent);
    }
    let mut watch = Watch::new(w, m, &map_name, engine, agent, o.focus, o.lives - 1);
    watch.hold = hold;
    watch.endless_bullets = o.endless_bullets;
    if o.keys {
        watch.agent.keys_only();
    }
    watch.ultra_after = o.ultra_after;
    watch.ultra_charge = watch.ultra_cap();
    watch.ultra_back = (o.ultra_back * TICRATE) as i64;
    // (hers by dying, and no workers asked for: eight, or each look would stop the picture)
    watch.ultra_workers = if o.ultra_after.is_some() && o.workers == 0 {
        8
    } else {
        o.workers
    };
    watch.ultra_speed = o.ultra_speed;
    watch.ultra_pool_up();
    if o.ultra {
        watch.ultra_stays = true;
        watch.ultra_toggle();
    }
    if let Some(p) = &o.tape {
        watch.tape = crate::replay::Recording::read(p)?
            .entries
            .into_iter()
            .filter_map(|en| match en {
                crate::replay::Entry::Tic(t, c) => Some((t, c)),
                _ => None,
            })
            .collect();
    }
    watch.speed = o.speed;
    watch.show_all = o.show_all;
    watch.show_keys = o.show_keys;
    Ok(watch)
}

/// The episode's next level, once she's out of this one: what she got out with carried, the
/// map the exit she took leads to (None: the episode's over).
fn next_level(watch: Watch, w: &Wad, o: &WatchOptions) -> Result<Option<Watch>, String> {
    let secret = watch.engine.secret_exit();
    let Some(next) = crate::run::next_map(&watch.map_name, secret) else {
        return Ok(None);
    };
    let loaded = load_level(w, &next, o)?;
    let carry = watch.engine.loadout();
    enter_level(watch, w, o, &next, loaded, Some(carry)).map(Some)
}

/// A level's map and its analysis, made ready (what can fail, before anything's changed).
fn load_level(
    w: &Wad,
    name: &str,
    o: &WatchOptions,
) -> Result<(Arc<MapData>, Rc<Navigator>), String> {
    crate::engine::episode_map(name)?;
    let m = Arc::new(crate::wad::load_level(w, name)?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    Ok((m, Rc::new(nav)))
}

/// The engine on to the level `load_level` made ready, she in it afresh with `carry` (none: as
/// the run was started, a pistol start unless --carry); the window's settings and the console
/// kept.
fn enter_level(
    watch: Watch,
    w: &Wad,
    o: &WatchOptions,
    name: &str,
    (m, nav): (Arc<MapData>, Rc<Navigator>),
    carry: Option<crate::dsda::Loadout>,
) -> Result<Watch, String> {
    let (speed, show_all, follow) = (watch.speed, watch.show_all, watch.follow);
    let show_keys = watch.show_keys;
    let frame_per_tic = watch.frame_per_tic;
    let (sound, muted, sound_on) = (watch.sound, watch.muted, watch.sound_on);
    let (paused, mut console, present, stream) =
        (watch.paused, watch.console, watch.present, watch.stream);
    // (whoever directs her goes on to the next level with her: the orders file read on from
    // where it was, not from its start again; Neuro's link kept, not made afresh. What was
    // said of the old map (names, orders, what's been told) is the old map's)
    // (out by its exit: the level's end told first, before the next one's begun)
    let exited = watch.engine.exited();
    let (mut feed, mut neuro) = (watch.feed, watch.neuro);
    let (choices, wad_maps) = (watch.choices, watch.wad_maps);
    if let Some(f) = feed.as_mut() {
        if exited {
            f.director.level_done();
        }
        f.director.new_map();
    }
    if let Some(n) = neuro.as_mut() {
        if exited {
            n.director.level_done();
        }
        n.director.new_map();
    }
    let lives = watch.lives_left;
    let mut engine = watch.engine;
    engine.carry = carry.or_else(|| o.carry.clone());
    engine.goto(name)?;
    let mut watch = level(w, m, nav, engine, o)?;
    // (her lives for the whole of it: what's left of them comes with her)
    if o.lives_shared {
        watch.lives_left = lives;
    }
    (watch.speed, watch.show_all, watch.follow) = (speed, show_all, follow);
    watch.show_keys = show_keys;
    watch.frame_per_tic = frame_per_tic;
    (watch.sound, watch.muted, watch.sound_on) = (sound, muted, sound_on);
    watch.paused = paused;
    if let Some(c) = console.as_mut() {
        c.new_level();
    }
    watch.console = console;
    watch.present = present;
    watch.stream = stream;
    watch.feed = feed;
    watch.neuro = neuro;
    (watch.choices, watch.wad_maps) = (choices, wad_maps);
    Ok(watch)
}

/// A fight set up already (an arena: the engine with its monsters spawned and her loadout given,
/// drawn; her agent), watched: one life, no switches or doors (as `evil-boom arena` plays it).
pub fn watch_prepared(
    p: crate::arena::Prepared,
    map_name: &str,
    speed: f64,
    sound: bool,
    title: &str,
) -> Result<(), String> {
    let mut watch = Watch::new(&p.wad, p.map, map_name, p.engine, p.agent, None, 0);
    watch.speed = speed;
    (watch.sound, watch.sound_on) = (sound, sound);
    watch.no_use = true;
    show(watch, title, None)
}

/// A watch window's log, written as it plays (out/watch/live.log, as a headless watch's).
struct LiveLog {
    log: crate::config::Log,
    level: crate::config::LogLevel,
    /// the lines of the agent's log written so far, and the level they're of
    shown: usize,
    map: String,
}

impl LiveLog {
    fn create(
        seed: Option<u32>,
        level: Option<crate::config::LogLevel>,
    ) -> Result<LiveLog, String> {
        let live = out_dir().join("watch").join("live.log");
        let tag = seed.map_or_else(String::new, |s| format!("s{s}"));
        let level = level.unwrap_or_else(crate::config::LogLevel::usual);
        Ok(LiveLog {
            log: crate::config::Log::create_at(&live, &tag, level).map_err(|e| e.to_string())?,
            level,
            shown: 0,
            map: String::new(),
        })
    }

    /// What's been added to her log since, written.
    fn write(&mut self, watch: &Watch) {
        use std::io::Write;
        // (a level begun is a new agent, its log new)
        if watch.map_name != self.map {
            let _ = writeln!(self.log, "== {}", watch.map_name);
            self.map.clone_from(&watch.map_name);
            self.shown = 0;
        }
        let lines = &watch.agent.x().log;
        let at = watch.engine.tic as f64 / TICRATE;
        for l in lines[self.shown.min(lines.len())..]
            .iter()
            .filter(|l| self.level.keeps(l))
        {
            let _ = writeln!(self.log, "[{at:7.1}s] {l}");
        }
        self.shown = lines.len();
    }
}

/// The window, till it's shut or Esc: space pauses, N (or the right arrow) steps a tic while
/// paused, +/- the speed, A the whole map, F follow, the wheel zooms.
fn show(mut watch: Watch, title: &str, given: Option<(&Wad, &WatchOptions)>) -> Result<(), String> {
    // (the options as she may change them: a game she starts, its skill and whether it goes
    // on through the episode)
    let wad = given.map(|(w, _)| w);
    let mut opts = given.map(|(_, o)| o.clone());
    // (the stream's window its own size, and a plain title to find it by in OBS)
    let (ww, wh, title) = if watch.stream.is_some() {
        (stream::SW, stream::SH, "Evil BOOM")
    } else {
        (W, H, title)
    };
    let mut frames = frames::Frames::from_env();
    let spec = opts.as_ref().map(|o| o.window).unwrap_or_default();
    let mut window = window::Window::open(&watch.engine.game, title, (ww, wh), spec)?;
    if frames.is_some() {
        println!("window: {}", window.describe());
    }
    // (the stream's title kept, for OBS to find the window by)
    let fixed_title = watch.stream.is_some();
    let retitle = |watch: &Watch, title: &str| {
        if !fixed_title {
            window::Window::set_title(&watch.engine.game, title);
        }
    };
    // (the game drawn each frame, between its tics, rather than once a tic)
    watch.frame_per_tic = false;
    let mut canvas = Canvas {
        px: vec![0; ww * wh],
        width: ww,
        clip: (0, 0, ww, wh),
        text_scale: 1.0,
    };
    let mut aside: Option<Canvas> = None;
    let mut last = Instant::now();
    // (the window goes on showing the game while the ultra instinct's workers look)
    watch.ultra_block = false;
    if let Some(u) = watch.ultra.as_mut() {
        u.ultra.block = false;
    }
    let mut owed = 0.0;
    // (her log written as it plays, as a headless watch's is: a watched run read afterwards)
    let mut live = LiveLog::create(
        opts.as_ref().and_then(|o| o.seed),
        opts.as_ref().and_then(|o| o.log),
    )?;
    'frames: loop {
        for event in window::Window::events(&watch.engine.game) {
            use window::key;
            let k = match event {
                WindowEvent::Close => break 'frames,
                WindowEvent::Wheel(dy) => {
                    watch.zoom = (watch.zoom * if dy > 0 { 1.25 } else { 0.8 }).clamp(1.0, 20.0);
                    continue;
                }
                WindowEvent::Key(k) => k,
                // (no play window here)
                WindowEvent::PlayKey(_) | WindowEvent::PlayWheel(_) | WindowEvent::PlayClose => {
                    continue
                }
            };
            match k {
                key::ESCAPE => break 'frames,
                key::SPACE => watch.paused.toggle(),
                key::N | key::RIGHT if watch.paused && !watch.engine.finished() => watch.tic(),
                key::EQUALS | key::KP_PLUS => watch.speed = (watch.speed * 2.0).min(16.0),
                key::MINUS | key::KP_MINUS => watch.speed = (watch.speed / 2.0).max(0.25),
                key::A => watch.show_all.toggle(),
                key::K => watch.show_keys.toggle(),
                key::F => watch.follow.toggle(),
                key::U => watch.ultra_toggle(),
                key::L => watch.evil_log.toggle(),
                key::T => watch.text_size = (watch.text_size + 1) % TEXT_SIZES.len(),
                key::M => watch.muted.toggle(),
                _ => {}
            }
        }
        if let Some(s) = watch.start_wanted.take() {
            if let Some(o) = opts.as_mut() {
                if o.fight > 0 {
                    o.fight = s.skill;
                }
                o.episode = s.continuous;
            }
            watch.engine.set_skill(s.skill);
            if let Some(mut p) = watch.present.take() {
                p.choose(&s);
                p.begin(&watch);
                watch.present = Some(p);
            }
        }
        let game = wad.zip(opts.as_ref());
        match watch.console_frame() {
            console::Frame::Go => {}
            console::Frame::Quit => return Ok(()), // (the console's asked the game to close)
            console::Frame::Goto { map, carry } => {
                watch = console::goto(watch, game, &map, carry)?;
                retitle(&watch, &format!("evil-boom explorer: {}", watch.map_name));
            }
        }
        watch.serve_paused();
        // (the sound only at the game's own speed: faster or slower it'd pile up or lag)
        let sound =
            watch.sound && !watch.muted && !watch.paused && (watch.speed - 1.0).abs() < 1e-9;
        if sound != watch.sound_on {
            watch.engine.sound(sound);
            watch.sound_on = sound;
        }
        let (now, tic0) = (Instant::now(), watch.engine.tic);
        // (the ultra instinct searching: slowed, which is what its looks have to fit in; eased in
        // and out with its glow, not from one tic to the next)
        let ultra_pace = if watch.ultra.is_some() {
            1.0 - watch.ultra_heat.get().0 * (1.0 - watch.ultra_speed)
        } else {
            1.0
        };
        owed += (now - last).as_secs_f64() * TICRATE * watch.speed * ultra_pace;
        last = now;
        owed = f64::min(owed, 8.0); // (don't race to catch up after a slow plan)
        if watch.present.is_some() {
            // (Doom's own screens: the tics as they have them, the levels, deaths and the
            // episode's going on with them; see present)
            let was = watch.map_name.clone();
            watch = present::run(watch, game, &mut owed)?;
            if watch.map_name != was {
                retitle(&watch, &format!("evil-boom explorer: {}", watch.map_name));
            }
        } else {
            let wanted = watch.agent.x().restart_wanted;
            if wanted && watch.lives_left <= 0 {
                watch.agent.x_mut().restart_wanted = false; // (out with what she has)
            }
            // (dead: told whoever directs her, once, as it came)
            // (dead with the ultra instinct on: its to go back from, if it has a game kept)
            if watch.engine.dead() && !watch.paused {
                watch.ultra_died();
            }
            if watch.engine.dead() {
                let again = watch.lives_left > 0;
                watch.tell_death(again);
            }
            if (watch.engine.dead() || wanted) && watch.lives_left > 0 && !watch.paused {
                if watch.engine.dead() {
                    watch.ultra_real_death();
                }
                watch.agent.x_mut().restart_wanted = false;
                watch.lives_left -= 1;
                watch.engine.restart(true);
                watch.agent.restart();
                watch.controls = None;
            }
            while owed >= 1.0 && !watch.paused && !watch.engine.finished() {
                watch.tic();
                if watch.ultra_waiting {
                    break; // (its workers are looking: the tic's still to play, next frame)
                }
                owed -= 1.0;
            }
            if let Some((w, o)) = game.filter(|(_, o)| o.episode && watch.engine.exited()) {
                // (out: on to the next level, what she got out with carried)
                let name = watch.map_name.clone();
                let carried = watch.engine.loadout();
                match next_level(watch, w, o)? {
                    Some(next) => {
                        watch = next;
                        println!(
                            "{name} done: on to {} with health {} armor {}, ammo {}/{}/{}/{}",
                            watch.map_name,
                            carried.health,
                            carried.armor,
                            carried.clip,
                            carried.shell,
                            carried.cell,
                            carried.rocket
                        );
                        retitle(
                            &watch,
                            &format!(
                                "evil-boom explorer: {} (from {name}: health {} armor {})",
                                watch.map_name, carried.health, carried.armor
                            ),
                        );
                    }
                    None => return Ok(()), // (the episode's over)
                }
            }
            // (over, with no game screens to serve her: what she asks still answered)
            if watch.engine.finished() && watch.present.is_none() {
                watch.serve_over();
            }
        }
        live.write(&watch);
        if watch.paused {
            owed = 0.0;
        }
        // (the game as it is between the tic just played and the next, so that what moves moves
        // at the screen's rate: drawn as far on from the tic before as the time owed says)
        let between = if watch.paused {
            1.0
        } else {
            owed.clamp(0.0, 1.0)
        };
        if watch.engine.tic != tic0 || !watch.paused || watch.frame.is_none() {
            watch.frame = watch.engine.frame_at(between);
            watch.sync_palette();
        }
        let stepped = Instant::now();
        paint(&mut watch, &mut canvas, &mut aside);
        let painted = Instant::now();
        window.show(&watch.engine.game, &canvas.px, ww, wh);
        crate::debug::slow_frame(watch.engine.tic, now, stepped, painted);
        if let Some(f) = frames.as_mut() {
            let picture = watch.frame.as_ref().map_or(&[][..], |f| &f.2[..]);
            f.frame(now, stepped, painted, watch.engine.tic - tic0, picture);
            if f.done() {
                // (and the picture as it was last, to see what it looked like)
                let png = out_dir().join("watch").join("frames.png");
                if let Err(e) = save_png_sized(&png, &canvas.px, ww, wh) {
                    println!("{}: {e}", png.display());
                }
                break;
            }
        }
    }
    Ok(())
}

/// The window's picture this frame, the stream's layout or the watcher's; and the console's
/// pictures, which are the watcher's own drawing (in the stream's layout, drawn aside for them,
/// only when they're due).
fn paint(watch: &mut Watch, canvas: &mut Canvas, aside: &mut Option<Canvas>) {
    if watch.stream.is_some() {
        watch.draw_stream(canvas);
        if watch
            .console
            .as_ref()
            .is_some_and(console::Console::views_due)
        {
            // (only what a page is showing: drawn on the window's own time)
            let (map, game) = watch
                .console
                .as_ref()
                .map_or((false, false), |c| (c.wants("map"), c.wants("game")));
            if let Some(c) = watch.console.as_mut().filter(|c| c.wants("stream")) {
                c.stream_view(&canvas.px, canvas.width, canvas.px.len() / canvas.width);
            }
            if !(map || game) {
                if let Some(c) = watch.console.as_mut() {
                    c.viewed();
                }
                return;
            }
            let a = aside.get_or_insert_with(|| Canvas {
                px: vec![0; W * H],
                width: W,
                clip: (0, 0, W, H),
                text_scale: 1.0,
            });
            watch.draw(a);
            if let Some(c) = watch.console.as_mut() {
                c.views(&a.px, watch.frame.as_ref(), &watch.palette);
            }
        }
    } else {
        watch.draw(canvas);
        if let Some(c) = watch.console.as_mut() {
            c.views(&canvas.px, watch.frame.as_ref(), &watch.palette);
        }
    }
}

/// The window as it is now (the game's frame, the map, the panels) into out/watch.png, for a run
/// played as a sweep plays it (run-map --snapshot): drawn once, at the end.
pub fn save_snapshot(
    w: &Wad,
    m: Arc<MapData>,
    map_name: &str,
    engine: Engine,
    agent: Agent,
    focus: Option<(f64, f64, f64)>,
) -> Result<(), String> {
    snapshot_to(
        &out_dir().join("watch.png"),
        w,
        m,
        map_name,
        engine,
        agent,
        focus,
    )
    .2
}

/// The window drawn once into `path`, the game and Evil handed back after (evil-boom drive).
pub fn snapshot_to(
    path: &Path,
    w: &Wad,
    m: Arc<MapData>,
    map_name: &str,
    engine: Engine,
    agent: Agent,
    focus: Option<(f64, f64, f64)>,
) -> (Engine, Agent, Result<(), String>) {
    let mut watch = Watch::new(w, m, map_name, engine, agent, focus, 0);
    let obs = watch.engine.observe();
    watch.frame = watch.engine.frame();
    watch.sync_palette();
    watch.options = watch.agent.objectives();
    watch.options_tic = obs.tic;
    watch.obs = Some(obs);
    let mut canvas = Canvas {
        px: vec![0; W * H],
        width: W,
        clip: (0, 0, W, H),
        text_scale: 1.0,
    };
    watch.draw(&mut canvas);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let result = save_png(path, &canvas.px);
    (watch.engine, watch.agent, result)
}

/// How bright the game's frame is shown: 0, as the game has it, to 10 (the console's; the
/// program's, so a new level's watch has it too).
static BRIGHTNESS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// How strong the game's flashes are shown, a hundredth each: the reds (hurt, the berserk),
/// the gold (a pickup), the green (the radiation suit). 100 is Doom's own; half unless set
/// (the stream's viewers found the old game hard on the eyes).
static FLASHES: [std::sync::atomic::AtomicI32; 3] = [
    std::sync::atomic::AtomicI32::new(50),
    std::sync::atomic::AtomicI32::new(50),
    std::sync::atomic::AtomicI32::new(50),
];

fn flashes() -> [i32; 3] {
    [0, 1, 2].map(|i| FLASHES[i].load(std::sync::atomic::Ordering::Relaxed))
}

fn set_flashes(to: [Option<i32>; 3]) {
    for (i, v) in to.into_iter().enumerate() {
        if let Some(v) = v {
            FLASHES[i].store(v.clamp(0, 100), std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn brightness() -> i32 {
    BRIGHTNESS.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_brightness(level: i32) {
    BRIGHTNESS.store(level.clamp(0, 10), std::sync::atomic::Ordering::Relaxed);
}

/// A palette as bright as's set: a gamma curve as Doom's own gamma levels are, black still
/// black and white white, the dark lifted most (10: a channel's square root).
fn brightened(base: &[u32]) -> Vec<u32> {
    let level = brightness();
    if level == 0 {
        return base.to_vec();
    }
    let power = 1.0 / (1.0 + 0.1 * f64::from(level));
    let lut: Vec<u32> = (0..256)
        .map(|v| ((f64::from(v) / 255.0).powf(power) * 255.0).round() as u32)
        .collect();
    base.iter()
        .map(|&c| {
            (lut[(c >> 16 & 0xFF) as usize] << 16)
                | (lut[(c >> 8 & 0xFF) as usize] << 8)
                | lut[(c & 0xFF) as usize]
        })
        .collect()
}

/// PLAYPAL's palettes (14 in Doom's: the plain one, the reds, the golds, the green), each as
/// 0RGB; a grey one alone without the lump.
fn palettes(w: &Wad) -> Vec<Vec<u32>> {
    let Some(lump) = w.lumps.iter().find(|l| l.name == "PLAYPAL") else {
        return vec![(0..256).map(|i| i * 0x0001_0101).collect()];
    };
    let all: Vec<u32> = w
        .read(lump)
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]))
        .collect();
    all.as_chunks::<256>()
        .0
        .iter()
        .map(|p| p.to_vec())
        .collect()
}

impl Watch {
    /// The palette the game shows its last frame in, made ours to colour it with (as bright
    /// as's set): after each frame taken.
    fn sync_palette(&mut self) {
        let now = self
            .engine
            .palette()
            .min(self.palettes.len().saturating_sub(1));
        if now != self.palette_now {
            self.palette_now = now;
            self.set_palette();
        }
    }

    /// `palette` made again from the game's one in use (the brightness changed, or which, or
    /// how strong its flashes are shown): the plain one tinted towards it by that much.
    fn set_palette(&mut self) {
        let (Some(plain), Some(p)) = (self.palettes.first(), self.palettes.get(self.palette_now))
        else {
            return;
        };
        let share = match self.palette_now {
            0 => 0,
            1..=8 => flashes()[0],
            9..=12 => flashes()[1],
            _ => flashes()[2],
        };
        let mix = |a: u32, b: u32| (a * (100 - share as u32) + b * share as u32 + 50) / 100;
        let tinted: Vec<u32> = plain
            .iter()
            .zip(p)
            .map(|(&a, &b)| {
                (mix(a >> 16 & 0xFF, b >> 16 & 0xFF) << 16)
                    | (mix(a >> 8 & 0xFF, b >> 8 & 0xFF) << 8)
                    | mix(a & 0xFF, b & 0xFF)
            })
            .collect();
        self.palette = brightened(&tinted);
    }
}

fn save_png(path: &Path, px: &[u32]) -> Result<(), String> {
    save_png_sized(path, px, W, H)
}

/// A picture `w` by `h` (0RGB) into a PNG.
fn save_png_sized(path: &Path, px: &[u32], w: usize, h: usize) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = px
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    writer.write_image_data(&data).map_err(|e| e.to_string())
}

impl Watch {
    fn new(
        w: &Wad,
        m: Arc<MapData>,
        map_name: &str,
        engine: Engine,
        agent: Agent,
        focus: Option<(f64, f64, f64)>,
        lives_left: i32,
    ) -> Watch {
        let xs = m.vertexes.iter().map(|v| v.x);
        let ys = m.vertexes.iter().map(|v| v.y);
        let bounds = (
            xs.clone().fold(f64::INFINITY, f64::min),
            ys.clone().fold(f64::INFINITY, f64::min),
            xs.fold(f64::NEG_INFINITY, f64::max),
            ys.fold(f64::NEG_INFINITY, f64::max),
        );
        Watch {
            console: None,
            present: None,
            stream: None,
            hold: None,
            feed: None,
            neuro: None,
            choices: crate::control::game::Choices::none(),
            wad_maps: Vec::new(),
            offered: None,
            start_wanted: None,
            next_wanted: false,
            no_use: false,
            secrets_seen: 0,
            endless_bullets: false,
            tape: std::collections::BTreeMap::new(),
            ultra: None,
            ultra_stays: false,
            ultra_pool: None,
            ultra_workers: 0,
            ultra_wait: None,
            ultra_waiting: false,
            ultra_block: true,
            ultra_speed: crate::ultra::LIVE_SPEED,
            ultra_mask: std::cell::RefCell::new((((0, 0, 0, 0), 0), Vec::new())),
            ultra_heat: std::cell::Cell::new((0.0, 0.0)),
            ultra_after: None,
            ultra_back: crate::ultra::LIVE_BACK,
            ultra_deaths: 0,
            ultra_charge: 0.0,
            ultra_earned: false,
            ultra_rolling: crate::ultra::Rolling::default(),
            clock_ahead: 0,
            ahead_seen: 0,
            hits: Vec::new(),
            m,
            map_name: map_name.to_string(),
            engine,
            agent,
            palette: brightened(&palettes(w)[0]),
            palettes: palettes(w),
            palette_now: 0,
            frame: None,
            frame_per_tic: true,
            obs: None,
            controls: None,
            show_keys: false,
            pressed: std::collections::VecDeque::new(),
            options: Vec::new(),
            options_tic: -1_000_000_000,
            speed: 1.0,
            paused: false,
            sound: false,
            muted: false,
            sound_on: false,
            show_all: false,
            follow: false,
            evil_log: false,
            text_size: 0,
            zoom: focus.map_or(1.0, |f| f.2),
            center: focus.map(|f| (f.0, f.1)),
            bounds,
            lives_left,
        }
    }

    /// The game paused in a level (the watcher's space bar, the stream console): what she
    /// sends is answered meanwhile, so nothing of hers hangs on a game that isn't running
    /// (Neuro waits on an answer to every action, 20 s at most). Whether it's held.
    fn serve_paused(&mut self) -> bool {
        let Some(n) = self.neuro.as_mut() else {
            return false;
        };
        let held = self.paused
            && self.obs.is_some()
            && !self.engine.finished()
            && !self.engine.dead()
            && !self.engine.exited();
        n.set_paused(held, self.engine.tic);
        if let (true, Some(obs)) = (held, self.obs.as_ref()) {
            let lines = n.tic_paused(&mut self.agent, obs);
            self.agent.x_mut().log.extend(lines);
        }
        held
    }

    /// She's dead, in a loop that plays on by itself (no window, or a recording): the ultra
    /// instinct's to go back from if it has a game kept; else, with a life left, that life
    /// spent and the level again from its start, what she came in with and what she's learned
    /// of it kept, whoever directs her told (`directed`: and given her back); else it's over.
    pub(super) fn died(&mut self, directed: bool) -> Died {
        self.ultra_died();
        if !self.engine.dead() {
            return Died::Undone;
        }
        if self.lives_left <= 0 {
            return Died::Over;
        }
        let at = self.engine.tic;
        self.ultra_real_death();
        self.lives_left -= 1;
        self.tell_death(true);
        self.engine.restart(true);
        self.agent.restart();
        self.controls = None;
        if directed {
            crate::control::directed::Director::take_over(&mut self.agent);
        }
        Died::Again(at)
    }

    /// A tic's first half: the level as she sees it, whoever directs her told, and what she'd
    /// do.
    fn tic_see(&mut self) -> (Observation, Controls) {
        // (the level again, as she asked: what she came in with, what she knows kept, no life)
        let asked = self
            .feed
            .as_ref()
            .is_some_and(|f| f.director.restart_wanted)
            || self
                .neuro
                .as_ref()
                .is_some_and(|n| n.director.restart_wanted);
        if asked {
            self.engine.restart(true);
            self.agent.start_over();
            self.controls = None;
            let tic = self.engine.tic;
            if let Some(f) = self.feed.as_mut() {
                f.director.restarted(tic);
            }
            if let Some(n) = self.neuro.as_mut() {
                n.director.restarted(tic);
            }
            self.agent
                .x_mut()
                .log
                .push("the level again, as she asked".into());
        }
        let mut obs = self.engine.observe();
        obs.tic += self.clock_ahead;
        obs.hit_tic = obs.hit_tic.map(|t| t + self.clock_ahead);
        if let Some(u) = self.ultra.as_mut() {
            u.see(&mut obs);
        }
        // (her clock over the game's: more than it was is a game put back since she last
        // looked, and what she did in the tics gone back over didn't happen)
        let ahead = obs.tic - self.engine.tic;
        if ahead > self.ahead_seen {
            let since = obs.tic - (ahead - self.ahead_seen);
            self.agent.x_mut().gone_back(since, obs.tic);
        }
        self.ahead_seen = ahead;
        if self.endless_bullets && obs.tic % 35 == 0 {
            crate::run::top_up_bullets(&self.engine.game, &obs);
        }
        if let (Some(st), Some(m)) = (self.stream.as_mut(), obs.message.as_deref()) {
            st.message(m, obs.tic);
        }
        // (a secret found told her, as a run tells her: watched, it plays as it did)
        self.secrets_seen =
            crate::run::secret_found(&self.engine, &mut self.agent, &obs, self.secrets_seen);
        if self.frame_per_tic {
            self.frame = self.engine.frame();
            self.sync_palette();
        }
        let tally = crate::control::directed::Tally::of(self.engine.tally());
        let gibbed = self.engine.gibbed();
        // (the ultra instinct on: and what she has left of it, where it's hers by dying)
        let ultra = self.ultra.is_some().then(|| {
            self.ultra_earned
                .then(|| (self.ultra_charge / TICRATE).floor())
        });
        let hits = std::mem::take(&mut self.hits);
        let level = crate::control::directed::Level {
            map: self.map_name.clone(),
            title: self.engine.level_title(),
            skill: self.engine.skill(),
        };
        if let Some(feed) = &mut self.feed {
            feed.director.tally = tally;
            feed.director.level = level.clone();
            feed.director.restart_allowed = self.choices.restart;
            feed.director.ultra = ultra;
            feed.director.gibbed.clone_from(&gibbed);
            feed.director.hits.clone_from(&hits);
            for l in feed.tic(&mut self.agent, &obs) {
                self.agent.x_mut().log.push(l);
            }
        }
        if let Some(neuro) = &mut self.neuro {
            neuro.set_choices(&self.choices);
            neuro.director.tally = tally;
            neuro.director.level = level;
            neuro.director.restart_allowed = self.choices.restart;
            neuro.director.ultra = ultra;
            neuro.director.gibbed = gibbed;
            neuro.director.hits = hits;
            let said = crate::debug::timed("neuro.tic", || neuro.tic(&mut self.agent, &obs));
            for l in said {
                self.agent.x_mut().log.push(l);
            }
            // (and what the GPT stand-in said, if it's in this program)
            for s in crate::neuro::gpt::take_said() {
                self.agent
                    .x_mut()
                    .log
                    .push(format!("evil says: {}", s.replace('\n', " ")));
            }
        }
        let mut c =
            crate::debug::timed("agent.act", || self.agent.act(&obs, self.controls.as_ref()));
        crate::debug::slow_report(obs.tic);
        if self.no_use {
            c.use_ = false;
        }
        // (a tape: its controls for this tic, whatever she'd have done. She's asked all the
        // same, so what she knows and what the column shows go on)
        if let Some(t) = self.tape.get(&self.engine.tic) {
            c = *t;
        }
        (obs, c)
    }

    fn tic(&mut self) {
        // (waiting on the ultra instinct's workers for this tic: seen and asked already)
        self.ultra_waiting = false;
        let (obs, mut c) = match self.ultra_wait.take() {
            Some(seen) => seen,
            None => self.tic_see(),
        };
        // (the ultra instinct: its controls for hers, or the game put back and nothing played)
        if let Some(u) = self.ultra.as_mut() {
            match u.decide(&mut self.engine, &self.agent, c) {
                crate::ultra::Went::On(theirs) => c = theirs,
                crate::ultra::Went::Back(back) => {
                    self.ultra_log(format!(
                        "back {:.1}s: no way she lives on",
                        back as f64 / TICRATE
                    ));
                    self.controls = None;
                    self.frame = None;
                    self.obs = Some(obs);
                    return;
                }
                crate::ultra::Went::NoWay => {
                    self.ultra_log("no way she lives on, and no game kept to go back to".into());
                    self.ultra_end("it has no way");
                }
                crate::ultra::Went::Wait => {
                    self.ultra_wait = Some((obs, c));
                    self.ultra_waiting = true;
                    return;
                }
            }
        }
        self.engine.step(&c);
        let mut put_back = false;
        if let Some(u) = self.ultra.as_mut() {
            if let Some(back) = u.stepped(&mut self.engine) {
                put_back = true;
                self.frame = None;
                self.ultra_log(format!("back {:.1}s: hurt too fast", back as f64 / TICRATE));
            } else if !self.ultra_stays && self.engine.tic - u.searched_at > crate::ultra::LIVE_OVER
            {
                self.ultra_end("the fight's over");
            }
        }
        // (hers by dying: what she has of it spent while it's on, got back as she plays; and
        // the games a death is undone to, kept as she plays)
        if self.ultra_unlocked() {
            if self.ultra.is_some() && self.ultra_earned {
                self.ultra_charge -= 1.0;
                if self.ultra_charge <= 0.0 {
                    self.ultra_charge = 0.0;
                    self.ultra_end("spent");
                }
            } else if self.ultra.is_none() {
                self.ultra_charge =
                    (self.ultra_charge + crate::ultra::LIVE_REFILL).min(self.ultra_cap());
                // (kept whatever she has of it just now: by the time she's got enough back,
                // there are five seconds of them to go back over)
                if !self.engine.finished() {
                    self.ultra_rolling.keep(&mut self.engine);
                }
            }
        }
        let hits = self.engine.take_hits();
        self.hits.clone_from(&hits);
        for h in hits {
            let armor = if h.armor > 0 {
                format!(" (and {} armor)", h.armor)
            } else {
                String::new()
            };
            self.agent.x_mut().log.push(format!(
                "tic {}: hit for {}{armor} by {}",
                self.engine.tic,
                h.health,
                h.from()
            ));
        }
        if self.engine.exited() {
            self.ultra_end("out of the level");
        }
        if !put_back {
            self.keys_note(&c);
        }
        self.controls = (!put_back).then_some(c);
        if let Some(h) = self.hold.as_mut().filter(|_| obs.tic % TICRATE as i64 == 0) {
            let now = progress(&self.engine, self.agent.x(), &h.o);
            if now.iter().zip(&h.best).any(|(n, b)| n > b) {
                h.best = std::array::from_fn(|i| now[i].max(h.best[i]));
                h.best_tic = obs.tic;
            } else if !h.given_up && (obs.tic - h.best_tic) as f64 > h.o.stall_seconds * TICRATE {
                h.given_up = true;
                self.agent.x_mut().log.push(format!(
                    "tic {}: nothing more collected, learned or tried in {:.0}s: for the exit",
                    obs.tic, h.o.stall_seconds
                ));
            }
            hold_exit(
                &self.engine,
                &mut self.agent,
                &h.o,
                h.secrets_short,
                h.items_short,
                h.given_up,
            );
        }
        // (her options, for the map that marks them: not listed for the stream's window, which
        // has none, unless a console page is showing that map. A listing's tens of milliseconds
        // on a big level well known)
        let marked = self.stream.is_none() || self.console.as_ref().is_some_and(|c| c.wants("map"));
        if marked && obs.tic - self.options_tic >= TICRATE as i64 {
            self.options = self.agent.objectives();
            self.options_tic = obs.tic;
        }
        self.obs = Some(obs);
    }
}

/// A line of the log between the game and Evil: her action (>>), what she's told (<<), what she
/// said.
fn is_evil_line(l: &str) -> bool {
    l.starts_with(">> ") || l.starts_with("<< ") || l.starts_with("evil says: ")
}
