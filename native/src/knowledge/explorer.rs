//! Playing a level on what Evil knows: the objectives open to her now, and an explorer that
//! picks one, plans a way there through the known map, and follows it.
//!
//! Objectives: the exit, once she has seen it and can get to it; keys she has seen; switches
//! and doors she has seen and not tried (what they do is learned by trying); the edge of the
//! known map (places she can get to next to ones she hasn't seen); and, with nothing else,
//! walls that might hide something.

mod execution;
mod objectives;
mod positions;
mod routing;
mod selection;
mod world;

mod viewpoints;

use super::model::{Heard, Knowledge, Note};
use crate::geom::{dist, Point};
use crate::motor::follower::RouteFollower;
use crate::motor::{Controls, Observation};
use crate::navigator::{Navigator, States, TriggerLine, View};
use crate::specials::{classify_line, Key, Lock};
use crate::walkthrough::Step;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// the most plan steps a route is searched for in
const PLAN_EXPANSIONS: usize = 20000;

/// an objective not done by then is dropped for a while...
const GIVE_UP_TICS: i64 = 20 * 35;

/// a route failing stuck in the same poly this often: not through there again
pub const STUCK_TIMES: i32 = 2;

/// a run-jump missed this often isn't tried again; missed once, only when there's nothing else
pub const JUMP_MISSES: i32 = 2;

/// with nothing to do, look for something again this often
const IDLE_RETRY: i64 = 35;

/// a turn on the spot, at the edge of the known map
const LOOK_TICS: i64 = 35;

/// how far from a pickup spot she may stop (the follower's PICKUP_REACHED)
const ARRIVE_SLACK: f64 = 6.0;
/// degrees a tic, turning to look at something
const TURN_RATE: f64 = 20.0;

/// (an objective to be had only while something she set off is open)
const WHILE_OPEN: &str = " (while it's open)";

/// an objective mustn't leave less than this share reachable
const KEEP_REACH: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Exit,
    Key,
    Switch,
    Door,
    Frontier,
    Room,
    Visit,
    Noise,
    Teleporter,
    Recheck,
    Item,
    Hunt,
    Wall,
    /// a sector she found a secret in, another life: back into it
    Secret,
    /// a barrel she's seen: shot from out of its chain's blasts, before a shot (hers or a
    /// monster's) sets it off by her (the user's)
    Barrel,
}

impl Kind {
    /// how much nearer an objective of each kind counts as (keys, switches first; looking again
    /// last)
    fn weight(self) -> f64 {
        match self {
            Kind::Exit => 0.5,
            Kind::Key | Kind::Item | Kind::Secret => 0.6,
            Kind::Switch | Kind::Door => 0.8,
            Kind::Frontier | Kind::Wall => 1.0,
            Kind::Room => 1.1,
            Kind::Visit => 2.0,
            Kind::Noise => 0.7,
            Kind::Teleporter => 1.2,
            Kind::Recheck => 3.0,
            Kind::Hunt => 1.5,
            Kind::Barrel => 0.7,
        }
    }
}

/// "to the NE" and so on.
fn compass(bearing: f64) -> &'static str {
    const NAMES: [&str; 8] = ["to the E", "to the NE", "to the N", "to the NW", "to the W", "to the SW", "to the S", "to the SE"];
    NAMES[((bearing / 45.0).round_ties_even() as i64).rem_euclid(8) as usize]
}

/// Something to go and do.
#[derive(Clone, Debug)]
pub struct Objective {
    pub kind: Kind,
    pub goal: Point,
    pub label: String,
    pub trigger: Option<TriggerLine>,
    pub key: Option<Key>,
    /// frontier: the unseen polys it looks into (rooms: lines; rechecks: sectors; noises: tics)
    pub reveals: BTreeSet<i64>,
    /// noise: the sound
    pub heard: Option<Heard>,
    /// room, wall: the wall to face once there
    pub look_at: Option<Point>,
    /// wall: the line to press use on, and how suspicious it is
    pub wall: Option<usize>,
    pub suspicion: f64,
    /// a wall tried in a round of them (nothing else to do), not one by the way: gone right up
    /// to if it stands out
    pub round: bool,
    /// nothing safer left: one-way drops, nukage are all right
    pub risky: bool,
    /// teleporter: all the lines of its pad (one objective)
    pub pad: BTreeSet<usize>,
    /// item, hunt: the thing's id (the engine's)
    pub thing: Option<i32>,
    /// noise: just after she set something off; go now
    pub urgent: bool,
    /// urgent only by a route no longer than this (further: in its turn)
    pub worth: Option<f64>,
    /// it would cut her off from where other objectives are, as far as she knows: those first
    pub later: bool,
}

