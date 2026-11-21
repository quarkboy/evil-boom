// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Evil playing with monsters about: the explorer finds the way, the fight (`Combat`) takes
//! the controls when it needs them, and between them she goes for what she needs to keep
//! fighting (weapons, ammo, health, armour she has seen) and, once there's nothing else, hunts
//! down the monsters she knows are left (some maps open only when they're dead).

use std::collections::BTreeSet;
use std::rc::Rc;

use super::arbiter::{self, Legs};
use super::combat::{eye, eye_of, spread, Combat, Move, Seen};
use super::tuning::Tuning;
use crate::advisor::{Advisor, Question};
use crate::bestiary::{
    ammo_item, armor_item, carries, health_item, monster, weapon_in, weapon_item, Attack, CHAINSAW,
    SUPER_SHOTGUN,
};
use crate::decisions::Record;
use crate::domain::{Controls, Observation, Thing};
use crate::geom::segment_point_distance;
use crate::geom::{dist, Point};
use crate::knowledge::completion::Completionist;
use crate::knowledge::explorer::{Explorer, Hooks, Kind, Objective};
use crate::knowledge::model::{Knowledge, Known};
use crate::motor::follower::RouteFollower;
use crate::navigator::{Navigator, States, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::walkthrough::{Action, Step};

/// the most of each ammo she can carry (without a backpack), by weapon slot
use crate::bestiary::ammo_max;

mod advice;
mod boss;
mod cover;
mod errands;
mod going;
mod retreats;
mod stocking;

/// ...the nearest this many of them
const COVER_SPOTS: usize = 48;
/// ...the nearest this many of them...
const POST_LOOKS: usize = 3000;
/// ...looked at on a grid this fine...
const POST_GRID: f64 = 48.0;
/// monsters whose attacks, landing on others, set them fighting
const HEAVIES: [&str; 7] = [
    "Cyberdemon",
    "SpiderMastermind",
    "Fatso",
    "Arachnotron",
    "Revenant",
    "BaronOfHell",
    "HellKnight",
];
/// units a second she gets along a route, about (turns, doors, lifts)
const RUN_SPEED: f64 = 400.0;
/// the trail back: a place every this far, at most this many
const TRAIL_STEP: f64 = 48.0;
const TRAIL_LEN: usize = 96;

/// barons' and knights' fireballs don't hurt each other
fn species(name: &str) -> &str {
    if name == "HellKnight" {
        "BaronOfHell"
    } else {
        name
    }
}

/// The rest of a route passes none of them nearer than `clear`.
fn keeps_clear(obs: &Observation, f: &RouteFollower, them: &[Thing], clear: f64) -> bool {
    let path = f.planned_path(Some(obs.pos()));
    !path.is_empty()
        && path
            .windows(2)
            .filter_map(|w| Some((w[0]?, w[1]?)))
            .all(|(a, b)| {
                them.iter()
                    .all(|t| segment_point_distance(a, b, (t.x, t.y)) >= clear)
            })
}

fn mhealth(name: &str) -> f64 {
    monster(name).map_or(0.0, |m| f64::from(m.health))
}

/// What the player's hooks into the explorer need: the fight's state.
struct Fight {
    combat: Combat,
    /// the game goes on to another level after this one (an episode played through): what
    /// she'd take with her is had before the exit (`stock`)
    going_on: bool,
    /// the numbers she judges by (the fight's are the same)
    tune: Rc<Tuning>,
    /// what the hooks had to say (logged once the explorer's done)
    pending_log: Vec<String>,
    /// better guns she's turned aside for already
    offered: BTreeSet<i32>,
    present: BTreeSet<i32>,
    /// hunts that got her to where she was going with nothing to show for it: (the monster,
    /// the spot, whole units). Not gone to again: it's somewhere she can't get at from there
    hunted_spots: BTreeSet<(i32, i64, i64)>,
    /// --complete: every counted item too, and the level again for ones out of reach
    completion: Option<Completionist>,
    /// what she was doing each time she died (lives before this one)
    deaths: Vec<Death>,
    /// the way's been worked out under new costs: choose again (once)
    rethink: bool,
    /// a better gun seen that she can't get to (as of the last list of things to do)
    gun_out_of_reach: bool,
    /// the polys not yet seen of the sectors such guns lie in
    toward_gun: BTreeSet<i64>,
}

/// Why she died, as best her plight then tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    Blast,
    Unarmed,
    HurtingFloor,
    Surrounded,
    Outmatched,
    WornDown,
}

impl Cause {
    fn label(self) -> &'static str {
        match self {
            Cause::Blast => "a blast",
            Cause::Unarmed => "out of ammo",
            Cause::HurtingFloor => "on a floor that hurts",
            Cause::Surrounded => "surrounded",
            Cause::Outmatched => "outmatched",
            Cause::WornDown => "worn down",
        }
    }
}

/// A death: where, and what she was going for.
#[derive(Clone, Debug)]
struct Death {
    at: Point,
    kind: Kind,
    goal: Point,
    label: String,
    why: Cause,
}

/// What things were like for her a moment ago (what killed her, at a restart).
#[derive(Clone, Copy, Debug, Default)]
struct Plight {
    armed: bool,
    hurting_floor: bool,
    surrounded: bool,
    outmatched: bool,
    /// the last hit's size, and whether anything was close then
    last_hit: f64,
    close: bool,
}

impl Plight {
    /// Why she died, as best it can tell: a blast (a barrel, a rocket: one big hit, nothing
    /// close), out of ammo, on a floor that hurts (a pit), surrounded, outmatched, else worn down.
    fn why(&self, blast_hit: f64) -> Cause {
        if self.last_hit >= blast_hit && !self.close {
            Cause::Blast
        } else if !self.armed {
            Cause::Unarmed
        } else if self.hurting_floor {
            Cause::HurtingFloor
        } else if self.surrounded {
            Cause::Surrounded
        } else if self.outmatched {
            Cause::Outmatched
        } else {
            Cause::WornDown
        }
    }
}

/// Why she's falling back: each kind starts on its own rule (`Player::start_retreat`, in that
/// order) and ends on its own (`Player::retreat_over`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetreatKind {
    /// hurt, with something that shoots in sight (or an arch-vile): out of their sight
    Cover,
    /// nothing to shoot with and something after her: to ammo, or away
    ForAmmo,
    /// caught in a bad place: back the way she came
    Back,
    /// surrounded: out of the middle of them
    Out,
    /// a heavy she can't kill soon: where its shots land on the others
    Bait,
    /// in a long fight with a hitscan shooter, out in the open with nowhere near to duck: to
    /// a corner she knows of, across the open if it must be, to fight it from
    Post,
    /// monsters woken on ground she's never been on: back a little the way she came, to let
    /// them come to her
    FallBack,
    /// in among barrels with something about to set them off: the shortest way to somewhere out
    /// of every blast, run flat out, nothing else attended to till she's there
    OutOfBarrels,
}

impl RetreatKind {
    const ALL: [RetreatKind; 8] = [
        RetreatKind::Cover,
        RetreatKind::ForAmmo,
        RetreatKind::Back,
        RetreatKind::Out,
        RetreatKind::Bait,
        RetreatKind::Post,
        RetreatKind::FallBack,
        RetreatKind::OutOfBarrels,
    ];

