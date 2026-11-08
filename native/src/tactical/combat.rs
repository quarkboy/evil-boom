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
mod movement;
mod steering;
mod style;
mod tree;

use super::bestiary::{missile, monster, weapon_in, Attack, Missile};
use super::sound::Hearing;
use crate::geom::{dist, Point};
use crate::los::{Body, LineOfSight};
use crate::motor::{heading, norm, Controls, Observation, Thing, FRICTION, RUN_THRUST, TICRATE};
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
/// closing in on something out of reach this long without getting it in reach: left...
const CLOSE_GIVE_UP: i64 = 20 * 35;
/// ...for this long
const GIVEN_UP_TICS: i64 = 30 * 35;
/// a ceiling seen coming down this lately, this low over its floor, is a crusher at work
const CRUSH_TICS: i64 = 2 * 35;
const CRUSH_GAP: f64 = 120.0;
/// health lost lately: over this long (tics)
const DAMAGE_WINDOW: i64 = 5 * 35;
/// an awake monster out of sight this long is still fought (where it was)
const LOST_TICS: i64 = 3 * 35;
/// outmatched by an awake monster this near: keeping clear of it, not getting past
const KEEP_CLEAR: f64 = 1024.0;
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
/// monsters standing about (or by where she's going) she'd take longer than this to kill: left
/// be
pub const LEAVE_BE: f64 = 15.0;
/// ...or below this health, anything in sight
pub(super) const LOW_HEALTH: f64 = 35.0;
/// with something shooting in sight, keep this far from barrels
const BARREL_CLEAR: f64 = 160.0;
/// a barrel's height (seen over a ledge this high or not)
const BARREL_HEIGHT: f64 = 42.0;

/// a monster seen after another, not her, this long: they're fighting each other: leave them
const INFIGHT_TICS: i32 = 18;

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
    /// where the route's taking her mid-fight (an errand: health, a gun, ammo), for dodging
    /// towards (the user's: on to it while dodging, not stuck dodging where she stands)
    pub toward: Option<Point>,
    /// floors she knows have turned to something that hurts since the map began (kept from what
    /// she knows by the player)
    pub changed_floors: BTreeSet<usize>,
    /// floors a walked-over line lowers into something that hurts (going down: as good as hurting)
    sinks_into_harm: BTreeSet<usize>,
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
    /// health lost lately: (tic, how much)
    damage: Vec<(i64, f64)>,
    /// (the log's: rounds spent this fight, and the health monsters lost, as the engine has it;
    /// nothing's decided by them)
    spent: f64,
    dealt: f64,
    ammo_was: Vec<(i32, i32)>,
    health_was: BTreeMap<i32, i32>,
    /// each sector's ceiling last tic, and when it was last seen coming down
    ceilings: Vec<f64>,
    ceiling_down: Vec<i64>,
    /// out of reach, closed in on since when, by id...
    closing_since: BTreeMap<i32, i64>,
    /// ...and given up on till when
    given_up: BTreeMap<i32, i64>,
    /// the gun last fired and when (its reload)
    pub(super) last_shot: Option<(i32, i64)>,
}

