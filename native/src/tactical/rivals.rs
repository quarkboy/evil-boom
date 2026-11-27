// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A deathmatch: where the others might be, by what she's seen and heard of them, and whether
//! she's equipped to go after them. No more than a player would know: Doom's deathmatch automap
//! shows no one, and the game's own word on who made a sound isn't heard. A rival seen is where
//! she saw them; a sound only a player makes is a player, about as far off as it's loud, on the
//! side it came from (in front and behind sound the same); in a game without monsters a gun
//! any of them fire, a door or a lift she didn't set going, is a player too.

use crate::domain::{Observation, Thing};
use crate::geom::{dist, Point};
use crate::knowledge::model::Heard;
use crate::navigator::Navigator;
use crate::tactical::tuning::Deathmatch;

/// Sounds only a player makes: the super shotgun's shot and reload, the chainsaw, the fist, the
/// BFG, a player's pain, death and grunts.
const PLAYER_ONLY: &[&str] = &[
    "dsdshtgn", "dsdbopn", "dsdbload", "dsdbcls", "dssawup", "dssawidl", "dssawful", "dssawhit",
    "dspunch", "dsbfg", "dsplpain", "dspldeth", "dspdiehi", "dsoof", "dsnoway",
];

/// Sounds a player makes and monsters too (a pistol, a shotgun, a rocket, the plasma rifle, a
/// teleport's flash, someone born again): a player's only where there are no monsters.
const SHARED: &[&str] = &["dspistol", "dsshotgn", "dsrlaunc", "dsplasma", "dstelept"];

/// as far as a sound is heard (Doom's S_CLIPPING_DIST)
const HEAR_FAR: f64 = 1200.0;
/// a clue put this far at most from the nearest floor she knows (else it's in a wall: not there)
const SNAP: f64 = 128.0;

/// Somewhere a rival was, or might have been.
#[derive(Clone, Debug)]
pub struct Clue {
    /// its own number (kept while it's heard again)
    pub id: i64,
    pub at: Point,
    pub tic: i64,
    /// what it was, as she'd say it ("seen", "a super shotgun", "a door")
    pub what: String,
    /// 1 seen; less for a sound placed one of two ways
    pub sure: f64,
}

/// What she knows of the others in a deathmatch.
#[derive(Clone, Debug, Default)]
pub struct Rivals {
    /// there are monsters in this game: a gun's sound or a door may be theirs
    pub monsters: bool,
    pub clues: Vec<Clue>,
    /// geared up and going after them (else gearing up)
    pub hunting: bool,
    /// told to hunt them (directed): gone after whatever she's got
    pub ordered: bool,
    /// the heard sounds of movers taken so far (`Knowledge::heard`)
    pub heard_seen: usize,
    /// the tic a hunt last took her off something else (not again at once)
    pub interrupted: i64,
    /// the best clue as of then (a newer one is news to choose again for)
    pub offered: Option<i64>,
    next_id: i64,
}

impl Rivals {
    pub fn new(monsters: bool) -> Rivals {
        Rivals {
            monsters,
            interrupted: i64::MIN / 2,
            ..Rivals::default()
        }
    }

    /// This tic's news of them: those in sight (`visible`: what she sees), the sounds she heard
    /// (a player's), the doors and lifts newly heard going that weren't hers nor going by
    /// themselves (`movers`), where each might be. Lines for her log, if anything's new.
    pub fn sense(
        &mut self,
        obs: &Observation,
        visible: &[Thing],
        movers: &[Heard],
        nav: &Navigator,
        fov: f64,
        tune: &Deathmatch,
    ) -> Vec<String> {
        let mut log = Vec::new();
        let tic = obs.tic;
        let pos = obs.pos();
        self.clues
            .retain(|c| (tic - c.tic) as f64 <= tune.clue_secs * crate::domain::TICRATE);
        for t in visible.iter().filter(|t| t.name == "Opponent") {
            let at = (t.x, t.y);
            // (seen: where they are now. The clue of them nearest, moved along with them, the
            // same clue: a hunt after it goes on after them, not chosen again each tic)
            let near = self
                .clues
                .iter()
                .enumerate()
                .filter(|(_, c)| dist(c.at, at) <= tune.spent_near)
                .min_by(|a, b| dist(a.1.at, at).total_cmp(&dist(b.1.at, at)))
                .map(|(i, _)| i);
            let id = match near {
                Some(i) => self.clues[i].id,
                None => {
                    self.next_id += 1;
                    self.next_id
                }
            };
            self.clues
                .retain(|c| c.id == id || dist(c.at, at) > tune.spent_near);
            match self.clues.iter_mut().find(|c| c.id == id) {
                Some(c) => {
                    (c.at, c.tic, c.sure) = (at, tic, 1.0);
                    c.what = "seen".into();
                }
                None => self.clues.push(Clue {
                    id,
                    at,
                    tic,
                    what: "seen".into(),
                    sure: 1.0,
                }),
            }
        }
        for s in obs.sounds.iter().filter(|s| !s.own) {
            let theirs =
                PLAYER_ONLY.contains(&s.name) || (!self.monsters && SHARED.contains(&s.name));
            if !theirs {
                continue;
            }
            let far = if s.volume >= 127 {
                160.0
            } else {
                HEAR_FAR - f64::from(s.volume) * 1040.0 / 127.0
            };
            // (the side by the balance between her ears: the same in front and behind, so in
            // front only if that's out of her view, where she'd have seen them)
            let off = ((128.0 - f64::from(s.separation)) / 96.0)
                .clamp(-1.0, 1.0)
                .asin()
                .to_degrees();
            let mut ways = vec![obs.angle + 180.0 - off];
            if off.abs() >= fov {
                ways.push(obs.angle + off);
            }
            let places: Vec<Point> = ways
                .into_iter()
                .filter_map(|w| {
                    let p = (
                        pos.0 + far * w.to_radians().cos(),
                        pos.1 + far * w.to_radians().sin(),
                    );
                    snap(nav, p)
                })
                .collect();
            let what = called(s.name);
            let sure = 1.0 / places.len().max(1) as f64;
            for at in places {
                self.add(tic, at, what, sure, tune);
            }
            log.push(format!(
                "tic {tic}: heard a player ({what}), about {far:.0} away"
            ));
        }
        // (a door or a lift going that wasn't hers, out of sight: someone set it going)
        if !self.monsters {
            for h in movers {
                if let Some(at) = snap(nav, h.origin) {
                    self.add(tic, at, "a door or lift", 0.7, tune);
                    log.push(format!(
                        "tic {tic}: heard something set going, about {:.0} away: a player?",
                        h.distance
                    ));
                }
            }
        }
        log
    }

