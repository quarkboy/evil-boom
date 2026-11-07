//! Run-off jumps: finding them across the map's gaps (`finder`), and checking them in a
//! player-movement simulator that follows ZDoom's own fixed-point physics (`physics`,
//! `check`).

mod check;
mod finder;
mod physics;

pub use finder::FinderInput;
pub use physics::{Controls, PlayerSim, World};

use crate::geom::{Cell, Point, Seg};
use crate::map::MapData;
use crate::mesh::Mesh;
use crate::motor::{MAX_MOVE, RUN_TURN};
use crate::nav::Heights;
use crate::det::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

/// A run-off jump: sprint from `runup` through `takeoff` (a ledge edge), land at `dest`.
#[derive(Clone)]
pub struct Jump {
    pub src_polys: Vec<usize>,
    pub dest_poly: usize,
    pub dest: Point,
    pub runup: Point,
    pub takeoff: Point,
    pub speed: f64,
    /// (sector, heights) of movable sectors it depends on
    pub requires: Vec<(usize, Heights)>,
}

impl Jump {
    fn new(src: usize, dest_poly: usize, dest: Point, runup: Point, takeoff: Point, speed: f64, requires: Vec<(usize, Heights)>) -> Jump {
        Jump { src_polys: vec![src], dest_poly, dest, runup, takeoff, speed, requires }
    }

    /// Its identity, as evildoom.nav.reach.Jump.key has it.
    pub fn key(&self) -> String {
        let (r, t, d) = (self.runup, self.takeoff, self.dest);
        format!("{:.0},{:.0}>{:.0},{:.0}>{:.0},{:.0}", r.0, r.1, t.0, t.1, d.0, d.1)
    }
}

/// Where the player is as a run starts: position, velocity (units a tic), feet height, facing
/// (degrees).
pub struct RunStart {
    pub pos: Point,
    pub vel: Point,
    pub z: f64,
    pub angle: f64,
}

/// Bumped whenever the finder's results could change (it keys Python's jump cache).
pub const FINDER_VERSION: u32 = 4;

/// The map as the jump tools need it.
pub struct JumpLab {
    pub map: Arc<MapData>,
    pub mesh: Arc<Mesh>,
    /// cell -> walls that never change, and solid things' box edges
    pub static_grid: HashMap<Cell, Vec<Seg>>,
    /// cell -> solid things' box edges
    pub obstacle_grid: HashMap<Cell, Vec<Seg>>,
    /// per sector: (floor, ceiling) at the start
    pub initial: Vec<(f64, f64)>,
    /// teleport lines (the simulator stops at one)
    pub teleports: HashSet<usize>,
    /// the player start's facing (degrees)
    pub start_angle: f64,
    /// per trigger whose moves stay put: its effects (to set a jump's required heights up as
    /// the trigger would: everything it moves)
    pub permanent: Vec<Vec<(usize, Vec<Heights>)>>,
    /// floors that sink once stepped on: not for stopping on, only running on
    pub sinking: HashSet<usize>,
    /// per sector (in the order the mesh first has them): the middle of its polys' centres
    sector_centres: Vec<(usize, Point)>,
    sector_edges: HashMap<usize, Vec<Seg>>,
    sector_lines: HashMap<usize, Vec<usize>>,
    /// the simulator's geometry (built on first use)
    world: OnceLock<World>,
}

/// What JumpLab is built from, besides the map and the mesh.
pub struct JumpFacts {
    pub static_grid: HashMap<Cell, Vec<Seg>>,
    pub obstacle_grid: HashMap<Cell, Vec<Seg>>,
    pub initial: Vec<(f64, f64)>,
    pub teleports: HashSet<usize>,
    pub start_angle: f64,
    pub permanent: Vec<Vec<(usize, Vec<Heights>)>>,
    pub sinking: HashSet<usize>,
}

impl JumpLab {
    /// Can the box be at p (the map's starting heights), and the floor and ceiling there?
    pub fn fits_at(&self, p: Point) -> (bool, f64, f64) {
        PlayerSim::new(self, self.world()).fits_at(p)
    }

