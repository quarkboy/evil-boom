// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! `evil-boom`: Evil from the command line. The commands, their arguments and options are in
//! help.rs (`evil-boom help`), and at more length in docs/USAGE.md.

mod cli;
use cli::{option_args, run_options};
use evilboom::config::resolve_wad;
use evilboom::navigator::Navigator;
use evilboom::specials::classify_line;
#[cfg(feature = "viewer")]
use evilboom::sweep::run_map;
use evilboom::wad::{load_level, Wad};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitCode, Stdio};
use std::rc::Rc;
use std::sync::Arc;

fn dump_nav(wad: &str, map: &str) -> Result<(), String> {
    let wad = Wad::open(&resolve_wad(wad))?;
    let m = Arc::new(load_level(&wad, map)?);
    let mut nav = Navigator::new(Arc::clone(&m), 3)?;
    println!("jumps found {}", nav.jumps.len());
    let checks = nav.keep_verified();
    println!("jumps kept {} {checks:?}", nav.jumps.len());
    println!(
        "polys {} portals {} fits {}",
        nav.mesh.polys.len(),
        nav.mesh.portals.len(),
        nav.portal_fits.iter().filter(|&&f| f).count()
    );
    println!("trenches {:?}", nav.trenches);
    for t in &nav.triggers {
        let mut eff: Vec<String> = t
            .effects
            .iter()
            .map(|(s, hs)| {
                format!(
                    "{s}:{:?}",
                    hs.iter().map(|h| (h.floor, h.ceiling)).collect::<Vec<_>>()
                )
            })
            .collect();
        eff.sort();
        println!(
            "trigger {} {} {:?} act={:?} eff={:?} rel={:?}",
            t.line, t.info.description, t.sectors, t.activators, eff, t.relevant
        );
    }
    for tp in &nav.teleports {
        println!(
            "teleport {} {:?} -> {} ({:.0}, {:.0})",
            tp.line, tp.src_polys, tp.dest_poly, tp.dest.0, tp.dest.1
        );
    }
    let mut ug: Vec<_> = nav.use_gates.iter().collect();
    ug.sort();
    println!("use_gates {ug:?}");
    let mut ulg: Vec<_> = nav.use_line_gates.iter().collect();
    ulg.sort();
    println!("use_line_gates {ulg:?}");
    let mut sg: Vec<_> = nav.stand_gates.iter().collect();
    sg.sort();
    println!("stand_gates {sg:?}");
    let mut sf: Vec<_> = nav.shot_floors.iter().map(|(k, v)| (*k, *v)).collect();
    sf.sort_by_key(|a| a.0);
    println!("shot_floors {sf:?}");
    let nav = Rc::new(nav);
    let start = m
        .player_start()
        .map(|t| (t.x, t.y))
        .ok_or("no player start")?;
    // (EVIL_DEBUG_FROM=x,y: the reach from there instead, for a place she was stranded in)
    let start = std::env::var("EVIL_DEBUG_FROM")
        .ok()
        .and_then(|v| {
            let mut n = v.split(',').filter_map(|x| x.trim().parse::<f64>().ok());
            Some((n.next()?, n.next()?))
        })
        .unwrap_or(start);
    let view = nav.full_view();
    let sp = nav.poly_at(start).ok_or("start off the map")?;
    let reach = view.reachable(sp, &nav.initial_states(), None);
    println!("reach from start {}", reach.len());
    // Where the reach ends with every trigger she could get to fired and every key taken: the
    // edges from a sector reached to one that isn't, with both as the map starts. What's on
    // the far side of one is what the planner has no way into (for a map with no route).
    let stages = view.progression(sp);
    for (i, st) in stages.iter().enumerate() {
        println!(
            "stage {i}: {} polys reached, {} triggers fired, exits {:?}",
            st.reached.len(),
            st.fired.len(),
            st.exits
        );
    }
    if let Some(last) = stages.last() {
        let sector = |p: usize| nav.mesh.polys[p].as_ref().map(|q| q.sector);
        let reached: BTreeSet<usize> = last.reached.iter().filter_map(|&p| sector(p)).collect();
        let all: BTreeSet<usize> = (0..nav.mesh.polys.len()).filter_map(sector).collect();
        println!("sectors reached {} of {}", reached.len(), all.len());
        if let Ok(v) = std::env::var("EVIL_DEBUG_AT") {
            let c: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
            for q in c.chunks(2) {
                let p = nav.poly_at((q[0], q[1]));
                println!(
                    "  at ({}, {}): poly {p:?} {:?}, sector {:?}, reached {}",
                    q[0],
                    q[1],
                    p.and_then(|p| nav.mesh.polys[p].as_ref())
                        .map(|q| q
                            .vertices
                            .iter()
                            .map(|v| (v.0.round(), v.1.round()))
                            .collect::<Vec<_>>())
                        .unwrap_or_default(),
                    p.and_then(sector),
                    p.is_some_and(|p| last.reached.contains(&p))
                );
                if let Some(p) = p {
                    for &pi in &nav.mesh.polys[p].as_ref().unwrap().portals {
                        let portal = &nav.mesh.portals[pi];
                        let o = portal.other(p);
                        println!("    portal {pi} ({:.0}, {:.0})-({:.0}, {:.0}) line {:?} to poly {o} sector {:?} reached {} fits {} passable now {}", portal.p1.0, portal.p1.1, portal.p2.0, portal.p2.1, portal.linedef, sector(o), last.reached.contains(&o), nav.portal_fits[pi], view.nav.passable(pi, p, &nav.initial_states(), None, &BTreeSet::new()));
                    }
                }
            }
        }
        if reached.len() < all.len() {
            println!("  reached: {reached:?}");
        }
        let mut edges: BTreeMap<(usize, usize), (usize, Option<usize>, usize)> = BTreeMap::new();
        for &p in &last.reached {
            let Some(poly) = nav.mesh.polys[p].as_ref() else {
                continue;
            };
            for &pi in &poly.portals {
                let portal = &nav.mesh.portals[pi];
                let q = portal.other(p);
                if let (false, Some(to)) = (last.reached.contains(&q), sector(q)) {
                    let e = edges
                        .entry((poly.sector, to))
                        .or_insert((0, portal.linedef, 0));
                    e.0 += 1;
                    e.2 += usize::from(nav.portal_fits[pi]);
                }
            }
        }
        let h = |s: usize| {
            let h = nav.movers.initial[s];
            format!(
                "{s} ({}..{}, tag {})",
                h.floor, h.ceiling, nav.map.sectors[s].tag
            )
        };
        // (and the triggers never fired: where each would be set off from)
        let fired: BTreeSet<usize> = stages
            .iter()
            .flat_map(|s| s.fired.iter().copied())
            .collect();
        for t in view.triggers.iter().filter(|t| !fired.contains(&t.id)) {
            let from: BTreeSet<usize> = t.activators.iter().filter_map(|&p| sector(p)).collect();
            println!(
                "  never fired: line {} {} {:?} (lock {:?}), set off from sectors {from:?}",
                t.line, t.info.description, t.sectors, t.info.lock
            );
        }
        for ((a, b), (n, line, wide)) in &edges {
            println!(
                "  reach ends: sector {} | sector {}, line {line:?}, {n} portals ({wide} wide enough)",
                h(*a),
                h(*b)
            );
        }
    }
    let t0 = std::time::Instant::now();
    let plan = evilboom::walkthrough::plan_level(&view, start, 60.0, 20000);
    eprintln!("planned in {:.2}s", t0.elapsed().as_secs_f64());
    println!("plan complete={} problem={:?}", plan.complete, plan.problem);
    for (i, st) in plan.steps.iter().enumerate() {
        println!("step {i}: {} | {}", st.label, st.substeps().join(" -> "));
    }
    Ok(())
}

