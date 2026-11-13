// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The watcher's window for the stream (`watch --stream [right|left]`): 16:9, 1920x1080. The game
//! at its own 4:3 fills 1440x1080 on one side; the 480 left over is a column for the viewers (in
//! Barlow Condensed, assets/fonts): the logo, a box for OBS's chat, the level and its name, a map of what she's seen that follows her (her route, her objectives), what
//! she's doing, what Evil said, the tally, the run so far, what's happened lately. What the game's
//! own status bar shows (health, ammo, keys) isn't said again. Which side the column's on is the
//! console's to set, live.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

use std::collections::BTreeMap;

use super::present::Stage;
use super::{Canvas, MapView, Rect, Rgb, Watch, DIM};
use crate::control::directed::plain;

/// the stream's window
pub const SW: usize = 1920;
pub const SH: usize = 1080;
/// the game's part: 4:3 at the window's height
const GW: usize = SH * 4 / 3;
/// the column's width, and its margin
const PW: usize = SW - GW;
const PAD: usize = 18;
/// the box for chat's height unless the console says, and the most it may be
pub const CHAT_HEIGHT: usize = 400;
pub const CHAT_MOST: usize = 800;
/// the chat box's heading, above it
const CHAT_HEAD: usize = 24;
/// the column's map's height unless the console says (its width: square), and the least
pub const MAP_HEIGHT: usize = PW - 2 * PAD;
const MAP_LEAST: usize = 120;
/// map units across the column's map (it follows her)
const MAP_SPAN: f64 = 1800.0;
/// what's happened lately: this many kept
const EVENTS: usize = 24;
/// Evil's lines kept
const SAID: usize = 3;

const BG: Rgb = (14, 14, 18);
const TEXT: Rgb = (232, 232, 236);
const RED: Rgb = (214, 52, 40);
const EVIL: Rgb = (200, 150, 255);

/// The column's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            _ => None,
        }
    }
}

/// The column's memory: the run so far and what's happened, across the levels.
pub(super) struct Stream {
    pub(super) side: Side,
    /// the column's items not shown (`console::COLUMN_ITEMS`; the console's, live)
    pub(super) hidden: Vec<String>,
    /// the box for chat's height, and the map's
    pub(super) chat_height: usize,
    pub(super) map_height: usize,
    /// the skill's name
    skill: &'static str,
    /// this level's title ("Dead Simple"), and the map it's for
    title: (String, String),
    /// what's happened, newest last, each with its colour and how many times in a row
    events: VecDeque<(String, Rgb, u32)>,
    /// what Evil said lately
    said: VecDeque<String>,
    /// her log's lines looked through
    log_seen: usize,
    levels: u32,
    deaths: u32,
    /// the run's game time (tics), and the engine's tic last counted
    run_tics: i64,
    last_tic: i64,
    /// (map, dead, out, secrets found) as last seen: what's changed is news
    last: (String, bool, bool, i32),
}

impl Stream {
    /// The column's box for chat, if it's shown: x, y, width, height in the picture.
    pub(super) fn chat_box(&self) -> Option<Rect> {
        if self.hidden.iter().any(|h| h == "chat") {
            return None;
        }
        let x = match self.side {
            Side::Right => GW,
            Side::Left => 0,
        };
        let h = self.chat_height.clamp(60, CHAT_MOST);
        Some((
            x + PAD,
            PAD + self.logo_height() + CHAT_HEAD,
            PW - 2 * PAD,
            h,
        ))
    }

    /// The room the logo takes at the column's top (none if it's hidden).
    fn logo_height(&self) -> usize {
        match logo() {
            Some(l) if !self.hidden.iter().any(|h| h == "logo") => l.1 + 12,
            _ => 0,
        }
    }

