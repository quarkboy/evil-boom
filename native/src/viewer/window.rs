// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The watcher's window: the game's own SDL (evil_window_*), a picture shown at the screen's
//! refresh where there's vsync, scaled on the graphics card to whatever size the window's given.

use std::time::{Duration, Instant};

use crate::dsda::{Dsda, WindowEvent, WindowSpec};

/// The keys the window answers to, as SDL's keycodes have them.
pub(super) mod key {
    pub const ESCAPE: i32 = 27;
    pub const SPACE: i32 = 32;
    pub const MINUS: i32 = 45;
    pub const EQUALS: i32 = 61;
    pub const A: i32 = 97;
    pub const F: i32 = 102;
    pub const L: i32 = 108;
    pub const M: i32 = 109;
    pub const N: i32 = 110;
    pub const T: i32 = 116;
    pub const RIGHT: i32 = 0x4000_004F;
    pub const KP_MINUS: i32 = 0x4000_0056;
    pub const KP_PLUS: i32 = 0x4000_0057;
}

pub(super) struct Window {
    /// the screen's refresh
    period: Duration,
    vsync: bool,
    /// when the last frame was shown (by the clock: when it was due)
    last: Instant,
}

impl Window {
    /// Opened for a picture `w` by `h`.
    pub(super) fn open(
        game: &Dsda,
        title: &str,
        (w, h): (usize, usize),
        spec: WindowSpec,
    ) -> Result<Window, String> {
        let info = game.window_open(title, w, h, spec)?;
        let hz = if info.refresh > 0 { info.refresh } else { 60 };
        Ok(Window {
            period: Duration::from_secs_f64(1.0 / f64::from(hz)),
            vsync: info.vsync,
            last: Instant::now(),
        })
    }

    /// Whether it waits for the refresh, and the refresh.
    pub(super) fn describe(&self) -> String {
        format!(
            "{} at {:.0} Hz",
            if self.vsync {
                "vsync"
            } else {
                "no vsync (paced by the clock)"
            },
            1.0 / self.period.as_secs_f64()
        )
    }

    /// The picture shown. With vsync that waits for the refresh; without (or where it didn't
    /// wait: some drivers, a window minimised) a frame's kept to the refresh by the clock.
    pub(super) fn show(&mut self, game: &Dsda, px: &[u32], w: usize, h: usize) {
        game.window_show(px, w, h);
        // (by the clock: each frame due a refresh after the last was due, not after it was
        // shown, so that a sleep's running over doesn't add up into a slower rate; behind by
        // more than a frame, due from now)
        let now = Instant::now();
        let paced = !self.vsync || now - self.last < self.period / 2;
        self.last = if paced {
            let due = self.last + self.period;
            if due > now {
                std::thread::sleep(due - now);
                due
            } else {
                now
            }
        } else {
            now
        };
    }

    pub(super) fn events(game: &Dsda) -> Vec<WindowEvent> {
        game.window_events()
    }

    pub(super) fn set_title(game: &Dsda, title: &str) {
        game.window_title(title);
    }
}
