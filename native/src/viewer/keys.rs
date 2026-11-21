// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Her hands over the game's picture (`--show-keys`, or K): each key lit while it's down, with
//! the last few seconds of it beside it, and how she turned. For seeing what `--keys` presses
//! (`motor::keys`), a tap too quick to catch by the light alone.

use super::*;

/// The keys shown, top to bottom, and each one's bit in a tic of `Watch::pressed`.
const ROWS: [(&str, u8); 7] = [
    ("FWD", FWD),
    ("BACK", BACK),
    ("LEFT", LEFT),
    ("RIGHT", RIGHT),
    ("RUN", RUN),
    ("FIRE", FIRE),
    ("USE", USE),
];
const FWD: u8 = 1;
const BACK: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const RUN: u8 = 16;
const FIRE: u8 = 32;
const USE: u8 = 64;

/// The tics of presses kept and shown: three seconds.
pub(super) const KEYS_SHOWN: usize = 105;
/// The turn in a tic that fills the turn row's half height, in degrees.
const TURN_FULL: f64 = 8.0;

const DOWN: Rgb = (255, 220, 90);
const UP: Rgb = (70, 70, 80);

impl Watch {
    /// This tic's presses kept for showing: the controls the game was given, and with `--keys`
    /// the keys as her hand holds them (run too, which pushes nothing by itself).
    pub(super) fn keys_note(&mut self, c: &Controls) {
        let (forward, side, run) = match self.agent.x().keys.as_ref() {
            Some(k) => k.held(),
            None => (
                c.forward.partial_cmp(&0.0).map_or(0, |o| o as i8),
                c.strafe.partial_cmp(&0.0).map_or(0, |o| o as i8),
                c.run,
            ),
        };
        let mut bits = 0;
        for (on, bit) in [
            (forward > 0, FWD),
            (forward < 0, BACK),
            (side < 0, LEFT),
            (side > 0, RIGHT),
            (run, RUN),
            (c.attack, FIRE),
            (c.use_, USE),
        ] {
            if on {
                bits |= bit;
            }
        }
        if self.pressed.len() >= KEYS_SHOWN {
            self.pressed.pop_front();
        }
        self.pressed.push_back((bits, c.turn));
    }

    /// Her keys over the game drawn in `r`: bottom left, above the status bar.
    pub(super) fn draw_keys(&self, c: &mut Canvas, r: Rect) {
        if !self.show_keys {
            return;
        }
        // (sized to the picture: a tic two pixels wide in the watcher's 800 by 600)
        let unit = (r.3 / 300).max(1);
        let (g, row) = (c.glyph(1), c.glyph(1) + 2 * unit);
        let (label, strip) = (6 * g, KEYS_SHOWN * unit);
        let (w, h) = (label + strip + 3 * unit, (ROWS.len() + 2) * row + 2 * unit);
        // (the status bar is the picture's bottom 32 lines of 200)
        let (x0, y0) = (r.0 + 4 * unit, r.1 + r.3 * 168 / 200 - h - 4 * unit);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                c.blend(x as i64, y as i64, (0, 0, 0), 150);
            }
        }
        let now = self.pressed.back().copied().unwrap_or((0, 0.0));
        let sx = x0 + unit + label;
        // (the newest tic at the right, whatever's been kept so far)
        let first = sx + (KEYS_SHOWN - self.pressed.len()) * unit;
        for (i, &(name, bit)) in ROWS.iter().enumerate() {
            let y = y0 + unit + i * row;
            let lit = if now.0 & bit != 0 { DOWN } else { DIM };
            c.text(name, (x0 + unit, y + unit), lit, 1);
            c.fill((sx, y + row / 2, strip, 1), UP);
            for (t, &(bits, _)) in self.pressed.iter().enumerate() {
                if bits & bit != 0 {
                    c.fill((first + t * unit, y + unit, unit, row - 2 * unit), DOWN);
                }
            }
        }
        // (the mouse: each tic's turn as a bar from the middle, up for left, down for right)
        let (y, tall) = (y0 + unit + ROWS.len() * row, 2 * row);
        let mid = y + tall / 2;
        c.text("TURN", (x0 + unit, mid - g / 2), TEXT, 1);
        c.fill((sx, mid, strip, 1), UP);
        for (t, &(_, turn)) in self.pressed.iter().enumerate() {
            let len =
                ((turn.abs() / TURN_FULL).min(1.0) * (tall / 2 - unit) as f64).round() as usize;
            if len > 0 {
                let top = if turn < 0.0 { mid - len } else { mid + 1 };
                c.fill((first + t * unit, top, unit, len), SAID);
            }
        }
    }
}
