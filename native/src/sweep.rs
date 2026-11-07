//! Running the explorer through a level (knowing only what she has seen) and reporting how it
//! went; `evil sweep` runs many at once, a process a map (the game is one a process).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use crate::knowledge::model::Note;
use crate::map::MapData;
use crate::replay::{Entry, Recording};
use crate::config::{out_dir, resolve_wad};
use crate::engine::{Engine, Options};
use crate::knowledge::completion::{most_items, COUNTED};
use crate::knowledge::explorer::Explorer;
use crate::knowledge::model::Knowledge;
use crate::play::Agent;
use crate::motor::{Controls, Observation, TICRATE};
use crate::nav::Heights;
use crate::navigator::{Navigator, States};
use crate::walkthrough::{plan_from, LevelPlan, Rest};
use crate::wad::{load_map, Wad};

/// nothing to do this long (with lives left): start the level again
const STRANDED_TICS: i64 = 60 * 35;

/// Levels with secrets nobody can get: how many short of the tally counts as all (E4M3: two
/// torch sectors no one can stand in; E4M7: one; MAP15's #9 and MAP27's #5: teleporter sectors
/// the teleport line keeps everyone out of).
fn broken_secrets(map: &str) -> i32 {
    match map {
        "E4M3" => 2,
        "E4M7" | "MAP15" | "MAP27" => 1,
        _ => 0,
    }
}

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
    /// the game's random numbers from this; None: the clock
    pub seed: Option<u32>,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { max_seconds: 3600.0, mortal: false, lives: i32::MAX, peek: false, secrets: false, expert: false, fight: 0, seed: Some(0), snapshot: None, focus: None, from: None, stall_seconds: 300.0 }
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
            let off: BTreeSet<usize> = path.polys.iter().map(|&p| mesh.poly(p).sector).filter(|&s| real.get(s).is_some_and(|h| !believed[s].contains(h))).collect();
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
fn hold_exit(e: &Engine, agent: &mut Agent, o: &RunOptions, secrets_short: i32, items_short: i32) {
    let tally = e.tally();
    if o.secrets {
        agent.x_mut().exit_last = tally[2].0 < tally[2].1 - secrets_short;
    } else if o.expert {
        agent.x_mut().exit_last = tally[0].0 < tally[0].1 - items_short;
    }
}

/// (recordings of different kinds of run kept apart)
fn mode_suffix(o: &RunOptions) -> String {
    let mut out = String::new();
    for (on, name) in [(o.fight > 0, "_fight"), (o.expert, "_expert"), (o.secrets, "_secrets"), (o.peek, "_peek")] {
        if on {
            out.push_str(name);
        }
    }
    out
}

pub fn plan_here(nav: &Rc<Navigator>, obs: &Observation, max_seconds: f64) -> LevelPlan {
    let view = nav.full_view();
    let states: States = obs.heights.iter().map(|&h| vec![h]).collect();
    plan_from(&view, obs.pos(), &Rest { keys: obs.keys.unwrap_or_default(), states: Some(&states), reach: None, max_seconds, max_expansions: 20000, fired: &BTreeSet::new() })
}


/// Play one map; (outcome, detail). Writes the log to out/explore/<IWAD>_<MAP>.txt.
/// A level set up to play: the game started, Evil in it, and (`--from`) played back to a
/// moment of a recorded run. `drawn`: the game draws its frames (for pictures).
pub struct Setup {
    pub iwad: PathBuf,
    pub w: Wad,
    pub m: Arc<MapData>,
    pub map_name: String,
    pub skill: i32,
    pub nav: Rc<Navigator>,
    pub e: Engine,
    pub agent: Agent,
    pub deaths: i32,
    /// the last tic's controls, the last observation
    pub c: Option<Controls>,
    pub obs: Option<Observation>,
    /// the run so far, as its inputs
    pub rec: Recording,
    pub secrets_short: i32,
}