    /// A clue, unless there's one as good there already.
    fn add(&mut self, tic: i64, at: Point, what: &str, sure: f64, tune: &Deathmatch) {
        if let Some(c) = self
            .clues
            .iter_mut()
            .find(|c| dist(c.at, at) < tune.spent_near / 2.0)
        {
            c.tic = tic;
            c.sure = c.sure.max(sure);
            c.what = what.to_string();
            return;
        }
        self.next_id += 1;
        self.clues.push(Clue {
            id: self.next_id,
            at,
            tic,
            what: what.to_string(),
            sure,
        });
    }

    /// The clue to go after from `pos`: the freshest and surest, the nearer of two alike.
    pub fn best(&self, tic: i64, pos: Point, tune: &Deathmatch) -> Option<&Clue> {
        let life = tune.clue_secs * crate::domain::TICRATE;
        self.clues
            .iter()
            .map(|c| {
                let fresh = 1.0 - (tic - c.tic) as f64 / life;
                (c.sure * fresh - dist(pos, c.at) / 4000.0, c)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, c)| c)
    }

    /// She's been where a clue was and no one was there: it, and those near it, done with.
    pub fn spent(&mut self, at: Point, tune: &Deathmatch) {
        self.clues.retain(|c| dist(c.at, at) > tune.spent_near);
    }

    /// The clue a hunt is after, by its number (`Objective::reveals`).
    pub fn by_id(&self, id: i64) -> Option<&Clue> {
        self.clues.iter().find(|c| c.id == id)
    }
}

/// The nearest floor she could stand on to `p` (its middle if `p` is on none), within `SNAP`.
fn snap(nav: &Navigator, p: Point) -> Option<Point> {
    if nav.poly_at(p).is_some() {
        return Some(p);
    }
    let mesh = &nav.mesh;
    mesh.polys
        .iter()
        .flatten()
        .map(|q| q.center)
        .filter(|&c| dist(c, p) <= SNAP)
        .min_by(|a, b| dist(*a, p).total_cmp(&dist(*b, p)))
}

/// A player's sound as she'd name it.
fn called(name: &str) -> &'static str {
    match name {
        "dsdshtgn" | "dsdbopn" | "dsdbload" | "dsdbcls" => "a super shotgun",
        "dssawup" | "dssawidl" | "dssawful" | "dssawhit" => "a chainsaw",
        "dspunch" => "a punch",
        "dsbfg" => "a BFG",
        "dsplpain" | "dsoof" | "dsnoway" => "a player's grunt",
        "dspldeth" | "dspdiehi" => "a player dying",
        "dspistol" => "a pistol",
        "dsshotgn" => "a shotgun",
        "dsrlaunc" => "a rocket",
        "dsplasma" => "a plasma rifle",
        "dstelept" => "a teleport",
        _ => "a player",
    }
}

/// Equipped to hunt (`tune`): a gun past the pistol with enough for it, and health enough. With
/// `half`, half the ammo, and health over what sends a hunter back to gearing up.
pub fn geared(obs: &Observation, tune: &Deathmatch, half: bool) -> bool {
    let ammo = |slot: i32| {
        obs.ammo
            .iter()
            .flatten()
            .find(|(k, _)| *k == slot)
            .map_or(0, |(_, n)| *n)
    };
    let need = |n: i32| if half { (n + 1) / 2 } else { n };
    let gun = obs.owned.iter().flatten().any(|w| match w {
        2 | 8 => ammo(3) >= need(tune.shells),
        3 => ammo(4) >= need(tune.bullets),
        4 => ammo(5) >= need(tune.rockets),
        5 | 6 => ammo(6) >= need(tune.cells),
        _ => false,
    });
    let health = obs.health.unwrap_or(0.0);
    let armor = obs.armor.unwrap_or(0.0);
    let body = if half {
        health > tune.weak_health
    } else {
        health >= tune.health || (health >= tune.armored_health && armor >= tune.armor)
    };
    gun && body
}

/// Too weak to hunt on: hurt below `weak_health`, or nothing past the pistol to shoot with.
pub fn weak(obs: &Observation, tune: &Deathmatch) -> bool {
    let ammo = |slot: i32| {
        obs.ammo
            .iter()
            .flatten()
            .find(|(k, _)| *k == slot)
            .map_or(0, |(_, n)| *n)
    };
    let gun = obs.owned.iter().flatten().any(|w| match w {
        2 | 8 => ammo(3) > 0,
        3 => ammo(4) > 0,
        4 => ammo(5) > 0,
        5 | 6 => ammo(6) > 0,
        _ => false,
    });
    !gun || obs.health.unwrap_or(0.0) < tune.weak_health
}
