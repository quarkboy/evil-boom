//! `evil-boom`: Evil from the command line.
//!
//!   evil-boom sweep <wad> [maps...] [<wad> [maps...]] [--jobs N] [run options]
//!       many maps at once, a process a map; a line each as they finish, then a summary
//!   evil-boom run-map <wad> <map> [run options]      one map; its log goes to out/explore
//!   evil-boom episode <wad> <first map> [--seeds K] [run options]   an episode on one life from that
//!                                               map, what she has carried level to level; K seeds
//!                                               (8) side by side, --jobs at a time
//!   evil-boom score [doom|doom2 [maps...]]... [--skills 3,4] [--seeds K] [--jobs N] [--out FILE]
//!       the bot's score: every level, pistol start, one life, K random seeds a skill (HMP and
//!       UV, 8); a row a run (out/score/), then the report (see score.rs)
//!       --like FILE: the very runs of an earlier set (its levels, skills and seeds); --tuning FILE
//!       and --bar NAMES go to every run
//!   evil-boom score-report FILE...                   the report of score TSVs (more: how far apart;
//!                                               two of the same runs: run for run)
//!   evil-boom score-watch FILE [--total N]           a score's progress in a small window (`score`
//!                                               opens one unless --no-window)
//!   evil-boom tuning                                 the numbers she judges a fight by, as JSON: a file
//!                                               to change and give a run with --tuning FILE
//!   evil-boom dump-nav <wad> <map>                   the level's analysis (for comparing)
//!   evil-boom neuro-mock [--port N] [FILE]           a stand-in Neuro server (port 8000): actions sent
//!                                               from FILE (out/neuro/actions.txt) as lines are added,
//!                                               everything the game sends shown and in FILE.log
//!   evil-boom secret-survey <wad> <map>... [--jobs N]   the maps' secrets: the lines pressed or shot
//!       to reveal each, and how the wall rubric scores them and every other wall once she's
//!       explored all she can (out/survey/<WAD>.tsv, .txt)
//!   evil-boom neuro-gpt [--port N] [--config FILE]   a language model as Evil (config/neuro-gpt.json, and
//!                                               config/neuro-gpt.local.json for the key): log in
//!                                               out/neuro/gpt.log
//!   evil-boom follow <wad> <map>                     plan the level, follow the plan (seed 0)
//!   evil-boom drive <wad> <map> --script FILE [run options]   drive her by hand (see drive.rs)
//!   evil-boom sector <wad> <map> <sector>...         sectors as the map has them: heights, lines, things,
//!                                               the run-jumps landing there (and their check)
//!   evil-boom arena FILE [names...] [--jobs N] [--seeds K]
//!       the fights set up in FILE (see arena.rs), each K times (seeds 0..K), a process a go;
//!       a line each, then how each scenario went: won, lost, drawn
//!   evil-boom arena-one FILE NAME [--seed N]         one of them, once; its log to out/arena/NAME.txt
//!   evil-boom arena-watch FILE NAME [--seed N] [--speed X] [--mute]   one of them, watched (space
//!                                               pause, N a tic at a time, +/- speed, M mute)
//!   evil-boom record <wad> <map> <out.mp4> [--view game|full|hud] [--size WxH] [--speed N] [--episode]
//!       [--crf N] [--preset P] [--ffmpeg PATH] [--gpt [CONFIG]] [--neuro URL] [run options]   a watch
//!       played as fast as it goes into an MP4 (ffmpeg, H.264: CRF 20, veryfast unless told);
//!       directed (--gpt, --neuro, --directed), in real time; at x1, with the game's sound
//!   evil-boom watch <wad> <map> [--speed X] [--all] [--episode] [--mute] [--directed FILE] [--neuro URL] [--gpt [CONFIG]] [--headless] [run options]
//!       watch her play, with what she knows beside the game, the game's sound on (space pause,
//!       +/- speed, A the whole map, F follow, M mute, L Evil's log, T text size, wheel zoom, Esc quit; the sound off while
//!       paused or not at x1). --directed FILE: directed play, orders read from FILE as lines are
//!       added to it (list, name, order: see control::script), what comes back in the log and in
//!       FILE.out. --neuro URL: directed by Neuro over its WebSocket (ws://127.0.0.1:8000: Neuro,
//!       or evil-boom neuro-mock). --gpt [CONFIG]: directed by the GPT stand-in, run in this
//!       program (config/neuro-gpt.json unless named; its log out/neuro/gpt.log)
//!
//! Run options: --lives N (unlimited), --max-seconds S (3600, all lives together), --seed N|random (0), --peek, --secrets,
//! --expert, --fight SKILL, --mortal, --complete (every kill, item and secret, then out: --fight 4
//! --expert --secrets together, the exit held while any tally's short), --stall S (the 100% runs: over after S game seconds, 300
//! unless told, with nothing more collected or learned); --snapshot S (stop after S game seconds and draw the
//! viewer's window to out/watch.png; nothing's drawn till then) and --focus X,Y,ZOOM (its map
//! centred there); --from FILE@S (play back the run recorded in FILE, out/runs/<IWAD>_<MAP>.run,
//! which every run-map writes (_expert etc. by the kind of run), to S game seconds, then the bot as it is now: the run from there
//! goes to <IWAD>_<MAP>_branch.run).

