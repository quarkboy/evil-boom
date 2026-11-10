//! Evil playing with monsters about: the explorer finds the way, the fight (Combat) takes the
//! controls when it needs them, and between them she goes for what she needs to keep fighting
//! (weapons, ammo, health, armour she has seen) and, once there's nothing else, hunts down the
//! monsters she knows are left (some maps open only when they're dead: E1M8's barons).

use std::collections::BTreeSet;
use std::rc::Rc;

use super::bestiary::{ammo_item, armor_item, health_item, monster, weapon_in, weapon_item, Attack, CHAINSAW, SUPER_SHOTGUN};
use super::arbiter::{self, Legs};
use super::combat::{eye, eye_of, segment_point_distance, spread, Combat, Move, Seen};
use super::tuning::Tuning;
use crate::advisor::{Advisor, Question};
use crate::decisions::Record;
use crate::geom::{dist, Point};
use crate::knowledge::completion::Completionist;
use crate::knowledge::explorer::{Explorer, Hooks, Kind, Objective};
use crate::knowledge::model::Knowledge;
use crate::motor::follower::RouteFollower;
use crate::motor::{Controls, Observation, Thing};
use crate::navigator::{Navigator, States, PLAYER_HEIGHT, PLAYER_RADIUS};
use crate::walkthrough::{Action, Step};

/// the most of each ammo she can carry (without a backpack), by weapon slot
use crate::tactical::bestiary::ammo_max;

/// ...the nearest this many of them
const COVER_SPOTS: usize = 48;
/// ...the nearest this many of them...
const POST_LOOKS: usize = 3000;
/// ...looked at on a grid this fine...
const POST_GRID: f64 = 48.0;
/// monsters whose attacks, landing on others, set them fighting
const HEAVIES: [&str; 7] = ["Cyberdemon", "SpiderMastermind", "Fatso", "Arachnotron", "Revenant", "BaronOfHell", "HellKnight"];
/// units a second she gets along a route, about (turns, doors, lifts)
const RUN_SPEED: f64 = 400.0;
/// the trail back: a place every this far, at most this many
const TRAIL_STEP: f64 = 48.0;
const TRAIL_LEN: usize = 96;

/// (barons' and knights' fireballs don't hurt each other)
fn species(name: &str) -> &str {
    if name == "HellKnight" {
        "BaronOfHell"
    } else {
        name
    }
}

/// The rest of a route passes none of them nearer than `clear`.
fn keeps_clear(obs: &Observation, f: &RouteFollower, them: &[Thing], clear: f64) -> bool {
    let path = f.planned_path(Some(obs.pos()));
    !path.is_empty()
        && path.windows(2).filter_map(|w| Some((w[0]?, w[1]?))).all(|(a, b)| them.iter().all(|t| segment_point_distance(a, b, (t.x, t.y)) >= clear))
}

fn mhealth(name: &str) -> f64 {
    monster(name).map_or(0.0, |m| f64::from(m.health))
}

/// What the player's hooks into the explorer need: the fight's state.
struct Fight {
    combat: Combat,
    /// the numbers she judges by (the fight's are the same)
    tune: Rc<Tuning>,
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
    /// the polys not yet seen of the sectors such guns lie in
    toward_gun: BTreeSet<i64>,
}

/// Why she died, as best her plight then tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    Blast,
    Unarmed,
    HurtingFloor,
    Surrounded,
    Outmatched,
    WornDown,
}

impl Cause {
    fn label(self) -> &'static str {
        match self {
            Cause::Blast => "a blast",
            Cause::Unarmed => "out of ammo",
            Cause::HurtingFloor => "on a floor that hurts",
            Cause::Surrounded => "surrounded",
            Cause::Outmatched => "outmatched",
            Cause::WornDown => "worn down",
        }
    }
}

/// A death: where, and what she was going for.
#[derive(Clone, Debug)]
struct Death {
    at: Point,
    kind: Kind,
    goal: Point,
    label: String,
    why: Cause,
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
    fn why(&self, blast_hit: f64) -> Cause {
        if self.last_hit >= blast_hit && !self.close {
            Cause::Blast
        } else if !self.armed {
            Cause::Unarmed
        } else if self.hurting_floor {
            Cause::HurtingFloor
        } else if self.surrounded {
            Cause::Surrounded
        } else if self.outmatched {
            Cause::Outmatched
        } else {
            Cause::WornDown
        }
    }
}

