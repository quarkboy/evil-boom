// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The monsters she knows of and isn't looking at: where each was and would be by now, which
//! she can't reach, which are dead, which are against her.

use super::*;

impl Combat {
    /// Where a monster out of her sight is, as she'd reckon it: where she last saw it, and on
    /// the way it was going for as long as `senses.reckon_tics` allows.
    pub(super) fn reckoned(&self, s: &Seen, tic: i64) -> Point {
        let dt = (tic - s.tic).clamp(0, self.tune.senses.reckon_tics) as f64;
        (s.x + s.going.0 * dt, s.y + s.going.1 * dt)
    }

    /// The monsters she can't see now but has in mind: seen awake within `senses.memory_tics`,
    /// not known dead, not found gone from where she left them; each as she last saw it, where
    /// she last saw it, doing nothing she knows of.
    pub(super) fn remembered(&self, obs: &Observation) -> Vec<Thing> {
        self.known
            .values()
            .filter(|s| {
                s.awake
                    && !s.lost
                    && !self.dead.contains(&s.id)
                    && obs.tic - s.tic <= self.tune.senses.memory_tics
                    && mon(s.name).attack != Attack::None
            })
            .filter(|s| !self.visible.iter().any(|t| t.id == s.id))
            .filter_map(|s| {
                self.last_seen
                    .get(&s.id)
                    .map(|t| (self.reckoned(s, obs.tic), t))
            })
            .map(|(at, t)| Thing {
                x: at.0,
                y: at.1,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
                attack: -1,
                ..t.clone()
            })
            .collect()
    }

    /// A monster that only claws or charges, in her sight a while with her staying where she is,
    /// and no nearer for it: it can't get at her (bars she sees through, a gap, a ledge). Not
    /// something to spend shots on while anything else is about.
    pub(super) fn cant_reach(&self, obs: &Observation, t: &Thing) -> bool {
        self.reach_watch.get(&t.id).is_some_and(|&(since, far, _)| {
            obs.tic - since > self.tune.engage.cant_reach_tics
                && dist((t.x, t.y), obs.pos()) > far - 64.0
                && dist((t.x, t.y), obs.pos()) > self.tune.engage.close
        })
    }

    /// One that walks, on a floor above hers or below it, with no way to walk to her from where
    /// it stands, by the floors as they stand: steps of 24 at most up or down (a monster doesn't walk off a ledge), not through
    /// lines that block it, looked for no further than `WITHIN` of her (a way that leaves that
    /// may come round: for all she knows it can). Up on a ledge over her or down in a pit, near
    /// as the crow flies: it won't be clawing her, and isn't backed off from, whatever it throws
    /// (that's dodged, as ever). Doom II MAP13's key room: thirteen imps on tiers 96 apart over
    /// the floor she crosses, each backed off from in its turn.
    pub fn kept_from_her(&self, obs: &Observation, t: &Thing) -> bool {
        /// how far from her a way round is looked for
        const WITHIN: f64 = 768.0;
        if matches!(t.name, "Cacodemon" | "LostSoul" | "PainElemental") {
            return false;
        }
        let pos = obs.pos();
        let (mesh, map) = (&self.nav.mesh, &self.nav.map);
        let (Some(from), Some(to)) = (self.nav.poly_at((t.x, t.y)), self.nav.poly_at(pos)) else {
            return false;
        };
        let floor = |p: usize| self.heights_now.get(mesh.poly(p).sector).map(|h| h.0);
        // Only one on another level than hers. Behind a fence on her own floor it's kept from
        // her as surely, and backing off from it is still worth having, for the room it gives
        // her to dodge what it throws from that near: excused too, E1M9's pen of imps by the
        // start cost 10 exits of 48 (25 to 15, three seed sets).
        if !floor(from)
            .zip(floor(to))
            .is_some_and(|(a, b)| (a - b).abs() > MAX_STEP)
        {
            return false;
        }
        let mut seen = BTreeSet::from([from]);
        let mut todo = vec![from];
        while let Some(p) = todo.pop() {
            if p == to || dist(mesh.poly(p).center, pos) > WITHIN {
                return false;
            }
            let Some(f) = floor(p) else {
                return false;
            };
            for &pi in &mesh.poly(p).portals {
                let portal = &mesh.portals[pi];
                if portal.linedef.is_some_and(|li| {
                    map.linedefs[li].flags & (ML_BLOCKING | ML_BLOCKMONSTERS) != 0
                }) {
                    continue;
                }
                let o = portal.other(p);
                let Some(of) = floor(o) else {
                    return false;
                };
                if (of - f).abs() <= MAX_STEP && seen.insert(o) {
                    todo.push(o);
                }
            }
        }
        true
    }

    /// Is p off the ground she's been on (a poly of the mesh she's never stood in, or none)?
    pub(super) fn off_known(&self, p: Point) -> bool {
        self.nav
            .poly_at(p)
            .is_none_or(|q| !self.been.get(q).copied().unwrap_or(false))
    }

    /// Is the route about to take her onto ground she's never been on (the way it walked her
    /// last tic, a few steps on)?
    pub(super) fn route_into_new(&self, obs: &Observation) -> bool {
        self.route_way.is_some_and(|w| {
            let (pos, w) = (obs.pos(), w.to_radians());
            [48.0, 96.0]
                .into_iter()
                .any(|far| self.off_known((pos.0 + w.cos() * far, pos.1 + w.sin() * far)))
        })
    }