    pub(super) fn new(side: Side, fight: i32) -> Stream {
        let skill = match fight {
            0 => "no monsters",
            1 => "I'm too young to die",
            2 => "Hey, not too rough",
            3 => "Hurt me plenty",
            4 => "Ultra-Violence",
            _ => "Nightmare!",
        };
        Stream {
            side,
            hidden: Vec::new(),
            chat_height: CHAT_HEIGHT,
            map_height: MAP_HEIGHT,
            skill,
            title: (String::new(), String::new()),
            events: VecDeque::new(),
            said: VecDeque::new(),
            log_seen: 0,
            levels: 0,
            deaths: 0,
            run_tics: 0,
            last_tic: 0,
            last: (String::new(), false, false, 0),
        }
    }

    fn event(&mut self, text: String, colour: Rgb) {
        // (the same again: counted, not repeated)
        if let Some(last) = self.events.back_mut().filter(|e| e.0 == text) {
            last.2 += 1;
            return;
        }
        self.events.push_back((text, colour, 1));
        while self.events.len() > EVENTS {
            self.events.pop_front();
        }
    }

    /// The game's message of a tic ("Picked up a shotgun."), as news; not at a level's start (what
    /// the game's message held from before then).
    pub(super) fn message(&mut self, m: &str, tic: i64) {
        let m = m.trim();
        if !m.is_empty() && tic > crate::engine::WARMUP_TICS + 1 {
            self.event(m.to_string(), (190, 190, 200));
        }
    }

    /// What's changed since the last frame.
    fn update(&mut self, w: &Watch) {
        let tic = w.engine.tic;
        if tic > self.last_tic
            && w.engine.screen().is_none_or(|s| s.state == 0)
            && !w.engine.finished()
        {
            self.run_tics += tic - self.last_tic;
        }
        self.last_tic = tic;
        // (a level's title once it's on, the automap's, its "level 7:" left off: the game has it
        // for a level only)
        if self.title.1 != w.map_name
            && w.engine.screen().is_none_or(|s| s.state == 0 && !s.wiping)
            && w.engine.tic > crate::engine::WARMUP_TICS
        {
            if let Some(t) = w.engine.level_title() {
                let name = t.split_once(':').map_or(t.as_str(), |(_, n)| n).trim();
                let name = title_case(name);
                self.event(format!("{}: {name}", w.map_name), (110, 170, 255));
                self.title = (name, w.map_name.clone());
            }
        }
        let secrets = w.engine.tally()[2].0;
        let now = (
            w.map_name.clone(),
            w.engine.dead(),
            w.engine.exited(),
            secrets,
        );
        if now.0 == self.last.0 {
            if now.1 && !self.last.1 {
                self.deaths += 1;
                self.event(format!("Died on {}", now.0), RED);
            }
            if now.2 && !self.last.2 {
                self.levels += 1;
                let t = w.engine.tally();
                self.event(
                    format!(
                        "{} done: kills {}/{}, secrets {}/{}",
                        now.0, t[1].0, t[1].1, t[2].0, t[2].1
                    ),
                    (90, 210, 110),
                );
            }
            if now.3 > self.last.3 {
                self.event("Found a secret!".into(), (240, 210, 70));
            }
        }
        self.last = now;
        // (Evil's words and orders, from the log: her new agent's on a new level starts it again)
        let log = &w.agent.x().log;
        if log.len() < self.log_seen {
            self.log_seen = 0;
        }
        let new: Vec<String> = log[self.log_seen..].to_vec();
        self.log_seen = log.len();
        for l in new {
            if let Some(said) = l.strip_prefix("evil says: ") {
                self.said.push_back(said.trim().to_string());
                while self.said.len() > SAID {
                    self.said.pop_front();
                }
            } else if let Some(order) = l.strip_prefix(">> ").filter(|o| {
                o.starts_with("doom_")
                    && !o.starts_with("doom_list")
                    && !o.starts_with("doom_inspect")
                    && !o.starts_with("doom_status")
            }) {
                self.event(
                    format!(
                        "Evil: {}",
                        order.replace("doom_", "").replace(['{', '}', '"'], "")
                    ),
                    EVIL,
                );
            }
        }
    }
}

