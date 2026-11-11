//! Finding standable and reachable pickup positions at believed sector heights.

use super::{Explorer, ARRIVE_SLACK};
use crate::geom::{dist, Point};
use crate::motor::Observation;
use crate::navigator::{MAX_STEP, PICKUP_BELOW, PLAYER_HEIGHT, PLAYER_RADIUS};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// a pickup's box and hers overlap this near (20 + 16), either way
const TOUCH: i32 = 36;

/// Square pickup rings, visited from the centre outwards, then by distance and coordinates.
const PICKUP_RADII: [i32; 6] = [0, 8, 16, 24, 32, TOUCH - 1];
const PICKUP_SAMPLE_STEP: usize = 4;

fn pickup_offsets() -> &'static [(i32, i32)] {
    static OFFSETS: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    OFFSETS.get_or_init(|| {
        PICKUP_RADII
            .into_iter()
            .flat_map(|r| {
                let mut ring: Vec<_> = (-r..=r)
                    .step_by(PICKUP_SAMPLE_STEP)
                    .chain(std::iter::once(r))
                    .flat_map(|i| [(i, -r), (i, r), (-r, i), (r, i)])
                    .collect();
                ring.sort_by_key(|&(dx, dy)| (dx * dx + dy * dy, dx, dy));
                ring.dedup();
                ring
            })
            .collect()
    })
}

impl Explorer {
    /// Where she stands, for planning from: her middle, unless that's over a floor below her
    /// feet while she's held up on a lip (the game keeps her at the highest floor under her
    /// box); then the nearest point under her box on the floor she's on. Planned from her
    /// middle, the plan would start from down in whatever she's standing over.
    pub fn standing_point(&self, obs: &Observation) -> Point {
        let pos = obs.pos();
        let Some(z) = obs.z else { return pos };
        let floor_at = |q: Point| {
            self.nav
                .poly_at(q)
                .and_then(|p| obs.height(self.nav.sector_of_poly(p)))
                .map(|h| h.floor)
        };
        // Only over a drop she couldn't step back up: off a step, a moment above the floor below,
        // she's where her middle is.
        if floor_at(pos).is_none_or(|f| f >= z - MAX_STEP) {
            return pos;
        }
        let r = PLAYER_RADIUS - 1.0;
        [
            (r, 0.0),
            (-r, 0.0),
            (0.0, r),
            (0.0, -r),
            (r, r),
            (r, -r),
            (-r, r),
            (-r, -r),
        ]
        .into_iter()
        .map(|(dx, dy)| (pos.0 + dx, pos.1 + dy))
        .filter(|&q| floor_at(q).is_some_and(|f| (f - z).abs() < 1.0))
        .min_by(|a, b| dist(*a, pos).total_cmp(&dist(*b, pos)))
        .unwrap_or(pos)
    }

    /// Can the box stand at q, come to from the floor there: clear of walls and things, room
    /// overhead, and not hanging over a floor more than a step higher? `play`: with a little
    /// play round it, for a spot to stop at.
    pub fn standable(&self, q: Point, play: bool) -> bool {
        // At the heights she last saw, not the map's starting ones: a door opened since, a floor
        // risen.
        let heights = self.as_seen();
        self.standable_at(q, play, &heights)
    }

    /// `standable` at these heights (hers as seen), remembered while they stay as they are:
    /// it's asked of every poly she can get to for each list of what she could do, with the
    /// same answers each time.
    pub(super) fn standable_at(&self, q: Point, play: bool, heights: &[(f64, f64)]) -> bool {
        let mut cache = self.standable_cache.borrow_mut();
        if cache.heights != heights {
            cache.heights.clear();
            cache.heights.extend_from_slice(heights);
            cache.answers.clear();
        }
        let key = (q.0.to_bits(), q.1.to_bits(), play);
        if let Some(&ok) = cache.answers.get(&key) {
            return ok;
        }
        let probe = self.nav.jump_lab.at_heights(heights);
        let ok = self.stands_with(q, heights, &probe) && (!play || with_play(&probe, q));
        cache.answers.insert(key, ok);
        ok
    }

    /// Each sector as she last saw it (floor, ceiling); the starting heights where she hasn't.
    pub(super) fn as_seen(&self) -> Vec<(f64, f64)> {
        let k = &self.knowledge;
        self.nav
            .movers
            .initial
            .iter()
            .enumerate()
            .map(|(s, &h)| k.seen_heights.get(&s).copied().unwrap_or(h))
            .map(|h| (h.floor, h.ceiling))
            .collect()
    }