    /// The monsters about her, awake, that are on ground she's never been on.
    pub fn woken_ahead(&self) -> Vec<Thing> {
        self.now
            .about
            .iter()
            .filter(|t| {
                self.known.get(&t.id).is_some_and(|k| k.awake) && self.off_known((t.x, t.y))
            })
            .cloned()
            .collect()
    }

    /// How many monsters she first saw within the last `tics`.
    pub fn newly_seen(&self, tic: i64, tics: i64) -> usize {
        self.known
            .values()
            .filter(|s| tic - s.first <= tics && !self.dead.contains(&s.id))
            .count()
    }

    /// The level known beforehand: every monster in it where it stands at the start, as if seen
    /// in a life before (`obs`: the game's own, the first of the level).
    pub fn know_monsters(&mut self, obs: &Observation) {
        for t in obs
            .things
            .iter()
            .flatten()
            .filter(|t| monster(t.name).is_some())
        {
            // One she's met before too: where it stands, not where she first saw it.
            self.known.insert(
                t.id,
                Seen {
                    id: t.id,
                    name: t.name,
                    x: t.x,
                    y: t.y,
                    z: t.z,
                    tic: i64::MIN / 2,
                    first: i64::MIN / 2,
                    x0: t.x,
                    y0: t.y,
                    angle0: t.angle,
                    awake: false,
                    lost: false,
                    going: (0.0, 0.0),
                    sighted: i64::MIN / 2,
                    foreknown: true,
                },
            );
        }
    }

    /// Every monster she knows of, forgotten (for measuring what knowing them is worth).
    pub fn forget_monsters(&mut self) {
        self.known.clear();
    }

    /// The counts kept for measuring a run, from nothing (what went before isn't the run's).
    pub fn forget_counts(&mut self) {
        self.tally.clear();
        (self.kills, self.spent, self.dealt) = (0, 0.0, 0.0);
    }

    /// How many monsters she knows of.
    pub fn known_count(&self) -> usize {
        self.known.len()
    }

    /// The monsters she knows of from before this life and hasn't seen in it yet, within `range`
    /// of her: what she expects there.
    pub fn expected(&self, pos: Point, range: f64) -> Vec<&Seen> {
        self.known
            .values()
            .filter(|s| {
                s.foreknown && !s.lost && !self.dead.contains(&s.id) && dist(s.pos(), pos) < range
            })
            .collect()
    }

    /// Is that monster dead, as far as she knows (seen or heard to die)?
    pub fn known_dead(&self, id: i32) -> bool {
        self.dead.contains(&id)
    }

    /// Is a monster she knows of firing now, as far as she can tell: seen at it, or its kind's
    /// attack heard a moment ago (a chaingun's heard through walls, within the game's 1200)?
    pub fn firing(&self, obs: &Observation, s: &Seen) -> bool {
        match self.visible.iter().find(|t| t.id == s.id) {
            Some(t) => crate::bestiary::first_shot(t.name).is_some_and(|f| t.attack >= f),
            None => self.senses.attacks_heard.iter().any(|&(tic, kinds)| {
                obs.tic - tic <= self.tune.senses.heard_firing && kinds.contains(&s.name)
            }),
        }
    }

    /// The monsters she knows are dead: (id, kind, where, when last in her sight).
    pub fn known_deaths(&self) -> Vec<(i32, &'static str, Point, i64)> {
        self.dead
            .iter()
            .filter_map(|i| self.known.get(i))
            .map(|s| (s.id, s.name, s.pos(), s.sighted))
            .collect()
    }

    /// Monsters seen that are still about, as far as she knows, and where she last saw them (to
    /// hunt down, to keep clear of): not one she knows is dead, nor one she's seen isn't where
    /// she left it.
    pub fn remaining(&self) -> Vec<Seen> {
        self.about().cloned().collect()
    }

    /// The same, looked through where they lie: for what's asked many times a tic (ten thousand
    /// of them copied for each of a thousand guns lying about was a fifth of a second a tic).
    pub fn about(&self) -> impl Iterator<Item = &Seen> {
        self.known
            .iter()
            .filter(|(i, s)| !self.dead.contains(i) && !s.lost)
            .map(|(_, s)| s)
    }

    /// The monsters she's in a fight with: in sight, awake, after her, and able to hurt her from
    /// where they are (one with a gun or missiles within `engaged_ranged` of her, a lost soul
    /// within `engaged_charge`, a clawing one within `engaged_melee`). This is what pauses her
    /// exploring: not anything awake in sight, such as a zombieman across the map or a demon
    /// over a pit.
    pub fn engaged(&self, obs: &Observation) -> Vec<Thing> {
        let pos = obs.pos();
        self.seen_against_her()
            .into_iter()
            .filter(|t| {
                t.health > 0 && t.after_her && self.known.get(&t.id).is_some_and(|k| k.awake)
            })
            .filter(|t| {
                let d = dist((t.x, t.y), pos);
                match mon(t.name).attack {
                    Attack::Hitscan | Attack::Missile | Attack::Fire => {
                        d < self.tune.engage.engaged_ranged
                    }
                    Attack::Charge => d < self.tune.engage.engaged_charge,
                    Attack::Melee => d < self.tune.engage.engaged_melee,
                    Attack::None => false,
                }
            })
            .collect()
    }
}
