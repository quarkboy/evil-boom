// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A person playing in a co-op or deathmatch game with the bots (`watch --players N --human`):
//! the last player's controls from the window's keyboard and mouse, as Doom's own are, read
//! each frame and given the game a tic at a time.
//!
//! Keys: W/S or Up/Down forward and back, A/D strafe, Left/Right turn (strafe with Alt), the
//! mouse turns; Ctrl or the left button fires, Space, E or the right button uses (and, dead,
//! brings you back), 1-7 and the wheel the weapons. You run; Shift held, you walk. Esc lets the
//! mouse go (a click takes it again; Esc again, the game's over).

use crate::domain::{Controls, Observation};
use crate::dsda::{Dsda, Input};

/// SDL's scancodes for the keys used
mod scan {
    pub const A: usize = 4;
    pub const D: usize = 7;
    pub const E: usize = 8;
    pub const S: usize = 22;
    pub const W: usize = 26;
    /// 1 to 7 (8, 9, 0 after)
    pub const N1: usize = 30;
    pub const SPACE: usize = 44;
    pub const RIGHT: usize = 79;
    pub const LEFT: usize = 80;
    pub const DOWN: usize = 81;
    pub const UP: usize = 82;
    pub const LCTRL: usize = 224;
    pub const LSHIFT: usize = 225;
    pub const LALT: usize = 226;
    pub const RCTRL: usize = 228;
    pub const RSHIFT: usize = 229;
    pub const RALT: usize = 230;
}

/// as Doom moves a running player each tic, forward and sideways (walking: half, `Controls::run`)
const FORWARD: f64 = 50.0;
const SIDE: f64 = 40.0;
/// a turn key's turn a tic (degrees): Doom's angleturn, 1280 running, 640 walking, 320 for the
/// first tics held (to aim a little)
const TURN_RUN: f64 = 1280.0 * 360.0 / 65536.0;
const TURN_WALK: f64 = 640.0 * 360.0 / 65536.0;
const TURN_SLOW: f64 = 320.0 * 360.0 / 65536.0;
const SLOW_TICS: i32 = 6;

/// The guns in the order the wheel goes through them (the game's numbers: the fist, the
/// chainsaw, the pistol, the shotgun, the super shotgun, the chaingun, the rocket launcher, the
/// plasma rifle, the BFG), and each one's slot.
const WHEEL: [(i32, i32); 9] = [
    (0, 1),
    (7, 1),
    (1, 2),
    (2, 3),
    (8, 3),
    (3, 4),
    (4, 5),
    (5, 6),
    (6, 7),
];

/// What's come of the window this frame, for the game's loop.
pub(super) enum Asked {
    Nothing,
    /// Esc with the mouse let go: the game's over
    Quit,
}

pub(super) struct Human {
    /// the mouse kept in the window
    grabbed: bool,
    /// degrees a mouse pixel turns
    sens: f64,
    /// the mouse's turn owed the next tic (degrees)
    turn: f64,
    /// held since the last tic (a click between two tics still a shot): fire, use
    attack: bool,
    use_: bool,
    /// a weapon asked for since the last tic: (slot, the gun)
    weapon: Option<(i32, Option<i32>)>,
    /// the wheel's turns owed (up positive: the next gun)
    wheel: i32,
    /// tics a turn key's been held
    turn_held: i32,
    /// as last read
    input: Input,
    /// a button held last frame (a click, once)
    clicked_was: bool,
}

impl Human {
    pub(super) fn new(sens: f64) -> Human {
        Human {
            grabbed: false,
            sens,
            turn: 0.0,
            attack: false,
            use_: false,
            weapon: None,
            wheel: 0,
            turn_held: 0,
            input: Input::default(),
            clicked_was: false,
        }
    }

    /// The wheel turned (the window's event: up positive).
    pub(super) fn wheel(&mut self, by: i32) {
        self.wheel += by;
    }

