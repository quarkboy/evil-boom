// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The ultra instinct in the watcher (`crate::ultra`): hers by a key or by dying, what she has of
//! it and how it's spent and got back, its glow, a death undone.

use super::*;

impl Watch {
    pub(super) fn ultra_log(&mut self, what: String) {
        let tic = self.engine.tic;
        self.agent
            .x_mut()
            .log
            .push(format!("tic {tic}: ultra instinct: {what}"));
    }

    /// Its workers up, if any are asked for and none are (or they're another level's).
    pub(super) fn ultra_pool_up(&mut self) {
        if self.ultra_workers == 0
            || self
                .ultra_pool
                .as_ref()
                .is_some_and(|p| p.map == self.map_name && !p.is_empty())
        {
            return;
        }
        self.ultra_pool = None; // (another level's: let go first)
        match crate::ultra::Pool::spawn(
            &self.engine.iwad,
            &self.map_name,
            self.engine.skill(),
            &crate::ultra::Tune::default(),
            self.ultra_workers,
        ) {
            Ok(p) => self.ultra_pool = Some(p),
            Err(e) => self.ultra_log(e),
        }
    }

    /// The ultra instinct showing: an angry red glow in from the edges of the game's picture
    /// while it's on, throbbing, and stronger while its search has her.
    pub(super) fn ultra_glow(&self, c: &mut Canvas, r: Rect) {
        let Some(u) = &self.ultra else { return };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64() % 1000.0);
        let throb = (now * std::f64::consts::TAU * 1.1).sin();
        // (eased from one to the other over half a second: the search has her and hasn't from
        // tic to tic as one of them comes into sight and goes, and the glow flickered with it)
        let want = if u.searching { 1.0 } else { 0.0 };
        let (mut heat, last) = self.ultra_heat.get();
        let dt = (now - last).rem_euclid(1000.0).min(0.1);
        heat += (want - heat).clamp(-2.0 * dt, 2.0 * dt);
        self.ultra_heat.set((heat, now));
        let strength = (0.20 + 0.05 * throb) + heat * (0.24 + 0.07 * throb);
        // (how red each pixel of the bands is, by how near the edge: worked out once for a
        // picture's box, then a frame is an integer blend of those pixels only)
        let mut mask = self.ultra_mask.borrow_mut();
        if mask.0 != (r, c.width) {
            let m = (r.2.min(r.3) / 6).max(1);
            let fall = |d: usize| {
                if d < m {
                    1.0 - d as f64 / m as f64
                } else {
                    0.0
                }
            };
            let mut at = Vec::new();
            for y in 0..r.3 {
                let ey = fall(y.min(r.3 - 1 - y));
                for x in 0..r.2 {
                    let t = fall(x.min(r.2 - 1 - x)).hypot(ey).min(1.0);
                    let a = (t * t * 255.0) as u8;
                    // (too faint to see left alone: a third of the band, and of the work)
                    if a >= 10 {
                        at.push((((r.1 + y) * c.width + r.0 + x) as u32, a));
                    }
                }
            }
            *mask = ((r, c.width), at);
        }
        let s = (strength * 256.0) as u32;
        for &(i, a) in &mask.1 {
            let Some(p) = c.px.get_mut(i as usize) else {
                continue;
            };
            // (red and blue together in one multiply, green in another)
            let a = (u32::from(a) * s) >> 8;
            let rb = ((*p & 0x00ff_00ff) * (256 - a) + 0x00ff_0006 * a) >> 8;
            let g = ((*p & 0x0000_ff00) * (256 - a) + 0x0000_0e00 * a) >> 8;
            *p = (rb & 0x00ff_00ff) | (g & 0x0000_ff00);
        }
    }

    /// Is it hers on this level (she's died here often enough)?
    pub(super) fn ultra_unlocked(&self) -> bool {
        self.ultra_after.is_some_and(|n| self.ultra_deaths >= n)
    }

    /// What she has of it, for the watcher's status line.
    pub(super) fn ultra_status(&self) -> Option<String> {
        let after = self.ultra_after?;
        let secs = (self.ultra_charge / TICRATE).floor();
        Some(if !self.ultra_unlocked() {
            let more = after - self.ultra_deaths;
            format!(
                "ultra after {more} more death{}",
                if more == 1 { "" } else { "s" }
            )
        } else if self.ultra.is_some() {
            format!("ULTRA {secs:.0}s")
        } else if self.ultra_charge < crate::ultra::LIVE_LEAST {
            format!("ultra {secs:.0}s (needs 5)")
        } else {
            format!("ultra {secs:.0}s")
        })
    }

    /// A death that stands (the level's about to begin again, or the game's over): counted,
    /// and the ultra instinct hers from the one that makes enough. What it had going is let go.
    pub(super) fn ultra_real_death(&mut self) {
        self.ultra_end("she died");
        self.ultra_rolling.clear(&self.engine);
        let Some(after) = self.ultra_after else {
            return;
        };
        let had = self.ultra_cap();
        self.ultra_deaths += 1;
        // (and what she has of it full again, for the level's next go)
        self.ultra_charge = self.ultra_cap();
        if self.ultra_cap() > had {
            let secs = self.ultra_cap() / TICRATE;
            let text = if had == 0.0 {
                self.ultra_log(format!("hers on this level: {after} deaths here"));
                "Ultra Instinct unlocked.".to_string()
            } else {
                self.ultra_log(format!(
                    "{secs:.0}s of it now: {} deaths here",
                    self.ultra_deaths
                ));
                format!("Ultra Instinct: {secs:.0}s now.")
            };
            let tic = self.engine.tic + self.clock_ahead;
            self.tell_screens(|d| d.ultra(tic, &text, crate::control::directed::Event::Ultra));
        }
    }

    /// The most she can have of it, in tics: `ultra::LIVE_CAP` for every `--ultra-after` deaths
    /// on the level, up to `ultra::LIVE_TIERS` times (none before the first of them).
    pub(super) fn ultra_cap(&self) -> f64 {
        // (`--ultra-after 0`: hers from the level's first tic, the one measure of it and no
        // more for dying)
        match self.ultra_after {
            None => 0.0,
            Some(0) => crate::ultra::LIVE_CAP,
            Some(n) => {
                crate::ultra::LIVE_CAP
                    * f64::from((self.ultra_deaths / n).min(crate::ultra::LIVE_TIERS))
            }
        }
    }

    /// (`U`) The ultra instinct on from here, or dropped.
    pub(super) fn ultra_toggle(&mut self) {
        if self.ultra.is_some() {
            self.ultra_end("dropped");
        } else if !self.engine.finished() {
            self.ultra_earned = false;
            self.ultra_begin();
        }
    }

    /// A session begun from the game as it stands.
    pub(super) fn ultra_begin(&mut self) {
        {
            let mut session = crate::ultra::Session::new(
                crate::ultra::Tune::default(),
                &self.agent,
                self.engine.tic,
            );
            self.ultra_pool_up();
            session.ultra.pool = self.ultra_pool.take();
            session.ultra.block = self.ultra_block;
            let with = session
                .ultra
                .pool
                .as_ref()
                .map_or("no workers".to_string(), |p| format!("{} workers", p.len()));
            self.ultra = Some(session);
            self.ultra_log(format!("on ({with})"));
        }
    }

    pub(super) fn ultra_end(&mut self, why: &str) {
        let Some(mut u) = self.ultra.take() else {
            return;
        };
        self.ultra_pool = u.end(&self.engine);
        self.clock_ahead += u.gone_back;
        if std::mem::take(&mut self.ultra_earned) {
            let tic = self.engine.tic + self.clock_ahead;
            let text = "Ultra Instinct off.".to_string();
            self.tell_screens(|d| d.ultra(tic, &text, crate::control::directed::Event::UltraOver));
        }
        let t = &u.ultra.tally;
        let mean = 1000.0 * t.searched_secs / (t.decisions.max(1) as f64);
        self.ultra_log(format!(
            "off ({why}): {} tics searched in {} looks, {mean:.0} ms a look, the longest {:.0} ms; back {} times for a death and {} for a hurt ({} tics)",
            t.ultra_tics,
            t.decisions,
            1000.0 * t.worst_look,
            t.back,
            t.hurt_back,
            u.gone_back
        ));
    }

    /// Dead with the ultra instinct on: back to a game it kept, or it's over.
    pub(super) fn ultra_died(&mut self) {
        let Some(u) = self.ultra.as_mut() else {
            // (not on, and hers by dying with enough of it left: the death undone, and on)
            if !self.ultra_unlocked() || self.ultra_charge < crate::ultra::LIVE_LEAST {
                return;
            }
            let Some(back) = self.ultra_rolling.back(&mut self.engine, self.ultra_back) else {
                return;
            };
            self.clock_ahead += back;
            self.controls = None;
            self.frame = None;
            self.ultra_wait = None;
            self.ultra_earned = true;
            self.ultra_begin();
            let secs = back as f64 / TICRATE;
            self.ultra_log(format!("a death undone: back {secs:.1}s"));
            let tic = self.engine.tic + self.clock_ahead;
            let text = format!("You died. Ultra Instinct on: back {secs:.0}s.");
            self.tell_screens(|d| d.ultra(tic, &text, crate::control::directed::Event::Ultra));
            return;
        };
        match u.died(&mut self.engine) {
            Some(back) => {
                self.controls = None;
                self.frame = None;
                self.ultra_log(format!("back {:.1}s: she died", back as f64 / TICRATE));
            }
            None => self.ultra_end("she died, and no game kept to go back to"),
        }
    }
}
