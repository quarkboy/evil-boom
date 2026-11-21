// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Giving a monster up: no shot for too long, not closing, the whole lot left; a boss dying,
//! a crusher doing the work; what to do when the last of them is lost from sight.

use super::*;

impl Combat {
    /// A fight with no shot at anything in sight is one go at the same thing, whichever of them
    /// is the target. Each effort at a monster has its own clock (`Patience`), and with several
    /// out of her guns' way taking turns to be the target no clock ran out before the next one's
    /// began: Doom II MAP13's key room on Ultra-Violence, thirteen imps on ledges over her, 137
    /// given up one at a time in 21 minutes and one to two hours before she'd crossed the room.
    /// So, as out of ammo (`backing_off`), one clock for the lot: `engage.close_give_up` of such
    /// fighting with nothing hurt meanwhile (breaks of `engage.given_up_tics` and over start it
    /// afresh), and all in sight she can't hit are left a while together, those that come into
    /// sight meanwhile with them; twice as long each time till something's hurt. One she gets a
    /// shot at is none of the lot from then.
    pub(super) fn leave_the_lot(&mut self, obs: &Observation) {
        if self.dealt > self.lot.dealt {
            (self.lot.spent, self.lot.times) = (0, 0);
        }
        self.lot.dealt = self.dealt;
        if obs.tic >= self.lot.till {
            self.lot.ids.clear();
        }
        let foes: Vec<Thing> = self
            .visible
            .iter()
            .filter(|t| mon(t.name).attack != Attack::None)
            .cloned()
            .collect();
        if foes.is_empty() {
            return;
        }
        let no_shot: Vec<i32> = foes
            .iter()
            .filter(|t| !self.can_hit(obs, t, None))
            .map(|t| t.id)
            .collect();
        // (a shot at one of them: none of the lot from now, and the lot's leaving it taken back
        // with that, or it stood given up on till the lot's time was out, 16 minutes at the
        // longest, and wasn't stood to with a clear shot at it. One given up on for itself
        // since stays so.)
        let shot_at: Vec<i32> = self
            .lot
            .ids
            .iter()
            .copied()
            .filter(|id| !no_shot.contains(id) && foes.iter().any(|t| t.id == *id))
            .collect();
        for id in shot_at {
            self.lot.ids.remove(&id);
            self.patience.take_back(id, self.lot.till);
        }
        if obs.tic < self.lot.till {
            for &id in &no_shot {
                if self.lot.ids.insert(id) {
                    self.patience.give_up(id, self.lot.till);
                }
            }
            return;
        }
        if no_shot.len() < foes.len() {
            return;
        }
        if obs.tic - self.lot.last > self.tune.engage.given_up_tics {
            self.lot.spent = 0;
        }
        self.lot.last = obs.tic;
        self.lot.spent += 1;
        if self.lot.spent <= self.tune.engage.close_give_up {
            return;
        }
        self.lot.spent = 0;
        self.lot.till = obs.tic + (self.tune.engage.given_up_tics << self.lot.times.min(5));
        self.lot.times += 1;
        for &id in &no_shot {
            self.lot.ids.insert(id);
            self.patience.give_up(id, self.lot.till);
        }
        self.log.push(format!(
            "tic {}: no shot at any of the {} in sight: leaving the lot a while",
            obs.tic,
            no_shot.len()
        ));
    }

    /// Is id one of a lot left as out of her guns' way (`leave_the_lot`)?
    pub(super) fn left_with_the_lot(&self, id: i32) -> bool {
        self.lot.ids.contains(&id)
    }

    /// Give up on getting at id a while.
    pub(super) fn give_up(&mut self, obs: &Observation, id: i32) {
        self.patience
            .give_up(id, obs.tic + self.tune.engage.given_up_tics);
        self.log.push(format!(
            "tic {}: can't get at {id}: leaving it a while",
            obs.tic
        ));
    }

