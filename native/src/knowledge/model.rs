//! What Evil knows about the level: only what she has seen and what she has learned by doing.
//!
//! Seeing follows the automap: a linedef is mapped once the renderer draws any part of it
//! (`sight`), and the sectors either side of a mapped line are known, with what's in them.
//! What lines do is learned by using them: a line she sets off (pressing it, walking over it,
//! shooting it) is fired, and its effect on a sector is learned when she sees that sector move,
//! then or later. What a line *is* can be told on sight from its special type (not its
//! textures, which custom WADs draw however they like): switches and use-doors, exits, the keys
//! a lock needs, teleporters.

mod learning;
mod perception;
mod walls;
pub use walls::{Signs, Wall, SECRET_MIN};

use crate::geom::{cells, dist, Point};
use crate::map::ML_MAPPED;
use crate::motor::Observation;
use crate::movers::Effects;
use crate::nav::Heights;
use crate::navigator::{Navigator, TriggerLine};
use crate::sight::Sight;
use crate::specials::{classify_line, Category, Keys, LineSpecial, Lock, Trigger};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

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
    /// sector -> the smaller side of its bounding box (thin: a door)
    narrowness: Vec<f64>,
    /// a doubted line pressed again, watched: (fired, sectors in doubt)
    checking: Vec<(Fired, BTreeSet<usize>)>,
    /// where each sector's sounds come from: the middle of its bounding box (P_GroupLines)
    pub sound_origin: BTreeMap<usize, Point>,
    /// lines that get drawn when looked at (not two-sided ones alike on both sides)
    drawable: Vec<usize>,
    /// every sector's heights last tic
    live: Vec<Heights>,
    moving: BTreeSet<usize>,
    /// sector -> the tic it was last heard stopping
    stopped_at: BTreeMap<usize, i64>,
    /// sector -> the tic it was last heard moving
    moved_at: BTreeMap<usize, i64>,
    /// once-only lines set off: did they take? (with what they act on, and its heights then)
    taking: Vec<(Fired, BTreeSet<usize>, BTreeMap<usize, Heights>)>,
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
                        (sf.floor, sf.ceiling, &sf.floor_tex, &sf.ceiling_tex, sf.light) != (sb.floor, sb.ceiling, &sb.floor_tex, &sb.ceiling_tex, sb.light)
                            || !matches!(m.sidedefs[usize::from(l.front)].middle.as_str(), "-" | "")
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
        let mut boxes = vec![(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY); m.sectors.len()];
        for (i, l) in m.linedefs.iter().enumerate() {
            for v in [l.v1, l.v2] {
                ends.entry(v).or_default().push(i);
            }
            let (a, b) = m.line_seg(i);
            let (f, bk) = m.sides(i);
            for s in [f, bk].into_iter().flatten() {
                let bx = &mut boxes[s];
                *bx = (bx.0.min(a.0).min(b.0), bx.1.min(a.1).min(b.1), bx.2.max(a.0).max(b.0), bx.3.max(a.1).max(b.1));
            }
        }
        let narrowness = boxes.iter().map(|b| (b.2 - b.0).min(b.3 - b.1)).collect();
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
            narrowness,
            hurt_on: None,
            ends,
            checking: Vec::new(),
            sound_origin,
            drawable,
            live: Vec::new(),
            moving: BTreeSet::new(),
            stopped_at: BTreeMap::new(),
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

    // --- doing ------------------------------------------------------------------------------

    /// Linedefs within `r` of `p`, by number.
    fn lines_near(&self, p: Point, r: f64) -> Vec<usize> {
        let m = &self.nav.map;
        let mut out: BTreeSet<usize> = BTreeSet::new();
        for cell in cells(p.0 - r, p.1 - r, p.0 + r, p.1 + r) {
            for &li in self.nav.mesh.line_grid.get(&cell).into_iter().flatten() {
                let (a, b) = m.line_seg(li);
                if crate::geom::segment_distance(a, b, p, p) <= r {
                    out.insert(li);
                }
            }
        }
        out.into_iter().collect()
    }

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

    /// P_UseLines from `pos` facing `angle` (degrees): the first line along the use trace with
    /// a special she knows of, unless something shut is in the way first. A once-only line used
    /// this life has no special any more: the trace goes past it (E2M7).
    pub fn use_hits(&self, pos: Point, angle: f64, heights: &[Heights]) -> Option<usize> {
        let a = angle.to_radians();
        let end = (pos.0 + USE_RANGE * a.cos(), pos.1 + USE_RANGE * a.sin());
        for (_, li) in self.lines_along(pos, end) {
            if self.info.contains_key(&li) && !self.spent(li) {
                return Some(li);
            }
            match self.opening(li, heights) {
                Some((lo, hi)) if hi > lo => {}
                _ => return None,
            }
        }
        None
    }

    /// Is `li` an exit she knows of?
    pub fn is_exit(&self, li: usize) -> bool {
        self.info.get(&li).is_some_and(|i| i.category.is_exit())
    }

    fn front(&self, li: usize, x: f64, y: f64) -> bool {
        let (a, b) = self.nav.map.line_seg(li);
        (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0) <= 0.0
    }

    fn opening(&self, li: usize, heights: &[Heights]) -> Option<(f64, f64)> {
        let (Some(f), Some(b)) = self.nav.map.sides(li) else { return None };
        Some((heights[f].floor.max(heights[b].floor), heights[f].ceiling.min(heights[b].ceiling)))
    }

    /// Teleport line `li` and those joined to it end to end with the same tag (the same
    /// destination): the sides of one pad.
    pub fn teleport_pad(&self, li: usize) -> BTreeSet<usize> {
        let m = &self.nav.map;
        let tag = m.linedefs[li].tag;
        let same: Vec<usize> =
            self.info.iter().filter(|(&o, i)| i.category == Category::Teleport && i.players && m.linedefs[o].tag == tag).map(|(&o, _)| o).collect();
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

    pub fn special(&self, li: usize) -> Option<&LineSpecial> {
        self.info.get(&li)
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
                        && (!self.perceive
                            || self.looks_interactive(li)
                            || self.door_shaped(li)
                            || self.in_the_way.contains(&li)
                            || self.locked.contains(&li)
                            || (i.trigger.shot() && self.odd(li).0))
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
                info.category.is_exit()
                    && (self.mapped.contains(&i) || (info.trigger.walked() && [f, b].into_iter().flatten().any(|s| self.sectors.contains(&s))))
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
                info.category == Category::Teleport
                    && info.players
                    && (self.mapped.contains(&i) || [f, b].into_iter().flatten().any(|s| self.sectors.contains(&s)))
            })
            .map(|(&i, _)| i)
            .collect()
    }

    /// The map's things in sectors she has seen (at the skill played).
    pub fn known_things(&self) -> Vec<crate::map::Thing> {
        let nav = &self.nav;
        nav.map
            .things
            .iter()
            .filter(|t| t.in_skill(nav.skill) && nav.mesh.poly_at((t.x, t.y)).is_some_and(|p| self.sectors.contains(&nav.sector_of_poly(p))))
            .copied()
            .collect()
    }

    /// The last tic a line was first fired whose effect hasn't been seen at all, if any (or
    /// pressed or shot again: a lift's down only a while, and pressed to be watched, MAP21's
    /// switch 172, its lift seen since the first press, never looked for after the next).
    pub fn unexplained_since(&self) -> Option<i64> {
        // (and a teleport she's been through does what it does: takes her there, nothing to look
        // round for; MAP29's line 516, her first time through it setting off 25 rechecks)
        let explained: BTreeSet<i64> =
            self.learned.iter().filter(|(_, s)| !s.is_empty()).map(|(&i, _)| self.trigger(i).line).chain(self.teleports.keys().map(|&l| l as i64)).collect();
        let mut first: BTreeMap<usize, i64> = BTreeMap::new();
        for f in &self.fired {
            if f.how == How::Walk {
                first.entry(f.line).or_insert(f.tic);
            } else {
                first.insert(f.line, f.tic);
            }
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
        self.stopped_at.clear();
        self.taking.clear();
        self.lives += 1;
        self.events.push(format!("--- life {}: the level from the start, knowing what she knew", self.lives));
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

    /// What she knows that a route could turn on: the summary but for the lines fired (a wall
    /// pressed that did nothing is no news to the planner)
    pub fn known_summary(&self) -> String {
        let m = &self.nav.map;
        format!(
            "{}/{} lines, {}/{} sectors, {} effects learned ({} in doubt), {} untried switches/doors, {} exits seen",
            self.mapped.len(),
            m.linedefs.len(),
            self.sectors.len(),
            m.sectors.len(),
            self.learned.values().map(BTreeSet::len).sum::<usize>(),
            self.doubts.len(),
            self.interactables().len(),
            self.exits().len()
        )
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

    /// Has sector s been still, after moving, for at least `tics` (by the sounds of its starts and
    /// stops)? Not if it's moving, or never seen to move.
    /// Is sector s moving now, as she hears it?
    pub fn is_moving(&self, s: usize) -> bool {
        self.moving.contains(&s)
    }

    pub fn stopped_for(&self, s: usize, tic: i64, tics: i64) -> bool {
        !self.moving.contains(&s) && self.stopped_at.get(&s).is_some_and(|&t| tic - t >= tics)
    }
}

/// What a suspected wall hides: the sector behind it, or (none) the line itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Behind {
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
