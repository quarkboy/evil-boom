// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Her movement as keys (`--keys`): what the game takes from a keyboard, and nothing else.
//!
//! The rest of her asks for a push of any size in any direction (`Controls::forward` and
//! `strafe`, each anything from -50 to 50). A keyboard has no such thing. It has forward and
//! back, left and right, and run: five keys, each down or up, which the game turns into a
//! forward push of 25 or 50 and a sideways one of 24 or 40 (`forwardmove` and `sidemove` in
//! `g_game.c`), run deciding which of the pair for both at once. So eight ways to go from
//! where she faces, at two paces, and standing.
//!
//! Each tic this picks the keys nearest what was asked. What that leaves over is carried to
//! the next tic and asked for again, so a push no key gives (a slow step, a way between two
//! of the eight) comes out as taps: the keys nearest it, pressed for the share of the time
//! that makes it up. And a hand's limits are kept: a key just pressed or just let go stays so
//! for a few tics (`Hand::hold`, and longer for run, `Hand::run_hold`), which is as fast as
//! fingers tap.
//!
//! Her turning isn't touched: that's the mouse, and already kept to what a hand does where
//! it's decided. Nor are fire, use and the weapon keys, which are keys already.
//!
//! What comes out is a tic command anyone at a keyboard and a mouse could have made, which is
//! what a demo is a list of.

use crate::domain::Controls;
use crate::tactical::tuning::Hand;

/// The game's pushes for the keys: forward or back at a walk and a run, sideways the same
/// (`forwardmove[2]` and `sidemove[2]`, g_game.c).
const FORWARD: [f64; 2] = [25.0, 50.0];
const SIDE: [f64; 2] = [24.0, 40.0];

/// The keys as they're held, and since when.
#[derive(Clone, Debug, Default)]
pub struct Keys {
    /// forward 1, back -1, neither 0; right 1, left -1, neither 0; run down
    forward: i8,
    side: i8,
    run: bool,
    /// the tic each last changed (forward and back are one hand's choice, as left and right)
    forward_at: i64,
    side_at: i64,
    run_at: i64,
    /// what was asked for and no key gave, carried on (forward, right)
    carry: (f64, f64),
    /// presses and releases so far, for measuring by
    pub changes: u64,
    pub tics: u64,
    /// how fast her hand is
    hand: Hand,
}

impl Keys {
    pub fn new(hand: Hand) -> Keys {
        Keys {
            hand,
            run: true, // (as anyone plays: run held down, let go to walk)
            forward_at: i64::MIN / 2,
            side_at: i64::MIN / 2,
            run_at: i64::MIN / 2,
            ..Keys::default()
        }
    }

    /// The level begun again: no keys down, nothing owed.
    pub fn let_go(&mut self) {
        *self = Keys {
            changes: self.changes,
            tics: self.tics,
            ..Keys::new(self.hand.clone())
        };
    }

    /// This tic's controls as keys give them: `c` with its forward and sideways pushes made
    /// ones the keys make (and `run` set, so they go to the game as they are).
    pub fn press(&mut self, tic: i64, mut c: Controls) -> Controls {
        let hand = self.hand.clone();
        let scale = if c.run { 1.0 } else { 0.5 };
        let want = (
            c.forward * scale + self.carry.0,
            c.strafe * scale + self.carry.1,
        );
        // (a key may change once it's been as it is for long enough)
        let free = |since: i64, hold: i64| tic - since >= hold;
        let (f_free, s_free, r_free) = (
            free(self.forward_at, hand.hold),
            free(self.side_at, hand.hold),
            free(self.run_at, hand.run_hold),
        );
        let mut best: Option<(f64, i8, i8, bool)> = None;
        for run in [true, false] {
            if run != self.run && !r_free {
                continue;
            }
            for f in [-1i8, 0, 1] {
                if f != self.forward && !f_free {
                    continue;
                }
                for s in [-1i8, 0, 1] {
                    if s != self.side && !s_free {
                        continue;
                    }
                    let got = Self::push(f, s, run);
                    let changed = f64::from(u8::from(f != self.forward))
                        + f64::from(u8::from(s != self.side))
                        // (run with no key down pushes nothing: its change is free then)
                        + f64::from(u8::from(run != self.run && (f != 0 || s != 0)));
                    let cost = (got.0 - want.0).powi(2)
                        + (got.1 - want.1).powi(2)
                        + changed * hand.change_cost.powi(2);
                    if best.is_none_or(|b| cost < b.0) {
                        best = Some((cost, f, s, run));
                    }
                }
            }
        }
        // (the keys as they are is always one of them)
        let (_, f, s, run) = best.unwrap_or((0.0, self.forward, self.side, self.run));
        if f != self.forward {
            self.forward_at = tic;
            self.changes += 1;
        }
        if s != self.side {
            self.side_at = tic;
            self.changes += 1;
        }
        if run != self.run {
            self.run_at = tic;
            self.changes += 1;
        }
        (self.forward, self.side, self.run) = (f, s, run);
        self.tics += 1;
        let got = Self::push(f, s, run);
        let owed = |want: f64, got: f64| ((want - got) * hand.carry).clamp(-hand.owed, hand.owed);
        self.carry = (owed(want.0, got.0), owed(want.1, got.1));
        c.forward = got.0;
        c.strafe = got.1;
        c.run = true;
        c
    }

