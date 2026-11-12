// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A player-movement simulator on the level geometry, for checking jumps without the game.
//!
//! Our own Rust, written to follow the ZDoom family's player movement per tic, in its 16.16 fixed
//! point, function by function (the names are those of GZDoom 3.0, GPL-3.0, unchanged in these
//! since ZDoom 2.8.1; the functions are id Software's, GPL, with ZDoom's changes), but for the
//! push off the ground, which is vanilla's (DSDA-Doom, Evil's engine):
//!   input: moves and the turn rounded to the nearest step, as the DSDA-Doom adapter's ticcmd;
//!     the turn in 1/32768ths of a half turn, moves clamped to 50 and shifted up 8.
//!   P_MovePlayer: turn, then thrust along the new facing (P_ForwardThrust/P_SideThrust,
//!     finesine as R_InitTables makes it); off the ground none (vanilla; ZDoom gives 1/256 of
//!     it, aircontrol, and jumps checked with it land 7-12 short in DSDA-Doom).
//!   P_XYMovement: sub-steps of at most radius - 1, each P_TryMove; blocked by a wall,
//!     FSlide::SlideMove; by a solid thing, y alone, else x alone. Friction on the ground.
//!   P_CheckPosition / PIT_CheckLine: the floor and ceiling over every line the box straddles
//!     (from the blockmap's blocks), blocked by one-sided and blocking lines.
//!   P_TryMove: the opening must be tall enough and a step up at most 24 above z (z itself
//!     isn't raised: P_ZMovement lands the player on the new floor).
//!   P_ZMovement: z += velz; above the floor gravity (twice on the first tic off a ledge); at
//!     or below it, land.
//! Left out (nothing a run-off jump meets): monsters, items, sector specials, slopes, 3D
//! floors, water; solid things are infinitely tall boxes (ZDoom lets a player pass over one).

use super::JumpLab;
use crate::geom::{cells, Point};
use crate::map::{Blockmap, MapData, NF_SUBSECTOR};
use std::sync::OnceLock;

pub type Fixed = i64;
pub const FRAC: Fixed = 1 << 16;
const RADIUS: Fixed = 16 * FRAC;
const HEIGHT: Fixed = 56 * FRAC;
const MAX_STEP: Fixed = 24 * FRAC;
const FRICTION: Fixed = 0xE800;
const STOP_SPEED: Fixed = 0x1000;
const GRAVITY: Fixed = FRAC;
/// ORIG_FRICTION_FACTOR (on the ground: none off it)
const MOVE_FACTOR: Fixed = 2048;
/// MAXPLMOVE: the most a delta move button gives
const MAX_PLAYER_MOVE: f64 = 50.0;
const FINE_ANGLES: usize = 8192;
const ANGLE_TO_FINE: u32 = 19;
const ANG90: u32 = 0x4000_0000;
const ML_TWOSIDED: u16 = 0x0004;
/// MAPBLOCKSHIFT (128-unit blocks, in fixed point) and MAPBTOFRAC
const BLOCK_SHIFT: u32 = 23;
const BLOCK_TO_FRAC: u32 = BLOCK_SHIFT - 16;
const BLOCK_SIZE: Fixed = 128 * FRAC;

pub fn to_fixed(v: f64) -> Fixed {
    (v * FRAC as f64).round() as Fixed
}

pub fn to_units(v: Fixed) -> f64 {
    v as f64 / FRAC as f64
}

/// FixedMul (MulScale16)
fn fixed_mul(a: Fixed, b: Fixed) -> Fixed {
    (a * b) >> 16
}

/// Scale: a * b / c, truncated, as a 32-bit result
fn scale(a: Fixed, b: Fixed, c: Fixed) -> Fixed {
    i64::from((a * b / c) as i32)
}

/// FixedDiv (SafeDivScale16)
fn fixed_div(a: Fixed, b: Fixed) -> Fixed {
    if (a.abs() >> 14) >= b.abs() {
        if (a ^ b) < 0 {
            i64::from(i32::MIN)
        } else {
            i64::from(i32::MAX)
        }
    } else {
        (a << 16) / b
    }
}

/// DMulScale32(a, b, c, d) > 0: the side test of P_PointOnLineSide and R_PointOnSide
fn dmul_scale32_positive(a: Fixed, b: Fixed, c: Fixed, d: Fixed) -> bool {
    ((a * b + c * d) >> 32) > 0
}