    /// Where to stand to pick up a thing at `item`: there if the box can stand there, else the
    /// nearest spot it can, in a poly she can get to, close enough for the boxes to touch, on a
    /// floor near the item's (an item in an alcove, say). `None` if there isn't one.
    pub fn touch_spot(&self, item: Point, reach: &BTreeSet<usize>) -> Option<Point> {
        let nav = &self.nav;
        let heights = self.roomiest();
        let probe = nav.jump_lab.at_heights(&heights);
        // The item on its floor as she believes it now (a floor it lies on may have moved).
        let floor = nav.poly_at(item).map(|p| heights[nav.sector_of_poly(p)].0);
        // Square rings, as the boxes touch, every 4 round each: an item on the point of a ledge can
        // be had only from a corner of the square.
        for &(dx, dy) in pickup_offsets() {
            let q = (item.0 + f64::from(dx), item.1 + f64::from(dy));
            if !nav.poly_at(q).is_some_and(|p| reach.contains(&p))
                || !self.stands_with(q, &heights, &probe)
            {
                continue;
            }
            // P_TouchSpecialThing: the item no higher than her height over her feet, and no more than 8
            // under them (so one on a ledge above can be had from below). Her feet are on the highest
            // floor under the box. And it must be where she can stand: not the box over a ledge she
            // can't step up onto.
            let feet = [
                (0.0, 0.0),
                (ARRIVE_SLACK, 0.0),
                (-ARRIVE_SLACK, 0.0),
                (0.0, ARRIVE_SLACK),
                (0.0, -ARRIVE_SLACK),
            ]
            .iter()
            .map(|(dx, dy)| (q.0 + dx, q.1 + dy))
            .filter(|&at| at == q || self.stands_with(at, &heights, &probe))
            .map(|at| probe.fits_at(at).1)
            .fold(f64::NEG_INFINITY, f64::max);
            if floor.is_some_and(|f| !(-PICKUP_BELOW..=PLAYER_HEIGHT).contains(&(f - feet))) {
                continue;
            }
            return Some(q);
        }
        None
    }

    /// `spot`, or somewhere near it the box can stand (`stand_spot`), for objectives from
    /// elsewhere (a monster's last known spot).
    pub fn stand_spot_for(&self, spot: Point, reach: &BTreeSet<usize>) -> Point {
        self.stand_spot(spot, reach)
    }

    /// Each sector at its roomiest as she believes it now (floor, ceiling).
    pub(super) fn roomiest(&self) -> Vec<(f64, f64)> {
        // And what the triggers she knows could give, as of her last list: a lift she can lower.
        let could = &self.reach_states;
        self.states()
            .iter()
            .enumerate()
            .map(|(s, hs)| {
                hs.iter()
                    .chain(could.get(s).into_iter().flatten())
                    .max_by(|a, b| (a.ceiling - a.floor).total_cmp(&(b.ceiling - b.floor)))
                    .map_or((0.0, 0.0), |h| (h.floor, h.ceiling))
            })
            .collect()
    }

    /// `standable`, at `heights` rather than the map's starting ones: the box against the lines,
    /// as the game has it now (a door shut at the start and open since; a wall opened by a shot).
    pub(super) fn stands_at(&self, q: Point, heights: &[(f64, f64)]) -> bool {
        self.stands_with(q, heights, &self.nav.jump_lab.at_heights(heights))
    }

    fn stands_with(
        &self,
        q: Point,
        heights: &[(f64, f64)],
        probe: &crate::jumps::PlayerSim<'_>,
    ) -> bool {
        let mesh = &self.nav.mesh;
        // In the map, not in the solid between its rooms, which the BSP puts in the subsector beside
        // it all the same.
        let Some(p) = mesh.poly_at(q).filter(|&p| mesh.contains(p, q)) else {
            return false;
        };
        let (fits, floor, ceiling) = probe.fits_at(q);
        fits && ceiling - floor >= PLAYER_HEIGHT
            && floor <= heights[mesh.poly(p).sector].0 + MAX_STEP
    }

    /// `spot`, if the box can stand there; else the middle of its poly, or of one next to it she
    /// can get to, where it can.
    pub(super) fn stand_spot(&self, spot: Point, reach: &BTreeSet<usize>) -> Point {
        // And clear of what's shut now: a door that moves isn't a wall at the map's own heights, but
        // a spot against it shut is no spot.
        let heights = self.roomiest();
        let mesh = &self.nav.mesh;
        let probe = self.nav.jump_lab.at_heights(&heights);
        // With play: a spot where the box fits only to the unit (between a wall and a shut door's
        // edge exactly 32 apart) is one the follower never gets to.
        let clear = |q: Point| self.stands_with(q, &heights, &probe) && with_play(&probe, q);
        if clear(spot) {
            return spot;
        }
        let Some(p) = mesh.poly_at(spot) else {
            return spot;
        };
        // Two rings out, nearest first: the nearest strip that fits can be the next one over.
        let next = |ps: &[usize]| -> Vec<usize> {
            ps.iter()
                .flat_map(|&a| {
                    mesh.poly(a)
                        .portals
                        .iter()
                        .map(move |&q| mesh.portals[q].other(a))
                })
                .filter(|q| reach.contains(q))
                .collect()
        };
        let one = next(&[p]);
        let two = next(&one);
        for ring in [
            vec![p].into_iter().filter(|q| reach.contains(q)).collect(),
            one,
            two,
        ] {
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
}

/// Does the box fit half a unit to either side of `q`, each way (not only to the unit there)?
fn with_play(probe: &crate::jumps::PlayerSim<'_>, q: Point) -> bool {
    [(0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)]
        .iter()
        .all(|(dx, dy)| probe.fits_at((q.0 + dx, q.1 + dy)).0)
}
