//! Playing a level on what Evil knows: the objectives open to her now, and an explorer that
//! picks one, plans a way there through the known map, and follows it.
//!
//! Objectives: the exit, once she has seen it and can get to it; keys she has seen; switches
//! and doors she has seen and not tried (what they do is learned by trying); the edge of the
//! known map (places she can get to next to ones she hasn't seen); and, with nothing else,
//! walls that might hide something.

mod viewpoints;

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;
use std::sync::Arc;

use super::model::{How, Knowledge, Heard, Note, SUSPICION_MIN};
use crate::movers::cycles_back;
use crate::geom::{dist, segment_distance, segments_cross, Point};
use crate::motor::follower::RouteFollower;
use crate::motor::{Controls, Observation};
use crate::nav::{can_cross, Heights};
use crate::navigator::{add_height, Navigator, Plan, Route, States, TeleEdge, TriggerLine, View, MAX_STEP, PLAYER_HEIGHT};
use crate::specials::{classify_line, Category, Key, Lock, Trigger};
use crate::walkthrough::{action_for, steps_from_legs, Action, Step};

/// a backstop only: the expansions decide (a clock limit made a run depend on the machine)
const PLAN_SECONDS: f64 = 600.0;
/// the most plan steps a route is searched for in
const PLAN_EXPANSIONS: usize = 2000;
/// a suspected wall pressed: watched this long for something to happen
const WALL_TICS: i64 = 25;
/// objectives routed and compared by the way there, each pick
const COMPARED: usize = 4;
/// an objective not done by then is dropped for a while...
const GIVE_UP_TICS: i64 = 20 * 35;
/// ...or by the time it takes to walk its route at this (units a tic)
const ROUTE_SPEED: f64 = 8.0;
/// ...and this
const ROUTE_SLACK: i64 = 15 * 35;
/// a threshold's spot: this far over the line
const THRESHOLD_IN: f64 = 8.0;
/// the expert's run: no goal this near a walk-over exit line (she'd slide over)
const EXIT_MARGIN: f64 = 64.0;
/// a route failing stuck in the same poly this often: not through there again
pub const STUCK_TIMES: i32 = 2;
/// a wall suspected this much is tried when she's this near where she'd stand to (not only when
/// there's nothing else to do)
const NEAR_SUSPECT: f64 = 6.0;
const NEAR_WALL: f64 = 128.0;
/// suspected walls offered at a time (the most suspicious)
const SUSPECTS: usize = 6;
/// ...and in a panic, the nearest walls of all
const PANIC_WALLS: usize = 12;
/// a run-jump missed this often isn't tried again; missed once, only when there's nothing else
pub const JUMP_MISSES: i32 = 2;
/// a repeatable switch whose effect wasn't seen: another go after this
const REPRESS_AFTER: i64 = 60 * 35;
/// with nothing to do, look for something again this often
const IDLE_RETRY: i64 = 35;
/// ...and offered again after this
const RETRY_TICS: i64 = 30 * 35;
/// a turn on the spot, at the edge of the known map
const LOOK_TICS: i64 = 35;
/// an item still there on arrival: shuffled over this long
const SHUFFLE_TICS: i64 = 35;
/// a pickup's box and hers overlap this near (20 + 16), either way
const TOUCH: i32 = 36;
/// how far from a pickup spot she may stop (the follower's PICKUP_REACHED)
const ARRIVE_SLACK: f64 = 6.0;
/// an item this far under her feet is still hers to pick up
const PICKUP_BELOW: f64 = 8.0;
/// degrees a tic, turning to look at something
const TURN_RATE: f64 = 20.0;
/// sounds from about the same place...
const AMBIENT_RADIUS: f64 = 96.0;
/// ...heard more often than this are background
const AMBIENT_REPEATS: usize = 4;
/// a sound is worth going to look into for this long
const NOISE_TICS: i64 = 60 * 35;
/// a sound this soon after she set off a line she doesn't know...
const CAUSE_TICS: i64 = 35;
/// ...is gone to look at first, for this long (a timed door shuts)
const URGENT_TICS: i64 = 6 * 35;
/// how fast she's taken to get places while a door she opened holds (units a tic: below a run)
const OPEN_SPEED: f64 = 10.0;
/// (an objective to be had only while something she set off is open)
const WHILE_OPEN: &str = " (while it's open)";
/// (an objective that would cut her off from others: after them)
const LATER: &str = " (after what it cuts off)";
/// at most one look round this often (stairs build in many steps)
const GLANCE_GAP: i64 = 5 * 35;
/// tics after a walk-over line goes off unseen, before looking
const GLANCE_AFTER: i64 = 8;
/// a noise is worth walking towards if it gets her this much nearer
const HEAR_NEARER: f64 = 128.0;
/// a frontier's look-out: this far back from the way into the unseen
const FRONTIER_BACK: f64 = 24.0;
/// a room's walked once nowhere in it is further than this from her steps
const COVER_GAP: f64 = 96.0;
/// how far over a hurting floor a route may go, normally...
const WADE_LIMIT: f64 = 256.0;
/// ...and not at all below this health
const WADE_HEALTH: f64 = 50.0;
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
}

impl Kind {
    /// how much nearer an objective of each kind counts as (keys, switches first; looking again
    /// last)
    fn weight(self) -> f64 {
        match self {
            Kind::Exit => 0.5,
            Kind::Key | Kind::Item => 0.6,
            Kind::Switch | Kind::Door => 0.8,
            Kind::Frontier | Kind::Wall => 1.0,
            Kind::Room => 1.1,
            Kind::Visit => 2.0,
            Kind::Noise => 0.7,
            Kind::Teleporter => 1.2,
            Kind::Recheck => 3.0,
            Kind::Hunt => 1.5,
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
        }
    }

