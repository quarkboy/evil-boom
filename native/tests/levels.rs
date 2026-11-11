//! Against the real IWADs (skipped when they aren't installed): reading them, the movement
//! rules, the navmesh, planning, and the movement simulator against the engine.

use evildoom::config::resolve_wad;
use evildoom::nav::{can_cross, Heights};
use evildoom::navigator::{Navigator, Plan};
use evildoom::specials::{classify_line, classify_sector, Category, Key, Keys};
use evildoom::wad::{load_map, Wad};
use evildoom::walkthrough::{action_for, Action};
use std::rc::Rc;
use std::sync::Arc;

fn wad(name: &str) -> Option<Wad> {
    let path = resolve_wad(name);
    path.exists()
        .then(|| Wad::open(&path).expect("the IWAD reads"))
}

fn h(floor: f64, ceiling: f64) -> Heights {
    Heights { floor, ceiling }
}

#[test]
fn movement_rules() {
    // a step up of at most 24; any drop, but not back up; 56 of headroom
    assert!(can_cross(h(0.0, 128.0), h(24.0, 128.0)));
    assert!(!can_cross(h(0.0, 128.0), h(25.0, 128.0)));
    assert!(can_cross(h(200.0, 400.0), h(0.0, 400.0)));
    assert!(!can_cross(h(0.0, 400.0), h(200.0, 400.0)));
    assert!(can_cross(h(0.0, 56.0), h(0.0, 56.0)));
    assert!(!can_cross(h(0.0, 55.0), h(0.0, 128.0)));
    assert!(!can_cross(h(0.0, 128.0), h(16.0, 64.0)));
    assert!(!can_cross(h(0.0, 0.0), h(0.0, 128.0)));
}

#[test]
fn reading_doom() {
    let Some(w) = wad("doom") else { return };
    let names = w.map_names();
    assert_eq!(
        (names.len(), names[0].as_str(), names[35].as_str()),
        (36, "E1M1", "E4M9")
    );
    let m = load_map(&w, "E1M1", &[]).expect("E1M1 loads");
    assert_eq!((m.linedefs.len(), m.sectors.len()), (486, 88));
    let start = m
        .things
        .iter()
        .find(|t| t.kind == 1)
        .expect("a player start");
    assert_eq!((start.x, start.y), (1056.0, -3616.0));
    let exits: Vec<usize> = m
        .linedefs
        .iter()
        .filter(|l| classify_line(l.special).is_some_and(|i| i.category == Category::Exit))
        .map(|l| l.index)
        .collect();
    assert_eq!(exits, vec![326]);
    let secrets: Vec<usize> = m
        .sectors
        .iter()
        .filter(|s| classify_sector(s.special).secret)
        .map(|s| s.index)
        .collect();
    assert_eq!(secrets, vec![83, 86, 87]);
    assert!(m.texture_heights.len() > 100 && m.texture_heights["STARTAN3"] == 128);
    for name in &names {
        let m = load_map(&w, name, &[]).expect("every map loads");
        assert!(!m.linedefs.is_empty() && !m.sectors.is_empty() && !m.things.is_empty());
    }
}

#[test]
fn raise_to_texture_uses_real_heights() {
    let Some(w) = wad("doom") else { return };
    let m = Arc::new(load_map(&w, "E4M7", &[]).expect("E4M7 loads"));
    let nav = Navigator::new(Arc::clone(&m), 3).expect("the analysis");
    let t = nav
        .triggers
        .iter()
        .find(|t| t.line == 264)
        .expect("the W1 raise to shortest texture");
    let [(sector, hs)] = t.effects.as_slice() else {
        panic!("one sector")
    };
    assert_eq!(hs[0].floor - m.sectors[*sector].floor, 128.0);
    assert_eq!(nav.movers.shortest_lower_texture(*sector), 128.0);
}

#[test]
fn mesh_polys_are_found_by_bsp_lookup() {
    let Some(w) = wad("doom") else { return };
    let m = Arc::new(load_map(&w, "E1M1", &[]).expect("E1M1 loads"));
    let nav = Navigator::new(Arc::clone(&m), 3).expect("the analysis");
    let polys: Vec<_> = nav.mesh.live_polys().collect();
    assert!(polys.len() >= m.subsectors.len());
    for (i, p) in polys {
        let vs = &p.vertices;
        let n = vs.len();
        let area: f64 = (0..n)
            .map(|k| vs[k].0 * vs[(k + 1) % n].1 - vs[(k + 1) % n].0 * vs[k].1)
            .sum::<f64>()
            .abs()
            / 2.0;
        assert!(area > 0.0);
        assert_eq!(nav.poly_at(p.center), Some(i));
    }
}

