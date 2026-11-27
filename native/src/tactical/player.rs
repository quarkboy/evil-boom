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
/// this near what the invulnerability's for, she's there
const ARRIVED: f64 = 96.0;
/// a sphere spent on the same death this often is spent on it no more
const CHARGES: u32 = 2;
/// what she's had in sight this lately (tics) is what was on her when she died
const RECENT_TICS: i64 = 10 * 35;
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
    /// a co-op game (`watch --players`): where the level puts things down (a gun put down
    /// stays there when picked up, and one she has already can't be: `left_lying`)
    coop: Option<BTreeSet<(i32, i32)>>,
    /// a deathmatch she hunts the others in (`watch --deathmatch`, `--hunt`): what she knows of
    /// them, and whether she's geared up to go after them (`tactical/rivals.rs`)
    rivals: Option<super::rivals::Rivals>,
    /// deathmatch 2.0 (`--altdeath`): what the level put down comes back 30 s after it's taken
    /// (`Returns`)
    returns: Option<Returns>,
    /// the spots she's off to see whether something's back at (`checks`), this choosing
    checking: Vec<(Point, (&'static str, i32, i32))>,
    /// the numbers she judges by (the fight's are the same)
    tune: Rc<Tuning>,
    /// what the hooks had to say (logged once the explorer's done)
    pending_log: Vec<String>,
    /// better guns she's turned aside for already
    offered: BTreeSet<i32>,
    /// ...the spheres among them, this life (one left lying in a life before is to be had again)
    offered_spheres: BTreeSet<i32>,
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
    /// a better gun than she has is among what she could go for now: one lying where she can
    /// get to, or carried by something she can (`supplies`)
    gun_to_have: bool,
    /// invulnerable, and what it's to be spent on (where, and what she calls it): gone for
    /// before anything else while it lasts (`sphere_use`, chosen as it's picked up)
    charge: Option<(Point, String)>,
    /// ...chosen for this sphere already (once a pickup)
    charged: bool,
    /// about to do again what she died after doing: the sphere first, from this tic (given up
    /// after `errands.sphere_wait`, and not tried twice a life)
    sphere_first: Option<i64>,
    sphere_tried: bool,
    /// where she's been this life of the places a sphere would be for: done with, this life
    answered: Vec<Point>,
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

/// What she's settled by herself at the level's scale, for whoever directs her to be told
/// (`Player::plans`): these are choices a player makes of the level, not reflexes of a fight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// about to do again what she died after doing (the line she set off, and what came of
    /// it): the invulnerability she knows of fetched first
    SphereFirst { line: usize, by: String },
    /// invulnerable, and off to what it's for: what came of something she set off (`by`), or
    /// with none, where she died going for something
    Charged { by: Option<String> },
    /// only the pistol, and a gun to go for: keys wait
    GunBeforeKey,
}

/// A deadly action's words as the journal has them, "line 12 (Archvile)": what came of it.
fn came_of(what: &str) -> String {
    what.split_once('(')
        .map_or(what, |(_, by)| by.trim_end_matches(')'))
        .to_string()
}

/// A death: where, and what she was going for.
#[derive(Clone, Debug)]
struct Death {
    at: Point,
    kind: Kind,
    goal: Point,
    label: String,
    why: Cause,
    /// what she set off just before the worst of what was on her came into sight (a switch, a
    /// door, a line walked over: `deaths.reveal_secs`): its line, where it is, what it's
    /// called, the sectors it moves. Doing it again is what brings them on again.
    action: Option<(usize, Point, String, Vec<usize>, f64)>,
    /// times she's set off with a sphere for this (`Fight::charge`): tried twice, it isn't
    /// what a sphere mends
    charged: u32,
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

/// The ground she knows, as it was when she knew of `known` polys: its measures, and for the
/// polys asked about, whether each is in a pocket (`Player::in_pocket`).
struct Pocketed {
    known: usize,
    polys: std::collections::BTreeSet<usize>,
    measures: crate::pockets::Measures,
    answers: std::collections::BTreeMap<usize, bool>,
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
    /// ammo gone for out of ammo: the thing, how often, and the rounds she had (`for_ammo`)
    ammo_gone_for: Vec<(i32, u32, i32)>,
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
    pocketed: std::cell::RefCell<Option<Pocketed>>,
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
                coop: None,
                rivals: None,
                returns: None,
                checking: Vec::new(),
                tune: Rc::clone(&tune),
                pending_log: Vec::new(),
                offered: BTreeSet::new(),
                offered_spheres: BTreeSet::new(),
                present: BTreeSet::new(),
                hunted_spots: BTreeSet::new(),
                completion: None,
                deaths: Vec::new(),
                rethink: false,
                gun_out_of_reach: false,
                gun_to_have: false,
                charge: None,
                charged: false,
                sphere_first: None,
                sphere_tried: false,
                answered: Vec::new(),
                toward_gun: BTreeSet::new(),
            },
            retreat: None,
            bait_tic: -1_000_000_000,
            barrel_run_again: 0,
            barrel_runs: Vec::new(),
            retreat_tic: -1_000_000_000,
            away_since: None,
            ammo_gone_for: Vec::new(),
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
        self.notice_returns(obs);
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

    /// Deathmatch 2.0: the things she knew of lying about whose spots are in view this tic: gone
    /// since now if they're not there (or, gone and due back, still not: taken again since);
    /// there, back. (Before `notice_things` takes in what's in view.)
    fn notice_returns(&mut self, obs: &Observation) {
        let Some(r) = self.fight.returns.as_mut() else {
            return;
        };
        let (k, nav) = (&self.explorer.knowledge, &self.explorer.nav);
        let present: BTreeSet<(&'static str, i32, i32)> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| Self::lies_about(t))
            .map(Self::what_where)
            .collect();
        let in_view = |x: i32, y: i32| {
            nav.poly_at((f64::from(x), f64::from(y))).is_some_and(|p| {
                k.seen_tic
                    .get(&nav.sector_of_poly(p))
                    .is_some_and(|&tic| tic == obs.tic)
            })
        };
        for &key in self.things_seen.iter() {
            if !r.comes_back(key) || !in_view(key.1, key.2) {
                continue;
            }
            if present.contains(&key) {
                r.gone.remove(&key);
            } else {
                let due = r
                    .gone
                    .get(&key)
                    .is_some_and(|&t| obs.tic - t >= RETURN_TICS);
                if due || !r.gone.contains_key(&key) {
                    r.gone.insert(key, obs.tic);
                }
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
                    let key = Self::what_where(t);
                    // (deathmatch 2.0: one she's seen gone is gone to her till she sees it back)
                    !Self::lies_about(t)
                        || (self.things_seen.contains(&key)
                            && !self
                                .fight
                                .returns
                                .as_ref()
                                .is_some_and(|r| r.gone.contains_key(&key)))
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
    /// How fast her hand is on the keys (`Tuning::hand`, as the run began with it).
    pub fn hand(&self) -> crate::tactical::tuning::Hand {
        self.base_tune.hand.clone()
    }

    pub fn set_going_on(&mut self, on: bool) {
        self.fight.going_on = on;
    }

    /// A co-op game: the guns the level puts down stay there for the others (`Fight::left_lying`).
    pub fn set_coop(&mut self) {
        let spots = (self.explorer.nav.map.things.iter())
            .map(|m| (m.x.round() as i32, m.y.round() as i32))
            .collect();
        self.fight.coop = Some(spots);
    }

    pub fn going_on(&self) -> bool {
        self.fight.going_on
    }

    /// A deathmatch she hunts the others in: gearing up first, then going after them by what
    /// she's seen and heard (`monsters`: there are monsters in the game too, whose guns and
    /// doors sound as a player's do).
    pub fn set_hunting(&mut self, monsters: bool) {
        self.fight.rivals = Some(super::rivals::Rivals::new(monsters));
    }

    /// Deathmatch 2.0: what the level put down comes back 30 s after it's taken, as every player
    /// knows (but the spheres of invulnerability and invisibility).
    pub fn set_items_return(&mut self) {
        let spots = (self.explorer.nav.map.things.iter())
            .map(|m| (m.x.round() as i32, m.y.round() as i32))
            .collect();
        self.fight.returns = Some(Returns {
            spots,
            gone: std::collections::BTreeMap::new(),
        });
    }

    /// Told to hunt the others (directed: `doom_hunt`), geared up or not; or not (any other order).
    pub fn set_hunt_ordered(&mut self, on: bool) {
        if let Some(r) = self.fight.rivals.as_mut() {
            r.ordered = on;
            r.offered = None;
        }
    }

    /// Geared up to hunt, by her numbers (`tactical/rivals.rs`).
    pub fn geared_up(&self, obs: &Observation) -> bool {
        super::rivals::geared(obs, &self.fight.tune.deathmatch, false)
    }

    /// Whether she hunts the others (`set_hunting`).
    pub fn hunt_state_on(&self) -> bool {
        self.fight.rivals.is_some()
    }

    /// The hunting's state for her status and the console: geared up to hunt or not, and the
    /// best word of a rival there is ("geared up; last of a rival: a shotgun NE 600, 3s ago").
    /// None if she doesn't hunt.
    pub fn hunt_state(&self, obs: &Observation) -> Option<String> {
        let r = self.fight.rivals.as_ref()?;
        let tune = &self.fight.tune.deathmatch;
        let clue = r.best(obs.tic, obs.pos(), tune).map_or(String::new(), |c| {
            format!(
                "; last of a rival: {} {} {:.0}, {:.0}s ago",
                c.what,
                crate::control::directed::compass(crate::domain::heading(obs.pos(), c.at)),
                dist(obs.pos(), c.at),
                (obs.tic - c.tic) as f64 / 35.0
            )
        });
        Some(format!(
            "{}{clue}",
            if r.hunting {
                "geared up"
            } else {
                "not geared up"
            }
        ))
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

    /// What she's settled by herself at the level's scale, as things stand (`Plan`).
    pub fn plans(&self) -> Vec<Plan> {
        let f = &self.fight;
        let mut out = Vec::new();
        if f.sphere_first.is_some()
            && let Some((line, _, what, ..)) = f.sphere_death().and_then(|d| d.action.as_ref())
        {
            out.push(Plan::SphereFirst {
                line: *line,
                by: came_of(what),
            });
        }
        if let Some((_, what)) = f.charge.as_ref().filter(|_| f.combat.now.invulnerable) {
            // (an action's words begin "line": else it's what she was going for)
            let by = what.starts_with("line ").then(|| came_of(what));
            out.push(Plan::Charged { by });
        }
        if f.gun_to_have && f.combat.now.firepower <= f.tune.errands.pistol_only {
            out.push(Plan::GunBeforeKey);
        }
        out
    }

    /// The sphere not fetched first after all (whoever directs her says so): on with what
    /// she was about, and not asked of it again this life.
    pub fn no_sphere_first(&mut self) {
        self.fight.sphere_first = None;
        self.fight.sphere_tried = true;
    }

    /// How many deaths `obj` is left alone for on instinct (0: it isn't): died going for it,
    /// or a place she's died at again and again (`Hooks::dangerous`'s own count).
    pub fn deaths_at(&self, obj: &Objective) -> usize {
        let f = &self.fight;
        let died = f.died_for(obj);
        let here = f
            .deaths
            .iter()
            .filter(|d| d.why != Cause::Unarmed && dist(d.at, obj.goal) < f.tune.deaths.death_zone)
            .count();
        if died > 0 || here >= f.tune.deaths.death_zone_count {
            died.max(here)
        } else {
            0
        }
    }

    /// What came of setting off `line` in a life before, if she died of it (`Death::action`).
    pub fn died_after(&self, line: usize) -> Option<String> {
        self.fight
            .deaths
            .iter()
            .rev()
            .filter_map(|d| d.action.as_ref())
            .find(|a| a.0 == line)
            .map(|a| came_of(&a.2))
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
        (self.fight.sphere_first, self.fight.sphere_tried) = (None, false);
        self.fight.answered.clear();
        self.fight.offered_spheres.clear();
    }

    /// The level known beforehand: its monsters, where they stand at the start.
    pub fn know_monsters(&mut self, obs: &Observation) {
        self.fight.combat.know_monsters(obs);
    }

    /// What she set off just before the worst of what's on her came into sight, if anything
    /// (`Death::action`): of what she's had in sight these last seconds and first saw this
    /// life, the one that matters most (`Monster::threat`), and the last line she fired no
    /// more than `deaths.reveal_secs` before she first saw it.
    fn revealed_by(&self) -> Option<(usize, Point, String, Vec<usize>, f64)> {
        let now = self.fight.combat.now.tic;
        let worst = self
            .fight
            .combat
            .remaining()
            .into_iter()
            .filter(|s| !s.foreknown && now - s.sighted < RECENT_TICS)
            .max_by(|a, b| {
                let threat = |s: &Seen| monster(s.name).map_or(0.0, |m| m.threat);
                threat(a).total_cmp(&threat(b)).then(b.first.cmp(&a.first))
            })?;
        let k = &self.explorer.knowledge;
        let window = (self.fight.tune.deaths.reveal_secs * 35.0) as i64;
        let fired =
            k.fired.iter().rev().find(|f| {
                f.life == k.lives && f.tic <= worst.first && worst.first - f.tic <= window
            })?;
        let nav = &self.explorer.nav;
        let (a, b) = nav.map.line_seg(fired.line);
        let sectors = nav
            .triggers
            .iter()
            .filter(|t| t.linedef() == Some(fired.line))
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .collect();
        Some((
            fired.line,
            ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
            format!("line {} ({})", fired.line, worst.name),
            sectors,
            monster(worst.name).map_or(0.0, |m| m.threat),
        ))
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
                action: self.revealed_by(),
                charged: 0,
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

    /// Dead in a co-op game and born again at her start: what killed her noted, as `restart`
    /// notes it, but the level goes on as it is (no level begun again: `Explorer::reborn`,
    /// `Combat::reborn`).
    pub fn reborn(&mut self) {
        // Dead with something on: noted, so as not to walk into it again the same way. A level
        // begun again for what's left has nothing on.
        if let Some(cur) = &self.explorer.current {
            let d = Death {
                at: self.last_pos,
                kind: cur.kind,
                goal: cur.goal,
                label: cur.label.clone(),
                why: self.plight.why(self.fight.tune.deaths.blast_hit),
                action: self.revealed_by(),
                charged: 0,
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
        self.explorer.reborn();
        self.fight.combat.reborn();
        self.retreat = None;
        self.fight.combat.baiting = false;
        self.trail.clear();
        self.fight.hunted_spots.clear();
        self.fight.answered.clear();
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
        use crate::debug::timed;
        timed("player.sense", || self.explorer.sense(obs, last));
        timed("player.notice_things", || self.notice_things(obs));
        let seen = timed("player.percept", || self.percept(obs));
        let obs = &seen;
        self.sense_rivals(obs);
        timed("player.consult", || self.consult(obs));
        self.plight = timed("player.plight", || self.plight_now(obs));
        timed("player.keep_trail", || self.keep_trail(obs));
        for line in std::mem::take(&mut self.fight.combat.log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        timed("player.keep_out_of_sight", || self.keep_out_of_sight(obs));
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
            return timed("player.decide_on", || self.decide_on(obs, last, None));
        };
        // Its route stands in for the exploring's while the fight has its say.
        self.explorer.hold(obs, last);
        std::mem::swap(&mut self.explorer.follower, &mut self.boss.route);
        let c = self.decide_on(obs, last, Some(turn));
        std::mem::swap(&mut self.explorer.follower, &mut self.boss.route);
        c
    }

    /// A deathmatch: what's new of the others, and whether she's geared up to hunt them now
    /// (a hunter too weak goes back to gearing up).
    fn sense_rivals(&mut self, obs: &Observation) {
        let Some(r) = self.fight.rivals.as_mut() else {
            return;
        };
        let tune = Rc::clone(&self.fight.tune);
        let t = &tune.deathmatch;
        let heard = &self.explorer.knowledge.heard;
        let movers: Vec<_> = heard[r.heard_seen.min(heard.len())..]
            .iter()
            .filter(|h| self.explorer.worth_a_look(h))
            .copied()
            .collect();
        r.heard_seen = heard.len();
        let lines = r.sense(
            obs,
            &self.fight.combat.visible,
            &movers,
            &self.explorer.nav,
            tune.senses.fov,
            t,
        );
        self.explorer.log.extend(lines);
        let was = r.hunting;
        r.hunting = if was {
            !super::rivals::weak(obs, t)
        } else {
            super::rivals::geared(obs, t, false)
        };
        if r.hunting != was {
            r.offered = None; // (a hunt to choose again for)
            self.explorer.log.push(format!(
                "tic {}: {} (health {:.0}, armor {:.0})",
                obs.tic,
                if r.hunting {
                    "geared up: hunting the others"
                } else {
                    "too weak to hunt: gearing up again"
                },
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0)
            ));
        }
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
        // In a jump, or crossing a crusher it has timed, or going on across a floor sinking
        // before its teleporter, the route keeps the legs and the fight only shoots.
        let free = matches!(icon, Some(boss::Turn::Free(_)));
        let stay = matches!(icon, Some(boss::Turn::Stay(_)));
        // ...as on the last dash to a sphere: nothing she could do instead is worth more than
        // having it a second sooner. No falling back, no cover, no standing to shoot.
        let busy = (icon.is_some() && !free)
            || self
                .explorer
                .follower
                .as_ref()
                .is_some_and(|f| f.jumping() || f.crossing_crusher() || f.dashing(obs.tic))
            || crate::debug::timed("player.dashing", || self.fight.dashing(&self.explorer, obs))
            // ...and with the sphere on, on her way to what it's for: straight there, through
            // whatever's between, shooting only what's ahead. Its seconds aren't for them.
            || (self.fight.charge.is_some() && self.fight.combat.now.invulnerable);
        let (errand, toward) = if busy {
            (String::new(), None)
        } else {
            crate::debug::timed("player.errand", || self.fight.errand(&self.explorer, obs))
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
        let retreating =
            !busy && !free && crate::debug::timed("player.retreating", || self.retreating(obs));
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
            None => crate::debug::timed("player.explorer_act", || {
                self.explorer.act(obs, last, &mut self.fight)
            }),
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
    /// A co-op game's gun she has already, lying where the level put it: it stays there for the
    /// others, and walking over it gives her nothing (a monster's dropped gun gives its ammo and
    /// goes, as alone). Placed, by lying where a thing of the map's does.
    /// A key she has too, there: a netgame leaves every key where it lies.
    fn left_lying(&self, obs: &Observation, t: &crate::domain::Thing) -> bool {
        if self.coop.is_some() && t.category == "Key" {
            return crate::specials::Key::had_already(t.name, obs.keys);
        }
        self.coop.as_ref().is_some_and(|spots| {
            crate::bestiary::weapon_item(t.name).is_some()
                && !Self::new_gun(obs, t.name)
                && spots.contains(&(t.x.round() as i32, t.y.round() as i32))
        })
    }

    /// The way to p (as the crow flies) isn't past anything about that she can't take on: a run
    /// for the invulnerability doesn't go straight through what it's for. Till it has moved off
    /// the line, she keeps clear.
    pub(super) fn clear_way_to(&self, obs: &Observation, p: Point) -> bool {
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

    /// On her way to a sphere and no more than a dash from it (`errands.sphere_dash`).
    fn dashing(&self, x: &Explorer, obs: &Observation) -> bool {
        !self.combat.now.invulnerable
            && x.current.as_ref().is_some_and(|cur| {
                dist(obs.pos(), cur.goal) < self.tune.errands.sphere_dash
                    && cur
                        .thing
                        .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
                        .is_some_and(|t| t.name == "InvulnerabilitySphere")
            })
    }

    /// A sphere's way: clear as above, or the sphere no more than a dash off
    /// (`errands.sphere_dash`). She has it before what's by the way has done much, and then
    /// nothing it does matters: a sphere put in the room with what it's for is run for
    /// through them. (Doom II MAP27: the library's arch-vile and three hell knights behind a
    /// door, the sphere two hundred units inside it and left lying, the way to it "not clear".)
    pub(super) fn clear_dash_to(&self, obs: &Observation, p: Point) -> bool {
        dist(obs.pos(), p) < self.tune.errands.sphere_dash || self.clear_way_to(obs, p)
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
    pub(super) fn wants_invulnerability(&self, obs: &Observation, sphere: Point) -> bool {
        self.tune.errands.invulnerability_now
            // (with them in sight: how things stood when she last died isn't how they stand)
            || (!self.combat.visible.is_empty()
                && (self.combat.now.outmatched || self.combat.now.surrounded))
            || self.sphere_use(obs, Some(sphere)).is_some()
    }

    /// The death a sphere is for: of those it would have mended (not want of a gun, nor a
    /// floor) whose place she hasn't been back to this life, the one that followed on
    /// something she set off with the worst thing behind it; with none such, the latest.
    fn sphere_death(&self) -> Option<&Death> {
        let open = |d: &&Death| {
            let at = d.action.as_ref().map_or(d.goal, |a| a.1);
            !matches!(d.why, Cause::Unarmed | Cause::HurtingFloor)
                && d.charged < CHARGES
                && !self
                    .answered
                    .iter()
                    .any(|a| dist(*a, at) < self.tune.deaths.same_goal)
        };
        let threat = |d: &Death| d.action.as_ref().map_or(-1.0, |a| a.4);
        self.deaths
            .iter()
            .filter(open)
            // (the latest of the worst: `max_by` keeps the last of equals)
            .max_by(|a, b| threat(a).total_cmp(&threat(b)))
    }

    /// What invulnerability would be for, and where: what she last died going for, to what
    /// was there (not a floor, a blast or want of a gun: thirty seconds mend none of those);
    /// else the nearest thing awake that she can't take on. From `sphere`, only what she'd get
    /// to with most of the thirty seconds left (`errands.sphere_secs`, the way reckoned as the
    /// straight line and `sphere_detour` times it): a sphere the far side of the level from
    /// the fight isn't for it.
    pub(super) fn sphere_use(
        &self,
        obs: &Observation,
        sphere: Option<Point>,
    ) -> Option<(Point, String)> {
        let e = &self.tune.errands;
        let in_time = |at: Point| {
            sphere.is_none_or(|s| dist(s, at) * e.sphere_detour / RUN_SPEED <= e.sphere_secs)
        };
        let died = self
            .sphere_death()
            // (died after something she did on purpose: it's for doing that again, and kept
            // till she's about to, `sphere_first`; what's before it is fought as it comes)
            .filter(|d| d.action.is_none() || self.sphere_first.is_some() || sphere.is_none())
            .map(|d| match &d.action {
                Some((_, at, what, _, _)) => (*at, what.clone()),
                None => (d.goal, d.label.clone()),
            })
            .filter(|(at, _)| in_time(*at));
        // (kept for what she died after doing: not spent on whatever else is about meanwhile)
        let kept = sphere.is_some()
            && self.sphere_first.is_none()
            && self.sphere_death().is_some_and(|d| d.action.is_some());
        if kept {
            return None;
        }
        died.or_else(|| {
            let from = sphere.unwrap_or(obs.pos());
            self.combat
                .remaining()
                .iter()
                .filter(|s| {
                    s.awake
                        && in_time(s.pos())
                        && self
                            .combat
                            .too_much(obs, &[s.name], self.tune.engage.leave_be)
                })
                .min_by(|a, b| dist(a.pos(), from).total_cmp(&dist(b.pos(), from)))
                .map(|s| (s.pos(), format!("{} {}", s.name, s.id)))
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

impl Fight {
    /// Deathmatch 2.0: what she needs that's due back by now where she saw it taken, to go and
    /// see: health hurt, armor without much, the big ones (a soul sphere, a megasphere, the
    /// blue armor, a berserk pack) whenever. Whether it's there, she'll see (a rival may have
    /// had it again).
    fn checks(
        &mut self,
        x: &Explorer,
        obs: &Observation,
        reach: &BTreeSet<usize>,
    ) -> Vec<Objective> {
        self.checking.clear();
        let Some(r) = &self.returns else {
            return Vec::new();
        };
        let health = obs.health.unwrap_or(100.0);
        let armor = obs.armor.unwrap_or(0.0);
        let mut out = Vec::new();
        for &key in r.gone.keys().filter(|&&k| r.due(k, obs.tic)) {
            let (name, cx, cy) = key;
            let big = matches!(name, "Soulsphere" | "Megasphere" | "BlueArmor" | "Berserk");
            let wanted = big
                || (crate::bestiary::category(name) == "Health"
                    && health < self.tune.errands.hurt + 20.0)
                || (name == "GreenArmor" && armor < 50.0);
            if !wanted {
                continue;
            }
            let at = (f64::from(cx), f64::from(cy));
            if x.nav.poly_at(at).is_none_or(|p| !reach.contains(&p)) {
                continue;
            }
            let goal = x.stand_spot_for(at, reach);
            let mut o = Objective::new(
                Kind::Visit,
                goal,
                format!("see if the {} is back", crate::control::names::called(name)),
            );
            o.needed = true;
            self.checking.push((goal, key));
            out.push(o);
        }
        out
    }

    /// A deathmatch: the others to go after, where the best clue of them is (from `reach`:
    /// what she can get to), geared up; or half geared up, for a fresh clue near her.
    fn hunt(
        &self,
        x: &Explorer,
        obs: &Observation,
        reach: &BTreeSet<usize>,
        pos: Point,
    ) -> Option<Objective> {
        let r = self.rivals.as_ref()?;
        let t = &self.tune.deathmatch;
        let c = r.best(obs.tic, pos, t)?;
        let age = (obs.tic - c.tic) as f64 / crate::domain::TICRATE;
        let near = dist(pos, c.at);
        let go = r.hunting
            || r.ordered
            || (age <= t.fresh_secs && near <= t.fresh_near && super::rivals::geared(obs, t, true));
        if !go {
            return None;
        }
        let nav = &x.nav;
        let goal = match nav.poly_at(c.at).filter(|p| reach.contains(p)) {
            Some(_) => c.at,
            None => {
                let pi = reach
                    .iter()
                    .copied()
                    .filter(|&pi| dist(nav.poly_center(pi), c.at) < self.tune.errands.hunt_near)
                    .min_by(|&a, &b| {
                        dist(nav.poly_center(a), c.at).total_cmp(&dist(nav.poly_center(b), c.at))
                    })?;
                nav.poly_center(pi)
            }
        };
        let mut o = Objective::new(
            Kind::Hunt,
            x.stand_spot_for(goal, reach),
            format!(
                "hunt: {} {} ~{near:.0}, {age:.0}s ago",
                c.what,
                crate::control::directed::compass(crate::domain::heading(pos, c.at)),
            ),
        );
        o.reveals = BTreeSet::from([c.id]);
        o.needed = true;
        Some(o)
    }

    /// A deathmatch: news of the others to choose again for (a clue better than the one she's
    /// after, or one at all if she's after something else, once each; geared up just now).
    fn hunt_news(&mut self, obs: &Observation, obj: &Objective) -> bool {
        let Some(r) = self.rivals.as_mut() else {
            return false;
        };
        let t = &self.tune.deathmatch;
        let Some(best) = r.best(obs.tic, obs.pos(), t) else {
            return false;
        };
        let (id, tic) = (best.id, best.tic);
        if r.offered == Some(id) || obs.tic - r.interrupted < 35 {
            return false;
        }
        // (none she'd go after now: no news to her)
        let age = (obs.tic - tic) as f64 / crate::domain::TICRATE;
        if !r.hunting
            && !r.ordered
            && !(age <= t.fresh_secs
                && dist(obs.pos(), best.at) <= t.fresh_near
                && super::rivals::geared(obs, t, true))
        {
            return false;
        }
        r.offered = Some(id);
        // (after it already, or after one as fresh: nothing new)
        if obj.kind == Kind::Hunt && obj.thing.is_none() {
            let after = obj.reveals.iter().find_map(|&k| r.by_id(k));
            if after.is_none_or(|c| {
                c.id == id || ((tic - c.tic) as f64) < t.newer_secs * crate::domain::TICRATE
            }) {
                return false;
            }
        }
        r.interrupted = obs.tic;
        true
    }
}

/// Deathmatch 2.0's things that come back: where the level put things down, and those she's
/// seen gone, since when (her `what_where`).
struct Returns {
    spots: BTreeSet<(i32, i32)>,
    gone: std::collections::BTreeMap<(&'static str, i32, i32), i64>,
}

/// a thing taken comes back this long after (Doom's P_RespawnSpecials: 30 s)
const RETURN_TICS: i64 = 30 * 35;

impl Returns {
    /// It comes back: the level put it there, and it's not a sphere of invulnerability or
    /// invisibility (P_RemoveMobj queues neither, nor what's dropped).
    fn comes_back(&self, key: (&'static str, i32, i32)) -> bool {
        self.spots.contains(&(key.1, key.2))
            && !matches!(key.0, "InvulnerabilitySphere" | "BlurSphere")
    }

    /// Seen gone and back by now, as far as she knows.
    fn due(&self, key: (&'static str, i32, i32), tic: i64) -> bool {
        self.gone.get(&key).is_some_and(|&t| tic - t >= RETURN_TICS)
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
        // (and the sphere a level puts in with what it's for: shut in there with them is
        // where it's wanted)
        (t.name == "InvulnerabilitySphere" && self.wants_invulnerability(obs, (t.x, t.y)))
            || (obj.urgent
                && (ammo_item(t.name).is_some() || weapon_item(t.name).is_some())
                && self.combat.now.short_of_ammo)
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
        out.extend(self.hunt(x, obs, reach, pos));
        out.extend(self.checks(x, obs, reach));
        if let Some((at, what)) = self.charge.clone().filter(|_| self.combat.now.invulnerable) {
            let mut o = Objective::new(
                Kind::Visit,
                x.stand_spot_for(at, reach),
                format!("back with the invulnerability ({what})"),
            );
            o.urgent = true;
            o.needed = true;
            out.push(o);
        }
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
        // (a clue gone after and no one there: done with, and those about it)
        if obj.thing.is_none()
            && let Some(r) = self.rivals.as_mut()
        {
            let at = obj
                .reveals
                .iter()
                .find_map(|&id| r.by_id(id))
                .map_or(obj.goal, |c| c.at);
            r.spent(at, &self.tune.deathmatch);
            r.spent(obj.goal, &self.tune.deathmatch);
            return;
        }
        if let Some(id) = obj.thing {
            self.hunted_spots
                .insert((id, obj.goal.0.round() as i64, obj.goal.1.round() as i64));
        }
    }

    fn gone(&mut self, obs: &Observation, obj: &Objective) -> bool {
        match obj.kind {
            // (in co-op a gun the level put down stays when it's hers: had, it's done with)
            Kind::Item => obj.thing.is_none_or(|t| {
                !self.present.contains(&t)
                    || obs
                        .things
                        .iter()
                        .flatten()
                        .find(|x| x.id == t)
                        .is_some_and(|x| self.left_lying(obs, x))
            }),
            Kind::Hunt => match obj.thing {
                Some(t) => !self.combat.remaining().iter().any(|s| s.id == t),
                // (a clue of the others': gone when it is, or she's no longer after them)
                None => self.rivals.as_ref().is_none_or(|r| {
                    obj.reveals.iter().all(|&id| r.by_id(id).is_none())
                        || !(r.hunting || super::rivals::geared(obs, &self.tune.deathmatch, true))
                }),
            },
            // (a spot she's off to see whether something's back at: gone once she's seen it)
            Kind::Visit if obj.label.starts_with("see if the ") => {
                !self.returns.as_ref().is_some_and(|r| {
                    self.checking
                        .iter()
                        .any(|(at, key)| *at == obj.goal && r.due(*key, obs.tic))
                })
            }
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
            return !self.wants_invulnerability(obs, obj.goal);
        }
        if self.combat.now.invulnerable {
            return false; // (nothing can hurt her: the risky ones now)
        }
        // What she died after doing waits while she fetches the sphere for it.
        if self.sphere_first.is_some()
            && let Some(li) = obj
                .trigger
                .as_ref()
                .and_then(crate::navigator::TriggerLine::linedef)
            && self
                .deaths
                .iter()
                .any(|d| d.action.as_ref().is_some_and(|a| a.0 == li))
        {
            return true;
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
        // A deathmatch: a hunt before her errands; hunting, what she doesn't need after it;
        // gearing up, a better gun first.
        if let Some(r) = &self.rivals {
            let t = &self.tune.deathmatch;
            if obj.kind == Kind::Hunt && obj.thing.is_none() {
                return t.hunt_weigh;
            }
            if obj.kind == Kind::Item {
                let item = obj
                    .thing
                    .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id));
                if r.hunting && !obj.needed && !obj.urgent {
                    return t.item_weigh;
                }
                if !r.hunting
                    && item.is_some_and(|i| {
                        weapon_item(i.name).is_some() && self.better_gun(obs, i.name)
                    })
                {
                    return t.gun_weigh;
                }
            }
        }
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
        // Fetching the sphere for what she's about to do: the sphere before anything.
        if self.sphere_first.is_some()
            && obj
                .thing
                .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
                .is_some_and(|t| t.name == "InvulnerabilitySphere")
        {
            return self.tune.errands.sphere_first;
        }
        // Invulnerable and off to what it's for: that, and what's to be done there, before
        // anything else, however far beside the rest.
        if let Some((at, _)) = &self.charge
            && dist(obj.goal, *at) < self.tune.deaths.same_goal
        {
            return self.tune.errands.sphere_first;
        }
        // With only the pistol and a better gun to go for, a key waits: it's no nearer for being
        // a key (`Objective::nearer`), and the gun, or what carries one, comes first unless the
        // key's on the way. The pistol alone is next to never enough for what's past a key.
        // (Doom II MAP24, HMP: the blue key in a room of imps gone for life after life, the
        // shotgun known and half as far again; the maintainer watching.)
        if obj.kind == Kind::Key
            && self.gun_to_have
            && self.combat.now.firepower <= self.tune.errands.pistol_only
        {
            return self.tune.errands.key_unarmed;
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
        if self.hunt_news(obs, obj) {
            return true;
        }
        // About to do again what she died after doing (her way sets it off, and she's near
        // it), with a sphere she knows of near enough to it: the sphere first.
        let deadly = self.sphere_death().and_then(|d| d.action.clone());
        if let Some((li, at, what, sectors, _)) = deadly
            && !self.sphere_tried
            && !self.combat.now.invulnerable
        {
            let e = &self.tune.errands;
            let sphere = obs.things.iter().flatten().any(|t| {
                t.name == "InvulnerabilitySphere"
                    && dist((t.x, t.y), at) * e.sphere_detour / RUN_SPEED <= e.sphere_secs
            });
            let line_of = |t: &Option<crate::navigator::TriggerLine>| {
                t.as_ref().and_then(crate::navigator::TriggerLine::linedef)
            };
            let on_her_way = line_of(&obj.trigger) == Some(li)
                || x.follower.as_ref().is_some_and(|f| {
                    f.steps.iter().any(|st| {
                        line_of(&st.trigger) == Some(li)
                            || st.path.as_ref().is_some_and(|p| {
                                p.notes.iter().any(|n| {
                                    matches!(n, crate::navigator::PathNote::OpenDoor { sector }
                                        if sectors.contains(sector))
                                })
                            })
                    })
                });
            if sphere && on_her_way {
                self.sphere_tried = true;
                self.sphere_first = Some(obs.tic);
                self.pending_log.push(format!(
                    "tic {}: died after {what} before: the invulnerability first",
                    obs.tic
                ));
                return true;
            }
        }
        if self
            .sphere_first
            .is_some_and(|t| obs.tic - t > (self.tune.errands.sphere_wait * 35.0) as i64)
        {
            self.sphere_first = None; // (not had in time: on without it)
        }
        // Where a sphere would be for, and she's there: done with for this life.
        if let Some((at, _)) = self.sphere_use(obs, None)
            && dist(obs.pos(), at) < ARRIVED
            && self
                .deaths
                .iter()
                .any(|d| dist(d.action.as_ref().map_or(d.goal, |a| a.1), at) < 1.0)
        {
            self.answered.push(at);
        }
        if self
            .charge
            .as_ref()
            .is_some_and(|(at, _)| dist(obs.pos(), *at) < ARRIVED)
        {
            self.charge = None; // (there with it: on with what's there)
            return true;
        }
        // Invulnerable just now: what it's for chosen, and she turns to that at once.
        if !self.combat.now.invulnerable {
            (self.charge, self.charged) = (None, false);
        } else if !self.charged {
            self.charged = true;
            // (only what she'd reach with most of it left, from where she has it: a sphere
            // taken for the plight she's in isn't carried off across the level for an old one.
            // Doom E3M9: taken surrounded, and every life after run 8,000 units of doors and
            // lifts for a baron's line, out before she was half way)
            let e = &self.tune.errands;
            self.charge = self.sphere_use(obs, None).filter(|(at, _)| {
                dist(obs.pos(), *at) * e.sphere_detour / RUN_SPEED <= e.sphere_secs
            });
            if let Some((at, _)) = &self.charge {
                let at = *at;
                for d in self
                    .deaths
                    .iter_mut()
                    .filter(|d| dist(d.action.as_ref().map_or(d.goal, |a| a.1), at) < 1.0)
                {
                    d.charged += 1;
                }
            }
            if let Some((at, what)) = &self.charge {
                self.pending_log.push(format!(
                    "tic {}: invulnerable: off to ({:.0}, {:.0}) with it ({what})",
                    obs.tic, at.0, at.1
                ));
                return true;
            }
        }
        if obj.urgent || obj.kind == Kind::Exit {
            return false;
        }
        let Some(t) = self.errands(x, obs).into_iter().next() else {
            return false;
        };
        if t.name == "InvulnerabilitySphere" {
            self.offered_spheres.insert(t.id);
        } else {
            self.offered.insert(t.id);
        }
        self.pending_log
            .push(format!("tic {}: spotted a {}", obs.tic, t.name));
        true
    }
}
