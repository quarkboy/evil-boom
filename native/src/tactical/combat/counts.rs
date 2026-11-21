// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The fight's counts and checks, for measuring a change by: how often a thing happened,
//! which node decided, what she knew of what hit her.

use super::*;

impl Combat {
    /// For measuring a run: add `by` to the count called `key`.
    pub fn count(&mut self, key: &str, by: f64) {
        *self.tally.entry(key.to_string()).or_insert(0.0) += by;
    }

    /// For measuring a run, never for deciding: a judgement number's check, evaluated at `tic`,
    /// `this` its own clause and `others` the rest of the check (what would decide the same
    /// without it). A tic counts once however many times the check's asked in it (a target at a
    /// time, a gun at a time): held if the clause was true at any asking, decided if it was true
    /// and the rest false at one.
    pub fn decided(&self, key: &'static str, tic: i64, this: bool, others: bool) {
        let mut checks = self.checks.borrow_mut();
        let c = checks.entry(key).or_default();
        if c.tic != Some(tic) {
            c.close_tic();
            c.tic = Some(tic);
        }
        c.now_held |= this;
        c.now_decided |= this && !others;
    }

    /// The judgement checks' counts (`decided`), as `STATS` lines: for each, the tics it was
    /// asked, held and decided in, and the occasions it decided (a second or more apart).
    pub fn check_counts(&self) -> Vec<(String, f64)> {
        let mut checks = self.checks.borrow_mut();
        let mut out = Vec::new();
        for (key, c) in checks.iter_mut() {
            c.close_tic();
            c.tic = None;
            for (what, n) in [
                ("tics asked", c.asked),
                ("tics held", c.held),
                ("tics decided", c.decided),
                ("occasions decided", c.occasions),
            ] {
                out.push((format!("check {key}: {what}"), n));
            }
        }
        out
    }

    /// For measuring a run, never for deciding: what she knew of the monster with that id when
    /// it hit her ("in view", "remembered", "known" of but out of mind, "unknown"), and how far
    /// off it was (near: within 384).
    pub fn measure(&self, pos: Point, id: i32) -> (&'static str, &'static str) {
        let knew = if self.visible.iter().any(|t| t.id == id) {
            "in view"
        } else if self.now.remembered.iter().any(|t| t.id == id) {
            "remembered"
        } else if self.known.contains_key(&id) {
            "known"
        } else {
            "unknown"
        };
        let far = match self.senses.game_monsters.get(&id) {
            Some(t) if dist((t.x, t.y), pos) < 384.0 => "near",
            Some(_) => "far",
            None => "gone",
        };
        (knew, far)
    }
}
