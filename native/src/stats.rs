//! Where a plan search's time goes: counts and seconds per phase, reported on stderr when
//! EVILDOOM_NATIVE_STATS is set.

use std::cell::RefCell;
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
        Tally { count: [0; PHASES], seconds: [0.0; PHASES] }
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

pub fn enabled() -> bool {
    std::env::var_os("EVILDOOM_NATIVE_STATS").is_some()
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
