//! The heights a special can move sectors to, and how long it takes.
//!
//! Targets follow the vanilla source (p_doors.c, p_plats.c, p_floor.c, p_ceilng.c), worked
//! out from the heights of the sectors round them (the map's, or as they are now when given).
//! Boom generalized movers aren't predicted.

use std::sync::Arc;

use crate::map::{MapData, ML_TWOSIDED};
use crate::nav::{can_cross, Heights};

pub const TICRATE: f64 = 35.0;

/// Per sector moved: the heights it can be moved to (in the order found).
pub type Effects = Vec<(usize, Vec<Heights>)>;

const DOOR_OPEN: &[u16] = &[
    1, 2, 4, 26, 27, 28, 29, 31, 32, 33, 34, 46, 61, 63, 86, 90, 99, 103, 105, 106, 108, 109, 111,
    112, 114, 115, 117, 118, 133, 134, 135, 136, 137,
];
const DOOR_CLOSE: &[u16] = &[3, 16, 42, 50, 75, 76, 107, 110, 113, 116];
const LIFT: &[u16] = &[10, 21, 62, 88, 120, 121, 122, 123];
const PERPETUAL_LIFT: &[u16] = &[53, 87];
const FLOOR_LOWEST: &[u16] = &[23, 37, 38, 60, 82, 84];
const FLOOR_HIGHEST: &[u16] = &[19, 45, 83, 102];
const FLOOR_TURBO: &[u16] = &[36, 70, 71, 98];
const FLOOR_LOWEST_CEIL: &[u16] = &[5, 24, 64, 91, 101];
const FLOOR_LOWEST_CEIL_CRUSH: &[u16] = &[55, 56, 65, 94];
const FLOOR_NEXT_HIGHER: &[u16] = &[18, 20, 22, 47, 68, 69, 95, 119, 128, 129, 130, 131, 132];
const FLOOR_SHORTEST_TEX: &[u16] = &[30, 96];
const CEIL_TO_FLOOR: &[u16] = &[41, 43];
const CEIL_TO_FLOOR_8: &[u16] = &[44, 72];
const CEIL_HIGHEST: &[u16] = &[40];
const CRUSHER: &[u16] = &[6, 25, 49, 73, 77, 141];
/// raiseAndChange, raiseToNearestAndChange: plats, slower
const PLAT_CHANGE: &[u16] = &[14, 15, 20, 22, 47, 66, 67, 68, 95];
/// close, wait 30 s, open
const DOOR_CLOSE_30: &[u16] = &[16, 76];

fn floor_raise(special: u16) -> Option<f64> {
    match special {
        14 | 67 => Some(32.0),
        15 | 58 | 59 | 66 | 92 | 93 => Some(24.0),
        140 => Some(512.0),
        _ => None,
    }
}

/// Does it go once and come back to where it was (a door that waits and closes, or closes and
/// opens again; a lift down and up)? Not a crusher, nor a lift that goes up and down for ever.
pub fn cycles_back(special: u16, description: &str) -> bool {
    LIFT.contains(&special)
        || DOOR_CLOSE_30.contains(&special)
        || ((DOOR_OPEN.contains(&special) || DOOR_CLOSE.contains(&special))
            && description.contains("wait"))
}

/// Stairs: the step height.
pub fn stairs_step(special: u16) -> Option<f64> {
    match special {
        7 | 8 => Some(8.0),
        100 | 127 => Some(16.0),
        _ => None,
    }
}

pub fn is_stairs(special: u16) -> bool {
    stairs_step(special).is_some()
}

/// Specials whose target depends on the heights round them (or their own) when they fire (a
/// door opens to the lowest ceiling round it then, less 4: MAP14's, by a crusher stopped low,
/// opened 42 high and she couldn't get under it).
pub fn depends_on_heights(special: u16) -> bool {
    [
        FLOOR_LOWEST,
        FLOOR_HIGHEST,
        FLOOR_TURBO,
        FLOOR_LOWEST_CEIL,
        FLOOR_LOWEST_CEIL_CRUSH,
        FLOOR_NEXT_HIGHER,
        FLOOR_SHORTEST_TEX,
        CEIL_HIGHEST,
        DOOR_OPEN,
    ]
    .iter()
    .any(|set| set.contains(&special))
        || floor_raise(special).is_some()
}