    /// What the game pushes for these keys (forward, right).
    fn push(forward: i8, side: i8, run: bool) -> (f64, f64) {
        let pace = usize::from(run);
        (
            f64::from(forward) * FORWARD[pace],
            f64::from(side) * SIDE[pace],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(k: &mut Keys, tics: i64, forward: f64, strafe: f64) -> Vec<(f64, f64)> {
        (0..tics)
            .map(|t| {
                let c = k.press(
                    t,
                    Controls {
                        forward,
                        strafe,
                        ..Controls::default()
                    },
                );
                assert!(c.run);
                (c.forward, c.strafe)
            })
            .collect()
    }

    /// Whatever's asked, what comes out is a push the keys make.
    #[test]
    fn only_what_keys_give() {
        for (f, s) in [
            (50.0, 0.0),
            (13.0, -7.0),
            (-31.0, 44.0),
            (3.0, 3.0),
            (0.0, 0.0),
        ] {
            for (gf, gs) in pressed(&mut Keys::new(Hand::default()), 70, f, s) {
                assert!([0.0, 25.0, 50.0].contains(&gf.abs()), "forward {gf}");
                assert!([0.0, 24.0, 40.0].contains(&gs.abs()), "sideways {gs}");
                // (one run key for both: a walk one way is never with a run the other)
                assert!(!(gf.abs() == 50.0 && gs.abs() == 24.0));
                assert!(!(gf.abs() == 25.0 && gs.abs() == 40.0));
            }
        }
    }

    /// A full run forward is the forward key held with run: no tapping.
    #[test]
    fn a_run_is_a_key_held() {
        let mut k = Keys::new(Hand::default());
        assert!(pressed(&mut k, 70, 50.0, 0.0)
            .iter()
            .all(|&g| g == (50.0, 0.0)));
        assert_eq!(k.changes, 1);
    }

    /// A push no key gives comes out, over a while, as about that much.
    #[test]
    fn a_slow_step_is_taps() {
        let mut k = Keys::new(Hand::default());
        let got = pressed(&mut k, 350, 10.0, 0.0);
        let mean = got.iter().map(|g| g.0).sum::<f64>() / got.len() as f64;
        assert!(
            (mean - 10.0).abs() < 2.5,
            "asked 10 forward, got {mean:.1} on average"
        );
        assert!(got.iter().all(|g| g.1 == 0.0));
    }

    /// No key changes faster than a hand does.
    #[test]
    fn keys_are_held_as_long_as_a_hand_holds_them() {
        let hand = Hand::default();
        let mut k = Keys::new(Hand::default());
        let got = pressed(&mut k, 350, 17.0, -9.0);
        for axis in [0, 1] {
            let v: Vec<f64> = got
                .iter()
                .map(|g| if axis == 0 { g.0 } else { g.1 })
                .collect();
            let mut held = hand.hold; // (free to begin with)
            for w in v.windows(2) {
                // (the key itself: its sign, whatever run makes of its size)
                if w[0].signum() != w[1].signum() || (w[0] == 0.0) != (w[1] == 0.0) {
                    assert!(held >= hand.hold, "a key changed after {held} tics");
                    held = 0;
                }
                held += 1;
            }
        }
    }
}
