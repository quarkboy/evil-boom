//! Recognizing interactive surfaces and ranking suspicious walls from visible evidence.

use super::{Behind, Knowledge, LOOKS_INTERACTIVE, SUSPICION_MIN};
use crate::geom::{dist, Point};
use crate::map::{Linedef, ML_DONTPEGBOTTOM, ML_DONTPEGTOP, ML_SECRET, NO_SIDEDEF};
use crate::navigator::{MAX_STEP, PLAYER_HEIGHT};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// a shut sector no thicker than this behind a line is a door
const DOOR_THICKNESS: f64 = 32.0;

/// a line pressed this lately (tics) isn't a wall to try: what it does may still be going on
const PRESSED_LATELY: i64 = 10 * 35;

/// how far round a wall with nothing joined to it its fellows are looked for (`odd`)
const AROUND: f64 = 128.0;

/// a wall this short joined to another is a jamb, the side of a recess (`odd`)
const JAMB: f64 = 24.0;

/// an exit sign this near the spot in front of a wall marks it
const EXIT_SIGN_NEAR: f64 = 64.0;

/// how far behind a wall a blank on her map is looked for...
const BLANK_DEPTH: f64 = 96.0;

/// ...and how far round it the walls she's mapped must close it in
const BLANK_REACH: f64 = 384.0;

impl Knowledge {
    /// Does the line look like a switch, a door or a lift from its front (its textures)?
    pub fn looks_interactive(&self, li: usize) -> bool {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        if l.front == NO_SIDEDEF {
            return false;
        }
        let sd = &m.sidedefs[usize::from(l.front)];
        [&sd.upper, &sd.middle, &sd.lower].iter().any(|t| {
            if t.is_empty() {
                return false;
            }
            let upper = t.to_uppercase();
            LOOKS_INTERACTIVE.iter().any(|p| upper.starts_with(p))
        })
    }

    /// Walls worth pressing use on, as a player would suspect them, best first: (score, line,
    /// where to stand, the wall's middle). Only what she can see counts: the automap drawing a
    /// wall as a two-sided line (a mapper hides a secret one by flagging it), an item she can
    /// see but not get to just behind it, a texture unlike the walls either side, a short piece
    /// of a longer wall; and only walls whose far side she can't get to, faced from floor she
    /// can (one tried for each place behind). `panic`: stuck with nothing at all to do, every
    /// wall round where she is counts, however unsuspicious.
    pub fn suspicious(
        &mut self,
        reach: &BTreeSet<usize>,
        unreached_items: &[Point],
        panic: bool,
        unreached_places: Option<&dyn Fn(Point, f64) -> bool>,
    ) -> Vec<(f64, usize, Point, Point)> {
        let ctx = self.wall_context(reach, unreached_items, unreached_places);
        let mut best: BTreeMap<Behind, (f64, usize, Point, Point)> = BTreeMap::new();
        let mut order: Vec<Behind> = Vec::new();
        for &li in &self.mapped {
            let Wall::Suspect { signs, behind, stand, aim } = self.wall_signs(li, &ctx) else { continue };
            let score = signs.score();
            if score < SUSPICION_MIN && !panic {
                continue;
            }
            let key = behind.unwrap_or(Behind::Line(li));
            match best.get(&key) {
                Some(b) if b.0 >= score => {}
                Some(_) => {
                    best.insert(key, (score, li, stand, aim));
                }
                None => {
                    order.push(key);
                    best.insert(key, (score, li, stand, aim));
                }
            }
        }
        // (best first; ties in the order first found, as a stable sort of Python's dict has it)
        let mut out: Vec<(f64, usize, Point, Point)> = order.iter().map(|k| best[k]).collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out
    }

    /// Every wall she's mapped as `suspicious` sees it, whatever its score: its signs, or why it's
    /// no wall to try at all (for the survey of the maps' secrets, `evil-boom secret-survey`).
    pub fn wall_survey(&self, reach: &BTreeSet<usize>, unreached_items: &[Point], unreached_places: Option<&dyn Fn(Point, f64) -> bool>) -> Vec<(usize, Wall)> {
        let ctx = self.wall_context(reach, unreached_items, unreached_places);
        self.mapped.iter().map(|&li| (li, self.wall_signs(li, &ctx))).collect()
    }