// vanilla speeds, units a tic (p_spec.h, p_plats.c)
const DOOR_SPEED: f64 = 2.0;
const DOOR_BLAZE: f64 = 8.0;
const PLAT_SPEED: f64 = 4.0;
const PLAT_BLAZE: f64 = 8.0;
const PERPETUAL_SPEED: f64 = 1.0;
const PLAT_CHANGE_SPEED: f64 = 0.5;
const FLOOR_SPEED: f64 = 1.0;
const FLOOR_FAST: f64 = 4.0;
/// VDOORWAIT
const DOOR_WAIT: f64 = 150.0;
/// PLATWAIT
const PLAT_WAIT: f64 = 3.0 * TICRATE;

/// How a mover gets to its target: `travel` tics (from the initial heights), then it stays
/// `hold` tics before going back (None: for good).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub travel: f64,
    pub hold: Option<f64>,
}

pub struct MoverModel {
    pub map: Arc<MapData>,
    /// per sector: the sectors round it
    neighbors: Vec<Vec<usize>>,
    /// the map's heights, which movers go by
    pub real: Vec<Heights>,
    /// the planner's idea of where the player can stand, which can differ
    /// (Navigator's trenches)
    pub initial: Vec<Heights>,
}

/// Heights now, where not the map's, by sector.
pub type HeightsNow<'a> = Option<&'a dyn Fn(usize) -> Option<Heights>>;

impl MoverModel {
    pub fn new(map: Arc<MapData>, neighbors: Vec<Vec<usize>>) -> Self {
        let real: Vec<Heights> = map
            .sectors
            .iter()
            .map(|s| Heights {
                floor: s.floor,
                ceiling: s.ceiling,
            })
            .collect();
        MoverModel {
            initial: real.clone(),
            real,
            neighbors,
            map,
        }
    }

    pub fn neighbors(&self, s: usize) -> &[usize] {
        &self.neighbors[s]
    }

    fn at(&self, s: usize, now: HeightsNow<'_>) -> Heights {
        now.and_then(|f| f(s)).unwrap_or(self.real[s])
    }

    /// Heights each affected sector can take once the special fires (several for movers that
    /// go back and forth; doors that close again report only the open state: the closed one
    /// is where they start). `now`: sectors' heights as they are, where not the map's.
    pub fn effects(&self, special: u16, sectors: &[usize], now: HeightsNow<'_>) -> Effects {
        if let Some(step) = stairs_step(special) {
            return self.stairs(sectors, step);
        }
        sectors
            .iter()
            .filter_map(|&s| {
                let states = self.sector_effect(special, s, now);
                (!states.is_empty()).then_some((s, states))
            })
            .collect()
    }