    fn name(self) -> &'static str {
        match self {
            RetreatKind::Cover => "cover",
            RetreatKind::ForAmmo => "for ammo",
            RetreatKind::Back => "back",
            RetreatKind::Out => "out",
            RetreatKind::Bait => "bait",
            RetreatKind::Post => "post",
            RetreatKind::FallBack => "fall back",
            RetreatKind::OutOfBarrels => "out of the barrels",
        }
    }
}

/// A retreat under way: why, and the way there.
struct Retreat {
    kind: RetreatKind,
    route: RouteFollower,
}

pub struct Player {
    pub explorer: Explorer,
    fight: Fight,
    retreat: Option<Retreat>,
    bait_tic: i64,
    /// no way out of the barrels found: not looked for again till this tic
    barrel_run_again: i64,
    /// where she's begun a run out of barrels from
    barrel_runs: Vec<Point>,
    /// when the last retreat started or found no cover
    retreat_tic: i64,
    /// away from them out of ammo with none known: since when, and what she had and knew
    /// then (rounds, lines and sectors seen); None: not fled so yet, or better off since
    away_since: Option<(i64, (i32, usize))>,
    /// retreats to cover one after another with nothing come of them: how many, when the last
    /// was, and what she'd killed and knew then
    cover_runs: (u32, i64, (i32, Known)),
    /// no cover till then (`retreat.cover_rest`), and how many times it's been rested so
    cover_off: (i64, u32),
    /// the same of backing out of an ambush (`back_spent`)
    back_runs: (u32, i64, (i32, Known)),
    back_off: (i64, u32),
    /// the last retreat tried was for cover
    tried_cover: bool,
    /// when a way off for ammo was last called off with one of them on it
    walled_tic: i64,
    /// when something that shoots was last in sight
    shooters_tic: i64,
    /// where she was last tic (where she died, at a restart)
    last_pos: Point,
    /// and what things were like for her
    plight: Plight,
    /// who had her legs last tic (the journal notes a change)
    legs: Legs,
    /// where she was when the route last had her legs, while it hasn't
    left_route_at: Option<Point>,
    /// tics she's been pushing and not moving
    pushing_still: i64,
    /// the monsters she's fallen back from already (`fall_back`): not again for the same ones
    fallen_for: BTreeSet<i32>,
    /// fallen back, waiting for them to come to her till this tic
    wait_until: i64,
    /// which polys are in a pocket of the ground she knows (`pockets`), as far as asked: for
    /// that many polys known
    pocketed: std::cell::RefCell<(usize, std::collections::BTreeMap<usize, bool>)>,
    /// the things lying about that she's seen (items, and whatever else isn't a monster or a
    /// missile), by what and where (`what_where`): seen when the floor they're on was in her view with them on it
    things_seen: BTreeSet<(&'static str, i32, i32)>,
    /// a decision model's say, if a run's asked for one (`crate::advisor`)
    advisor: Option<Advisor>,
    /// the tuning she plays by with no stance over it
    base_tune: Rc<Tuning>,
    /// the run's own numbers, under any policy (`set_policy`)
    run_tune: Rc<Tuning>,
    /// the stance she's in by the advisor's word ("" none), and till when
    stance: (String, i64),
    /// the advisor's target holds till then
    target_until: i64,
    /// when a target and a stance were last asked
    asked: (i64, i64),
    /// retreats she isn't to make (`bar`)
    barred_retreats: Vec<RetreatKind>,
    /// places she's stood lately with nothing shooting at her and nothing hurting her, oldest
    /// first: the way back out of an ambush
    trail: Vec<Point>,
    /// the ground in sight of hitscan shooters she can't take on, as last worked out: when, and
    /// from where they were
    exposure: Option<(i64, Vec<Point>)>,
    /// when holding at the edge of cover was last logged
    edge_log: i64,
    /// the Icon of Sin's business, on a level with a boss brain (`boss`)
    boss: boss::Boss,
}

impl Player {
    pub fn new(nav: Rc<Navigator>, knowledge: Knowledge, tune: Rc<Tuning>) -> Player {
        let boss = boss::Boss::new(&nav);
        Player {
            explorer: Explorer::new(Rc::clone(&nav), knowledge),
            fight: Fight {
                combat: Combat::new(nav, Rc::clone(&tune)),
                going_on: false,
                tune: Rc::clone(&tune),
                pending_log: Vec::new(),
                offered: BTreeSet::new(),
                present: BTreeSet::new(),
                hunted_spots: BTreeSet::new(),
                completion: None,
                deaths: Vec::new(),
                rethink: false,
                gun_out_of_reach: false,
                toward_gun: BTreeSet::new(),
            },
            retreat: None,
            bait_tic: -1_000_000_000,
            barrel_run_again: 0,
            barrel_runs: Vec::new(),
            retreat_tic: -1_000_000_000,
            away_since: None,
            cover_runs: (0, 0, (0, Known::default())),
            cover_off: (0, 0),
            back_runs: (0, 0, (0, Known::default())),
            back_off: (0, 0),
            tried_cover: false,
            walled_tic: -1_000_000_000,
            shooters_tic: -1_000_000_000,
            last_pos: (0.0, 0.0),
            plight: Plight::default(),
            legs: Legs::Route,
            left_route_at: None,
            pushing_still: 0,
            fallen_for: BTreeSet::new(),
            wait_until: 0,
            pocketed: Default::default(),
            things_seen: BTreeSet::new(),
            advisor: None,
            base_tune: Rc::clone(&tune),
            run_tune: Rc::clone(&tune),
            stance: (String::new(), 0),
            target_until: 0,
            asked: (i64::MIN / 2, i64::MIN / 2),
            barred_retreats: Vec::new(),
            trail: Vec::new(),
            exposure: None,
            edge_log: i64::MIN / 2,
            boss,
        }
    }

    /// Every item that counts as well (the exit held back till nothing's left: --complete);
    /// `restarts` levels again she may ask for, for ones out of reach.
    /// Something that lies where it is (an item, a lamp, a column): not a monster or a missile.
    fn lies_about(t: &Thing) -> bool {
        t.category != "Monster"
            && t.category != "Projectile"
            && monster(t.name).is_none()
            && crate::bestiary::missile(t.name).is_none()
    }

    /// A thing lying about as she remembers it: what it is and where. Not the game's number for
    /// it: the level begun again numbers what's dropped and fired afresh, and a level looked
    /// round with no monsters in it numbers everything otherwise.
    fn what_where(t: &Thing) -> (&'static str, i32, i32) {
        (t.name, t.x.round() as i32, t.y.round() as i32)
    }

    /// The things lying about that have come into her view: on a floor in view this tic (her
    /// eyes': the explorer's, `Knowledge::seen_tic`). Once seen, a thing is known to be there
    /// till she sees it isn't. What a monster drops dying round a corner she finds when she
    /// looks.
    fn notice_things(&mut self, obs: &Observation) {
        let (k, nav) = (&self.explorer.knowledge, &self.explorer.nav);
        for t in obs.things.iter().flatten().filter(|t| Self::lies_about(t)) {
            if self.things_seen.contains(&Self::what_where(t)) {
                continue;
            }
            let in_view = nav.poly_at((t.x, t.y)).is_some_and(|p| {
                k.seen_tic
                    .get(&nav.sector_of_poly(p))
                    .is_some_and(|&tic| tic == obs.tic)
            });
            if in_view {
                self.things_seen.insert(Self::what_where(t));
            }
        }
    }

