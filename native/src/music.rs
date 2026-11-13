// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The game's music as recorded, from the player's own copy of the DOOM + DOOM II re-release on
//! Steam: its `rerelease/extras.wad` has the original soundtrack recorded on a Roland SC-55 Mk2
//! (`O_` lumps) and Andrew Hulshult's IDKFA (`H_`), Ogg Vorbis. For an IWAD, a music WAD is made
//! in out/music/ with each song's lump (`D_...`) the recording of it, loaded before any PWAD (a
//! PWAD's own songs stay its own). Nothing of the re-release is copied anywhere else. Songs
//! without a recording play through the MIDI synth (OPL, or a SoundFont).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::config::{doom_dir, env, out_dir};

/// Which recordings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recordings {
    /// the original soundtrack, recorded on a Roland SC-55 Mk2
    Sc55,
    /// Andrew Hulshult's IDKFA
    Idkfa,
}

impl Recordings {
    pub fn parse(s: &str) -> Option<Recordings> {
        match s {
            "sc55" => Some(Recordings::Sc55),
            "idkfa" => Some(Recordings::Idkfa),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Recordings::Sc55 => "sc55",
            Recordings::Idkfa => "idkfa",
        }
    }

    /// Its lumps' prefix in extras.wad.
    fn prefix(self) -> &'static str {
        match self {
            Recordings::Sc55 => "O_",
            Recordings::Idkfa => "H_",
        }
    }
}

include!("music_songs.rs");

/// The re-release's folder, if there's one with extras.wad: EVILBOOM_RERELEASE_DIR; beside the
/// WADs' base/ (Steam's own folder, as on Windows); Steam's install on Linux; Windows' Steam under
/// WSL (the WADs copied out of it, as BUILDING.md has it).
pub fn rerelease_dir() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = env("RERELEASE_DIR")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    places.push(doom_dir().join("rerelease"));
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for steam in [".local/share/Steam", ".steam/steam"] {
            places.push(
                home.join(steam)
                    .join(crate::config::STEAM_DOOM)
                    .join("rerelease"),
            );
        }
    }
    if cfg!(unix) {
        places.push(
            Path::new("/mnt/c/Program Files (x86)/Steam")
                .join(crate::config::STEAM_DOOM)
                .join("rerelease"),
        );
    }
    places.into_iter().find(|d| d.join("extras.wad").is_file())
}

/// The IWAD's short name ("doom", "doom2", "tnt", "plutonia") by its file's (DOOM1.WAD, the
/// shareware's, as DOOM's: its songs are some of DOOM's).
fn iwad_kind(iwad: &Path) -> Option<&'static str> {
    let stem = iwad.file_stem()?.to_string_lossy().to_ascii_lowercase();
    match stem.as_str() {
        "doom" | "doom1" | "doomu" => Some("doom"),
        "doom2" => Some("doom2"),
        "tnt" => Some("tnt"),
        "plutonia" => Some("plutonia"),
        _ => None,
    }
}

/// A WAD's directory: (name, offset, size).
fn directory(f: &mut std::fs::File) -> std::io::Result<Vec<(String, u32, u32)>> {
    let mut head = [0u8; 12];
    f.read_exact(&mut head)?;
    let count = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let at = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    f.seek(SeekFrom::Start(u64::from(at)))?;
    let mut dir = vec![0u8; 16 * count];
    f.read_exact(&mut dir)?;
    Ok(dir
        .chunks(16)
        .map(|e| {
            let n = |i: usize| u32::from_le_bytes([e[i], e[i + 1], e[i + 2], e[i + 3]]);
            let name: String = e[8..16]
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| char::from(c))
                .collect();
            (name.to_ascii_uppercase(), n(0), n(4))
        })
        .collect())
}