impl Watch {
    /// The stream's window: the game on its side, the column on the other.
    pub(super) fn draw_stream(&mut self, c: &mut Canvas) {
        let Some(mut st) = self.stream.take() else {
            return;
        };
        st.update(self);
        let (game, column): (Rect, Rect) = match st.side {
            Side::Right => ((0, 0, GW, SH), (GW, 0, PW, SH)),
            Side::Left => ((PW, 0, GW, SH), (0, 0, PW, SH)),
        };
        c.clip = (0, 0, SW, SH);
        c.text_scale = 1.0;
        self.draw_game_in(c, game);
        self.draw_column(c, column, &st);
        self.stream = Some(st);
    }

    /// The game's frame scaled into `r` as it is (Doom's pixels, the nearest).
    fn draw_game_in(&self, c: &mut Canvas, r: Rect) {
        let Some((fw, fh, px)) = &self.frame else {
            c.fill(r, (0, 0, 0));
            say(c, "starting...", (r.0 + 40, r.1 + 40), 24.0, TEXT);
            return;
        };
        let xs: Vec<usize> = Vec::from_fn(r.2, |x| x * fw / r.2);
        for y in 0..r.3 {
            let row = (y * fh / r.3) * fw;
            let out = (r.1 + y) * c.width + r.0;
            for (x, &sx) in xs.iter().enumerate() {
                if let Some(&i) = px.get(row + sx) {
                    c.px[out + x] = self.palette[usize::from(i)];
                }
            }
        }
    }