/// An objective's identity, for dropping it a while.
pub type Ident = (Kind, Option<i64>, BTreeSet<usize>, Option<Key>, BTreeSet<i64>, bool, Option<i32>);

impl Objective {
    pub fn new(kind: Kind, goal: Point, label: impl Into<String>) -> Objective {
        Objective {
            kind,
            goal,
            label: label.into(),
            trigger: None,
            key: None,
            reveals: BTreeSet::new(),
            heard: None,
            look_at: None,
            wall: None,
            risky: false,
            pad: BTreeSet::new(),
            thing: None,
            urgent: false,
            worth: None,
            later: false,
            suspicion: 0.0,
            round: false,
        }
    }

    pub fn ident(&self) -> Ident {
        // (a wall by its line: not every wall given up on with the one that had no way there,
        // the walls of the floor she stood on with the far ones out of reach. E4M6 UV)
        let line = if self.pad.is_empty() { self.trigger.as_ref().map(|t| t.line).or(self.wall.map(|w| w as i64)) } else { None };
        (self.kind, line, self.pad.clone(), self.key, self.reveals.clone(), self.risky, self.thing)
    }

    /// Taken up before anything nearer of a lower one: the exit, then the doors and switches a
    /// key she has opens (what the key was got for), and what can't wait; last, what would cut
    /// her off from other things to do.
    pub fn priority(&self) -> i32 {
        if self.kind == Kind::Exit {
            return 0;
        }
        if self.later && !self.urgent {
            return 3; // (first what it would cut her off from; but not what's to be had only now)
        }
        if matches!(self.kind, Kind::Door | Kind::Switch) && self.trigger.as_ref().is_some_and(|t| t.info.lock != Lock::None) {
            return 1;
        }
        if self.urgent {
            return 1;
        }
        2
    }
}

/// Other things to do, from elsewhere (the fight: what to pick up, monsters to hunt down; the
/// 100% runs: items, secrets).
pub trait Hooks {
    /// offered with the rest
    fn wanted(&mut self, _x: &Explorer, _obs: &Observation, _reach: &BTreeSet<usize>, _pos: Point) -> Vec<Objective> {
        Vec::new()
    }
    /// only once there's nothing else
    fn last_resort(&mut self, _x: &Explorer, _obs: &Observation, _reach: &BTreeSet<usize>, _pos: Point) -> Vec<Objective> {
        Vec::new()
    }
    /// it's no longer there to be done
    fn gone(&mut self, _obs: &Observation, _obj: &Objective) -> bool {
        false
    }
    /// too dangerous to go for unless nothing else
    fn dangerous(&mut self, _obs: &Observation, _obj: &Objective) -> bool {
        false
    }
    /// worth going for even where she knows no way back from (as the exit, a key)
    fn worth_cut_off(&mut self, _obs: &Observation, _obj: &Objective) -> bool {
        false
    }
    /// how much further than it is it counts as (1: as far as it is)
    fn weigh(&mut self, _obs: &Observation, _obj: &Objective) -> f64 {
        1.0
    }
    /// something better to do turned up: choose again
    fn interrupt(&mut self, _x: &Explorer, _obs: &Observation, _obj: &Objective) -> bool {
        false
    }
    /// rather the level again than out (asks restart_wanted)
    fn before_exit(&mut self, _x: &Explorer, _obs: &Observation) -> bool {
        false
    }
}

/// No hooks: plain exploring.
pub struct NoHooks;

impl Hooks for NoHooks {}

/// Which tier of last resorts objectives are offered from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Suspecting {
    No,
    Walls,
    Panic,
}

/// Points bucketed by a grid, for "is there one near here".
struct Spots(BTreeMap<(i64, i64), Vec<Point>>);

impl Spots {
    const CELL: f64 = 128.0;

    fn new(points: impl Iterator<Item = Point>) -> Spots {
        let mut cells: BTreeMap<(i64, i64), Vec<Point>> = BTreeMap::new();
        for p in points {
            cells.entry(((p.0 / Self::CELL).floor() as i64, (p.1 / Self::CELL).floor() as i64)).or_default().push(p);
        }
        Spots(cells)
    }

