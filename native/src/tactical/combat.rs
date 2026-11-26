// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Fighting: which monsters Evil knows are there, which to shoot at and with what, getting out
//! of the way of what's flying at her, and backing off from what would claw her.
//!
//! `Combat::decide` runs every tic and says how much of the controls the fight needs: nothing
//! (no monster she knows is about: the explorer has them); the aim (a monster in sight that
//! isn't pressing: she carries on along her route, turning to shoot as she goes); or everything
//! (something's flying at her, something's close, there are too many, she's low: she stands and
//! fights, strafing and dodging, or backs off; the route waits).
//!
//! What she knows of monsters follows what she can see: a monster is noticed when it's in line
//! of sight in front of her (or near, or she's being hurt), then remembered where last seen.

mod aiming;
mod arsenal;
mod assess;
mod counts;
mod firing;
mod hearing;
mod leaving;
mod memory;
mod metrics;
mod movement;
mod moves;
mod patience;
mod rockets;
mod seeing;
mod sight;
mod state;
mod steering;
mod style;
mod tree;

use super::sound::Hearing;
use super::tuning::{Dodge, Tuning};
use crate::bestiary::{missile, monster, weapon_in, Attack, Missile};
use crate::decisions::{Journal, Record};
use crate::domain::BARREL_HEIGHT;
use crate::domain::{
    heading, norm, Controls, Heard, Observation, Thing, FRICTION, RUN_THRUST, TICRATE,
};
use crate::geom::segment_point_distance;
use crate::geom::{dist, Point};
use crate::los::{Body, LineOfSight};
use crate::map::{ML_BLOCKING, ML_BLOCKMONSTERS};
use crate::navigator::{Navigator, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::specials::Category;
pub use arsenal::Odds;
pub use assess::Assessment;
pub use moves::Move;
use patience::{Effort, Patience};
pub use sight::{eye, eye_of};
use state::{Cover, Dodging, Movers, OffBarrels, Senses, Stepping, Trigger};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// invulnerability lasts this long (INVULNTICS)...
const INVULNERABLE_TICS: i64 = 30 * 35;
/// A boss kind's death, from the killing shot to the game's look for others of its kind (the
/// longest of their dying frames, an arachnotron's 55 tics, and what it takes her to see it's
/// dead)
const BOSS_DEATH_TICS: i64 = 70;

/// a rocket's blast reaches this far (less her radius: 128 in all)
const SPLASH: f64 = 112.0;

/// a missile's height, about (for whether she can see it)
const MISSILE_HEIGHT: f64 = 8.0;

const SLOTS: [i32; 7] = [1, 2, 3, 4, 5, 6, 7];
/// The chaingun fires twice a press: tics from its first bullet to its second (A_FireCGun's frames).
const CHAINGUN_SECOND: i64 = 4;

fn mon(name: &str) -> crate::bestiary::Monster {
    monster(name).unwrap_or(crate::bestiary::Monster {
        radius: 20.0,
        height: 56.0,
        health: 100,
        attack: Attack::None,
        melee: false,
        threat: 0.0,
        boss: false,
    })
}

/// Something to turn and look at: all a sound, a hit or a bump tells her is a way to look.
#[derive(Clone, Debug)]
struct Alert {
    /// given up on after this
    until: i64,
    /// the ways to look (world degrees), the likeliest first: a sound's side is front or back
    ways: Vec<f64>,
    /// a hit: looked at even with something else to shoot
    hit: bool,
    /// what was in sight when it came: anything new is what it was
    in_sight: BTreeSet<i32>,
}

/// A monster she knows of.
#[derive(Clone, Debug)]
pub struct Seen {
    pub id: i32,
    pub name: &'static str,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// last seen
    pub tic: i64,
    /// first seen
    pub first: i64,
    /// where first seen, and facing which way
    pub x0: f64,
    pub y0: f64,
    pub angle0: f64,
    /// seen to move or turn (else it's standing about: a shot may wake it)
    pub awake: bool,
    /// not where she last saw it, when she looked there again: she doesn't know where it is
    /// (till she sees it again)
    pub lost: bool,
    /// how it was moving when she last saw it (a tic)
    pub going: Point,
    /// when she last had it in her sight with nothing between (`tic` goes on through instinct)
    pub sighted: i64,
    /// known of from before this life (a life before, or the level known beforehand) and not
    /// seen yet in this one: where it was first seen, taken to be standing there still
    pub foreknown: bool,
}

impl Seen {
    pub fn pos(&self) -> Point {
        (self.x, self.y)
    }
}

#[derive(Clone, Debug)]
pub struct Decision {
    /// which decision it is (the label, `why`, is for the log)
    pub kind: Move,
    /// the node said so: the route keeps the legs (whatever else: missiles flying)
    pub route_keeps_legs: bool,
    /// a missile coming, or an arch-vile's fire: not walked on through
    pub urgent: bool,
    /// the angle to turn to, if fighting at all
    pub aim: Option<f64>,
    pub fire: bool,
    /// the slot to switch to (0: keep)
    pub weapon: i32,
    /// the world direction to run in (unit), if moving
    pub moving: Option<Point>,
    pub why: String,
}

impl Decision {
    /// A decision with nothing in her sights: no shot, no change of gun. The fight's legs
    /// unless it's a look (`looking`).
    fn without_shot(kind: Move, aim: Option<f64>, moving: Option<Point>, why: String) -> Decision {
        Decision {
            kind,
            route_keeps_legs: false,
            urgent: false,
            aim,
            fire: false,
            weapon: 0,
            moving,
            why,
        }
    }

    /// Turning to look `way`, the route keeping her legs.
    fn looking(way: f64) -> Decision {
        Decision {
            route_keeps_legs: true,
            ..Decision::without_shot(Move::Looking, Some(way), None, "looking round".into())
        }
    }

    /// Does this move want her legs (`busy`: in a jump or a timed crossing, the route's unless
    /// she's pressed)? Whether it gets them is the arbiter's (`tactical/arbiter.rs`).
    pub fn wants_legs(&self, busy: bool) -> bool {
        !self.route_keeps_legs && ((!busy && !self.kind.route_goes_on()) || self.urgent)
    }
}

/// a way taken this long (tics) after the last is a turn about from it, if it's against it
const DART_GAP: i64 = 9;

/// A judgement check's counts (`Combat::decided`), for measuring a run.
#[derive(Clone, Copy, Debug, Default)]
struct Check {
    asked: f64,
    held: f64,
    decided: f64,
    occasions: f64,
    /// the tic being counted, and what came of it so far
    tic: Option<i64>,
    now_held: bool,
    now_decided: bool,
    /// the last tic it decided
    last_decided: Option<i64>,
}

impl Check {
    /// The tic being counted, added in.
    fn close_tic(&mut self) {
        let Some(tic) = self.tic else {
            return;
        };
        self.asked += 1.0;
        if self.now_held {
            self.held += 1.0;
        }
        if self.now_decided {
            self.decided += 1.0;
            if self.last_decided.is_none_or(|t| tic - t > 35) {
                self.occasions += 1.0;
            }
            self.last_decided = Some(tic);
        }
        self.now_held = false;
        self.now_decided = false;
    }
}

pub struct Combat {
    nav: Rc<Navigator>,
    /// the numbers she judges by (the player's, shared)
    pub(super) tune: Rc<Tuning>,
    /// how things stand this tic (see `assess.rs`): worked out once, as she looks
    pub now: Assessment,
    /// what the fight decided, for the journal (the player's takes them)
    pub journal: Journal,
    /// her target, whatever the rule would pick, while it's in sight and after her (a choice
    /// forced: an experiment's, a decision model's)
    pub forced_target: Option<i32>,
    /// moves the tree isn't to make: a node giving one is passed over (forced likewise)
    pub barred: Vec<Move>,
    /// the gun to fight with, whatever the rule would pick, while it has a shot in it (a choice
    /// forced: whoever directs her): its slot, and which gun of the slot (the game's number:
    /// the plain shotgun or the super, the fist or the chainsaw)
    pub ordered_gun: Option<i32>,
    /// the kinds whose last death moves something on this map, and when one of each last died
    boss_kinds: Vec<&'static str>,
    kind_died: BTreeMap<&'static str, i64>,
    /// her rockets, measured for the run's statistics (`rockets`)
    rockets: rockets::Rockets,
    pub ordered_weapon: Option<i32>,
    /// switched away from the ordered gun (something on her, hits coming): not back to it before
    /// this tic, and only once it'd fire (i64::MIN: not switched away)
    pub(super) ordered_away_until: std::cell::Cell<i64>,
    /// the one she's gone at with the berserk fist (`best_gun`, `guns.fist_commit`)
    pub(super) fist_on: std::cell::Cell<Option<i32>>,
    /// the fist or the saw, being brought up, is up by this tic (`tree::melee`)
    pub(super) melee_up: std::cell::Cell<i64>,
    /// the trigger let go, whatever's in her sights (a choice forced likewise)
    pub ordered_hold_fire: bool,
    pub los: LineOfSight,
    /// every monster seen, by id
    pub known: BTreeMap<i32, Seen>,
    /// the works of her senses: the monsters they read, what was heard, when she looked where
    senses: Senses,
    /// the monsters she knows are dead: seen to die, or heard to (a death's heard within
    /// `HEARD`, walls or none)
    dead: BTreeSet<i32>,
    /// each monster as she last saw it
    last_seen: BTreeMap<i32, Thing>,
    /// waiting out a crusher on that one, since then
    crush_wait: Option<(i32, i64)>,
    /// the missiles in flight she's noticed (seen as monsters are; her own always)
    pub missiles_seen: BTreeSet<i32>,
    /// monsters that only claw or charge, in her sight: since when, how far off then, and where
    /// she stood (for whether one can get at her: `cant_reach`)
    reach_watch: BTreeMap<i32, (i64, f64, Point)>,
    /// what's in sight with no shot at any of it, as one effort (`leave_the_lot`)
    lot: Lot,
    /// the ground she's been on, by the mesh's polys: where she knows is safe
    been: Vec<bool>,
    /// the fight's move last tic: for measuring, and for directed play (an order paused while
    /// the fight has her legs)
    pub last_move: Option<Move>,
    /// counts for measuring a run (`STATS`): nothing decides by them
    pub tally: BTreeMap<String, f64>,
    /// judgement checks, for measuring a run (`decided`): nothing decides by them
    checks: std::cell::RefCell<BTreeMap<&'static str, Check>>,
    /// monsters she can see now
    pub visible: Vec<Thing>,
    /// barrels she's seen that are still there
    pub barrels_seen: Vec<Point>,
    /// a route under way (the player's): its follower has its own crusher rules
    pub route_under_way: bool,
    /// ...and it's lining up a run-jump, sprinting for one or in the air off it
    pub jump_under_way: bool,
    /// the route's way on from where she is, as far as it goes unbroken (empty: none)
    pub route_ahead: Vec<Point>,
    pub target: Option<i32>,
    pub kills: i32,
    pub log: Vec<String>,
    health: Option<f64>,
    switch_tic: i64,
    strafe: f64,
    strafe_until: i64,
    rate: f64,
    /// what the fight was last logged as doing (`announce`): logged again when it changes
    announced: String,
    /// the monster a rocket's way at was last blocked close by (whatever's up), and when: the
    /// launcher isn't chosen for it for a while after. A while, not each tic it's clear, or she
    /// would switch guns back and forth as the lane opens and shuts.
    rocket_blocked: Option<(i32, i64)>,
    /// baiting: the monsters in the way are the point, the trigger's let go
    pub baiting: bool,
    /// monsters she's after for the gun they carry (as of the last list of things to do): shot
    /// whatever the shot would wake
    pub wanted_dead: BTreeSet<i32>,
    /// monster -> tics it's looked to be after another
    infight: BTreeMap<i32, i32>,
    heights_tic: Option<i64>,
    heights_now: Vec<(f64, f64)>,
    hit_cache: BTreeMap<(i32, u64), bool>,
    /// the missiles she's getting out of the way of, and the way
    dodge: Dodging,
    /// where the route's taking her mid-fight (an errand: health, a gun, ammo), for dodging
    /// towards: on to it while dodging, not dodging where she stands
    pub going_to: Option<Point>,
    /// the floors that would hurt her now, as she takes them, by sector (what she knows: the
    /// player's, each tic; `Knowledge::hazards`)
    pub hazards: Vec<bool>,
    last_pos: Option<Point>,
    velocity: Point,
    /// the way the route last walked her (in the world, whichever way she was turned to aim;
    /// kept while the fight holds the route): what's in her way is what's that way, not where
    /// she's facing (None: the route standing her still)
    route_way: Option<f64>,
    /// keeping off the barrels by her while there's shooting
    off_barrels: OffBarrels,
    /// her stepping about a target: the way last taken, and what came of it
    steer: Stepping,
    /// getting out of fire there's no dodging: an arch-vile's, a hitscan shooter's
    cover: Cover,
    /// invulnerable till this tic (from the pickup's message: 30 s, as a player knows it)
    invulnerable_until: i64,
    /// nothing can hurt her (the pass before a level known beforehand): as invulnerable, all of it
    pub immune: bool,
    /// where each monster in sight has been lately: (tic, x, y), oldest first
    tracks: BTreeMap<i32, std::collections::VecDeque<(i64, f64, f64)>>,
    /// the gun's timing: when the trigger was down, when a shot left, what her legs wait for
    trigger: Trigger,
    /// health lost lately: (tic, how much)
    damage: Vec<(i64, f64)>,
    /// ...and of it, what the floor took (nothing having hit her, on a floor she knows hurts)
    floor_damage: Vec<(i64, f64)>,
    /// for the log: rounds spent this fight, and the health monsters lost as the engine has it.
    /// Nothing is decided by them.
    spent: f64,
    dealt: f64,
    ammo_was: Vec<(i32, i32)>,
    /// what she's put into each monster, as she'd reckon it: the rounds gone from the gun up while
    /// it was her target, at what such a shot does landing at that range (never its health)
    put_in: BTreeMap<i32, f64>,
    health_was: Vec<(i32, i32)>,
    /// what she's seen each monster lose (hits landing in her sight, hers or another's), by id:
    /// its health as she reckons it is its kind's less this
    seen_hurt: BTreeMap<i32, i32>,
    /// floors and ceilings on the move, as she's seen them go, and which of them crush
    pub movers: Movers,
    /// how long she's been at each monster she's getting nowhere with, and those given up on
    /// (see `patience.rs`)
    pub(super) patience: Patience,
    /// the sector each stood in when closing in on it to hit it in melee got her no nearer
    pub(super) fist_stalled: BTreeMap<i32, usize>,
}

impl Combat {
    pub fn new(nav: Rc<Navigator>, tune: Rc<Tuning>) -> Combat {
        let sinks_into_harm = (0..nav.map.sectors.len())
            .filter(|&s| {
                nav.triggers.iter().any(|t| {
                    t.info.trigger.walked()
                        && t.effect(s).is_some()
                        && nav.floor_after(t, s) == Some(true)
                })
            })
            .collect();
        let moved_only_by = |only: fn(Category) -> bool| -> BTreeSet<usize> {
            (0..nav.map.sectors.len())
                .filter(|&s| {
                    let mut movers = nav
                        .triggers
                        .iter()
                        .filter(|t| t.sectors.contains(&s))
                        .peekable();
                    movers.peek().is_some() && movers.all(|t| only(t.info.category))
                })
                .collect()
        };
        let doors = moved_only_by(Category::is_door);
        let lifts = moved_only_by(|c| c == Category::Lift);
        Combat {
            tune,
            now: Assessment::default(),
            journal: Journal::default(),
            forced_target: None,
            barred: Vec::new(),
            ordered_gun: None,
            boss_kinds: nav.boss_death_kinds(),
            kind_died: BTreeMap::new(),
            rockets: rockets::Rockets::default(),
            ordered_weapon: None,
            ordered_away_until: std::cell::Cell::new(i64::MIN),
            fist_on: std::cell::Cell::new(None),
            melee_up: std::cell::Cell::new(0),
            ordered_hold_fire: false,
            los: LineOfSight::new(&nav.map),
            known: BTreeMap::new(),
            senses: Senses {
                game_monsters: BTreeMap::new(),
                hearing: Hearing::new(&nav.map),
                cries_due: Vec::new(),
                attacks_heard: Vec::new(),
                alert: None,
                flinch: None,
                glanced: 0,
                checked: 0,
                looked: [i64::MIN / 2; 16],
            },
            dead: BTreeSet::new(),
            last_seen: BTreeMap::new(),
            crush_wait: None,
            missiles_seen: BTreeSet::new(),
            reach_watch: BTreeMap::new(),
            lot: Lot::default(),
            been: Vec::new(),
            nav,
            last_move: None,
            tally: BTreeMap::new(),
            checks: std::cell::RefCell::new(BTreeMap::new()),
            visible: Vec::new(),
            barrels_seen: Vec::new(),
            route_under_way: false,
            jump_under_way: false,
            route_ahead: Vec::new(),
            target: None,
            kills: 0,
            log: Vec::new(),
            health: None,
            switch_tic: -1_000_000_000,
            strafe: 1.0,
            strafe_until: 0,
            rate: 0.0,
            announced: String::new(),
            rocket_blocked: None,
            baiting: false,
            infight: BTreeMap::new(),
            heights_tic: None,
            heights_now: Vec::new(),
            hit_cache: BTreeMap::new(),
            dodge: Dodging {
                missiles: BTreeSet::new(),
                homing_since: BTreeMap::new(),
                shake_spot: None,
                way: None,
                clear: false,
            },
            going_to: None,
            hazards: Vec::new(),
            last_pos: None,
            velocity: (0.0, 0.0),
            route_way: None,
            off_barrels: OffBarrels {
                way: None,
                since: 0,
                last: -1_000_000_000,
                rest_until: -1_000_000_000,
            },
            steer: Stepping {
                way: None,
                from: None,
                tic: -1,
                still: 0,
                stuck: None,
                turned_about: i64::MIN / 2,
                legs_way: None,
                legs_turned: i64::MIN / 2,
                no_step: BTreeMap::new(),
                plasma_to: None,
                no_back: BTreeMap::new(),
                no_keep: BTreeMap::new(),
            },
            wanted_dead: BTreeSet::new(),
            cover: Cover {
                vile_way: None,
                vile_until: 0,
                duck_way: None,
                duck_until: 0,
                no_duck_tic: i64::MIN / 2,
            },
            tracks: BTreeMap::new(),
            invulnerable_until: i64::MIN / 2,
            immune: false,
            trigger: Trigger {
                shot_at: None,
                no_effect: None,
                spread_at: None,
                ammo_log: std::collections::VecDeque::new(),
                blocked_at: None,
                held: false,
                shot_out: None,
                line_held: None,
                burst_rest: i64::MIN / 2,
                burst_from: i64::MIN / 2,
                last_shot: None,
                rocket_facing: None,
                held_fire: None,
            },
            damage: Vec::new(),
            floor_damage: Vec::new(),
            spent: 0.0,
            dealt: 0.0,
            ammo_was: Vec::new(),
            put_in: BTreeMap::new(),
            health_was: Vec::new(),
            seen_hurt: BTreeMap::new(),
            movers: Movers {
                ceilings: Vec::new(),
                floors: Vec::new(),
                floor_up: Vec::new(),
                ceiling_down: Vec::new(),
                crushers: BTreeMap::new(),
                crusher_out: BTreeMap::new(),
                crushed_low: Vec::new(),
                doors,
                lifts,
                sinks_into_harm,
            },
            patience: Patience::default(),
            fist_stalled: BTreeMap::new(),
        }
    }

    /// A new life: the level from the start (what's been seen is remembered).
    pub fn restart(&mut self) {
        self.senses.game_monsters.clear();
        self.dead.clear();
        self.last_seen.clear();
        // What she had when she died isn't rounds spent in the life after.
        self.ammo_was.clear();
        // The level begun again: every monster she's seen is back where she first saw it, for all
        // she knows, standing as it stood. She knows of it before it knows of her.
        for s in self.known.values_mut() {
            (s.x, s.y, s.awake, s.lost, s.going, s.foreknown) =
                (s.x0, s.y0, false, false, (0.0, 0.0), true);
            (s.tic, s.sighted) = (i64::MIN / 2, i64::MIN / 2);
        }
        self.reach_watch.clear();
        self.lot = Lot::default();
        self.patience = Patience::default();
        self.missiles_seen.clear();
        self.seen_hurt.clear();
        self.senses.cries_due.clear();
        self.senses.attacks_heard.clear();
        self.senses.alert = None;
        self.visible.clear();
        self.now.nothing_in_sight();
        self.tracks.clear();
        self.put_in.clear();
        self.invulnerable_until = i64::MIN / 2;
        self.target = None;
        self.health = None;
        self.announced.clear();
        self.dodge.missiles.clear();
        self.dodge.homing_since.clear();
        self.dodge.shake_spot = None;
        self.last_pos = None;
        self.velocity = (0.0, 0.0);
        self.route_way = None;
        self.steer.from = None;
    }

    /// Born again at her start in a co-op game: the level as it is, the monsters where she last
    /// knew them, the dead dead. What goes is what was of her last life's fight: what she was
    /// shooting, dodging, had in sight.
    pub fn reborn(&mut self) {
        self.ammo_was.clear();
        self.reach_watch.clear();
        self.lot = Lot::default();
        self.patience = Patience::default();
        self.missiles_seen.clear();
        self.seen_hurt.clear();
        self.senses.cries_due.clear();
        self.senses.attacks_heard.clear();
        self.senses.alert = None;
        self.visible.clear();
        self.now.nothing_in_sight();
        self.tracks.clear();
        self.invulnerable_until = i64::MIN / 2;
        self.target = None;
        self.health = None;
        self.dodge.missiles.clear();
        self.dodge.homing_since.clear();
        self.dodge.shake_spot = None;
        self.last_pos = None;
        self.velocity = (0.0, 0.0);
        self.route_way = None;
        self.steer.from = None;
    }

    // --- what's about -----------------------------------------------------------------------

    pub fn observe(&mut self, obs: &Observation) {
        let _timed = crate::debug::Timed::new("fight.observe");
        let pos = obs.pos();
        if obs
            .message
            .as_deref()
            .is_some_and(|m| m.starts_with("Invulnerability"))
        {
            self.invulnerable_until = obs.tic + INVULNERABLE_TICS;
        }
        if let Some(v) = obs.velocity {
            self.velocity = v;
        } else if let Some(l) = self.last_pos {
            self.velocity = (pos.0 - l.0, pos.1 - l.1);
        }
        self.last_pos = Some(pos);
        let ammo: Vec<(i32, i32)> = obs.ammo.clone().unwrap_or_default();
        self.note_rounds_gone(obs, &ammo);
        self.reckon_put_in(obs, &ammo);
        self.ammo_was = ammo;
        self.note_health_lost(obs);
        self.watch_movers(obs);
        self.note_hurt(obs);
        self.note_deaths(obs);
        self.hear(obs);
        self.refresh_tic_cache(obs);
        let me = Body {
            x: obs.x,
            y: obs.y,
            z: self.z(obs),
            height: PLAYER_HEIGHT,
        };
        self.see_monsters(obs, me);
        self.see_missiles(obs, me);
        self.measure_rockets(obs);
        self.see_barrels(obs, me);
        self.heed(obs);
        self.watch_infighting(obs);
        self.now = self.assess(obs);
        self.watch_reach(obs);
        self.note_ground_and_view(obs);
        self.check(obs);
        let what = if !self.now.against_her.is_empty() {
            "tics with something in view"
        } else if !self.now.remembered.is_empty() {
            "tics with only remembered"
        } else {
            "tics with nothing about"
        };
        self.count(what, 1.0);
    }

    /// Which monsters in sight are after each other, not her: facing another, not her, and
    /// either moving or faced back.
    fn watch_infighting(&mut self, obs: &Observation) {
        let mut now: BTreeMap<i32, i32> = BTreeMap::new();
        for t in &self.visible {
            let awake = self.known.get(&t.id).is_some_and(|s| s.awake);
            // Awake and after something, not her: which way it runs and shoots shows it.
            if !awake || t.after_her || !(t.vx.hypot(t.vy) > 0.5 || t.attack >= 0) {
                continue; // standing about where it was put isn't fighting
            }
            let n = self.infight.get(&t.id).copied().unwrap_or(0) + 1;
            now.insert(t.id, n);
            if n == self.tune.senses.infight_tics {
                self.log.push(format!(
                    "tic {}: {} {} is fighting another: leaving it to it",
                    obs.tic, t.name, t.id
                ));
            }
        }
        self.infight = now;
    }

    pub fn infighting(&self, t: &Thing) -> bool {
        self.infight.get(&t.id).copied().unwrap_or(0) >= self.tune.senses.infight_tics
    }

    /// The monsters in sight that are after her (not each other): `now.against_her`.
    fn seen_against_her(&self) -> Vec<Thing> {
        self.visible
            .iter()
            .filter(|t| !self.infighting(t))
            .cloned()
            .collect()
    }

    /// If t is standing about, and a shot would be heard by enough monsters she's seen standing
    /// about too that she'd be outmatched once they're awake: what it'd wake (else "").
    fn would_wake(&mut self, obs: &Observation, t: &Thing) -> String {
        let Some(s) = self.known.get(&t.id) else {
            return String::new();
        };
        if s.awake || self.wanted_dead.contains(&t.id) {
            return String::new(); // (the fist too wakes what hears it: P_FireWeapon's noise alert, no weapon WPF_SILENT)
        }
        if self
            .now
            .about
            .iter()
            .any(|o| self.known.get(&o.id).is_some_and(|k| k.awake))
        {
            return String::new(); // (already in a fight)
        }
        let Some(here) = self.nav.poly_at(obs.pos()) else {
            return String::new();
        };
        let hs: Vec<crate::nav::Heights> = self
            .heights(obs)
            .iter()
            .map(|&(floor, ceiling)| crate::nav::Heights { floor, ceiling })
            .collect();
        let heard = self
            .senses
            .hearing
            .heard(self.nav.sector_of_poly(here), &hs);
        let sleepers: Vec<Seen> = self
            .remaining()
            .into_iter()
            .filter(|m| {
                !m.awake
                    && self
                        .nav
                        .poly_at(m.pos())
                        .is_some_and(|p| heard.contains(&self.nav.sector_of_poly(p)))
            })
            .collect();
        let hp: f64 = sleepers.iter().map(|m| f64::from(mon(m.name).health)).sum();
        if sleepers.len() < 2 || hp / self.now.firepower <= self.tune.engage.leave_be {
            return String::new();
        }
        let names: BTreeSet<&str> = sleepers.iter().map(|m| m.name).collect();
        format!(
            "{}: {}",
            sleepers.len(),
            names.into_iter().collect::<Vec<_>>().join(", ")
        )
    }

    // --- deciding ---------------------------------------------------------------------------

    /// A rocket on its way out (8 tics after the trigger, the way she faces then): the way to
    /// face till it's out, and whether that is turning off a way it would go off near her (the
    /// last resort: the trigger is pressed and can't be called back). Otherwise her facing is
    /// held the way it was judged safe, whatever new target comes up: a swing through the launch
    /// sends the rocket out mid-turn.
    fn rocket_held(&mut self, obs: &Observation) -> Option<(f64, bool)> {
        if !self
            .trigger
            .last_shot
            .is_some_and(|(slot, tic)| slot == 5 && ((obs.tic - tic) as f64) < aiming::ROCKET_DELAY)
        {
            return None;
        }
        // (invulnerable: her own blast can't hurt her, and a rocket turned off is a rocket
        // wasted. End to end, E2M7: a sphere's thirty seconds with the launcher, nineteen
        // rockets turned off something near, two rounds spent and nothing hurt)
        if !self.now.invulnerable
            && let Some(h) = self.rocket_way_out(obs)
        {
            return Some((h, true));
        }
        self.trigger.rocket_facing.map(|f| (f, false))
    }

    /// What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
    /// something that wants the controls (a run-up, a jump). `errand`: the route's going for
    /// something to fight with: the fight only aims and shoots on the way.
    pub fn decide(&mut self, obs: &Observation, busy: bool, errand: &str) -> Option<Decision> {
        let _timed = crate::debug::Timed::new("fight.decide");
        // Invulnerable: nothing to dodge, a missile does nothing.
        let missiles = if self.now.invulnerable {
            Vec::new()
        } else {
            self.incoming(obs)
        };
        let target = crate::debug::timed("fight.pick_target", || self.pick_target(obs));
        self.last_move = None;
        if target.is_none() && missiles.is_empty() {
            // The fight over with a rocket still to go out: the way is held all the same, and the
            // route's turn comes after.
            if let Some((aim, _)) = self.rocket_held(obs) {
                return Some(Decision {
                    route_keeps_legs: true,
                    ..Decision::without_shot(
                        Move::RocketHeld,
                        Some(aim),
                        None,
                        "holding a rocket's way till it's out".into(),
                    )
                });
            }
            // A barred move isn't made here either (`barred`).
            let d = self
                .no_target(obs, busy, errand)
                .filter(|d| !self.barred.contains(&d.kind));
            self.last_move = d.as_ref().map(|d| d.kind);
            return d;
        }
        let (s, fire, weapon) = crate::debug::timed("fight.size_up", || {
            self.size_up(obs, busy, errand, missiles, target)
        });
        let mut act = crate::debug::timed("fight.engage", || tree::engage(self, obs, &s));
        // A rocket on its way out, 8 tics after the trigger, the way she faces then: if that way it
        // would go off near her, she turns to the nearest way it won't.
        if s.vile.is_none()
            && let Some((aim, out)) = self.rocket_held(obs)
        {
            act.aim = Some(aim);
            if out {
                act.kind = Move::RocketTurnedOff;
                act.why = "turned off a rocket's way out into something near".into();
            }
        }
        // Hit from where she isn't looking: round to it, whatever she was shooting, and with
        // missiles in the air too. A dodge is her legs'; her eyes are free.
        if s.vile.is_none()
            && let Some(way) = self.look(obs, true)
        {
            act.aim = Some(way);
            act.hold_fire = true;
        }
        let aim = act.aim.or(s.aim);
        let fire = fire && !act.hold_fire && !self.baiting && !self.ordered_hold_fire;
        let why = act.why;
        let kind = act.kind;
        let urgent = !s.missiles.is_empty() || s.vile.is_some();
        if why != self.announced {
            let t = s.target.as_ref().map_or(String::new(), |t| {
                format!(
                    " (target {} {}, {:.0} threat; health {:.0})",
                    t.name, t.id, self.now.threat, self.now.health
                )
            });
            self.log.push(format!("tic {}: {why}{t}", obs.tic));
            self.journal.note(|| Record {
                detail: why.clone(),
                ..Record::new(obs.tic, "move", format!("{kind:?}"))
            });
            self.announced = why.clone();
        }
        if obs.tic % 175 == 0
            && crate::debug::on().shot
            && let Some(t) = &s.target
        {
            self.log.push(format!("tic {}: (she's at ({:.0}, {:.0}, {:.0}); {} {} at ({:.0}, {:.0}, {:.0}), {:.0} off; the move {kind:?}, going {:?}; gun {:?}, ammo {:?}, keys {:?})", obs.tic, obs.x, obs.y, self.z(obs), t.name, t.id, t.x, t.y, t.z, dist((t.x, t.y), obs.pos()), act.moving.map(|m| (m.0.round(), m.1.round())), obs.weapon, obs.ammo, obs.keys));
        }
        self.last_move = Some(kind);
        Some(Decision {
            kind,
            route_keeps_legs: act.route_keeps_legs,
            urgent,
            aim,
            fire,
            weapon,
            moving: act.moving,
            why,
        })
    }

    /// The fight as it stands, for the tree: the aim and the trigger at the target (or a barrel
    /// by it), the weapon to switch to, and what the nodes look at.
    fn size_up<'a>(
        &mut self,
        obs: &Observation,
        busy: bool,
        errand: &'a str,
        missiles: Vec<Thing>,
        target: Option<Thing>,
    ) -> (tree::Situation<'a>, bool, i32) {
        let pos = obs.pos();
        let first_missile = missiles.first().cloned();
        let (mut aim, mut fire, mut weapon) = (None, false, 0);
        if let Some(t) = &target {
            let (a, f, w) = self.aim_and_fire(obs, t);
            (aim, fire, weapon) = (Some(a), f, w);
            if let Some(shot) = self.barrel_shot(obs)
                && matches!(obs.weapon, Some(2..=4))
            {
                // The barrels round a monster do what her gun can't.
                let a = heading(pos, shot);
                let d = dist(shot, pos);
                aim = Some(a);
                let floor = self.floor_at(obs, shot);
                fire = norm(a - obs.angle).abs() < 0.75f64.max(8.0f64.atan2(d).to_degrees())
                    && d < weapon_in(obs.weapon.unwrap_or(2), obs).reach
                    && self.shot_clear(obs, shot, floor, 32.0);
            }
        }
        if let Some(t) = target.as_ref().filter(|t| self.reachable(obs, t)) {
            self.patience.rest(Effort::Closing, t.id); // (got it in reach: the clock again, next time)
        }
        if let Some(t) = target.clone().filter(|t| self.can_hit(obs, t, None)) {
            self.patience.rest(Effort::LostAgain, t.id); // (a shot at it: not round again for nothing)
        }
        self.leave_the_lot(obs);
        let close: Vec<Thing> = self
            .now
            .about
            .iter()
            .filter(|t| {
                let m = mon(t.name);
                // Not one that can't get at her: nothing to back off from. Nor one of a lot
                // she's left as out of her guns' way.
                (matches!(m.attack, Attack::Melee | Attack::Charge | Attack::Fire) || m.melee)
                    && dist((t.x, t.y), pos) < self.tune.engage.close + m.radius
                    && !self.cant_reach(obs, t)
                    && !self.left_with_the_lot(t.id)
                    && !self.kept_from_her(obs, t)
            })
            .cloned()
            .collect();
        let shooting = self
            .visible
            .iter()
            .any(|t| matches!(mon(t.name).attack, Attack::Hitscan | Attack::Missile));
        let barrel = obs.barrels.iter().flatten().copied().reduce(|a, b| {
            if dist(b, pos) < dist(a, pos) {
                b
            } else {
                a
            }
        });
        let mut barrel_away = None;
        // With something shooting in sight, or her shooting at something: a shot, hers or theirs,
        // sets a barrel off.
        if first_missile.is_none()
            && !busy
            && (shooting || target.is_some())
            && let Some(b) = barrel.filter(|&b| dist(b, pos) < self.tune.barrels.keep_clear + 64.0)
        {
            barrel_away = self.away_from_barrels(obs, b);
        }
        // Invulnerable: nothing to hide from. Her BFG ball in the air: she stays in their sight
        // till it lands, since its spray comes from where she is.
        let unhurt = self.now.invulnerable;
        if unhurt {
            barrel_away = None;
        }
        // A barrel going off by her, whoever set it off: away from it before a missile's dodge. Its
        // blast is the worse of the two.
        let barrel_lit = if busy || unhurt {
            None
        } else {
            Self::lit_barrel_by(obs).and_then(|b| self.away_from(obs, b))
        };
        let vile = if busy || unhurt {
            None
        } else {
            self.vile_fire(obs)
        };
        let duck = if busy || unhurt || vile.is_some() || self.bfg_flying(obs) {
            None
        } else {
            self.duck(obs)
        };
        let s = tree::Situation {
            pos,
            busy,
            errand,
            missiles,
            target,
            aim,
            close,
            barrel_away,
            barrel_lit,
            vile,
            duck,
        };
        (s, fire, weapon)
    }

    /// What the fight is doing is logged when it changes (`mode`), not every tic.
    pub(super) fn announce(&mut self, obs: &Observation, why: &str) {
        if why != self.announced {
            self.log.push(format!("tic {}: {why}", obs.tic));
            self.announced = why.to_string();
        }
    }

    /// Why the trigger's held with a target in her sights, in plain words (None: it isn't).
    pub fn holding_fire(&self) -> Option<&'static str> {
        self.trigger.held_fire.map(|h| h.label())
    }

    /// ...only when it's for a reason that lasts (`HeldFire::lasting`).
    pub fn holding_fire_long(&self) -> Option<&'static str> {
        self.trigger
            .held_fire
            .filter(|h| h.lasting())
            .map(|h| h.label())
    }

    /// Has she given up on getting at id a while?
    pub(super) fn given_up_on(&self, obs: &Observation, id: i32) -> bool {
        self.patience.given_up(id, obs.tic)
    }

    /// Out of her fist's reach: given up on, or still on the floor it stood on when closing in
    /// on it got her no nearer, while she isn't on that floor (a ledge above her stays one,
    /// whether she walks along under it or not).
    pub(super) fn out_of_fist_reach(&self, obs: &Observation, t: &Thing) -> bool {
        self.given_up_on(obs, t.id)
            || self.fist_stalled.get(&t.id).is_some_and(|&s| {
                self.sector_at((t.x, t.y)) == Some(s) && self.sector_at(obs.pos()) != Some(s)
            })
    }

    /// The sector under a point.
    pub(super) fn sector_at(&self, p: Point) -> Option<usize> {
        self.nav.poly_at(p).map(|pi| self.nav.sector_of_poly(pi))
    }

    /// The route's controls this tic (the way it walks her, for what's in her way).
    pub fn route_walks(&mut self, obs: &Observation, b: &Controls) {
        let _timed = crate::debug::Timed::new("fight.route_walks");
        self.route_way = (b.forward.hypot(b.strafe) > 0.0)
            .then(|| obs.angle - b.turn + (-b.strafe).atan2(b.forward).to_degrees());
    }

    // --- helpers ----------------------------------------------------------------------------

    /// The last hit she took: (tic, health lost).
    pub fn last_hit(&self) -> Option<(i64, f64)> {
        self.damage.last().copied()
    }

    /// A crusher at work in sector s: its ceiling has come down lower than she stands within
    /// the last `senses.active_crusher` tics, whether or not it's at the top of its stroke now.
    /// The fight's moves don't go there at all, not just while it's on its way down.
    pub fn crusher_active(&self, obs: &Observation, s: usize) -> bool {
        !self.movers.doors.contains(&s)
            && self
                .movers
                .crushed_low
                .get(s)
                .is_some_and(|&t| obs.tic - t < self.tune.senses.active_crusher)
    }

    /// Is sector s a crusher she knows of that is too low, stopped or not, to get out from under
    /// if it starts again? That is: less room over her head, in tics, than getting out of it
    /// takes at the worst, and a stroke that costs more than she'd take on (one that holds her:
    /// always; a fast one: more than a tenth of her health).
    pub fn crusher_trap(&self, obs: &Observation, s: usize) -> bool {
        let (Some(&kind), Some(&out), Some(h)) = (
            self.movers.crushers.get(&s),
            self.movers.crusher_out.get(&s),
            obs.height(s),
        ) else {
            return false;
        };
        crate::crush::trap(
            h,
            kind,
            out,
            crate::domain::RISK_SHARE * obs.health.unwrap_or(100.0),
            obs,
        )
    }

    /// Is sector s crushing now? Its ceiling coming down lately and low; or its floor coming up
    /// lately with little more than her height left under the ceiling (a floor raised to
    /// crush; a lift ends level with a floor beside it, with room enough over it).
    pub fn crushing(&self, obs: &Observation, s: usize) -> bool {
        let gap = obs
            .heights
            .get(s)
            .map_or(f64::INFINITY, |h| h.ceiling - h.floor);
        (!self.movers.doors.contains(&s)
            && self
                .movers
                .ceiling_down
                .get(s)
                .is_some_and(|&t| obs.tic - t < self.tune.senses.crush_tics)
            && gap < self.tune.senses.crush_gap)
            || (!self.movers.lifts.contains(&s)
                && self
                    .movers
                    .floor_up
                    .get(s)
                    .is_some_and(|&t| obs.tic - t < self.tune.senses.crush_tics)
                && gap < self.tune.senses.floor_crush_gap)
    }

    /// Health lost a second lately (over the last few seconds): `now.damage_rate`; and of it,
    /// what something dealt her (not the floor's): `now.hit_rate`.
    fn damage_rate(&mut self, tic: i64) -> (f64, f64) {
        let window = self.tune.senses.damage_window;
        self.damage.retain(|&(t, _)| tic - t < window);
        self.floor_damage.retain(|&(t, _)| tic - t < window);
        let per =
            |d: &[(i64, f64)]| d.iter().map(|(_, d)| d).sum::<f64>() / (window as f64 / TICRATE);
        let all = per(&self.damage);
        (all, (all - per(&self.floor_damage)).max(0.0))
    }

    /// Could a hitscan shooter she knows of, awake, from where she last saw it, see her at p? (Its
    /// bullets reach whatever it sees, however far.)
    pub fn shot_from(&mut self, obs: &Observation, s: &Seen, p: Point) -> bool {
        if !s.awake || !self.sniper(obs, s.name) {
            return false;
        }
        let floor = self.floor_at(obs, p);
        let heights = self.heights(obs).to_vec();
        self.sees_her(eye_of(s), p, floor, &heights)
    }

    /// Invulnerable, with time enough left to make something of it (the last seconds counted out:
    /// not caught in the open as it wears off)?
    pub fn invulnerable(&self, tic: i64) -> bool {
        self.immune || tic < self.invulnerable_until - self.tune.senses.invulnerable_spare
    }

    /// Her own BFG ball in the air: its spray comes from where she is when it lands, along the
    /// way it flew (A_BFGSpray: forty lines from her, a quarter turn wide, 1024 out), so she
    /// stays where she can see them till then.
    pub fn bfg_flying(&self, obs: &Observation) -> bool {
        let charging = self
            .trigger
            .last_shot
            .is_some_and(|(slot, tic)| slot == 7 && obs.tic - tic <= aiming::BFG_DELAY + 1);
        charging
            || obs
                .things
                .iter()
                .flatten()
                .any(|t| t.name == "BFGBall" && t.owner == 0)
    }

    /// How t is moving, a tic: its momentum, or, walking (a monster walks by steps and has no
    /// momentum as it goes), how far it has gone over the last few tics. For leading a rocket
    /// or a plasma ball at something walking.
    pub fn moving(&self, t: &Thing) -> Point {
        if t.vx.hypot(t.vy) > 0.5 {
            return (t.vx, t.vy);
        }
        match self
            .tracks
            .get(&t.id)
            .and_then(|tr| Some((*tr.front()?, *tr.back()?)))
        {
            Some(((t0, x0, y0), (t1, x1, y1))) if t1 - t0 >= self.tune.senses.track_min => {
                let dt = (t1 - t0) as f64;
                ((x1 - x0) / dt, (y1 - y0) / dt)
            }
            _ => (0.0, 0.0),
        }
    }

    /// Was there nowhere to duck from a hitscan shooter's fire just now (in a long fight with
    /// it)?
    pub fn in_the_open(&self, tic: i64) -> bool {
        tic - self.cover.no_duck_tic <= 1
    }

    /// What the fight's doing now ("" with none on).
    pub fn mode(&self) -> &str {
        &self.announced
    }

    /// A switch to the ready gun asked for (not again till it's had time).
    pub fn readied(&mut self, obs: &Observation) {
        self.switch_tic = obs.tic;
    }

    /// Every sector's floor and ceiling now (made once a tic: it's asked for a lot).
    pub fn heights(&mut self, obs: &Observation) -> &[(f64, f64)] {
        self.refresh_tic_cache(obs);
        &self.heights_now
    }

    /// Heights and cached shots both belong to the current observation tic.
    fn refresh_tic_cache(&mut self, obs: &Observation) {
        if self.heights_tic != Some(obs.tic) {
            self.heights_now = obs.heights.iter().map(|h| (h.floor, h.ceiling)).collect();
            self.heights_tic = Some(obs.tic);
            self.hit_cache.clear();
        }
    }

    pub fn z(&self, obs: &Observation) -> f64 {
        obs.z.unwrap_or_else(|| self.floor_at(obs, obs.pos()))
    }
}

