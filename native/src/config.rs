//! Where things are on this machine: the game's WADs, the project's own folders.

use std::path::{Path, PathBuf};

const DEFAULT_DOOM_DIR: &str = r"C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom";

/// The classic v1.9 IWADs inside the Steam install, by short name.
const IWADS: [(&str, &str); 4] = [
    ("doom", "base/DOOM.WAD"),
    ("doom2", "base/doom2/DOOM2.WAD"),
    ("tnt", "base/tnt/TNT.WAD"),
    ("plutonia", "base/plutonia/PLUTONIA.WAD"),
];

pub fn doom_dir() -> PathBuf {
    std::env::var_os("EVILDOOM_DOOM_DIR").map_or_else(|| PathBuf::from(DEFAULT_DOOM_DIR), PathBuf::from)
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
        || Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        PathBuf::from,
    )
}

/// The project's out/ folder (caches, logs).
pub fn out_dir() -> PathBuf {
    root().join("out")
}
