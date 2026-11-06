//! WAD files: the lump directory, a map's lumps read into `MapData`, wall texture heights.
//! Doom-format maps only (Hexen-format and UDMF ones are refused).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::map::{Blockmap, Linedef, MapData, MapSeg, Node, Sector, Sidedef, Subsector, Thing, Vertex};

/// Lumps that may follow a map marker, in any of the three map formats.
const MAP_LUMPS: [&str; 16] = [
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS",
    "REJECT", "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ZNODES", "DIALOGUE", "ENDMAP",
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
    pub fn open(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bad = || format!("{} is not a WAD", path.display());
        if data.len() < 12 || !(data.starts_with(b"IWAD") || data.starts_with(b"PWAD")) {
            return Err(bad());
        }
        let count = usize::try_from(i32_at(&data, 4).ok_or_else(bad)?).map_err(|_| bad())?;
        let dir = usize::try_from(i32_at(&data, 8).ok_or_else(bad)?).map_err(|_| bad())?;
        let mut lumps = Vec::with_capacity(count);
        for i in 0..count {
            let at = dir + 16 * i;
            let entry = data.get(at..at + 16).ok_or_else(bad)?;
            let offset = usize::try_from(i32_at(entry, 0).ok_or_else(bad)?).map_err(|_| bad())?;
            let size = usize::try_from(i32_at(entry, 4).ok_or_else(bad)?).map_err(|_| bad())?;
            lumps.push(Lump { name: name8(&entry[8..16]), offset, size });
        }
        Ok(Self { path: path.to_path_buf(), data, lumps })
    }

    pub fn read(&self, lump: &Lump) -> &[u8] {
        let end = (lump.offset + lump.size).min(self.data.len());
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
                && self.lumps.get(i + 1).is_some_and(|l| l.name == "THINGS" || l.name == "TEXTMAP")
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
        Err(format!("map {map_name} not found in {}", self.path.display()))
    }

    /// Wall texture heights from TEXTURE1/TEXTURE2 (for "raise floor by shortest lower
    /// texture").
    pub fn texture_heights(&self) -> BTreeMap<String, i32> {
        let mut out = BTreeMap::new();
        for lump in self.lumps.iter().filter(|l| l.name == "TEXTURE1" || l.name == "TEXTURE2") {
            let data = self.read(lump);
            let count = i32_at(data, 0).and_then(|c| usize::try_from(c).ok()).unwrap_or(0);
            for i in 0..count {
                let Some(offset) = i32_at(data, 4 + 4 * i).and_then(|o| usize::try_from(o).ok()) else {
                    continue;
                };
                if offset + 16 > data.len() {
                    continue;
                }
                // name[8], masked (i32), width (i16), height (i16)
                out.insert(name8(&data[offset..offset + 8]), i32::from(i16_at(data, offset + 14)));
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
    let raw: Vec<u16> = (0..words).map(|i| u16_at(data, 2 * i)).collect();
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
    Some(Blockmap { x, y, columns, rows, blocks })
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
        .map(|r| Vertex { x: f64::from(i16_at(r, 0)), y: f64::from(i16_at(r, 2)) })
        .collect();
    let linedefs = records(lump("LINEDEFS"), 14)
        .enumerate()
        .map(|(index, r)| Linedef {
            index,
            v1: usize::from(u16_at(r, 0)),
            v2: usize::from(u16_at(r, 2)),
            flags: u16_at(r, 4),
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
    let extended = |d: &[u8]| [b"XNOD", b"ZNOD", b"XGLN", b"ZGLN"].iter().any(|m| d.starts_with(*m));
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
            .map(|r| Subsector { seg_count: usize::from(u16_at(r, 0)), first_seg: usize::from(u16_at(r, 2)) })
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
    Ok(MapData {
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
    })
}
