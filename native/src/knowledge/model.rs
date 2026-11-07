//! What Evil knows about the level: only what she has seen and what she has learned by doing.
//!
//! Seeing follows the automap: a linedef is mapped once the renderer draws any part of it
//! (`sight`), and the sectors either side of a mapped line are known, with what's in them.
//! What lines do is learned by using them: a line she sets off (pressing it, walking over it,
//! shooting it) is fired, and its effect on a sector is learned when she sees that sector move,
//! then or later. What a line *is* can be told on sight from its special type (not its
//! textures, which custom WADs draw however they like): switches and use-doors, exits, the keys
//! a lock needs, teleporters.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::geom::{cells, dist, Point};
use crate::map::{Linedef, ML_MAPPED, ML_SECRET, NO_SIDEDEF};
use crate::motor::{Controls, Observation};
use crate::nav::Heights;
use crate::movers::Effects;
use crate::navigator::{Navigator, States, TriggerLine, MAX_STEP, PLAYER_HEIGHT, SHOT_HEIGHT};
use crate::sight::{Sight, View as SightView};
use crate::specials::{classify_line, Category, Keys, LineSpecial, Lock, Trigger};

/// P_UseLines
pub const USE_RANGE: f64 = 64.0;
/// far enough for any gun line worth shooting
pub const SHOT_RANGE: f64 = 2048.0;
/// screen columns the view is worked out for (the engine's width)
pub const VIEW_WIDTH: i32 = 320;
/// S_CLIPPING_DIST: sounds further off aren't heard at all
pub const HEAR_RANGE: f64 = 1200.0;
/// how well she can tell where a sound came from: degrees...
pub const HEAR_BEARING: f64 = 15.0;
/// ...and units
pub const HEAR_DISTANCE: f64 = 64.0;
/// further than this in a tic is a teleport
pub const TELEPORT_JUMP: f64 = 64.0;
/// ...through a teleport line this near where she was
pub const TELEPORT_NEAR: f64 = 48.0;
/// a sound this near is something she'll see anyway
pub const NEAR_SOUND: f64 = 128.0;
/// a sound this near a known mover's is that mover...
pub const EXPECTED_RADIUS: f64 = 128.0;
/// ...which she set off at most this many tics before (later, it moved by itself)
pub const EXPECTED_FOR: i64 = 8 * 35;
/// a once-only line set off moves nothing in this long...
pub const TAKE_TICS: i64 = 35;
/// a sector starting to move this soon after a press: the press did it (later: in doubt)
pub const SURE_TICS: i64 = 8;
/// ...where it acts on what moved this lately: it didn't take (Doom leaves a switch on a busy
/// sector unused)
pub const BUSY_TICS: i64 = 200;
/// textures that make a line look like a switch, a door or a lift
pub const LOOKS_INTERACTIVE: [&str; 11] = ["SW1", "SW2", "DOOR", "BIGDOOR", "SPCDOOR", "EXITDOOR", "ZDOOR", "ICKDOOR", "ZELDOOR", "PLAT", "SUPPORT"];
/// a wall suspected this much is worth a press of use
pub const SUSPICION_MIN: f64 = 2.0;
/// unhurt on a floor this long: it doesn't hurt
const FELT_TICS: i64 = 70;
/// a hurting floor hurts on every 32nd tic of the level's clock...
const FLOOR_PERIOD: i64 = 32;
/// ...so hurt twice on it a multiple of that apart, this soon: it's the floor
const FLOOR_CONFIRM: i64 = 3 * FLOOR_PERIOD;
/// a radiation suit lasts a minute
const SUIT_TICS: i64 = 60 * 35;
/// sounds this close together from the same way are one
const TICS_SAME_SOUND: i64 = 4;
/// floors that look like they'd hurt (liquids)
const HAZARD_FLATS: [&str; 4] = ["NUKAGE", "LAVA", "BLOOD", "SLIME0"];

/// "the game says the line needs a key"
fn needs_key(message: &str) -> bool {
    let m = message.to_lowercase();
    let words: Vec<&str> = m.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let Some(i) = words.iter().position(|w| *w == "need") else { return false };
    words[i + 1..].iter().any(|w| matches!(*w, "key" | "keys" | "card" | "skull"))
}

/// "floor raise N" and the like: each press adds on.
fn adds_on(description: &str) -> bool {
    let mut words = description.split(|c: char| c.is_whitespace() || c == ',');
    while let Some(w) = words.next() {
        if w == "raise" && words.clone().next().is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            return true;
        }
    }
    false
}

/// What trying a line tries (or what it works): lines alike work the same thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mechanism {
    /// a door line: its door (the sector behind it; the line itself if none)
    Door(usize),
    /// the special and the tag
    Tag(u16, u16),
    /// the line alone
    Line(usize),
}

impl std::fmt::Display for Mechanism {
    /// As the logs have it: ('door', 31), ('tag', 61, 2), ('line', 7).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mechanism::Door(s) => write!(f, "('door', {s})"),
            Mechanism::Tag(special, tag) => write!(f, "('tag', {special}, {tag})"),
            Mechanism::Line(li) => write!(f, "('line', {li})"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Use,
    Walk,
    Shoot,
}

impl How {
    pub fn name(self) -> &'static str {
        match self {
            How::Use => "use",
            How::Walk => "walk",
            How::Shoot => "shoot",
        }
    }
}

/// A line she set off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fired {
    pub line: usize,
    pub tic: i64,
    pub how: How,
    /// which try at the level (a once-only line works again in the next)
    pub life: i32,
}

/// What the explorer puts down in the knowledge itself (the rest she sees).
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// a wall pressed, suspecting it, with these keys held
    TriedWall(usize, Keys),
    /// a route failed stuck at this poly: the times it has now
    Stuck(usize, i32),
    /// a run-jump came down off its landing: the times it has now
    MissedJump(String, i32),
    /// a place she was stuck in with nothing to do
    Trap(BTreeSet<usize>),
    /// a switch pressed from here did nothing
    FailedPress(Point, usize),
}

/// A sound she heard: roughly which way and how far, not what made it.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub tic: i64,
    /// degrees, anticlockwise from east, from where she was
    pub bearing: f64,
    pub distance: f64,
    /// where that puts it (a guess)
    pub origin: Point,
    /// what made it was in view anyway
    pub seen: bool,
}

