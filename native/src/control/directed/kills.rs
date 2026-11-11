//! Kills and blasts worth telling: a heavy monster dead, several dead at once (and how many of
//! them in pieces), barrels going off and what they took with them, her own blast catching her,
//! the level's last monster.
//!
//! A death is one she knows of (the fight's: seen, or heard and placed); gibbed counts only for
//! one that was in her sight as it died. A barrel going off is one near enough to hear. What
//! happens together is told together, once it's over: a chain of barrels and everything it
//! killed is one line, not six.

use std::collections::{BTreeMap, BTreeSet};

use super::{compass, Director};
use crate::dsda::HitCause;
use crate::geom::{dist, Point};
use crate::motor::{heading, Observation};
use crate::play::Agent;
use crate::tactical::bestiary::monster;

/// deaths this close together (tics) are "at once": one shot's, one blast's
const TOGETHER: i64 = 3;
/// a chain of barrels: over when none has gone off for this long (a barrel hit blows 15 tics
/// later, and its blast takes a moment to light the next)
const CHAIN_OVER: i64 = 30;
/// a barrel going off is heard this far (the game's sounds carry 1200 units)
const HEARD: f64 = 1200.0;
/// a barrel's blast reaches 128 units from it, to a monster's edge: one dead this near one that
/// went off, as it went off, is taken for the blast's
const BLAST: f64 = 192.0;
/// the same barrel a tic on (one hit and not yet blown slides a little)
const SAME: f64 = 16.0;
/// seen to die: in her sight within this (tics) of being known dead
const SEEN: i64 = 35;
/// her own blast: told once nothing more of it has hurt her for this long (tics)...
const OWN_OVER: i64 = 10;
/// ...and worth a reaction from this much health
const OWN_NOTABLE: i32 = 20;
/// a monster's death told by itself: a boss, or one with this much health (a hell knight and
/// up: one that took some killing)
const HEAVY: i32 = 500;

/// What's been told, and what's happening that's told when it's over.
#[derive(Default)]
pub(super) struct Kills {
    /// the dead already counted
    told: BTreeSet<i32>,
    /// the barrels as they were last tic
    barrels: Option<Vec<Point>>,
    burst: Option<Burst>,
    /// her own blast hurting her: (when last, health lost)
    own: Option<(i64, i32)>,
    told_all_dead: bool,
}

