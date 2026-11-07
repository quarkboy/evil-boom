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
mod movement;
mod steering;

use super::bestiary::{missile, monster, weapon_in, Attack, Missile};
use super::sound::Hearing;
use crate::geom::{dist, Point};
use crate::los::{Body, LineOfSight};
use crate::motor::{heading, norm, Controls, Observation, Thing, FRICTION, RUN_THRUST};
use crate::navigator::{Navigator, PLAYER_HEIGHT, PLAYER_RADIUS};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

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
/// an awake monster out of sight this long is still fought (where it was)
const LOST_TICS: i64 = 3 * 35;
/// no shooting at anything further
const ENGAGE_RANGE: f64 = 2048.0;

/// a missile arriving sooner than this is dodged...
const DODGE_TICS: i64 = 35;
/// ...if it would pass nearer than the two radii and this
const DODGE_MARGIN: f64 = 12.0;

/// a rocket's blast reaches this far (less her radius: 128 in all)
const SPLASH: f64 = 112.0;

/// a clawing monster nearer than this: back off from it
const CLOSE: f64 = 160.0;
/// this much threat in sight: stand and fight rather than walk on
const STAND_THREAT: f64 = 6.0;
/// ...unless killing what's in sight would take longer than this (s), or more ammo than she
/// has: then get past it
const OUTMATCHED: f64 = 30.0;
/// monsters standing about (or by where she's going) she'd take longer than this to kill: left
/// be
pub const LEAVE_BE: f64 = 15.0;
/// of what she shoots that would land (the spread allowed for), this much does (her ammo's
/// worth, roughly)
const AMMO_LANDS: f64 = 0.9;
/// ...outmatched only clearly short of it (fifty rockets for a cyberdemon will about do)
const SHORT_BY: f64 = 0.8;
/// fewer shots than this for the guns held: supplies come first
const SHORT_OF_AMMO: i32 = 15;
/// ...or below this health, anything in sight
const LOW_HEALTH: f64 = 35.0;
/// with something shooting in sight, keep this far from barrels
const BARREL_CLEAR: f64 = 160.0;