    /// The game as she perceives it: the observation with what she couldn't know taken out
    /// (docs/design/realism.md). What decides anything is given this, not the game's own. So far: only
    /// the barrels she's seen (where they are now: pushed about, they're the ones she saw), the
    /// things lying about that she's seen (`notice_things`), the monsters in her sight and the
    /// missiles she's noticed (the fight's senses, `Combat::observe`).
    fn percept(&self, obs: &Observation) -> Observation {
        let mut seen = obs.clone();
        // A sound as heard: what it is, how loud, which side. Not what made it or where.
        for s in &mut seen.sounds {
            s.from = None;
            s.at = (0.0, 0.0);
        }
        if let Some(things) = &mut seen.things {
            let c = &self.fight.combat;
            // A monster in her sight as her senses have it: its health is her reckoning.
            for t in things.iter_mut() {
                if let Some(v) = c.visible.iter().find(|v| v.id == t.id) {
                    *t = v.clone();
                }
            }
            things.retain(|t| {
                if monster(t.name).is_some() {
                    c.visible.iter().any(|v| v.id == t.id)
                } else if crate::bestiary::missile(t.name).is_some() {
                    c.missiles_seen.contains(&t.id)
                } else {
                    !Self::lies_about(t) || self.things_seen.contains(&Self::what_where(t))
                }
            });
        }
        // Rockets kept for a boss brain: the fight has no launcher.
        if self.rockets_kept()
            && let Some(w) = &mut seen.weapons
        {
            w.retain(|&slot| slot != 5);
        }
        let barrels = &self.fight.combat.barrels_seen;
        seen.barrels = obs.barrels.as_ref().map(|_| barrels.clone());
        seen.barrels_lit
            .retain(|&l| barrels.iter().any(|&b| dist(b, l) < 1.0));
        seen
    }

    /// The game goes on to another level after this one (or doesn't): what she'd take with her
    /// is had before the exit.
    pub fn set_going_on(&mut self, on: bool) {
        self.fight.going_on = on;
    }

    pub fn going_on(&self) -> bool {
        self.fight.going_on
    }

    /// What she'd take with her into the next level, of what she's seen lying about and could
    /// get to (going on: `Objective::stock`), by name, nearest first; empty if she's not going on.
    pub fn stock_left(&mut self, obs: &Observation) -> Vec<String> {
        if !self.fight.going_on {
            return Vec::new();
        }
        let seen = self.percept(obs);
        let pos = obs.pos();
        let mut out: Vec<(f64, String)> = self
            .explorer
            .ranked(&seen, false, &mut self.fight)
            .into_iter()
            .filter(|o| o.stock)
            .filter_map(|o| {
                let t = seen
                    .things
                    .iter()
                    .flatten()
                    .find(|t| Some(t.id) == o.thing)?;
                Some((dist((t.x, t.y), pos), t.name.to_string()))
            })
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.into_iter().map(|(_, n)| n).collect()
    }

    /// Her standing numbers (whoever directs her: her policy, `over` the run's own tuning), from
    /// now on; a stance taken for a while ends with it.
    pub fn set_policy(&mut self, over: &serde_json::Value) -> Result<(), String> {
        let t = Rc::new(self.run_tune.with(over)?);
        self.stance.0.clear();
        self.set_tuning_only(t);
        Ok(())
    }

    fn set_tuning_only(&mut self, tune: Rc<Tuning>) {
        self.base_tune = Rc::clone(&tune);
        self.fight.combat.tune = Rc::clone(&tune);
        self.fight.tune = tune;
    }

    /// Other numbers to judge by, from now on (a decision model's choice of stance).
    pub fn set_tuning(&mut self, tune: Rc<Tuning>) {
        self.run_tune = Rc::clone(&tune);
        self.base_tune = Rc::clone(&tune);
        self.fight.combat.tune = Rc::clone(&tune);
        self.fight.tune = tune;
    }

    /// Decisions she isn't to make (a choice forced: an experiment's, a decision model's): the
    /// fight's moves and the retreats named, by their names (`Move`'s and `RetreatKind`'s, as
    /// the journal gives them), commas between.
    pub fn bar(&mut self, names: &str) -> Result<(), String> {
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            if let Some(m) = Move::named(name) {
                self.fight.combat.barred.push(m);
            } else if let Some(k) = RetreatKind::ALL
                .into_iter()
                .find(|k| format!("{k:?}") == name)
            {
                self.barred_retreats.push(k);
            } else {
                let moves: Vec<String> = Move::ALL.iter().map(|m| format!("{m:?}")).collect();
                let retreats: Vec<String> =
                    RetreatKind::ALL.iter().map(|k| format!("{k:?}")).collect();
                return Err(format!(
                    "--bar: no move or retreat called {name}; moves: {}; retreats: {}",
                    moves.join(", "),
                    retreats.join(", ")
                ));
            }
        }
        Ok(())
    }

    /// A retreat to cover begun on someone's say (whoever directs her), as her own is: out of
    /// the sight of what's shooting at her, by a way she knows. Err: nowhere to go.
    /// Somewhere out of their sight she knows a way to (what a retreat to cover would take)?
    pub fn has_cover(&mut self, obs: &Observation) -> bool {
        self.find_cover(obs).is_some()
    }

    /// Falling back (a retreat under way).
    pub fn falling_back(&self) -> bool {
        self.retreat.is_some()
    }

    pub fn order_retreat(&mut self, obs: &Observation) -> Result<(), String> {
        let route = self
            .find_cover(obs)
            .ok_or("nowhere out of their sight that you know a way to")?;
        self.retreat = Some(Retreat {
            kind: RetreatKind::Cover,
            route,
        });
        self.retreat_tic = obs.tic;
        self.tried_cover = true;
        self.log(format!("tic {}: to cover, as ordered", obs.tic));
        Ok(())
    }

    /// A stance taken: its numbers over her tuning (any groups and fields, as a tuning file)
    /// till it's given up (`steady`). Whoever directs her says when.
    pub fn take_stance(&mut self, name: &str, over: &serde_json::Value) -> Result<(), String> {
        let t = Rc::new(self.base_tune.with(over)?);
        self.fight.combat.tune = Rc::clone(&t);
        self.fight.tune = t;
        self.stance = (name.to_string(), i64::MAX);
        Ok(())
    }

    /// Her own tuning again.
    pub fn steady(&mut self) {
        self.stance.0.clear();
        self.fight.combat.tune = Rc::clone(&self.base_tune);
        self.fight.tune = Rc::clone(&self.base_tune);
    }

    /// Her target from now on, while it's in sight and after her, whatever the rule would pick
    /// (None: the rule's again).
    pub fn force_target(&mut self, id: Option<i32>) {
        self.fight.combat.forced_target = id;
    }