pub struct Knowledge {
    pub nav: Rc<Navigator>,
    pub width: i32,
    /// she gets hurt (no god mode): a floor's harm is learned from her health, else from
    /// standing on it
    pub mortal: bool,
    /// she can tell a switch or a door only by its look; else every line with an action is
    /// known on sight (peeking at the map)
    pub perceive: bool,
    pub mapped: BTreeSet<usize>,
    pub sectors: BTreeSet<usize>,
    /// as last seen
    pub seen_heights: BTreeMap<usize, Heights>,
    /// sector -> the last tic it was in view
    pub seen_tic: BTreeMap<usize, i64>,
    pub fired: Vec<Fired>,
    /// trigger id -> sectors whose effect she knows
    pub learned: BTreeMap<usize, BTreeSet<usize>>,
    /// teleport line used -> where it took her
    pub teleports: BTreeMap<usize, Point>,
    /// polys stood in
    pub visited: BTreeSet<usize>,
    /// polys seen into (or in front of a mapped line)
    pub seen_polys: BTreeSet<usize>,
    pub heard: Vec<Heard>,
    /// sector stood on -> did it hurt
    pub felt: BTreeMap<usize, bool>,
    /// run-jump key -> times it came down off its landing
    pub missed_jumps: BTreeMap<String, i32>,
    /// places she got stuck in, nothing to do and no way out
    pub traps: Vec<BTreeSet<usize>>,
    /// poly -> times a route failed stuck there
    pub stuck_at: BTreeMap<usize, i32>,
    /// walls she's pressed use on, suspecting them -> the keys she held then (with a key she
    /// didn't have, worth another press: MAP05's blue door like the wall round it)
    pub tried_walls: BTreeMap<usize, Keys>,
    /// lines between her and a switch that did nothing
    pub in_the_way: BTreeSet<usize>,
    /// (trigger, tic it fired) -> what it did, worked out then: a floor raised "to the lowest
    /// ceiling round it" goes by the ceilings as they were, not as they are later (E1M4: the
    /// walkway rose to 104, the door beside it shut; opened since, worked out afresh it seemed
    /// to have risen to 160, sealing the room)
    pub fired_effects: BTreeMap<(usize, i64), Effects>,
    /// what the explorer put down here itself (not seen: done), with the tic: so a run played
    /// back from its inputs (replay) knows it too
    pub journal: Vec<(i64, Note)>,
    /// lines the game said need a key when she pressed them: a door or a switch, however it
    /// looks
    pub locked: BTreeSet<usize>,
    /// what she's learned a line does, put down to it on timing alone: (tried key, sector) ->
    /// tic. The planner doesn't build on these; once nothing else is left the line is pressed
    /// again and watched (rechecked: (tried key, life) done)
    pub doubts: BTreeMap<(Mechanism, usize), i64>,
    pub rechecked: BTreeSet<(Mechanism, i32)>,
    /// sector -> does it hurt now, for floors seen moved by a line that changes their type
    /// (this life)
    pub changed_floor: BTreeMap<usize, bool>,
    /// a radiation suit on till this tic
    pub suit_until: Option<i64>,
    /// tries at the level so far
    pub lives: i32,
    pub events: Vec<String>,
    pub(super) sight: Sight,
    /// line -> trigger ids
    by_line: BTreeMap<usize, Vec<usize>>,
    /// line -> its special
    info: BTreeMap<usize, LineSpecial>,
    pub last: Option<Observation>,
    /// (sector, since tic) on its floor
    standing: Option<(usize, i64)>,
    /// (sector, tic) last hurt standing on it
    hurt_on: Option<(usize, i64)>,
    /// vertex -> lines ending there
    ends: BTreeMap<usize, Vec<usize>>,
    /// a doubted line pressed again, watched: (fired, sectors in doubt)
    checking: Vec<(Fired, BTreeSet<usize>)>,
    /// where each sector's sounds come from: the middle of its bounding box (P_GroupLines)
    pub sound_origin: BTreeMap<usize, Point>,
    /// lines that get drawn when looked at (not two-sided ones alike on both sides)
    drawable: Vec<usize>,
    /// every sector's heights last tic
    live: Vec<Heights>,
    moving: BTreeSet<usize>,
    /// sector -> the tic it was last heard moving
    moved_at: BTreeMap<usize, i64>,
    /// once-only lines set off: did they take?
    taking: Vec<(Fired, BTreeSet<usize>)>,
    /// per linedef: the subsectors its segs bound
    line_subsectors: BTreeMap<usize, BTreeSet<usize>>,
}

