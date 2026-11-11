//! The fight as a tree, each tic: the first node that applies says what she does.
//!
//! ```text
//! decide
//! ├─ nothing in sight, nothing flying at her
//! │    out from under a crusher · out of a vile's sight · ducked: waiting out its fire ·
//! │    lost sight of it: keeping at it ·
//! │    the fight over
//! └─ a target, or a missile on its way
//!    ├─ survive        out from under a crusher · out of a vile's sight · through a field
//!    │                 of barrels · away from a barrel going off · ducking a hitscan heavy ·
//!    │                 dodging · still for the BFG · away from a barrel
//!    ├─ busy           (a jump: the route keeps the legs) shooting on the way
//!    ├─ melee          (the weapon she'd fight it with, up close: `Style::Melee`) closing in
//!    │                 · at it
//!    └─ fight          setting two bosses on each other · backing off from what claws ·
//!                      sneaking past · shooting on the way to an errand · out of ammo:
//!                      getting past · closing in (out of reach) · out of reach: going on ·
//!                      stepping for a shot · outmatched: keeping clear · getting past ·
//!                      keeping off · standing and fighting · shooting on the way
//! ```
//!
//! The nodes are tried in that order and the first to give an `Act` wins. A node may look,
//! keep something (a clock, the steering's way) and still pass, as the branches it was cut from
//! did. The aim and the trigger are sized up before the tree (`Situation`): a node only
//! overrides them. How things stand besides (her health, the threat, outmatched or not) is
//! `c.now`, the tic's assessment.

use super::moves::{HeldFire, Move};
use super::patience::Effort;
use super::style::{window, Style};
use super::{mon, Combat, Decision};
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
    /// which decision it is: what the rest of her goes by
    pub kind: Move,
    pub moving: Option<Point>,
    /// the same, for the log
    pub why: String,
    /// turned to this rather than as sized up
    pub aim: Option<f64>,
    /// the trigger let go
    pub hold_fire: bool,
    /// the route keeps the legs (whatever else: missiles flying)
    pub route: bool,
}

impl Act {
    fn new(kind: Move, moving: Option<Point>, why: String) -> Act {
        Act {
            kind,
            moving,
            why,
            aim: None,
            hold_fire: false,
            route: false,
        }
    }
}

type Node = fn(&mut Combat, &Observation, &Situation) -> Option<Act>;

/// Before anything else: out of the way of what would kill her outright.
const SURVIVE: [Node; 8] = [
    off_crusher,
    out_of_vile_sight,
    through_barrels,
    off_lit_barrel,
    ducking,
    dodging,
    bfg_hold,
    off_barrel,
];
/// In a jump, the route has the legs.
const BUSY: [Node; 1] = [busy];
/// The weapon she'd fight her target with fought up close.
const MELEE: [Node; 1] = [melee];
/// The fight itself.
const FIGHT: [Node; 14] = [
    setting_on,
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
    let mut act = SURVIVE
        .iter()
        .chain(&BUSY)
        .chain(&MELEE)
        .chain(&FIGHT)
        .find_map(|node| node(c, obs, s).filter(|a| !c.barred.contains(&a.kind)))
        .unwrap_or_else(|| Act::new(Move::ShootingOnTheWay, None, "shooting on the way".into()));
    // No shot at either of two bosses she'd set on each other, whatever the move: a shot of hers
    // turns it back on her. Once one is dead the other is fought as anything else is.
    if !act.hold_fire
        && s.target.as_ref().is_some_and(|t| mon(t.name).boss)
        && c.boss_pair(obs).is_some()
    {
        act.hold_fire = true;
    }
    act
}

// --- survive -----------------------------------------------------------------------------------

/// Out of an arch-vile's sight (its fire goes off only if it can still see her), facing it: the
/// blast's put the far side of the shelter.
fn out_of_vile_sight(c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    let (name, way) = s.vile?;
    let mut a = Act::new(
        Move::OutOfVileSight,
        way,
        format!("out of the {name}'s sight"),
    );
    a.aim = c.vile_face().map(|v| heading(s.pos, v));
    Some(a)
}