    /// Keep the journal of decisions (`crate::decisions`).
    pub fn keep_journal(&mut self) {
        self.explorer.journal.on = true;
        self.fight.combat.journal.on = true;
    }

    /// The fight's decisions into the journal, and how things stood put to every record that
    /// hasn't it yet: called before she next looks (the assessment still that tic's), and at
    /// the end.
    pub fn file_decisions(&mut self) {
        if !self.explorer.journal.on {
            return;
        }
        self.explorer
            .journal
            .records
            .append(&mut self.fight.combat.journal.records);
        let state = self.fight.combat.now.summary();
        for r in self
            .explorer
            .journal
            .records
            .iter_mut()
            .filter(|r| r.state.is_none())
        {
            r.state = Some(state.clone());
        }
    }

    pub fn complete(&mut self, restarts: i32) {
        self.fight.completion = Some(Completionist::new(&mut self.explorer, restarts));
    }

    /// For measuring a run: who had her legs last tic.
    pub fn legs(&self) -> Legs {
        self.legs
    }

    /// For measuring a run: the fight's counts.
    pub fn combat_mut(&mut self) -> &mut Combat {
        &mut self.fight.combat
    }

    pub fn combat(&self) -> &Combat {
        &self.fight.combat
    }

    /// Monsters she's seen that are still about: (id, kind, where last seen, when, awake).
    pub fn known_monsters(&self) -> Vec<(i32, &'static str, crate::geom::Point, i64, bool)> {
        self.fight
            .combat
            .remaining()
            .into_iter()
            .map(|s| (s.id, s.name, s.pos(), s.tic, s.awake))
            .collect()
    }

    /// Her options now, best first, the fight's own wants in (supplies, barrels).
    pub fn ranked(
        &mut self,
        obs: &Observation,
        risky: bool,
    ) -> Vec<crate::knowledge::explorer::Objective> {
        self.explorer.ranked(obs, risky, &mut self.fight)
    }

    /// The way on from `pos`, as points: the rest of the route's step she's on, else straight to
    /// what the explorer has her going for. Empty: nowhere. (The ultra instinct's: where she'd
    /// be going is a term in what it weighs.)
    pub fn way_on(&self, pos: crate::geom::Point) -> Vec<crate::geom::Point> {
        // (a retreat's way first: where she's going just now)
        let follower = self
            .retreat
            .as_ref()
            .map(|r| &r.route)
            .or(self.explorer.follower.as_ref());
        let pts: Vec<crate::geom::Point> = follower
            .map_or(Vec::new(), |f| f.planned_path(Some(pos)))
            .into_iter()
            .map_while(|p| p)
            .collect();
        if pts.len() >= 2 {
            return pts;
        }
        self.explorer
            .current
            .as_ref()
            .map_or(Vec::new(), |c| vec![pos, c.goal])
    }

    /// The level begun again with no death to note (a look round it first, then the level; her
    /// order, or the console's). All that's a life's is set back, the Icon's fight with it: left
    /// out, a level begun again by an order in the middle of that fight kept its stage and its
    /// route, riding a platform that was no longer under her (the review of 18 November).
    pub fn start_over(&mut self) {
        self.boss_restart();
        self.forget_dropped();
        self.explorer.restart();
        self.fight.combat.restart();
        self.retreat = None;
        self.fight.combat.baiting = false;
        self.trail.clear();
        self.fallen_for.clear();
        self.fight.hunted_spots.clear();
    }

    /// The level known beforehand: its monsters, where they stand at the start.
    pub fn know_monsters(&mut self, obs: &Observation) {
        self.fight.combat.know_monsters(obs);
    }

    /// Dead, and the level again: what killed her noted, then as any level begun again
    /// (`start_over`).
    pub fn restart(&mut self) {
        // Dead with something on: noted, so as not to walk into it again the same way. A level
        // begun again for what's left has nothing on.
        if let Some(cur) = &self.explorer.current {
            let d = Death {
                at: self.last_pos,
                kind: cur.kind,
                goal: cur.goal,
                label: cur.label.clone(),
                why: self.plight.why(self.fight.tune.deaths.blast_hit),
            };
            self.explorer.log.push(format!(
                "  [fight] died at ({:.0}, {:.0}) going for: {} ({})",
                d.at.0,
                d.at.1,
                d.label,
                d.why.label()
            ));
            self.fight.deaths.push(d);
        }
        self.start_over();
    }

    /// The level begun again: of the things she remembers lying about, only what lies there at
    /// its start (what the map puts there). What a monster dropped dying isn't there now.
    fn forget_dropped(&mut self) {
        let map = &self.explorer.nav.map;
        self.things_seen.retain(|&(_, x, y)| {
            map.things
                .iter()
                .any(|t| t.x.round() as i32 == x && t.y.round() as i32 == y)
        });
    }

    /// What she does this tic: whoever decides it (`decide`), and then her hand. One hand turns
    /// her, whoever is asking, no faster than a hand turns and with no jerk from one tic to the
    /// next: the fight's aim, a route's turn and a look each have a limit of their own, and
    /// taking turns they would make a jitter none of them makes alone. She walks the way that
    /// was meant, whatever she ends up facing.
    pub fn act(&mut self, obs: &Observation, last: Option<&Controls>) -> Controls {
        let mut c = self.decide(obs, last);
        let (most, accel) = (
            self.fight.tune.aim.hand_turn,
            self.fight.tune.aim.hand_accel,
        );
        if accel > 0.0 {
            let was = last.map_or(0.0, |l| l.turn);
            let turn = c.turn.clamp(-most, most).clamp(was - accel, was + accel);
            // The legs' way in the world is kept: forward and strafe are by where she faces after the
            // turn, which is now another way.
            let by = (c.turn - turn).to_radians();
            let (f, s) = (c.forward, c.strafe);
            c.forward = f * by.cos() - s * by.sin();
            c.strafe = f * by.sin() + s * by.cos();
            c.turn = turn;
        }
        c
    }