/// The fine sine table, as R_InitTables makes it: the first quarter truncated, mirrored into
/// the second (off by one entry, as ZDoom has it), negated for the second half, and the
/// peaks exact.
fn finesine(i: usize) -> Fixed {
    static TABLE: OnceLock<Vec<Fixed>> = OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let quarter = FINE_ANGLES / 4;
        let step = std::f64::consts::TAU / FINE_ANGLES as f64;
        let mut t = vec![0; FINE_ANGLES * 5 / 4];
        for (i, v) in t.iter_mut().take(quarter).enumerate() {
            *v = (FRAC as f64 * (i as f64 * step).sin()) as Fixed;
        }
        for i in 0..quarter {
            t[i + quarter] = t[quarter - 1 - i];
        }
        for i in 0..FINE_ANGLES / 2 {
            t[i + FINE_ANGLES / 2] = -t[i];
        }
        t[quarter] = FRAC;
        t[FINE_ANGLES * 3 / 4] = -FRAC;
        for i in 0..quarter {
            t[FINE_ANGLES + i] = t[i];
        }
        t
    });
    t[i]
}

fn finecosine(i: usize) -> Fixed {
    finesine(i + FINE_ANGLES / 4)
}

pub fn degrees_to_angle(deg: f64) -> u32 {
    let turns = (deg / 360.0).rem_euclid(1.0);
    (turns * 4_294_967_296.0) as u64 as u32
}

pub fn angle_to_degrees(a: u32) -> f64 {
    a as f64 / 4_294_967_296.0 * 360.0
}

/// What the player does in a tic: forward/strafe as the engine's delta buttons (50 = running),
/// turn in degrees (positive: clockwise).
#[derive(Clone, Copy, Default)]
pub struct Controls {
    pub forward: f64,
    pub strafe: f64,
    pub turn: f64,
}

impl Controls {
    /// As the game gets them: (ucmd.yaw, ucmd.forwardmove, ucmd.sidemove), each rounded to
    /// the nearest step as the DSDA-Doom adapter makes its ticcmd (crate::dsda; floored, jumps
    /// checked so run a little slow and land short in DSDA-Doom).
    fn ticcmd(self) -> (i32, Fixed, Fixed) {
        let view = (self.turn / 180.0 * 32768.0).round() as i32;
        let mv = |v: f64| (v.round().clamp(-MAX_PLAYER_MOVE, MAX_PLAYER_MOVE) as Fixed) << 8;
        (view.wrapping_neg(), mv(self.forward), mv(self.strafe))
    }
}

/// A line (line_t), and a divline_t: a point and a direction.
#[derive(Clone, Copy)]
struct SimLine {
    x1: Fixed,
    y1: Fixed,
    x2: Fixed,
    y2: Fixed,
    dx: Fixed,
    dy: Fixed,
    front: Option<usize>,
    back: Option<usize>,
    /// ML_TWOSIDED set
    two_sided_flag: bool,
    blocking: bool,
    teleport: bool,
}

#[derive(Clone, Copy)]
struct Divline {
    x: Fixed,
    y: Fixed,
    dx: Fixed,
    dy: Fixed,
}

impl Divline {
    /// P_PointOnDivlineSide
    fn point_side(&self, x: Fixed, y: Fixed) -> bool {
        dmul_scale32_positive(y - self.y, self.dx, self.x - x, self.dy)
    }

    /// P_InterceptVector(self, other): how far along self (16.16) it meets other.
    fn intercept(&self, other: &Divline) -> Fixed {
        let (v2x, v2y, v2dx, v2dy) = (self.x as f64, self.y as f64, self.dx as f64, self.dy as f64);
        let (v1x, v1y, v1dx, v1dy) = (
            other.x as f64,
            other.y as f64,
            other.dx as f64,
            other.dy as f64,
        );
        let den = v1dy * v2dx - v1dx * v2dy;
        if den == 0.0 {
            return 0;
        }
        let num = (v1x - v2x) * v1dy + (v2y - v1y) * v1dx;
        // FLOAT2FIXED (xs_Fix: round to nearest)
        i64::from((num / den * FRAC as f64).round_ties_even() as i32)
    }
}

impl SimLine {
    /// P_PointOnLineSide: true on the back.
    fn point_side(&self, x: Fixed, y: Fixed) -> bool {
        self.divline().point_side(x, y)
    }

    /// P_MakeDivline
    fn divline(&self) -> Divline {
        Divline {
            x: self.x1,
            y: self.y1,
            dx: self.dx,
            dy: self.dy,
        }
    }