    fn near(&self, q: Point, r: f64) -> bool {
        let (cx, cy) = ((q.0 / Self::CELL).floor() as i64, (q.1 / Self::CELL).floor() as i64);
        let reach = (r / Self::CELL).floor() as i64 + 1;
        (-reach..=reach).any(|dx| (-reach..=reach).any(|dy| self.0.get(&(cx + dx, cy + dy)).is_some_and(|ps| ps.iter().any(|&p| dist(p, q) < r))))
    }
}

/// How an objective she was given ended (directed play: reported to whoever gave it).
#[derive(Clone, Debug)]
pub struct Ended {
    pub label: String,
    /// done (got there, did it, or it came to nothing to do: seen on the way)
    pub ok: bool,
    pub why: String,
    /// a switch or door pressed with nothing seen to go off (pressed all the same)
    pub pressed: bool,
}

/// What an objective's ending means for what's next, besides its words for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndKind {
    Plain,
    /// pressed, and nothing seen to go off
    PressedNothing,
}

pub struct Explorer {
    pub nav: Rc<Navigator>,
    pub knowledge: Knowledge,
    pub current: Option<Objective>,
    pub follower: Option<RouteFollower>,
    /// directed play: she doesn't choose objectives herself; she's given them (`take`), and when
    /// one ends, how is kept in `ended` for whoever gave it, and she waits for the next
    pub directed: bool,
    /// (directed) the last objective's end, till it's taken
    pub ended: Option<Ended>,
    /// why the objective in hand ended, as `ends` or `drop_objective` had it
    /// the tic her senses last took in (`sense`)
    sensed: Option<i64>,
    end_why: Option<(bool, String)>,
    /// ...and what kind of ending it was
    end_kind: EndKind,
    pub log: Vec<String>,
    /// what she decided and on what, if a run's asked for it (`crate::decisions`)
    pub journal: crate::decisions::Journal,
    /// the routes planned so far, and the time they took (the live status: `EVIL_STATUS`)
    pub stats: RouteStats,
    /// the goals the planner found no way to, and the plan signature then
    no_route: BTreeMap<GoalKey, (u64, i64)>,
    /// what was given up on for want of a route (the health margin's refusals among them) this
    /// think, and its tic: the dead-end pass's to take up again, not what was dropped for being
    /// gone or done
    refused_now: (i64, BTreeSet<Ident>),
    /// the walls she suspected last, and a hash of all that went into it (see `suspected`)
    suspects: Option<(u64, Vec<Suspect>)>,
    /// the same for a search for secrets asked for (`secret_suspects`), by its own score
    secret_suspects: Option<(u64, Vec<Suspect>)>,
    /// what standable() found, at the heights then (see `standable_at`)
    standable_cache: std::cell::RefCell<StandableCache>,
    /// the most plan steps a route is searched for in (`drive budget`)
    pub plan_expansions: usize,
    /// times the budget's been halved, and the tic a search last used it up
    budget_cuts: u32,
    maxed_tic: i64,
    dropped: BTreeMap<Ident, i64>,
    /// ...and how often (each time it waits twice as long to be offered again)
    drop_count: BTreeMap<Ident, i32>,
    /// walls given up on, by line, how often: the walls share one wait (that keeps the wall
    /// tier paced), but which goes next counts each one's own failures (MAP14)
    wall_drops: BTreeMap<usize, i32>,
    since: i64,
    exits_seen: usize,
    allowed: i64,
    look_until: Option<i64>,
    glance_until: Option<i64>,
    /// fired lines looked round for so far
    glanced: usize,
    /// sounds dealt with so far
    heard_n: usize,
    last_glance: i64,
    sector: Option<usize>,
    /// how much of the follower's log has been passed on
    follower_seen: usize,
    glance_at: Option<f64>,
    idle_since: i64,
    /// the exit only once there's nothing else to do (the 100% runs)
    pub exit_last: bool,
    /// her waits to give something given up on another go, as a share of the usual (her run's
    /// stall clock over 300 s: her own waits inside it, the user's)
    pub patience: f64,
    /// (for whoever runs the game: the level again, knowing what she knows)
    pub restart_wanted: bool,
    /// (mortal runs) when a route last cost more health than she had to spare (floors that hurt:
    /// healing or a suit would do)
    short_of_health: Option<i64>,
    /// per crusher sector: tics to get out of it at the worst (crush::out_tics)
    crusher_out: BTreeMap<usize, f64>,
    /// a suit on, last she looked (what cost too much then gets another go once it's on)
    suited: bool,
    /// the secrets run: every secret, then out
    pub secrets_mode: bool,
    /// sectors she's found a secret in (the tally went up as she stepped in: "A secret is
    /// revealed!"), any life; and those this life (the level again, the tally again from 0)
    secret_sectors: BTreeSet<usize>,
    secrets_had: BTreeSet<usize>,
    /// switches set out to press, and what she knew then (pressed again only knowing more)
    pressed_knowing: BTreeMap<usize, String>,
    /// where she last failed stuck, and how many objectives in a row did there
    stuck_run: Option<(Point, usize)>,
    /// what she knew when last stuck (no news since: still the same trap)
    stuck_known: String,
    held_since: Option<i64>,
    urgent_n: usize,
    /// polys she could get to, as of the last look
    pub reach: BTreeSet<usize>,
    /// the heights the triggers she knows could give, with that reach
    reach_states: States,
    view: Option<(Rc<View>, ViewKey)>,
    /// the jumps missed once are in, for want of anything else
    difficult: bool,
    exits_open: bool,
    /// polys beyond walk-over exit lines, and those lines
    pads: (BTreeSet<usize>, Vec<(Point, Point)>),
    /// where she could get to, with nothing at all to do
    trapped: Option<BTreeSet<usize>>,
    /// things whoever directs her wants picked up whenever it's safe (directed play's own
    /// pickups: bonuses, health that won't go to waste, ammo with room for it; the user's): offered
    /// as items, past the same checks as any (danger, cut off)
    pub spares: BTreeSet<i32>,
    /// when she first had nothing else to do but try walls (the survey's moment: everything
    /// explored but for what's hidden)
    pub out_of_options: Option<i64>,
    /// directed: the objective in hand pressed on with mid-fight, shooting and dodging on the
    /// way, not stopped for the fight (Evil's rush, the user's: MAP07's arena, a switch never got
    /// to for the mancubi chased)
    pub press_on: bool,
    suspecting: Suspecting,
    /// walls pressed round about since there was last anything else to do (the 100% runs)
    panic_tries: usize,
    /// (that round done: only the walls with more to go on)
    strong_only: bool,
    /// waiting for a door that opens again by itself before anything else (logged once)
    waiting_reopen: bool,
    /// strafing run-jumps allowed (nothing else to do)
    strafe_ok: bool,
    /// switches pressed from a floor that hurts allowed (nothing else at all to do)
    harm_ok: bool,
    /// her health when she stepped onto a floor that hurts for a risky objective there (on
    /// purpose, not fallen in), while she's still down on it
    waded: Option<f64>,
    /// the objective she's on was taken up with nothing else at all to do (`harm_ok`)
    from_dead_end: bool,
    /// she waded in from a dead end (`from_dead_end`), while she's still down on it: the floor
    /// she left is where there was nothing to do
    waded_dead_end: bool,
    /// when she last shot at a wall she was trying (single taps)
    wall_shot: i64,
    /// what she knew when what was given up on last had its other go: another once she knows
    /// more (not merely when one of them is taken up again: E1M3 went round two for ever)
    retried: Option<String>,
    /// (for tracing: the tics, from EVIL_DEBUG_OBJECTIVES=from-to, whose choices are logged in full)
    debug_tics: Option<(i64, i64)>,
}

