//! Arenas: a fight set up where wanted, to see how she does in it. A map with its own monsters
//! left out (its items left in), her put somewhere with a loadout (weapons, ammo, health,
//! armor), the monsters of the scenario spawned where it says; then the fighting bot plays till
//! they're all dead (won), she is (lost), or the time's up (drawn).
//!
//! Scenarios come from a text file, one after another:
//!
//! ```text
//! arena cyber-rockets            # a name
//! map doom2 MAP07                # the WAD and map
//! skill 4                        # (4: ultra-violence, the default)
//! at 0 0 90                      # where she starts: x y angle
//! give shotgun rocket shell=40 rocket=30 health=100 armor=100 blue
//! spawn Cyberdemon 512 0 180     # a monster: its name, x y angle, `ambush` for deaf
//! seconds 120                    # the most it may take
//! items                          # the map's pickups left in (else they're taken out)
//! ```
//!
//! `give`: weapons by name (fist pistol shotgun chaingun rocket plasma bfg chainsaw ssg; the
//! fist and pistol always), ammo as clip= shell= cell= rocket=, health=, armor= with green or
//! blue, backpack.

use crate::config::resolve_wad;
use crate::dsda::{type_of, Loadout};
use crate::engine::{Engine, Options};
use crate::knowledge::model::Knowledge;
use crate::motor::{Controls, TICRATE};
use crate::navigator::Navigator;
use crate::play::Agent;
use crate::map::MapData;
use crate::wad::{load_map, Wad};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Spawn {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub ambush: bool,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: String,
    pub wad: String,
    pub map: String,
    pub skill: i32,
    pub at: (f64, f64, f64),
    pub give: Loadout,
    pub spawns: Vec<Spawn>,
    pub seconds: f64,
    /// the map's own pickups left lying about
    pub items: bool,
}

/// How a scenario went.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// "WON", "LOST" or "DRAWN"
    pub result: &'static str,
    pub seconds: f64,
    pub health: i32,
    pub armor: i32,
    pub kills: usize,
    pub monsters: usize,
}

fn default_loadout() -> Loadout {
    Loadout { weapons: 0b11, clip: 50, shell: 0, cell: 0, rocket: 0, backpack: false, health: 100, armor: 0, armor_type: 0 }
}

/// The scenarios in `text`.
pub fn parse(text: &str) -> Result<Vec<Scenario>, String> {
    let mut out: Vec<Scenario> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        let bad = |what: &str| format!("line {}: {what}: {raw}", n + 1);
        let num = |i: usize| w.get(i).and_then(|v| v.parse::<f64>().ok()).ok_or_else(|| bad("a number wanted"));
        if w[0] == "arena" {
            out.push(Scenario {
                name: w.get(1).ok_or_else(|| bad("a name wanted"))?.to_string(),
                wad: "doom2".into(),
                map: "MAP01".into(),
                skill: 4,
                at: (0.0, 0.0, 0.0),
                give: default_loadout(),
                spawns: Vec::new(),
                seconds: 120.0,
                items: false,
            });
            continue;
        }
        let s = out.last_mut().ok_or_else(|| bad("no `arena` line before this"))?;
        match w[0] {
            "map" => {
                s.wad = w.get(1).ok_or_else(|| bad("a WAD wanted"))?.to_string();
                s.map = w.get(2).ok_or_else(|| bad("a map wanted"))?.to_uppercase();
            }
            "skill" => s.skill = num(1)? as i32,
            "at" => s.at = (num(1)?, num(2)?, num(3)?),
            "seconds" => s.seconds = num(1)?,
            "items" => s.items = true,
            "spawn" => s.spawns.push(Spawn {
                name: w.get(1).ok_or_else(|| bad("a monster wanted"))?.to_string(),
                x: num(2)?,
                y: num(3)?,
                angle: num(4)?,
                ambush: w.get(5) == Some(&"ambush"),
            }),
            "give" => {
                let g = &mut s.give;
                for item in &w[1..] {
                    let (k, v) = item.split_once('=').map_or((*item, None), |(k, v)| (k, v.parse::<i32>().ok()));
                    let bit = |b: i32| 1 << b;
                    match (k, v) {
                        ("fist", None) => g.weapons |= bit(0),
                        ("pistol", None) => g.weapons |= bit(1),
                        ("shotgun", None) => g.weapons |= bit(2),
                        ("chaingun", None) => g.weapons |= bit(3),
                        ("rocket", None) => g.weapons |= bit(4),
                        ("plasma", None) => g.weapons |= bit(5),
                        ("bfg", None) => g.weapons |= bit(6),
                        ("chainsaw", None) => g.weapons |= bit(7),
                        ("ssg", None) => g.weapons |= bit(8),
                        ("clip", Some(v)) => g.clip = v,
                        ("shell", Some(v)) => g.shell = v,
                        ("cell", Some(v)) => g.cell = v,
                        ("rocket", Some(v)) => g.rocket = v,
                        ("health", Some(v)) => g.health = v,
                        ("armor", Some(v)) => {
                            g.armor = v;
                            if g.armor_type == 0 {
                                g.armor_type = 1;
                            }
                        }
                        ("green", None) => g.armor_type = 1,
                        ("blue", None) => g.armor_type = 2,
                        ("backpack", None) => g.backpack = true,
                        _ => return Err(bad(&format!("what's {item}?"))),
                    }
                }
            }
            other => return Err(bad(&format!("what's {other}?"))),
        }
    }
    Ok(out)
}