    fn world(&self) -> &World {
        self.world.get_or_init(|| World::new(self))
    }

    /// Running from `start` pushing towards each of `headings`, with the sectors at `heights`:
    /// where each first comes down on a floor off `sector` (point and sector), within `tics`,
    /// or None (a teleport, or nowhere).
    pub fn run_landings(&self, start: &RunStart, heights: &[(f64, f64)], sector: usize, headings: &[f64], tics: usize) -> Vec<Option<(Point, usize)>> {
        let mut sim = PlayerSim::new(self, self.world());
        sim.heights = heights.iter().map(|&(f, c)| (physics::to_fixed(f), physics::to_fixed(c))).collect();
        headings
            .iter()
            .map(|&heading| {
                sim.place(start.pos, start.angle);
                sim.set_motion(start.vel, start.z);
                for _ in 0..tics {
                    // as the follower's _run_controls: full thrust towards the heading, turning
                    let d = (heading - sim.angle_degrees() + 180.0).rem_euclid(360.0) - 180.0;
                    let (s, c) = d.to_radians().sin_cos();
                    sim.step(Controls { forward: MAX_MOVE * c, strafe: -MAX_MOVE * s, turn: (-d).clamp(-RUN_TURN, RUN_TURN) });
                    if sim.teleported {
                        return None;
                    }
                    let here = self.mesh.poly_at(sim.pos()).map(|p| self.mesh.poly(p).sector);
                    if let Some(q) = here.filter(|&q| q != sector) {
                        if sim.on_floor() && (sim.floorz_units() - heights[q].0).abs() < 0.5 {
                            return Some((sim.pos(), q));
                        }
                    }
                }
                None
            })
            .collect()
    }

    /// The player from standing at `start` (x, y, angle) on the floor, given these controls a
    /// tic (forward, strafe, turn): where after each, (x, y, z, vx, vy). For comparing the
    /// simulator with the game.
    pub fn simulate(&self, start: (f64, f64, f64), heights: &[(f64, f64)], controls: &[Controls]) -> Vec<(f64, f64, f64, f64, f64)> {
        let mut sim = PlayerSim::new(self, self.world());
        sim.heights = heights.iter().map(|&(f, c)| (physics::to_fixed(f), physics::to_fixed(c))).collect();
        sim.place((start.0, start.1), start.2);
        controls
            .iter()
            .map(|&c| {
                sim.step(c);
                let ((x, y), (vx, vy)) = (sim.pos(), sim.velocity());
                (x, y, sim.z_units(), vx, vy)
            })
            .collect()
    }

    pub fn new(map: Arc<MapData>, mesh: Arc<Mesh>, facts: JumpFacts) -> JumpLab {
        let mut centres: Vec<(usize, Vec<Point>)> = Vec::new();
        let mut sector_edges: HashMap<usize, Vec<Seg>> = HashMap::default();
        for (_, poly) in mesh.live_polys() {
            match centres.iter_mut().find(|(s, _)| *s == poly.sector) {
                Some((_, v)) => v.push(poly.center),
                None => centres.push((poly.sector, vec![poly.center])),
            }
            sector_edges.entry(poly.sector).or_default().extend(poly.edges());
        }
        let sector_centres = centres
            .into_iter()
            .map(|(s, ps)| {
                let n = ps.len() as f64;
                (s, (ps.iter().map(|p| p.0).sum::<f64>() / n, ps.iter().map(|p| p.1).sum::<f64>() / n))
            })
            .collect();
        let mut sector_lines: HashMap<usize, Vec<usize>> = HashMap::default();
        for li in 0..map.linedefs.len() {
            let (f, b) = map.sides(li);
            for s in [f, b].into_iter().flatten().collect::<HashSet<_>>() {
                sector_lines.entry(s).or_default().push(li);
            }
        }
        let JumpFacts { static_grid, obstacle_grid, initial, teleports, start_angle, permanent, sinking } = facts;
        JumpLab { map, mesh, static_grid, obstacle_grid, initial, teleports, start_angle, permanent, sinking, sector_centres, sector_edges, sector_lines, world: OnceLock::new() }
    }
}
