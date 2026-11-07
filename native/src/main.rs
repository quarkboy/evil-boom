//! `evil`: Evil from the command line.
//!
//!   evil sweep <wad> [maps...] [<wad> [maps...]] [--jobs N] [run options]
//!       many maps at once, a process a map; a line each as they finish, then a summary
//!   evil run-map <wad> <map> [run options]      one map; its log goes to out/explore
//!   evil dump-nav <wad> <map>                   the level's analysis (for comparing)
//!   evil follow <wad> <map>                     plan the level, follow the plan (seed 0)
//!   evil drive <wad> <map> --script FILE [run options]   drive her by hand (see drive.rs)
//!   evil sector <wad> <map> <sector>...         sectors as the map has them: heights, lines, things
//!   evil watch <wad> <map> [--speed X] [--all] [run options]
//!       watch her play, with what she knows beside the game (space pause, +/- speed, A the
//!       whole map, F follow, wheel zoom, Esc quit)
//!
//! Run options: --lives N (unlimited), --max-seconds S (3600, all lives together), --seed N|random (0), --peek, --secrets,
//! --expert, --fight SKILL, --mortal, --stall S (the 100% runs: over after S game seconds, 300
//! unless told, with nothing more collected or learned); --snapshot S (stop after S game seconds and draw the
//! viewer's window to out/watch.png; nothing's drawn till then) and --focus X,Y,ZOOM (its map
//! centred there); --from FILE@S (play back the run recorded in FILE, out/runs/<IWAD>_<MAP>.run,
//! which every run-map writes (_expert etc. by the kind of run), to S game seconds, then the bot as it is now: the run from there
//! goes to <IWAD>_<MAP>_branch.run).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitCode, Stdio};
use std::rc::Rc;
use std::sync::Arc;

use evildoom::config::resolve_wad;
use evildoom::specials::classify_line;
use evildoom::navigator::Navigator;
use evildoom::sweep::{run_map, RunOptions};
use evildoom::wad::{load_map, Wad};

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
    for arg in sectors {
        let si: usize = arg.parse().map_err(|_| format!("not a sector: {arg}"))?;
        let s = m.sectors.get(si).ok_or_else(|| format!("no sector {si}"))?;
        println!("sector {si}: floor {} ceiling {} ({} / {}), light {}, special {}, tag {}", s.floor, s.ceiling, s.floor_tex, s.ceiling_tex, s.light, s.special, s.tag);
        for l in &m.linedefs {
            let (front, back) = (m.front_sector(l), m.back_sector(l));
            if front != Some(si) && back != Some(si) {
                continue;
            }
            let other = if front == Some(si) { back } else { front };
            let side = |sd: u16| m.sidedefs.get(usize::from(sd)).map_or_else(String::new, |d| format!("[{} {} {} @{},{}]", d.upper, d.middle, d.lower, d.x_offset, d.y_offset));
            let (a, b) = (m.line_seg(l.index).0, m.line_seg(l.index).1);
            let what = classify_line(l.special).map_or_else(String::new, |i| format!(" {}", i.description));
            println!("  line {} ({:.0},{:.0})-({:.0},{:.0}) flags {:#x} special {}{what} tag {} -> {} front {} back {}", l.index, a.0, a.1, b.0, b.1, l.flags, l.special, l.tag, other.map_or_else(|| "-".to_string(), |o| format!("sector {o} ({}..{})", m.sectors[o].floor, m.sectors[o].ceiling)), side(l.front), side(l.back));
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

/// The run options among `args`: (options, jobs, the rest as they were).
fn run_options(args: &[String]) -> Result<(RunOptions, usize, Vec<String>), String> {
    let mut o = RunOptions::default();
    let mut jobs = 8;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--lives" => o.lives = value("--lives")?.parse().map_err(|_| "--lives: a number")?,
            "--snapshot" => o.snapshot = Some(value("--snapshot")?.parse().map_err(|_| "--snapshot: game seconds")?),
            "--stall" => o.stall_seconds = value("--stall")?.parse().map_err(|_| "--stall: game seconds")?,
            "--from" => {
                let v = value("--from")?;
                let (file, at) = v.rsplit_once('@').ok_or("--from: FILE@SECONDS")?;
                o.from = Some((std::path::PathBuf::from(file), at.parse().map_err(|_| "--from: FILE@SECONDS")?));
            }
            "--focus" => {
                let v: Vec<f64> = value("--focus")?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                let [x, y, z] = v.as_slice() else { return Err("--focus: X,Y,ZOOM".into()) };
                o.focus = Some((*x, *y, *z));
            }
            "--max-seconds" => o.max_seconds = value("--max-seconds")?.parse().map_err(|_| "--max-seconds: a number")?,
            "--seed" => {
                let v = value("--seed")?;
                o.seed = if v == "random" { None } else { Some(v.parse().map_err(|_| "--seed: a number or 'random'")?) };
            }
            "--jobs" => jobs = value("--jobs")?.parse().map_err(|_| "--jobs: a number")?,
            "--fight" => o.fight = value("--fight")?.parse().map_err(|_| "--fight: a skill, 1-5")?,
            "--expert" => o.expert = true,
            "--peek" => o.peek = true,
            "--secrets" => o.secrets = true,
            "--mortal" => o.mortal = true,
            _ => rest.push(a.clone()),
        }
    }
    Ok((o, jobs, rest))
}

