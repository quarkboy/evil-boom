// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Levels made in memory (`evilboom::test_support`): the level read, meshed, analysed and
//! planned with no IWAD and no game, so these run in CI, where tests/levels.rs skips itself.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use evilboom::navigator::Navigator;
use evilboom::test_support::{room, wad_bytes, Wall};
use evilboom::wad::{load_map, Wad};
use evilboom::walkthrough::plan_level;

fn level(walls: [Wall; 4]) -> Arc<evilboom::map::MapData> {
    let w = Wad::from_bytes(Path::new("room.wad"), wad_bytes(&room(512, walls))).expect("a WAD");
    Arc::new(load_map(&w, "MAP01", &[]).expect("a map"))
}

/// A room whose east wall is the exit switch: the plan is to walk to it and press it.
#[test]
fn a_room_with_an_exit_switch_is_planned_out_of() {
    let m = level([Wall::PLAIN, Wall::PLAIN, Wall::EXIT_SWITCH, Wall::PLAIN]);
    let nav = Rc::new(Navigator::new(Arc::clone(&m), 3).expect("the analysis"));
    let start = m.player_start().map(|t| (t.x, t.y)).expect("a start");
    assert!(nav.poly_at(start).is_some(), "the start is on the mesh");
    let view = nav.full_view();
    let plan = plan_level(&view, start, 10.0, 20000);
    assert!(plan.complete, "{}", plan.problem);
    let last = plan.steps.last().expect("a step");
    assert!(
        last.trigger
            .as_ref()
            .is_some_and(|t| t.linedef() == Some(2)),
        "the last step presses the exit switch (line 2): {}",
        last.label
    );
}

/// The same room with no exit: the plan says so, and nothing panics for it.
#[test]
fn a_room_with_no_exit_has_no_plan() {
    let m = level([Wall::PLAIN; 4]);
    let nav = Rc::new(Navigator::new(Arc::clone(&m), 3).expect("the analysis"));
    let start = m.player_start().map(|t| (t.x, t.y)).expect("a start");
    let plan = plan_level(&nav.full_view(), start, 5.0, 20000);
    assert!(!plan.complete);
    assert!(!plan.problem.is_empty());
}