/// The routes planned (found and not) and the seconds they took, all told.
#[derive(Clone, Copy, Default, Debug)]
pub struct RouteStats {
    pub routed: u64,
    pub routed_secs: f64,
    pub failed: u64,
    pub failed_secs: f64,
    /// no route, known already (nothing changed since)
    pub known_none: u64,
    /// the lists of what she could do made, the seconds they took, and of those in saturate
    pub listed: u64,
    pub listed_secs: f64,
    pub saturate_secs: f64,
}

/// A wall she suspects: (score, line, where to stand, the wall's middle).
type Suspect = (f64, usize, Point, Point);

/// standable()'s answers at a set of heights: the heights, and by (x, y, play) bits.
#[derive(Default)]
struct StandableCache {
    heights: Vec<(f64, f64)>,
    answers: crate::det::HashMap<(u64, u64, bool), bool>,
}

/// A goal planned for: where, the line to press there, and whether other spots to press it
/// from are tried.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GoalKey {
    x_bits: u64,
    y_bits: u64,
    trigger_line: Option<i64>,
    try_other_activators: bool,
}

/// What the view's built from: rebuilt when it changes.
#[derive(PartialEq, Eq)]
struct ViewKey {
    sectors: usize,
    seen_polys: usize,
    visited: usize,
    learned: usize,
    teleports: usize,
    spent_doors: BTreeSet<usize>,
    spent_teleports: BTreeSet<usize>,
    exits_open: bool,
    blocked: BTreeSet<usize>,
    difficult: bool,
}

