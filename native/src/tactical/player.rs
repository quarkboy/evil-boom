//! Evil playing with monsters about: the explorer finds the way, the fight (Combat) takes the
//! controls when it needs them, and between them she goes for what she needs to keep fighting
//! (weapons, ammo, health, armour she has seen) and, once there's nothing else, hunts down the
//! monsters she knows are left (some maps open only when they're dead: E1M8's barons).

use std::collections::BTreeSet;
use std::rc::Rc;

use super::bestiary::{ammo_item, armor_item, first_shot, health_item, monster, weapon_in, weapon_item, Attack, CHAINSAW, SUPER_SHOTGUN};
use super::combat::{segment_point_distance, spread, Combat, Seen, LEAVE_BE};
use crate::geom::{dist, Point};
use crate::knowledge::completion::Completionist;
use crate::knowledge::explorer::{Explorer, Hooks, Kind, Objective};
use crate::knowledge::model::Knowledge;
use crate::los::Body;
use crate::motor::follower::RouteFollower;
use crate::motor::{Controls, Observation, Thing};
use crate::navigator::{Navigator, States, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::walkthrough::{Action, Step};

/// the most of each ammo she can carry (without a backpack), by weapon slot
use crate::tactical::bestiary::ammo_max;

/// items further than this (as the crow flies) only when they're needed badly
const ITEM_RANGE: f64 = 1536.0;
/// a health pickup the route's going for this near is gone on to mid-fight
const HEAL_NEAR: f64 = 512.0;
/// a monster's last known spot: hunted from the reachable poly nearest it, this near
const HUNT_NEAR: f64 = 512.0;
/// below this health, health items come first
const HURT: f64 = 50.0;
/// monsters this near an objective's spot count against going there
const DANGER_NEAR: f64 = 640.0;
/// the way to something urgent passes nothing she can't take on closer than this (and its radius)
const WAY_CLEAR: f64 = 320.0;
/// ammo for what she has counts as this much further, outmatched by something she knows of
const AMMO_OUTMATCHED: f64 = 2.0;
/// a switch counts as this much as far, with a better gun seen that she can't get to
const SWITCH_FOR_GUN: f64 = 0.25;
/// below this, with something shooting at her: back off out of its sight
const RETREAT_HEALTH: f64 = 40.0;
/// cover looked for among the places she's been this near...
const COVER_RANGE: f64 = 768.0;
/// ...the nearest this many of them
const COVER_SPOTS: usize = 48;
/// a retreat is over once nothing that shoots has been in sight this long
const CLEAR_TICS: i64 = 35;
/// a step this far ahead, into a firing sniper's sight from cover, held back
const EDGE_LOOK: f64 = 48.0;
/// a way across ground in sight of a hitscan shooter she can't take on counts this many times
/// its length...
const EXPOSED: f64 = 3.0;
/// ...that ground judged this far from it at most...
const EXPOSURE_RANGE: f64 = 2048.0;
/// ...and again once it's moved this far, or this long on
const EXPOSURE_MOVED: f64 = 64.0;
const EXPOSURE_AGAIN: i64 = 35;
/// a post (cover to fight from) looked for among the places she knows of this near...
const POST_RANGE: f64 = 1536.0;
/// ...the nearest this many of them...
const POST_LOOKS: usize = 3000;
/// ...looked at on a grid this fine...
const POST_GRID: f64 = 48.0;
/// ...each counting for this much nearer for every unit it's further from them
const POST_FAR: f64 = 0.5;
/// ...with somewhere they'd see her this near it (to step out to and shoot from)
const POST_PEEK: f64 = 128.0;
/// on the way to one this long at most
const POST_TICS: i64 = 6 * 35;
/// after a retreat that found no cover, not again for this long
const RETREAT_AGAIN: i64 = 3 * 35;
/// monsters this near, this many of them, spread round her this far (degrees): get out
const SURROUNDED_NEAR: f64 = 640.0;
const SURROUNDED_COUNT: usize = 3;
const SURROUNDED_SPREAD: f64 = 160.0;
/// getting out: at most this long (then the fight goes on from there)
const OUT_TICS: i64 = 4 * 35;
/// out of ammo: off for ammo this long at most, before thinking again
const AMMO_TICS: i64 = 20 * 35;
/// out of ammo and none known: away to somewhere she's been at most this far
const AMMO_AWAY: f64 = 1536.0;
/// a way away is no nearer than this to any monster than she is now
const AMMO_MARGIN: f64 = 16.0;
/// a way somewhere passes no monster nearer than this
const PAST_CLEAR: f64 = 96.0;
/// monsters whose attacks, landing on others, set them fighting
const HEAVIES: [&str; 7] = ["Cyberdemon", "SpiderMastermind", "Fatso", "Arachnotron", "Revenant", "BaronOfHell", "HellKnight"];
/// a heavy taking longer than this (s) to kill with what she has: let others take its shots...
const BAIT_HEAVY: f64 = 8.0;
/// ...from a place she's been this near...
const BAIT_RANGE: f64 = 768.0;
/// ...and not again for this long
const BAIT_AGAIN: i64 = 8 * 35;
/// an arch-vile she'd take longer than this (s) to kill: into cover
const VILE_KILL: f64 = 2.0;
/// somewhere better looked for among the places she's been this near
const SPOT_RANGE: f64 = 1024.0;
/// firepower (damage a second) no better than this: the pistol's, about
const PISTOL_ONLY: f64 = 25.0;
/// units a second she gets along a route, about (turns, doors, lifts)
const RUN_SPEED: f64 = 400.0;
/// the trail back: a place every this far, at most this many
const TRAIL_STEP: f64 = 48.0;
const TRAIL_LEN: usize = 96;
/// an ambush: losing health faster than this (a second), or this many close...
const AMBUSH_RATE: f64 = 6.0;
const AMBUSH_COUNT: usize = 3;
const AMBUSH_NEAR: f64 = 512.0;
/// ...where it's tight (the mean way out round her this short a share of this)...
const TIGHT: f64 = 0.45;
const OPEN_LOOK: f64 = 256.0;
/// ...or they're spread round her this far (degrees)
const SIDES: f64 = 120.0;
/// backing out: to somewhere on the way she came at least this far back...
const BACK_MIN: f64 = 128.0;
/// ...that none of them see, or this far from them all; at most this long
const BACK_CLEAR: f64 = 768.0;
const BACK_TICS: i64 = 6 * 35;
/// a hit this big with nothing close: a blast
const BLAST_HIT: f64 = 40.0;
/// below this health, losing it fast enough to be dead within this long (s): health first
const SURVIVE_HEALTH: f64 = 80.0;
const SURVIVE_SECS: f64 = 8.0;
/// died going for something this many times: not again
const DEATHS_GIVE_UP: usize = 2;
/// a goal this near where she's died this many times (any reason but want of ammo) waits; this
/// many, even when urgent
const DEATH_ZONE: f64 = 256.0;
const DEATH_ZONE_COUNT: usize = 2;
const DEATH_ZONE_URGENT: usize = 3;
/// a goal this near where she died going for the same kind of thing is the same
const SAME_GOAL: f64 = 96.0;
/// monsters this near the way to an item can shoot at her on it
const EXPOSED_NEAR: f64 = 768.0;
/// of a monster's threat (damage a second, roughly), this much lands
const HITS: f64 = 0.5;
/// below this, whatever an item's worth, the way to it's all right
const COST_FLOOR: f64 = 10.0;
/// seconds a better gun is worth going for anyway (the fights to come)
const GUN_WORTH: f64 = 10.0;
/// barrels this near her (as the crow flies) are cleared...
const BARREL_NEAR: f64 = 1024.0;
/// ...from a place she can get to this near one (the follower finds where to shoot from)...
const BARREL_SPOT: f64 = 256.0;
/// ...with this much for the pistol, or this many shells for the shotgun
const BARREL_BULLETS: i32 = 10;
const BARREL_SHELLS: i32 = 12;

/// (barons' and knights' fireballs don't hurt each other)
fn species(name: &str) -> &str {
    if name == "HellKnight" {
        "BaronOfHell"
    } else {
        name
    }
}

fn mhealth(name: &str) -> f64 {
    monster(name).map_or(0.0, |m| f64::from(m.health))
}

/// What the player's hooks into the explorer need: the fight's state.
struct Fight {
    combat: Combat,
    /// what the hooks had to say (logged once the explorer's done)
    pending_log: Vec<String>,
    /// better guns she's turned aside for already
    offered: BTreeSet<i32>,
    present: BTreeSet<i32>,
    /// --complete: every counted item too, and the level again for ones out of reach
    completion: Option<Completionist>,
    /// what she was doing each time she died (lives before this one)
    deaths: Vec<Death>,
    /// the way's been worked out under new costs: choose again (once)
    rethink: bool,
    /// a better gun seen that she can't get to (as of the last list of things to do)
    gun_out_of_reach: bool,
}

/// A death: where, and what she was going for.
#[derive(Clone, Debug)]
struct Death {
    at: Point,
    kind: Kind,
    goal: Point,
    label: String,
    why: &'static str,
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
    fn why(&self) -> &'static str {
        if self.last_hit >= BLAST_HIT && !self.close {
            "a blast"
        } else if !self.armed {
            "out of ammo"
        } else if self.hurting_floor {
            "on a floor that hurts"
        } else if self.surrounded {
            "surrounded"
        } else if self.outmatched {
            "outmatched"
        } else {
            "worn down"
        }
    }
}

