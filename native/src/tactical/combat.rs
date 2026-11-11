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
mod movement;
mod moves;
mod patience;
mod sight;
mod state;
mod steering;
mod style;
mod tree;

use super::bestiary::{missile, monster, weapon_in, Attack, Missile};
use super::sound::Hearing;
use super::tuning::{Dodge, Tuning};
use crate::decisions::{Journal, Record};
use crate::geom::{dist, Point};
use crate::los::{Body, LineOfSight};
use crate::motor::{
    heading, norm, Controls, Heard, Observation, Thing, FRICTION, RUN_THRUST, TICRATE,
};
use crate::navigator::{Navigator, PLAYER_HEIGHT, PLAYER_RADIUS};
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

/// a rocket's blast reaches this far (less her radius: 128 in all)
const SPLASH: f64 = 112.0;

/// a barrel's height (seen over a ledge this high or not)
const BARREL_HEIGHT: f64 = 42.0;

/// a missile's height, about (for whether she can see it)
const MISSILE_HEIGHT: f64 = 8.0;

const SLOTS: [i32; 7] = [1, 2, 3, 4, 5, 6, 7];
/// The chaingun fires twice a press: tics from its first bullet to its second (A_FireCGun's frames).
const CHAINGUN_SECOND: i64 = 4;