/// The music WAD for `iwad` with `kind`'s recordings, made in out/music/ if it isn't there or is
/// older than extras.wad; none if there's no re-release, the IWAD isn't one it has, or none of
/// its songs is recorded.
pub fn music_wad(iwad: &Path, kind: Recordings) -> Result<Option<PathBuf>, String> {
    let Some(dir) = rerelease_dir() else {
        return Ok(None);
    };
    let Some(songs) = iwad_kind(iwad)
        .and_then(|k| SONGS.iter().find(|(n, _)| *n == k))
        .map(|(_, s)| *s)
    else {
        return Ok(None);
    };
    let extras = dir.join("extras.wad");
    let out = out_dir().join("music").join(format!(
        "{}-{}.wad",
        kind.name(),
        iwad_kind(iwad).unwrap_or("iwad")
    ));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if out.is_file() && modified(&out) >= modified(&extras) {
        return Ok(Some(out));
    }
    let err = |e: std::io::Error| format!("{}: {e}", extras.display());
    let mut f = std::fs::File::open(&extras).map_err(err)?;
    let lumps = directory(&mut f).map_err(err)?;
    // (each song's recording, read once however many songs it plays for)
    let mut wanted: Vec<(&str, (u32, u32))> = Vec::new();
    for (song, rec) in songs {
        let name = format!("{}{rec}", kind.prefix());
        if let Some((_, at, size)) = lumps.iter().find(|(n, _, _)| *n == name) {
            wanted.push((song, (*at, *size)));
        }
    }
    if wanted.is_empty() {
        return Ok(None);
    }
    let mut data: Vec<u8> = Vec::new();
    let mut entries: Vec<(String, u32, u32)> = Vec::new();
    let mut read: Vec<((u32, u32), u32)> = Vec::new();
    for (song, (at, size)) in &wanted {
        let start = match read.iter().find(|(r, _)| *r == (*at, *size)) {
            Some((_, s)) => *s,
            None => {
                let s = 12 + data.len() as u32;
                f.seek(SeekFrom::Start(u64::from(*at))).map_err(err)?;
                let mut buf = vec![0u8; *size as usize];
                f.read_exact(&mut buf).map_err(err)?;
                data.extend_from_slice(&buf);
                read.push(((*at, *size), s));
                s
            }
        };
        entries.push((format!("D_{song}"), start, *size));
    }
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    // (written aside and moved into place: a half-written one is never taken for made)
    let part = out.with_extension("wad.part");
    let mut w = std::io::BufWriter::new(std::fs::File::create(&part).map_err(|e| e.to_string())?);
    let dir_at = 12 + data.len() as u32;
    let mut bytes = Vec::with_capacity(12 + data.len() + 16 * entries.len());
    bytes.extend_from_slice(b"PWAD");
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&dir_at.to_le_bytes());
    bytes.extend_from_slice(&data);
    for (name, at, size) in &entries {
        bytes.extend_from_slice(&at.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        let mut n = [0u8; 8];
        n[..name.len().min(8)].copy_from_slice(&name.as_bytes()[..name.len().min(8)]);
        bytes.extend_from_slice(&n);
    }
    w.write_all(&bytes).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())?;
    drop(w);
    std::fs::rename(&part, &out).map_err(|e| e.to_string())?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every recording the table names is in the re-release (SC-55), and a music WAD made from it
    /// has each song as an Ogg stream. Skipped without a re-release.
    #[test]
    fn songs_and_music_wads() {
        let Some(dir) = rerelease_dir() else {
            return;
        };
        let mut f = std::fs::File::open(dir.join("extras.wad")).expect("extras.wad");
        let lumps = directory(&mut f).expect("its directory");
        for (iwad, songs) in SONGS {
            for (song, rec) in *songs {
                assert!(
                    lumps.iter().any(|(n, _, _)| *n == format!("O_{rec}")),
                    "{iwad} {song}: no O_{rec}"
                );
            }
        }
        for (iwad, file) in [("doom", "DOOM.WAD"), ("doom2", "DOOM2.WAD")] {
            let made = music_wad(Path::new(file), Recordings::Sc55)
                .expect("made")
                .expect("a WAD");
            let mut w = std::fs::File::open(&made).expect("the music WAD");
            let dir = directory(&mut w).expect("its directory");
            let songs = SONGS
                .iter()
                .find(|(n, _)| *n == iwad)
                .map_or(0, |(_, s)| s.len());
            assert_eq!(dir.len(), songs, "{iwad}: every song");
            for (name, at, _) in &dir {
                let mut magic = [0u8; 4];
                w.seek(SeekFrom::Start(u64::from(*at))).expect("seek");
                w.read_exact(&mut magic).expect("read");
                assert_eq!(&magic, b"OggS", "{iwad} {name}");
            }
        }
    }
}