/// Deaths and barrels going off together.
struct Burst {
    last: i64,
    barrels: Vec<Point>,
    /// (kind, where, in pieces)
    dead: Vec<(&'static str, Point, bool)>,
    /// what the barrels cost her
    hurt: i32,
}

/// "2 DoomImp, 1 Zombieman", and how many of them in pieces.
fn tally(dead: &[&(&'static str, Point, bool)]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for d in dead {
        *counts.entry(d.0).or_insert(0) += 1;
    }
    let names: Vec<String> = counts.iter().map(|(n, c)| format!("{c} {n}")).collect();
    let gibbed = dead.iter().filter(|d| d.2).count();
    let gibs = match gibbed {
        0 => String::new(),
        g if g == dead.len() && g > 1 => ", all gibbed".to_string(),
        g => format!(", {g} gibbed"),
    };
    format!("{}{gibs}", names.join(", "))
}

impl Director {
    /// Each tic: the deaths, barrels and hits since the last, gathered; what's over, told.
    pub(super) fn kills(&mut self, agent: &Agent, obs: &Observation) {
        let pos = obs.pos();
        let tic = obs.tic;
        let mut k = std::mem::take(&mut self.kills);
        let join = |k: &mut Kills| {
            let b = k.burst.get_or_insert(Burst {
                last: tic,
                barrels: Vec::new(),
                dead: Vec::new(),
                hurt: 0,
            });
            b.last = tic;
        };

        // The newly dead (one raised again and killed again is new again).
        let deaths = agent
            .player()
            .map(|p| p.combat().known_deaths())
            .unwrap_or_default();
        k.told.retain(|i| deaths.iter().any(|d| d.0 == *i));
        for (id, name, at, sighted) in deaths {
            if !k.told.insert(id) {
                continue;
            }
            let gibbed = self.gibbed.contains(&id) && tic - sighted <= SEEN;
            if monster(name).is_some_and(|m| m.boss || m.health >= HEAVY) {
                self.event(obs, true, format!("The {name} is dead."));
            }
            join(&mut k);
            if let Some(b) = k.burst.as_mut() {
                b.dead.push((name, at, gibbed));
            }
        }

        // Barrels gone since last tic, near enough to hear.
        let now: Option<Vec<Point>> = obs.barrels.clone();
        if let (Some(before), Some(now)) = (&k.barrels, &now) {
            let gone: Vec<Point> = before
                .iter()
                .filter(|b| !now.iter().any(|n| dist(**b, *n) <= SAME))
                .take(before.len().saturating_sub(now.len()))
                .filter(|b| dist(pos, **b) <= HEARD)
                .copied()
                .collect();
            if !gone.is_empty() {
                join(&mut k);
                if let Some(b) = k.burst.as_mut() {
                    b.barrels.extend(gone);
                }
            }
        }
        k.barrels = now;

        // What hurt her: a barrel's blast is the burst's; her own rocket's is told apart.
        let barrel = crate::dsda::type_of("ExplosiveBarrel");
        for h in std::mem::take(&mut self.hits) {
            if h.cause != HitCause::Thing {
                continue;
            }
            if h.inflictor.is_some() && h.inflictor == barrel {
                join(&mut k);
                if let Some(b) = k.burst.as_mut() {
                    b.hurt += h.health;
                }
            } else if h.own {
                let had = k.own.map_or(0, |o| o.1);
                k.own = Some((tic, had + h.health));
            }
        }

        // Over: told.
        let lit = obs.barrels_lit.iter().any(|b| dist(pos, *b) <= HEARD);
        let over = k.burst.as_ref().is_some_and(|b| {
            if b.barrels.is_empty() && b.hurt == 0 {
                tic - b.last >= TOGETHER
            } else {
                !lit && tic - b.last >= CHAIN_OVER
            }
        });
        if let Some(b) = k.burst.take_if(|_| over) {
            let way = |p: Point| format!("{} {:.0}", compass(heading(pos, p)), dist(pos, p));
            let blasted = b.hurt > 0 || !b.barrels.is_empty();
            let (by_blast, rest): (Vec<_>, Vec<_>) = b
                .dead
                .iter()
                .partition(|d| b.barrels.iter().any(|p| dist(*p, d.1) <= BLAST));
            if blasted {
                let what = match b.barrels.len() {
                    0 => "A barrel went off by you".to_string(),
                    1 => format!("A barrel went off ({})", way(b.barrels[0])),
                    n => format!("{n} barrels went off in a chain ({})", way(b.barrels[0])),
                };
                let killed = if by_blast.is_empty() {
                    String::new()
                } else {
                    format!(": killed {}", tally(&by_blast))
                };
                let hurt = if b.hurt > 0 {
                    format!("; it cost you {} health", b.hurt)
                } else {
                    String::new()
                };
                // (one barrel that took nothing with it is her clearing barrels: routine)
                if b.barrels.len() >= 2 || !by_blast.is_empty() || b.hurt > 0 {
                    self.event(obs, true, format!("{what}{killed}{hurt}."));
                }
            }
            match rest.len() {
                0 => {}
                1 if rest[0].2 => self.event(obs, false, format!("{} gibbed.", rest[0].0)),
                1 => {}
                n => self.event(obs, true, format!("{n} killed at once: {}.", tally(&rest))),
            }
        }
        if let Some((_, health)) = k.own.take_if(|o| tic - o.0 >= OWN_OVER) {
            self.event(
                obs,
                health >= OWN_NOTABLE,
                format!("Your own blast caught you: -{health} health."),
            );
        }

        // The level's last monster.
        let (dead, of) = self.tally.kills;
        if of > 0 && dead >= of && !k.told_all_dead {
            k.told_all_dead = true;
            self.event(
                obs,
                true,
                format!("Every monster on the level is dead ({dead}/{of})."),
            );
        }
        self.kills = k;
    }
}