/// Why she's falling back: each kind starts on its own rule (`Player::start_retreat`, in that
/// order) and ends on its own (`Player::retreat_over`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetreatKind {
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
}

impl RetreatKind {
    fn name(self) -> &'static str {
        match self {
            RetreatKind::Cover => "cover",
            RetreatKind::ForAmmo => "for ammo",
            RetreatKind::Back => "back",
            RetreatKind::Out => "out",
            RetreatKind::Bait => "bait",
            RetreatKind::Post => "post",
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
    /// when the last retreat started or found no cover
    retreat_tic: i64,
    /// when something that shoots was last in sight
    shooters_tic: i64,
    /// where she was last tic (where she died, at a restart)
    last_pos: Point,
    /// and what things were like for her
    plight: Plight,
    /// places she's stood lately with nothing shooting at her and nothing hurting her, oldest
    /// first: the way back out of an ambush
    trail: Vec<Point>,
    /// the ground in sight of hitscan shooters she can't take on, as last worked out: when, and
    /// from where they were
    exposure: Option<(i64, Vec<Point>)>,
    /// when holding at the edge of cover was last logged
    edge_log: i64,
}

impl Player {
    pub fn new(nav: Rc<Navigator>, knowledge: Knowledge) -> Player {
        Player {
            explorer: Explorer::new(Rc::clone(&nav), knowledge),
            fight: Fight { combat: Combat::new(nav), pending_log: Vec::new(), offered: BTreeSet::new(), present: BTreeSet::new(), completion: None, deaths: Vec::new(), rethink: false, gun_out_of_reach: false },
            retreat: None,
            bait_tic: -1_000_000_000,
            retreat_tic: -1_000_000_000,
            shooters_tic: -1_000_000_000,
            last_pos: (0.0, 0.0),
            plight: Plight::default(),
            trail: Vec::new(),
            exposure: None,
            edge_log: i64::MIN / 2,
        }
    }

    /// Every item that counts as well (the exit held back till nothing's left: --complete);
    /// `restarts` levels again she may ask for, for ones out of reach.
    pub fn complete(&mut self, restarts: i32) {
        self.fight.completion = Some(Completionist::new(&mut self.explorer, restarts));
    }

    pub fn combat(&self) -> &Combat {
        &self.fight.combat
    }

    /// Monsters she's seen that are still about: (id, kind, where last seen, when, awake).
    pub fn known_monsters(&self) -> Vec<(i32, &'static str, crate::geom::Point, i64, bool)> {
        self.fight.combat.remaining().into_iter().map(|s| (s.id, s.name, s.pos(), s.tic, s.awake)).collect()
    }

    /// Her options now, best first, the fight's own wants in (supplies, barrels).
    pub fn ranked(&mut self, obs: &Observation, risky: bool) -> Vec<crate::knowledge::explorer::Objective> {
        self.explorer.ranked(obs, risky, &mut self.fight)
    }

    pub fn restart(&mut self) {
        // (dead with something on: noted, so as not to walk into it again the same way; MAP29's
        // green armor down in the nukage under fire, five lives running. A level again asked for
        // for what's left has nothing on)
        if let Some(cur) = &self.explorer.current {
            let d = Death { at: self.last_pos, kind: cur.kind, goal: cur.goal, label: cur.label.clone(), why: self.plight.why() };
            self.explorer.log.push(format!("  [fight] died at ({:.0}, {:.0}) going for: {} ({})", d.at.0, d.at.1, d.label, d.why));
            self.fight.deaths.push(d);
        }
        self.explorer.restart();
        self.fight.combat.restart();
        self.retreat = None;
        self.fight.combat.hold_fire = false;
        self.trail.clear();
    }

