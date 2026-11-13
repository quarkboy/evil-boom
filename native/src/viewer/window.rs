// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The watcher's window: the game's own SDL (evil_window_*), a picture shown at the screen's
//! refresh where there's vsync, scaled on the graphics card to whatever size the window's given.

use std::time::{Duration, Instant};

use crate::dsda::{Dsda, WindowEvent};

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
    last: Instant,
}

impl Window {
    pub(super) fn open(game: &Dsda, title: &str, w: usize, h: usize) -> Result<Window, String> {
        let info = game.window_open(title, w, h)?;
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
        let soonest = if self.vsync {
            self.period / 2
        } else {
            self.period
        };
        let since = self.last.elapsed();
        if since < soonest {
            std::thread::sleep(self.period - since);
        }
        self.last = Instant::now();
    }

    pub(super) fn events(game: &Dsda) -> Vec<WindowEvent> {
        game.window_events()
    }

    pub(super) fn set_title(game: &Dsda, title: &str) {
        game.window_title(title);
    }
}
