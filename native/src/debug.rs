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