    pub fn act(&mut self, obs: &Observation, last: Option<&Controls>) -> Controls {
        self.last_pos = obs.pos();
        self.fight.present = obs.things.iter().flatten().map(|t| t.id).collect();
        self.fight.combat.observe(obs);
        self.plight = self.plight_now(obs);
        self.keep_trail(obs);
        for line in std::mem::take(&mut self.fight.combat.log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        self.keep_out_of_sight(obs);
        // (in a jump, or crossing a crusher it's timed: the route keeps the legs, the fight shoots;
        // E2M2 seed 7, a crossing begun, an imp seen 2000 off, the fight's steering under it)
        let busy = self.explorer.follower.as_ref().is_some_and(|f| f.jumping() || f.crossing_crusher());
        let (errand, toward) = if busy { (String::new(), None) } else { self.fight.errand(&self.explorer, obs).map_or((String::new(), None), |(e, at)| (e, Some(at))) };
        self.fight.combat.toward = toward;
        self.fight.combat.routed = self.explorer.follower.as_ref().is_some_and(|f| !f.done && !f.failed);
        // (what she knows of floors gone to something that hurts, for where the fight takes her)
        self.fight.combat.changed_floors = self.explorer.knowledge.changed_floor.iter().filter(|(_, &h)| h).map(|(&s, _)| s).collect();
        // (and the crushers she knows of, for where they could have her if they start again)
        self.fight.combat.crushers = self.explorer.knowledge.crusher_sectors();
        let d = self.fight.combat.decide(obs, busy, &errand);
        let retreat = !busy && self.retreating(obs);
        if retreat {
            self.explorer.hold(obs, last);
            let mut c = self.retreat.as_mut().map_or_else(Controls::default, |r| r.route.act(obs));
            if self.hold_at_edge(obs, &c) {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            }
            return match d {
                Some(d) if d.moving.is_some() && d.why.starts_with("dodging") => self.fight.combat.controls(obs, &d, None), // (a missile about to hit still comes first)
                Some(d) => self.fight.combat.controls(obs, &d, Some(&c)),
                None => c,
            };
        }
        if let Some(d) = d.as_ref().filter(|d| d.takeover) {
            self.explorer.hold(obs, last);
            let mut c = self.fight.combat.controls(obs, d, None);
            // (the fight's own way too, unless it's out of the way of something: keeping clear
            // took her out into the spider mastermind's sight as surely as a route)
            let escaping = ["dodging", "ducking", "ducked", "out of", "out from", "away from"].iter().any(|w| d.why.starts_with(w));
            if !escaping && self.hold_at_edge(obs, &c) {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            }
            return c;
        }
        let mut c = self.explorer.act(obs, last, &mut self.fight);
        for line in std::mem::take(&mut self.fight.pending_log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        let shooting = self.explorer.follower.as_ref().is_some_and(RouteFollower::shooting);
        if d.is_none() {
            self.fight.combat.route_walks(obs, &c);
        }
        if d.is_none() && c.weapon == 0 && !shooting {
            // (no fight: the gun for the next one up, the fight's call)
            if let Some(slot) = self.fight.combat.ready_gun(obs) {
                c.weapon = slot;
                self.fight.combat.readied(obs);
            }
        }
        if d.is_none() && self.hold_at_edge(obs, &c) {
            (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
        }
        match d {
            Some(d) => {
                let mut c = self.fight.combat.controls(obs, &d, Some(&c));
                if self.hold_at_edge(obs, &c) {
                    (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                }
                c
            }
            None => c,
        }
    }

    fn plight_now(&mut self, obs: &Observation) -> Plight {
        let nav = &self.explorer.nav;
        let pos = obs.pos();
        let hurting_floor = nav.poly_at(pos).is_some_and(|p| self.explorer.knowledge.damaging(nav.sector_of_poly(p)));
        let c = &self.fight.combat;
        let (last_hit, close) = match c.last_hit() {
            Some((tic, amount)) if obs.tic - tic <= 1 => (amount, c.alive.values().any(|t| dist((t.x, t.y), pos) < 128.0)),
            _ => (self.plight.last_hit, self.plight.close),
        };
        Plight { armed: c.armed(obs), hurting_floor, surrounded: self.surrounded(obs), outmatched: c.outmatched(obs), last_hit, close }
    }

    // --- backing off ------------------------------------------------------------------------

    fn shooters(&self) -> Vec<Thing> {
        self.fight.combat.against_her().into_iter().filter(|t| monster(t.name).is_some_and(|m| matches!(m.attack, Attack::Hitscan | Attack::Missile | Attack::Fire))).collect()
    }

    fn log(&mut self, s: String) {
        self.explorer.log.push(format!("  [fight] {s}"));
    }

    /// Is she falling back? A retreat under way goes on till it's there, it fails, or its own
    /// end comes; else, not too soon after the last, one may start.
    fn retreating(&mut self, obs: &Observation) -> bool {
        if !self.shooters().is_empty() {
            self.shooters_tic = obs.tic;
        }
        if let Some(r) = &self.retreat {
            if r.route.done || r.route.failed || self.retreat_over(obs, r.kind) || (r.kind == RetreatKind::ForAmmo && self.walled_off(obs, &r.route)) {
                let msg = if r.route.done { "got there".to_string() } else { format!("{} over", r.kind.name()) };
                self.log(format!("tic {}: {msg}", obs.tic));
                self.retreat = None;
                self.fight.combat.hold_fire = false;
                if let Some(f) = &mut self.explorer.follower {
                    f.resume(obs.tic);
                }
            }
            return self.retreat.is_some();
        }
        if obs.tic - self.retreat_tic < RETREAT_AGAIN {
            return false;
        }
        self.retreat = self.start_retreat(obs);
        self.retreat.is_some()
    }

    /// A retreat's own end: getting out and baiting after a while (baiting too once they're at
    /// each other: leave them to it); for ammo once armed again, or after a while; backing out
    /// once nothing's shot at her for a moment, or after a while; cover once nothing's shot at
    /// her for a moment.
    fn retreat_over(&self, obs: &Observation, kind: RetreatKind) -> bool {
        let since = obs.tic - self.retreat_tic;
        let calm = obs.tic - self.shooters_tic > CLEAR_TICS;
        match kind {
            RetreatKind::Out => since > OUT_TICS,
            RetreatKind::Bait => since > OUT_TICS || self.fight.combat.visible.iter().any(|t| self.fight.combat.infighting(t)),
            RetreatKind::ForAmmo => self.fight.combat.armed(obs) || since > AMMO_TICS,
            RetreatKind::Back => calm || since > BACK_TICS,
            RetreatKind::Cover => calm,
            RetreatKind::Post => since > POST_TICS,
        }
    }

    /// Unarmed, the rest of her way runs into one of them: it's moved onto her way since she
    /// chose it, and nothing moves it off (E2M5 on UV: a demon between her and the clip she was
    /// off for, pushed against, bitten from 96 health to 29).
    fn walled_off(&self, obs: &Observation, route: &RouteFollower) -> bool {
        let path = route.planned_path(Some(obs.pos()));
        self.fight.combat.against_her().iter().any(|t| {
            let touch = monster(t.name).map_or(20.0, |m| m.radius) + PLAYER_RADIUS;
            path.windows(2).filter_map(|w| Some((w[0]?, w[1]?))).any(|(a, b)| segment_point_distance(a, b, (t.x, t.y)) < touch)
        })
    }

    /// A retreat to start, the first whose rule holds (a rule that holds with no way to go still
    /// counts as a try: not again for a while):
    /// - cover: hurt, with something that shoots in sight, or an arch-vile;
    /// - for ammo: nothing to shoot with and something after her (away, for ammo, not fists at
    ///   it; the user's. E3M1 on UV: out of bullets, a cacodemon on her, "getting past" on to a
    ///   door, chased round till its fireballs had her);
    /// - back: ambushed in a bad place (out of the middle of them instead, surrounded, with no
    ///   way back);
    /// - out: surrounded;
    /// - bait: a heavy she can't kill soon, with others for its shots to land on.
    fn start_retreat(&mut self, obs: &Observation) -> Option<Retreat> {
        if self.fight.combat.invulnerable(obs.tic) || self.fight.combat.bfg_flying(obs) {
            return None; // (nothing to fall back from; or her BFG's spray to come, from where she is)
        }
        let health = obs.health.unwrap_or(100.0);
        let unarmed = !self.fight.combat.armed(obs) && !self.fight.combat.against_her().is_empty();
        let (kind, route) = if self.vile(obs) {
            self.retreat_tic = obs.tic;
            (RetreatKind::Cover, self.find_cover(obs))
        } else if health < RETREAT_HEALTH && !self.shooters().is_empty() {
            self.retreat_tic = obs.tic;
            // (unarmed, cover doesn't end it: nothing she does from there kills what's after her,
            // and the cover's end is when they stop shooting. The ammo first, cover if there's no
            // way off for it. E1M5 UV: fists out at 13 health, an imp's fireballs, a demon on her;
            // hid from the imp 190 tics, the demon followed, dead as the cover ended)
            match unarmed.then(|| self.for_ammo(obs)).flatten() {
                Some(r) => (RetreatKind::ForAmmo, Some(r)),
                None => (RetreatKind::Cover, self.find_cover(obs)),
            }
        } else if self.fight.combat.in_the_open(obs.tic) {
            self.retreat_tic = obs.tic;
            (RetreatKind::Post, self.find_post(obs))
        } else if unarmed {
            self.retreat_tic = obs.tic;
            (RetreatKind::ForAmmo, self.for_ammo(obs))
        } else if let Some(why) = self.ambushed(obs) {
            self.retreat_tic = obs.tic;
            match self.back_out(obs, why) {
                Some(r) => (RetreatKind::Back, Some(r)),
                None if self.surrounded(obs) => (RetreatKind::Out, self.out_of_it(obs)),
                None => (RetreatKind::Back, None),
            }
        } else if self.surrounded(obs) {
            self.retreat_tic = obs.tic;
            (RetreatKind::Out, self.out_of_it(obs))
        } else if obs.tic - self.bait_tic > BAIT_AGAIN {
            let b = self.baiting(obs)?;
            self.retreat_tic = obs.tic;
            self.bait_tic = obs.tic;
            self.fight.combat.hold_fire = true;
            (RetreatKind::Bait, Some(b))
        } else {
            return None;
        };
        route.map(|route| Retreat { kind, route })
    }

    /// Where she stands now, while nothing's shooting at her and nothing's hurting her: on the
    /// trail back.
    fn keep_trail(&mut self, obs: &Observation) {
        let calm = self.shooters().is_empty() && self.fight.combat.damage_rate(obs.tic) == 0.0 && !self.plight.hurting_floor;
        if !calm {
            return;
        }
        let pos = obs.pos();
        if self.trail.last().is_none_or(|&p| dist(p, pos) > TRAIL_STEP) {
            self.trail.push(pos);
            if self.trail.len() > TRAIL_LEN {
                self.trail.remove(0);
            }
        }
    }

    /// Caught in a bad place: in a fight that's going badly (losing health fast, or several
    /// awake close by) where she can't fight well (walls close round her, a floor that hurts,
    /// them on more than one side). Why, if so (MAP29's ambushes in tight spaces: out, or kill
    /// fast, the user's).
    fn ambushed(&mut self, obs: &Observation) -> Option<String> {
        let them = self.shooters();
        if them.is_empty() {
            return None;
        }
        let pos = obs.pos();
        let close: Vec<Point> = self.fight.combat.against_her().iter().filter(|t| dist((t.x, t.y), pos) < AMBUSH_NEAR).map(|t| (t.x, t.y)).collect();
        let losing = self.fight.combat.damage_rate(obs.tic) > AMBUSH_RATE;
        if !losing && close.len() < AMBUSH_COUNT {
            return None;
        }
        let open = self.fight.combat.openness(obs, pos, OPEN_LOOK);
        let sides = spread(pos, &close);
        let bad = if self.plight.hurting_floor {
            "on a floor that hurts"
        } else if open < TIGHT {
            "a tight spot"
        } else if sides > SIDES {
            "them on several sides"
        } else {
            return None;
        };
        Some(format!("{bad}, {}", if losing { "losing health fast" } else { "several close" }))
    }

    /// Back the way she came, to the nearest place on it none of them can see her (or well clear
    /// of them all), by a way not through them.
    fn back_out(&mut self, obs: &Observation, why: String) -> Option<RouteFollower> {
        let them: Vec<Thing> = self.fight.combat.against_her();
        let pos = obs.pos();
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        // (not back onto a floor that hurts now: the way she came may have gone down into it,
        // E3M1's way over the blood, backed out down it into the blood from imps on the far bank)
        let k = &self.explorer.knowledge;
        let trail: Vec<Point> =
            self.trail.iter().rev().copied().filter(|&p| dist(p, pos) > BACK_MIN && nav.poly_at(p).is_none_or(|pi| !k.damaging(nav.sector_of_poly(pi)))).collect();
        for p in trail {
            let Some(pi) = nav.poly_at(p) else { continue };
            let body = Body { x: p.0, y: p.1, z: heights[nav.sector_of_poly(pi)].0, height: PLAYER_HEIGHT };
            let seen = them.iter().any(|t| {
                let eye = Body { x: t.x, y: t.y, z: t.z, height: monster(t.name).map_or(56.0, |m| m.height) };
                self.fight.combat.los.clear(eye, body, &heights)
            });
            let clear = them.iter().all(|t| dist((t.x, t.y), p) > BACK_CLEAR);
            if !seen || clear {
                if let Some(f) = self.go_to(obs, pi, &them, "back out") {
                    self.log(format!("tic {}: caught in {why}: backing out the way she came to ({:.0}, {:.0})", obs.tic, p.0, p.1));
                    return Some(f);
                }
            }
        }
        None
    }

    /// The places she's been, not on hurting floors, between `near` and `far` from her: nearest
    /// first, at most `n`.
    fn visited_spots(&self, obs: &Observation, near: f64, far: f64, n: usize) -> Vec<(f64, usize)> {
        let k = &self.explorer.knowledge;
        let nav = &self.explorer.nav;
        let mut spots: Vec<(f64, usize)> = k.visited.iter().filter(|&&p| !k.damaging(nav.sector_of_poly(p))).map(|&p| (dist(nav.poly_center(p), obs.pos()), p)).filter(|&(d, _)| near < d && d < far).collect();
        spots.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        spots.truncate(n);
        spots
    }

    /// With a heavy after her she can't see off quickly and monsters of another kind near it:
    /// to a place she's been where its shots at her go through them, holding fire.
    fn baiting(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let them = self.fight.combat.against_her();
        let fp = self.fight.combat.firepower(obs);
        let heavies: Vec<&Thing> = them.iter().filter(|h| HEAVIES.contains(&h.name) && mhealth(h.name) / fp > BAIT_HEAVY).collect();
        if heavies.is_empty() {
            return None;
        }
        let spots = self.visited_spots(obs, 64.0, BAIT_RANGE, COVER_SPOTS * 2);
        let nav = Rc::clone(&self.explorer.nav);
        let mut good: Vec<(f64, usize, Thing, Thing)> = Vec::new();
        for h in &heavies {
            let kind = species(h.name);
            let shields: Vec<&Thing> = self.fight.combat.visible.iter().filter(|b| b.id != h.id && species(b.name) != kind && dist((b.x, b.y), (h.x, h.y)) < 1024.0).collect();
            for &(d, pi) in &spots {
                let c = nav.poly_center(pi);
                if let Some(b) = shields.iter().find(|b| {
                    segment_point_distance((h.x, h.y), c, (b.x, b.y)) < monster(b.name).map_or(20.0, |m| m.radius) + 8.0 && 64.0 < dist((h.x, h.y), (b.x, b.y)) && dist((h.x, h.y), (b.x, b.y)) < dist((h.x, h.y), c) - 128.0
                }) {
                    good.push((d, pi, (*h).clone(), (*b).clone()));
                }
            }
        }
        good.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, pi, h, b) in good.into_iter().take(6) {
            if let Some(f) = self.go_to(obs, pi, &them, "bait") {
                self.log(format!("tic {}: baiting {} {} into {} {}", obs.tic, h.name, h.id, b.name, b.id));
                return Some(f);
            }
        }
        None
    }

    /// Out of ammo with something after her: to the nearest ammo or gun she knows of that she
    /// can use, by a way not past them; else to the spot she's been that's furthest from them.
    /// Through doors if need be (E3M1: the doors shut behind her, every way off through one).
    fn for_ammo(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let them = self.fight.combat.against_her();
        let pos = obs.pos();
        // (no nearer to any of them than she is: one on her already, every way off starts by
        // it, and away is still away)
        let nearest = them.iter().map(|m| dist((m.x, m.y), pos)).fold(f64::INFINITY, f64::min);
        let clear = PAST_CLEAR.min(nearest - AMMO_MARGIN).max(0.0);
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        let known = self.explorer.knowledge.known_polys();
        let nav = Rc::clone(&self.explorer.nav);
        // (to the thing itself, not the middle of the floor it's on: a big floor's middle can be
        // further off it than she reaches, and she'd stop there none the richer, again and again)
        let mut ammo: Vec<(f64, Point, &'static str)> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| {
                weapon_item(t.name).is_some_and(|s| s != 1 && s != 8)
                    || ammo_item(t.name).is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s)))
            })
            .filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
            .map(|t| (dist((t.x, t.y), pos), (t.x, t.y), t.name))
            .collect();
        ammo.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, at, name) in ammo.into_iter().take(6) {
            if let Some(f) = self.go_to_point(obs, at, &them, "for ammo", clear, true) {
                self.log(format!("tic {}: out of ammo: away for the {name}", obs.tic));
                return Some(f);
            }
        }
        let mut spots: Vec<(f64, usize)> = self
            .visited_spots(obs, 128.0, AMMO_AWAY, COVER_SPOTS)
            .into_iter()
            .map(|(_, pi)| (them.iter().map(|m| dist((m.x, m.y), nav.poly_center(pi))).fold(f64::INFINITY, f64::min), pi))
            .collect();
        spots.sort_by(|a, b| b.0.total_cmp(&a.0));
        // (all of them, furthest first: the furthest may be beyond a lift, no way there for a
        // plain way; E3M1's start, back down the lift from the cacodemon's room)
        for (_, pi) in spots {
            if let Some(f) = self.go_to_clear(obs, pi, &them, "for ammo", clear, true) {
                self.log(format!("tic {}: out of ammo, none known: away from them", obs.tic));
                return Some(f);
            }
        }
        self.log(format!("tic {}: out of ammo: no way off for ammo or away that isn't past them", obs.tic));
        None
    }

    /// A way to that poly (not past monsters), if there is one.
    fn go_to(&mut self, obs: &Observation, pi: usize, them: &[Thing], label: &str) -> Option<RouteFollower> {
        self.go_to_clear(obs, pi, them, label, PAST_CLEAR, false)
    }

    /// A way to that poly passing no monster closer than `clear` (`doors`: through doors she
    /// can open, else none).
    fn go_to_clear(&mut self, obs: &Observation, pi: usize, them: &[Thing], label: &str, clear: f64, doors: bool) -> Option<RouteFollower> {
        let goal = self.explorer.nav.poly_center(pi);
        self.go_to_point(obs, goal, them, label, clear, doors)
    }

    /// The same, to a point.
    fn go_to_point(&mut self, obs: &Observation, goal: Point, them: &[Thing], label: &str, clear: f64, doors: bool) -> Option<RouteFollower> {
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        let openable = doors.then(|| view.local_doors(obs.keys.unwrap_or_default()));
        let path = view.find_path(obs.pos(), goal, &live, openable.as_ref())?;
        let through = path.waypoints.windows(2).filter_map(|w| Some((w[0]?, w[1]?))).flat_map(|(a, b)| them.iter().map(move |t| segment_point_distance(a, b, (t.x, t.y)))).fold(f64::INFINITY, f64::min);
        if through < clear {
            return None; // (not past them)
        }
        let step = Step { path: Some(path), action: Action::Walk, trigger: None, key: None, barrel: None, label: label.to_string(), expects: Vec::new() };
        Some(RouteFollower::new(view, vec![step], obs.keys.unwrap_or_default(), Some(live), false, obs.tic))
    }

    /// An arch-vile in sight she can't kill in a moment: out of its sight, whatever her health
    /// (MAP11: three deaths at 50-70 health, one in sight).
    fn vile(&self, obs: &Observation) -> bool {
        let fp = self.fight.combat.firepower(obs);
        self.fight.combat.against_her().iter().any(|t| t.name == "Archvile" && mhealth(t.name) / fp > VILE_KILL)
    }

    /// Several monsters near her, and round her rather than all one side.
    fn surrounded(&self, obs: &Observation) -> bool {
        let pos = obs.pos();
        let near: Vec<Point> = self.fight.combat.against_her().iter().filter(|t| dist((t.x, t.y), pos) < SURROUNDED_NEAR && monster(t.name).is_some_and(|m| m.attack != Attack::None)).map(|t| (t.x, t.y)).collect();
        near.len() >= SURROUNDED_COUNT && spread(pos, &near) > SURROUNDED_SPREAD
    }

    /// Seen-by score of spots, by the monsters' eyes.
    fn seen_from(&mut self, obs: &Observation, spots: &[(f64, usize)], them: &[Thing], weigh: bool) -> Vec<f64> {
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        let bodies: Vec<Body> = spots.iter().map(|&(_, pi)| {
            let c = nav.poly_center(pi);
            Body { x: c.0, y: c.1, z: heights[nav.sector_of_poly(pi)].0, height: PLAYER_HEIGHT }
        }).collect();
        let mut seen = vec![0.0; spots.len()];
        for t in them {
            let Some(m) = monster(t.name) else { continue };
            let eye = Body { x: t.x, y: t.y, z: t.z, height: m.height };
            for (i, b) in bodies.iter().enumerate() {
                if self.fight.combat.los.clear(eye, *b, &heights) {
                    seen[i] += if weigh { m.threat } else { 1.0 };
                }
            }
        }
        seen
    }

    /// To somewhere she's been nearby where fewer of them can see her, they're further, and all
    /// on one side of her, by a way that doesn't go through them.
    fn out_of_it(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let them: Vec<Thing> = self.fight.combat.visible.iter().filter(|t| monster(t.name).is_some_and(|m| m.attack != Attack::None)).cloned().collect();
        let spots = self.visited_spots(obs, 96.0, SPOT_RANGE, COVER_SPOTS * 2);
        if spots.is_empty() || them.is_empty() {
            return None;
        }
        let seen = self.seen_from(obs, &spots, &them, true);
        let nav = Rc::clone(&self.explorer.nav);
        let mut scored: Vec<(f64, usize)> = spots
            .iter()
            .zip(&seen)
            .map(|(&(d, pi), &sees)| {
                let c = nav.poly_center(pi);
                let nearest = them.iter().map(|t| dist(c, (t.x, t.y))).fold(f64::INFINITY, f64::min);
                let pts: Vec<Point> = them.iter().map(|t| (t.x, t.y)).collect();
                (sees + spread(c, &pts) / 45.0 - nearest.min(640.0) / 96.0 + d / 256.0, pi)
            })
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (_, pi) in scored.into_iter().take(6) {
            if let Some(f) = self.go_to(obs, pi, &them, "out of it") {
                let g = nav.poly_center(pi);
                self.log(format!("tic {}: surrounded by {}: getting out to ({:.0}, {:.0})", obs.tic, them.len(), g.0, g.1));
                return Some(f);
            }
        }
        self.log(format!("tic {}: surrounded, nowhere better in reach", obs.tic));
        None
    }

    /// On her way, hidden, about to step into the sight of a sniper that's firing (heard: its
    /// guns; at her, as far as she knows, unless she can see it's at something else): not yet.
    /// Once it stops, it winds up again before its next burst (the spider mastermind's 20 tics of
    /// turning to her, after a chase step or two), time to get across to the next cover. The
    /// user's: a player hears when its attack's over. E3M8: the way to a post taken into its
    /// bursts at 600, dead in a second.
    fn hold_at_edge(&mut self, obs: &Observation, c: &Controls) -> bool {
        if c.forward.hypot(c.strafe) < 1.0 || self.fight.combat.invulnerable(obs.tic) {
            return false;
        }
        let firing: Vec<Seen> = self
            .fight
            .combat
            .remaining()
            .into_iter()
            .filter(|s| self.fight.combat.sniper(obs, s.name))
            .filter(|s| {
                let now = self.fight.combat.alive.get(&s.id);
                let seen = self.fight.combat.visible.iter().find(|t| t.id == s.id);
                now.is_some_and(|t| first_shot(t.name).is_some_and(|f| t.attack >= f)) && seen.is_none_or(|t| t.after_her)
            })
            .collect();
        if firing.is_empty() {
            return false;
        }
        let pos = obs.pos();
        let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
        let ahead = (pos.0 + way.cos() * EDGE_LOOK, pos.1 + way.sin() * EDGE_LOOK);
        let hidden_now = !firing.iter().any(|s| self.fight.combat.shot_from(obs, s, pos));
        let seen_ahead = firing.iter().any(|s| self.fight.combat.shot_from(obs, s, ahead));
        if hidden_now && seen_ahead {
            if obs.tic - self.edge_log > 35 {
                self.edge_log = obs.tic;
                self.log(format!("tic {}: holding at the edge of cover till the {} stops firing", obs.tic, firing[0].name));
            }
            return true;
        }
        false
    }

    /// Ways across ground a hitscan shooter she can't take on would see her on (from where she
    /// last saw it) count for `EXPOSED` times their length: round the back of what's between them
    /// rather than across the open. The user's, for E3M8: staying on the other side of the
    /// building from the spider mastermind; every hit she took was on her way to a box of rockets
    /// or a door, across the ring in its sight. Worked out again once one's moved, or a while on.
    fn keep_out_of_sight(&mut self, obs: &Observation) {
        let unhurt = self.fight.combat.invulnerable(obs.tic);
        let them: Vec<Seen> = self
            .fight
            .combat
            .remaining()
            .into_iter()
            .filter(|s| !unhurt && s.awake && self.fight.combat.sniper(obs, s.name))
            // (not one she can see fighting something else: it won't shoot her till it's done
            // or she hurts it, the time to cross; out of sight, taken to be after her. A monster
            // sees all round once awake (A_Chase): its target's what counts, not its facing)
            .filter(|s| self.fight.combat.visible.iter().find(|t| t.id == s.id).is_none_or(|t| t.after_her))
            .collect();
        let at: Vec<Point> = them.iter().map(Seen::pos).collect();
        if at.is_empty() {
            if self.exposure.take().is_some() {
                self.explorer.view().set_exposure(Vec::new());
            }
            return;
        }
        let fresh = self.exposure.as_ref().is_some_and(|(tic, was)| {
            obs.tic - tic < EXPOSURE_AGAIN && was.len() == at.len() && was.iter().zip(&at).all(|(&a, &b)| dist(a, b) < EXPOSURE_MOVED)
        });
        if fresh {
            return;
        }
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        let los = &self.fight.combat.los;
        let exposure: Vec<f64> = (0..nav.mesh.polys.len())
            .map(|pi| {
                if nav.mesh.polys[pi].is_none() {
                    return 1.0;
                }
                let c = nav.poly_center(pi);
                let z = heights.get(nav.sector_of_poly(pi)).map_or(0.0, |h| h.0);
                let seen = them.iter().any(|s| {
                    dist(c, s.pos()) < EXPOSURE_RANGE && {
                        let eye = Body { x: s.x, y: s.y, z: s.z, height: monster(s.name).map_or(56.0, |m| m.height) };
                        los.clear(eye, Body { x: c.0, y: c.1, z, height: PLAYER_HEIGHT }, &heights)
                    }
                });
                if seen { EXPOSED } else { 1.0 }
            })
            .collect();
        // (first seen: the way she's on was chosen without it; again)
        self.fight.rethink |= self.exposure.is_none();
        self.explorer.view().set_exposure(exposure);
        self.exposure = Some((obs.tic, at));
    }

    /// Out in the open in a long fight with a hitscan shooter: to the nearest place she knows of
    /// (seen or been) that none of them can see, with somewhere near it they can (a corner to
    /// step out from and back behind), by a way that doesn't go through them. The user's, for
    /// E3M8: the building in the middle, across the open to get to it, rather than standing in
    /// the open with the spider mastermind's bursts.
    fn find_post(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let shooters = self.shooters();
        if shooters.is_empty() {
            return None;
        }
        let nav = Rc::clone(&self.explorer.nav);
        let known = self.explorer.knowledge.known_polys();
        let heights = self.fight.combat.heights(obs).to_vec();
        let pos = obs.pos();
        // (places on a grid, not the nav's polys: a hall's a few big ones, the shadow behind a
        // pillar inside one whose middle's in plain sight; E3M8, 27 polys in reach, 2 hidden)
        let on_known = |p: Point| -> Option<f64> {
            let pi = nav.poly_in(p)?;
            let s = nav.sector_of_poly(pi);
            (known.contains(&pi) && !self.explorer.knowledge.damaging(s)).then(|| heights[s].0)
        };
        let fits = |p: Point| {
            let r = PLAYER_RADIUS;
            [(-r, -r), (r, -r), (r, r), (-r, r)].into_iter().all(|(dx, dy)| on_known((p.0 + dx, p.1 + dy)).is_some())
        };
        let n = (POST_RANGE / POST_GRID) as i32;
        let mut grid: Vec<(f64, Point, f64)> = (-n..=n)
            .flat_map(|i| (-n..=n).map(move |j| (pos.0 + f64::from(i) * POST_GRID, pos.1 + f64::from(j) * POST_GRID)))
            .filter_map(|p| {
                let d = dist(p, pos);
                (32.0 < d && d < POST_RANGE && fits(p)).then_some((d, p, on_known(p)?))
            })
            .collect();
        grid.sort_by(|a, b| a.0.total_cmp(&b.0));
        let los = &self.fight.combat.los;
        let seen = |p: Point, z: f64| {
            shooters.iter().any(|t| {
                let eye = Body { x: t.x, y: t.y, z: t.z, height: monster(t.name).map_or(56.0, |m| m.height) };
                los.clear(eye, Body { x: p.0, y: p.1, z, height: PLAYER_HEIGHT }, &heights)
            })
        };
        let from_them = |p: Point| shooters.iter().map(|t| dist(p, (t.x, t.y))).fold(f64::INFINITY, f64::min);
        let mut posts: Vec<(f64, Point)> = Vec::new();
        for &(d, p, z) in grid.iter().take(POST_LOOKS) {
            if seen(p, z) {
                continue;
            }
            let peek = (0..8).any(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                [POST_PEEK / 2.0, POST_PEEK].into_iter().any(|r| {
                    let q = (p.0 + a.cos() * r, p.1 + a.sin() * r);
                    on_known(q).is_some_and(|qz| seen(q, qz))
                })
            });
            if peek {
                // (the far side of something, the further from them the better, not only the
                // nearest: the user's, E3M8's building between her and the spider mastermind,
                // 500 off with it 1500 away; its bullets spread, the further the fewer land)
                posts.push((d - POST_FAR * from_them(p), p));
            }
        }
        posts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let found = posts.len();
        for g in posts.into_iter().take(4).map(|(_, p)| p) {
            if let Some(f) = self.go_to_point(obs, g, &shooters, "post", PAST_CLEAR, false) {
                self.log(format!("tic {}: in the open: to a post at ({:.0}, {:.0}), health {:.0}", obs.tic, g.0, g.1, obs.health.unwrap_or(0.0)));
                return Some(f);
            }
        }
        self.log(format!("tic {}: in the open, no post in reach ({} places, {found} posts)", obs.tic, grid.len()));
        None
    }

    fn find_cover(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let shooters = self.shooters();
        let spots = self.visited_spots(obs, f64::NEG_INFINITY, COVER_RANGE, COVER_SPOTS);
        if spots.is_empty() {
            return None;
        }
        let seen = self.seen_from(obs, &spots, &shooters, false);
        let hidden: Vec<usize> = spots.iter().zip(&seen).filter(|(&(d, _), &v)| v == 0.0 && d > 32.0).map(|(&(_, pi), _)| pi).collect();
        if hidden.is_empty() {
            self.log(format!("tic {}: no cover in reach", obs.tic));
            return None;
        }
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        for pi in hidden.into_iter().take(4) {
            let goal = view.nav.poly_center(pi);
            let Some(path) = view.find_path(obs.pos(), goal, &live, None) else { continue };
            self.log(format!("tic {}: falling back to cover at ({:.0}, {:.0}), health {:.0}", obs.tic, goal.0, goal.1, obs.health.unwrap_or(0.0)));
            let step = Step { path: Some(path), action: Action::Walk, trigger: None, key: None, barrel: None, label: "cover".into(), expects: Vec::new() };
            return Some(RouteFollower::new(Rc::clone(&view), vec![step], obs.keys.unwrap_or_default(), Some(live), false, obs.tic));
        }
        None
    }
}

