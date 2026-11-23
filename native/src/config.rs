// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Where things are on this machine: the game's WADs, the project's own folders.

use std::path::{Path, PathBuf};

/// Steam's Ultimate Doom folder (base/ holds the IWADs), inside Steam's library.
pub const STEAM_DOOM: &str = "steamapps/common/Ultimate Doom";

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
        crate::dsda::set_env("EVILBOOM_PWADS", Some(joined.as_ref()));
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

/// Where the SoundFonts for the game's music are (soundfonts/: the player's own).
pub fn soundfonts_dir() -> PathBuf {
    root().join("soundfonts")
}

/// The SoundFonts there (.sf2, .sf3), by file name, sorted.
pub fn soundfonts() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(soundfonts_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l.ends_with(".sf2") || l.ends_with(".sf3")
        })
        .collect();
    names.sort();
    names
}

/// The SoundFont the MIDI plays with unless another's asked for (or OPL): soundfonts/SC-55.sf2, if
/// it's there (Patch93's SC-55 SoundFont, put there by the player: see soundfonts/README.md).
pub const DEFAULT_SOUNDFONT: &str = "SC-55.sf2";

/// The default SoundFont, if it's there; none: the OPL synth.
pub fn default_soundfont() -> Option<PathBuf> {
    Some(soundfonts_dir().join(DEFAULT_SOUNDFONT)).filter(|p| p.is_file())
}

/// A SoundFont by its name in soundfonts/, or by its path.
pub fn resolve_soundfont(name_or_path: &str) -> Result<PathBuf, String> {
    let here = soundfonts_dir().join(name_or_path);
    let path = if here.is_file() {
        here
    } else {
        PathBuf::from(name_or_path)
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "no SoundFont {name_or_path:?} (in {}: {})",
            soundfonts_dir().display(),
            soundfonts().join(", ")
        ))
    }
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
/// extension: out/explore/DOOM_E1M1.txt -> out/explore/DOOM_E1M1.s3.704a9a3-20261108-231500.txt.
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

/// How much of a watch's log is kept (`watch --log`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    /// none written
    Off,
    /// what Evil's told and asked, what she does, the Neuro layer, levels and deaths: a sixth
    /// of the whole. The newest `LOGS_KEPT` kept, the rest cleared away
    Brief,
    /// all of it, the fight's reasoning, the explorer's choices and the route follower's steps
    /// too: for tracing a run. Every one kept
    Full,
}

/// at `LogLevel::Brief`, this many of a log's dated copies are kept
const LOGS_KEPT: usize = 20;

impl LogLevel {
    pub fn named(word: &str) -> Option<LogLevel> {
        Some(match word {
            "off" | "none" => LogLevel::Off,
            "brief" => LogLevel::Brief,
            "full" | "all" => LogLevel::Full,
            _ => return None,
        })
    }

    /// The level unless told: all of it from a checkout, where it's what a change is traced
    /// by; brief from a release, whose player has no use for the rest till something's wrong
    /// (then: `--log full`).
    pub fn usual() -> LogLevel {
        if release_dir().is_some() {
            LogLevel::Brief
        } else {
            LogLevel::Full
        }
    }

    /// Is this line of her log (as the agent has it, before its time's put on) one to write?
    /// The explorer's begin "tic N:", the fight's and the follower's are indented; the rest
    /// is what she's told and does, the Neuro layer, a level's or a life's end.
    pub fn keeps(self, line: &str) -> bool {
        match self {
            LogLevel::Off => false,
            LogLevel::Full => true,
            LogLevel::Brief => !line.starts_with("tic ") && !line.starts_with(' '),
        }
    }
}

/// A log written as it goes, to `path` (the latest) and its kept name both.
pub struct Log {
    files: Vec<std::fs::File>,
}

impl Log {
    pub fn create(path: &Path, extra: &str) -> std::io::Result<Log> {
        Log::create_at(path, extra, LogLevel::Full)
    }

    /// ...at a level: none written at `Off`; at `Brief`, the dated copies of this log kept
    /// to the newest few.
    pub fn create_at(path: &Path, extra: &str, level: LogLevel) -> std::io::Result<Log> {
        if level == LogLevel::Off {
            return Ok(Log { files: Vec::new() });
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if level == LogLevel::Brief {
            clear_old_logs(path);
        }
        Ok(Log {
            files: vec![
                std::fs::File::create(path)?,
                std::fs::File::create(stamped(path, extra))?,
            ],
        })
    }
}

/// The dated copies of the log at `path` (live.log: live.*.log beside it), all but the newest
/// `LOGS_KEPT - 1` removed: room for the one about to be made. Only files of that name's own
/// shape, in its own folder.
fn clear_old_logs(path: &Path) {
    let (Some(dir), Some(stem), Some(ext)) = (
        path.parent(),
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|s| s.to_str()),
    ) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let (head, tail) = (format!("{stem}."), format!(".{ext}"));
    let mut dated: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| {
                n.starts_with(&head) && n.ends_with(&tail) && n.len() > head.len() + tail.len()
            })
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dated.sort();
    let keep = LOGS_KEPT.saturating_sub(1);
    let gone = dated.len().saturating_sub(keep);
    for (_, old) in dated.into_iter().take(gone) {
        let _ = std::fs::remove_file(old);
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

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn brief_keeps_what_shes_told_and_does() {
        let b = LogLevel::Brief;
        assert!(b.keeps("<< (!) Exit found: exit-l178 (S 156)."));
        assert!(b.keeps(">> doom_go {\"ref\":\"area-3\"}"));
        assert!(b.keeps("neuro: connected"));
        assert!(b.keeps("--- died: life 2"));
        assert!(!b.keeps("tic 11282: nothing else to do"));
        assert!(!b.keeps("  [fight] tic 60: shooting on the way"));
        assert!(!b.keeps("    [ 544.4s] waiting for the crusher (sector 2) to come up"));
        assert!(LogLevel::Full.keeps("tic 1: x") && !LogLevel::Off.keeps("<< x"));
        assert_eq!(LogLevel::named("brief"), Some(LogLevel::Brief));
    }

    /// At brief, a log's dated copies are kept to the newest few; nothing else in the folder
    /// is touched.
    #[test]
    fn old_logs_cleared_at_brief_only() {
        let dir = std::env::temp_dir().join(format!("evil-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..30 {
            std::fs::write(dir.join(format!("live.old{i:02}.log")), "x").unwrap();
        }
        std::fs::write(dir.join("frames.tsv"), "x").unwrap();
        std::fs::write(dir.join("other.log"), "x").unwrap();
        let count = |d: &Path| std::fs::read_dir(d).unwrap().count();
        // (full: all kept, and the two it writes)
        drop(Log::create_at(&dir.join("live.log"), "a", LogLevel::Full).unwrap());
        assert_eq!(count(&dir), 30 + 2 + 2);
        // (off: nothing written, nothing cleared)
        drop(Log::create_at(&dir.join("live.log"), "b", LogLevel::Off).unwrap());
        assert_eq!(count(&dir), 30 + 2 + 2);
        // (brief: the dated ones down to LOGS_KEPT with its own, the latest, and the others)
        drop(Log::create_at(&dir.join("live.log"), "c", LogLevel::Brief).unwrap());
        assert_eq!(count(&dir), LOGS_KEPT + 1 + 2);
        assert!(dir.join("frames.tsv").exists() && dir.join("other.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