    fn decide(&mut self, obs: &Observation, last: Option<&Controls>) -> Controls {
        self.file_decisions();
        self.last_pos = obs.pos();
        self.fight.present = obs.things.iter().flatten().map(|t| t.id).collect();
        // Her senses take the game as it is; everything after goes by what they made of it.
        self.fight.combat.observe(obs);
        // Pushing and not moving, where the map has room: a bump, something to look at.
        let pushed = last.filter(|c| c.forward.hypot(c.strafe) > 10.0);
        let still = obs.velocity.is_some_and(|v| v.0.hypot(v.1) < 1.0);
        self.pushing_still = if pushed.is_some() && still {
            self.pushing_still + 1
        } else {
            0
        };
        if let Some(c) = pushed.filter(|_| self.pushing_still == self.fight.tune.steer.stuck_tics) {
            self.fight
                .combat
                .bumped(obs, obs.angle + (-c.strafe).atan2(c.forward).to_degrees());
        }
        self.explorer.sense(obs, last);
        self.notice_things(obs);
        let seen = self.percept(obs);
        let obs = &seen;
        self.consult(obs);
        self.plight = self.plight_now(obs);
        self.keep_trail(obs);
        for line in std::mem::take(&mut self.fight.combat.log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        self.keep_out_of_sight(obs);
        // A boss brain awake and rockets in hand: that's all there is to do, and it's done its
        // own way.
        let turn = match self.boss(obs) {
            Some(boss::Turn::Hers(c)) => {
                self.explorer.hold(obs, last);
                return c;
            }
            t => t,
        };
        let Some(turn) = turn else {
            return self.decide_on(obs, last, None);
        };
        // Its route stands in for the exploring's while the fight has its say.
        self.explorer.hold(obs, last);
        std::mem::swap(&mut self.explorer.follower, &mut self.boss.route);
        let c = self.decide_on(obs, last, Some(turn));
        std::mem::swap(&mut self.explorer.follower, &mut self.boss.route);
        c
    }

    /// The fight and the route between them. `icon`: the Icon of Sin's business has the route
    /// (its own follower in the exploring's place, or stood waiting): it keeps her legs, the
    /// fight shooting over it, and nothing's chosen by the exploring.
    fn decide_on(
        &mut self,
        obs: &Observation,
        last: Option<&Controls>,
        icon: Option<boss::Turn>,
    ) -> Controls {
        // In a jump, or crossing a crusher it has timed, the route keeps the legs and the fight
        // only shoots.
        let free = matches!(icon, Some(boss::Turn::Free(_)));
        let stay = matches!(icon, Some(boss::Turn::Stay(_)));
        let busy = (icon.is_some() && !free)
            || self
                .explorer
                .follower
                .as_ref()
                .is_some_and(|f| f.jumping() || f.crossing_crusher());
        let (errand, toward) = if busy {
            (String::new(), None)
        } else {
            self.fight
                .errand(&self.explorer, obs)
                .map_or((String::new(), None), |(e, at)| (e, Some(at)))
        };
        // Falling back, the retreat's way is the way she's going: a dodge that's as clear along it
        // takes it, not the errand's.
        let back = self.retreat.as_ref().and_then(|r| {
            r.route
                .planned_path(Some(obs.pos()))
                .into_iter()
                .flatten()
                .find(|&p| dist(p, obs.pos()) > self.fight.tune.retreat.ahead)
        });
        self.fight.combat.going_to = if busy { toward } else { back.or(toward) };
        self.fight.combat.route_under_way = self
            .explorer
            .follower
            .as_ref()
            .is_some_and(|f| !f.done && !f.failed);
        self.fight.combat.jump_under_way = self.retreat.is_none()
            && self
                .explorer
                .follower
                .as_ref()
                .is_some_and(RouteFollower::in_jump);
        // (the way the explorer's route goes on, for a run along it: none while a retreat has her)
        self.fight.combat.route_ahead = match (&self.retreat, &self.explorer.follower) {
            (None, Some(f)) => f
                .planned_path(Some(obs.pos()))
                .into_iter()
                .map_while(|p| p)
                .collect(),
            _ => Vec::new(),
        };
        // What she knows of floors gone to something that hurts, for where the fight takes her.
        self.fight.combat.hazards = self.explorer.knowledge.hazards(obs.tic);
        // And the crushers she knows of, for where they could have her if they start again.
        self.fight.combat.movers.crushers = self.explorer.knowledge.crusher_sectors();
        let d = self.fight.combat.decide(obs, busy, &errand);
        // (the Icon of Sin's business: no looking round at what she hears, the brain's cries
        // among it)
        let d = d.filter(|d| !(busy && icon.is_some() && d.kind == Move::Looking));
        let retreating = !busy && !free && self.retreating(obs);
        let legs = arbiter::arbitrate(
            d.as_ref(),
            busy,
            retreating,
            self.fight.combat.dodge_clear(),
        );
        // The Icon of Sin's business is a race: on its route her legs are the fight's only to
        // get out of the way of what's about to hit her.
        let legs = match d.as_ref() {
            Some(d) if busy && icon.is_some() && (d.kind != Move::Dodging || stay) => Legs::Route,
            _ => legs,
        };
        // Moved a long way by the fight or a retreat: what she was going for is thought again as
        // the route takes her legs back, since something else may be nearer by now.
        if legs == Legs::Route {
            let moved = self
                .left_route_at
                .take()
                .map_or(0.0, |at| dist(at, obs.pos()));
            let far = self.fight.tune.errands.rethink_moved;
            if far > 0.0 && moved > far {
                self.fight.rethink = true;
                self.fight.combat.count("objectives thought again", 1.0);
            }
        } else if self.left_route_at.is_none() {
            self.left_route_at = Some(obs.pos());
        }
        if legs != self.legs {
            self.legs = legs;
            self.fight.combat.count("changes of legs", 1.0);
            self.explorer
                .journal
                .note(|| Record::new(obs.tic, "legs", format!("{legs:?}")));
        }
        if retreating {
            self.explorer.hold(obs, last);
            let mut c = self
                .retreat
                .as_mut()
                .map_or_else(Controls::default, |r| r.route.act(obs));
            // Out of the barrels: her legs and her eyes are the run's, whatever the fight says.
            if self
                .retreat
                .as_ref()
                .is_some_and(|r| r.kind == RetreatKind::OutOfBarrels)
            {
                return c;
            }
            if self.hold_at_edge(obs, &c) {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            }
            return match d {
                // A missile about to hit still comes first, when there's a way clear of it.
                Some(d) if d.kind == Move::Dodging && legs == Legs::Fight => {
                    self.fight.combat.controls(obs, &d, None, true)
                }
                // The retreat's legs with the fight aiming over them; or the fight's move, with the
                // retreat's way where its steering leans.
                Some(d) => {
                    let pressing = self
                        .retreat
                        .as_ref()
                        .is_some_and(|r| r.route.pressing(obs.tic));
                    Self::press_turn(
                        self.fight
                            .combat
                            .controls(obs, &d, Some(&c), legs == Legs::Fight),
                        &c,
                        pressing,
                    )
                }
                None => c,
            };
        }
        if let Some(d) = d.as_ref().filter(|_| legs == Legs::Fight) {
            self.explorer.hold(obs, last);
            let mut c = self.fight.combat.controls(obs, d, None, true);
            // The fight's own way is held at the edge of a sniper's sight too, unless the move is out
            // of the way of something: keeping clear can take her into its sight as surely as a route.
            if !d.kind.escaping() && self.hold_at_edge(obs, &c) {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            }
            return c;
        }
        // Fallen back, none of them in sight yet: she waits where she is, facing where the nearest
        // was, and doesn't walk back in to them.
        if d.is_none() && obs.tic < self.wait_until && self.fight.combat.now.against_her.is_empty()
        {
            let pos = obs.pos();
            let near = self
                .fight
                .combat
                .woken_ahead()
                .into_iter()
                .min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos)));
            if let Some(t) = near {
                self.explorer.hold(obs, last);
                let most = self.fight.tune.aim.hand_turn;
                let turn = crate::domain::norm(obs.angle - crate::domain::heading(pos, (t.x, t.y)))
                    .clamp(-most, most);
                let (forward, strafe) = crate::motor::brake(obs, obs.angle - turn);
                return Controls {
                    forward,
                    strafe,
                    turn,
                    ..Controls::default()
                };
            }
        }
        // (a monster about that shoots, as far as she knows: no barrel in the way shot from
        // beside it)
        let shooters = !self.fight.combat.now.shooters.is_empty();
        if let Some(f) = self.explorer.follower.as_mut() {
            f.shooters_about = shooters;
        }
        let mut c = match icon {
            Some(boss::Turn::Wait(c) | boss::Turn::Stay(c)) => c,
            // (time to spare: its route if it has one on, as the fight leaves her legs)
            Some(boss::Turn::Free(c))
                if !self
                    .explorer
                    .follower
                    .as_ref()
                    .is_some_and(|f| !f.done && !f.failed) =>
            {
                c
            }
            Some(_) => self
                .explorer
                .follower
                .as_mut()
                .map_or_else(Controls::default, |f| {
                    // (a race: at full pace whichever way the fight has her faced)
                    f.motion.any_way = true;
                    f.act(obs)
                }),
            None => self.explorer.act(obs, last, &mut self.fight),
        };
        for line in std::mem::take(&mut self.fight.pending_log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        let shooting = self
            .explorer
            .follower
            .as_ref()
            .is_some_and(RouteFollower::shooting);
        if d.is_none() {
            self.fight.combat.route_walks(obs, &c);
        }
        if d.is_none() && c.weapon == 0 && !shooting {
            // No fight: the gun for the next one is brought up, the fight's call.
            if let Some((slot, exact)) = self.fight.combat.ready_gun(obs) {
                c.weapon = slot;
                c.exact = exact;
                self.fight.combat.readied(obs);
            }
        }
        if d.is_none() && self.hold_at_edge(obs, &c) {
            (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
        }
        // The route stood to shoot a barrel, and the fight's move no more than a shot as the route
        // goes on: the gun's the route's too. The shot is how the route goes on; the fight aiming
        // elsewhere, she'd stand there for as long as the barrel's given.
        let stood = self
            .explorer
            .follower
            .as_ref()
            .is_some_and(|f| f.stood_to_shoot(obs.tic));
        // The same lining up a run-jump: square on to the takeoff is the jump, and aimed
        // elsewhere she never is. A second or two with no shot, then she's off (and the fight
        // shoots through the run as before); what she has to get out of the way of still takes
        // her.
        let lining = self
            .explorer
            .follower
            .as_ref()
            .is_some_and(RouteFollower::lining_up);
        // And through the sprint and the jump itself, whatever the move (one that can't wait has
        // her legs already, the arbiter's call): the sprint pushes forward the way she faces, so
        // turned to aim at an imp across the room she ran at the imp, not along the jump, and
        // came down short or never left. Under a second with no shot.
        let sprinting = self
            .explorer
            .follower
            .as_ref()
            .is_some_and(RouteFollower::sprinting);
        match d {
            // (lining up: whichever move it is that leaves her legs to the route, shooting on the
            // way, or on the way to an errand, getting past, out of reach)
            Some(d)
                if sprinting
                    || (stood && d.kind == Move::ShootingOnTheWay)
                    || (lining && d.kind.route_goes_on() && !d.urgent) =>
            {
                self.fight.combat.route_walks(obs, &c);
                c
            }
            Some(d) => {
                let pressing = self
                    .explorer
                    .follower
                    .as_ref()
                    .is_some_and(|f| f.pressing(obs.tic));
                let mut c = Self::press_turn(
                    self.fight.combat.controls(obs, &d, Some(&c), false),
                    &c,
                    pressing,
                );
                if self.hold_at_edge(obs, &c) {
                    (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                }
                c
            }
            None => c,
        }
    }

    /// The fight's controls over a route's (`route`): turned the route's way while the route is
    /// waiting to press a door or a switch and nothing is fired this tic. Use goes where she
    /// faces, so turned to aim at something she isn't shooting, the press would never come.
    fn press_turn(mut c: Controls, route: &Controls, pressing: bool) -> Controls {
        if pressing && !c.attack {
            c.turn = route.turn;
        }
        c
    }

    fn plight_now(&mut self, obs: &Observation) -> Plight {
        let nav = &self.explorer.nav;
        let pos = obs.pos();
        let hurting_floor = nav
            .poly_at(pos)
            .is_some_and(|p| self.explorer.knowledge.damaging(nav.sector_of_poly(p)));
        let c = &self.fight.combat;
        let (last_hit, close) = match c.last_hit() {
            Some((tic, amount)) if obs.tic - tic <= 1 => (
                amount,
                c.visible.iter().any(|t| dist((t.x, t.y), pos) < 128.0),
            ),
            _ => (self.plight.last_hit, self.plight.close),
        };
        Plight {
            armed: c.armed(obs),
            hurting_floor,
            surrounded: c.now.surrounded,
            outmatched: c.now.outmatched,
            last_hit,
            close,
        }
    }

    // --- backing off ------------------------------------------------------------------------

    fn log(&mut self, s: String) {
        self.explorer.log.push(format!("  [fight] {s}"));
    }
}

impl Fight {
    /// The way to p (as the crow flies) isn't past anything about that she can't take on: a run
    /// for the invulnerability doesn't go straight through what it's for. Till it has moved off
    /// the line, she keeps clear.
    fn clear_way_to(&self, obs: &Observation, p: Point) -> bool {
        let pos = obs.pos();
        self.combat
            .remaining()
            .iter()
            .filter(|s| {
                s.awake
                    && self
                        .combat
                        .too_much(obs, &[s.name], self.tune.engage.leave_be)
            })
            .all(|s| {
                segment_point_distance(pos, p, s.pos())
                    > self.tune.errands.way_clear + monster(s.name).map_or(20.0, |m| m.radius)
            })
    }

    /// The way to the ammo or gun at p, armed with it (`obs`): clear as above, and all of what's
    /// awake by it together not more than it and what she has would kill. Each alone may be
    /// nothing and the lot too much.
    fn clear_way_armed(&self, obs: &Observation, p: Point) -> bool {
        let pos = obs.pos();
        let by: Vec<&str> = self
            .combat
            .remaining()
            .iter()
            .filter(|s| {
                s.awake
                    && segment_point_distance(pos, p, s.pos())
                        <= self.tune.errands.way_clear + monster(s.name).map_or(20.0, |m| m.radius)
            })
            .map(|s| s.name)
            .collect();
        self.clear_way_to(obs, p) && !self.combat.ammo_short_for_each(obs, &by)
    }

    /// Invulnerability worth taking now: a fight that needs it, or her policy taking it as it
    /// comes (`errands.invulnerability_now`).
    fn wants_invulnerability(&self, obs: &Observation) -> bool {
        self.tune.errands.invulnerability_now || self.needs_invulnerability(obs)
    }

    /// Something about she can't take on (awake, known), or what's about her too much together
    /// (outmatched, or round her on several sides): invulnerability's for that.
    fn needs_invulnerability(&self, obs: &Observation) -> bool {
        // One of them too much for her, or all that's about her together: a ring of weak ones is
        // each nothing and together what the sphere is for.
        self.combat.now.outmatched
            || self.combat.now.surrounded
            || self.combat.remaining().iter().any(|s| {
                s.awake
                    && self
                        .combat
                        .too_much(obs, &[s.name], self.tune.engage.leave_be)
            })
    }

    /// Barrels she's seen, near, with nothing in sight and the ammo for it: each to be shot from
    /// out of its chain's blasts (the follower's job), before a shot sets it off by her, hers
    /// at something beside it or a monster's at her.
    fn barrels(&self, x: &Explorer, obs: &Observation, pos: Point) -> Vec<Objective> {
        if !self.combat.visible.is_empty() {
            return Vec::new(); // (in a fight, the fight's: shot by what they'd take with them)
        }
        // Nor with anything about her that she knows of, in sight or seen awake a while ago:
        // clearing barrels is for when it's safe, not something to stand to between two
        // rushes of the same fight. (MAP25: begun on the tic the spectres went out of sight,
        // given up four seconds later with them back on her, begun again for the barrel beside
        // it; nine to eighteen times, and she died in that room.)
        if !self.combat.now.about.is_empty() {
            return Vec::new();
        }
        if !obs.barrels_lit.is_empty() {
            return Vec::new(); // (nor while any are going off: the chain's not done)
        }
        let ammo = |slot: i32| {
            obs.ammo
                .iter()
                .flatten()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n)
        };
        let held = obs.weapons.clone().unwrap_or_default();
        // The barrel rule: the shotgun with enough shells, else single pistol taps.
        if !(held.contains(&2) && ammo(2) >= self.tune.barrels.bullets)
            && !(held.contains(&3) && ammo(3) >= self.tune.barrels.shells)
        {
            return Vec::new();
        }
        let nav = &x.nav;
        let mut out = Vec::new();
        for &b in &self.combat.barrels_seen {
            if dist(b, pos) > self.tune.barrels.clear_within {
                continue;
            }
            // From where she stands. The step is the follower's from its start: it looks over
            // everywhere within the gun's own aim of the barrel (1,024, as far as the game aims
            // a shot for her) for the nearest place to her the shot reaches from, on a floor
            // that doesn't hurt and out of the blasts, and a standing shot is as good from far
            // as from near. All it wants of the errand is a way that isn't turned down. (It was
            // to the place she could get to nearest the barrel: by MAP25's pillar of five in
            // the blood that's down in the blood, a drop she won't take, and they were never
            // shot, with a ledge in plain view of them across the room.)
            // Not from a floor that hurts: off it first.
            if nav
                .poly_at(pos)
                .is_none_or(|here| x.knowledge.damaging(nav.sector_of_poly(here)))
            {
                continue;
            }
            let spot = pos;
            let mut o = Objective::new(
                Kind::Barrel,
                spot,
                format!("clear the barrel at ({:.0}, {:.0})", b.0, b.1),
            );
            o.look_at = Some(b);
            // One objective a barrel, by where it is: given up on as any other is.
            o.reveals =
                std::iter::once(((b.0.round() as i64) << 32) | (b.1.round() as i64 & 0xffff_ffff))
                    .collect();
            out.push(o);
        }
        out
    }
}

