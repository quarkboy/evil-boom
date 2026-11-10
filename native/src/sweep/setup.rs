//! Map loading, agent initialization and replaying recorded input to a branch point.

use super::{hold_exit, RunOptions};
use crate::config::resolve_wad;
use crate::engine::{Engine, Options};
use crate::knowledge::completion::most_items;
use crate::knowledge::model::{Knowledge, Note};
use crate::map::MapData;
use crate::motor::{Controls, Observation, TICRATE};
use crate::navigator::Navigator;
use crate::play::Agent;
use crate::tactical::tuning::Tuning;
use crate::replay::{Entry, Recording};
use crate::wad::{load_map, Wad};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Levels with secrets nobody can get: how many short of the tally counts as all (E4M3: two
/// torch sectors no one can stand in; E4M7: one; MAP15's #9 and MAP27's #5: teleporter sectors
/// the teleport line keeps everyone out of).
pub(crate) fn broken_secrets(map: &str) -> i32 {
    match map {
        "E4M3" => 2,
        "E4M7" | "MAP15" | "MAP27" => 1,
        _ => 0,
    }
}

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

/// `engine`: the game already going (an episode's next level, in the same process: on to this
/// map with what she carries), else a new one.
pub fn setup(wad: &str, map_name: &str, o: &RunOptions, drawn: bool, engine: Option<Engine>) -> Result<Setup, String> {
    let iwad = resolve_wad(wad);
    let w = Wad::open(&iwad)?;
    let map_name = map_name.to_uppercase();
    let m = Arc::new(load_map(&w, &map_name, &[])?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    let nav = Rc::new(nav);
    let fresh = engine.is_none();
    let mut e = match engine {
        Some(e) => e,
        None => Engine::new(&iwad, &map_name, &Options { skill, nomonsters: o.fight == 0, objects: o.fight > 0 || o.expert || o.things, drawn, sound: false, capture: false, seed: o.seed })?,
    };
    e.carry.clone_from(&o.carry);
    if fresh {
        e.start();
    } else {
        e.goto(&map_name)?;
    }
    if !o.mortal {
        e.set_god();
    }
    let knowledge = Knowledge::new(Rc::clone(&nav), o.mortal, !o.peek);
    let mut agent = Agent::new(&nav, knowledge, o.fight, o.expert, o.lives, Tuning::from(o.tuning.as_deref())?);
    if o.decisions {
        agent.x_mut().journal.on = true;
        if let Some(p) = agent.player_mut() {
            p.keep_journal();
        }
    }
    if let (Some(names), Some(p)) = (&o.bar, agent.player_mut()) {
        p.bar(names)?;
    }
    let secrets_short = broken_secrets(&map_name);
    if o.secrets {
        agent.x_mut().exit_last = true;
        agent.x_mut().secrets_mode = true;
    }
    // (her own waits inside the stall clock: given up on things back as soon, for its share of
    // the usual 300 s, and counted stranded in half of it at most; MAP24's way out of the pit, a
    // wall given another go a minute on, and E3M3's trap, stranded a minute on, both over first
    // at a stall of 60 s. At 300 s, as ever: MAP14's door by the crusher, back every 150 s, lost
    // it the blur sphere)
    agent.x_mut().patience = (o.stall_seconds / 300.0).min(1.0);
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
        let restarts: BTreeMap<i64, String> =
            run.entries.iter().filter_map(|en| if let Entry::Restart(t, why) = en { Some((*t, why.clone())) } else { None }).collect();
        let mut notes: BTreeMap<i64, Vec<Note>> = BTreeMap::new();
        for en in &run.entries {
            if let Entry::Note(t, n) = en {
                notes.entry(*t).or_default().push(n.clone());
            }
        }
        // (on past a death: the level again where the run started it again)
        while e.tic < to && (!e.finished() || restarts.contains_key(&e.tic)) {
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
                hold_exit(&e, &mut agent, o, secrets_short, most_items(&m, skill), false);
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