/// Out of a hitscan shooter's sight before its first shot (it turns to her first), in a long
/// fight: shooting on the way, as sized up. Not into a missile's way: a direct hit costs more
/// than the bullets would, so the dodge comes first.
fn ducking(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let (name, way) = s.duck?;
    if !c.clear_of_hits(obs, &s.missiles, way) {
        return None;
    }
    Some(Act::new(
        Move::Ducking,
        way,
        format!("ducking out of the {name}'s sight"),
    ))
}

/// Out of the way of the missile due soonest.
fn dodging(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let m = s.missiles.first()?;
    // A homing missile circling her is dodged towards a corner it will fly into.
    if let Some((name, way)) = c.shake_off(obs, &s.missiles) {
        let moving = c.dodge(obs, &s.missiles, way);
        return Some(Act::new(
            Move::ShakingOff,
            moving,
            format!("shaking off {name} round a corner"),
        ));
    }
    let moving = c.dodge(obs, &s.missiles, None);
    let why = if s.errand.is_empty() {
        format!("dodging {}", m.name)
    } else {
        format!("dodging {} on the way to the {}", m.name, s.errand)
    };
    Some(Act::new(Move::Dodging, moving, why))
}

/// Her BFG charging or its ball in the air: still, where she is (the spray comes from her when it
/// lands, along its way: walked round a corner, it hits the wall). Missiles still dodged first.
fn bfg_hold(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    c.bfg_flying(obs).then(|| {
        Act::new(
            Move::BfgHold,
            None,
            "holding still for the BFG's spray".into(),
        )
    })
}

/// Out from under a crusher first, whatever else (a missile may hit; a crusher at work does).
/// With a route under way, the route keeps the legs: its own crusher logic times the crossing
/// and takes her out, through or back, by the nearest way to room overhead. Without one, the
/// shortest straight way out. Never by the fight's steering, which sees every way under a
/// crusher as clear and would hold her there.
fn off_crusher(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    // Any crusher at work counts, between its strokes too: it is no place for a fight.
    if !c.under_crusher(obs) && !c.in_crusher(obs) {
        return None;
    }
    if c.routed {
        let mut a = Act::new(
            Move::OffCrusher,
            None,
            "out from under a crusher (the route's way)".into(),
        );
        a.route = true;
        return Some(a);
    }
    if s.busy {
        return None;
    }
    let moving = c.way_out_of_crusher(obs).or_else(|| {
        let gun = s
            .target
            .as_ref()
            .map_or_else(|| c.gun(obs), |t| c.preferred_gun(obs, t));
        c.steer(obs, s.target.as_ref(), &gun)
    });
    Some(Act::new(
        Move::OffCrusher,
        moving,
        "out from under a crusher".into(),
    ))
}

/// A barrel going off by her: its blast (up to 128) before any missile's dodge.
fn off_lit_barrel(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.barrel_lit.map(|away| {
        Act::new(
            Move::OffLitBarrel,
            Some(away),
            "away from a barrel going off".into(),
        )
    })
}

/// In a field of barrels (`barrels.field` or more in the chain whose blasts reach her) with
/// something that shoots awake: no standing in it to fight, dodge or keep clear. On along her
/// route flat out, the trigger let go: a shot of hers sets them off as surely as theirs.
fn through_barrels(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    if !c.routed || c.now.shooters.is_empty() || Combat::barrel_field(obs) < c.tune.barrels.field {
        return None;
    }
    let mut a = Act::new(
        Move::ThroughBarrels,
        None,
        "in a field of barrels: on through it".into(),
    );
    a.route = true;
    a.hold_fire = true;
    Some(a)
}