impl Fight {
    /// The way to p (as the crow flies) not past anything about she can't take on: the run for
    /// the invulnerability not straight through what it's for (E3M9's, behind the cyberdemon:
    /// run at past it, one rocket, dead at full health). Till it's moved off the line, kept clear.
    fn clear_way_to(&self, obs: &Observation, p: Point) -> bool {
        let pos = obs.pos();
        self.combat
            .remaining()
            .iter()
            .filter(|s| s.awake && self.combat.too_much(obs, &[s.name], LEAVE_BE))
            .all(|s| segment_point_distance(pos, p, s.pos()) > WAY_CLEAR + monster(s.name).map_or(20.0, |m| m.radius))
    }

    /// Something about she can't take on (awake, known): invulnerability's for that.
    fn needs_invulnerability(&self, obs: &Observation) -> bool {
        self.combat.remaining().iter().any(|s| s.awake && self.combat.too_much(obs, &[s.name], LEAVE_BE))
    }

    /// Barrels she's seen, near, with nothing in sight and the ammo for it: each to be shot from out
    /// of its chain's blasts (the follower's), before a shot sets it off by her, hers at something
    /// beside it or a monster's at her (the user's: barrels are dangerous wherever they are).
    fn barrels(&self, x: &Explorer, obs: &Observation, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        if !self.combat.visible.is_empty() {
            return Vec::new(); // (in a fight, the fight's: shot by what they'd take with them)
        }
        let ammo = |slot: i32| obs.ammo.iter().flatten().find(|(s, _)| *s == slot).map_or(0, |(_, n)| *n);
        let held = obs.weapons.clone().unwrap_or_default();
        // (the barrel rule: the shotgun with 12 shells and more, else single pistol taps)
        if !(held.contains(&2) && ammo(2) >= BARREL_BULLETS) && !(held.contains(&3) && ammo(3) >= BARREL_SHELLS) {
            return Vec::new();
        }
        let nav = &x.nav;
        let mut out = Vec::new();
        for &b in &self.combat.barrels_seen {
            if dist(b, pos) > BARREL_NEAR {
                continue;
            }
            let Some(pi) = reach.iter().copied().filter(|&pi| dist(nav.poly_center(pi), b) < BARREL_SPOT).min_by(|&p, &q| dist(nav.poly_center(p), b).total_cmp(&dist(nav.poly_center(q), b))) else { continue };
            let mut o = Objective::new(Kind::Barrel, nav.poly_center(pi), format!("clear the barrel at ({:.0}, {:.0})", b.0, b.1));
            o.look_at = Some(b);
            // (one objective a barrel, by where it is: given up on as any other is)
            o.reveals = std::iter::once(((b.0.round() as i64) << 32) | (b.1.round() as i64 & 0xffff_ffff)).collect();
            out.push(o);
        }
        out
    }