/// What the map says about some sectors: heights, special and tag, each line round them (with
/// what's on the other side), the lines whose tag moves them, and the things in them.
/// Where a rocket can be fired from at the level's boss brain, the map's heights as it starts:
/// a line a sector, for looking into `tactical::icon::lanes`.
fn brain_posts(wad: &str, map: &str) -> Result<(), String> {
    use evilboom::tactical::icon::lanes;
    let w = Wad::open(&resolve_wad(wad))?;
    let m = load_level(&w, map)?;
    let nav = Navigator::new(Arc::new(m), 3)?;
    let brain = lanes::brain_of(&nav.map, &nav.mesh).ok_or("no boss brain on that map")?;
    let heights: Vec<_> = nav.initial_states().iter().map(|h| h[0]).collect();
    println!(
        "brain at ({:.0}, {:.0}) in sector {} (floor {})",
        brain.at.0, brain.at.1, brain.sector, heights[brain.sector].floor
    );
    let posts = lanes::posts(&nav.map, &nav.mesh, &heights, brain);
    let sectors: BTreeSet<usize> = posts.iter().map(|p| p.sector).collect();
    for s in sectors {
        let of: Vec<_> = posts.iter().filter(|p| p.sector == s).collect();
        let floor = heights[s].floor;
        let now = of
            .iter()
            .filter(|p| p.feet.0 <= floor && floor <= p.feet.1)
            .count();
        let best = of
            .iter()
            .max_by(|a, b| a.damage.total_cmp(&b.damage))
            .ok_or("none")?;
        println!(
            "sector {s} (floor {floor}): {} posts, {now} good as it stands; feet {:.0} to {:.0}; best ({:.0}, {:.0}) facing {:.1}, feet {:.0} to {:.0}, {:.0} a rocket, {:.0} off",
            of.len(),
            of.iter().map(|p| p.feet.0).fold(f64::INFINITY, f64::min),
            of.iter().map(|p| p.feet.1).fold(f64::NEG_INFINITY, f64::max),
            best.at.0,
            best.at.1,
            best.heading,
            best.feet.0,
            best.feet.1,
            best.damage,
            lanes::range(best, brain),
        );
    }
    Ok(())
}