/// Away from a barrel by her while something that shoots is in sight: a stray shot sets it off.
fn off_barrel(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.barrel_away
        .map(|away| Act::new(Move::OffBarrel, Some(away), "away from a barrel".into()))
}

// --- busy --------------------------------------------------------------------------------------

/// In a jump or a timed crossing the route keeps the legs; she still shoots.
fn busy(_c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    s.busy
        .then(|| Act::new(Move::ShootingOnTheWay, None, "shooting on the way".into()))
}

// --- up close ----------------------------------------------------------------------------------

/// The target is one she'd fight up close (`Style::Melee`): straight at it, then standing at it
/// hitting. The steering isn't used: its pull is to a gun's window, hundreds of units out, and
/// it never brings her into a fist's reach.
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
    // Not for ever: one she isn't hurting is given up on a while, as one out of reach is. The
    // clock is set back only when it is hurt, not when it is near: one on a floor below can be
    // close across and still out of the fist's slope.
    if c.patience.spent(
        Effort::MeleeUnhurt,
        t.id,
        obs.tic,
        c.tune.engage.close_give_up,
        Some((f64::from(t.health), 0.0)),
        None,
    ) {
        c.give_up(obs, t.id);
        return None;
    }
    Some(if d > w.reach - c.tune.engage.melee_slack {
        // Nor closing in on what she gets no nearer to: the way straight at it is blocked (a
        // ledge above her, a wall between) and the fist has no other. It is given up on, and the
        // sector it stood in remembered (`fist_stalled`), so the fist isn't chosen for it again
        // from outside that sector.
        let stall = c.tune.engage.melee_stall;
        if c.patience.spent(
            Effort::MeleeNoNearer,
            t.id,
            obs.tic,
            stall,
            Some((d, c.tune.engage.melee_gain)),
            Some(stall),
        ) {
            if let Some(sector) = c.sector_at((t.x, t.y)) {
                c.fist_stalled.insert(t.id, sector);
            }
            c.give_up(obs, t.id);
            return None;
        }
        Act::new(
            Move::MeleeClosing,
            c.closing_in(obs, t, None),
            format!("closing in on {} (melee)", t.name),
        )
    } else {
        Act::new(Move::MeleeAt, None, format!("at {} (melee)", t.name))
    })
}

// --- the fight ---------------------------------------------------------------------------------

/// Backing off from the nearest thing that claws, when it's close and she is armed, or low, or
/// one hit of it could kill her. Unarmed and in no such danger she doesn't: a clawing monster
/// follows for ever, and the route takes her past it faster than backing off does.
fn backing_off(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    // Nothing can hurt her (a sphere, or the practice pass): nothing to back off from.
    if c.now.invulnerable {
        return None;
    }
    let pos = s.pos;
    let armed = c.armed(obs);
    let nearest = s
        .close
        .iter()
        .filter(|t| {
            armed || c.now.health < c.tune.engage.low_health || claw_max(t.name) >= c.now.health
        })
        .min_by(|a, b| dist((a.x, a.y), pos).total_cmp(&dist((b.x, b.y), pos)))?;
    let t = nearest.clone();
    let gun = c.preferred_gun(obs, &t);
    let moving = c
        .steer(obs, Some(&t), &gun)
        .or_else(|| c.away_from(obs, (t.x, t.y)));
    let why = if s.errand.is_empty() {
        format!("backing off from {}", nearest.name)
    } else {
        format!("backing off from {} towards the {}", nearest.name, s.errand)
    };
    Some(Act::new(Move::BackingOff, moving, why))
}