    /// FBoundingBox::BoxOnLineSide == -1: the box straddles the line.
    fn box_straddles(&self, b: &BBox) -> bool {
        if self.dx == 0 {
            (b.left < self.x1) != (b.right < self.x1)
        } else if self.dy == 0 {
            (b.top > self.y1) != (b.bottom > self.y1)
        } else if (self.dy ^ self.dx) >= 0 {
            self.point_side(b.right, b.bottom) != self.point_side(b.left, b.top)
        } else {
            self.point_side(b.left, b.bottom) != self.point_side(b.right, b.top)
        }
    }

    /// Outside the line's bounding box (PIT_CheckLine's first test).
    fn bbox_misses(&self, b: &BBox) -> bool {
        b.right <= self.x1.min(self.x2)
            || b.left >= self.x1.max(self.x2)
            || b.top <= self.y1.min(self.y2)
            || b.bottom >= self.y1.max(self.y2)
    }
}

/// The player's box at a spot.
struct BBox {
    left: Fixed,
    right: Fixed,
    bottom: Fixed,
    top: Fixed,
}

impl BBox {
    fn around(x: Fixed, y: Fixed) -> BBox {
        BBox {
            left: x - RADIUS,
            right: x + RADIUS,
            bottom: y - RADIUS,
            top: y + RADIUS,
        }
    }
}

/// A segment in fixed point.
type FixedSeg = ((Fixed, Fixed), (Fixed, Fixed));

/// A BSP node in fixed point (node_t).
struct SimNode {
    x: Fixed,
    y: Fixed,
    dx: Fixed,
    dy: Fixed,
    /// [front, back]
    children: [u16; 2],
}

/// The blockmap in fixed point.
struct Blocks {
    x: Fixed,
    y: Fixed,
    columns: i64,
    rows: i64,
    lines: Vec<Vec<usize>>,
}

impl Blocks {
    fn new(b: &Blockmap) -> Blocks {
        Blocks {
            x: Fixed::from(b.x) << 16,
            y: Fixed::from(b.y) << 16,
            columns: b.columns as i64,
            rows: b.rows as i64,
            lines: b.blocks.clone(),
        }
    }

    fn block(&self, bx: i64, by: i64) -> &[usize] {
        if (0..self.columns).contains(&bx) && (0..self.rows).contains(&by) {
            &self.lines[(by * self.columns + bx) as usize]
        } else {
            &[]
        }
    }
}

/// The geometry the simulator runs on (built once per map).
pub struct World {
    lines: Vec<SimLine>,
    nodes: Vec<SimNode>,
    /// per subsector: its sector
    subsector_sectors: Vec<usize>,
    blocks: Option<Blocks>,
    /// cell -> solid things' box edges
    things: crate::det::HashMap<crate::geom::Cell, Vec<FixedSeg>>,
}

