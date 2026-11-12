// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The expert's run: every item that counts towards the tally picked up, then the exit (no
//! monsters). The explorer's own way round, with the exit held back till nothing else is left
//! and the items she has seen and can get to offered as things to do. In vanilla a second
//! computer area map can't be picked up (the power's had already), though it counts: such a
//! level can't be 100%.

use std::collections::BTreeSet;

use super::explorer::{Explorer, Hooks, Kind, Objective};
use crate::geom::Point;
use crate::map::MapData;
use crate::motor::Observation;
use crate::navigator::Plan;
use crate::specials::Keys;

/// the computer area map's thing type
const ALLMAP: u16 = 2026;

/// What counts towards the items tally (MF_COUNTITEM in the game's tables).
pub const COUNTED: [&str; 9] = [
    "HealthBonus",
    "ArmorBonus",
    "Soulsphere",
    "Megasphere",
    "Berserk",
    "Allmap",
    "Infrared",
    "InvulnerabilitySphere",
    "BlurSphere",
];

/// How many fewer than the level's item total is the best there is: every computer area map
/// after the first.
pub fn most_items(m: &MapData, skill: i32) -> i32 {
    let maps = m
        .things
        .iter()
        .filter(|t| t.kind == ALLMAP && t.in_skill(skill))
        .count();
    i32::try_from(maps.saturating_sub(1)).unwrap_or(0)
}

/// Hooks an explorer up to collect every counted item before it leaves.
pub struct Completionist {
    /// levels again she may ask for
    pub restarts: i32,
    /// what she knew and the items left when the level from its start last had no way to them
    no_way_back: Option<(String, Vec<Point>)>,
    /// the items left when she last asked for the level again for them
    asked_for: Option<Vec<Point>>,
}

impl Completionist {
    /// the explorer's exit is held back
    pub fn new(explorer: &mut Explorer, restarts: i32) -> Completionist {
        explorer.exit_held_back = true;
        Completionist {
            restarts,
            no_way_back: None,
            asked_for: None,
        }
    }

    /// Does a thing of this name count for the items tally now (a computer map after the first doesn't)?
    pub fn counted(name: &str, obs: &Observation) -> bool {
        COUNTED.contains(&name) && !(name == "Allmap" && obs.allmap == Some(true))
    }
}

impl Hooks for Completionist {
    fn wanted(
        &mut self,
        x: &Explorer,
        obs: &Observation,
        reach: &BTreeSet<usize>,
        _pos: Point,
    ) -> Vec<Objective> {
        let known = x.knowledge.known_polys();
        let mut out = Vec::new();
        for t in obs.things.iter().flatten() {
            if !Self::counted(t.name, obs) {
                continue;
            }
            // Seen. Where it is needn't be somewhere she can get to, only somewhere to touch it from
            // (on a ledge she can reach up onto but not climb).
            let Some(p) = x.nav.poly_at((t.x, t.y)) else {
                continue;
            };
            if !known.contains(&p) {
                continue;
            }
            if let Some(spot) = x.touch_spot((t.x, t.y), reach) {
                let mut o = Objective::new(Kind::Item, spot, format!("pick up {}", t.name));
                o.thing = Some(t.id);
                out.push(o);
            }
        }
        out
    }

    fn gone(&mut self, obs: &Observation, obj: &Objective) -> bool {
        obj.kind == Kind::Item && !obs.things.iter().flatten().any(|t| Some(t.id) == obj.thing)
    }

    /// Items she's seen that she can't get to now but could from the start of the level, as far
    /// as she knows (a floor that rises to them only with her on it): rather the level again,
    /// while she may, than out without them.
    fn before_exit(&mut self, x: &Explorer, obs: &Observation) -> bool {
        if self.restarts <= 0 {
            return false;
        }
        let known = x.knowledge.known_polys();
        let left: Vec<Point> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| {
                Self::counted(t.name, obs)
                    && x.nav
                        .poly_at((t.x, t.y))
                        .is_some_and(|p| known.contains(&p))
            })
            .map(|t| (t.x, t.y))
            .collect();
        if left.is_empty() {
            return false;
        }
        // No way to them from the start then, knowing what she does now: none now. This is asked
        // every think while there's nothing else to do, a whole plan each time.
        let asked = (x.knowledge.summary(), left.clone());
        if self.no_way_back.as_ref() == Some(&asked) {
            return false;
        }
        let Some(start) = x
            .nav
            .map
            .things
            .iter()
            .find(|t| t.kind == 1)
            .map(|t| (t.x, t.y))
        else {
            return false;
        };
        let view = x.fresh_view();
        let states = view.initial_states();
        let route = view.plan(
            start,
            &left,
            &Plan {
                keys: Keys::default(),
                states: Some(&states),
                max_expansions: 20000,
                max_seconds: 600.0,
                ..Plan::default()
            },
        );
        if route.is_none() {
            self.no_way_back = Some(asked);
            return false;
        }
        // The level again for the same ones it last didn't get her: it won't now either.
        if self.asked_for.as_ref() == Some(&left) {
            return false;
        }
        self.asked_for = Some(left);
        self.restarts -= 1;
        true
    }
}
