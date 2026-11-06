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

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::bestiary::{missile, monster, weapon_in, Attack, Missile, Weapon};
use super::sound::Hearing;
use crate::geom::{dist, Point};
use crate::los::{Body, LineOfSight};
use crate::motor::{heading, norm, Controls, Observation, Thing, FRICTION, RUN_THRUST, TICRATE};
use crate::navigator::{Navigator, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS};

/// half the view, degrees: monsters in line of sight inside it are seen...
const FOV: f64 = 55.0;
/// ...and any this near, in any direction (heard)
const NEAR: f64 = 384.0;
/// being hurt: everything in line of sight is noticed for this long
const HURT_TICS: i64 = 2 * 35;
/// A recently seen monster can still be noticed outside the usual field of view.
const RECENT_SIGHT_TICS: i64 = 35;
/// Movement from the first sighting that counts as waking up (units, units/tic, degrees).
const WAKE_DISTANCE: f64 = 16.0;
const WAKE_SPEED: f64 = 0.5;
const WAKE_TURN: f64 = 1.0;
/// no shooting at anything further
const ENGAGE_RANGE: f64 = 2048.0;
/// degrees a tic, turning to aim
const AIM_TURN: f64 = 18.0;
const TURN_ACCEL: f64 = 5.0;
/// a missile arriving sooner than this is dodged...
const DODGE_TICS: i64 = 35;
/// ...if it would pass nearer than the two radii and this
const DODGE_MARGIN: f64 = 12.0;
const DODGE_WAYS: usize = 16;
/// going some way: pushing that way this long, then...
const PUSH_TICS: usize = 8;
/// ...letting go: where she'd slide to in this long must be safe too
const SLIDE_TICS: usize = 40;
/// a clawing monster nearer than this: back off from it
const CLOSE: f64 = 160.0;
/// this much threat in sight: stand and fight rather than walk on
const STAND_THREAT: f64 = 6.0;
/// ...unless killing what's in sight would take longer than this (s): then get past it
pub const OUTMATCHED: f64 = 15.0;
/// fewer shots than this for the guns held: supplies come first
const SHORT_OF_AMMO: i32 = 15;
/// ...or below this health, anything in sight
const LOW_HEALTH: f64 = 35.0;
/// with something shooting in sight, keep this far from barrels
const BARREL_CLEAR: f64 = 160.0;
/// a barrel this near a monster (and well clear of her) is shot instead
const BARREL_NEAR: f64 = 96.0;
/// getting clear of barrels: at most this long at a time...
const BARREL_TICS: i64 = 2 * 35;
/// ...then not again for this long (E1M7: pressed against a wall for ever)
const BARREL_REST: i64 = 3 * 35;
/// a strafe one way lasts between these
const STRAFE_TICS: (i64, i64) = (18, 40);
/// shots leave her this far above her feet...
const SHOT_HEIGHT: f64 = 36.0;
/// ...and find what they're aimed at only this steep
const AUTOAIM: f64 = 100.0 / 160.0;
/// a biter seen this lately near her rules out rockets
const ROCKET_WARY: i64 = 3 * 35;
/// a monster facing another this nearly...
const INFIGHT_FACING: f64 = 30.0;
/// ...and not her...
const INFIGHT_NOT_HER: f64 = 45.0;
/// ...this long: they're fighting each other: leave them
const INFIGHT_TICS: i32 = 18;
/// something that shoots back from anywhere: shot at past a gun's reach while this much lands
const FAR_HITS: f64 = 0.2;
/// a weapon switch asked for: give it this long before asking again
const SWITCH_WAIT: i64 = 40;
/// seconds a weapon switch costs
const SWITCH_COST: f64 = 0.8;

/// What using up ammo costs, in seconds of fighting a unit, and below how much the cost goes up.
fn ammo_cost(slot: i32) -> (f64, f64) {
    match slot {
        2 | 4 => (0.01, 20.0),
        3 => (0.05, 8.0),
        5 => (0.3, 4.0),
        6 => (0.015, 40.0),
        _ => (0.015, 80.0),
    }
}

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
    /// the fight needs the movement too
    pub takeover: bool,
    /// the angle to turn to, if fighting at all
    pub aim: Option<f64>,
    pub fire: bool,
    /// the slot to switch to (0: keep)
    pub weapon: i32,
    /// the world direction to run in (unit), if moving
    pub moving: Option<Point>,
    pub why: String,
}

pub struct Combat {
    nav: Rc<Navigator>,
    pub los: LineOfSight,
    hearing: Hearing,
    /// every monster seen, by id
    pub known: BTreeMap<i32, Seen>,
    /// every monster about now (as the engine has it)
    pub alive: BTreeMap<i32, Thing>,
    /// monsters she can see now
    pub visible: Vec<Thing>,
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
    held_fire: String,
    /// (baiting: the monsters in the way are the point)
    pub hold_fire: bool,
    /// monster -> tics it's looked to be after another
    infight: BTreeMap<i32, i32>,
    heights_tic: Option<i64>,
    heights_now: Vec<(f64, f64)>,
    hit_cache: BTreeMap<(i32, u64), bool>,
    /// missiles being dodged (till they've gone by)
    dodging: BTreeSet<i32>,
    dodge_way: Option<Point>,
    last_pos: Option<Point>,
    velocity: Point,
    barrel_way: Option<Point>,
    barrel_since: i64,
    barrel_rest_until: i64,
}