fn sector(wad: &str, map: &str, sectors: &[String]) -> Result<(), String> {
    let w = Wad::open(&resolve_wad(wad))?;
    let m = load_level(&w, map)?;
    let nav = Navigator::new(Arc::new(m), 3)?;
    let m = &nav.map;
    let places = evilboom::control::places::Places::build(&nav);
    for arg in sectors {
        let si: usize = arg.parse().map_err(|_| format!("not a sector: {arg}"))?;
        let s = m.sectors.get(si).ok_or_else(|| format!("no sector {si}"))?;
        println!(
            "sector {si}: floor {} ceiling {} ({} / {}), light {}, special {}, tag {}",
            s.floor, s.ceiling, s.floor_tex, s.ceiling_tex, s.light, s.special, s.tag
        );
        let area = places.area_of(si);
        println!(
            "  in {} ({}, {:.0} square units): sectors {:?}",
            area.id,
            evilboom::control::places::Places::sketch(area),
            area.size,
            area.sectors
        );
        for l in &m.linedefs {
            let (front, back) = (m.front_sector(l), m.back_sector(l));
            if front != Some(si) && back != Some(si) {
                continue;
            }
            let other = if front == Some(si) { back } else { front };
            let side = |sd: u16| {
                m.sidedefs
                    .get(usize::from(sd))
                    .map_or_else(String::new, |d| {
                        format!(
                            "[{} {} {} @{},{}]",
                            d.upper, d.middle, d.lower, d.x_offset, d.y_offset
                        )
                    })
            };
            let (a, b) = (m.line_seg(l.index).0, m.line_seg(l.index).1);
            let what = classify_line(l.special)
                .map_or_else(String::new, |i| format!(" {}", i.description));
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
            for l in m
                .linedefs
                .iter()
                .filter(|l| l.tag == s.tag && l.special != 0)
            {
                let what = classify_line(l.special).map_or_else(String::new, |i| i.description);
                println!(
                    "  moved by line {} ({what}) between sectors {:?}/{:?}",
                    l.index,
                    m.front_sector(l),
                    m.back_sector(l)
                );
            }
        }
        for t in m.things.iter().filter(|t| {
            nav.poly_at((t.x, t.y))
                .is_some_and(|p| nav.mesh.polys[p].as_ref().is_some_and(|q| q.sector == si))
        }) {
            println!(
                "  thing {} at ({}, {}) flags {:#x}",
                t.kind, t.x, t.y, t.flags
            );
        }
        // (the navmesh's portals along its lines: where the mesh cuts an edge)
        for (pi, portal) in nav.mesh.portals.iter().enumerate() {
            if portal.linedef.is_some_and(|li| {
                m.front_sector(&m.linedefs[li]) == Some(si)
                    || m.back_sector(&m.linedefs[li]) == Some(si)
            }) {
                println!(
                    "  portal {pi} on line {:?}: ({:.0},{:.0})-({:.0},{:.0})",
                    portal.linedef, portal.p1.0, portal.p1.1, portal.p2.0, portal.p2.1
                );
            }
        }
        // (the run-jumps found landing here, and what the movement check makes of them)
        let into: Vec<&evilboom::navigator::Jump> = nav
            .jumps
            .iter()
            .filter(|j| {
                nav.mesh.polys[j.dest_poly]
                    .as_ref()
                    .is_some_and(|q| q.sector == si)
            })
            .collect();
        let lab: Vec<evilboom::jumps::Jump> = into
            .iter()
            .map(|j| evilboom::jumps::Jump {
                src_polys: j.src_polys.iter().copied().collect(),
                dest_poly: j.dest_poly,
                dest: j.dest,
                runup: j.runup,
                takeoff: j.takeoff,
                speed: 0.0,
                requires: j.requires.clone(),
                strafe: j.strafe,
                ledge: j.ledge,
            })
            .collect();
        for (j, verdict) in into.iter().zip(nav.jump_lab.check_jumps(&lab)) {
            let from: BTreeSet<usize> = j
                .src_polys
                .iter()
                .filter_map(|&p| nav.mesh.polys[p].as_ref().map(|q| q.sector))
                .collect();
            println!(
                "  jump in from sectors {from:?}: run-up ({:.0}, {:.0}), takeoff ({:.0}, {:.0}), lands ({:.0}, {:.0}): {verdict}",
                j.runup.0, j.runup.1, j.takeoff.0, j.takeoff.1, j.dest.0, j.dest.1
            );
        }
    }
    Ok(())
}

/// `console`'s options: `--bind ADDR` (0.0.0.0:7878 unless given) and `--new-token`; anything
/// else refused (a misspelt `--bind` had been passed over, the page served to the whole LAN).
fn console_args(args: &[String]) -> Result<(String, bool), String> {
    let (mut bind, mut new_token) = ("0.0.0.0:7878".to_string(), false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bind" => bind = it.next().ok_or("--bind ADDRESS:PORT")?.clone(),
            "--new-token" => new_token = true,
            other => {
                return Err(format!(
                    "console: {other:?} isn't an option (--bind ADDRESS:PORT, --new-token)"
                ))
            }
        }
    }
    Ok((bind, new_token))
}

/// Plan the level from the start and follow the plan in the game (seed 0): the follower's log.
fn follow(wad: &str, map: &str) -> Result<(), String> {
    use evilboom::engine::{Engine, Options};
    use evilboom::motor::follower::RouteFollower;
    let iwad = resolve_wad(wad);
    let w = Wad::open(&iwad)?;
    let m = Arc::new(load_level(&w, map)?);
    let mut nav = Navigator::new(Arc::clone(&m), 3)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    let view = std::rc::Rc::new(nav.full_view());
    let start = m
        .player_start()
        .map(|t| (t.x, t.y))
        .ok_or("no player start")?;
    let plan = evilboom::walkthrough::plan_level(&view, start, 60.0, 20000);
    if !plan.complete {
        return Err(plan.problem);
    }
    let mut e = Engine::new(
        &iwad,
        map,
        &Options {
            seed: Some(0),
            ..Options::default()
        },
    )?;
    e.start();
    e.set_god(true);
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
    cli::no_flags(&rest)?;
    let [wad, map] = rest.as_slice() else {
        return Err("usage: evil-boom run-map <wad> <map> [options]".into());
    };
    let r = evilboom::sweep::run_map_full(wad, map, &o)?;
    // (what she got out with, for the next level of an episode; the secret exit taken)
    if let Some(l) = &r.carry {
        println!(
            "CARRY {}{}",
            cli::carry_arg(l),
            if r.secret { " SECRET" } else { "" }
        );
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
    cli::no_flags(&names)?;
    let [wad, first] = names.as_slice() else {
        return Err("usage: evil-boom episode <wad> <first map> [--seeds K] [run options]".into());
    };
    // (one life unless told: `--lives N`, see `episode_one`)
    if o.lives == i32::MAX {
        o.lives = 1;
    }
    let lives = o.lives;
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
        println!(
            "    to watch it all: evil-boom watch {wad} {first} {} --episode",
            option_args(&os).join(" ")
        );
    }
    let mean = done.iter().map(|d| d.1 as f64).sum::<f64>() / done.len().max(1) as f64;
    let through = done.iter().filter(|d| d.2 == "the episode through").count();
    println!();
    println!(
        "{mean:.1} levels on {} on average; the episode through in {through}/{}",
        if lives == 1 {
            "one life".to_string()
        } else {
            format!("{lives} lives")
        },
        done.len()
    );
    Ok(())
}