    pub fn ident(&self) -> Ident {
        let line = if self.pad.is_empty() { self.trigger.as_ref().map(|t| t.line) } else { None };
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

pub struct Explorer {
    pub nav: Rc<Navigator>,
    pub knowledge: Knowledge,
    pub current: Option<Objective>,
    pub follower: Option<RouteFollower>,
    pub log: Vec<String>,
    /// the routes planned so far, and the time they took (the live status: `EVIL_STATUS`)
    pub stats: RouteStats,
    /// the goals the planner found no way to, and the plan signature then
    no_route: BTreeMap<GoalKey, u64>,
    /// the walls she suspected last, and a hash of all that went into it (see `suspected`)
    suspects: Option<(u64, Vec<Suspect>)>,
    /// what standable() found, at the heights then (see `standable_at`)
    standable_cache: std::cell::RefCell<StandableCache>,
    /// the most plan steps a route is searched for in (`drive budget`)
    pub plan_expansions: usize,
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
    /// (for whoever runs the game: the level again, knowing what she knows)
    pub restart_wanted: bool,
    held_since: Option<i64>,
    urgent_n: usize,
    /// polys she could get to, as of the last look
    pub reach: BTreeSet<usize>,
    view: Option<(Rc<View>, ViewKey)>,
    /// the jumps missed once are in, for want of anything else
    difficult: bool,
    exits_open: bool,
    /// polys beyond walk-over exit lines, and those lines
    pads: (BTreeSet<usize>, Vec<(Point, Point)>),
    /// where she could get to, with nothing at all to do
    trapped: Option<BTreeSet<usize>>,
    suspecting: Suspecting,
    /// walls pressed round about since there was last anything else to do (the 100% runs)
    panic_tries: usize,
    /// (that round done: only the walls with more to go on)
    strong_only: bool,
    /// strafing run-jumps allowed (nothing else to do)
    strafe_ok: bool,
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
type StandableCache = (Vec<(f64, f64)>, crate::det::HashMap<(u64, u64, bool), bool>);

/// A goal planned for: where, the line to press there, and whether other spots to press it
/// from are tried.
type GoalKey = (u64, u64, Option<i64>, bool);

/// What the view's built from: rebuilt when it changes.
type ViewKey = (usize, usize, usize, usize, usize, BTreeSet<usize>, BTreeSet<usize>, bool, BTreeSet<usize>, bool);

impl Explorer {
    pub fn new(nav: Rc<Navigator>, knowledge: Knowledge) -> Explorer {
        Explorer {
            pads: exit_pads(&nav),
            nav,
            knowledge,
            current: None,
            follower: None,
            log: Vec::new(),
            stats: RouteStats::default(),
            no_route: BTreeMap::new(),
            standable_cache: std::cell::RefCell::new((Vec::new(), crate::det::HashMap::default())),
            suspects: None,
            plan_expansions: PLAN_EXPANSIONS,
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
            restart_wanted: false,
            held_since: None,
            urgent_n: 0,
            reach: BTreeSet::new(),
            view: None,
            difficult: false,
            exits_open: true,
            trapped: None,
            suspecting: Suspecting::No,
            panic_tries: 0,
            strong_only: false,
            strafe_ok: false,
            wall_shot: -1_000_000,
            retried: None,
            debug_tics: debug_tics(),
        }
    }

    fn now(&self) -> i64 {
        self.knowledge.last.as_ref().map_or(0, |l| l.tic)
    }

    // --- the known world ------------------------------------------------------------------

    /// The heights the planner should take: as last seen where seen, else as at the start; and
    /// where she set something off she knows the effect of and hasn't looked since, as it'll
    /// have left it (a floor lowered out of sight is down).
    pub fn states(&self) -> States {
        let k = &self.knowledge;
        let nav = &self.nav;
        let now = self.now();
        let mut out: States = nav.movers.initial.iter().enumerate().map(|(s, &h)| vec![k.seen_heights.get(&s).copied().unwrap_or(h)]).collect();
        let known: BTreeMap<i64, (TriggerLine, BTreeSet<usize>)> = k.known_triggers().into_iter().map(|(t, s)| (t.line, (t, s))).collect();
        for f in &k.fired {
            if f.life != k.lives {
                continue; // (this life's: the level's started again)
            }
            let Some((t, sectors)) = known.get(&(f.line as i64)) else { continue };
            // (as worked out when it went off; else from the heights as they're believed now)
            let effects = k.fired_effects.get(&(t.id, f.tic)).cloned().unwrap_or_else(|| nav.effects_now(t, &out));
            for &s in sectors {
                let Some(hs) = effects.iter().find(|(x, _)| *x == s).map(|(_, hs)| hs.clone()).filter(|hs| !hs.is_empty()) else { continue };
                if t.permanent() && k.doubtful(f.line, s) {
                    continue; // (in doubt that it's gone for good: as seen, not built on)
                }
                // (not seen since the move should have finished: as it'll have left it; seen
                // after, as seen. Stairs rise slowly: seen part way up is not the top)
                let h = hs[0];
                let timing = (t.line >= 0).then(|| nav.movers.timing(t.info.special, &t.info.description, s, h));
                let done = f.tic + timing.map_or(0, |tm| tm.travel as i64);
                // (what comes back is only as moved till it's due back: MAP23's timed door)
                let back = done + timing.and_then(|tm| tm.hold.map(|hold| (hold + tm.travel) as i64)).unwrap_or(0);
                if !t.permanent() && timing.is_some_and(|tm| tm.hold.is_some()) && now > back {
                    // (back at rest, as far as she knows, whatever had it before: a door opened
                    // for good, then pressed open again, shuts after its wait: E2M4's door 48)
                    if cycles_back(t.info.special, &t.info.description) && k.seen_tic.get(&s).copied().unwrap_or(-1) < back {
                        out[s] = vec![nav.movers.initial[s]];
                    }
                    continue;
                }
                if k.seen_tic.get(&s).copied().unwrap_or(-1) < done {
                    if t.permanent() {
                        out[s] = Vec::new();
                    }
                    for x in hs {
                        add_height(&mut out[s], x);
                    }
                }
            }
        }
        // a lift or a timed door she knows comes back to where it rests, wherever she last saw
        // it (E3M3)
        for (t, sectors) in known.values() {
            if !t.permanent() {
                for &s in sectors {
                    add_height(&mut out[s], nav.movers.initial[s]);
                }
            }
        }
        // a slot narrower than the box is walked over at its sides' floor, however deep she saw
        // it (E4M1), at each floor the sides can be at (MAP08's moat)
        for &tr in nav.trenches.keys() {
            let lo = nav.trench_floor(tr, |o| out[o].iter().map(|h| h.floor).fold(f64::INFINITY, f64::min));
            let hi = nav.trench_floor(tr, |o| out[o].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max));
            let floors: Vec<f64> = if lo == hi { vec![lo] } else { vec![lo, hi] };
            let mut hs: Vec<Heights> = Vec::new();
            for h in &out[tr] {
                for &floor in &floors {
                    add_height(&mut hs, Heights { floor, ceiling: h.ceiling });
                }
            }
            out[tr] = hs;
        }
        out
    }

    /// The navigator as far as she knows (rebuilt when what she knows changes). `fresh`: for the
    /// level from its start (what's spent this life works again; exits open).
    pub fn view(&mut self) -> Rc<View> {
        self.view_for(false)
    }

    pub fn view_for(&mut self, fresh: bool) -> Rc<View> {
        if fresh {
            return self.fresh_view();
        }
        let key = self.view_key(false);
        if let Some((v, vk)) = &self.view {
            if *vk == key && self.banned_jumps() == *v.banned_jumps() {
                return Rc::clone(v);
            }
        }
        let view = Rc::new(self.build_view(false));
        self.view = Some((Rc::clone(&view), key));
        view
    }

    /// The navigator as she knows the level from its start (what's spent this life works
    /// again; exits open).
    pub fn fresh_view(&self) -> Rc<View> {
        Rc::new(self.build_view(true))
    }

    /// The once-only lines she's used this life: spent, to the planner (E3M4's door; E2M1's
    /// door switch 288, pressed once from the alcove: the room behind is had then or not at all).
    fn spent_once(&self) -> BTreeSet<usize> {
        let k = &self.knowledge;
        k.fired.iter().filter(|f| f.life == k.lives && self.nav.triggers.iter().any(|t| t.line == f.line as i64 && t.once())).map(|f| f.line).collect()
    }

    /// Sectors a door or lift she pressed open this life holds open for now (as `states` has
    /// them), and the tic each starts back. (Pressed: what she opened to go through; not a lift
    /// she walked onto, which takes her away from where she was: E2M4's lift 66.)
    fn open_for_now(&self, tic: i64) -> BTreeMap<usize, i64> {
        let (k, nav) = (&self.knowledge, &self.nav);
        let known: BTreeMap<i64, (TriggerLine, BTreeSet<usize>)> = k.known_triggers().into_iter().map(|(t, s)| (t.line, (t, s))).collect();
        let mut out: BTreeMap<usize, i64> = BTreeMap::new();
        for f in k.fired.iter().filter(|f| f.life == k.lives && f.how == How::Use) {
            let Some((t, sectors)) = known.get(&(f.line as i64)) else { continue };
            if t.permanent() || t.line < 0 || !cycles_back(t.info.special, &t.info.description) {
                continue;
            }
            for (s, hs) in t.effects.iter().filter(|(s, _)| sectors.contains(s)) {
                let Some(&h) = hs.first() else { continue };
                let timing = nav.movers.timing(t.info.special, &t.info.description, *s, h);
                let Some(hold) = timing.hold else { continue };
                let until = f.tic + (timing.travel + hold) as i64;
                if tic < until {
                    let e = out.entry(*s).or_insert(until);
                    *e = (*e).max(until);
                }
            }
        }
        out
    }

    fn spent(&self, fresh: bool) -> (BTreeSet<usize>, BTreeSet<usize>) {
        let k = &self.knowledge;
        let once_used = |pred: &dyn Fn(Trigger, Category) -> bool| -> BTreeSet<usize> {
            if fresh {
                return BTreeSet::new();
            }
            k.fired.iter().filter(|f| f.life == k.lives && k.special(f.line).is_some_and(|i| pred(i.trigger, i.category))).map(|f| f.line).collect()
        };
        // (once-only door lines used this life open nothing again: MAP19's door)
        let spent_doors = once_used(&|t, _| t == Trigger::D1);
        // (and once-only teleporters used this life send no one anywhere: MAP27's)
        let spent_teleports = once_used(&|t, c| t == Trigger::W1 && c == Category::Teleport);
        (spent_doors, spent_teleports)
    }

    fn blocked(&self) -> BTreeSet<usize> {
        self.knowledge.stuck_at.iter().filter(|(_, &n)| n >= STUCK_TIMES).map(|(&p, _)| p).collect()
    }

    fn view_key(&self, fresh: bool) -> ViewKey {
        let k = &self.knowledge;
        let (spent_doors, spent_teleports) = self.spent(fresh);
        (
            k.sectors.len(),
            k.seen_polys.len(),
            k.visited.len(),
            k.learned.values().map(BTreeSet::len).sum(),
            k.teleports.len(),
            spent_doors,
            spent_teleports,
            fresh || self.exits_open,
            self.blocked(),
            self.difficult,
        )
    }

    fn build_view(&self, fresh: bool) -> View {
        let k = &self.knowledge;
        let (spent_doors, spent_teleports) = self.spent(fresh);
        let exits_open = fresh || self.exits_open;
        let blocked = self.blocked();
        let nav = &self.nav;
        let mut triggers = Vec::new();
        for (t, sectors) in k.known_triggers() {
            if t.linedef().is_some_and(|li| spent_doors.contains(&li)) {
                continue;
            }
            // (and the slots beside them: walked at their sides' floor, they never move to be
            // seen to; MAP08's moat)
            let mut keep: BTreeSet<usize> = sectors.clone();
            for (s, _) in &t.effects {
                if nav.trenches.get(s).is_some_and(|around| around.iter().any(|x| sectors.contains(x))) {
                    keep.insert(*s);
                }
            }
            let mut known = t.clone();
            known.effects.retain(|(s, _)| keep.contains(s));
            known.keep = Some(keep);
            triggers.push(known);
        }
        let mut keys_at: Vec<(Key, Vec<Point>)> = Vec::new();
        for th in k.known_things() {
            if let Some(kk) = Key::from_thing(th.kind) {
                match keys_at.iter_mut().find(|(x, _)| *x == kk) {
                    Some((_, v)) => v.push((th.x, th.y)),
                    None => keys_at.push((kk, vec![(th.x, th.y)])),
                }
            }
        }
        let mut polys: BTreeSet<usize> = k.known_polys().difference(&blocked).copied().collect();
        if !exits_open {
            polys.retain(|p| !self.pads.0.contains(p));
        }
        let teleports: BTreeSet<usize> = k.teleports.keys().copied().filter(|l| !spent_teleports.contains(l)).collect();
        let banned = if fresh { BTreeSet::new() } else { self.banned_jumps() };
        self.nav.restricted_to(polys, teleports, triggers, keys_at, banned)
    }

    /// The run-jumps left out: those missed too often (and, unless with nothing else to do,
    /// those missed at all).
    fn banned_jumps(&self) -> BTreeSet<usize> {
        let limit = if self.difficult { JUMP_MISSES } else { 1 };
        let missed = &self.knowledge.missed_jumps;
        // (and strafing runs, but when there's nothing else to do: for where a run straight
        // won't go, not a way round; MAP13's, taken as one, led her off from what was left)
        self.nav.jumps.iter().enumerate().filter(|(_, j)| missed.get(&j.key()).copied().unwrap_or(0) >= limit || (j.strafe && !self.strafe_ok)).map(|(i, _)| i).collect()
    }

    fn set_jumps(&mut self, difficult: bool) {
        self.difficult = difficult;
        self.view = None; // (it holds the old edges)
    }

    // --- objectives ---------------------------------------------------------------------------

    /// How what she'd be offered (the saturated reach) gets her to `goal`: the flood fill's way
    /// there, at all the heights the triggers she could fire give (`drive reach`); or, `planner`,
    /// as the planner's first check has it (no use-doors among the triggers, keys she knows of).
    pub fn reach_chain(&mut self, obs: &Observation, goal: Point, planner: bool) -> Option<Vec<String>> {
        let nav = Rc::clone(&self.nav);
        let view = self.view();
        let here = nav.poly_at(obs.pos())?;
        let to = nav.poly_at(goal)?;
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        let spent = self.spent_once();
        let mut key_items: Vec<(Key, Vec<(usize, Point)>)> = Vec::new();
        if planner {
            for th in self.knowledge.known_things() {
                if let Some(kk) = Key::from_thing(th.kind) {
                    key_items.push((kk, view.pickup_spots(th.x, th.y, &states)));
                }
            }
        }
        let actions: Vec<&TriggerLine> = view.triggers.iter().filter(|t| !t.effects.is_empty() && !t.linedef().is_some_and(|li| spent.contains(&li)) && !(planner && t.info.trigger == Trigger::DR)).collect();
        let (_, all, keys_all) = view.saturate(here, &states, keys, &key_items, &actions);
        view.reach_chain(here, to, &all, Some(&view.local_doors(keys_all)))
    }

    /// What she could go and do now.
    pub fn objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> Vec<Objective> {
        let t0 = Instant::now();
        let out = self.list_objectives(obs, risky, hooks);
        self.stats.listed += 1;
        self.stats.listed_secs += t0.elapsed().as_secs_f64();
        out
    }

    fn list_objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> Vec<Objective> {
        let nav = Rc::clone(&self.nav);
        let view = self.view();
        let pos = obs.pos();
        let Some(here) = nav.poly_at(pos) else { return Vec::new() };
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        // (what she could get to using what she's learned: the planner works out how)
        let spent = self.spent_once();
        let actions: Vec<&TriggerLine> = view.triggers.iter().filter(|t| !t.effects.is_empty() && !t.linedef().is_some_and(|li| spent.contains(&li))).collect();
        let t_sat = Instant::now();
        let (reach, states_all, _) = view.saturate(here, &states, keys, &[], &actions);
        self.stats.saturate_secs += t_sat.elapsed().as_secs_f64();
        self.reach = reach.clone();
        // (what she can get to only while a door or lift she's pressed open is open: go now, as a
        // player goes through a door before it shuts, if near enough to make it: E2M1's door 44,
        // whose only other switch is on the far side. Only somewhere she's never walked: back
        // where she's been can wait, there'll be a way or she'll find one)
        let open = self.open_for_now(obs.tic);
        let while_open: BTreeSet<usize> = if open.is_empty() {
            BTreeSet::new()
        } else {
            let mut shut = states.clone();
            for &s in open.keys() {
                shut[s] = vec![nav.movers.initial[s]];
            }
            let (reach_shut, _, _) = view.saturate(here, &shut, keys, &[], &actions);
            let beyond: BTreeSet<usize> = reach.difference(&reach_shut).copied().collect();
            if beyond.is_disjoint(&self.knowledge.visited) {
                beyond
            } else {
                BTreeSet::new()
            }
        };
        let closes = open.values().copied().min().unwrap_or(obs.tic);
        let mut out: Vec<Objective> = Vec::new();
        let by_line: BTreeMap<usize, &TriggerLine> = nav.triggers.iter().filter_map(|t| t.linedef().map(|li| (li, t))).collect();
        let k = &self.knowledge;
        // (where she can set off an exit from, as things are)
        let exit_polys: BTreeSet<usize> = k
            .exits()
            .into_iter()
            .filter_map(|li| by_line.get(&li).filter(|t| t.info.lock.satisfied(keys)))
            .flat_map(|t| t.activators.intersection(&reach).copied().collect::<Vec<_>>())
            .collect();
        if self.exits_open {
            for li in k.exits() {
                if let Some(t) = by_line.get(&li).filter(|t| !t.activators.is_disjoint(&reach) && t.info.lock.satisfied(keys)) {
                    let mut o = Objective::new(Kind::Exit, nav.activation_point(t, pos, &reach), format!("exit (line {li})"));
                    o.trigger = Some((*t).clone());
                    out.push(o);
                }
            }
        }
        for th in k.known_things() {
            let Some(kk) = Key::from_thing(th.kind).filter(|kk| !keys.has(*kk)) else { continue };
            let spots: Vec<Point> = view.pickup_spots(th.x, th.y, &states).into_iter().filter(|(p, _)| reach.contains(p)).map(|(_, pt)| pt).collect();
            if let Some(pt) = spots.into_iter().reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a }) {
                let mut o = Objective::new(Kind::Key, pt, format!("pick up {}", kk.name()));
                o.key = Some(kk);
                out.push(o);
            }
        }
        for li in k.interactables() {
            let Some(t) = by_line.get(&li).filter(|t| !t.activators.is_disjoint(&reach) && t.info.lock.satisfied(keys)) else { continue };
            let kind = if t.info.category.is_door() { Kind::Door } else { Kind::Switch };
            let mut o = Objective::new(kind, nav.activation_point(t, pos, &reach), format!("try {} (line {li})", if kind == Kind::Door { "door" } else { "switch" }));
            o.trigger = Some((*t).clone());
            out.push(o);
        }
        // repeatable switches pressed a while ago whose effect she never saw: worth another go
        // once there's nothing else (E3M4); by what it works (MAP08's switch, pressed "again"
        // every few seconds, set off the one in front each time); and those whose effect she's
        // put down to them on timing alone, unsure: pressed again and watched, once a life
        let learned_lines: BTreeSet<i64> = k.learned.iter().filter(|(_, s)| !s.is_empty()).map(|(&i, _)| nav.triggers[i].line).collect();
        let mut last_by_key: BTreeMap<super::model::Mechanism, i64> = BTreeMap::new();
        for f in &k.fired {
            last_by_key.insert(k.tried_key(f.line), f.tic);
        }
        let mut last_press: Vec<(usize, i64)> = Vec::new();
        for f in &k.fired {
            let tic = last_by_key[&k.tried_key(f.line)];
            match last_press.iter_mut().find(|(l, _)| *l == f.line) {
                Some(e) => e.1 = tic,
                None => last_press.push((f.line, tic)),
            }
        }
        let doubted: BTreeSet<super::model::Mechanism> = k.doubts.keys().map(|(m, _)| *m).collect();
        let mut again = Vec::new();
        for &(li, tic) in &last_press {
            let Some(t) = by_line.get(&li) else { continue };
            if !k.mapped.contains(&li) || !matches!(t.info.trigger, Trigger::SR | Trigger::GR) {
                continue;
            }
            let unsure = doubted.contains(&k.tried_key(li)) && !k.rechecked.contains(&(k.tried_key(li), k.lives));
            if learned_lines.contains(&(li as i64)) && !unsure {
                continue;
            }
            if (obs.tic - tic < REPRESS_AFTER && !unsure) || t.activators.is_disjoint(&reach) || !t.info.lock.satisfied(keys) {
                continue;
            }
            let mut o = Objective::new(Kind::Switch, nav.activation_point(t, pos, &reach), format!("try switch again (line {li}{})", if unsure { ", unsure what it does" } else { "" }));
            o.trigger = Some((*t).clone());
            again.push(o);
        }
        if self.suspecting != Suspecting::No {
            let panic = self.suspecting == Suspecting::Panic;
            out.extend(self.walls(obs, &reach, panic));
        } else {
            // (a wall she strongly suspects, right by her: tried there and then, as a player in
            // front of it would, not left for when there's nothing else; MAP05's yellow door
            // under the exit sign, passed by and left for a recheck round that never ended)
            let near: Vec<Objective> = self
                .suspected(obs, &reach, false)
                .into_iter()
                .filter(|w| w.0 >= NEAR_SUSPECT && dist(w.2, pos) <= NEAR_WALL && self.standable(w.2, false))
                .map(|(score, li, stand, mid)| {
                    let mut o = Objective::new(Kind::Wall, stand, format!("try the wall (line {li}, {score:.1})"));
                    o.look_at = Some(mid);
                    o.wall = Some(li);
                    o.suspicion = score;
                    o
                })
                .collect();
            out.extend(near);
        }
        out.extend(self.teleporters(&reach, pos));
        out.extend(self.frontier(&reach, pos));
        out.extend(self.recheck(&reach, pos));
        out.extend(self.unfinished(&reach, pos));
        out.extend(self.unvisited(&reach, pos));
        out.extend(self.noises(&reach, pos, obs.tic));
        out.extend(hooks.wanted(self, obs, &reach, pos));
        if !while_open.is_empty() {
            for o in out.iter_mut().filter(|o| o.kind != Kind::Exit && nav.poly_at(o.goal).is_some_and(|p| while_open.contains(&p))) {
                o.urgent = true;
                o.worth = Some((closes - obs.tic) as f64 * OPEN_SPEED);
                o.label = format!("{}{WHILE_OPEN}", o.label);
            }
        }
        // (not anywhere that would cut her off from most of what she can get to now, as far as
        // she knows; the way back as it'll be: a door that closes again shut, unless something
        // opens it for good, but for one she can open from where she'll be: MAP12's door 31; a
        // lift the same, at rest unless she can call it from there: MAP27's lift 156, whose only
        // switch is on it, over the room below)
        let doors = view.local_doors(keys);
        let known_triggers = self.knowledge.known_triggers();
        // (not where a door or lift that comes back was set off since one that stays: E2M4's
        // door 48, open for good, pressed again from outside, shuts behind her)
        let last_fired = |pred: &dyn Fn(&TriggerLine) -> bool, s: usize| -> i64 {
            let lines: BTreeSet<i64> = known_triggers.iter().filter(|(t, secs)| pred(t) && secs.contains(&s)).map(|(t, _)| t.line).collect();
            self.knowledge.fired.iter().filter(|f| f.life == self.knowledge.lives && lines.contains(&(f.line as i64))).map(|f| f.tic).max().unwrap_or(-1)
        };
        let for_good: BTreeSet<usize> = known_triggers
            .iter()
            .filter(|(t, _)| t.permanent())
            .flat_map(|(_, s)| s.iter().copied())
            .filter(|&s| last_fired(&|t| !t.permanent() && cycles_back(t.info.special, &t.info.description), s) <= last_fired(&|t| t.permanent(), s))
            .collect();
        let mut states_back = states_all.clone();
        let mut openers: Vec<(TriggerLine, BTreeSet<usize>)> = Vec::new();
        for (t, secs) in &known_triggers {
            let comes_back = t.info.category.is_door() || (t.info.category == Category::Lift && cycles_back(t.info.special, &t.info.description));
            if !t.permanent() && comes_back && !t.linedef().is_some_and(|li| spent.contains(&li)) {
                let shut: BTreeSet<usize> = secs.difference(&for_good).copied().collect();
                for &s in &shut {
                    states_back[s] = vec![nav.movers.initial[s]];
                }
                openers.push((t.clone(), shut));
            }
        }
        let way_back = |start: usize| -> BTreeSet<usize> {
            // (opening on the way the doors she can from there: a switch or a walk-over line on
            // the far side too: MAP19's door)
            let mut st = states_back.clone();
            loop {
                let after = view.reachable(start, &st, Some(&doors));
                let more: Vec<&BTreeSet<usize>> = openers
                    .iter()
                    .filter(|(t, secs)| !t.activators.is_disjoint(&after) && t.info.lock.satisfied(keys) && secs.iter().any(|&x| st[x] == vec![nav.movers.initial[x]]))
                    .map(|(_, secs)| secs)
                    .collect();
                let before = st.clone();
                for secs in more {
                    for &x in secs {
                        for &h in &states_all[x] {
                            add_height(&mut st[x], h);
                        }
                    }
                }
                if st == before {
                    return after; // (nothing more opens)
                }
            }
        };
        // (what of where she can get to now she couldn't get back to from a poly)
        let mut afters: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        // (and whether where she'd be left is still open to finding a way out: an edge onto
        // somewhere she's never seen, or a sector there she's never set foot in but the goal's,
        // where a walk-over line can be: no way out known isn't no way out. E1M4's nukage pit,
        // its switch for the lift out in an alcove she'd not seen into; its room of bonuses, the
        // lines round the pad in the middle opening its doors for good)
        let known_polys = self.knowledge.known_polys();
        let entered: BTreeSet<usize> = self.knowledge.visited.iter().map(|&q| nav.sector_of_poly(q)).collect();
        // (lines she's pressed or set off, and sectors she believes shut: a door's face she's not
        // tried is a way out to try, MAP04's hidden door shutting behind her)
        let pressed: BTreeSet<usize> = self.knowledge.fired.iter().map(|f| f.line).chain(self.knowledge.tried_walls.keys().copied()).collect();
        let shut = |sec: usize| states[sec].iter().all(|h| h.ceiling - h.floor < PLAYER_HEIGHT);
        let mut lost_from = |p: usize| -> (BTreeSet<usize>, bool) {
            let after = afters.entry(p).or_insert_with(|| way_back(p));
            if after.contains(&here) {
                return (BTreeSet::new(), true);
            }
            let goal_sector = nav.sector_of_poly(p);
            let unseen_edge = after.iter().any(|&q| nav.mesh.poly(q).portals.iter().any(|&pi| !known_polys.contains(&nav.mesh.portals[pi].other(q))));
            let unwalked = after.iter().map(|&q| nav.sector_of_poly(q)).any(|sec| sec != goal_sector && !entered.contains(&sec));
            let untried_door = after.iter().any(|&q| {
                nav.mesh.poly(q).portals.iter().any(|&pi| {
                    let portal = &nav.mesh.portals[pi];
                    let other = nav.sector_of_poly(portal.other(q));
                    shut(other) && portal.linedef.is_some_and(|li| !pressed.contains(&li))
                })
            });
            (reach.difference(after).copied().collect(), unseen_edge || unwalked || untried_door)
        };
        let tic = obs.tic;
        let mut usable = |this: &Self, hooks: &mut dyn Hooks, objectives: Vec<Objective>| -> Vec<Objective> {
            if risky {
                // nothing safe left: anything, one-way drops and nukage and all; but not for a
                // sound (MAP09: a noise far off took her down into a pit), and never away from
                // every way out there is, as far as she knows (MAP24's secret ledge over the
                // nukage pit: the level given up for a look)
                return objectives
                    .into_iter()
                    .filter(|o| o.kind != Kind::Noise)
                    .filter(|o| exit_polys.is_empty() || o.kind == Kind::Exit || nav.poly_at(o.goal).map(&mut lost_from).is_none_or(|(g, open)| open || !exit_polys.is_subset(&g)))
                    .map(|o| Objective { label: format!("{} (risky)", o.label), risky: true, ..o })
                    .filter(|o| this.fresh(o, tic))
                    .collect();
            }
            let mut kept: Vec<Objective> = Vec::new();
            let mut lost: Vec<BTreeSet<usize>> = Vec::new();
            for o in objectives {
                if !this.fresh(&o, tic) || hooks.dangerous(obs, &o) {
                    continue;
                }
                let cut = nav.poly_at(o.goal).map(&mut lost_from);
                let open = cut.as_ref().is_none_or(|c| c.1);
                let gone = cut.map(|c| c.0);
                // (worth the risk, however far she can't get back from: the exit, a teleporter,
                // where it leads is the unknown; a key, it opens the way)
                let risk_it = matches!(o.kind, Kind::Exit | Kind::Teleporter | Kind::Key);
                // (most of what she can get to, and never the way out, as far as she knows: MAP24's
                // nukage pit, where a wall was worth a one-way drop by the rest of it)
                let keeps = gone.as_ref().is_some_and(|g| (reach.len() - g.len()) as f64 >= KEEP_REACH * reach.len() as f64 && (exit_polys.is_empty() || open || !exit_polys.is_subset(g)));
                if risk_it || keeps {
                    lost.push(gone.unwrap_or_default());
                    kept.push(o);
                }
            }
            // what would cut her off from where there's more to do, as far as she knows, after
            // that (as a player finishes a room before dropping out of it: E2M1's, with the
            // teleporter in it, left by its door, whose switches are spent or inside)
            let goals: Vec<Option<usize>> = kept.iter().map(|o| nav.poly_at(o.goal)).collect();
            for (i, o) in kept.iter_mut().enumerate() {
                if o.kind == Kind::Exit || lost[i].is_empty() {
                    continue;
                }
                if goals.iter().enumerate().any(|(j, g)| j != i && g.is_some_and(|g| lost[i].contains(&g))) {
                    o.later = true;
                    o.label = format!("{}{LATER}", o.label);
                }
            }
            kept
        };
        // with nothing else left, the ways between rooms never walked over (walk-over lines are
        // thresholds: E4M7's), and last of all, those onto floors that hurt (E4M2's door opener
        // is on lava)
        let first = usable(self, hooks, out);
        if !first.is_empty() {
            return first;
        }
        let t1 = self.thresholds(&reach, false);
        let x = usable(self, hooks, t1);
        if !x.is_empty() {
            return x;
        }
        let t2 = self.thresholds(&reach, true);
        let x = usable(self, hooks, t2);
        if !x.is_empty() {
            return x;
        }
        let x = usable(self, hooks, again);
        if !x.is_empty() {
            return x;
        }
        let lr = hooks.last_resort(self, obs, &reach, pos);
        usable(self, hooks, lr)
    }