impl World {
    pub fn new(lab: &JumpLab) -> World {
        let m = &lab.map;
        let lines = (0..m.linedefs.len())
            .map(|li| {
                let l = &m.linedefs[li];
                let (a, b) = m.line_seg(li);
                let (x1, y1, x2, y2) = (to_fixed(a.0), to_fixed(a.1), to_fixed(b.0), to_fixed(b.1));
                let (front, back) = m.sides(li);
                SimLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    dx: x2 - x1,
                    dy: y2 - y1,
                    front,
                    back,
                    two_sided_flag: l.flags & ML_TWOSIDED != 0,
                    blocking: l.blocks_player(),
                    teleport: lab.teleports.contains(&li),
                }
            })
            .collect();
        let nodes = m
            .nodes
            .iter()
            .map(|n| SimNode {
                x: to_fixed(n.x),
                y: to_fixed(n.y),
                dx: to_fixed(n.dx),
                dy: to_fixed(n.dy),
                children: [n.front, n.back],
            })
            .collect();
        let things = lab
            .obstacle_grid
            .iter()
            .map(|(c, es)| {
                (
                    *c,
                    es.iter()
                        .map(|&(a, b)| {
                            (
                                (to_fixed(a.0), to_fixed(a.1)),
                                (to_fixed(b.0), to_fixed(b.1)),
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        World {
            lines,
            nodes,
            subsector_sectors: subsector_sectors(m),
            blocks: m.blockmap.as_ref().map(Blocks::new),
            things,
        }
    }

    /// The lines in the blockmap blocks from (minx, miny) to (maxx, maxy) (block numbers), each
    /// once, in FBlockLinesIterator's order; added to `out`.
    fn block_lines(
        &self,
        (minx, miny): (i64, i64),
        (maxx, maxy): (i64, i64),
        out: &mut Vec<usize>,
    ) {
        let Some(b) = &self.blocks else { return };
        for by in miny..=maxy {
            for bx in minx..=maxx {
                for &li in b.block(bx, by) {
                    if !out.contains(&li) {
                        out.push(li);
                    }
                }
            }
        }
    }
}

/// Per subsector, its sector (from its first seg's side).
fn subsector_sectors(m: &MapData) -> Vec<usize> {
    m.subsectors
        .iter()
        .map(|ss| {
            let seg = &m.segs[ss.first_seg];
            let (front, back) = m.sides(seg.linedef);
            if seg.direction == 0 { front } else { back }
                .or(front)
                .unwrap_or(0)
        })
        .collect()
}

/// The result of checking a spot for the box: is it clear of blocking lines and solid
/// things, the floor and ceiling it gets, whether a solid thing is in the way, and the
/// teleport lines it touches.
struct Check {
    clear: bool,
    floor: Fixed,
    ceil: Fixed,
    thing: bool,
    teleports: Vec<usize>,
}

/// The player, in the simulator.
pub struct PlayerSim<'a> {
    lab: &'a JumpLab,
    world: &'a World,
    /// per sector: (floor, ceiling) now
    pub heights: Vec<(Fixed, Fixed)>,
    pub x: Fixed,
    pub y: Fixed,
    pub z: Fixed,
    vx: Fixed,
    vy: Fixed,
    vz: Fixed,
    pub angle: u32,
    floorz: Fixed,
    ceilz: Fixed,
    /// the last P_TryMove was stopped by a solid thing (BlockingMobj)
    blocked_by_thing: bool,
    /// crossed a teleport line from its front: the sim doesn't follow it
    pub teleported: bool,
}

impl<'a> PlayerSim<'a> {
    pub fn new(lab: &'a JumpLab, world: &'a World) -> Self {
        Self::with_heights(lab, world, &lab.initial)
    }

    pub(super) fn with_heights(lab: &'a JumpLab, world: &'a World, heights: &[(f64, f64)]) -> Self {
        let heights = heights
            .iter()
            .map(|&(f, c)| (to_fixed(f), to_fixed(c)))
            .collect();
        PlayerSim {
            lab,
            world,
            heights,
            x: 0,
            y: 0,
            z: 0,
            vx: 0,
            vy: 0,
            vz: 0,
            angle: 0,
            floorz: 0,
            ceilz: 0,
            blocked_by_thing: false,
            teleported: false,
        }
    }

    /// Stand still at (x, y) on the floor facing `angle` (like the "warp" console command).
    pub fn place(&mut self, p: Point, angle: f64) {
        self.x = to_fixed(p.0);
        self.y = to_fixed(p.1);
        self.angle = degrees_to_angle(angle);
        self.vx = 0;
        self.vy = 0;
        self.vz = 0;
        let c = self.check(self.x, self.y);
        self.floorz = c.floor;
        self.ceilz = c.ceil;
        self.z = c.floor;
        self.teleported = false;
    }

    pub fn pos(&self) -> Point {
        (to_units(self.x), to_units(self.y))
    }

    pub fn angle_degrees(&self) -> f64 {
        angle_to_degrees(self.angle)
    }

    pub fn z_units(&self) -> f64 {
        to_units(self.z)
    }

    pub fn velocity(&self) -> Point {
        (to_units(self.vx), to_units(self.vy))
    }

    /// Moving at (vx, vy) units a tic, at height z (for picking up from where the player is).
    pub fn set_motion(&mut self, v: Point, z: f64) {
        self.vx = to_fixed(v.0);
        self.vy = to_fixed(v.1);
        self.z = to_fixed(z);
    }

    pub fn on_floor(&self) -> bool {
        self.z <= self.floorz
    }

    pub fn floorz_units(&self) -> f64 {
        to_units(self.floorz)
    }

    /// Is the box at p clear of blocking lines and solid things, and the floor and ceiling
    /// it would have there (P_CheckPosition, but for the room between them)?
    pub fn fits_at(&self, p: Point) -> (bool, f64, f64) {
        let c = self.check(to_fixed(p.0), to_fixed(p.1));
        (c.clear, to_units(c.floor), to_units(c.ceil))
    }

    pub fn floor_of(&self, s: usize) -> f64 {
        to_units(self.heights[s].0)
    }

    /// One tic: P_MovePlayer, then the actor's P_XYMovement and P_ZMovement.
    pub fn step(&mut self, c: Controls) {
        let (yaw, fm, sm) = c.ticcmd();
        self.angle = self.angle.wrapping_add((yaw as u32) << 16);
        if (fm != 0 || sm != 0) && self.z <= self.floorz {
            let forward = scale(fm, MOVE_FACTOR * 35, 35 << 8);
            let side = scale(sm, MOVE_FACTOR * 35, 35 << 8);
            if forward != 0 {
                self.thrust(self.angle, forward);
            }
            if side != 0 {
                self.thrust(self.angle.wrapping_sub(ANG90), side);
            }
        }
        let oldfloorz = self.floorz;
        self.xy_move(fm != 0 || sm != 0);
        if !self.teleported && (self.vz != 0 || self.z != self.floorz) {
            self.z_move(oldfloorz);
        }
    }

    fn thrust(&mut self, angle: u32, mv: Fixed) {
        let fine = (angle >> ANGLE_TO_FINE) as usize;
        self.vx += fixed_mul(mv, finecosine(fine));
        self.vy += fixed_mul(mv, finesine(fine));
    }

    // --- movement -------------------------------------------------------------------------------

    /// P_XYMovement (for a player: sliding along walls).
    fn xy_move(&mut self, pushing: bool) {
        let (mut xmove, mut ymove) = (self.vx, self.vy);
        if xmove == 0 && ymove == 0 {
            return;
        }
        let maxmove = RADIUS - FRAC;
        let (xspeed, yspeed) = (xmove.abs(), ymove.abs());
        let speed = if xspeed > yspeed { xspeed } else { yspeed };
        let mut steps = if speed > maxmove {
            1 + speed / maxmove
        } else {
            1
        };
        let (mut onestepx, mut onestepy) = (xmove / steps, ymove / steps);
        let (mut startx, mut starty) = (self.x, self.y);
        let totalsteps = steps;
        let mut step = 1;
        while step <= steps {
            let (ptryx, ptryy) = (
                startx + scale(xmove, step, steps),
                starty + scale(ymove, step, steps),
            );
            let (startvx, startvy) = (self.vx, self.vy);
            if !self.try_move(ptryx, ptryy) {
                if self.teleported {
                    return;
                }
                if self.blocked_by_thing {
                    // against another actor: y alone, else x alone
                    if self.try_move(self.x, self.y + onestepy) {
                        self.vx = 0;
                    } else if self.try_move(self.x + onestepx, self.y) {
                        self.vy = 0;
                    } else {
                        self.vx = 0;
                        self.vy = 0;
                    }
                    steps = 0;
                } else if self.vx == startvx && self.vy == startvy {
                    self.slide_move(onestepx, onestepy, totalsteps);
                    if self.vx == 0 && self.vy == 0 {
                        steps = 0;
                    } else {
                        xmove = self.vx;
                        ymove = self.vy;
                        onestepx = xmove / steps;
                        onestepy = ymove / steps;
                        startx = self.x - scale(xmove, step, steps);
                        starty = self.y - scale(ymove, step, steps);
                    }
                } else {
                    steps = 0;
                }
            }
            if self.teleported {
                return;
            }
            step += 1;
        }
        // friction: none in the air
        if self.z > self.floorz {
            return;
        }
        if self.vx > -STOP_SPEED
            && self.vx < STOP_SPEED
            && self.vy > -STOP_SPEED
            && self.vy < STOP_SPEED
            && !pushing
        {
            self.vx = 0;
            self.vy = 0;
        } else {
            self.vx = fixed_mul(self.vx, FRICTION);
            self.vy = fixed_mul(self.vy, FRICTION);
        }
    }

    /// FSlide::SlideMove: trace the move from the box's leading corners, go as far as fits
    /// towards the nearest wall in the way (less a 1/32 fudge), then the rest along it, which
    /// also becomes the velocity; up to two walls, then (or with no wall to slide along) y
    /// alone, else x alone.
    fn slide_move(&mut self, mut tryx: Fixed, mut tryy: Fixed, numsteps: Fixed) {
        for _ in 0..2 {
            let (leadx, trailx) = if tryx > 0 {
                (self.x + RADIUS, self.x - RADIUS)
            } else {
                (self.x - RADIUS, self.x + RADIUS)
            };
            let (leady, traily) = if tryy > 0 {
                (self.y + RADIUS, self.y - RADIUS)
            } else {
                (self.y - RADIUS, self.y + RADIUS)
            };
            let mut best: Option<(Fixed, usize)> = None;
            self.slide_traverse((leadx, leady), (leadx + tryx, leady + tryy), &mut best);
            self.slide_traverse((trailx, leady), (trailx + tryx, leady + tryy), &mut best);
            self.slide_traverse((leadx, traily), (leadx + tryx, traily + tryy), &mut best);
            let Some((frac, line)) = best else { break };
            let frac = frac - FRAC / 32;
            if frac > 0 {
                let (startvx, startvy) = (self.vx, self.vy);
                if !self.try_move(
                    self.x + fixed_mul(tryx, frac),
                    self.y + fixed_mul(tryy, frac),
                ) {
                    break;
                }
                if self.vx != startvx || self.vy != startvy || self.teleported {
                    return;
                }
            }
            let rest = (FRAC - (frac + FRAC / 32)).min(FRAC);
            if rest <= 0 {
                return;
            }
            tryx = fixed_mul(tryx, rest);
            tryy = fixed_mul(tryy, rest);
            let (mx, my) = self.hit_slide_line(line, tryx, tryy);
            self.vx = mx * numsteps;
            self.vy = my * numsteps;
            if self.try_move(self.x + mx, self.y + my) || self.teleported {
                return;
            }
        }
        // stairstep
        if !self.try_move(self.x, self.y + tryy) && !self.teleported {
            self.try_move(self.x + tryx, self.y);
        }
    }

    /// FSlide::HitSlideLine: the move (mx, my) along line `li`.
    fn hit_slide_line(&self, li: usize, mx: Fixed, my: Fixed) -> (Fixed, Fixed) {
        let l = &self.world.lines[li];
        if l.dx == 0 {
            return (0, my);
        }
        if l.dy == 0 {
            return (mx, 0);
        }
        let dll = l.divline();
        let across = Divline {
            x: self.x,
            y: self.y,
            dx: dll.dy,
            dy: -dll.dx,
        };
        let inter1 = dll.intercept(&across);
        let along = Divline {
            dx: mx,
            dy: my,
            ..across
        };
        let inter2 = dll.intercept(&along);
        let inter3 = along.intercept(&dll);
        if inter3 == 0 {
            return (0, 0);
        }
        (
            scale(inter2 - inter1, dll.dx, inter3),
            scale(inter2 - inter1, dll.dy, inter3),
        )
    }

    /// FSlide::SlideTraverse: the first line along the trace that would stop the box (if
    /// nearer than `best`, it becomes `best`).
    fn slide_traverse(
        &self,
        from: (Fixed, Fixed),
        to: (Fixed, Fixed),
        best: &mut Option<(Fixed, usize)>,
    ) {
        for (frac, li) in self.path_traverse(from, to) {
            let l = &self.world.lines[li];
            let blocks = match (l.front, l.back) {
                (Some(f), Some(b)) if l.two_sided_flag => {
                    l.blocking || {
                        // P_LineOpening (with no reference point: the higher floor)
                        let ((ff, fc), (bf, bc)) = (self.heights[f], self.heights[b]);
                        let (top, bottom) = (fc.min(bc), ff.max(bf));
                        top - bottom < HEIGHT || top - self.z < HEIGHT || bottom - self.z > MAX_STEP
                    }
                }
                _ => !l.point_side(self.x, self.y),
            };
            if blocks {
                if best.is_none_or(|(b, _)| frac < b) {
                    *best = Some((frac, li));
                }
                return;
            }
        }
    }

    /// FPathTraverse (lines only): the lines the trace crosses as (how far along, line),
    /// nearest first (ties in the order they were found).
    fn path_traverse(
        &self,
        (mut x1, mut y1): (Fixed, Fixed),
        (x2, y2): (Fixed, Fixed),
    ) -> Vec<(Fixed, usize)> {
        let mut candidates = Vec::new();
        match &self.world.blocks {
            Some(b) => {
                // don't start exactly on a block edge
                if (x1 - b.x) & (BLOCK_SIZE - 1) == 0 {
                    x1 += FRAC;
                }
                if (y1 - b.y) & (BLOCK_SIZE - 1) == 0 {
                    y1 += FRAC;
                }
                for block in walk_blocks(b, (x1, y1), (x2, y2)) {
                    self.world.block_lines(block, block, &mut candidates);
                }
            }
            None => {
                let (ax, ay, bx, by) = (to_units(x1), to_units(y1), to_units(x2), to_units(y2));
                self.grid_lines(
                    ax.min(bx),
                    ay.min(by),
                    ax.max(bx),
                    ay.max(by),
                    &mut candidates,
                );
            }
        }
        let trace = Divline {
            x: x1,
            y: y1,
            dx: x2 - x1,
            dy: y2 - y1,
        };
        let long = trace.dx.abs() > 16 * FRAC || trace.dy.abs() > 16 * FRAC;
        let mut found: Vec<(Fixed, usize)> = candidates
            .into_iter()
            .filter_map(|li| {
                let l = &self.world.lines[li];
                let (s1, s2) = if long {
                    (trace.point_side(l.x1, l.y1), trace.point_side(l.x2, l.y2))
                } else {
                    (
                        l.point_side(trace.x, trace.y),
                        l.point_side(trace.x + trace.dx, trace.y + trace.dy),
                    )
                };
                if s1 == s2 {
                    return None;
                }
                let frac = trace.intercept(&l.divline());
                (0..=FRAC).contains(&frac).then_some((frac, li))
            })
            .collect();
        found.sort_by_key(|&(frac, _)| frac);
        found
    }

    // --- geometry -------------------------------------------------------------------------------

    /// R_PointInSubsector's sector.
    fn sector_at(&self, x: Fixed, y: Fixed) -> Option<usize> {
        let nodes = &self.world.nodes;
        let Some(last) = nodes.len().checked_sub(1) else {
            return self
                .lab
                .mesh
                .poly_at((to_units(x), to_units(y)))
                .map(|p| self.lab.mesh.poly(p).sector);
        };
        let mut child = last as u16;
        while child & NF_SUBSECTOR == 0 {
            let n = &nodes[usize::from(child)];
            child = n.children[usize::from(dmul_scale32_positive(y - n.y, n.dx, n.x - x, n.dy))];
        }
        self.world
            .subsector_sectors
            .get(usize::from(child & !NF_SUBSECTOR))
            .copied()
    }

    /// Lines near the box, from the navmesh's grid (when the map has no blockmap).
    fn grid_lines(&self, x0: f64, y0: f64, x1: f64, y1: f64, out: &mut Vec<usize>) {
        for c in cells(x0, y0, x1, y1) {
            for &li in self.lab.mesh.line_grid.get(&c).into_iter().flatten() {
                if !out.contains(&li) {
                    out.push(li);
                }
            }
        }
    }

    /// The lines PIT_CheckLine sees for the box at (x, y).
    fn lines_near(&self, b: &BBox) -> Vec<usize> {
        let mut out = Vec::new();
        match &self.world.blocks {
            Some(bm) => {
                let lo = (
                    (b.left - bm.x) >> BLOCK_SHIFT,
                    (b.bottom - bm.y) >> BLOCK_SHIFT,
                );
                let hi = (
                    (b.right - bm.x) >> BLOCK_SHIFT,
                    (b.top - bm.y) >> BLOCK_SHIFT,
                );
                self.world.block_lines(lo, hi, &mut out);
            }
            None => self.grid_lines(
                to_units(b.left),
                to_units(b.bottom),
                to_units(b.right),
                to_units(b.top),
                &mut out,
            ),
        }
        out
    }

    /// P_CheckPosition for the box at (x, y): solid things first, then the sector under the
    /// centre, raised/lowered by every two-sided line the box straddles; not clear on a
    /// one-sided or blocking line. (Its last test, the room left between floor and ceiling,
    /// is try_move's.)
    fn check(&self, x: Fixed, y: Fixed) -> Check {
        let blocked = |floor, ceil, thing| Check {
            clear: false,
            floor,
            ceil,
            thing,
            teleports: Vec::new(),
        };
        let Some(sector) = self.sector_at(x, y) else {
            return blocked(0, 0, false);
        };
        let (mut floor, mut ceil) = self.heights[sector];
        let b = BBox::around(x, y);
        let (ux, uy, r) = (to_units(x), to_units(y), to_units(RADIUS));
        for c in cells(ux - r, uy - r, ux + r, uy + r) {
            for &((ax, ay), (bx, by)) in self.world.things.get(&c).into_iter().flatten() {
                if ax.max(bx) > b.left
                    && ax.min(bx) < b.right
                    && ay.max(by) > b.bottom
                    && ay.min(by) < b.top
                {
                    return blocked(floor, ceil, true);
                }
            }
        }
        let mut good = true;
        let mut teleports = Vec::new();
        for li in self.lines_near(&b) {
            let l = &self.world.lines[li];
            if l.bbox_misses(&b) || !l.box_straddles(&b) {
                continue;
            }
            let (Some(f), Some(bk)) = (l.front, l.back) else {
                good = false;
                continue;
            };
            if l.blocking {
                good = false;
                continue;
            }
            // P_LineOpening, from the box's centre: the front floor unless the floors differ
            let ((ff, fc), (bf, bc)) = (self.heights[f], self.heights[bk]);
            let bottom = if (ff - bf).abs() > 256 {
                ff.max(bf)
            } else {
                ff
            };
            ceil = ceil.min(fc.min(bc));
            floor = floor.max(bottom);
            if l.teleport {
                teleports.push(li);
            }
        }
        Check {
            clear: good,
            floor,
            ceil,
            thing: false,
            teleports,
        }
    }

    /// P_TryMove: into (x, y) if the box fits there from where it is (a step of at most 24 up,
    /// room overhead); `blocked_by_thing` says what stopped it.
    fn try_move(&mut self, x: Fixed, y: Fixed) -> bool {
        let c = self.check(x, y);
        self.blocked_by_thing = c.thing;
        if !c.clear
            || c.ceil - c.floor < HEIGHT
            || c.ceil - self.z < HEIGHT
            || c.floor - self.z > MAX_STEP
        {
            return false;
        }
        // spechit: a teleport line fires going from its front to its back
        for &li in &c.teleports {
            let l = &self.world.lines[li];
            if !l.point_side(self.x, self.y) && l.point_side(x, y) {
                self.teleported = true;
            }
        }
        self.x = x;
        self.y = y;
        self.floorz = c.floor;
        self.ceilz = c.ceil;
        true
    }

    /// P_ZMovement
    fn z_move(&mut self, oldfloorz: Fixed) {
        self.z += self.vz;
        if self.z > self.floorz {
            if self.vz == 0 && oldfloorz > self.floorz && self.z == oldfloorz {
                self.vz -= 2 * GRAVITY;
            } else {
                self.vz -= GRAVITY;
            }
        }
        if self.z <= self.floorz {
            self.z = self.floorz;
            if self.vz < 0 {
                self.vz = 0;
            }
        }
        if self.z + HEIGHT > self.ceilz {
            self.z = self.ceilz - HEIGHT;
            if self.vz > 0 {
                self.vz = 0;
            }
        }
    }
}

/// The blocks FPathTraverse visits from (x1, y1) to (x2, y2), in order.
fn walk_blocks(b: &Blocks, (x1, y1): (Fixed, Fixed), (x2, y2): (Fixed, Fixed)) -> Vec<(i64, i64)> {
    let (x1, y1, x2, y2) = (x1 - b.x, y1 - b.y, x2 - b.x, y2 - b.y);
    let (xt1, yt1, xt2, yt2) = (
        x1 >> BLOCK_SHIFT,
        y1 >> BLOCK_SHIFT,
        x2 >> BLOCK_SHIFT,
        y2 >> BLOCK_SHIFT,
    );
    let frac_in = |v: Fixed| (v >> BLOCK_TO_FRAC) & (FRAC - 1);
    let (mapxstep, mut partialx, ystep) = match xt2.cmp(&xt1) {
        std::cmp::Ordering::Greater => (1, FRAC - frac_in(x1), fixed_div(y2 - y1, (x2 - x1).abs())),
        std::cmp::Ordering::Less => (-1, frac_in(x1), fixed_div(y2 - y1, (x2 - x1).abs())),
        std::cmp::Ordering::Equal => (0, FRAC, 256 * FRAC),
    };
    let mut yintercept = (y1 >> BLOCK_TO_FRAC) + fixed_mul(partialx, ystep);
    let (mapystep, mut partialy, xstep) = match yt2.cmp(&yt1) {
        std::cmp::Ordering::Greater => (1, FRAC - frac_in(y1), fixed_div(x2 - x1, (y2 - y1).abs())),
        std::cmp::Ordering::Less => (-1, frac_in(y1), fixed_div(x2 - x1, (y2 - y1).abs())),
        std::cmp::Ordering::Equal => (0, FRAC, 256 * FRAC),
    };
    let mut xintercept = (x1 >> BLOCK_TO_FRAC) + fixed_mul(partialy, xstep);
    if xstep.abs() == FRAC && ystep.abs() == FRAC {
        if ystep < 0 {
            partialx = FRAC - partialx;
        }
        if xstep < 0 {
            partialy = FRAC - partialy;
        }
        if partialx == partialy {
            xintercept = xt1 << 16;
            yintercept = yt1 << 16;
        }
    }
    let (mut mapx, mut mapy) = (xt1, yt1);
    let mut out = Vec::new();
    for _ in 0..100 {
        out.push((mapx, mapy));
        if mapx == xt2 && mapy == yt2 {
            break;
        }
        match ((yintercept >> 16) == mapy, (xintercept >> 16) == mapx) {
            (false, false) => break,
            (false, true) => {
                xintercept += xstep;
                mapy += mapystep;
            }
            (true, false) => {
                yintercept += ystep;
                mapx += mapxstep;
            }
            (true, true) => {
                out.push((mapx + mapxstep, mapy));
                out.push((mapx, mapy + mapystep));
                xintercept += xstep;
                yintercept += ystep;
                mapx += mapxstep;
                mapy += mapystep;
            }
        }
    }
    out
}
