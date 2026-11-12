// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What Evil knows of the level (`model`) and how she goes about finding out the rest
//! (`explorer`).

pub mod completion;

/// the share of her health something on the way may cost her and still not be a risk (the
/// user's: "10% or less of current health = not risky"): floors that hurt, a fast crusher's stroke
pub const RISK_SHARE: f64 = 0.1;

pub mod explorer;
pub mod model;
