// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The debug switches: environment variables a run can be given to have more written to its
//! log (or to stderr) while tracing one. Read once, when first asked for: a switch tested
//! every tic costs a field read, not a walk through the environment.
//!
//! What each prints is in docs/DEBUGGING.md. None of them changes what she does.

use std::sync::OnceLock;

/// The switches, as the run was started with them.
#[derive(Debug, Default)]
pub struct Switches {
    /// `EVIL_DEBUG_SHOT`: each shot out, shots with nothing on her line, where she and her
    /// target are every 175 tics
    pub shot: bool,
    /// `EVIL_DEBUG_BARRELS`: barrels going off and the ways out of a chain weighed
    pub barrels: bool,
    /// `EVIL_DEBUG_DARTS`: each dart (a run one way, then at once back the other) in the log
    pub darts: bool,
    /// EVIL_DEBUG_KEYS: with `--keys`, each tic's push as asked and as the keys gave it (stderr)
    pub keys: bool,
    /// `EVIL_DEBUG_CARRY`: the loadout a level is begun with
    pub carry: bool,
    /// `EVIL_DEBUG_DEAD_END`: what a drop refused as a dead end leads to
    pub dead_end: bool,
    /// `EVIL_DEBUG_NO_ROUTE`: why an objective the planner had a way to was refused
    pub no_route: bool,
    /// `EVIL_DEBUG_PACE`: what limits the route's pace, the controls, every follower's log
    pub pace: bool,
    /// `EVIL_DEBUG_SET_ON`: two bosses about, and the spot to line them up from
    pub set_on: bool,
    /// `EVIL_DEBUG_USE`: every use press and the line the model has it hit
    pub use_press: bool,
    /// `EVIL_DEBUG_RELAXED`: the planner's relaxed pass
    pub relaxed: bool,
    /// `EVIL_DEBUG_SCOUT`: the log of the pass before a level known beforehand is kept
    pub scout: bool,
    /// `EVIL_DEBUG_REACH=<tic>`: her reach at that tic
    pub reach: Option<i64>,
    /// `EVIL_DEBUG_HEIGHTS=<sector>,...`: with `reach`, those sectors' heights
    pub heights: Vec<usize>,
    /// `EVIL_DEBUG_JUMPS_INTO=<sector>`: the jumps the planner has into a sector
    pub jumps_into: Option<usize>,
    /// `EVIL_DEBUG_OBJECTIVES=<from>-<to>`: the tics whose choices are logged in full
    pub objectives: Option<(i64, i64)>,
    /// `EVIL_KNOWN_ONLY=map|monsters`: which half of knowing a level beforehand she has
    pub known_only: Option<String>,
}

/// The switches (read from the environment the first time).
pub fn on() -> &'static Switches {
    static SWITCHES: OnceLock<Switches> = OnceLock::new();
    SWITCHES.get_or_init(|| {
        let set = |name: &str| std::env::var_os(name).is_some();
        let value = |name: &str| std::env::var(name).ok();
        Switches {
            shot: set("EVIL_DEBUG_SHOT"),
            barrels: set("EVIL_DEBUG_BARRELS"),
            darts: set("EVIL_DEBUG_DARTS"),
            keys: set("EVIL_DEBUG_KEYS"),
            carry: set("EVIL_DEBUG_CARRY"),
            dead_end: set("EVIL_DEBUG_DEAD_END"),
            no_route: set("EVIL_DEBUG_NO_ROUTE"),
            pace: set("EVIL_DEBUG_PACE"),
            set_on: set("EVIL_DEBUG_SET_ON"),
            use_press: set("EVIL_DEBUG_USE"),
            relaxed: set("EVIL_DEBUG_RELAXED"),
            scout: set("EVIL_DEBUG_SCOUT"),
            reach: value("EVIL_DEBUG_REACH").and_then(|v| v.trim().parse().ok()),
            heights: value("EVIL_DEBUG_HEIGHTS")
                .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                .unwrap_or_default(),
            jumps_into: value("EVIL_DEBUG_JUMPS_INTO").and_then(|v| v.trim().parse().ok()),
            objectives: value("EVIL_DEBUG_OBJECTIVES").and_then(|v| {
                let (a, b) = v.split_once('-')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            }),
            known_only: value("EVIL_KNOWN_ONLY"),
        }
    })
}