/// One episode on one life (see `episode`), in a process of its own (`episode-one`): a line for
/// each level as it ends, then how many levels she got through, and how it ended.
fn episode_chain(
    me: &std::path::Path,
    wad: &str,
    first: &str,
    o: &evilboom::run::RunOptions,
) -> (usize, String) {
    let seed = o.seed.unwrap_or(0);
    let Ok(mut child) = Command::new(me)
        .arg("episode-one")
        .arg(wad)
        .arg(first)
        .args(option_args(o))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return (0, "couldn't run".into());
    };
    let mut end = (0, "no result".to_string());
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(l) = line.strip_prefix("LEVEL ") {
                println!("seed {seed} {l}");
            } else if let Some(l) = line.strip_prefix("STOCK ") {
                println!("seed {seed} stock {l}");
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
    cli::no_flags(&rest)?;
    let [wad, first] = rest.as_slice() else {
        return Err("usage: evil-boom episode-one <wad> <first map> [run options]".into());
    };
    // (`--lives N`: N lives for the whole episode. Dead with one left, the level again from its
    // start with what she came into it with, as she got out of the one before, and what she's
    // learned of the level kept: a level's run with the lives she has left, as `run-map --lives`
    // plays one. Its time's cap is for all its lives together: thrice a level's, with lives to
    // spend)
    let mut left = if o.lives == i32::MAX {
        1
    } else {
        o.lives.max(1)
    };
    if left > 1 {
        o.max_seconds *= 3.0;
    }
    let mut map = Some(first.to_uppercase());
    let mut engine = None;
    let mut levels = 0;
    // (what she was given to start the episode with: `o.carry` is from here on what she takes
    // from each level into the next)
    let given = o.carry.clone();
    while let Some(m) = map.take() {
        o.lives = left;
        // (a level with one after it: what she'd take with her had before she leaves)
        o.going_on = evilboom::run::next_map(&m, false).is_some()
            || evilboom::run::next_map(&m, true).is_some();
        let r = evilboom::sweep::run_map_in(wad, &m, &o, engine.take())?;
        left -= r.deaths;
        let lost = if r.deaths > 0 {
            format!(" | {} lives lost, {left} left", r.deaths)
        } else {
            String::new()
        };
        let line = format!(
            "{m}: {} {}{lost}",
            r.outcome,
            r.detail.chars().take(70).collect::<String>()
        );
        let Some(l) = r.carry else {
            println!("LEVEL {line}");
            if !r.stock.is_empty() {
                println!("STOCK {m}: {}", r.stock);
            }
            println!("DAMAGE {}", r.damage);
            let mut os = o.clone();
            os.carry = given.clone();
            println!(
                "WATCH evil-boom watch {wad} {first} {} --episode",
                option_args(&os).join(" ")
            );
            println!("END {levels} {m}: {}", r.outcome);
            return Ok(());
        };
        levels += 1;
        println!(
            "LEVEL {line} | out with health {} armor {}, ammo {}/{}/{}/{}",
            l.health, l.armor, l.clip, l.shell, l.cell, l.rocket
        );
        if !r.stock.is_empty() {
            println!("STOCK {m}: {}", r.stock);
        }
        o.carry = Some(l);
        engine = r.engine;
        map = evilboom::run::next_map(&m, r.secret);
    }
    println!("END {levels} the episode through");
    Ok(())
}

/// `ultra WAD MAP [run options]`: the level played with the ultra instinct on, offline;
/// `ultra-check`: the game saved, put back and played again, and what that costs.
fn ultra(args: &[String], check: bool) -> Result<(), String> {
    let mut tune = evilboom::ultra::Tune::default();
    // (`ultra-worker`, started by a run or the window: the port it's asked its looks over)
    let mut port: Option<u16> = None;
    let mut rest_args = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = |what: &str| -> Result<f64, String> {
            it.next()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("{what} N"))
        };
        match a.as_str() {
            "--depth" => tune.depth = num("--depth")? as usize,
            "--width" => tune.width = num("--width")? as usize,
            "--move" => tune.move_tics = num("--move")? as usize,
            "--way" => tune.way = num("--way")?,
            "--hurt" => tune.hurt = num("--hurt")?,
            "--hurt-tries" => tune.hurt_tries = num("--hurt-tries")? as u32,
            "--port" => port = Some(num("--port")? as u16),
            _ => rest_args.push(a.clone()),
        }
    }
    let (mut o, _, rest) = run_options(&rest_args)?;
    cli::no_flags(&rest)?;
    let [wad, map] = rest.as_slice() else {
        return Err(
            "usage: evil-boom ultra <wad> <map> [--depth N --width N --move N] [run options]"
                .into(),
        );
    };
    o.mortal = true;
    if let Some(port) = port {
        return evilboom::ultra::worker(wad, map, &o, tune, port);
    }
    println!(
        "{}",
        if let (true, Some(tape)) = (check, &o.tape) {
            evilboom::ultra::check_tape(wad, map, &o, tape)?
        } else if check {
            evilboom::ultra::check(wad, map, &o)?
        } else {
            evilboom::ultra::run(wad, map, &o, tune)?
        }
    );
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
    cli::no_flags(&names)?;
    let ([wad, map], Some(script)) = (names.as_slice(), script) else {
        return Err("usage: evil-boom drive <wad> <map> --script FILE [run options]".into());
    };
    evilboom::drive::drive(wad, map, &o, std::path::Path::new(&script))
}

fn sweep(args: &[String]) -> Result<(), String> {
    let (o, jobs, rest) = run_options(args)?;
    cli::no_flags(&rest)?;
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
        let maps = if maps.is_empty() {
            Wad::open(&resolve_wad(&w))?.map_names()
        } else {
            maps
        };
        todo.extend(maps.into_iter().map(|m| (w.clone(), m)));
    }
    let total = todo.len();
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut running: Vec<(String, String, Child)> = Vec::new();
    let mut results: Vec<String> = Vec::new();
    while !todo.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some((w, m)) = todo.pop_front() else {
                break;
            };
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
            .and_then(|r| {
                r.lines()
                    .map_while(Result::ok)
                    .find(|l| l.starts_with("RESULT "))
            })
            .unwrap_or_else(|| "RESULT ERROR (no result)".into());
        let line = line.trim_start_matches("RESULT ").to_string();
        let outcome = line.split(' ').next().unwrap_or("ERROR").to_string();
        results.push(outcome);
        println!("[{:3}/{total}] {w} {m}: {line}", results.len());
    }
    let done = results
        .iter()
        .filter(|o| matches!(o.as_str(), "EXITED" | "EXPERT" | "SECRETS" | "COMPLETE"))
        .count();
    println!("\n{done}/{} exited", results.len());
    if o.expert {
        println!(
            "{}/{} with every item",
            results.iter().filter(|o| *o == "EXPERT").count(),
            results.len()
        );
    }
    if o.secrets {
        println!(
            "{}/{} with every secret",
            results.iter().filter(|o| *o == "SECRETS").count(),
            results.len()
        );
    }
    if o.complete {
        println!(
            "{}/{} with every kill, item and secret",
            results.iter().filter(|o| *o == "COMPLETE").count(),
            results.len()
        );
    }
    Ok(())
}