impl Explorer {
    pub fn new(nav: Rc<Navigator>, knowledge: Knowledge) -> Explorer {
        Explorer {
            pads: exit_pads(&nav),
            nav,
            knowledge,
            current: None,
            follower: None,
            directed: false,
            ended: None,
            sensed: None,
            end_why: None,
            end_kind: EndKind::Plain,
            log: Vec::new(),
            journal: crate::decisions::Journal::default(),
            stats: RouteStats::default(),
            no_route: BTreeMap::new(),
            refused_now: (i64::MIN, BTreeSet::new()),
            standable_cache: std::cell::RefCell::new(StandableCache::default()),
            suspects: None,
            secret_suspects: None,
            plan_expansions: PLAN_EXPANSIONS,
            budget_cuts: 0,
            maxed_tic: i64::MIN / 2,
            dropped: BTreeMap::new(),
            drop_count: BTreeMap::new(),
            wall_drops: BTreeMap::new(),
            since: 0,
            exits_seen: 0,
            allowed: GIVE_UP_TICS,
            look_until: None,
            glance_until: None,
            glanced: 0,
            heard_n: 0,
            last_glance: -1_000_000_000,
            sector: None,
            follower_seen: 0,
            glance_at: None,
            idle_since: -1_000_000_000,
            exit_last: false,
            patience: 1.0,
            restart_wanted: false,
            short_of_health: None,
            crusher_out: BTreeMap::new(),
            suited: false,
            pressed_knowing: BTreeMap::new(),
            secrets_mode: false,
            secret_sectors: BTreeSet::new(),
            secrets_had: BTreeSet::new(),
            stuck_run: None,
            stuck_known: String::new(),
            held_since: None,
            urgent_n: 0,
            reach: BTreeSet::new(),
            reach_states: Vec::new(),
            view: None,
            difficult: false,
            exits_open: true,
            trapped: None,
            suspecting: Suspecting::No,
            spares: BTreeSet::new(),
            out_of_options: None,
            press_on: false,
            panic_tries: 0,
            strong_only: false,
            waiting_reopen: false,
            strafe_ok: false,
            harm_ok: false,
            from_dead_end: false,
            waded_dead_end: false,
            waded: None,
            wall_shot: -1_000_000,
            retried: None,
            debug_tics: debug_tics(),
        }
    }

    fn now(&self) -> i64 {
        self.knowledge.last.as_ref().map_or(0, |l| l.tic)
    }

    // --- playing ------------------------------------------------------------------------------

    /// Died: the level from the start again, keeping what's known of it. Stuck with nothing at
    /// all to do before it: that place is a trap, kept in mind.
    pub fn restart(&mut self) {
        if let Some(t) = self.trapped.take() {
            self.log.push(format!("--- that was a trap ({} polys): not going back in", t.len()));
            self.knowledge.note(Note::Trap(t));
        }
        self.sensed = None;
        self.knowledge.restart();
        self.log.push(format!("--- died: life {}", self.knowledge.lives));
        self.current = None;
        self.follower = None;
        self.dropped.clear();
        self.drop_count.clear();
        self.secrets_had.clear();
        self.look_until = None;
        self.glance_until = None;
        self.heard_n = self.knowledge.heard.len();
        self.glanced = self.knowledge.fired.len();
        self.sector = None;
        self.view = None;
        self.held_since = None;
        self.urgent_n = self.knowledge.heard.len();
        self.panic_tries = 0;
        self.retried = None;
    }

