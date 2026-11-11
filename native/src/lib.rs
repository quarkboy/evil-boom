//! Evil: a Doom bot that explores levels knowing only what a player could know, and finishes
//! them, playing DSDA-Doom in-process.
//!
//! - `wad`, `map`, `specials`, `things`: the level as the WAD has it.
//! - `mesh`: the navmesh, one convex poly per subsector, joined by portals.
//! - `movers`: where doors, lifts and floors go when set off, and how long they take.
//! - `navigator`: the level's analysis (triggers, teleports, jumps, what the box fits
//!   through) and the questions asked of it (where she can get to, routes to goals).
//! - `nav`: which heights let the player across where, flood fill, path-finding with
//!   smoothing and fitting the player's box along the path.
//! - `plan`: the search for a route through triggers and keys to a goal.
//! - `jumps`: finding run-off jumps and checking them in a fixed-point movement simulator.
//! - `sight`: which lines the player sees from where (what the automap reveals).
//! - `los`: line of sight between things (who can see, and shoot, whom).
//! - `dsda`: DSDA-Doom in-process (the game as a library, a tic at a time).

pub mod advisor;
pub mod arena;
pub mod config;
pub mod console;
pub mod control;
pub mod crush;
pub mod decisions;
mod det;
pub mod drive;
pub mod dsda;
pub mod engine;
pub mod geom;
pub mod jumps;
pub mod knowledge;
pub mod los;
pub mod map;
pub mod mesh;
pub mod motor;
pub mod movers;
pub mod nav;
pub mod navigator;
pub mod neuro;
pub mod plan;
pub mod play;
pub mod replay;
pub mod score;
pub mod sight;
pub mod specials;
mod stats;
pub mod survey;
pub mod sweep;
pub mod tactical;
pub mod things;
#[cfg(feature = "viewer")]
pub mod viewer;
pub mod wad;
pub mod walkthrough;