#[cfg(feature = "viewer")]
fn watch(args: &[String]) -> Result<(), String> {
    let (mut w, names, o) = watch_options(args)?;
    cli::no_flags(&names)?;
    let [wad, map] = names.as_slice() else {
        return Err("usage: evil-boom watch <wad> <map> [options]".into());
    };
    if w.headless {
        // (SDL takes Ctrl-C and SIGTERM for its own quit event, which nothing headless reads: left
        // to the shell, they stop it. Set while the process is one thread still)
        evilboom::dsda::set_env("SDL_NO_SIGNAL_HANDLERS", Some("1".as_ref()));
    }
    if let Some(config) = &w.gpt {
        // (the stand-in in this program, on a port of its own: one program to run. Its thread
        // begins once the game's set up: `gpt::go`)
        w.neuro = Some(evilboom::neuro::gpt::start(config)?);
    }
    if o.snapshot.is_some() {
        // (a picture: the run as a sweep plays it, nothing drawn till then)
        let (outcome, detail) = run_map(wad, map, &o)?;
        println!("RESULT {outcome} {detail}");
        return Ok(());
    }
    evilboom::viewer::watch(wad, map, &w)
}

/// A run watched into an MP4 (see `viewer::record`): evil-boom record <wad> <map> <out.mp4>
/// [--view game|full|hud|stream] [--hide ITEMS] [--size WxH] [--speed N] [--ffmpeg PATH] [--episode] [run options].
#[cfg(feature = "viewer")]
fn record(args: &[String]) -> Result<(), String> {
    use evilboom::viewer::{RecordOptions, RecordView};
    let (mut view, mut size, mut every, mut ffmpeg) = (RecordView::Hud, None, 1u32, None);
    let (mut crf, mut preset) = (20u32, "veryfast".to_string());
    let mut hide = Vec::new();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--view" => {
                view = match it.next().map(String::as_str) {
                    Some("game") => RecordView::Game,
                    Some("full") => RecordView::Full,
                    Some("hud") => RecordView::Hud,
                    Some("stream") => RecordView::Stream,
                    _ => return Err("--view game|full|hud|stream".into()),
                };
            }
            "--size" => {
                let v = it.next().ok_or("--size WxH")?;
                let (x, y) = v.split_once(['x', 'X']).ok_or("--size WxH")?;
                size = Some((
                    x.parse().map_err(|_| "--size WxH")?,
                    y.parse().map_err(|_| "--size WxH")?,
                ));
            }
            "--speed" => {
                every = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--speed N (whole times as fast)")?
            }
            "--ffmpeg" => {
                ffmpeg = Some(std::path::PathBuf::from(it.next().ok_or("--ffmpeg PATH")?))
            }
            "--hide" => {
                hide = it
                    .next()
                    .ok_or("--hide ITEMS (the stream column's, commas between)")?
                    .split(',')
                    .map(str::to_string)
                    .collect()
            }
            "--crf" => {
                crf = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--crf N (0-51; 20 unless told)")?
            }
            "--preset" => {
                preset = it
                    .next()
                    .ok_or("--preset NAME (ultrafast .. veryslow)")?
                    .clone()
            }
            _ => rest.push(a.clone()),
        }
    }
    let (mut w, names, _) = watch_options(&rest)?;
    cli::no_flags(&names)?;
    let [wad, map, out] = names.as_slice() else {
        return Err("usage: evil-boom record <wad> <map> <out.mp4> [--view game|full|hud|stream] [--size WxH] [--speed N] [--episode] [options]".into());
    };
    // (the game's sound, at x1: each tic's beside each frame)
    w.capture_sound = every == 1;
    if let Some(config) = &w.gpt {
        // (the stand-in in this program, as watch has it; directed, it plays in real time)
        w.neuro = Some(evilboom::neuro::gpt::start(config)?);
    }
    let ffmpeg = ffmpeg
        .or_else(evilboom::viewer::find_ffmpeg)
        .ok_or("no ffmpeg found (on the path, or winget's on Windows): --ffmpeg PATH")?;
    evilboom::viewer::record(
        wad,
        map,
        &w,
        &RecordOptions {
            out: out.into(),
            view,
            size,
            every,
            crf,
            preset,
            ffmpeg,
            hide,
        },
    )
}

/// The watcher's options among `args`: (them, the names left, the run options).
#[cfg(feature = "viewer")]
fn watch_options(
    args: &[String],
) -> Result<
    (
        evilboom::viewer::WatchOptions,
        Vec<String>,
        evilboom::run::RunOptions,
    ),
    String,