/// Why she's falling back: each kind starts on its own rule (`Player::start_retreat`, in that
/// order) and ends on its own (`Player::retreat_over`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetreatKind {
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
    const ALL: [RetreatKind; 6] = [RetreatKind::Cover, RetreatKind::ForAmmo, RetreatKind::Back, RetreatKind::Out, RetreatKind::Bait, RetreatKind::Post];

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
    /// the last retreat tried was for cover
    tried_cover: bool,
    /// when a way off for ammo was last called off with one of them on it
    walled_tic: i64,
    /// when something that shoots was last in sight
    shooters_tic: i64,
    /// where she was last tic (where she died, at a restart)
    last_pos: Point,
    /// and what things were like for her
    plight: Plight,
    /// who had her legs last tic (the journal notes a change)
    legs: Legs,
    /// the things lying about that she's seen (items, and whatever else isn't a monster or a
    /// missile), by id: seen when the floor they're on was in her view with them on it
    things_seen: BTreeSet<i32>,
    /// a decision model's say, if a run's asked for one (`crate::advisor`)
    advisor: Option<Advisor>,
    /// the tuning she plays by with no stance over it
    base_tune: Rc<Tuning>,
    /// the stance she's in by the advisor's word ("" none), and till when
    stance: (String, i64),
    /// the advisor's target holds till then
    target_until: i64,
    /// when a target and a stance were last asked
    asked: (i64, i64),
    /// retreats she isn't to make (`bar`)
    barred_retreats: Vec<RetreatKind>,
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
    pub fn new(nav: Rc<Navigator>, knowledge: Knowledge, tune: Rc<Tuning>) -> Player {
        Player {
            explorer: Explorer::new(Rc::clone(&nav), knowledge),
            fight: Fight { combat: Combat::new(nav, Rc::clone(&tune)), tune: Rc::clone(&tune), pending_log: Vec::new(), offered: BTreeSet::new(), present: BTreeSet::new(), completion: None, deaths: Vec::new(), rethink: false, gun_out_of_reach: false, toward_gun: BTreeSet::new() },
            retreat: None,
            bait_tic: -1_000_000_000,
            retreat_tic: -1_000_000_000,
            tried_cover: false,
            walled_tic: -1_000_000_000,
            shooters_tic: -1_000_000_000,
            last_pos: (0.0, 0.0),
            plight: Plight::default(),
            legs: Legs::Route,
            things_seen: BTreeSet::new(),
            advisor: None,
            base_tune: Rc::clone(&tune),
            stance: (String::new(), 0),
            target_until: 0,
            asked: (i64::MIN / 2, i64::MIN / 2),
            barred_retreats: Vec::new(),
            trail: Vec::new(),
            exposure: None,
            edge_log: i64::MIN / 2,
        }
    }

    /// Every item that counts as well (the exit held back till nothing's left: --complete);
    /// `restarts` levels again she may ask for, for ones out of reach.
    /// Something that lies where it is (an item, a lamp, a column): not a monster or a missile.
    fn lies_about(t: &Thing) -> bool {
        t.category != "Monster" && t.category != "Projectile" && monster(t.name).is_none() && super::bestiary::missile(t.name).is_none()
    }

    /// The things lying about that have come into her view: on a floor in view this tic (her
    /// eyes': the explorer's, `Knowledge::seen_tic`). Once seen it's known
    /// to be there till she sees it isn't. What a monster drops dying round a corner, she finds
    /// when she looks.
    fn notice_things(&mut self, obs: &Observation) {
        let (k, nav) = (&self.explorer.knowledge, &self.explorer.nav);
        for t in obs.things.iter().flatten().filter(|t| Self::lies_about(t)) {
            if self.things_seen.contains(&t.id) {
                continue;
            }
            let in_view = nav.poly_at((t.x, t.y)).is_some_and(|p| k.seen_tic.get(&nav.sector_of_poly(p)).is_some_and(|&tic| tic == obs.tic));
            if in_view {
                self.things_seen.insert(t.id);
            }
        }
    }

    /// The game as she perceives it: the observation with what she couldn't know taken out
    /// (docs/REALISM.md). What decides anything is given this, not the game's own. So far: only
    /// the barrels she's seen (where they are now: pushed about, they're the ones she saw), the
    /// things lying about that she's seen (`notice_things`), the monsters in her sight and the
    /// missiles she's noticed (the fight's senses, `Combat::observe`).
    fn percept(&self, obs: &Observation) -> Observation {
        let mut seen = obs.clone();
        if let Some(things) = &mut seen.things {
            let c = &self.fight.combat;
            things.retain(|t| {
                if monster(t.name).is_some() {
                    c.visible.iter().any(|v| v.id == t.id)
                } else if super::bestiary::missile(t.name).is_some() {
                    c.missiles_seen.contains(&t.id)
                } else {
                    !Self::lies_about(t) || self.things_seen.contains(&t.id)
                }
            });
        }
        let barrels = &self.fight.combat.barrels_seen;
        seen.barrels = obs.barrels.as_ref().map(|_| barrels.clone());
        seen.barrels_lit.retain(|&l| barrels.iter().any(|&b| dist(b, l) < 1.0));
        seen
    }

    /// A decision model to ask (see `crate::advisor`).
    pub fn advise(&mut self, advisor: Advisor) {
        self.advisor = Some(advisor);
    }

    /// The advisor's answers that are due, taken; then what there is to ask it, asked. An
    /// answer holds a while (`hold_tics`) and lapses: the rules', and the tuning she came with.
    fn consult(&mut self, obs: &Observation) {
        let Some(advisor) = self.advisor.as_mut() else { return };
        let (hold, every) = (advisor.config.hold_tics, (advisor.config.target_every, advisor.config.stance_every));
        let mut said: Vec<String> = std::mem::take(&mut advisor.errors);
        for a in advisor.due(obs.tic) {
            match a.point {
                "target" => {
                    let Some(id) = a.choice.strip_prefix('m').and_then(|n| n.parse::<i32>().ok()) else { continue };
                    self.fight.combat.forced_target = Some(id);
                    self.target_until = obs.tic + hold;
                }
                "stance" => {
                    let Some(stance) = advisor.config.stances.iter().find(|s| s.name == a.choice) else { continue };
                    match self.base_tune.with(&stance.tuning) {
                        Ok(t) => {
                            let t = Rc::new(t);
                            self.fight.combat.tune = Rc::clone(&t);
                            self.fight.tune = t;
                            self.stance = (stance.name.clone(), obs.tic + hold);
                        }
                        Err(e) => {
                            said.push(format!("the {} stance: {e}", stance.name));
                            continue;
                        }
                    }
                }
                _ => continue,
            }
            said.push(format!("advised (asked at tic {}): {} {}, {:.0}% sure", a.asked, a.point, a.choice, a.share * 100.0));
            self.explorer.journal.note(|| Record { detail: format!("{:.2}", a.share), ..Record::new(obs.tic, "advised", format!("{} {}", a.point, a.choice)) });
        }
        if obs.tic >= self.target_until {
            self.fight.combat.forced_target = None;
        }
        if !self.stance.0.is_empty() && obs.tic >= self.stance.1 {
            self.stance.0.clear();
            self.fight.combat.tune = Rc::clone(&self.base_tune);
            self.fight.tune = Rc::clone(&self.base_tune);
        }
        let now = &self.fight.combat.now;
        let state = || now.words(obs.armor.unwrap_or(0.0));
        if now.against_her.len() >= 2 && obs.tic - self.asked.0 >= every.0 && !advisor.waiting("target") {
            self.asked.0 = obs.tic;
            let options = now.against_her.iter().map(|t| (format!("m{}", t.id), format!("the {} {:.0} away", t.name, dist((t.x, t.y), now.pos)))).collect();
            advisor.ask(Question { point: "target", tic: obs.tic, state: state(), instructions: "Which should she shoot first?".into(), options });
        }
        if !now.against_her.is_empty() && obs.tic - self.asked.1 >= every.1 && !advisor.waiting("stance") {
            self.asked.1 = obs.tic;
            let options = advisor.config.stances.iter().map(|s| (s.name.clone(), s.says.clone())).collect();
            advisor.ask(Question { point: "stance", tic: obs.tic, state: state(), instructions: "How should she fight this?".into(), options });
        }
        for line in said {
            self.explorer.log.push(format!("  [fight] tic {}: {line}", obs.tic));
        }
    }

    /// Other numbers to judge by, from now on (a decision model's choice of stance).
    pub fn set_tuning(&mut self, tune: Rc<Tuning>) {
        self.base_tune = Rc::clone(&tune);
        self.fight.combat.tune = Rc::clone(&tune);
        self.fight.tune = tune;
    }

    /// Decisions she isn't to make (a choice forced: an experiment's, a decision model's): the
    /// fight's moves and the retreats named, by their names (`Move`'s and `RetreatKind`'s, as
    /// the journal gives them), commas between.
    pub fn bar(&mut self, names: &str) -> Result<(), String> {
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            if let Some(m) = Move::named(name) {
                self.fight.combat.barred.push(m);
            } else if let Some(k) = RetreatKind::ALL.into_iter().find(|k| format!("{k:?}") == name) {
                self.barred_retreats.push(k);
            } else {
                let moves: Vec<String> = Move::ALL.iter().map(|m| format!("{m:?}")).collect();
                let retreats: Vec<String> = RetreatKind::ALL.iter().map(|k| format!("{k:?}")).collect();
                return Err(format!("--bar: no move or retreat called {name}; moves: {}; retreats: {}", moves.join(", "), retreats.join(", ")));
            }
        }
        Ok(())
    }

    /// Her target from now on, while it's in sight and after her, whatever the rule would pick
    /// (None: the rule's again).
    pub fn force_target(&mut self, id: Option<i32>) {
        self.fight.combat.forced_target = id;
    }

    /// Keep the journal of decisions (`crate::decisions`).
    pub fn keep_journal(&mut self) {
        self.explorer.journal.on = true;
        self.fight.combat.journal.on = true;
    }

    /// The fight's decisions into the journal, and how things stood put to every record that
    /// hasn't it yet: called before she next looks (the assessment still that tic's), and at
    /// the end.
    pub fn file_decisions(&mut self) {
        if !self.explorer.journal.on {
            return;
        }
        self.explorer.journal.records.append(&mut self.fight.combat.journal.records);
        let state = self.fight.combat.now.summary();
        for r in self.explorer.journal.records.iter_mut().filter(|r| r.state.is_none()) {
            r.state = Some(state.clone());
        }
    }

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
            let d = Death { at: self.last_pos, kind: cur.kind, goal: cur.goal, label: cur.label.clone(), why: self.plight.why(self.fight.tune.deaths.blast_hit) };
            self.explorer.log.push(format!("  [fight] died at ({:.0}, {:.0}) going for: {} ({})", d.at.0, d.at.1, d.label, d.why.label()));
            self.fight.deaths.push(d);
        }
        self.explorer.restart();
        self.fight.combat.restart();
        self.retreat = None;
        self.fight.combat.hold_fire = false;
        self.trail.clear();
    }

    pub fn act(&mut self, obs: &Observation, last: Option<&Controls>) -> Controls {
        self.file_decisions();
        self.last_pos = obs.pos();
        self.fight.present = obs.things.iter().flatten().map(|t| t.id).collect();
        // (her senses take the game as it is; everything after goes by what they made of it)
        self.fight.combat.observe(obs);
        self.explorer.sense(obs, last);
        self.notice_things(obs);
        let seen = self.percept(obs);
        let obs = &seen;
        self.consult(obs);
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
        // (falling back, the retreat's way is the way she's going: a dodge as clear along it
        // takes it, not the errand's. Its errand the key she stood on, she dodged back and forth
        // over it under fire from all round, the way back out never taken)
        let back = self.retreat.as_ref().and_then(|r| r.route.planned_path(Some(obs.pos())).into_iter().flatten().find(|&p| dist(p, obs.pos()) > self.fight.tune.retreat.ahead));
        self.fight.combat.toward = if busy { toward } else { back.or(toward) };
        self.fight.combat.routed = self.explorer.follower.as_ref().is_some_and(|f| !f.done && !f.failed);
        // (what she knows of floors gone to something that hurts, for where the fight takes her)
        self.fight.combat.hazards = self.explorer.knowledge.hazards(obs.tic);
        // (and the crushers she knows of, for where they could have her if they start again)
        self.fight.combat.crushers = self.explorer.knowledge.crusher_sectors();
        let d = self.fight.combat.decide(obs, busy, &errand);
        let retreating = !busy && self.retreating(obs);
        let legs = arbiter::arbitrate(d.as_ref(), busy, retreating, self.fight.combat.dodge_clear());
        if legs != self.legs {
            self.legs = legs;
            self.explorer.journal.note(|| Record::new(obs.tic, "legs", format!("{legs:?}")));
        }
        if retreating {
            self.explorer.hold(obs, last);
            let mut c = self.retreat.as_mut().map_or_else(Controls::default, |r| r.route.act(obs));
            if self.hold_at_edge(obs, &c) {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            }
            return match d {
                // (a missile about to hit still comes first, when there's a way clear of it)
                Some(d) if d.kind == Move::Dodging && legs == Legs::Fight => self.fight.combat.controls(obs, &d, None, true),
                // (the retreat's legs, the fight aiming over them; or the fight's move, the
                // retreat's way where its steering leans)
                Some(d) => {
                    let pressing = self.retreat.as_ref().is_some_and(|r| r.route.pressing(obs.tic));
                    Self::press_turn(self.fight.combat.controls(obs, &d, Some(&c), legs == Legs::Fight), &c, pressing)
                }
                None => c,
            };
        }
        if let Some(d) = d.as_ref().filter(|_| legs == Legs::Fight) {
            self.explorer.hold(obs, last);
            let mut c = self.fight.combat.controls(obs, d, None, true);
            // (the fight's own way too, unless it's out of the way of something: keeping clear
            // took her out into the spider mastermind's sight as surely as a route)
            if !d.kind.escaping() && self.hold_at_edge(obs, &c) {
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
                let pressing = self.explorer.follower.as_ref().is_some_and(|f| f.pressing(obs.tic));
                let mut c = Self::press_turn(self.fight.combat.controls(obs, &d, Some(&c), false), &c, pressing);
                if self.hold_at_edge(obs, &c) {
                    (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                }
                c
            }
            None => c,
        }
    }

    /// The fight's controls over a route's (`route`): turned the route's way while the route's
    /// waiting to press a door or switch and nothing's fired this tic. Use goes where she faces;
    /// turned to aim at what she isn't shooting, the press never came (E4M5 UV: falling back to
    /// cover behind a shut door, 3 shells, an imp 1100 off out of reach aimed at, 6000 tics at
    /// the door, never pressed).
    fn press_turn(mut c: Controls, route: &Controls, pressing: bool) -> Controls {
        if pressing && !c.attack {
            c.turn = route.turn;
        }
        c
    }

    fn plight_now(&mut self, obs: &Observation) -> Plight {
        let nav = &self.explorer.nav;
        let pos = obs.pos();
        let hurting_floor = nav.poly_at(pos).is_some_and(|p| self.explorer.knowledge.damaging(nav.sector_of_poly(p)));
        let c = &self.fight.combat;
        let (last_hit, close) = match c.last_hit() {
            Some((tic, amount)) if obs.tic - tic <= 1 => (amount, c.visible.iter().any(|t| dist((t.x, t.y), pos) < 128.0)),
            _ => (self.plight.last_hit, self.plight.close),
        };
        Plight { armed: c.armed(obs), hurting_floor, surrounded: c.now.surrounded, outmatched: c.now.outmatched, last_hit, close }
    }

    // --- backing off ------------------------------------------------------------------------

    fn log(&mut self, s: String) {
        self.explorer.log.push(format!("  [fight] {s}"));
    }

    /// Is she falling back? A retreat under way goes on till it's there, it fails, or its own
    /// end comes; else, not too soon after the last, one may start.
    fn retreating(&mut self, obs: &Observation) -> bool {
        if !self.fight.combat.now.shooters.is_empty() {
            self.shooters_tic = obs.tic;
        }
        if let Some(r) = &self.retreat {
            let walled = r.kind == RetreatKind::ForAmmo && self.walled_off(obs, &r.route);
            if walled {
                self.walled_tic = obs.tic;
            }
            if r.route.done || r.route.failed || self.retreat_over(obs, r.kind) || walled {
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
        // (a try at some other retreat that found no way doesn't hold off cover when it's due:
        // MAP04 UV, a way back tried and not found at full health, cover due 105 tics later at
        // 28, too late)
        if obs.tic - self.retreat_tic < self.fight.tune.retreat.again && (self.tried_cover || !self.cover_due()) {
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
        let calm = obs.tic - self.shooters_tic > self.fight.tune.retreat.clear_tics;
        match kind {
            RetreatKind::Out => since > self.fight.tune.retreat.out_tics,
            RetreatKind::Bait => since > self.fight.tune.retreat.out_tics || self.fight.combat.visible.iter().any(|t| self.fight.combat.infighting(t)),
            RetreatKind::ForAmmo => self.fight.combat.armed(obs) || since > self.fight.tune.retreat.ammo_tics,
            RetreatKind::Back => calm || since > self.fight.tune.retreat.back_tics,
            RetreatKind::Cover => calm,
            RetreatKind::Post => since > self.fight.tune.retreat.post_tics,
        }
    }

    /// Unarmed, the rest of her way runs into one of them: it's moved onto her way since she
    /// chose it, and nothing moves it off (E2M5 on UV: a demon between her and the clip she was
    /// off for, pushed against, bitten from 96 health to 29). One beside her already walls off
    /// only a way that takes her nearer it, as `for_ammo` chose: a way leading off from beside it
    /// isn't into it (MAP19 ITYTD: two spectres on her, the way away dropped the tic after it was
    /// chosen, every time, bitten to death where she stood).
    fn walled_off(&self, obs: &Observation, route: &RouteFollower) -> bool {
        let pos = obs.pos();
        let path = route.planned_path(Some(pos));
        self.fight.combat.now.against_her.iter().any(|t| {
            let touch = (monster(t.name).map_or(20.0, |m| m.radius) + PLAYER_RADIUS).min(dist((t.x, t.y), pos) - self.fight.tune.retreat.ammo_margin);
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
        if self.fight.combat.now.invulnerable || self.fight.combat.bfg_flying(obs) {
            return None; // (nothing to fall back from; or her BFG's spray to come, from where she is)
        }
        let unarmed = !self.fight.combat.armed(obs) && !self.fight.combat.now.against_her.is_empty();
        let on_way = unarmed && !self.fight.combat.now.vile && self.on_for_ammo(obs);
        let (kind, route) = if self.fight.combat.now.vile {
            self.retreat_tic = obs.tic;
            (RetreatKind::Cover, self.find_cover(obs))
        } else if self.cover_due() {
            self.retreat_tic = obs.tic;
            // (unarmed, cover doesn't end it: nothing she does from there kills what's after her,
            // and the cover's end is when they stop shooting. The ammo first, cover if there's no
            // way off for it. E1M5 UV: fists out at 13 health, an imp's fireballs, a demon on her;
            // hid from the imp 190 tics, the demon followed, dead as the cover ended)
            // (and a way off for ammo just called off, one of them on it, is that tried: cover,
            // not the next way off past them, called off the tic after. MAP18 HNTR: out of
            // shells, cacodemons on her, a way off chosen and called off every tic for 30 tics
            // under their fireballs)
            let walled = obs.tic - self.walled_tic < self.fight.tune.retreat.again;
            match (unarmed && !walled).then(|| self.for_ammo(obs, on_way, false)).flatten() {
                Some(r) => (RetreatKind::ForAmmo, Some(r)),
                None if on_way => (RetreatKind::ForAmmo, None),
                None => (RetreatKind::Cover, self.find_cover(obs)),
            }
        } else if self.fight.combat.in_the_open(obs.tic) {
            self.retreat_tic = obs.tic;
            (RetreatKind::Post, self.find_post(obs))
        } else if unarmed {
            self.retreat_tic = obs.tic;
            (RetreatKind::ForAmmo, self.for_ammo(obs, on_way, true))
        } else if let Some(why) = self.ambushed(obs) {
            self.retreat_tic = obs.tic;
            match self.back_out(obs, why) {
                Some(r) => (RetreatKind::Back, Some(r)),
                None if self.fight.combat.now.surrounded => (RetreatKind::Out, self.out_of_it(obs)),
                None => (RetreatKind::Back, None),
            }
        } else if self.fight.combat.now.surrounded {
            self.retreat_tic = obs.tic;
            (RetreatKind::Out, self.out_of_it(obs))
        } else if obs.tic - self.bait_tic > self.fight.tune.retreat.bait_again {
            let b = self.baiting(obs)?;
            self.retreat_tic = obs.tic;
            self.bait_tic = obs.tic;
            self.fight.combat.hold_fire = true;
            (RetreatKind::Bait, Some(b))
        } else {
            return None;
        };
        self.tried_cover = kind == RetreatKind::Cover;
        if self.barred_retreats.contains(&kind) {
            return None;
        }
        if route.is_some() {
            self.explorer.journal.note(|| Record::new(obs.tic, "retreat", format!("{kind:?}")));
        }
        route.map(|route| Retreat { kind, route })
    }

    /// Hurt, with something shooting at her in sight: under RETREAT_HEALTH, or as low as that by
    /// the time she'd get to cover as far off as it's looked for, at the rate she's losing health
    /// (MAP04 UV: a pack of hitscanners raking her in the open, 67 to 28 in 8 tics, the cover 600
    /// back; gone for at 28, dead on the way).
    fn cover_due(&mut self) -> bool {
        let (health, rate) = (self.fight.combat.now.health, self.fight.combat.now.damage_rate);
        (health < self.fight.tune.retreat.health || health - rate * self.fight.tune.retreat.cover_range / RUN_SPEED < self.fight.tune.retreat.health) && !self.fight.combat.now.shooters.is_empty()
    }

    /// Where she stands now, while nothing's shooting at her and nothing's hurting her: on the
    /// trail back.
    fn keep_trail(&mut self, obs: &Observation) {
        let calm = self.fight.combat.now.shooters.is_empty() && self.fight.combat.now.damage_rate == 0.0 && !self.plight.hurting_floor;
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
        let them = self.fight.combat.now.shooters.clone();
        if them.is_empty() {
            return None;
        }
        let pos = obs.pos();
        let close: Vec<Point> = self.fight.combat.now.against_her.iter().filter(|t| dist((t.x, t.y), pos) < self.fight.tune.retreat.ambush_near).map(|t| (t.x, t.y)).collect();
        let losing = self.fight.combat.now.damage_rate > self.fight.tune.retreat.ambush_rate;
        if !losing && close.len() < self.fight.tune.retreat.ambush_count {
            return None;
        }
        let open = self.fight.combat.openness(obs, pos, self.fight.tune.retreat.open_look);
        let sides = spread(pos, &close);
        let bad = if self.plight.hurting_floor {
            "on a floor that hurts"
        } else if open < self.fight.tune.retreat.tight {
            "a tight spot"
        } else if sides > self.fight.tune.retreat.sides {
            "them on several sides"
        } else {
            return None;
        };
        Some(format!("{bad}, {}", if losing { "losing health fast" } else { "several close" }))
    }

    /// Back the way she came, to the nearest place on it none of them can see her (or well clear
    /// of them all), by a way not through them.
    fn back_out(&mut self, obs: &Observation, why: String) -> Option<RouteFollower> {
        let them: Vec<Thing> = self.fight.combat.now.against_her.clone();
        let pos = obs.pos();
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        // (not back onto a floor that hurts now: the way she came may have gone down into it,
        // E3M1's way over the blood, backed out down it into the blood from imps on the far bank)
        let k = &self.explorer.knowledge;
        let trail: Vec<Point> =
            self.trail.iter().rev().copied().filter(|&p| dist(p, pos) > self.fight.tune.retreat.back_min && nav.poly_at(p).is_none_or(|pi| !k.damaging(nav.sector_of_poly(pi)))).collect();
        for p in trail {
            let Some(pi) = nav.poly_at(p) else { continue };
            let z = heights[nav.sector_of_poly(pi)].0;
            let seen = them.iter().any(|t| self.fight.combat.sees(eye(t), p, z, &heights));
            let clear = them.iter().all(|t| dist((t.x, t.y), p) > self.fight.tune.retreat.back_clear);
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
        let them = self.fight.combat.now.against_her.clone();
        let fp = self.fight.combat.now.firepower;
        let heavies: Vec<&Thing> = them.iter().filter(|h| HEAVIES.contains(&h.name) && mhealth(h.name) / fp > self.fight.tune.retreat.bait_heavy).collect();
        if heavies.is_empty() {
            return None;
        }
        let spots = self.visited_spots(obs, 64.0, self.fight.tune.retreat.bait_range, COVER_SPOTS * 2);
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
    /// No plain way to ammo but on her way for some already (`on_way`): on with that, not away.
    /// No way to ammo at all, not hurt (`keep_on`), and her way on clear of them: on with it.
    fn for_ammo(&mut self, obs: &Observation, on_way: bool, keep_on: bool) -> Option<RouteFollower> {
        let them = self.fight.combat.now.against_her.clone();
        let pos = obs.pos();
        // (no nearer to any of them than she is: one on her already, every way off starts by
        // it, and away is still away)
        let nearest = them.iter().map(|m| dist((m.x, m.y), pos)).fold(f64::INFINITY, f64::min);
        let clear = self.fight.tune.errands.past_clear.min(nearest - self.fight.tune.retreat.ammo_margin).max(0.0);
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        let known = self.explorer.knowledge.known_polys();
        let nav = Rc::clone(&self.explorer.nav);
        // (to the thing itself, not the middle of the floor it's on: a big floor's middle can be
        // further off it than she reaches, and she'd stop there none the richer, again and again)
        let mut ammo: Vec<(f64, Point, &'static str, i32)> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| {
                weapon_item(t.name).is_some_and(|s| s != 1 && s != 8)
                    || ammo_item(t.name).is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s)))
            })
            .filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
            .map(|t| (dist((t.x, t.y), pos), (t.x, t.y), t.name, t.id))
            .collect();
        ammo.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut refused: Vec<i32> = Vec::new();
        for (_, at, name, id) in ammo.into_iter().take(6) {
            if let Some(f) = self.go_to_point(obs, at, &them, "for ammo", clear, true) {
                self.log(format!("tic {}: out of ammo: away for the {name}", obs.tic));
                return Some(f);
            }
            refused.push(id);
        }
        // (and the errand for one of those given up: unarmed, with no way to it clear of them,
        // it'd only take her back into them once she's away, again and again. E4M9 UV: a rocket
        // launcher behind a door that opened on an imp and a baron, 8 rockets in hand, 1200 tics
        // of off and back in until the route gave up; chosen afresh, with them known)
        if self.explorer.current.as_ref().is_some_and(|c| c.kind == Kind::Item && c.thing.is_some_and(|t| refused.contains(&t))) {
            self.explorer.give_up(obs, "out of ammo, no way to it clear of them");
        }
        if on_way {
            self.log(format!("tic {}: out of ammo: on her way for it already", obs.tic));
            return None;
        }
        // (no way to ammo clear of them, and her own way on keeps clear of them: on with it, not
        // away. Away finds no ammo; her way on is where new ammo turns up, and every away drags
        // her off it, a lift's window missed each time. MAP06 HMP: out of shells at 198 health, a
        // demon 300-700 off and imps across the map; the way on a lift pressed and ridden in 5 s,
        // away every 3 s, 20 game minutes of it). Clear by the full margin, not the nearer one
        // `clear` allows: with one on her, away gains distance her way on doesn't (MAP06 HMP: two
        // imps 45 off clawing her while she waited on a lift, 120 health to 19). And none of them
        // a hitscanner: no way is clear of its shots (MAP06 HMP: a chaingunner and a sergeant
        // raking her on her way across their room, dead)
        let hitscan = them.iter().any(|t| monster(t.name).is_some_and(|m| matches!(m.attack, Attack::Hitscan)));
        if keep_on && !hitscan && self.route_clear(obs, &them, self.fight.tune.errands.past_clear) {
            self.log(format!("tic {}: out of ammo, none to get to: on her way, clear of them", obs.tic));
            return None;
        }
        let mut spots: Vec<(f64, usize)> = self
            .visited_spots(obs, 128.0, self.fight.tune.retreat.ammo_away, COVER_SPOTS)
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

    /// Unarmed, the explorer's already on her way to ammo or a gun she can use, by a way not past
    /// them: when `for_ammo` finds no plain way to any (on the heights as they are: no lifts,
    /// drops or jumps), that's the way off for ammo. Neither away nor to cover then: nothing she
    /// does there kills what's after her, and the route takes her back the way she came (E4M7 UV:
    /// out of ammo at 39 health, an imp 400 off; the way to the ammo by a door, a drop and a
    /// run-jump; away through the door out of its sight, back through it on the route, 600 s of
    /// it, every errand dropped).
    fn on_for_ammo(&self, obs: &Observation) -> bool {
        let Some(cur) = self.explorer.current.as_ref().filter(|c| c.kind == Kind::Item) else { return false };
        let Some(f) = self.explorer.follower.as_ref() else { return false };
        let name = cur.label.strip_prefix("pick up ").unwrap_or(&cur.label);
        let name = name.split(' ').next().unwrap_or(name);
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        if !(weapon_item(name).is_some_and(|s| s != 1 && s != 8) || ammo_item(name).is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s)))) {
            return false;
        }
        let them = self.fight.combat.now.against_her.clone();
        let nearest = them.iter().map(|m| dist((m.x, m.y), obs.pos())).fold(f64::INFINITY, f64::min);
        let clear = self.fight.tune.errands.past_clear.min(nearest - self.fight.tune.retreat.ammo_margin).max(0.0);
        keeps_clear(obs, f, &them, clear)
    }

    /// The explorer's way on, the rest of it, passes none of them nearer than `clear`.
    fn route_clear(&self, obs: &Observation, them: &[Thing], clear: f64) -> bool {
        self.explorer.current.is_some() && self.explorer.follower.as_ref().is_some_and(|f| keeps_clear(obs, f, them, clear))
    }

    /// A way to that poly (not past monsters), if there is one.
    fn go_to(&mut self, obs: &Observation, pi: usize, them: &[Thing], label: &str) -> Option<RouteFollower> {
        self.go_to_clear(obs, pi, them, label, self.fight.tune.errands.past_clear, false)
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

    /// Seen-by score of spots, by the monsters' eyes.
    fn seen_from(&mut self, obs: &Observation, spots: &[(f64, usize)], them: &[Thing], weigh: bool) -> Vec<f64> {
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        let places: Vec<(Point, f64)> = spots.iter().map(|&(_, pi)| (nav.poly_center(pi), heights[nav.sector_of_poly(pi)].0)).collect();
        let mut seen = vec![0.0; spots.len()];
        for t in them {
            let Some(m) = monster(t.name) else { continue };
            for (i, &(p, z)) in places.iter().enumerate() {
                if self.fight.combat.sees(eye(t), p, z, &heights) {
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
        let spots = self.visited_spots(obs, 96.0, self.fight.tune.retreat.spot_range, COVER_SPOTS * 2);
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
        if c.forward.hypot(c.strafe) < 1.0 || self.fight.combat.now.invulnerable {
            return false;
        }
        let firing: Vec<Seen> = self
            .fight
            .combat
            .remaining()
            .into_iter()
            .filter(|s| self.fight.combat.sniper(obs, s.name))
            .filter(|s| {
                let seen = self.fight.combat.visible.iter().find(|t| t.id == s.id);
                self.fight.combat.firing(obs, s) && seen.is_none_or(|t| t.after_her)
            })
            .collect();
        if firing.is_empty() {
            return false;
        }
        let pos = obs.pos();
        let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
        let ahead = (pos.0 + way.cos() * self.fight.tune.retreat.edge_look, pos.1 + way.sin() * self.fight.tune.retreat.edge_look);
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
        let unhurt = self.fight.combat.now.invulnerable;
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
            obs.tic - tic < self.fight.tune.retreat.exposure_again && was.len() == at.len() && was.iter().zip(&at).all(|(&a, &b)| dist(a, b) < self.fight.tune.retreat.exposure_moved)
        });
        if fresh {
            return;
        }
        let nav = Rc::clone(&self.explorer.nav);
        let heights = self.fight.combat.heights(obs).to_vec();
        let combat = &self.fight.combat;
        let exposure: Vec<f64> = (0..nav.mesh.polys.len())
            .map(|pi| {
                if nav.mesh.polys[pi].is_none() {
                    return 1.0;
                }
                let c = nav.poly_center(pi);
                let z = heights.get(nav.sector_of_poly(pi)).map_or(0.0, |h| h.0);
                let seen = them.iter().any(|s| {
                    dist(c, s.pos()) < self.fight.tune.retreat.exposure_range && combat.sees(eye_of(s), c, z, &heights)
                });
                if seen { self.fight.tune.retreat.exposed } else { 1.0 }
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
        let shooters = self.fight.combat.now.shooters.clone();
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
        let n = (self.fight.tune.retreat.post_range / POST_GRID) as i32;
        let mut grid: Vec<(f64, Point, f64)> = (-n..=n)
            .flat_map(|i| (-n..=n).map(move |j| (pos.0 + f64::from(i) * POST_GRID, pos.1 + f64::from(j) * POST_GRID)))
            .filter_map(|p| {
                let d = dist(p, pos);
                (32.0 < d && d < self.fight.tune.retreat.post_range && fits(p)).then_some((d, p, on_known(p)?))
            })
            .collect();
        grid.sort_by(|a, b| a.0.total_cmp(&b.0));
        let combat = &self.fight.combat;
        let seen = |p: Point, z: f64| shooters.iter().any(|t| combat.sees(eye(t), p, z, &heights));
        let from_them = |p: Point| shooters.iter().map(|t| dist(p, (t.x, t.y))).fold(f64::INFINITY, f64::min);
        let mut posts: Vec<(f64, Point)> = Vec::new();
        for &(d, p, z) in grid.iter().take(POST_LOOKS) {
            if seen(p, z) {
                continue;
            }
            let peek = (0..8).any(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                [self.fight.tune.retreat.post_peek / 2.0, self.fight.tune.retreat.post_peek].into_iter().any(|r| {
                    let q = (p.0 + a.cos() * r, p.1 + a.sin() * r);
                    on_known(q).is_some_and(|qz| seen(q, qz))
                })
            });
            if peek {
                // (the far side of something, the further from them the better, not only the
                // nearest: the user's, E3M8's building between her and the spider mastermind,
                // 500 off with it 1500 away; its bullets spread, the further the fewer land)
                posts.push((d - self.fight.tune.retreat.post_far * from_them(p), p));
            }
        }
        posts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let found = posts.len();
        for g in posts.into_iter().take(4).map(|(_, p)| p) {
            if let Some(f) = self.go_to_point(obs, g, &shooters, "post", self.fight.tune.errands.past_clear, false) {
                self.log(format!("tic {}: in the open: to a post at ({:.0}, {:.0}), health {:.0}", obs.tic, g.0, g.1, obs.health.unwrap_or(0.0)));
                return Some(f);
            }
        }
        self.log(format!("tic {}: in the open, no post in reach ({} places, {found} posts)", obs.tic, grid.len()));
        None
    }

    fn find_cover(&mut self, obs: &Observation) -> Option<RouteFollower> {
        let shooters = self.fight.combat.now.shooters.clone();
        let spots = self.visited_spots(obs, f64::NEG_INFINITY, self.fight.tune.retreat.cover_range, COVER_SPOTS);
        if spots.is_empty() {
            return None;
        }
        let seen = self.seen_from(obs, &spots, &shooters, false);
        // (not in a door shut now: hidden only while it's shut, and open with her in it; MAP20,
        // fallen back to the door out of a cyberdemon's lane, rockets on her waiting at it)
        let open = |pi: usize| obs.heights.get(self.explorer.nav.sector_of_poly(pi)).is_some_and(|h| h.ceiling - h.floor >= PLAYER_HEIGHT);
        let hidden: Vec<usize> = spots.iter().zip(&seen).filter(|(&(d, pi), &v)| v == 0.0 && d > 32.0 && open(pi)).map(|(&(_, pi), _)| pi).collect();
        if hidden.is_empty() {
            self.log(format!("tic {}: no cover in reach", obs.tic));
            return None;
        }
        let view = self.explorer.view();
        let live: States = obs.heights.iter().map(|&h| vec![h]).collect();
        // (open now first; else through a door she can open: the one she came in by may have
        // shut behind her. MAP04 UV: out of the start room through a door that closes again,
        // into the chaingunners' yard; at 28 health the only cover was back behind it, not
        // found shut, shot down where she stood)
        let doors = view.local_doors(obs.keys.unwrap_or_default());
        let hidden: Vec<usize> = hidden.into_iter().take(4).collect();
        let found = [None, Some(&doors)].into_iter().find_map(|d| hidden.iter().find_map(|&pi| {
            let goal = view.nav.poly_center(pi);
            view.find_path(obs.pos(), goal, &live, d).map(|path| (goal, path))
        }));
        if let Some((goal, path)) = found {
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
            .filter(|s| s.awake && self.combat.too_much(obs, &[s.name], self.tune.engage.leave_be))
            .all(|s| segment_point_distance(pos, p, s.pos()) > self.tune.errands.way_clear + monster(s.name).map_or(20.0, |m| m.radius))
    }

    /// The way to the ammo or gun at p, armed with it (obs): clear as above, and all of what's
    /// awake by it together not more than it and what she has would kill. Each alone may be
    /// nothing (MAP05 UV: three imps round a box of four shells, no ammo left; the way clear of
    /// every one of them, gone down to it with the fist and clawed to death with it in hand).
    fn clear_way_armed(&self, obs: &Observation, p: Point) -> bool {
        let pos = obs.pos();
        let by: Vec<&str> = self
            .combat
            .remaining()
            .iter()
            .filter(|s| s.awake && segment_point_distance(pos, p, s.pos()) <= self.tune.errands.way_clear + monster(s.name).map_or(20.0, |m| m.radius))
            .map(|s| s.name)
            .collect();
        self.clear_way_to(obs, p) && !self.combat.ammo_short_for_each(obs, &by)
    }

    /// Something about she can't take on (awake, known): invulnerability's for that.
    fn needs_invulnerability(&self, obs: &Observation) -> bool {
        self.combat.remaining().iter().any(|s| s.awake && self.combat.too_much(obs, &[s.name], self.tune.engage.leave_be))
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
        if !(held.contains(&2) && ammo(2) >= self.tune.barrels.bullets) && !(held.contains(&3) && ammo(3) >= self.tune.barrels.shells) {
            return Vec::new();
        }
        let nav = &x.nav;
        let mut out = Vec::new();
        for &b in &self.combat.barrels_seen {
            if dist(b, pos) > self.tune.barrels.clear_within {
                continue;
            }
            let Some(pi) = reach.iter().copied().filter(|&pi| dist(nav.poly_center(pi), b) < self.tune.barrels.spot).min_by(|&p, &q| dist(nav.poly_center(p), b).total_cmp(&dist(nav.poly_center(q), b))) else { continue };
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
        // (judged on a boss she knows of still about, if any: what the level's fight comes down
        // to, and it takes no rocket's blast. E3M8 from a pistol start: the plasma rifle 246 off
        // "no better" than the launcher, so not gone for first; the spider mastermind wandered
        // into sight of it, it was left, and she was shot down with the rockets spent)
        let boss = self.combat.remaining().iter().filter_map(|s| monster(s.name)).find(|m| m.boss);
        name != "Chainsaw" && Self::new_gun(obs, name) && Combat::dps_on(&Self::gun(obs, name), boss.as_ref()) > self.combat.firepower_on(obs, boss.as_ref())
    }

    /// Seconds of walking a gun is worth: what it'd save killing what she knows is still about
    /// (and some, for what's to come).
    fn gun_worth(&self, obs: &Observation, name: &str) -> f64 {
        let w = Self::gun(obs, name);
        let now = self.combat.now.firepower;
        let hp: f64 = self.combat.remaining().iter().map(|s| mhealth(s.name)).sum();
        self.tune.errands.gun_worth + (hp / now - hp / w.dps().max(now)).max(0.0)
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
            let heals = health_item(name).is_some_and(|(amount, cap)| amount >= 10 && self.combat.now.health < f64::from(cap));
            // (ammo, urgent, only within the reach ammo is turned aside for mid-fight: HMP, six
            // rockets too near four cacodemons to fire, a rocket box 3900 off the errand, backed
            // off "towards" it into them from 180 health to dead)
            let near = dist(cur.goal, obs.pos()) < self.tune.errands.item_range;
            if cur.urgent && health_item(name).is_none() && (near || ammo_item(name).is_none()) {
                return Some((name.to_string(), cur.goal));
            }
            if heals && dist(cur.goal, obs.pos()) < self.tune.errands.heal_near {
                return Some((name.to_string(), cur.goal));
            }
        }
        if self.combat.now.firepower > self.tune.errands.pistol_only && !self.combat.now.short_of_ammo {
            return None;
        }
        self.errands(x, obs).first().map(|t| (t.name.to_string(), (t.x, t.y)))
    }

    /// Guns and ammo she knows where they are and hasn't turned aside for yet: better guns worth
    /// the walk, and, short of ammo, ammo for what she has (or any gun).
    fn errands(&self, x: &Explorer, obs: &Observation) -> Vec<Thing> {
        let known = x.knowledge.known_polys();
        let pos = obs.pos();
        let short = self.combat.now.short_of_ammo;
        let held = obs.weapons.clone().unwrap_or_else(|| vec![1, 2]);
        obs.things
            .iter()
            .flatten()
            .filter(|t| !self.offered.contains(&t.id))
            .filter(|t| {
                if t.name == "InvulnerabilitySphere" {
                    // (in the middle of a fight she can't win as she is: what wins it)
                    return dist((t.x, t.y), pos) < 2.0 * self.tune.errands.item_range && self.needs_invulnerability(obs) && self.clear_way_to(obs, (t.x, t.y));
                }
                if let Some(slot) = weapon_item(t.name).filter(|&s| s != 1 && held.contains(&s) && !Self::new_gun(obs, t.name)) {
                    // (one she has: its ammo)
                    short && held.contains(&slot) && dist((t.x, t.y), pos) < self.tune.errands.item_range
                } else if weapon_item(t.name).is_some() {
                    (self.better_gun(obs, t.name) || (short && Self::new_gun(obs, t.name) && t.name != "Chainsaw")) && dist((t.x, t.y), pos) <= self.gun_worth(obs, t.name) * RUN_SPEED
                } else {
                    short && ammo_item(t.name).is_some_and(|(slots, _)| slots.iter().any(|s| held.contains(s))) && dist((t.x, t.y), pos) < self.tune.errands.item_range
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
        let health = self.combat.now.health;
        let armor = obs.armor.unwrap_or(0.0);
        let held = obs.weapons.clone().unwrap_or_default();
        let ammo = |s: i32| f64::from(obs.ammo.iter().flatten().find(|(x, _)| *x == s).map_or(0, |(_, n)| *n));
        // (then ammo and guns before anything else. Not sooner for having died for want of it:
        // made urgent, past the dangers, MAP12's start, the ammo there walked into for half the
        // map)
        let short = self.combat.now.short_of_ammo;
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
            if need <= 0.1 || (d > self.tune.errands.item_range && need < 0.8) {
                continue;
            }
            let Some(goal) = x.touch_spot((t.x, t.y), reach) else { continue };
            let mut why = "";
            if invulnerability {
                // (E3M9's, behind the cyberdemon, spotted mid-fight and left for the blue key, a
                // wall, a noise; killed at full health by one of its rockets)
                why = " (something about she can't take on)";
            } else if (weapon_item(t.name).is_some() || ammo_item(t.name).is_some()) && !self.clear_way_armed(&Self::armed_with(obs, t.name), (t.x, t.y)) {
                // (something to fight with, but on the way something she couldn't take on even
                // with it: not worth dropping everything for. MAP20, a rocket launcher by the
                // cyberdemon, no ammo left, gone for again and again under its rockets for the
                // two it holds)
            } else if weapon_item(t.name).is_some() && need > 0.0 && self.better_gun(obs, t.name) {
                why = " (a better gun)";
            } else if short && (weapon_item(t.name).is_some() || ammo_item(t.name).is_some()) {
                why = " (out of ammo)";
            } else if health < self.tune.errands.hurt && health_item(t.name).is_some_and(|(a, _)| a >= 10) {
                why = " (hurt)";
            } else if health < self.tune.errands.survive_health && health_item(t.name).is_some_and(|(a, _)| a >= 10) && self.combat.now.time_to_death < self.tune.errands.survive_secs {
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
            .filter(|d| d.why != Cause::Unarmed)
            // (by place: a level again spawns everything afresh, new ids; MAP12's clips by the
            // start, died going for 130 times)
            .filter(|d| d.kind == obj.kind && dist(d.goal, obj.goal) < self.tune.deaths.same_goal)
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
        let health = self.combat.now.health;
        let armor = obs.armor.unwrap_or(0.0);
        let short = self.combat.now.short_of_ammo;
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
            .filter(|s| s.awake && monster(s.name).is_some_and(|m| m.attack != Attack::None) && segment_point_distance(pos, obj.goal, s.pos()) < self.tune.errands.exposed_near)
            .collect();
        if threats.is_empty() {
            return false;
        }
        let supplies = ammo_item(t.name).is_some() || weapon_item(t.name).is_some();
        if !self.combat.armed(obs) && !supplies {
            return true;
        }
        let cost: f64 = threats.iter().map(|s| monster(s.name).map_or(0.0, |m| m.threat) * self.tune.errands.hits * exposed).sum();
        cost > worth && cost > self.tune.errands.cost_floor
    }
}

impl Hooks for Fight {
    /// Ammo or a gun she's off for because she's short of ammo.
    fn worth_cut_off(&mut self, obs: &Observation, obj: &Objective) -> bool {
        let Some(t) = obj.thing.and_then(|id| obs.things.iter().flatten().find(|t| t.id == id)) else { return false };
        obj.urgent && (ammo_item(t.name).is_some() || weapon_item(t.name).is_some()) && self.combat.now.short_of_ammo
    }

    /// Items she could use now; with --complete, every counted item she's seen too.
    fn wanted(&mut self, x: &Explorer, obs: &Observation, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let known = x.knowledge.known_polys();
        let gun_sectors: BTreeSet<usize> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| weapon_item(t.name).is_some() && self.better_gun(obs, t.name))
            .filter_map(|t| x.nav.poly_at((t.x, t.y)))
            .filter(|p| known.contains(p) && !reach.contains(p))
            .map(|p| x.nav.sector_of_poly(p))
            .collect();
        self.gun_out_of_reach = !gun_sectors.is_empty();
        let mesh = &x.nav.mesh;
        self.toward_gun = (0..mesh.polys.len())
            .filter(|&p| mesh.polys[p].as_ref().is_some_and(|q| gun_sectors.contains(&q.sector)) && !known.contains(&p))
            .map(|p| p as i64)
            .collect();
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
                    let Some(pi) = reach.iter().copied().filter(|&pi| dist(nav.poly_center(pi), s.pos()) < self.tune.errands.hunt_near).reduce(|a, b| if dist(nav.poly_center(b), s.pos()) < dist(nav.poly_center(a), s.pos()) { b } else { a }) else { continue };
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
            Kind::Hunt => obj.thing.is_none_or(|t| !self.combat.remaining().iter().any(|s| s.id == t)),
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
        if self.combat.now.invulnerable {
            return false; // (nothing can hurt her: the risky ones now)
        }
        // (died going for it: not first again; twice, not at all. And a place she's died at again
        // and again, whatever for: MAP04's corner, five lives for the shotgun, the chaingun,
        // a wall, the key there)
        let died = self.died_for(obj);
        let here = self.deaths.iter().filter(|d| d.why != Cause::Unarmed && dist(d.at, obj.goal) < self.tune.deaths.death_zone).count();
        if died >= self.tune.deaths.deaths_give_up || here >= self.tune.deaths.death_zone_urgent {
            return true;
        }
        if obj.urgent {
            return false;
        }
        if died > 0 || here >= self.tune.deaths.death_zone_count {
            return true;
        }
        // (and a hitscan shooter that'd see her there, however far: E3M8 from a pistol start, rocket
        // boxes picked up in the spider mastermind's sight, 0/16, the building with the plasma
        // rifle and the blue armor left shut)
        let known = self.combat.remaining();
        let near: Vec<&str> = known.iter().filter(|s| dist(s.pos(), obj.goal) < self.tune.errands.danger_near || self.combat.shot_from(obs, s, obj.goal)).map(|s| s.name).collect();
        if !near.is_empty() && self.combat.too_much(obs, &near, self.tune.engage.leave_be) {
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
            return self.tune.errands.switch_for_gun;
        }
        // (and a look into the unseen part of the ground it lies on: the likeliest way to it on
        // foot. E3M4 from a pistol start: the shotgun guy's dropped shotgun and a chaingun seen
        // on ground she hadn't found the way onto, 20 shells in hand; with only ways past
        // monsters left, the nearest taken, two cacodemons over a hurting floor with the pistol,
        // the ammo gone, bitten to death)
        if obj.kind == Kind::Frontier && !obj.reveals.is_disjoint(&self.toward_gun) {
            return self.tune.errands.switch_for_gun;
        }
        let ammo = obj.kind == Kind::Item && obj.thing.and_then(|id| obs.things.iter().flatten().find(|t| t.id == id)).is_some_and(|t| ammo_item(t.name).is_some());
        if !ammo || self.combat.now.short_of_ammo {
            return 1.0;
        }
        let known: Vec<&'static str> = self.combat.remaining().iter().filter(|s| s.awake).map(|s| s.name).collect();
        if known.iter().any(|n| self.combat.too_much(obs, &[n], self.tune.engage.leave_be)) {
            self.tune.errands.ammo_outmatched
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