/// On to the errand, shooting on the way. A monster standing against her on the way to it is
/// aimed at first, whatever the target: the route can only push into its body, so it has to be
/// shot out of the way.
fn to_errand(c: &mut Combat, _obs: &Observation, s: &Situation) -> Option<Act> {
    if s.errand.is_empty() {
        return None;
    }
    let mut a = Act::new(
        Move::ToErrand,
        None,
        format!("shooting on the way to {}", s.errand),
    );
    let way = c.toward.map(|to| heading(s.pos, to));
    let in_the_way = c
        .visible
        .iter()
        .filter(|t| mon(t.name).attack != Attack::None)
        .find(|t| {
            dist((t.x, t.y), s.pos)
                < PLAYER_RADIUS + mon(t.name).radius + c.tune.engage.errand_contact
                && way.is_some_and(|w| norm(heading(s.pos, (t.x, t.y)) - w).abs() < 60.0)
        });
    if let Some(t) = in_the_way {
        a.aim = Some(heading(s.pos, (t.x, t.y)));
        a.why = format!("shooting on the way to {}: {} in the way", s.errand, t.name);
    }
    Some(a)
}

/// Nothing to shoot with: the route goes on past them. She doesn't walk up to punch.
fn unarmed(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    (!c.armed(obs)).then(|| Act::new(Move::Unarmed, None, "out of ammo: getting past".into()))
}

/// A shot that would wake a sleeping pack worth too much (`Combat::would_wake`) isn't fired, on
/// an errand too: the route keeps the legs and the trigger is let go.
fn sneaking(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let woken = s
        .target
        .as_ref()
        .map(|t| c.would_wake(obs, t))
        .filter(|w| !w.is_empty())?;
    let mut a = Act::new(
        Move::Sneaking,
        None,
        format!("sneaking past (a shot would wake {woken})"),
    );
    a.hold_fire = true;
    Some(a)
}

/// Awake and after her, only too far for any gun she'd bring up: closer, till the closing has
/// gone on too long without getting it in reach (`Combat::still_closing`).
fn closing_out_of_reach(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let t = s.target.as_ref().filter(|t| {
        !c.reachable(obs, t)
            && t.after_her
            && !c.now.outmatched
            && c.known.get(&t.id).is_some_and(|k| k.awake)
            && c.still_closing(obs, t.id)
    })?;
    let gun = c.preferred_gun(obs, t);
    let moving = c
        .steer(obs, Some(t), &gun)
        .or_else(|| c.closing_in(obs, t, None));
    Some(Act::new(
        Move::ClosingOutOfReach,
        moving,
        format!("closing in on {} (out of reach)", t.name),
    ))
}

/// Out of reach of every gun she'd bring up, and not being closed in on: the route goes on.
fn out_of_reach(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    s.target
        .as_ref()
        .is_some_and(|t| !c.reachable(obs, t))
        .then(|| Act::new(Move::OutOfReach, None, "out of reach: going on".into()))
}

/// The only one in sight, near, and no shot at it from here: a step to where there is one. Not
/// on a floor that hurts: the step keeps her on it, and the shot can be taken on the way off.
fn stepping_for_shot(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    if c.on_hurting_floor(obs) {
        return None;
    }
    let pos = s.pos;
    let way = s
        .target
        .as_ref()
        .filter(|t| {
            c.visible.len() == 1 && dist((t.x, t.y), pos) < 512.0 && !c.can_hit(obs, t, None)
        })
        .and_then(|t| c.shot_way(obs, t))?;
    Some(Act::new(
        Move::SteppingForShot,
        Some(way),
        format!(
            "stepping for a shot at {}",
            s.target.as_ref().map_or("", |t| t.name)
        ),
    ))
}

/// Plasma up and autoaim taking it short of the target (into a ledge's lip with them down in a
/// pit, or the floor in front of them up on one): a step on to where it would get there, never
/// off the ledge. Not for rockets (see `plasma_spot`).
fn stepping_for_plasma(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let t = s
        .target
        .as_ref()
        .filter(|_| obs.weapon == Some(6) && c.held_fire == Some(HeldFire::PlasmaBlocked))?;
    let way = c.plasma_spot(obs, t)?;
    Some(Act::new(
        Move::SteppingForShot,
        Some(way),
        format!("stepping for a shot at {} (its way blocked)", t.name),
    ))
}

