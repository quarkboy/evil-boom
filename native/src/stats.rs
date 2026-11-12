// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Where a plan search's time goes: counts and seconds per phase, reported on stderr when
//! EVILBOOM_NATIVE_STATS is set.

use std::cell::{Cell, RefCell};
use std::time::Instant;

#[derive(Clone, Copy)]
pub enum Phase {
    /// path-finding for a plan leg (including its smoothing)
    TimedPath,
    FloodFill,
    /// within path-finding:
    AStar,
    Corners,
    Fit,
    Straighten,
}

const PHASES: usize = 6;

struct Tally {
    count: [usize; PHASES],
    seconds: [f64; PHASES],
}

impl Tally {
    const fn new() -> Self {
        Tally {
            count: [0; PHASES],
            seconds: [0.0; PHASES],
        }
    }
}

thread_local! {
    static TALLY: RefCell<Tally> = const { RefCell::new(Tally::new()) };
}

pub fn reset() {
    TALLY.with(|t| *t.borrow_mut() = Tally::new());
}

/// Count one `phase`, begun at `since`.
pub fn record(phase: Phase, since: Instant) {
    let secs = since.elapsed().as_secs_f64();
    TALLY.with(|t| {
        let mut t = t.borrow_mut();
        t.count[phase as usize] += 1;
        t.seconds[phase as usize] += secs;
    });
}

/// How a plan search ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// a route, no squeezes
    Found,
    /// the best squeezed route there was
    Squeezed,
    /// nowhere left to look: no route at all
    Exhausted,
    /// out of expansions (or time) with more to look at: no route found in the budget
    Budget,
}

/// One plan search: its budget, the expansions it made, how it ended, and its seconds.
#[derive(Clone, Copy, Debug)]
pub struct Search {
    pub budget: usize,
    pub expansions: usize,
    pub ended: Ended,
    pub secs: f64,
}

thread_local! {
    static SEARCHES: RefCell<Vec<Search>> = const { RefCell::new(Vec::new()) };
    static LAST_ENDED: Cell<Option<Ended>> = const { Cell::new(None) };
    static MAXED_SINCE: Cell<bool> = const { Cell::new(false) };
    static COLLECT_SEARCHES: Cell<bool> = const { Cell::new(false) };
}

/// Note a search done (for the live status's record of them: `EVIL_STATUS`).
pub fn search_done(s: Search) {
    LAST_ENDED.with(|l| l.set(Some(s.ended)));
    if s.ended == Ended::Budget {
        MAXED_SINCE.with(|n| n.set(true));
    }
    if COLLECT_SEARCHES.with(Cell::get) {
        SEARCHES.with(|v| v.borrow_mut().push(s));
    }
}

/// Retain search histories only while a live status writer consumes them. The planner's
/// outcome and adaptive budget signals are always recorded, independently of diagnostics.
pub fn collect_searches(enabled: bool) {
    COLLECT_SEARCHES.with(|c| c.set(enabled));
    SEARCHES.with(|v| v.borrow_mut().clear());
}

/// Has a plan search used up its budget since last asked?
pub fn take_maxed() -> bool {
    MAXED_SINCE.with(|n| n.replace(false))
}

/// How the last plan search ended.
pub fn last_ended() -> Option<Ended> {
    LAST_ENDED.with(Cell::get)
}

/// The searches noted since last asked.
pub fn take_searches() -> Vec<Search> {
    SEARCHES.with(|v| std::mem::take(&mut *v.borrow_mut()))
}

pub fn enabled() -> bool {
    crate::config::env("NATIVE_STATS").is_some()
}

/// One line on the search so far, which took `total` seconds.
pub fn report(total: f64) -> String {
    TALLY.with(|t| {
        let t = t.borrow();
        let (n, s) = (|p: Phase| t.count[p as usize], |p: Phase| t.seconds[p as usize]);
        format!(
            "native plan {total:.2}s: {} timed paths {:.2}s (A* {:.2} corners {:.2} fit {:.2} straighten {:.2}), {} flood fills {:.2}s",
            n(Phase::TimedPath),
            s(Phase::TimedPath),
            s(Phase::AStar),
            s(Phase::Corners),
            s(Phase::Fit),
            s(Phase::Straighten),
            n(Phase::FloodFill),
            s(Phase::FloodFill),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_outcomes_do_not_depend_on_diagnostic_collection() {
        for enabled in [false, true, false] {
            collect_searches(enabled);
            take_maxed();
            search_done(Search {
                budget: 10,
                expansions: 10,
                ended: Ended::Budget,
                secs: 0.0,
            });
            search_done(Search {
                budget: 10,
                expansions: 3,
                ended: Ended::Found,
                secs: 0.0,
            });
            assert_eq!(last_ended(), Some(Ended::Found));
            assert!(take_maxed());
            assert!(!take_maxed());
            assert_eq!(take_searches().len(), if enabled { 2 } else { 0 });
            assert!(take_searches().is_empty());
        }
    }
}
