//! The fight as a tree, each tic: the first node that applies says what she does.
//!
//! ```text
//! decide
//! ├─ nothing in sight, nothing flying at her
//! │    out from under a crusher · out of a vile's sight · lost sight of it: keeping at it ·
//! │    the fight over
//! └─ a target, or a missile on its way
//!    ├─ survive        out of a vile's sight · dodging · out from under a crusher ·
//!    │                 away from a barrel
//!    ├─ busy           (a jump: the route keeps the legs) shooting on the way
//!    ├─ melee          (the weapon she'd fight it with, up close: `Style::Melee`) closing in
//!    │                 · at it
//!    └─ fight          backing off from what claws · shooting on the way to an errand ·
//!                      out of ammo: getting past · sneaking past · closing in (out of reach) ·
//!                      out of reach: going on · stepping for a shot · outmatched: keeping
//!                      clear · getting past · keeping off · standing and fighting ·
//!                      shooting on the way
//! ```
//!
//! The nodes are tried in that order and the first to give an `Act` wins. A node may look,
//! keep something (a clock, the steering's way) and still pass, as the branches it was cut from
//! did. The aim and the trigger are sized up before the tree (`Situation`): a node only
//! overrides them.

use super::style::{window, Style};
use super::{mon, Combat, Decision, KEEP_CLEAR, LOW_HEALTH, STAND_THREAT};
use crate::geom::{dist, Point};
use crate::motor::{heading, Observation, Thing};
use crate::tactical::bestiary::Attack;

/// The fight as it stands this tic, sized up before the tree.
pub(super) struct Situation<'a> {
    pub pos: Point,
    /// in a jump: the route keeps the legs
    pub busy: bool,
    /// where the route's taking her mid-fight (health, a gun, ammo), if anywhere
    pub errand: &'a str,
    /// missiles to dodge, soonest first
    pub missiles: Vec<Thing>,
    pub target: Option<Thing>,
    /// where she'd aim (the target, or a barrel by it)
    pub aim: Option<f64>,
    pub health: f64,
    pub threat: f64,
    pub outmatched: bool,
    /// clawing monsters in sight, close
    pub close: Vec<Thing>,
    /// the way away from a barrel by her, with something shooting in sight
    pub barrel_away: Option<Point>,
    /// an arch-vile's fire to get out of the sight of: its name, the way
    pub vile: Option<(&'static str, Option<Point>)>,
}

/// What a node has her do.
pub(super) struct Act {
    pub moving: Option<Point>,
    pub why: String,
    /// turned to this rather than as sized up
    pub aim: Option<f64>,
    /// the trigger let go
    pub hold_fire: bool,
}

impl Act {
    fn new(moving: Option<Point>, why: String) -> Act {
        Act { moving, why, aim: None, hold_fire: false }
    }
}

type Node = fn(&mut Combat, &Observation, &Situation) -> Option<Act>;

/// Before anything else: out of the way of what would kill her outright.
const SURVIVE: [Node; 4] = [out_of_vile_sight, dodging, off_crusher, off_barrel];
/// In a jump, the route has the legs.
const BUSY: [Node; 1] = [busy];
/// The weapon she'd fight her target with fought up close.
const MELEE: [Node; 1] = [melee];
/// The fight itself.
const FIGHT: [Node; 12] = [
    backing_off,
    to_errand,
    unarmed,
    sneaking,
    closing_out_of_reach,
    out_of_reach,
    stepping_for_shot,
    outmatched_keep_clear,
    outmatched_get_past,
    keeping_off,
    standing,
    shooting_on_the_way,
];

/// What she does, a target or a missile about: the first node that applies.
pub(super) fn engage(c: &mut Combat, obs: &Observation, s: &Situation) -> Act {
    SURVIVE.iter().chain(&BUSY).chain(&MELEE).chain(&FIGHT).find_map(|node| node(c, obs, s)).unwrap_or_else(|| Act::new(None, "shooting on the way".into()))
}

// --- survive -----------------------------------------------------------------------------------

/// Out of an arch-vile's sight (its fire goes off only if it can still see her), facing it: the
/// blast's put the far side of the shelter.
fn out_of_vile_sight(c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    let (name, way) = s.vile?;
    let mut a = Act::new(way, format!("out of the {name}'s sight"));
    a.aim = c.vile_face().map(|v| heading(s.pos, v));
    Some(a)
}

