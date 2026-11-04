//! WAD container and Doom-format map lumps (evildoom.wad.wad / mapdata / textures).

use std::collections::HashMap;

const MAP_LUMPS: [&str; 16] = [
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS",
    "REJECT", "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ZNODES", "DIALOGUE", "ENDMAP",
];
pub const NO_SIDEDEF: u16 = 0xFFFF;
pub const NF_SUBSECTOR: u16 = 0x8000;

struct Lump {
    name: String,
    offset: usize,
    size: usize,
}

pub struct Wad {
    data: Vec<u8>,
    lumps: Vec<Lump>,
}

fn i16_at(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}
fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn i32_at(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn name_at(d: &[u8], o: usize) -> String {
    let raw = &d[o..o + 8];
    let end = raw.iter().position(|&c| c == 0).unwrap_or(8);
    String::from_utf8_lossy(&raw[..end]).to_uppercase()
}

impl Wad {
    pub fn open(path: &str) -> Result<Wad, String> {
        let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        if data.len() < 12 || (&data[0..4] != b"IWAD" && &data[0..4] != b"PWAD") {
            return Err(format!("{path} is not a WAD"));
        }
        let count = i32_at(&data, 4) as usize;
        let dir = i32_at(&data, 8) as usize;
        let mut lumps = Vec::with_capacity(count);
        for i in 0..count {
            let o = dir + 16 * i;
            lumps.push(Lump {
                offset: i32_at(&data, o) as usize,
                size: i32_at(&data, o + 4) as usize,
                name: name_at(&data, o + 8),
            });
        }
        Ok(Wad { data, lumps })
    }

    fn read(&self, l: &Lump) -> &[u8] {
        &self.data[l.offset..l.offset + l.size]
    }

    /// Wad.map_lumps: the lumps of a map; later ones win, so search from the end.
    pub fn map_lumps(&self, map_name: &str) -> Result<HashMap<String, &[u8]>, String> {
        let name = map_name.to_uppercase();
        for i in (0..self.lumps.len()).rev() {
            if self.lumps[i].name == name
                && i + 1 < self.lumps.len()
                && (self.lumps[i + 1].name == "THINGS" || self.lumps[i + 1].name == "TEXTMAP")
            {
                let mut out = HashMap::new();
                for l in &self.lumps[i + 1..] {
                    if !MAP_LUMPS.contains(&l.name.as_str()) || out.contains_key(&l.name) {
                        break;
                    }
                    out.insert(l.name.clone(), self.read(l));
                }
                return Ok(out);
            }
        }
        Err(format!("map {name} not found"))
    }

    /// textures.texture_heights
    pub fn texture_heights(&self) -> HashMap<String, i32> {
        let mut out = HashMap::new();
        for l in &self.lumps {
            if l.name != "TEXTURE1" && l.name != "TEXTURE2" {
                continue;
            }
            let d = self.read(l);
            let count = i32_at(d, 0) as usize;
            for i in 0..count {
                let off = i32_at(d, 4 + 4 * i) as usize;
                out.insert(name_at(d, off), i16_at(d, off + 14) as i32);
            }
        }
        out
    }
}

#[derive(Clone)]
pub struct Thing {
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub kind: i32,
    pub flags: i32,
}
impl Thing {
    pub fn in_skill(&self, skill: i32) -> bool {
        let bit = if skill <= 2 { 0x1 } else if skill == 3 { 0x2 } else { 0x4 };
        self.flags & bit != 0 && self.flags & 0x10 == 0
    }
}
#[derive(Clone, Copy)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone)]
pub struct Linedef {
    pub v1: usize,
    pub v2: usize,
    pub flags: u16,
    pub special: u16,
    pub tag: u16,
    pub front: u16,
    pub back: u16,
}
impl Linedef {
    pub fn two_sided(&self) -> bool {
        self.back != NO_SIDEDEF
    }
}
#[derive(Clone)]
pub struct Sidedef {
    pub upper: String,
    pub lower: String,
    pub middle: String,
    pub sector: usize,
}
#[derive(Clone)]
pub struct Sector {
    pub floor: i32,
    pub ceiling: i32,
    pub floor_tex: String,
    pub ceiling_tex: String,
    pub light: i32,
    pub special: i32,
    pub tag: i32,
}
#[derive(Clone)]
pub struct Seg {
    pub v1: usize,
    pub v2: usize,
    pub linedef: usize,
    pub direction: i16,
}
#[derive(Clone, Copy)]
pub struct Subsector {
    pub seg_count: usize,
    pub first_seg: usize,
}
#[derive(Clone, Copy)]
pub struct Node {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub front: u16,
    pub back: u16,
}

pub struct MapData {
    pub things: Vec<Thing>,
    pub vertexes: Vec<Vertex>,
    pub linedefs: Vec<Linedef>,
    pub sidedefs: Vec<Sidedef>,
    pub sectors: Vec<Sector>,
    pub segs: Vec<Seg>,
    pub subsectors: Vec<Subsector>,
    pub nodes: Vec<Node>,
    pub texture_heights: HashMap<String, i32>,
}

impl MapData {
    pub fn front_sector(&self, l: &Linedef) -> Option<usize> {
        (l.front != NO_SIDEDEF).then(|| self.sidedefs[l.front as usize].sector)
    }
    pub fn back_sector(&self, l: &Linedef) -> Option<usize> {
        (l.back != NO_SIDEDEF).then(|| self.sidedefs[l.back as usize].sector)
    }
}

fn records(d: &[u8], size: usize) -> impl Iterator<Item = usize> {
    (0..d.len() / size).map(move |i| i * size)
}

/// mapdata.load_map (texture sizes from `texture_wads` then the map's own WAD).
pub fn load_map(wad: &Wad, map_name: &str, texture_wads: &[&Wad]) -> Result<MapData, String> {
    let lumps = wad.map_lumps(map_name)?;
    if lumps.contains_key("TEXTMAP") {
        return Err(format!("{map_name}: UDMF maps are not supported yet"));
    }
    if lumps.contains_key("BEHAVIOR") {
        return Err(format!("{map_name}: Hexen-format maps are not supported yet"));
    }
    let get = |n: &str| lumps.get(n).copied().unwrap_or(&[]);
    let d = get("THINGS");
    let things = records(d, 10)
        .map(|o| Thing {
            x: i16_at(d, o) as i32,
            y: i16_at(d, o + 2) as i32,
            angle: i16_at(d, o + 4) as i32,
            kind: u16_at(d, o + 6) as i32,
            flags: u16_at(d, o + 8) as i32,
        })
        .collect();
    let d = get("VERTEXES");
    let vertexes = records(d, 4)
        .map(|o| Vertex { x: i16_at(d, o) as f64, y: i16_at(d, o + 2) as f64 })
        .collect();
    let d = get("LINEDEFS");
    let linedefs = records(d, 14)
        .map(|o| Linedef {
            v1: u16_at(d, o) as usize,
            v2: u16_at(d, o + 2) as usize,
            flags: u16_at(d, o + 4),
            special: u16_at(d, o + 6),
            tag: u16_at(d, o + 8),
            front: u16_at(d, o + 10),
            back: u16_at(d, o + 12),
        })
        .collect();
    let d = get("SIDEDEFS");
    let sidedefs = records(d, 30)
        .map(|o| Sidedef {
            upper: name_at(d, o + 4),
            lower: name_at(d, o + 12),
            middle: name_at(d, o + 20),
            sector: u16_at(d, o + 28) as usize,
        })
        .collect();
    let d = get("SECTORS");
    let sectors = records(d, 26)
        .map(|o| Sector {
            floor: i16_at(d, o) as i32,
            ceiling: i16_at(d, o + 2) as i32,
            floor_tex: name_at(d, o + 4),
            ceiling_tex: name_at(d, o + 12),
            light: i16_at(d, o + 20) as i32,
            special: u16_at(d, o + 22) as i32,
            tag: u16_at(d, o + 24) as i32,
        })
        .collect();
    let (mut segs, mut subsectors, mut nodes) = (Vec::new(), Vec::new(), Vec::new());
    let ss = get("SSECTORS");
    let nd = get("NODES");
    let extended = |b: &[u8]| b.len() >= 4 && [&b"XNOD"[..], b"ZNOD", b"XGLN", b"ZGLN"].contains(&&b[..4]);
    if !ss.is_empty() && !extended(ss) && !extended(nd) {
        let d = get("SEGS");
        segs = records(d, 12)
            .map(|o| Seg {
                v1: u16_at(d, o) as usize,
                v2: u16_at(d, o + 2) as usize,
                linedef: u16_at(d, o + 6) as usize,
                direction: i16_at(d, o + 8),
            })
            .collect();
        subsectors = records(ss, 4)
            .map(|o| Subsector { seg_count: u16_at(ss, o) as usize, first_seg: u16_at(ss, o + 2) as usize })
            .collect();
        nodes = records(nd, 28)
            .map(|o| Node {
                x: i16_at(nd, o) as f64,
                y: i16_at(nd, o + 2) as f64,
                dx: i16_at(nd, o + 4) as f64,
                dy: i16_at(nd, o + 6) as f64,
                front: u16_at(nd, o + 24),
                back: u16_at(nd, o + 26),
            })
            .collect();
    }
    let mut texture_heights = HashMap::new();
    for w in texture_wads.iter().copied().chain(std::iter::once(wad)) {
        texture_heights.extend(w.texture_heights());
    }
    Ok(MapData { things, vertexes, linedefs, sidedefs, sectors, segs, subsectors, nodes, texture_heights })
}

impl MapData {
    /// The parts the navmesh needs, handed over from Python's MapData (no WAD file needed).
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        verts: Vec<(f64, f64)>,
        lines: Vec<(usize, usize, u16, u16, u16, u16, u16)>,
        side_sectors: Vec<usize>,
        segs: Vec<(usize, usize, usize, i16)>,
        subsectors: Vec<(usize, usize)>,
        nodes: Vec<(f64, f64, f64, f64, u16, u16)>,
    ) -> MapData {
        MapData {
            things: Vec::new(),
            vertexes: verts.into_iter().map(|(x, y)| Vertex { x, y }).collect(),
            linedefs: lines.into_iter().map(|(v1, v2, flags, special, tag, front, back)| Linedef { v1, v2, flags, special, tag, front, back }).collect(),
            sidedefs: side_sectors.into_iter().map(|sector| Sidedef { upper: String::new(), lower: String::new(), middle: String::new(), sector }).collect(),
            sectors: Vec::new(),
            segs: segs.into_iter().map(|(v1, v2, linedef, direction)| Seg { v1, v2, linedef, direction }).collect(),
            subsectors: subsectors.into_iter().map(|(seg_count, first_seg)| Subsector { seg_count, first_seg }).collect(),
            nodes: nodes.into_iter().map(|(x, y, dx, dy, front, back)| Node { x, y, dx, dy, front, back }).collect(),
            texture_heights: HashMap::new(),
        }
    }
}