    fn draw_column(&self, c: &mut Canvas, r: Rect, st: &Stream) {
        c.fill(r, BG);
        // (an edge against the game)
        let edge = if r.0 == 0 { r.0 + r.2 - 3 } else { r.0 };
        c.fill((edge, 0, 3, r.3), (90, 16, 16));
        let (x, inner) = (r.0 + PAD, r.2 - 2 * PAD);
        let shows = |item: &str| !st.hidden.iter().any(|h| h == item);
        // (the box for chat first, at the top, so it never moves as what's below it wraps or
        // empties: OBS lays the chat over it; nothing past the column's foot)
        let clip = c.clip;
        c.clip = (r.0, 0, r.2, r.3 - PAD);
        let mut y = PAD;
        if let Some((w, h, px)) = logo().filter(|_| shows("logo")) {
            for (i, &p) in px.iter().enumerate() {
                let (lx, ly) = (x + (inner.saturating_sub(*w)) / 2 + i % w, y + i / w);
                c.put(lx as i64, ly as i64, p);
            }
            y += h + 12;
        }
        if let Some(b) = st.chat_box() {
            heading(c, "CHAT", (b.0, b.1 - CHAT_HEAD + 4));
            c.fill(b, (8, 8, 10));
            outline(c, b, (90, 16, 16));
            y = b.1 + b.3 + 14;
        }
        let director = self
            .neuro
            .as_ref()
            .map(|n| &n.director)
            .or(self.feed.as_ref().map(|f| &f.director));
        // the level, its name and its time, on one line (the name cut short to fit)
        if shows("level") {
            say(c, &self.map_name, (x, y), 30.0, RED);
            let time = clock(self.engine.tic);
            let time_w = text_w(&time, 18.0);
            say(c, &time, (x + inner - time_w, y + 12), 18.0, DIM);
            let from = x + text_w(&self.map_name, 30.0) + 12;
            let room = (x + inner).saturating_sub(time_w + 14 + from);
            say(c, &fit(&st.title.0, room, 20.0), (from, y + 10), 20.0, TEXT);
            y += line_h(30.0) + 8;
        }
        // the map of what she's seen, following her
        let bx: Rect = (x, y, inner, st.map_height.clamp(MAP_LEAST, inner));
        match &self.obs {
            _ if !shows("map") => {}
            Some(o)
                if self
                    .present
                    .as_ref()
                    .is_none_or(|p| matches!(p.stage, Stage::Level | Stage::Dead { .. })) =>
            {
                self.draw_map_in(
                    c,
                    MapView {
                        r: bx,
                        centre: (o.x, o.y),
                        scale: inner as f64 / MAP_SPAN,
                    },
                    false,
                );
            }
            _ => c.fill(bx, (8, 8, 10)),
        }
        if shows("map") {
            outline(c, bx, (52, 52, 64));
            y += bx.3 + 16;
        }
        // what she's doing
        if shows("now") {
            y = heading(c, "NOW", (x, y));
            y = lines(
                c,
                (x, y),
                &wrap_px(&self.doing_now(), inner, 18.0),
                3,
                18.0,
                TEXT,
            ) + 10;
        }
        // her calls: how she's fighting, what she's said that still holds
        let calls = director
            .map(|d| d.calls(self.engine.tic))
            .filter(|_| shows("calls"));
        if let Some(calls) = calls {
            y = heading(c, "EVIL'S CALLS", (x, y));
            y = lines(c, (x, y), &calls, 4, 15.0, EVIL) + 10;
        }
        // what she's been asked, its choices and its clock
        if let Some(q) = director
            .and_then(|d| d.question())
            .filter(|_| shows("asked"))
        {
            y = heading(c, "ASKED", (x, y));
            y = lines(c, (x, y), &wrap_px(&q.text, inner, 16.0), 3, 16.0, EVIL);
            let mut choices = Vec::new();
            for (i, ch) in q.choices.iter().enumerate() {
                let mark = match (i == q.default, q.now, q.deadline) {
                    (true, true, _) => " (doing it)".to_string(),
                    (true, false, Some(d)) => {
                        format!(" (default, {} s)", ((d - self.engine.tic).max(0) + 34) / 35)
                    }
                    (true, false, None) => " (default)".to_string(),
                    _ => String::new(),
                };
                let letter = char::from(b'a' + u8::try_from(i.min(25)).unwrap_or(0));
                choices.extend(wrap_px(
                    &format!("{letter}) {}{mark}", ch.label),
                    inner,
                    15.0,
                ));
            }
            y = lines(c, (x, y), &choices, 4, 15.0, TEXT) + 10;
        }
        // what Evil said
        if !st.said.is_empty() && shows("says") {
            y = heading(c, "EVIL SAYS", (x, y));
            let mut said: Vec<String> = Vec::new();
            for s in st.said.iter().rev().take(2) {
                said.extend(wrap_px(&format!("\"{s}\""), inner, 15.0));
            }
            y = lines(c, (x, y), &said, 4, 15.0, EVIL) + 10;
        }
        // the tally: kills, items and secrets side by side, each with its bar
        if shows("tally") {
            y = heading(c, "TALLY", (x, y));
            let t = self.engine.tally();
            let gap = 14;
            let w = (inner - 2 * gap) / 3;
            for (k, (name, (got, of), colour)) in [
                ("Kills", t[1], (200, 64, 52)),
                ("Items", t[0], (72, 160, 224)),
                ("Secrets", t[2], (232, 200, 64)),
            ]
            .into_iter()
            .enumerate()
            {
                let cx = x + k * (w + gap);
                say(c, name, (cx, y), 15.0, TEXT);
                let n = format!("{got}/{of}");
                say(c, &n, (cx + w - text_w(&n, 15.0), y), 15.0, TEXT);
                c.fill((cx, y + 19, w, 7), (40, 40, 48));
                let filled = if of > 0 {
                    w * usize::try_from(got.clamp(0, of)).unwrap_or(0)
                        / usize::try_from(of).unwrap_or(1)
                } else {
                    0
                };
                c.fill((cx, y + 19, filled, 7), colour);
            }
            y += 19 + 7 + 14;
        }
        // the run
        if shows("run") {
            y = heading(c, "RUN", (x, y));
            y = say(
                c,
                &format!(
                    "{}  ·  levels {}  ·  deaths {}  ·  {}",
                    st.skill,
                    st.levels,
                    st.deaths,
                    clock(st.run_tics)
                ),
                (x, y),
                15.0,
                TEXT,
            ) + 10;
        }
        let bottom = r.3 - PAD;
        // lately, newest first, till the column's full
        'lately: {
            if !shows("lately") {
                break 'lately;
            }
            y = heading(c, "LATELY", (x, y));
            for (i, (text, colour, n)) in st.events.iter().rev().enumerate() {
                let text = if *n > 1 {
                    format!("{text} x{n}")
                } else {
                    text.clone()
                };
                let fade = 1.0 - (i as f64 * 0.07).min(0.6);
                let col = (
                    (f64::from(colour.0) * fade) as u8,
                    (f64::from(colour.1) * fade) as u8,
                    (f64::from(colour.2) * fade) as u8,
                );
                for line in wrap_px(&text, inner, 14.0) {
                    if y + 16 > bottom {
                        break 'lately;
                    }
                    y = say(c, &line, (x, y), 14.0, col);
                }
            }
        }
        c.clip = clip;
    }

    /// What she's doing, for the viewers: the screen if it's not a level; her order (directed);
    /// else what she's after.
    fn doing_now(&self) -> String {
        if let Some(p) = &self.present {
            match &p.stage {
                Stage::Title(_) => return "On the title screen".into(),
                Stage::Dead { .. } => return "Dead!".into(),
                Stage::Over => return "Game over".into(),
                Stage::Between { last: true, .. } => return "The end".into(),
                Stage::Between { .. } => return "Level done".into(),
                Stage::Level => {}
            }
        }
        // (in a fight: what she's fighting, as the directed status has it)
        if let Some(obs) = &self.obs {
            let foes = self
                .agent
                .player()
                .map(|p| p.combat().engaged(obs))
                .unwrap_or_default();
            if !foes.is_empty() {
                let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
                for t in &foes {
                    *counts.entry(t.name).or_insert(0) += 1;
                }
                let what: Vec<String> = counts
                    .iter()
                    .map(|(n, c)| {
                        let (one, many) = monster(n);
                        if *c > 1 {
                            format!("{c} {many}")
                        } else if one.starts_with(['A', 'E', 'I', 'O', 'U']) {
                            format!("an {one}")
                        } else {
                            format!("a {one}")
                        }
                    })
                    .collect();
                return format!("Fighting {}", what.join(", "));
            }
        }
        let doing = if let Some(n) = &self.neuro {
            n.director.doing(&self.agent)
        } else if let Some(f) = &self.feed {
            f.director.doing(&self.agent)
        } else {
            self.agent
                .x()
                .current
                .as_ref()
                .map_or_else(|| "exploring".into(), |o| plain(o.kind, &o.label))
        };
        // (on her own or for her goal: what she's doing, no more; "your goal" is said to her,
        // not the viewers)
        let own = |how: &str| how == "by instinct" || how.starts_with("for your goal");
        let doing = match doing.split_once(": ") {
            Some((how, what)) if own(how) => what.to_string(),
            _ if own(&doing) => "exploring".to_string(),
            _ => doing,
        };
        let mut chars = doing.chars();
        chars.next().map_or(doing.clone(), |first| {
            first.to_uppercase().chain(chars).collect()
        })
    }
}