/// How far a missile's middle has to pass from hers to miss (and its blast).
fn clearance(tune: &Dodge, m: &Missile) -> f64 {
    hit_clearance(tune, m) + if m.splash { SPLASH } else { 0.0 }
}

/// ...and this far, clear of the missile itself (its blast or not)
fn hit_clearance(tune: &Dodge, m: &Missile) -> f64 {
    PLAYER_RADIUS + m.radius + tune.margin
}

/// How near t comes to her as it goes by, if she runs `way` from now. She is taken to stop
/// once she has gone `room` (a wall that way). A rocket is counted from its middle: its
/// blast, the clearance, does the rest.
fn miss_by(tune: &Dodge, pos: Point, vel: Point, way: Point, room: f64, t: &Thing) -> f64 {
    let (mut x, mut y) = pos;
    let (mut vx, mut vy) = vel;
    let mut nearest = f64::INFINITY;
    for k in 1..=tune.within_tics {
        vx += RUN_THRUST * way.0;
        vy += RUN_THRUST * way.1;
        if dist((x + vx, y + vy), pos) > room {
            (vx, vy) = (0.0, 0.0);
        }
        x += vx;
        y += vy;
        vx *= FRICTION;
        vy *= FRICTION;
        let k = k as f64;
        let d = (t.x + t.vx * k - x).hypot(t.y + t.vy * k - y);
        nearest = nearest.min(d);
        if d > nearest + 64.0 {
            break; // (gone by)
        }
    }
    nearest
}

/// How far round `at` the points are spread (degrees: 360 less the widest gap).
pub fn spread(at: Point, points: &[Point]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut b: Vec<f64> = points
        .iter()
        .map(|p| {
            (p.1 - at.1)
                .atan2(p.0 - at.0)
                .to_degrees()
                .rem_euclid(360.0)
        })
        .collect();
    b.sort_by(f64::total_cmp);
    let mut widest = b[0] + 360.0 - b[b.len() - 1];
    for w in b.windows(2) {
        widest = widest.max(w[1] - w[0]);
    }
    360.0 - widest
}

/// The fight with no shot at anything in sight, as one effort (`Combat::leave_the_lot`).
#[derive(Clone, Debug, Default)]
struct Lot {
    /// tics of it so far
    spent: i64,
    /// when last at it
    last: i64,
    /// the damage done as last looked at: more is progress
    dealt: f64,
    /// the lot's left till then
    till: i64,
    /// how many times running, nothing hurt between
    times: u32,
    /// who's left
    ids: BTreeSet<i32>,
}