/// Fought where it stands, whatever else: a sniper she'd see off first (`takes_on`), or,
/// outmatched, a hitscan shooter she'd see off quick (`quick_hitscan`): kept clear of or walked
/// on past, its bullets reach her all the same.
fn fought(c: &Combat, obs: &Observation, t: &Thing) -> bool {
    c.takes_on(obs, t.name) || (c.now.outmatched && c.quick_hitscan(obs, t.name))
}

/// Two bosses about, one with a gun: set on each other (`Combat::set_on`). To where the other's
/// between her and the gun, the trigger let go (a shot of hers turns them on her); the two at
/// each other, left to it: on along the route, the trigger let go still.
fn setting_on(c: &mut Combat, obs: &Observation, _s: &Situation) -> Option<Act> {
    let (gun, other, to) = c.set_on(obs)?;
    let mut a = match to {
        Some(way) => Act::new(
            Move::SettingOn,
            way,
            format!(
                "getting the {} between her and the {}",
                other.name, gun.name
            ),
        ),
        None => Act::new(
            Move::LeavingToIt,
            None,
            format!(
                "leaving the {} and the {} to each other",
                gun.name, other.name
            ),
        ),
    };
    a.hold_fire = true;
    Some(a)
}

/// Outmatched by something awake within `engage.keep_clear` that she wouldn't fight where it
/// stands (`fought`): her distance kept, by the fight's steering. Still shooting, unless all the
/// ammo she has couldn't kill it.
fn outmatched_keep_clear(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    let pos = s.pos;
    let t = s.target.as_ref().filter(|t| {
        c.now.outmatched
            && !fought(c, obs, t)
            && c.known.get(&t.id).is_some_and(|k| k.awake)
            && dist((t.x, t.y), pos) < c.tune.engage.keep_clear
    })?;
    let gun = c.preferred_gun(obs, t);
    let moving = c.steer(obs, Some(t), &gun);
    let mut a = Act::new(
        Move::KeepingClear,
        moving,
        format!("outmatched: keeping clear of {}", t.name),
    );
    // No ammo is spent on it when all she has couldn't kill it, unless it is in her way: every
    // round would be a round short for what she can kill.
    if c.ammo_short_against(obs, t) && !c.in_her_way(obs, t) {
        a.hold_fire = true;
        a.why = format!("outmatched: keeping clear of {} (ammo kept)", t.name);
        // A hitscanner's bullets reach her at any range, so with no shot to take she goes out of
        // its sight, not further off in it.
        if mon(t.name).attack == Attack::Hitscan {
            if let Some(way) = c.out_of_sight_of(obs, t) {
                a.moving = Some(way);
                a.kind = Move::HidingFromOutmatcher;
                a.why = format!("outmatched: out of {}'s sight (ammo kept)", t.name);
                return Some(a);
            }
        }
    }
    // The route is walking her away from it already: on along the route, further each step.
    // The steering would hold her at a distance, still in its fire.
    if c.route_way
        .is_some_and(|way| norm(way - heading((t.x, t.y), pos)).abs() < 90.0)
    {
        a.moving = None;
        a.route = true;
        a.why.push_str(": on along the route, away");
    }
    Some(a)
}

/// Outmatched: shooting on the way, the route going on.
fn outmatched_get_past(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    (c.now.outmatched && !s.target.as_ref().is_some_and(|t| fought(c, obs, t)))
        .then(|| Act::new(Move::GettingPast, None, "getting past".into()))
}

/// Nearer the target than the gun she'd fight it with wants (`style::window`): back out to it.
fn keeping_off(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    // Nothing can hurt her: nothing to keep off from.
    if c.now.invulnerable {
        return None;
    }
    let pos = s.pos;
    let way = s
        .target
        .as_ref()
        .filter(|t| {
            mon(t.name).attack != Attack::None
                && dist((t.x, t.y), pos) < window(&c.preferred_gun(obs, t), t).0
        })
        .and_then(|t| {
            let gun = c.preferred_gun(obs, t);
            c.steer(obs, Some(t), &gun)
                .or_else(|| c.keeping_off(obs, t, s.aim))
        })?;
    Some(Act::new(
        Move::KeepingOff,
        Some(way),
        format!(
            "keeping off from {}",
            s.target.as_ref().map_or("", |t| t.name)
        ),
    ))
}