mod cli;
use cli::{option_args, run_options};
use evildoom::config::resolve_wad;
use evildoom::navigator::Navigator;
use evildoom::specials::classify_line;
#[cfg(feature = "viewer")]
use evildoom::sweep::run_map;
use evildoom::wad::{load_map, Wad};
use std::collections::{BTreeSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitCode, Stdio};
use std::rc::Rc;
use std::sync::Arc;

fn dump_nav(wad: &str, map: &str) -> Result<(), String> {
    let wad = Wad::open(&resolve_wad(wad))?;
    let m = Arc::new(load_map(&wad, map, &[])?);
    let mut nav = Navigator::new(Arc::clone(&m), 3)?;
    println!("jumps found {}", nav.jumps.len());
    let checks = nav.keep_verified();
    println!("jumps kept {} {checks:?}", nav.jumps.len());
    println!("polys {} portals {} fits {}", nav.mesh.polys.len(), nav.mesh.portals.len(), nav.portal_fits.iter().filter(|&&f| f).count());
    println!("trenches {:?}", nav.trenches);
    for t in &nav.triggers {
        let mut eff: Vec<String> = t.effects.iter().map(|(s, hs)| format!("{s}:{:?}", hs.iter().map(|h| (h.floor, h.ceiling)).collect::<Vec<_>>())).collect();
        eff.sort();
        println!("trigger {} {} {:?} act={:?} eff={:?} rel={:?}", t.line, t.info.description, t.sectors, t.activators, eff, t.relevant);
    }
    for tp in &nav.teleports {
        println!("teleport {} {:?} -> {} ({:.0}, {:.0})", tp.line, tp.src_polys, tp.dest_poly, tp.dest.0, tp.dest.1);
    }
    let mut ug: Vec<_> = nav.use_gates.iter().collect();
    ug.sort();
    println!("use_gates {ug:?}");
    let mut ulg: Vec<_> = nav.use_line_gates.iter().collect();
    ulg.sort();
    println!("use_line_gates {ulg:?}");
    let mut sf: Vec<_> = nav.shot_floors.iter().map(|(k, v)| (*k, *v)).collect();
    sf.sort_by_key(|a| a.0);
    println!("shot_floors {sf:?}");
    let nav = Rc::new(nav);
    let start = m.things.iter().find(|t| t.kind == 1).map(|t| (t.x, t.y)).ok_or("no player start")?;
    let view = nav.full_view();
    let sp = nav.poly_at(start).ok_or("start off the map")?;
    let reach = view.reachable(sp, &nav.initial_states(), None);
    println!("reach from start {}", reach.len());
    let t0 = std::time::Instant::now();
    let plan = evildoom::walkthrough::plan_level(&view, start, 60.0, 20000);
    eprintln!("planned in {:.2}s", t0.elapsed().as_secs_f64());
    println!("plan complete={} problem={:?}", plan.complete, plan.problem);
    for (i, st) in plan.steps.iter().enumerate() {
        println!("step {i}: {} | {}", st.label, st.substeps().join(" -> "));
    }
    Ok(())
}