    /// What every wall's weighed against: where she can get to, what she's seen.
    fn wall_context<'a>(&self, reach: &'a BTreeSet<usize>, unreached_items: &'a [Point], unreached_places: Option<&'a dyn Fn(Point, f64) -> bool>) -> WallContext<'a> {
        let nav = Rc::clone(&self.nav);
        let m = &nav.map;
        let mesh = &nav.mesh;
        let init = &nav.movers.initial;
        let reach_sectors: BTreeSet<usize> = reach.iter().map(|&p| mesh.poly(p).sector).collect();
        let keys = self.last.as_ref().and_then(|l| l.keys).unwrap_or_default();
        let known = self.known_polys();
        // (the exit signs she's seen: the middles of lines showing one)
        let signs: Vec<Point> = self
            .mapped
            .iter()
            .filter(|&&li| {
                let l = &m.linedefs[li];
                [l.front, l.back].iter().filter(|&&sd| sd != NO_SIDEDEF).any(|&sd| {
                    let d = &m.sidedefs[usize::from(sd)];
                    [&d.upper, &d.middle, &d.lower].iter().any(|t| t.eq_ignore_ascii_case("EXITSIGN"))
                })
            })
            .map(|&li| {
                let (a, b) = m.line_seg(li);
                ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
            })
            .collect();
        // (the heights as she last saw them)
        let heights: Vec<(f64, f64)> = (0..init.len()).map(|x| self.seen_heights.get(&x).copied().unwrap_or(init[x])).map(|h| (h.floor, h.ceiling)).collect();
        // (not a line pressed a moment ago, its work still going on: MAP10's door, pressed open as
        // it shut, then tried "as a wall" and pressed shut again, and she with it in the closet)
        let now = self.last.as_ref().map_or(0, |l| l.tic);
        let lately: BTreeSet<usize> = self.fired.iter().filter(|f| now - f.tic < PRESSED_LATELY).map(|f| f.line).collect();
        WallContext { reach, reach_sectors, keys, known, signs, heights, lately, unreached_items, unreached_places }
    }

    /// One wall's signs, or why it's none to try (`suspicious`, the rubric).
    fn wall_signs(&self, li: usize, ctx: &WallContext) -> Wall {
        let nav = Rc::clone(&self.nav);
        let (m, mesh) = (&nav.map, &nav.mesh);
        let init = &nav.movers.initial;
        let WallContext { reach, reach_sectors, keys, known, heights, lately, unreached_items, unreached_places, .. } = ctx;
        let (reach, keys, unreached_items, unreached_places) = (*reach, *keys, *unreached_items, *unreached_places);
        if self.tried_walls.get(&li).is_some_and(|k| k.covers(keys)) {
            return Wall::Skip("tried");
        }
        if self.looks_interactive(li) {
            return Wall::Skip("looks interactive");
        }
        if lately.contains(&li) {
            return Wall::Skip("pressed lately");
        }
        let line = &m.linedefs[li];
        if line.front == NO_SIDEDEF {
            return Wall::Skip("no front side");
        }
        let (a, b) = m.line_seg(li);
        let length = dist(a, b);
        if length < 16.0 {
            return Wall::Skip("too short");
        }
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        // (the front is right of v1->v2; use only works from there)
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length);
        let (front, back) = m.sides(li);
        let Some(front) = front else { return Wall::Skip("no front side") };
        // (something set into a two-sided wall, a grate or a panel: a thing to shoot, from
        // further off: MAP19's)
        let target = back.is_some() && !matches!(m.sidedefs[usize::from(line.front)].middle.as_str(), "" | "-");
        let dists: &[f64] = if target { &[40.0, 96.0, 160.0, 256.0] } else { &[40.0] };
        // (in front of its middle; else, the box not fitting there, of another part of it:
        // MAP11's secret door, a column of pillars in front of its middle; facing that part)
        // (and on the floor she'd be on there, not up on something beside it the box overlaps:
        // MAP15's lift crate, pressed from beside the crate next to it, picked a spot with the
        // box over that one's top, 128 up, and was never offered)
        let fits = |q: Point| {
            mesh.poly_at(q).is_some_and(|p| {
                reach.contains(&p) && {
                    let (clear, floor, ceiling) = nav.jump_lab.fits_at_heights(q, heights);
                    clear && ceiling - floor >= PLAYER_HEIGHT && floor - heights[mesh.poly(p).sector].0 <= MAX_STEP
                }
            })
        };
        let Some(mut stand) = dists.iter().map(|d| (mid.0 + nx * d, mid.1 + ny * d)).find(|&q| mesh.poly_at(q).is_some_and(|p| reach.contains(&p))) else {
            return Wall::Skip("nowhere to stand in reach");
        };
        let mut aim = mid;
        if !fits(stand) {
            let along = [0.25, 0.75, 0.1, 0.9].iter().map(|t| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            if let Some((q, w)) = along.flat_map(|w| dists.iter().chain(&[24.0]).map(move |d| ((w.0 + nx * d, w.1 + ny * d), w))).find(|&(q, _)| fits(q)) {
                (stand, aim) = (q, w);
            }
        }
        let mut signs = Signs { target: if target { 1.0 } else { 0.0 }, ..Signs::default() };
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
            } else if !closed {
                return Wall::Skip("a way through");
            } else if reach_sectors.contains(&bk) {
                return Wall::Skip("reaches behind anyway");
            } else if line.flags & ML_SECRET == 0 {
                signs.not_wall = 3.0; // (the automap draws it as not a wall)
            }
            if closed && bh.floor - fh.floor > MAX_STEP {
                signs.platform = 1.0; // (a raised platform's side: a lift?)
            }
        }
        let far = (mid.0 - nx * 64.0, mid.1 - ny * 64.0);
        if unreached_items.iter().any(|&it| dist(far, it) < 160.0) {
            signs.item_behind = 2.0;
            // (and a gap under it, or over it, the item seen through: a door left ajar, as
            // good a sign as the automap's: E4M5's berserk, under a wall 8 off the floor)
            let h = |x: usize| self.seen_heights.get(&x).copied().unwrap_or(init[x]);
            if back.is_some_and(|bk| {
                let (fh, bh) = (h(front), h(bk));
                fh.ceiling.min(bh.ceiling) > fh.floor.max(bh.floor)
            }) {
                signs.item_gap = 3.0;
            }
        }
        // (somewhere she's seen and can't get to, just behind: the way there may be here)
        if unreached_places.is_some_and(|near| near(far, 128.0)) {
            signs.place_behind = 2.0;
        }
        // (an exit sign hung in front of it: the way out's through here, as a player reads
        // it; MAP05's yellow door, its face the walls' own TEKBRON2, the sign 50 in front)
        let ahead = (mid.0 + nx * 48.0, mid.1 + ny * 48.0);
        if ctx.signs.iter().any(|&q| dist(q, ahead) < EXIT_SIGN_NEAR) {
            signs.exit_sign = 3.0;
        }
        // (a blank on her map behind it, walled round by what she has mapped: room for
        // somewhere she's never seen, as a player reads the automap: E1M6's secret room
        // behind the south wall of the alcove)
        let deep = (mid.0 - nx * BLANK_DEPTH, mid.1 - ny * BLANK_DEPTH);
        if !mesh.poly_at(deep).is_some_and(|p| known.contains(&p)) && self.walled_round(deep) {
            signs.blank_behind = 2.0;
        }
        if odd {
            signs.odd = 1.5;
            if length <= 96.0 {
                signs.short = 0.5;
            }
            // (one panel unlike the same panel either side of it: a mark, the way a mapper
            // shows a switch: MAP28's symbol between two plain panels)
            let faces: BTreeSet<String> = neighbours.iter().map(|&n| self.face(&m.linedefs[n])).collect();
            if neighbours.len() >= 2 && faces.len() == 1 {
                signs.panel = 1.5;
            }
        }
        Wall::Suspect { signs, behind, stand, aim }
    }

    /// Is `p` walled round by lines she has mapped, every way (up, down, left, right) within
    /// BLANK_REACH? (Not out past the edge of the map as she knows it.)
    fn walled_round(&self, p: Point) -> bool {
        [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
            .iter()
            .all(|(dx, dy)| self.lines_along(p, (p.0 + dx * BLANK_REACH, p.1 + dy * BLANK_REACH)).iter().any(|(_, li)| self.mapped.contains(li)))
    }

    /// Does the line look like a door by its shape: in front of a sector shut flat, thin as a
    /// door is, with a face unlike the walls either side (MAP14's exit door, wooden panels at the
    /// back of a niche in stone, a texture no list of door textures has)?
    pub fn door_shaped(&self, li: usize) -> bool {
        let m = &self.nav.map;
        let (Some(_), Some(b)) = m.sides(li) else { return false };
        let h = self.seen_heights.get(&b).copied().unwrap_or(self.nav.movers.initial[b]);
        h.ceiling <= h.floor && self.narrowness[b] <= DOOR_THICKNESS && self.odd(li).0
    }

    /// Does the wall stand out from those round it (`odd`): a mark, as a player reads it?
    pub fn marked(&self, li: usize) -> bool {
        self.odd(li).0
    }

    /// Is the wall's face unlike the walls either side of it (another texture, or the same one
    /// shifted)? And those walls, the ones with a face.
    pub(super) fn odd(&self, li: usize) -> (bool, Vec<usize>) {
        let m = &self.nav.map;
        let line = &m.linedefs[li];
        let face = self.face(line);
        // (a short jamb either side, the sides of a recess the wall's set back in: the walls past
        // them instead, what the panel's set into; MAP19's grey door between two satyr faces in
        // a brown wall, its jambs grey as the door)
        let neighbours: Vec<usize> = self
            .wall_neighbours(li)
            .into_iter()
            .flat_map(|n| {
                let (a, b) = m.line_seg(n);
                if m.linedefs[n].back == NO_SIDEDEF && dist(a, b) <= JAMB {
                    self.wall_neighbours(n).into_iter().filter(|&x| x != li).collect()
                } else {
                    vec![n]
                }
            })
            .filter(|&n| !self.face(&m.linedefs[n]).is_empty())
            .collect();
        if neighbours.is_empty() && !face.is_empty() {
            // (nothing joined to it on its side, only ways through either end: the walls round
            // it instead; E2M4's mossy door at the end of a dark corridor of plain stone)
            let (a, b) = m.line_seg(li);
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let around: Vec<usize> = self
                .lines_near(mid, AROUND)
                .into_iter()
                .filter(|&n| n != li && m.linedefs[n].back == NO_SIDEDEF && self.mapped.contains(&n) && !self.face(&m.linedefs[n]).is_empty())
                .collect();
            let odd = around.len() >= 2 && around.iter().all(|&n| self.face(&m.linedefs[n]) != face);
            return (odd, Vec::new());
        }
        let odd = !neighbours.is_empty() && neighbours.iter().all(|&n| self.face(&m.linedefs[n]) != face || self.misaligned(line, &m.linedefs[n]));
        (odd, neighbours)
    }

    /// Does `line`'s face sit at another height from `other`'s, the same texture (its rows not
    /// lining up with those beside it)? A face is placed by where the game anchors it, and
    /// its offset: a door's upper hangs from the door's own ceiling, the wall's from the room's
    /// (E2M2's door in the round room of TEKWALL1, its panels out of line; MAP28's one red-eyed
    /// skull of three, its offset another).
    fn misaligned(&self, line: &Linedef, other: &Linedef) -> bool {
        // (only a door's face, shut: a plain wall's face set off from the next is too common a
        // thing to go by, and a round of walls was spent on them before E1M6's that mattered)
        let h = |x: usize| self.seen_heights.get(&x).copied().unwrap_or(self.nav.movers.initial[x]);
        let door = self.nav.map.back_sector(line).is_some_and(|b| {
            let hb = h(b);
            hb.ceiling <= hb.floor
        });
        match (door, self.face_phase(line), self.face_phase(other)) {
            (true, Some(a), Some(b)) => a != b,
            _ => self.shifted(line, other),
        }
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

    /// Where a wall's face starts, up and down, wrapped by its texture's height (at the heights
    /// she last saw): as R_StoreWallRange anchors it, with the sidedef's offset.
    fn face_phase(&self, line: &Linedef) -> Option<i64> {
        if line.front == NO_SIDEDEF {
            return None;
        }
        let m = &self.nav.map;
        let sd = &m.sidedefs[usize::from(line.front)];
        let h = |x: usize| self.seen_heights.get(&x).copied().unwrap_or(self.nav.movers.initial[x]);
        let front = h(sd.sector);
        let named = |t: &str| !t.is_empty() && t != "-";
        let height = |t: &str| m.texture_heights.get(&t.to_uppercase()).copied().filter(|&x| x > 0);
        let top = match m.back_sector(line).map(h) {
            None => {
                let tex = height(&sd.middle)?;
                if line.flags & ML_DONTPEGBOTTOM != 0 {
                    front.floor + f64::from(tex)
                } else {
                    front.ceiling
                }
            }
            Some(back) => {
                let shut = front.ceiling.min(back.ceiling) <= front.floor.max(back.floor);
                // (the part she sees, as `face` picks it)
                if (shut || !named(&sd.middle)) && named(&sd.upper) && back.ceiling < front.ceiling {
                    let tex = height(&sd.upper)?;
                    if line.flags & ML_DONTPEGTOP != 0 {
                        front.ceiling
                    } else {
                        back.ceiling + f64::from(tex)
                    }
                } else if (shut || !named(&sd.middle)) && named(&sd.lower) && back.floor > front.floor {
                    if line.flags & ML_DONTPEGBOTTOM != 0 {
                        front.ceiling
                    } else {
                        back.floor
                    }
                } else {
                    return None;
                }
            }
        };
        let tex = [&sd.upper, &sd.lower, &sd.middle].into_iter().find(|t| named(t)).and_then(|t| height(t))?;
        Some(((top as i64) + i64::from(sd.y_offset)).rem_euclid(i64::from(tex)))
    }

    /// The texture a wall shows from its front.
    pub(super) fn face(&self, line: &Linedef) -> String {
        if line.front == NO_SIDEDEF {
            return String::new();
        }
        let m = &self.nav.map;
        let sd = &m.sidedefs[usize::from(line.front)];
        // (a two-sided line shut, a door down: no opening for the middle texture to show in,
        // only what's above and below it: MAP11's secret door, STONE5 over the opening,
        // between walls of STONE4)
        let h = |x: usize| self.seen_heights.get(&x).copied().unwrap_or(self.nav.movers.initial[x]);
        let shut = matches!(m.sides(line.index), (Some(f), Some(b)) if h(f).ceiling.min(h(b).ceiling) <= h(f).floor.max(h(b).floor));
        let order = if shut { [&sd.upper, &sd.lower, &sd.middle] } else { [&sd.middle, &sd.upper, &sd.lower] };
        order.iter().find(|t| !t.is_empty() && t.as_str() != "-").map(|t| t.to_uppercase()).unwrap_or_default()
    }

    /// The lines joined to this one at its ends, on the same sector's side.
    fn wall_neighbours(&self, li: usize) -> Vec<usize> {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        let front = m.front_sector(l);
        [l.v1, l.v2]
            .iter()
            .flat_map(|v| self.ends.get(v).into_iter().flatten().copied())
            .filter(|&n| n != li && m.front_sector(&m.linedefs[n]) == front)
            .collect()
    }
}

/// What `wall_signs` weighs every wall against (`wall_context`).
struct WallContext<'a> {
    reach: &'a BTreeSet<usize>,
    reach_sectors: BTreeSet<usize>,
    keys: crate::specials::Keys,
    known: BTreeSet<usize>,
    /// the middles of the exit signs she's seen
    signs: Vec<Point>,
    heights: Vec<(f64, f64)>,
    lately: BTreeSet<usize>,
    unreached_items: &'a [Point],
    unreached_places: Option<&'a dyn Fn(Point, f64) -> bool>,
}

