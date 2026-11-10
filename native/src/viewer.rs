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

use font8x8::legacy::BASIC_LEGACY;
use minifb::{Key as K, KeyRepeat, Scale, ScaleMode, Window, WindowOptions};

use crate::config::{out_dir, resolve_wad};
use crate::engine::{Engine, Options};
use crate::geom::{dist, Point};
use crate::knowledge::completion::most_items;
use crate::knowledge::explorer::{Kind, Objective};
use crate::sweep::{broken_secrets, hold_exit, progress, RunOptions};
use crate::knowledge::model::Knowledge;
use crate::play::Agent;
use crate::tactical::tuning::Tuning;
use crate::map::{MapData, ML_DONTDRAW};
use crate::motor::{Controls, Observation, TICRATE};
use crate::navigator::Navigator;
use crate::specials::Lock;
use crate::things::{pickup, PickupKind};

mod console;
use crate::wad::{load_map, Wad};

const W: usize = 1600;
const H: usize = 900;
/// (x, y, w, h)
type Rect = (usize, usize, usize, usize);
const GAME: Rect = (0, 0, 800, 600);
const LOG: Rect = (0, 600, 800, 300);
const MAP: Rect = (800, 0, 800, 520);
const PANEL: Rect = (800, 520, 800, 380);

type Rgb = (u8, u8, u8);
const BG: Rgb = (12, 12, 16);
const DIM: Rgb = (120, 120, 130);
const TEXT: Rgb = (220, 220, 220);
const HEAD: Rgb = (255, 210, 120);
/// what Evil said (the GPT stand-in's words)
const SAID: Rgb = (200, 160, 255);

fn kind_colour(k: Kind) -> Rgb {
    match k {
        Kind::Exit => (255, 80, 255),
        Kind::Key => (255, 255, 0),
        Kind::Switch => (0, 220, 255),
        Kind::Door => (0, 160, 255),
        Kind::Frontier => (120, 255, 120),
        Kind::Visit => (80, 180, 80),
        Kind::Recheck => (200, 160, 60),
        Kind::Noise => (255, 120, 40),
        Kind::Teleporter => (255, 255, 255),
        Kind::Room => (160, 255, 200),
        _ => TEXT,
    }
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Exit => "exit",
        Kind::Key => "key",
        Kind::Switch => "switch",
        Kind::Door => "door",
        Kind::Frontier => "frontier",
        Kind::Room => "room",
        Kind::Visit => "visit",
        Kind::Noise => "noise",
        Kind::Teleporter => "teleporter",
        Kind::Recheck => "recheck",
        Kind::Item => "item",
        Kind::Hunt => "hunt",
        Kind::Wall => "wall",
        Kind::Secret => "secret",
        Kind::Barrel => "barrel",
    }
}

fn item_colour(k: PickupKind) -> Rgb {
    match k {
        PickupKind::Key => (255, 255, 0),
        PickupKind::Weapon => (255, 140, 0),
        PickupKind::Ammo => (180, 140, 90),
        PickupKind::Health => (80, 220, 80),
        PickupKind::Armor => (80, 160, 255),
        PickupKind::Powerup => (220, 120, 255),
    }
}

fn key_colour(name: &str) -> Rgb {
    let n = name.to_lowercase();
    if n.contains("blue") {
        (60, 120, 255)
    } else if n.contains("yellow") {
        (255, 230, 40)
    } else if n.contains("red") {
        (255, 50, 50)
    } else {
        (255, 255, 0)
    }
}

/// A canvas of W x H pixels (0RGB) with a clip rectangle.
struct Canvas {
    px: Vec<u32>,
    clip: Rect,
    /// all text this much bigger (T cycles TEXT_SIZES; the user's: it was small)
    text_scale: f64,
}

/// the text sizes T cycles through
const TEXT_SIZES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

impl Canvas {
    fn put(&mut self, x: i64, y: i64, c: Rgb) {
        let (cx, cy, cw, ch) = self.clip;
        if x < cx as i64 || y < cy as i64 || x >= (cx + cw) as i64 || y >= (cy + ch) as i64 {
            return;
        }
        self.px[y as usize * W + x as usize] = (u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2);
    }

    fn fill(&mut self, r: Rect, c: Rgb) {
        for y in r.1..r.1 + r.3 {
            for x in r.0..r.0 + r.2 {
                self.put(x as i64, y as i64, c);
            }
        }
    }

    fn line(&mut self, a: Point, b: Point, c: Rgb, width: i64) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let n = dx.abs().max(dy.abs()).ceil().max(1.0) as i64;
        if n > 20000 {
            return;
        }
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let (x, y) = ((a.0 + dx * t).round() as i64, (a.1 + dy * t).round() as i64);
            for ox in -(width / 2)..=((width - 1) / 2) {
                for oy in -(width / 2)..=((width - 1) / 2) {
                    self.put(x + ox, y + oy, c);
                }
            }
        }
    }

    fn circle(&mut self, at: Point, r: f64, c: Rgb, width: Option<f64>) {
        let ri = r.ceil() as i64 + 1;
        let (x0, y0) = (at.0.round() as i64, at.1.round() as i64);
        for y in -ri..=ri {
            for x in -ri..=ri {
                let d = ((x * x + y * y) as f64).sqrt();
                let on = match width {
                    None => d <= r,
                    Some(w) => d <= r && d > r - w,
                };
                if on {
                    self.put(x0 + x, y0 + y, c);
                }
            }
        }
    }

    fn triangle(&mut self, p: [Point; 3], c: Rgb) {
        let (x0, x1) = (p.iter().map(|q| q.0).fold(f64::INFINITY, f64::min), p.iter().map(|q| q.0).fold(f64::NEG_INFINITY, f64::max));
        let (y0, y1) = (p.iter().map(|q| q.1).fold(f64::INFINITY, f64::min), p.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max));
        let side = |a: Point, b: Point, q: Point| (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
        for y in y0.floor() as i64..=y1.ceil() as i64 {
            for x in x0.floor() as i64..=x1.ceil() as i64 {
                let q = (x as f64, y as f64);
                let s = [side(p[0], p[1], q), side(p[1], p[2], q), side(p[2], p[0], q)];
                if s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0) {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Text in the 8x8 font at `scale`; returns the next line's y.
    fn text(&mut self, s: &str, at: (usize, usize), c: Rgb, scale: usize) -> usize {
        // (the 8x8 font at any size, each pixel the glyph's nearest)
        let size = self.glyph(scale);
        let eff = size as f64 / 8.0;
        let mut x = at.0;
        for ch in s.chars() {
            let glyph = BASIC_LEGACY.get(ch as usize).copied().unwrap_or([0; 8]);
            for oy in 0..size {
                let bits = glyph[((oy as f64 / eff) as usize).min(7)];
                for ox in 0..size {
                    if bits & (1 << ((ox as f64 / eff) as usize).min(7)) != 0 {
                        self.put((x + ox) as i64, (at.1 + oy) as i64, c);
                    }
                }
            }
            x += size;
        }
        at.1 + size + (3.0 * self.text_scale).round() as usize
    }

    /// A glyph's side in pixels at `scale` (and the text size).
    fn glyph(&self, scale: usize) -> usize {
        (8.0 * scale as f64 * self.text_scale).round() as usize
    }

    /// How many characters, and lines, of `scale` text fit `width` by `height`.
    fn fits(&self, scale: usize, width: usize, height: usize) -> (usize, usize) {
        let g = self.glyph(scale);
        (width / g.max(1), height / (g + (3.0 * self.text_scale).round() as usize).max(1))
    }
}


/// How to watch.
pub struct WatchOptions {
    pub speed: f64,
    pub show_all: bool,
    pub mortal: bool,
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
    /// through the logs: the user's) rather than flat out
    pub paced: bool,
    pub seed: Option<u32>,
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
        Feed { director: crate::control::directed::Director::new(), input: input.to_path_buf(), read: 0, out, shown: 0 }
    }

    /// The tic's orders read and carried out, the reports since; the lines for the log.
    fn tic(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        let mut lines = Vec::new();
        if obs.tic % 5 == 0 {
            if let Ok(bytes) = std::fs::read(&self.input) {
                if bytes.len() > self.read {
                    let text = String::from_utf8_lossy(&bytes[self.read..]).to_string();
                    // (whole lines only: one still being written waits)
                    let upto = text.rfind('\n').map_or(0, |i| i + 1);
                    self.read += upto;
                    for cmd in text[..upto].lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
                        lines.push(format!(">> {cmd}"));
                        match crate::control::script::command(&mut self.director, agent, obs, cmd) {
                            Ok(out) => lines.extend(out.into_iter().map(|l| format!("<< {l}"))),
                            Err(e) => lines.push(format!("<< refused: {e}")),
                        }
                    }
                }
            }
        }
        self.director.tick(agent, obs);
        for r in &self.director.reports[self.shown..] {
            lines.push(format!("<< [{:.1}s] {}{}{}", r.tic as f64 / 35.0, if r.notable { "(!) " } else { "" }, if r.ok { "" } else { "FAILED: " }, r.text));
        }
        self.shown = self.director.reports.len();
        if !lines.is_empty() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&self.out) {
                for l in &lines {
                    let _ = writeln!(f, "{l}");
                }
            }
        }
        lines
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
    hold: Option<Hold>,
    /// directed play's orders, if it's directed
    feed: Option<Feed>,
    /// directed by Neuro
    neuro: Option<crate::neuro::session::Session>,
    /// no switches or doors (an arena's fight)
    no_use: bool,
    /// secrets found so far (as the tally has them)
    secrets_seen: i32,
    m: Arc<MapData>,
    map_name: String,
    engine: Engine,
    agent: Agent,
    palette: Vec<u32>,
    frame: Option<(usize, usize, Vec<u8>)>,
    obs: Option<Observation>,
    controls: Option<Controls>,
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
    /// the log panel showing only what passes between the game and Evil (L; the user's)
    evil_log: bool,
    /// which of TEXT_SIZES the text's drawn at (T)
    text_size: usize,
    zoom: f64,
    center: Option<Point>,
    bounds: (f64, f64, f64, f64),
    lives_left: i32,
}

