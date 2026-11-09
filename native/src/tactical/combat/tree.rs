//! The fight as a tree, each tic: the first node that applies says what she does.
//!
//! ```text
//! decide
//! ├─ nothing in sight, nothing flying at her
//! │    out from under a crusher · out of a vile's sight · ducked: waiting out its fire ·
//! │    lost sight of it: keeping at it ·
//! │    the fight over
//! └─ a target, or a missile on its way
//!    ├─ survive        out of a vile's sight · ducking a hitscan heavy · dodging · out from
//!    │                 under a crusher ·
//!    │                 away from a barrel
//!    ├─ busy           (a jump: the route keeps the legs) shooting on the way
//!    ├─ melee          (the weapon she'd fight it with, up close: `Style::Melee`) closing in
//!    │                 · at it
//!    └─ fight          backing off from what claws · sneaking past · shooting on the way to an
//!                      errand · out of ammo: getting past · closing in (out of reach) ·
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
use super::{mon, Combat, Decision, CLOSE_GIVE_UP, KEEP_CLEAR, LOW_HEALTH, STAND_THREAT};
use crate::geom::{dist, Point};
use crate::motor::{heading, norm, Observation, Thing};
use crate::navigator::PLAYER_RADIUS;
use crate::tactical::bestiary::{claw_max, Attack};

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
    /// the way away from a barrel going off whose blast (or its chain's) reaches her
    pub barrel_lit: Option<Point>,
    /// an arch-vile's fire to get out of the sight of: its name, the way
    pub vile: Option<(&'static str, Option<Point>)>,
    /// a hitscan shooter's fire to duck out of the sight of: its name, the way
    pub duck: Option<(&'static str, Option<Point>)>,
}

/// What a node has her do.
pub(super) struct Act {
    pub moving: Option<Point>,
    pub why: String,
    /// turned to this rather than as sized up
    pub aim: Option<f64>,
    /// the trigger let go
    pub hold_fire: bool,
    /// the route keeps the legs (whatever else: missiles flying)
    pub route: bool,
}

impl Act {
    fn new(moving: Option<Point>, why: String) -> Act {
        Act { moving, why, aim: None, hold_fire: false, route: false }
    }
}

type Node = fn(&mut Combat, &Observation, &Situation) -> Option<Act>;

/// Before anything else: out of the way of what would kill her outright.
const SURVIVE: [Node; 7] = [off_crusher, out_of_vile_sight, off_lit_barrel, ducking, dodging, bfg_hold, off_barrel];
/// In a jump, the route has the legs.
const BUSY: [Node; 1] = [busy];
/// The weapon she'd fight her target with fought up close.
const MELEE: [Node; 1] = [melee];
/// The fight itself.
const FIGHT: [Node; 13] = [
    backing_off,
    sneaking,
    to_errand,
    unarmed,
    closing_out_of_reach,
    out_of_reach,
    stepping_for_shot,
    stepping_for_plasma,
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

/// Out of a hitscan shooter's sight before its first shot (it turns to her first), in a long
/// fight: shooting on the way, as sized up. Not into a missile's way: then the dodge (a rocket's
/// hit is up to 160 and its blast; MAP20, ducked from the spider mastermind's sight across a
/// cyberdemon's rocket's line, 90 health gone at once).
fn ducking(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let (name, way) = s.duck?;
    if !c.clear_of_hits(obs, &s.missiles, way) {
        return None;
    }
    Some(Act::new(way, format!("ducking out of the {name}'s sight")))
}

fn dodging(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let m = s.missiles.first()?;
    // (a homing one going round her: dodged towards a corner it'll fly into, never into it)
    if let Some((name, way)) = c.shake_off(obs, &s.missiles) {
        let moving = c.dodge(obs, &s.missiles, way);
        return Some(Act::new(moving, format!("shaking off {name} round a corner")));
    }
    let moving = c.dodge(obs, &s.missiles, None);
    let why = if s.errand.is_empty() { format!("dodging {}", m.name) } else { format!("dodging {} on the way to the {}", m.name, s.errand) };
    Some(Act::new(moving, why))
}

/// Her BFG charging or its ball in the air: still, where she is (the spray comes from her when it
/// lands, along its way: walked round a corner, it hits the wall). Missiles still dodged first.
fn bfg_hold(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    c.bfg_flying(obs).then(|| Act::new(None, "holding still for the BFG's spray".into()))
}

/// Out from under a crusher first, whatever else (a missile maybe hits; a crusher at work
/// does). With a route under way, its way: the route's own crusher logic times the crossing and
/// has her out through or back by the nearest way to room overhead. Without, the shortest
/// straight way out. Not the fight's steering: under a crusher every way is clear to it, and it
/// kept her under one after an imp 2000 off, 7 a hit three times over (E2M2, seed 7).
fn off_crusher(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    // (any crusher at work, between its strokes too: no place for a fight)
    if !c.under_crusher(obs) && !c.in_crusher(obs) {
        return None;
    }
    if c.routed {
        let mut a = Act::new(None, "out from under a crusher (the route's way)".into());
        a.route = true;
        return Some(a);
    }
    if s.busy {
        return None;
    }
    let moving = c.way_out_of_crusher(obs).or_else(|| {
        let gun = s.target.as_ref().map_or_else(|| c.gun(obs), |t| c.preferred_gun(obs, t));
        c.steer(obs, s.target.as_ref(), &gun)
    });
    Some(Act::new(moving, "out from under a crusher".into()))
}

/// A barrel going off by her: its blast (up to 128) before any missile's dodge.
fn off_lit_barrel(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.barrel_lit.map(|away| Act::new(Some(away), "away from a barrel going off".into()))
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

/// punching or sawing: this much inside the weapon's reach, centre to centre, she stops closing
/// in (it steps about). Not more: up against a demon she's 46 off. (MAP22 HNTR: berserk, stood
/// 68-77 off a chaingunner, inside the old 64-past-her-front-plus-its-radius, every punch short
/// of its centre, 16 of its bullets to 10 health)
pub(super) const MELEE_SLACK: f64 = 12.0;
/// closing in to hit it in melee, no nearer by this much (units)...
const MELEE_GAIN: f64 = 32.0;
/// ...in this long (tics): it's out of her reach, given up a while
const MELEE_STALL: i64 = 3 * 35;

/// Straight at it, then standing at it hitting. The steering's pull to the gun's window, fine
/// for a gun's 500 units, left her circling a revenant 100-200 off, never in the fist's reach,
/// and holding the saw at a demon 99 off as it bit her (E3M4).
fn melee(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let t = s.target.as_ref()?;
    let w = c.preferred_gun(obs, t);
    if Style::of(&w, t) != Style::Melee {
        return None;
    }
    if c.out_of_fist_reach(obs, t) {
        return None;
    }
    let d = dist((t.x, t.y), s.pos);
    // (not for ever: one she can't hurt, given up a while, as out of reach is; E3M4, a demon
    // closed in on and lost sight of, round and round, an hour. The clock goes back only when
    // it's hurt, not when it's near: a demon on a floor below, close across but out of the
    // fist's slope, "at" it and off again for an hour, never hit, MAP05 ITYTD)
    let (since, had) = *c.melee_since.entry(t.id).or_insert((obs.tic, t.health));
    if t.health < had {
        c.melee_since.insert(t.id, (obs.tic, t.health));
    } else if obs.tic - since > CLOSE_GIVE_UP {
        c.melee_since.remove(&t.id);
        c.give_up(obs, t.id);
        return None;
    }
    Some(if d > w.reach - MELEE_SLACK {
        // (nor closing in on what she gets no nearer: the way straight at it is blocked, a ledge
        // above her or a wall between, and the fist has nothing else; MAP14 HMP, out of ammo,
        // berserk, a chaingunner up on a ledge 300 off, walked at from below for 16 s of its
        // bullets, to death, with ammo known about)
        let (nearest, at, _) = c.melee_nearest.get(&t.id).copied().filter(|&(_, _, last)| obs.tic - last <= MELEE_STALL).unwrap_or((d, obs.tic, obs.tic));
        let (nearest, at) = if d < nearest - MELEE_GAIN { (d, obs.tic) } else { (nearest, at) };
        if obs.tic - at > MELEE_STALL {
            c.melee_nearest.remove(&t.id);
            if let Some(sector) = c.sector_at((t.x, t.y)) {
                c.fist_stalled.insert(t.id, sector);
            }
            c.give_up(obs, t.id);
            return None;
        }
        c.melee_nearest.insert(t.id, (nearest, at, obs.tic));
        Act::new(c.closing_in(obs, t, None), format!("closing in on {} (melee)", t.name))
    } else {
        Act::new(None, format!("at {} (melee)", t.name))
    })
}

// --- the fight ---------------------------------------------------------------------------------

/// Backing off from the nearest thing that claws, close (armed, or low, or one hit of it could
/// kill her). (Out of ammo, a demon that follows for ever: backing off got her nowhere, MAP22;
/// the route takes her past it, faster than it. Not past one whose claw can take all she has:
/// E2M3, out of ammo at 35, the route to a shell went by a baron, 10-80 a hit.)
fn backing_off(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let armed = c.armed(obs);
    let nearest = s.close.iter().filter(|t| armed || s.health < LOW_HEALTH || claw_max(t.name) >= s.health).min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos)))?;
    let t = nearest.clone();
    let gun = c.preferred_gun(obs, &t);
    let moving = c.steer(obs, Some(&t), &gun).or_else(|| c.away_from(obs, (t.x, t.y)));
    let why = if s.errand.is_empty() { format!("backing off from {}", nearest.name) } else { format!("backing off from {} towards the {}", nearest.name, s.errand) };
    Some(Act::new(moving, why))
}