    /// Closing in on t, out of reach: still worth it? Not once she has been at it this long
    /// without getting it in reach: then it's left a while.
    pub(super) fn still_closing(&mut self, obs: &Observation, id: i32) -> bool {
        if self.given_up_on(obs, id) {
            return false;
        }
        // (getting no nearer for a while: no way to it from here, whatever's between)
        let near = self
            .known
            .get(&id)
            .map(|k| dist(k.pos(), obs.pos()))
            .unwrap_or(f64::INFINITY);
        if self.patience.spent(
            Effort::ClosingStall,
            id,
            obs.tic,
            self.tune.engage.close_stall,
            Some((near, self.tune.engage.close_gain)),
            None,
        ) || self.patience.spent(
            Effort::Closing,
            id,
            obs.tic,
            self.tune.engage.close_give_up,
            None,
            None,
        ) {
            self.patience.rest(Effort::ClosingStall, id);
            self.patience.rest(Effort::Closing, id);
            self.give_up(obs, id);
            return false;
        }
        true
    }

    /// One of t's kind died a moment ago, and the last of that kind dead moves something on
    /// this map: the game looks, as each finishes dying, for another of its kind still alive,
    /// and two dying together each find none. What their death moves then moves twice (MAP07's
    /// step to the exit, raised a second time, is too high to get onto: no way out of the
    /// level). The next isn't shot till the one before has done dying.
    pub(super) fn boss_dying(&self, obs: &Observation, t: &Thing) -> bool {
        self.boss_kinds.contains(&t.name)
            && self
                .kind_died
                .get(t.name)
                .is_some_and(|&d| obs.tic - d < BOSS_DEATH_TICS)
    }

    /// Standing to fight t with no shot at it from here: given up on a while once she has had
    /// none for `engage.close_give_up` (a shot at it sets the clock back; so does a fight with
    /// it broken off that long). Standing gets her no shot, so the route goes on, shooting on
    /// the way when one comes.
    pub(super) fn no_shot_long(&mut self, obs: &Observation, t: &Thing) -> bool {
        if self.given_up_on(obs, t.id) {
            return true;
        }
        // (in her sights and the plasma in her hand going into something short of it is no
        // shot either: stood facing one across a pit's lip at 2 health for five minutes, not a
        // round fired)
        let blocked = obs.weapon == Some(6)
            && self.target == Some(t.id)
            && self.trigger.held_fire == Some(moves::HeldFire::PlasmaBlocked);
        if !blocked && self.can_hit(obs, t, None) {
            self.patience.rest(Effort::NoShot, t.id);
            return false;
        }
        let limit = self.tune.engage.close_give_up;
        if self
            .patience
            .spent(Effort::NoShot, t.id, obs.tic, limit, None, Some(limit))
        {
            self.give_up(obs, t.id);
            return true;
        }
        false
    }

    /// What outmatches her, standing in a sector a crusher she knows of moves (its line learned),
    /// whether it's moving now or not.
    pub(super) fn under_her_crusher(&self, obs: &Observation, t: &Thing) -> bool {
        self.too_much(obs, &[t.name], self.tune.guns.outmatched)
            && self
                .nav
                .poly_at((t.x, t.y))
                .is_some_and(|q| self.crusher_known(self.nav.sector_of_poly(q)))
    }

    /// Is sector s one a crusher she knows of moves?
    pub(super) fn crusher_known(&self, s: usize) -> bool {
        self.movers.crushers.contains_key(&s)
    }

    /// What outmatches her that a crusher she knows of is at work on, as she last saw it: standing
    /// in one of her crusher sectors whose ceiling has come down within `engage.crush_wait`. It
    /// dies where it stands, given time, and she has only to keep out of its sight till it has
    /// (for `engage.crush_hold` at most: out of its sight she can't tell it's still under it).
    pub(super) fn being_crushed(&mut self, obs: &Observation) -> Option<Thing> {
        let t = self
            .last_seen
            .values()
            .filter(|t| !self.dead.contains(&t.id) && t.health > 0)
            .filter(|t| self.too_much(obs, &[t.name], self.tune.guns.outmatched))
            .find(|t| {
                self.nav
                    .poly_at((t.x, t.y))
                    .map(|q| self.nav.sector_of_poly(q))
                    .is_some_and(|s| {
                        self.movers.crushers.contains_key(&s)
                            && self
                                .movers
                                .ceiling_down
                                .get(s)
                                .is_some_and(|&at| obs.tic - at < self.tune.engage.crush_wait)
                    })
            })
            .cloned();
        let Some(t) = t else {
            self.crush_wait = None;
            return None;
        };
        let since = match self.crush_wait {
            Some((id, since)) if id == t.id => since,
            _ => {
                self.crush_wait = Some((t.id, obs.tic));
                obs.tic
            }
        };
        (obs.tic - since < self.tune.engage.crush_hold).then_some(t)
    }