    fn sector_effect(&self, special: u16, s: usize, now: HeightsNow<'_>) -> Vec<Heights> {
        let Heights {
            floor,
            ceiling: ceil,
        } = self.at(s, now);
        let nf: Vec<f64> = self.neighbors[s]
            .iter()
            .map(|&n| self.at(n, now).floor)
            .collect();
        let nc: Vec<f64> = self.neighbors[s]
            .iter()
            .map(|&n| self.at(n, now).ceiling)
            .collect();
        let min = |v: &[f64], d: f64| {
            v.iter()
                .copied()
                .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
                .unwrap_or(d)
        };
        let max = |v: &[f64], d: f64| {
            v.iter()
                .copied()
                .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
                .unwrap_or(d)
        };
        let h = |floor, ceiling| vec![Heights { floor, ceiling }];
        let is = |set: &[u16]| set.contains(&special);
        if is(DOOR_OPEN) {
            return h(floor, min(&nc, ceil) - 4.0);
        }
        if is(DOOR_CLOSE) {
            return h(floor, floor);
        }
        if is(LIFT) || is(FLOOR_LOWEST) {
            let low = min(&nf, floor).min(floor);
            let mut out = h(low, ceil);
            if is(LIFT) {
                out.extend(self.step_offs(s, low, floor, ceil, now));
            }
            return out;
        }
        if is(PERPETUAL_LIFT) {
            return vec![
                Heights {
                    floor: min(&nf, floor).min(floor),
                    ceiling: ceil,
                },
                Heights {
                    floor: max(&nf, floor).max(floor),
                    ceiling: ceil,
                },
            ];
        }
        if is(FLOOR_HIGHEST) {
            return h(max(&nf, -500.0), ceil);
        }
        if is(FLOOR_TURBO) {
            let dest = max(&nf, -500.0);
            return h(if dest != floor { dest + 8.0 } else { dest }, ceil);
        }
        if is(FLOOR_LOWEST_CEIL) || is(FLOOR_LOWEST_CEIL_CRUSH) {
            let dest = min(&nc, ceil).min(ceil);
            return h(
                if is(FLOOR_LOWEST_CEIL_CRUSH) {
                    dest - 8.0
                } else {
                    dest
                },
                ceil,
            );
        }
        if is(FLOOR_NEXT_HIGHER) {
            let higher: Vec<f64> = nf.iter().copied().filter(|&f| f > floor).collect();
            return h(min(&higher, floor), ceil);
        }
        if let Some(by) = floor_raise(special) {
            return h(floor + by, ceil);
        }
        if is(FLOOR_SHORTEST_TEX) {
            return h(floor + self.shortest_lower_texture(s), ceil);
        }
        if is(CEIL_TO_FLOOR) {
            return h(floor, floor);
        }
        if is(CEIL_TO_FLOOR_8) || is(CRUSHER) {
            return h(floor, floor + 8.0);
        }
        if is(CEIL_HIGHEST) {
            return h(floor, max(&nc, ceil).max(ceil));
        }
        Vec::new()
    }

    /// A lift goes through every height between its ends: where a sector beside it can be
    /// stepped onto only on the way (its floor between the lift's two, out of reach from either
    /// end), the lift level with it is a height it has too (MAP17: the strip to the megasphere's
    /// secret, a step off the blazing lift as it passes, under a ceiling it's above at the top).
    /// Only those: every other lift stays two heights.
    fn step_offs(
        &self,
        s: usize,
        low: f64,
        high: f64,
        ceiling: f64,
        now: HeightsNow<'_>,
    ) -> Vec<Heights> {
        let (at_low, at_high) = (
            Heights {
                floor: low,
                ceiling,
            },
            Heights {
                floor: high,
                ceiling,
            },
        );
        let mut out: Vec<Heights> = Vec::new();
        for &n in &self.neighbors[s] {
            let hn = self.at(n, now);
            if hn.floor <= low
                || hn.floor >= high
                || can_cross(at_low, hn)
                || can_cross(at_high, hn)
            {
                continue;
            }
            let level = Heights {
                floor: hn.floor,
                ceiling,
            };
            if can_cross(level, hn) && !out.contains(&level) {
                out.push(level);
            }
        }
        out
    }

