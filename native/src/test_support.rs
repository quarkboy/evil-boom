// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Levels made in memory, for tests that must run where no IWAD is (CI): a WAD's bytes from a
//! list of lumps, and a one-room level with whatever its walls are to be. A room is one convex
//! sector, so one subsector and no BSP nodes, which the mesh takes as it is. Not used by the
//! program itself.

/// A WAD of these lumps (name, bytes), as bytes.
pub fn wad_bytes(lumps: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut data = b"PWAD".to_vec();
    data.extend(i32::try_from(lumps.len()).unwrap_or(0).to_le_bytes());
    data.extend([0u8; 4]); // (the directory's place, set below)
    let mut dir = Vec::new();
    for (name, bytes) in lumps {
        dir.extend(i32::try_from(data.len()).unwrap_or(0).to_le_bytes());
        dir.extend(i32::try_from(bytes.len()).unwrap_or(0).to_le_bytes());
        let mut n = name.as_bytes().to_vec();
        n.resize(8, 0);
        dir.extend(n);
        data.extend(bytes);
    }
    let at = i32::try_from(data.len()).unwrap_or(0).to_le_bytes();
    data[8..12].copy_from_slice(&at);
    data.extend(dir);
    data
}

/// Shorts as a lump has them.
pub fn u16s(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn name8(name: &str) -> Vec<u8> {
    let mut n = name.as_bytes().to_vec();
    n.resize(8, 0);
    n
}

/// A wall of a room: the line's special and tag, and the texture on it.
#[derive(Clone, Copy)]
pub struct Wall {
    pub special: u16,
    pub tag: u16,
    pub texture: &'static str,
}

impl Wall {
    pub const PLAIN: Wall = Wall {
        special: 0,
        tag: 0,
        texture: "STARTAN3",
    };
    /// (special 11: a switch that ends the level)
    pub const EXIT_SWITCH: Wall = Wall {
        special: 11,
        tag: 0,
        texture: "SW1EXIT",
    };
}

/// A square room `size` across with its corner at the origin, floor 0 and ceiling 128, the
/// player started in its middle, as the lumps of MAP01. Its four walls in order: west, north,
/// east, south (each a one-sided line facing in).
pub fn room(size: i16, walls: [Wall; 4]) -> Vec<(&'static str, Vec<u8>)> {
    let s = size as u16;
    let half = (size / 2) as u16;
    // (clockwise from the south-west corner, so each line's right side, its front, is inside)
    let vertexes = u16s(&[0, 0, 0, s, s, s, s, 0]);
    let mut lines = Vec::new();
    let mut sides = Vec::new();
    let mut segs = Vec::new();
    for (i, w) in walls.iter().enumerate() {
        let (a, b) = (i as u16, ((i + 1) % 4) as u16);
        // v1, v2, flags (1: impassable), special, tag, front side, back side
        lines.extend(u16s(&[a, b, 1, w.special, w.tag, i as u16, 0xFFFF]));
        sides.extend([0u8; 4]);
        sides.extend(name8("-"));
        sides.extend(name8("-"));
        sides.extend(name8(w.texture));
        sides.extend(0u16.to_le_bytes());
        // v1, v2, angle, line, side (0: the line's front), offset
        let angle: u16 = [0x4000, 0, 0xC000, 0x8000][i];
        segs.extend(u16s(&[a, b, angle, i as u16, 0, 0]));
    }
    let mut sectors = u16s(&[0, 128]);
    sectors.extend(name8("FLOOR4_8"));
    sectors.extend(name8("CEIL3_5"));
    sectors.extend(u16s(&[160, 0, 0]));
    vec![
        ("MAP01", Vec::new()),
        // x, y, angle, type (1: the player's start), flags (every skill)
        ("THINGS", u16s(&[half, half, 0, 1, 7])),
        ("LINEDEFS", lines),
        ("SIDEDEFS", sides),
        ("VERTEXES", vertexes),
        ("SEGS", segs),
        // segs, the first of them
        ("SSECTORS", u16s(&[4, 0])),
        ("SECTORS", sectors),
    ]
}