> {
    let (o, _, rest) = run_options(args)?;
    let mut w = evilboom::viewer::WatchOptions {
        charmed: o.charmed,
        endless_bullets: o.endless_bullets,
        keys: o.keys,
        speed: 1.0,
        show_all: false,
        show_keys: false,
        mortal: o.mortal,
        practice: o.known,
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
        tape: o.tape.clone(),
        ultra: o.ultra,
        workers: o.workers,
        ultra_speed: o.ultra_speed,
        ultra_after: o.ultra_after,
        ultra_back: o.ultra_back,
        headless: false,
        paced: false,
        max_seconds: o.max_seconds,
        seed: o.seed,
        focus: None,
        sound: true,
        log: None,
        directed: None,
        neuro: None,
        gpt: None,
        capture_sound: false,
        control: None,
        present: false,
        title_wait: false,
        stream: None,
        choices: None,
        lives_shared: false,
        locked: Vec::new(),
        instinct_off: false,
        human: false,
        mouse_sens: 0.1,
        hunt: None,
        window: evilboom::dsda::WindowSpec::default(),
        render: None,
        soundfont: evilboom::config::default_soundfont(),
        music: None,
        players: 1,
        deathmatch: 0,
        frag_limit: 20,
        minutes: 10.0,
    };
    let mut names = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--speed" => {
                w.speed = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--speed: a number")?;
                w.paced = true;
            }
            "--all" => w.show_all = true,
            "--show-keys" => w.show_keys = true,
            "--players" => {
                w.players = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| (1..=4).contains(n))
                    .ok_or("--players N: 1 to 4 (more than one: a co-op game, each a bot)")?;
            }
            "--deathmatch" => w.deathmatch = 1,
            "--altdeath" => w.deathmatch = 2,
            "--frags" => {
                w.frag_limit = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--frags N (a deathmatch over at N frags; 0: no limit)")?;
            }
            "--minutes" => {
                w.minutes = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--minutes M (a deathmatch over after M minutes; 0: no limit)")?;
            }
            "--episode" => w.episode = true,
            "--episode-lives" => {
                w.lives = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--episode-lives N (her lives for all the levels played on through)")?;
                w.lives_shared = true;
            }
            "--human" => w.human = true,
            "--hunt" => {
                let v = it.next().ok_or("--hunt all|none|N,M (the players who hunt, 1 the first)")?;
                w.hunt = match v.as_str() {
                    "all" => None,
                    "none" => Some(Vec::new()),
                    list => Some(
                        list.split(',')
                            .map(|n| n.trim().parse::<usize>().ok().filter(|&n| (1..=4).contains(&n)).map(|n| n - 1))
                            .collect::<Option<Vec<_>>>()
                            .ok_or("--hunt all|none|N,M (the players who hunt, 1 to 4)")?,
                    ),
                };
            }
            "--mouse" => {
                w.mouse_sens = it
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| *v > 0.0 && *v <= 5.0)
                    .ok_or("--mouse DEG (degrees a mouse pixel turns you, over 0 to 5; 0.1 unless set)")?;
            }
            "--instinct" => {
                w.instinct_off = match it.next().map(String::as_str) {
                    Some("off") => true,
                    Some("on") => false,
                    _ => return Err("--instinct on|off".into()),
                };
            }
            "--lock" => {
                let all = evilboom::control::directed::LOCKABLE;
                let v = it.next().ok_or("--lock instinct,finish (settings not hers to change)")?;
                for s in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if !all.contains(&s) {
                        return Err(format!("--lock: of {}, not \"{s}\"", all.join(", ")));
                    }
                    w.locked.push(s.to_string());
                }
            }
            "--headless" => w.headless = true,
            "--mute" => w.sound = false,
            "--log" => {
                let v = it.next().ok_or("--log full|brief|off")?;
                w.log = Some(
                    evilboom::config::LogLevel::named(v)
                        .ok_or(format!("--log: full, brief or off, not \"{v}\""))?,
                );
            }
            "--window-size" => {
                w.window.size = Some(evilboom::dsda::WindowSpec::parse_size(
                    it.next().ok_or("--window-size WIDTHxHEIGHT")?,
                    evilboom::dsda::MAX_WINDOW,
                )?);
            }
            "--no-vsync" => w.window.vsync = false,
            "--music" => {
                let m = it.next().ok_or("--music sc55|idkfa|midi")?;
                w.music = match m.as_str() {
                    "midi" => None,
                    _ => Some(
                        evilboom::music::Recordings::parse(m)
                            .ok_or("--music: sc55, idkfa or midi")?,
                    ),
                };
            }
            "--soundfont" => {
                let name = it.next().ok_or("--soundfont NAME (in soundfonts/), FILE, or opl")?;
                w.soundfont = if name == "opl" {
                    None
                } else {
                    Some(evilboom::config::resolve_soundfont(name)?)
                };
            }
            "--render" => {
                w.render = Some(evilboom::dsda::WindowSpec::parse_size(
                    it.next().ok_or("--render WIDTHxHEIGHT")?,
                    evilboom::dsda::MAX_RENDER,
                )?);
            }
            "--fullscreen" => w.window.fullscreen = true,
            "--neuro" => {
                w.neuro = Some(
                    it.next()
                        .cloned()
                        .unwrap_or_else(|| "ws://127.0.0.1:8000".into()),
                )
            }
            "--gpt" => {
                // (its config, if one's named: the next argument, a .json)
                let named = it
                    .as_slice()
                    .first()
                    .filter(|a| a.ends_with(".json"))
                    .cloned();
                if named.is_some() {
                    it.next();
                }
                w.gpt = Some(named.map_or_else(
                    || {
                        evilboom::config::root()
                            .join("config")
                            .join("neuro-gpt.json")
                    },
                    std::path::PathBuf::from,
                ));
            }
            "--directed" => {
                w.directed = Some(std::path::PathBuf::from(
                    it.next()
                        .ok_or("--directed FILE (orders, a line each, read as they're added)")?,
                ))
            }
            "--stream" => {
                // (the column's side, if one's named: right unless told)
                let named = it
                    .as_slice()
                    .first()
                    .and_then(|a| evilboom::viewer::StreamSide::parse(a));
                if named.is_some() {
                    it.next();
                }
                w.stream = Some(named.unwrap_or(evilboom::viewer::StreamSide::Right));
            }
            "--present" => w.present = true,
            "--choices" => {
                w.choices = Some(it.next().cloned().ok_or(
                    "--choices \"maps=E1M1,E1M2|all;skills=3,4;play=single,continuous\" (the games hers to start)",
                )?)
            }
            "--title" => {
                w.present = true;
                w.title_wait = true;
            }
            "--control" => {
                w.control = Some(it.next().cloned().ok_or(
                    "--control HOST:PORT (the stream console's, which starts the game with it)",
                )?)
            }
            _ => names.push(a.clone()),
        }
    }
    if w.episode && !o.lives_given && !w.lives_shared {
        w.lives = 1; // (an episode watched as it's played: one life, unless `--lives` says)
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
            seed = if v == "random" {
                None
            } else {
                Some(v.parse().map_err(|_| "--seed N")?)
            };
            i += 2;
            continue;
        }
        rest.push(&args[i]);
        i += 1;
    }
    let [file, name] = rest.as_slice() else {
        return Err("usage: evil-boom arena-one FILE NAME [--seed N]".into());
    };
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let scenarios = evilboom::arena::parse(&text)?;
    let s = scenarios
        .iter()
        .find(|s| &s.name == *name)
        .ok_or_else(|| format!("{name}: not in {file}"))?;
    let mut log = Vec::new();
    let o = evilboom::arena::run(s, seed, &mut log)?;
    let dir = evilboom::config::out_dir().join("arena");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tag = seed.map_or("random".to_string(), |s| s.to_string());
    evilboom::config::write_log(&dir.join(format!("{name}_{tag}.txt")), "", &log.join("\n"))
        .map_err(|e| e.to_string())?;
    println!(
        "RESULT {} {:.0}s health {} armor {} kills {}/{}",
        o.result, o.seconds, o.health, o.armor, o.kills, o.monsters
    );
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
                seed = if v == "random" {
                    None
                } else {
                    Some(v.parse().map_err(|_| "--seed N")?)
                };
                i += 2;
            }
            "--speed" => {
                speed = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--speed X")?;
                i += 2;
            }
            _ => {
                rest.push(&args[i]);
                i += 1;
            }
        }
    }
    let [file, name] = rest.as_slice() else {
        return Err(
            "usage: evil-boom arena-watch FILE NAME [--seed N] [--speed X] [--mute]".into(),
        );
    };
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let scenarios = evilboom::arena::parse(&text)?;
    let s = scenarios
        .iter()
        .find(|s| &s.name == *name)
        .ok_or_else(|| format!("{name}: not in {file}"))?;
    let p = evilboom::arena::prepare(s, seed, true, sound)?;
    evilboom::viewer::watch_prepared(p, &s.map, speed, sound, &format!("evil-boom arena: {name}"))
}

