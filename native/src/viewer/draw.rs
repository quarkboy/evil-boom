// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The watcher's picture drawn: the game, the log, the map as she knows it, the panel under it,
//! the stream's corner map.

use super::*;

impl Watch {
    /// The watcher's map: its box, the level fitted to it (zoomed), centred on her (follow), on
    /// a point asked for, or on the level.
    pub(super) fn map_view(&self) -> MapView {
        let (x0, y0, x1, y1) = self.bounds;
        let pad = 12.0;
        let (mw, mh) = (MAP.2 as f64, MAP.3 as f64);
        let scale = ((mw - 2.0 * pad) / (x1 - x0).max(1.0))
            .min((mh - 2.0 * pad) / (y1 - y0).max(1.0))
            * self.zoom;
        let centre = match (&self.obs, self.follow, self.center) {
            (Some(o), true, _) => (o.x, o.y),
            (_, _, Some(c)) => c,
            _ => ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        };
        MapView {
            r: MAP,
            centre,
            scale,
        }
    }

    pub(super) fn draw(&self, c: &mut Canvas) {
        c.text_scale = TEXT_SIZES[self.text_size];
        c.clip = (0, 0, W, H);
        c.fill((0, 0, W, H), BG);
        self.draw_game(c);
        // (each panel's text kept inside it, however big)
        c.clip = LOG;
        self.draw_log(c);
        // (the map's box is cut to the clip it's drawn under: its own here, not the log's)
        c.clip = MAP;
        self.draw_map(c);
        c.clip = PANEL;
        self.draw_panel(c);
        c.clip = (0, 0, W, H);
    }

    pub(super) fn draw_game(&self, c: &mut Canvas) {
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
        self.ultra_glow(c, GAME);
        self.draw_keys(c, GAME);
        if self.engine.finished() && self.present.is_none() {
            c.text(
                if self.engine.exited() {
                    "EXITED"
                } else {
                    "GAME OVER"
                },
                (GAME.0 + 20, GAME.1 + 20),
                (255, 255, 0),
                3,
            );
        }
        if self.paused {
            c.text(
                "PAUSED",
                (GAME.0 + GAME.2 - 110, GAME.1 + 10),
                (255, 255, 0),
                2,
            );
        }
    }