/// A step counts as towards something when it's within about 70 degrees of the way to it (the
/// cosine of the angle between them is over this).
const NEARER: f64 = 0.3;

/// The route waits and she fights from where she is: when the threat in sight is enough, or she
/// is low, or the target is one she'd fight where it stands (`fought`), or it is in her way, or
/// her route is about to take her onto ground she's never been on. She moves into the gun's
/// window ("closing in"), else across the line of fire.
///
/// Not on a floor that hurts unless the target is in her way: holding the route there costs a
/// hit every 32 tics, and the route's way off the floor is the fight's way too.
fn standing(c: &mut Combat, obs: &Observation, s: &Situation) -> Option<Act> {
    if c.on_hurting_floor(obs) && !c.blocking(obs, s.target.as_ref()) {
        return None;
    }
    // Not for one she's had no shot at for a long while (`no_shot_long`), in front of her or
    // not: standing gets her no shot either way.
    if s.target.as_ref().is_some_and(|t| c.no_shot_long(obs, t)) {
        return None;
    }
    // Nor for one that can't get at her and that she has no shot at: nothing comes of standing.
    if s.target
        .as_ref()
        .is_some_and(|t| c.cant_reach(obs, t) && !c.can_hit(obs, t, None))
    {
        return None;
    }
    let duel = s.target.as_ref().is_some_and(|t| fought(c, obs, t));
    // Her route is about to take her onto ground she's never been on, with something to shoot
    // from here: from here, then.
    let hold = c.tune.engage.hold_known
        && c.route_into_new(obs)
        && s.target.as_ref().is_some_and(|t| c.reachable(obs, t));
    if !(c.now.threat >= c.tune.engage.stand_threat
        || c.now.health < c.tune.engage.low_health
        || duel
        || hold
        || c.blocking(obs, s.target.as_ref()))
    {
        return None;
    }
    let mut kind = Move::Standing;
    let mut why = "standing and fighting".to_string();
    let moving = if let Some(t) = &s.target {
        let gun = c.preferred_gun(obs, t);
        let d = dist((t.x, t.y), s.pos);
        let mut moving = c.steer(obs, Some(t), &gun);
        // Closed in on this long already without getting it where she wants it: left a while,
        // as `closing_out_of_reach` leaves it.
        if (!c.in_reach(&gun, t, d) || d > window(&gun, t).1) && !c.still_closing(obs, t.id) {
            return None;
        }
        if !c.in_reach(&gun, t, d) || d > window(&gun, t).1 {
            // Out of reach, closing in has to go nearer. The steering keeps to ground she's
            // been on and to the side, so a step of its that isn't towards the target is no
            // closing, and isn't taken for one.
            if !c.in_reach(&gun, t, d) {
                let to = ((t.x - s.pos.0) / d.max(1.0), (t.y - s.pos.1) / d.max(1.0));
                let nearer =
                    |m: &Point| (m.0 * to.0 + m.1 * to.1) / m.0.hypot(m.1).max(1e-6) > NEARER;
                moving = moving
                    .filter(nearer)
                    .or_else(|| c.closing_in(obs, t, None).filter(nearer));
            } else {
                moving = moving.or_else(|| c.closing_in(obs, t, None));
            }
            // Out of reach with no safe step towards it: standing there she neither shoots nor
            // closes, so the route goes on, and a shot is taken on the way when there is one.
            if moving.is_none() && !c.in_reach(&gun, t, d) {
                return None;
            }
            kind = Move::ClosingIn;
            why = format!("closing in on {}", t.name);
        }
        moving
    } else {
        c.strafe_dir(obs, s.aim)
    };
    Some(Act::new(kind, moving, why))
}

