// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The commit the binary's built from (EVIL_COMMIT: the short hash, "-dirty" with changes not yet
//! committed), for the logs' names: each run's kept, not written over (the user's).

use std::process::Command;

fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "nogit".into());
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    println!(
        "cargo:rustc-env=EVIL_COMMIT={hash}{}",
        if dirty { "-dirty" } else { "" }
    );
    // (again on a commit, or a change to the sources)
    for p in ["../.git/HEAD", "../.git/index", "src"] {
        println!("cargo:rerun-if-changed={p}");
    }
}
