// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Her rockets, measured: what each one did, and what she and its target did between the
//! trigger and the launch 8 tics on. For the run's statistics only (`Combat::count`), by the
//! game's own state: nothing here is hers to know, and nothing she does goes by it.
//!
//! A rocket's counted when it's gone: it hurt what she was aiming at, hurt something else, or
//! hurt nothing. One that hurt nothing is put down to what moved while it was coming out: her
//! feet, her facing, its target; and how far from the target it went off.

use std::collections::{BTreeMap, VecDeque};

use super::{Combat, Move};
use crate::geom::{dist, Point};
use crate::motor::{heading, norm, Observation};

/// the launcher lets a rocket go this long after the trigger (its first frame)
const DELAY: i64 = 8;
/// a blast hurts within this of where it goes off, and a little for the thing's own size
const BLAST: f64 = 128.0 + 64.0;
/// moved this far between trigger and launch, she "moved"; turned this far, she "turned";
/// its target this far across her line, it "moved"
const STEPPED: f64 = 16.0;
const TURNED: f64 = 3.0;
const DRIFTED: f64 = 32.0;

/// Her and her target, one tic.
#[derive(Clone, Copy, Debug)]
struct Snap {
    tic: i64,
    at: Point,
    angle: f64,
    target: Option<(i32, Point)>,
    kind: Option<Move>,
}

/// A rocket in the air.
#[derive(Clone, Debug)]
struct Flight {
    press: Snap,
    launch: Snap,
    last: Point,
    velocity: Point,
}

/// The bookkeeping.
#[derive(Default)]
pub struct Rockets {
    lately: VecDeque<Snap>,
    flying: BTreeMap<i32, Flight>,
    /// gone off and counted, the thing still there as its blast plays out
    spent: std::collections::BTreeSet<i32>,
    health: BTreeMap<i32, i32>,
}

