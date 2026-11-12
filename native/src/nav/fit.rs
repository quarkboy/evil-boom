// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Path smoothing: the funnel through the portals A* found, then the waypoints pushed off
//! corners and walls, checked (and detoured) so the player's box fits between them, and
//! straightened where it can go direct.

use super::path::Chain;
use super::{
    can_cross, crossable_either_way, top_floor, Doors, HeightSource, Heights, NavCore, Open,
    TeleKind, Via,
};
use crate::det::HashSet;
use crate::geom::{
    cells, cells_around, closest_point, dist, funnel, oriented, point_key, seg_key,
    segment_distance, segments_cross, swept_box_hits, Cell, Point, PointKey, Seg, SegKey, MAX_STEP,
    PATH_CLEARANCE, PLAYER_HEIGHT, PLAYER_RADIUS,
};
use crate::stats::{self, Phase};
use std::collections::BinaryHeap;
use std::time::Instant;

/// How far waypoints are kept from walls.
const WAYPOINT_CLEARANCE: f64 = 18.0;
/// How far the path is kept from wall corners: the box's corner, coming at one diagonally,
/// and room to spare, as nobody follows a line to the unit (a unit off a wall's end on the
/// path is wedged against it two units off the line).
const CORNER_CLEARANCE: f64 = PLAYER_RADIUS * std::f64::consts::SQRT_2 + 4.0;
/// Extra room kept from solid things' corners when detouring round them.
const THING_CLEARANCE: f64 = 6.0;
/// How much further than the box a path keeps from an exit line it doesn't mean to cross.
const TRIP_MARGIN: f64 = 24.0;
/// The box checks' tolerance.
const BOX_SLACK: f64 = 0.25;
/// How many waypoints ahead straightening looks for a direct line.
const STRAIGHTEN_REACH: usize = 10;
/// Most expansions of a detour search.
const DETOUR_EXPANSIONS: usize = 150;

/// What a line is to cross, at given heights.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Crossing {
    Open,
    Ledge,
    Wall,
}

/// Where the box got squeezed along a path, and whether any of it was against shut sectors
/// only (no door to open: the path's no good at all).
#[derive(Default)]
pub struct Squeezes {
    pub points: Vec<Point>,
    pub hard: bool,
}

/// How close to a wall a waypoint can be: enough for the box, whatever the wall's angle.
/// (More than that squeezes paths between waypoints in narrow places.)
fn box_clearance(a: Point, b: Point) -> f64 {
    let length = dist(a, b).max(1e-6);
    let (c, s) = ((b.0 - a.0).abs() / length, (b.1 - a.1).abs() / length);
    WAYPOINT_CLEARANCE.max(PLAYER_RADIUS * (c + s) + 1.0)
}

/// The points every 8 units from a to b (both ends included).
fn steps_along(a: Point, b: Point) -> impl Iterator<Item = Point> {
    let n = ((dist(a, b) / 8.0).floor() as i64).max(1);
    (0..=n).map(move |i| {
        (
            a.0 + (b.0 - a.0) * i as f64 / n as f64,
            a.1 + (b.1 - a.1) * i as f64 / n as f64,
        )
    })
}

/// The teleport or jump `chain[i]` was entered by, if any.
fn tele_into<'a>(nav: &'a NavCore, chain: &Chain, i: usize) -> Option<&'a TeleKind> {
    match chain[i].1 {
        Some(Via::Tele(ti)) => Some(&nav.tele[chain[i - 1].0][ti].kind),
        _ => None,
    }
}