/// The arena's scenarios (those named, else all), each `seeds` times, `jobs` at a time.
fn arena(args: &[String]) -> Result<(), String> {
    let (mut jobs, mut seeds) = (8usize, 3u32);
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--jobs" => {
                jobs = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--jobs N")?;
                i += 2;
            }
            "--seeds" => {
                seeds = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--seeds K")?;
                i += 2;
            }
            _ => {
                rest.push(args[i].clone());
                i += 1;
            }
        }
    }
    let file = rest
        .first()
        .ok_or("usage: evil-boom arena FILE [names...] [--jobs N] [--seeds K]")?
        .clone();
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
    let names: Vec<String> = evilboom::arena::parse(&text)?
        .into_iter()
        .map(|s| s.name)
        .filter(|n| rest.len() < 2 || rest[1..].contains(n))
        .collect();
    let mut todo: VecDeque<(String, u32)> = names
        .iter()
        .flat_map(|n| (0..seeds).map(move |k| (n.clone(), k)))
        .collect();
    let total = todo.len();
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    type Reading = Option<std::thread::JoinHandle<String>>;
    let mut running: Vec<(String, u32, Child, Reading, Reading)> = Vec::new();
    let mut tally: std::collections::BTreeMap<String, [u32; 4]> = std::collections::BTreeMap::new();
    // (health left, a lost fight's none; seconds taken, won)
    let mut left: std::collections::BTreeMap<String, (f64, f64)> =
        std::collections::BTreeMap::new();
    let mut n = 0;
    while !todo.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some((name, k)) = todo.pop_front() else {
                break;
            };
            let mut child = Command::new(&me)
                .args(["arena-one", &file, &name, "--seed", &k.to_string()])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            // (both pipes read as the child writes: one waited on till it had ended, having
            // written more than a pipe holds, would wait for ever, the child stuck on its
            // write)
            let result = child.stdout.take().map(|s| {
                std::thread::spawn(move || {
                    let mut lines = BufReader::new(s).lines().map_while(Result::ok);
                    let first = lines.by_ref().find(|l| l.starts_with("RESULT "));
                    lines.for_each(drop); // (the rest read too, to its end)
                    first.unwrap_or_default()
                })
            });
            let errors = child.stderr.take().map(|s| {
                std::thread::spawn(move || {
                    BufReader::new(s)
                        .lines()
                        .map_while(Result::ok)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
            });
            running.push((name, k, child, result, errors));
        }
        let Some(i) = running
            .iter_mut()
            .position(|(_, _, c, ..)| c.try_wait().ok().flatten().is_some())
        else {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        };
        let (name, k, _child, result, errors) = running.swap_remove(i);
        let read = |h: Reading| h.and_then(|h| h.join().ok()).unwrap_or_default();
        let (mut out, err) = (read(result), read(errors));
        if out.is_empty() {
            out = format!("RESULT ERROR {err}");
        }
        let line = out.trim_start_matches("RESULT ").to_string();
        let t = tally.entry(name.clone()).or_default();
        let words: Vec<&str> = line.split(' ').collect();
        let after = |k: &str| {
            words
                .iter()
                .position(|w| *w == k)
                .and_then(|i| words.get(i + 1))
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        let l = left.entry(name.clone()).or_default();
        if words.first() == Some(&"WON") {
            l.0 += after("health");
            l.1 += words
                .get(1)
                .and_then(|s| s.trim_end_matches('s').parse::<f64>().ok())
                .unwrap_or(0.0);
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
            if *e > 0 {
                format!("  errors {e}")
            } else {
                String::new()
            }
        );
    }
    println!(
        "\n{won}/{total} won, health left {:.1} a fight",
        health / (total as f64).max(1.0)
    );
    Ok(())
}