    fn new_gun(obs: &Observation, name: &str) -> bool {
        if name == "SuperShotgun" {
            return obs.super_shotgun != Some(true);
        }
        if name == "Chainsaw" {
            return obs.chainsaw != Some(true);
        }
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        weapon_item(name).is_some_and(|s| !held.contains(&s))
    }

    fn gun(obs: &Observation, name: &str) -> super::bestiary::Weapon {
        if name == "SuperShotgun" {
            SUPER_SHOTGUN
        } else if name == "Chainsaw" {
            CHAINSAW
        } else {
            weapon_in(weapon_item(name).unwrap_or(2), obs)
        }
    }

    /// What she'd have with that gun or ammo picked up: the ammo, or the gun and the ammo a gun
    /// lying about comes with (twice its clip: two rockets, eight shells, twenty bullets, forty
    /// cells).
    fn armed_with(obs: &Observation, name: &str) -> Observation {
        let mut o = obs.clone();
        let (pool, comes): (&[i32], i32) = if let Some(slot) = weapon_item(name).filter(|&s| s != 1) {
            let held = o.weapons.get_or_insert_with(|| vec![1, 2]);
            if !held.contains(&slot) {
                held.push(slot);
            }
            let clip = match slot {
                3 => "Shell",
                5 => "RocketAmmo",
                6 | 7 => "Cell",
                _ => "Clip",
            };
            ammo_item(clip).map_or((&[], 0), |(pool, n)| (pool, 2 * n))
        } else if let Some((pool, n)) = ammo_item(name).filter(|_| name != "Backpack") {
            (pool, n)
        } else {
            return o;
        };
        let ammo = o.ammo.get_or_insert_with(Vec::new);
        for &s in pool {
            match ammo.iter_mut().find(|(k, _)| *k == s) {
                Some((_, n)) => *n += comes,
                None => ammo.push((s, comes)),
            }
        }
        o
    }