    pub(super) fn draw_log(&self, c: &mut Canvas) {
        c.fill(LOG, (20, 20, 26));
        let x = self.agent.x();
        if self.evil_log {
            self.draw_evil_log(c);
            return;
        }
        let mut y = c.text("Log (L: Evil only)", (LOG.0 + 8, LOG.1 + 4), HEAD, 2);
        y = self.draw_standing(c, y);
        let tic_of = |s: &str| {
            if s.starts_with("tic ") {
                s.split(':')
                    .next()
                    .and_then(|t| t.split_whitespace().last())
                    .and_then(|t| t.parse::<i64>().ok())
            } else {
                None
            }
        };
        // (a line without a tic of its own (Evil's, the orders') at the last one before it: not
        // sorted to the top, out of sight)
        let mut last = 0;
        let mut merged: Vec<(i64, &String)> = x
            .log
            .iter()
            .rev()
            .take(40)
            .rev()
            .map(|l| {
                last = tic_of(l).unwrap_or(last);
                (last, l)
            })
            .collect();
        merged.extend(
            x.knowledge
                .events
                .iter()
                .rev()
                .take(40)
                .rev()
                .map(|l| (tic_of(l).unwrap_or(0), l)),
        );
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
            let s: String = line
                .split(" | ")
                .next()
                .unwrap_or("")
                .chars()
                .take(width)
                .collect();
            y = c.text(&s, (LOG.0 + 8, y), colour, 1);
        }
    }

    /// The log panel with only what passes between the game and Evil: her actions (>>), what
    /// she's told (<<), the connection; whole, wrapped, the latest at the bottom.
    /// Directed: her standing choices as they are now (her goal, her settings), under the
    /// log's heading from `y`; the row after them.
    pub(super) fn draw_standing(&self, c: &mut Canvas, mut y: usize) -> usize {
        let director = self
            .neuro
            .as_ref()
            .map(|n| &n.director)
            .or(self.feed.as_ref().map(|f| &f.director));
        if let Some(d) = director {
            let (fit, _) = c.fits(1, LOG.2 - 16, LOG.3);
            let standing = [
                format!("goal: {}", d.goal_label().unwrap_or_else(|| "none".into())),
                format!(
                    "policy: {}, instinct {}",
                    d.policy.line(),
                    if d.idle_auto.is_some() { "on" } else { "off" }
                ),
            ];
            for line in standing {
                let chars: Vec<char> = line.chars().collect();
                for (i, ch) in chars.chunks(fit.saturating_sub(3).max(10)).enumerate() {
                    let row = format!(
                        "{}{}",
                        if i > 0 { "   " } else { "" },
                        ch.iter().collect::<String>()
                    );
                    y = c.text(&row, (LOG.0 + 8, y), (255, 220, 120), 1);
                }
            }
            y += 4;
        }
        y
    }

    pub(super) fn draw_evil_log(&self, c: &mut Canvas) {
        let mut y = c.text("Evil (L: whole log)", (LOG.0 + 8, LOG.1 + 4), HEAD, 2);
        y = self.draw_standing(c, y);
        let (fit, rows_fit) = c.fits(1, LOG.2 - 16, LOG.3 - (y - LOG.1));
        let (width, rows_max) = (fit.saturating_sub(3).max(10), rows_fit);
        let mut rows: Vec<(String, (u8, u8, u8))> = Vec::new();
        for line in self
            .agent
            .x()
            .log
            .iter()
            .rev()
            .filter(|l| is_evil_line(l) || l.starts_with("neuro: "))
        {
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
            let wrapped: Vec<String> = chars
                .chunks(width)
                .enumerate()
                .map(|(i, ch)| {
                    format!(
                        "{}{}",
                        if i > 0 { "   " } else { "" },
                        ch.iter().collect::<String>()
                    )
                })
                .collect();
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
    pub(super) fn draw_hud_talk(&self, c: &mut Canvas, bottom: usize) {
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
                let t = t
                    .split_once("s] ")
                    .filter(|(at, _)| at.starts_with('['))
                    .map_or(t, |(_, rest)| rest);
                (
                    t.trim_start_matches("(!) ").to_string(),
                    if notable { (255, 150, 150) } else { TEXT },
                )
            };
            let chars: Vec<char> = text.chars().collect();
            let wrapped: Vec<(String, Rgb)> = chars
                .chunks(width.max(10))
                .map(|ch| (ch.iter().collect(), colour))
                .collect();
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
    pub(super) fn draw_hud(&self, c: &mut Canvas) {
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
                let (x0, x1) = (
                    MAP.0 + x * MAP.2 / mw,
                    MAP.0 + ((x + 1) * MAP.2 / mw).max(x * MAP.2 / mw + 1),
                );
                let (y0, y1) = (
                    MAP.1 + y * MAP.3 / mh,
                    MAP.1 + ((y + 1) * MAP.3 / mh).max(y * MAP.3 / mh + 1),
                );
                (y0..y1)
                    .flat_map(|yy| (x0..x1).map(move |xx| (xx, yy)))
                    .map(|(xx, yy)| c.px[yy * W + xx])
                    .max_by_key(|&p| bright(p))
                    .unwrap_or(0)
            })
            .collect();
        // (and two pixels wide: each the brightest of it and the ones right of and below it)
        let small: Vec<u32> = (0..mh)
            .flat_map(|y| (0..mw).map(move |x| (x, y)))
            .map(|(x, y)| {
                [(x, y), ((x + 1).min(mw - 1), y), (x, (y + 1).min(mh - 1))]
                    .iter()
                    .map(|&(a, b)| small[b * mw + a])
                    .max_by_key(|&p| bright(p))
                    .unwrap_or(0)
            })
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
            let ammo: Vec<String> = [2, 3, 5, 6]
                .iter()
                .map(|&s| {
                    obs.ammo
                        .iter()
                        .flatten()
                        .find(|(x, _)| *x == s)
                        .map_or(0, |(_, n)| *n)
                        .to_string()
                })
                .collect();
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
        // her above it, the latest few, for a recording of an LLM playing)
        let director = self
            .neuro
            .as_ref()
            .map(|n| &n.director)
            .or(self.feed.as_ref().map(|f| &f.director));
        let doing = director.map_or_else(
            || {
                self.agent
                    .x()
                    .current
                    .as_ref()
                    .map_or("-".to_string(), |o| o.label.clone())
            },
            |d| d.doing(&self.agent),
        );
        c.text(
            &format!(
                "{}: {}",
                if director.is_some() { "order" } else { "doing" },
                doing.chars().take(46).collect::<String>()
            ),
            (GAME.0 + 8, low - 53),
            TEXT,
            2,
        );
        if director.is_some() {
            self.draw_hud_talk(c, low - 62);
        }
        let fight = self.agent.player().map_or("", |p| p.combat().mode());
        let fight = if fight.is_empty() { "no fight" } else { fight };
        c.text(
            &format!("fight: {}", fight.chars().take(48).collect::<String>()),
            (GAME.0 + 8, low - 25),
            (255, 200, 120),
            2,
        );
    }

    pub(super) fn draw_map(&self, c: &mut Canvas) {
        self.draw_map_in(c, self.map_view(), true);
    }

    /// The map as she knows it in `v`'s box: lines, what they are, things, sounds, objectives,
    /// the route, her; `labelled`: its title in the corner.
    pub(super) fn draw_map_in(&self, c: &mut Canvas, v: MapView, labelled: bool) {
        c.fill(v.r, (8, 8, 10));
        // (within the box, and within what was clipped already: a box past a column's foot cut
        // there)
        let clip = c.clip;
        let (x0, y0) = (v.r.0.max(clip.0), v.r.1.max(clip.1));
        let x1 = (v.r.0 + v.r.2).min(clip.0 + clip.2);
        let y1 = (v.r.1 + v.r.3).min(clip.1 + clip.3);
        c.clip = (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0));
        let m = &self.m;
        let x = self.agent.x();
        let k = &x.knowledge;
        let heights = self
            .obs
            .as_ref()
            .map(|o| o.heights.clone())
            .unwrap_or_default();
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
            c.line(v.at(a), v.at(b), colour, 1);
        }
        // what lines are, as far as she can tell
        let fired: std::collections::BTreeSet<usize> = k.fired.iter().map(|f| f.line).collect();
        let mut marks: Vec<(usize, Rgb)> = k
            .interactables()
            .into_iter()
            .map(|li| {
                (
                    li,
                    if fired.contains(&li) {
                        (90, 140, 90)
                    } else {
                        kind_colour(Kind::Switch)
                    },
                )
            })
            .collect();
        marks.extend(
            fired
                .iter()
                .filter(|li| k.mapped.contains(li))
                .map(|&li| (li, (90, 140, 90))),
        );
        marks.extend(
            k.exits()
                .into_iter()
                .map(|li| (li, kind_colour(Kind::Exit))),
        );
        marks.extend(k.teleporters().into_iter().map(|li| (li, (255, 255, 255))));
        for (li, colour) in marks {
            let (a, b) = m.line_seg(li);
            c.line(v.at(a), v.at(b), colour, 3);
        }
        // things she knows about
        for th in k.known_things() {
            let Some((name, kind)) = pickup(th.kind) else {
                continue;
            };
            let p = v.at((th.x, th.y));
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
            let col = (
                (f64::from(n.0) * fade) as u8,
                (f64::from(n.1) * fade) as u8,
                (f64::from(n.2) * fade) as u8,
            );
            c.circle(v.at(h.origin), 6.0 + (age / 4) as f64, col, Some(2.0));
        }
        // objectives open to her, the current one, the route
        for o in &self.options {
            let p = v.at(o.goal);
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
                            c.line(v.at(w[0]), v.at(w[1]), (60, 255, 60), 2);
                        }
                        run.clear();
                    }
                }
            }
        }
        if let Some(cur) = &x.current {
            c.circle(v.at(cur.goal), 8.0, kind_colour(cur.kind), Some(2.0));
        }
        if let Some(obs) = &self.obs {
            let p = v.at(obs.pos());
            let a = obs.angle.to_radians();
            let at = |r: f64, off: f64| (p.0 + r * (a + off).cos(), p.1 - r * (a + off).sin());
            c.triangle(
                [at(10.0, 0.0), at(6.0, 2.5), at(6.0, -2.5)],
                (255, 255, 255),
            );
        }
        c.clip = clip;
        if labelled {
            let label = format!(
                "map as known{}{}",
                if self.show_all {
                    "  (+ unknown, faint)"
                } else {
                    ""
                },
                if self.follow { "  [follow]" } else { "" }
            );
            c.text(&label, (v.r.0 + 8, v.r.1 + 4), DIM, 1);
        }
    }

    pub(super) fn draw_panel(&self, c: &mut Canvas) {
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
        if let Some(u) = self.ultra_status() {
            state += &format!("  {u}");
        }
        if k.protected(tic) {
            state += &format!(
                "  suit {:.0}s",
                (k.suit_until.unwrap_or(tic) - tic) as f64 / TICRATE
            );
        }
        let mut y = c.text(
            &format!(
                "{}  {:6.1}s  x{}  {state}",
                self.map_name,
                tic as f64 / TICRATE,
                self.speed
            ),
            (left, PANEL.1 + 6),
            HEAD,
            1,
        );
        let cur = x.current.as_ref();
        y = c.text(
            &format!("Now: {}", cur.map_or("-", |o| o.label.as_str())),
            (left, y + 2),
            cur.map_or(TEXT, |o| kind_colour(o.kind)),
            1,
        );
        if let Some(f) = &x.follower {
            let s: String = f.objective().chars().take(48).collect();
            y = c.text(&format!("     {s}"), (left, y), DIM, 1);
        }
        let m = &self.m;
        y = c.text(
            &format!(
                "seen {}/{} lines, {}/{} sectors; {} fired",
                k.mapped.len(),
                m.linedefs.len(),
                k.sectors.len(),
                m.sectors.len(),
                k.fired.len()
            ),
            (left, y + 2),
            DIM,
            1,
        );
        y = c.text("Objectives open", (left, y + 6), HEAD, 1);
        let pos = self.obs.as_ref().map_or((0.0, 0.0), Observation::pos);
        let mut opts: Vec<&Objective> = self.options.iter().collect();
        opts.sort_by(|a, b| dist(a.goal, pos).total_cmp(&dist(b.goal, pos)));
        for o in opts.into_iter().take(14) {
            let mark = if cur.is_some_and(|c| c.ident() == o.ident()) {
                ">"
            } else {
                " "
            };
            let label: String = o.label.chars().take(26).collect();
            y = c.text(
                &format!(
                    "{mark} {:8} {label:26} {:5.0}",
                    kind_name(o.kind),
                    dist(o.goal, pos)
                ),
                (left, y),
                kind_colour(o.kind),
                1,
            );
        }
        // what she has found
        let mut y = c.text("Found", (right, PANEL.1 + 4), HEAD, 2);
        let held = self.obs.as_ref().and_then(|o| o.keys).unwrap_or_default();
        let mut keys: Vec<String> = Vec::new();
        let mut items: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for th in k.known_things() {
            let Some((name, kind)) = pickup(th.kind) else {
                continue;
            };
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
        y = c.text(
            &format!(
                "Keys: {}",
                if keys.is_empty() {
                    "none seen".to_string()
                } else {
                    keys.join(", ")
                }
            ),
            (right, y + 2),
            (255, 255, 0),
            1,
        );
        for li in k.exits() {
            y = c.text(
                &format!("Exit: line {li}"),
                (right, y),
                kind_colour(Kind::Exit),
                1,
            );
        }
        for li in k.teleporters() {
            let to = k.teleports.get(&li).map_or(" (not used)".to_string(), |w| {
                format!(" -> ({:.0}, {:.0})", w.0, w.1)
            });
            y = c.text(
                &format!("Teleporter: line {li}{to}"),
                (right, y),
                (255, 255, 255),
                1,
            );
        }
        let untried = k.interactables();
        let locked: std::collections::BTreeSet<&str> = untried
            .iter()
            .map(|&li| k.lock(li))
            .filter(|l| *l != Lock::None)
            .map(Lock::value)
            .collect();
        let n_locked = untried
            .iter()
            .filter(|&&li| k.lock(li) != Lock::None)
            .count();
        let extra = if n_locked > 0 {
            format!(
                " ({n_locked} locked: {})",
                locked.into_iter().collect::<Vec<_>>().join(", ")
            )
        } else {
            String::new()
        };
        y = c.text(
            &format!("Untried switches/doors: {}{extra}", untried.len()),
            (right, y),
            kind_colour(Kind::Switch),
            1,
        );
        for h in k
            .heard
            .iter()
            .filter(|h| !h.seen)
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            y = c.text(
                &format!(
                    "Heard ({:.0}s ago): something moving, bearing {:.0}, ~{:.0} away",
                    (tic - h.tic) as f64 / TICRATE,
                    h.bearing,
                    h.distance
                ),
                (right, y),
                kind_colour(Kind::Noise),
                1,
            );
        }
        let learned = k.known_triggers();
        y = c.text(
            &format!("Learned: {} lines", learned.len()),
            (right, y),
            (140, 220, 255),
            1,
        );
        for (t, secs) in learned
            .iter()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let d: String = t.info.description.chars().take(28).collect();
            let s: String = format!(
                "  line {}: {d} -> sectors {:?}",
                t.line,
                secs.iter().collect::<Vec<_>>()
            )
            .chars()
            .take(60)
            .collect();
            y = c.text(&s, (right, y), (140, 220, 255), 1);
        }
        for cat in ["weapon", "health", "armor", "ammo", "powerup"] {
            let Some(names) = items.get(cat) else {
                continue;
            };
            let mut uniq: Vec<&str> = names.clone();
            uniq.sort_unstable();
            uniq.dedup();
            let summary: Vec<String> = uniq
                .iter()
                .map(|n| {
                    let k = names.iter().filter(|x| *x == n).count();
                    if k > 1 {
                        format!("{n} x{k}")
                    } else {
                        (*n).to_string()
                    }
                })
                .collect();
            let colour = match cat {
                "weapon" => item_colour(PickupKind::Weapon),
                "health" => item_colour(PickupKind::Health),
                "armor" => item_colour(PickupKind::Armor),
                "ammo" => item_colour(PickupKind::Ammo),
                _ => item_colour(PickupKind::Powerup),
            };
            let mut title = cat.to_string();
            title[..1].make_ascii_uppercase();
            let s: String = format!("{title}: {}", summary.join(", "))
                .chars()
                .take(62)
                .collect();
            y = c.text(&s, (right, y), colour, 1);
        }
    }
}