/// `EVIL_DEBUG_SLOW=MS`: the parts of a tic that took MS milliseconds or more, to stderr
/// ("slow tic 18725: director.questions 41.2, neuro.tic 43.0"). For a window that stutters:
/// which of her thinking is holding the frame.
fn slow_over() -> Option<f64> {
    static OVER: OnceLock<Option<f64>> = OnceLock::new();
    *OVER.get_or_init(|| std::env::var("EVIL_DEBUG_SLOW").ok()?.parse().ok())
}

thread_local! {
    static SLOW: std::cell::RefCell<Vec<(&'static str, f64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `f` run, and how long it took kept under `label` if `EVIL_DEBUG_SLOW` asks.
pub fn timed<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let Some(over) = slow_over() else {
        return f();
    };
    let began = std::time::Instant::now();
    let out = f();
    let ms = began.elapsed().as_secs_f64() * 1000.0;
    if ms >= over {
        SLOW.with(|s| s.borrow_mut().push((label, ms)));
    }
    out
}

/// A stretch of a function begun at `began` is over: kept under `label` if it was slow and
/// `EVIL_DEBUG_SLOW` asks (for where a closure won't go). The time now, for the next stretch.
pub fn lap(label: &'static str, began: std::time::Instant) -> std::time::Instant {
    let now = std::time::Instant::now();
    if let Some(over) = slow_over() {
        let ms = now.duration_since(began).as_secs_f64() * 1000.0;
        if ms >= over {
            SLOW.with(|s| s.borrow_mut().push((label, ms)));
        }
    }
    now
}

/// The rest of a function, timed from here to wherever it leaves (`let _t = Timed::new(..)`).
pub struct Timed(&'static str, std::time::Instant);

impl Timed {
    pub fn new(label: &'static str) -> Timed {
        Timed(label, std::time::Instant::now())
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        lap(self.0, self.1);
    }
}

/// A tic's over: what of it was slow, said.
pub fn slow_report(tic: i64) {
    if slow_over().is_none() {
        return;
    }
    SLOW.with(|s| {
        let mut s = s.borrow_mut();
        if !s.is_empty() {
            let parts: Vec<String> = s.iter().map(|(l, ms)| format!("{l} {ms:.1}")).collect();
            eprintln!("slow tic {tic}: {}", parts.join(", "));
            s.clear();
        }
    });
}

/// a frame this long, begun to shown, is said (`slow_frame`): two and a half of the screen's
const SLOW_FRAME_MS: f64 = 40.0;

/// A frame of the window shown, begun at `began`, its tics played by `stepped`, painted by
/// `painted`: said to stderr if it was long and `EVIL_DEBUG_SLOW` asks ("slow frame at tic
/// 15097: 250 ms: step 3, paint 2, present 245"). The whole of it: the engine's own step and
/// the screen's, which the parts of a tic (`timed`) don't cover; with a step that's short, the
/// time went outside her.
pub fn slow_frame(
    tic: i64,
    began: std::time::Instant,
    stepped: std::time::Instant,
    painted: std::time::Instant,
) {
    if slow_over().is_none() {
        return;
    }
    let ms =
        |a: std::time::Instant, b: std::time::Instant| b.duration_since(a).as_secs_f64() * 1000.0;
    let shown = std::time::Instant::now();
    if ms(began, shown) >= SLOW_FRAME_MS {
        eprintln!(
            "slow frame at tic {tic}: {:.0} ms: step {:.0}, paint {:.0}, present {:.0}",
            ms(began, shown),
            ms(began, stepped),
            ms(stepped, painted),
            ms(painted, shown)
        );
    }
}
