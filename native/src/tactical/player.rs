//! Evil playing with monsters about: the explorer finds the way, the fight (Combat) takes the
//! controls when it needs them, and between them she goes for what she needs to keep fighting
//! (weapons, ammo, health, armour she has seen) and, once there's nothing else, hunts down the
//! monsters she knows are left (some maps open only when they're dead: E1M8's barons).

use std::collections::BTreeSet;
use std::rc::Rc;

use super::bestiary::{ammo_item, armor_item, health_item, monster, weapon_in, weapon_item, Attack, SUPER_SHOTGUN};
use super::combat::{segment_point_distance, spread, Combat, LEAVE_BE};
use crate::geom::{dist, Point};
use crate::knowledge::completion::Completionist;
use crate::knowledge::explorer::{Explorer, Hooks, Kind, Objective};
use crate::knowledge::model::Knowledge;
use crate::los::Body;
use crate::motor::follower::RouteFollower;
use crate::motor::{Controls, Observation, Thing};
use crate::navigator::{Navigator, States, PLAYER_HEIGHT};
use crate::walkthrough::{Action, Step};

/// the most of each ammo she can carry (without a backpack), by weapon slot
fn ammo_max(slot: i32) -> f64 {
    match slot {
        2 | 4 => 200.0,
        3 | 5 => 50.0,
        _ => 300.0,
    }
}

/// items further than this (as the crow flies) only when they're needed badly
const ITEM_RANGE: f64 = 1536.0;
/// a monster's last known spot: hunted from the reachable poly nearest it, this near
const HUNT_NEAR: f64 = 512.0;
/// below this health, health items come first
const HURT: f64 = 50.0;
/// monsters this near an objective's spot count against going there
const DANGER_NEAR: f64 = 640.0;
/// below this, with something shooting at her: back off out of its sight
const RETREAT_HEALTH: f64 = 40.0;
/// cover looked for among the places she's been this near...
const COVER_RANGE: f64 = 768.0;
/// ...the nearest this many of them
const COVER_SPOTS: usize = 48;
/// a retreat is over once nothing that shoots has been in sight this long
const CLEAR_TICS: i64 = 35;
/// after a retreat that found no cover, not again for this long
const RETREAT_AGAIN: i64 = 3 * 35;
/// monsters this near, this many of them, spread round her this far (degrees): get out
const SURROUNDED_NEAR: f64 = 640.0;
const SURROUNDED_COUNT: usize = 3;
const SURROUNDED_SPREAD: f64 = 160.0;
/// getting out: at most this long (then the fight goes on from there)
const OUT_TICS: i64 = 4 * 35;
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
/// seconds a better gun is worth going for anyway (the fights to come)
const GUN_WORTH: f64 = 10.0;

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
}

pub struct Player {
    pub explorer: Explorer,
    fight: Fight,
    retreat: Option<RouteFollower>,
    retreat_why: &'static str,
    bait_tic: i64,
    /// when the last retreat started or found no cover
    retreat_tic: i64,
    /// when something that shoots was last in sight
    shooters_tic: i64,
}

