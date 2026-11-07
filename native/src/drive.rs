//! Driving her by hand, from a script (`evil drive`): to try things out in a level as it is at
//! some moment of a run (played back with --from), and see what the game does and what she makes
//! of it. Her knowledge goes on taking in what she sees the whole time.
//!
//! Commands, one a line (# comments):
//!   goto X Y            walk there, routed by what she knows (her planner and follower)
//!   goto-full X Y       the same, routed knowing the whole map as it is now
//!   warp X Y [ANGLE]    put her there
//!   face X Y            turn to face a point
//!   use                 press use (facing as she is)
//!   move F S T N        forward, strafe, turn (degrees a tic, right) for N tics
//!   wait N              N tics, nothing pressed
//!   fire N [SLOT]       hold fire for N tics (facing as she is), with that weapon first
//!   bot N               the bot plays N tics
//!   where               position, sector, keys, the tally
//!   items               counted items there: seen, hers to get now, or from the level's start
//!   exit?               is there a way out from here (knowing the whole map)?
//!   sector S...         sectors' heights now, and as she believes them
//!   line L...           what she knows of lines
//!   objectives          what she'd do next: her options, numbered, with the way there
//!   next                let her choose her next objective and do it; what came of it
//!   pick N              do option N of the last `objectives` instead; what came of it
//!   force visit X Y | force wall LINE | force item X Y
//!                       give her an objective she wasn't offering; what came of it
//!   snapshot NAME [X,Y,ZOOM]   the viewer's window into out/drive/NAME.png
//!   reach X Y [plan]    how the flood fill her options come from gets there (or, plan, the
//!                       planner's first check): the polys on the way and how each was entered
//!   fits X Y            does the box stand there, at the heights she last saw
//!   budget N            search her routes in up to N plan steps from now on
//!   stats               the routes planned, found and not, and the lists of options made

use std::path::Path;
use std::rc::Rc;

use crate::config::out_dir;
use crate::geom::{dist, Point};
use crate::knowledge::explorer::{Ident, Kind, Objective};
use crate::motor::follower::RouteFollower;
use crate::motor::{heading, norm, Controls, Observation, TICRATE};
use crate::navigator::{Plan, States};
use crate::specials::classify_line;
use crate::sweep::{setup, RunOptions, Setup};
use crate::walkthrough::steps_from_legs;

/// a walk gives up after this long
const GOTO_TICS: i64 = 90 * 35;

