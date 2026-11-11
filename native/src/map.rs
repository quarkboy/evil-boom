//! A Doom-format map, as `wad::load_map` reads it.

use std::collections::BTreeMap;

use crate::geom::{Point, Seg};

/// A linedef side that isn't there.
pub const NO_SIDEDEF: u16 = 0xFFFF;
/// A BSP child that is a subsector, not a node.
pub const NF_SUBSECTOR: u16 = 0x8000;
/// Linedef flags (vanilla, Boom, MBF21).
pub const ML_BLOCKING: u16 = 0x0001;
pub const ML_BLOCKMONSTERS: u16 = 0x0002;
pub const ML_TWOSIDED: u16 = 0x0004;
/// (shows as one-sided on the automap)
pub const ML_SECRET: u16 = 0x0020;
/// upper texture drawn down from the ceiling, not up from the far side's
pub const ML_DONTPEGTOP: u16 = 0x0008;
/// lower (and a one-sided wall's) texture drawn up from the floor
pub const ML_DONTPEGBOTTOM: u16 = 0x0010;
pub const ML_SOUNDBLOCK: u16 = 0x0040;
pub const ML_DONTDRAW: u16 = 0x0080;
pub const ML_MAPPED: u16 = 0x0100;
/// (Boom: use passes through to lines behind)
pub const ML_PASSUSE: u16 = 0x0200;
pub const ML_BLOCKPLAYERS: u16 = 0x2000;
/// (set, the rest past the vanilla ones are left over junk, not flags)
pub const ML_RESERVED: u16 = 0x0800;
pub const ML_VANILLA: u16 = 0x01ff;

/// A linedef's flags as the game takes them: with the reserved one set, only the vanilla ones,
/// as DSDA-Doom has it at MBF21 (comp_reservedlineflag): E2M7's crates, flagged 0xfe04, would
/// otherwise block the player.
pub fn line_flags(raw: u16) -> u16 {
    if raw & ML_RESERVED != 0 {
        raw & ML_VANILLA
    } else {
        raw
    }
}

/// Thing flags.
pub const MTF_EASY: u16 = 0x0001;
pub const MTF_NORMAL: u16 = 0x0002;
pub const MTF_HARD: u16 = 0x0004;
pub const MTF_AMBUSH: u16 = 0x0008;
/// (multiplayer only)
pub const MTF_NOTSINGLE: u16 = 0x0010;

#[derive(Clone, Copy, Debug)]
pub struct Thing {
    pub x: f64,
    pub y: f64,
    pub angle: i32,
    pub kind: u16,
    pub flags: u16,
}

impl Thing {
    /// Is it there at this skill (1-5: 1-2 the easy bit, 3 normal, 4-5 hard), in single player?
    pub fn in_skill(&self, skill: i32) -> bool {
        let bit = if skill <= 2 {
            MTF_EASY
        } else if skill == 3 {
            MTF_NORMAL
        } else {
            MTF_HARD
        };
        self.flags & bit != 0 && self.flags & MTF_NOTSINGLE == 0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
}

impl Vertex {
    pub fn point(self) -> Point {
        (self.x, self.y)
    }
}

#[derive(Clone, Debug)]
pub struct Linedef {
    pub index: usize,
    pub v1: usize,
    pub v2: usize,
    pub flags: u16,
    pub special: u16,
    pub tag: u16,
    /// sidedef indices, NO_SIDEDEF if absent
    pub front: u16,
    pub back: u16,
}

impl Linedef {
    pub fn two_sided(&self) -> bool {
        self.back != NO_SIDEDEF
    }

    pub fn blocks_player(&self) -> bool {
        self.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) != 0
    }
}

#[derive(Clone, Debug)]
pub struct Sidedef {
    pub x_offset: i32,
    pub y_offset: i32,
    pub sector: usize,
    /// texture names, "-" for none
    pub upper: String,
    pub lower: String,
    pub middle: String,
}

#[derive(Clone, Debug)]
pub struct Sector {
    pub index: usize,
    /// heights at the start
    pub floor: f64,
    pub ceiling: f64,
    pub floor_tex: String,
    pub ceiling_tex: String,
    pub light: i32,
    pub special: u16,
    pub tag: u16,
}

#[derive(Clone, Debug)]
pub struct MapSeg {
    pub v1: usize,
    pub v2: usize,
    pub linedef: usize,
    /// 0: on the linedef's front side, else its back
    pub direction: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct Subsector {
    pub seg_count: usize,
    pub first_seg: usize,
}

/// A BSP node: a partition line and the children in front of (right of) and behind it.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub front: u16,
    pub back: u16,
}

impl Node {
    /// The child on the side of the partition line that (x, y) is on.
    pub fn child_toward(&self, x: f64, y: f64) -> u16 {
        if self.dx * (y - self.y) - self.dy * (x - self.x) < 0.0 {
            self.front
        } else {
            self.back
        }
    }
}

/// The BLOCKMAP: 128-unit blocks from (x, y), `columns` across; per block (row by row), its
/// lines.
#[derive(Clone, Debug)]
pub struct Blockmap {
    pub x: i32,
    pub y: i32,
    pub columns: usize,
    pub rows: usize,
    pub blocks: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
pub struct MapData {
    pub name: String,
    pub things: Vec<Thing>,
    pub vertexes: Vec<Vertex>,
    pub linedefs: Vec<Linedef>,
    pub sidedefs: Vec<Sidedef>,
    pub sectors: Vec<Sector>,
    pub segs: Vec<MapSeg>,
    pub subsectors: Vec<Subsector>,
    pub nodes: Vec<Node>,
    /// wall texture -> height
    pub texture_heights: BTreeMap<String, i32>,
    pub blockmap: Option<Blockmap>,
}

impl MapData {
    fn side_sector(&self, side: u16) -> Option<usize> {
        (side != NO_SIDEDEF).then(|| self.sidedefs[usize::from(side)].sector)
    }

    pub fn front_sector(&self, l: &Linedef) -> Option<usize> {
        self.side_sector(l.front)
    }

    pub fn back_sector(&self, l: &Linedef) -> Option<usize> {
        self.side_sector(l.back)
    }

    /// Line `li`'s (front, back) sectors.
    pub fn sides(&self, li: usize) -> (Option<usize>, Option<usize>) {
        let l = &self.linedefs[li];
        (self.front_sector(l), self.back_sector(l))
    }

    /// The sectors tagged `tag` (none for tag 0).
    pub fn tagged_sectors(&self, tag: u16) -> Vec<usize> {
        if tag == 0 {
            return Vec::new();
        }
        self.sectors
            .iter()
            .filter(|s| s.tag == tag)
            .map(|s| s.index)
            .collect()
    }

    /// Line `li` as a segment from its first vertex to its second.
    pub fn line_seg(&self, li: usize) -> Seg {
        let l = &self.linedefs[li];
        (self.vertexes[l.v1].point(), self.vertexes[l.v2].point())
    }
}