/// What the map says about some sectors: heights, special and tag, each line round them (with
/// what's on the other side), the lines whose tag moves them, and the things in them.
fn sector(wad: &str, map: &str, sectors: &[String]) -> Result<(), String> {
    let w = Wad::open(&resolve_wad(wad))?;
    let m = load_map(&w, map, &[])?;
    let nav = Navigator::new(Arc::new(m), 3)?;
    let m = &nav.map;
    let places = evildoom::control::places::Places::build(&nav);
    for arg in sectors {
        let si: usize = arg.parse().map_err(|_| format!("not a sector: {arg}"))?;
        let s = m.sectors.get(si).ok_or_else(|| format!("no sector {si}"))?;
        println!(
            "sector {si}: floor {} ceiling {} ({} / {}), light {}, special {}, tag {}",
            s.floor, s.ceiling, s.floor_tex, s.ceiling_tex, s.light, s.special, s.tag
        );
        let area = places.area_of(si);
        println!("  in {} ({}, {:.0} square units): sectors {:?}", area.id, evildoom::control::places::Places::sketch(area), area.size, area.sectors);
        for l in &m.linedefs {
            let (front, back) = (m.front_sector(l), m.back_sector(l));
            if front != Some(si) && back != Some(si) {
                continue;
            }
            let other = if front == Some(si) { back } else { front };
            let side = |sd: u16| {
                m.sidedefs.get(usize::from(sd)).map_or_else(String::new, |d| format!("[{} {} {} @{},{}]", d.upper, d.middle, d.lower, d.x_offset, d.y_offset))
            };
            let (a, b) = (m.line_seg(l.index).0, m.line_seg(l.index).1);
            let what = classify_line(l.special).map_or_else(String::new, |i| format!(" {}", i.description));
            println!(
                "  line {} ({:.0},{:.0})-({:.0},{:.0}) flags {:#x} special {}{what} tag {} -> {} front {} back {}",
                l.index,
                a.0,
                a.1,
                b.0,
                b.1,
                l.flags,
                l.special,
                l.tag,
                other.map_or_else(|| "-".to_string(), |o| format!("sector {o} ({}..{})", m.sectors[o].floor, m.sectors[o].ceiling)),
                side(l.front),
                side(l.back)
            );
        }
        if s.tag != 0 {
            for l in m.linedefs.iter().filter(|l| l.tag == s.tag && l.special != 0) {
                let what = classify_line(l.special).map_or_else(String::new, |i| i.description);
                println!("  moved by line {} ({what}) between sectors {:?}/{:?}", l.index, m.front_sector(l), m.back_sector(l));
            }
        }
        for t in m.things.iter().filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| nav.mesh.polys[p].as_ref().is_some_and(|q| q.sector == si))) {
            println!("  thing {} at ({}, {}) flags {:#x}", t.kind, t.x, t.y, t.flags);
        }
        // (the navmesh's portals along its lines: where the mesh cuts an edge)
        for (pi, portal) in nav.mesh.portals.iter().enumerate() {
            if portal.linedef.is_some_and(|li| m.front_sector(&m.linedefs[li]) == Some(si) || m.back_sector(&m.linedefs[li]) == Some(si)) {
                println!("  portal {pi} on line {:?}: ({:.0},{:.0})-({:.0},{:.0})", portal.linedef, portal.p1.0, portal.p1.1, portal.p2.0, portal.p2.1);
            }
        }
        // (the run-jumps found landing here, and what the movement check makes of them)
        let into: Vec<&evildoom::navigator::Jump> = nav.jumps.iter().filter(|j| nav.mesh.polys[j.dest_poly].as_ref().is_some_and(|q| q.sector == si)).collect();
        let lab: Vec<evildoom::jumps::Jump> = into
            .iter()
            .map(|j| evildoom::jumps::Jump {
                src_polys: j.src_polys.iter().copied().collect(),
                dest_poly: j.dest_poly,
                dest: j.dest,
                runup: j.runup,
                takeoff: j.takeoff,
                speed: 0.0,
                requires: j.requires.clone(),
                strafe: j.strafe,
            })
            .collect();
        for (j, verdict) in into.iter().zip(nav.jump_lab.check_jumps(&lab)) {
            let from: BTreeSet<usize> = j.src_polys.iter().filter_map(|&p| nav.mesh.polys[p].as_ref().map(|q| q.sector)).collect();
            println!(
                "  jump in from sectors {from:?}: run-up ({:.0}, {:.0}), takeoff ({:.0}, {:.0}), lands ({:.0}, {:.0}): {verdict}",
                j.runup.0, j.runup.1, j.takeoff.0, j.takeoff.1, j.dest.0, j.dest.1
            );
        }
    }
    Ok(())
}

/// Plan the level from the start and follow the plan in the game (seed 0): the follower's log.
fn follow(wad: &str, map: &str) -> Result<(), String> {
    use evildoom::engine::{Engine, Options};
    use evildoom::motor::follower::RouteFollower;
    let iwad = resolve_wad(wad);
    let w = Wad::open(&iwad)?;
    let m = Arc::new(load_map(&w, map, &[])?);
    let mut nav = Navigator::new(Arc::clone(&m), 3)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    let view = std::rc::Rc::new(nav.full_view());
    let start = m.things.iter().find(|t| t.kind == 1).map(|t| (t.x, t.y)).ok_or("no player start")?;
    let plan = evildoom::walkthrough::plan_level(&view, start, 60.0, 20000);
    if !plan.complete {
        return Err(plan.problem);
    }
    let mut e = Engine::new(&iwad, map, &Options { seed: Some(0), ..Options::default() })?;
    e.start();
    e.set_god();
    let mut f = RouteFollower::new(view, plan.steps, Default::default(), None, true, e.tic);
    while !e.finished() && !f.failed && !f.done && e.tic < 600 * 35 {
        let obs = e.observe();
        let c = f.act(&obs);
        e.step(&c);
    }
    for ev in &f.log.events {
        println!("{ev}");
    }
    println!("exited={} tic={} replans={}", e.exited(), e.tic, f.replans);
    Ok(())
}