pub fn drive(wad: &str, map: &str, o: &RunOptions, script: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(script).map_err(|e| format!("{}: {e}", script.display()))?;
    let mut s = setup(wad, map, o, true)?;
    let mut listed: Vec<Objective> = Vec::new();
    println!("at tic {} ({:.1}s)", s.e.tic, s.e.tic as f64 / TICRATE);
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        println!("> {line}");
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |k: usize| w.get(k).and_then(|v| v.parse::<f64>().ok()).ok_or_else(|| format!("line {}: {line}: a number wanted", i + 1));
        match w[0] {
            "fits" => {
                // (does the box stand at a point, at the heights she last saw: as the wall stand
                // spots and standable() have it)
                let q = (num(1)?, num(2)?);
                let x = s.agent.x();
                let k = &x.knowledge;
                let init = &s.nav.movers.initial;
                let heights: Vec<(f64, f64)> = (0..init.len()).map(|i| k.seen_heights.get(&i).copied().unwrap_or(init[i])).map(|h| (h.floor, h.ceiling)).collect();
                let (clear, floor, ceiling) = s.nav.jump_lab.fits_at_heights(q, &heights);
                println!("  ({:.0}, {:.0}): clear {clear}, floor {floor}, ceiling {ceiling}; standable {}", q.0, q.1, x.standable(q, false));
            }
            "budget" => {
                // (the most plan steps her routes are searched for in, from now on)
                let n = num(1)? as usize;
                let x = s.agent.x_mut();
                x.plan_expansions = n;
                x.forget_no_routes();
                println!("  routes searched for in up to {n} steps");
            }
            "stats" => {
                // (the routes planned and lists of options made so far, and their time)
                println!("  {:?}", s.agent.x().stats);
            }
            "reach" => {
                // (how the flood fill her options come from gets there, at every height she
                // could give things: for an option offered with no route)
                let goal = (num(1)?, num(2)?);
                let obs = s.obs();
                match s.agent.x_mut().reach_chain(&obs, goal, w.get(3) == Some(&"plan")) {
                    None => println!("  not in reach"),
                    Some(chain) => {
                        for l in chain {
                            println!("    {l}");
                        }
                    }
                }
            }
            "path" => {
                // (the route she'd take there, and each leg's waypoints, as smoothed)
                let goal = (num(1)?, num(2)?);
                let obs = s.obs();
                let x = s.agent.x_mut();
                let known = x.knowledge.known_polys();
                let at = s.nav.poly_at(goal);
                println!("    goal poly {:?}: known {}, sector {:?}", at, at.is_some_and(|p| known.contains(&p)), at.map(|p| s.nav.sector_of_poly(p)));
                match x.route_to(&Objective::new(Kind::Visit, goal, "go there"), &obs) {
                    None => println!("  no way there, by what she knows"),
                    Some(steps) => {
                        for st in &steps {
                            println!("  step: {}", st.label);
                            if let Some(p) = &st.path {
                                let pts: Vec<String> = p.waypoints.iter().map(|w| w.map_or("teleport".to_string(), |w| format!("({:.0}, {:.0})", w.0, w.1))).collect();
                                println!("    {}", pts.join(" "));
                                let polys: Vec<String> = p.polys.iter().map(|&q| format!("{q}/{}", s.nav.sector_of_poly(q))).collect();
                                println!("    polys (poly/sector): {}", polys.join(" "));
                                if !p.steps.is_empty() {
                                    println!("    {}", p.steps.join("; "));
                                }
                            }
                        }
                    }
                }
            }
            "goto" | "goto-full" => {
                let goal = (num(1)?, num(2)?);
                goto(&mut s, goal, w[0] == "goto-full");
            }
            "warp" => {
                let angle = num(3).unwrap_or(s.obs().angle);
                if !s.e.game.warp(num(1)?, num(2)?, angle) {
                    println!("  the game wouldn't put her there (blocked)");
                }
                tick(&mut s, Controls::default());
            }
            "face" => {
                let at = (num(1)?, num(2)?);
                for _ in 0..40 {
                    let obs = s.obs();
                    let off = norm(heading(obs.pos(), at) - obs.angle);
                    if off.abs() < 1.0 {
                        break;
                    }
                    tick(&mut s, Controls { turn: (-off).clamp(-20.0, 20.0), ..Controls::default() });
                }
            }
            "use" => {
                tick(&mut s, Controls { use_: true, ..Controls::default() });
                for _ in 0..3 {
                    tick(&mut s, Controls::default());
                }
                if let Some(m) = &s.obs().message {
                    println!("  the game says: {m}");
                }
            }
            "move" => {
                let ctl = Controls { forward: num(1)?, strafe: num(2)?, turn: num(3)?, run: true, ..Controls::default() };
                for _ in 0..num(4)? as i64 {
                    tick(&mut s, ctl);
                }
            }
            "fire" => {
                if let Ok(slot) = num(2) {
                    tick(&mut s, Controls { weapon: slot as i32, ..Controls::default() });
                    for _ in 0..20 {
                        tick(&mut s, Controls::default());
                    }
                    let o = s.obs();
                    println!("  weapon now {:?} (owns {:?}, ammo {:?})", o.weapon, o.weapons, o.ammo);
                }
                for _ in 0..num(1)? as i64 {
                    tick(&mut s, Controls { attack: true, ..Controls::default() });
                }
                let obs = s.obs();
                println!("  barrels left: {}", obs.barrels.as_ref().map_or(0, Vec::len));
            }
            "wait" => {
                for _ in 0..num(1)? as i64 {
                    tick(&mut s, Controls::default());
                }
            }
            "bot" => {
                for _ in 0..num(1)? as i64 {
                    let obs = s.e.observe();
                    let c = s.agent.act(&obs, s.c.as_ref());
                    s.e.step(&c);
                    s.c = Some(c);
                    s.obs = Some(obs);
                }
                let x = s.agent.x();
                println!("  doing: {}", x.current.as_ref().map_or("-", |c| c.label.as_str()));
            }
            "where" => where_(&mut s),
            "walls" => {
                // (the walls she suspects from here, best first; "walls all": every wall round)
                let obs = s.obs();
                let x = s.agent.x_mut();
                x.objectives(&obs, false, &mut crate::knowledge::explorer::NoHooks); // (her reach now)
                let reach = x.reach.clone();
                let found = x.suspected(&obs, &reach, w.get(1) == Some(&"all"));
                println!("  {} walls suspected", found.len());
                for (score, li, stand, _) in found.iter().take(30) {
                    println!("    line {li}: {score:.1}, from ({:.0}, {:.0})", stand.0, stand.1);
                }
            }
            "items" => {
                // (each counted item still there: seen? hers to get now? from the level's start?)
                let obs = s.obs();
                let x = s.agent.x_mut();
                x.objectives(&obs, false, &mut crate::knowledge::explorer::NoHooks); // (her reach now)
                let known = x.knowledge.known_polys();
                let fresh = x.fresh_view();
                let start = s.m.things.iter().find(|t| t.kind == 1).map(|t| (t.x, t.y));
                let states = fresh.initial_states();
                for t in obs.things.iter().flatten().filter(|t| crate::knowledge::completion::COUNTED.contains(&t.name)) {
                    let p = s.nav.poly_at((t.x, t.y));
                    let seen = p.is_some_and(|p| known.contains(&p));
                    let now = seen && x.touch_spot((t.x, t.y), &x.reach).is_some();
                    let from_start = seen && start.is_some_and(|st| fresh.plan(st, &[(t.x, t.y)], &Plan { states: Some(&states), max_expansions: 2000, max_seconds: 10.0, ..Plan::default() }).is_some());
                    println!("  {} at ({:.0}, {:.0}) sector {}: {}", t.name, t.x, t.y, p.map_or("-".into(), |p| s.nav.sector_of_poly(p).to_string()), if !seen { "never seen" } else if now { "seen, hers to get now" } else if from_start { "seen, not now, but from the start" } else { "seen, not to be had" });
                }
            }
            "exit?" => {
                let obs = s.obs();
                let plan = crate::sweep::plan_here(&s.nav, &obs, 20.0);
                println!("  knowing the whole map, from here: {}", if plan.complete { format!("out in {} steps", plan.steps.len()) } else { plan.problem.clone() });
            }
            "sector" => {
                let states = s.agent.x().states();
                let obs = s.obs();
                for v in &w[1..] {
                    let Ok(sec) = v.parse::<usize>() else { continue };
                    let live = obs.heights.get(sec).copied();
                    let seen = s.agent.x().knowledge.seen_tic.get(&sec).copied();
                    println!(
                        "  sector {sec}: now {}, she believes {}, last seen tic {}",
                        live.map_or("?".into(), |h| format!("{}..{}", h.floor, h.ceiling)),
                        states.get(sec).map_or("?".into(), |hs| hs.iter().map(|h| format!("{}..{}", h.floor, h.ceiling)).collect::<Vec<_>>().join(" or ")),
                        seen.map_or("never".into(), |t| t.to_string())
                    );
                }
            }
            "line" => {
                let k = &s.agent.x().knowledge;
                for v in &w[1..] {
                    let Ok(li) = v.parse::<usize>() else { continue };
                    let special = classify_line(s.m.linedefs[li].special).map_or("-".to_string(), |i| i.description);
                    let fired = k.fired.iter().filter(|f| f.line == li).count();
                    println!(
                        "  line {li} ({special}): mapped {}, fired {fired}x, tried as a wall {}, locked {}, in the way {}",
                        k.mapped.contains(&li),
                        k.tried_walls.contains_key(&li),
                        k.locked.contains(&li),
                        k.in_the_way.contains(&li)
                    );
                    // (and as a trigger: does she take it for one, and where it's set off from)
                    let x = s.agent.x();
                    println!("    looks interactive {}, offered {}", k.looks_interactive(li), k.interactables().contains(&li));
                    for t in s.nav.triggers.iter().filter(|t| t.linedef() == Some(li)) {
                        let reach = t.activators.intersection(&x.reach).count();
                        let shots: std::collections::BTreeSet<(i64, i64)> = t.activators.iter().filter_map(|&p| s.nav.shot_floors.get(&(li, p))).map(|&(lo, hi)| (lo as i64, hi as i64)).collect();
                        let sectors: std::collections::BTreeSet<usize> = t.activators.iter().map(|&p| s.nav.sector_of_poly(p)).collect();
                        println!("    trigger {}: {} activators ({reach} in reach), sectors {sectors:?}{}", t.id, t.activators.len(), if shots.is_empty() { String::new() } else { format!(", shot from floors {shots:?}") });
                        let polys: Vec<String> = t.activators.iter().map(|&p| format!("{p}{}@({:.0},{:.0})", if x.reach.contains(&p) { "*" } else { "" }, s.nav.mesh.center(p).0, s.nav.mesh.center(p).1)).collect();
                        println!("      from polys (* in reach): {}", polys.join(" "));
                    }
                }
            }
            "objectives" => {
                let obs = s.obs();
                listed.clear();
                for risky in [false, true] {
                    let options = s.agent.ranked(&obs, risky);
                    println!("  {} options{}", options.len(), if risky { " (risky)" } else { "" });
                    for obj in options.into_iter().take(12) {
                        let route = s.agent.x_mut().route_to(&obj, &obs);
                        let how = route.map_or("no route".to_string(), |st| format!("{} steps, {:.0} long", st.len(), crate::knowledge::explorer::route_length(&st)));
                        println!("    {:2}. {} at ({:.0}, {:.0}), priority {}: {how}", listed.len() + 1, obj.label, obj.goal.0, obj.goal.1, obj.priority());
                        listed.push(obj);
                    }
                }
            }
            "next" => {
                // (whatever she's doing given up first: her own choice from here)
                s.agent.x_mut().current = None;
                carry_out(&mut s, None);
            }
            "pick" => {
                let n = num(1)? as usize;
                let Some(obj) = n.checked_sub(1).and_then(|i| listed.get(i)).cloned() else { return Err(format!("line {}: no option {n} (`objectives` first)", i + 1)) };
                take(&mut s, obj);
            }
            "force" => {
                let obj = match w.get(1).copied() {
                    Some("visit") => Objective::new(Kind::Visit, (num(2)?, num(3)?), "go there (forced)"),
                    Some("item") => Objective::new(Kind::Item, (num(2)?, num(3)?), "pick it up (forced)"),
                    Some("wall") => {
                        let li = num(2)? as usize;
                        let (a, b) = s.m.line_seg(li);
                        let len = dist(a, b).max(1e-6);
                        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                        // (pressed from in front: the front is right of v1->v2)
                        let (nx, ny) = ((b.1 - a.1) / len, -(b.0 - a.0) / len);
                        let mut o = Objective::new(Kind::Wall, (mid.0 + nx * 40.0, mid.1 + ny * 40.0), format!("try the wall (line {li}, forced)"));
                        o.wall = Some(li);
                        o.look_at = Some(mid);
                        o
                    }
                    _ => return Err(format!("line {}: force visit X Y | force wall LINE | force item X Y", i + 1)),
                };
                take(&mut s, obj);
            }
            "snapshot" => {
                let name = w.get(1).copied().unwrap_or("shot");
                let focus = w.get(2).and_then(|v| {
                    let p: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    (p.len() == 3).then(|| (p[0], p[1], p[2]))
                });
                let (back, r) = snapshot(s, &out_dir().join("drive").join(format!("{name}.png")), focus);
                s = back;
                r?;
            }
            other => return Err(format!("line {}: what's {other:?}?", i + 1)),
        }
    }
    where_(&mut s);
    Ok(())
}