/// Nothing above applies: the route goes on and she shoots as she goes.
fn shooting_on_the_way(_c: &mut Combat, _obs: &Observation, _s: &Situation) -> Option<Act> {
    Some(Act::new(
        Move::ShootingOnTheWay,
        None,
        "shooting on the way".into(),
    ))
}

// --- nothing in sight --------------------------------------------------------------------------

impl Combat {
    /// Nothing in sight, nothing flying at her: hidden from an arch-vile's fire (staying hidden
    /// comes first), an awake monster just out of sight fought where it was, else the fight's
    /// over.
    pub(super) fn no_target(
        &mut self,
        obs: &Observation,
        busy: bool,
        errand: &str,
    ) -> Option<Decision> {
        // Out from under a crusher at work when there is no fight and no route under way:
        // between objectives nothing else moves her. A route under way is left alone: it has
        // its own crusher rules, and taking over from it turns a timed crossing back.
        if !busy && !self.routed && self.under_crusher(obs) {
            let gun = self.gun(obs);
            let moving = self.steer(obs, None, &gun);
            let why = "out from under a crusher".to_string();
            if why != self.mode {
                self.log.push(format!("tic {}: {why}", obs.tic));
                self.mode = why.clone();
            }
            return Some(Decision {
                kind: Move::OffCrusher,
                yields: false,
                pressed: false,
                aim: None,
                fire: false,
                weapon: 0,
                moving,
                why,
            });
        }
        if !busy && errand.is_empty() {
            if let Some((name, way)) = self.vile_fire(obs) {
                let why = format!("out of the {name}'s sight");
                if why != self.mode {
                    self.log.push(format!("tic {}: {why}", obs.tic));
                    self.mode = why.clone();
                }
                let aim = self.vile_face().map(|v| heading(obs.pos(), v));
                return Some(Decision {
                    kind: Move::OutOfVileSight,
                    yields: false,
                    pressed: true,
                    aim,
                    fire: false,
                    weapon: 0,
                    moving: way,
                    why,
                });
            }
            if let Some((_, way)) = self.duck(obs) {
                // Out of its sight now: held there till its fire is broken off.
                let why = "ducked: waiting out its fire".to_string();
                if why != self.mode {
                    self.log.push(format!("tic {}: {why}", obs.tic));
                    self.mode = why.clone();
                }
                return Some(Decision {
                    kind: Move::Ducked,
                    yields: false,
                    pressed: false,
                    aim: None,
                    fire: false,
                    weapon: 0,
                    moving: way,
                    why,
                });
            }
            // Hit from where she isn't looking: she turns to that before going after a monster
            // she has lost sight of.
            if let Some(way) = self.look(obs, true) {
                return Some(Decision {
                    kind: Move::Looking,
                    yields: true,
                    pressed: false,
                    aim: Some(way),
                    fire: false,
                    weapon: 0,
                    moving: None,
                    why: "looking round".into(),
                });
            }
            if let Some(d) = self.lost_sight(obs) {
                return Some(d);
            }
        }
        // Nothing to fight that she can see: she turns to what she heard, was hit from or
        // bumped into, or takes a look about as she walks.
        self.glance(obs);
        if let Some(way) = self.look(obs, false) {
            return Some(Decision {
                kind: Move::Looking,
                yields: true,
                pressed: false,
                aim: Some(way),
                fire: false,
                weapon: 0,
                moving: None,
                why: "looking round".into(),
            });
        }
        if !self.mode.is_empty() {
            self.log.push(format!(
                "tic {}: fight over ({} kills; {:.0} rounds spent, {:.0} damage done)",
                obs.tic, self.kills, self.spent, self.dealt
            ));
            (self.spent, self.dealt) = (0.0, 0.0);
        }
        self.mode.clear();
        self.target = None;
        None
    }
}