/// One wall as the rubric sees it: why it's no wall to try, or its signs.
#[derive(Clone, Debug)]
pub enum Wall {
    Skip(&'static str),
    Suspect { signs: Signs, behind: Option<Behind>, stand: Point, aim: Point },
}

/// What a wall shows, each sign the points it adds to its suspicion (0: not there).
#[derive(Clone, Copy, Debug, Default)]
pub struct Signs {
    /// two-sided with a middle texture: a grate or panel set in it
    pub target: f64,
    /// two-sided, closed, not flagged secret: the automap draws it as not a wall
    pub not_wall: f64,
    /// the side of a raised platform
    pub platform: f64,
    /// an item she can't get to just behind
    pub item_behind: f64,
    /// ...seen through a gap over or under the wall
    pub item_gap: f64,
    /// somewhere she's seen and can't get to just behind
    pub place_behind: f64,
    /// an exit sign in front
    pub exit_sign: f64,
    /// a blank on her map behind, walled round
    pub blank_behind: f64,
    /// a face unlike the walls either side
    pub odd: f64,
    /// ...on a short wall
    pub short: f64,
    /// ...one panel between two alike
    pub panel: f64,
}

/// A search for secrets asked for (Evil's explore with secrets) presses the walls with a
/// `secret_score` this high or more.
pub const SECRET_MIN: f64 = 1.0;

impl Signs {
    /// How likely the wall hides a secret, for a search for one asked for: each sign's log2 lift
    /// (how much more often it's on a wall that's pressed or shot into a secret than on any
    /// other wall), and for the signs nearly every secret wall has, what its absence says. From
    /// `evil-boom secret-survey` over Doom's E1-E3 and Doom II (86 secret walls, 18,667 others;
    /// each lift shrunk toward 1 as if 4 more secret walls had shown the other walls' rate,
    /// rounded to halves; the exit sign, never on a secret wall, counts nothing either way: it's
    /// the way out's). Fitted on Doom alone it kept the same Doom II secrets with a fifth fewer
    /// walls; with SECRET_MIN, the same 78 of 86 as a score over 2 with a quarter fewer walls.
    /// (The rubric's own `score` stays for the rounds of walls she makes by herself: the way on,
    /// not only secrets.)
    pub fn secret_score(&self) -> f64 {
        // (each sign: there, and not there)
        let w = |there: f64, present: f64, absent: f64| if there > 0.0 { present } else { absent };
        [
            w(self.target, 1.5, 0.0),
            w(self.not_wall, 3.0, -0.5),
            w(self.platform, 1.5, 0.0),
            w(self.item_behind, 4.5, -0.5),
            w(self.item_gap, 4.0, 0.0),
            w(self.place_behind, 1.5, -3.0),
            w(self.blank_behind, 3.5, 0.0),
            w(self.odd, 1.5, -1.0),
            w(self.short, 1.5, -0.5),
            w(self.panel, 2.5, -0.5),
        ]
        .iter()
        .sum()
    }

    /// The suspicion: the signs' points (added in the rubric's order).
    pub fn score(&self) -> f64 {
        [self.target, self.not_wall, self.platform, self.item_behind, self.item_gap, self.place_behind, self.exit_sign, self.blank_behind, self.odd, self.short, self.panel].iter().sum()
    }

    /// Every sign's name, in the rubric's order.
    pub fn named_all(&self) -> [&'static str; 11] {
        ["target", "not_wall", "platform", "item_behind", "item_gap", "place_behind", "exit_sign", "blank_behind", "odd", "short", "panel"]
    }

    /// The signs there, by name, as (name, points).
    pub fn named(&self) -> Vec<(&'static str, f64)> {
        [
            ("target", self.target),
            ("not_wall", self.not_wall),
            ("platform", self.platform),
            ("item_behind", self.item_behind),
            ("item_gap", self.item_gap),
            ("place_behind", self.place_behind),
            ("exit_sign", self.exit_sign),
            ("blank_behind", self.blank_behind),
            ("odd", self.odd),
            ("short", self.short),
            ("panel", self.panel),
        ]
        .into_iter()
        .filter(|(_, v)| *v > 0.0)
        .collect()
    }
}