impl Setup {
    fn obs(&mut self) -> Observation {
        self.e.observe()
    }
}

/// Have her do this objective (her own route and follower), and what came of it.
fn take(s: &mut Setup, obj: Objective) {
    let obs = s.obs();
    let (label, id) = (obj.label.clone(), obj.ident());
    if !s.agent.x_mut().take(&obs, obj) {
        println!("  no route to '{label}'");
        return;
    }
    carry_out(s, Some(id));
}

/// The bot plays till the objective it's on (`doing`; None: the one it chooses next) is over:
/// what she did, what she learned, what she went on to.
fn carry_out(s: &mut Setup, doing: Option<Ident>) {
    let start_log = s.agent.x().log.len();
    let start_tic = s.e.tic;
    let knew = s.agent.x().knowledge.summary();
    let tally = s.e.tally();
    let mut doing = doing;
    while s.e.tic - start_tic < GOTO_TICS * 2 && !s.e.finished() {
        let obs = s.e.observe();
        let c = s.agent.act(&obs, s.c.as_ref());
        s.e.step(&c);
        s.c = Some(c);
        s.obs = Some(obs);
        let now = s.agent.x().current.as_ref().map(Objective::ident);
        match (&doing, &now) {
            (None, Some(l)) => doing = Some(l.clone()),
            (Some(d), n) if n.as_ref() != Some(d) => break,
            _ => {}
        }
    }
    let x = s.agent.x();
    for l in &x.log[start_log.min(x.log.len())..] {
        println!("  {l}");
    }
    let after = s.e.tally();
    println!("  took {:.1}s; items {} -> {}, secrets {} -> {}", (s.e.tic - start_tic) as f64 / TICRATE, tally[0].0, after[0].0, tally[2].0, after[2].0);
    let k = x.knowledge.summary();
    if k != knew {
        println!("  knows now: {k}");
    }
    println!("  next: {}", x.current.as_ref().map_or("-", |c| c.label.as_str()));
}