fn mon(name: &str) -> super::bestiary::Monster {
    monster(name).unwrap_or(super::bestiary::Monster {
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
    /// forced: whoever directs her)
    pub ordered_gun: Option<i32>,
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
    /// the missiles in flight she's noticed (seen as monsters are; her own always)
    pub missiles_seen: BTreeSet<i32>,
    /// monsters that only claw or charge, in her sight: since when, how far off then, and where
    /// she stood (for whether one can get at her: `cant_reach`)
    reach_watch: BTreeMap<i32, (i64, f64, Point)>,
    /// the ground she's been on, by the mesh's polys: where she knows is safe
    been: Vec<bool>,
    /// the fight's move last tic: for measuring, and for directed play (an order paused while
    /// the fight has her legs)
    pub last_move: Option<Move>,
    /// counts for measuring a run (`STATS`): nothing decides by them
    pub tally: BTreeMap<String, f64>,
    /// monsters she can see now
    pub visible: Vec<Thing>,
    /// barrels she's seen that are still there
    pub barrels_seen: Vec<Point>,
    /// a route under way (the player's): its follower has its own crusher rules
    pub route_under_way: bool,
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
    /// for the log: rounds spent this fight, and the health monsters lost as the engine has it.
    /// Nothing is decided by them.
    spent: f64,
    dealt: f64,
    ammo_was: Vec<(i32, i32)>,
    /// what she's put into each monster, as she'd reckon it: the rounds gone from the gun up while
    /// it was her target, at what such a shot does landing at that range (never its health)
    put_in: BTreeMap<i32, f64>,
    health_was: BTreeMap<i32, i32>,
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
            ordered_hold_fire: false,
            los: LineOfSight::new(&nav.map),
            known: BTreeMap::new(),
            senses: Senses {
                game_monsters: BTreeMap::new(),
                hearing: Hearing::new(&nav.map),
                cries_due: Vec::new(),
                attacks_heard: Vec::new(),
                alert: None,
                glanced: 0,
                checked: 0,
                looked: [i64::MIN / 2; 16],
            },
            dead: BTreeSet::new(),
            last_seen: BTreeMap::new(),
            missiles_seen: BTreeSet::new(),
            reach_watch: BTreeMap::new(),
            been: Vec::new(),
            nav,
            last_move: None,
            tally: BTreeMap::new(),
            visible: Vec::new(),
            barrels_seen: Vec::new(),
            route_under_way: false,
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
            spent: 0.0,
            dealt: 0.0,
            ammo_was: Vec::new(),
            put_in: BTreeMap::new(),
            health_was: BTreeMap::new(),
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

    // --- what's about -----------------------------------------------------------------------

    pub fn observe(&mut self, obs: &Observation) {
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

    /// Ceilings coming down and floors coming up, by sector, and when: what the crusher tests go
    /// by. Crushers are where a fight's moves don't go.
    fn watch_movers(&mut self, obs: &Observation) {
        if self.movers.ceilings.len() != obs.heights.len() {
            self.movers.ceilings = obs.heights.iter().map(|h| h.ceiling).collect();
            self.movers.ceiling_down = vec![i64::MIN / 2; obs.heights.len()];
            self.movers.crushed_low = vec![i64::MIN / 2; obs.heights.len()];
        }
        if self.movers.floors.len() != obs.heights.len() {
            self.movers.floors = obs.heights.iter().map(|h| h.floor).collect();
            self.movers.floor_up = vec![i64::MIN / 2; obs.heights.len()];
        }
        for (i, h) in obs.heights.iter().enumerate() {
            if h.ceiling < self.movers.ceilings[i] - 0.01 {
                self.movers.ceiling_down[i] = obs.tic;
                if h.ceiling - h.floor < PLAYER_HEIGHT {
                    self.movers.crushed_low[i] = obs.tic;
                }
            }
            self.movers.ceilings[i] = h.ceiling;
            if h.floor > self.movers.floors[i] + 0.01 {
                self.movers.floor_up[i] = obs.tic;
            }
            self.movers.floors[i] = h.floor;
        }
        for &s in self.movers.crushers.keys() {
            if !self.movers.crusher_out.contains_key(&s) {
                self.movers
                    .crusher_out
                    .insert(s, crate::crush::out_tics(&self.nav, &obs.heights, s));
            }
        }
    }

    /// Health lost since last tic: kept (for the rate she's losing it at) and logged with what
    /// was by her when it went.
    fn note_hurt(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if let Some(h) = obs.health {
            if let Some(before) = self.health.filter(|&before| h < before) {
                self.damage.push((obs.tic, before - h));
                // What by: what has gone off by her, what's up against her.
                let by: Vec<String> = obs
                    .things
                    .iter()
                    .flatten()
                    .filter(|t| {
                        (missile(t.name).is_some() && dist((t.x, t.y), pos) < 256.0)
                            || (monster(t.name).is_some() && dist((t.x, t.y), pos) < 128.0)
                    })
                    .map(|t| format!("{} {:.0} off", t.name, dist((t.x, t.y), pos)))
                    .collect();
                self.log.push(format!(
                    "tic {}: hurt {:.0}, health {h:.0} ({})",
                    obs.tic,
                    before - h,
                    if by.is_empty() {
                        "a shot".to_string()
                    } else {
                        by.join(", ")
                    }
                ));
            }
            self.health = Some(h);
        }
    }

    /// A monster gone from the game is dead. It is hers to know if she saw it go; else by its
    /// cry, if she hears it and can tell whose it was (`hear`); else it's still about, for all
    /// she knows, where she last saw it.
    fn note_deaths(&mut self, obs: &Observation) {
        let monsters: BTreeMap<i32, Thing> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| monster(t.name).is_some())
            .map(|t| (t.id, t.clone()))
            .collect();
        let was_visible: BTreeSet<i32> = self.visible.iter().map(|t| t.id).collect();
        for i in self.senses.game_monsters.keys() {
            if monsters.contains_key(i) || self.dead.contains(i) || !was_visible.contains(i) {
                continue;
            }
            if let Some(s) = self.known.get(i) {
                self.dead.insert(*i);
                self.kills += 1;
                self.senses.cries_due.push((obs.tic, s.name));
                *self.tally.entry("deaths seen".to_string()).or_insert(0.0) += 1.0;
                self.log
                    .push(format!("tic {}: {} {i} dead", obs.tic, s.name));
            }
        }
        self.senses.game_monsters = monsters;
    }

    /// The monsters in her sight this tic (`visible`), and what she keeps of each: where and
    /// how she last saw it, whether it's awake, whom it's after, its health as she reckons it.
    fn see_monsters(&mut self, obs: &Observation, me: Body) {
        let pos = obs.pos();
        self.visible.clear();
        for t in self.senses.game_monsters.values() {
            let d = dist((t.x, t.y), pos);
            if !(0.0..self.tune.engage.range).contains(&d) {
                continue;
            }
            let recent = self
                .known
                .get(&t.id)
                .is_some_and(|s| obs.tic - s.tic < self.tune.senses.recent_sight_tics);
            // In her view, or seen a moment ago in the corner of her eye. Not for being near: one behind
            // her that makes no sound she doesn't know of, till it does or she backs into it.
            if !(recent
                || d < self.tune.senses.see_near
                || norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < self.tune.senses.fov)
            {
                continue;
            }
            let target = Body {
                x: t.x,
                y: t.y,
                z: t.z,
                height: mon(t.name).height,
            };
            // Nothing between them; or, by instinct, something come between only a moment ago.
            let sees = self.los.clear(me, target, &self.heights_now);
            if !sees
                && !self
                    .known
                    .get(&t.id)
                    .is_some_and(|s| obs.tic - s.sighted <= self.tune.senses.instinct_tics)
            {
                continue;
            }
            let seen = self.known.entry(t.id).or_insert_with(|| {
                self.log.push(format!(
                    "tic {}: saw {} {} at ({:.0}, {:.0})",
                    obs.tic, t.name, t.id, t.x, t.y
                ));
                Seen {
                    id: t.id,
                    name: t.name,
                    x: t.x,
                    y: t.y,
                    z: t.z,
                    tic: obs.tic,
                    first: obs.tic,
                    x0: t.x,
                    y0: t.y,
                    angle0: t.angle,
                    awake: false,
                    lost: false,
                    going: (0.0, 0.0),
                    sighted: obs.tic,
                    foreknown: false,
                }
            });
            seen.x = t.x;
            seen.y = t.y;
            seen.z = t.z;
            seen.tic = obs.tic;
            seen.lost = false;
            seen.foreknown = false;
            if sees {
                seen.sighted = obs.tic;
            }
            if self.dead.remove(&t.id) {
                self.seen_hurt.remove(&t.id); // (raised again, whole: an arch-vile's work)
            }
            seen.awake = seen.awake
                || dist(seen.pos(), (seen.x0, seen.y0)) > self.tune.senses.wake_distance
                || t.vx.hypot(t.vy) > self.tune.senses.wake_speed
                || norm(t.angle - seen.angle0).abs() > self.tune.senses.wake_turn;
            let track = self.tracks.entry(t.id).or_default();
            if track
                .back()
                .is_some_and(|&(tic, _, _)| obs.tic - tic > self.tune.senses.track_tics)
            {
                track.clear(); // (out of sight a while: where it was then says nothing)
            }
            track.push_back((obs.tic, t.x, t.y));
            while track
                .front()
                .is_some_and(|&(tic, _, _)| obs.tic - tic > self.tune.senses.track_tics)
            {
                track.pop_front();
            }
            self.visible.push(t.clone());
        }
        // Each as she can know it. Its health is her reckoning (its kind's less what she's seen it
        // lose), never the game's own number. Whom it's after is what she can see of it: her, unless
        // it's turned to another monster in sight and not to her; never the game's own word for its
        // target.
        let facing = self.tune.senses.facing;
        let after_her: Vec<bool> = self
            .visible
            .iter()
            .map(|t| {
                let off = |p: Point| norm(heading((t.x, t.y), p) - t.angle).abs();
                off(pos) < facing
                    || !self
                        .visible
                        .iter()
                        .any(|o| o.id != t.id && off((o.x, o.y)) < facing)
            })
            .collect();
        for (t, after) in self.visible.iter_mut().zip(after_her) {
            t.after_her = after;
        }
        for t in &mut self.visible {
            // One going down she sees going down.
            if t.health > 0 {
                t.health =
                    (mon(t.name).health - self.seen_hurt.get(&t.id).copied().unwrap_or(0)).max(1);
            }
        }
        for t in &self.visible {
            self.last_seen.insert(t.id, t.clone());
        }
        let going: Vec<(i32, Point)> = self
            .visible
            .iter()
            .map(|t| (t.id, self.moving(t)))
            .collect();
        for (id, v) in going {
            if let Some(s) = self.known.get_mut(&id) {
                s.going = v;
            }
        }
        // One not where she last saw it, with the place in her sight again and a while gone by: she
        // doesn't know where it is.
        let in_sight: BTreeSet<i32> = self.visible.iter().map(|t| t.id).collect();
        let looked: Vec<i32> = self
            .known
            .values()
            .filter(|s| {
                !s.lost
                    && !in_sight.contains(&s.id)
                    && !self.dead.contains(&s.id)
                    && obs.tic - s.tic > self.tune.engage.lost_tics
            })
            .filter(|s| {
                let d = dist(s.pos(), pos);
                d < self.tune.engage.range
                    && norm(heading(pos, s.pos()) - obs.angle).abs() < self.tune.senses.fov
                    && self.los.clear(me, sight::eye_of(s), &self.heights_now)
            })
            .map(|s| s.id)
            .collect();
        for id in looked {
            if let Some(s) = self.known.get_mut(&id) {
                s.lost = true;
                self.log.push(format!(
                    "tic {}: {} {id} isn't where she last saw it",
                    obs.tic, s.name
                ));
                *self.tally.entry("lost".to_string()).or_insert(0.0) += 1.0;
            }
        }
    }

    /// Missiles: noticed in her view with nothing in the way, then followed while they fly. A
    /// missile in flight makes no sound, only its launch, which turns her to look. Her own she
    /// knows of.
    fn see_missiles(&mut self, obs: &Observation, me: Body) {
        let pos = obs.pos();
        let flying: BTreeMap<i32, &Thing> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| missile(t.name).is_some() && monster(t.name).is_none())
            .map(|t| (t.id, t))
            .collect();
        self.missiles_seen.retain(|i| flying.contains_key(i));
        for (&id, t) in &flying {
            if self.missiles_seen.contains(&id) {
                continue;
            }
            let d = dist((t.x, t.y), pos);
            let noticed = t.owner == 0
                || (d < self.tune.engage.range
                    && norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < self.tune.senses.fov
                    && self.los.clear(
                        me,
                        Body {
                            x: t.x,
                            y: t.y,
                            z: t.z,
                            height: MISSILE_HEIGHT,
                        },
                        &self.heights_now,
                    ));
            if noticed {
                self.missiles_seen.insert(id);
            }
        }
    }

    /// Barrels are seen as monsters are: in her view (or near), nothing in the way; kept while
    /// they're there.
    fn see_barrels(&mut self, obs: &Observation, me: Body) {
        let pos = obs.pos();
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        // The same barrel pushed by shots is still the one she saw.
        self.barrels_seen = self
            .barrels_seen
            .iter()
            .filter_map(|&b| crate::motor::follower::barrel_now(obs, b))
            .collect();
        self.barrels_seen.dedup_by(|a, b| dist(*a, *b) < 1.0);
        for &b in &barrels {
            let d = dist(b, pos);
            if d >= self.tune.engage.range || self.barrels_seen.iter().any(|&x| dist(x, b) < 1.0) {
                continue;
            }
            if !(d < self.tune.senses.near
                || norm(heading(pos, b) - obs.angle).abs() < self.tune.senses.fov)
            {
                continue;
            }
            let floor = self.nav.poly_at(b).map_or(0.0, |p| {
                self.heights_now
                    .get(self.nav.sector_of_poly(p))
                    .map_or(0.0, |h| h.0)
            });
            if self.los.clear(
                me,
                Body {
                    x: b.0,
                    y: b.1,
                    z: floor,
                    height: BARREL_HEIGHT,
                },
                &self.heights_now,
            ) {
                self.barrels_seen.push(b);
            }
        }
    }

    /// What only claws or charges, awake and in her sight, is watched for whether it gets any
    /// nearer while she stays put (`cant_reach`); the watch begins again when she moves off.
    fn watch_reach(&mut self, obs: &Observation) {
        let pos = obs.pos();
        let watched: Vec<(i32, f64)> = self
            .visible
            .iter()
            .filter(|t| {
                matches!(mon(t.name).attack, Attack::Melee | Attack::Charge)
                    && self.known.get(&t.id).is_some_and(|k| k.awake)
            })
            .map(|t| (t.id, dist((t.x, t.y), pos)))
            .collect();
        self.reach_watch
            .retain(|id, w| watched.iter().any(|(i, _)| i == id) && dist(w.2, pos) < 128.0);
        for (id, d) in watched {
            self.reach_watch.entry(id).or_insert((obs.tic, d, pos));
        }
    }

    /// The ground she's on counts as been on, from now; and the ways round her that are in her
    /// view this tic are noted as looked along.
    fn note_ground_and_view(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if self.been.is_empty() {
            self.been = vec![false; self.nav.mesh.polys.len()];
        }
        if let Some(p) = self.nav.poly_at(pos) {
            self.been[p] = true;
        }
        // The ways round her that are in her view this tic.
        for (i, at) in self.senses.looked.iter_mut().enumerate() {
            if norm(22.5 * i as f64 - obs.angle).abs() < self.tune.senses.fov {
                *at = obs.tic;
            }
        }
    }

    /// The rounds gone from her guns since last tic. A bullet or a shell gone is a shot out:
    /// when, and from which pool, is kept (`shot_out`, and `burst_from` for the pistol's and
    /// the chaingun's presses), since the trigger's timing goes by it. The rest is counting,
    /// for the log and the run's statistics: rounds spent, shots, and shots with nothing on
    /// her line. Pools shared by two guns are counted once.
    fn note_rounds_gone(&mut self, obs: &Observation, ammo: &[(i32, i32)]) {
        let pos = obs.pos();
        for &(s, n) in ammo {
            if matches!(s, 4 | 7) {
                continue;
            }
            if let Some(&(_, before)) = self.ammo_was.iter().find(|(x, _)| *x == s) {
                if n < before {
                    self.spent += f64::from(before - n);
                    *self.tally.entry("rounds spent".to_string()).or_insert(0.0) +=
                        f64::from(before - n);
                    // A bullet or shell gone with nothing it could hit on her line: into a wall, or the corner
                    // she'd stepped back behind since the trigger.
                    if matches!(s, 2 | 3) {
                        // The chaingun's second bullet, 4 tics on, is of the same press.
                        let second = obs.weapon == Some(4)
                            && self
                                .trigger
                                .shot_out
                                .is_some_and(|(p, t)| p == s && obs.tic - t <= CHAINGUN_SECOND);
                        if s == 2 && !second {
                            self.trigger.burst_from = obs.tic;
                        }
                        if crate::debug::on().shot {
                            let gap = self.trigger.shot_out.map_or(0, |(_, t)| obs.tic - t);
                            self.log.push(format!(
                                "tic {}: a shot out, gun {:?}, {gap} tics after the last",
                                obs.tic, obs.weapon
                            ));
                        }
                        self.trigger.shot_out = Some((s, obs.tic));
                        let seen = self.visible.clone();
                        let on = seen.iter().any(|t| {
                            let d = dist((t.x, t.y), pos).max(1.0);
                            let wide = (mon(t.name).radius / d).atan().to_degrees()
                                + aiming::AUTOAIM_SPREAD;
                            norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < wide
                                && self.shot_clear(obs, (t.x, t.y), t.z, mon(t.name).height)
                        });
                        *self
                            .tally
                            .entry("shots (bullets, shells)".to_string())
                            .or_insert(0.0) += 1.0;
                        if !on {
                            if crate::debug::on().shot {
                                let what: Vec<String> = seen
                                    .iter()
                                    .map(|t| {
                                        let clear = self.shot_clear(
                                            obs,
                                            (t.x, t.y),
                                            t.z,
                                            mon(t.name).height,
                                        );
                                        format!(
                                            "{} {} {:.0} off, {:.1} degrees off her aim, {}",
                                            t.name,
                                            t.id,
                                            dist((t.x, t.y), pos),
                                            norm(heading(pos, (t.x, t.y)) - obs.angle),
                                            if clear { "clear" } else { "not clear" }
                                        )
                                    })
                                    .collect();
                                self.log.push(format!("tic {}: a shot (slot {s}) with nothing on her line; target {:?}, pressed {:?}, the move {:?}; in sight: {}", obs.tic, self.target, self.trigger.last_shot, self.last_move, what.join("; ")));
                            }
                            *self
                                .tally
                                .entry("shots with nothing on her line".to_string())
                                .or_insert(0.0) += 1.0;
                            let why = if seen.is_empty() {
                                "nothing in sight"
                            } else {
                                "something in sight"
                            };
                            *self
                                .tally
                                .entry(format!(
                                    "shots with nothing on her line: {why}, the move {:?}",
                                    self.last_move
                                ))
                                .or_insert(0.0) += 1.0;
                        }
                    }
                }
            }
        }
    }

    /// What has gone from the gun in her hands is put down to her target, as she'd reckon it:
    /// so many shots, at what such a shot does landing at that range (`put_in`).
    fn reckon_put_in(&mut self, obs: &Observation, ammo: &[(i32, i32)]) {
        let pos = obs.pos();
        if let (Some(slot), Some(id)) = (obs.weapon, self.target) {
            let was = self
                .ammo_was
                .iter()
                .find(|(x, _)| *x == slot)
                .map(|&(_, n)| n);
            let now = ammo.iter().find(|(x, _)| *x == slot).map(|&(_, n)| n);
            if let (Some(was), Some(now)) = (was, now) {
                if let Some(t) = obs
                    .things
                    .iter()
                    .flatten()
                    .find(|t| t.id == id)
                    .filter(|_| now < was)
                {
                    if let Some(m) = monster(t.name) {
                        let w = weapon_in(slot, obs);
                        let shots = f64::from(was - now) / f64::from(w.ammo_per_shot.max(1));
                        *self.put_in.entry(id).or_insert(0.0) +=
                            shots * w.damage_on(&m) * w.hits(m.radius, dist((t.x, t.y), pos));
                    }
                }
            }
        }
    }

    /// The health monsters have lost since last tic. What one in her sight lost is what she
    /// saw it lose (`seen_hurt`), whoever hurt it. The rest is counting: damage dealt, and
    /// shots that hurt nothing with a monster in sight.
    fn note_health_lost(&mut self, obs: &Observation) {
        let pos = obs.pos();
        let mut now: BTreeMap<i32, i32> = BTreeMap::new();
        let mut hurt_any = false;
        for t in obs
            .things
            .iter()
            .flatten()
            .filter(|t| monster(t.name).is_some())
        {
            if let Some(&h) = self.health_was.get(&t.id) {
                // Seen: in her sight the tic before, the hit landing in front of her.
                if t.health < h && self.visible.iter().any(|v| v.id == t.id) {
                    *self.seen_hurt.entry(t.id).or_insert(0) += h - t.health.max(0);
                }
                if t.health < h {
                    hurt_any = true;
                    self.dealt += f64::from(h - t.health.max(0));
                    *self.tally.entry("damage dealt".to_string()).or_insert(0.0) +=
                        f64::from(h - t.health.max(0));
                }
            }
            now.insert(t.id, t.health);
        }
        // One killed is gone from the list, and that counts as hurt.
        hurt_any |= self
            .health_was
            .keys()
            .any(|id| !now.contains_key(id) && self.visible.iter().any(|v| v.id == *id));
        // A bullet or a shell gone this tic, a monster in sight, and nothing hurt by it.
        if self.trigger.shot_out.is_some_and(|(_, t)| t == obs.tic)
            && !self.visible.is_empty()
            && !hurt_any
        {
            *self
                .tally
                .entry("shots that hurt nothing, a monster in sight".to_string())
                .or_insert(0.0) += 1.0;
            *self
                .tally
                .entry(format!(
                    "shots that hurt nothing, a monster in sight: the move {:?}",
                    self.last_move
                ))
                .or_insert(0.0) += 1.0;
            let t = self
                .target
                .and_then(|id| self.visible.iter().find(|t| t.id == id).cloned());
            let how = match &t {
                None => "no target in sight".to_string(),
                Some(t) => {
                    let d = dist((t.x, t.y), pos);
                    let off = norm(heading(pos, (t.x, t.y)) - obs.angle).abs();
                    let wide = (mon(t.name).radius / d.max(1.0)).atan().to_degrees();
                    let clear = self.shot_clear(obs, (t.x, t.y), t.z, mon(t.name).height);
                    format!(
                        "{}, {}, {}",
                        if off < wide {
                            "aimed on it"
                        } else if off < wide + aiming::AUTOAIM_SPREAD {
                            "aimed near it"
                        } else {
                            "aimed off it"
                        },
                        if clear { "the line clear" } else { "no line" },
                        if d < 256.0 {
                            "under 256"
                        } else if d < 640.0 {
                            "256-640"
                        } else {
                            "over 640"
                        }
                    )
                }
            };
            *self
                .tally
                .entry(format!(
                    "shots that hurt nothing, gun {}: {how}",
                    obs.weapon.unwrap_or(0)
                ))
                .or_insert(0.0) += 1.0;
        }
        // Gone: killed, and what it had left was the last of it.
        self.dealt += self
            .health_was
            .iter()
            .filter(|(id, _)| !now.contains_key(id))
            .map(|(_, &h)| f64::from(h.max(0)))
            .sum::<f64>();
        self.health_was = now;
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

    /// What she makes of this tic's sounds (the game's, as she'd hear them: `Observation::sounds`).
    /// A death cry: if it's the cry of a death she just saw, that; else one of the kinds that
    /// cry so has died, about that far off on that side: the one she knows of, out of sight,
    /// last seen where the cry fits is taken to be it (none fits: she can't tell whose). An
    /// attack: kept a moment, for whether something out of sight is firing.
    fn hear(&mut self, obs: &Observation) {
        let (tune, pos) = (Rc::clone(&self.tune), obs.pos());
        self.senses
            .cries_due
            .retain(|&(tic, _)| obs.tic - tic <= tune.senses.cry_due);
        self.senses
            .attacks_heard
            .retain(|&(tic, _)| obs.tic - tic <= tune.senses.heard_firing);
        for s in &obs.sounds {
            match s.what {
                Heard::Attack => self.senses.attacks_heard.push((obs.tic, s.kinds)),
                // A monster's own steps. The few kinds that make any are heard all the while they walk, a
                // step at a time, and say about where it is: as far off as the step is loud, on the side it
                // came from. The one she knows of, of that kind, out of her sight, is put there: of the two
                // places the sound could be (in front and behind sound the same), the one nearer where she
                // had it.
                Heard::Walking if !s.own && self.tune.senses.track_steps => {
                    let far = if s.volume >= 127 {
                        160.0
                    } else {
                        1200.0 - f64::from(s.volume) * 1040.0 / 127.0
                    };
                    let off = ((128.0 - f64::from(s.separation)) / 96.0)
                        .clamp(-1.0, 1.0)
                        .asin()
                        .to_degrees();
                    let at = |way: f64| {
                        (
                            pos.0 + far * way.to_radians().cos(),
                            pos.1 + far * way.to_radians().sin(),
                        )
                    };
                    let places = [at(obs.angle + off), at(obs.angle + 180.0 - off)];
                    let whose = self
                        .known
                        .values()
                        .filter(|k| {
                            s.kinds.contains(&k.name)
                                && !self.dead.contains(&k.id)
                                && !self.visible.iter().any(|t| t.id == k.id)
                        })
                        .flat_map(|k| places.iter().map(move |&p| (dist(p, k.pos()), k.id, p)))
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    if let Some((_, id, p)) = whose {
                        if let Some(k) = self.known.get_mut(&id) {
                            (k.x, k.y, k.tic, k.lost, k.awake, k.going) =
                                (p.0, p.1, obs.tic, false, true, (0.0, 0.0));
                        }
                        *self.tally.entry("steps heard".to_string()).or_insert(0.0) += 1.0;
                    }
                }
                Heard::Death => {
                    if let Some(i) = self
                        .senses
                        .cries_due
                        .iter()
                        .position(|(_, kind)| s.kinds.contains(kind))
                    {
                        self.senses.cries_due.remove(i);
                        continue;
                    }
                    // How far off by how loud: full within 160, nothing at 1200. A boss's cry is as loud
                    // anywhere and says nothing of where.
                    let far = if s.volume >= 127 {
                        160.0
                    } else {
                        1200.0 - f64::from(s.volume) * 1040.0 / 127.0
                    };
                    let fits = |k: &Seen| {
                        if s.own {
                            return Some(0.0);
                        }
                        let d = dist(k.pos(), pos);
                        let off_by = if s.volume >= 127 {
                            (d - far).max(0.0)
                        } else {
                            (d - far).abs()
                        };
                        let side = 128.0
                            - 96.0 * norm(heading(pos, k.pos()) - obs.angle).to_radians().sin();
                        let side_off = (side - f64::from(s.separation)).abs();
                        (off_by <= tune.senses.heard_fit && side_off <= tune.senses.heard_side)
                            .then_some(
                                off_by / tune.senses.heard_fit + side_off / tune.senses.heard_side,
                            )
                    };
                    let whose = self
                        .known
                        .values()
                        .filter(|k| {
                            s.kinds.contains(&k.name)
                                && !self.dead.contains(&k.id)
                                && !self.visible.iter().any(|t| t.id == k.id)
                        })
                        .filter_map(|k| fits(k).map(|f| (f, k.id, k.name)))
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    if let Some((_, id, name)) = whose {
                        self.dead.insert(id);
                        self.kills += 1;
                        self.log
                            .push(format!("tic {}: {name} {id} dead (heard)", obs.tic));
                        self.count("deaths heard", 1.0);
                    }
                }
                _ => {}
            }
        }
    }

    /// Something to look at, `ways` round (world degrees, the likeliest first).
    fn alerted(&mut self, obs: &Observation, ways: Vec<f64>, hit: bool, why: &str) {
        // One thing at a time. What she's looking round for is looked round for before the next,
        // its ways reckoned from where she faced when it came: a second sound reckoned from mid-turn
        // would have her turning to and fro and never round. Only a hit takes her from a sound.
        if ways.is_empty() || self.senses.alert.as_ref().is_some_and(|a| a.hit || !hit) {
            return;
        }
        let side = |w: f64| match norm(w - obs.angle) {
            d if d.abs() > 135.0 => "behind her",
            d if d > 0.0 => "to her left",
            _ => "to her right",
        };
        self.log
            .push(format!("tic {}: {why}: looking {}", obs.tic, side(ways[0])));
        self.count(&format!("alert: {why}"), 1.0);
        self.journal.note(|| Record {
            detail: why.to_string(),
            ..Record::new(obs.tic, "alert", side(ways[0]).to_string())
        });
        self.senses.alert = Some(Alert {
            until: obs.tic + self.tune.senses.alert_tics,
            ways,
            hit,
            in_sight: self.visible.iter().map(|t| t.id).collect(),
        });
    }

    /// What tells her to look round, this tic. A sound from nothing she can see (a missile
    /// fired, a monster's cry on seeing her, an attack): its side by the balance between her
    /// ears, which is the same for in front and behind, so in front only if that's out of her
    /// view. A hit: the way the status bar's face looked, to one side (straight at it, she's
    /// looking there). Not a wandering monster's noise: there's always one.
    fn heed(&mut self, obs: &Observation) {
        let fov = self.tune.senses.fov;
        let (heed, heed_active) = (
            self.tune.senses.heed_volume,
            self.tune.senses.heed_active_volume,
        );
        let loud = |s: &crate::motor::Sound| match s.what {
            Heard::Launch | Heard::Sight | Heard::Attack => s.volume >= heed,
            Heard::Active => s.volume >= heed_active,
            _ => false,
        };
        for s in obs.sounds.iter().filter(|s| !s.own && loud(s)) {
            let seen = if s.what == Heard::Launch {
                obs.things
                    .iter()
                    .flatten()
                    .any(|t| self.missiles_seen.contains(&t.id) && s.kinds.contains(&t.name))
            } else {
                self.visible.iter().any(|t| s.kinds.contains(&t.name))
            };
            if seen {
                continue;
            }
            // The balance: 128 less 96 times the sine of its bearing, left of her counted up.
            let off = ((128.0 - f64::from(s.separation)) / 96.0)
                .clamp(-1.0, 1.0)
                .asin()
                .to_degrees();
            let (front, back) = (obs.angle + off, obs.angle + 180.0 - off);
            let ways = if off.abs() < fov {
                vec![back]
            } else {
                vec![front, back]
            };
            let what = match s.what {
                Heard::Launch => "a shot fired",
                Heard::Sight => "something's seen her",
                Heard::Active => "something close",
                _ => "an attack",
            };
            self.alerted(obs, ways, false, &format!("heard {what}"));
        }
        for &side in obs.hit_sides.iter().filter(|&&side| side != 0) {
            // Her right is the lesser angle. The face's side is anywhere from 45 degrees off to right
            // behind: to that side first, then on round behind.
            let way = obs.angle - 90.0 * f64::from(side);
            let known = self.visible.iter().any(|t| {
                let rel = norm(heading(obs.pos(), (t.x, t.y)) - obs.angle);
                rel.abs() > 45.0 && (rel < 0.0) == (side > 0)
            });
            if !known {
                self.alerted(obs, vec![way, obs.angle + 180.0], true, "hit from the side");
            }
        }
    }

    /// Walking a route with nothing to shoot and nothing to look at: a look to either side every
    /// so often, so that what comes up beside her as she travels is seen before it's on her.
    pub(super) fn glance(&mut self, obs: &Observation) {
        let every = self.tune.senses.glance_every;
        if every <= 0
            || !self.route_under_way
            || self.senses.alert.is_some()
            || obs.tic - self.senses.glanced < every
        {
            return;
        }
        self.senses.glanced = obs.tic;
        let by = self.tune.senses.glance_angle;
        self.count("glances", 1.0);
        self.senses.alert = Some(Alert {
            until: obs.tic + self.tune.senses.alert_tics,
            ways: vec![obs.angle + by, obs.angle - by],
            hit: false,
            in_sight: self.visible.iter().map(|t| t.id).collect(),
        });
    }

    /// One she remembers, close, not looked at for a while: round to where she last saw it.
    fn check(&mut self, obs: &Observation) {
        let (every, within) = (self.tune.senses.check_every, self.tune.senses.check_within);
        if every <= 0 || self.senses.alert.is_some() || obs.tic - self.senses.checked < every {
            return;
        }
        let pos = obs.pos();
        let Some(way) = self
            .now
            .remembered
            .iter()
            .map(|t| (dist((t.x, t.y), pos), t))
            .filter(|&(d, _)| d < within)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, t)| heading(pos, (t.x, t.y)))
        else {
            return;
        };
        self.senses.checked = obs.tic;
        self.count("checks", 1.0);
        self.senses.alert = Some(Alert {
            until: obs.tic + self.tune.senses.alert_tics,
            ways: vec![way],
            hit: true,
            in_sight: self.visible.iter().map(|t| t.id).collect(),
        });
    }

    /// For measuring a run: was she going towards the monster with that id, away from it, or
    /// about still, by her own speed along the line to it.
    pub fn measure_approach(&self, pos: Point, id: i32) -> &'static str {
        let Some(t) = self.senses.game_monsters.get(&id) else {
            return "gone";
        };
        let d = dist((t.x, t.y), pos).max(1.0);
        let along = (self.velocity.0 * (t.x - pos.0) + self.velocity.1 * (t.y - pos.1)) / d;
        match along {
            a if a > 2.0 => "going towards it",
            a if a < -2.0 => "going away from it",
            _ => "about still",
        }
    }

    /// Stopped while pushing `way` (world degrees) where the map has room: something's there.
    pub fn bumped(&mut self, obs: &Observation, way: f64) {
        let d = (way.to_radians().cos(), way.to_radians().sin());
        let reach = 3.0 * PLAYER_RADIUS;
        if norm(way - obs.angle).abs() > self.tune.senses.fov
            && self.clear_run(obs, d, reach) >= reach - 1.0
        {
            self.alerted(obs, vec![way], false, "stopped against something");
        }
    }

    /// The way to turn and look, if there's something to look at (`hit_only`: only for a hit:
    /// she has something to shoot). Looked each way, or something new in sight, or time up:
    /// over.
    fn look(&mut self, obs: &Observation, hit_only: bool) -> Option<f64> {
        let looked = self.tune.senses.looked;
        let a = self.senses.alert.as_mut()?;
        while a
            .ways
            .first()
            .is_some_and(|&w| norm(w - obs.angle).abs() < looked)
        {
            a.ways.remove(0);
        }
        if obs.tic >= a.until
            || a.ways.is_empty()
            || self.visible.iter().any(|t| !a.in_sight.contains(&t.id))
        {
            self.senses.alert = None;
            return None;
        }
        (a.hit || !hit_only).then(|| a.ways[0])
    }

    /// Where a monster out of her sight is, as she'd reckon it: where she last saw it, and on
    /// the way it was going for as long as `senses.reckon_tics` allows.
    pub(super) fn reckoned(&self, s: &Seen, tic: i64) -> Point {
        let dt = (tic - s.tic).clamp(0, self.tune.senses.reckon_tics) as f64;
        (s.x + s.going.0 * dt, s.y + s.going.1 * dt)
    }

    /// The monsters she can't see now but has in mind: seen awake within `senses.memory_tics`,
    /// not known dead, not found gone from where she left them; each as she last saw it, where
    /// she last saw it, doing nothing she knows of.
    pub(super) fn remembered(&self, obs: &Observation) -> Vec<Thing> {
        self.known
            .values()
            .filter(|s| {
                s.awake
                    && !s.lost
                    && !self.dead.contains(&s.id)
                    && obs.tic - s.tic <= self.tune.senses.memory_tics
                    && mon(s.name).attack != Attack::None
            })
            .filter(|s| !self.visible.iter().any(|t| t.id == s.id))
            .filter_map(|s| {
                self.last_seen
                    .get(&s.id)
                    .map(|t| (self.reckoned(s, obs.tic), t))
            })
            .map(|(at, t)| Thing {
                x: at.0,
                y: at.1,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
                attack: -1,
                ..t.clone()
            })
            .collect()
    }

    /// A monster that only claws or charges, in her sight a while with her staying where she is,
    /// and no nearer for it: it can't get at her (bars she sees through, a gap, a ledge). Not
    /// something to spend shots on while anything else is about.
    pub(super) fn cant_reach(&self, obs: &Observation, t: &Thing) -> bool {
        self.reach_watch.get(&t.id).is_some_and(|&(since, far, _)| {
            obs.tic - since > self.tune.engage.cant_reach_tics
                && dist((t.x, t.y), obs.pos()) > far - 64.0
                && dist((t.x, t.y), obs.pos()) > self.tune.engage.close
        })
    }

    /// Is p off the ground she's been on (a poly of the mesh she's never stood in, or none)?
    pub(super) fn off_known(&self, p: Point) -> bool {
        self.nav
            .poly_at(p)
            .is_none_or(|q| !self.been.get(q).copied().unwrap_or(false))
    }

    /// Is the route about to take her onto ground she's never been on (the way it walked her
    /// last tic, a few steps on)?
    pub(super) fn route_into_new(&self, obs: &Observation) -> bool {
        self.route_way.is_some_and(|w| {
            let (pos, w) = (obs.pos(), w.to_radians());
            [48.0, 96.0]
                .into_iter()
                .any(|far| self.off_known((pos.0 + w.cos() * far, pos.1 + w.sin() * far)))
        })
    }

    /// The monsters about her, awake, that are on ground she's never been on.
    pub fn woken_ahead(&self) -> Vec<Thing> {
        self.now
            .about
            .iter()
            .filter(|t| {
                self.known.get(&t.id).is_some_and(|k| k.awake) && self.off_known((t.x, t.y))
            })
            .cloned()
            .collect()
    }

    /// How many monsters she first saw within the last `tics`.
    pub fn newly_seen(&self, tic: i64, tics: i64) -> usize {
        self.known
            .values()
            .filter(|s| tic - s.first <= tics && !self.dead.contains(&s.id))
            .count()
    }

    /// For measuring a run: add `by` to the count called `key`.
    pub fn count(&mut self, key: &str, by: f64) {
        *self.tally.entry(key.to_string()).or_insert(0.0) += by;
    }

    /// For measuring a run, never for deciding: what she knew of the monster with that id when
    /// it hit her ("in view", "remembered", "known" of but out of mind, "unknown"), and how far
    /// off it was (near: within 384).
    pub fn measure(&self, pos: Point, id: i32) -> (&'static str, &'static str) {
        let knew = if self.visible.iter().any(|t| t.id == id) {
            "in view"
        } else if self.now.remembered.iter().any(|t| t.id == id) {
            "remembered"
        } else if self.known.contains_key(&id) {
            "known"
        } else {
            "unknown"
        };
        let far = match self.senses.game_monsters.get(&id) {
            Some(t) if dist((t.x, t.y), pos) < 384.0 => "near",
            Some(_) => "far",
            None => "gone",
        };
        (knew, far)
    }

    /// The level known beforehand: every monster in it where it stands at the start, as if seen
    /// in a life before (`obs`: the game's own, the first of the level).
    pub fn know_monsters(&mut self, obs: &Observation) {
        for t in obs
            .things
            .iter()
            .flatten()
            .filter(|t| monster(t.name).is_some())
        {
            // One she's met before too: where it stands, not where she first saw it.
            self.known.insert(
                t.id,
                Seen {
                    id: t.id,
                    name: t.name,
                    x: t.x,
                    y: t.y,
                    z: t.z,
                    tic: i64::MIN / 2,
                    first: i64::MIN / 2,
                    x0: t.x,
                    y0: t.y,
                    angle0: t.angle,
                    awake: false,
                    lost: false,
                    going: (0.0, 0.0),
                    sighted: i64::MIN / 2,
                    foreknown: true,
                },
            );
        }
    }

    /// Every monster she knows of, forgotten (for measuring what knowing them is worth).
    pub fn forget_monsters(&mut self) {
        self.known.clear();
    }

    /// The counts kept for measuring a run, from nothing (what went before isn't the run's).
    pub fn forget_counts(&mut self) {
        self.tally.clear();
        (self.kills, self.spent, self.dealt) = (0, 0.0, 0.0);
    }

    /// How many monsters she knows of.
    pub fn known_count(&self) -> usize {
        self.known.len()
    }

    /// The monsters she knows of from before this life and hasn't seen in it yet, within `range`
    /// of her: what she expects there.
    pub fn expected(&self, pos: Point, range: f64) -> Vec<&Seen> {
        self.known
            .values()
            .filter(|s| {
                s.foreknown && !s.lost && !self.dead.contains(&s.id) && dist(s.pos(), pos) < range
            })
            .collect()
    }

    /// Is that monster dead, as far as she knows (seen or heard to die)?
    pub fn known_dead(&self, id: i32) -> bool {
        self.dead.contains(&id)
    }

    /// Is a monster she knows of firing now, as far as she can tell: seen at it, or its kind's
    /// attack heard a moment ago (a chaingun's heard through walls, within the game's 1200)?
    pub fn firing(&self, obs: &Observation, s: &Seen) -> bool {
        match self.visible.iter().find(|t| t.id == s.id) {
            Some(t) => super::bestiary::first_shot(t.name).is_some_and(|f| t.attack >= f),
            None => self.senses.attacks_heard.iter().any(|&(tic, kinds)| {
                obs.tic - tic <= self.tune.senses.heard_firing && kinds.contains(&s.name)
            }),
        }
    }

    /// The monsters she knows are dead: (id, kind, where, when last in her sight).
    pub fn known_deaths(&self) -> Vec<(i32, &'static str, Point, i64)> {
        self.dead
            .iter()
            .filter_map(|i| self.known.get(i))
            .map(|s| (s.id, s.name, s.pos(), s.sighted))
            .collect()
    }

    /// Monsters seen that are still about, as far as she knows, and where she last saw them (to
    /// hunt down, to keep clear of): not one she knows is dead, nor one she's seen isn't where
    /// she left it.
    pub fn remaining(&self) -> Vec<Seen> {
        self.known
            .iter()
            .filter(|(i, s)| !self.dead.contains(i) && !s.lost)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// The monsters she's in a fight with: in sight, awake, after her, and able to hurt her from
    /// where they are (one with a gun or missiles within `engaged_ranged` of her, a lost soul
    /// within `engaged_charge`, a clawing one within `engaged_melee`). This is what pauses her
    /// exploring: not anything awake in sight, such as a zombieman across the map or a demon
    /// over a pit.
    pub fn engaged(&self, obs: &Observation) -> Vec<Thing> {
        let pos = obs.pos();
        self.seen_against_her()
            .into_iter()
            .filter(|t| {
                t.health > 0 && t.after_her && self.known.get(&t.id).is_some_and(|k| k.awake)
            })
            .filter(|t| {
                let d = dist((t.x, t.y), pos);
                match mon(t.name).attack {
                    Attack::Hitscan | Attack::Missile | Attack::Fire => {
                        d < self.tune.engage.engaged_ranged
                    }
                    Attack::Charge => d < self.tune.engage.engaged_charge,
                    Attack::Melee => d < self.tune.engage.engaged_melee,
                    Attack::None => false,
                }
            })
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
        if let Some(h) = self.rocket_way_out(obs) {
            return Some((h, true));
        }
        self.trigger.rocket_facing.map(|f| (f, false))
    }

    /// What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
    /// something that wants the controls (a run-up, a jump). `errand`: the route's going for
    /// something to fight with: the fight only aims and shoots on the way.
    pub fn decide(&mut self, obs: &Observation, busy: bool, errand: &str) -> Option<Decision> {
        // Invulnerable: nothing to dodge, a missile does nothing.
        let missiles = if self.now.invulnerable {
            Vec::new()
        } else {
            self.incoming(obs)
        };
        let target = self.pick_target(obs);
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
        let (s, fire, weapon) = self.size_up(obs, busy, errand, missiles, target);
        let mut act = tree::engage(self, obs, &s);
        // A rocket on its way out, 8 tics after the trigger, the way she faces then: if that way it
        // would go off near her, she turns to the nearest way it won't.
        if s.vile.is_none() {
            if let Some((aim, out)) = self.rocket_held(obs) {
                act.aim = Some(aim);
                if out {
                    act.kind = Move::RocketTurnedOff;
                    act.why = "turned off a rocket's way out into something near".into();
                }
            }
        }
        // Hit from where she isn't looking: round to it, whatever she was shooting, and with
        // missiles in the air too. A dodge is her legs'; her eyes are free.
        if s.vile.is_none() {
            if let Some(way) = self.look(obs, true) {
                act.aim = Some(way);
                act.hold_fire = true;
            }
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
        if obs.tic % 175 == 0 && crate::debug::on().shot {
            if let Some(t) = &s.target {
                self.log.push(format!("tic {}: (she's at ({:.0}, {:.0}, {:.0}); {} {} at ({:.0}, {:.0}, {:.0}), {:.0} off; the move {kind:?}, going {:?}; gun {:?}, ammo {:?}, keys {:?})", obs.tic, obs.x, obs.y, self.z(obs), t.name, t.id, t.x, t.y, t.z, dist((t.x, t.y), obs.pos()), act.moving.map(|m| (m.0.round(), m.1.round())), obs.weapon, obs.ammo, obs.keys));
            }
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
            if let Some(shot) = self.barrel_shot(obs) {
                if matches!(obs.weapon, Some(2..=4)) {
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
        }
        if let Some(t) = target.as_ref().filter(|t| self.reachable(obs, t)) {
            self.patience.rest(Effort::Closing, t.id); // (got it in reach: the clock again, next time)
        }
        let close: Vec<Thing> = self
            .now
            .about
            .iter()
            .filter(|t| {
                let m = mon(t.name);
                // Not one that can't get at her: nothing to back off from.
                (matches!(m.attack, Attack::Melee | Attack::Charge | Attack::Fire) || m.melee)
                    && dist((t.x, t.y), pos) < self.tune.engage.close + m.radius
                    && !self.cant_reach(obs, t)
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
        if first_missile.is_none() && !busy && (shooting || target.is_some()) {
            if let Some(b) = barrel.filter(|&b| dist(b, pos) < self.tune.barrels.keep_clear + 64.0)
            {
                barrel_away = self.away_from_barrels(obs, b);
            }
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

    /// Give up on getting at id a while.
    pub(super) fn give_up(&mut self, obs: &Observation, id: i32) {
        self.patience
            .give_up(id, obs.tic + self.tune.engage.given_up_tics);
        self.log.push(format!(
            "tic {}: can't get at {id}: leaving it a while",
            obs.tic
        ));
    }

    /// Closing in on t, out of reach: still worth it? Not once she has been at it this long
    /// without getting it in reach: then it's left a while.
    fn still_closing(&mut self, obs: &Observation, id: i32) -> bool {
        if self.given_up_on(obs, id) {
            return false;
        }
        if self.patience.spent(
            Effort::Closing,
            id,
            obs.tic,
            self.tune.engage.close_give_up,
            None,
            None,
        ) {
            self.give_up(obs, id);
            return false;
        }
        true
    }

    /// Standing to fight t with no shot at it from here: given up on a while once she has had
    /// none for `engage.close_give_up` (a shot at it sets the clock back; so does a fight with
    /// it broken off that long). Standing gets her no shot, so the route goes on, shooting on
    /// the way when one comes.
    pub(super) fn no_shot_long(&mut self, obs: &Observation, t: &Thing) -> bool {
        if self.given_up_on(obs, t.id) {
            return true;
        }
        if self.can_hit(obs, t, None) {
            self.patience.rest(Effort::NoShot, t.id);
            return false;
        }
        let limit = self.tune.engage.close_give_up;
        if self
            .patience
            .spent(Effort::NoShot, t.id, obs.tic, limit, None, Some(limit))
        {
            self.give_up(obs, t.id);
            return true;
        }
        false
    }

    /// An awake monster that was after her and went out of sight a moment ago (behind a pillar,
    /// round a corner): she keeps fighting it, aimed where it was and keeping her distance,
    /// rather than going about her business and walking up to it. Not on a floor that hurts (or
    /// one that will, a suit's time running out): the route's way off is the way to take then,
    /// not after a ghost.
    fn lost_sight(&mut self, obs: &Observation) -> Option<Decision> {
        // Nor when nothing can hurt her: one gone from sight is nothing to keep at.
        if self.on_hurting_floor(obs) || self.immune {
            return None;
        }
        let pos = obs.pos();
        let s = self
            .remaining()
            .into_iter()
            // Gone a moment, not a tic: one on the edge of a line of sight is in it and out of it tic
            // about as she moves a unit, and would pass her legs to and fro between this and the route.
            // A moment by her eyes, not her ears: a step heard says where it is and dates it, but isn't
            // sight of it, and every step of one walking about out of sight would be a fresh "just
            // lost".
            .filter(|s| {
                let eyes = obs.tic
                    - s.tic
                        .min(s.sighted.saturating_add(self.tune.senses.instinct_tics));
                s.awake
                    && eyes >= self.tune.engage.lost_after
                    && eyes < self.tune.engage.lost_tics
                    && mon(s.name).attack != Attack::None
                    && dist(s.pos(), pos) < self.tune.engage.range
            })
            .filter(|s| !self.patience.given_up(s.id, obs.tic))
            // Not after one she can't take on: out of its sight is where she wants to be.
            .filter(|s| !self.too_much(obs, &[s.name], self.tune.guns.outmatched))
            .max_by(|a, b| mon(a.name).threat.total_cmp(&mon(b.name).threat))?;
        let mut ghost = self.last_seen.get(&s.id)?.clone();
        let at = self.reckoned(&s, obs.tic);
        (ghost.x, ghost.y, ghost.z, ghost.vx, ghost.vy) = (at.0, at.1, s.z, 0.0, 0.0);
        let gun = self.preferred_gun(obs, &ghost);
        // (stepped about as for one in sight, and that's kept: holding still for it, as a player
        // waits at a corner, cost her on the score, -0.028 (2026-11-11): what comes back round
        // the corner hits one who's standing)
        let moving = self.steer(obs, Some(&ghost), &gun);
        let why = format!("lost sight of {}: keeping at it", s.name);
        self.announce(obs, &why);
        Some(Decision::without_shot(
            Move::LostSight,
            Some(heading(pos, at)),
            moving,
            why,
        ))
    }

    /// The route's controls this tic (the way it walks her, for what's in her way).
    pub fn route_walks(&mut self, obs: &Observation, b: &Controls) {
        self.route_way = (b.forward.hypot(b.strafe) > 0.0)
            .then(|| obs.angle - b.turn + (-b.strafe).atan2(b.forward).to_degrees());
    }

    /// The controls for a decision. `legs`: the fight's move has her legs (the arbiter's call);
    /// else `base`'s do (a route's, the explorer's or a retreat's), and only the aim and the
    /// trigger are the fight's. `base` is given with the fight's legs too when a route's
    /// walking under it, for what's in her way.
    pub fn controls(
        &mut self,
        obs: &Observation,
        d: &Decision,
        base: Option<&Controls>,
        legs: bool,
    ) -> Controls {
        let turn = self.turn(obs, d.aim);
        if let Some(b) = base {
            self.route_walks(obs, b);
        }
        let mut c = match base {
            Some(b) if !legs => {
                let mut c = Controls {
                    forward: b.forward,
                    strafe: b.strafe,
                    turn,
                    use_: b.use_,
                    run: b.run,
                    weapon: b.weapon,
                    ..Controls::default()
                };
                // Nothing to shoot with: aiming fires nothing, and the route's turn is what faces a shut
                // door on her way to press it.
                if d.aim.is_none() || !self.armed(obs) || d.kind.route_faces() {
                    c.turn = b.turn;
                } else {
                    // The route's way in the world, whichever way she's turned to aim.
                    let way = obs.angle - b.turn + (-b.strafe).atan2(b.forward).to_degrees();
                    let rel = norm(way - (obs.angle - turn)).to_radians();
                    let mag = b.forward.hypot(b.strafe);
                    c.forward = mag * rel.cos();
                    c.strafe = -mag * rel.sin();
                }
                c
            }
            _ => {
                let mut c = Controls {
                    turn,
                    ..Controls::default()
                };
                let facing = obs.angle - turn; // (thrust goes the way faced after this tic's turn)
                                               // A turn about soon after the last one: she stops a moment instead, and goes
                                               // when the moment's past. To and fro inside a second is a jitter no player's
                                               // legs make; getting out of the way of something isn't held up for it.
                let mag = d.moving.map_or(0.0, |m| m.0.hypot(m.1)).max(1e-6);
                let way = d.moving.map(|m| (m.0 / mag, m.1 / mag));
                // (against the way she last went: a turn about if that was just now, or if she's
                // been held from it since)
                let since_turn = obs.tic - self.steer.legs_turned;
                let about = way.zip(self.steer.legs_way).is_some_and(|(m, (w, at))| {
                    m.0 * w.0 + m.1 * w.1 < -0.5
                        && (obs.tic - at <= DART_GAP || since_turn < self.tune.steer.dart_tics)
                });
                let held = about
                    && !d.urgent
                    && !d.kind.escaping()
                    && since_turn < self.tune.steer.dart_tics;
                if about && !held {
                    self.steer.legs_turned = obs.tic;
                }
                if let Some(w) = way.filter(|_| !held) {
                    self.steer.legs_way = Some((w, obs.tic));
                }
                if held {
                    (c.forward, c.strafe) = crate::motor::brake(obs, facing);
                } else if let Some(m) = d.moving {
                    let rel = norm(m.1.atan2(m.0).to_degrees() - facing).to_radians();
                    c.forward = 50.0 * rel.cos();
                    c.strafe = -50.0 * rel.sin();
                } else if self.coasting_off(obs) {
                    // Nowhere the fight wants her: not where the speed she has would take her either.
                    (c.forward, c.strafe) = crate::motor::brake(obs, facing);
                }
                c
            }
        };
        c.attack = d.fire || base.is_some_and(|b| b.attack && !legs);
        // The rocket launcher and the BFG don't fire again with the trigger held: it is let go a tic
        // after each press, which is the game's way to fire the next.
        if matches!(obs.weapon, Some(5 | 7)) && self.trigger.held {
            c.attack = false;
        }
        self.keep_line(obs, d, &mut c, legs || base.is_none());
        if d.fire {
            self.tap(obs, &mut c);
        }
        self.trigger.held = c.attack;
        // A shot begun, that is: the trigger pulled on a gun with too little in it begins none, and
        // must not be taken for one.
        if let Some(s) = obs
            .weapon
            .filter(|&s| c.attack && Self::ammo(obs, s) >= weapon_in(s, obs).ammo_per_shot)
        {
            self.trigger.last_shot = Some((s, obs.tic));
            if s == 5 {
                self.trigger.rocket_facing = Some(obs.angle);
            }
        }
        if d.weapon != 0 {
            c.weapon = d.weapon;
        }
        if crate::debug::on().pace && obs.tic % 10 == 0 {
            eprintln!("  fight tic {}: {:?} legs {legs}, base {:?}, out ({:.1}, {:.1}) turn {:.1}, facing {:.1}", obs.tic, d.kind, base.map(|b| (b.forward, b.strafe, b.turn)), c.forward, c.strafe, c.turn, obs.angle);
        }
        c
    }

    /// A bullet or a shell goes from where she is when it leaves the gun, a few tics after the
    /// trigger (the pistol's 4, the shotguns' 3: the frames before A_FirePistol, A_FireShotgun),
    /// not from where she pressed it. So the trigger isn't pulled when the speed she has would
    /// have her, by then, where there's no line to what she's shooting at. Her speed, not her
    /// legs: she can't stop in three tics, and not at all in the air coming down a stair. Once
    /// it's pulled, her legs are held those tics if where they're taking her has no line and
    /// where she stands has. Between shots her legs are her own: the rest of the gun's cycle is
    /// the time to get behind something. None of this applies when the move is one that can't
    /// wait (a missile coming, an arch-vile's fire).
    fn keep_line(&mut self, obs: &Observation, d: &Decision, c: &mut Controls, legs: bool) {
        /// longer than this with no shot out, the gun isn't coming up to fire (a switch, say)
        const GIVEN_UP: i64 = 12;
        let Some(held) = obs.weapon.filter(|w| matches!(w, 2..=4) && !d.urgent) else {
            self.trigger.line_held = None;
            return;
        };
        if held == 4 {
            self.trigger.line_held = None;
            self.keep_burst(obs, c, legs);
            return;
        }
        let delay: i64 = if held == 2 { 4 } else { 3 };
        let since = self
            .trigger
            .shot_out
            .filter(|&(s, _)| s == held)
            .map_or(i64::MAX / 2, |(_, t)| obs.tic - t);
        let pressed = c.attack
            || self
                .trigger
                .last_shot
                .is_some_and(|(s, t)| s == held && obs.tic - t < delay);
        // The trigger held, the next shot leaves a cycle after the last.
        let again = weapon_in(held, obs).cycle as i64 - delay;
        let due = pressed && since >= again - 1;
        let target = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id).cloned());
        let (Some(t), true) = (target, due) else {
            self.trigger.line_held = None;
            return;
        };
        let pos = obs.pos();
        let reach = delay as f64;
        let h = mon(t.name).height;
        if !self.shot_clear(obs, (t.x, t.y), t.z, h) {
            self.trigger.line_held = None;
            return;
        }
        // The trigger not pulled yet, or held and the gun asking whether to fire again.
        let coast = (
            pos.0 + self.velocity.0 * reach,
            pos.1 + self.velocity.1 * reach,
        );
        if c.attack
            && d.fire
            && (!self.trigger.held || since <= again + 1)
            && !self.shot_clear_from(obs, coast, (t.x, t.y), t.z, h)
        {
            c.attack = false;
            *self
                .tally
                .entry("triggers not pulled: no line where she'd be by the shot".to_string())
                .or_insert(0.0) += 1.0;
            // And she slows for it: with a line to it and a mind to shoot, she stays in the line till
            // she can. Otherwise she is out and back round a corner with never a shot.
            if legs {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                *self
                    .tally
                    .entry("tics she slowed to get a shot off".to_string())
                    .or_insert(0.0) += 1.0;
            }
            self.trigger.line_held = None;
            return;
        }
        if !legs || c.forward.hypot(c.strafe) == 0.0 {
            self.trigger.line_held = None;
            return;
        }
        const STRIDE: f64 = 10.0; // (a tic's run, about, from a walk)
        let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
        let spot = (
            pos.0 + way.cos() * STRIDE * reach,
            pos.1 + way.sin() * STRIDE * reach,
        );
        if self.shot_clear_from(obs, spot, (t.x, t.y), t.z, h) {
            self.trigger.line_held = None;
            return;
        }
        let from = *self.trigger.line_held.get_or_insert(obs.tic);
        if obs.tic - from > GIVEN_UP {
            return;
        }
        (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
        *self
            .tally
            .entry("tics her legs held for a shot on its way out".to_string())
            .or_insert(0.0) += 1.0;
    }

    /// The trigger let go between shots at something far off. A bullet goes dead straight
    /// unless the trigger has been held since the shot before (P_GunShot's `accurate`: the
    /// player's `refire` is 0); held, it's up to 5.6 degrees off either way, most near the
    /// middle ((P_Random - P_Random) << 18). The gun asks once a cycle whether the trigger is
    /// still down (A_ReFire: the pistol 10 tics after its shot, the chaingun 8 after the first
    /// of its two). Let go for that tic, it comes back to ready and the next press is a first
    /// shot again: the pistol's cycle is then 19 tics for 14, the chaingun's 10 for 8. So she
    /// taps when holding would land less than that share of its bullets on a target as wide as
    /// this one is from here: the same damage a second or more, and every bullet on the target.
    fn tap(&mut self, obs: &Observation, c: &mut Controls) {
        /// the most a held shot goes off, degrees
        const SPREAD: f64 = 5.6;
        // Tics from the press's first bullet to the gun's asking; a held cycle over a tapped one.
        let (asks, worth) = match obs.weapon {
            Some(2) => (10, 14.0 / 19.0),
            Some(4) => (8, 8.0 / 10.0),
            _ => return,
        };
        if !c.attack {
            return;
        }
        let Some(t) = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id))
        else {
            return;
        };
        let half = (mon(t.name).radius / dist((t.x, t.y), obs.pos()).max(1.0))
            .atan()
            .to_degrees();
        let lands = if half >= SPREAD {
            1.0
        } else {
            1.0 - (1.0 - half / SPREAD).powi(2)
        };
        if lands >= worth {
            return;
        }
        // She sees the bullet gone a tic after the gun has let it go, and what she does now is the
        // game's next tic.
        let since = obs.tic - self.trigger.burst_from;
        if (asks - 2..=asks).contains(&since) {
            c.attack = false;
            *self
                .tally
                .entry("tics the trigger let go for a straight shot".to_string())
                .or_insert(0.0) += 1.0;
        }
    }

    /// The chaingun's way of the same. It fires as long as the trigger is down, two bullets a
    /// press, the second 4 tics after the first whether she has let go or not (A_FireCGun,
    /// twice). So she fires a burst at a time. Where her speed would have her out of the line by
    /// the burst's end, the trigger isn't pulled and she slows for it. Once pulled, her legs
    /// wait the burst out if they'd take her out of the line, and no second burst is begun.
    /// After that they're her own for a while (`REST`), with the trigger let go where they take
    /// her out of the line: without that rest the legs wait for the shot and the shot for the
    /// legs, and she does neither.
    fn keep_burst(&mut self, obs: &Observation, c: &mut Controls, legs: bool) {
        const BURST: i64 = 5;
        /// her legs are her own this long after a burst they waited for (tics)
        const REST: i64 = 12;
        let burst = self
            .trigger
            .last_shot
            .is_some_and(|(s, t)| s == 4 && obs.tic - t < BURST);
        if !c.attack && !burst {
            return;
        }
        let Some(t) = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id).cloned())
        else {
            return;
        };
        let pos = obs.pos();
        let h = mon(t.name).height;
        if !self.shot_clear(obs, (t.x, t.y), t.z, h) {
            return;
        }
        let reach = BURST as f64;
        let coast = (
            pos.0 + self.velocity.0 * reach,
            pos.1 + self.velocity.1 * reach,
        );
        let out_coast = !self.shot_clear_from(obs, coast, (t.x, t.y), t.z, h);
        let out_legs = legs && c.forward.hypot(c.strafe) > 0.0 && {
            const STRIDE: f64 = 10.0;
            let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
            !self.shot_clear_from(
                obs,
                (
                    pos.0 + way.cos() * STRIDE * reach,
                    pos.1 + way.sin() * STRIDE * reach,
                ),
                (t.x, t.y),
                t.z,
                h,
            )
        };
        if !out_coast && !out_legs {
            return;
        }
        if obs.tic < self.trigger.burst_rest {
            c.attack = false;
            return;
        }
        if burst {
            // The burst is begun: no other after it, her legs waiting it out.
            c.attack = false;
            if legs {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                *self
                    .tally
                    .entry("tics her legs held for a shot on its way out".to_string())
                    .or_insert(0.0) += 1.0;
            }
            if self
                .trigger
                .last_shot
                .is_some_and(|(_, t)| obs.tic - t == BURST - 1)
            {
                self.trigger.burst_rest = obs.tic + 1 + REST;
            }
            return;
        }
        if out_coast {
            c.attack = false;
            *self
                .tally
                .entry("triggers not pulled: no line where she'd be by the shot".to_string())
                .or_insert(0.0) += 1.0;
        }
        if legs {
            (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            *self
                .tally
                .entry("tics she slowed to get a shot off".to_string())
                .or_insert(0.0) += 1.0;
        }
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
            crate::knowledge::RISK_SHARE * obs.health.unwrap_or(100.0),
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

    /// Health lost a second lately (over the last few seconds): `now.damage_rate`.
    fn damage_rate(&mut self, tic: i64) -> f64 {
        self.damage
            .retain(|&(t, _)| tic - t < self.tune.senses.damage_window);
        self.damage.iter().map(|(_, d)| d).sum::<f64>()
            / (self.tune.senses.damage_window as f64 / TICRATE)
    }

    /// Could a hitscan shooter she knows of, awake, from where she last saw it, see her at p? (Its
    /// bullets reach whatever it sees, however far.)
    pub fn shot_from(&mut self, obs: &Observation, s: &Seen, p: Point) -> bool {
        if !s.awake || !self.sniper(obs, s.name) {
            return false;
        }
        let floor = self.floor_at(obs, p);
        let heights = self.heights(obs).to_vec();
        self.sees(eye_of(s), p, floor, &heights)
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

pub fn segment_point_distance(a: Point, b: Point, p: Point) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let n = dx * dx + dy * dy;
    let u = if n == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / n).clamp(0.0, 1.0)
    };
    dist(p, (a.0 + dx * u, a.1 + dy * u))
}
