// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A deathmatch's styles of play, each numbers over the tuning (as a stance is,
//! `control/directed/policy.rs`), most aggressive first: how she hunts the others (when she's
//! geared up enough to go after them, how far she goes for a clue, how soon she gives up and
//! gears up again), and how she fights them (how she stands and falls back, how widely she
//! sprays rockets and plasma across where they'd dodge to). For the bots now (`--hunt-style`,
//! `--fight-style`); for a deathmatch policy and stances of Evil's later.

use serde_json::{json, Value};

/// The hunting styles, most aggressive first.
pub const HUNT_STYLES: [&str; 5] = ["relentless", "eager", "steady", "careful", "passive"];

/// The fighting styles, most aggressive first.
pub const FIGHT_STYLES: [&str; 5] = ["berserk", "aggressive", "steady", "careful", "evasive"];

/// How she hunts, as numbers over the tuning (None: not a style).
pub fn hunt_style(name: &str) -> Option<Value> {
    Some(match name {
        // (any gun past the pistol with a few rounds, health hardly matters; a clue gone after
        // from across the map and long after; what she doesn't need left lying)
        "relentless" => json!({ "deathmatch": {
            "shells": 4, "bullets": 20, "rockets": 1, "cells": 10,
            "health": 30.0, "armored_health": 20.0, "armor": 0.0, "weak_health": 15.0,
            "fresh_secs": 10.0, "fresh_near": 1600.0, "clue_secs": 30.0,
            "hunt_weigh": 0.15, "item_weigh": 3.0 } }),
        "eager" => json!({ "deathmatch": {
            "shells": 10, "bullets": 50, "rockets": 3, "cells": 20,
            "health": 45.0, "armored_health": 30.0, "armor": 25.0, "weak_health": 30.0,
            "fresh_secs": 7.0, "fresh_near": 1000.0, "clue_secs": 25.0,
            "hunt_weigh": 0.2, "item_weigh": 2.5 } }),
        // (the tuning's own)
        "steady" => json!({ "deathmatch": {
            "shells": 20, "bullets": 100, "rockets": 5, "cells": 40,
            "health": 60.0, "armored_health": 40.0, "armor": 50.0, "weak_health": 40.0,
            "fresh_secs": 5.0, "fresh_near": 640.0, "clue_secs": 20.0,
            "hunt_weigh": 0.3, "item_weigh": 2.0 } }),
        // (well armed and armored first, back to gearing up early; only a clue near her)
        "careful" => json!({ "deathmatch": {
            "shells": 30, "bullets": 150, "rockets": 10, "cells": 80,
            "health": 80.0, "armored_health": 60.0, "armor": 100.0, "weak_health": 60.0,
            "fresh_secs": 4.0, "fresh_near": 400.0, "clue_secs": 15.0,
            "hunt_weigh": 0.5, "item_weigh": 1.2 } }),
        // (never goes looking: gears up and fights what comes to her)
        "passive" => json!({ "deathmatch": {
            "shells": 10000, "bullets": 10000, "rockets": 10000, "cells": 10000,
            "health": 1000.0, "armored_health": 1000.0, "weak_health": 1000.0,
            "fresh_near": 0.0 } }),
        _ => return None,
    })
}

/// How she fights, as numbers over the tuning (None: not a style). The stances' numbers for
/// standing and falling back (`policy::stance_tuning`: yolo, bold, steady, careful, avoid), and
/// the spray of rockets and plasma at a rival, wide to none.
pub fn fight_style(name: &str) -> Option<Value> {
    Some(match name {
        // (everything fought head on, never fallen back from; rockets and plasma across all the
        // ground they could dodge to)
        "berserk" => json!({ "engage": { "stand_threat": 0.0, "low_health": 0.0,
            "leave_be": 1000.0 }, "retreat": { "health": 0.0 },
            "guns": { "outmatched": 1000.0 }, "steer": { "near_by_gun": false },
            "deathmatch": { "spray": 1.0 } }),
        "aggressive" => json!({ "engage": { "stand_threat": 3.0, "low_health": 25.0 },
            "retreat": { "health": 30.0 }, "guns": { "outmatched": 45.0 },
            "steer": { "near_by_gun": false }, "deathmatch": { "spray": 0.75 } }),
        "steady" => json!({ "engage": { "stand_threat": 6.0, "low_health": 35.0 },
            "retreat": { "health": 40.0 }, "guns": { "outmatched": 30.0 },
            "steer": { "near_by_gun": false }, "deathmatch": { "spray": 0.5 } }),
        "careful" => json!({ "engage": { "stand_threat": 9.0, "low_health": 50.0 },
            "retreat": { "health": 55.0 }, "guns": { "outmatched": 20.0 },
            "steer": { "near_by_gun": false }, "deathmatch": { "spray": 0.25 } }),
        // (kept clear of, fallen back from early; each shot aimed, none sprayed)
        "evasive" => json!({ "engage": { "stand_threat": 9.0, "low_health": 50.0,
            "keep_clear": 1536.0, "leave_be": 8.0 }, "retreat": { "health": 70.0 },
            "guns": { "outmatched": 20.0 }, "errands": { "danger_near": 1024.0 },
            "steer": { "near_by_gun": true }, "deathmatch": { "spray": 0.0 } }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tactical::tuning::Tuning;

    /// Every style is numbers the tuning has.
    #[test]
    fn styles_fit_the_tuning() {
        let t = Tuning::default();
        for s in HUNT_STYLES {
            t.with(&hunt_style(s).unwrap()).unwrap();
        }
        for s in FIGHT_STYLES {
            t.with(&fight_style(s).unwrap()).unwrap();
        }
        // (and steady hunting is the tuning's own)
        assert_eq!(t.with(&hunt_style("steady").unwrap()).unwrap(), t);
    }
}