impl NavCore {
    /// The waypoints along `chain` (None where it teleports), and where the box got squeezed.
    pub(super) fn smooth(
        &self,
        start: Point,
        goal: Point,
        chain: &Chain,
        st: &mut dyn HeightSource,
        doors: Option<&Doors>,
    ) -> (Vec<Option<Point>>, Squeezes) {
        let mut points: Vec<Option<Point>> = Vec::new();
        let mut seg_start = start;
        let mut portals: Vec<(Point, Point)> = Vec::new();
        for i in 1..chain.len() {
            let prev_poly = chain[i - 1].0;
            match chain[i].1 {
                Some(Via::Tele(ti)) => {
                    let t = &self.tele[prev_poly][ti];
                    match &t.kind {
                        TeleKind::Jump { runup, takeoff, .. } => {
                            points
                                .extend(funnel(seg_start, *runup, &portals).into_iter().map(Some));
                            points.push(Some(*takeoff));
                        }
                        TeleKind::Line { line, .. } => {
                            // walk onto the line from 24 in front of it, where the box gets over
                            let (mid, approach) = self.teleport_crossing(*line);
                            points.extend(
                                funnel(seg_start, approach, &portals).into_iter().map(Some),
                            );
                            points.push(Some(mid));
                            points.push(None);
                        }
                    }
                    seg_start = t.dest_pt;
                    portals.clear();
                }
                Some(Via::Portal(pi)) => {
                    let q = &self.mesh.portals[pi];
                    portals.push(oriented(q.p1, q.p2, self.mesh.center(prev_poly)));
                }
                None => {}
            }
        }
        points.extend(funnel(seg_start, goal, &portals).into_iter().map(Some));

        // the lines the path crosses aren't walls to it (unless they shut by themselves)
        let mut exempt: HashSet<usize> = HashSet::default();
        for (i, c) in chain.iter().enumerate() {
            match c.1 {
                Some(Via::Portal(pi)) => exempt.extend(self.mesh.portals[pi].linedef),
                Some(Via::Tele(_)) => {
                    if let Some(TeleKind::Line { line, .. }) = tele_into(self, chain, i) {
                        exempt.insert(*line);
                    }
                }
                None => {}
            }
        }
        exempt.retain(|li| !self.shutting.contains(li));
        let mut pinned: HashSet<PointKey> = HashSet::default();
        let mut takeoffs: HashSet<PointKey> = HashSet::default();
        for i in 1..chain.len() {
            if let Some(TeleKind::Jump { runup, takeoff, .. }) = tele_into(self, chain, i) {
                pinned.extend([point_key(*runup), point_key(*takeoff)]);
                takeoffs.insert(point_key(*takeoff));
            }
        }
        let sectors: HashSet<usize> = chain.iter().map(|c| self.poly_sector[c.0]).collect();
        let mut fitter = Fitter {
            nav: self,
            st,
            doors,
            exempt,
            pinned,
            takeoffs,
            sectors,
            squeezes: Squeezes::default(),
        };

        let mut out: Vec<Option<Point>> = Vec::new();
        for (k, run) in points.split(Option::is_none).enumerate() {
            if k > 0 {
                out.push(None);
            }
            let run: Vec<Point> = run.iter().flatten().copied().collect();
            if run.is_empty() {
                continue;
            }
            let t = Instant::now();
            let a = fitter.push_from_corners(&run);
            stats::record(Phase::Corners, t);
            let t = Instant::now();
            let b = fitter.box_fits_path(&a);
            stats::record(Phase::Fit, t);
            let t = Instant::now();
            let c = fitter.straighten(&b);
            stats::record(Phase::Straighten, t);
            out.extend(c.into_iter().map(Some));
        }
        (out, fitter.squeezes)
    }

    /// Static walls (`static_grid`) the box swept from p to q hits, less `ignore`.
    fn static_hits(
        &self,
        p: Point,
        q: Point,
        r: f64,
        ignore: &HashSet<SegKey>,
        out: &mut HashSet<SegKey>,
    ) {
        for c in cells_around(p, q, PLAYER_RADIUS + 1.0) {
            for &e in self.static_grid.get(&c).into_iter().flatten() {
                let k = seg_key(e);
                if !ignore.contains(&k) && !out.contains(&k) && swept_box_hits(p, q, r, e) {
                    out.insert(k);
                }
            }
        }
    }

    /// Where to cross teleport line `li`, and the spot 24 in front of it to come from: the
    /// point of the line nearest its middle where the box fits both just over it and at that
    /// spot (the line can be a face of a closet too narrow at its middle for the box).
    fn teleport_crossing(&self, li: usize) -> (Point, Point) {
        let (a, b) = self.map.line_seg(li);
        let length = dist(a, b).max(1.0);
        let (nx, ny) = ((b.1 - a.1) / length, -(b.0 - a.0) / length); // towards the front
        let at = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let fits = |c: Point| {
            !self.box_blocked_static((c.0 - nx * 2.0, c.1 - ny * 2.0))
                && !self.box_blocked_static((c.0 + nx * 24.0, c.1 + ny * 24.0))
        };
        let steps = (length / 4.0).floor().max(1.0) as i64;
        let best = (0..=steps)
            .map(|i| i as f64 / steps as f64)
            .filter(|&t| fits(at(t)))
            .min_by(|x, y| (x - 0.5).abs().total_cmp(&(y - 0.5).abs()))
            .unwrap_or(0.5);
        let c = at(best);
        (c, (c.0 + nx * 24.0, c.1 + ny * 24.0))
    }

    /// Does the box at p overlap a static wall?
    pub fn box_blocked_static(&self, p: Point) -> bool {
        let mut hits = HashSet::default();
        self.static_hits(
            p,
            p,
            PLAYER_RADIUS + BOX_SLACK,
            &HashSet::default(),
            &mut hits,
        );
        !hits.is_empty()
    }

    /// The linedefs in these cells, each once, in the order first met.
    fn lines_in<'a>(
        &'a self,
        cs: impl Iterator<Item = Cell> + 'a,
    ) -> impl Iterator<Item = usize> + 'a {
        let mut seen: HashSet<usize> = HashSet::default();
        cs.flat_map(|c| self.mesh.line_grid.get(&c).into_iter().flatten().copied())
            .filter(move |&li| seen.insert(li))
    }
}