pub fn watch(wad: &str, map_name: &str, o: &WatchOptions) -> Result<(), String> {
    let sound = o.sound && !o.headless;
    let (w, mut watch) = first_watch(wad, map_name, o, sound)?;
    (watch.sound, watch.sound_on) = (sound, sound);
    if let Some(addr) = &o.control {
        watch.console = Some(console::Console::connect(addr, wad, map_name)?);
    }
    if o.headless {
        return headless(watch, &w, o);
    }
    let title = format!("evil-doom explorer: {}", watch.map_name);
    show(watch, &title, o.episode.then_some((&w, o)))
}

/// The WAD and the watcher for the first level.
fn first_watch(wad: &str, map_name: &str, o: &WatchOptions, sound: bool) -> Result<(Wad, Watch), String> {
    let iwad = resolve_wad(wad);
    let w = Wad::open(&iwad)?;
    let map_name = map_name.to_uppercase();
    let m = Arc::new(load_map(&w, &map_name, &[])?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    let mut engine = Engine::new(&iwad, &map_name, &Options { skill, nomonsters: o.fight == 0, objects: o.fight > 0 || o.expert, drawn: true, sound, capture: o.capture_sound, seed: o.seed })?;
    engine.carry.clone_from(&o.carry);
    engine.start();
    if !o.mortal {
        engine.set_god(true);
    }
    let watch = level(&w, m, nav, engine, o)?;
    Ok((w, watch))
}

/// the HUD's corner map: zoomed in this much, round her
const HUD_ZOOM: f64 = 3.0;

/// What a recording shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordView {
    /// the game's own view
    Game,
    /// the whole watcher window (the game, the map of what she knows, her log)
    Full,
    /// the game's view with what she's at over it (health, ammo, what she's doing, the fight,
    /// a map in the corner)
    Hud,
}

/// How to record.
pub struct RecordOptions {
    pub out: std::path::PathBuf,
    pub view: RecordView,
    /// the video's size (None: the view's own; the game's, 1280x960)
    pub size: Option<(u32, u32)>,
    /// a frame kept every this many tics (2: twice as fast)
    pub every: u32,
    /// x264's quality (lower: better and bigger; 18 about lossless to the eye) and speed preset
    pub crf: u32,
    pub preset: String,
    pub ffmpeg: std::path::PathBuf,
}

