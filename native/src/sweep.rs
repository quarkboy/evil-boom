//! Running the explorer through a level (knowing only what she has seen) and reporting how it
//! went; `evil-boom sweep` runs many at once, a process a map (the game is one a process).

mod setup;
mod status;

#[cfg(feature = "viewer")]
pub(crate) use setup::broken_secrets;
pub use setup::{setup, Setup};

use crate::config::out_dir;
use crate::engine::Engine;
use crate::knowledge::completion::{most_items, COUNTED};
use crate::knowledge::explorer::Explorer;
use crate::map::MapData;
use crate::motor::{Observation, TICRATE};
use crate::nav::Heights;
use crate::navigator::{Navigator, States};
use crate::play::Agent;
use crate::replay::Entry;
use crate::wad::Wad;
use crate::walkthrough::{plan_from, LevelPlan, Rest};
use status::Status;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// nothing to do this long (with lives left): start the level again
const STRANDED_TICS: i64 = 60 * 35;

/// How a level is played.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// game seconds before a map counts as stuck
    pub max_seconds: f64,
    /// no god mode: floors hurt
    pub mortal: bool,
    /// tries at a map (dying starts it again, knowing what was learned), the game time cap
    /// counting them all (unlimited unless told: lives are for testing, not a resource)
    pub lives: i32,
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
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            max_seconds: 3600.0,
            mortal: false,
            lives: i32::MAX,
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
            decisions: false,
            bar: None,
            advisor: None,
            known: false,
        }
    }
}

/// Where the logs go.
pub fn log_dir() -> PathBuf {
    out_dir().join("explore")
}

/// The omniscient plan's steps, each with what the explorer knew of it: the trigger, and the way
/// there where it goes through what she doesn't know or by heights she doesn't take a sector to be.
fn plan_report(lp: &LevelPlan, x: &Explorer, real: &[Heights]) -> Vec<String> {
    let k = &x.knowledge;
    let learned: BTreeSet<i64> = k.known_triggers().iter().map(|(t, _)| t.line).collect();
    let fired: BTreeSet<usize> = k.fired.iter().map(|f| f.line).collect();
    let known = k.known_polys();
    let believed = x.states();
    let mesh = &x.nav.mesh;
    let mut out = Vec::new();
    for st in &lp.steps {
        if let Some(path) = &st.path {
            let unknown: Vec<usize> = path.polys.iter().copied().filter(|p| !known.contains(p)).collect();
            let off: BTreeSet<usize> =
                path.polys.iter().map(|&p| mesh.poly(p).sector).filter(|&s| real.get(s).is_some_and(|h| !believed[s].contains(h))).collect();
            if !unknown.is_empty() || !off.is_empty() {
                let mut line = format!("    (the way: {} of {} polys unknown", unknown.len(), path.polys.len());
                if let Some(&u) = unknown.first() {
                    let c = mesh.poly(u).center;
                    line += &format!(", from poly {u} at ({}, {})", c.0.round(), c.1.round());
                }
                for s in &off {
                    let mut b: Vec<String> = believed[*s].iter().map(|h| crate::motor::fmt_live(Some(*h))).collect();
                    b.sort();
                    line += &format!("; sector {s} is {}, she takes it as [{}]", crate::motor::fmt_live(Some(real[*s])), b.join(", "));
                }
                if !path.steps.is_empty() {
                    line += &format!("; via {:?}", path.steps);
                }
                out.push(line + ")");
            }
            let reach = &x.reach;
            if let Some(cut) = path.polys.iter().position(|p| !reach.contains(p)).filter(|_| !reach.is_empty()) {
                let p = path.polys[cut];
                let c = mesh.poly(p).center;
                let mut line = format!("    (her reach ends at poly {p}, sector {}, ({}, {})", mesh.poly(p).sector, c.0.round(), c.1.round());
                if cut > 0 {
                    line += &format!(", from sector {}", mesh.poly(path.polys[cut - 1]).sector);
                }
                if let Some(n) = k.stuck_at.get(&p) {
                    line += &format!("; stuck there {n}x");
                }
                if k.traps.iter().any(|t| t.contains(&p)) {
                    line += "; in a trap";
                }
                out.push(line + ")");
            }
        }
        let Some(t) = &st.trigger else {
            out.push(format!("  {} {}", st.action.name(), st.label));
            continue;
        };
        let li = t.line;
        let mapped = t.linedef().is_some_and(|l| k.mapped.contains(&l));
        let was_fired = t.linedef().is_some_and(|l| fired.contains(&l));
        out.push(format!(
            "  {} line {li} {} {}: mapped {}, fired {}, learned {}, activators known {}/{}, effect sectors known {}/{}",
            st.action.name(),
            t.info.trigger.name(),
            t.info.description,
            py_bool(mapped),
            py_bool(was_fired),
            py_bool(learned.contains(&li)),
            t.activators.iter().filter(|p| known.contains(p)).count(),
            t.activators.len(),
            t.effects.iter().filter(|(s, _)| k.sectors.contains(s)).count(),
            t.effects.len()
        ));
    }
    out
}

