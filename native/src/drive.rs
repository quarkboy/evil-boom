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
//!   bot N               the bot plays N tics
//!   where               position, sector, keys, the tally
//!   exit?               is there a way out from here (knowing the whole map)?
//!   sector S...         sectors' heights now, and as she believes them
//!   line L...           what she knows of lines
//!   objectives          what she'd do next: her options, with the length of the way there
//!   snapshot NAME [X,Y,ZOOM]   the viewer's window into out/drive/NAME.png

use std::path::Path;
use std::rc::Rc;

use crate::config::out_dir;
use crate::geom::{dist, Point};
use crate::knowledge::explorer::{Kind, Objective};
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
            "goto" | "goto-full" => {
                let goal = (num(1)?, num(2)?);
                goto(&mut s, goal, w[0] == "goto-full");
            }
            "warp" => {
                let angle = num(3).unwrap_or(s.obs().angle);
                s.e.game.warp(num(1)?, num(2)?, angle);
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
                }
            }
            "objectives" => {
                let obs = s.obs();
                for risky in [false, true] {
                    let options = s.agent.x_mut().ranked(&obs, risky, &mut crate::knowledge::explorer::NoHooks);
                    println!("  {} options{}", options.len(), if risky { " (risky)" } else { "" });
                    for obj in options.iter().take(12) {
                        let route = s.agent.x_mut().route_to(obj, &obs);
                        let how = route.map_or("no route".to_string(), |st| format!("{} steps", st.len()));
                        println!("    {} at ({:.0}, {:.0}), priority {}: {how}", obj.label, obj.goal.0, obj.goal.1, obj.priority());
                    }
                }
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
        let steps = steps_from_legs(&view, &route.legs, None, Some(&states));
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