impl Combat {
    /// A tic of it (`obs`: the game as it is, not as she has it).
    pub(super) fn measure_rockets(&mut self, obs: &Observation) {
        let things: Vec<_> = obs.things.iter().flatten().collect();
        let place = |id: i32| things.iter().find(|t| t.id == id).map(|t| (t.x, t.y));
        let now = Snap {
            tic: obs.tic,
            at: obs.pos(),
            angle: obs.angle,
            target: self.target.and_then(|id| place(id).map(|p| (id, p))),
            kind: self.last_move,
        };
        // New in the air: with what was so at the trigger.
        self.rockets
            .spent
            .retain(|id| things.iter().any(|t| t.id == *id));
        for t in things.iter().filter(|t| t.name == "Rocket" && t.owner == 0) {
            let velocity = (t.vx, t.vy);
            if self.rockets.spent.contains(&t.id) {
                continue;
            }
            if let Some(f) = self.rockets.flying.get_mut(&t.id) {
                // (stopped: gone off, where it is)
                if velocity.0.hypot(velocity.1) >= 1.0 {
                    f.last = (t.x, t.y);
                    f.velocity = velocity;
                }
                continue;
            }
            let press = self
                .rockets
                .lately
                .iter()
                .find(|s| s.tic >= obs.tic - DELAY)
                .copied()
                .unwrap_or(now);
            self.rockets.flying.insert(
                t.id,
                Flight {
                    press,
                    launch: now,
                    last: (t.x, t.y),
                    velocity,
                },
            );
        }
        // Gone off (stopped, or gone altogether): what it did.
        let gone: Vec<(i32, Option<Point>)> = self
            .rockets
            .flying
            .keys()
            .copied()
            .filter_map(|id| match things.iter().find(|t| t.id == id) {
                None => Some((id, None)),
                Some(t) if t.vx.hypot(t.vy) < 1.0 => Some((id, Some((t.x, t.y)))),
                Some(_) => None,
            })
            .collect();
        for (id, stopped) in gone {
            let Some(f) = self.rockets.flying.remove(&id) else {
                continue;
            };
            if stopped.is_some() {
                self.rockets.spent.insert(id);
            }
            let burst = stopped.unwrap_or((f.last.0 + f.velocity.0, f.last.1 + f.velocity.1));
            let hurt: Vec<i32> = self
                .rockets
                .health
                .iter()
                .filter(|(m, &was)| {
                    let here = things.iter().find(|t| t.id == **m);
                    was > 0
                        && here.is_none_or(|t| t.health < was)
                        && here.is_none_or(|t| dist((t.x, t.y), burst) < BLAST)
                })
                .map(|(m, _)| *m)
                .collect();
            let aimed = f.press.target.map(|t| t.0);
            self.count("rockets", 1.0);
            // Every rocket by whether her feet moved while it was coming out, and how far its
            // target was: what the misses below are a share of.
            let feet = if dist(f.press.at, f.launch.at) > STEPPED {
                "her feet moving"
            } else {
                "her feet still"
            };
            let range = match f.press.target.map(|t| dist(t.1, f.press.at)) {
                None => "no target",
                Some(d) if d < 256.0 => "under 256",
                Some(d) if d < 640.0 => "256 to 640",
                Some(_) => "over 640",
            };
            let hit = aimed.is_some_and(|a| hurt.contains(&a));
            let did = if hit { "hurt it" } else { "didn't" };
            self.count(&format!("rockets with {feet}: {did}"), 1.0);
            self.count(&format!("rockets at {range}: {did}"), 1.0);
            self.count(&format!("rockets with {feet} at {range}: {did}"), 1.0);
            // And her own blast: how near she was to where it went off, and whether she'd
            // come nearer since the trigger.
            let (then, at_burst) = (dist(f.press.at, burst), dist(obs.pos(), burst));
            if at_burst < BLAST {
                self.count("rockets that went off within her blast's reach of her", 1.0);
                if then >= BLAST {
                    self.count(
                        "rockets that went off within her blast's reach of her: out of it at the trigger",
                        1.0,
                    );
                }
                if let Some(kind) = self.last_move {
                    self.count(
                        &format!("rockets that went off within her blast's reach of her: the move {kind:?}"),
                        1.0,
                    );
                }
            }
            self.count(
                "rockets: units she closed on where it went off",
                (then - at_burst).max(0.0),
            );
            if hit {
                self.count("rockets that hurt what she aimed at", 1.0);
                continue;
            }
            if !hurt.is_empty() {
                self.count("rockets that hurt something else", 1.0);
                continue;
            }
            self.count("rockets that hurt nothing", 1.0);
            let Some((target, was)) = f.press.target else {
                self.count("rockets that hurt nothing: no target at the trigger", 1.0);
                continue;
            };
            let stepped = dist(f.press.at, f.launch.at) > STEPPED;
            let turned = norm(f.launch.angle - f.press.angle).abs() > TURNED;
            // (its target's move across her line, trigger to launch)
            let then = f.launch.target.filter(|t| t.0 == target).map(|t| t.1);
            let across = then.map_or(0.0, |p| {
                let way = heading(f.press.at, was).to_radians();
                ((p.0 - was.0) * -way.sin() + (p.1 - was.1) * way.cos()).abs()
            });
            let drifted = across > DRIFTED;
            let switched = f.launch.target.map(|t| t.0) != Some(target);
            let why = match (stepped, turned, drifted, switched) {
                (_, _, _, true) => "her target another by the launch",
                (true, true, _, _) => "she stepped and turned",
                (true, false, _, _) => "she stepped",
                (false, true, true, _) => "she turned, it moved across",
                (false, true, false, _) => "she turned",
                (false, false, true, _) => "it moved across",
                (false, false, false, _) => "nothing moved",
            };
            self.count(&format!("rockets that hurt nothing: {why}"), 1.0);
            // How far off its target (where that was as it went off) it went off.
            let off = place(target).map_or(f64::INFINITY, |p| dist(p, burst));
            let by = if off < 96.0 {
                "under 96 from it"
            } else if off < 256.0 {
                "96 to 256 from it"
            } else {
                "over 256 from it"
            };
            self.count(&format!("rockets that hurt nothing: went off {by}"), 1.0);
            // And how far short of it or past it, along her line.
            let reach = dist(f.launch.at, burst);
            let far = place(target).map_or(0.0, |p| dist(f.launch.at, p));
            let depth = if reach < far - 96.0 {
                "short of it (something in the way)"
            } else if reach > far + 96.0 {
                "past it"
            } else {
                "beside it"
            };
            self.count(&format!("rockets that hurt nothing: {depth}"), 1.0);
            if let Some(kind) = f.press.kind {
                self.count(
                    &format!("rockets that hurt nothing: the move {kind:?}"),
                    1.0,
                );
            }
        }
        self.rockets.lately.push_back(now);
        while self.rockets.lately.len() > 12 {
            self.rockets.lately.pop_front();
        }
        self.rockets.health = things
            .iter()
            .filter(|t| crate::tactical::bestiary::monster(t.name).is_some())
            .map(|t| (t.id, t.health))
            .collect();
    }
}
