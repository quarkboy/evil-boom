// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A Doom-format map, as `wad::load_map` reads it.

use std::collections::BTreeMap;

use crate::geom::{Point, Seg};

/// A linedef side that isn't there.
pub const NO_SIDEDEF: u16 = 0xFFFF;
/// A BSP child that is a subsector, not a node.
pub const NF_SUBSECTOR: u32 = 0x8000_0000;
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
/// as DSDA-Doom has it at MBF21 (comp_reservedlineflag): lines flagged with junk in the high
/// bits would otherwise block the player.
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
    pub front: u32,
    pub back: u32,
}

impl Node {
    /// The child every one of `corners` goes to, if it's the same one for all with something
    /// to spare (a millionth of a unit off the line: far more than the arithmetic's own error),
    /// so that any point among them goes there too (`NodeGrid`).
    pub fn child_of_all(&self, corners: [Point; 4]) -> Option<u32> {
        let spare = 1e-6 * (self.dx.abs() + self.dy.abs());
        let side = |(x, y): Point| self.dx * (y - self.y) - self.dy * (x - self.x);
        if corners.iter().all(|&c| side(c) < -spare) {
            Some(self.front)
        } else if corners.iter().all(|&c| side(c) > spare) {
            Some(self.back)
        } else {
            None
        }
    }

    /// The child on the side of the partition line that (x, y) is on.
    pub fn child_toward(&self, x: f64, y: f64) -> u32 {
        if self.dx * (y - self.y) - self.dy * (x - self.x) < 0.0 {
            self.front
        } else {
            self.back
        }
    }
}

/// Where a search down the BSP for a point may begin: for each square of a grid over the map,
/// the lowest node (or the subsector) that the whole square lies under. Every point of the
/// square goes the same way at each node above it, so a search from there ends where a search
/// from the root does, some thirty nodes sooner on a big map. (By the profiler the search
/// from the root was the costliest single thing in her thinking on DOOM II's own maps, and
/// a quarter of the jump check's time on Sunder's.)
#[derive(Clone, Debug, Default)]
pub struct NodeGrid {
    x0: i32,
    y0: i32,
    cell: i32,
    columns: usize,
    rows: usize,
    start: Vec<u32>,
}

impl NodeGrid {
    /// The grid for the map's nodes. `whole(node, corners)`: the child all four corners of a
    /// square go to at that node, if they all go to the same one, and surely (a side test
    /// that's exact in whole numbers; one in floating point with something to spare).
    pub fn build(m: &MapData, mut whole: impl FnMut(usize, [Point; 4]) -> Option<u32>) -> NodeGrid {
        let Some(root) = m.nodes.len().checked_sub(1) else {
            return NodeGrid::default();
        };
        let (mut lo, mut hi) = (
            (f64::INFINITY, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for v in &m.vertexes {
            lo = (lo.0.min(v.x), lo.1.min(v.y));
            hi = (hi.0.max(v.x), hi.1.max(v.y));
        }
        if !(lo.0 <= hi.0 && lo.1 <= hi.1) {
            return NodeGrid::default();
        }
        let (x0, y0) = (lo.0.floor() as i32, lo.1.floor() as i32);
        let (wide, high) = (hi.0.ceil() as i32 - x0 + 1, hi.1.ceil() as i32 - y0 + 1);
        // (squares of 64, or as much bigger as keeps a huge map's grid to a few megabytes)
        let mut cell = 64;
        while (i64::from(wide) / i64::from(cell) + 1) * (i64::from(high) / i64::from(cell) + 1)
            > 1 << 20
        {
            cell *= 2;
        }
        let (columns, rows) = ((wide / cell + 1) as usize, (high / cell + 1) as usize);
        let mut start = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            for column in 0..columns {
                let (x, y) = (
                    f64::from(x0 + column as i32 * cell),
                    f64::from(y0 + row as i32 * cell),
                );
                let side = f64::from(cell);
                let corners = [(x, y), (x + side, y), (x, y + side), (x + side, y + side)];
                let mut at = root as u32;
                while at & NF_SUBSECTOR == 0 {
                    match whole(at as usize, corners) {
                        Some(child) => at = child,
                        None => break,
                    }
                }
                start.push(at);
            }
        }
        NodeGrid {
            x0,
            y0,
            cell,
            columns,
            rows,
            start,
        }
    }

    /// Where to begin for the point at (x, y) (whole map units, rounded down): None off the
    /// grid, where the search is from the root.
    pub fn start(&self, x: i64, y: i64) -> Option<u32> {
        if self.cell == 0 {
            return None;
        }
        let (dx, dy) = (x - i64::from(self.x0), y - i64::from(self.y0));
        if dx < 0 || dy < 0 {
            return None;
        }
        let (column, row) = (
            (dx / i64::from(self.cell)) as usize,
            (dy / i64::from(self.cell)) as usize,
        );
        (column < self.columns && row < self.rows).then(|| self.start[row * self.columns + column])
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
    /// Where the player starts: the last player 1 start among the things. The game sets her
    /// down at each in turn, and the one she plays is the last; the ones before are left
    /// standing where they were put (voodoo dolls).
    pub fn player_start(&self) -> Option<&Thing> {
        self.things.iter().rev().find(|t| t.kind == 1)
    }

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

/// (episode, map) from a map name: E1M1 or MAP01.
pub fn episode_map(name: &str) -> Result<(i32, i32), String> {
    let n = name.to_uppercase();
    let bad = || format!("not a map name: {name}");
    if let Some(rest) = n.strip_prefix("MAP") {
        return Ok((1, rest.parse().map_err(|_| bad())?));
    }
    let b = n.as_bytes();
    if b.len() == 4
        && b[0] == b'E'
        && b[2] == b'M'
        && b[1].is_ascii_digit()
        && b[3].is_ascii_digit()
    {
        return Ok((i32::from(b[1] - b'0'), i32::from(b[3] - b'0')));
    }
    Err(bad())
}