    /// How long the special takes to move sector s from its initial heights to `target`, and
    /// how long it stays there (near enough: vanilla speeds, from rest).
    pub fn timing(&self, special: u16, description: &str, s: usize, target: Heights) -> Timing {
        let Heights { floor, ceiling } = self.initial[s];
        let fast = description.contains("blazing")
            || description.contains("turbo")
            || description.contains("fast");
        let moved = (target.floor - floor)
            .abs()
            .max((target.ceiling - ceiling).abs());
        let is = |set: &[u16]| set.contains(&special);
        let (speed, hold) = if is(DOOR_OPEN) || is(DOOR_CLOSE) {
            let hold = if is(DOOR_CLOSE_30) {
                Some(30.0 * TICRATE)
            } else if description.contains("wait") {
                Some(DOOR_WAIT)
            } else {
                None
            };
            (if fast { DOOR_BLAZE } else { DOOR_SPEED }, hold)
        } else if is(LIFT) {
            (if fast { PLAT_BLAZE } else { PLAT_SPEED }, Some(PLAT_WAIT))
        } else if is(PERPETUAL_LIFT) {
            (PERPETUAL_SPEED, Some(PLAT_WAIT))
        } else if is(PLAT_CHANGE) {
            (PLAT_CHANGE_SPEED, None)
        } else if is_stairs(special) {
            (if fast { FLOOR_FAST } else { FLOOR_SPEED / 4.0 }, None)
        } else if is(CRUSHER) {
            (if fast { 2.0 } else { 1.0 }, Some(0.0))
        } else {
            (if fast { FLOOR_FAST } else { FLOOR_SPEED }, None)
        };
        Timing {
            travel: (moved / speed).ceil() + 1.0,
            hold,
        }
    }

    /// The shortest lower texture on the sector's two-sided lines (both sides). Missing ones
    /// ("-") are skipped, as ZDoom does (vanilla counted texture 0); 64 if there's nothing to
    /// go by.
    pub fn shortest_lower_texture(&self, s: usize) -> f64 {
        let m = &self.map;
        let mut best: Option<i32> = None;
        for line in m
            .linedefs
            .iter()
            .filter(|l| m.front_sector(l) == Some(s) || m.back_sector(l) == Some(s))
        {
            if !line.two_sided() {
                continue;
            }
            for side in [line.front, line.back] {
                let tex = &m.sidedefs[usize::from(side)].lower;
                if !tex.is_empty() && tex != "-" {
                    if let Some(&h) = m.texture_heights.get(tex) {
                        best = Some(best.map_or(h, |b| b.min(h)));
                    }
                }
            }
        }
        f64::from(best.unwrap_or(64))
    }

    /// EV_BuildStairs: the tagged sector up a step, then on through two-sided lines whose front
    /// is the current step into back sectors with the same floor flat.
    fn stairs(&self, sectors: &[usize], step: f64) -> Effects {
        let m = &self.map;
        let mut out: Effects = Vec::new();
        let has = |out: &Effects, s: usize| out.iter().any(|(x, _)| *x == s);
        // (a dict, as Python's: a sector set again keeps its place)
        let set =
            |out: &mut Effects, s: usize, h: Heights| match out.iter_mut().find(|(x, _)| *x == s) {
                Some(e) => e.1 = vec![h],
                None => out.push((s, vec![h])),
            };
        for &start in sectors {
            let mut sec = start;
            let mut height = self.real[sec].floor + step;
            let texture = &m.sectors[sec].floor_tex;
            set(
                &mut out,
                sec,
                Heights {
                    floor: height,
                    ceiling: self.real[sec].ceiling,
                },
            );
            loop {
                let mut next = None;
                for line in m
                    .linedefs
                    .iter()
                    .filter(|l| m.front_sector(l) == Some(sec) || m.back_sector(l) == Some(sec))
                {
                    if line.flags & ML_TWOSIDED == 0 || m.front_sector(line) != Some(sec) {
                        continue;
                    }
                    let Some(back) = m.back_sector(line) else {
                        continue;
                    };
                    if &m.sectors[back].floor_tex != texture {
                        continue;
                    }
                    height += step;
                    if has(&out, back) {
                        continue;
                    }
                    next = Some(back);
                    break;
                }
                let Some(n) = next else { break };
                sec = n;
                set(
                    &mut out,
                    sec,
                    Heights {
                        floor: height,
                        ceiling: self.real[sec].ceiling,
                    },
                );
            }
        }
        out
    }
}