/// a monster facing another this nearly...
const INFIGHT_FACING: f64 = 30.0;
/// ...and not her...
const INFIGHT_NOT_HER: f64 = 45.0;
/// ...this long: they're fighting each other: leave them
const INFIGHT_TICS: i32 = 18;

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
    /// the trigger was down last tic
    trigger_held: bool,
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
            steer_way: None,
            steer_tic: -1,
            steer_still: 0,
            steer_stuck: None,
            vile_way: None,
            vile_until: 0,
            trigger_held: false,
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
            if let Some(before) = self.health.filter(|&before| h < before) {
                self.hurt_tic = obs.tic;
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
            obs.things.iter().flatten().filter(|t| monster(t.name).is_some() && t.name != "CommanderKeen").map(|t| (t.id, t.clone())).collect();
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
                if norm(t.angle - heading((t.x, t.y), (o.x, o.y))).abs() < INFIGHT_FACING
                    && (moving || norm(o.angle - heading((o.x, o.y), (t.x, t.y))).abs() < INFIGHT_FACING)
                {
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
    /// pistol: MAP08 at pistol start), or more ammo than she has (a cyberdemon, ten rockets)?
    pub fn outmatched(&self, obs: &Observation) -> bool {
        let near: Vec<&'static str> = self.visible.iter().filter(|t| dist((t.x, t.y), obs.pos()) < 1200.0).map(|t| t.name).collect();
        self.too_much(obs, &near, OUTMATCHED)
    }

    /// Would these take her longer than `secs` to kill, or more than her ammo's worth?
    pub fn too_much(&self, obs: &Observation, names: &[&str], secs: f64) -> bool {
        let hp: f64 = names.iter().map(|n| f64::from(mon(n).health)).sum();
        if hp / self.firepower(obs) > secs {
            return true;
        }
        // (what each kind of ammo's worth, with the best gun she has for it; bosses take no blast)
        let held = Self::held(obs);
        let boss = names.iter().any(|n| mon(n).boss);
        let radius = names.iter().map(|n| mon(n).radius).fold(20.0, f64::max);
        let mut worth = 0.0;
        for pool in [&[2, 4][..], &[3], &[5], &[6, 7]] {
            let best = pool
                .iter()
                .filter(|s| held.contains(s))
                .map(|&s| {
                    let w = weapon_in(s, obs.super_shotgun);
                    let per = if boss && s == 5 { w.damage / 2.0 } else { w.damage };
                    f64::from(Self::ammo(obs, s) / w.ammo_per_shot.max(1)) * per * w.hits(radius, 384.0)
                })
                .fold(0.0, f64::max);
            worth += best;
        }
        let melee = held.contains(&1) && self.firepower(obs) == weapon_in(1, None).dps();
        !melee && worth * AMMO_LANDS < SHORT_BY * hp
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
        let sleepers: Vec<Seen> = self
            .remaining()
            .into_iter()
            .filter(|m| !m.awake && self.nav.poly_at(m.pos()).is_some_and(|p| heard.contains(&self.nav.sector_of_poly(p))))
            .collect();
        let hp: f64 = sleepers.iter().map(|m| f64::from(mon(m.name).health)).sum();
        if sleepers.len() < 2 || hp / self.firepower(obs) <= LEAVE_BE {
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
            if !busy && errand.is_empty() {
                // (hidden from an arch-vile's fire: staying hidden comes first)
                if let Some((name, way)) = self.vile_fire(obs) {
                    let why = format!("out of the {name}'s sight");
                    if why != self.mode {
                        self.log.push(format!("tic {}: {why}", obs.tic));
                        self.mode = why.clone();
                    }
                    return Some(Decision { takeover: true, aim: None, fire: false, weapon: 0, moving: way, why });
                }
                if let Some(d) = self.lost_sight(obs) {
                    return Some(d);
                }
            }
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
                    fire = norm(a - obs.angle).abs() < 0.75f64.max(8.0f64.atan2(d).to_degrees())
                        && d < weapon_in(obs.weapon.unwrap_or(2), obs.super_shotgun).reach
                        && self.shot_clear(obs, shot, floor, 32.0);
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
        let vile = if busy { None } else { self.vile_fire(obs) };
        if let Some((name, way)) = vile {
            // (its fire goes off only if it can still see her)
            moving = way;
            why = format!("out of the {name}'s sight");
        } else if let Some(m) = &first_missile {
            moving = self.dodge(obs, &missiles);
            why = format!("dodging {}", m.name);
        } else if let Some(away) = barrel_away {
            // (MAP23: a mancubus's fireball set off the barrels she stood by)
            moving = Some(away);
            why = "away from a barrel".into();
        } else if busy {
            why = "shooting on the way".into();
        } else if let Some(nearest) = close.iter().min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos))) {
            let t = nearest.clone();
            let gun = self.preferred_gun(obs, &t);
            moving = self.steer(obs, Some(&t), &gun).or_else(|| self.away_from(obs, (t.x, t.y)));
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
        } else if let Some(way) = target
            .as_ref()
            .filter(|t| self.visible.len() == 1 && dist((t.x, t.y), pos) < 512.0 && !self.can_hit(obs, t, None))
            .and_then(|t| self.shot_way(obs, t))
        {
            moving = Some(way);
            why = format!("stepping for a shot at {}", target.as_ref().map_or("", |t| t.name));
        } else if outmatched {
            why = "getting past".into(); // (shooting on the way, the route going on)
        } else if let Some(way) = target
            .as_ref()
            .filter(|t| mon(t.name).attack != Attack::None && dist((t.x, t.y), pos) < aiming::window(&self.preferred_gun(obs, t), t).0)
            .and_then(|t| {
                let gun = self.preferred_gun(obs, t);
                self.steer(obs, Some(t), &gun).or_else(|| self.keeping_off(obs, t, aim))
            })
        {
            // (too near for the gun she has, or for them to miss)
            moving = Some(way);
            why = format!("keeping off from {}", target.as_ref().map_or("", |t| t.name));
        } else if threat >= STAND_THREAT || health < LOW_HEALTH || self.blocking(obs, target.as_ref()) {
            let mut w = "standing and fighting".to_string();
            if let Some(t) = &target {
                let gun = self.preferred_gun(obs, t);
                let d = dist((t.x, t.y), pos);
                moving = self.steer(obs, Some(t), &gun);
                if !Self::in_reach(&gun, t, d) || d > aiming::window(&gun, t).1 {
                    moving = moving.or_else(|| self.closing_in(obs, t, None));
                    w = format!("closing in on {}", t.name);
                }
            } else {
                moving = self.strafe_dir(obs, aim);
            }
            why = w;
        } else {
            why = "shooting on the way".into();
        }
        if self.hold_fire {
            fire = false;
        }
        let takeover = (!busy && !["shooting on the way", "getting past", "out of", "sneaking"].iter().any(|p| why.starts_with(p))) || first_missile.is_some() || vile.is_some();
        if why != self.mode {
            let t = target.as_ref().map_or(String::new(), |t| format!(" (target {} {}, {threat:.0} threat; health {health:.0})", t.name, t.id));
            self.log.push(format!("tic {}: {why}{t}", obs.tic));
            self.mode = why.clone();
        }
        Some(Decision { takeover, aim, fire, weapon, moving, why })
    }

    /// An awake monster that was after her gone out of sight a moment ago (behind a pillar,
    /// round a corner): she keeps fighting it, aimed where it was, keeping her distance, rather
    /// than going about her business (MAP32: walked up to a cyberdemon behind a pillar).
    fn lost_sight(&mut self, obs: &Observation) -> Option<Decision> {
        let pos = obs.pos();
        let s = self
            .remaining()
            .into_iter()
            .filter(|s| s.awake && obs.tic - s.tic < LOST_TICS && mon(s.name).attack != Attack::None && dist(s.pos(), pos) < ENGAGE_RANGE)
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
        Some(Decision { takeover: true, aim: Some(heading(pos, s.pos())), fire: false, weapon: 0, moving, why })
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
        // (the rocket launcher and the BFG don't fire again with the trigger held: let go a tic
        // after each press, the game's way to fire the next; held, only the first went)
        if matches!(obs.weapon, Some(5 | 7)) && self.trigger_held {
            c.attack = false;
        }
        self.trigger_held = c.attack;
        if d.weapon != 0 {
            c.weapon = d.weapon;
        }
        c
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
    PLAYER_RADIUS + m.radius + DODGE_MARGIN + if m.splash { SPLASH } else { 0.0 }
}

/// How near t comes to her as it goes by, if she runs `way` from now (stopped once she's gone
/// `room`: a wall that way; a rocket by a wall she'd have run into met her there). A rocket's
/// counted from its middle: its blast (the clearance) does the rest.
fn miss_by(pos: Point, vel: Point, way: Point, room: f64, t: &Thing) -> f64 {
    let (mut x, mut y) = pos;
    let (mut vx, mut vy) = vel;
    let mut nearest = f64::INFINITY;
    for k in 1..=DODGE_TICS {
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