impl Combat {
    pub fn new(nav: Rc<Navigator>) -> Combat {
        Combat {
            los: LineOfSight::new(&nav.map),
            hearing: Hearing::new(&nav.map),
            nav,
            known: BTreeMap::new(),
            alive: BTreeMap::new(),
            visible: Vec::new(),
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
            held_fire: String::new(),
            hold_fire: false,
            infight: BTreeMap::new(),
            heights_tic: None,
            heights_now: Vec::new(),
            hit_cache: BTreeMap::new(),
            dodging: BTreeSet::new(),
            dodge_way: None,
            last_pos: None,
            velocity: (0.0, 0.0),
            barrel_way: None,
            barrel_since: 0,
            barrel_rest_until: -1_000_000_000,
        }
    }

    /// A new life: the level from the start (what's been seen is remembered).
    pub fn restart(&mut self) {
        self.alive.clear();
        self.visible.clear();
        self.target = None;
        self.health = None;
        self.hurt_tic = -1_000_000_000;
        self.mode.clear();
        self.dodging.clear();
        self.last_pos = None;
        self.velocity = (0.0, 0.0);
    }

    // --- what's about -----------------------------------------------------------------------

    pub fn observe(&mut self, obs: &Observation) {
        let pos = obs.pos();
        if let Some(v) = obs.velocity {
            self.velocity = v;
        } else if let Some(l) = self.last_pos {
            self.velocity = (pos.0 - l.0, pos.1 - l.1);
        }
        self.last_pos = Some(pos);
        if let Some(h) = obs.health {
            if self.health.is_some_and(|before| h < before) {
                self.hurt_tic = obs.tic;
            }
            self.health = Some(h);
        }
        let monsters: BTreeMap<i32, Thing> = obs.things.iter().flatten().filter(|t| monster(t.name).is_some() && t.name != "CommanderKeen").map(|t| (t.id, t.clone())).collect();
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
        let hurt = obs.tic - self.hurt_tic < HURT_TICS;
        self.visible.clear();
        for t in self.alive.values() {
            let d = dist((t.x, t.y), pos);
            if !(0.0..ENGAGE_RANGE).contains(&d) {
                continue;
            }
            let recent = self.known.get(&t.id).is_some_and(|s| obs.tic - s.tic < RECENT_SIGHT_TICS);
            if !(hurt || recent || d < NEAR || norm(heading(pos, (t.x, t.y)) - obs.angle).abs() < FOV) {
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
                || dist(seen.pos(), (seen.x0, seen.y0)) > WAKE_DISTANCE
                || t.vx.hypot(t.vy) > WAKE_SPEED
                || norm(t.angle - seen.angle0).abs() > WAKE_TURN;
            self.visible.push(t.clone());
        }
        self.watch_infighting(obs);
    }

    /// Which monsters in sight are after each other, not her: facing another, not her, and
    /// either moving or faced back.
    fn watch_infighting(&mut self, obs: &Observation) {
        let pos = obs.pos();
        let mut now: BTreeMap<i32, i32> = BTreeMap::new();
        for t in &self.visible {
            let awake = self.known.get(&t.id).is_some_and(|s| s.awake);
            if !awake || norm(t.angle - heading((t.x, t.y), pos)).abs() < INFIGHT_NOT_HER {
                continue; // (standing about where it was put isn't fighting: MAP09's zombies)
            }
            let moving = t.vx.hypot(t.vy) > 0.5;
            for o in &self.visible {
                if o.id == t.id || dist((o.x, o.y), (t.x, t.y)) < 1.0 {
                    continue;
                }
                if o.name == t.name && mon(t.name).attack != Attack::Hitscan {
                    continue; // (a kind's fireballs and claws don't hurt its own)
                }
                if norm(t.angle - heading((t.x, t.y), (o.x, o.y))).abs() < INFIGHT_FACING && (moving || norm(o.angle - heading((o.x, o.y), (t.x, t.y))).abs() < INFIGHT_FACING) {
                    let n = self.infight.get(&t.id).copied().unwrap_or(0) + 1;
                    now.insert(t.id, n);
                    if n == INFIGHT_TICS {
                        self.log.push(format!("tic {}: {} {} is fighting {} {}: leaving them to it", obs.tic, t.name, t.id, o.name, o.id));
                    }
                    break;
                }
            }
        }
        self.infight = now;
    }

    pub fn infighting(&self, t: &Thing) -> bool {
        self.infight.get(&t.id).copied().unwrap_or(0) >= INFIGHT_TICS
    }

    /// The monsters in sight that are after her (not each other).
    pub fn against_her(&self) -> Vec<Thing> {
        self.visible.iter().filter(|t| !self.infighting(t)).cloned().collect()
    }

    /// Monsters seen that are still about (to hunt down).
    pub fn remaining(&self) -> Vec<Seen> {
        self.known.iter().filter(|(i, _)| self.alive.contains_key(i)).map(|(_, s)| s.clone()).collect()
    }

    pub fn threats(&self) -> f64 {
        self.against_her().iter().map(|t| mon(t.name).threat).sum()
    }

    fn held(obs: &Observation) -> Vec<i32> {
        obs.weapons.clone().unwrap_or_else(|| vec![1, 2])
    }

    fn ammo(obs: &Observation, slot: i32) -> i32 {
        obs.ammo.iter().flatten().find(|(s, _)| *s == slot).map_or(0, |(_, n)| *n)
    }

    /// Damage a second she can do with the best gun she has ammo for (the fist's, else).
    pub fn firepower(&self, obs: &Observation) -> f64 {
        let held = Self::held(obs);
        SLOTS
            .iter()
            .filter(|s| held.contains(s))
            .map(|&s| weapon_in(s, obs.super_shotgun))
            .filter(|w| w.ammo_per_shot == 0 || Self::ammo(obs, w.slot) >= w.ammo_per_shot)
            .map(|w| w.dps())
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Would killing what's in sight take too long with what she has (a room of barons with the
    /// pistol: MAP08 at pistol start)?
    pub fn outmatched(&self, obs: &Observation) -> bool {
        let hp: f64 = self.visible.iter().filter(|t| dist((t.x, t.y), obs.pos()) < 1200.0).map(|t| f64::from(mon(t.name).health)).sum();
        hp / self.firepower(obs) > OUTMATCHED
    }

    /// Can any gun she has ammo for get to t from here?
    pub fn reachable(&self, obs: &Observation, t: &Thing) -> bool {
        let held = Self::held(obs);
        let d = dist((t.x, t.y), obs.pos());
        SLOTS.iter().any(|&s| {
            let w = weapon_in(s, obs.super_shotgun);
            held.contains(&s) && Self::ammo(obs, s) >= w.ammo_per_shot && Self::in_reach(&w, t, d)
        })
    }

    /// If t is standing about, and a shot would be heard by enough monsters she's seen standing
    /// about too that she'd be outmatched once they're awake: what it'd wake (else "").
    fn would_wake(&mut self, obs: &Observation, t: &Thing) -> String {
        let Some(s) = self.known.get(&t.id) else { return String::new() };
        if s.awake || self.gun(obs).slot == 1 {
            return String::new(); // (the fist makes no noise)
        }
        if self.against_her().iter().any(|o| self.known.get(&o.id).is_some_and(|k| k.awake)) {
            return String::new(); // (already in a fight)
        }
        let Some(here) = self.nav.poly_at(obs.pos()) else { return String::new() };
        let hs: Vec<crate::nav::Heights> = self.heights(obs).iter().map(|&(floor, ceiling)| crate::nav::Heights { floor, ceiling }).collect();
        let heard = self.hearing.heard(self.nav.sector_of_poly(here), &hs);
        let sleepers: Vec<Seen> = self.remaining().into_iter().filter(|m| !m.awake && self.nav.poly_at(m.pos()).is_some_and(|p| heard.contains(&self.nav.sector_of_poly(p)))).collect();
        let hp: f64 = sleepers.iter().map(|m| f64::from(mon(m.name).health)).sum();
        if sleepers.len() < 2 || hp / self.firepower(obs) <= OUTMATCHED {
            return String::new();
        }
        let names: BTreeSet<&str> = sleepers.iter().map(|m| m.name).collect();
        format!("{}: {}", sleepers.len(), names.into_iter().collect::<Vec<_>>().join(", "))
    }

    /// Has she a gun with ammo for it (more than the fist)?
    pub fn armed(&self, obs: &Observation) -> bool {
        let held = Self::held(obs);
        SLOTS.iter().any(|&s| s != 1 && held.contains(&s) && Self::ammo(obs, s) >= weapon_in(s, obs.super_shotgun).ammo_per_shot)
    }

    /// Next to nothing left to shoot with (the fist doesn't count).
    pub fn short_of_ammo(&self, obs: &Observation) -> bool {
        let held = Self::held(obs);
        let shots: i32 = SLOTS
            .iter()
            .filter(|&&s| held.contains(&s) && s != 4 && s != 7)
            .map(|&s| (s, super::bestiary::weapon(s)))
            .filter(|(_, w)| w.ammo_per_shot > 0)
            .map(|(s, w)| Self::ammo(obs, s) / w.ammo_per_shot)
            .sum();
        shots < SHORT_OF_AMMO
    }

    // --- deciding ---------------------------------------------------------------------------

    /// What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
    /// something that wants the controls (a run-up, a jump). `errand`: the route's going for
    /// something to fight with: the fight only aims and shoots on the way.
    pub fn decide(&mut self, obs: &Observation, busy: bool, errand: &str) -> Option<Decision> {
        let pos = obs.pos();
        let missiles = self.incoming(obs);
        let first_missile = missiles.first().cloned();
        let target = self.pick_target(obs);
        if target.is_none() && first_missile.is_none() {
            if !self.mode.is_empty() {
                self.log.push(format!("tic {}: fight over ({} kills)", obs.tic, self.kills));
            }
            self.mode.clear();
            self.target = None;
            return None;
        }
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
                    fire = norm(a - obs.angle).abs() < 0.75f64.max(8.0f64.atan2(d).to_degrees()) && d < weapon_in(obs.weapon.unwrap_or(2), obs.super_shotgun).reach && self.shot_clear(obs, shot, floor, 32.0);
                }
            }
        }
        let health = obs.health.unwrap_or(100.0);
        let threat = self.threats();
        let outmatched = self.outmatched(obs);
        let close: Vec<Thing> = self
            .visible
            .iter()
            .filter(|t| {
                let m = mon(t.name);
                (matches!(m.attack, Attack::Melee | Attack::Charge | Attack::Fire) || m.melee) && dist((t.x, t.y), pos) < CLOSE + m.radius
            })
            .cloned()
            .collect();
        let shooting = self.visible.iter().any(|t| matches!(mon(t.name).attack, Attack::Hitscan | Attack::Missile));
        let barrel = obs.barrels.iter().flatten().copied().reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a });
        let mut moving = None;
        let why: String;
        let mut barrel_away = None;
        if first_missile.is_none() && !busy && shooting {
            if let Some(b) = barrel.filter(|&b| dist(b, pos) < BARREL_CLEAR + 64.0) {
                barrel_away = self.away_from_barrels(obs, b);
            }
        }
        if let Some(m) = &first_missile {
            moving = self.dodge(obs, &missiles);
            why = format!("dodging {}", m.name);
        } else if let Some(away) = barrel_away {
            // (MAP23: a mancubus's fireball set off the barrels she stood by)
            moving = Some(away);
            why = "away from a barrel".into();
        } else if busy {
            why = "shooting on the way".into();
        } else if let Some(nearest) = close.iter().min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos))) {
            moving = self.away_from(obs, (nearest.x, nearest.y));
            why = format!("backing off from {}", nearest.name);
        } else if !errand.is_empty() {
            why = format!("shooting on the way to {errand}");
        } else if !self.armed(obs) {
            // (MAP05, MAP11: out of ammo, fist out, walked up to imps to punch them)
            why = "out of ammo: getting past".into();
        } else if let Some(woken) = target.as_ref().map(|t| self.would_wake(obs, t)).filter(|w| !w.is_empty()) {
            fire = false;
            why = format!("sneaking past (a shot would wake {woken})");
        } else if target.as_ref().is_some_and(|t| !self.reachable(obs, t)) {
            why = "out of reach: going on".into();
        } else if let Some(way) = target.as_ref().filter(|t| self.visible.len() == 1 && dist((t.x, t.y), pos) < 512.0 && !self.can_hit(obs, t, None)).and_then(|t| self.shot_way(obs, t)) {
            moving = Some(way);
            why = format!("stepping for a shot at {}", target.as_ref().map_or("", |t| t.name));
        } else if outmatched {
            why = "getting past".into(); // (shooting on the way, the route going on)
        } else if let Some(way) = target.as_ref().filter(|t| mon(t.name).attack != Attack::None && dist((t.x, t.y), pos) < self.gun(obs).keep.0).and_then(|t| self.keeping_off(obs, t, aim)) {
            // (too near for the gun she has, or for them to miss)
            moving = Some(way);
            why = format!("keeping off from {}", target.as_ref().map_or("", |t| t.name));
        } else if threat >= STAND_THREAT || health < LOW_HEALTH || self.blocking(obs, target.as_ref()) {
            moving = self.strafe_dir(obs, aim);
            let mut w = "standing and fighting".to_string();
            let gun = self.gun(obs);
            if let Some(t) = &target {
                let d = dist((t.x, t.y), pos);
                if !Self::in_reach(&gun, t, d) || d > gun.keep.1 {
                    moving = self.closing_in(obs, t, moving);
                    w = format!("closing in on {}", t.name);
                }
            }
            why = w;
        } else {
            why = "shooting on the way".into();
        }
        if self.hold_fire {
            fire = false;
        }
        let takeover = (!busy && !["shooting on the way", "getting past", "out of", "sneaking"].iter().any(|p| why.starts_with(p))) || first_missile.is_some();
        if why != self.mode {
            let t = target.as_ref().map_or(String::new(), |t| format!(" (target {} {}, {threat:.0} threat)", t.name, t.id));
            self.log.push(format!("tic {}: {why}{t}", obs.tic));
            self.mode = why.clone();
        }
        Some(Decision { takeover, aim, fire, weapon, moving, why })
    }

    /// The controls for a decision: over the route's (`base`: only the aim and the trigger are
    /// the fight's), or all of them.
    pub fn controls(&mut self, obs: &Observation, d: &Decision, base: Option<&Controls>) -> Controls {
        let turn = self.turn(obs, d.aim);
        let mut c = match base {
            Some(b) if !d.takeover => {
                let mut c = Controls { forward: b.forward, strafe: b.strafe, turn, use_: b.use_, run: b.run, weapon: b.weapon, ..Controls::default() };
                if d.aim.is_none() {
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
                if let Some(m) = d.moving {
                    let facing = obs.angle - turn; // (thrust goes the way faced after this tic's turn)
                    let rel = norm(m.1.atan2(m.0).to_degrees() - facing).to_radians();
                    c.forward = 50.0 * rel.cos();
                    c.strafe = -50.0 * rel.sin();
                }
                c
            }
        };
        c.attack = d.fire || base.is_some_and(|b| b.attack && !d.takeover);
        if d.weapon != 0 {
            c.weapon = d.weapon;
        }
        c
    }

    // --- targets, weapons, aim ----------------------------------------------------------------

    fn pick_target(&mut self, obs: &Observation) -> Option<Thing> {
        let pos = obs.pos();
        let mut best: Option<(f64, Thing)> = None;
        for t in self.against_her() {
            let info = mon(t.name);
            let d = dist((t.x, t.y), pos).max(64.0);
            let mut s = (info.threat + 1.0) / d * if Some(t.id) == self.target { 1.6 } else { 1.0 };
            let off = norm(heading(pos, (t.x, t.y)) - obs.angle).abs();
            s *= 1.0 - off.min(150.0) / 300.0; // (rather what's in front than what's behind)
            if !self.can_hit(obs, &t, None) {
                s *= 0.3; // (rather what a shot gets to)
            }
            if best.as_ref().is_none_or(|(bs, _)| s > *bs) {
                best = Some((s, t));
            }
        }
        self.target = best.as_ref().map(|(_, t)| t.id);
        best.map(|(_, t)| t)
    }

    fn aim_and_fire(&mut self, obs: &Observation, t: &Thing) -> (f64, bool, i32) {
        let pos = obs.pos();
        let d = dist((t.x, t.y), pos);
        let slot = self.weapon_for(obs, t, d);
        let mut weapon = 0;
        let held = obs.weapon.unwrap_or(2);
        if slot != held && obs.tic - self.switch_tic > SWITCH_WAIT {
            weapon = slot;
            self.switch_tic = obs.tic;
        }
        let w = weapon_in(if (1..=7).contains(&held) { held } else { 2 }, obs.super_shotgun);
        // lead a moving target with a missile weapon
        let (mut ax, mut ay) = (t.x, t.y);
        if !w.hitscan && w.speed > 0.0 {
            let tof = d / w.speed;
            ax += t.vx * tof;
            ay += t.vy * tof;
        }
        let aim = heading(pos, (ax, ay));
        let info = mon(t.name);
        let tolerance = 1.5f64.max((info.radius * 0.7).atan2(d.max(1.0)).to_degrees());
        let on = norm(aim - obs.angle).abs() < tolerance;
        let held_fire = if !Self::in_reach(&w, t, d) {
            "out of reach"
        } else if held == 5 && !self.rocket_safe(obs, t) {
            "rocket not safe"
        } else if !self.can_hit(obs, t, None) {
            "no shot (too steep, or something in the way)"
        } else {
            ""
        };
        if held_fire != self.held_fire {
            if !held_fire.is_empty() {
                self.log.push(format!("tic {}: holding fire at {} {}, {d:.0} off: {held_fire}", obs.tic, t.name, t.id));
            }
            self.held_fire = held_fire.to_string();
        }
        (aim, on && held_fire.is_empty(), weapon)
    }

    /// Close enough to shoot at with w: inside its reach, or (at something that hurts her from
    /// anywhere) as far as enough of a shot still lands.
    pub fn in_reach(w: &Weapon, t: &Thing, d: f64) -> bool {
        let info = mon(t.name);
        if d >= ENGAGE_RANGE {
            return false;
        }
        if d < w.reach + if w.slot == 1 { info.radius } else { 0.0 } {
            return true;
        }
        info.attack == Attack::Hitscan && w.slot != 1 && w.hits(info.radius, d) >= FAR_HITS
    }

    /// Would a shot at t get to it, as Doom aims one (not too steep, nothing in the way); else out
    /// level, past it unless it's level with her.
    pub fn can_hit(&mut self, obs: &Observation, t: &Thing, height: Option<f64>) -> bool {
        let h = height.unwrap_or_else(|| mon(t.name).height);
        self.refresh_tic_cache(obs);
        let key = (t.id, h.to_bits());
        if let Some(&v) = self.hit_cache.get(&key) {
            return v;
        }
        let v = self.shot_clear(obs, (t.x, t.y), t.z, h);
        self.hit_cache.insert(key, v);
        v
    }

    fn shot_clear(&mut self, obs: &Observation, at: Point, z: f64, h: f64) -> bool {
        let gun = self.z(obs) + SHOT_HEIGHT;
        let d = dist(at, obs.pos()).max(1.0);
        let (mut lo, mut hi) = (z.max(gun - AUTOAIM * d), (z + h).min(gun + AUTOAIM * d));
        if hi <= lo {
            if !(z < gun && gun < z + h) {
                return false;
            }
            (lo, hi) = (gun - 1.0, gun + 1.0); // (out level: it only hits what's level with it)
        }
        let me = Body { x: obs.x, y: obs.y, z: gun - 0.75 * PLAYER_HEIGHT, height: PLAYER_HEIGHT }; // (the eye's at 0.75)
        self.refresh_tic_cache(obs);
        self.los.clear(me, Body { x: at.0, y: at.1, z: lo, height: hi - lo }, &self.heights_now)
    }

    /// Where to step for a shot at t she can see but can't hit: back from it when it's above
    /// her, towards it when it's below; None if neither way is safe.
    fn shot_way(&mut self, obs: &Observation, t: &Thing) -> Option<Point> {
        let away = self.away_from(obs, (t.x, t.y));
        if t.z <= self.z(obs) {
            if let Some(a) = away {
                let toward = (-a.0, -a.1);
                return self.safe(obs, toward, 64.0).then_some(toward);
            }
        }
        away
    }

    /// A barrel she can see right by a monster, well clear of her: shoot that.
    fn barrel_shot(&mut self, obs: &Observation) -> Option<Point> {
        let pos = obs.pos();
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().filter(|&b| dist(b, pos) > BARREL_CLEAR + 64.0 && self.visible.iter().any(|t| dist(b, (t.x, t.y)) < BARREL_NEAR + mon(t.name).radius)).collect();
        if barrels.is_empty() {
            return None;
        }
        self.refresh_tic_cache(obs);
        let me = Body { x: obs.x, y: obs.y, z: self.z(obs), height: PLAYER_HEIGHT };
        barrels
            .into_iter()
            .filter(|&b| self.los.clear(me, Body { x: b.0, y: b.1, z: self.floor_at(obs, b), height: 32.0 }, &self.heights_now))
            .reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a })
    }

    fn floor_at(&self, obs: &Observation, p: Point) -> f64 {
        self.nav.poly_at(p).and_then(|q| obs.height(self.nav.sector_of_poly(q))).map_or(0.0, |h| h.floor)
    }

    /// The way to go to get clear of the barrels by her: the way she's going already if still
    /// safe and clearing them, else a new one; None once clear (or she can't get so).
    fn away_from_barrels(&mut self, obs: &Observation, nearest: Point) -> Option<Point> {
        let pos = obs.pos();
        if obs.tic < self.barrel_rest_until {
            return None;
        }
        if self.barrel_way.is_none() {
            self.barrel_since = obs.tic;
        } else if obs.tic - self.barrel_since > BARREL_TICS {
            self.barrel_way = None;
            self.barrel_rest_until = obs.tic + BARREL_REST;
            return None;
        }
        if dist(nearest, pos) >= BARREL_CLEAR + if self.barrel_way.is_some() { 64.0 } else { 0.0 } {
            self.barrel_way = None;
            return None;
        }
        if let Some(way) = self.barrel_way {
            if self.safe(obs, way, 48.0) && self.clear_of_barrels(obs, way) {
                return Some(way);
            }
        }
        let way = self.away_from(obs, nearest);
        self.barrel_way = way.filter(|&w| self.clear_of_barrels(obs, w));
        self.barrel_way
    }

    /// Does going that way (a second's run) leave her further from every barrel near her?
    fn clear_of_barrels(&self, obs: &Observation, d: Point) -> bool {
        let pos = obs.pos();
        let ahead = (pos.0 + d.0 * 96.0, pos.1 + d.1 * 96.0);
        obs.barrels.iter().flatten().filter(|&&b| dist(b, pos) < BARREL_CLEAR).all(|&b| dist(b, ahead) > dist(b, pos))
    }

    /// Is a rocket at t sure not to blow up by her (E1M9's died at 88 health, nothing in sight)?
    fn rocket_safe(&self, obs: &Observation, t: &Thing) -> bool {
        let pos = obs.pos();
        let clear = super::bestiary::weapon(5).splash + 48.0;
        if dist((t.x, t.y), pos) < clear + mon(t.name).radius {
            return false;
        }
        let mut others: Vec<(f64, f64, f64)> = self.visible.iter().filter(|o| o.id != t.id).map(|o| (o.x, o.y, mon(o.name).radius)).collect();
        others.extend(obs.barrels.iter().flatten().map(|b| (b.0, b.1, 10.0)));
        // (E1M9: dead at full health, a demon 83 away: it came round a corner into the way)
        others.extend(self.remaining().iter().filter(|m| m.id != t.id && obs.tic - m.tic < ROCKET_WARY && matches!(mon(m.name).attack, Attack::Melee | Attack::Charge)).map(|m| (m.x, m.y, mon(m.name).radius + 128.0)));
        for (x, y, r) in others {
            if dist((x, y), pos) < clear + r {
                return false;
            }
            if segment_point_distance(pos, (t.x, t.y), (x, y)) < r + 12.0 && dist((x, y), pos) < dist((t.x, t.y), pos) {
                return false;
            }
        }
        true
    }

    /// The gun that'd see t off soonest, counting the switch and what its ammo is worth.
    fn weapon_for(&self, obs: &Observation, t: &Thing, d: f64) -> i32 {
        let held = Self::held(obs);
        let info = mon(t.name);
        let crowd = self.visible.len();
        let (mut best, mut cost) = (obs.weapon.unwrap_or(2), f64::INFINITY);
        for &slot in &SLOTS {
            if !held.contains(&slot) {
                continue;
            }
            let w = weapon_in(slot, obs.super_shotgun);
            let have = Self::ammo(obs, slot);
            if (w.ammo_per_shot > 0 && have < w.ammo_per_shot) || (slot == 5 && !self.rocket_safe(obs, t)) {
                continue;
            }
            let mut c = self.time_to_kill(&w, t, d);
            if w.ammo_per_shot > 0 {
                let shots = (f64::from(info.health) / (w.damage * w.hits(info.radius, d)).max(1.0)).max(1.0);
                let (per, low) = ammo_cost(slot);
                c += shots * f64::from(w.ammo_per_shot) * per * (low / f64::from(have)).max(1.0);
            }
            if slot == 7 && (info.boss || crowd >= 4) {
                c *= 0.3;
            }
            if Some(slot) != obs.weapon {
                c += SWITCH_COST;
            }
            if c < cost {
                best = slot;
                cost = c;
            }
        }
        best
    }

    /// Seconds of shooting it'd take (a lot, out of the gun's reach: anything else first).
    fn time_to_kill(&self, w: &Weapon, t: &Thing, d: f64) -> f64 {
        let info = mon(t.name);
        if !Self::in_reach(w, t, d) {
            return 1e4 + d;
        }
        let per_shot = w.damage * w.hits(info.radius, d);
        (f64::from(info.health) / per_shot.max(0.1)).max(1.0) * w.cycle / TICRATE
    }

    /// This tic's turn (positive is clockwise) towards `aim`.
    fn turn(&mut self, obs: &Observation, aim: Option<f64>) -> f64 {
        let Some(aim) = aim else {
            self.rate = 0.0;
            return 0.0;
        };
        let err = norm(aim - obs.angle);
        let want = AIM_TURN.min((2.0 * TURN_ACCEL * err.abs()).sqrt()).min(err.abs()).copysign(err);
        self.rate = (self.rate - TURN_ACCEL).max((self.rate + TURN_ACCEL).min(want));
        if err.abs() < 1.0 {
            self.rate = err;
        }
        -self.rate
    }

    // --- moving -----------------------------------------------------------------------------

    /// The missiles that'd hit her, soonest first: any that would if she went on as she's going,
    /// and any she's getting out of the way of that haven't gone by yet.
    fn incoming(&mut self, obs: &Observation) -> Vec<Thing> {
        let pos = obs.pos();
        let z = self.z(obs);
        let vel = self.velocity;
        let mut out: Vec<(f64, Thing)> = Vec::new();
        for t in obs.things.iter().flatten() {
            let Some(m) = missile(t.name) else { continue };
            if t.name == "LostSoul" && t.vx.hypot(t.vy) < 8.0 {
                continue;
            }
            // (relative to her, as she's going)
            let (rvx, rvy) = (t.vx - vel.0, t.vy - vel.1);
            let v2 = rvx * rvx + rvy * rvy;
            if v2 < 1.0 {
                continue;
            }
            let (rx, ry) = (pos.0 - t.x, pos.1 - t.y);
            let when = (rx * rvx + ry * rvy) / v2;
            if !(0.0 < when && when < DODGE_TICS as f64) {
                continue;
            }
            let miss = (t.x + rvx * when - pos.0).hypot(t.y + rvy * when - pos.1);
            let hz = t.z + t.vz * when;
            if !(z - 24.0 < hz && hz < z + PLAYER_HEIGHT + 24.0) {
                continue;
            }
            if !self.dodging.contains(&t.id) && miss > clearance(&m) {
                continue;
            }
            if self.dodging.insert(t.id) {
                self.log.push(format!("tic {}: dodging {} {} (due in {when:.0} tics, {miss:.0} off)", obs.tic, t.name, t.id));
            }
            out.push((when, t.clone()));
        }
        let here: BTreeSet<i32> = out.iter().map(|(_, t)| t.id).collect();
        self.dodging.retain(|i| here.contains(i));
        if here.is_empty() {
            self.dodge_way = None;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.into_iter().map(|(_, t)| t).collect()
    }

    /// The way to run that keeps her furthest from all of them as they go by, where the floor's
    /// safe; the way she's going already when it's as good (no dithering).
    fn dodge(&mut self, obs: &Observation, missiles: &[Thing]) -> Option<Point> {
        let pos = obs.pos();
        let mut ways: Vec<Point> = (0..DODGE_WAYS).map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / DODGE_WAYS as f64;
            (a.cos(), a.sin())
        }).collect();
        if let Some(w) = self.dodge_way {
            ways.push(w);
        }
        let mut best: Option<(f64, Point)> = None;
        for way in ways {
            if !self.safe(obs, way, 64.0) {
                continue;
            }
            let mut s = missiles.iter().filter_map(|t| missile(t.name).map(|m| miss_by(pos, self.velocity, way, t) - clearance(&m))).fold(f64::INFINITY, f64::min);
            s = s.min(48.0); // (clear is clear: then the rest decides)
            if let Some(dw) = self.dodge_way {
                s += 6.0 * (way.0 * dw.0 + way.1 * dw.1);
            }
            if best.is_none_or(|(bs, _)| s > bs) {
                best = Some((s, way));
            }
        }
        self.dodge_way = best.map(|b| b.1);
        self.dodge_way
    }

    /// Back off from `what`: straight away if the floor's safe that way, else as near that as is.
    fn away_from(&mut self, obs: &Observation, what: Point) -> Option<Point> {
        let away = (obs.y - what.1).atan2(obs.x - what.0);
        for off in [0.0f64, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0] {
            let a = away + off.to_radians();
            let d = (a.cos(), a.sin());
            if self.safe(obs, d, 64.0) {
                return Some(d);
            }
        }
        None
    }

    fn gun(&self, obs: &Observation) -> Weapon {
        weapon_in(obs.weapon.filter(|w| (1..=7).contains(w)).unwrap_or(2), obs.super_shotgun)
    }

    /// Away from t, aslant (the strafe's way and back), as far as it's safe.
    fn keeping_off(&mut self, obs: &Observation, t: &Thing, aim: Option<f64>) -> Option<Point> {
        let a = (obs.y - t.y).atan2(obs.x - t.x);
        let back = (a.cos(), a.sin());
        let mut ways = vec![back];
        if let Some(s) = self.strafe_dir(obs, aim) {
            let mixed = (back.0 + s.0, back.1 + s.1);
            let n = mixed.0.hypot(mixed.1);
            if n > 0.1 {
                ways.insert(0, (mixed.0 / n, mixed.1 / n));
            }
        }
        if let Some(w) = ways.into_iter().find(|&w| self.safe(obs, w, 64.0)) {
            return Some(w);
        }
        self.away_from(obs, (t.x, t.y))
    }

    /// Towards t, zig-zagging (the strafe's way and forwards), as far as it's safe.
    fn closing_in(&mut self, obs: &Observation, t: &Thing, strafe: Option<Point>) -> Option<Point> {
        let a = (t.y - obs.y).atan2(t.x - obs.x);
        let ahead = (a.cos(), a.sin());
        let mut ways = vec![ahead];
        if let Some(s) = strafe {
            let mixed = (ahead.0 + s.0, ahead.1 + s.1);
            let n = mixed.0.hypot(mixed.1);
            if n > 0.1 {
                ways.insert(0, (mixed.0 / n, mixed.1 / n));
            }
        }
        ways.into_iter().find(|&w| self.safe(obs, w, 64.0)).or(strafe)
    }

    /// Side to side across the line of fire, a second or so each way, where it's safe.
    fn strafe_dir(&mut self, obs: &Observation, aim: Option<f64>) -> Option<Point> {
        if obs.tic >= self.strafe_until {
            self.strafe = -self.strafe;
            let (lo, hi) = STRAFE_TICS;
            self.strafe_until = obs.tic + lo + (obs.tic * 7919).rem_euclid(hi - lo);
        }
        let aim = aim?;
        let a = (aim + 90.0 * self.strafe).to_radians();
        let d = (a.cos(), a.sin());
        if self.safe(obs, d, 48.0) {
            return Some(d);
        }
        self.strafe = -self.strafe;
        let d = (-d.0, -d.1);
        self.safe(obs, d, 48.0).then_some(d)
    }

    /// A monster near and in front: walking on would walk into it.
    fn blocking(&self, obs: &Observation, t: Option<&Thing>) -> bool {
        t.is_some_and(|t| dist((t.x, t.y), obs.pos()) < 320.0 && norm(heading(obs.pos(), (t.x, t.y)) - obs.angle).abs() < 60.0)
    }

    /// Is it all right to go `length` that way: floor all the way, no drop or step up she
    /// couldn't come back from, nothing that hurts, room to stand? And where her speed would
    /// carry her, pushing that way a moment and then letting go (E1M3: strafing by the start,
    /// slid into the nukage)?
    fn safe(&self, obs: &Observation, d: Point, length: f64) -> bool {
        let nav = &self.nav;
        let Some(here) = nav.poly_at(obs.pos()) else { return false };
        let floor_of = |p: usize| obs.height(nav.sector_of_poly(p)).unwrap_or(crate::nav::Heights { floor: 0.0, ceiling: 0.0 });
        let floor0 = floor_of(here).floor;
        let mut floor = floor0;
        let n = ((length / 16.0).floor() as usize).max(2);
        for i in 1..=n {
            let f = length * i as f64 / n as f64;
            let q = (obs.x + d.0 * f, obs.y + d.1 * f);
            let Some(p) = nav.poly_at(q) else { return false };
            let h = floor_of(p);
            if (h.floor - floor).abs() > MAX_STEP || h.ceiling - h.floor < PLAYER_HEIGHT || nav.damage[nav.sector_of_poly(p)] != 0 {
                return false;
            }
            floor = h.floor;
        }
        // (the slide: a wall or a step up stops her, which is all right; a drop or a hurting floor
        // isn't)
        let (mut x, mut y) = obs.pos();
        let (mut vx, mut vy) = self.velocity;
        floor = floor0;
        for k in 0..SLIDE_TICS {
            if k < PUSH_TICS {
                vx += RUN_THRUST * d.0;
                vy += RUN_THRUST * d.1;
            } else if vx * vx + vy * vy < 1.0 {
                break;
            }
            x += vx;
            y += vy;
            vx *= FRICTION;
            vy *= FRICTION;
            let Some(p) = nav.poly_at((x, y)) else { break };
            let h = floor_of(p);
            if h.floor - floor > MAX_STEP || h.ceiling - h.floor < PLAYER_HEIGHT {
                break;
            }
            if floor - h.floor > MAX_STEP || nav.damage[nav.sector_of_poly(p)] != 0 {
                return false;
            }
            floor = h.floor;
        }
        true
    }

    // --- helpers ----------------------------------------------------------------------------

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
fn clearance(m: &Missile) -> f64 {
    PLAYER_RADIUS + m.radius + DODGE_MARGIN + if m.splash { 64.0 } else { 0.0 }
}

/// How near t comes to her as it goes by, if she runs `way` from now.
fn miss_by(pos: Point, vel: Point, way: Point, t: &Thing) -> f64 {
    let (mut x, mut y) = pos;
    let (mut vx, mut vy) = vel;
    let mut nearest = f64::INFINITY;
    for k in 1..=DODGE_TICS {
        vx += RUN_THRUST * way.0;
        vy += RUN_THRUST * way.1;
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

pub fn segment_point_distance(a: Point, b: Point, p: Point) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let n = dx * dx + dy * dy;
    let u = if n == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / n).clamp(0.0, 1.0) };
    dist(p, (a.0 + dx * u, a.1 + dy * u))
}