impl Fight {}

impl Fight {
    /// How many times she's died going for obj (the same kind of thing at the same place).
    fn died_for(&self, obj: &Objective) -> usize {
        self.deaths
            .iter()
            .filter(|d| d.why != Cause::Unarmed)
            // By place: a level begun again spawns everything afresh, with new ids.
            .filter(|d| d.kind == obj.kind && dist(d.goal, obj.goal) < self.tune.deaths.same_goal)
            .count()
    }

    /// Would going for item obj likely cost more than it's worth: the damage from what's awake
    /// and can see the way there and back, for as long as that takes, against what it gives
    /// (health she'd get, armour; ammo or a gun when she's short)? And, with nothing to shoot
    /// with, anything but ammo or a gun while something is about.
    fn not_worth_it(&mut self, obs: &Observation, obj: &Objective) -> bool {
        let Some(t) = obj
            .thing
            .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
            .cloned()
        else {
            return false;
        };
        let pos = obs.pos();
        let health = self.combat.now.health;
        let short = self.combat.now.short_of_ammo;
        let worth = if let Some((amount, cap)) = health_item(t.name) {
            f64::from(amount).min(f64::from(cap) - health).max(1.0)
        } else if armor_item(t.name).is_some() {
            Combat::pickup_gain(obs, t.name).max(1.0)
        } else if ammo_item(t.name).is_some() {
            if short {
                60.0
            } else {
                15.0
            }
        } else if weapon_item(t.name).is_some() {
            if Self::new_gun(obs, t.name) {
                60.0
            } else {
                15.0
            }
        } else {
            5.0
        };
        let way = dist(pos, obj.goal);
        let exposed = 2.0 * way / RUN_SPEED; // (there and back, seconds)
        let threats: Vec<Seen> = self
            .combat
            .remaining()
            .into_iter()
            .filter(|s| {
                s.awake
                    && monster(s.name).is_some_and(|m| m.attack != Attack::None)
                    && segment_point_distance(pos, obj.goal, s.pos())
                        < self.tune.errands.exposed_near
            })
            .collect();
        // Lying on a floor that hurts: a hit or two to fetch it, set against what it's worth.
        let on_harm = self
            .combat
            .sector_at(obj.goal)
            .is_some_and(|s| self.combat.hazards.get(s).copied().unwrap_or(false));
        let floor = if on_harm {
            self.tune.errands.floor_cost
        } else {
            0.0
        };
        if threats.is_empty() {
            return floor > worth;
        }
        let supplies = ammo_item(t.name).is_some() || weapon_item(t.name).is_some();
        if !self.combat.armed(obs) && !supplies {
            return true;
        }
        let cost: f64 = floor
            + threats
                .iter()
                .map(|s| {
                    monster(s.name).map_or(0.0, |m| m.threat) * self.tune.errands.hits * exposed
                })
                .sum::<f64>();
        cost > worth && cost > self.tune.errands.cost_floor
    }
}