fn run_one(args: &[String]) -> Result<(), String> {
    let (o, _, rest) = run_options(args)?;
    let [wad, map] = rest.as_slice() else { return Err("usage: evil-boom run-map <wad> <map> [options]".into()) };
    let r = evildoom::sweep::run_map_full(wad, map, &o)?;
    // (what she got out with, for the next level of an episode; the secret exit taken)
    if let Some(l) = &r.carry {
        println!("CARRY {}{}", cli::carry_arg(l), if r.secret { " SECRET" } else { "" });
    }
    println!("DAMAGE {}", r.damage);
    println!("STATS {}", r.stats);
    println!("RESULT {} {}", r.outcome, r.detail);
    Ok(())
}

/// Episodes played through on one life from `first`, a run each for `--seeds K` seeds (8 unless
/// told), `--jobs` of them at once: each level begun with what she got out of the one before
/// (weapons, ammo, the backpack, health, armour; not keys or powers), on by the exit taken (the
/// secret exit to the secret level), till she dies, gets stuck, or the episode ends. A process a
/// level; each seed's levels one after another, the seeds side by side.
fn episode(args: &[String]) -> Result<(), String> {
    let (mut o, jobs, rest) = run_options(args)?;
    let mut seeds = 8u32;
    let mut names = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--seeds" {
            seeds = it.next().and_then(|v| v.parse().ok()).ok_or("--seeds K")?;
        } else {
            names.push(a.clone());
        }
    }
    let [wad, first] = names.as_slice() else { return Err("usage: evil-boom episode <wad> <first map> [--seeds K] [run options]".into()) };
    o.lives = 1;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let base = o.seed.unwrap_or(0);
    let todo = std::sync::Mutex::new((base..base + seeds).collect::<VecDeque<u32>>());
    let done = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..jobs.clamp(1, seeds as usize) {
            s.spawn(|| {
                while let Some(seed) = todo.lock().ok().and_then(|mut q| q.pop_front()) {
                    let mut os = o.clone();
                    os.seed = Some(seed);
                    let (levels, end) = episode_chain(&me, wad, first, &os);
                    if let Ok(mut d) = done.lock() {
                        d.push((seed, levels, end));
                    }
                }
            });
        }
    });
    let mut done = done.into_inner().map_err(|e| e.to_string())?;
    done.sort_by_key(|d| d.0);
    println!();
    for (seed, levels, end) in &done {
        let mut os = o.clone();
        os.seed = Some(*seed);
        println!("seed {seed}: {levels} levels, {end}");
        println!("    to watch it all: evil-boom watch {wad} {first} {} --episode", option_args(&os).join(" "));
    }
    let mean = done.iter().map(|d| d.1 as f64).sum::<f64>() / done.len().max(1) as f64;
    let through = done.iter().filter(|d| d.2 == "the episode through").count();
    println!();
    println!("{mean:.1} levels on one life on average; the episode through in {through}/{}", done.len());
    Ok(())
}

/// One episode on one life (see `episode`), in a process of its own (`episode-one`): a line for
/// each level as it ends, then how many levels she got through, and how it ended.
fn episode_chain(me: &std::path::Path, wad: &str, first: &str, o: &evildoom::sweep::RunOptions) -> (usize, String) {
    let seed = o.seed.unwrap_or(0);
    let Ok(mut child) = Command::new(me).arg("episode-one").arg(wad).arg(first).args(option_args(o)).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return (0, "couldn't run".into());
    };
    let mut end = (0, "no result".to_string());
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(l) = line.strip_prefix("LEVEL ") {
                println!("seed {seed} {l}");
            } else if let Some(l) = line.strip_prefix("DAMAGE ") {
                println!("    damage taken: {l}");
            } else if let Some(l) = line.strip_prefix("WATCH ") {
                println!("    to watch: {l}");
            } else if let Some(l) = line.strip_prefix("END ") {
                let (n, how) = l.split_once(' ').unwrap_or((l, ""));
                end = (n.parse().unwrap_or(0), how.to_string());
            }
        }
    }
    let _ = child.wait();
    end
}

/// One episode on one life in this process, the game going on from level to level as Doom's
/// does (what she gets out with carried; see `episode`). Lines: LEVEL (each level's result),
/// WATCH (how to watch the level it ended on), END (levels got through, how it ended).
fn episode_one(args: &[String]) -> Result<(), String> {
    let (mut o, _, rest) = run_options(args)?;
    let [wad, first] = rest.as_slice() else { return Err("usage: evil-boom episode-one <wad> <first map> [run options]".into()) };
    o.lives = 1;
    let mut map = Some(first.to_uppercase());
    let mut engine = None;
    let mut levels = 0;
    while let Some(m) = map.take() {
        let r = evildoom::sweep::run_map_in(wad, &m, &o, engine.take())?;
        let line = format!("{m}: {} {}", r.outcome, r.detail.chars().take(70).collect::<String>());
        let Some(l) = r.carry else {
            println!("LEVEL {line}");
            println!("DAMAGE {}", r.damage);
            let mut os = o.clone();
            os.carry = None;
            println!("WATCH evil-boom watch {wad} {first} {} --episode", option_args(&os).join(" "));
            println!("END {levels} {m}: {}", r.outcome);
            return Ok(());
        };
        levels += 1;
        println!("LEVEL {line} | out with health {} armor {}, ammo {}/{}/{}/{}", l.health, l.armor, l.clip, l.shell, l.cell, l.rocket);
        o.carry = Some(l);
        engine = r.engine;
        map = evildoom::sweep::next_map(&m, r.secret);
    }
    println!("END {levels} the episode through");
    Ok(())
}

