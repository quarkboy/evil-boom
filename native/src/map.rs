//! The parts of a Doom-format map the navigation core needs, read straight from Python's
//! `evildoom.wad.mapdata.MapData` (its records' attributes have the same names).

use crate::geom::{Point, Seg};
use pyo3::prelude::*;

/// A linedef side that isn't there.
pub const NO_SIDEDEF: u16 = 0xFFFF;
/// A BSP child that is a subsector, not a node.
pub const NF_SUBSECTOR: u16 = 0x8000;
/// Linedef flags that stop the player.
pub const ML_BLOCKING: u16 = 0x0001;
pub const ML_BLOCKPLAYERS: u16 = 0x2000;

#[derive(Clone, Copy, FromPyObject)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
}

impl Vertex {
    pub fn point(self) -> Point {
        (self.x, self.y)
    }
}

#[derive(Clone, FromPyObject)]
pub struct Linedef {
    pub v1: usize,
    pub v2: usize,
    pub flags: u16,
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

#[derive(Clone, FromPyObject)]
pub struct Sidedef {
    pub sector: usize,
}

#[derive(Clone, FromPyObject)]
pub struct MapSeg {
    pub v1: usize,
    pub v2: usize,
    pub linedef: usize,
    /// 0: on the linedef's front side, else its back
    pub direction: i16,
}

#[derive(Clone, Copy, FromPyObject)]
pub struct Subsector {
    pub seg_count: usize,
    pub first_seg: usize,
}

/// A BSP node: a partition line and the children in front of (right of) and behind it.
#[derive(Clone, Copy, FromPyObject)]
pub struct Node {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    #[pyo3(attribute("front_child"))]
    pub front: u16,
    #[pyo3(attribute("back_child"))]
    pub back: u16,
}

impl Node {
    /// The child on the side of the partition line that (x, y) is on.
    pub fn child_toward(&self, x: f64, y: f64) -> u16 {
        if self.dx * (y - self.y) - self.dy * (x - self.x) < 0.0 { self.front } else { self.back }
    }
}

#[derive(FromPyObject)]
pub struct MapData {
    pub vertexes: Vec<Vertex>,
    pub linedefs: Vec<Linedef>,
    pub sidedefs: Vec<Sidedef>,
    pub segs: Vec<MapSeg>,
    pub subsectors: Vec<Subsector>,
    pub nodes: Vec<Node>,
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

    /// Line `li` as a segment from its first vertex to its second.
    pub fn line_seg(&self, li: usize) -> Seg {
        let l = &self.linedefs[li];
        (self.vertexes[l.v1].point(), self.vertexes[l.v2].point())
    }
}