pub fn setup(wad: &str, map_name: &str, o: &RunOptions, drawn: bool) -> Result<Setup, String> {
    let iwad = resolve_wad(wad);
    let w = Wad::open(&iwad)?;
    let map_name = map_name.to_uppercase();
    let m = Arc::new(load_map(&w, &map_name, &[])?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    let mut e = Engine::new(&iwad, &map_name, &Options { skill, nomonsters: o.fight == 0, objects: o.fight > 0 || o.expert, drawn, seed: o.seed })?;
    e.start();
    if !o.mortal {
        e.set_god();
    }
    let knowledge = Knowledge::new(Rc::clone(&nav), o.mortal, !o.peek);
    let mut agent = Agent::new(&nav, knowledge, o.fight, o.expert, o.lives);
    let secrets_short = broken_secrets(&map_name);
    if o.secrets {
        agent.x_mut().exit_last = true;
    }
    let mut c: Option<Controls> = None;
    let mut obs: Option<Observation> = None;
    let mut deaths = 0;
    let skill_used = skill;
    let mut rec = Recording::default();
    if let Some((path, at)) = &o.from {
        if o.fight > 0 {
            return Err("--from: not with monsters (the fight's state isn't played back)".into());
        }
        let run = Recording::read(path)?;
        let to = (at * TICRATE).round() as i64;
        let mut tics = run.entries.iter().filter_map(|en| if let Entry::Tic(t, ctl) = en { Some((*t, *ctl)) } else { None });
        let restarts: BTreeMap<i64, String> = run.entries.iter().filter_map(|en| if let Entry::Restart(t, why) = en { Some((*t, why.clone())) } else { None }).collect();
        let mut notes: BTreeMap<i64, Vec<Note>> = BTreeMap::new();
        for en in &run.entries {
            if let Entry::Note(t, n) = en {
                notes.entry(*t).or_default().push(n.clone());
            }
        }
        while e.tic < to && !e.finished() {
            if let Some(why) = restarts.get(&e.tic) {
                deaths += 1;
                if why == "for what's left" {
                    agent.used_restart();
                }
                rec.entries.push(Entry::Restart(e.tic, why.clone()));
                e.restart(true);
                agent.restart();
                c = None;
                continue;
            }
            if e.tic % 35 == 0 {
                hold_exit(&e, &mut agent, o, secrets_short, most_items(&m, skill));
            }
            let ob = e.observe();
            agent.x_mut().knowledge.update(&ob, c.as_ref());
            for n in notes.remove(&ob.tic).unwrap_or_default() {
                agent.x_mut().knowledge.note(n);
            }
            let Some((t, ctl)) = tics.next() else { return Err(format!("{}: ends before {at}s", path.display())) };
            if t != ob.tic {
                return Err(format!("{}: tic {t} where {} was expected (a different level, options or engine?)", path.display(), ob.tic));
            }
            rec.entries.push(Entry::Tic(t, ctl));
            e.step(&ctl);
            c = Some(ctl);
            obs = Some(ob);
        }
        let n = agent.x().knowledge.journal.len();
        rec.entries.extend(agent.x().knowledge.journal[..n].iter().map(|(t, nt)| Entry::Note(*t, nt.clone())));
        agent.x_mut().resume(e.tic);
    }
    Ok(Setup { iwad, w, m, map_name, skill: skill_used, nav, e, agent, deaths, c, obs, rec, secrets_short })
}

pub fn run_map(wad: &str, map_name: &str, o: &RunOptions) -> Result<(String, String), String> {
    let Setup { iwad, w, m, map_name, skill, nav, mut e, mut agent, mut deaths, mut c, mut obs, mut rec, secrets_short } = setup(wad, map_name, o, o.snapshot.is_some())?;
    let t0 = Instant::now();
    let mut idle_since: Option<i64> = None;
    let (mut t_engine, mut t_observe, mut t_think) = (0.0, 0.0, 0.0);
    let mut journal_n = agent.x().knowledge.journal.len();
    let until = o.snapshot.map_or(o.max_seconds, |s| s.min(o.max_seconds));
    // (the 100% runs: the count she's after and what she knows of the level, and when either
    // last grew: exploring a teleporter maze before any items is getting somewhere, MAP24)
    let items_short = most_items(&m, skill);
    let progress = |e: &Engine, x: &Explorer| {
        let k = &x.knowledge;
        (if o.secrets { e.tally()[2].0 } else { e.tally()[0].0 }, k.sectors.len(), k.learned.values().map(BTreeSet::len).sum::<usize>())
    };
    // (the best so far over the whole run: a level again collecting the same things isn't more)
    let (mut best, mut best_tic) = (progress(&e, agent.x()), e.tic);
    let mut stalled = false;
    // (the most items and secrets one life got: a level again starts the tally afresh)
    let mut best_life = (0, 0);
    while (e.tic as f64) < until * TICRATE {
        if o.expert || o.secrets {
            let now = progress(&e, agent.x());
            if now.0 > best.0 || now.1 > best.1 || now.2 > best.2 {
                best = (now.0.max(best.0), now.1.max(best.1), now.2.max(best.2));
                best_tic = e.tic;
            } else if (e.tic - best_tic) as f64 > o.stall_seconds * TICRATE {
                agent.x_mut().log.push(format!("tic {}: nothing more collected or learned in {:.0}s: over", e.tic, o.stall_seconds));
                stalled = true;
                break;
            }
        }
        let mut stranded = idle_since.is_some_and(|i| e.tic - i > STRANDED_TICS) || agent.x().restart_wanted;
        if let (true, Some(ob), Some(p)) = (e.finished() && e.dead(), &obs, agent.player_mut()) {
            let pos = ob.pos();
            let seen: Vec<String> = p.combat().visible.iter().map(|t| format!("{} {:.0} away", t.name, crate::geom::dist((t.x, t.y), pos))).collect();
            let doing = p.explorer.current.as_ref().map_or("-".to_string(), |c| c.label.clone());
            let msg = format!(
                "tic {}: killed: health {:.0}, armor {:.0}, weapon {}, weapons {:?}, ammo {:?}; in sight: {}; doing: {doing}",
                e.tic,
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
                        let head = format!("  stranded at ({:.0}, {:.0}), objective {label}; omniscient plan from there: {}", ob.x, ob.y, if lp.complete { "ok".to_string() } else { lp.problem.clone() });
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
                continue;
            }
            break;
        }
        if e.tic % 35 == 0 {
            hold_exit(&e, &mut agent, o, secrets_short, items_short);
        }
        let t1 = Instant::now();
        let ob = e.observe();
        let t2 = Instant::now();
        let ctl = agent.act(&ob, c.as_ref());
        let t3 = Instant::now();
        if agent.x().current.is_none() {
            idle_since.get_or_insert(e.tic);
        } else {
            idle_since = None;
        }
        e.step(&ctl);
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
    let (exited, died, game_s) = (e.exited(), e.finished() && e.dead(), e.tic as f64 / TICRATE);
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
    lines.push(format!("RESULT {outcome} {detail}"));
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let stem = iwad.file_stem().map_or_else(|| "WAD".to_string(), |s| s.to_string_lossy().to_uppercase());
    let _ = std::fs::write(dir.join(format!("{stem}_{map_name}.txt")), lines.join("\n") + "\n");
    {
        let k = &agent.x().knowledge;
        rec.entries.extend(k.journal[journal_n.min(k.journal.len())..].iter().map(|(t, nt)| Entry::Note(*t, nt.clone())));
    }
    let branch = if o.from.is_some() { "_branch" } else { "" };
    rec.write(&out_dir().join("runs").join(format!("{stem}_{map_name}{}{branch}.run", mode_suffix(o))))?;
    if o.snapshot.is_some() {
        snapshot(&w, m, &map_name, e, agent, o.focus)?;
    }
    Ok((outcome.to_string(), detail))
}

#[cfg(feature = "viewer")]
fn snapshot(w: &Wad, m: Arc<MapData>, map_name: &str, e: Engine, agent: Agent, focus: Option<(f64, f64, f64)>) -> Result<(), String> {
    crate::viewer::save_snapshot(w, m, map_name, e, agent, focus)
}

#[cfg(not(feature = "viewer"))]
fn snapshot(_: &Wad, _: Arc<MapData>, _: &str, _: Engine, _: Agent, _: Option<(f64, f64, f64)>) -> Result<(), String> {
    Err("built without the viewer (the `viewer` feature)".into())
}