/// A monster's name as players know it (the game's own is its class's: "DoomImp"), one and many.
fn monster(class: &str) -> (String, String) {
    let (one, many) = match class {
        "DoomImp" => ("Imp", "Imps"),
        "ShotgunGuy" => ("Shotgun Guy", "Shotgun Guys"),
        "ChaingunGuy" => ("Heavy Weapon Dude", "Heavy Weapon Dudes"),
        "LostSoul" => ("Lost Soul", "Lost Souls"),
        "HellKnight" => ("Hell Knight", "Hell Knights"),
        "BaronOfHell" => ("Baron of Hell", "Barons of Hell"),
        "PainElemental" => ("Pain Elemental", "Pain Elementals"),
        "Fatso" => ("Mancubus", "Mancubi"),
        "Archvile" => ("Arch-Vile", "Arch-Viles"),
        "SpiderMastermind" => ("Spider Mastermind", "Spider Masterminds"),
        "WolfensteinSS" => ("SS Guard", "SS Guards"),
        "CommanderKeen" => ("Commander Keen", "Commander Keens"),
        "BossBrain" => ("Icon of Sin", "Icons of Sin"),
        other => return (other.to_string(), format!("{other}s")),
    };
    (one.to_string(), many.to_string())
}

/// Text at `px` pixels a character; the next line's y.
/// `s` at `at` (its top), `px` tall as the old 8x8 cell was, in the column's typeface; where
/// the next line starts.
fn say(c: &mut Canvas, s: &str, at: (usize, usize), px: f64, colour: Rgb) -> usize {
    let em = em_of(px);
    let base = at.1 as i64 + px.round() as i64;
    let mut x = at.0 as f64;
    let mut before = None;
    for ch in s.chars() {
        if let Some(b) = before {
            x += f64::from(face().horizontal_kern(b, ch, em).unwrap_or(0.0));
        }
        with_glyph(ch, em, |m, cover| {
            let top = base - (m.height as i64 + i64::from(m.ymin));
            let left = x.round() as i64 + i64::from(m.xmin);
            for (k, &a) in cover.iter().enumerate() {
                let (gx, gy) = ((k % m.width.max(1)) as i64, (k / m.width.max(1)) as i64);
                c.blend(left + gx, top + gy, colour, a);
            }
            x += f64::from(m.advance_width);
        });
        before = Some(ch);
    }
    at.1 + line_h(px)
}