impl Knowledge {
    pub fn new(nav: Rc<Navigator>, mortal: bool, perceive: bool) -> Knowledge {
        let m = Arc::clone(&nav.map);
        let mut by_line: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for t in &nav.triggers {
            if let Some(li) = t.linedef() {
                by_line.entry(li).or_default().push(t.id);
            }
        }
        let info: BTreeMap<usize, LineSpecial> = m.linedefs.iter().enumerate().filter_map(|(i, l)| classify_line(l.special).map(|s| (i, s))).collect();
        let mut boxes: BTreeMap<usize, [f64; 4]> = BTreeMap::new();
        for (li, l) in m.linedefs.iter().enumerate() {
            let (a, b) = m.line_seg(li);
            for s in [m.front_sector(l), m.back_sector(l)].into_iter().flatten() {
                let bx = boxes.entry(s).or_insert([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]);
                for v in [a, b] {
                    bx[0] = bx[0].min(v.0);
                    bx[1] = bx[1].min(v.1);
                    bx[2] = bx[2].max(v.0);
                    bx[3] = bx[3].max(v.1);
                }
            }
        }
        let sound_origin = boxes.into_iter().map(|(s, b)| (s, ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0))).collect();
        let drawable = (0..m.linedefs.len())
            .filter(|&li| {
                let l = &m.linedefs[li];
                match m.sides(li) {
                    (Some(f), Some(b)) => {
                        let (sf, sb) = (&m.sectors[f], &m.sectors[b]);
                        (sf.floor, sf.ceiling, &sf.floor_tex, &sf.ceiling_tex, sf.light) != (sb.floor, sb.ceiling, &sb.floor_tex, &sb.ceiling_tex, sb.light) || !matches!(m.sidedefs[usize::from(l.front)].middle.as_str(), "-" | "")
                    }
                    _ => true,
                }
            })
            .collect();
        let mut line_subsectors: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for (ss, sub) in m.subsectors.iter().enumerate() {
            for seg in &m.segs[sub.first_seg..(sub.first_seg + sub.seg_count).min(m.segs.len())] {
                line_subsectors.entry(seg.linedef).or_default().insert(ss);
            }
        }
        let mut ends: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, l) in m.linedefs.iter().enumerate() {
            for v in [l.v1, l.v2] {
                ends.entry(v).or_default().push(i);
            }
        }
        let mut k = Knowledge {
            sight: Sight::new(&m),
            nav,
            width: VIEW_WIDTH,
            mortal,
            perceive,
            mapped: BTreeSet::new(),
            sectors: BTreeSet::new(),
            seen_heights: BTreeMap::new(),
            seen_tic: BTreeMap::new(),
            fired: Vec::new(),
            learned: BTreeMap::new(),
            teleports: BTreeMap::new(),
            visited: BTreeSet::new(),
            seen_polys: BTreeSet::new(),
            heard: Vec::new(),
            felt: BTreeMap::new(),
            missed_jumps: BTreeMap::new(),
            traps: Vec::new(),
            stuck_at: BTreeMap::new(),
            tried_walls: BTreeMap::new(),
            in_the_way: BTreeSet::new(),
            journal: Vec::new(),
            fired_effects: BTreeMap::new(),
            locked: BTreeSet::new(),
            doubts: BTreeMap::new(),
            rechecked: BTreeSet::new(),
            changed_floor: BTreeMap::new(),
            suit_until: None,
            lives: 1,
            events: Vec::new(),
            by_line,
            info,
            last: None,
            standing: None,
            hurt_on: None,
            ends,
            checking: Vec::new(),
            sound_origin,
            drawable,
            live: Vec::new(),
            moving: BTreeSet::new(),
            moved_at: BTreeMap::new(),
            taking: Vec::new(),
            line_subsectors,
        };
        for (i, l) in m.linedefs.iter().enumerate() {
            if l.flags & ML_MAPPED != 0 {
                k.map_line(i);
            }
        }
        k
    }

    fn trigger(&self, id: usize) -> &TriggerLine {
        &self.nav.triggers[id]
    }

    // --- seeing -----------------------------------------------------------------------------

    fn see_subsector(&mut self, ss: usize) {
        let nav = Rc::clone(&self.nav);
        self.seen_polys.extend(nav.mesh.subsector_polys(ss));
    }

    fn map_line(&mut self, li: usize) {
        self.mapped.insert(li);
        let subs: Vec<usize> = self.line_subsectors.get(&li).into_iter().flatten().copied().collect();
        for ss in subs {
            self.see_subsector(ss);
        }
        let (f, b) = self.nav.map.sides(li);
        for s in [f, b].into_iter().flatten() {
            if self.sectors.insert(s) {
                let sec = &self.nav.map.sectors[s];
                self.seen_heights.entry(s).or_insert(Heights { floor: sec.floor, ceiling: sec.ceiling });
            }
        }
    }

    /// Take in a tic: what's in view now, what got set off (`controls`: what she did on the tic
    /// that led here).
    pub fn update(&mut self, obs: &Observation, controls: Option<&Controls>) {
        let nav = Rc::clone(&self.nav);
        let m = &nav.map;
        let heights: Vec<(f64, f64)> = obs.heights.iter().map(|h| (h.floor, h.ceiling)).collect();
        let seen = self.sight.view(&SightView { x: obs.x, y: obs.y, angle: obs.angle, width: self.width, heights: &heights });
        for &ss in &seen.subsectors {
            self.see_subsector(ss);
        }
        let mut in_view: BTreeSet<usize> = BTreeSet::new();
        for &li in &seen.through {
            // (not on the automap, but the floor across it is in view)
            let (f, b) = m.sides(li);
            for s in [f, b].into_iter().flatten() {
                in_view.insert(s);
                self.sectors.insert(s);
            }
        }
        for &li in &seen.drawn {
            let (f, b) = m.sides(li);
            in_view.extend([f, b].into_iter().flatten());
            if !self.mapped.contains(&li) {
                self.map_line(li);
            }
        }
        if let Some(poly) = nav.mesh.poly_at(obs.pos()) {
            let sector = nav.sector_of_poly(poly);
            self.visited.insert(poly);
            let on_floor = obs.z.is_none_or(|z| z <= obs.heights[sector].floor + 1.0);
            self.feel(if on_floor { Some(sector) } else { None }, obs);
            in_view.insert(sector);
            self.sectors.insert(sector);
        }
        if let Some(last) = &self.last {
            if let (Some(now), Some(before)) = (obs.suits, last.suits) {
                if now > before {
                    self.suit_until = Some(obs.tic + SUIT_TICS);
                    self.events.push(format!("tic {}: radiation suit on", obs.tic));
                }
            }
        }
        if let Some(last) = self.last.take() {
            if dist(last.pos(), obs.pos()) > TELEPORT_JUMP {
                self.teleported(&last, obs);
            } else {
                self.detect_fired(&last, obs, controls);
            }
            self.last = Some(last);
        }
        for &s in &in_view {
            self.seen_tic.insert(s, obs.tic);
            let now = obs.heights[s];
            if self.seen_heights.get(&s).is_some_and(|&h| h != now) {
                self.saw_move(s, obs.tic, now);
            }
            self.seen_heights.insert(s, now);
        }
        self.hear(obs, &in_view);
        self.took(obs.tic);
        self.check_doubts(obs.tic);
        self.last = Some(obs.clone());
    }

    // --- hearing ----------------------------------------------------------------------------

    /// This tic's sounds, as the engine plays them: a floor or ceiling makes one from its
    /// sector's sound origin as it starts moving and as it stops, heard anywhere within
    /// HEAR_RANGE (through walls), with which way it came from and roughly how far.
    fn hear(&mut self, obs: &Observation, in_view: &BTreeSet<usize>) {
        let first = self.live.is_empty();
        for (s, &h) in obs.heights.iter().enumerate() {
            let prev = if first { None } else { self.live.get(s).copied() };
            if self.live.len() <= s {
                self.live.push(h);
            } else {
                self.live[s] = h;
            }
            let Some(prev) = prev else { continue };
            let moving = prev != h;
            if moving == self.moving.contains(&s) {
                continue;
            }
            if moving {
                self.moving.insert(s);
                self.moved_at.insert(s, obs.tic);
            } else {
                self.moving.remove(&s);
            }
            let Some(&origin) = self.sound_origin.get(&s) else { continue };
            let d = dist(obs.pos(), origin);
            if d > HEAR_RANGE {
                continue;
            }
            // (+ 0.0: Python's round() gives an int, never -0)
            let bearing = ((origin.1 - obs.y).atan2(origin.0 - obs.x).to_degrees() / HEAR_BEARING).round_ties_even() * HEAR_BEARING + 0.0;
            let distance = HEAR_DISTANCE.max((d / HEAR_DISTANCE).round_ties_even() * HEAR_DISTANCE) + 0.0;
            let n = self.heard.len();
            if self.heard[n.saturating_sub(8)..].iter().any(|hd| hd.tic >= obs.tic - TICS_SAME_SOUND && hd.bearing == bearing && hd.distance == distance) {
                continue; // (one sound, several sectors: a door's two halves, stairs)
            }
            let est = (obs.x + distance * bearing.to_radians().cos(), obs.y + distance * bearing.to_radians().sin());
            let seen = in_view.contains(&s);
            self.heard.push(Heard { tic: obs.tic, bearing, distance, origin: est, seen });
            if !seen {
                self.events.push(format!("tic {}: heard something move, bearing {bearing:.0}, about {distance:.0} away", obs.tic));
            }
        }
    }

    /// Sector s is seen at new heights: put it down to the latest line fired before it started
    /// moving that moves it, and moves it this way (a door seen closing is no open-for-good
    /// switch's doing: MAP12's). Sure if it started moving right after the press, else in doubt.
    fn saw_move(&mut self, s: usize, tic: i64, now: Heights) {
        let was = self.seen_heights.get(&s).copied();
        // (when it started moving, by its sound; this tic, if it wasn't moving till now)
        let prev = self.live.get(s).copied();
        let fresh = !self.moving.contains(&s) && prev.is_some_and(|p| p != now);
        let started = if fresh { tic } else { self.moved_at.get(&s).copied().unwrap_or(tic) };
        let fired: Vec<Fired> = self.fired.iter().rev().copied().collect();
        for f in fired {
            if f.life != self.lives || f.tic > started {
                continue;
            }
            let ids = self.by_line.get(&f.line).cloned().unwrap_or_default();
            for ti in ids {
                let t = self.trigger(ti).clone();
                if t.effect(s).is_none() || !self.heads_for(&t, s, was, now) {
                    continue;
                }
                let key = (self.tried_key(f.line), s);
                if started - f.tic <= SURE_TICS {
                    if self.doubts.remove(&key).is_some() {
                        self.events.push(format!("tic {tic}: line {} does move sector {s}: sure now", f.line));
                    }
                } else if !self.doubts.contains_key(&key) && !self.learned.get(&ti).is_some_and(|l| l.contains(&s)) {
                    self.doubts.insert(key, tic);
                }
                if let Some(hurts) = self.nav.floor_after(&t, s) {
                    if self.changed_floor.get(&s) != Some(&hurts) {
                        self.changed_floor.insert(s, hurts);
                        self.events.push(format!("tic {tic}: sector {s}'s floor {}", if hurts { "turns to something that hurts" } else { "is safe now" }));
                    }
                }
                if self.learned.entry(ti).or_default().insert(s) {
                    self.events.push(format!("tic {tic}: learned line {} ({}) moves sector {s}", f.line, t.info.description));
                }
                return;
            }
        }
    }

    /// Is sector s, seen go from `was` to `now`, on its way to where t puts it (or, for a line
    /// whose move comes back, back to rest)?
    fn heads_for(&self, t: &TriggerLine, s: usize, was: Option<Heights>, now: Heights) -> bool {
        let Some(was) = was else { return true };
        if t.shift_from.is_some() || t.relevant.is_some() {
            return true; // (where it goes depends on how things are: no telling)
        }
        let mut targets: Vec<Heights> = t.effect(s).map(<[Heights]>::to_vec).unwrap_or_default();
        if !t.permanent() {
            targets.push(self.nav.movers.initial[s]);
        }
        let off = |h: Heights, at: Heights| (at.floor - h.floor).abs() + (at.ceiling - h.ceiling).abs();
        targets.iter().any(|&h| off(h, now) < off(h, was) - 0.5 || off(h, now) < 0.5)
    }

    pub fn doubtful(&self, line: usize, s: usize) -> bool {
        self.doubts.contains_key(&(self.tried_key(line), s))
    }

    /// A line in doubt pressed again: what it's put down to moving, moving right after (sure),
    /// already where it puts it (no telling, but it fits: kept), or neither (unlearned).
    fn check_doubts(&mut self, tic: i64) {
        let checking = std::mem::take(&mut self.checking);
        let mut still = Vec::new();
        for (f, sectors) in checking {
            let left: BTreeSet<usize> = sectors.iter().copied().filter(|&s| self.doubts.contains_key(&(self.tried_key(f.line), s))).collect();
            if left.is_empty() {
                continue;
            }
            if tic - f.tic < TAKE_TICS {
                still.push((f, left));
                continue;
            }
            for s in left {
                let key = (self.tried_key(f.line), s);
                let mut fits = false;
                for &ti in self.by_line.get(&f.line).into_iter().flatten() {
                    let t = self.trigger(ti);
                    if let Some(hs) = t.effect(s).filter(|_| t.shift_from.is_none() && t.relevant.is_none()) {
                        fits = fits || self.seen_heights.get(&s).is_some_and(|seen| hs.iter().any(|h| (seen.floor - h.floor).abs() + (seen.ceiling - h.ceiling).abs() < 0.5));
                    }
                }
                if fits || self.running(s, tic) || self.seen_tic.get(&s).copied().unwrap_or(-1) < f.tic {
                    continue; // (still in doubt: no telling from here)
                }
                self.doubts.remove(&key);
                for &ti in self.by_line.get(&f.line).into_iter().flatten() {
                    if let Some(l) = self.learned.get_mut(&ti) {
                        l.remove(&s);
                    }
                }
                self.events.push(format!("tic {tic}: line {} doesn't move sector {s} after all", f.line));
            }
        }
        self.checking = still;
    }

    // --- doing ------------------------------------------------------------------------------

    /// Linedefs the segment a-b crosses, as (fraction along, line), nearest first.
    fn lines_along(&self, a: Point, b: Point) -> Vec<(f64, usize)> {
        let m = &self.nav.map;
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut out = Vec::new();
        for cell in cells(a.0, a.1, b.0, b.1) {
            for &li in self.nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                if !seen.insert(li) {
                    continue;
                }
                let (p, q) = m.line_seg(li);
                if let Some(t) = intersect(a, b, p, q) {
                    out.push((t, li));
                }
            }
        }
        out.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        out
    }

    fn front(&self, li: usize, x: f64, y: f64) -> bool {
        let (a, b) = self.nav.map.line_seg(li);
        (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0) <= 0.0
    }

    fn opening(&self, li: usize, heights: &[Heights]) -> Option<(f64, f64)> {
        let (Some(f), Some(b)) = self.nav.map.sides(li) else { return None };
        Some((heights[f].floor.max(heights[b].floor), heights[f].ceiling.min(heights[b].ceiling)))
    }

    fn detect_fired(&mut self, last: &Observation, obs: &Observation, controls: Option<&Controls>) {
        // walking over
        for (_, li) in self.lines_along(last.pos(), obs.pos()) {
            let Some(info) = self.info.get(&li) else { continue };
            if !info.trigger.walked() || info.category == Category::Teleport {
                continue; // (going through a teleport is seen by where she ends up)
            }
            self.fire(li, obs.tic, How::Walk);
        }
        let Some(c) = controls else { return };
        let a = last.angle.to_radians();
        let heights = &obs.heights;
        if c.use_ {
            // P_UseLines: the first line along the use trace with a special, unless something
            // shut is in the way first (only from its front). A once-only line used this life has
            // no special any more: the trace goes past it (E2M7)
            let end = (last.x + USE_RANGE * a.cos(), last.y + USE_RANGE * a.sin());
            for (_, li) in self.lines_along(last.pos(), end) {
                if let Some(info) = self.info.get(&li).filter(|_| !self.spent(li)).cloned() {
                    // (any line with a special stops it, a walk-over one too: E2M4's door
                    // behind one; only one that's used goes off. A locked one without its key
                    // does nothing, and the game says so: something to come back to with the key)
                    if info.trigger.used() && self.front(li, last.x, last.y) {
                        if info.lock.satisfied(last.keys.unwrap_or_default()) {
                            self.fire(li, obs.tic, How::Use);
                        } else if obs.message.as_deref().is_some_and(needs_key) && !self.locked.contains(&li) {
                            self.locked.insert(li);
                            self.events.push(format!("tic {}: line {li} is locked (\"{}\")", obs.tic, obs.message.as_deref().unwrap_or("")));
                        }
                    }
                    break;
                }
                match self.opening(li, heights) {
                    Some((lo, hi)) if hi > lo => {}
                    _ => break,
                }
            }
        }
        if c.attack {
            let z = last.z.unwrap_or_else(|| heights[self.sector_at(last)].floor) + SHOT_HEIGHT;
            let end = (last.x + SHOT_RANGE * a.cos(), last.y + SHOT_RANGE * a.sin());
            for (_, li) in self.lines_along(last.pos(), end) {
                if self.info.get(&li).is_some_and(|i| i.trigger.shot()) {
                    self.fire(li, obs.tic, How::Shoot);
                }
                match self.opening(li, heights) {
                    Some((lo, hi)) if lo < z && z < hi => {}
                    _ => break,
                }
            }
        }
    }

    /// She moved further in a tic than anyone can walk: a teleport, the one nearest where she was.
    fn teleported(&mut self, last: &Observation, obs: &Observation) {
        let m = Arc::clone(&self.nav.map);
        let mut best: Option<(f64, usize)> = None;
        for (&li, info) in &self.info {
            if info.category != Category::Teleport || !info.players {
                continue;
            }
            let (a, b) = m.line_seg(li);
            let d = segment_point_distance(a, b, last.pos());
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, li));
            }
        }
        let Some((_, li)) = best.filter(|(d, _)| *d <= TELEPORT_NEAR) else {
            self.events.push(format!("tic {}: moved to ({:.0}, {:.0}) all at once", obs.tic, obs.x, obs.y));
            return;
        };
        self.fire(li, obs.tic, How::Walk);
        if !self.teleports.contains_key(&li) {
            self.events.push(format!("tic {}: teleport line {li} goes to ({:.0}, {:.0})", obs.tic, obs.x, obs.y));
        }
        // (the rest of its pad goes there too)
        for other in self.teleport_pad(li) {
            self.teleports.insert(other, obs.pos());
        }
    }

    /// Teleport line `li` and those joined to it end to end with the same tag (the same
    /// destination): the sides of one pad.
    pub fn teleport_pad(&self, li: usize) -> BTreeSet<usize> {
        let m = &self.nav.map;
        let tag = m.linedefs[li].tag;
        let same: Vec<usize> = self.info.iter().filter(|(&o, i)| i.category == Category::Teleport && i.players && m.linedefs[o].tag == tag).map(|(&o, _)| o).collect();
        let mut pad = BTreeSet::from([li]);
        let mut todo = vec![li];
        while let Some(x) = todo.pop() {
            let l = &m.linedefs[x];
            for &o in &same {
                let ol = &m.linedefs[o];
                if !pad.contains(&o) && [ol.v1, ol.v2].iter().any(|v| *v == l.v1 || *v == l.v2) {
                    pad.insert(o);
                    todo.push(o);
                }
            }
        }
        pad
    }

    fn sector_at(&self, obs: &Observation) -> usize {
        self.nav.mesh.poly_at(obs.pos()).map_or(0, |p| self.nav.sector_of_poly(p))
    }

    pub fn special(&self, li: usize) -> Option<&LineSpecial> {
        self.info.get(&li)
    }

    /// A once-only line used this life (its special's gone: the game clears it).
    fn spent(&self, li: usize) -> bool {
        self.info.get(&li).is_some_and(|i| i.trigger.once()) && self.fired.iter().any(|f| f.line == li && f.life == self.lives)
    }

    fn fire(&mut self, li: usize, tic: i64, how: How) {
        let Some(info) = self.info.get(&li).cloned() else { return };
        if info.trigger.once() && self.fired.iter().any(|f| f.line == li && f.life == self.lives) {
            return; // (once only: nothing happens again this life)
        }
        if matches!(info.trigger, Trigger::SR | Trigger::GR) && info.category != Category::Crusher && self.by_line.get(&li).into_iter().flatten().any(|&ti| self.trigger(ti).permanent()) {
            // (Doom turns a switch down on a sector still busy; a repeatable one that would leave
            // it so for good, taken to have, misleads the planner: MAP12's)
            let busy: Vec<usize> = self.targets(li).into_iter().filter(|&t| self.running(t, tic)).collect();
            if let Some(b) = busy.first() {
                self.events.push(format!("tic {tic}: {} line {li} ({}): sector {b} is busy, so nothing", how.name(), info.description));
                return;
            }
        }
        let f = Fired { line: li, tic, how, life: self.lives };
        self.fired.push(f);
        // (what it does, by the heights as she knows them now)
        let now: States = self.nav.movers.initial.iter().enumerate().map(|(s, &h)| vec![self.seen_heights.get(&s).copied().unwrap_or(h)]).collect();
        let ids: Vec<usize> = self.by_line.get(&li).cloned().unwrap_or_default();
        for ti in ids {
            let effects = self.nav.effects_now(self.trigger(ti), &now);
            self.fired_effects.insert((ti, tic), effects);
        }
        self.events.push(format!("tic {tic}: {} line {li} ({})", how.name(), info.description));
        let tk = self.tried_key(li);
        let doubted: BTreeSet<usize> = self.doubts.keys().filter(|(k, _)| *k == tk).map(|(_, s)| *s).collect();
        // (watched to see if it does what it's in doubt of: once a life, and only pressed with
        // what it acts on still a while; the go it gets either way: MAP27's lift switch)
        if !doubted.is_empty() && self.rechecked.insert((tk, self.lives)) && self.targets(li).iter().all(|t| tic - self.moved_at.get(t).copied().unwrap_or(-1_000_000_000) >= BUSY_TICS) {
            self.checking.push((f, doubted));
        }
        if info.trigger.once() {
            let targets = self.targets(li);
            if targets.iter().any(|t| tic - self.moved_at.get(t).copied().unwrap_or(-1_000_000_000) < BUSY_TICS) {
                self.taking.push((f, targets));
            }
        }
    }

    /// Is sector s, as far as she can tell, still in a move that comes back (a door open on its
    /// timer, a lift away): moved lately, by a line of hers that doesn't leave it for good (or
    /// by she doesn't know what), and not back where it started?
    fn running(&self, s: usize, tic: i64) -> bool {
        if tic - self.moved_at.get(&s).copied().unwrap_or(-1_000_000_000) >= BUSY_TICS {
            return false;
        }
        let init = self.nav.movers.initial[s];
        let seen = self.seen_heights.get(&s).copied().unwrap_or(init);
        if (seen.floor - init.floor).abs() < 0.5 && (seen.ceiling - init.ceiling).abs() < 0.5 {
            return false;
        }
        let mut last: Option<(i64, usize)> = None;
        for g in self.fired.iter().filter(|g| g.life == self.lives) {
            for &ti in self.by_line.get(&g.line).into_iter().flatten() {
                if self.trigger(ti).effect(s).is_some() && last.is_none_or(|(lt, _)| g.tic >= lt) {
                    last = Some((g.tic, ti));
                }
            }
        }
        last.is_none_or(|(_, ti)| !self.trigger(ti).permanent())
    }

    /// The sectors a line acts on, by the game's rule: those with its tag; a door line with
    /// none, the door behind it.
    fn targets(&self, li: usize) -> BTreeSet<usize> {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        if l.tag != 0 {
            return m.sectors.iter().filter(|s| s.tag == l.tag).map(|s| s.index).collect();
        }
        m.back_sector(l).into_iter().collect()
    }

    /// Once-only lines set off on sectors that were busy: if nothing's moved since, the game
    /// turned them down (E3M4's door 152).
    fn took(&mut self, tic: i64) {
        let taking = std::mem::take(&mut self.taking);
        let mut still = Vec::new();
        for (f, targets) in taking {
            if targets.iter().any(|t| self.moved_at.get(t).copied().unwrap_or(-1) > f.tic) {
                continue; // (it took)
            }
            if tic - f.tic < TAKE_TICS {
                still.push((f, targets));
                continue;
            }
            if self.already_so(&f, &targets) {
                continue; // (it took: what it moves was already where it puts it)
            }
            if let Some(i) = self.fired.iter().position(|g| *g == f) {
                self.fired.remove(i);
            }
            self.events.push(format!("tic {tic}: line {} didn't take (what it moves was busy): not used up", f.line));
        }
        self.taking = still;
    }

    /// Was everything line f moves left where it is for good by the last line she set off that
    /// moves it (E3M7)? Not if that was a door that closes again (E3M4's) or she doesn't know
    /// what moved it; only for a line with a tag (MAP28's door line).
    fn already_so(&self, f: &Fired, targets: &BTreeSet<usize>) -> bool {
        if self.nav.map.linedefs[f.line].tag == 0 {
            return false;
        }
        for &t in targets {
            let mut last: Option<(i64, usize)> = None;
            for g in &self.fired {
                if g == f || g.life != self.lives || g.tic > f.tic {
                    continue;
                }
                for &ti in self.by_line.get(&g.line).into_iter().flatten() {
                    if self.trigger(ti).effect(t).is_some() && last.is_none_or(|(lt, _)| g.tic >= lt) {
                        last = Some((g.tic, ti));
                    }
                }
            }
            if last.is_none_or(|(_, ti)| !self.trigger(ti).permanent()) {
                return false;
            }
        }
        true
    }

    // --- what she can tell -----------------------------------------------------------------

    /// Mapped lines she can tell are switches or use-doors (or gun targets), not used yet. A
    /// gun target by its look, or by a face unlike the walls either side: MAP16's flame panel in
    /// a notch in the bookcase.
    pub fn interactables(&self) -> Vec<usize> {
        let used: BTreeSet<Mechanism> = self.fired.iter().map(|f| self.tried_key(f.line)).collect();
        self.mapped
            .iter()
            .copied()
            .filter(|&li| {
                self.info.get(&li).is_some_and(|i| {
                    (i.trigger.used() || i.trigger.shot())
                        && !used.contains(&self.tried_key(li))
                        && !i.category.is_exit()
                        && (!self.perceive || self.looks_interactive(li) || self.in_the_way.contains(&li) || self.locked.contains(&li) || (i.trigger.shot() && self.odd(li).0))
                })
            })
            .collect()
    }

    /// Put down something the explorer did (and keep it in the journal). What's new of it, for a
    /// failed press: the lines in the way.
    pub fn note(&mut self, n: Note) -> Vec<usize> {
        let tic = self.last.as_ref().map_or(0, |l| l.tic);
        let out = match &n {
            Note::TriedWall(li, keys) => {
                self.tried_walls.insert(*li, *keys);
                Vec::new()
            }
            Note::Stuck(p, times) => {
                self.stuck_at.insert(*p, *times);
                Vec::new()
            }
            Note::MissedJump(key, times) => {
                self.missed_jumps.insert(key.clone(), *times);
                Vec::new()
            }
            Note::Trap(polys) => {
                self.traps.push(polys.clone());
                Vec::new()
            }
            Note::FailedPress(at, li) => self.failed_press(*at, *li),
        };
        self.journal.push((tic, n));
        out
    }

    /// A switch or door pressed from `at` did nothing: what's in the way of the press (lines
    /// along the way to it, near her) is worth a go itself (MAP19). The lines noted.
    fn failed_press(&mut self, at: Point, li: usize) -> Vec<usize> {
        let (a, b) = self.nav.map.line_seg(li);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let mut out = Vec::new();
        for (_, other) in self.lines_along(at, mid) {
            if other != li && dist(at, mid) <= 2.0 * USE_RANGE && self.in_the_way.insert(other) {
                out.push(other);
            }
        }
        out
    }

    /// Does the line look like a switch, a door or a lift from its front (its textures)?
    pub fn looks_interactive(&self, li: usize) -> bool {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        if l.front == NO_SIDEDEF {
            return false;
        }
        let sd = &m.sidedefs[usize::from(l.front)];
        [&sd.upper, &sd.middle, &sd.lower].iter().any(|t| !t.is_empty() && LOOKS_INTERACTIVE.iter().any(|p| t.to_uppercase().starts_with(p)))
    }

    /// Walls worth pressing use on, as a player would suspect them, best first: (score, line,
    /// where to stand, the wall's middle). Only what she can see counts: the automap drawing a
    /// wall as a two-sided line (a mapper hides a secret one by flagging it), an item she can
    /// see but not get to just behind it, a texture unlike the walls either side, a short piece
    /// of a longer wall; and only walls whose far side she can't get to, faced from floor she
    /// can (one tried for each place behind). `panic`: stuck with nothing at all to do, every
    /// wall round where she is counts, however unsuspicious.
    pub fn suspicious(&mut self, reach: &BTreeSet<usize>, unreached_items: &[Point], panic: bool, unreached_places: Option<&dyn Fn(Point, f64) -> bool>) -> Vec<(f64, usize, Point, Point)> {
        let nav = Rc::clone(&self.nav);
        let (m, mesh) = (&nav.map, &nav.mesh);
        let init = &nav.movers.initial;
        let reach_sectors: BTreeSet<usize> = reach.iter().map(|&p| mesh.poly(p).sector).collect();
        let keys = self.last.as_ref().and_then(|l| l.keys).unwrap_or_default();
        let mut best: BTreeMap<Behind, (f64, usize, Point, Point)> = BTreeMap::new();
        let mut order: Vec<Behind> = Vec::new();
        for &li in &self.mapped {
            if self.tried_walls.get(&li).is_some_and(|k| k.covers(keys)) || self.looks_interactive(li) {
                continue;
            }
            let line = &m.linedefs[li];
            if line.front == NO_SIDEDEF {
                continue;
            }
            let (a, b) = m.line_seg(li);
            let length = dist(a, b);
            if length < 16.0 {
                continue;
            }
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            // (the front is right of v1->v2; use only works from there)
            let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
            let (front, back) = m.sides(li);
            let Some(front) = front else { continue };
            // (something set into a two-sided wall, a grate or a panel: a thing to shoot, from
            // further off: MAP19's)
            let target = back.is_some() && !matches!(m.sidedefs[usize::from(line.front)].middle.as_str(), "" | "-");
            let dists: &[f64] = if target { &[40.0, 96.0, 160.0, 256.0] } else { &[40.0] };
            let Some(stand) = dists.iter().map(|d| (mid.0 + nx * d, mid.1 + ny * d)).find(|&q| mesh.poly_at(q).is_some_and(|p| reach.contains(&p))) else { continue };
            let mut score = if target { 1.0 } else { 0.0 };
            let mut behind = back.map(Behind::Sector);
            let (odd, neighbours) = self.odd(li);
            if let Some(bk) = back {
                // (as she last saw them: a stair since built makes a wall of what was floor)
                let h = |x: usize| self.seen_heights.get(&x).copied().unwrap_or(init[x]);
                let (fh, bh) = (h(front), h(bk));
                // (a niche she can see into but not step into is a wall all the same: bump into it
                // and it's plain; E4M4's niche with the torch, pressed, lowers the lift to a secret)
                let closed = line.blocks_player() || fh.ceiling.min(bh.ceiling) - fh.floor.max(bh.floor) < PLAYER_HEIGHT || bh.floor - fh.floor > MAX_STEP;
                if !closed && odd {
                    // (a way through, but with a face above or below it unlike the walls round it,
                    // where a switch can be: MAP28's red-eyed skull): only that counts
                    behind = None;
                } else if !closed || reach_sectors.contains(&bk) {
                    continue; // (a way through, or somewhere she gets to anyway)
                } else if line.flags & ML_SECRET == 0 {
                    score += 3.0; // (the automap draws it as not a wall)
                }
                if closed && bh.floor - fh.floor > MAX_STEP {
                    score += 1.0; // (a raised platform's side: a lift?)
                }
            }
            let far = (mid.0 - nx * 64.0, mid.1 - ny * 64.0);
            if unreached_items.iter().any(|&it| dist(far, it) < 160.0) {
                score += 2.0;
            }
            // (somewhere she's seen and can't get to, just behind: the way there may be here)
            if unreached_places.is_some_and(|near| near(far, 128.0)) {
                score += 2.0;
            }
            if odd {
                score += 1.5;
                if length <= 96.0 {
                    score += 0.5;
                }
                // (one panel unlike the same panel either side of it: a mark, the way a mapper
                // shows a switch: MAP28's symbol between two plain panels)
                let faces: BTreeSet<String> = neighbours.iter().map(|&n| self.face(&m.linedefs[n])).collect();
                if neighbours.len() >= 2 && faces.len() == 1 {
                    score += 1.5;
                }
            }
            if score < SUSPICION_MIN && !panic {
                continue;
            }
            let key = behind.unwrap_or(Behind::Line(li));
            match best.get(&key) {
                Some(b) if b.0 >= score => {}
                Some(_) => {
                    best.insert(key, (score, li, stand, mid));
                }
                None => {
                    order.push(key);
                    best.insert(key, (score, li, stand, mid));
                }
            }
        }
        // (best first; ties in the order first found, as a stable sort of Python's dict has it)
        let mut out: Vec<(f64, usize, Point, Point)> = order.iter().map(|k| best[k]).collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out
    }

    /// Is the wall's face unlike the walls either side of it (another texture, or the same one
    /// shifted)? And those walls, the ones with a face.
    fn odd(&self, li: usize) -> (bool, Vec<usize>) {
        let m = &self.nav.map;
        let line = &m.linedefs[li];
        let face = self.face(line);
        let neighbours: Vec<usize> = self.wall_neighbours(li).into_iter().filter(|&n| !self.face(&m.linedefs[n]).is_empty()).collect();
        let odd = !neighbours.is_empty() && neighbours.iter().all(|&n| self.face(&m.linedefs[n]) != face || self.shifted(line, &m.linedefs[n]));
        (odd, neighbours)
    }

    /// Does `line` show its texture shifted up or down from `other` beside it, both between the
    /// same sectors (another part of it shown: MAP28's one red-eyed skull of three)?
    fn shifted(&self, line: &Linedef, other: &Linedef) -> bool {
        let m = &self.nav.map;
        if line.front == NO_SIDEDEF || other.front == NO_SIDEDEF {
            return false;
        }
        if (m.front_sector(line), m.back_sector(line)) != (m.front_sector(other), m.back_sector(other)) {
            return false;
        }
        m.sidedefs[usize::from(line.front)].y_offset != m.sidedefs[usize::from(other.front)].y_offset
    }

    /// The texture a wall shows from its front.
    fn face(&self, line: &Linedef) -> String {
        if line.front == NO_SIDEDEF {
            return String::new();
        }
        let sd = &self.nav.map.sidedefs[usize::from(line.front)];
        [&sd.middle, &sd.upper, &sd.lower].iter().find(|t| !t.is_empty() && t.as_str() != "-").map(|t| t.to_uppercase()).unwrap_or_default()
    }

    /// The lines joined to this one at its ends, on the same sector's side.
    fn wall_neighbours(&self, li: usize) -> Vec<usize> {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        let front = m.front_sector(l);
        [l.v1, l.v2].iter().flat_map(|v| self.ends.get(v).into_iter().flatten().copied()).filter(|&n| n != li && m.front_sector(&m.linedefs[n]) == front).collect()
    }

    /// What a line works, as far as she can tell: lines with the same special and tag work the
    /// same thing (the faces of one set of bars), as do door lines on the same door.
    pub fn mechanism(&self, li: usize) -> Mechanism {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        if self.info.get(&li).is_some_and(|i| matches!(i.trigger, Trigger::D1 | Trigger::DR)) {
            return Mechanism::Door(m.back_sector(l).unwrap_or(li));
        }
        if l.tag != 0 {
            return Mechanism::Tag(l.special, l.tag);
        }
        Mechanism::Line(li)
    }

    /// What trying a line tries: its mechanism, but for a line whose every press adds on (a
    /// stair built from where it is, a floor raised by so much) each is worth a press of its
    /// own (MAP21's four switches each build its stair 16 higher).
    pub fn tried_key(&self, li: usize) -> Mechanism {
        if self.info.get(&li).is_some_and(|i| i.category == Category::Stairs || adds_on(&i.description)) {
            return Mechanism::Line(li);
        }
        self.mechanism(li)
    }

    /// Exit lines she can tell are there: mapped ones, or walk-over ones in a sector she has
    /// seen (they're seldom drawn themselves; the exit room is).
    pub fn exits(&self) -> Vec<usize> {
        let m = &self.nav.map;
        self.info
            .iter()
            .filter(|(&i, info)| {
                let (f, b) = m.sides(i);
                info.category.is_exit() && (self.mapped.contains(&i) || (info.trigger.walked() && [f, b].into_iter().flatten().any(|s| self.sectors.contains(&s))))
            })
            .map(|(&i, _)| i)
            .collect()
    }

    /// The keys a line she has seen needs (told on sight).
    pub fn lock(&self, li: usize) -> Lock {
        self.info.get(&li).map_or(Lock::None, |i| i.lock)
    }

    /// Teleport lines she can tell are there (mapped, or in a sector she has seen).
    pub fn teleporters(&self) -> Vec<usize> {
        let m = &self.nav.map;
        self.info
            .iter()
            .filter(|(&i, info)| {
                let (f, b) = m.sides(i);
                info.category == Category::Teleport && info.players && (self.mapped.contains(&i) || [f, b].into_iter().flatten().any(|s| self.sectors.contains(&s)))
            })
            .map(|(&i, _)| i)
            .collect()
    }

    /// The map's things in sectors she has seen (at the skill played).
    pub fn known_things(&self) -> Vec<crate::map::Thing> {
        let nav = &self.nav;
        nav.map.things.iter().filter(|t| t.in_skill(nav.skill) && nav.mesh.poly_at((t.x, t.y)).is_some_and(|p| self.sectors.contains(&nav.sector_of_poly(p)))).copied().collect()
    }

    /// The last tic a line was first fired whose effect hasn't been seen at all, if any.
    pub fn unexplained_since(&self) -> Option<i64> {
        let explained: BTreeSet<i64> = self.learned.iter().filter(|(_, s)| !s.is_empty()).map(|(&i, _)| self.trigger(i).line).collect();
        let mut first: BTreeMap<usize, i64> = BTreeMap::new();
        for f in &self.fired {
            first.entry(f.line).or_insert(f.tic);
        }
        first.iter().filter(|(&l, _)| !explained.contains(&(l as i64))).map(|(_, &t)| t).max()
    }

    /// Known sectors not closed on the automap yet: sector -> its lines that would be drawn if
    /// looked at (walls, steps, windows) but haven't been.
    /// (In the order the sectors first come up, line by line.)
    pub fn unseen_walls(&self) -> Vec<(usize, Vec<usize>)> {
        let m = &self.nav.map;
        let mut out: Vec<(usize, Vec<usize>)> = Vec::new();
        for &li in &self.drawable {
            if self.mapped.contains(&li) {
                continue;
            }
            let (f, b) = m.sides(li);
            for s in [f, b].into_iter().flatten() {
                if self.sectors.contains(&s) {
                    match out.iter_mut().find(|(x, _)| *x == s) {
                        Some((_, v)) => v.push(li),
                        None => out.push((s, vec![li])),
                    }
                }
            }
        }
        out
    }

    /// The level started again (after dying): what she knows of the map stays; what was so this
    /// time goes. The heights she knows go back to the map's.
    pub fn restart(&mut self) {
        let m = Arc::clone(&self.nav.map);
        for &s in &self.sectors {
            self.seen_heights.insert(s, Heights { floor: m.sectors[s].floor, ceiling: m.sectors[s].ceiling });
        }
        self.suit_until = None;
        self.changed_floor.clear();
        self.last = None;
        self.standing = None;
        self.hurt_on = None;
        self.live.clear();
        self.moving.clear();
        self.moved_at.clear();
        self.taking.clear();
        self.lives += 1;
        self.events.push(format!("--- life {}: the level from the start, knowing what she knew", self.lives));
    }

    /// What standing on sector s's floor (None: not on a floor) tells her about it.
    fn feel(&mut self, s: Option<usize>, obs: &Observation) {
        let Some(s) = s else {
            self.standing = None;
            return;
        };
        if !self.mortal {
            if !self.felt.contains_key(&s) {
                self.learn_floor(s, self.nav.damage[s] != 0, obs.tic);
            }
            return;
        }
        if self.standing.is_none_or(|(x, _)| x != s) {
            self.standing = Some((s, obs.tic));
        }
        let hurt = self.last.as_ref().is_some_and(|last| matches!((obs.health, last.health), (Some(now), Some(before)) if now < before));
        if hurt {
            // (a floor hurts on the level's clock, every 32 tics: hurt again on the same floor a
            // multiple of that later, it's the floor; once alone may be a monster's shot)
            if let Some((ps, pt)) = self.hurt_on {
                let gap = obs.tic - pt;
                if ps == s && 0 < gap && gap <= FLOOR_CONFIRM && gap % FLOOR_PERIOD == 0 {
                    self.learn_floor(s, true, obs.tic);
                }
            }
            self.hurt_on = Some((s, obs.tic));
        } else {
            let since = self.standing.map_or(obs.tic, |x| x.1).max(self.hurt_on.filter(|h| h.0 == s).map_or(0, |h| h.1));
            if obs.tic - since > FELT_TICS && self.felt.get(&s) != Some(&false) && !self.protected(obs.tic) {
                self.learn_floor(s, false, obs.tic);
            }
        }
    }

    fn learn_floor(&mut self, s: usize, hurts: bool, tic: i64) {
        if self.felt.get(&s) == Some(&hurts) {
            return;
        }
        self.felt.insert(s, hurts);
        if hurts != self.looks_damaging(s) {
            self.events.push(format!("tic {tic}: sector {s} {}", if hurts { "hurts after all" } else { "doesn't hurt after all" }));
        }
    }

    /// Is she wearing a radiation suit (one picked up in the last minute)?
    pub fn protected(&self, tic: i64) -> bool {
        self.suit_until.is_some_and(|u| tic < u)
    }

    /// A floor that looks like it'd hurt (its flat is a liquid).
    pub fn looks_damaging(&self, s: usize) -> bool {
        let tex = self.nav.map.sectors[s].floor_tex.to_uppercase();
        HAZARD_FLATS.iter().any(|h| tex.starts_with(h))
    }

    /// Does she take sector s's floor to hurt: as found out by standing on it, else by its look.
    pub fn damaging(&self, s: usize) -> bool {
        if let Some(&c) = self.changed_floor.get(&s) {
            return c; // (seen changed since: MAP08's walkway gone to nukage)
        }
        self.felt.get(&s).copied().unwrap_or_else(|| self.looks_damaging(s))
    }

    /// A sound from something she knows moves and set off a moment before (a door closing behind
    /// her, a lift coming back), or from right beside her: nothing new. One she set off long
    /// before moving again is news (E1M6's door that shut on her opened again 30 s later).
    pub fn expected(&self, h: &Heard) -> bool {
        if h.distance <= NEAR_SOUND {
            return true;
        }
        let mut fired_at: BTreeMap<usize, i64> = BTreeMap::new();
        for f in &self.fired {
            fired_at.insert(f.line, f.tic);
        }
        for (t, sectors) in self.known_triggers() {
            let Some(&tic) = t.linedef().and_then(|li| fired_at.get(&li)) else { continue };
            if !(0..=EXPECTED_FOR).contains(&(h.tic - tic)) {
                continue;
            }
            if sectors.iter().filter_map(|s| self.sound_origin.get(s)).any(|&o| dist(o, h.origin) <= EXPECTED_RADIUS) {
                return true;
            }
        }
        false
    }

    /// Where she can plan to go: the parts of the map she's seen into or stood in.
    pub fn known_polys(&self) -> BTreeSet<usize> {
        self.seen_polys.union(&self.visited).copied().collect()
    }

    /// Triggers whose effect on some sectors she has learned, with those sectors; also the
    /// other lines of the same mechanism as far as they're mapped, which she takes to do the
    /// same.
    pub fn known_triggers(&self) -> Vec<(TriggerLine, BTreeSet<usize>)> {
        let mut by_mechanism: BTreeMap<Mechanism, BTreeSet<usize>> = BTreeMap::new();
        for (&i, sectors) in &self.learned {
            let t = self.trigger(i);
            if let (false, Some(li)) = (sectors.is_empty(), t.linedef()) {
                by_mechanism.entry(self.mechanism(li)).or_default().extend(sectors.iter().copied());
            }
        }
        let mut out = Vec::new();
        for t in &self.nav.triggers {
            let Some(li) = t.linedef() else { continue };
            if let Some(l) = self.learned.get(&t.id).filter(|l| !l.is_empty()) {
                out.push((t.clone(), l.clone()));
            } else if self.mapped.contains(&li) {
                if let Some(same) = by_mechanism.get(&self.mechanism(li)) {
                    let both: BTreeSet<usize> = t.effects.iter().map(|(s, _)| *s).filter(|s| same.contains(s)).collect();
                    if !both.is_empty() {
                        out.push((t.clone(), both));
                    }
                }
            }
        }
        out
    }

    pub fn summary(&self) -> String {
        let m = &self.nav.map;
        format!(
            "{}/{} lines, {}/{} sectors, {} fired, {} effects learned ({} in doubt), {} untried switches/doors, {} exits seen",
            self.mapped.len(),
            m.linedefs.len(),
            self.sectors.len(),
            m.sectors.len(),
            self.fired.len(),
            self.learned.values().map(BTreeSet::len).sum::<usize>(),
            self.doubts.len(),
            self.interactables().len(),
            self.exits().len()
        )
    }

    /// Sectors last tic, as the knowledge saw them move.
    pub fn moved_at(&self, s: usize) -> Option<i64> {
        self.moved_at.get(&s).copied()
    }
}

/// What a suspected wall hides: the sector behind it, or (none) the line itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Behind {
    Sector(usize),
    Line(usize),
}

fn segment_point_distance(a: Point, b: Point, p: Point) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    dist(p, (a.0 + t * dx, a.1 + t * dy))
}

/// Where along a-b it crosses p-q (0..1), or None.
fn intersect(a: Point, b: Point, p: Point, q: Point) -> Option<f64> {
    let (rx, ry) = (b.0 - a.0, b.1 - a.1);
    let (sx, sy) = (q.0 - p.0, q.1 - p.1);
    let den = rx * sy - ry * sx;
    if den == 0.0 {
        return None;
    }
    let t = ((p.0 - a.0) * sy - (p.1 - a.1) * sx) / den;
    let u = ((p.0 - a.0) * ry - (p.1 - a.1) * rx) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}