    /// Known polys she can get to next to polys she hasn't seen into: per sector those are in,
    /// at the reachable poly nearest her (done once she's seen into any of them).
    fn frontier(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let mesh = &self.nav.mesh;
        let known = self.knowledge.known_polys();
        let mut best: BTreeMap<usize, (f64, Point)> = BTreeMap::new();
        let mut order: Vec<usize> = Vec::new();
        let mut unseen: BTreeMap<usize, BTreeSet<i64>> = BTreeMap::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            for &q in &poly.portals {
                let portal = &mesh.portals[q];
                let other = portal.other(pi);
                if known.contains(&other) {
                    continue;
                }
                let os = mesh.poly(other).sector;
                unseen.entry(os).or_default().insert(other as i64);
                // look from the way in, just this side of it (from the poly's middle it can be
                // round a corner: E2M4)
                let mid = ((portal.p1.0 + portal.p2.0) / 2.0, (portal.p1.1 + portal.p2.1) / 2.0);
                let back = dist(mid, poly.center);
                let k = if back > 0.0 { 1.0f64.min(FRONTIER_BACK / back) } else { 0.0 };
                let mut spot = (mid.0 + (poly.center.0 - mid.0) * k, mid.1 + (poly.center.1 - mid.1) * k);
                if mesh.poly_at(spot) != Some(pi) {
                    spot = poly.center;
                }
                let d = dist(spot, pos);
                match best.get(&os) {
                    Some((bd, _)) if d >= *bd => {}
                    Some(_) => {
                        best.insert(os, (d, spot));
                    }
                    None => {
                        order.push(os);
                        best.insert(os, (d, spot));
                    }
                }
            }
        }
        order
            .into_iter()
            .map(|s| {
                let mut o = Objective::new(Kind::Frontier, self.stand_spot(best[&s].1, reach), format!("look into sector {s}"));
                o.reveals = unseen[&s].clone();
                o
            })
            .collect()
    }

    /// Reachable polys she hasn't stood in across a line from ones she has: just over the line
    /// (all a walk-over line needs; a strip's middle may have no room for the box: E4M7's).
    fn thresholds(&self, reach: &BTreeSet<usize>, hurting: bool) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let mut out = Vec::new();
        for &pi in reach.difference(&k.visited) {
            let poly = mesh.poly(pi);
            if k.damaging(poly.sector) != hurting {
                continue;
            }
            for &q in &poly.portals {
                let portal = &mesh.portals[q];
                let Some(li) = portal.linedef.filter(|_| k.visited.contains(&portal.other(pi))) else { continue };
                let mid = ((portal.p1.0 + portal.p2.0) / 2.0, (portal.p1.1 + portal.p2.1) / 2.0);
                let d = dist(mid, poly.center);
                if d < 1e-6 {
                    continue;
                }
                let spot = (mid.0 + (poly.center.0 - mid.0) / d * THRESHOLD_IN, mid.1 + (poly.center.1 - mid.1) / d * THRESHOLD_IN);
                if mesh.poly_at(spot) != Some(pi) || !self.standable(spot, false) {
                    continue; // (a sliver by a wall or shut door the box can't stand in)
                }
                let mut o = Objective::new(Kind::Visit, spot, format!("step over line {li}"));
                o.reveals = BTreeSet::from([pi as i64]);
                out.push(o);
                break;
            }
        }
        out
    }

    /// Can the box stand at q, come to from the floor there: clear of walls and things, room
    /// overhead, and not hanging over a floor more than a step higher (MAP16)? `play`: with a
    /// little play round it, for a spot to stop at (MAP21's look-out between a wall and key bars).
    pub fn standable(&self, q: Point, play: bool) -> bool {
        // (at the heights she last saw, not the map's starting ones: a door opened since, a
        // floor risen)
        let heights = self.as_seen();
        self.standable_at(q, play, &heights)
    }

    /// `standable` at these heights (hers as seen), remembered while they stay as they are:
    /// asked of every poly she can get to for each list of what she could do (E4M8: two
    /// thirds of the time making them, the same answers each time).
    pub(super) fn standable_at(&self, q: Point, play: bool, heights: &[(f64, f64)]) -> bool {
        let mut cache = self.standable_cache.borrow_mut();
        if cache.0 != heights {
            cache.0 = heights.to_vec();
            cache.1.clear();
        }
        let key = (q.0.to_bits(), q.1.to_bits(), play);
        if let Some(&ok) = cache.1.get(&key) {
            return ok;
        }
        let lab = &self.nav.jump_lab;
        let ok = self.stands_at(q, heights) && (!play || [(0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)].iter().all(|(dx, dy)| lab.fits_at_heights((q.0 + dx, q.1 + dy), heights).0));
        cache.1.insert(key, ok);
        ok
    }

    /// Each sector as she last saw it (floor, ceiling); the starting heights where she hasn't.
    pub(super) fn as_seen(&self) -> Vec<(f64, f64)> {
        let k = &self.knowledge;
        self.nav.movers.initial.iter().enumerate().map(|(s, &h)| k.seen_heights.get(&s).copied().unwrap_or(h)).map(|h| (h.floor, h.ceiling)).collect()
    }

    /// Where to stand to pick up a thing at `item`: there if the box can stand there, else the
    /// nearest spot it can, in a poly she can get to, close enough for the boxes to touch, on a
    /// floor near the item's; None if there isn't one (MAP10's soulsphere in an alcove).
    pub fn touch_spot(&self, item: Point, reach: &BTreeSet<usize>) -> Option<Point> {
        let nav = &self.nav;
        let heights = self.roomiest();
        // (the item on its floor as she believes it now: E3M9's invulnerability on a walkway
        // lowered 64)
        let floor = nav.poly_at(item).map(|p| heights[nav.sector_of_poly(p)].0);
        // (square rings, as the boxes touch, every 4 round each: E4M7's blur sphere, on the point
        // of a ledge, can be had only from a corner of the square)
        for r in [0, 8, 16, 24, 32, TOUCH - 1] {
            // (nearest first)
            let mut ring: Vec<(i32, i32)> = (-r..=r).step_by(4).chain(std::iter::once(r)).flat_map(|i| [(i, -r), (i, r), (-r, i), (r, i)]).collect();
            ring.sort_by_key(|&(dx, dy)| (dx * dx + dy * dy, dx, dy));
            ring.dedup();
            for (dx, dy) in ring {
                let q = (item.0 + f64::from(dx), item.1 + f64::from(dy));
                if !nav.poly_at(q).is_some_and(|p| reach.contains(&p)) || !self.stands_at(q, &heights) {
                    continue;
                }
                // (P_TouchSpecialThing: the item no higher than her height over her feet, no
                // more than 8 under them; E4M7's blur sphere on a ledge 56 up, had from below.
                // Her feet on the highest floor under the box: E2M7's armor bonus in a sunken
                // strip, the box half over the floor 24 above it, where she'd come to a stop
                // a few units short)
                // (where she can stand, that is: not the box over a ledge she can't step up onto,
                // MAP11's soulsphere 296 up)
                let feet = [(0.0, 0.0), (ARRIVE_SLACK, 0.0), (-ARRIVE_SLACK, 0.0), (0.0, ARRIVE_SLACK), (0.0, -ARRIVE_SLACK)]
                    .iter()
                    .map(|(dx, dy)| (q.0 + dx, q.1 + dy))
                    .filter(|&at| at == q || self.stands_at(at, &heights))
                    .map(|at| nav.jump_lab.fits_at_heights(at, &heights).1)
                    .fold(f64::NEG_INFINITY, f64::max);
                if floor.is_some_and(|f| !(-PICKUP_BELOW..=PLAYER_HEIGHT).contains(&(f - feet))) {
                    continue;
                }
                return Some(q);
            }
        }
        None
    }

    /// `spot`, or somewhere near it the box can stand (`stand_spot`), for objectives from
    /// elsewhere (a monster's last known spot).
    pub fn stand_spot_for(&self, spot: Point, reach: &BTreeSet<usize>) -> Point {
        self.stand_spot(spot, reach)
    }

    /// `spot`, if the box can stand there; else the middle of its poly, or of one next to it she
    /// can get to, where it can (E3M4).
    /// Each sector at its roomiest as she believes it now (floor, ceiling).
    fn roomiest(&self) -> Vec<(f64, f64)> {
        self.states().iter().map(|hs| hs.iter().max_by(|a, b| (a.ceiling - a.floor).total_cmp(&(b.ceiling - b.floor))).map_or((0.0, 0.0), |h| (h.floor, h.ceiling))).collect()
    }

    /// `standable`, at `heights` rather than the map's starting ones: the box against the lines,
    /// as the game has it (E3M4's door 147, shut at the start, open now; MAP16's bookcase,
    /// opened by a shot, the megasphere in it).
    fn stands_at(&self, q: Point, heights: &[(f64, f64)]) -> bool {
        let mesh = &self.nav.mesh;
        // (in the map, not in the solid between its rooms, which the BSP puts in the subsector
        // beside it all the same: MAP15's blur sphere, touched for from inside the wall by it)
        let Some(p) = mesh.poly_at(q).filter(|&p| mesh.contains(p, q)) else { return false };
        let (fits, floor, ceiling) = self.nav.jump_lab.fits_at_heights(q, heights);
        fits && ceiling - floor >= PLAYER_HEIGHT && floor <= heights[mesh.poly(p).sector].0 + MAX_STEP
    }

    fn stand_spot(&self, spot: Point, reach: &BTreeSet<usize>) -> Point {
        // (and clear of what's shut now: a door that moves isn't a wall to the map's own heights,
        // but a spot against it shut is no spot: E3M4's door 146, a recheck 4 from its face)
        let heights = self.roomiest();
        let mesh = &self.nav.mesh;
        let clear = |q: Point| self.stands_at(q, &heights);
        if clear(spot) {
            return spot;
        }
        let Some(p) = mesh.poly_at(spot) else { return spot };
        // (two rings out, nearest first: E3M4's double door, a strip 8 deep between a door open
        // and one shut, and the next strip 8 deep too)
        let next = |ps: &[usize]| -> Vec<usize> { ps.iter().flat_map(|&a| mesh.poly(a).portals.iter().map(move |&q| mesh.portals[q].other(a))).filter(|q| reach.contains(q)).collect() };
        let one = next(&[p]);
        let two = next(&one);
        for ring in [vec![p].into_iter().filter(|q| reach.contains(q)).collect(), one, two] {
            let best = ring
                .into_iter()
                .map(|pi| mesh.poly(pi).center)
                .filter(|&c| clear(c))
                .min_by(|a, b| dist(*a, spot).total_cmp(&dist(*b, spot)));
            if let Some(c) = best {
                return c;
            }
        }
        spot
    }

    /// Teleporters she has seen and not been through: walk over one (from the side that sends
    /// you) to find out where it goes. One per pad, over its side nearest her.
    fn teleporters(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, nav) = (&self.knowledge, &self.nav);
        let mut best: Vec<(BTreeSet<usize>, bool, Objective)> = Vec::new();
        let now = self.states();
        let heights = self.roomiest();
        for li in k.teleporters() {
            if k.teleports.contains_key(&li) {
                continue;
            }
            let src: BTreeSet<usize> = nav
                .tele_from
                .iter()
                .enumerate()
                .filter(|(_, es)| es.iter().any(|e| matches!(e, TeleEdge::Teleport(i) if nav.teleports[*i].line == li)))
                .map(|(p, _)| p)
                .filter(|p| reach.contains(p))
                .collect();
            if src.is_empty() {
                continue;
            }
            let (front, back) = nav.map.sides(li);
            if let (Some(f), Some(b)) = (front, back) {
                if !now[f].iter().any(|&a| now[b].iter().any(|&c| can_cross(a, c))) {
                    continue; // (the pad isn't down yet, as far as she's seen)
                }
            }
            let (a, b) = nav.map.line_seg(li);
            let length = dist(a, b).max(1.0);
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            // the front side (the one that teleports), a little back from the line
            let mut spot = (mid.0 + (b.1 - a.1) / length * 24.0, mid.1 - (b.0 - a.0) / length * 24.0);
            if !nav.poly_at(spot).is_some_and(|p| reach.contains(&p)) {
                spot = src.iter().map(|&p| nav.poly_center(p)).reduce(|x, y| if dist(y, mid) < dist(x, mid) { y } else { x }).unwrap_or(spot);
            }
            // (somewhere she fits first: not up against a torch in front of the pad, MAP11's,
            // when another side of it is clear)
            let fits = self.stands_at(spot, &heights);
            let Some(info) = classify_line(nav.map.linedefs[li].special) else { continue };
            let t = TriggerLine { id: usize::MAX, line: li as i64, info, sectors: Vec::new(), effects: Vec::new(), activators: src, shift_from: None, relevant: None, keep: None };
            let pad = k.teleport_pad(li);
            let mut o = Objective::new(Kind::Teleporter, spot, format!("try teleporter (line {li})"));
            o.trigger = Some(t);
            o.pad = pad.clone();
            match best.iter_mut().find(|(p, _, _)| *p == pad) {
                Some((_, bf, b)) if (fits, -dist(spot, pos)) > (*bf, -dist(b.goal, pos)) => (*bf, *b) = (fits, o),
                Some(_) => {}
                None => best.push((pad, fits, o)),
            }
        }
        best.into_iter().map(|(_, _, o)| o).collect()
    }

    /// Sounds she heard but couldn't see the source of, lately: go to the reachable spot nearest
    /// where it seemed to come from (if that's nearer than here) and look.
    fn noises(&self, reach: &BTreeSet<usize>, pos: Point, tic: i64) -> Vec<Objective> {
        let mesh = &self.nav.mesh;
        let mut out = Vec::new();
        for h in &self.knowledge.heard {
            if tic - h.tic > NOISE_TICS || !self.worth_a_look(h) || self.noise_seen(h) {
                continue;
            }
            let urgent = self.urgent(h, tic);
            let Some(best) = reach.iter().copied().reduce(|a, b| if dist(mesh.poly(b).center, h.origin) < dist(mesh.poly(a).center, h.origin) { b } else { a }) else { continue };
            // (where the box can stand: MAP05's)
            let spot = self.stand_spot(mesh.poly(best).center, reach);
            if dist(spot, h.origin) > dist(pos, h.origin) - HEAR_NEARER {
                continue;
            }
            let mut o = Objective::new(Kind::Noise, spot, format!("check a noise {} (~{:.0}){}", compass(h.bearing), h.distance, if urgent { " right away" } else { "" }));
            o.reveals = BTreeSet::from([h.tic]);
            o.heard = Some(*h);
            o.urgent = urgent;
            out.push(o);
        }
        out
    }

    /// A sound heard just after she set off a line she doesn't know the effect of: go and look
    /// now, while it's still moving (MAP19's line 443).
    fn urgent(&self, h: &Heard, tic: i64) -> bool {
        let k = &self.knowledge;
        if tic - h.tic >= URGENT_TICS || h.seen {
            return false;
        }
        let n = k.fired.len();
        let lines: BTreeSet<i64> = k.fired[n.saturating_sub(8)..].iter().filter(|f| (0..=CAUSE_TICS).contains(&(h.tic - f.tic))).map(|f| f.line as i64).collect();
        self.nav.triggers.iter().any(|t| lines.contains(&t.line) && k.learned.get(&t.id).is_none_or(BTreeSet::is_empty))
    }

    /// Has where a sound seemed to come from been in view since?
    fn noise_seen(&self, h: &Heard) -> bool {
        self.nav.poly_at(h.origin).is_some_and(|p| self.knowledge.seen_tic.get(&self.nav.sector_of_poly(p)).copied().unwrap_or(-1) > h.tic)
    }

    /// Parts of known rooms she can get to but hasn't walked (walking round finds the walk-over
    /// lines nobody can see): per sector, the reachable poly furthest from anywhere she's stood,
    /// if far enough to be worth it; and every room stepped into once (E1M4's door opener).
    fn unvisited(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let stood: Vec<Point> = k.visited.iter().map(|&p| mesh.poly(p).center).collect();
        let entered: BTreeSet<usize> = k.visited.iter().map(|&p| mesh.poly(p).sector).collect();
        let mut best: Vec<(usize, f64, usize)> = Vec::new();
        let mut nearest: Vec<(usize, f64, usize)> = Vec::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            if k.visited.contains(&pi) || k.damaging(poly.sector) {
                continue; // (not wading through nukage just to look)
            }
            if !self.standable(poly.center, false) {
                continue; // (an 8-wide strip by a shut door, E1M4)
            }
            if !entered.contains(&poly.sector) {
                let d = dist(poly.center, pos);
                match nearest.iter_mut().find(|e| e.0 == poly.sector) {
                    Some(e) if d < e.1 => *e = (poly.sector, d, pi),
                    Some(_) => {}
                    None => nearest.push((poly.sector, d, pi)),
                }
            }
            let gap = stood.iter().map(|&c| dist(poly.center, c)).fold(f64::INFINITY, f64::min);
            if gap < COVER_GAP {
                continue;
            }
            match best.iter_mut().find(|e| e.0 == poly.sector) {
                Some(e) if gap > e.1 => *e = (poly.sector, gap, pi),
                Some(_) => {}
                None => best.push((poly.sector, gap, pi)),
            }
        }
        for (s, _, pi) in nearest {
            if !best.iter().any(|e| e.0 == s) {
                best.push((s, 0.0, pi));
            }
        }
        best.into_iter()
            .map(|(s, _, pi)| {
                let mut o = Objective::new(Kind::Visit, mesh.poly(pi).center, format!("walk across sector {s}"));
                o.reveals = BTreeSet::from([pi as i64]);
                o
            })
            .collect()
    }

    /// Since a press whose effect wasn't seen: the known sectors round the reachable area not
    /// seen since, one per sector, at the reachable poly next to it nearest her.
    fn recheck(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let Some(since) = k.unexplained_since() else { return Vec::new() };
        let reach_sectors: BTreeSet<usize> = reach.iter().map(|&p| mesh.poly(p).sector).collect();
        let mut best: Vec<(usize, f64, Point)> = Vec::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            for &q in &poly.portals {
                let s = mesh.poly(mesh.portals[q].other(pi)).sector;
                if reach_sectors.contains(&s) || !k.sectors.contains(&s) || k.seen_tic.get(&s).copied().unwrap_or(-1) > since {
                    continue;
                }
                let d = dist(poly.center, pos);
                match best.iter_mut().find(|e| e.0 == s) {
                    Some(e) if d < e.1 => *e = (s, d, poly.center),
                    Some(_) => {}
                    None => best.push((s, d, poly.center)),
                }
            }
        }
        best.into_iter()
            .map(|(s, _, c)| {
                let mut o = Objective::new(Kind::Recheck, self.stand_spot(c, reach), format!("look at sector {s} again"));
                o.reveals = BTreeSet::from([s as i64]);
                o
            })
            .collect()
    }

    /// The objectives open, the explorer's way round: the exit first if she can get there, then
    /// the locks her keys open, then by distance as the crow flies (weighted by kind).
    pub fn ranked(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> Vec<Objective> {
        let mut options = self.objectives(obs, risky, hooks);
        let mesh = Arc::clone(&self.nav.mesh);
        if risky && !self.knowledge.traps.is_empty() {
            // (not back into a trap she's been stuck in before: MAP18's moat; but in one already,
            // it's all there is: MAP21's pit)
            let here = mesh.poly_at(obs.pos());
            let traps: Vec<&BTreeSet<usize>> = self.knowledge.traps.iter().filter(|t| here.is_none_or(|h| !t.contains(&h))).collect();
            options.retain(|o| !traps.iter().any(|trap| mesh.poly_at(o.goal).is_some_and(|p| trap.contains(&p))));
        }
        if !self.exits_open {
            // (nor anywhere she'd slide over one getting there: MAP24's pad, 38 off a goal)
            let segs = self.pads.1.clone();
            options.retain(|o| segs.iter().all(|&(a, b)| segment_distance(a, b, o.goal, o.goal) >= EXIT_MARGIN));
        }
        let pos = obs.pos();
        options.sort_by(|a, b| (a.priority(), dist(a.goal, pos) * a.kind.weight()).partial_cmp(&(b.priority(), dist(b.goal, pos) * b.kind.weight())).unwrap_or(std::cmp::Ordering::Equal));
        options
    }

    /// How far a path goes on floors she takes to hurt: not over them (a hurting pit well below
    /// a walkway she runs along, or jumps across: MAP24).
    fn wading(&self, waypoints: &[Option<Point>], heights: &States) -> f64 {
        let (mesh, k) = (&self.nav.mesh, &self.knowledge);
        let mut out = 0.0;
        let mut prev: Option<Point> = None;
        let mut on: Option<f64> = None;
        for &p in waypoints {
            let (Some(p), Some(pr)) = (p, prev) else {
                prev = p;
                continue;
            };
            let n = ((dist(pr, p) / 16.0).floor() as i64).max(1);
            let step = dist(pr, p) / n as f64;
            for i in 0..n {
                let f = (i as f64 + 0.5) / n as f64;
                let q = (pr.0 + (p.0 - pr.0) * f, pr.1 + (p.1 - pr.1) * f);
                let Some(poly) = mesh.poly_at(q) else { continue };
                let s = mesh.poly(poly).sector;
                let floor = heights[s].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max);
                if !k.damaging(s) {
                    on = Some(floor);
                } else if on.is_none_or(|o| floor >= o - MAX_STEP) {
                    out += step; // (down in it, not passing over it)
                }
            }
            prev = Some(p);
        }
        out
    }

    /// Steps to get to an objective and do it, through what's known (using switches she has
    /// learned on the way if need be).
    pub fn route_to(&mut self, obj: &Objective, obs: &Observation) -> Option<Vec<Step>> {
        // (no way there last time, with nothing a plan goes by changed since: none now. What
        // she's offered comes from a rougher reckoning than the planner's, and a goal it can't
        // reach costs a whole search: E4M9's ledges and MAP15's secret exit ring, seconds each,
        // were planned for again every think)
        let goal: GoalKey = (obj.goal.0.to_bits(), obj.goal.1.to_bits(), obj.trigger.as_ref().map(|t| t.line), matches!(obj.kind, Kind::Switch | Kind::Door | Kind::Exit));
        let sig = self.plan_signature(obs);
        if self.no_route.get(&goal) == Some(&sig) {
            self.stats.known_none += 1;
            return None;
        }
        let t0 = Instant::now();
        let route = self.plan_route(obj, obs, (goal, sig));
        let took = t0.elapsed().as_secs_f64();
        let s = &mut self.stats;
        if route.is_some() {
            s.routed += 1;
            s.routed_secs += took;
        } else {
            s.failed += 1;
            s.failed_secs += took;
        }
        route
    }

    /// Forget the goals there was no route to (`drive budget`: searched for again).
    pub fn forget_no_routes(&mut self) {
        self.no_route.clear();
    }

    /// All a plan goes by, hashed: what she knows (the view), the jumps she won't try, her keys,
    /// the lines spent this life, the heights as she has them, and where she can get to from
    /// here (for where she's starting from).
    fn plan_signature(&mut self, obs: &Observation) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.view_key(false).hash(&mut h);
        self.banned_jumps().hash(&mut h);
        obs.keys.unwrap_or_default().0.hash(&mut h);
        self.spent_once().hash(&mut h);
        for hs in self.states() {
            hs.len().hash(&mut h);
            for x in hs {
                x.key().hash(&mut h);
            }
        }
        self.reach.hash(&mut h);
        h.finish()
    }

    fn plan_route(&mut self, obj: &Objective, obs: &Observation, failed: (GoalKey, u64)) -> Option<Vec<Step>> {
        let view = self.view();
        let nav = Rc::clone(&self.nav);
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        let k = &self.knowledge;
        let mut key_items: Vec<(Key, Vec<(usize, Point)>)> = Vec::new();
        for th in k.known_things() {
            if let Some(kk) = Key::from_thing(th.kind) {
                let spots = view.pickup_spots(th.x, th.y, &states);
                match key_items.iter_mut().find(|(x, _)| *x == kk) {
                    Some((_, v)) => v.extend(spots),
                    None => key_items.push((kk, spots)),
                }
            }
        }
        // (the once-only lines she's used this life are spent: E3M4's door)
        let fired = self.spent_once();
        let plan = |goal: Point| view.plan(obs.pos(), &[goal], &Plan { keys, states: Some(&states), key_items: &key_items, max_expansions: self.plan_expansions, max_seconds: PLAN_SECONDS, goal_triggers: &[], fired: &fired });
        let mut route = plan(obj.goal);
        // no way to the nearest spot to press it from: the next nearest (MAP08's exit door)
        let mut goal = obj.goal;
        let mut tried: BTreeSet<usize> = BTreeSet::new();
        while route.is_none() && matches!(obj.kind, Kind::Switch | Kind::Door | Kind::Exit) && tried.len() < 3 {
            let (Some(t), Some(at)) = (obj.trigger.as_ref(), nav.poly_at(goal)) else { break };
            tried.insert(at);
            let rest: BTreeSet<usize> = t.activators.intersection(&self.reach).copied().filter(|p| !tried.contains(p)).collect();
            if rest.is_empty() {
                break;
            }
            goal = nav.activation_point(t, obs.pos(), &rest);
            route = plan(goal);
        }
        let Some(route) = route else {
            // (only the planner's none: not a way she won't take, which the risky twin may)
            self.no_route.insert(failed.0, failed.1);
            return None;
        };
        let step = |action: Action, trigger: Option<TriggerLine>, key: Option<Key>| Step { path: None, action, trigger, key, label: obj.label.clone(), expects: Vec::new() };
        let last = match obj.kind {
            Kind::Frontier | Kind::Recheck | Kind::Visit | Kind::Noise | Kind::Room | Kind::Item | Kind::Hunt | Kind::Wall => step(Action::Walk, None, None),
            Kind::Key => step(Action::Pickup, None, obj.key),
            Kind::Teleporter => step(Action::Cross, obj.trigger.clone(), None),
            _ => {
                let t = obj.trigger.clone()?;
                let action = match action_for(&t) {
                    Action::Use if obj.kind == Kind::Exit => Action::Exit,
                    a => a,
                };
                step(action, Some(t), None)
            }
        };
        if obj.kind != Kind::Exit && !obj.risky && !self.knowledge.protected(obs.tic) {
            // (nukage and the like only for a few steps, and never dropping into it; but where
            // she's been down before and got out again is all right: MAP24's)
            let k = &self.knowledge;
            let here = nav.poly_at(obs.pos()).map(|p| nav.sector_of_poly(p));
            let escaped: BTreeSet<usize> = k.visited.iter().map(|&p| nav.sector_of_poly(p)).filter(|s| Some(*s) != here).collect();
            let mut wade = 0.0;
            for leg in &route.legs {
                for n in &leg.path.steps {
                    if !n.contains("one-way drop") {
                        continue;
                    }
                    if let Some(s) = n.rsplit(' ').next().and_then(|x| x.parse::<usize>().ok()) {
                        if k.damaging(s) && !escaped.contains(&s) {
                            return None;
                        }
                    }
                }
                wade += self.wading(&leg.path.waypoints, &states);
            }
            if wade > WADE_LIMIT || (wade > 0.0 && obs.health.is_some_and(|h| h < WADE_HEALTH)) {
                return None;
            }
        }
        if obj.kind != Kind::Exit && !obj.risky && self.shuts_in(&route, obj, obs, &view) {
            return None;
        }
        Some(steps_from_legs(&view, &route.legs, Some(last), Some(&states)))
    }

    /// Would the walk-over lines the route goes over cut her off at its end, where she wouldn't
    /// be without them? (MAP08: the line by the red skull drops the floor she'd come back over.)
    fn shuts_in(&mut self, route: &Route, obj: &Objective, obs: &Observation, view: &View) -> bool {
        let nav = Rc::clone(&self.nav);
        let (Some(end), Some(here)) = (nav.poly_at(obj.goal), nav.poly_at(obs.pos())) else { return false };
        if route.legs.is_empty() {
            return false;
        }
        let doors = view.local_doors(route.keys);
        let k = &self.knowledge;
        let spent: BTreeSet<usize> = k.fired.iter().filter(|f| f.life == k.lives).map(|f| f.line).collect();
        let legs: Vec<(Point, Point)> = route.legs.iter().flat_map(|leg| leg.path.waypoints.windows(2).filter_map(|w| Some((w[0]?, w[1]?)))).collect();
        let mut final_states = route.states.clone();
        let mut changed = false;
        for t in &view.triggers {
            let Some(li) = t.linedef() else { continue };
            if t.effects.is_empty() || !t.info.trigger.walked() || (t.once() && spent.contains(&li)) {
                continue;
            }
            let (a, b) = nav.map.line_seg(li);
            if legs.iter().any(|&(p, q)| segments_cross(p, q, a, b)) {
                final_states = nav.apply_trigger(&final_states, t);
                changed = true;
            }
        }
        if !changed {
            return false;
        }
        // (doors gone through on the way: open on the way back too, as far as this goes)
        let opened = |st: &States| -> States {
            let mut out = st.clone();
            for (&d, o) in &doors {
                for &h in &o.heights {
                    add_height(&mut out[d], h);
                }
            }
            out
        };
        // (with what she knows how to work from there: MAP08's switch by the red skull)
        let actions: Vec<&TriggerLine> = view.triggers.iter().filter(|t| !t.effects.is_empty()).collect();
        let before = view.saturate(end, &opened(&route.states), route.keys, &[], &actions).0;
        let after = view.saturate(end, &opened(&final_states), route.keys, &[], &actions).0;
        if after.contains(&here) || after.len() as f64 >= KEEP_REACH * before.len() as f64 {
            return false;
        }
        // (shut in with something left to try is the way on, maybe: unseen parts, switches not
        // pressed; MAP08's sinking floors to the exit door)
        let known = k.known_polys();
        if after.iter().any(|&pi| nav.mesh.poly(pi).portals.iter().any(|&q| !known.contains(&nav.mesh.portals[q].other(pi)))) {
            return false;
        }
        let by_line: BTreeMap<usize, &TriggerLine> = nav.triggers.iter().filter_map(|t| t.linedef().map(|li| (li, t))).collect();
        if k.interactables().iter().any(|li| by_line.get(li).is_some_and(|t| !t.activators.is_disjoint(&after))) {
            return false;
        }
        if self.current.as_ref().is_none_or(|c| c.ident() != obj.ident()) {
            self.log.push(format!("tic {}: '{}': what the way there sets off would shut her in ({} polys to get to after, {} before)", obs.tic, obj.label, after.len(), before.len()));
        }
        true
    }

    // --- playing ------------------------------------------------------------------------------

    /// Died: the level from the start again, keeping what's known of it. Stuck with nothing at
    /// all to do before it: that place is a trap, kept in mind.
    pub fn restart(&mut self) {
        if let Some(t) = self.trapped.take() {
            self.log.push(format!("--- that was a trap ({} polys): not going back in", t.len()));
            self.knowledge.note(Note::Trap(t));
        }
        self.knowledge.restart();
        self.log.push(format!("--- died: life {}", self.knowledge.lives));
        self.current = None;
        self.follower = None;
        self.dropped.clear();
        self.drop_count.clear();
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
    pub fn hold(&mut self, obs: &Observation, last: Option<&Controls>) {
        self.knowledge.update(obs, last);
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
        self.knowledge.update(obs, last);
        if let Some(s) = self.nav.poly_at(obs.pos()).map(|p| self.nav.sector_of_poly(p)) {
            if Some(s) != self.sector {
                if self.nav.damage[s] != 0 {
                    let doing = self.follower.as_ref().map_or("-".to_string(), RouteFollower::objective);
                    self.log.push(format!("tic {}: onto damaging floor (sector {s}) from sector {}, doing: {doing}", obs.tic, self.sector.map_or("None".to_string(), |x| x.to_string())));
                }
                self.sector = Some(s);
            }
        }
        // (no looking round in the middle of a jump: MAP08's)
        let busy = self.follower.as_ref().is_some_and(RouteFollower::in_jump);
        if !busy {
            if let Some(g) = self.glance(obs) {
                return g;
            }
        }
        if self.current.is_some() && self.follower.as_ref().is_some_and(|f| f.done) {
            if let Some(look) = self.arrived(obs, hooks) {
                return look;
            }
        }
        if self.need_new(obs, hooks) && (self.current.is_some() || self.follower.is_some() || obs.tic - self.idle_since >= IDLE_RETRY) {
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

    /// Stop and look: towards a sound she couldn't see the source of, or round about when
    /// something went off underfoot and nothing's been seen or heard to happen.
    fn glance(&mut self, obs: &Observation) -> Option<Controls> {
        if let Some(until) = self.glance_until {
            if obs.tic < until {
                return Some(turn_to(obs, self.glance_at));
            }
            self.glance_until = None;
            if let Some(f) = &mut self.follower {
                f.resume(obs.tic); // (MAP24: "stuck" after looking round off a teleporter)
            }
        }
        while self.heard_n < self.knowledge.heard.len() {
            let h = self.knowledge.heard[self.heard_n];
            self.heard_n += 1;
            if self.worth_a_look(&h) && obs.tic - self.last_glance >= GLANCE_GAP {
                return Some(self.start_glance(obs, Some(h.bearing), &format!("heard something, bearing {:.0}", h.bearing)));
            }
        }
        let learned: BTreeSet<i64> = self.knowledge.learned.iter().filter(|(_, s)| !s.is_empty()).map(|(&i, _)| self.nav.triggers[i].line).collect();
        while self.glanced < self.knowledge.fired.len() {
            let f = self.knowledge.fired[self.glanced];
            let unknown = f.how == How::Walk && !learned.contains(&(f.line as i64));
            if unknown && obs.tic - f.tic < GLANCE_AFTER {
                return None; // (give it a moment to be seen first)
            }
            self.glanced += 1;
            if unknown {
                return Some(self.start_glance(obs, None, &format!("set off line {} walking", f.line)));
            }
        }
        None
    }

    /// A sound she couldn't see the source of, not one she'd expect, not background.
    fn worth_a_look(&self, h: &Heard) -> bool {
        !h.seen && !self.knowledge.expected(h) && !self.ambient(h)
    }

    fn start_glance(&mut self, obs: &Observation, bearing: Option<f64>, why: &str) -> Controls {
        self.last_glance = obs.tic;
        self.log.push(format!("tic {}: {why}: looking {}", obs.tic, if bearing.is_none() { "round" } else { "that way" }));
        self.glance_until = Some(obs.tic + LOOK_TICS);
        self.glance_at = bearing;
        turn_to(obs, bearing)
    }

    /// A sound heard again and again from the same place (a lift going up and down for ever):
    /// tuned out.
    fn ambient(&self, h: &Heard) -> bool {
        self.knowledge.heard.iter().filter(|o| dist(o.origin, h.origin) < AMBIENT_RADIUS && o.tic <= h.tic).count() > AMBIENT_REPEATS
    }

    /// Has what the objective was to look at been seen?
    fn seen_to(&self, obj: &Objective) -> bool {
        let k = &self.knowledge;
        match obj.kind {
            Kind::Frontier => {
                let known = k.known_polys();
                obj.reveals.iter().any(|&p| known.contains(&(p as usize)))
            }
            Kind::Recheck => obj.reveals.iter().all(|&s| k.seen_tic.get(&(s as usize)).copied().unwrap_or(-1) >= self.since),
            Kind::Noise => obj.heard.as_ref().is_some_and(|h| self.noise_seen(h)),
            Kind::Room => obj.reveals.iter().all(|&l| k.mapped.contains(&(l as usize))),
            Kind::Visit => obj.reveals.iter().all(|&p| k.visited.contains(&(p as usize))),
            _ => false,
        }
    }

    /// The route to the objective is done: check a press went off, and look round where there's
    /// something to look for. Controls while still looking, else None (a new objective's wanted).
    fn arrived(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> Option<Controls> {
        let obj = self.current.clone()?;
        let k = &self.knowledge;
        // (this time: one pressed long ago doesn't count; MAP08's switch)
        let pressed_now = |li: i64| usize::try_from(li).is_ok_and(|li| k.fired.iter().any(|f| k.tried_key(f.line) == k.tried_key(li) && f.tic >= self.since));
        if matches!(obj.kind, Kind::Switch | Kind::Door) && !obj.trigger.as_ref().is_some_and(|t| pressed_now(t.line)) {
            self.drop_objective(&obj, obs, "pressed, but nothing went off");
        } else if matches!(obj.kind, Kind::Frontier | Kind::Switch | Kind::Recheck | Kind::Noise | Kind::Room) && !self.seen_to(&obj) {
            let until = *self.look_until.get_or_insert(obs.tic + LOOK_TICS);
            if obs.tic < until {
                let at = if obj.kind == Kind::Noise { obj.heard.map(|h| h.origin) } else { obj.look_at };
                return Some(match at {
                    Some(at) => turn_to(obs, Some((at.1 - obs.y).atan2(at.0 - obs.x).to_degrees())),
                    None => Controls { turn: 360.0 / LOOK_TICS as f64, ..Controls::default() },
                });
            }
            if obj.kind != Kind::Switch && !self.seen_to(&obj) {
                self.drop_objective(&obj, obs, "nothing to see from there");
            }
        } else if obj.kind == Kind::Visit && !self.seen_to(&obj) {
            self.drop_objective(&obj, obs, "got there but not in it");
        } else if obj.kind == Kind::Teleporter && !obj.trigger.as_ref().is_some_and(|t| usize::try_from(t.line).is_ok_and(|li| self.knowledge.teleports.contains_key(&li))) {
            self.drop_objective(&obj, obs, "didn't take me anywhere");
        } else if obj.kind == Kind::Wall {
            // face it, press use, watch a moment (what it does, if anything, the knowledge sees)
            let wall = obj.wall.unwrap_or(0);
            if self.look_until.is_none() {
                self.look_until = Some(obs.tic + WALL_TICS + 20);
                self.knowledge.note(Note::TriedWall(wall, obs.keys.unwrap_or_default()));
            }
            let at = obj.look_at.unwrap_or(obj.goal);
            let bearing = (at.1 - obs.y).atan2(at.0 - obs.x).to_degrees();
            let off = crate::motor::norm(bearing - obs.angle).abs();
            let until = self.look_until.unwrap_or(obs.tic);
            if obs.tic < until {
                let mut c = turn_to(obs, Some(bearing));
                // (not where the press would reach an exit behind it, with the exit held back:
                // MAP22's wall in front of the exit switch, the level ended an item short)
                let exit = self.exit_last && self.knowledge.use_hits(obs.pos(), obs.angle, &obs.heights).is_some_and(|li| self.knowledge.is_exit(li));
                if off < 4.0 && obs.tic < until - WALL_TICS && !exit {
                    c.use_ = true;
                    self.look_until = Some(obs.tic + WALL_TICS); // (pressed: now watch)
                } else if off < 4.0 && obs.tic > until - WALL_TICS + 4 && obs.tic < until - 8 {
                    // (and a few shots: some walls answer to a gun, not a hand; with the pistol,
                    // single taps: a rocket sets off no wall, and blows up in her face; MAP11)
                    let gun = crate::motor::follower::tap_gun(obs);
                    if obs.weapon != Some(gun) {
                        c.weapon = gun;
                    } else if obs.tic - self.wall_shot >= crate::motor::follower::BARREL_TAP {
                        c.attack = true;
                        self.wall_shot = obs.tic;
                    }
                }
                if c.use_ {
                    // (out with the pistol as she presses, ready to shoot after)
                    c.weapon = crate::motor::follower::tap_gun(obs);
                }
                return Some(c);
            }
        } else if matches!(obj.kind, Kind::Item | Kind::Hunt) && !hooks.gone(obs, &obj) {
            if obj.kind == Kind::Item {
                // Doom picks things up only moving onto them: come down on one from a ledge and
                // stand still, and it stays (MAP10's bonuses in the nukage pits). A shuffle first.
                let until = *self.look_until.get_or_insert(obs.tic + SHUFFLE_TICS);
                if obs.tic < until {
                    let a = (obj.goal.1 - obs.y).atan2(obj.goal.0 - obs.x) - obs.angle.to_radians();
                    let push = if (obs.tic / 4) % 2 == 0 { 20.0 } else { -20.0 };
                    return Some(Controls { forward: push * a.cos(), strafe: -push * a.sin(), ..Controls::default() });
                }
            }
            self.drop_objective(&obj, obs, if obj.kind == Kind::Hunt { "not there" } else { "couldn't pick it up" });
        }
        self.look_until = None;
        self.follower = None;
        None
    }

    /// Presses that did nothing: what's in their way is worth trying.
    fn note_presses(&mut self, obs: &Observation) {
        let Some(f) = &mut self.follower else { return };
        let presses = std::mem::take(&mut f.failed_presses);
        for (at, li) in presses {
            for other in self.knowledge.note(Note::FailedPress(at, li)) {
                self.log.push(format!("tic {}: line {li} did nothing: line {other} is in the way of it", obs.tic));
            }
        }
    }

    /// Run-jumps the follower missed: counted (for the level, across lives), and left out from
    /// now on (E4M2). True if one was.
    fn note_misses(&mut self, obs: &Observation) -> bool {
        let Some(f) = &mut self.follower else { return false };
        if f.missed.is_empty() {
            return false;
        }
        let missed = std::mem::take(&mut f.missed);
        for j in missed {
            let n = self.knowledge.missed_jumps.get(&j.key()).copied().unwrap_or(0) + 1;
            self.knowledge.note(Note::MissedJump(j.key(), n));
            self.log.push(format!(
                "tic {}: missed the jump off ({:.0}, {:.0}) {}",
                obs.tic,
                j.takeoff.0,
                j.takeoff.1,
                if n >= JUMP_MISSES { "for good: not trying it again" } else { "once: another way first" }
            ));
        }
        self.set_jumps(false);
        true
    }

    /// A route failed stuck: where is counted, for the level across lives; stuck there
    /// STUCK_TIMES times, it's left out of the way (MAP18).
    fn note_stuck(&mut self, obs: &Observation) {
        let Some(p) = self.nav.poly_at(obs.pos()) else { return };
        let n = self.knowledge.stuck_at.get(&p).copied().unwrap_or(0) + 1;
        self.knowledge.note(Note::Stuck(p, n));
        if n == STUCK_TIMES {
            self.log.push(format!("tic {}: stuck at ({:.0}, {:.0}) {STUCK_TIMES} times: not going that way again", obs.tic, obs.x, obs.y));
        }
    }

    fn need_new(&mut self, obs: &Observation, hooks: &mut dyn Hooks) -> bool {
        self.note_presses(obs);
        if self.note_misses(obs) {
            return true; // (the way it was going may take that jump again: plan anew)
        }
        let Some(obj) = self.current.clone() else {
            return true;
        };
        let (done, failed) = match &self.follower {
            None => return true,
            Some(f) => (f.done, f.failed),
        };
        if done || failed {
            if failed {
                let why = self.follower.as_ref().and_then(|f| f.log.events.iter().rev().find(|e| e.contains("FAILED")).cloned()).unwrap_or_default();
                let why = why.split_once("] ").map_or(why.as_str(), |(_, r)| r).to_string();
                self.drop_objective(&obj, obs, &format!("failed: {why}"));
                if why.contains("stuck") {
                    self.note_stuck(obs);
                }
            }
            return true;
        }
        let k = &self.knowledge;
        if obj.kind == Kind::Frontier {
            let known = k.known_polys();
            if obj.reveals.iter().any(|&p| known.contains(&(p as usize))) {
                return self.ends(obs, "seen it on the way");
            }
        }
        if obj.kind == Kind::Teleporter && obj.trigger.as_ref().is_some_and(|t| usize::try_from(t.line).is_ok_and(|li| k.teleports.contains_key(&li))) {
            return self.ends(obs, "been through");
        }
        if obj.kind != Kind::Exit && !self.exit_last && k.exits().len() > self.exits_seen {
            return self.ends(obs, "an exit came into view");
        }
        if hooks.gone(obs, &obj) {
            return self.ends(obs, "gone");
        }
        let heard: Vec<Heard> = k.heard[self.urgent_n.min(k.heard.len())..].to_vec();
        self.urgent_n = k.heard.len();
        // (something just went off out of sight: go and look first; not half way through a run
        // of jumps: MAP08's)
        let jumping = self.follower.as_ref().is_some_and(RouteFollower::jumps_ahead);
        if !(obj.kind == Kind::Noise && obj.urgent) && !jumping && heard.iter().any(|h| self.urgent(h, obs.tic) && self.worth_a_look(h)) {
            return self.ends(obs, "something went off out of sight");
        }
        if !jumping && hooks.interrupt(self, obs, &obj) {
            return self.ends(obs, "interrupted");
        }
        if obs.tic - self.since > self.allowed {
            self.drop_objective(&obj, obs, "took too long");
            let stuck = self.follower.as_ref().is_some_and(|f| f.log.events.iter().rev().take(6).any(|e| e.contains("stuck at")));
            if stuck {
                self.note_stuck(obs); // (timed out stuck: the same as failing stuck)
            }
            return true;
        }
        false
    }

    /// (an objective given up mid-way, for whoever's tracing: why)
    fn ends(&mut self, obs: &Observation, why: &str) -> bool {
        if self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic)) {
            self.log.push(format!("  (objective ends at tic {}: {why})", obs.tic));
        }
        true
    }

    fn drop_objective(&mut self, obj: &Objective, obs: &Observation, why: &str) {
        let id = obj.ident();
        self.dropped.insert(id.clone(), obs.tic);
        *self.drop_count.entry(id).or_insert(0) += 1;
        if let Some(w) = obj.wall {
            *self.wall_drops.entry(w).or_insert(0) += 1;
        }
        self.log.push(format!("tic {}: dropped '{}' ({why})", obs.tic, obj.label));
    }

    /// Walls she suspects, to press use on: tried once nothing else is left (as a player humps
    /// walls for secrets).
    fn walls(&mut self, obs: &Observation, reach: &BTreeSet<usize>, panic: bool) -> Vec<Objective> {
        let mut found = self.suspected(obs, reach, panic);
        if panic {
            // (all of them: those with somewhere she can't get to behind first, nearest first)
            found.sort_by(|a, b| (-a.0).total_cmp(&-b.0).then(dist(a.2, obs.pos()).total_cmp(&dist(b.2, obs.pos()))));
        }
        found
            .into_iter()
            .take(if panic { PANIC_WALLS } else { SUSPECTS })
            .filter(|(score, _, stand, _)| self.standable(*stand, false) && !(self.strong_only && *score <= SUSPICION_MIN))
            .map(|(score, li, stand, mid)| {
                let mut o = Objective::new(Kind::Wall, stand, format!("try the wall (line {li}, {score:.1})"));
                o.look_at = Some(mid);
                o.wall = Some(li);
                o.suspicion = score;
                o
            })
            .collect()
    }

    /// The walls she suspects from where she can get to now, best first: (score, line, where
    /// to stand, the wall's middle). (`drive`'s walls.)
    pub fn suspected(&mut self, obs: &Observation, reach: &BTreeSet<usize>, panic: bool) -> Vec<(f64, usize, Point, Point)> {
        let mesh = Arc::clone(&self.nav.mesh);
        let known = self.knowledge.known_polys();
        let unreached: Vec<Point> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| matches!(t.category, "Health" | "Armor" | "Ammo" | "Weapon" | "Powerup" | "Key"))
            .filter(|t| mesh.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p) && !reach.contains(&p)))
            .map(|t| (t.x, t.y))
            .collect();
        // (the same again while nothing it goes by has changed: asked for every list of what she
        // could do, for the strong suspects by her, every wall of the level weighed each time)
        let key = {
            use std::hash::{Hash, Hasher};
            let k = &self.knowledge;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            k.mapped.hash(&mut h);
            k.tried_walls.hash(&mut h);
            for (s, x) in &k.seen_heights {
                (s, x.key()).hash(&mut h);
            }
            k.last.as_ref().and_then(|l| l.keys).unwrap_or_default().hash(&mut h);
            known.hash(&mut h);
            reach.hash(&mut h);
            for p in &unreached {
                (p.0.to_bits(), p.1.to_bits()).hash(&mut h);
            }
            panic.hash(&mut h);
            h.finish()
        };
        if let Some((_, found)) = self.suspects.as_ref().filter(|(k, _)| *k == key) {
            return found.clone();
        }
        let places = Spots::new(known.difference(reach).map(|&p| mesh.poly(p).center));
        let near = |q: Point, r: f64| places.near(q, r);
        let found = self.knowledge.suspicious(reach, &unreached, panic, Some(&near));
        self.suspects = Some((key, found.clone()));
        found
    }

    /// Not dropped lately (the longer, the more often it has been).
    fn fresh(&self, o: &Objective, tic: i64) -> bool {
        let id = o.ident();
        let times = self.drop_count.get(&id).copied().unwrap_or(0);
        tic - self.dropped.get(&id).copied().unwrap_or(-1_000_000_000) > RETRY_TICS * 2i64.pow((times - 1).clamp(0, 5) as u32)
    }

    fn next(&mut self, obs: &Observation, hooks: &mut dyn Hooks) {
        self.follower = None;
        if self.difficult {
            self.set_jumps(false); // (the jumps missed once: out again, other ways first)
        }
        self.exits_open = !self.exit_last;
        let perceive = self.knowledge.perceive;
        self.strafe_ok = false;
        let mut found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        if !found && self.nav.jumps.iter().any(|j| j.strafe) {
            // (nothing else: what a strafing run gets to, MAP03's well)
            self.strafe_ok = true;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                self.log.push(format!("tic {}: nothing else to do: strafing runs too", obs.tic));
            } else {
                self.strafe_ok = false;
            }
        }
        if found {
            self.panic_tries = 0;
        }
        if !found && perceive && self.suspecting == Suspecting::No {
            // (nothing else: walls that might hide something, pressed in turn)
            self.suspecting = Suspecting::Walls;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                self.panic_tries = 0;
                self.log.push(format!("tic {}: nothing else to do: trying walls", obs.tic));
            }
        }
        if !found && !self.dropped.is_empty() && self.retried.as_deref() != Some(self.knowledge.known_summary().as_str()) {
            // (nothing else: what was given up on lately gets another go, its wait cut short,
            // once till there's something else to do: dropped when the way was shut a while,
            // E1M4's bonus behind the door a walk-over line opens, it waited out her whole life)
            // (once till she knows something more: not each wall pressed that did nothing, which
            // threw MAP15's and E4M9's goals out of reach back in, seconds of planning each)
            self.retried = Some(self.knowledge.known_summary());
            self.log.push(format!("tic {}: nothing else to do: the {} things given up on lately get another go", obs.tic, self.dropped.len()));
            self.dropped.clear();
            self.suspecting = Suspecting::No;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        // (with the exit there, held back for what's left: one round of the walls with little to
        // go on, then out; a press that turns up something to do starts another. E2M5 went round
        // every wall of the level)
        self.strong_only = self.exit_last && self.panic_tries >= PANIC_WALLS;
        if !found && perceive && (!self.exit_last || !self.exits_open) {
            // (and nothing to try either: is there a way out of here at all? Every wall round,
            // nearest first, as a player in a pit with no way out would)
            self.suspecting = Suspecting::Panic;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
            if found {
                // (counted: walls with no more to go on than one weak sign; those with more are
                // all pressed: E4M7's line 1283, E4M9's 1637)
                if self.current.as_ref().is_some_and(|o| o.suspicion <= SUSPICION_MIN) {
                    self.panic_tries += 1;
                }
                self.log.push(format!("tic {}: nothing at all to do: pressing every wall round here", obs.tic));
            }
        }
        self.suspecting = Suspecting::No;
        self.strong_only = false;
        if !found && self.exit_last && hooks.before_exit(self, obs) {
            self.log.push(format!("tic {}: nothing else to do here: the level again, for what's left", obs.tic));
            self.restart_wanted = true;
            self.current = None;
            return;
        }
        if !found && self.exit_last {
            self.log.push(format!("tic {}: nothing else to do: for the exit", obs.tic));
            self.exits_open = true;
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        if !found && self.knowledge.missed_jumps.values().any(|&n| n < JUMP_MISSES) {
            self.log.push(format!("tic {}: nothing else to do: the jumps missed once get another go", obs.tic));
            self.set_jumps(true);
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        let stuck: Vec<usize> = self.knowledge.stuck_at.iter().filter(|(_, &n)| n >= STUCK_TIMES).map(|(&p, _)| p).collect();
        if !found && !stuck.is_empty() {
            // (the places she got stuck, given another go: two goes can be bad luck, or a goal that
            // was the trouble, not the way: E3M4's, MAP05's, MAP27's)
            self.log.push(format!("tic {}: nothing else to do: the {} places she got stuck get another go", obs.tic, stuck.len()));
            for p in stuck {
                self.knowledge.note(Note::Stuck(p, STUCK_TIMES - 1));
            }
            found = self.try_objectives(obs, false, hooks) || self.try_objectives(obs, true, hooks);
        }
        if !found {
            if self.current.is_some() {
                self.log.push(format!("tic {}: nothing left to do", obs.tic));
            }
            self.current = None;
            // (the exit out of reach too: where she can get from here is a trap, if it's where
            // she's started again from; known for the next go)
            self.trapped = Some(self.reach.clone());
        }
    }

    /// Take up the best objective there's a route to: of the nearest few as the crow flies, the
    /// one nearest by the way there (teleporters and lifts make those very different: MAP24).
    fn try_objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> bool {
        for _ in 0..3 {
            let ranked = self.ranked(obs, risky, hooks);
            if self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic)) {
                for o in ranked.iter().skip(COMPARED) {
                    self.log.push(format!("  (not compared) '{}' at ({:.0}, {:.0}), priority {}", o.label, o.goal.0, o.goal.1, o.priority()));
                }
            }
            let options: Vec<Objective> = ranked.into_iter().take(COMPARED).collect();
            if options.is_empty() {
                return false;
            }
            let mut routed: Vec<(f64, Objective, Vec<Step>)> = Vec::new();
            let debug = self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic));
            for obj in options {
                let route = self.route_to(&obj, obs);
                if debug {
                    let how = route.as_ref().map_or_else(|| "no route".to_string(), |st| format!("{:.0} ({} steps)", route_length(st), st.len()));
                    self.log.push(format!("  option '{}' at ({:.0}, {:.0}), priority {}: {how}", obj.label, obj.goal.0, obj.goal.1, obj.priority()));
                }
                match route {
                    Some(steps) if !steps.is_empty() => {
                        let length = route_length(&steps);
                        let mut obj = obj;
                        if let Some(w) = obj.worth.filter(|&w| length > w) {
                            self.log.push(format!("tic {}: '{}' not worth {length:.0} to get to (at most {w:.0}): in its turn", obs.tic, obj.label));
                            obj.urgent = false;
                            obj.worth = None;
                            obj.label = obj.label.trim_end_matches(WHILE_OPEN).trim_end_matches(" right away").to_string();
                        }
                        // (a wall given up on before: as if twice as far for each time)
                        let tries = obj.wall.map_or(1.0, |w| 2f64.powi(self.wall_drops.get(&w).copied().unwrap_or(0)));
                        let prio = obj.priority();
                        routed.push((length * obj.kind.weight() * tries, obj, steps));
                        if prio < 2 {
                            break; // (nothing further down the list comes first anyway)
                        }
                    }
                    _ => self.drop_objective(&obj, obs, "no route"),
                }
            }
            if routed.is_empty() {
                continue;
            }
            routed.sort_by(|a, b| (a.1.priority(), a.0).partial_cmp(&(b.1.priority(), b.0)).unwrap_or(std::cmp::Ordering::Equal));
            let (_, obj, steps) = routed.swap_remove(0);
            self.start(obs, obj, steps);
            return true;
        }
        false
    }

    /// Set about `obj` by `steps` (its route).
    fn start(&mut self, obs: &Observation, obj: Objective, steps: Vec<Step>) {
        self.since = obs.tic;
        // (time to do it: walking the route, and some)
        self.allowed = GIVE_UP_TICS.max((route_length(&steps) / ROUTE_SPEED) as i64 + ROUTE_SLACK);
        self.exits_seen = self.knowledge.exits().len();
        let n = steps.len();
        let states = self.states();
        let view = self.view();
        let mut follower = RouteFollower::new(view, steps, obs.keys.unwrap_or_default(), Some(states), false, obs.tic);
        follower.fired.extend(self.spent_once()); // (its replans: not by them either)
        self.follower = Some(follower);
        self.follower_seen = 0;
        self.log.push(format!("tic {}: {} ({n} steps) | {}", obs.tic, obj.label, self.knowledge.summary()));
        self.current = Some(obj);
    }

    /// Set about this objective now, whatever she'd have chosen (evil drive: what if she had?).
    /// False if she has no route to it.
    pub fn take(&mut self, obs: &Observation, obj: Objective) -> bool {
        match self.route_to(&obj, obs) {
            Some(steps) if !steps.is_empty() => {
                self.start(obs, obj, steps);
                true
            }
            _ => false,
        }
    }
}

fn debug_tics() -> Option<(i64, i64)> {
    let v = std::env::var("EVIL_DEBUG_OBJECTIVES").ok()?;
    let (a, b) = v.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
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
    steps
        .iter()
        .filter_map(|st| st.path.as_ref())
        .flat_map(|p| p.waypoints.windows(2).filter_map(|w| Some(dist(w[0]?, w[1]?))))
        .sum()
}

/// The polys beyond walk-over exit lines (of the two sides of each, the sector more of whose
/// edge is exit lines: MAP24's pad), and those lines.
fn exit_pads(nav: &Navigator) -> (BTreeSet<usize>, Vec<(Point, Point)>) {
    let m = &nav.map;
    let exit_lines: Vec<usize> = (0..m.linedefs.len()).filter(|&li| classify_line(m.linedefs[li].special).is_some_and(|i| i.category.is_exit() && i.trigger.walked())).collect();
    let length = |li: usize| {
        let (a, b) = m.line_seg(li);
        dist(a, b)
    };
    let share = |sec: usize| -> f64 {
        let edge: Vec<usize> = (0..m.linedefs.len()).filter(|&li| { let (f, b) = m.sides(li); f == Some(sec) || b == Some(sec) }).collect();
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