    /// Taking over a run played back to here (replay): what she's heard and set off so far is
    /// old news, nothing's being done.
    pub fn resume(&mut self, tic: i64) {
        self.heard_n = self.knowledge.heard.len();
        self.glanced = self.knowledge.fired.len();
        self.urgent_n = self.knowledge.heard.len();
        self.current = None;
        self.follower = None;
        self.since = tic;
        self.log.push(format!("--- played back to tic {tic}: from here, the bot as it is now"));
    }

    /// Something else has the controls this tic (a fight): take in what's seen, and carry on
    /// from here afterwards.
    /// Her eyes and ears on the game as it is this tic (once a tic: `hold` and `act` do it
    /// themselves unless it's been done). The player does it first, with the game's own
    /// observation, before making what she perceives of it for everything that decides.
    pub fn sense(&mut self, obs: &Observation, last: Option<&Controls>) {
        if self.sensed != Some(obs.tic) {
            self.sensed = Some(obs.tic);
            self.knowledge.update(obs, last);
        }
    }

    pub fn hold(&mut self, obs: &Observation, last: Option<&Controls>) {
        self.sense(obs, last);
        if self.held_since.is_none() {
            self.held_since = Some(obs.tic);
        }
    }

    pub fn act(&mut self, obs: &Observation, last: Option<&Controls>, hooks: &mut dyn Hooks) -> Controls {
        if let Some(h) = self.held_since.take() {
            self.since += obs.tic - h;
            if let Some(f) = &mut self.follower {
                f.resume(obs.tic);
            }
        }
        self.sense(obs, last);
        if let Some(s) = self.nav.poly_at(obs.pos()).map(|p| self.nav.sector_of_poly(p)) {
            if self.nav.damage[s] == 0 {
                self.waded = None;
                self.waded_dead_end = false;
            }
            if Some(s) != self.sector {
                let meant = self.current.as_ref().is_some_and(|o| o.risky && self.nav.poly_at(o.goal).is_some_and(|p| self.knowledge.damaging(self.nav.sector_of_poly(p))));
                if meant && self.waded.is_none() && self.knowledge.damaging(s) {
                    self.waded = obs.health;
                    self.waded_dead_end = self.from_dead_end;
                }
                if self.nav.damage[s] != 0 {
                    let doing = self.follower.as_ref().map_or("-".to_string(), RouteFollower::objective);
                    self.log.push(format!(
                        "tic {}: onto damaging floor (sector {s}) from sector {}, doing: {doing}",
                        obs.tic,
                        self.sector.map_or("None".to_string(), |x| x.to_string())
                    ));
                }
                self.sector = Some(s);
            }
        }
        // (no looking round in the middle of a jump: MAP08's; nor with any of her under something
        // she hears moving: the route's crusher rules don't run while she looks, E3M4's ring of a
        // slow crusher, stood still in it half a second to see what a line had done as it came
        // down, pinned)
        let busy = self.follower.as_ref().is_some_and(RouteFollower::in_jump);
        let r = crate::navigator::PLAYER_RADIUS + 2.0;
        let under_mover = [(0.0, 0.0), (-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .filter_map(|(dx, dy)| self.nav.poly_at((obs.x + dx, obs.y + dy)))
            .any(|p| self.knowledge.is_moving(self.nav.sector_of_poly(p)));
        if !busy && !under_mover {
            if let Some(g) = self.glance(obs) {
                return g;
            }
        }
        if self.current.is_some() && self.follower.as_ref().is_some_and(|f| f.done) {
            if let Some(look) = self.arrived(obs, hooks) {
                return look;
            }
        }
        if self.directed {
            // (given objectives only: when the one in hand ends, how, and then nothing till the
            // next is given; standing still where she is, facing as she is: not turning round and
            // round, the user's)
            if (self.current.is_some() || self.follower.is_some()) && self.need_new(obs, hooks) {
                self.note_end(obs);
            }
            if self.current.is_none() {
                let (forward, strafe) = crate::motor::brake(obs, obs.angle);
                return Controls { forward, strafe, ..Controls::default() };
            }
        } else if self.need_new(obs, hooks) && (self.current.is_some() || self.follower.is_some() || obs.tic - self.idle_since >= IDLE_RETRY) {
            self.next(obs, hooks);
            if self.current.is_none() {
                self.idle_since = obs.tic;
            }
        }
        let Some(f) = &mut self.follower else {
            return Controls { turn: 10.0, ..Controls::default() }; // (nothing to do: look round)
        };
        let out = f.act(obs);
        // (what the follower had to say: jumps, waits, trouble)
        for ev in &f.log.events[self.follower_seen.min(f.log.events.len())..] {
            self.log.push(format!("    {ev}"));
        }
        self.follower_seen = f.log.events.len();
        out
    }
}

fn debug_tics() -> Option<(i64, i64)> {
    let v = std::env::var("EVIL_DEBUG_OBJECTIVES").ok()?;
    let (a, b) = v.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

impl Explorer {
    /// (directed) The objective in hand is over: how, kept for whoever gave it; nothing in hand.
    fn note_end(&mut self, obs: &Observation) {
        let Some(obj) = self.current.take() else {
            self.follower = None;
            return;
        };
        let pressed = self.end_why.is_some() && self.end_kind == EndKind::PressedNothing;
        let (ok, why) = match (&self.follower, self.end_why.take()) {
            (_, Some(e)) => e,
            // (got there and done with it: arrived() lets the route go)
            (None, None) => (true, "done".to_string()),
            (Some(f), None) if f.done && !f.failed => (true, "done".to_string()),
            _ => {
                // (not over: a plan anew wanted (a jump missed); the same again, if there's a way)
                self.follower = None;
                if self.take(obs, obj.clone()) {
                    return;
                }
                (false, "no way there now".to_string())
            }
        };
        self.log.push(format!("tic {}: '{}' over: {}", obs.tic, obj.label, why));
        self.ended = Some(Ended { label: obj.label, ok, why, pressed });
        self.follower = None;
    }
}

/// Turn towards `bearing` (or all the way round, for None).
fn turn_to(obs: &Observation, bearing: Option<f64>) -> Controls {
    let turn = match bearing {
        None => 360.0 / LOOK_TICS as f64,
        Some(b) => {
            let diff = crate::motor::norm(b - obs.angle); // (positive: anticlockwise, to the left)
            (-diff).clamp(-TURN_RATE, TURN_RATE)
        }
    };
    // (standing still while she turns: no sliding on with what momentum she had)
    let (forward, strafe) = crate::motor::brake(obs, obs.angle - turn);
    Controls { turn, forward, strafe, ..Controls::default() }
}

pub fn route_length(steps: &[Step]) -> f64 {
    steps.iter().filter_map(|st| st.path.as_ref()).flat_map(|p| p.waypoints.windows(2).filter_map(|w| Some(dist(w[0]?, w[1]?)))).sum()
}

/// The polys beyond walk-over exit lines (of the two sides of each, the sector more of whose
/// edge is exit lines: MAP24's pad), and those lines.
fn exit_pads(nav: &Navigator) -> (BTreeSet<usize>, Vec<(Point, Point)>) {
    let m = &nav.map;
    let exit_lines: Vec<usize> =
        (0..m.linedefs.len()).filter(|&li| classify_line(m.linedefs[li].special).is_some_and(|i| i.category.is_exit() && i.trigger.walked())).collect();
    let length = |li: usize| {
        let (a, b) = m.line_seg(li);
        dist(a, b)
    };
    let share = |sec: usize| -> f64 {
        let edge: Vec<usize> = (0..m.linedefs.len())
            .filter(|&li| {
                let (f, b) = m.sides(li);
                f == Some(sec) || b == Some(sec)
            })
            .collect();
        let total: f64 = edge.iter().map(|&li| length(li)).sum();
        let total = if total == 0.0 { 1.0 } else { total };
        edge.iter().filter(|li| exit_lines.contains(li)).map(|&li| length(li)).sum::<f64>() / total
    };
    let mut pads: BTreeSet<usize> = BTreeSet::new();
    for &li in &exit_lines {
        let (f, b) = m.sides(li);
        let sides: Vec<usize> = [f, b].into_iter().flatten().collect();
        if let Some(best) = sides.iter().copied().reduce(|x, y| if share(y) > share(x) { y } else { x }) {
            pads.insert(best);
        }
    }
    let polys = nav.mesh.live_polys().filter(|(_, p)| pads.contains(&p.sector)).map(|(i, _)| i).collect();
    let segs = exit_lines.iter().map(|&li| m.line_seg(li)).collect();
    (polys, segs)
}