fn py_bool(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

/// The omniscient plan from where she is, with the heights as they are.
/// The 100% runs: the exit last while the count she's after (on the automap, as a player sees it)
/// is short of all there is; then out.
pub(crate) fn hold_exit(e: &Engine, agent: &mut Agent, o: &RunOptions, secrets_short: i32, items_short: i32, given_up: bool) {
    let tally = e.tally();
    if given_up {
        agent.x_mut().exit_last = false;
    } else if o.complete {
        // (any of the three short: the kills too, whatever's left of them that she knows of
        // hunted down last)
        agent.x_mut().exit_last = tally[0].0 < tally[0].1 - items_short || tally[1].0 < tally[1].1 || tally[2].0 < tally[2].1 - secrets_short;
    } else if o.secrets {
        agent.x_mut().exit_last = tally[2].0 < tally[2].1 - secrets_short;
    } else if o.expert {
        agent.x_mut().exit_last = tally[0].0 < tally[0].1 - items_short;
    }
}

/// How far a 100% run's got: what it's after collected (items; secrets; for --complete all three
/// tallies together) and what's been learned of the level, each counted (any of them up is
/// progress; the stall clock runs while none is).
pub(crate) fn progress(e: &Engine, x: &Explorer, o: &RunOptions) -> [usize; 7] {
    let k = &x.knowledge;
    let fired: BTreeSet<usize> = k.fired.iter().map(|f| f.line).collect();
    let t = e.tally();
    let collected = if o.complete {
        t[0].0 + t[1].0 + t[2].0
    } else if o.secrets {
        t[2].0
    } else {
        // (the kills as well, with monsters in: a fight going on is something happening)
        t[0].0 + t[1].0
    };
    [
        usize::try_from(collected).unwrap_or(0),
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

/// (recordings of different kinds of run kept apart)
fn mode_suffix(o: &RunOptions) -> String {
    let mut out = String::new();
    for (on, name) in [(o.complete, "_complete"), (o.fight > 0, "_fight"), (o.expert, "_expert"), (o.secrets, "_secrets"), (o.peek, "_peek")] {
        if on {
            out.push_str(name);
        }
    }
    out
}

pub fn plan_here(nav: &Rc<Navigator>, obs: &Observation, max_seconds: f64) -> LevelPlan {
    let view = nav.full_view();
    let states: States = obs.heights.iter().map(|&h| vec![h]).collect();
    plan_from(
        &view,
        obs.pos(),
        &Rest { keys: obs.keys.unwrap_or_default(), states: Some(&states), reach: None, max_seconds, max_expansions: 20000, fired: &BTreeSet::new() },
    )
}

/// Play one map; (outcome, detail). Writes the log to out/explore/<IWAD>_<MAP>.txt.
/// One map played: the outcome, its detail; and, if she got out, what she has to carry on with
/// and whether by the secret exit.
pub struct MapRun {
    pub outcome: String,
    pub detail: String,
    pub carry: Option<crate::dsda::Loadout>,
    pub secret: bool,
    /// what hurt her, most first (see `damage_summary`)
    pub damage: String,
    /// counts for measuring the run, as JSON (the fight's: what hurt her and what she knew of
    /// it, what she spent and dealt, what alerted her)
    pub stats: String,
    /// the game, for an episode's next level in the same process
    pub engine: Option<Engine>,
}

/// The damage she took by where it came from, most first, with the tics of the hits (the
/// first few, to look up in the log): "ShotgunGuy 45 (+10 armor) at tic 1583 (45s), 1614".
fn damage_summary(damage: &BTreeMap<String, (i32, i32, Vec<i64>)>) -> String {
    const SHOWN: usize = 4;
    let mut d: Vec<_> = damage.iter().collect();
    d.sort_by_key(|(_, (h, a, _))| std::cmp::Reverse(h + a));
    d.iter()
        .map(|(from, (h, a, tics))| {
            let armor = if *a > 0 { format!(" (+{a} armor)") } else { String::new() };
            let mut at: Vec<String> = tics.iter().take(SHOWN).map(i64::to_string).collect();
            at[0] = format!("{} ({:.0}s)", tics[0], tics[0] as f64 / TICRATE);
            if tics.len() > SHOWN {
                at.push(format!("+{} more", tics.len() - SHOWN));
            }
            format!("{from} {h}{armor} at tic {}", at.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn run_map(wad: &str, map_name: &str, o: &RunOptions) -> Result<(String, String), String> {
    run_map_full(wad, map_name, o).map(|r| (r.outcome, r.detail))
}

pub fn run_map_full(wad: &str, map_name: &str, o: &RunOptions) -> Result<MapRun, String> {
    run_map_in(wad, map_name, o, None)
}

/// The look round an empty level that knowing it beforehand stands for: the level with no
/// monsters in it, played till she's out, or has had `SCOUT_SECONDS` of it, or `SCOUT_STALL`
/// goes by with nothing new mapped or walked into; dying of a floor or a crusher, again. Then
/// the level itself from the start, monsters in, with what she learned and where each of them
/// stands. Nothing of the look round is counted or logged as part of the run.
fn scout(e: &mut Engine, agent: &mut Agent) -> i64 {
    const SCOUT_SECONDS: f64 = 1500.0;
    const SCOUT_STALL: i64 = 120 * 35;
    e.set_monsters(false);
    e.restart(true);
    agent.start_over();
    let start = e.tic;
    let log_from = agent.x().log.len();
    let held = std::mem::replace(&mut agent.x_mut().exit_last, true); // (everything first, then out)
    let mut c: Option<crate::motor::Controls> = None;
    let (mut best, mut best_tic, mut lives) = (0usize, e.tic, 0);
    while ((e.tic - start) as f64) < SCOUT_SECONDS * TICRATE && !e.exited() {
        if e.finished() && e.dead() {
            lives += 1;
            if lives > 4 {
                break;
            }
            e.restart(true);
            agent.start_over();
            c = None;
            continue;
        }
        let ob = e.observe();
        let ctl = agent.act(&ob, c.as_ref());
        e.step(&ctl);
        e.take_hits();
        c = Some(ctl);
        let k = &agent.x().knowledge;
        let known = k.mapped.len() + k.visited.len() + k.learned.len();
        if known > best {
            (best, best_tic) = (known, e.tic);
        } else if e.tic - best_tic > SCOUT_STALL {
            break;
        }
    }
    let x = agent.x_mut();
    x.exit_last = held;
    let summary = format!("--- the level looked round beforehand (no monsters in it): {}", x.knowledge.summary());
    x.log.truncate(log_from);
    x.log.push(summary);
    // (the clock goes on: what she knows is dated by it)
    e.set_monsters(true);
    e.restart(true);
    agent.start_over();
    let first = e.observe();
    if let Some(p) = agent.player_mut() {
        p.know_monsters(&first);
        let n = p.combat().known_count();
        p.explorer.log.push(format!("--- and its monsters known: {n} of them, where they stand"));
    }
    e.tic
}

/// A map played in the game already going if there is one (see `setup`).
pub fn run_map_in(wad: &str, map_name: &str, o: &RunOptions, engine: Option<Engine>) -> Result<MapRun, String> {
    let Setup { iwad, w, m, map_name, skill, nav, mut e, mut agent, mut deaths, mut c, mut obs, mut rec, secrets_short } =
        setup(wad, map_name, o, o.snapshot.is_some(), engine)?;
    // (the level looked round first: the run's own time is from after that)
    let mut tic0 = 0;
    if o.known {
        tic0 = scout(&mut e, &mut agent);
        (c, obs, deaths) = (None, None, 0);
    }
    let t0 = Instant::now();
    let mut idle_since: Option<i64> = None;
    let (mut t_engine, mut t_observe, mut t_think) = (0.0, 0.0, 0.0);
    let mut status = Status::open(&iwad.file_stem().map_or_else(|| "WAD".to_string(), |s| s.to_string_lossy().to_uppercase()), &map_name);
    let mut journal_n = agent.x().knowledge.journal.len();
    let until = o.snapshot.map_or(o.max_seconds, |s| s.min(o.max_seconds));
    // (the 100% runs: the count she's after and what she knows of the level, and when either
    // last grew: exploring a teleporter maze before any items is getting somewhere, MAP24; and
    // the walls she's tried, each one a place she's looked for a way: E1M2's gun panel, a round
    // of walls away when the clock ran out)
    let items_short = most_items(&m, skill);
    // (and finding her way about: lines mapped, lines set off, places walked into; MAP03's
    // exploring, none of it counted, over in a minute; the user's. Places by the poly: E4M9's
    // steps over the lines beyond, each somewhere new in a sector she'd been in)
    let progress = |e: &Engine, x: &Explorer| progress(e, x, o);
    // (the best so far this life: a level again starts the clock afresh, collecting the same
    // things again counted, the user's call; the run's time cap bounds it)
    let (mut best, mut best_tic) = (progress(&e, agent.x()), e.tic);
    let mut stalled = false;
    // (the stall clock ran out once: the exit, whatever's left)
    let mut given_up = false;
    // (the most items and secrets one life got: a level again starts the tally afresh)
    let mut best_life = (0, 0);
    // (the secrets tally so far this life: up by one, she's just found the one where she stands)
    let mut secrets_seen = 0;
    // (what hurt her, and how much: health, armour, by where it came from; the last hit)
    let mut damage: BTreeMap<String, (i32, i32, Vec<i64>)> = BTreeMap::new();
    let mut last_hit: Option<String> = None;
    let (mut turn_flips, mut last_jitter): (Vec<i64>, i64) = (Vec::new(), i64::MIN / 2);
    let (mut side_from, mut side_run): (Option<crate::geom::Point>, f64) = (None, 0.0);
    while ((e.tic - tic0) as f64) < until * TICRATE {
        if o.expert || o.secrets || o.stall_out {
            let now = progress(&e, agent.x());
            if now.iter().zip(&best).any(|(n, b)| n > b) {
                best = std::array::from_fn(|i| now[i].max(best[i]));
                best_tic = e.tic;
            } else if (e.tic - best_tic) as f64 > o.stall_seconds * TICRATE {
                if !given_up && (o.expert || o.secrets) {
                    // (what's left out of reach as far as she can tell: out, as a player gives up
                    // on the last secret; a stall's time again to get there. E4M5's second secret)
                    agent.x_mut().log.push(format!("tic {}: nothing more collected, learned or tried in {:.0}s: for the exit", e.tic, o.stall_seconds));
                    given_up = true;
                    best_tic = e.tic;
                    hold_exit(&e, &mut agent, o, secrets_short, items_short, given_up);
                } else {
                    agent.x_mut().log.push(format!("tic {}: nothing more collected, learned or tried in {:.0}s: over", e.tic, o.stall_seconds));
                    stalled = true;
                    break;
                }
            }
        }
        let mut stranded = idle_since.is_some_and(|i| e.tic - i > STRANDED_TICS.min((o.stall_seconds * TICRATE / 2.0) as i64)) || agent.x().restart_wanted;
        if let (true, Some(ob), Some(p)) = (e.finished() && e.dead(), &obs, agent.player_mut()) {
            let pos = ob.pos();
            let seen: Vec<String> = p.combat().visible.iter().map(|t| format!("{} {:.0} away ({} health)", t.name, crate::geom::dist((t.x, t.y), pos), t.health)).collect();
            let doing = p.explorer.current.as_ref().map_or("-".to_string(), |c| c.label.clone());
            let msg = format!(
                "tic {}: killed at ({:.0}, {:.0}) by {}: health {:.0}, armor {:.0}, weapon {}, weapons {:?}, ammo {:?}; in sight: {}; doing: {doing}",
                e.tic,
                pos.0,
                pos.1,
                last_hit.as_deref().unwrap_or("?"),
                ob.health.unwrap_or(0.0),
                ob.armor.unwrap_or(0.0),
                ob.weapon.unwrap_or(0),
                ob.weapons.clone().unwrap_or_default(),
                ob.ammo.clone().unwrap_or_default(),
                if seen.is_empty() { "nothing".to_string() } else { seen.join(", ") }
            );
            p.explorer.log.push(msg);
        }
        if agent.x().restart_wanted && deaths + 1 >= o.lives {
            // (no level again to be had: out with what she has)
            agent.x_mut().restart_wanted = false;
            agent.no_more_restarts();
            stranded = false;
        }
        let finished = e.finished();
        if finished || (stranded && deaths + 1 < o.lives) {
            // dead, or stranded with nothing to do (a trap she couldn't know of: MAP12's floor
            // that drops into a pit): the level again, knowing what she learned
            let dead = e.dead();
            if (dead || stranded) && deaths + 1 < o.lives {
                deaths += 1;
                let why = if dead {
                    "died"
                } else if agent.x().restart_wanted {
                    "for what's left"
                } else {
                    "stranded"
                };
                agent.x_mut().log.push(format!("tic {}: {why}: starting again", e.tic));
                if why == "stranded" {
                    if let Some(ob) = &obs {
                        // (a trap she walked into, or a way on she couldn't see?)
                        let lp = plan_here(&nav, ob, 20.0);
                        let x = agent.x();
                        let label = x.current.as_ref().map_or("-".to_string(), |c| c.label.clone());
                        let head = format!(
                            "  stranded at ({:.0}, {:.0}), objective {label}; omniscient plan from there: {}",
                            ob.x,
                            ob.y,
                            if lp.complete { "ok".to_string() } else { lp.problem.clone() }
                        );
                        let rep = plan_report(&lp, x, &ob.heights);
                        let x = agent.x_mut();
                        x.log.push(head);
                        x.log.extend(rep.into_iter().map(|l| format!("  {l}")));
                    }
                }
                agent.x_mut().restart_wanted = false;
                let t = e.tally();
                best_life = (best_life.0.max(t[0].0), best_life.1.max(t[2].0));
                rec.entries.push(Entry::Restart(e.tic, why.to_string()));
                e.restart(true);
                agent.restart();
                c = None;
                idle_since = None;
                (best, best_tic) = (progress(&e, agent.x()), e.tic);
                secrets_seen = 0;
                given_up = false;
                continue;
            }
            break;
        }
        if e.tic % 35 == 0 {
            hold_exit(&e, &mut agent, o, secrets_short, items_short, given_up);
        }
        let t1 = Instant::now();
        let ob = e.observe();
        let found = e.tally()[2].0;
        if found > secrets_seen {
            if let Some(p) = agent.x().nav.poly_at(ob.pos()) {
                let s = agent.x().nav.sector_of_poly(p);
                agent.x_mut().found_secret(s, ob.tic);
            }
        }
        secrets_seen = found;
        let t2 = Instant::now();
        let log_n = agent.x().log.len();
        let ctl = agent.act(&ob, c.as_ref());
        // (for measuring: how she turns. A turn one way then the other on the next tic, both
        // more than a couple of degrees, is a jitter no hand makes; and how fast she turns at
        // the most, and how sharply the rate changes)
        if let Some(p) = agent.player_mut() {
            let was = c.as_ref().map_or(0.0, |c| c.turn);
            let (now, legs) = (ctl.turn, p.legs());
            if was * now < 0.0 && was.abs() > 2.0 && now.abs() > 2.0 {
                turn_flips.push(ob.tic);
                p.combat_mut().count(&format!("turn reversals, on the {legs:?} legs"), 1.0);
                // (four in a third of a second: a jitter to look at)
                if turn_flips.len() >= 4 && ob.tic - turn_flips[turn_flips.len() - 4] <= 12 && ob.tic - last_jitter > 35 {
                    last_jitter = ob.tic;
                    p.combat_mut().count("jitters", 1.0);
                    p.explorer.log.push(format!("tic {}: (measuring) jitter: her turn back and forth four times in a third of a second, the last {was:.0} then {now:.0} degrees", ob.tic));
                }
            }
            // (and her steps to the side in a fight, across the line to her target: each run one
            // way, how far it went before she stopped or came back)
            let pos = ob.pos();
            let target = p.combat().target.and_then(|id| p.combat().visible.iter().find(|t| t.id == id)).map(|t| (t.x, t.y));
            let across = match (legs, target, side_from) {
                (crate::tactical::arbiter::Legs::Fight, Some(t), Some(from)) if crate::geom::dist(t, pos) > 1.0 => {
                    let (dx, dy) = (t.0 - pos.0, t.1 - pos.1);
                    Some(((pos.0 - from.0) * dy - (pos.1 - from.1) * dx) / dx.hypot(dy))
                }
                _ => None,
            };
            side_from = Some(pos);
            match across {
                Some(a) if a.abs() <= 0.5 => {}
                Some(a) if side_run == 0.0 || a * side_run > 0.0 => side_run += a,
                _ => {
                    let far = side_run.abs();
                    if far >= 16.0 {
                        let doing = p.combat().last_move.map_or("no move".to_string(), |m| format!("{m:?}"));
                        p.combat_mut().count("side runs", 1.0);
                        p.combat_mut().count("side runs, units", far);
                        for over in [96.0, 192.0] {
                            if far > over {
                                p.combat_mut().count(&format!("side runs over {over:.0}"), 1.0);
                                p.combat_mut().count(&format!("side runs over {over:.0}, the move: {doing}"), 1.0);
                            }
                        }
                    }
                    side_run = across.unwrap_or(0.0);
                }
            }
            if now.abs() > 18.5 {
                p.combat_mut().count("tics turning over 18 degrees", 1.0);
            }
            if (now - was).abs() > 12.0 {
                p.combat_mut().count("tics the turn changed by over 12 degrees", 1.0);
            }
        }
        let t3 = Instant::now();
        if let Some(st) = status.as_mut() {
            st.think(&ob, (t3 - t2).as_secs_f64(), agent.x(), log_n, t0.elapsed().as_secs_f64());
        }
        if agent.x().current.is_none() {
            idle_since.get_or_insert(e.tic);
        } else {
            idle_since = None;
        }
        e.step(&ctl);
        for h in e.take_hits() {
            let from = h.from();
            let armor = if h.armor > 0 { format!(" (and {} armor)", h.armor) } else { String::new() };
            agent.x_mut().log.push(format!("tic {}: hit for {}{armor} by {from}", e.tic, h.health));
            // (for measuring: what hit her, how, and what she knew of it)
            if let Some(p) = agent.player_mut() {
                let how = match (h.cause, h.own, h.inflictor, h.source) {
                    (crate::dsda::HitCause::Floor, ..) => "floor",
                    (crate::dsda::HitCause::Crusher, ..) => "crusher",
                    (crate::dsda::HitCause::Other, ..) => "other",
                    (_, true, ..) => "her own",
                    (_, _, Some(i), _) if crate::dsda::type_name(i) == Some("ExplosiveBarrel") => "barrel",
                    (_, _, Some(i), Some(s)) if i != s => "missile",
                    (_, _, _, Some(s)) if crate::dsda::type_name(s).and_then(crate::tactical::bestiary::monster).is_some_and(|m| m.attack == crate::tactical::bestiary::Attack::Hitscan) => "bullet",
                    _ => "claw",
                };
                let key = match h.source_id.filter(|_| matches!(how, "missile" | "bullet" | "claw")) {
                    Some(id) => {
                        let (knew, far) = p.combat().measure((ob.x, ob.y), id);
                        let side = match h.side {
                            Some(0) => "ahead",
                            Some(_) => "to a side",
                            None => "nowhere",
                        };
                        if knew != "in view" && far == "near" {
                            let (legs, going) = (p.legs(), p.combat().measure_approach((ob.x, ob.y), id));
                            p.combat_mut().count(&format!("unseen near: on the {legs:?} legs"), f64::from(h.health));
                            p.combat_mut().count(&format!("unseen near: {going}"), f64::from(h.health));
                            p.combat_mut().count(&format!("unseen near: {how}, on the {legs:?} legs, {going}"), f64::from(h.health));
                            let doing = p.combat().last_move.map_or("no move".to_string(), |m| format!("{m:?}"));
                            p.combat_mut().count(&format!("unseen near, the move: {doing}"), f64::from(h.health));
                            p.combat_mut().count(&format!("unseen near, move and legs: {doing} on the {legs:?} legs, {going}"), f64::from(h.health));
                        }
                        format!("hurt by a {how}, its source {knew}, {far}, {side}")
                    }
                    None => format!("hurt by: {how}"),
                };
                p.combat_mut().count(&key, f64::from(h.health));
                p.combat_mut().count("health lost", f64::from(h.health));
            }
            let d = damage.entry(from.clone()).or_default();
            (d.0, d.1) = (d.0 + h.health, d.1 + h.armor);
            d.2.push(e.tic);
            last_hit = Some(from);
        }
        rec.entries.push(Entry::Tic(ob.tic, ctl));
        {
            let k = &agent.x().knowledge;
            rec.entries.extend(k.journal[journal_n.min(k.journal.len())..].iter().map(|(t, nt)| Entry::Note(*t, nt.clone())));
            journal_n = k.journal.len();
        }
        t_observe += (t2 - t1).as_secs_f64();
        t_think += (t3 - t2).as_secs_f64();
        t_engine += t3.elapsed().as_secs_f64();
        c = Some(ctl);
        obs = Some(ob);
    }
    let (exited, died, game_s) = (e.exited(), e.finished() && e.dead(), (e.tic - tic0) as f64 / TICRATE);
    if o.decisions {
        if let Some(p) = agent.player_mut() {
            p.file_decisions();
        }
        agent.x_mut().journal.records.sort_by_key(|r| r.tic);
    }
    let x = agent.x();
    let k = &x.knowledge;
    let mut lines: Vec<String> = x.log.clone();
    lines.push(String::new());
    lines.push("knowledge events:".into());
    lines.extend(k.events.iter().cloned());
    lines.push(String::new());
    lines.push(k.summary());
    if !k.doubts.is_empty() {
        let d: Vec<String> = k.doubts.iter().map(|((key, s), tic)| format!("{key} moves sector {s} (since tic {tic})")).collect();
        lines.push(format!("in doubt: {}", d.join(", ")));
    }
    if let (false, Some(ob)) = (exited, &obs) {
        let label = x.current.as_ref().map_or("-".to_string(), |c| c.label.clone());
        lines.push(format!("ended at ({:.0}, {:.0}), objective: {label}", ob.x, ob.y));
        let lp = plan_here(&nav, ob, 30.0);
        lines.push(format!("omniscient plan from there: {}", if lp.complete { "ok".to_string() } else { lp.problem.clone() }));
        lines.extend(plan_report(&lp, x, &ob.heights));
    }
    let tally = e.tally();
    if let (true, Some(ob)) = (o.expert, &obs) {
        let known = k.known_polys();
        for t in ob.things.iter().flatten().filter(|t| COUNTED.contains(&t.name)) {
            let poly = nav.poly_at((t.x, t.y));
            let seen = poly.is_some_and(|p| known.contains(&p));
            lines.push(format!(
                "left: {} at ({:.0}, {:.0}), sector {}, {}",
                t.name,
                t.x,
                t.y,
                poly.map_or("None".to_string(), |p| nav.sector_of_poly(p).to_string()),
                if seen { "seen" } else { "never seen" }
            ));
        }
    }
    let mut outcome = if exited {
        "EXITED"
    } else if died {
        "DIED"
    } else if stalled {
        "STALLED"
    } else {
        "STUCK"
    };
    if o.secrets && exited && tally[2].0 >= tally[2].1 - secrets_short {
        outcome = "SECRETS"; // (every secret there is to find, then out)
    }
    if o.expert && exited && tally[0].0 >= tally[0].1 - most_items(&m, skill) {
        outcome = "EXPERT"; // (every item there is to get, then out)
    }
    if o.complete && outcome == "EXPERT" && (!o.secrets || tally[2].0 >= tally[2].1 - secrets_short) && tally[1].0 >= tally[1].1 {
        outcome = "COMPLETE"; // (every kill, item and secret, then out)
    }
    let real = t0.elapsed().as_secs_f64();
    lines.push(format!(
        "time: engine {t_engine:.1}s, observe {t_observe:.1}s, think {t_think:.1}s, other {:.1}s; {:.0} tics/s",
        real - t_engine - t_observe - t_think,
        e.tic as f64 / real.max(1e-9)
    ));
    let mut detail = format!("{game_s:.0}s game, {real:.0}s real, {deaths} deaths; {}", k.summary());
    detail = format!("items {}/{}, {detail}", tally[0].0.max(best_life.0), tally[0].1);
    if o.secrets {
        detail = format!("secrets {}/{}, {detail}", tally[2].0.max(best_life.1), tally[2].1);
    }
    // (the kills always: the score's partial credit for a run that doesn't get out)
    detail = format!("kills {}/{}, {detail}", tally[1].0, tally[1].1);
    let stats = agent.player().map_or_else(|| "{}".to_string(), |p| serde_json::to_string(&p.combat().tally).unwrap_or_default());
    let hurt = damage_summary(&damage);
    lines.push(format!("damage taken: {}", if hurt.is_empty() { "none" } else { &hurt }));
    lines.push(format!("RESULT {outcome} {detail}"));
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let stem = iwad.file_stem().map_or_else(|| "WAD".to_string(), |s| s.to_string_lossy().to_uppercase());
    // (and kept, by the commit, the time and the seed: not written over by the next run)
    let _ = crate::config::write_log(&dir.join(format!("{stem}_{map_name}.txt")), &o.seed.map_or_else(|| "random".to_string(), |s| format!("s{s}")), &(lines.join("\n") + "\n"));
    if o.decisions {
        let _ = std::fs::write(dir.join(format!("{stem}_{map_name}.decisions.jsonl")), agent.x().journal.lines());
    }
    {
        let k = &agent.x().knowledge;
        rec.entries.extend(k.journal[journal_n.min(k.journal.len())..].iter().map(|(t, nt)| Entry::Note(*t, nt.clone())));
    }
    let branch = if o.from.is_some() { "_branch" } else { "" };
    rec.write(&out_dir().join("runs").join(format!("{stem}_{map_name}{}{branch}.run", mode_suffix(o))))?;
    let carry = exited.then(|| e.loadout());
    let secret = exited && e.secret_exit();
    if o.snapshot.is_some() {
        snapshot(&w, m, &map_name, e, agent, o.focus)?;
        return Ok(MapRun { outcome: outcome.to_string(), detail, carry, secret, damage: hurt, stats, engine: None });
    }
    Ok(MapRun { outcome: outcome.to_string(), detail, carry, secret, damage: hurt, stats, engine: Some(e) })
}

#[cfg(feature = "viewer")]
fn snapshot(w: &Wad, m: Arc<MapData>, map_name: &str, e: Engine, agent: Agent, focus: Option<(f64, f64, f64)>) -> Result<(), String> {
    crate::viewer::save_snapshot(w, m, map_name, e, agent, focus)
}

#[cfg(not(feature = "viewer"))]
fn snapshot(_: &Wad, _: Arc<MapData>, _: &str, _: Engine, _: Agent, _: Option<(f64, f64, f64)>) -> Result<(), String> {
    Err("built without the viewer (the `viewer` feature)".into())
}
