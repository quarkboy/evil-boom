//! Choosing reachable viewpoints for walls the explorer has yet to see.
use super::{Explorer, Kind, Objective};
use crate::geom::{dist, midpoint, Point};
use crate::sight::{Sight, View};
use std::collections::BTreeSet;

/// reachable spots tried for seeing an unseen wall from
const ROOM_SPOTS: usize = 6;
/// an unseen wall is looked at from a reachable spot this near it
const ROOM_LOOK_RANGE: f64 = 768.0;
/// Bound CPU use per map when several sweep processes are running.
const MAX_VIEWPOINT_WORKERS: usize = 4;
/// Small searches finish faster on the calling thread.
const WALLS_PER_WORKER: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
struct WallViewpoint {
    score: f64,
    line: usize,
    position: Point,
    wall_middle: Point,
}

/// Only the nearest few candidates are inspected. Include the input index in the key so
/// partial selection preserves the stable ordering of equally distant polygon centres.
fn nearest_centres(centres: &[Point], mid: Point) -> Vec<Point> {
    let mut near: Vec<_> = centres
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| {
            let distance = dist(c, mid);
            (distance <= ROOM_LOOK_RANGE).then_some((distance, i, c))
        })
        .collect();
    let compare = |a: &(f64, usize, Point), b: &(f64, usize, Point)| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1));
    if near.len() > ROOM_SPOTS {
        near.select_nth_unstable_by(ROOM_SPOTS, compare);
        near.truncate(ROOM_SPOTS);
    }
    near.sort_unstable_by(compare);
    near.into_iter().map(|(_, _, c)| c).collect()
}

fn visible_viewpoint(line: usize, mid: Point, pos: Point, centres: &[Point], sight: &Sight, width: i32, heights: &[(f64, f64)]) -> Option<WallViewpoint> {
    nearest_centres(centres, mid).into_iter().find_map(|c| {
        let angle = (mid.1 - c.1).atan2(mid.0 - c.0).to_degrees();
        let seen = sight.view(&View { x: c.0, y: c.1, angle, width, heights });
        seen.drawn.contains(&line).then(|| WallViewpoint { score: dist(c, pos) + dist(c, mid), line, position: c, wall_middle: mid })
    })
}

fn best_viewpoint(visible: impl Iterator<Item = WallViewpoint>) -> Option<WallViewpoint> {
    visible.reduce(|best, next| if next.score < best.score { next } else { best })
}

/// Test independent walls in parallel, returning results in the original order so equal
/// scores select the same objective. The calling thread also evaluates one batch.
fn wall_viewpoints(walls: &[usize], viewpoint: impl Fn(usize) -> Option<WallViewpoint> + Sync) -> Vec<Option<WallViewpoint>> {
    let evaluate = |batch: &[usize]| batch.iter().copied().map(&viewpoint).collect::<Vec<_>>();
    let batches = (walls.len() / WALLS_PER_WORKER).min(MAX_VIEWPOINT_WORKERS);
    if batches < 2 {
        return evaluate(walls);
    }
    let workers = batches.min(std::thread::available_parallelism().map_or(1, |n| n.get()));
    if workers == 1 {
        return evaluate(walls);
    }
    std::thread::scope(|scope| {
        let batch_size = walls.len().div_ceil(workers);
        let (first, rest) = walls.split_at(batch_size);
        let pending: Vec<_> = rest
            .chunks(batch_size)
            .map(|batch| {
                let worker = std::thread::Builder::new().spawn_scoped(scope, || evaluate(batch));
                (batch, worker)
            })
            .collect();
        let mut results = evaluate(first);
        for (batch, worker) in pending {
            match worker {
                Ok(handle) => results.extend(handle.join().expect("viewpoint worker panicked")),
                Err(_) => results.extend(evaluate(batch)),
            }
        }
        results
    })
}

impl Explorer {
    /// Rooms not closed on the map yet: for each known sector with walls not seen, a reachable
    /// spot it can be seen from (checked by looking from there), nearest her first.
    pub(super) fn unfinished(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (mesh, m, k) = (&self.nav.mesh, &self.nav.map, &self.knowledge);
        let heights: Vec<(f64, f64)> =
            m.sectors.iter().enumerate().map(|(i, s)| k.seen_heights.get(&i).map_or((s.floor, s.ceiling), |h| (h.floor, h.ceiling))).collect();
        // (only spots the box can stand at: E4M7's was in a shut door)
        let seen = self.as_seen();
        let centres: Vec<Point> = reach.iter().map(|&p| mesh.poly(p).center).filter(|&c| self.standable_at(c, true, &seen)).collect();
        let sight = &k.sight;
        let rooms = k.unseen_walls();
        // A wall bordering two known rooms has the same viewpoint for both.
        let mut slots = vec![None; m.linedefs.len()];
        let mut walls = Vec::new();
        for &li in rooms.iter().flat_map(|(_, lines)| lines) {
            slots[li].get_or_insert_with(|| {
                walls.push(li);
                walls.len() - 1
            });
        }
        let width = k.width;
        let candidates = wall_viewpoints(&walls, |line| {
            let (a, b) = m.line_seg(line);
            visible_viewpoint(line, midpoint(a, b), pos, &centres, sight, width, &heights)
        });
        let mut out = Vec::new();
        for (s, lines) in rooms {
            let visible = lines.into_iter().filter_map(|line| {
                let slot = slots[line].expect("each unseen wall has a search slot");
                candidates[slot]
            });
            if let Some(best) = best_viewpoint(visible) {
                let mut o = Objective::new(Kind::Room, best.position, format!("look at a wall of sector {s} not seen yet"));
                o.reveals.insert(best.line as i64);
                o.look_at = Some(best.wall_middle);
                out.push(o);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_selection_matches_stable_sort_including_cutoff_ties() {
        let mut centres = vec![(1000.0, 0.0)];
        for r in (0..100).rev() {
            let r = f64::from(r);
            centres.extend([(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)]);
        }
        for len in [0, 1, ROOM_SPOTS, ROOM_SPOTS + 1, centres.len()] {
            for mid in [(0.0, 0.0), (10.0, -3.0), (768.0, 0.0)] {
                let input = &centres[..len];
                let mut expected: Vec<_> = input.iter().copied().filter(|&c| dist(c, mid) <= ROOM_LOOK_RANGE).collect();
                expected.sort_by(|a, b| dist(*a, mid).total_cmp(&dist(*b, mid)));
                expected.truncate(ROOM_SPOTS);
                assert_eq!(nearest_centres(input, mid), expected);
            }
        }
    }

    #[test]
    fn viewpoint_batches_preserve_wall_order_and_missing_results() {
        let walls: Vec<_> = (0..MAX_VIEWPOINT_WORKERS * WALLS_PER_WORKER + 1).rev().collect();
        let viewpoint = |line| (line % 3 != 0).then_some(WallViewpoint { score: 1.0, line, position: (0.0, 0.0), wall_middle: (1.0, 1.0) });
        for len in [0, WALLS_PER_WORKER, 2 * WALLS_PER_WORKER, walls.len()] {
            let batch = &walls[..len];
            let serial: Vec<_> = batch.iter().copied().map(viewpoint).collect();
            assert_eq!(wall_viewpoints(batch, viewpoint), serial);
        }
    }
    #[test]
    fn equally_scored_walls_keep_the_first_in_room_order() {
        let first = WallViewpoint { score: 2.0, line: 17, position: (0.0, 0.0), wall_middle: (1.0, 1.0) };
        let tied = WallViewpoint { line: 3, ..first };
        assert_eq!(best_viewpoint([first, tied].into_iter()), Some(first));
    }
}