fn dodging(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let m = s.missiles.first()?;
    let moving = c.dodge(obs, &s.missiles);
    let why = if s.errand.is_empty() { format!("dodging {}", m.name) } else { format!("dodging {} on the way to the {}", m.name, s.errand) };
    Some(Act::new(moving, why))
}

/// Out from under a crusher first, whatever else.
fn off_crusher(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    if !c.under_crusher(obs) || s.busy {
        return None;
    }
    let gun = s.target.as_ref().map_or_else(|| c.gun(obs), |t| c.preferred_gun(obs, t));
    let moving = c.steer(obs, s.target.as_ref(), &gun);
    Some(Act::new(moving, "out from under a crusher".into()))
}

/// (MAP23: a mancubus's fireball set off the barrels she stood by)
fn off_barrel(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.barrel_away.map(|away| Act::new(Some(away), "away from a barrel".into()))
}

// --- busy --------------------------------------------------------------------------------------

fn busy(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.busy.then(|| Act::new(None, "shooting on the way".into()))
}

// --- up close ----------------------------------------------------------------------------------

/// punching or sawing: this much inside the weapon's reach (its centre's, by her target's
/// radius) she stops closing in (it steps about; the punch reaches 64 past her front)
const MELEE_SLACK: f64 = 24.0;

/// Straight at it, then standing at it hitting. The steering's pull to the gun's window, fine
/// for a gun's 500 units, left her circling a revenant 100-200 off, never in the fist's reach,
/// and holding the saw at a demon 99 off as it bit her (E3M4).
fn melee(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let t = s.target.as_ref()?;
    let w = c.preferred_gun(obs, t);
    if Style::of(&w, t) != Style::Melee {
        return None;
    }
    let d = dist((t.x, t.y), s.pos);
    Some(if d > w.reach + mon(t.name).radius - MELEE_SLACK {
        Act::new(c.closing_in(obs, t, None), format!("closing in on {} (melee)", t.name))
    } else {
        Act::new(None, format!("at {} (melee)", t.name))
    })
}

// --- the fight ---------------------------------------------------------------------------------

/// Backing off from the nearest thing that claws, close (armed, or low). (Out of ammo, a demon
/// that follows for ever: backing off got her nowhere, MAP22; the route takes her past it,
/// faster than it.)
fn backing_off(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let nearest = s.close.iter().filter(|_| c.armed(obs) || s.health < LOW_HEALTH).min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos)))?;
    let t = nearest.clone();
    let gun = c.preferred_gun(obs, &t);
    let moving = c.steer(obs, Some(&t), &gun).or_else(|| c.away_from(obs, (t.x, t.y)));
    let why = if s.errand.is_empty() { format!("backing off from {}", nearest.name) } else { format!("backing off from {} towards the {}", nearest.name, s.errand) };
    Some(Act::new(moving, why))
}

fn to_errand(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    (!s.errand.is_empty()).then(|| Act::new(None, format!("shooting on the way to {}", s.errand)))
}

/// (MAP05, MAP11: out of ammo, fist out, walked up to imps to punch them)
fn unarmed(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    (!c.armed(obs)).then(|| Act::new(None, "out of ammo: getting past".into()))
}

fn sneaking(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let woken = s.target.as_ref().map(|t| c.would_wake(obs, t)).filter(|w| !w.is_empty())?;
    let mut a = Act::new(None, format!("sneaking past (a shot would wake {woken})"));
    a.hold_fire = true;
    Some(a)
}

/// After her, only too far for the guns she has: closer, as a player would (a cacodemon
/// hanging back out of the shotgun's reach).
fn closing_out_of_reach(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let t = s.target.as_ref().filter(|t| !c.reachable(obs, t) && t.after_her && !s.outmatched && c.known.get(&t.id).is_some_and(|k| k.awake) && c.still_closing(obs, t.id))?;
    let gun = c.preferred_gun(obs, t);
    let moving = c.steer(obs, Some(t), &gun).or_else(|| c.closing_in(obs, t, None));
    Some(Act::new(moving, format!("closing in on {} (out of reach)", t.name)))
}

fn out_of_reach(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    s.target.as_ref().is_some_and(|t| !c.reachable(obs, t)).then(|| Act::new(None, "out of reach: going on".into()))
}

/// The only one in sight, near, and no shot at it from here: a step to where there is.
fn stepping_for_shot(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let way = s.target.as_ref().filter(|t| c.visible.len() == 1 && dist((t.x, t.y), pos) < 512.0 && !c.can_hit(obs, t, None)).and_then(|t| c.shot_way(obs, t))?;
    Some(Act::new(Some(way), format!("stepping for a shot at {}", s.target.as_ref().map_or("", |t| t.name))))
}