fn drive(args: &[String]) -> Result<(), String> {
    let (o, _, rest) = run_options(args)?;
    let mut names = Vec::new();
    let mut script = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--script" {
            script = it.next().cloned();
        } else {
            names.push(a.clone());
        }
    }
    let ([wad, map], Some(script)) = (names.as_slice(), script) else { return Err("usage: evil-boom drive <wad> <map> --script FILE [run options]".into()) };
    evildoom::drive::drive(wad, map, &o, std::path::Path::new(&script))
}

fn sweep(args: &[String]) -> Result<(), String> {
    let (o, jobs, rest) = run_options(args)?;
    // (wad, maps named after it)
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for t in rest {
        let lower = t.to_lowercase();
        if lower == "doom" || lower == "doom2" || lower.ends_with(".wad") {
            groups.push((t, Vec::new()));
        } else if let Some(g) = groups.last_mut() {
            g.1.push(t.to_uppercase());
        } else {
            return Err(format!("{t}: name a WAD before its maps"));
        }
    }
    let mut todo: VecDeque<(String, String)> = VecDeque::new();
    for (w, maps) in groups {
        let maps = if maps.is_empty() { Wad::open(&resolve_wad(&w))?.map_names() } else { maps };
        todo.extend(maps.into_iter().map(|m| (w.clone(), m)));
    }
    let total = todo.len();
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut running: Vec<(String, String, Child)> = Vec::new();
    let mut results: Vec<String> = Vec::new();
    while !todo.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some((w, m)) = todo.pop_front() else { break };
            let child = Command::new(&me)
                .arg("run-map")
                .arg(&w)
                .arg(&m)
                .args(option_args(&o))
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            running.push((w, m, child));
        }
        let mut finished = None;
        for (i, (_, _, child)) in running.iter_mut().enumerate() {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                finished = Some(i);
                break;
            }
        }
        let Some(i) = finished else {
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        };
        let (w, m, mut child) = running.swap_remove(i);
        let line = child
            .stdout
            .take()
            .map(BufReader::new)
            .and_then(|r| r.lines().map_while(Result::ok).find(|l| l.starts_with("RESULT ")))
            .unwrap_or_else(|| "RESULT ERROR (no result)".into());
        let line = line.trim_start_matches("RESULT ").to_string();
        let outcome = line.split(' ').next().unwrap_or("ERROR").to_string();
        results.push(outcome);
        println!("[{:3}/{total}] {w} {m}: {line}", results.len());
    }
    let done = results.iter().filter(|o| matches!(o.as_str(), "EXITED" | "EXPERT" | "SECRETS" | "COMPLETE")).count();
    println!("\n{done}/{} exited", results.len());
    if o.expert {
        println!("{}/{} with every item", results.iter().filter(|o| *o == "EXPERT").count(), results.len());
    }
    if o.secrets {
        println!("{}/{} with every secret", results.iter().filter(|o| *o == "SECRETS").count(), results.len());
    }
    if o.complete {
        println!("{}/{} with every kill, item and secret", results.iter().filter(|o| *o == "COMPLETE").count(), results.len());
    }
    Ok(())
}

#[cfg(feature = "viewer")]
fn watch(args: &[String]) -> Result<(), String> {
    let (mut w, names, o) = watch_options(args)?;
    let [wad, map] = names.as_slice() else { return Err("usage: evil-boom watch <wad> <map> [options]".into()) };
    if let Some(config) = &w.gpt {
        // (the stand-in in this program, on a port of its own: one program to run, the user's)
        w.neuro = Some(evildoom::neuro::gpt::start(config)?);
    }
    if o.snapshot.is_some() {
        // (a picture: the run as a sweep plays it, nothing drawn till then)
        let (outcome, detail) = run_map(wad, map, &o)?;
        println!("RESULT {outcome} {detail}");
        return Ok(());
    }
    if w.headless {
        // (SDL takes Ctrl-C and SIGTERM for its own quit event, which nothing headless reads: left
        // to the shell, they stop it)
        std::env::set_var("SDL_NO_SIGNAL_HANDLERS", "1");
    }
    evildoom::viewer::watch(wad, map, &w)
}