impl Hooks for Fight {
    fn unarmed(&self, obs: &Observation) -> bool {
        !self.combat.armed(obs)
    }

    fn stocking(&self) -> bool {
        self.going_on
    }

    /// Ammo or a gun she's off for because she's short of ammo.
    fn worth_cut_off(&mut self, obs: &Observation, obj: &Objective) -> bool {
        let Some(t) = obj
            .thing
            .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
        else {
            return false;
        };
        obj.urgent
            && (ammo_item(t.name).is_some() || weapon_item(t.name).is_some())
            && self.combat.now.short_of_ammo
    }

    /// Items she could use now; with --complete, every counted item she's seen too.
    fn wanted(
        &mut self,
        x: &Explorer,
        obs: &Observation,
        reach: &BTreeSet<usize>,
        pos: Point,
    ) -> Vec<Objective> {
        let known = x.knowledge.known_polys();
        let gun_sectors: BTreeSet<usize> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| weapon_item(t.name).is_some() && self.better_gun(obs, t.name))
            .filter_map(|t| x.nav.poly_at((t.x, t.y)))
            .filter(|p| known.contains(p) && !reach.contains(p))
            .map(|p| x.nav.sector_of_poly(p))
            .collect();
        self.gun_out_of_reach = !gun_sectors.is_empty();
        let mesh = &x.nav.mesh;
        self.toward_gun = (0..mesh.polys.len())
            .filter(|&p| {
                mesh.polys[p]
                    .as_ref()
                    .is_some_and(|q| gun_sectors.contains(&q.sector))
                    && !known.contains(&p)
            })
            .map(|p| p as i64)
            .collect();
        let mut out = self.supplies(x, obs, reach, pos);
        out.extend(self.barrels(x, obs, pos));
        if let Some(c) = &mut self.completion {
            for o in c.wanted(x, obs, reach, pos) {
                if !out.iter().any(|w| w.thing.is_some() && w.thing == o.thing) {
                    out.push(o);
                }
            }
        }
        out
    }

    /// --complete: rather the level again than out without items she could get from its start.
    fn before_exit(&mut self, x: &Explorer, obs: &Observation) -> bool {
        self.completion
            .as_mut()
            .is_some_and(|c| c.before_exit(x, obs))
    }

    /// Monsters seen that are still about: go where each was last seen.
    fn last_resort(
        &mut self,
        x: &Explorer,
        _obs: &Observation,
        reach: &BTreeSet<usize>,
        _pos: Point,
    ) -> Vec<Objective> {
        let nav = &x.nav;
        let mut out = Vec::new();
        for s in self.combat.remaining() {
            let goal = match nav.poly_at(s.pos()).filter(|p| reach.contains(p)) {
                Some(_) => s.pos(),
                None => {
                    let Some(pi) = reach
                        .iter()
                        .copied()
                        .filter(|&pi| {
                            dist(nav.poly_center(pi), s.pos()) < self.tune.errands.hunt_near
                        })
                        .reduce(|a, b| {
                            if dist(nav.poly_center(b), s.pos()) < dist(nav.poly_center(a), s.pos())
                            {
                                b
                            } else {
                                a
                            }
                        })
                    else {
                        continue;
                    };
                    nav.poly_center(pi)
                }
            };
            let mut o = Objective::new(
                Kind::Hunt,
                x.stand_spot_for(goal, reach),
                format!("hunt {} {}", s.name, s.id),
            );
            // Been there for it once and it wasn't to be had from there: not again.
            if self
                .hunted_spots
                .contains(&(s.id, o.goal.0.round() as i64, o.goal.1.round() as i64))
            {
                continue;
            }
            o.thing = Some(s.id);
            out.push(o);
        }
        out
    }

    fn hunted_in_vain(&mut self, obj: &Objective) {
        if let Some(id) = obj.thing {
            self.hunted_spots
                .insert((id, obj.goal.0.round() as i64, obj.goal.1.round() as i64));
        }
    }

    fn gone(&mut self, obs: &Observation, obj: &Objective) -> bool {
        match obj.kind {
            Kind::Item => obj.thing.is_none_or(|t| !self.present.contains(&t)),
            Kind::Hunt => obj
                .thing
                .is_none_or(|t| !self.combat.remaining().iter().any(|s| s.id == t)),
            Kind::Barrel => obj
                .look_at
                .is_none_or(|b| crate::motor::follower::barrel_now(obs, b).is_none()),
            _ => false,
        }
    }

    /// Monsters she knows of by the objective's spot she couldn't take on with what she has?
    /// (Supplies she badly needs and the exit are worth it anyway.)
    fn dangerous(&mut self, obs: &Observation, obj: &Objective) -> bool {
        if matches!(obj.kind, Kind::Exit | Kind::Key) {
            return false; // (worth it whatever: the way on)
        }
        // Invulnerability is kept for a fight that needs it: not picked up in passing, its thirty
        // seconds spent on nothing. With something about she can't take on, it's what she goes for.
        if obj
            .thing
            .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
            .is_some_and(|t| t.name == "InvulnerabilitySphere")
        {
            return !self.wants_invulnerability(obs);
        }
        if self.combat.now.invulnerable {
            return false; // (nothing can hurt her: the risky ones now)
        }
        // Died going for it: not first again; twice, not at all. Likewise a place she has died at
        // again and again, whatever for.
        let died = self.died_for(obj);
        let here = self
            .deaths
            .iter()
            .filter(|d| {
                d.why != Cause::Unarmed && dist(d.at, obj.goal) < self.tune.deaths.death_zone
            })
            .count();
        if died >= self.tune.deaths.deaths_give_up || here >= self.tune.deaths.death_zone_urgent {
            return true;
        }
        if obj.urgent {
            return false;
        }
        if died > 0 || here >= self.tune.deaths.death_zone_count {
            return true;
        }
        // And a hitscan shooter that would see her there, however far.
        let known = self.combat.remaining();
        let near: Vec<&str> = known
            .iter()
            .filter(|s| {
                dist(s.pos(), obj.goal) < self.tune.errands.danger_near
                    || self.combat.shot_from(obs, s, obj.goal)
            })
            .map(|s| s.name)
            .collect();
        if !near.is_empty() && self.combat.too_much(obs, &near, self.tune.engage.leave_be) {
            return true;
        }
        obj.kind == Kind::Item && self.not_worth_it(obs, obj)
    }

    /// Ammo for the guns she has, with a monster she knows of about that she can't take on: as
    /// if twice as far (unless she's short). More of the same won't change the odds; what might
    /// is somewhere not yet seen, or a door not yet tried (another gun, armor, cover).
    fn weigh(&mut self, obs: &Observation, obj: &Objective) -> f64 {
        // A better gun in sight that she can't get to: a switch, which changes floors and opens
        // doors, is the likeliest way there, so it's pressed before the rest.
        if obj.kind == Kind::Switch && self.gun_out_of_reach {
            return self.tune.errands.switch_for_gun;
        }
        // And a look into the unseen part of the ground it lies on: the likeliest way to it on
        // foot.
        if obj.kind == Kind::Frontier && !obj.reveals.is_disjoint(&self.toward_gun) {
            return self.tune.errands.switch_for_gun;
        }
        let ammo = obj.kind == Kind::Item
            && obj
                .thing
                .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
                .is_some_and(|t| ammo_item(t.name).is_some());
        if !ammo || self.combat.now.short_of_ammo {
            return 1.0;
        }
        let known: Vec<&'static str> = self
            .combat
            .remaining()
            .iter()
            .filter(|s| s.awake)
            .map(|s| s.name)
            .collect();
        if known
            .iter()
            .any(|n| self.combat.too_much(obs, &[n], self.tune.engage.leave_be))
        {
            self.tune.errands.ammo_outmatched
        } else {
            1.0
        }
    }

    /// Something to fight with come into what she knows of the map (a better gun; ammo, when
    /// she's short): go for it now (once for each).
    fn interrupt(&mut self, x: &Explorer, obs: &Observation, obj: &Objective) -> bool {
        if std::mem::take(&mut self.rethink) {
            return true;
        }
        if obj.urgent || obj.kind == Kind::Exit {
            return false;
        }
        let Some(t) = self.errands(x, obs).into_iter().next() else {
            return false;
        };
        self.offered.insert(t.id);
        self.pending_log
            .push(format!("tic {}: spotted a {}", obs.tic, t.name));
        true
    }
}