    /// An awake monster that was after her and went out of sight a moment ago (behind a pillar,
    /// round a corner): she keeps fighting it, aimed where it was and keeping her distance,
    /// rather than going about her business and walking up to it. Not on a floor that hurts (or
    /// one that will, a suit's time running out): the route's way off is the way to take then,
    /// not after a ghost.
    pub(super) fn lost_sight(&mut self, obs: &Observation) -> Option<Decision> {
        // Nor when nothing can hurt her: one gone from sight is nothing to keep at.
        if self.on_hurting_floor(obs) || self.immune {
            return None;
        }
        let pos = obs.pos();
        let s = self
            .remaining()
            .into_iter()
            // Gone a moment, not a tic: one on the edge of a line of sight is in it and out of it tic
            // about as she moves a unit, and would pass her legs to and fro between this and the route.
            // A moment by her eyes, not her ears: a step heard says where it is and dates it, but isn't
            // sight of it, and every step of one walking about out of sight would be a fresh "just
            // lost".
            .filter(|s| {
                let eyes = obs.tic
                    - s.tic
                        .min(s.sighted.saturating_add(self.tune.senses.instinct_tics));
                s.awake
                    && eyes >= self.tune.engage.lost_after
                    && eyes < self.tune.engage.lost_tics
                    && mon(s.name).attack != Attack::None
                    && dist(s.pos(), pos) < self.tune.engage.range
            })
            .filter(|s| !self.patience.given_up(s.id, obs.tic))
            // Not after one she can't take on: out of its sight is where she wants to be.
            .filter(|s| !self.too_much(obs, &[s.name], self.tune.guns.outmatched))
            // Nor one out of reach of every gun she'd bring up, where it's reckoned to be: in
            // sight, that one's passed by (`out_of_reach`), and gone from sight it's no more to
            // keep at. (Kept at, a mancubus on a far ledge in and out of view as she walked on
            // took her back into the fight and out again every second and a half, a minute
            // long, MAP07.)
            .filter_map(|s| {
                let mut ghost = self.last_seen.get(&s.id)?.clone();
                let at = self.reckoned(&s, obs.tic);
                (ghost.x, ghost.y, ghost.z, ghost.vx, ghost.vy) = (at.0, at.1, s.z, 0.0, 0.0);
                self.reachable(obs, &ghost).then_some((s, ghost, at))
            })
            .max_by(|a, b| mon(a.0.name).threat.total_cmp(&mon(b.0.name).threat));
        let (s, ghost, at) = s?;
        // Kept at so time and again with no shot at it in between (in sight a moment, no shot,
        // gone, back): given up on a while, as one closed in on and never reached is. The once
        // is worth having, a pause where the fight was while what's coming comes to her; it's
        // the going round that isn't (two imps 1,100 off, 133 times, E1M7).
        let again = self.tune.engage.lost_again;
        if self
            .patience
            .spent(Effort::LostAgain, s.id, obs.tic, again, None, Some(again))
        {
            self.give_up(obs, s.id);
            return None;
        }
        let gun = self.preferred_gun(obs, &ghost);
        // (stepped about as for one in sight, and that's kept: holding still for it, as a player
        // waits at a corner, cost her on the score, -0.028 (2026-11-11): what comes back round
        // the corner hits one who's standing)
        let moving = self.steer(obs, Some(&ghost), &gun);
        let why = format!("lost sight of {}: keeping at it", s.name);
        self.announce(obs, &why);
        Some(Decision::without_shot(
            Move::LostSight,
            Some(heading(pos, at)),
            moving,
            why,
        ))
    }
}