/// `evil-boom help TOPIC`: a command's or a topic's help.
fn help_on(topic: &str) -> ExitCode {
    match evilboom::help::topic(topic) {
        Some(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("evil-boom: no help on {topic:?} (evil-boom help lists the commands)");
            ExitCode::FAILURE
        }
    }
}

/// `EVIL_PROFILE=FILE` (a build with `--features profile`): the whole run sampled, 500 times
/// a second, and the stacks written to FILE folded (a line a stack, outermost first, and how
/// often it was seen), for `tools/dev/profile.py`.
#[cfg(feature = "profile")]
fn main() -> ExitCode {
    let Some(to) = std::env::var_os("EVIL_PROFILE") else {
        return run();
    };
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(499)
        .build()
        .expect("the profiler starts");
    let code = run();
    match guard.report().build() {
        Ok(report) => {
            let mut out = String::new();
            for (frames, count) in &report.data {
                let stack: Vec<String> = frames
                    .frames
                    .iter()
                    .rev()
                    .flat_map(|f| f.iter().rev().map(pprof::Symbol::name))
                    .collect();
                out.push_str(&format!("{} {count}\n", stack.join(";")));
            }
            if let Err(e) = std::fs::write(&to, out) {
                eprintln!("EVIL_PROFILE: {e}");
            }
        }
        Err(e) => eprintln!("EVIL_PROFILE: {e}"),
    }
    code
}

#[cfg(not(feature = "profile"))]
fn main() -> ExitCode {
    run()
}

fn run() -> ExitCode {
    let args = match evilboom::config::take_pwads(std::env::args().skip(1).collect()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    // (the help, and the version: before anything's started)
    let first = args.first().map(String::as_str);
    let asked = |a: &String| a == "--help" || a == "-h";
    match first {
        None | Some("help" | "--help" | "-h") => {
            return match args.get(1).map(String::as_str) {
                None => {
                    print!("{}", evilboom::help::overview());
                    ExitCode::SUCCESS
                }
                Some(t) => help_on(t),
            };
        }
        Some("--version" | "-V" | "version") => {
            println!("{}", evilboom::help::version());
            return ExitCode::SUCCESS;
        }
        Some(cmd) if args[1..].iter().any(asked) => return help_on(cmd),
        _ => {}
    }
    let result = match first {
        Some("dump-nav") if args.len() == 3 => dump_nav(&args[1], &args[2]),
        // (the ultra instinct, a prototype: docs/design/ultra-instinct-plan.md)
        Some("ultra-check") => ultra(&args[1..], true),
        Some("ultra") | Some("ultra-worker") => ultra(&args[1..], false),
        Some("secret-survey") => {
            // (one map in this process, its rows out: the survey's children)
            let one = args.iter().any(|a| a == "--one");
            let jobs = args
                .iter()
                .position(|a| a == "--jobs")
                .and_then(|i| args.get(i + 1))
                .and_then(|j| j.parse().ok())
                .unwrap_or(12);
            let words: Vec<String> = args[1..]
                .iter()
                .filter(|a| !a.starts_with("--"))
                .filter(|a| a.parse::<usize>().is_err())
                .cloned()
                .collect();
            match (words.split_first(), one) {
                (Some((wad, maps)), true) if maps.len() == 1 => {
                    evilboom::survey::survey_map(wad, &maps[0])
                }
                (Some((wad, maps)), false) if !maps.is_empty() => {
                    evilboom::survey::survey(wad, maps, jobs)
                }
                _ => Err("usage: evil-boom secret-survey <wad> <map>... [--jobs N]".into()),
            }
        }
        Some("neuro-gpt") => {
            let port = args
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000);
            let config = args
                .iter()
                .position(|a| a == "--config")
                .and_then(|i| args.get(i + 1))
                .map_or_else(
                    || {
                        evilboom::config::root()
                            .join("config")
                            .join("neuro-gpt.json")
                    },
                    std::path::PathBuf::from,
                );
            evilboom::neuro::gpt::serve(port, &config)
        }
        Some("console") => console_args(&args[1..])
            .and_then(|(bind, new_token)| evilboom::console::server::serve(&bind, new_token)),
        Some("neuro-mock") => {
            let port = args
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000);
            let file = args
                .iter()
                .skip(1)
                .rfind(|a| !a.starts_with("--") && a.parse::<u16>().is_err())
                .cloned()
                .unwrap_or_else(|| "out/neuro/actions.txt".into());
            evilboom::neuro::mock::serve(port, std::path::Path::new(&file))
        }
        Some("follow") if args.len() == 3 => follow(&args[1], &args[2]),
        Some("sector") if args.len() >= 4 => sector(&args[1], &args[2], &args[3..]),
        Some("brain-posts") if args.len() == 3 => brain_posts(&args[1], &args[2]),
        Some("run-map") => run_one(&args[1..]),
        Some("episode") => episode(&args[1..]),
        #[cfg(feature = "viewer")]
        Some("record") => record(&args[1..]),
        Some("episode-one") => episode_one(&args[1..]),
        Some("drive") => drive(&args[1..]),
        Some("sweep") => sweep(&args[1..]),
        Some("score") => evilboom::score::run(&args[1..]),
        Some("tuning") => {
            println!("{}", evilboom::tactical::tuning::Tuning::default().json());
            Ok(())
        }
        Some("score-report") if args.len() >= 2 => evilboom::score::report_files(&args[1..]),
        #[cfg(feature = "viewer")]
        Some("score-watch") => evilboom::score::watch(&args[1..]),
        Some("watch") => watch(&args[1..]),
        Some("arena") => arena(&args[1..]),
        Some("arena-one") => arena_one(&args[1..]),
        #[cfg(feature = "viewer")]
        Some("arena-watch") => arena_watch(&args[1..]),
        Some(cmd) if evilboom::help::topic(cmd).is_some() => Err(format!(
            "evil-boom {cmd}: not as given (evil-boom help {cmd})"
        )),
        Some(cmd) => Err(format!(
            "evil-boom: no command {cmd:?} (evil-boom help lists them)"
        )),
        None => Ok(()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
