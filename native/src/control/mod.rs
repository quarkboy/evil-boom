// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Play directed by someone else (Evil, through the Neuro integration): what she's told to do,
//! what she can be told to do, and what she reports back. The bot underneath stays the same; in
//! directed play it chooses no objectives of its own (`Explorer::directed`).

pub mod directed;
pub mod game;
// (what things are called to her: below everything that writes for her, `crate::names`)
pub use crate::names;
pub mod places;
pub mod script;