/// On to the errand, shooting on the way; turned to a monster standing against her on the way to
/// it (the route only pushes into its body): it's shot first. (MAP04, HMP: on her way to a
/// shotgun, a zombieman in the doorway, 80 tics pushed into while she shot at the shotgun guys
/// beyond, shot down there.)
fn to_errand(c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    if s.errand.is_empty() {
        return None;
    }
    let mut a = Act::new(None, format!("shooting on the way to {}", s.errand));
    let way = c.toward.map(|to| heading(s.pos, to));
    let in_the_way = c.visible.iter().filter(|t| mon(t.name).attack != Attack::None).find(|t| {
        dist((t.x, t.y), s.pos) < PLAYER_RADIUS + mon(t.name).radius + ERRAND_CONTACT && way.is_some_and(|w| norm(heading(s.pos, (t.x, t.y)) - w).abs() < 60.0)
    });
    if let Some(t) = in_the_way {
        a.aim = Some(heading(s.pos, (t.x, t.y)));
        a.why = format!("shooting on the way to {}: {} in the way", s.errand, t.name);
    }
    Some(a)
}

/// how far past touching a monster still stands in her way
const ERRAND_CONTACT: f64 = 16.0;

/// (MAP05, MAP11: out of ammo, fist out, walked up to imps to punch them)
fn unarmed(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    (!c.armed(obs)).then(|| Act::new(None, "out of ammo: getting past".into()))
}