    /// Would that gun do more than the best she has (worth dropping what she's doing for)?
    fn better_gun(&self, obs: &Observation, name: &str) -> bool {
        name != "Chainsaw" && Self::new_gun(obs, name) && Self::gun(obs, name).dps() > self.combat.firepower(obs)
    }

    /// Seconds of walking a gun is worth: what it'd save killing what she knows is still about
    /// (and some, for what's to come).
    fn gun_worth(&self, obs: &Observation, name: &str) -> f64 {
        let w = Self::gun(obs, name);
        let now = self.combat.firepower(obs);
        let hp: f64 = self.combat.remaining().iter().map(|s| mhealth(s.name)).sum();
        GUN_WORTH + (hp / now - hp / w.dps().max(now)).max(0.0)
    }

    /// What she's going for to fight with (and where it is), if that's what the route's for; or, mid-fight, with
    /// only the pistol or next to no ammo, something to fight with she hasn't turned aside for
    /// yet (MAP09's hot start, the shotgun across the room).
    fn errand(&self, x: &Explorer, obs: &Observation) -> Option<(String, Point)> {
        // (an order to press on with, mid-fight: the objective in hand is the errand)
        if let Some(cur) = x.current.as_ref().filter(|_| x.press_on) {
            return Some((cur.label.clone(), cur.goal));
        }
        if let Some(cur) = x.current.as_ref().filter(|c| c.kind == Kind::Item) {
            // (the item's class name, before what the label adds: "pick up Medikit (to heal)")
            let name = cur.label.strip_prefix("pick up ").unwrap_or(&cur.label);
            let name = name.split(' ').next().unwrap_or(name);
            // (health she's on her way to and can use, close by: on to it, shooting on the way.
            // The user's: a cacodemon on her, the medikit the explorer was going for in the
            // room, fought where she stood and never picked up. Not one far off, low or not:
            // MAP10 UV, at 38 a stimpack 1100 off across a yard of imps and zombiemen, walked
            // on into them shooting, down to 3 and dead; the fight's own retreats and cover
            // are what keep her alive there, and its 10 don't pay for the way)
            let heals = health_item(name).is_some_and(|(amount, cap)| amount >= 10 && obs.health.unwrap_or(100.0) < f64::from(cap));
            if cur.urgent && health_item(name).is_none() {
                return Some((name.to_string(), cur.goal));
            }
            if heals && dist(cur.goal, obs.pos()) < HEAL_NEAR {
                return Some((name.to_string(), cur.goal));
            }
        }
        if self.combat.firepower(obs) > PISTOL_ONLY && !self.combat.short_of_ammo(obs) {
            return None;
        }
        self.errands(x, obs).first().map(|t| (t.name.to_string(), (t.x, t.y)))
    }

