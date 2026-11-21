// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A run of a level, whoever plays it out: how it's to be played (`RunOptions`), the level set
//! up to play (`setup`), and what every way of running her goes by (how far a run has got,
//! the exit held back for a 100% run, the level after this one). The sweep, the watcher and
//! the ultra instinct's runner are each built on this and not on each other.

mod setup;

#[cfg(feature = "viewer")]
pub(crate) use setup::broken_secrets;
pub use setup::{setup, Setup};

use crate::domain::Observation;
use crate::engine::Engine;
use crate::knowledge::explorer::Explorer;
use crate::play::Agent;
use std::collections::BTreeSet;

/// The ultra instinct's game speed while its search has her, of the watcher's (`--ultra-speed`
/// unless told): half...
pub const ULTRA_SPEED: f64 = 0.5;
/// ...and how far back a death is undone to, in tics (`--ultra-back`, seconds, unless told)
pub const ULTRA_BACK: i64 = 5 * 35;

/// How a level is played.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// game seconds before a map counts as stuck
    pub max_seconds: f64,
    /// no god mode: floors hurt
    pub mortal: bool,
    /// for testing whether a level can be got through at all: she plays as one who can die
    /// (no drops into what she can't get out of, no wading she hasn't the health for), and
    /// nothing hurts her. `mortal` is set with it.
    pub charmed: bool,
    /// for testing with the want of ammo taken out of it: the chaingun in her hands from the
    /// start and its bullets topped up every second. What she'd do with rounds for ever, not
    /// what she does.
    pub endless_bullets: bool,
    /// tries at a map (dying starts it again, knowing what was learned), the game time cap
    /// counting them all (unlimited unless told: lives are for testing, not a resource)
    pub lives: i32,
    /// `--lives` given (`unlimited` too): a watched episode's one life by default doesn't apply
    pub lives_given: bool,
    /// the level again this many times for being stranded (nothing left to do and no way on),
    /// over and above the lives: dying still takes a life. The score's: stranded, on stream
    /// she starts the level again within the minute, knowing the trap; dead to it is dead.
    pub stranded: i32,
    /// every switch and door known on sight from the map data
    pub peek: bool,
    /// every secret there is found (going by the tally's count), then the exit
    pub secrets: bool,
    /// the 100% runs: over once this much game time goes by with nothing more collected (items;
    /// secrets for --secrets) and nothing new learned of the level (sectors seen, what lines do)
    pub stall_seconds: f64,
    /// stop after this much game time and draw the window to out/watch.png (the viewer's)
    pub snapshot: Option<f64>,
    /// that picture's map centred here, zoomed in this much
    pub focus: Option<(f64, f64, f64)>,
    /// played back from this recording (out/runs) to this many game seconds, then the bot
    pub from: Option<(std::path::PathBuf, f64)>,
    /// every item that counts picked up, then the exit (no monsters)
    pub expert: bool,
    /// monsters in, at this skill (4: Ultra-Violence), and she fights them (0: none)
    pub fight: i32,
    /// what she starts with (and has again at every start again): playing an episode through,
    /// what she ended the level before with
    pub carry: Option<crate::dsda::Loadout>,
    /// every kill, item and secret there is (going by the tally's counts, as the automap shows
    /// them), then the exit: monsters in (UV unless --fight says), the items and secrets runs
    /// together
    pub complete: bool,
    /// the game's random numbers from this; None: the clock
    pub seed: Option<u32>,
    /// the things reported even with no fight (items seen: the secret survey's walls with an
    /// item behind them)
    pub things: bool,
    /// over (STALLED) once `stall_seconds` of game time go by with nothing new: nothing picked up
    /// or killed, seen, learned, tried or set off (the score's runs: a stuck run plays out the
    /// hour for its kill share, which by then it has)
    pub stall_out: bool,
    /// the numbers she judges by, changed from their defaults by this file (see
    /// `tactical::tuning`)
    pub tuning: Option<std::path::PathBuf>,
    /// (`watch --tape FILE`) a run's controls, tic by tic, played in place of hers: an ultra
    /// instinct run, which is searched offline, watched
    pub tape: Option<std::path::PathBuf>,
    /// (`watch --ultra`) the ultra instinct on in the window from the start, and kept on
    pub ultra: bool,
    /// (`--workers N`) the ultra instinct's looks by N worker processes (0: its own)
    pub workers: usize,
    /// (`--ultra-speed X`) how fast the window plays while the ultra instinct's search has
    /// her, of its own speed (0.5: half)
    pub ultra_speed: f64,
    /// (`--ultra-after N`) the ultra instinct hers on a level once she's died N times there
    /// (None: never)
    pub ultra_after: Option<i32>,
    /// (`--ultra-back S`) how many seconds back a death is undone to (5 unless told; 20 at most)
    pub ultra_back: f64,
    /// the journal of her decisions kept, and written beside the log (`crate::decisions`)
    pub decisions: bool,
    /// moves and retreats she isn't to make, by name (`Player::bar`)
    pub bar: Option<String>,
    /// a decision model asked as she plays: its settings file (`crate::advisor`)
    pub advisor: Option<std::path::PathBuf>,
    /// the level known beforehand, as by someone who's played it: she looks round it first with
    /// no monsters in it (what she learns of the map, its doors and switches, where things
    /// lie), then plays it knowing that and where every monster stands at the start
    pub known: bool,
    /// the game goes on to another level after this one (an episode's, with one after it): what
    /// she'd take with her had before the exit
    pub going_on: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            max_seconds: 3600.0,
            mortal: false,
            charmed: false,
            endless_bullets: false,
            lives: i32::MAX,
            lives_given: false,
            stranded: 0,
            peek: false,
            secrets: false,
            expert: false,
            fight: 0,
            complete: false,
            carry: None,
            seed: Some(0),
            snapshot: None,
            focus: None,
            from: None,
            stall_seconds: 300.0,
            things: false,
            stall_out: false,
            tuning: None,
            tape: None,
            ultra: false,
            workers: 0,
            ultra_speed: ULTRA_SPEED,
            ultra_after: None,
            ultra_back: ULTRA_BACK as f64 / 35.0,
            decisions: false,
            bar: None,
            advisor: None,
            known: false,
            going_on: false,
        }
    }
}

