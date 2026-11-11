//! Plan E1M1 and follow the plan in the game (seed 0).

use std::rc::Rc;
use std::sync::Arc;

use evildoom::config::resolve_wad;
use evildoom::engine::{library, Engine, Options};
use evildoom::motor::follower::RouteFollower;
use evildoom::motor::Controls;
use evildoom::navigator::Navigator;
use evildoom::wad::{load_map, Wad};
use evildoom::walkthrough::plan_level;

/// a side room off E1M1's route
const KNOCKED_TO: (f64, f64) = (2160.0, -2432.0);

/// (exited, replans, the follower's log); None when the IWAD or the game isn't there. `knock`:
/// warped off the route 6 s in, like being blasted into a side room.
pub fn drive(knock: bool) -> Option<(bool, i32, Vec<String>)> {
    let iwad = resolve_wad("doom");
    if !iwad.exists() || !library().exists() {
        return None;
    }
    let w = Wad::open(&iwad).expect("the IWAD reads");
    let m = Arc::new(load_map(&w, "E1M1", &[]).expect("E1M1 loads"));
    let mut nav = Navigator::new(Arc::clone(&m), 3).expect("the analysis");
    nav.keep_verified();
    let nav = Rc::new(nav);
    let view = Rc::new(nav.full_view());
    let start = m
        .things
        .iter()
        .find(|t| t.kind == 1)
        .map(|t| (t.x, t.y))
        .expect("a start");
    let plan = plan_level(&view, start, 60.0, 20000);
    assert!(plan.complete, "{}", plan.problem);
    let mut e = Engine::new(
        &iwad,
        "E1M1",
        &Options {
            seed: Some(0),
            ..Options::default()
        },
    )
    .expect("the game starts");
    e.start();
    let mut f = RouteFollower::new(view, plan.steps, Default::default(), None, true, e.tic);
    let mut knocked = !knock;
    while !e.finished() && !f.failed && e.tic < 120 * 35 {
        if !knocked && e.tic >= 6 * 35 {
            e.game.warp(KNOCKED_TO.0, KNOCKED_TO.1, 0.0);
            knocked = true;
            e.step(&Controls::default());
            continue;
        }
        let obs = e.observe();
        let c = f.act(&obs);
        e.step(&c);
    }
    Some((e.exited(), f.replans, f.log.events.clone()))
}