/// A shot that would wake a sleeping pack worth too much isn't fired, on an errand too (the route
/// keeps the legs, on to it). (E3M5 UV: out of ammo, the way to a clip through a room of seven
/// sleeping demons she'd sneaked past three times; on the errand she shot at them on the way, the
/// last bullets woke all seven, bitten to death.)
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

/// The only one in sight, near, and no shot at it from here: a step to where there is. Not on a
/// floor that hurts: the step keeps her on it, a hurt every 32 tics, for a shot she can take on
/// the way off (E3M5's nukage pit, an imp on the ledge above 500 off: stepped away for the angle,
/// out past 512, the route's way off brought her back in, 570 tics of it, dead).
fn stepping_for_shot(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    if c.on_hurting_floor(obs) {
        return None;
    }
    let pos = s.pos;
    let way = s.target.as_ref().filter(|t| c.visible.len() == 1 && dist((t.x, t.y), pos) < 512.0 && !c.can_hit(obs, t, None)).and_then(|t| c.shot_way(obs, t))?;
    Some(Act::new(Some(way), format!("stepping for a shot at {}", s.target.as_ref().map_or("", |t| t.name))))
}

/// Plasma up and autoaim taking it short of the target (into a ledge's lip with them down in a pit,
/// or the floor in front of them up on one): a step on to where it would get there, never off the
/// ledge (the user's: on till the weapon can hit, not falling down). Not for rockets (see
/// `plasma_spot`).
fn stepping_for_plasma(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let t = s.target.as_ref().filter(|_| obs.weapon == Some(6) && c.held_fire == "plasma wouldn't get there")?;
    let way = c.plasma_spot(obs, t)?;
    Some(Act::new(Some(way), format!("stepping for a shot at {} (its way blocked)", t.name)))
}