#[test]
fn e1m1_needs_no_keys_and_starts_with_doors_closed() {
    let Some(w) = wad("doom") else { return };
    let m = Arc::new(load_map(&w, "E1M1", &[]).expect("E1M1 loads"));
    let nav = Rc::new(Navigator::new(Arc::clone(&m), 3).expect("the analysis"));
    let view = nav.full_view();
    let start = m
        .things
        .iter()
        .find(|t| t.kind == 1)
        .map(|t| (t.x, t.y))
        .expect("a start");
    let stages = view.progression(nav.poly_at(start).expect("on the map"));
    assert_eq!(stages.len(), 1);
    assert!(stages[0]
        .exits
        .iter()
        .any(|&id| view.trigger(id).info.category == Category::Exit));
    let exit_t = nav
        .triggers
        .iter()
        .find(|t| t.info.category == Category::Exit)
        .expect("an exit");
    let all = nav.mesh.live_polys().map(|(i, _)| i).collect();
    let goal = nav.activation_point(exit_t, start, &all);
    // every door starts shut, so there's no path without opening one...
    assert!(view
        .find_path(start, goal, &nav.initial_states(), None)
        .is_none());
    // ...but the planner opens doors on the way
    let route = view
        .plan(start, &[goal], &Plan::default())
        .expect("a route");
    assert!(route
        .legs
        .iter()
        .any(|l| l.path.steps.iter().any(|n| n.contains("open door"))));
    assert_eq!(action_for(exit_t), Action::Use);
}

#[test]
fn e1m3_exit_is_behind_the_blue_door() {
    let Some(w) = wad("doom") else { return };
    let m = Arc::new(load_map(&w, "E1M3", &[]).expect("E1M3 loads"));
    let nav = Rc::new(Navigator::new(Arc::clone(&m), 3).expect("the analysis"));
    let view = nav.full_view();
    let start = m
        .things
        .iter()
        .find(|t| t.kind == 1)
        .map(|t| (t.x, t.y))
        .expect("a start");
    let stages = view.progression(nav.poly_at(start).expect("on the map"));
    // both keycards can be had without keys; the exit opens up once they're held
    assert_eq!(
        stages[0].keys_available,
        [Key::BlueCard, Key::YellowCard]
            .into_iter()
            .collect::<Keys>()
    );
    assert!(!stages[0]
        .exits
        .iter()
        .any(|&id| view.trigger(id).info.category == Category::Exit));
    assert!(stages[1]
        .exits
        .iter()
        .any(|&id| view.trigger(id).info.category == Category::Exit));
    let exit_t = nav
        .triggers
        .iter()
        .find(|t| t.info.category == Category::Exit)
        .expect("an exit");
    let all = nav.mesh.live_polys().map(|(i, _)| i).collect();
    let goal = nav.activation_point(exit_t, start, &all);
    let with = |keys: &[Key]| {
        view.plan(
            start,
            &[goal],
            &Plan {
                keys: keys.iter().copied().collect(),
                ..Plan::default()
            },
        )
    };
    assert!(with(&[]).is_none());
    assert!(with(&[Key::YellowCard]).is_none());
    assert!(with(&[Key::BlueCard]).is_some());
}

#[test]
fn jumps_match_the_engine() {
    use evildoom::engine::{library, Engine, Options};
    use evildoom::motor::Controls;

    let Some(w) = wad("doom2") else { return };
    if !library().exists() {
        return;
    }
    let m = Arc::new(load_map(&w, "MAP13", &[]).expect("MAP13 loads"));
    let nav = Navigator::new(Arc::clone(&m), 3).expect("the analysis");
    let mut engine = Engine::new(
        &w.path,
        "MAP13",
        &Options {
            seed: Some(0),
            ..Options::default()
        },
    )
    .expect("the game starts");
    engine.start();
    // Fixed ledge probes, independent of which jumps the finder currently accepts. Compare
    // against the current DSDA engine: the former fixture used a historical ViZDoom trace.
    for (x, y, angle) in [(1703.0, -3032.0, -120.0), (1700.0, -3048.0, 135.0)] {
        assert!(engine.game.warp(x, y, angle));
        let start = engine.observe();
        let controls: Vec<_> = (0..80)
            .map(|tic| Controls {
                forward: if tic < 20 { 50.0 } else { 0.0 },
                ..Controls::default()
            })
            .collect();
        let heights: Vec<_> = start.heights.iter().map(|h| h.tuple()).collect();
        let sim_controls: Vec<_> = controls
            .iter()
            .map(|c| evildoom::jumps::Controls {
                forward: c.forward,
                strafe: c.strafe,
                turn: c.turn,
            })
            .collect();
        let trace = nav
            .jump_lab
            .simulate((start.x, start.y, start.angle), &heights, &sim_controls);
        let mut airborne = false;
        for (tic, (command, &(sx, sy, sz, _, _))) in controls.iter().zip(&trace).enumerate() {
            engine.step(command);
            let obs = engine.observe();
            // The simulator retains ZDoom's trig tables; DSDA differs by less than one
            // map unit over these flights. Vertical motion should still agree.
            assert!(
                (sx - obs.x).hypot(sy - obs.y) < 1.0,
                "probe {x},{y}, tic {tic}: horizontal drift"
            );
            assert!(
                (sz - obs.z.unwrap()).abs() < 0.5,
                "probe {x},{y}, tic {tic}: vertical drift"
            );
            let sector = nav
                .poly_at(obs.pos())
                .map(|p| nav.sector_of_poly(p))
                .expect("on map");
            airborne |= obs.z.unwrap() > obs.heights[sector].floor + 1.0;
        }
        assert!(airborne, "the fixture must exercise flight");
    }
}
