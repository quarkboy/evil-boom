// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What the follower keeps from tic to tic, by what it's for: each a part of `RouteFollower`.

use super::Run;
use crate::geom::Point;
use crate::nav::Heights;
use crate::navigator::Jump;
use std::collections::{BTreeMap, BTreeSet};

/// A barrel in the way being shot: where from, the way there, the run off after.
pub struct BarrelShot {
    /// (where, since tic) a barrel in the way being shot
    pub(super) target: Option<(Point, i64)>,
    /// (mortal) where to shoot it from: back along the way she came, outside every blast
    pub(super) post: Option<Point>,
    /// ...and the way there (its waypoints, the next first)
    pub(super) way: Vec<Point>,
    /// ...and the nearest she's got to it, and when
    pub(super) best: Option<(f64, i64)>,
    /// the tic of the last shot at it
    pub(super) last_shot: i64,
    /// the tic she last stood to shoot it (where it's shot from, turning to it or tapping)
    pub(super) stood: i64,
    /// a tap not yet seen to have set anything off: the barrels there were then, and when
    pub(super) tapped: Option<(usize, i64)>,
    /// shot: running off till this tic, this way
    pub(super) flee_until: Option<i64>,
    /// ...along this way (the next first)
    pub(super) flee_way: Vec<Point>,
    /// (mortal) the barrel's shot from somewhere out of every blast of its chain: she stays
    /// there till it's over, no running off
    pub(super) stay: bool,
    /// the step whose barrel (`Step::barrel`) she's taken on already
    pub(super) step: Option<i64>,
    /// shot and gone: how many barrels there were, and since when (the chain going off: not back
    /// till none has gone for a while)
    pub(super) settle: Option<(usize, i64)>,
    /// (mortal) spots chosen to shoot this barrel from that turned out to have no shot (a line
    /// grazing a corner): not chosen again for it
    pub(super) failed_posts: Vec<Point>,
}

/// A chain of barrels going off that she didn't set off: kept off it, or run out of it.
pub struct LitChain {
    /// kept off a chain of barrels going off that she didn't set off (logged once a wait)
    pub(super) waiting: bool,
    /// in a chain's blasts: the way out of them she's running (the next first), chosen when
    pub(super) flee: Vec<Point>,
    pub(super) flee_tic: i64,
}

/// Steering between the barrels she sees, which the mesh's way knows nothing of.
pub struct Weave {
    /// the barrels as last seen (her steering keeps its line clear of them)
    pub(super) barrels: Vec<Point>,
    /// a way round barrels standing in her track: the points to go by, the next first
    pub(super) detour: Vec<Point>,
    /// when a way round was last looked for and not found
    pub(super) detour_tic: i64,
}

/// Ceilings coming down: what she's seen of them, and the wait or the crossing in hand.
pub struct Crushers {
    /// (mortal) waiting at the edge of a crusher for it to come up, logged
    pub(super) wait: Option<usize>,
    /// (mortal) a crusher's crossing timed and begun: its sector, and since when
    pub(super) cross: Option<(usize, i64)>,
    /// (mortal) setting off a stopped crusher from its edge, to cross it when it comes up: its
    /// sector, since when, and the spot just over its line she steps to
    pub(super) kick: Option<(usize, i64, Point)>,
    /// sector -> how fast its ceiling was last seen moving (units a tic)
    pub(super) ceiling_pace: BTreeMap<usize, f64>,
    pub(super) moved_at: BTreeMap<usize, i64>,
    /// since when the box has been under a ceiling too low for her
    pub(super) pinned_since: Option<i64>,
}

/// A run-jump or a run over sinking floors in hand.
pub struct Jumping {
    /// (jump, heading, start tic) while sprinting
    pub(super) sprint: Option<(Jump, f64, i64)>,
    /// sprints called off for want of speed, one after another
    pub(super) sprints_short: u32,
    /// (jump, tic) a run-jump done: where does it come down?
    pub(super) touchdown: Option<(Jump, i64)>,
    pub(super) run: Option<Run>,
    /// the floor that stays put a run over sinking floors is making for (logged when it changes)
    pub(super) run_for: Option<usize>,
    pub(super) lineup_tic: Option<i64>,
    pub(super) brake_until: i64,
    /// (sector, live heights, since)
    pub(super) wait: Option<(usize, Option<Heights>, i64)>,
    pub(super) sinking: BTreeSet<usize>,
}

/// Backing off from where she got nowhere.
pub struct Unstick {
    pub(super) count: i32,
    pub(super) until: i64,
    pub(super) dir: f64,
}

/// Where she is on the segment's track.
pub struct OnTrack {
    /// the track's piece she's on (`Track::project`)
    pub(super) index: usize,
    /// how far along the track she is
    pub(super) along: f64,
    /// how far off its line
    pub(super) off: f64,
    /// the furthest along she's got (progress: getting somewhere, not stuck)
    pub(super) furthest: f64,
}
