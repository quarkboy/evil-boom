// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! WAD files: the lump directory, a map's lumps read into `MapData`, wall texture heights.
//! Doom-format maps only (Hexen-format and UDMF ones are refused).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::map::{
    Blockmap, Linedef, MapData, MapSeg, Node, Sector, Sidedef, Subsector, Thing, Vertex,
};

/// Lumps that may follow a map marker, in any of the three map formats.
const MAP_LUMPS: [&str; 16] = [
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS", "REJECT",
    "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ZNODES", "DIALOGUE", "ENDMAP",
];

pub struct Lump {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

pub struct Wad {
    pub path: PathBuf,
    pub data: Vec<u8>,
    pub lumps: Vec<Lump>,
}

fn i32_at(data: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn i16_at(data: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([data[at], data[at + 1]])
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

/// An 8-byte lump or texture name: up to its first nul, upper case.
fn name8(raw: &[u8]) -> String {
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).to_uppercase()
}

impl Wad {
    /// Its levels that a game can start at (named as E1M1 or MAP01 are), in the order it has
    /// them, each once.
    pub fn level_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.lumps {
            let n = l.name.to_uppercase();
            let shaped = (n.len() == 4 && n.starts_with('E') && n.as_bytes()[2] == b'M')
                || (n.len() == 5 && n.starts_with("MAP"));
            if shaped && crate::map::episode_map(&n).is_ok() && !out.contains(&n) {
                out.push(n);
            }
        }
        out
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_bytes(path, data)
    }

    /// A WAD's bytes read (`path`: what it's called by). A file that isn't one, or is cut
    /// short or says more than it holds, is an error, never a panic or a room's worth of
    /// memory asked for on a broken header's word.
    pub fn from_bytes(path: &Path, data: Vec<u8>) -> Result<Self, String> {
        let bad = || format!("{} is not a WAD", path.display());
        if data.len() < 12 || !(data.starts_with(b"IWAD") || data.starts_with(b"PWAD")) {
            return Err(bad());
        }
        let count = usize::try_from(i32_at(&data, 4).ok_or_else(bad)?).map_err(|_| bad())?;
        let dir = usize::try_from(i32_at(&data, 8).ok_or_else(bad)?).map_err(|_| bad())?;
        // (the directory it says it has is all there, before room is made for it)
        let end = count
            .checked_mul(16)
            .and_then(|n| n.checked_add(dir))
            .ok_or_else(bad)?;
        if end > data.len() {
            return Err(format!(
                "{}: its directory ({count} lumps at {dir}) runs past its end",
                path.display()
            ));
        }
        let mut lumps = Vec::with_capacity(count);
        for i in 0..count {
            let at = dir + 16 * i;
            let entry = data.get(at..at + 16).ok_or_else(bad)?;
            let offset = usize::try_from(i32_at(entry, 0).ok_or_else(bad)?).map_err(|_| bad())?;
            let size = usize::try_from(i32_at(entry, 4).ok_or_else(bad)?).map_err(|_| bad())?;
            lumps.push(Lump {
                name: name8(&entry[8..16]),
                offset,
                size,
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            data,
            lumps,
        })
    }

    pub fn read(&self, lump: &Lump) -> &[u8] {
        let end = lump.offset.saturating_add(lump.size).min(self.data.len());
        &self.data[lump.offset.min(end)..end]
    }

    /// Map markers: lumps straight before THINGS (Doom, Hexen) or TEXTMAP (UDMF).
    pub fn map_names(&self) -> Vec<String> {
        self.lumps
            .windows(2)
            .filter(|w| w[1].name == "THINGS" || w[1].name == "TEXTMAP")
            .map(|w| w[0].name.clone())
            .collect()
    }

    /// A map's lumps (later ones win: searched from the end).
    pub fn map_lumps(&self, map_name: &str) -> Result<BTreeMap<String, &[u8]>, String> {
        let map_name = map_name.to_uppercase();
        for i in (0..self.lumps.len()).rev() {
            if self.lumps[i].name == map_name
                && self
                    .lumps
                    .get(i + 1)
                    .is_some_and(|l| l.name == "THINGS" || l.name == "TEXTMAP")
            {
                let mut out = BTreeMap::new();
                for lump in &self.lumps[i + 1..] {
                    if !MAP_LUMPS.contains(&lump.name.as_str()) || out.contains_key(&lump.name) {
                        break;
                    }
                    out.insert(lump.name.clone(), self.read(lump));
                }
                return Ok(out);
            }
        }
        Err(format!(
            "map {map_name} not found in {}",
            self.path.display()
        ))
    }

    /// Wall texture heights from TEXTURE1/TEXTURE2 (for "raise floor by shortest lower
    /// texture").
    pub fn texture_heights(&self) -> BTreeMap<String, i32> {
        let mut out = BTreeMap::new();
        for lump in self
            .lumps
            .iter()
            .filter(|l| l.name == "TEXTURE1" || l.name == "TEXTURE2")
        {
            let data = self.read(lump);
            // (no more than the lump has room to point at, whatever it says)
            let count = i32_at(data, 0)
                .and_then(|c| usize::try_from(c).ok())
                .unwrap_or(0)
                .min(data.len() / 4);
            for i in 0..count {
                let Some(offset) = i32_at(data, 4 + 4 * i).and_then(|o| usize::try_from(o).ok())
                else {
                    continue;
                };
                if offset + 16 > data.len() {
                    continue;
                }
                // name[8], masked (i32), width (i16), height (i16)
                out.insert(
                    name8(&data[offset..offset + 8]),
                    i32::from(i16_at(data, offset + 14)),
                );
            }
        }
        out
    }
}

/// Fixed-size records of `size` bytes.
fn records(data: &[u8], size: usize) -> impl Iterator<Item = &[u8]> {
    data.chunks_exact(size)
}

fn blockmap(data: &[u8]) -> Option<Blockmap> {
    let words = data.len() / 2;
    if words < 4 {
        return None;
    }
    let (x, y) = (i32::from(i16_at(data, 0)), i32::from(i16_at(data, 2)));
    let (columns, rows) = (i16_at(data, 4), i16_at(data, 6));
    if columns <= 0 || rows <= 0 {
        return None;
    }
    let (columns, rows) = (usize::try_from(columns).ok()?, usize::try_from(rows).ok()?);
    let count = columns * rows;
    if words < 4 + count {
        return None;
    }
    let raw: Vec<u16> = Vec::from_fn(words, |i| u16_at(data, 2 * i));
    let mut blocks = Vec::with_capacity(count);
    for &offset in &raw[4..4 + count] {
        // (as ZDoom reads it: each list's first entry skipped, to the 0xFFFF)
        let mut lines = Vec::new();
        let mut i = usize::from(offset) + 1;
        while i < words && raw[i] != 0xFFFF {
            lines.push(usize::from(raw[i]));
            i += 1;
        }
        if i >= words {
            return None; // (ZDoom would build its own)
        }
        blocks.push(lines);
    }
    Some(Blockmap {
        x,
        y,
        columns,
        rows,
        blocks,
    })
}

/// Load a level as the game will play it, with the PWADs (`config::pwads`) over the IWAD: the map
/// from the last of them that has it, else the IWAD's; texture heights from the IWAD and the
/// PWADs before it, then its own.
pub fn load_level(iwad: &Wad, map_name: &str) -> Result<MapData, String> {
    let pwads = crate::config::pwads()
        .iter()
        .map(|p| Wad::open(p))
        .collect::<Result<Vec<Wad>, String>>()?;
    let Some(i) = pwads.iter().rposition(|w| w.map_lumps(map_name).is_ok()) else {
        return load_map(iwad, map_name, &[]);
    };
    let textures: Vec<&Wad> = std::iter::once(iwad).chain(&pwads[..i]).collect();
    load_map(&pwads[i], map_name, &textures)
}

/// Load a map. Texture heights come from `texture_wads` (the IWAD when the map is in a PWAD)
/// and then the map's own WAD, later ones overriding earlier ones.
pub fn load_map(wad: &Wad, map_name: &str, texture_wads: &[&Wad]) -> Result<MapData, String> {
    let lumps = wad.map_lumps(map_name)?;
    if lumps.contains_key("TEXTMAP") {
        return Err(format!("{map_name}: UDMF maps are not supported"));
    }
    if lumps.contains_key("BEHAVIOR") {
        return Err(format!("{map_name}: Hexen-format maps are not supported"));
    }
    let lump = |name: &str| lumps.get(name).copied().unwrap_or(&[]);
    let things = records(lump("THINGS"), 10)
        .map(|r| Thing {
            x: f64::from(i16_at(r, 0)),
            y: f64::from(i16_at(r, 2)),
            angle: i32::from(i16_at(r, 4)),
            kind: u16_at(r, 6),
            flags: u16_at(r, 8),
        })
        .collect();
    let vertexes = records(lump("VERTEXES"), 4)
        .map(|r| Vertex {
            x: f64::from(i16_at(r, 0)),
            y: f64::from(i16_at(r, 2)),
        })
        .collect();
    let linedefs = records(lump("LINEDEFS"), 14)
        .enumerate()
        .map(|(index, r)| Linedef {
            index,
            v1: usize::from(u16_at(r, 0)),
            v2: usize::from(u16_at(r, 2)),
            flags: crate::map::line_flags(u16_at(r, 4)),
            special: u16_at(r, 6),
            tag: u16_at(r, 8),
            front: u16_at(r, 10),
            back: u16_at(r, 12),
        })
        .collect();
    let sidedefs = records(lump("SIDEDEFS"), 30)
        .map(|r| Sidedef {
            x_offset: i32::from(i16_at(r, 0)),
            y_offset: i32::from(i16_at(r, 2)),
            upper: name8(&r[4..12]),
            lower: name8(&r[12..20]),
            middle: name8(&r[20..28]),
            sector: usize::from(u16_at(r, 28)),
        })
        .collect();
    let sectors = records(lump("SECTORS"), 26)
        .enumerate()
        .map(|(index, r)| Sector {
            index,
            floor: f64::from(i16_at(r, 0)),
            ceiling: f64::from(i16_at(r, 2)),
            floor_tex: name8(&r[4..12]),
            ceiling_tex: name8(&r[12..20]),
            light: i32::from(i16_at(r, 20)),
            special: u16_at(r, 22),
            tag: u16_at(r, 24),
        })
        .collect();
    // standard nodes only (extended/GL nodes are left empty)
    let (ssectors, nodes_raw) = (lump("SSECTORS"), lump("NODES"));
    let extended = |d: &[u8]| {
        [b"XNOD", b"ZNOD", b"XGLN", b"ZGLN"]
            .iter()
            .any(|m| d.starts_with(*m))
    };
    let (mut segs, mut subsectors, mut nodes) = (Vec::new(), Vec::new(), Vec::new());
    if !ssectors.is_empty() && !extended(ssectors) && !extended(nodes_raw) {
        segs = records(lump("SEGS"), 12)
            .map(|r| MapSeg {
                v1: usize::from(u16_at(r, 0)),
                v2: usize::from(u16_at(r, 2)),
                linedef: usize::from(u16_at(r, 6)),
                direction: i16_at(r, 8),
            })
            .collect();
        subsectors = records(ssectors, 4)
            .map(|r| Subsector {
                seg_count: usize::from(u16_at(r, 0)),
                first_seg: usize::from(u16_at(r, 2)),
            })
            .collect();
        // x, y, dx, dy, front bbox (4), back bbox (4), front child, back child
        nodes = records(nodes_raw, 28)
            .map(|r| Node {
                x: f64::from(i16_at(r, 0)),
                y: f64::from(i16_at(r, 2)),
                dx: f64::from(i16_at(r, 4)),
                dy: f64::from(i16_at(r, 6)),
                front: u16_at(r, 24),
                back: u16_at(r, 26),
            })
            .collect();
    }
    let mut texture_heights = BTreeMap::new();
    for w in texture_wads.iter().copied().chain(std::iter::once(wad)) {
        texture_heights.extend(w.texture_heights());
    }
    let map = MapData {
        name: map_name.to_uppercase(),
        things,
        vertexes,
        linedefs,
        sidedefs,
        sectors,
        segs,
        subsectors,
        nodes,
        texture_heights,
        blockmap: blockmap(lump("BLOCKMAP")),
    };
    whole(&map)?;
    Ok(map)
}

/// Everything in the map that points at something points at something there: a line's ends
/// and sides, a side's sector, a seg's ends and line, a subsector's segs, a node's children.
/// The rest of the program indexes by them without asking; a map that fails is refused here,
/// in words.
fn whole(m: &MapData) -> Result<(), String> {
    let no = |what: String| Err(format!("{}: {what}: the map is broken", m.name));
    for l in &m.linedefs {
        if l.v1 >= m.vertexes.len() || l.v2 >= m.vertexes.len() {
            return no(format!(
                "line {} ends at a vertex that isn't there",
                l.index
            ));
        }
        for side in [l.front, l.back] {
            if side != crate::map::NO_SIDEDEF && usize::from(side) >= m.sidedefs.len() {
                return no(format!(
                    "line {} has side {side}, which isn't there",
                    l.index
                ));
            }
        }
    }
    if let Some(i) = m.sidedefs.iter().position(|s| s.sector >= m.sectors.len()) {
        return no(format!(
            "side {i} is of sector {}, which isn't there",
            m.sidedefs[i].sector
        ));
    }
    for (i, g) in m.segs.iter().enumerate() {
        if g.v1 >= m.vertexes.len() || g.v2 >= m.vertexes.len() || g.linedef >= m.linedefs.len() {
            return no(format!("seg {i} is of a vertex or a line that isn't there"));
        }
    }
    if let Some(i) = m
        .subsectors
        .iter()
        .position(|s| s.first_seg + s.seg_count > m.segs.len())
    {
        return no(format!("subsector {i} has segs that aren't there"));
    }
    for (i, n) in m.nodes.iter().enumerate() {
        for child in [n.front, n.back] {
            let there = if child & 0x8000 != 0 {
                usize::from(child & 0x7FFF) < m.subsectors.len()
            } else {
                usize::from(child) < m.nodes.len()
            };
            if !there {
                return no(format!("node {i} has a child that isn't there"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{u16s, wad_bytes};

    /// A one-room map: a square of four lines, one sector. `vertex`: the first line's second
    /// end, to point it somewhere that isn't there.
    fn square(vertex: u16, sector: u16) -> Vec<(&'static str, Vec<u8>)> {
        let mut sides = Vec::new();
        for _ in 0..4 {
            sides.extend([0u8; 4]);
            for _ in 0..3 {
                sides.extend(b"-\0\0\0\0\0\0\0");
            }
            sides.extend(sector.to_le_bytes());
        }
        let mut sectors = u16s(&[0, 128]);
        sectors.extend(b"FLOOR4_8CEIL3_5\0");
        sectors.extend(u16s(&[160, 0, 0]));
        let line = |a: u16, b: u16, side: u16| u16s(&[a, b, 1, 0, 0, side, 0xFFFF]);
        vec![
            ("MAP01", Vec::new()),
            ("THINGS", u16s(&[32, 32, 0, 1, 7])),
            (
                "LINEDEFS",
                [
                    line(0, vertex, 0),
                    line(1, 2, 1),
                    line(2, 3, 2),
                    line(3, 0, 3),
                ]
                .concat(),
            ),
            ("SIDEDEFS", sides),
            ("VERTEXES", u16s(&[0, 0, 0, 64, 64, 64, 64, 0])),
            ("SECTORS", sectors),
        ]
    }

    fn opened(data: Vec<u8>) -> Result<Wad, String> {
        Wad::from_bytes(Path::new("test.wad"), data)
    }

    #[test]
    fn a_small_whole_map_reads() {
        let w = opened(wad_bytes(&square(1, 0))).expect("a WAD");
        let m = load_map(&w, "MAP01", &[]).expect("a map");
        assert_eq!(
            (m.linedefs.len(), m.sectors.len(), m.things.len()),
            (4, 1, 1)
        );
        assert_eq!(m.sectors[0].floor_tex, "FLOOR4_8");
    }

    #[test]
    fn what_isnt_a_wad_is_refused() {
        assert!(opened(Vec::new()).is_err());
        assert!(opened(b"IWAD".to_vec()).is_err());
        assert!(opened(b"NOPE\0\0\0\0\0\0\0\0".to_vec()).is_err());
    }

    #[test]
    fn a_directory_past_the_end_is_refused_before_room_is_made_for_it() {
        // (two thousand million lumps, it says: once that was room asked for at once)
        let mut data = b"PWAD".to_vec();
        data.extend(i32::MAX.to_le_bytes());
        data.extend(12i32.to_le_bytes());
        assert!(opened(data)
            .err()
            .is_some_and(|e| e.contains("runs past its end")));
        // (cut short: the directory's last entries gone)
        let mut cut = wad_bytes(&square(1, 0));
        cut.truncate(cut.len() - 20);
        assert!(opened(cut).is_err());
        // (a count or a place below nothing)
        let mut data = b"PWAD".to_vec();
        data.extend((-1i32).to_le_bytes());
        data.extend(12i32.to_le_bytes());
        assert!(opened(data).is_err());
    }

    #[test]
    fn a_lump_past_the_end_reads_as_what_there_is() {
        let mut data = wad_bytes(&[("THING", vec![1, 2, 3, 4])]);
        // (the one lump's size, in its directory entry: far more than the file)
        let dir = data.len() - 16;
        data[dir + 4..dir + 8].copy_from_slice(&i32::MAX.to_le_bytes());
        let w = opened(data).expect("a WAD");
        assert!(w.read(&w.lumps[0]).len() < 64);
    }

    #[test]
    fn a_map_pointing_at_what_isnt_there_is_refused() {
        let w = opened(wad_bytes(&square(9, 0))).expect("a WAD");
        let e = load_map(&w, "MAP01", &[]).err().unwrap_or_default();
        assert!(
            e.contains("line 0 ends at a vertex that isn't there"),
            "{e}"
        );
        let w = opened(wad_bytes(&square(1, 5))).expect("a WAD");
        let e = load_map(&w, "MAP01", &[]).err().unwrap_or_default();
        assert!(e.contains("of sector 5, which isn't there"), "{e}");
    }

    #[test]
    fn a_texture_list_that_says_too_much_is_read_to_its_end() {
        // (a count of two thousand million in eight bytes: read in no time, nothing found)
        let mut lump = i32::MAX.to_le_bytes().to_vec();
        lump.extend(0i32.to_le_bytes());
        let w = opened(wad_bytes(&[("TEXTURE1", lump)])).expect("a WAD");
        assert!(w.texture_heights().is_empty());
    }
}