/// A run watched into an MP4 (see `viewer::record`): evil-boom record <wad> <map> <out.mp4>
/// [--view game|full|hud] [--size WxH] [--speed N] [--ffmpeg PATH] [--episode] [run options].
#[cfg(feature = "viewer")]
fn record(args: &[String]) -> Result<(), String> {
    use evildoom::viewer::{RecordOptions, RecordView};
    let (mut view, mut size, mut every, mut ffmpeg) = (RecordView::Hud, None, 1u32, None);
    let (mut crf, mut preset) = (20u32, "veryfast".to_string());
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--view" => {
                view = match it.next().map(String::as_str) {
                    Some("game") => RecordView::Game,
                    Some("full") => RecordView::Full,
                    Some("hud") => RecordView::Hud,
                    _ => return Err("--view game|full|hud".into()),
                };
            }
            "--size" => {
                let v = it.next().ok_or("--size WxH")?;
                let (x, y) = v.split_once(['x', 'X']).ok_or("--size WxH")?;
                size = Some((x.parse().map_err(|_| "--size WxH")?, y.parse().map_err(|_| "--size WxH")?));
            }
            "--speed" => every = it.next().and_then(|v| v.parse().ok()).ok_or("--speed N (whole times as fast)")?,
            "--ffmpeg" => ffmpeg = Some(std::path::PathBuf::from(it.next().ok_or("--ffmpeg PATH")?)),
            "--crf" => crf = it.next().and_then(|v| v.parse().ok()).ok_or("--crf N (0-51; 20 unless told)")?,
            "--preset" => preset = it.next().ok_or("--preset NAME (ultrafast .. veryslow)")?.clone(),
            _ => rest.push(a.clone()),
        }
    }
    let (mut w, names, _) = watch_options(&rest)?;
    let [wad, map, out] = names.as_slice() else { return Err("usage: evil-boom record <wad> <map> <out.mp4> [--view game|full|hud] [--size WxH] [--speed N] [--episode] [options]".into()) };
    // (the game's sound, at x1: each tic's beside each frame)
    w.capture_sound = every == 1;
    if let Some(config) = &w.gpt {
        // (the stand-in in this program, as watch has it; directed, it plays in real time)
        w.neuro = Some(evildoom::neuro::gpt::start(config)?);
    }
    let ffmpeg = ffmpeg.or_else(evildoom::viewer::find_ffmpeg).ok_or("no ffmpeg found (on the path, or winget's on Windows): --ffmpeg PATH")?;
    evildoom::viewer::record(wad, map, &w, &RecordOptions { out: out.into(), view, size, every, crf, preset, ffmpeg })
}

/// The watcher's options among `args`: (them, the names left, the run options).
#[cfg(feature = "viewer")]
fn watch_options(args: &[String]) -> Result<(evildoom::viewer::WatchOptions, Vec<String>, evildoom::sweep::RunOptions), String> {
    let (o, _, rest) = run_options(args)?;
    let mut w = evildoom::viewer::WatchOptions {
        speed: 1.0,
        show_all: false,
        mortal: o.mortal,
        lives: o.lives,
        fight: o.fight,
        peek: o.peek,
        expert: o.expert,
        secrets: o.secrets,
        complete: o.complete,
        stall_seconds: o.stall_seconds,
        carry: o.carry.clone(),
        episode: false,
        tuning: o.tuning.clone(),
        headless: false,
        paced: false,
        max_seconds: o.max_seconds,
        seed: o.seed,
        focus: None,
        sound: true,
        directed: None,
        neuro: None,
        gpt: None,
        capture_sound: false,
    };
    let mut names = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--speed" => {
                w.speed = it.next().and_then(|v| v.parse().ok()).ok_or("--speed: a number")?;
                w.paced = true;
            }
            "--all" => w.show_all = true,
            "--episode" => w.episode = true,
            "--headless" => w.headless = true,
            "--mute" => w.sound = false,
            "--neuro" => w.neuro = Some(it.next().cloned().unwrap_or_else(|| "ws://127.0.0.1:8000".into())),
            "--gpt" => {
                // (its config, if one's named: the next argument, a .json)
                let named = it.as_slice().first().filter(|a| a.ends_with(".json")).cloned();
                if named.is_some() {
                    it.next();
                }
                w.gpt = Some(named.map_or_else(|| evildoom::config::root().join("config").join("neuro-gpt.json"), std::path::PathBuf::from));
            }
            "--directed" => w.directed = Some(std::path::PathBuf::from(it.next().ok_or("--directed FILE (orders, a line each, read as they're added)")?)),
            _ => names.push(a.clone()),
        }
    }
    if w.episode {
        w.lives = 1; // (an episode watched as it's played: one life)
    }
    w.focus = o.focus;
    Ok((w, names, o))
}

