// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Where things are on this machine: the game's WADs, the project's own folders.

use std::path::{Path, PathBuf};

/// Steam's Ultimate Doom folder (base/ holds the IWADs), inside Steam's library.
const STEAM_DOOM: &str = "steamapps/common/Ultimate Doom";

/// The classic v1.9 IWADs inside the Steam install, by short name.
const IWADS: [(&str, &str); 4] = [
    ("doom", "base/DOOM.WAD"),
    ("doom2", "base/doom2/DOOM2.WAD"),
    ("tnt", "base/tnt/TNT.WAD"),
    ("plutonia", "base/plutonia/PLUTONIA.WAD"),
];

/// The IWADs known by a short name ("doom", "doom2", ...).
pub fn iwad_names() -> impl Iterator<Item = &'static str> {
    IWADS.iter().map(|(n, _)| *n)
}

pub fn doom_dir() -> PathBuf {
    env("DOOM_DIR").map_or_else(default_doom_dir, PathBuf::from)
}

/// A setting from the environment: EVILBOOM_<name>, or EVILDOOM_<name> as it was called before
/// the project was Evil BOOM.
pub fn env(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("EVILBOOM_{name}"))
        .or_else(|| std::env::var_os(format!("EVILDOOM_{name}")))
}

/// Windows: Steam's own folder. Linux: ~/.local/share/evilboom, the Steam folder's base/ copied
/// there (under WSL, reading through /mnt/c is slow); ~/.local/share/evildoom, the folder's old
/// name, if that's the one there is.
fn default_doom_dir() -> PathBuf {
    if cfg!(windows) {
        return Path::new(r"C:\Program Files (x86)\Steam").join(STEAM_DOOM);
    }
    let share = std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".local/share");
    let new = share.join("evilboom");
    let old = share.join("evildoom");
    if !new.exists() && old.exists() {
        old
    } else {
        new
    }
}

/// The PWADs loaded over the IWAD, in order (`EVILBOOM_PWADS`, paths joined as PATH is; set from
/// `--pwad FILE` by `take_pwads`, and so passed on to the runs a sweep or an episode starts).
pub fn pwads() -> Vec<PathBuf> {
    env("PWADS").map_or_else(Vec::new, |v| std::env::split_paths(&v).collect())
}

/// `args` less each `--pwad FILE`, the files (each a path, or a name in the Doom folder) added to
/// `pwads()` for this run and every run it starts.
pub fn take_pwads(args: Vec<String>) -> Result<Vec<String>, String> {
    let mut rest = Vec::new();
    let mut files = pwads();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a != "--pwad" {
            rest.push(a);
            continue;
        }
        let name = it.next().ok_or("--pwad needs a file")?;
        let path = resolve_wad(&name);
        if !path.exists() {
            return Err(format!("--pwad {name}: no such file"));
        }
        files.push(std::fs::canonicalize(&path).unwrap_or(path));
    }
    if !files.is_empty() {
        let joined = std::env::join_paths(&files).map_err(|e| format!("--pwad: {e}"))?;
        // (before anything else runs: the process is one thread still)
        std::env::set_var("EVILBOOM_PWADS", joined);
    }
    Ok(rest)
}

/// What a run's logs are named by: the first PWAD's name (the maps, as a rule; music and the
/// like after), else the IWAD's (`DOOM2`, `MM`).
pub fn wad_tag(iwad: &Path) -> String {
    pwads()
        .first()
        .map_or(iwad, PathBuf::as_path)
        .file_stem()
        .map_or_else(|| "WAD".to_string(), |s| s.to_string_lossy().to_uppercase())
}

/// A short IWAD name ("doom2") or a path, absolute or from the Doom folder.
pub fn resolve_wad(name_or_path: &str) -> PathBuf {
    let lower = name_or_path.to_lowercase();
    if let Some((_, rel)) = IWADS.iter().find(|(n, _)| *n == lower) {
        return doom_dir().join(rel);
    }
    let path = Path::new(name_or_path);
    if path.exists() {
        return path.to_path_buf();
    }
    doom_dir().join(path)
}

/// The engine library's file name on this platform.
pub const ENGINE_LIBRARY: &str = if cfg!(windows) {
    "libdsda-evil.dll"
} else {
    "libdsda-evil.so"
};

/// The program's own folder when it's a release: the engine library beside it (a release is
/// unpacked anywhere, its `config/` and `out/` with it). None in a source checkout, where the
/// program is built into `out/` and the engine into `third_party/`.
fn release_dir() -> Option<&'static Path> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        dir.join(ENGINE_LIBRARY).is_file().then_some(dir)
    })
    .as_deref()
}

/// The project folder: EVILBOOM_ROOT; else a release's own folder; else the checkout this was
/// built from (the folder above `native/`).
pub fn root() -> PathBuf {
    if let Some(r) = env("ROOT") {
        return PathBuf::from(r);
    }
    if let Some(dir) = release_dir() {
        return dir.to_path_buf();
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Where the engine library and its `dsda-doom.wad` are: beside the program in a release, else
/// the engine's build folder in the checkout (`tools/build-engine.sh`).
pub fn engine_dir() -> PathBuf {
    release_dir().map_or_else(
        || {
            root()
                .join("third_party")
                .join("dsda-doom")
                .join("prboom2")
                .join("build")
        },
        Path::to_path_buf,
    )
}

/// The project's out/ folder (caches, logs).
pub fn out_dir() -> PathBuf {
    root().join("out")
}

/// The commit this was built from ("-dirty": with changes not yet committed).
pub const COMMIT: &str = env!("EVIL_COMMIT");

/// This run's stamp for its logs' names: the commit and when it started (UTC), the same for every
/// log of the run.
pub fn run_stamp() -> &'static str {
    static STAMP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STAMP.get_or_init(|| format!("{COMMIT}-{}", utc_now()))
}

/// Now, UTC, as YYYYMMDD-HHMMSS.
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // (days since 1970 to a civil date: Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// A log's kept name: `path` with the run's stamp (and `extra`, a seed, if any) before its
/// extension: out/explore/DOOM_E1M1.txt -> out/explore/DOOM_E1M1.s3.81d5352-20261108-231500.txt.
pub fn stamped(path: &Path, extra: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().to_string());
    let ext = path
        .extension()
        .map_or_else(String::new, |e| format!(".{}", e.to_string_lossy()));
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(".{extra}")
    };
    path.with_file_name(format!("{stem}{extra}.{}{ext}", run_stamp()))
}

/// Write a log twice: at `path` (the latest, written over) and at its kept name (`stamped`).
pub fn write_log(path: &Path, extra: &str, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    std::fs::write(stamped(path, extra), text)
}

/// A log written as it goes, to `path` (the latest) and its kept name both.
pub struct Log {
    files: Vec<std::fs::File>,
}

impl Log {
    pub fn create(path: &Path, extra: &str) -> std::io::Result<Log> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Log {
            files: vec![
                std::fs::File::create(path)?,
                std::fs::File::create(stamped(path, extra))?,
            ],
        })
    }
}

impl std::io::Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for f in &mut self.files {
            f.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.files.iter_mut().try_for_each(std::io::Write::flush)
    }
}