/// The column's typeface: Barlow Condensed SemiBold (SIL Open Font License, assets/fonts).
fn face() -> &'static fontdue::Font {
    static FACE: OnceLock<fontdue::Font> = OnceLock::new();
    FACE.get_or_init(|| {
        let bytes: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-SemiBold.ttf");
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .expect("the column's typeface")
    })
}

/// A text's size (the old cell's height) as the typeface's em: its capitals about that tall.
fn em_of(px: f64) -> f32 {
    (px * 1.4) as f32
}

/// A line's height at `px`, as the old font stepped it (the layout's heights unchanged).
fn line_h(px: f64) -> usize {
    px.round() as usize + (3.0 * px / 8.0).round() as usize
}

/// A glyph as drawn: its metrics and coverage.
type Glyph = (fontdue::Metrics, Vec<u8>);

thread_local! {
    /// glyphs drawn already, by character and size (sixteenths of a pixel)
    static GLYPHS: RefCell<HashMap<(char, u32), Glyph>> = RefCell::new(HashMap::new());
}

/// `f` with `ch`'s glyph at `em`: its metrics and coverage, rasterized once.
fn with_glyph<R>(ch: char, em: f32, f: impl FnOnce(&fontdue::Metrics, &[u8]) -> R) -> R {
    GLYPHS.with(|g| {
        let mut g = g.borrow_mut();
        let (m, cover) = g
            .entry((ch, (em * 16.0) as u32))
            .or_insert_with(|| face().rasterize(ch, em));
        f(m, cover)
    })
}

/// `s` cut short with an ellipsis to fit `width` at `px` (as it is if it fits).
fn fit(s: &str, width: usize, px: f64) -> String {
    if text_w(s, px) <= width {
        return s.to_string();
    }
    let mut out: String = s.to_string();
    while !out.is_empty() && text_w(&format!("{out}…"), px) > width {
        out.pop();
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{}…", out.trim_end())
    }
}

/// How wide `s` is at `px`.
fn text_w(s: &str, px: f64) -> usize {
    let em = em_of(px);
    let mut w = 0.0;
    let mut before = None;
    for ch in s.chars() {
        if let Some(b) = before {
            w += face().horizontal_kern(b, ch, em).unwrap_or(0.0);
        }
        w += face().metrics(ch, em).advance_width;
        before = Some(ch);
    }
    w.ceil() as usize
}

