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
    std::env::var_os("EVILDOOM_DOOM_DIR").map_or_else(default_doom_dir, PathBuf::from)
}

/// Windows: Steam's own folder. Linux: ~/.local/share/evildoom, the Steam folder's base/ copied
/// there (under WSL, reading through /mnt/c is slow).
fn default_doom_dir() -> PathBuf {
    if cfg!(windows) {
        return Path::new(r"C:\Program Files (x86)\Steam").join(STEAM_DOOM);
    }
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".local/share/evildoom")
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

/// The project folder (EVILDOOM_ROOT, else the folder above the one this was built in).
pub fn root() -> PathBuf {
    std::env::var_os("EVILDOOM_ROOT").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        },
        PathBuf::from,
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
