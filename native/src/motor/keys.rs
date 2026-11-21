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
//! it's decided. (Tried and dropped, 2026-11-21: by keys, going only where she faces and
//! turning to go another way, no sidestep but to stop a slide. As written it lost sixty exits
//! of 544 on the three-lives test against sidestepping by taps: her turn is eased in and out
//! over many tics, far slower than a hand on a mouse, and the route was walked wide.) Nor are fire, use and the weapon keys, which are keys already.
//!
//! What comes out is a tic command anyone at a keyboard and a mouse could have made, which is
//! what a demo is a list of.

use crate::domain::Controls;
use crate::tactical::tuning::Hand;

/// The game's pushes for the keys: forward or back at a walk and a run, sideways the same
/// (`forwardmove[2]` and `sidemove[2]`, g_game.c).
const FORWARD: [f64; 2] = [25.0, 50.0];
const SIDE: [f64; 2] = [24.0, 40.0];

/// Her movement is by keys, this run (`Agent::keys_only`). The process's, as the game is: one
/// game a process, and whoever moves her in it (the follower, the fight, a boss's lanes) asks
/// here how.
static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn on() -> bool {
    ON.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_on() {
    ON.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The keys as they're held, and since when.
#[derive(Clone, Debug, Default)]
pub struct Keys {
    /// forward, back, right, left: each down or up, and the tic it last changed
    down: [bool; 4],
    at: [i64; 4],
    run: bool,
    run_at: i64,
    /// what was asked for and no key gave, owed (forward, right)
    carry: (f64, f64),
    /// presses and releases so far, for measuring by
    pub changes: u64,
    pub tics: u64,
    /// how fast her hand is
    hand: Hand,
}

const FWD: usize = 0;
const BACK: usize = 1;
const RIGHT: usize = 2;
const LEFT: usize = 3;

impl Keys {
    pub fn new(hand: Hand) -> Keys {
        Keys {
            hand,
            run: true, // (as anyone plays: run held down, let go to walk)
            at: [i64::MIN / 2; 4],
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

    /// May key `k` be `down` this tic: as it is, or changed after long enough as it was (a
    /// press lasts `Hand::press` tics at least, and a key let go stays up `Hand::gap`).
    fn may(&self, k: usize, down: bool, tic: i64) -> bool {
        self.down[k] == down
            || tic - self.at[k]
                >= if self.down[k] {
                    self.hand.press
                } else {
                    self.hand.gap
                }
    }

    /// This tic's controls as keys give them: `c` with its forward and sideways pushes made
    /// ones the keys make (and `run` set, so they go to the game as they are).
    pub fn press(&mut self, tic: i64, mut c: Controls) -> Controls {
        let hand = self.hand.clone();
        let scale = if c.run { 1.0 } else { 0.5 };
        let asked = (c.forward * scale, c.strafe * scale);
        // (a key pressed stays pressed a while, so what's owed is asked for a share a tic over
        // that long: a tap is begun when about half of what it will give is wanted, and not
        // for a crumb, which it would overpay several times and then have to pay back. Asked
        // for whole each tic, as it first was, cost twenty exits more of the 544: 251 for 271)
        let over = hand.press.max(1) as f64;
        let want = (asked.0 + self.carry.0 / over, asked.1 + self.carry.1 / over);
        let run_free = tic - self.run_at >= hand.run_hold;
        let (was_f, was_s) = (self.axis(FWD, BACK), self.axis(RIGHT, LEFT));
        let mut best: Option<(f64, i8, i8, bool)> = None;
        for run in [true, false] {
            if run != self.run && !run_free {
                continue;
            }
            for f in [-1i8, 0, 1] {
                // (one key of a pair down at most: the other let go as this one's pressed, as
                // a hand goes from one to the other)
                if !(self.may(FWD, f > 0, tic) && self.may(BACK, f < 0, tic)) {
                    continue;
                }
                for s in [-1i8, 0, 1] {
                    if !(self.may(RIGHT, s > 0, tic) && self.may(LEFT, s < 0, tic)) {
                        continue;
                    }
                    let got = Self::push(f, s, run);
                    let changed = f64::from(u8::from(f != was_f))
                        + f64::from(u8::from(s != was_s))
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
        let (_, f, s, run) = best.unwrap_or((0.0, was_f, was_s, self.run));
        for (k, down) in [(FWD, f > 0), (BACK, f < 0), (RIGHT, s > 0), (LEFT, s < 0)] {
            if self.down[k] != down {
                self.down[k] = down;
                self.at[k] = tic;
                self.changes += 1;
            }
        }
        if run != self.run {
            self.run = run;
            self.run_at = tic;
            self.changes += 1;
        }
        self.tics += 1;
        let got = Self::push(f, s, run);
        let owed = |owed: f64, asked: f64, got: f64| {
            ((owed + asked - got) * hand.carry).clamp(-hand.owed, hand.owed)
        };
        self.carry = (
            owed(self.carry.0, asked.0, got.0),
            owed(self.carry.1, asked.1, got.1),
        );
        c.forward = got.0;
        c.strafe = got.1;
        c.run = true;
        c
    }

    /// The keys as her hand holds them: forward (1) or back (-1), right (1) or left (-1), and
    /// run. For showing (`watch --show-keys`).
    pub fn held(&self) -> (i8, i8, bool) {
        (self.axis(FWD, BACK), self.axis(RIGHT, LEFT), self.run)
    }

    /// A pair of keys as one way: 1, -1, or neither.
    fn axis(&self, plus: usize, minus: usize) -> i8 {
        i8::from(self.down[plus]) - i8::from(self.down[minus])
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
            // (each key of the pair by itself: down while the push is its way)
            for way in [1.0, -1.0] {
                let mut held = hand.gap; // (free to begin with)
                for w in v.windows(2) {
                    let (was, is) = (w[0] * way > 0.0, w[1] * way > 0.0);
                    if was != is {
                        let least = if was { hand.press } else { hand.gap };
                        assert!(held >= least, "a key changed after {held} tics");
                        held = 0;
                    }
                    held += 1;
                }
            }
        }
    }
}
