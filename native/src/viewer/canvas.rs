// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The watcher's picture: a canvas of pixels with a clip, the lines, circles, triangles and
//! text drawn on it, where a map's point falls on it, and the colours things are drawn in.

use font8x8::legacy::BASIC_LEGACY;

use super::Rect;
use crate::geom::Point;
use crate::knowledge::explorer::Kind;
use crate::things::PickupKind;

pub(super) type Rgb = (u8, u8, u8);
pub(super) const BG: Rgb = (12, 12, 16);
pub(super) const DIM: Rgb = (120, 120, 130);
pub(super) const TEXT: Rgb = (220, 220, 220);
pub(super) const HEAD: Rgb = (255, 210, 120);
/// what Evil said (the GPT stand-in's words)
pub(super) const SAID: Rgb = (200, 160, 255);

pub(super) fn kind_colour(k: Kind) -> Rgb {
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

pub(super) fn kind_name(k: Kind) -> &'static str {
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

pub(super) fn item_colour(k: PickupKind) -> Rgb {
    match k {
        PickupKind::Key => (255, 255, 0),
        PickupKind::Weapon => (255, 140, 0),
        PickupKind::Ammo => (180, 140, 90),
        PickupKind::Health => (80, 220, 80),
        PickupKind::Armor => (80, 160, 255),
        PickupKind::Powerup => (220, 120, 255),
    }
}

pub(super) fn key_colour(name: &str) -> Rgb {
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
pub(super) struct Canvas {
    pub(super) px: Vec<u32>,
    /// its width in pixels (the watcher's W; the stream's layout its own)
    pub(super) width: usize,
    pub(super) clip: Rect,
    /// all text this much bigger (T cycles TEXT_SIZES)
    pub(super) text_scale: f64,
}

/// Where a map's drawn: its box on the canvas, the map's point at the box's middle, and pixels
/// a map unit.
#[derive(Clone, Copy, Debug)]
pub(super) struct MapView {
    pub(super) r: Rect,
    pub(super) centre: Point,
    pub(super) scale: f64,
}

impl MapView {
    pub(super) fn at(&self, p: Point) -> Point {
        (
            self.r.0 as f64 + self.r.2 as f64 / 2.0 + (p.0 - self.centre.0) * self.scale,
            self.r.1 as f64 + self.r.3 as f64 / 2.0 - (p.1 - self.centre.1) * self.scale,
        )
    }
}

/// the text sizes T cycles through
pub(super) const TEXT_SIZES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

impl Canvas {
    /// The pixel at (x, y), if it's inside the clip and the canvas (a clip that runs past the
    /// canvas is cut to it).
    pub(super) fn at(&self, x: i64, y: i64) -> Option<usize> {
        let (cx, cy, cw, ch) = self.clip;
        let height = self.px.len() / self.width.max(1);
        let inside = x >= cx as i64
            && y >= cy as i64
            && x < ((cx + cw).min(self.width)) as i64
            && y < ((cy + ch).min(height)) as i64;
        inside.then(|| y as usize * self.width + x as usize)
    }

    pub(super) fn put(&mut self, x: i64, y: i64, c: Rgb) {
        if let Some(i) = self.at(x, y) {
            self.px[i] = (u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2);
        }
    }

    /// `c` over what's there by `a` (0-255), within the clip.
    pub(super) fn blend(&mut self, x: i64, y: i64, c: Rgb, a: u8) {
        let Some(i) = self.at(x, y).filter(|_| a > 0) else {
            return;
        };
        let was = self.px[i];
        let mix = |new: u8, shift: u32| {
            let old = (was >> shift) & 0xff;
            (u32::from(new) * u32::from(a) + old * (255 - u32::from(a))) / 255
        };
        self.px[i] = (mix(c.0, 16) << 16) | (mix(c.1, 8) << 8) | mix(c.2, 0);
    }

    pub(super) fn fill(&mut self, r: Rect, c: Rgb) {
        for y in r.1..r.1 + r.3 {
            for x in r.0..r.0 + r.2 {
                self.put(x as i64, y as i64, c);
            }
        }
    }

    pub(super) fn line(&mut self, a: Point, b: Point, c: Rgb, width: i64) {
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

    pub(super) fn circle(&mut self, at: Point, r: f64, c: Rgb, width: Option<f64>) {
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

    pub(super) fn triangle(&mut self, p: [Point; 3], c: Rgb) {
        let (x0, x1) = (
            p.iter().map(|q| q.0).fold(f64::INFINITY, f64::min),
            p.iter().map(|q| q.0).fold(f64::NEG_INFINITY, f64::max),
        );
        let (y0, y1) = (
            p.iter().map(|q| q.1).fold(f64::INFINITY, f64::min),
            p.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max),
        );
        let side =
            |a: Point, b: Point, q: Point| (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
        for y in y0.floor() as i64..=y1.ceil() as i64 {
            for x in x0.floor() as i64..=x1.ceil() as i64 {
                let q = (x as f64, y as f64);
                let s = [
                    side(p[0], p[1], q),
                    side(p[1], p[2], q),
                    side(p[2], p[0], q),
                ];
                if s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0) {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Text in the 8x8 font at `scale`; returns the next line's y.
    pub(super) fn text(&mut self, s: &str, at: (usize, usize), c: Rgb, scale: usize) -> usize {
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
    pub(super) fn glyph(&self, scale: usize) -> usize {
        (8.0 * scale as f64 * self.text_scale).round() as usize
    }

    /// How many characters, and lines, of `scale` text fit `width` by `height`.
    pub(super) fn fits(&self, scale: usize, width: usize, height: usize) -> (usize, usize) {
        let g = self.glyph(scale);
        (
            width / g.max(1),
            height / (g + (3.0 * self.text_scale).round() as usize).max(1),
        )
    }
}