/// Outmatched by something awake and after her: keep her distance, still shooting (the route
/// walked her up to the cyberdemon with a rocket or two short).
fn outmatched_keep_clear(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let t = s.target.as_ref().filter(|t| s.outmatched && c.known.get(&t.id).is_some_and(|k| k.awake) && dist((t.x, t.y), pos) < KEEP_CLEAR)?;
    let gun = c.preferred_gun(obs, t);
    let moving = c.steer(obs, Some(t), &gun);
    Some(Act::new(moving, format!("outmatched: keeping clear of {}", t.name)))
}

/// Outmatched: shooting on the way, the route going on.
fn outmatched_get_past(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.outmatched.then(|| Act::new(None, "getting past".into()))
}

/// Too near for the gun she'd fight it with, or for them to miss.
fn keeping_off(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let way = s.target.as_ref().filter(|t| mon(t.name).attack != Attack::None && dist((t.x, t.y), pos) < window(&c.preferred_gun(obs, t), t).0).and_then(|t| {
        let gun = c.preferred_gun(obs, t);
        c.steer(obs, Some(t), &gun).or_else(|| c.keeping_off(obs, t, s.aim))
    })?;
    Some(Act::new(Some(way), format!("keeping off from {}", s.target.as_ref().map_or("", |t| t.name))))
}

/// Enough in sight, low, or in the way: the route waits; into the gun's window, else across the
/// line of fire.
fn standing(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    if !(s.threat >= STAND_THREAT || s.health < LOW_HEALTH || c.blocking(obs, s.target.as_ref())) {
        return None;
    }
    let mut why = "standing and fighting".to_string();
    let moving = if let Some(t) = &s.target {
        let gun = c.preferred_gun(obs, t);
        let d = dist((t.x, t.y), s.pos);
        let mut moving = c.steer(obs, Some(t), &gun);
        if !Combat::in_reach(&gun, t, d) || d > window(&gun, t).1 {
            moving = moving.or_else(|| c.closing_in(obs, t, None));
            why = format!("closing in on {}", t.name);
        }
        moving
    } else {
        c.strafe_dir(obs, s.aim)
    };
    Some(Act::new(moving, why))
}

fn shooting_on_the_way(_c: &mut Combat, _obs: &Observation, _s: &Situation) -> Option<Act> {
    Some(Act::new(None, "shooting on the way".into()))
}

// --- nothing in sight --------------------------------------------------------------------------

impl Combat {
    /// Nothing in sight, nothing flying at her: hidden from an arch-vile's fire (staying hidden
    /// comes first), an awake monster just out of sight fought where it was, else the fight's
    /// over.
    pub(super) fn no_target(&mut self, obs: &Observation, busy: bool, errand: &str) -> Option<Decision> {
        // (out from under a crusher at work with no fight on and no route under way: between
        // objectives nothing else has her, E3M5's, a switch pressed in a crusher's alcove, the
        // route over, stood there till it came down, pinned. A route has its own crusher rules:
        // taken over from one, a timed crossing under E2M2's fast crusher was turned back in it)
        if !busy && !self.routed && self.under_crusher(obs) {
            let gun = self.gun(obs);
            let moving = self.steer(obs, None, &gun);
            let why = "out from under a crusher".to_string();
            if why != self.mode {
                self.log.push(format!("tic {}: {why}", obs.tic));
                self.mode = why.clone();
            }
            return Some(Decision { takeover: true, aim: None, fire: false, weapon: 0, moving, why });
        }
        if !busy && errand.is_empty() {
            if let Some((name, way)) = self.vile_fire(obs) {
                let why = format!("out of the {name}'s sight");
                if why != self.mode {
                    self.log.push(format!("tic {}: {why}", obs.tic));
                    self.mode = why.clone();
                }
                let aim = self.vile_face().map(|v| heading(obs.pos(), v));
                return Some(Decision { takeover: true, aim, fire: false, weapon: 0, moving: way, why });
            }
            if let Some(d) = self.lost_sight(obs) {
                return Some(d);
            }
        }
        if !self.mode.is_empty() {
            self.log.push(format!("tic {}: fight over ({} kills; {:.0} rounds spent, {:.0} damage done)", obs.tic, self.kills, self.spent, self.dealt));
            (self.spent, self.dealt) = (0.0, 0.0);
        }
        self.mode.clear();
        self.target = None;
        None
    }
}