/// Fought where it stands, whatever else: a sniper she'd see off first (`takes_on`), or,
/// outmatched, a hitscan shooter she'd see off quick (`quick_hitscan`): kept clear of or walked
/// on past, its bullets reach her all the same.
fn fought(c: &Combat, obs: &Observation, s: &Situation, t: &Thing) -> bool {
    c.takes_on(obs, t.name) || (s.outmatched && c.quick_hitscan(obs, t.name))
}

/// Outmatched by something awake and after her: keep her distance, still shooting (the route
/// walked her up to the cyberdemon with a rocket or two short).
fn outmatched_keep_clear(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let t = s.target.as_ref().filter(|t| s.outmatched && !fought(c, obs, s, t) && c.known.get(&t.id).is_some_and(|k| k.awake) && dist((t.x, t.y), pos) < KEEP_CLEAR)?;
    let gun = c.preferred_gun(obs, t);
    let moving = c.steer(obs, Some(t), &gun);
    let mut a = Act::new(moving, format!("outmatched: keeping clear of {}", t.name));
    // (and no ammo spent on it when all she has couldn't kill it, unless it's in her way: every
    // round a round short for what she can kill; E4M4, two cacodemons kept clear of and shot at
    // till every gun was empty, then bitten to death unarmed)
    if c.ammo_short_against(obs, t) && !c.in_her_way(obs, t) {
        a.hold_fire = true;
        a.why = format!("outmatched: keeping clear of {} (ammo kept)", t.name);
        // (a hitscanner's bullets reach her at any range: no shot to take, out of its sight, not
        // further off in it; MAP06 UV, the spider mastermind 500-700 off, kept clear of back and
        // forth in its sight 20 s, the straight duck nowhere near enough, shot to death)
        if mon(t.name).attack == Attack::Hitscan {
            if let Some(way) = c.out_of_sight_of(obs, t) {
                a.moving = Some(way);
                a.why = format!("outmatched: out of {}'s sight (ammo kept)", t.name);
                return Some(a);
            }
        }
    }
    // (the route walking her away from it already: on along it, further each step. The
    // steering holds her at a distance, in its fire; MAP23, a mancubus across a hall of barrels,
    // held 400 off among them till its fireballs set them off round her, dead)
    if c.route_way.is_some_and(|way| norm(way - heading((t.x, t.y), pos)).abs() < 90.0) {
        a.moving = None;
        a.route = true;
        a.why.push_str(": on along the route, away");
    }
    Some(a)
}

/// Outmatched: shooting on the way, the route going on.
fn outmatched_get_past(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    (s.outmatched && !s.target.as_ref().is_some_and(|t| fought(c, obs, s, t))).then(|| Act::new(None, "getting past".into()))
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
/// line of fire. Not on a floor that hurts, unless it's in her way: the route held there costs a
/// hurt every 32 tics, the route's way off is the fight's way too (E3M6 HMP: low, ten lost souls
/// 1100-1400 off out of reach, the route over the nukage held for "closing in", 48 to 0).
fn standing(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    if c.on_hurting_floor(obs) && !c.blocking(obs, s.target.as_ref()) {
        return None;
    }
    // (not for one she's had no shot at a long while: see `no_shot_long`)
    if !c.blocking(obs, s.target.as_ref()) && s.target.as_ref().is_some_and(|t| c.no_shot_long(obs, t)) {
        return None;
    }
    let duel = s.target.as_ref().is_some_and(|t| fought(c, obs, s, t));
    if !(s.threat >= STAND_THREAT || s.health < LOW_HEALTH || duel || c.blocking(obs, s.target.as_ref())) {
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
            if let Some((_, way)) = self.duck(obs) {
                // (out of its sight now: held there till its fire's broken off)
                let why = "ducked: waiting out its fire".to_string();
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
            self.log.push(format!("tic {}: fight over ({} kills; {:.0} rounds spent, {:.0} damage done)", obs.tic, self.kills, self.spent, self.dealt));
            (self.spent, self.dealt) = (0.0, 0.0);
        }
        self.mode.clear();
        self.target = None;
        None
    }
}
