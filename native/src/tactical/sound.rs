//! Where a shot is heard: Doom's P_NoiseAlert. The noise floods out from the shooter's sector
//! through every two-sided line with an opening (a shut door stops it), and through at most one
//! line marked to block sound (it stops at the second). A monster in a sector it reached wakes
//! when it next looks about (unless it's deaf: an "ambush" monster wakes only on sight).
//!
//! This is how sound works in the game, the same everywhere: what Evil knows. The monsters she
//! counts as woken are only the ones she's seen.

use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::map::{MapData, ML_SOUNDBLOCK};
use crate::nav::Heights;

/// (from sector, the heights) -> where a shot is heard
type Heard = ((usize, Vec<Heights>), BTreeSet<usize>);

pub struct Hearing {
    /// per sector: (the sector on the other side, the line blocks sound)
    links: Vec<Vec<(usize, bool)>>,
    cache: RefCell<Option<Heard>>,
}

impl Hearing {
    pub fn new(m: &MapData) -> Hearing {
        let mut links: Vec<Vec<(usize, bool)>> = vec![Vec::new(); m.sectors.len()];
        for line in m.linedefs.iter().filter(|l| l.two_sided()) {
            let (Some(a), Some(b)) = (m.front_sector(line), m.back_sector(line)) else { continue };
            if a == b {
                continue;
            }
            let block = line.flags & ML_SOUNDBLOCK != 0;
            links[a].push((b, block));
            links[b].push((a, block));
        }
        Hearing { links, cache: RefCell::new(None) }
    }

    /// The sectors a shot from `sector` is heard in, the floors and ceilings as they are.
    pub fn heard(&self, sector: usize, heights: &[Heights]) -> BTreeSet<usize> {
        if let Some((key, out)) = self.cache.borrow().as_ref() {
            if key.0 == sector && key.1 == heights {
                return out.clone();
            }
        }
        // (P_RecursiveSound: a sector is gone into again only if it's reached through fewer
        // sound-blocking lines than before)
        let mut best: std::collections::BTreeMap<usize, u8> = std::collections::BTreeMap::from([(sector, 0)]);
        let mut stack = vec![(sector, 0u8)];
        while let Some((s, blocks)) = stack.pop() {
            if best.get(&s).copied().unwrap_or(2) < blocks {
                continue;
            }
            for &(other, block) in &self.links[s] {
                let f = heights[s].floor.max(heights[other].floor);
                let c = heights[s].ceiling.min(heights[other].ceiling);
                if c - f <= 0.0 {
                    continue; // (a shut door)
                }
                let b = if block { blocks + 1 } else { blocks };
                if b >= 2 || best.get(&other).copied().unwrap_or(2) <= b {
                    continue;
                }
                best.insert(other, b);
                stack.push((other, b));
            }
        }
        let out: BTreeSet<usize> = best.into_keys().collect();
        *self.cache.borrow_mut() = Some(((sector, heights.to_vec()), out.clone()));
        out
    }
}