/// A tic with these controls; what she sees goes into her knowledge.
fn tick(s: &mut Setup, c: Controls) {
    let obs = s.e.observe();
    s.agent.x_mut().knowledge.update(&obs, s.c.as_ref());
    s.e.step(&c);
    s.c = Some(c);
    s.obs = Some(obs);
}

fn where_(s: &mut Setup) {
    let obs = s.obs();
    let poly = s.nav.poly_at(obs.pos());
    let tally = s.e.tally();
    println!(
        "  tic {} ({:.1}s) at ({:.0}, {:.0}) z {:.0} facing {:.0}, poly {}, sector {}; keys {:?}; items {}/{}, secrets {}/{}",
        obs.tic,
        obs.tic as f64 / TICRATE,
        obs.x,
        obs.y,
        obs.z.unwrap_or(0.0),
        obs.angle,
        poly.map_or("-".into(), |p| p.to_string()),
        poly.map_or("-".into(), |p| s.nav.sector_of_poly(p).to_string()),
        obs.keys.unwrap_or_default().iter().map(|k| k.name()).collect::<Vec<_>>(),
        tally[0].0,
        tally[0].1,
        tally[2].0,
        tally[2].1
    );
}

fn goto(s: &mut Setup, goal: Point, full: bool) {
    let obs = s.obs();
    let keys = obs.keys.unwrap_or_default();
    let (view, steps, states) = if full {
        let view = Rc::new(s.nav.full_view());
        let states: States = obs.heights.iter().map(|h| vec![*h]).collect();
        let route = view.plan(obs.pos(), &[goal], &Plan { keys, states: Some(&states), max_expansions: 2000, max_seconds: 60.0, ..Plan::default() });
        let Some(route) = route else {
            println!("  no way there, knowing the whole map");
            return;
        };
        let walk = crate::walkthrough::Step { path: None, action: crate::walkthrough::Action::Walk, trigger: None, key: None, label: "go there".into(), expects: Vec::new() };
        let steps = steps_from_legs(&view, &route.legs, Some(walk), Some(&states));
        (view, steps, states)
    } else {
        let obj = Objective::new(Kind::Visit, goal, "go there");
        let x = s.agent.x_mut();
        let Some(steps) = x.route_to(&obj, &obs) else {
            println!("  no way there, by what she knows");
            return;
        };
        (x.view(), steps, x.states())
    };
    for st in &steps {
        println!("  step: {}", st.label);
    }
    let mut f = RouteFollower::new(view, steps, keys, Some(states), false, obs.tic);
    let t0 = obs.tic;
    let mut seen = 0;
    while !f.done && !f.failed && s.e.tic - t0 < GOTO_TICS && !s.e.finished() {
        let obs = s.e.observe();
        s.agent.x_mut().knowledge.update(&obs, s.c.as_ref());
        let c = f.act(&obs);
        s.e.step(&c);
        s.c = Some(c);
        s.obs = Some(obs);
        for ev in &f.log.events[seen..] {
            println!("    {ev}");
        }
        seen = f.log.events.len();
    }
    let obs = s.obs();
    println!("  {} at ({:.0}, {:.0}), {:.0} from the goal", if f.done { "arrived" } else if f.failed { "failed" } else { "gave up" }, obs.x, obs.y, dist(obs.pos(), goal));
}

#[cfg(feature = "viewer")]
fn snapshot(s: Setup, path: &Path, focus: Option<(f64, f64, f64)>) -> (Setup, Result<(), String>) {
    // (the viewer has the game and Evil for a moment)
    let Setup { iwad, w, m, map_name, skill, nav, e, agent, deaths, c, obs, rec, secrets_short } = s;
    let (e, agent, r) = crate::viewer::snapshot_to(path, &w, std::sync::Arc::clone(&m), &map_name, e, agent, focus);
    if r.is_ok() {
        println!("  saved {}", path.display());
    }
    (Setup { iwad, w, m, map_name, skill, nav, e, agent, deaths, c, obs, rec, secrets_short }, r)
}

#[cfg(not(feature = "viewer"))]
fn snapshot(s: Setup, _: &Path, _: Option<(f64, f64, f64)>) -> (Setup, Result<(), String>) {
    (s, Err("built without the viewer (the `viewer` feature)".into()))
}