/// The 100% runs: the exit last while the count she's after (on the automap, as a player sees it)
/// is short of all there is; then out.
pub(crate) fn hold_exit(
    e: &Engine,
    agent: &mut Agent,
    o: &RunOptions,
    secrets_short: i32,
    items_short: i32,
    given_up: bool,
) {
    let tally = e.tally();
    if given_up {
        agent.x_mut().exit_held_back = false;
    } else if o.complete {
        // (any of the three short: the kills too, whatever's left of them that she knows of
        // hunted down last)
        agent.x_mut().exit_held_back = tally[0].0 < tally[0].1 - items_short
            || tally[1].0 < tally[1].1
            || tally[2].0 < tally[2].1 - secrets_short;
    } else if o.secrets {
        agent.x_mut().exit_held_back = tally[2].0 < tally[2].1 - secrets_short;
    } else if o.expert {
        agent.x_mut().exit_held_back = tally[0].0 < tally[0].1 - items_short;
    }
}

/// How far a 100% run's got: what it's after collected (items; secrets; for --complete all three
/// tallies together) and what's been learned of the level, each counted (any of them up is
/// progress; the stall clock runs while none is).
pub(crate) fn progress(e: &Engine, x: &Explorer, o: &RunOptions) -> [usize; 7] {
    let t = e.tally();
    let collected = if o.complete {
        t[0].0 + t[1].0 + t[2].0
    } else if o.secrets {
        t[2].0
    } else {
        // (the kills as well, with monsters in: a fight going on is something happening)
        t[0].0 + t[1].0
    };
    let [seen, learned, walls, mapped, fired, walked] = known_progress(x);
    [
        usize::try_from(collected).unwrap_or(0),
        seen,
        learned,
        walls,
        mapped,
        fired,
        walked,
    ]
}

/// What she's come to know of the level, as progress counts it: the sectors seen, effects
/// learned, walls tried, lines mapped, lines set off, places walked (with what's been
/// collected, `progress`; directed play's restart goes by the same).
pub fn known_progress(x: &Explorer) -> [usize; 6] {
    let k = &x.knowledge;
    let fired: BTreeSet<usize> = k.fired.iter().map(|f| f.line).collect();
    [
        k.sectors.len(),
        k.learned.values().map(BTreeSet::len).sum::<usize>(),
        k.tried_walls.len(),
        k.mapped.len(),
        fired.len(),
        k.visited.len(),
    ]
}

/// The map after `map` in an episode played through, by the exit taken (None: the end).
pub fn next_map(map: &str, secret: bool) -> Option<String> {
    let up = map.to_uppercase();
    if let Some(n) = up.strip_prefix("MAP").and_then(|n| n.parse::<u32>().ok()) {
        return match (n, secret) {
            (30, _) => None,
            (15, true) => Some("MAP31".into()),
            (31, true) => Some("MAP32".into()),
            (31 | 32, _) => Some("MAP16".into()),
            _ => Some(format!("MAP{:02}", n + 1)),
        };
    }
    let b = up.as_bytes();
    let (e, m) = (char::from(*b.get(1)?), b.get(3)?.checked_sub(b'0')?);
    // (the secret exit's level, and the one after it the secret level goes back to)
    let secret_from = match e {
        '1' => 3,
        '2' => 5,
        '3' => 6,
        _ => 2,
    };
    match (m, secret) {
        (8, _) => None,
        (9, _) => Some(format!("E{e}M{}", secret_from + 1)),
        (_, true) => Some(format!("E{e}M9")),
        _ => Some(format!("E{e}M{}", m + 1)),
    }
}

/// `--endless-bullets`: her as she is, but the chaingun hers and its bullets full.
pub fn top_up_bullets(game: &crate::dsda::Dsda, ob: &Observation) {
    let slot = |n: i32| ob.weapons.as_ref().is_some_and(|w| w.contains(&n));
    let ammo = |n: i32| {
        ob.ammo
            .iter()
            .flatten()
            .find(|(s, _)| *s == n)
            .map_or(0, |(_, a)| *a)
    };
    let mut weapons = 1 | 2 | 8;
    for (n, bit) in [(3, 4), (5, 16), (6, 32), (7, 64)] {
        if slot(n) {
            weapons |= bit;
        }
    }
    if ob.chainsaw == Some(true) {
        weapons |= 128;
    }
    if ob.super_shotgun == Some(true) {
        weapons |= 256;
    }
    game.give(&crate::dsda::Loadout {
        weapons,
        clip: 400,
        shell: ammo(3),
        cell: ammo(6),
        rocket: ammo(5),
        backpack: false,
        health: ob.health.unwrap_or(100.0) as i32,
        armor: ob.armor.unwrap_or(0.0) as i32,
        armor_type: ob.armor_type.unwrap_or(0),
        ready: None,
    });
}