    /// The window's keyboard and mouse read (each frame; `esc`: Esc pressed since the last):
    /// the mouse's turn and what's pressed kept for the next tic.
    pub(super) fn frame(&mut self, game: &Dsda, esc: bool) -> Asked {
        let Some(input) = game.window_input(Some(self.grabbed)) else {
            return Asked::Nothing;
        };
        let clicked = input.buttons != 0;
        // (Esc lets the mouse go; let go, Esc again ends it; a click in the window takes it)
        if esc {
            if !self.grabbed {
                return Asked::Quit;
            }
            self.grabbed = false;
        } else if clicked && !self.clicked_was && !self.grabbed && input.focused {
            self.grabbed = true;
            self.clicked_was = true;
            self.input = input;
            return Asked::Nothing; // (the click that took it isn't a shot)
        }
        self.clicked_was = clicked;
        if !input.focused {
            self.grabbed = false;
        }
        if self.grabbed {
            self.turn += f64::from(input.mouse.0) * self.sens;
            self.attack |= input.buttons & 1 != 0;
            self.use_ |= input.buttons & 4 != 0;
        }
        self.attack |= input.held(scan::LCTRL) || input.held(scan::RCTRL);
        self.use_ |= input.held(scan::SPACE) || input.held(scan::E);
        for k in 0..7 {
            if input.held(scan::N1 + k) {
                self.weapon = Some((k as i32 + 1, None));
            }
        }
        self.input = input;
        Asked::Nothing
    }

    /// Whether the mouse is kept in the window.
    pub(super) fn grabbed(&self) -> bool {
        self.grabbed
    }

    /// The tic's controls, from what's held now and what's been pressed since the last
    /// (`obs`: her last look, for the wheel's next gun).
    pub(super) fn controls(&mut self, obs: Option<&Observation>) -> Controls {
        let i = &self.input;
        let walk = i.held(scan::LSHIFT) || i.held(scan::RSHIFT);
        let alt = i.held(scan::LALT) || i.held(scan::RALT);
        let axis = |plus: bool, minus: bool| f64::from(i8::from(plus) - i8::from(minus));
        let forward = axis(
            i.held(scan::W) || i.held(scan::UP),
            i.held(scan::S) || i.held(scan::DOWN),
        );
        let (left, right) = (i.held(scan::LEFT), i.held(scan::RIGHT));
        let mut strafe = axis(i.held(scan::D), i.held(scan::A));
        let mut turn_keys = 0.0;
        if alt {
            strafe += axis(right, left);
        } else {
            turn_keys = axis(right, left);
        }
        self.turn_held = if turn_keys == 0.0 {
            0
        } else {
            self.turn_held + 1
        };
        let key_turn = if self.turn_held <= SLOW_TICS {
            TURN_SLOW
        } else if walk {
            TURN_WALK
        } else {
            TURN_RUN
        };
        let mut c = Controls {
            forward: forward * FORWARD,
            strafe: strafe.clamp(-1.0, 1.0) * SIDE,
            turn: turn_keys * key_turn + std::mem::take(&mut self.turn),
            attack: std::mem::take(&mut self.attack),
            use_: std::mem::take(&mut self.use_),
            run: !walk,
            weapon: 0,
            exact: None,
        };
        if self.wheel != 0
            && let Some(o) = obs
        {
            self.weapon = next_gun(o, self.wheel.signum()).or(self.weapon);
        }
        self.wheel = 0;
        if let Some((slot, exact)) = self.weapon.take() {
            c.weapon = slot;
            c.exact = exact;
        }
        c
    }
}

/// The gun after (or before, `step` -1) the one up, of those she has, as the wheel goes.
fn next_gun(o: &Observation, step: i32) -> Option<(i32, Option<i32>)> {
    let owned = o.owned.as_ref()?;
    let have: Vec<(i32, i32)> = WHEEL
        .iter()
        .copied()
        .filter(|(g, _)| owned.contains(g))
        .collect();
    if have.is_empty() {
        return None;
    }
    let up = o.ready.and_then(|r| have.iter().position(|(g, _)| *g == r));
    let n = have.len() as i32;
    let at = match up {
        Some(p) => (p as i32 + step).rem_euclid(n),
        None => 0,
    } as usize;
    let (gun, slot) = have[at];
    Some((slot, Some(gun)))
}