/// The options as they'd be given again (for the child processes).
fn option_args(o: &RunOptions) -> Vec<String> {
    let mut out = vec!["--lives".to_string(), o.lives.to_string(), "--max-seconds".into(), o.max_seconds.to_string(), "--stall".into(), o.stall_seconds.to_string(), "--seed".into(), o.seed.map_or("random".into(), |s| s.to_string())];
    if o.fight > 0 {
        out.push("--fight".into());
        out.push(o.fight.to_string());
    }
    for (on, flag) in [(o.peek, "--peek"), (o.secrets, "--secrets"), (o.mortal, "--mortal"), (o.expert, "--expert")] {
        if on {
            out.push(flag.into());
        }
    }
    out
}

fn run_one(args: &[String]) -> Result<(), String> {
    let (o, _, rest) = run_options(args)?;
    let [wad, map] = rest.as_slice() else { return Err("usage: evil run-map <wad> <map> [options]".into()) };
    let (outcome, detail) = run_map(wad, map, &o)?;
    println!("RESULT {outcome} {detail}");
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
    let ([wad, map], Some(script)) = (names.as_slice(), script) else { return Err("usage: evil drive <wad> <map> --script FILE [run options]".into()) };
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
            let child = Command::new(&me).arg("run-map").arg(&w).arg(&m).args(option_args(&o)).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
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
        let line = child.stdout.take().map(BufReader::new).and_then(|r| r.lines().map_while(Result::ok).find(|l| l.starts_with("RESULT "))).unwrap_or_else(|| "RESULT ERROR (no result)".into());
        let line = line.trim_start_matches("RESULT ").to_string();
        let outcome = line.split(' ').next().unwrap_or("ERROR").to_string();
        results.push(outcome);
        println!("[{:3}/{total}] {w} {m}: {line}", results.len());
    }
    let done = results.iter().filter(|o| matches!(o.as_str(), "EXITED" | "EXPERT" | "SECRETS")).count();
    println!("\n{done}/{} exited", results.len());
    if o.expert {
        println!("{}/{} with every item", results.iter().filter(|o| *o == "EXPERT").count(), results.len());
    }
    if o.secrets {
        println!("{}/{} with every secret", results.iter().filter(|o| *o == "SECRETS").count(), results.len());
    }
    Ok(())
}

#[cfg(feature = "viewer")]
fn watch(args: &[String]) -> Result<(), String> {
    let (o, _, rest) = run_options(args)?;
    let mut w = evildoom::viewer::WatchOptions { speed: 1.0, show_all: false, mortal: o.mortal, lives: o.lives, fight: o.fight, peek: o.peek, expert: o.expert, seed: o.seed, focus: None };
    let mut names = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--speed" => w.speed = it.next().and_then(|v| v.parse().ok()).ok_or("--speed: a number")?,
            "--all" => w.show_all = true,
            _ => names.push(a.clone()),
        }
    }
    let [wad, map] = names.as_slice() else { return Err("usage: evil watch <wad> <map> [options]".into()) };
    if o.snapshot.is_some() {
        // (a picture: the run as a sweep plays it, nothing drawn till then)
        let (outcome, detail) = run_map(wad, map, &o)?;
        println!("RESULT {outcome} {detail}");
        return Ok(());
    }
    w.focus = o.focus;
    evildoom::viewer::watch(wad, map, &w)
}

#[cfg(not(feature = "viewer"))]
fn watch(_args: &[String]) -> Result<(), String> {
    Err("built without the viewer (the `viewer` feature)".into())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dump-nav") if args.len() == 3 => dump_nav(&args[1], &args[2]),
        Some("follow") if args.len() == 3 => follow(&args[1], &args[2]),
        Some("sector") if args.len() >= 4 => sector(&args[1], &args[2], &args[3..]),
        Some("run-map") => run_one(&args[1..]),
        Some("drive") => drive(&args[1..]),
        Some("sweep") => sweep(&args[1..]),
        Some("watch") => watch(&args[1..]),
        _ => Err("usage: evil sweep|run-map|watch|drive|dump-nav|follow|sector ... (see the source's header)".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
