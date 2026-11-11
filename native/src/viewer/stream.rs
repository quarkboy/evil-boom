//! The watcher's window for the stream (`watch --stream [right|left]`): 16:9, 1920x1080. The game
//! at its own 4:3 fills 1440x1080 on one side; the 480 left over is a column for the viewers: the
//! level and its name, a map of what she's seen that follows her (her route, her objectives), what
//! she's doing, what Evil said, the tally, the run so far, what's happened lately. What the game's
//! own status bar shows (health, ammo, keys) isn't said again. Which side the column's on is the
//! streamer's (the console's, live).

use std::collections::VecDeque;

use std::collections::BTreeMap;

use super::present::Stage;
use super::{Canvas, MapView, Rect, Rgb, Watch, DIM};
use crate::knowledge::explorer::Kind;

/// the stream's window
pub const SW: usize = 1920;
pub const SH: usize = 1080;
/// the game's part: 4:3 at the window's height
const GW: usize = SH * 4 / 3;
/// the column's width, and its margin
const PW: usize = SW - GW;
const PAD: usize = 18;
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
        let xs: Vec<usize> = (0..r.2).map(|x| x * fw / r.2).collect();
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
        let mut y = PAD;
        y = say(c, &self.map_name, (x, y), 30.0, RED);
        for line in wrap(&st.title.0, inner / 20).into_iter().take(2) {
            y = say(c, &line, (x, y), 20.0, TEXT);
        }
        let level_time = clock(self.engine.tic);
        y = say(
            c,
            &format!("{}  ·  {level_time}", st.skill),
            (x, y + 2),
            14.0,
            DIM,
        ) + 10;
        // the map of what she's seen, following her
        let bx: Rect = (x, y, inner, inner);
        match &self.obs {
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
        outline(c, bx, (52, 52, 64));
        y += inner + 16;
        // what she's doing
        y = heading(c, "NOW", (x, y));
        for line in wrap(&self.doing_now(), inner / 18).into_iter().take(3) {
            y = say(c, &line, (x, y), 18.0, TEXT);
        }
        y += 10;
        // what Evil said
        if !st.said.is_empty() {
            y = heading(c, "EVIL SAYS", (x, y));
            let mut lines: Vec<String> = Vec::new();
            for s in st.said.iter().rev().take(2) {
                lines.extend(wrap(&format!("\"{s}\""), inner / 15));
            }
            for line in lines.iter().take(6) {
                y = say(c, line, (x, y), 15.0, EVIL);
            }
            y += 10;
        }
        // the tally
        y = heading(c, "TALLY", (x, y));
        let t = self.engine.tally();
        for (name, (got, of), colour) in [
            ("Kills", t[1], (200, 64, 52)),
            ("Items", t[0], (72, 160, 224)),
            ("Secrets", t[2], (232, 200, 64)),
        ] {
            say(c, name, (x, y), 15.0, TEXT);
            let n = format!("{got}/{of}");
            say(c, &n, (x + inner - n.len() * 15, y), 15.0, TEXT);
            y += 19;
            c.fill((x, y, inner, 7), (40, 40, 48));
            let filled = if of > 0 {
                inner * usize::try_from(got.clamp(0, of)).unwrap_or(0)
                    / usize::try_from(of).unwrap_or(1)
            } else {
                0
            };
            c.fill((x, y, filled, 7), colour);
            y += 15;
        }
        y += 6;
        // the run
        y = heading(c, "RUN", (x, y));
        y = say(
            c,
            &format!(
                "levels {}  ·  deaths {}  ·  {}",
                st.levels,
                st.deaths,
                clock(st.run_tics)
            ),
            (x, y),
            15.0,
            TEXT,
        ) + 10;
        // lately, newest first, till the column's full
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
            for line in wrap(&text, inner / 14) {
                if y + 16 > r.3 - PAD {
                    return;
                }
                y = say(c, &line, (x, y), 14.0, col);
            }
        }
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

/// What she's after, in the viewers' words (not the objective's own: "try the wall (line 8)").
fn plain(kind: Kind, label: &str) -> String {
    match kind {
        Kind::Exit => "heading for the exit".into(),
        Kind::Key => "going for a key".into(),
        Kind::Switch => "going to press a switch".into(),
        Kind::Door => "going to open a door".into(),
        Kind::Frontier | Kind::Room | Kind::Visit | Kind::Recheck => "exploring".into(),
        Kind::Noise => "checking out a noise".into(),
        Kind::Teleporter => "taking a teleporter".into(),
        Kind::Item => label.strip_prefix("pick up ").map_or_else(
            || "picking something up".into(),
            |what| format!("picking up the {}", what.to_lowercase()),
        ),
        Kind::Hunt => "hunting".into(),
        Kind::Wall => "searching for secrets".into(),
        Kind::Secret => "going back to a secret".into(),
        Kind::Barrel => "clearing a barrel".into(),
    }
}

/// Text at `px` pixels a character; the next line's y.
fn say(c: &mut Canvas, s: &str, at: (usize, usize), px: f64, colour: Rgb) -> usize {
    let kept = c.text_scale;
    c.text_scale = px / 8.0;
    let y = c.text(s, at, colour, 1);
    c.text_scale = kept;
    y
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
fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > width {
            let head: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(head);
        }
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
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