/// The logo (assets/logo.png, made by tools/logo/make.py): its width, height and pixels.
fn logo() -> Option<&'static (usize, usize, Vec<Rgb>)> {
    static LOGO: OnceLock<Option<(usize, usize, Vec<Rgb>)>> = OnceLock::new();
    LOGO.get_or_init(|| {
        let bytes: &[u8] = include_bytes!("../../assets/logo.png");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .ok()?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).ok()?;
        let step = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            _ => return None,
        };
        let px = buf[..info.buffer_size()]
            .chunks(step)
            .map(|p| (p[0], p[1], p[2]))
            .collect();
        Some((info.width as usize, info.height as usize, px))
    })
    .as_ref()
}

/// Up to `most` of `texts` from `at`, one a line; where they end.
fn lines(
    c: &mut Canvas,
    at: (usize, usize),
    texts: &[String],
    most: usize,
    px: f64,
    colour: Rgb,
) -> usize {
    texts
        .iter()
        .take(most)
        .fold(at.1, |y, t| say(c, t, (at.0, y), px, colour))
}

fn heading(c: &mut Canvas, s: &str, at: (usize, usize)) -> usize {
    say(c, s, at, 13.0, (150, 150, 165)) + 2
}

fn outline(c: &mut Canvas, (x, y, w, h): Rect, colour: Rgb) {
    c.fill((x, y, w, 1), colour);
    c.fill((x, y + h - 1, w, 1), colour);
    c.fill((x, y, 1, h), colour);
    c.fill((x + w - 1, y, 1, h), colour);
}

/// Game tics as m:ss (h:mm:ss past an hour).
fn clock(tics: i64) -> String {
    let s = tics.max(0) / 35;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// "dead simple" as "Dead Simple".
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut cs = w.chars();
            cs.next()
                .map_or_else(String::new, |f| f.to_uppercase().chain(cs).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Words into lines of at most `width` characters (a longer word cut).
/// `s` in lines of at most `width` characters (a word longer than that broken).
#[cfg(test)]
fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    wrap_by(s, |t| t.chars().count() <= width)
}

/// `s` in lines at most `width` pixels wide at `px`.
fn wrap_px(s: &str, width: usize, px: f64) -> Vec<String> {
    wrap_by(s, |t| text_w(t, px) <= width)
}

/// `s` in lines that each `fit` (a word too long for one broken, as much of it as fits).
fn wrap_by(s: &str, fits: impl Fn(&str) -> bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let mut word = word.to_string();
        while !fits(&word) {
            let mut head = String::new();
            for ch in word.chars() {
                head.push(ch);
                if !fits(&head) {
                    head.pop();
                    break;
                }
            }
            if head.is_empty() {
                head = word.chars().take(1).collect();
            }
            word = word[head.len()..].to_string();
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(head);
        }
        if word.is_empty() {
            continue;
        }
        let joined = if line.is_empty() {
            word.clone()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && !fits(&joined) {
            lines.push(std::mem::replace(&mut line, word));
        } else {
            line = joined;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_wrapped() {
        assert_eq!(
            wrap("going to the blue door, rushing", 12),
            vec!["going to the", "blue door,", "rushing"]
        );
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert!(wrap("", 10).is_empty());
    }

    #[test]
    fn monsters_named() {
        assert_eq!(
            monster("Fatso"),
            ("Mancubus".to_string(), "Mancubi".to_string())
        );
        assert_eq!(
            monster("Demon"),
            ("Demon".to_string(), "Demons".to_string())
        );
    }

    #[test]
    fn titles_and_clocks() {
        assert_eq!(title_case("dead simple"), "Dead Simple");
        assert_eq!(clock(35 * 75), "1:15");
        assert_eq!(clock(35 * 3725), "1:02:05");
    }
}