    /// Guns and ammo she knows where they are and hasn't turned aside for yet: better guns worth
    /// the walk, and, short of ammo, ammo for what she has (or any gun).
    fn errands(&self, x: &Explorer, obs: &Observation) -> Vec<Thing> {
        let known = x.knowledge.known_polys();
        let pos = obs.pos();
        let short = self.combat.short_of_ammo(obs);
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        obs.things
            .iter()
            .flatten()
            .filter(|t| !self.offered.contains(&t.id))
            .filter(|t| {
                if t.name == "InvulnerabilitySphere" {
                    // (in the middle of a fight she can't win as she is: what wins it)
                    return dist((t.x, t.y), pos) < 2.0 * ITEM_RANGE && self.needs_invulnerability(obs) && self.clear_way_to(obs, (t.x, t.y));
                }
                if let Some(slot) = weapon_item(t.name).filter(|&s| s != 1 && held.contains(&s) && !Self::new_gun(obs, t.name)) {
                    // (one she has: its ammo)
                    short && held.contains(&slot) && dist((t.x, t.y), pos) < ITEM_RANGE
                } else if weapon_item(t.name).is_some() {
                    (self.better_gun(obs, t.name) || (short && Self::new_gun(obs, t.name) && t.name != "Chainsaw")) && dist((t.x, t.y), pos) <= self.gun_worth(obs, t.name) * RUN_SPEED
                } else {
                    short && ammo_item(t.name).is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s))) && dist((t.x, t.y), pos) < ITEM_RANGE
                }
            })
            .filter(|t| x.nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
            .cloned()
            .collect()
    }
}

impl Fight {
    /// Items she has seen that she could use now.
    fn supplies(&mut self, x: &Explorer, obs: &Observation, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let known = x.knowledge.known_polys();
        let health = obs.health.unwrap_or(100.0);
        let armor = obs.armor.unwrap_or(0.0);
        let held = obs.weapons.clone().unwrap_or_default();
        let ammo = |s: i32| f64::from(obs.ammo.iter().flatten().find(|(x, _)| *x == s).map_or(0, |(_, n)| *n));
        // (then ammo and guns before anything else. Not sooner for having died for want of it:
        // made urgent, past the dangers, MAP12's start, the ammo there walked into for half the
        // map)
        let short = self.combat.short_of_ammo(obs);
        let mut out = Vec::new();
        for t in obs.things.iter().flatten() {
            let Some(p) = x.nav.poly_at((t.x, t.y)) else { continue };
            if !known.contains(&p) || !reach.contains(&p) {
                continue;
            }
            let d = dist((t.x, t.y), pos);
            // how much it's needed (0: not at all, 1: badly)
            let mut need = 0.0;
            let invulnerability = t.name == "InvulnerabilitySphere" && self.needs_invulnerability(obs) && self.clear_way_to(obs, (t.x, t.y));
            if invulnerability || (weapon_item(t.name).is_some() && Self::new_gun(obs, t.name)) {
                need = 1.0;
            } else if let Some(slot) = weapon_item(t.name).filter(|&s| s != 1) {
                // (a gun she has already is its ammo: a shotgun guy's dropped one, 4 shells; left
                // lying about, MAP04's shells gone by the end and MAP05 begun without any)
                need = (1.0 - ammo(slot) / ammo_max(slot) * 1.5).max(0.0);
            } else if let Some((slots, _)) = ammo_item(t.name) {
                let fill: Vec<f64> = slots.iter().filter(|s| held.contains(s)).map(|&s| ammo(s) / ammo_max(s)).collect();
                if t.name == "Backpack" {
                    need = 0.8;
                } else if let Some(min) = fill.iter().copied().reduce(f64::min) {
                    need = (1.0 - min * 1.5).max(0.0);
                }
            } else if let Some((amount, cap)) = health_item(t.name) {
                if amount >= 10 && health < f64::from(cap) {
                    need = ((f64::from(cap) - health) / 60.0).min(1.0);
                }
            } else if let Some((amount, cap)) = armor_item(t.name) {
                if amount >= 100 && armor < f64::from(cap) * 0.7 {
                    need = 0.6;
                }
            }
            if need <= 0.1 || (d > ITEM_RANGE && need < 0.8) {
                continue;
            }
            let Some(goal) = x.touch_spot((t.x, t.y), reach) else { continue };
            let mut why = "";
            if invulnerability {
                // (E3M9's, behind the cyberdemon, spotted mid-fight and left for the blue key, a
                // wall, a noise; killed at full health by one of its rockets)
                why = " (something about she can't take on)";
            } else if (weapon_item(t.name).is_some() || ammo_item(t.name).is_some()) && !self.clear_way_to(&Self::armed_with(obs, t.name), (t.x, t.y)) {
                // (something to fight with, but on the way something she couldn't take on even
                // with it: not worth dropping everything for. MAP20, a rocket launcher by the
                // cyberdemon, no ammo left, gone for again and again under its rockets for the
                // two it holds)
            } else if weapon_item(t.name).is_some() && need > 0.0 && self.better_gun(obs, t.name) {
                why = " (a better gun)";
            } else if short && (weapon_item(t.name).is_some() || ammo_item(t.name).is_some()) {
                why = " (out of ammo)";
            } else if health < HURT && health_item(t.name).is_some_and(|(a, _)| a >= 10) {
                why = " (hurt)";
            } else if health < SURVIVE_HEALTH && health_item(t.name).is_some_and(|(a, _)| a >= 10) && self.combat.time_to_death(obs) < SURVIVE_SECS {
                // (losing health fast, not yet down to the rest: off for it now, while she can)
                why = " (to survive)";
            }
            let worth = (why == " (a better gun)").then(|| self.gun_worth(obs, t.name) * RUN_SPEED);
            if worth.is_some_and(|w| d > w) {
                why = ""; // (too far to drop everything for, as the crow flies even)
            }
            let mut o = Objective::new(Kind::Item, goal, format!("pick up {}{why}", t.name));
            o.thing = Some(t.id);
            o.urgent = !why.is_empty();
            o.worth = if why.is_empty() { None } else { worth };
            out.push(o);
        }
        out
    }

}

impl Fight {
    /// How many times she's died going for obj (the same kind of thing at the same place).
    fn died_for(&self, obj: &Objective) -> usize {
        self.deaths
            .iter()
            .filter(|d| d.why != "out of ammo")
            // (by place: a level again spawns everything afresh, new ids; MAP12's clips by the
            // start, died going for 130 times)
            .filter(|d| d.kind == obj.kind && dist(d.goal, obj.goal) < SAME_GOAL)
            .count()
    }