/// Smoothing one path: the heights it's at, and what's exempt from the wall checks or
/// mustn't be moved.
struct Fitter<'a> {
    nav: &'a NavCore,
    st: &'a mut dyn HeightSource,
    /// doors that can be opened on the way (their sectors don't shut the box in)
    doors: Option<&'a Doors>,
    /// lines the path crosses
    exempt: HashSet<usize>,
    /// jumps' runups and takeoffs: not moved or dropped
    pinned: HashSet<PointKey>,
    /// jumps' takeoffs: no shortcut from them (what follows is the flight, and going straight
    /// on past the landing cuts across the pit the jump clears)
    takeoffs: HashSet<PointKey>,
    /// the sectors the path goes through: straightening stays in them
    sectors: HashSet<usize>,
    squeezes: Squeezes,
}

impl Fitter<'_> {
    fn heights(&mut self, s: usize) -> std::rc::Rc<Vec<Heights>> {
        self.st.heights(s)
    }

    fn is_pinned(&self, p: Point) -> bool {
        self.pinned.contains(&point_key(p))
    }

    // --- box checks ---------------------------------------------------------------------------

    /// The floor the box at `a` stands on, for which ledges block it: the highest under it,
    /// but not one it's wedged between (higher than a step on both sides, not in the middle).
    fn standing_floor(&mut self, a: Point) -> Option<f64> {
        let nav = self.nav;
        // (on a line between two sectors, the one the path goes through: a path along the foot
        // of a ledge has its waypoints on the ledge's edge, and isn't on top of it)
        let here = [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)]
            .iter()
            .filter_map(|(dx, dy)| nav.sector_at((a.0 + dx, a.1 + dy)))
            .find(|s| self.sectors.contains(s))
            .or_else(|| nav.sector_at(a))?;
        let reach_up = top_floor(&self.heights(here)) + MAX_STEP;
        let mut grid = [[f64::NEG_INFINITY; 3]; 3];
        for (i, row) in grid.iter_mut().enumerate() {
            for (j, f) in row.iter_mut().enumerate() {
                if let Some(s) = nav.sector_at((
                    a.0 + (i as f64 - 1.0) * PLAYER_RADIUS,
                    a.1 + (j as f64 - 1.0) * PLAYER_RADIUS,
                )) {
                    // (not on a floor out of her reach in a sector the path never goes into: the
                    // box beside a ledge, not on it)
                    let top = top_floor(&self.st.heights(s));
                    if top <= reach_up || self.sectors.contains(&s) {
                        *f = top;
                    }
                }
            }
        }
        let high = |i: usize, j: usize| grid[i][j] > reach_up;
        let wedged = (0..3).any(|k| {
            (high(0, k) && high(2, k) && !high(1, k)) || (high(k, 0) && high(k, 2) && !high(k, 1))
        });
        grid.iter()
            .flatten()
            .copied()
            .filter(|&f| f > f64::NEG_INFINITY && (!wedged || f <= reach_up))
            .reduce(f64::max)
    }

    /// Does the box at p hang over a floor more than a step above the one under its centre (not
    /// wedged between two: a rim walk)? It can only be there having come along the top, never
    /// from beside it (a detour round something mustn't go along a high ledge's foot as if on it).
    fn up_against_ledge(&mut self, p: Point) -> bool {
        let Some(here) = self.nav.sector_at(p) else {
            return false;
        };
        let centre = top_floor(&self.heights(here));
        self.standing_floor(p)
            .is_some_and(|f| f > centre + MAX_STEP)
    }

    /// Does (non-exempt) line `li` stop the box standing at `at`? `standing` caches
    /// `standing_floor(at)`.
    fn line_blocks(&mut self, li: usize, at: Point, standing: &mut Option<Option<f64>>) -> bool {
        let nav = self.nav;
        if nav.trip_lines.contains(&li) {
            return true;
        }
        let (f, b) = nav.map.sides(li);
        let is_wall = nav.walls.contains(&li);
        if is_wall {
            // (a wall in the way unless it's open now)
            return match (f, b) {
                (Some(f), Some(b)) => !crossable_either_way(&self.heights(f), &self.heights(b)),
                _ => true,
            };
        }
        let (Some(f), Some(b)) = (f, b) else {
            // (a one-sided line that isn't a wall or a drop: never in the way)
            return false;
        };
        let (fs, bs) = (self.heights(f), self.heights(b));
        let Some(ours) = self.standing_floor_cached(at, standing) else {
            return false;
        };
        // a ledge (at the heights now, not only those at the start: a pillar can rise out of
        // the floor, and a path would cut its corner): in the way if its high side is more
        // than a step above where we stand
        let high = fs
            .iter()
            .flat_map(|hf| bs.iter().map(|hb| hf.floor.max(hb.floor)))
            .fold(f64::INFINITY, f64::min);
        if ours + MAX_STEP < high {
            return true;
        }
        if nav.drops.contains(&li) {
            return false;
        }
        // an opening: in the way if it's too low over where we stand
        let top = fs
            .iter()
            .flat_map(|hf| bs.iter().map(|hb| hf.ceiling.min(hb.ceiling)))
            .fold(f64::NEG_INFINITY, f64::max);
        top - ours < PLAYER_HEIGHT
    }

    /// Is line `li` a ledge more than a step above the floor under the box's centre at `at`?
    fn ledge_over_centre(&mut self, li: usize, at: Point) -> bool {
        let nav = self.nav;
        let (Some(f), Some(b)) = nav.map.sides(li) else {
            return false;
        };
        let Some(here) = nav.sector_at(at) else {
            return false;
        };
        let centre = top_floor(&self.heights(here));
        let low = |hs: &[Heights]| hs.iter().map(|h| h.floor).fold(f64::INFINITY, f64::min);
        low(&self.heights(f)).max(low(&self.heights(b))) > centre + MAX_STEP
    }

    fn standing_floor_cached(&mut self, at: Point, cache: &mut Option<Option<f64>>) -> Option<f64> {
        if let Some(f) = *cache {
            return f;
        }
        let f = self.standing_floor(at);
        *cache = Some(f);
        f
    }

    /// The edges the box swept from p to q hits (static walls, then lines in its way at
    /// these heights), less `ignore`.
    fn box_hits(&mut self, p: Point, q: Point, ignore: &HashSet<SegKey>) -> HashSet<SegKey> {
        self.box_hits_at(p, q, PLAYER_RADIUS + BOX_SLACK, ignore)
    }

    /// box_hits, for a box of half-width `r`.
    fn box_hits_at(
        &mut self,
        p: Point,
        q: Point,
        r: f64,
        ignore: &HashSet<SegKey>,
    ) -> HashSet<SegKey> {
        let nav = self.nav;
        let mut out = HashSet::default();
        nav.static_hits(p, q, r, ignore, &mut out);
        let mut standing = None;
        for li in nav.lines_in(cells_around(p, q, PLAYER_RADIUS + TRIP_MARGIN + 1.0)) {
            let e = nav.map.line_seg(li);
            let k = seg_key(e);
            // (an exit not meant to be crossed, given a wide berth: following a path wanders, and
            // a way round a pad whose edges are exits can be a box from them; not a teleport,
            // which ways often go close by)
            let trip = nav.trip_lines.contains(&li)
                && !self.exempt.contains(&li)
                && crate::specials::classify_line(nav.map.linedefs[li].special)
                    .is_some_and(|i| i.category.is_exit());
            if out.contains(&k) || !swept_box_hits(p, q, if trip { r + TRIP_MARGIN } else { r }, e)
            {
                continue;
            }
            // (one the box is over already doesn't count against leaving it, nor one the way
            // crosses, but a ledge more than a step above the floor under its centre does against
            // going further over it: where a fan of stairs meets at a point, the box set off beside
            // it is over the second step up already, and mustn't go along it)
            if swept_box_hits(p, p, r, e)
                && segment_distance(p, q, e.0, e.1) < dist(p, closest_point(p, e.0, e.1)) - 0.5
                && self.ledge_over_centre(li, p)
            {
                out.insert(k);
                continue;
            }
            if self.exempt.contains(&li) || ignore.contains(&k) {
                continue;
            }
            if self.line_blocks(li, p, &mut standing) {
                out.insert(k);
            }
        }
        out
    }

    fn box_clear(&mut self, p: Point, q: Point, ignore: &HashSet<SegKey>) -> bool {
        self.box_hits(p, q, ignore).is_empty()
    }

    /// What the box at `a` and at `b` already overlaps (not to count against going between).
    fn overlapped_at(&mut self, a: Point, b: Point) -> HashSet<SegKey> {
        let none = HashSet::default();
        let mut ignore = self.box_hits(a, a, &none);
        ignore.extend(self.box_hits(b, b, &none));
        ignore
    }

    /// The edges near a-b (within `pad`) in the box's way standing at `a`: static walls, then
    /// lines.
    fn edges_near(&mut self, a: Point, b: Point, pad: f64) -> Vec<Seg> {
        let nav = self.nav;
        let mut seen: HashSet<SegKey> = HashSet::default();
        let mut out: Vec<Seg> = cells_around(a, b, pad)
            .flat_map(|c| nav.static_grid.get(&c).into_iter().flatten().copied())
            .filter(|&e| seen.insert(seg_key(e)))
            .collect();
        let mut standing = None;
        for li in nav.lines_in(cells_around(a, b, pad)) {
            if !self.exempt.contains(&li) && self.line_blocks(li, a, &mut standing) {
                out.push(nav.map.line_seg(li));
            }
        }
        out
    }

    /// Does a-b cross a ledge (not exempt) the player can't climb back up, at these heights?
    fn ledges_crossed(&mut self, a: Point, b: Point) -> bool {
        let nav = self.nav;
        let lines: Vec<usize> = cells(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
            .flat_map(|c| nav.mesh.line_grid.get(&c).into_iter().flatten().copied())
            .filter(|li| !self.exempt.contains(li))
            .filter(|&li| {
                let (v1, v2) = nav.map.line_seg(li);
                segments_cross(a, b, v1, v2)
            })
            .collect();
        // (the ledges at the start, and those at these heights that weren't)
        lines.into_iter().any(|li| {
            nav.drops.contains(&li)
                || (!nav.walls.contains(&li) && self.crossing(li) == Crossing::Ledge)
        })
    }

    /// What a two-sided line is to cross at these heights, not only the starting ones (a floor
    /// that's risen since makes a ledge, a lift come down another): both ways, one way down, or
    /// neither. A one-sided line: neither.
    fn crossing(&mut self, li: usize) -> Crossing {
        let (Some(f), Some(b)) = self.nav.map.sides(li) else {
            return Crossing::Wall;
        };
        if self.nav.map.linedefs[li].blocks_player() {
            return Crossing::Wall;
        }
        let (fs, bs) = (self.heights(f), self.heights(b));
        let pairs = || fs.iter().flat_map(|&hf| bs.iter().map(move |&hb| (hf, hb)));
        if pairs().any(|(hf, hb)| can_cross(hf, hb) && can_cross(hb, hf)) {
            Crossing::Open
        } else if pairs().any(|(hf, hb)| can_cross(hf, hb) || can_cross(hb, hf)) {
            Crossing::Ledge
        } else {
            Crossing::Wall
        }
    }

    /// Are all these edges lines of shut sectors we can't open (so squeezing past them is no
    /// squeeze but a dead end)?
    fn shut_in(&mut self, edges: &HashSet<SegKey>) -> bool {
        if edges.is_empty() {
            return false;
        }
        let room = |h: &Heights| h.ceiling - h.floor >= PLAYER_HEIGHT;
        // (a wall along with it opens no more than they do: a gap between a door's jamb and a
        // block risen beside it, narrower than the box, is no squeeze to hand out; but one
        // between walls alone is as it was, a tight spot that may do)
        let mut shut = false;
        for e in edges {
            let Some(&li) = self.nav.seg_lines.get(e) else {
                return false;
            };
            let (Some(f), Some(b)) = self.nav.map.sides(li) else {
                continue;
            };
            // (a door's line: no more than its frame, open or shut, either way; a squeeze in a
            // doorway between its jambs is as before, one past the door to a block risen beside it
            // is no way at all, from either side)
            if self
                .doors
                .is_some_and(|d| d.contains_key(&f) || d.contains_key(&b))
            {
                continue;
            }
            let (fs, bs) = (self.heights(f), self.heights(b));
            let open = fs.iter().any(|hf| {
                bs.iter()
                    .any(|hb| hf.ceiling.min(hb.ceiling) - hf.floor.max(hb.floor) >= PLAYER_HEIGHT)
            }) && fs.iter().any(room)
                && bs.iter().any(room);
            if open {
                return false;
            }
            shut = true;
        }
        shut
    }

    // --- fitting the box --------------------------------------------------------------------

    /// The waypoints the box fits between: those it overlaps something at dropped (unless
    /// going straight on would walk off a ledge), detours round what's in the way between
    /// them, and where there's none, the squeezes noted.
    fn box_fits_path(&mut self, pts: &[Point]) -> Vec<Point> {
        if pts.len() < 2 {
            return pts.to_vec();
        }
        let none = HashSet::default();
        let last = pts.len() - 1;
        let mut kept: Vec<Point> = vec![pts[0]];
        for i in 1..last {
            let p = pts[i];
            let prev = kept[kept.len() - 1];
            if self.is_pinned(p)
                || self.box_clear(p, p, &none)
                || self.ledges_crossed(prev, pts[i + 1])
            {
                kept.push(p);
            }
        }
        kept.push(pts[last]);
        let mut out = vec![kept[0]];
        for &b in &kept[1..] {
            let a = out[out.len() - 1];
            let ignore = self.overlapped_at(a, b);
            let hits = self.box_hits(a, b, &ignore);
            if !hits.is_empty() {
                if let Some(d) = self.detour(a, b, &ignore) {
                    out.extend(d);
                } else {
                    let points = self.squeeze_points(a, b, &ignore);
                    self.squeezes.points.extend(points.iter().copied());
                    // (no way round a line that sets something off is no way at all: a path over the
                    // corner of an exit pad is held back, as a squeeze past a wall)
                    let trips = hits.iter().any(|e| {
                        self.nav.seg_lines.get(e).is_some_and(|li| {
                            self.nav.trip_lines.contains(li) && !self.exempt.contains(li)
                        })
                    });
                    // (and past shut sectors only, even where the box fits with no room to spare: a
                    // cage of door bars with gaps the box's own width is a way in Doom only on the
                    // exact unit, which she never hits, and a box fitting near each squeeze isn't one
                    // getting through)
                    if trips || self.shut_in(&hits) {
                        self.squeezes.hard = true;
                    }
                }
            }
            out.push(b);
        }
        out
    }

    /// Up to about 6 spots between a and b where the box overlaps something.
    fn squeeze_points(&mut self, a: Point, b: Point, ignore: &HashSet<SegKey>) -> Vec<Point> {
        let length = dist(a, b);
        if length < 1.0 {
            return Vec::new();
        }
        let (ux, uy) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
        let mut out = Vec::new();
        for t in (0..=length.floor() as i64).step_by(8) {
            let p = (a.0 + ux * t as f64, a.1 + uy * t as f64);
            if !self.box_clear(p, p, ignore) {
                out.push(p);
            }
        }
        let step = (out.len() / 6).max(1);
        out.into_iter().step_by(step).collect()
    }

    /// A way round what's in the box's way from a to b: a short best-first search over spots
    /// off the corners of the edges near the blocked stretch.
    fn detour(&mut self, a: Point, b: Point, ignore: &HashSet<SegKey>) -> Option<Vec<Point>> {
        let length = dist(a, b);
        if length < 1.0 {
            return None;
        }
        let (ux, uy) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
        let none = HashSet::default();
        let mut bad = Vec::new();
        for t in (0..=length.floor() as i64).step_by(8) {
            let tf = t as f64;
            let p = (a.0 + ux * tf, a.1 + uy * tf);
            let q = (
                a.0 + ux * (tf + 8.0).min(length),
                a.1 + uy * (tf + 8.0).min(length),
            );
            if !self.box_clear(p, q, ignore) {
                bad.push(tf);
            }
        }
        let (Some(&first_bad), Some(&last_bad)) = (bad.first(), bad.last()) else {
            return None;
        };
        // the stretch to get round: from a bit before the first blocked step to after the last
        let (t0, t1) = (first_bad - 40.0, last_bad + 48.0);
        let mut a2 = if t0 > 0.0 {
            (a.0 + ux * t0, a.1 + uy * t0)
        } else {
            a
        };
        let mut b2 = if t1 < length {
            (a.0 + ux * t1, a.1 + uy * t1)
        } else {
            b
        };
        if (a2 != a && !self.box_clear(a2, a2, &none))
            || (b2 != b && !self.box_clear(b2, b2, &none))
        {
            a2 = a;
            b2 = b;
        }
        let mut nodes = vec![a2, b2];
        let mut seen: HashSet<PointKey> = HashSet::default();
        for e in self.edges_near(a2, b2, 96.0) {
            for v in [e.0, e.1] {
                if seen.contains(&point_key(v)) || segment_distance(a2, b2, v, v) > 96.0 {
                    continue;
                }
                seen.insert(point_key(v));
                for d in [
                    PLAYER_RADIUS + THING_CLEARANCE,
                    PLAYER_RADIUS + 2.0 * BOX_SLACK,
                ] {
                    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                        let c = (v.0 + sx * d, v.1 + sy * d);
                        if self.nav.mesh.poly_at(c).is_some()
                            && self.box_clear(c, c, &none)
                            && !self.up_against_ledge(c)
                        {
                            nodes.push(c);
                        }
                    }
                }
            }
        }
        let limit = 3.0 * dist(a2, b2) + 192.0;
        let mut best = vec![f64::INFINITY; nodes.len()];
        best[0] = 0.0;
        let mut prev: Vec<Option<usize>> = vec![None; nodes.len()];
        let mut todo = BinaryHeap::from([Open {
            f: dist(a2, b2),
            g: 0.0,
            id: 0,
        }]);
        let mut expansions = 0;
        while let Some(Open { g: cost, id: i, .. }) = todo.pop() {
            if expansions >= DETOUR_EXPANSIONS || i == 1 {
                break;
            }
            if cost > best[i] {
                continue;
            }
            expansions += 1;
            for j in 1..nodes.len() {
                let c = cost + dist(nodes[i], nodes[j]);
                if j == i || c >= best[j] || c + dist(nodes[j], b2) > limit {
                    continue;
                }
                let (p, q) = (nodes[i], nodes[j]);
                let skip = if p == a || p == a2 || q == b || q == b2 {
                    ignore
                } else {
                    &none
                };
                if self.box_clear(p, q, skip) && !self.ledges_crossed(p, q) {
                    best[j] = c;
                    prev[j] = Some(i);
                    todo.push(Open {
                        f: c + dist(nodes[j], b2),
                        g: c,
                        id: j,
                    });
                }
            }
        }
        let mut i = prev[1]?;
        let mut chain = Vec::new();
        while i != 0 {
            chain.push(nodes[i]);
            let Some(p) = prev[i] else { break };
            i = p;
        }
        chain.reverse();
        let mut out = Vec::new();
        if a2 != a {
            out.push(a2);
        }
        out.extend(chain);
        if b2 != b {
            out.push(b2);
        }
        Some(out)
    }

    /// Shortcuts: from each waypoint straight to the farthest of the next few the box gets to
    /// directly, staying in the sectors the path went through (and not over ledges).
    fn straighten(&mut self, pts: &[Point]) -> Vec<Point> {
        if pts.len() < 3 {
            return pts.to_vec();
        }
        let nav = self.nav;
        let over: Vec<HashSet<usize>> = pts
            .windows(2)
            .map(|w| {
                steps_along(w[0], w[1])
                    .filter_map(|p| nav.sector_at(p))
                    .collect()
            })
            .collect();
        let mut out = vec![pts[0]];
        let mut i = 0;
        while i < pts.len() - 1 {
            let a = pts[i];
            let mut best = i + 1;
            let reach = if self.takeoffs.contains(&point_key(a)) {
                i + 1
            } else {
                (pts.len() - 1).min(i + STRAIGHTEN_REACH)
            };
            for j in (i + 2..=reach).rev() {
                if pts[i + 1..j].iter().any(|&p| self.is_pinned(p)) {
                    continue;
                }
                let b = pts[j];
                let allowed: HashSet<usize> = over[i..j]
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|s| self.sectors.contains(s))
                    .collect();
                if !steps_along(a, b)
                    .all(|p| nav.sector_at(p).is_some_and(|s| allowed.contains(&s)))
                {
                    continue;
                }
                let ignore = self.overlapped_at(a, b);
                // (a wall the box already touches doesn't count against its edges, but going
                // straight through it does: in a doorway with the box flush to its side wall, a
                // shortcut through that wall is pushed into for good)
                let through = ignore
                    .iter()
                    .any(|&(p, q)| segments_cross(a, b, key_point(p), key_point(q)));
                if !through && self.box_clear(a, b, &ignore) && !self.ledges_crossed(a, b) {
                    best = j;
                    break;
                }
            }
            out.push(pts[best]);
            i = best;
        }
        out
    }

    // --- keeping off walls ------------------------------------------------------------------

    /// The walls near a-b (within `pad`) a waypoint should keep clear of: shut walls, ledges
    /// and teleport lines not exempt, and solid things' edges.
    fn walls_near(&mut self, a: Point, b: Point, pad: f64) -> Vec<Seg> {
        let nav = self.nav;
        let mut out = Vec::new();
        let mut seen_lines: HashSet<usize> = HashSet::default();
        let mut seen_edges: HashSet<SegKey> = HashSet::default();
        for c in cells_around(a, b, pad) {
            for &li in nav.mesh.line_grid.get(&c).into_iter().flatten() {
                if self.exempt.contains(&li) || !seen_lines.insert(li) {
                    continue;
                }
                // (the walls and ledges at the start, the walls among them open now aside; and
                // a wall or a ledge at these heights that wasn't: a platform risen since, the path
                // hugging its foot)
                // (open both ways, that is: a wall become a ledge is a ledge, the path off its foot
                // like any other; a block lowered part way still pins her at the corner a path cuts)
                let open_wall = nav.walls.contains(&li) && self.crossing(li) == Crossing::Open;
                let at_start = (nav.walls.contains(&li) && !open_wall)
                    || nav.drops.contains(&li)
                    || nav.trip_lines.contains(&li);
                let now = !nav.walls.contains(&li)
                    && !nav.drops.contains(&li)
                    && self.crossing(li) != Crossing::Open;
                if at_start || now {
                    out.push(nav.map.line_seg(li));
                }
            }
            for &e in nav.obstacle_grid.get(&c).into_iter().flatten() {
                if seen_edges.insert(seg_key(e)) {
                    out.push(e);
                }
            }
        }
        out
    }

    fn wall_clearance(&mut self, p: Point) -> f64 {
        self.walls_near(p, p, PATH_CLEARANCE)
            .into_iter()
            .map(|(a, b)| dist(p, closest_point(p, a, b)))
            .fold(f64::INFINITY, f64::min)
    }

    /// The waypoint moved off the walls it's too close to (a few times: moving off one can
    /// bring it near another), as long as that doesn't take it out of its sector or through
    /// a wall.
    fn nudge_off_walls(&mut self, mut p: Point) -> Point {
        let nav = self.nav;
        for _ in 0..4 {
            let mut worst: Option<(f64, Point, Point, Point, f64)> = None;
            for (a, b) in self.walls_near(p, p, WAYPOINT_CLEARANCE + 6.0) {
                let q = closest_point(p, a, b);
                let d = dist(p, q);
                let need = box_clearance(a, b);
                if d < need && worst.is_none_or(|w| d - need < w.0) {
                    worst = Some((d - need, q, a, b, need));
                }
            }
            let Some((_, q, a, b, need)) = worst else {
                return p;
            };
            let d = dist(p, q);
            let (ux, uy) = if d > 1e-6 {
                ((p.0 - q.0) / d, (p.1 - q.1) / d)
            } else {
                // on the wall: off it to whichever side is in the map
                let length = dist(a, b).max(1e-6);
                let (ux, uy) = (-(b.1 - a.1) / length, (b.0 - a.0) / length);
                if nav.mesh.poly_at((p.0 + ux, p.1 + uy)).is_none() {
                    (-ux, -uy)
                } else {
                    (ux, uy)
                }
            };
            let moved = (q.0 + ux * need, q.1 + uy * need);
            let here = nav.sector_at(p);
            let there = nav.sector_at(moved);
            let crosses = self
                .walls_near(p, moved, 0.0)
                .into_iter()
                .any(|(e1, e2)| segments_cross(p, moved, e1, e2));
            if there.is_none() || (here.is_some() && there != here) || crosses {
                return p;
            }
            p = moved;
        }
        p
    }

    /// The waypoints with extra ones round corners the path cuts too close, then each (but
    /// the ends and pinned ones) nudged off walls.
    fn push_from_corners(&mut self, pts: &[Point]) -> Vec<Point> {
        let Some((&first, rest)) = pts.split_first() else {
            return Vec::new();
        };
        let mut out = vec![first];
        for &b in rest {
            let a = out[out.len() - 1];
            self.round_corners(a, b, 0, &mut out);
        }
        for i in 1..out.len().saturating_sub(1) {
            if !self.is_pinned(out[i]) {
                out[i] = self.nudge_off_walls(out[i]);
            }
        }
        out
    }

    /// The way from a to b (a already in `out`): if it passes too close to a corner, by a
    /// waypoint CORNER_CLEARANCE off it (recursively, a few deep).
    fn round_corners(&mut self, a: Point, b: Point, depth: u32, out: &mut Vec<Point>) {
        let mut worst: Option<(f64, Point, Point)> = None;
        if depth < 4 {
            let mut corners: Vec<Point> = Vec::new();
            let mut seen: HashSet<PointKey> = HashSet::default();
            for (p1, p2) in self.walls_near(a, b, CORNER_CLEARANCE) {
                for v in [p1, p2] {
                    if seen.insert(point_key(v)) {
                        corners.push(v);
                    }
                }
            }
            corners.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
            for v in corners {
                let q = closest_point(v, a, b);
                let d = dist(v, q);
                if d < CORNER_CLEARANCE - 1.0
                    && dist(v, a) > 1.0
                    && dist(v, b) > 1.0
                    && worst.is_none_or(|w| d < w.0)
                {
                    worst = Some((d, v, q));
                }
            }
        }
        if let Some((d, v, q)) = worst {
            let (ux, uy) = if d < 1e-6 {
                let length = dist(a, b).max(1e-6);
                (-(b.1 - a.1) / length, (b.0 - a.0) / length)
            } else {
                ((q.0 - v.0) / d, (q.1 - v.1) / d)
            };
            let off = |s: f64| {
                (
                    v.0 + s * ux * CORNER_CLEARANCE,
                    v.1 + s * uy * CORNER_CLEARANCE,
                )
            };
            // (off the side of the corner the way is on; a corner right on the way has no side to
            // it, and the one picked can be the wall, where a wall ends on the line between two
            // portals: pushed into the wall, she pushes into it for good)
            let walkable = |p: Point| {
                self.nav
                    .mesh
                    .poly_inside(p)
                    .is_some_and(|i| self.sectors.contains(&self.nav.poly_sector[i]))
            };
            let w = if walkable(off(1.0)) || !walkable(off(-1.0)) {
                off(1.0)
            } else {
                off(-1.0)
            };
            let (wp, qp) = (self.nav.sector_at(w), self.nav.sector_at(q));
            if wp.is_some()
                && wp == qp
                && dist(w, a) > 4.0
                && dist(w, b) > 4.0
                && self.wall_clearance(w) >= WAYPOINT_CLEARANCE
            {
                self.round_corners(a, w, depth + 1, out);
                self.round_corners(w, b, depth + 1, out);
                return;
            }
        }
        out.push(b);
    }
}

/// The point a PointKey was made from.
fn key_point(k: crate::geom::PointKey) -> Point {
    (f64::from_bits(k.0), f64::from_bits(k.1))
}
