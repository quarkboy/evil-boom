// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The commit the binary's built from (EVIL_COMMIT: the short hash, "-dirty" with changes not yet
//! committed), for the logs' names: each run's kept, not written over. And on Windows, the
//! program's icon.

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
    // (the version as it's shown: Cargo's three numbers, and the fix's after them if there is
    // one, `fix = N` under [package.metadata.evil])
    let fix = std::fs::read_to_string("Cargo.toml")
        .ok()
        .and_then(|t| {
            // (its own section's, not a `fix = ` anywhere else in the file)
            t.lines()
                .skip_while(|l| l.trim() != "[package.metadata.evil]")
                .skip(1)
                .take_while(|l| !l.trim_start().starts_with('['))
                .find_map(|l| l.strip_prefix("fix = ").map(str::trim).map(str::to_string))
        })
        .filter(|n| n != "0")
        .map_or(String::new(), |n| format!(".{n}"));
    println!(
        "cargo:rustc-env=EVIL_VERSION={}{fix}",
        env!("CARGO_PKG_VERSION")
    );
    // (again on a commit, or a change to the sources)
    for p in [
        "../.git/HEAD",
        "../.git/index",
        "src",
        "assets/icon.res",
        "Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={p}");
    }
    // The program's icon on Windows: assets/icon.res (a compiled resource, made by
    // tools/logo/icon.py) handed to the linker as it is. MSVC's takes one; the GNU one wants it
    // turned into an object first, so a build with that toolchain goes without.
    let target = |k: &str| std::env::var(format!("CARGO_CFG_TARGET_{k}")).unwrap_or_default();
    if target("OS") == "windows" && target("ENV") == "msvc" {
        if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let res = std::path::Path::new(&dir).join("assets").join("icon.res");
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
    }
}
