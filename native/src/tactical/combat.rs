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
mod steering;
mod style;
mod tree;

pub use arsenal::Odds;
pub use assess::Assessment;
pub use moves::Move;
pub use sight::{eye, eye_of};
use moves::HeldFire;
use patience::{Effort, Patience};
use super::bestiary::{missile, monster, weapon_in, Attack, Missile};
use super::sound::Hearing;
use super::tuning::{Dodge, Tuning};
use crate::decisions::{Journal, Record};
use crate::geom::{dist, Point};
use crate::specials::Category;
use crate::los::{Body, LineOfSight};
use crate::motor::{heading, norm, Controls, Observation, Thing, FRICTION, RUN_THRUST, TICRATE};
use crate::navigator::{Navigator, PLAYER_HEIGHT, PLAYER_RADIUS};
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

fn mon(name: &str) -> super::bestiary::Monster {
    monster(name).unwrap_or(super::bestiary::Monster { radius: 20.0, height: 56.0, health: 100, attack: Attack::None, melee: false, threat: 0.0, boss: false })
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
    pub yields: bool,
    /// a missile coming, or an arch-vile's fire: not walked on through
    pub pressed: bool,
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
    /// Does this move want her legs (`busy`: in a jump or a timed crossing, the route's unless
    /// she's pressed)? Whether it gets them is the arbiter's (`tactical/arbiter.rs`).
    pub fn wants_legs(&self, busy: bool) -> bool {
        !self.yields && ((!busy && !self.kind.route_goes_on()) || self.pressed)
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
    pub los: LineOfSight,
    hearing: Hearing,
    /// every monster seen, by id
    pub known: BTreeMap<i32, Seen>,
    /// every monster about now (as the engine has it)
    pub alive: BTreeMap<i32, Thing>,
    /// the missiles in flight she's noticed (seen as monsters are; her own always)
    pub missiles_seen: BTreeSet<i32>,
    /// monsters she can see now
    pub visible: Vec<Thing>,
    /// barrels she's seen that are still there
    pub barrels_seen: Vec<Point>,
    /// a route under way (the player's): its follower has its own crusher rules
    pub routed: bool,
    pub target: Option<i32>,
    pub kills: i32,
    pub log: Vec<String>,
    health: Option<f64>,
    hurt_tic: i64,
    switch_tic: i64,
    strafe: f64,
    strafe_until: i64,
    rate: f64,
    mode: String,
    held_fire: Option<HeldFire>,
    /// the monster a rocket's way at was last blocked close by (whatever's up), and when: not the
    /// launcher for it for a while after (the user's: a safer weapon, rather than a rocket held;
    /// a while, not each tic it's clear: E2M8, the cyberdemon, 24 switches back and forth)
    rocket_blocked: Option<(i32, i64)>,
    /// (baiting: the monsters in the way are the point)
    pub hold_fire: bool,
    /// monster -> tics it's looked to be after another
    infight: BTreeMap<i32, i32>,
    heights_tic: Option<i64>,
    heights_now: Vec<(f64, f64)>,
    hit_cache: BTreeMap<(i32, u64), bool>,
    /// missiles being dodged (till they've gone by)
    dodging: BTreeSet<i32>,
    /// homing missiles being dodged: since when; and where out of the sight of the one being
    /// shaken off she's going
    homing_since: BTreeMap<i32, i64>,
    shake_spot: Option<(i32, Option<Point>)>,
    dodge_way: Option<Point>,
    /// does that way take her out of every missile's way (its hit; a blast may still reach her)?
    dodge_clear: bool,
    /// where the route's taking her mid-fight (an errand: health, a gun, ammo), for dodging
    /// towards (the user's: on to it while dodging, not stuck dodging where she stands)
    pub toward: Option<Point>,
    /// the floors that would hurt her now, as she takes them, by sector (what she knows: the
    /// player's, each tic; `Knowledge::hazards`)
    pub hazards: Vec<bool>,
    /// floors a walked-over line lowers into something that hurts (going down: as good as hurting)
    sinks_into_harm: BTreeSet<usize>,
    /// sectors only doors move: a door closing on her goes back up, or rests on her, and does
    /// no damage (vanilla T_VerticalDoor), so its ceiling coming down is no crusher
    doors: BTreeSet<usize>,
    /// sectors only lifts move: a lift coming up on her goes back down (vanilla T_PlatRaise
    /// reverses a plat that doesn't crush, and none of the lifts do), so its floor coming up is no
    /// crusher either
    lifts: BTreeSet<usize>,
    last_pos: Option<Point>,
    velocity: Point,
    /// the way the route last walked her (in the world, whichever way she was turned to aim;
    /// kept while the fight holds the route): what's in her way is what's that way, not where
    /// she's facing (None: the route standing her still)
    route_way: Option<f64>,
    barrel_way: Option<Point>,
    barrel_since: i64,
    barrel_rest_until: i64,
    /// the way fighting moved her last
    steer_way: Option<Point>,
    steer_tic: i64,
    steer_still: i64,
    /// a way she got nowhere pushing, and till when it's left be
    steer_stuck: Option<(Point, i64)>,
    /// running from an arch-vile's fire: when the way was chosen, from which, the way
    vile_way: Option<(i64, i32, Point)>,
    /// ...and till when its fire might still go off
    vile_until: i64,
    /// ducking from a hitscan shooter's fire: from which, to where
    duck_way: Option<(i32, &'static str, Point)>,
    /// ...and till when it might still be firing
    duck_until: i64,
    /// when there was last nowhere to duck from one
    no_duck_tic: i64,
    /// invulnerable till this tic (from the pickup's message: 30 s, as a player knows it)
    invulnerable_until: i64,
    /// where each monster in sight has been lately: (tic, x, y), oldest first
    tracks: BTreeMap<i32, std::collections::VecDeque<(i64, f64, f64)>>,
    /// the trigger was down last tic
    trigger_held: bool,
    /// health lost lately: (tic, how much)
    damage: Vec<(i64, f64)>,
    /// (the log's: rounds spent this fight, and the health monsters lost, as the engine has it;
    /// nothing's decided by them)
    spent: f64,
    dealt: f64,
    ammo_was: Vec<(i32, i32)>,
    /// what she's put into each monster, as she'd reckon it: the rounds gone from the gun up while
    /// it was her target, at what such a shot does landing at that range (never its health)
    put_in: BTreeMap<i32, f64>,
    health_was: BTreeMap<i32, i32>,
    /// each sector's ceiling last tic, and when it was last seen coming down
    ceilings: Vec<f64>,
    /// each sector's floor last tic, and when it was last seen coming up
    floors: Vec<f64>,
    floor_up: Vec<i64>,
    ceiling_down: Vec<i64>,
    /// crusher sectors she knows of, by kind (their lines learned; kept from what she knows by
    /// the player)
    pub crushers: BTreeMap<usize, crate::specials::CrusherKind>,
    /// per crusher sector: tics to be out of it at the worst (`out_tics`)
    crusher_out: BTreeMap<usize, f64>,
    /// ...and last came down lower than she stands (a crusher at work, between strokes too)
    crushed_low: Vec<i64>,
    /// how long she's been at each monster she's getting nowhere with, and those given up on
    /// (see `patience.rs`)
    pub(super) patience: Patience,
    /// the sector each stood in when closing in on it to hit it in melee got her no nearer
    pub(super) fist_stalled: BTreeMap<i32, usize>,
    /// the gun last fired and when (its reload)
    pub(super) last_shot: Option<(i32, i64)>,
    /// the way she faced as she last pressed the rocket launcher's trigger (the rocket leaves along
    /// the way she faces 8 tics later)
    rocket_facing: Option<f64>,
}

impl Combat {
    pub fn new(nav: Rc<Navigator>, tune: Rc<Tuning>) -> Combat {
        let sinks_into_harm = (0..nav.map.sectors.len())
            .filter(|&s| nav.triggers.iter().any(|t| t.info.trigger.walked() && t.effect(s).is_some() && nav.floor_after(t, s) == Some(true)))
            .collect();
        let moved_only_by = |only: fn(Category) -> bool| -> BTreeSet<usize> {
            (0..nav.map.sectors.len())
                .filter(|&s| {
                    let mut movers = nav.triggers.iter().filter(|t| t.sectors.contains(&s)).peekable();
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
            doors,
            lifts,
            los: LineOfSight::new(&nav.map),
            hearing: Hearing::new(&nav.map),
            nav,
            known: BTreeMap::new(),
            alive: BTreeMap::new(),
            missiles_seen: BTreeSet::new(),
            visible: Vec::new(),
            barrels_seen: Vec::new(),
            routed: false,
            target: None,
            kills: 0,
            log: Vec::new(),
            health: None,
            hurt_tic: -1_000_000_000,
            switch_tic: -1_000_000_000,
            strafe: 1.0,
            strafe_until: 0,
            rate: 0.0,
            mode: String::new(),
            held_fire: None,
            rocket_blocked: None,
            hold_fire: false,
            infight: BTreeMap::new(),
            heights_tic: None,
            heights_now: Vec::new(),
            hit_cache: BTreeMap::new(),
            dodging: BTreeSet::new(),
            homing_since: BTreeMap::new(),
            shake_spot: None,
            dodge_way: None,
            dodge_clear: false,
            toward: None,
            hazards: Vec::new(),
            sinks_into_harm,
            last_pos: None,
            velocity: (0.0, 0.0),
            route_way: None,
            barrel_way: None,
            barrel_since: 0,
            barrel_rest_until: -1_000_000_000,
            steer_way: None,
            steer_tic: -1,
            steer_still: 0,
            steer_stuck: None,
            vile_way: None,
            vile_until: 0,
            duck_way: None,
            duck_until: 0,
            no_duck_tic: i64::MIN / 2,
            tracks: BTreeMap::new(),
            invulnerable_until: i64::MIN / 2,
            trigger_held: false,
            damage: Vec::new(),
            spent: 0.0,
            dealt: 0.0,
            ammo_was: Vec::new(),
            put_in: BTreeMap::new(),
            health_was: BTreeMap::new(),
            ceilings: Vec::new(),
            floors: Vec::new(),
            floor_up: Vec::new(),
            ceiling_down: Vec::new(),
            crushers: BTreeMap::new(),
            crusher_out: BTreeMap::new(),
            crushed_low: Vec::new(),
            patience: Patience::default(),
            fist_stalled: BTreeMap::new(),
            last_shot: None,
            rocket_facing: None,
        }
    }

    /// A new life: the level from the start (what's been seen is remembered).
    pub fn restart(&mut self) {
        self.alive.clear();
        self.missiles_seen.clear();
        self.visible.clear();
        self.now.nothing_in_sight();
        self.tracks.clear();
        self.put_in.clear();
        self.invulnerable_until = i64::MIN / 2;
        self.target = None;
        self.health = None;
        self.hurt_tic = -1_000_000_000;
        self.mode.clear();
        self.dodging.clear();
        self.homing_since.clear();
        self.shake_spot = None;
        self.last_pos = None;
        self.velocity = (0.0, 0.0);
        self.route_way = None;
    }

    // --- what's about -----------------------------------------------------------------------

    pub fn observe(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if obs.message.as_deref().is_some_and(|m| m.starts_with("Invulnerability")) {
            self.invulnerable_until = obs.tic + INVULNERABLE_TICS;
        }
        if let Some(v) = obs.velocity {
            self.velocity = v;
        } else if let Some(l) = self.last_pos {
            self.velocity = (pos.0 - l.0, pos.1 - l.1);
        }
        self.last_pos = Some(pos);
        // (for the log: rounds gone, health monsters lost; pools shared by two guns counted once)
        let ammo: Vec<(i32, i32)> = obs.ammo.clone().unwrap_or_default();
        for &(s, n) in &ammo {
            if matches!(s, 4 | 7) {
                continue;
            }
            if let Some(&(_, before)) = self.ammo_was.iter().find(|(x, _)| *x == s) {
                if n < before {
                    self.spent += f64::from(before - n);
                }
            }
        }
        // (what's gone from the gun up, put into her target, as she'd reckon it)
        if let (Some(slot), Some(id)) = (obs.weapon, self.target) {
            let was = self.ammo_was.iter().find(|(x, _)| *x == slot).map(|&(_, n)| n);
            let now = ammo.iter().find(|(x, _)| *x == slot).map(|&(_, n)| n);
            if let (Some(was), Some(now)) = (was, now) {
                if let Some(t) = obs.things.iter().flatten().find(|t| t.id == id).filter(|_| now < was) {
                    if let Some(m) = monster(t.name) {
                        let w = weapon_in(slot, obs);
                        let shots = f64::from(was - now) / f64::from(w.ammo_per_shot.max(1));
                        *self.put_in.entry(id).or_insert(0.0) += shots * w.damage_on(&m) * w.hits(m.radius, dist((t.x, t.y), pos));
                    }
                }
            }
        }
        self.ammo_was = ammo;
        let mut now: BTreeMap<i32, i32> = BTreeMap::new();
        for t in obs.things.iter().flatten().filter(|t| monster(t.name).is_some()) {
            if let Some(&h) = self.health_was.get(&t.id) {
                if t.health < h {
                    self.dealt += f64::from(h - t.health.max(0));
                }
            }
            now.insert(t.id, t.health);
        }
        // (gone: killed, what it had left the last of it)
        self.dealt += self.health_was.iter().filter(|(id, _)| !now.contains_key(id)).map(|(_, &h)| f64::from(h.max(0))).sum::<f64>();
        self.health_was = now;
        // (ceilings coming down: crushers, where a fight's moves don't go)
        if self.ceilings.len() != obs.heights.len() {
            self.ceilings = obs.heights.iter().map(|h| h.ceiling).collect();
            self.ceiling_down = vec![i64::MIN / 2; obs.heights.len()];
            self.crushed_low = vec![i64::MIN / 2; obs.heights.len()];
        }
        if self.floors.len() != obs.heights.len() {
            self.floors = obs.heights.iter().map(|h| h.floor).collect();
            self.floor_up = vec![i64::MIN / 2; obs.heights.len()];
        }
        for (i, h) in obs.heights.iter().enumerate() {
            if h.ceiling < self.ceilings[i] - 0.01 {
                self.ceiling_down[i] = obs.tic;
                if h.ceiling - h.floor < PLAYER_HEIGHT {
                    self.crushed_low[i] = obs.tic;
                }
            }
            self.ceilings[i] = h.ceiling;
            if h.floor > self.floors[i] + 0.01 {
                self.floor_up[i] = obs.tic;
            }
            self.floors[i] = h.floor;
        }
        for &s in self.crushers.keys() {
            if !self.crusher_out.contains_key(&s) {
                self.crusher_out.insert(s, crate::crush::out_tics(&self.nav, &obs.heights, s));
            }
        }
        if let Some(h) = obs.health {
            if let Some(before) = self.health.filter(|&before| h < before) {
                self.hurt_tic = obs.tic;
                self.damage.push((obs.tic, before - h));
                // (what by: what's gone off by her, what's up against her)
                let by: Vec<String> = obs
                    .things
                    .iter()
                    .flatten()
                    .filter(|t| (missile(t.name).is_some() && dist((t.x, t.y), pos) < 256.0) || (monster(t.name).is_some() && dist((t.x, t.y), pos) < 128.0))
                    .map(|t| format!("{} {:.0} off", t.name, dist((t.x, t.y), pos)))
                    .collect();
                self.log.push(format!("tic {}: hurt {:.0}, health {h:.0} ({})", obs.tic, before - h, if by.is_empty() { "a shot".to_string() } else { by.join(", ") }));
            }
            self.health = Some(h);
        }
        let monsters: BTreeMap<i32, Thing> =
            obs.things.iter().flatten().filter(|t| monster(t.name).is_some()).map(|t| (t.id, t.clone())).collect();
        for i in self.alive.keys() {
            if !monsters.contains_key(i) {
                if let Some(s) = self.known.get(i) {
                    self.kills += 1;
                    self.log.push(format!("tic {}: {} {i} dead", obs.tic, s.name));
                }
            }
        }
        self.alive = monsters;
        self.refresh_tic_cache(obs);
        let me = Body { x: obs.x, y: obs.y, z: self.z(obs), height: PLAYER_HEIGHT };
        let hurt = obs.tic - self.hurt_tic < self.tune.senses.hurt_tics;
        self.visible.clear();
        for t in self.alive.values() {
            let d = dist((t.x, t.y), pos);
            if !(0.0..self.tune.engage.range).contains(&d) {
                continue;
            }
            let recent = self.known.get(&t.id).is_some_and(|s| obs.tic - s.tic < self.tune.senses.recent_sight_tics);
            if !(hurt || recent || d < self.tune.senses.near || norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < self.tune.senses.fov) {
                continue;
            }
            let target = Body { x: t.x, y: t.y, z: t.z, height: mon(t.name).height };
            if !self.los.clear(me, target, &self.heights_now) {
                continue;
            }
            let seen = self.known.entry(t.id).or_insert_with(|| {
                self.log.push(format!("tic {}: saw {} {} at ({:.0}, {:.0})", obs.tic, t.name, t.id, t.x, t.y));
                Seen { id: t.id, name: t.name, x: t.x, y: t.y, z: t.z, tic: obs.tic, first: obs.tic, x0: t.x, y0: t.y, angle0: t.angle, awake: false }
            });
            seen.x = t.x;
            seen.y = t.y;
            seen.z = t.z;
            seen.tic = obs.tic;
            seen.awake = seen.awake
                || dist(seen.pos(), (seen.x0, seen.y0)) > self.tune.senses.wake_distance
                || t.vx.hypot(t.vy) > self.tune.senses.wake_speed
                || norm(t.angle - seen.angle0).abs() > self.tune.senses.wake_turn;
            let track = self.tracks.entry(t.id).or_default();
            if track.back().is_some_and(|&(tic, _, _)| obs.tic - tic > self.tune.senses.track_tics) {
                track.clear(); // (out of sight a while: where it was then says nothing)
            }
            track.push_back((obs.tic, t.x, t.y));
            while track.front().is_some_and(|&(tic, _, _)| obs.tic - tic > self.tune.senses.track_tics) {
                track.pop_front();
            }
            self.visible.push(t.clone());
        }
        // (missiles: noticed as monsters are, in her view or near, nothing in the way; then
        // followed while they fly. Her own she knows of)
        let flying: BTreeMap<i32, &Thing> = obs.things.iter().flatten().filter(|t| missile(t.name).is_some() && monster(t.name).is_none()).map(|t| (t.id, t)).collect();
        self.missiles_seen.retain(|i| flying.contains_key(i));
        for (&id, t) in &flying {
            if self.missiles_seen.contains(&id) {
                continue;
            }
            let d = dist((t.x, t.y), pos);
            let noticed = t.owner == 0
                || (d < self.tune.engage.range
                    && (hurt || d < self.tune.senses.near || norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < self.tune.senses.fov)
                    && self.los.clear(me, Body { x: t.x, y: t.y, z: t.z, height: MISSILE_HEIGHT }, &self.heights_now));
            if noticed {
                self.missiles_seen.insert(id);
            }
        }
        // (barrels as monsters are seen: in her view (or near), nothing in the way; kept while
        // they're there)
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        // (the same barrel pushed by shots is still the one she saw)
        self.barrels_seen = self.barrels_seen.iter().filter_map(|&b| crate::motor::follower::barrel_now(obs, b)).collect();
        self.barrels_seen.dedup_by(|a, b| dist(*a, *b) < 1.0);
        for &b in &barrels {
            let d = dist(b, pos);
            if d >= self.tune.engage.range || self.barrels_seen.iter().any(|&x| dist(x, b) < 1.0) {
                continue;
            }
            if !(d < self.tune.senses.near || norm(heading(pos, b) - obs.angle).abs() < self.tune.senses.fov) {
                continue;
            }
            let floor = self.nav.poly_at(b).map_or(0.0, |p| self.heights_now.get(self.nav.sector_of_poly(p)).map_or(0.0, |h| h.0));
            if self.los.clear(me, Body { x: b.0, y: b.1, z: floor, height: BARREL_HEIGHT }, &self.heights_now) {
                self.barrels_seen.push(b);
            }
        }
        self.watch_infighting(obs);
        self.now = self.assess(obs);
    }

    /// Which monsters in sight are after each other, not her: facing another, not her, and
    /// either moving or faced back.
    fn watch_infighting(&mut self, obs: &Observation) {
        let mut now: BTreeMap<i32, i32> = BTreeMap::new();
        for t in &self.visible {
            let awake = self.known.get(&t.id).is_some_and(|s| s.awake);
            // (awake and after something, not her: which way it runs and shoots shows it)
            if !awake || t.after_her || !(t.vx.hypot(t.vy) > 0.5 || t.attack >= 0) {
                continue; // (standing about where it was put isn't fighting: MAP09's zombies)
            }
            let n = self.infight.get(&t.id).copied().unwrap_or(0) + 1;
            now.insert(t.id, n);
            if n == self.tune.senses.infight_tics {
                self.log.push(format!("tic {}: {} {} is fighting another: leaving it to it", obs.tic, t.name, t.id));
            }
        }
        self.infight = now;
    }

    pub fn infighting(&self, t: &Thing) -> bool {
        self.infight.get(&t.id).copied().unwrap_or(0) >= self.tune.senses.infight_tics
    }

    /// The monsters in sight that are after her (not each other): `now.against_her`.
    fn seen_against_her(&self) -> Vec<Thing> {
        self.visible.iter().filter(|t| !self.infighting(t)).cloned().collect()
    }

    /// Monsters seen that are still about (to hunt down).
    pub fn remaining(&self) -> Vec<Seen> {
        self.known.iter().filter(|(i, _)| self.alive.contains_key(i)).map(|(_, s)| s.clone()).collect()
    }

    /// The monsters she's in a fight with: in sight, awake, after her, and able to hurt her from
    /// where they are (one with a gun or missiles within ENGAGED_RANGED of her, a lost soul
    /// within ENGAGED_CHARGE, a clawing one within ENGAGED_MELEE). Not a zombieman across the
    /// map, nor a demon over a pit: what pauses her exploring (the user's: "in a fight" isn't
    /// anything awake in sight).
    pub fn engaged(&self, obs: &Observation) -> Vec<Thing> {
        let pos = obs.pos();
        self.seen_against_her()
            .into_iter()
            .filter(|t| t.health > 0 && t.after_her && self.known.get(&t.id).is_some_and(|k| k.awake))
            .filter(|t| {
                let d = dist((t.x, t.y), pos);
                match mon(t.name).attack {
                    Attack::Hitscan | Attack::Missile | Attack::Fire => d < self.tune.engage.engaged_ranged,
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
        let Some(s) = self.known.get(&t.id) else { return String::new() };
        if s.awake {
            return String::new(); // (the fist too wakes what hears it: P_FireWeapon's noise alert, no weapon WPF_SILENT)
        }
        if self.now.against_her.iter().any(|o| self.known.get(&o.id).is_some_and(|k| k.awake)) {
            return String::new(); // (already in a fight)
        }
        let Some(here) = self.nav.poly_at(obs.pos()) else { return String::new() };
        let hs: Vec<crate::nav::Heights> = self.heights(obs).iter().map(|&(floor, ceiling)| crate::nav::Heights { floor, ceiling }).collect();
        let heard = self.hearing.heard(self.nav.sector_of_poly(here), &hs);
        let sleepers: Vec<Seen> = self
            .remaining()
            .into_iter()
            .filter(|m| !m.awake && self.nav.poly_at(m.pos()).is_some_and(|p| heard.contains(&self.nav.sector_of_poly(p))))
            .collect();
        let hp: f64 = sleepers.iter().map(|m| f64::from(mon(m.name).health)).sum();
        if sleepers.len() < 2 || hp / self.now.firepower <= self.tune.engage.leave_be {
            return String::new();
        }
        let names: BTreeSet<&str> = sleepers.iter().map(|m| m.name).collect();
        format!("{}: {}", sleepers.len(), names.into_iter().collect::<Vec<_>>().join(", "))
    }

    // --- deciding ---------------------------------------------------------------------------

    /// A rocket on its way out (8 tics after the trigger, the way she faces then): the way to
    /// face till it's out, and whether that's turning off a way it'd go off near her (the last
    /// resort, the trigger pressed and no calling it back; the user's). Else held the way it was
    /// judged safe: MAP32 UV, the trigger pressed at an SS, a new target the tic after, swung 86
    /// degrees through the launch, the rocket out mid-turn into the wall beside her, dead at 31.
    /// Tracking within 10 degrees instead did worse over the regression set: E3 8.2 -> 7.2.
    fn rocket_held(&mut self, obs: &Observation) -> Option<(f64, bool)> {
        if !self.last_shot.is_some_and(|(slot, tic)| slot == 5 && ((obs.tic - tic) as f64) < aiming::ROCKET_DELAY) {
            return None;
        }
        if let Some(h) = self.rocket_way_out(obs) {
            return Some((h, true));
        }
        self.rocket_facing.map(|f| (f, false))
    }

    /// What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
    /// something that wants the controls (a run-up, a jump). `errand`: the route's going for
    /// something to fight with: the fight only aims and shoots on the way.
    pub fn decide(&mut self, obs: &Observation, busy: bool, errand: &str) -> Option<Decision> {
        // (invulnerable: nothing to dodge, a missile does nothing)
        let missiles = if self.now.invulnerable { Vec::new() } else { self.incoming(obs) };
        let target = self.pick_target(obs);
        if target.is_none() && missiles.is_empty() {
            // (the fight over with a rocket still to go out: the way held all the same, the route's
            // turn after; E3M9, the last rocket pressed at a baron as it died, the route turned her
            // for the door through the launch, out into the wall by her, dead at 53)
            if let Some((aim, _)) = self.rocket_held(obs) {
                return Some(Decision { kind: Move::RocketHeld, yields: true, pressed: false, aim: Some(aim), fire: false, weapon: 0, moving: None, why: "holding a rocket's way till it's out".into() });
            }
            return self.no_target(obs, busy, errand);
        }
        let (s, fire, weapon) = self.size_up(obs, busy, errand, missiles, target);
        let mut act = tree::engage(self, obs, &s);
        // (a rocket on its way out, 8 tics after the trigger, the way she faces then: if that way
        // it'd go off near her, turned to the nearest way it won't, the last resort, the trigger
        // pressed and no calling it back; the user's)
        if s.vile.is_none() {
            if let Some((aim, out)) = self.rocket_held(obs) {
                act.aim = Some(aim);
                if out {
                    act.kind = Move::RocketTurnedOff;
                    act.why = "turned off a rocket's way out into something near".into();
                }
            }
        }
        let aim = act.aim.or(s.aim);
        let fire = fire && !act.hold_fire && !self.hold_fire;
        let why = act.why;
        let kind = act.kind;
        let pressed = !s.missiles.is_empty() || s.vile.is_some();
        if why != self.mode {
            let t = s.target.as_ref().map_or(String::new(), |t| format!(" (target {} {}, {:.0} threat; health {:.0})", t.name, t.id, self.now.threat, self.now.health));
            self.log.push(format!("tic {}: {why}{t}", obs.tic));
            self.journal.note(|| Record { detail: why.clone(), ..Record::new(obs.tic, "move", format!("{kind:?}")) });
            self.mode = why.clone();
        }
        Some(Decision { kind, yields: act.route, pressed, aim, fire, weapon, moving: act.moving, why })
    }

    /// The fight as it stands, for the tree: the aim and the trigger at the target (or a barrel
    /// by it), the weapon to switch to, and what the nodes look at.
    fn size_up<'a>(&mut self, obs: &Observation, busy: bool, errand: &'a str, missiles: Vec<Thing>, target: Option<Thing>) -> (tree::Situation<'a>, bool, i32) {
        let pos = obs.pos();
        let first_missile = missiles.first().cloned();
        let (mut aim, mut fire, mut weapon) = (None, false, 0);
        if let Some(t) = &target {
            let (a, f, w) = self.aim_and_fire(obs, t);
            (aim, fire, weapon) = (Some(a), f, w);
            if let Some(shot) = self.barrel_shot(obs) {
                if matches!(obs.weapon, Some(2..=4)) {
                    // (MAP23: the barrels round the mancubus do what the pistol can't)
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
            .visible
            .iter()
            .filter(|t| {
                let m = mon(t.name);
                (matches!(m.attack, Attack::Melee | Attack::Charge | Attack::Fire) || m.melee) && dist((t.x, t.y), pos) < self.tune.engage.close + m.radius
            })
            .cloned()
            .collect();
        let shooting = self.visible.iter().any(|t| matches!(mon(t.name).attack, Attack::Hitscan | Attack::Missile));
        let barrel = obs.barrels.iter().flatten().copied().reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a });
        let mut barrel_away = None;
        // (with something shooting in sight, or her shooting at something: a shot, hers or theirs,
        // sets a barrel off; E1M8's demons, biters only, fought among the barrels)
        if first_missile.is_none() && !busy && (shooting || target.is_some()) {
            if let Some(b) = barrel.filter(|&b| dist(b, pos) < self.tune.barrels.keep_clear + 64.0) {
                barrel_away = self.away_from_barrels(obs, b);
            }
        }
        // (invulnerable: nothing to hide from; her BFG ball in the air: in their sight till it
        // lands, its spray from where she is)
        let unhurt = self.now.invulnerable;
        if unhurt {
            barrel_away = None;
        }
        // (a barrel going off by her, whoever set it off: away from it before a missile's dodge,
        // its blast the worse; E1M6's, set off by another's blast and pushed her way, met
        // dodging an imp's fireball)
        let barrel_lit = if busy || unhurt { None } else { Self::lit_barrel_by(obs).and_then(|b| self.away_from(obs, b)) };
        let vile = if busy || unhurt { None } else { self.vile_fire(obs) };
        let duck = if busy || unhurt || vile.is_some() || self.bfg_flying(obs) { None } else { self.duck(obs) };
        let s = tree::Situation { pos, busy, errand, missiles, target, aim, close, barrel_away, barrel_lit, vile, duck };
        (s, fire, weapon)
    }

    /// Has she given up on getting at id a while?
    pub(super) fn given_up_on(&self, obs: &Observation, id: i32) -> bool {
        self.patience.given_up(id, obs.tic)
    }

    /// Out of her fist's reach: given up on, or still on the floor it stood on when closing in on
    /// it got her no nearer, and she's not (a ledge above her stays one, walked along or not;
    /// MAP14 HMP, the same chaingunner walked at again once the give-up ran out, to death).
    pub(super) fn out_of_fist_reach(&self, obs: &Observation, t: &Thing) -> bool {
        self.given_up_on(obs, t.id) || self.fist_stalled.get(&t.id).is_some_and(|&s| self.sector_at((t.x, t.y)) == Some(s) && self.sector_at(obs.pos()) != Some(s))
    }

    /// The sector under a point.
    pub(super) fn sector_at(&self, p: Point) -> Option<usize> {
        self.nav.poly_at(p).map(|pi| self.nav.sector_of_poly(pi))
    }

    /// Give up on getting at id a while.
    pub(super) fn give_up(&mut self, obs: &Observation, id: i32) {
        self.patience.give_up(id, obs.tic + self.tune.engage.given_up_tics);
        self.log.push(format!("tic {}: can't get at {id}: leaving it a while", obs.tic));
    }

    /// Closing in on t, out of reach: still worth it? Not once she's been at it this long without
    /// getting it in reach (E1M9's imps across the gap, 800 off, closed in on and lost sight of
    /// for an hour): then it's left a while.
    fn still_closing(&mut self, obs: &Observation, id: i32) -> bool {
        if self.given_up_on(obs, id) {
            return false;
        }
        if self.patience.spent(Effort::Closing, id, obs.tic, self.tune.engage.close_give_up, None, None) {
            self.give_up(obs, id);
            return false;
        }
        true
    }

    /// Standing to fight t, no shot at it from here: given up on a while once she's had none for
    /// CLOSE_GIVE_UP (a shot at it sets the clock back; so does a fight with it broken off that
    /// long). Standing gets her no shot: the route goes on, shooting on the way when one comes.
    /// (Pistol start, ITYTD-HNTR: on an island in a nukage pit, two zombiemen on ledges round it,
    /// too steep from her, never hitting her either; stood at for the hour, by turns, coming and
    /// going from sight, her route held and every step on given up as taking too long)
    pub(super) fn no_shot_long(&mut self, obs: &Observation, t: &Thing) -> bool {
        if self.given_up_on(obs, t.id) {
            return true;
        }
        if self.can_hit(obs, t, None) {
            self.patience.rest(Effort::NoShot, t.id);
            return false;
        }
        let limit = self.tune.engage.close_give_up;
        if self.patience.spent(Effort::NoShot, t.id, obs.tic, limit, None, Some(limit)) {
            self.give_up(obs, t.id);
            return true;
        }
        false
    }

    /// An awake monster that was after her gone out of sight a moment ago (behind a pillar,
    /// round a corner): she keeps fighting it, aimed where it was, keeping her distance, rather
    /// than going about her business (MAP32: walked up to a cyberdemon behind a pillar).
    /// Not on a floor that hurts (or will, a suit's time running out): the route's way off is
    /// the way to take, not after a ghost (E4M2 HNTR: in a lava hall under a suit, a demon with no
    /// shot at it (too steep) seen and lost by turns, kept at 1500 tics till the suit was out;
    /// E4M3 HMP, a cacodemon in and out of sight over the lava, held there 100 to 0).
    fn lost_sight(&mut self, obs: &Observation) -> Option<Decision> {
        if self.on_hurting_floor(obs) {
            return None;
        }
        let pos = obs.pos();
        let s = self
            .remaining()
            .into_iter()
            .filter(|s| s.awake && obs.tic - s.tic < self.tune.engage.lost_tics && mon(s.name).attack != Attack::None && dist(s.pos(), pos) < self.tune.engage.range)
            .filter(|s| !self.patience.given_up(s.id, obs.tic))
            // (not after one she can't take on: out of its sight's where she wants to be; E3M8,
            // the spider mastermind gone behind something, walked after with a pistol)
            .filter(|s| !self.too_much(obs, &[s.name], self.tune.guns.outmatched))
            .max_by(|a, b| mon(a.name).threat.total_cmp(&mon(b.name).threat))?;
        let mut ghost = self.alive.get(&s.id)?.clone();
        (ghost.x, ghost.y, ghost.z, ghost.vx, ghost.vy) = (s.x, s.y, s.z, 0.0, 0.0);
        let gun = self.preferred_gun(obs, &ghost);
        let moving = self.steer(obs, Some(&ghost), &gun);
        let why = format!("lost sight of {}: keeping at it", s.name);
        if why != self.mode {
            self.log.push(format!("tic {}: {why}", obs.tic));
            self.mode = why.clone();
        }
        Some(Decision { kind: Move::LostSight, yields: false, pressed: false, aim: Some(heading(pos, s.pos())), fire: false, weapon: 0, moving, why })
    }

    /// The route's controls this tic (the way it walks her, for what's in her way).
    pub fn route_walks(&mut self, obs: &Observation, b: &Controls) {
        self.route_way = (b.forward.hypot(b.strafe) > 0.0).then(|| obs.angle - b.turn + (-b.strafe).atan2(b.forward).to_degrees());
    }

    /// The controls for a decision. `legs`: the fight's move has her legs (the arbiter's call);
    /// else `base`'s do (a route's, the explorer's or a retreat's), and only the aim and the
    /// trigger are the fight's. `base` is given with the fight's legs too when a route's
    /// walking under it, for what's in her way.
    pub fn controls(&mut self, obs: &Observation, d: &Decision, base: Option<&Controls>, legs: bool) -> Controls {
        let turn = self.turn(obs, d.aim);
        if let Some(b) = base {
            self.route_walks(obs, b);
        }
        let mut c = match base {
            Some(b) if !legs => {
                let mut c = Controls { forward: b.forward, strafe: b.strafe, turn, use_: b.use_, run: b.run, weapon: b.weapon, ..Controls::default() };
                // (nothing to shoot with: aiming fires nothing, and the route's turn is what
                // faces a shut door on her way to press it; MAP31, HMP, out of ammo and off for
                // more, turned to the SS shooting her, 450 tics at a door she never pressed)
                if d.aim.is_none() || !self.armed(obs) {
                    c.turn = b.turn;
                } else {
                    // (the route's way in the world, whichever way she's turned to aim)
                    let way = obs.angle - b.turn + (-b.strafe).atan2(b.forward).to_degrees();
                    let rel = norm(way - (obs.angle - turn)).to_radians();
                    let mag = b.forward.hypot(b.strafe);
                    c.forward = mag * rel.cos();
                    c.strafe = -mag * rel.sin();
                }
                c
            }
            _ => {
                let mut c = Controls { turn, ..Controls::default() };
                let facing = obs.angle - turn; // (thrust goes the way faced after this tic's turn)
                if let Some(m) = d.moving {
                    let rel = norm(m.1.atan2(m.0).to_degrees() - facing).to_radians();
                    c.forward = 50.0 * rel.cos();
                    c.strafe = -50.0 * rel.sin();
                } else if self.coasting_off(obs) {
                    // (nowhere the fight wants her: not where the speed she has would take her)
                    (c.forward, c.strafe) = crate::motor::brake(obs, facing);
                }
                c
            }
        };
        c.attack = d.fire || base.is_some_and(|b| b.attack && !legs);
        // (the rocket launcher and the BFG don't fire again with the trigger held: let go a tic
        // after each press, the game's way to fire the next; held, only the first went)
        if matches!(obs.weapon, Some(5 | 7)) && self.trigger_held {
            c.attack = false;
        }
        self.trigger_held = c.attack;
        if let Some(s) = obs.weapon.filter(|_| c.attack) {
            self.last_shot = Some((s, obs.tic));
            if s == 5 {
                self.rocket_facing = Some(obs.angle);
            }
        }
        if d.weapon != 0 {
            c.weapon = d.weapon;
        }
        c
    }

    // --- helpers ----------------------------------------------------------------------------

    /// The last hit she took: (tic, health lost).
    pub fn last_hit(&self) -> Option<(i64, f64)> {
        self.damage.last().copied()
    }

    /// Is sector s a crusher at work: its ceiling coming down lately, and low (E2M2: backed off
    /// under one fighting a demon, crushed)? Or its floor coming up lately, with little more
    /// than her height left under the ceiling (E2M4's secret: a floor raised to crush, walked
    /// onto over its W1 line, stood on with nothing left to do till it had her; a lift ends
    /// level with a floor beside it, room enough over it).
    /// A crusher at work in sector s: its ceiling come down lower than she stands within the last
    /// ACTIVE_CRUSHER tics, at the top of its stroke now or not. Where the fight's moves don't go
    /// (the user's: active crushers no-go), not just while it's on its way down.
    pub fn crusher_active(&self, obs: &Observation, s: usize) -> bool {
        !self.doors.contains(&s) && self.crushed_low.get(s).is_some_and(|&t| obs.tic - t < self.tune.senses.active_crusher)
    }

    /// Is sector s a crusher she knows of, stopped (or not) too low to get out from under if it
    /// starts again: less room over her head, in tics, than getting out of it takes at the worst,
    /// and its stroke more than she'd take on (one that holds her: always; a fast one: more than a
    /// tenth of her health, the user's). E2M4's slow one: stopped 9 over her head, run into
    /// chasing an imp, its own start line its edge, set off; 9 tics, going the wrong way at full
    /// speed, pinned, crushed.
    pub fn crusher_trap(&self, obs: &Observation, s: usize) -> bool {
        let (Some(&kind), Some(&out), Some(h)) = (self.crushers.get(&s), self.crusher_out.get(&s), obs.height(s)) else { return false };
        crate::crush::trap(h, kind, out, crate::knowledge::RISK_SHARE * obs.health.unwrap_or(100.0), obs)
    }

    pub fn crushing(&self, obs: &Observation, s: usize) -> bool {
        let gap = obs.heights.get(s).map_or(f64::INFINITY, |h| h.ceiling - h.floor);
        (!self.doors.contains(&s) && self.ceiling_down.get(s).is_some_and(|&t| obs.tic - t < self.tune.senses.crush_tics) && gap < self.tune.senses.crush_gap)
            || (!self.lifts.contains(&s) && self.floor_up.get(s).is_some_and(|&t| obs.tic - t < self.tune.senses.crush_tics) && gap < self.tune.senses.floor_crush_gap)
    }

    /// Health lost a second lately (over the last few seconds): `now.damage_rate`.
    fn damage_rate(&mut self, tic: i64) -> f64 {
        self.damage.retain(|&(t, _)| tic - t < self.tune.senses.damage_window);
        self.damage.iter().map(|(_, d)| d).sum::<f64>() / (self.tune.senses.damage_window as f64 / TICRATE)
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
        tic < self.invulnerable_until - self.tune.senses.invulnerable_spare
    }

    /// Her own BFG ball in the air: its spray comes from where she is when it lands, along the
    /// way it flew (A_BFGSpray: forty lines from her, a quarter turn wide, 1024 out), so she
    /// stays where she can see them till then.
    pub fn bfg_flying(&self, obs: &Observation) -> bool {
        let charging = self.last_shot.is_some_and(|(slot, tic)| slot == 7 && obs.tic - tic <= aiming::BFG_DELAY + 1);
        charging || obs.things.iter().flatten().any(|t| t.name == "BFGBall" && t.owner == 0)
    }

    /// How t's moving, a tic: its momentum, or walking (a monster walks by steps, not momentum:
    /// its momentum's nothing as it goes), how far it's gone over the last few tics. For leading a
    /// rocket or a plasma ball: E3M8, 90 rockets at the spider mastermind walking 4 a tic, 1000
    /// off, aimed where it was, 200 damage done.
    pub fn moving(&self, t: &Thing) -> Point {
        if t.vx.hypot(t.vy) > 0.5 {
            return (t.vx, t.vy);
        }
        match self.tracks.get(&t.id).and_then(|tr| Some((*tr.front()?, *tr.back()?))) {
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
        tic - self.no_duck_tic <= 1
    }

    /// What the fight's doing now ("" with none on).
    pub fn mode(&self) -> &str {
        &self.mode
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

/// How near t comes to her as it goes by, if she runs `way` from now (stopped once she's gone
/// `room`: a wall that way; a rocket by a wall she'd have run into met her there). A rocket's
/// counted from its middle: its blast (the clearance) does the rest.
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
    let mut b: Vec<f64> = points.iter().map(|p| (p.1 - at.1).atan2(p.0 - at.0).to_degrees().rem_euclid(360.0)).collect();
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
    let u = if n == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / n).clamp(0.0, 1.0) };
    dist(p, (a.0 + dx * u, a.1 + dy * u))
}