/// A scenario set up: the engine (her put where it says with the loadout, the monsters spawned,
/// the map's pickups out unless kept), her agent, the monsters' ids, and the WAD and map.
pub struct Prepared {
    pub engine: Engine,
    pub agent: Agent,
    pub ids: Vec<i32>,
    pub wad: Wad,
    pub map: Arc<MapData>,
}

/// Set scenario `s` up (`drawn`: the game draws its frames, for watching; `sound`: its sound out
/// loud too).
pub fn prepare(s: &Scenario, seed: Option<u32>, drawn: bool, sound: bool) -> Result<Prepared, String> {
    let iwad = resolve_wad(&s.wad);
    let w = Wad::open(&iwad)?;
    let m = Arc::new(load_map(&w, &s.map, &[])?);
    let mut nav = Navigator::new(Arc::clone(&m), s.skill)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    let mut e = Engine::new(&iwad, &s.map, &Options { skill: s.skill, nomonsters: true, objects: true, drawn, sound, seed })?;
    e.start();
    // (a tic for the level to be there)
    e.step(&Controls::default());
    if !e.game.warp(s.at.0, s.at.1, s.at.2) {
        return Err(format!("{}: can't stand at ({}, {})", s.name, s.at.0, s.at.1));
    }
    if !s.items && e.game.clear_items().is_none() {
        return Err("the game library hasn't evil_clear_items (rebuild it)".into());
    }
    if !e.game.give(&s.give) {
        return Err("the game library hasn't evil_give (rebuild it)".into());
    }
    let mut ids: Vec<i32> = Vec::new();
    for sp in &s.spawns {
        let kind = type_of(&sp.name).ok_or_else(|| format!("{}: no such monster", sp.name))?;
        let id = e.game.spawn(kind, sp.x, sp.y, sp.angle, sp.ambush).ok_or_else(|| format!("{}: can't spawn at ({}, {})", sp.name, sp.x, sp.y))?;
        ids.push(id);
    }
    let knowledge = Knowledge::new(Rc::clone(&nav), true, true);
    let agent = Agent::new(&nav, knowledge, s.skill, false, 1);
    Ok(Prepared { engine: e, agent, ids, wad: w, map: m })
}

/// Play scenario `s` (one a process: the engine is all globals). The fight's log goes to `log`.
pub fn run(s: &Scenario, seed: Option<u32>, log: &mut Vec<String>) -> Result<Outcome, String> {
    let Prepared { engine: mut e, mut agent, ids, .. } = prepare(s, seed, false, false)?;
    let start = e.tic;
    let until = start + (s.seconds * TICRATE) as i64;
    let mut c: Option<Controls> = None;
    let mut log_n = 0;
    let result = loop {
        let ob = e.observe();
        let alive = ids.iter().filter(|id| ob.things.iter().flatten().any(|t| t.id == **id)).count();
        if e.dead() {
            break "LOST";
        }
        if alive == 0 {
            break "WON";
        }
        if e.tic >= until || e.exited() {
            break "DRAWN";
        }
        if (e.tic - start) % 35 == 0 {
            let near: Vec<String> = ob
                .things
                .iter()
                .flatten()
                .filter(|t| ids.contains(&t.id))
                .map(|t| format!("{} {:.0} ({} hp)", t.name, crate::geom::dist((t.x, t.y), ob.pos()), t.health))
                .collect();
            let p = e.game.player();
            let states: Vec<String> = ob.things.iter().flatten().filter(|t| ids.contains(&t.id)).map(|t| format!("{}@{}", t.state, t.attack)).collect();
            log.push(format!("  (pending {} mom {:.1},{:.1} states {})", p.pendingweapon, p.momx, p.momy, states.join(" ")));
            log.push(format!(
                "tic {}: health {} armor {}, ammo {:?}, at ({:.0}, {:.0}, z {:?}) weapon {:?}; {}; last {:?}",
                ob.tic,
                ob.health.unwrap_or(0.0),
                ob.armor.unwrap_or(0.0),
                ob.ammo.clone().unwrap_or_default(),
                ob.x,
                ob.y,
                ob.z,
                ob.weapon,
                near.join(", "),
                c
            ));
        }
        let mut ctl = agent.act(&ob, c.as_ref());
        // (the fight's the thing: no switches or doors, which in MAP07's ring set off what
        // crushed her, between fights)
        ctl.use_ = false;
        let l = &agent.x().log;
        log.extend(l[log_n.min(l.len())..].iter().cloned());
        log_n = l.len();
        e.step(&ctl);
        c = Some(ctl);
    };
    let ob = e.observe();
    let alive = ids.iter().filter(|id| ob.things.iter().flatten().any(|t| t.id == **id)).count();
    let p = e.game.player();
    Ok(Outcome { result, seconds: (e.tic - start) as f64 / TICRATE, health: p.health, armor: p.armor, kills: ids.len() - alive, monsters: ids.len() })
}