/// ffmpeg: on the path, else where winget put it.
pub fn find_ffmpeg() -> Option<std::path::PathBuf> {
    let ok = |p: &std::path::Path| std::process::Command::new(p).arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    if ok(Path::new("ffmpeg")) {
        return Some("ffmpeg".into());
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    let packages = Path::new(&local).join("Microsoft").join("WinGet").join("Packages");
    for e in std::fs::read_dir(&packages).ok()?.flatten() {
        if !e.file_name().to_string_lossy().to_lowercase().contains("ffmpeg") {
            continue;
        }
        for b in std::fs::read_dir(e.path()).ok()?.flatten() {
            let exe = b.path().join("bin").join("ffmpeg.exe");
            if ok(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

/// The run watched, as fast as it goes, into an MP4 (ffmpeg encoding the frames piped to it):
/// the level (and with --episode the ones after), as a watch plays it.
pub fn record(wad: &str, map_name: &str, o: &WatchOptions, r: &RecordOptions) -> Result<(), String> {
    use std::io::Write;
    let (w, mut watch) = first_watch(wad, map_name, o, false)?;
    let (fw, fh) = match r.view {
        RecordView::Full => (W, H),
        RecordView::Hud => (GAME.2, GAME.3),
        RecordView::Game => {
            watch.tic();
            watch.frame.as_ref().map(|f| (f.0, f.1)).ok_or("the game drew nothing")?
        }
    };
    let (ow, oh) = r.size.unwrap_or(match r.view {
        RecordView::Game => (1280, 960),
        _ => (fw as u32, fh as u32),
    });
    // (Doom's pixels are taller than wide: the game's view stretched to 4:3; crisp)
    let scale = format!("scale={ow}:{oh}:flags={}", if r.view == RecordView::Game { "neighbor" } else { "bicubic" });
    // (somewhere to put it: ffmpeg makes no folders)
    if let Some(dir) = r.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // (at x1, the game's sound too: taken a tic at a time beside the frames, put with them at the
    // end; the user's)
    let video_out = if o.capture_sound { r.out.with_extension("video.mp4") } else { r.out.clone() };
    let audio_out = r.out.with_extension("audio.raw");
    let mut audio = if o.capture_sound { Some(std::io::BufWriter::new(std::fs::File::create(&audio_out).map_err(|e| format!("{}: {e}", audio_out.display()))?)) } else { None };
    let mut audio_rate = 0;
    let mut ff = std::process::Command::new(&r.ffmpeg)
        .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "bgr0", "-s", &format!("{fw}x{fh}"), "-r", "35", "-i", "-"])
        .args(["-vf", &scale, "-c:v", "libx264", "-preset", &r.preset, "-crf", &r.crf.to_string(), "-pix_fmt", "yuv420p"])
        .arg(&video_out)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", r.ffmpeg.display()))?;
    let mut pipe = ff.stdin.take().ok_or("no pipe to ffmpeg")?;
    println!("recording {} at {ow}x{oh} (the view {fw}x{fh})", r.out.display());
    let mut began = Instant::now();
    let mut canvas = Canvas { px: vec![0; W * H], clip: (0, 0, W, H), text_scale: 1.0 };
    if r.view == RecordView::Hud {
        // (the corner map round her, near enough to read)
        (watch.follow, watch.zoom) = (true, HUD_ZOOM);
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(fw * fh * 4);
    let mut start = watch.engine.tic;
    let mut n: u64 = 0;
    // (directed, by Neuro or the GPT stand-in or an orders file: the game at its own pace, real
    // time, whoever directs her thinking on the clock; the user's. The video's speed is --speed's)
    let real_time = o.neuro.is_some() || o.directed.is_some();
    loop {
        let (paced_from, paced_tic) = (Instant::now(), watch.engine.tic);
        while !watch.engine.finished() && ((watch.engine.tic - start) as f64) < o.max_seconds * TICRATE {
            if real_time {
                let due = paced_from + std::time::Duration::from_secs_f64((watch.engine.tic - paced_tic) as f64 / TICRATE);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
            watch.tic();
            if let (Some(file), Some((rate, samples))) = (audio.as_mut(), watch.engine.grab_tic_sound()) {
                audio_rate = rate;
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                file.write_all(&bytes).map_err(|e| format!("{}: {e}", audio_out.display()))?;
            }
            n += 1;
            if !n.is_multiple_of(u64::from(r.every.max(1))) {
                continue;
            }
            bytes.clear();
            match r.view {
                RecordView::Game => {
                    let Some((gw, gh, px)) = &watch.frame else { continue };
                    if (*gw, *gh) != (fw, fh) {
                        continue;
                    }
                    for &i in px {
                        bytes.extend_from_slice(&watch.palette[usize::from(i)].to_le_bytes());
                    }
                }
                RecordView::Full => {
                    watch.draw(&mut canvas);
                    for &p in &canvas.px {
                        bytes.extend_from_slice(&p.to_le_bytes());
                    }
                }
                RecordView::Hud => {
                    watch.draw_hud(&mut canvas);
                    for y in GAME.1..GAME.1 + GAME.3 {
                        for &p in &canvas.px[y * W + GAME.0..y * W + GAME.0 + GAME.2] {
                            bytes.extend_from_slice(&p.to_le_bytes());
                        }
                    }
                }
            }
            pipe.write_all(&bytes).map_err(|e| format!("ffmpeg: {e}"))?;
            // (how far: the game time recorded, a line written over every game second)
            if (watch.engine.tic - start) % TICRATE as i64 == 0 {
                let secs = (watch.engine.tic - start) as f64 / TICRATE;
                let real = began.elapsed().as_secs_f64().max(1e-3);
                eprint!("
  {} {}:{:02} game  ({:.1}x real time)   ", watch.map_name, secs as i64 / 60, secs as i64 % 60, secs / real);
                let _ = std::io::stderr().flush();
            }
        }
        eprintln!();
        let t = watch.engine.tally();
        let how = if watch.engine.exited() { "EXITED" } else if watch.engine.dead() { "DIED" } else { "TIME" };
        println!("{}: {how} items {}/{}, {:.0}s game", watch.map_name, t[0].0, t[0].1, (watch.engine.tic - start) as f64 / TICRATE);
        if !(o.episode && watch.engine.exited()) {
            break;
        }
        match next_level(watch, &w, o)? {
            Some(next) => watch = next,
            None => break,
        }
        if r.view == RecordView::Hud {
            (watch.follow, watch.zoom) = (true, HUD_ZOOM);
        }
        start = watch.engine.tic;
        began = Instant::now(); // (the rate level by level)
    }
    drop(pipe);
    let status = ff.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("ffmpeg failed ({status})"));
    }
    if let Some(mut file) = audio.take() {
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        if audio_rate == 0 {
            // (no sound to be had from this library: the video as it is)
            std::fs::rename(&video_out, &r.out).map_err(|e| e.to_string())?;
            println!("(no sound: the engine library has no capture; rebuild it with tools/build-engine.sh)");
        } else {
            let muxed = std::process::Command::new(&r.ffmpeg)
                .args(["-y", "-loglevel", "error", "-i"])
                .arg(&video_out)
                .args(["-f", "s16le", "-ar", &audio_rate.to_string(), "-ac", "2", "-i"])
                .arg(&audio_out)
                .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-shortest"])
                .arg(&r.out)
                .status()
                .map_err(|e| e.to_string())?;
            if !muxed.success() {
                return Err(format!("ffmpeg putting the sound with the video failed ({muxed}); the parts are {} and {}", video_out.display(), audio_out.display()));
            }
            let _ = std::fs::remove_file(&video_out);
        }
        let _ = std::fs::remove_file(&audio_out);
    }
    println!("written {}", r.out.display());
    Ok(())
}

/// The watch played through without a window, as fast as it goes (the time cap as a run's):
/// a line as each level ends.
fn headless(mut watch: Watch, w: &Wad, o: &WatchOptions) -> Result<(), String> {
    use std::io::Write;
    let mut start = watch.engine.tic;
    // (the log as it's written, to follow live: tail -f)
    let live = out_dir().join("watch").join("live.log");
    std::fs::create_dir_all(live.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let mut log = crate::config::Log::create(&live, &o.seed.map_or_else(String::new, |s| format!("s{s}"))).map_err(|e| e.to_string())?;
    println!("log: {}{}", live.display(), if o.paced { format!(" (at x{})", o.speed) } else { String::new() });
    loop {
        let mut shown = 0;
        let (began, first) = (Instant::now(), watch.engine.tic);
        let _ = writeln!(log, "== {}", watch.map_name);
        while !watch.engine.finished() && ((watch.engine.tic - start) as f64) < o.max_seconds * TICRATE {
            watch.tic();
            let lines = &watch.agent.x().log;
            let at = watch.engine.tic as f64 / TICRATE;
            for l in &lines[shown.min(lines.len())..] {
                let _ = writeln!(log, "[{at:7.1}s] {l}");
            }
            shown = lines.len();
            if o.paced && o.speed > 0.0 {
                let due = began + std::time::Duration::from_secs_f64((watch.engine.tic - first) as f64 / (TICRATE * o.speed));
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        }
        let t = watch.engine.tally();
        let how = if watch.engine.exited() { "EXITED" } else if watch.engine.dead() { "DIED" } else { "TIME" };
        let line = format!("{}: {how} kills {}/{}, items {}/{}, secrets {}/{}, {:.0}s game", watch.map_name, t[1].0, t[1].1, t[0].0, t[0].1, t[2].0, t[2].1, (watch.engine.tic - start) as f64 / TICRATE);
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

/// The watcher for the level the engine's on: Evil in it afresh, as a run starts her.
fn level(w: &Wad, m: Arc<MapData>, nav: Rc<Navigator>, engine: Engine, o: &WatchOptions) -> Result<Watch, String> {
    let map_name = engine.map_name.clone();
    let skill = if o.fight > 0 { o.fight } else { 3 };
    // (seeing as a run does: watched, a run plays as it did; the user's)
    let knowledge = Knowledge::new(Rc::clone(&nav), o.mortal, !o.peek);
    let mut agent = Agent::new(&nav, knowledge, o.fight, o.expert, o.lives, Tuning::from(o.tuning.as_deref())?);
    // (a 100% run as a sweep plays it: the exit held back while what it's after is short, given
    // up on after a stall)
    let hold = (o.expert || o.secrets || o.complete).then(|| {
        let ro = RunOptions { expert: o.expert, secrets: o.secrets, complete: o.complete, fight: o.fight, stall_seconds: o.stall_seconds, ..RunOptions::default() };
        Hold { secrets_short: broken_secrets(&map_name), items_short: most_items(&m, skill), o: ro, best: [0; 7], best_tic: 0, given_up: false }
    });
    if o.secrets {
        agent.x_mut().exit_last = true;
        agent.x_mut().secrets_mode = true;
    }
    if o.directed.is_some() || o.neuro.is_some() {
        crate::control::directed::Director::take_over(&mut agent);
    }
    let mut watch = Watch::new(w, m, &map_name, engine, agent, o.focus, o.lives - 1);
    watch.feed = o.directed.as_deref().map(Feed::new);
    watch.neuro = o.neuro.as_deref().map(crate::neuro::session::Session::new);
    watch.hold = hold;
    watch.speed = o.speed;
    watch.show_all = o.show_all;
    Ok(watch)
}

/// The episode's next level, once she's out of this one: what she got out with carried, the
/// map the exit she took leads to (None: the episode's over).
fn next_level(watch: Watch, w: &Wad, o: &WatchOptions) -> Result<Option<Watch>, String> {
    let secret = watch.engine.secret_exit();
    let Some(next) = crate::sweep::next_map(&watch.map_name, secret) else { return Ok(None) };
    let (speed, show_all, follow) = (watch.speed, watch.show_all, watch.follow);
    let (sound, muted, sound_on) = (watch.sound, watch.muted, watch.sound_on);
    let console = watch.console;
    let mut engine = watch.engine;
    engine.carry = Some(engine.loadout());
    let m = Arc::new(load_map(w, &next, &[])?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    engine.goto(&next)?;
    let mut watch = level(w, m, Rc::new(nav), engine, o)?;
    (watch.speed, watch.show_all, watch.follow) = (speed, show_all, follow);
    (watch.sound, watch.muted, watch.sound_on) = (sound, muted, sound_on);
    watch.console = console;
    Ok(Some(watch))
}

/// A fight set up already (an arena: the engine with its monsters spawned and her loadout given,
/// drawn; her agent), watched: one life, no switches or doors (as `evil-boom arena` plays it).
pub fn watch_prepared(p: crate::arena::Prepared, map_name: &str, speed: f64, sound: bool, title: &str) -> Result<(), String> {
    let mut watch = Watch::new(&p.wad, p.map, map_name, p.engine, p.agent, None, 0);
    watch.speed = speed;
    (watch.sound, watch.sound_on) = (sound, sound);
    watch.no_use = true;
    show(watch, title, None)
}

/// The window, till it's shut or Esc: space pauses, N (or the right arrow) steps a tic while
/// paused, +/- the speed, A the whole map, F follow, the wheel zooms.
fn show(mut watch: Watch, title: &str, episode: Option<(&Wad, &WatchOptions)>) -> Result<(), String> {
    let mut window = Window::new(title, W, H, WindowOptions { resize: true, scale: Scale::FitScreen, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default() }).map_err(|e| e.to_string())?;
    window.set_target_fps(60);
    let mut canvas = Canvas { px: vec![0; W * H], clip: (0, 0, W, H), text_scale: 1.0 };
    let mut last = Instant::now();
    let mut owed = 0.0;
    while window.is_open() && !window.is_key_down(K::Escape) {
        for key in window.get_keys_pressed(KeyRepeat::No) {
            match key {
                K::Space => watch.paused = !watch.paused,
                K::N | K::Right if watch.paused && !watch.engine.finished() => watch.tic(),
                K::Equal | K::NumPadPlus => watch.speed = (watch.speed * 2.0).min(16.0),
                K::Minus | K::NumPadMinus => watch.speed = (watch.speed / 2.0).max(0.25),
                K::A => watch.show_all = !watch.show_all,
                K::F => watch.follow = !watch.follow,
                K::L => watch.evil_log = !watch.evil_log,
                K::T => watch.text_size = (watch.text_size + 1) % TEXT_SIZES.len(),
                K::M => watch.muted = !watch.muted,
                _ => {}
            }
        }
        if !watch.console_frame() {
            return Ok(()); // (the console's asked the game to close)
        }
        // (the sound only at the game's own speed: faster or slower it'd pile up or lag)
        let sound = watch.sound && !watch.muted && !watch.paused && (watch.speed - 1.0).abs() < 1e-9;
        if sound != watch.sound_on {
            watch.engine.sound(sound);
            watch.sound_on = sound;
        }
        if let Some((_, dy)) = window.get_scroll_wheel() {
            if dy != 0.0 {
                watch.zoom = (watch.zoom * if dy > 0.0 { 1.25 } else { 0.8 }).clamp(1.0, 20.0);
            }
        }
        let now = Instant::now();
        owed += (now - last).as_secs_f64() * TICRATE * watch.speed;
        last = now;
        owed = f64::min(owed, 8.0); // (don't race to catch up after a slow plan)
        let wanted = watch.agent.x().restart_wanted;
        if wanted && watch.lives_left <= 0 {
            watch.agent.x_mut().restart_wanted = false; // (out with what she has)
        }
        if (watch.engine.dead() || wanted) && watch.lives_left > 0 && !watch.paused {
            watch.agent.x_mut().restart_wanted = false;
            watch.lives_left -= 1;
            watch.engine.restart(true);
            watch.agent.restart();
            watch.controls = None;
        }
        while owed >= 1.0 && !watch.paused && !watch.engine.finished() {
            watch.tic();
            owed -= 1.0;
        }
        if let Some((w, o)) = episode.filter(|_| watch.engine.exited()) {
            // (out: on to the next level, what she got out with carried)
            let name = watch.map_name.clone();
            let carried = watch.engine.loadout();
            match next_level(watch, w, o)? {
                Some(next) => {
                    watch = next;
                    println!("{name} done: on to {} with health {} armor {}, ammo {}/{}/{}/{}", watch.map_name, carried.health, carried.armor, carried.clip, carried.shell, carried.cell, carried.rocket);
                    window.set_title(&format!("evil-doom explorer: {} (from {name}: health {} armor {})", watch.map_name, carried.health, carried.armor));
                }
                None => return Ok(()), // (the episode's over)
            }
        }
        if watch.paused {
            owed = 0.0;
        }
        watch.draw(&mut canvas);
        window.update_with_buffer(&canvas.px, W, H).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The window as it is now (the game's frame, the map, the panels) into out/watch.png, for a run
/// played as a sweep plays it (run-map --snapshot): drawn once, at the end.
pub fn save_snapshot(w: &Wad, m: Arc<MapData>, map_name: &str, engine: Engine, agent: Agent, focus: Option<(f64, f64, f64)>) -> Result<(), String> {
    snapshot_to(&out_dir().join("watch.png"), w, m, map_name, engine, agent, focus).2
}

/// The window drawn once into `path`, the game and Evil handed back after (evil-boom drive).
pub fn snapshot_to(path: &Path, w: &Wad, m: Arc<MapData>, map_name: &str, engine: Engine, agent: Agent, focus: Option<(f64, f64, f64)>) -> (Engine, Agent, Result<(), String>) {
    let mut watch = Watch::new(w, m, map_name, engine, agent, focus, 0);
    let obs = watch.engine.observe();
    watch.frame = watch.engine.frame();
    watch.options = watch.agent.objectives(&obs);
    watch.options_tic = obs.tic;
    watch.obs = Some(obs);
    let mut canvas = Canvas { px: vec![0; W * H], clip: (0, 0, W, H), text_scale: 1.0 };
    watch.draw(&mut canvas);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let result = save_png(path, &canvas.px);
    (watch.engine, watch.agent, result)
}

/// PLAYPAL's first palette, as 0RGB.
fn palette(w: &Wad) -> Vec<u32> {
    let Some(lump) = w.lumps.iter().find(|l| l.name == "PLAYPAL") else { return (0..256).map(|i| i * 0x0001_0101).collect() };
    w.read(lump).as_chunks::<3>().0.iter().take(256).map(|c| (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])).collect()
}

fn save_png(path: &Path, px: &[u32]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = px.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    writer.write_image_data(&data).map_err(|e| e.to_string())
}

impl Watch {
    fn new(w: &Wad, m: Arc<MapData>, map_name: &str, engine: Engine, agent: Agent, focus: Option<(f64, f64, f64)>, lives_left: i32) -> Watch {
        let xs = m.vertexes.iter().map(|v| v.x);
        let ys = m.vertexes.iter().map(|v| v.y);
        let bounds = (xs.clone().fold(f64::INFINITY, f64::min), ys.clone().fold(f64::INFINITY, f64::min), xs.fold(f64::NEG_INFINITY, f64::max), ys.fold(f64::NEG_INFINITY, f64::max));
        Watch {
            console: None,
            hold: None,
            feed: None,
            neuro: None,
            no_use: false,
            secrets_seen: 0,
            m,
            map_name: map_name.to_string(),
            engine,
            agent,
            palette: palette(w),
            frame: None,
            obs: None,
            controls: None,
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

    fn tic(&mut self) {
        let obs = self.engine.observe();
        // (a secret found told her, as a run tells her: watched, it plays as it did)
        let found = self.engine.tally()[2].0;
        if found > self.secrets_seen {
            if let Some(p) = self.agent.x().nav.poly_at(obs.pos()) {
                let s = self.agent.x().nav.sector_of_poly(p);
                self.agent.x_mut().found_secret(s, obs.tic);
            }
        }
        self.secrets_seen = found;
        self.frame = self.engine.frame();
        let tally = crate::control::directed::Tally::of(self.engine.tally());
        if let Some(feed) = &mut self.feed {
            feed.director.tally = tally;
            for l in feed.tic(&mut self.agent, &obs) {
                self.agent.x_mut().log.push(l);
            }
        }
        if let Some(neuro) = &mut self.neuro {
            neuro.director.tally = tally;
            for l in neuro.tic(&mut self.agent, &obs) {
                self.agent.x_mut().log.push(l);
            }
            // (and what the GPT stand-in said, if it's in this program)
            for s in crate::neuro::gpt::take_said() {
                self.agent.x_mut().log.push(format!("evil says: {}", s.replace('\n', " ")));
            }
        }
        let mut c = self.agent.act(&obs, self.controls.as_ref());
        if self.no_use {
            c.use_ = false;
        }
        self.engine.step(&c);
        for h in self.engine.take_hits() {
            let armor = if h.armor > 0 { format!(" (and {} armor)", h.armor) } else { String::new() };
            self.agent.x_mut().log.push(format!("tic {}: hit for {}{armor} by {}", self.engine.tic, h.health, h.from()));
        }
        self.controls = Some(c);
        if let Some(h) = self.hold.as_mut().filter(|_| obs.tic % TICRATE as i64 == 0) {
            let now = progress(&self.engine, self.agent.x(), &h.o);
            if now.iter().zip(&h.best).any(|(n, b)| n > b) {
                h.best = std::array::from_fn(|i| now[i].max(h.best[i]));
                h.best_tic = obs.tic;
            } else if !h.given_up && (obs.tic - h.best_tic) as f64 > h.o.stall_seconds * TICRATE {
                h.given_up = true;
                self.agent.x_mut().log.push(format!("tic {}: nothing more collected, learned or tried in {:.0}s: for the exit", obs.tic, h.o.stall_seconds));
            }
            hold_exit(&self.engine, &mut self.agent, &h.o, h.secrets_short, h.items_short, h.given_up);
        }
        if obs.tic - self.options_tic >= TICRATE as i64 {
            self.options = self.agent.objectives(&obs);
            self.options_tic = obs.tic;
        }
        self.obs = Some(obs);
    }

    fn to_screen(&self, p: Point) -> Point {
        let (x0, y0, x1, y1) = self.bounds;
        let pad = 12.0;
        let (mw, mh) = (MAP.2 as f64, MAP.3 as f64);
        let scale = ((mw - 2.0 * pad) / (x1 - x0).max(1.0)).min((mh - 2.0 * pad) / (y1 - y0).max(1.0)) * self.zoom;
        let (cx, cy) = match (&self.obs, self.follow, self.center) {
            (Some(o), true, _) => (o.x, o.y),
            (_, _, Some(c)) => c,
            _ => ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        };
        (MAP.0 as f64 + mw / 2.0 + (p.0 - cx) * scale, MAP.1 as f64 + mh / 2.0 - (p.1 - cy) * scale)
    }

    fn draw(&self, c: &mut Canvas) {
        c.text_scale = TEXT_SIZES[self.text_size];
        c.clip = (0, 0, W, H);
        c.fill((0, 0, W, H), BG);
        self.draw_game(c);
        // (each panel's text kept inside it, however big)
        c.clip = LOG;
        self.draw_log(c);
        self.draw_map(c);
        c.clip = PANEL;
        self.draw_panel(c);
        c.clip = (0, 0, W, H);
    }

    fn draw_game(&self, c: &mut Canvas) {
        let Some((fw, fh, px)) = &self.frame else {
            c.text("starting...", (20, 20), TEXT, 2);
            return;
        };
        for y in 0..GAME.3 {
            let sy = y * fh / GAME.3;
            for x in 0..GAME.2 {
                let sx = x * fw / GAME.2;
                if let Some(&i) = px.get(sy * fw + sx) {
                    c.px[(GAME.1 + y) * W + GAME.0 + x] = self.palette[usize::from(i)];
                }
            }
        }
        if self.engine.finished() {
            c.text(if self.engine.exited() { "EXITED" } else { "GAME OVER" }, (GAME.0 + 20, GAME.1 + 20), (255, 255, 0), 3);
        }
        if self.paused {
            c.text("PAUSED", (GAME.0 + GAME.2 - 110, GAME.1 + 10), (255, 255, 0), 2);
        }
    }

    fn draw_log(&self, c: &mut Canvas) {
        c.fill(LOG, (20, 20, 26));
        let x = self.agent.x();
        if self.evil_log {
            self.draw_evil_log(c);
            return;
        }
        let mut y = c.text("Log (L: Evil only)", (LOG.0 + 8, LOG.1 + 4), HEAD, 2);
        let tic_of = |s: &str| if s.starts_with("tic ") { s.split(':').next().and_then(|t| t.split_whitespace().last()).and_then(|t| t.parse::<i64>().ok()) } else { None };
        // (a line without a tic of its own (Evil's, the orders') at the last one before it: not
        // sorted to the top, out of sight)
        let mut last = 0;
        let mut merged: Vec<(i64, &String)> = x.log.iter().rev().take(40).rev().map(|l| {
            last = tic_of(l).unwrap_or(last);
            (last, l)
        }).collect();
        merged.extend(x.knowledge.events.iter().rev().take(40).rev().map(|l| (tic_of(l).unwrap_or(0), l)));
        merged.sort_by_key(|(t, _)| *t);
        let (width, rows) = c.fits(1, LOG.2 - 16, LOG.3 - (y - LOG.1));
        for (_, line) in merged.iter().rev().take(rows).rev() {
            let colour = if line.contains("dropped") {
                (255, 140, 140)
            } else if line.contains("learned") {
                (140, 220, 255)
            } else {
                TEXT
            };
            let s: String = line.split(" | ").next().unwrap_or("").chars().take(width).collect();
            y = c.text(&s, (LOG.0 + 8, y), colour, 1);
        }
    }

    /// The log panel with only what passes between the game and Evil: her actions (>>), what
    /// she's told (<<), the connection; whole, wrapped, the latest at the bottom.
    fn draw_evil_log(&self, c: &mut Canvas) {
        let mut y = c.text("Evil (L: whole log)", (LOG.0 + 8, LOG.1 + 4), HEAD, 2);
        let (fit, rows_fit) = c.fits(1, LOG.2 - 16, LOG.3 - (y - LOG.1));
        let (width, rows_max) = (fit.saturating_sub(3).max(10), rows_fit);
        let mut rows: Vec<(String, (u8, u8, u8))> = Vec::new();
        for line in self.agent.x().log.iter().rev().filter(|l| is_evil_line(l) || l.starts_with("neuro: ")) {
            let colour = if line.starts_with(">> ") {
                (255, 220, 120)
            } else if line.starts_with("evil says: ") {
                SAID
            } else if line.contains("(!)") || line.contains("FAILED") {
                (255, 140, 140)
            } else if line.starts_with("neuro: ") {
                (140, 220, 255)
            } else {
                TEXT
            };
            let chars: Vec<char> = line.chars().collect();
            let wrapped: Vec<String> = chars.chunks(width).enumerate().map(|(i, ch)| format!("{}{}", if i > 0 { "   " } else { "" }, ch.iter().collect::<String>())).collect();
            rows.splice(0..0, wrapped.into_iter().map(|w| (w, colour)));
            if rows.len() >= rows_max {
                break;
            }
        }
        let skip = rows.len().saturating_sub(rows_max);
        for (s, colour) in rows.into_iter().skip(skip) {
            y = c.text(&s, (LOG.0 + 8, y), colour, 1);
        }
    }

    /// The last few lines between the game and Evil (her actions, the news, what she said), in a
    /// dimmed box ending at `bottom`, wrapped to the view's width.
    fn draw_hud_talk(&self, c: &mut Canvas, bottom: usize) {
        const ROWS: usize = 6;
        let scale = c.text_scale;
        c.text_scale = 1.5;
        let (width, _) = c.fits(1, GAME.2 - 24, GAME.3);
        let line_h = c.glyph(1) + (3.0 * c.text_scale).round() as usize;
        let mut rows: Vec<(String, Rgb)> = Vec::new();
        for line in self.agent.x().log.iter().rev().filter(|l| is_evil_line(l)) {
            let (text, colour) = if let Some(t) = line.strip_prefix(">> ") {
                (format!("Evil: {t}"), (255, 220, 120))
            } else if let Some(t) = line.strip_prefix("evil says: ") {
                (format!("Evil: \"{t}\""), SAID)
            } else {
                let t = line.trim_start_matches("<< ");
                let notable = t.contains("(!)") || t.contains("FAILED");
                // (without the orders file's time and the notable mark: the colour says it)
                let t = t.split_once("s] ").filter(|(at, _)| at.starts_with('[')).map_or(t, |(_, rest)| rest);
                (t.trim_start_matches("(!) ").to_string(), if notable { (255, 150, 150) } else { TEXT })
            };
            let chars: Vec<char> = text.chars().collect();
            let wrapped: Vec<(String, Rgb)> = chars.chunks(width.max(10)).map(|ch| (ch.iter().collect(), colour)).collect();
            rows.splice(0..0, wrapped);
            if rows.len() >= ROWS {
                break;
            }
        }
        let rows: Vec<(String, Rgb)> = rows.split_off(rows.len().saturating_sub(ROWS));
        if !rows.is_empty() {
            let top = bottom.saturating_sub(rows.len() * line_h + 8);
            for yy in top..bottom {
                for x in GAME.0..GAME.0 + GAME.2 {
                    let i = yy * W + x;
                    c.px[i] = (c.px[i] >> 1) & 0x007f_7f7f;
                }
            }
            let mut y = top + 4;
            for (s, colour) in rows {
                y = c.text(&s, (GAME.0 + 12, y), colour, 1);
            }
        }
        c.text_scale = scale;
    }

    /// The game's view with what she's at over it: the level, the time, health, armour, ammo at
    /// the top; what she's doing and what the fight's doing at the bottom; the map of what she
    /// knows in a corner (drawn where the watcher has it, shrunk into the view).
    fn draw_hud(&self, c: &mut Canvas) {
        c.clip = (0, 0, W, H);
        c.fill((0, 0, W, H), BG);
        self.draw_map(c);
        // (the map, shrunk into the top right of the view, before the game's drawn over the rest)
        let (mw, mh) = (GAME.2 * 3 / 10, GAME.2 * 3 / 10 * MAP.3 / MAP.2);
        // (each pixel the brightest of the block of the map it stands for: a line a pixel wide
        // still there shrunk, not caught or missed as the map moves, shimmering)
        let bright = |p: u32| ((p >> 16) & 255) * 3 + ((p >> 8) & 255) * 6 + (p & 255);
        let small: Vec<u32> = (0..mh)
            .flat_map(|y| (0..mw).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (x0, x1) = (MAP.0 + x * MAP.2 / mw, MAP.0 + ((x + 1) * MAP.2 / mw).max(x * MAP.2 / mw + 1));
                let (y0, y1) = (MAP.1 + y * MAP.3 / mh, MAP.1 + ((y + 1) * MAP.3 / mh).max(y * MAP.3 / mh + 1));
                (y0..y1).flat_map(|yy| (x0..x1).map(move |xx| (xx, yy))).map(|(xx, yy)| c.px[yy * W + xx]).max_by_key(|&p| bright(p)).unwrap_or(0)
            })
            .collect();
        // (and two pixels wide: each the brightest of it and the ones right of and below it)
        let small: Vec<u32> = (0..mh)
            .flat_map(|y| (0..mw).map(move |x| (x, y)))
            .map(|(x, y)| [(x, y), ((x + 1).min(mw - 1), y), (x, (y + 1).min(mh - 1))].iter().map(|&(a, b)| small[b * mw + a]).max_by_key(|&p| bright(p)).unwrap_or(0))
            .collect();
        c.clip = (0, 0, W, H);
        self.draw_game(c);
        let (mx, my) = (GAME.0 + GAME.2 - mw - 8, GAME.1 + 40);
        for y in 0..mh {
            for x in 0..mw {
                let p = small[y * mw + x];
                let i = (my + y) * W + mx + x;
                // (half see-through over the view)
                let q = c.px[i];
                let mix = |s: u32| (((p >> s) & 255) * 3 / 4 + ((q >> s) & 255) / 4) << s;
                c.px[i] = mix(16) | mix(8) | mix(0);
            }
        }
        let bar = |c: &mut Canvas, y: usize| {
            for yy in y..y + 26 {
                for x in GAME.0..GAME.0 + GAME.2 {
                    let i = yy * W + x;
                    let q = c.px[i];
                    c.px[i] = (q >> 1) & 0x007f_7f7f;
                }
            }
        };
        bar(c, GAME.1 + 4);
        // (above the game's status bar, a sixth of its view)
        let low = GAME.1 + GAME.3 - GAME.3 * 32 / 200;
        bar(c, low - 58);
        bar(c, low - 30);
        if let Some(obs) = &self.obs {
            let ammo: Vec<String> = [2, 3, 5, 6].iter().map(|&s| obs.ammo.iter().flatten().find(|(x, _)| *x == s).map_or(0, |(_, n)| *n).to_string()).collect();
            let secs = (self.engine.tic as f64 / TICRATE) as i64;
            let top = format!(
                "{}  {}:{:02}   health {:.0}  armor {:.0}   ammo {}",
                self.map_name,
                secs / 60,
                secs % 60,
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0),
                ammo.join("/")
            );
            c.text(&top, (GAME.0 + 8, GAME.1 + 9), TEXT, 2);
        }
        // (directed: her orders, not the explorer's objective; and what passes between the game and
        // her above it, the latest few, for a recording of an LLM playing: the user's)
        let director = self.neuro.as_ref().map(|n| &n.director).or(self.feed.as_ref().map(|f| &f.director));
        let doing = director.map_or_else(|| self.agent.x().current.as_ref().map_or("-".to_string(), |o| o.label.clone()), |d| d.doing(&self.agent));
        c.text(&format!("{}: {}", if director.is_some() { "order" } else { "doing" }, doing.chars().take(46).collect::<String>()), (GAME.0 + 8, low - 53), TEXT, 2);
        if director.is_some() {
            self.draw_hud_talk(c, low - 62);
        }
        let fight = self.agent.player().map_or("", |p| p.combat().mode());
        let fight = if fight.is_empty() { "no fight" } else { fight };
        c.text(&format!("fight: {}", fight.chars().take(48).collect::<String>()), (GAME.0 + 8, low - 25), (255, 200, 120), 2);
    }

    fn draw_map(&self, c: &mut Canvas) {
        c.fill(MAP, (8, 8, 10));
        c.clip = MAP;
        let m = &self.m;
        let x = self.agent.x();
        let k = &x.knowledge;
        let heights = self.obs.as_ref().map(|o| o.heights.clone()).unwrap_or_default();
        for (li, line) in m.linedefs.iter().enumerate() {
            let (a, b) = m.line_seg(li);
            let mapped = k.mapped.contains(&li) && line.flags & ML_DONTDRAW == 0;
            if !mapped && !self.show_all {
                continue;
            }
            let colour = if !mapped {
                (45, 45, 55)
            } else {
                match m.sides(li) {
                    (_, None) => (220, 60, 60),
                    (Some(f), Some(bk)) => match (heights.get(f), heights.get(bk)) {
                        (Some(hf), Some(hb)) if hf.floor != hb.floor => (190, 130, 70),
                        (Some(hf), Some(hb)) if hf.ceiling != hb.ceiling => (220, 220, 80),
                        _ => (70, 70, 70),
                    },
                    _ => (70, 70, 70),
                }
            };
            c.line(self.to_screen(a), self.to_screen(b), colour, 1);
        }
        // what lines are, as far as she can tell
        let fired: std::collections::BTreeSet<usize> = k.fired.iter().map(|f| f.line).collect();
        let mut marks: Vec<(usize, Rgb)> = k.interactables().into_iter().map(|li| (li, if fired.contains(&li) { (90, 140, 90) } else { kind_colour(Kind::Switch) })).collect();
        marks.extend(fired.iter().filter(|li| k.mapped.contains(li)).map(|&li| (li, (90, 140, 90))));
        marks.extend(k.exits().into_iter().map(|li| (li, kind_colour(Kind::Exit))));
        marks.extend(k.teleporters().into_iter().map(|li| (li, (255, 255, 255))));
        for (li, colour) in marks {
            let (a, b) = m.line_seg(li);
            c.line(self.to_screen(a), self.to_screen(b), colour, 3);
        }
        // things she knows about
        for th in k.known_things() {
            let Some((name, kind)) = pickup(th.kind) else { continue };
            let p = self.to_screen((th.x, th.y));
            if kind == PickupKind::Key {
                c.circle(p, 6.0, key_colour(name), None);
                c.circle(p, 6.0, (255, 255, 255), Some(1.0));
            } else {
                c.circle(p, 2.0, item_colour(kind), None);
            }
        }
        // sounds she heard lately: a ring where each seemed to come from, fading
        let now = self.obs.as_ref().map_or(0, |o| o.tic);
        for h in k.heard.iter().rev().take(20) {
            let age = now - h.tic;
            if h.seen || age > 3 * 35 {
                continue;
            }
            let fade = 1.0 - age as f64 / (3.0 * TICRATE);
            let n = kind_colour(Kind::Noise);
            let col = ((f64::from(n.0) * fade) as u8, (f64::from(n.1) * fade) as u8, (f64::from(n.2) * fade) as u8);
            c.circle(self.to_screen(h.origin), 6.0 + (age / 4) as f64, col, Some(2.0));
        }
        // objectives open to her, the current one, the route
        for o in &self.options {
            let p = self.to_screen(o.goal);
            let col = kind_colour(o.kind);
            c.line((p.0 - 4.0, p.1 - 4.0), (p.0 + 4.0, p.1 + 4.0), col, 1);
            c.line((p.0 - 4.0, p.1 + 4.0), (p.0 + 4.0, p.1 - 4.0), col, 1);
        }
        if let (Some(obs), Some(f)) = (&self.obs, &x.follower) {
            let pts = f.planned_path(Some(obs.pos()));
            let mut run: Vec<Point> = Vec::new();
            for p in pts.into_iter().chain(std::iter::once(None)) {
                match p {
                    Some(q) => run.push(q),
                    None => {
                        for w in run.windows(2) {
                            c.line(self.to_screen(w[0]), self.to_screen(w[1]), (60, 255, 60), 2);
                        }
                        run.clear();
                    }
                }
            }
        }
        if let Some(cur) = &x.current {
            c.circle(self.to_screen(cur.goal), 8.0, kind_colour(cur.kind), Some(2.0));
        }
        if let Some(obs) = &self.obs {
            let p = self.to_screen(obs.pos());
            let a = obs.angle.to_radians();
            let at = |r: f64, off: f64| (p.0 + r * (a + off).cos(), p.1 - r * (a + off).sin());
            c.triangle([at(10.0, 0.0), at(6.0, 2.5), at(6.0, -2.5)], (255, 255, 255));
        }
        c.clip = (0, 0, W, H);
        let label = format!("map as known{}{}", if self.show_all { "  (+ unknown, faint)" } else { "" }, if self.follow { "  [follow]" } else { "" });
        c.text(&label, (MAP.0 + 8, MAP.1 + 4), DIM, 1);
    }

    fn draw_panel(&self, c: &mut Canvas) {
        c.fill(PANEL, (20, 20, 26));
        let x = self.agent.x();
        let k = &x.knowledge;
        let (left, right) = (PANEL.0 + 8, PANEL.0 + 410);
        let tic = self.obs.as_ref().map_or(0, |o| o.tic);
        let mut state = if self.engine.exited() {
            "EXITED".to_string()
        } else if self.engine.dead() {
            "DIED".to_string()
        } else if self.paused {
            "paused".to_string()
        } else {
            "exploring".to_string()
        };
        if let Some(h) = self.obs.as_ref().and_then(|o| o.health) {
            state += &format!("  {h:.0}hp");
        }
        if k.protected(tic) {
            state += &format!("  suit {:.0}s", (k.suit_until.unwrap_or(tic) - tic) as f64 / TICRATE);
        }
        let mut y = c.text(&format!("{}  {:6.1}s  x{}  {state}", self.map_name, tic as f64 / TICRATE, self.speed), (left, PANEL.1 + 6), HEAD, 1);
        let cur = x.current.as_ref();
        y = c.text(&format!("Now: {}", cur.map_or("-", |o| o.label.as_str())), (left, y + 2), cur.map_or(TEXT, |o| kind_colour(o.kind)), 1);
        if let Some(f) = &x.follower {
            let s: String = f.objective().chars().take(48).collect();
            y = c.text(&format!("     {s}"), (left, y), DIM, 1);
        }
        let m = &self.m;
        y = c.text(&format!("seen {}/{} lines, {}/{} sectors; {} fired", k.mapped.len(), m.linedefs.len(), k.sectors.len(), m.sectors.len(), k.fired.len()), (left, y + 2), DIM, 1);
        y = c.text("Objectives open", (left, y + 6), HEAD, 1);
        let pos = self.obs.as_ref().map_or((0.0, 0.0), Observation::pos);
        let mut opts: Vec<&Objective> = self.options.iter().collect();
        opts.sort_by(|a, b| dist(a.goal, pos).total_cmp(&dist(b.goal, pos)));
        for o in opts.into_iter().take(14) {
            let mark = if cur.is_some_and(|c| c.ident() == o.ident()) { ">" } else { " " };
            let label: String = o.label.chars().take(26).collect();
            y = c.text(&format!("{mark} {:8} {label:26} {:5.0}", kind_name(o.kind), dist(o.goal, pos)), (left, y), kind_colour(o.kind), 1);
        }
        // what she has found
        let mut y = c.text("Found", (right, PANEL.1 + 4), HEAD, 2);
        let held = self.obs.as_ref().and_then(|o| o.keys).unwrap_or_default();
        let mut keys: Vec<String> = Vec::new();
        let mut items: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for th in k.known_things() {
            let Some((name, kind)) = pickup(th.kind) else { continue };
            if kind == PickupKind::Key {
                let has = crate::specials::Key::from_thing(th.kind).is_some_and(|kk| held.has(kk));
                keys.push(format!("{name}{}", if has { " (held)" } else { "" }));
            } else {
                let cat = match kind {
                    PickupKind::Weapon => "weapon",
                    PickupKind::Health => "health",
                    PickupKind::Armor => "armor",
                    PickupKind::Ammo => "ammo",
                    _ => "powerup",
                };
                items.entry(cat).or_default().push(name);
            }
        }
        y = c.text(&format!("Keys: {}", if keys.is_empty() { "none seen".to_string() } else { keys.join(", ") }), (right, y + 2), (255, 255, 0), 1);
        for li in k.exits() {
            y = c.text(&format!("Exit: line {li}"), (right, y), kind_colour(Kind::Exit), 1);
        }
        for li in k.teleporters() {
            let to = k.teleports.get(&li).map_or(" (not used)".to_string(), |w| format!(" -> ({:.0}, {:.0})", w.0, w.1));
            y = c.text(&format!("Teleporter: line {li}{to}"), (right, y), (255, 255, 255), 1);
        }
        let untried = k.interactables();
        let locked: std::collections::BTreeSet<&str> = untried.iter().map(|&li| k.lock(li)).filter(|l| *l != Lock::None).map(Lock::value).collect();
        let n_locked = untried.iter().filter(|&&li| k.lock(li) != Lock::None).count();
        let extra = if n_locked > 0 { format!(" ({n_locked} locked: {})", locked.into_iter().collect::<Vec<_>>().join(", ")) } else { String::new() };
        y = c.text(&format!("Untried switches/doors: {}{extra}", untried.len()), (right, y), kind_colour(Kind::Switch), 1);
        for h in k.heard.iter().filter(|h| !h.seen).rev().take(3).collect::<Vec<_>>().into_iter().rev() {
            y = c.text(&format!("Heard ({:.0}s ago): something moving, bearing {:.0}, ~{:.0} away", (tic - h.tic) as f64 / TICRATE, h.bearing, h.distance), (right, y), kind_colour(Kind::Noise), 1);
        }
        let learned = k.known_triggers();
        y = c.text(&format!("Learned: {} lines", learned.len()), (right, y), (140, 220, 255), 1);
        for (t, secs) in learned.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
            let d: String = t.info.description.chars().take(28).collect();
            let s: String = format!("  line {}: {d} -> sectors {:?}", t.line, secs.iter().collect::<Vec<_>>()).chars().take(60).collect();
            y = c.text(&s, (right, y), (140, 220, 255), 1);
        }
        for cat in ["weapon", "health", "armor", "ammo", "powerup"] {
            let Some(names) = items.get(cat) else { continue };
            let mut uniq: Vec<&str> = names.clone();
            uniq.sort_unstable();
            uniq.dedup();
            let summary: Vec<String> = uniq.iter().map(|n| { let k = names.iter().filter(|x| *x == n).count(); if k > 1 { format!("{n} x{k}") } else { (*n).to_string() } }).collect();
            let colour = match cat {
                "weapon" => item_colour(PickupKind::Weapon),
                "health" => item_colour(PickupKind::Health),
                "armor" => item_colour(PickupKind::Armor),
                "ammo" => item_colour(PickupKind::Ammo),
                _ => item_colour(PickupKind::Powerup),
            };
            let mut title = cat.to_string();
            title[..1].make_ascii_uppercase();
            let s: String = format!("{title}: {}", summary.join(", ")).chars().take(62).collect();
            y = c.text(&s, (right, y), colour, 1);
        }
    }
}

/// A line of the log between the game and Evil: her action (>>), what she's told (<<), what she
/// said.
fn is_evil_line(l: &str) -> bool {
    l.starts_with(">> ") || l.starts_with("<< ") || l.starts_with("evil says: ")
}