impl Combat {
    pub fn new(nav: Rc<Navigator>) -> Combat {
        let sinks_into_harm = (0..nav.map.sectors.len())
            .filter(|&s| nav.triggers.iter().any(|t| t.info.trigger.walked() && t.effect(s).is_some() && nav.floor_after(t, s) == Some(true)))
            .collect();
        Combat {
            los: LineOfSight::new(&nav.map),
            hearing: Hearing::new(&nav.map),
            nav,
            known: BTreeMap::new(),
            alive: BTreeMap::new(),
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
            held_fire: String::new(),
            hold_fire: false,
            infight: BTreeMap::new(),
            heights_tic: None,
            heights_now: Vec::new(),
            hit_cache: BTreeMap::new(),
            dodging: BTreeSet::new(),
            dodge_way: None,
            toward: None,
            changed_floors: BTreeSet::new(),
            sinks_into_harm,
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
            damage: Vec::new(),
            spent: 0.0,
            dealt: 0.0,
            ammo_was: Vec::new(),
            health_was: BTreeMap::new(),
            ceilings: Vec::new(),
            ceiling_down: Vec::new(),
            closing_since: BTreeMap::new(),
            given_up: BTreeMap::new(),
            last_shot: None,
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
        }
        for (i, h) in obs.heights.iter().enumerate() {
            if h.ceiling < self.ceilings[i] - 0.01 {
                self.ceiling_down[i] = obs.tic;
            }
            self.ceilings[i] = h.ceiling;
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
        // (barrels as monsters are seen: in her view (or near), nothing in the way; kept while
        // they're there)
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        // (the same barrel pushed by shots is still the one she saw)
        self.barrels_seen = self.barrels_seen.iter().filter_map(|&b| crate::motor::follower::barrel_now(obs, b)).collect();
        self.barrels_seen.dedup_by(|a, b| dist(*a, *b) < 1.0);
        for &b in &barrels {
            let d = dist(b, pos);
            if d >= ENGAGE_RANGE || self.barrels_seen.iter().any(|&x| dist(x, b) < 1.0) {
                continue;
            }
            if !(d < NEAR || norm(heading(pos, b) - obs.angle).abs() < FOV) {
                continue;
            }
            let floor = self.nav.poly_at(b).map_or(0.0, |p| self.heights_now.get(self.nav.sector_of_poly(p)).map_or(0.0, |h| h.0));
            if self.los.clear(me, Body { x: b.0, y: b.1, z: floor, height: BARREL_HEIGHT }, &self.heights_now) {
                self.barrels_seen.push(b);
            }
        }
        self.watch_infighting(obs);
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
            if n == INFIGHT_TICS {
                self.log.push(format!("tic {}: {} {} is fighting another: leaving it to it", obs.tic, t.name, t.id));
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

    /// If t is standing about, and a shot would be heard by enough monsters she's seen standing
    /// about too that she'd be outmatched once they're awake: what it'd wake (else "").
    fn would_wake(&mut self, obs: &Observation, t: &Thing) -> String {
        let Some(s) = self.known.get(&t.id) else { return String::new() };
        if s.awake {
            return String::new(); // (the fist too wakes what hears it: P_FireWeapon's noise alert, no weapon WPF_SILENT)
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

    // --- deciding ---------------------------------------------------------------------------

    /// What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
    /// something that wants the controls (a run-up, a jump). `errand`: the route's going for
    /// something to fight with: the fight only aims and shoots on the way.
    pub fn decide(&mut self, obs: &Observation, busy: bool, errand: &str) -> Option<Decision> {
        let missiles = self.incoming(obs);
        let target = self.pick_target(obs);
        if target.is_none() && missiles.is_empty() {
            return self.no_target(obs, busy, errand);
        }
        let (s, fire, weapon) = self.size_up(obs, busy, errand, missiles, target);
        let mut act = tree::engage(self, obs, &s);
        // (a rocket on its way out, 8 tics after the trigger, the way she faces then: if that way
        // it'd go off near her, turned to the nearest way it won't, the last resort, the trigger
        // pressed and no calling it back; the user's)
        if self.last_shot.is_some_and(|(slot, tic)| slot == 5 && ((obs.tic - tic) as f64) < aiming::ROCKET_DELAY) && s.vile.is_none() {
            if let Some(h) = self.rocket_way_out(obs) {
                act.aim = Some(h);
                act.why = "turned off a rocket's way out into something near".into();
            }
        }
        let aim = act.aim.or(s.aim);
        let fire = fire && !act.hold_fire && !self.hold_fire;
        let why = act.why;
        let takeover = (!busy && !["shooting on the way", "getting past", "out of", "sneaking"].iter().any(|p| why.starts_with(p))) || !s.missiles.is_empty() || s.vile.is_some();
        if why != self.mode {
            let t = s.target.as_ref().map_or(String::new(), |t| format!(" (target {} {}, {:.0} threat; health {:.0})", t.name, t.id, s.threat, s.health));
            self.log.push(format!("tic {}: {why}{t}", obs.tic));
            self.mode = why.clone();
        }
        Some(Decision { takeover, aim, fire, weapon, moving: act.moving, why })
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
        let health = obs.health.unwrap_or(100.0);
        let threat = self.threats();
        let outmatched = self.outmatched(obs);
        if let Some(t) = target.as_ref().filter(|t| self.reachable(obs, t)) {
            self.closing_since.remove(&t.id); // (got it in reach: the clock again, next time)
        }
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
        let mut barrel_away = None;
        // (with something shooting in sight, or her shooting at something: a shot, hers or theirs,
        // sets a barrel off; E1M8's demons, biters only, fought among the barrels)
        if first_missile.is_none() && !busy && (shooting || target.is_some()) {
            if let Some(b) = barrel.filter(|&b| dist(b, pos) < BARREL_CLEAR + 64.0) {
                barrel_away = self.away_from_barrels(obs, b);
            }
        }
        let vile = if busy { None } else { self.vile_fire(obs) };
        let s = tree::Situation { pos, busy, errand, missiles, target, aim, health, threat, outmatched, close, barrel_away, vile };
        (s, fire, weapon)
    }

    /// Closing in on t, out of reach: still worth it? Not once she's been at it this long without
    /// getting it in reach (E1M9's imps across the gap, 800 off, closed in on and lost sight of
    /// for an hour): then it's left a while.
    fn still_closing(&mut self, obs: &Observation, id: i32) -> bool {
        if self.given_up.get(&id).is_some_and(|&until| obs.tic < until) {
            return false;
        }
        let since = *self.closing_since.entry(id).or_insert(obs.tic);
        if obs.tic - since > CLOSE_GIVE_UP {
            self.closing_since.remove(&id);
            self.given_up.insert(id, obs.tic + GIVEN_UP_TICS);
            self.log.push(format!("tic {}: can't get at {id}: leaving it a while", obs.tic));
            return false;
        }
        true
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
            .filter(|s| self.given_up.get(&s.id).is_none_or(|&until| obs.tic >= until))
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
        if let Some(s) = obs.weapon.filter(|_| c.attack) {
            self.last_shot = Some((s, obs.tic));
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
    /// under one fighting a demon, crushed)?
    pub fn crushing(&self, obs: &Observation, s: usize) -> bool {
        self.ceiling_down.get(s).is_some_and(|&t| obs.tic - t < CRUSH_TICS) && obs.heights.get(s).is_some_and(|h| h.ceiling - h.floor < CRUSH_GAP)
    }

    /// Health lost a second lately (over the last few seconds).
    pub fn damage_rate(&mut self, tic: i64) -> f64 {
        self.damage.retain(|&(t, _)| tic - t < DAMAGE_WINDOW);
        self.damage.iter().map(|(_, d)| d).sum::<f64>() / (DAMAGE_WINDOW as f64 / TICRATE)
    }

    /// Seconds till she's dead at the rate she's been losing health lately (a lot: not losing any).
    pub fn time_to_death(&mut self, obs: &Observation) -> f64 {
        let rate = self.damage_rate(obs.tic);
        if rate <= 0.0 {
            return f64::INFINITY;
        }
        obs.health.unwrap_or(100.0) / rate
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