#[cfg(not(feature = "viewer"))]
fn watch(_args: &[String]) -> Result<(), String> {
    Err("built without the viewer (the `viewer` feature)".into())
}

/// One arena scenario, once: a RESULT line, its fight's log to out/arena/NAME[_seed].txt.
fn arena_one(args: &[String]) -> Result<(), String> {
    let mut seed: Option<u32> = Some(0);
    let mut rest: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--seed" {
            let v = args.get(i + 1).ok_or("--seed N")?;
            seed = if v == "random" { None } else { Some(v.parse().map_err(|_| "--seed N")?) };
            i += 2;
            continue;
        }
        rest.push(&args[i]);
        i += 1;
    }
    let [file, name] = rest.as_slice() else { return Err("usage: evil-boom arena-one FILE NAME [--seed N]".into()) };
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let scenarios = evildoom::arena::parse(&text)?;
    let s = scenarios.iter().find(|s| &s.name == *name).ok_or_else(|| format!("{name}: not in {file}"))?;
    let mut log = Vec::new();
    let o = evildoom::arena::run(s, seed, &mut log)?;
    let dir = evildoom::config::out_dir().join("arena");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tag = seed.map_or("random".to_string(), |s| s.to_string());
    evildoom::config::write_log(&dir.join(format!("{name}_{tag}.txt")), "", &log.join("\n")).map_err(|e| e.to_string())?;
    println!("RESULT {} {:.0}s health {} armor {} kills {}/{}", o.result, o.seconds, o.health, o.armor, o.kills, o.monsters);
    Ok(())
}

/// One arena scenario, watched in the viewer's window.
#[cfg(feature = "viewer")]
fn arena_watch(args: &[String]) -> Result<(), String> {
    let (mut seed, mut speed, mut sound) = (Some(0), 1.0, true);
    let mut rest: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mute" => {
                sound = false;
                i += 1;
            }
            "--seed" => {
                let v = args.get(i + 1).ok_or("--seed N")?;
                seed = if v == "random" { None } else { Some(v.parse().map_err(|_| "--seed N")?) };
                i += 2;
            }
            "--speed" => {
                speed = args.get(i + 1).and_then(|v| v.parse().ok()).ok_or("--speed X")?;
                i += 2;
            }
            _ => {
                rest.push(&args[i]);
                i += 1;
            }
        }
    }
    let [file, name] = rest.as_slice() else { return Err("usage: evil-boom arena-watch FILE NAME [--seed N] [--speed X] [--mute]".into()) };
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let scenarios = evildoom::arena::parse(&text)?;
    let s = scenarios.iter().find(|s| &s.name == *name).ok_or_else(|| format!("{name}: not in {file}"))?;
    let p = evildoom::arena::prepare(s, seed, true, sound)?;
    evildoom::viewer::watch_prepared(p, &s.map, speed, sound, &format!("evil-doom arena: {name}"))
}

