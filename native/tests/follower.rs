// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Driving a planned route in DSDA-Doom (no monsters) to E1M1's exit. (The game is one a
//! process: this file is a process of its own; the knocked-off-course test is in another.)

#[path = "common/drive.rs"]
mod drive;

#[test]
fn bot_exits_e1m1() {
    let Some((exited, _, events)) = drive::drive(false) else {
        return;
    };
    assert!(exited, "{}", events.join("\n"));
}