    /// Would going for item obj likely cost more than it's worth: the damage from what's awake
    /// and can see the way there and back, for as long as that takes, against what it gives
    /// (health she'd get, armour; ammo or a gun when she's short)? And with nothing to shoot
    /// with, anything but ammo or a gun while something's about (MAP29: a green armor down a
    /// pit, under fire, no ammo).
    fn not_worth_it(&mut self, obs: &Observation, obj: &Objective) -> bool {
        let Some(t) = obj.thing.and_then(|id| obs.things.iter().flatten().find(|t| t.id == id)).cloned() else { return false };
        let pos = obs.pos();
        let health = obs.health.unwrap_or(100.0);
        let armor = obs.armor.unwrap_or(0.0);
        let short = self.combat.short_of_ammo(obs);
        let worth = if let Some((amount, cap)) = health_item(t.name) {
            f64::from(amount).min(f64::from(cap) - health).max(1.0)
        } else if let Some((amount, cap)) = armor_item(t.name) {
            0.5 * f64::from(amount).min(f64::from(cap) - armor).max(1.0)
        } else if ammo_item(t.name).is_some() {
            if short { 60.0 } else { 15.0 }
        } else if weapon_item(t.name).is_some() {
            if Self::new_gun(obs, t.name) { 60.0 } else { 15.0 }
        } else {
            5.0
        };
        let way = dist(pos, obj.goal);
        let exposed = 2.0 * way / RUN_SPEED; // (there and back, seconds)
        let threats: Vec<Seen> = self
            .combat
            .remaining()
            .into_iter()
            .filter(|s| s.awake && monster(s.name).is_some_and(|m| m.attack != Attack::None) && segment_point_distance(pos, obj.goal, s.pos()) < EXPOSED_NEAR)
            .collect();
        if threats.is_empty() {
            return false;
        }
        let supplies = ammo_item(t.name).is_some() || weapon_item(t.name).is_some();
        if !self.combat.armed(obs) && !supplies {
            return true;
        }
        let cost: f64 = threats.iter().map(|s| monster(s.name).map_or(0.0, |m| m.threat) * HITS * exposed).sum();
        cost > worth && cost > COST_FLOOR
    }
}

impl Hooks for Fight {
    /// Items she could use now; with --complete, every counted item she's seen too.
    fn wanted(&mut self, x: &Explorer, obs: &Observation, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let known = x.knowledge.known_polys();
        self.gun_out_of_reach = obs.things.iter().flatten().any(|t| {
            weapon_item(t.name).is_some() && self.better_gun(obs, t.name) && x.nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p) && !reach.contains(&p))
        });
        let mut out = self.supplies(x, obs, reach, pos);
        out.extend(self.barrels(x, obs, reach, pos));
        if let Some(c) = &mut self.completion {
            for o in c.wanted(x, obs, reach, pos) {
                if !out.iter().any(|w| w.thing.is_some() && w.thing == o.thing) {
                    out.push(o);
                }
            }
        }
        out
    }

    /// (--complete: rather the level again than out without items she could get from its start)
    fn before_exit(&mut self, x: &Explorer, obs: &Observation) -> bool {
        self.completion.as_mut().is_some_and(|c| c.before_exit(x, obs))
    }

    /// Monsters seen that are still about: go where each was last seen.
    fn last_resort(&mut self, x: &Explorer, _obs: &Observation, reach: &BTreeSet<usize>, _pos: Point) -> Vec<Objective> {
        let nav = &x.nav;
        let mut out = Vec::new();
        for s in self.combat.remaining() {
            let goal = match nav.poly_at(s.pos()).filter(|p| reach.contains(p)) {
                Some(_) => s.pos(),
                None => {
                    let Some(pi) = reach.iter().copied().filter(|&pi| dist(nav.poly_center(pi), s.pos()) < HUNT_NEAR).reduce(|a, b| if dist(nav.poly_center(b), s.pos()) < dist(nav.poly_center(a), s.pos()) { b } else { a }) else { continue };
                    nav.poly_center(pi)
                }
            };
            let mut o = Objective::new(Kind::Hunt, x.stand_spot_for(goal, reach), format!("hunt {} {}", s.name, s.id));
            o.thing = Some(s.id);
            out.push(o);
        }
        out
    }

    fn gone(&mut self, obs: &Observation, obj: &Objective) -> bool {
        match obj.kind {
            Kind::Item => obj.thing.is_none_or(|t| !self.present.contains(&t)),
            Kind::Hunt => obj.thing.is_none_or(|t| !self.combat.alive.contains_key(&t)),
            Kind::Barrel => obj.look_at.is_none_or(|b| crate::motor::follower::barrel_now(obs, b).is_none()),
            _ => false,
        }
    }

    /// Monsters she knows of by the objective's spot she couldn't take on with what she has?
    /// (Supplies she badly needs and the exit are worth it anyway.)
    fn dangerous(&mut self, obs: &Observation, obj: &Objective) -> bool {
        if matches!(obj.kind, Kind::Exit | Kind::Key) {
            return false; // (worth it whatever: the way on)
        }
        // (invulnerability kept for a fight that needs it: not picked up in passing, its thirty
        // seconds spent on nothing; with something about she can't take on, it's what she goes for)
        if obj.thing.and_then(|id| obs.things.iter().flatten().find(|t| t.id == id)).is_some_and(|t| t.name == "InvulnerabilitySphere") {
            return !self.needs_invulnerability(obs);
        }
        if self.combat.invulnerable(obs.tic) {
            return false; // (nothing can hurt her: the risky ones now)
        }
        // (died going for it: not first again; twice, not at all. And a place she's died at again
        // and again, whatever for: MAP04's corner, five lives for the shotgun, the chaingun,
        // a wall, the key there)
        let died = self.died_for(obj);
        let here = self.deaths.iter().filter(|d| d.why != "out of ammo" && dist(d.at, obj.goal) < DEATH_ZONE).count();
        if died >= DEATHS_GIVE_UP || here >= DEATH_ZONE_URGENT {
            return true;
        }
        if obj.urgent {
            return false;
        }
        if died > 0 || here >= DEATH_ZONE_COUNT {
            return true;
        }
        // (and a hitscan shooter that'd see her there, however far: E3M8 from a pistol start, rocket
        // boxes picked up in the spider mastermind's sight, 0/16, the building with the plasma
        // rifle and the blue armor left shut)
        let known = self.combat.remaining();
        let near: Vec<&str> = known.iter().filter(|s| dist(s.pos(), obj.goal) < DANGER_NEAR || self.combat.shot_from(obs, s, obj.goal)).map(|s| s.name).collect();
        if !near.is_empty() && self.combat.too_much(obs, &near, LEAVE_BE) {
            return true;
        }
        obj.kind == Kind::Item && self.not_worth_it(obs, obj)
    }

    /// Ammo for the guns she has, with a monster she knows of about that she can't take on: as if
    /// twice as far (unless she's short). More of the same won't change that; what might is
    /// somewhere not yet seen, a door not yet tried (another gun, armor, cover). E3M8 from a
    /// pistol start: boxes of rockets picked up in the open, the spider mastermind wandering into
    /// sight of each, the building with the plasma rifle and the blue armor never opened (8/16
    /// got them with this, 0/16 without; at half as far, 0/16).
    fn weigh(&mut self, obs: &Observation, obj: &Objective) -> f64 {
        // (a better gun in sight she can't get to: a switch, what changes the floors and opens
        // the doors, is the likeliest way there; pressed before the rest. E3M3 from a pistol
        // start, HMP and UV: the shotgun up on a floor a switch lowers, seen at the start, the
        // switch left for the walls, the level played out with the pistol, dead out of ammo)
        if obj.kind == Kind::Switch && self.gun_out_of_reach {
            return SWITCH_FOR_GUN;
        }
        let ammo = obj.kind == Kind::Item && obj.thing.and_then(|id| obs.things.iter().flatten().find(|t| t.id == id)).is_some_and(|t| ammo_item(t.name).is_some());
        if !ammo || self.combat.short_of_ammo(obs) {
            return 1.0;
        }
        let known: Vec<&'static str> = self.combat.remaining().iter().filter(|s| s.awake).map(|s| s.name).collect();
        if known.iter().any(|n| self.combat.too_much(obs, &[n], LEAVE_BE)) {
            AMMO_OUTMATCHED
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
        let Some(t) = self.errands(x, obs).into_iter().next() else { return false };
        self.offered.insert(t.id);
        self.pending_log.push(format!("tic {}: spotted a {}", obs.tic, t.name));
        true
    }
}