/// The arena's scenarios (those named, else all), each `seeds` times, `jobs` at a time.
fn arena(args: &[String]) -> Result<(), String> {
    let (mut jobs, mut seeds) = (8usize, 3u32);
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--jobs" => {
                jobs = args.get(i + 1).and_then(|v| v.parse().ok()).ok_or("--jobs N")?;
                i += 2;
            }
            "--seeds" => {
                seeds = args.get(i + 1).and_then(|v| v.parse().ok()).ok_or("--seeds K")?;
                i += 2;
            }
            _ => {
                rest.push(args[i].clone());
                i += 1;
            }
        }
    }
    let file = rest.first().ok_or("usage: evil-boom arena FILE [names...] [--jobs N] [--seeds K]")?.clone();
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
    let names: Vec<String> = evildoom::arena::parse(&text)?.into_iter().map(|s| s.name).filter(|n| rest.len() < 2 || rest[1..].contains(n)).collect();
    let mut todo: VecDeque<(String, u32)> = names.iter().flat_map(|n| (0..seeds).map(move |k| (n.clone(), k))).collect();
    let total = todo.len();
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut running: Vec<(String, u32, Child)> = Vec::new();
    let mut tally: std::collections::BTreeMap<String, [u32; 4]> = std::collections::BTreeMap::new();
    // (health left, a lost fight's none; seconds taken, won)
    let mut left: std::collections::BTreeMap<String, (f64, f64)> = std::collections::BTreeMap::new();
    let mut n = 0;
    while !todo.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some((name, k)) = todo.pop_front() else { break };
            let child = Command::new(&me)
                .args(["arena-one", &file, &name, "--seed", &k.to_string()])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            running.push((name, k, child));
        }
        let Some(i) = running.iter_mut().position(|(_, _, c)| c.try_wait().ok().flatten().is_some()) else {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        };
        let (name, k, mut child) = running.swap_remove(i);
        let mut out = String::new();
        if let Some(s) = child.stdout.take() {
            out = BufReader::new(s).lines().map_while(Result::ok).find(|l| l.starts_with("RESULT ")).unwrap_or_default();
        }
        if out.is_empty() {
            let err = child.stderr.take().map(|s| BufReader::new(s).lines().map_while(Result::ok).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            out = format!("RESULT ERROR {err}");
        }
        let line = out.trim_start_matches("RESULT ").to_string();
        let t = tally.entry(name.clone()).or_default();
        let words: Vec<&str> = line.split(' ').collect();
        let after = |k: &str| words.iter().position(|w| *w == k).and_then(|i| words.get(i + 1)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let l = left.entry(name.clone()).or_default();
        if words.first() == Some(&"WON") {
            l.0 += after("health");
            l.1 += words.get(1).and_then(|s| s.trim_end_matches('s').parse::<f64>().ok()).unwrap_or(0.0);
        }
        match words.first().copied() {
            Some("WON") => t[0] += 1,
            Some("LOST") => t[1] += 1,
            Some("DRAWN") => t[2] += 1,
            _ => t[3] += 1,
        }
        n += 1;
        println!("[{n:3}/{total}] {name} (seed {k}): {line}");
    }
    println!();
    let (mut won, mut health) = (0, 0.0);
    for (name, [w, l, d, e]) in &tally {
        won += w;
        let (h, s) = left.get(name).copied().unwrap_or_default();
        health += h;
        let runs = f64::from(w + l + d + e).max(1.0);
        println!(
            "{name:24} won {w:2}  lost {l:2}  drawn {d:2}  health left {:5.1}  {:4.0}s a win{}",
            h / runs,
            s / f64::from(*w).max(1.0),
            if *e > 0 { format!("  errors {e}") } else { String::new() }
        );
    }
    println!("\n{won}/{total} won, health left {:.1} a fight", health / (total as f64).max(1.0));
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dump-nav") if args.len() == 3 => dump_nav(&args[1], &args[2]),
        Some("secret-survey") => {
            // (one map in this process, its rows out: the survey's children)
            let one = args.iter().any(|a| a == "--one");
            let jobs = args.iter().position(|a| a == "--jobs").and_then(|i| args.get(i + 1)).and_then(|j| j.parse().ok()).unwrap_or(12);
            let words: Vec<String> = args[1..].iter().filter(|a| !a.starts_with("--")).filter(|a| a.parse::<usize>().is_err()).cloned().collect();
            match (words.split_first(), one) {
                (Some((wad, maps)), true) if maps.len() == 1 => evildoom::survey::survey_map(wad, &maps[0]),
                (Some((wad, maps)), false) if !maps.is_empty() => evildoom::survey::survey(wad, maps, jobs),
                _ => Err("usage: evil-boom secret-survey <wad> <map>... [--jobs N]".into()),
            }
        }
        Some("neuro-gpt") => {
            let port = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)).and_then(|p| p.parse().ok()).unwrap_or(8000);
            let config = args.iter().position(|a| a == "--config").and_then(|i| args.get(i + 1)).map_or_else(|| evildoom::config::root().join("config").join("neuro-gpt.json"), std::path::PathBuf::from);
            evildoom::neuro::gpt::serve(port, &config)
        }
        Some("neuro-mock") => {
            let port = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)).and_then(|p| p.parse().ok()).unwrap_or(8000);
            let file = args.iter().skip(1).rfind(|a| !a.starts_with("--") && a.parse::<u16>().is_err()).cloned().unwrap_or_else(|| "out/neuro/actions.txt".into());
            evildoom::neuro::mock::serve(port, std::path::Path::new(&file))
        }
        Some("follow") if args.len() == 3 => follow(&args[1], &args[2]),
        Some("sector") if args.len() >= 4 => sector(&args[1], &args[2], &args[3..]),
        Some("run-map") => run_one(&args[1..]),
        Some("episode") => episode(&args[1..]),
        #[cfg(feature = "viewer")]
        Some("record") => record(&args[1..]),
        Some("episode-one") => episode_one(&args[1..]),
        Some("drive") => drive(&args[1..]),
        Some("sweep") => sweep(&args[1..]),
        Some("score") => evildoom::score::run(&args[1..]),
        Some("tuning") => {
            println!("{}", evildoom::tactical::tuning::Tuning::default().json());
            Ok(())
        }
        Some("score-report") if args.len() >= 2 => evildoom::score::report_files(&args[1..]),
        #[cfg(feature = "viewer")]
        Some("score-watch") => evildoom::score::watch(&args[1..]),
        Some("watch") => watch(&args[1..]),
        Some("arena") => arena(&args[1..]),
        Some("arena-one") => arena_one(&args[1..]),
        #[cfg(feature = "viewer")]
        Some("arena-watch") => arena_watch(&args[1..]),
        _ => Err("usage: evil-boom sweep|run-map|watch|drive|dump-nav|follow|sector ... (see the source's header)".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