impl Player {
    pub fn new(nav: Rc<Navigator>, knowledge: Knowledge) -> Player {
        Player {
            explorer: Explorer::new(Rc::clone(&nav), knowledge),
            fight: Fight { combat: Combat::new(nav), pending_log: Vec::new(), offered: BTreeSet::new(), present: BTreeSet::new(), completion: None },
            retreat: None,
            retreat_why: "",
            bait_tic: -1_000_000_000,
            retreat_tic: -1_000_000_000,
            shooters_tic: -1_000_000_000,
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

    pub fn restart(&mut self) {
        self.explorer.restart();
        self.fight.combat.restart();
        self.retreat = None;
        self.fight.combat.hold_fire = false;
    }

    pub fn act(&mut self, obs: &Observation, last: Option<&Controls>) -> Controls {
        self.fight.present = obs.things.iter().flatten().map(|t| t.id).collect();
        self.fight.combat.observe(obs);
        for line in std::mem::take(&mut self.fight.combat.log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        let busy = self.explorer.follower.as_ref().is_some_and(RouteFollower::in_jump);
        let errand = if busy { String::new() } else { self.fight.errand(&self.explorer, obs) };
        let d = self.fight.combat.decide(obs, busy, &errand);
        let retreat = !busy && self.retreating(obs);
        if retreat {
            self.explorer.hold(obs, last);
            let c = self.retreat.as_mut().map_or_else(Controls::default, |r| r.act(obs));
            return match d {
                Some(d) if d.moving.is_some() && d.why.starts_with("dodging") => self.fight.combat.controls(obs, &d, None), // (a missile about to hit still comes first)
                Some(d) => self.fight.combat.controls(obs, &d, Some(&c)),
                None => c,
            };
        }
        if let Some(d) = d.as_ref().filter(|d| d.takeover) {
            self.explorer.hold(obs, last);
            return self.fight.combat.controls(obs, d, None);
        }
        let c = self.explorer.act(obs, last, &mut self.fight);
        for line in std::mem::take(&mut self.fight.pending_log) {
            self.explorer.log.push(format!("  [fight] {line}"));
        }
        match d {
            Some(d) => self.fight.combat.controls(obs, &d, Some(&c)),
            None => c,
        }
    }

    // --- backing off ------------------------------------------------------------------------

    fn shooters(&self) -> Vec<Thing> {
        self.fight.combat.against_her().into_iter().filter(|t| monster(t.name).is_some_and(|m| matches!(m.attack, Attack::Hitscan | Attack::Missile | Attack::Fire))).collect()
    }

    fn log(&mut self, s: String) {
        self.explorer.log.push(format!("  [fight] {s}"));
    }

    /// Is she falling back: hurt, with something that shoots in sight, to the nearest place
    /// she's been that nothing in sight can see (and on from there, the explorer's way)?
    fn retreating(&mut self, obs: &Observation) -> bool {
        if !self.shooters().is_empty() {
            self.shooters_tic = obs.tic;
        }
        if let Some(r) = &self.retreat {
            let mut over = if matches!(self.retreat_why, "out" | "bait") { obs.tic - self.retreat_tic > OUT_TICS } else { obs.tic - self.shooters_tic > CLEAR_TICS };
            if self.retreat_why == "bait" && self.fight.combat.visible.iter().any(|t| self.fight.combat.infighting(t)) {
                over = true; // (they're at it: leave them to it)
            }
            if r.done || r.failed || over {
                let msg = if r.done { "got there".to_string() } else { format!("{} over", self.retreat_why) };
                self.log(format!("tic {}: {msg}", obs.tic));
                self.retreat = None;
                self.fight.combat.hold_fire = false;
                if let Some(f) = &mut self.explorer.follower {
                    f.resume(obs.tic);
                }
            }
            return self.retreat.is_some();
        }
        let health = obs.health.unwrap_or(100.0);
        if obs.tic - self.retreat_tic < RETREAT_AGAIN {
            return false;
        }
        if (health < RETREAT_HEALTH && !self.shooters().is_empty()) || self.vile(obs) {
            self.retreat_tic = obs.tic;
            self.retreat = self.find_cover(obs);
            self.retreat_why = "cover";
        } else if self.surrounded(obs) {
            self.retreat_tic = obs.tic;
            self.retreat = self.out_of_it(obs);
            self.retreat_why = "out";
        } else if obs.tic - self.bait_tic > BAIT_AGAIN {
            if let Some(b) = self.baiting(obs) {
                self.retreat_tic = obs.tic;
                self.bait_tic = obs.tic;
                self.retreat = Some(b);
                self.retreat_why = "bait";
                self.fight.combat.hold_fire = true;
            }
        }
        self.retreat.is_some()
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

    /// A way to that poly (not past monsters), if there is one.
    fn go_to(&mut self, obs: &Observation, pi: usize, them: &[Thing], label: &str) -> Option<RouteFollower> {
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        let goal = view.nav.poly_center(pi);
        let path = view.find_path(obs.pos(), goal, &live, None)?;
        let through = path.waypoints.windows(2).filter_map(|w| Some((w[0]?, w[1]?))).flat_map(|(a, b)| them.iter().map(move |t| segment_point_distance(a, b, (t.x, t.y)))).fold(f64::INFINITY, f64::min);
        if through < 96.0 {
            return None; // (not past them)
        }
        let step = Step { path: Some(path), action: Action::Walk, trigger: None, key: None, label: label.to_string(), expects: Vec::new() };
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
            let step = Step { path: Some(path), action: Action::Walk, trigger: None, key: None, label: "cover".into(), expects: Vec::new() };
            return Some(RouteFollower::new(Rc::clone(&view), vec![step], obs.keys.unwrap_or_default(), Some(live), false, obs.tic));
        }
        None
    }
}

impl Fight {
    fn new_gun(obs: &Observation, name: &str) -> bool {
        if name == "SuperShotgun" {
            return obs.super_shotgun != Some(true);
        }
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        weapon_item(name).is_some_and(|s| !held.contains(&s))
    }

    fn gun(obs: &Observation, name: &str) -> super::bestiary::Weapon {
        if name == "SuperShotgun" {
            SUPER_SHOTGUN
        } else {
            weapon_in(weapon_item(name).unwrap_or(2), obs.super_shotgun)
        }
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

    /// What she's going for to fight with, if that's what the route's for; or, mid-fight, with
    /// only the pistol or next to no ammo, something to fight with she hasn't turned aside for
    /// yet (MAP09's hot start, the shotgun across the room).
    fn errand(&self, x: &Explorer, obs: &Observation) -> String {
        if let Some(cur) = x.current.as_ref().filter(|c| c.kind == Kind::Item && c.urgent) {
            return cur.label.strip_prefix("pick up ").unwrap_or(&cur.label).to_string();
        }
        if self.combat.firepower(obs) > PISTOL_ONLY && !self.combat.short_of_ammo(obs) {
            return String::new();
        }
        self.errands(x, obs).first().map_or(String::new(), |t| t.name.to_string())
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
                if weapon_item(t.name).is_some() {
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
        let short = self.combat.short_of_ammo(obs); // (then ammo and guns before anything else)
        let mut out = Vec::new();
        for t in obs.things.iter().flatten() {
            let Some(p) = x.nav.poly_at((t.x, t.y)) else { continue };
            if !known.contains(&p) || !reach.contains(&p) {
                continue;
            }
            let d = dist((t.x, t.y), pos);
            // how much it's needed (0: not at all, 1: badly)
            let mut need = 0.0;
            if weapon_item(t.name).is_some() {
                need = if Self::new_gun(obs, t.name) { 1.0 } else { 0.0 };
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
            if weapon_item(t.name).is_some() && need > 0.0 && self.better_gun(obs, t.name) {
                why = " (a better gun)";
            } else if short && (weapon_item(t.name).is_some() || ammo_item(t.name).is_some()) {
                why = " (out of ammo)";
            } else if health < HURT && health_item(t.name).is_some_and(|(a, _)| a >= 10) {
                why = " (hurt)";
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

impl Hooks for Fight {
    /// Items she could use now; with --complete, every counted item she's seen too.
    fn wanted(&mut self, x: &Explorer, obs: &Observation, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let mut out = self.supplies(x, obs, reach, pos);
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

    fn gone(&mut self, _obs: &Observation, obj: &Objective) -> bool {
        match obj.kind {
            Kind::Item => obj.thing.is_none_or(|t| !self.present.contains(&t)),
            Kind::Hunt => obj.thing.is_none_or(|t| !self.combat.alive.contains_key(&t)),
            _ => false,
        }
    }

    /// Monsters she knows of by the objective's spot she couldn't take on with what she has?
    /// (Supplies she badly needs and the exit are worth it anyway.)
    fn dangerous(&mut self, obs: &Observation, obj: &Objective) -> bool {
        if obj.urgent || obj.kind == Kind::Exit {
            return false;
        }
        let near: Vec<&str> = self.combat.remaining().iter().filter(|s| dist(s.pos(), obj.goal) < DANGER_NEAR).map(|s| s.name).collect();
        !near.is_empty() && self.combat.too_much(obs, &near, LEAVE_BE)
    }

    /// Something to fight with come into what she knows of the map (a better gun; ammo, when
    /// she's short): go for it now (once for each).
    fn interrupt(&mut self, x: &Explorer, obs: &Observation, obj: &Objective) -> bool {
        if obj.urgent || obj.kind == Kind::Exit {
            return false;
        }
        let Some(t) = self.errands(x, obs).into_iter().next() else { return false };
        self.offered.insert(t.id);
        self.pending_log.push(format!("tic {}: spotted a {}", obs.tic, t.name));
        true
    }
}

