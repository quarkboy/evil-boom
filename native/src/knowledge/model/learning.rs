//! Attributing sector movement to triggers and learning whether activations took effect.

use super::{needs_key, Fired, How, Knowledge, BUSY_TICS, SHOT_RANGE, SURE_TICS, TAKE_TICS};
use crate::motor::{Controls, Observation};
use crate::nav::Heights;
use crate::navigator::{States, TriggerLine, EXIT_FLOOR, SHOT_HEIGHT};
use crate::specials::{Category, Trigger};
use std::collections::{BTreeMap, BTreeSet};

impl Knowledge {
    /// Something heard to start moving, out of her sight, just after she set a line off, in a
    /// place she knows: taken for what the line does, as a player takes a lift's clank after a
    /// switch for the switch's lift. Some effects can't be seen from where they're set off and
    /// are over before she could get round to look.
    pub(super) fn heard_move(&mut self, s: usize, tic: i64) {
        if !self.sectors.contains(&s) {
            return;
        }
        let Some(f) = self
            .fired
            .iter()
            .rev()
            .find(|f| f.life == self.lives && f.tic <= tic)
            .copied()
            .filter(|f| tic - f.tic <= SURE_TICS)
        else {
            return;
        };
        for ti in self.by_line.get(&f.line).cloned().unwrap_or_default() {
            let t = self.trigger(ti).clone();
            if t.effect(s).is_some() && self.learned.entry(ti).or_default().insert(s) {
                self.events.push(format!(
                    "tic {tic}: took what she heard for line {} ({}) moving sector {s}",
                    f.line, t.info.description
                ));
                self.whole_stair(ti, &t, tic);
            }
        }
    }

    /// A step of a stair seen or heard to rise for a line: the rest of that stair she's seen
    /// goes with it. A stair builds as one thing, a step after a step, and a step neither seen
    /// nor heard would otherwise be taken for still on the floor, far under the steps after it.
    fn whole_stair(&mut self, ti: usize, t: &TriggerLine, tic: i64) {
        if t.info.category != Category::Stairs {
            return;
        }
        let rest: Vec<usize> = t
            .effects
            .iter()
            .map(|(s, _)| *s)
            .filter(|s| self.sectors.contains(s))
            .collect();
        let learned = self.learned.entry(ti).or_default();
        let new: Vec<usize> = rest.into_iter().filter(|&s| learned.insert(s)).collect();
        if !new.is_empty() {
            self.events.push(format!(
                "tic {tic}: the rest of the stair with it: sectors {new:?}"
            ));
        }
    }

    /// Sector s is seen at new heights: put it down to the latest line fired before it started
    /// moving that moves it, and moves it this way (a door seen closing isn't an open-for-good
    /// switch's doing). Sure if it started moving right after the press, else in doubt.
    pub(super) fn saw_move(&mut self, s: usize, tic: i64, now: Heights) {
        let was = self.seen_heights.get(&s).copied();
        // When it started moving, by its sound; this tic, if it wasn't moving till now.
        let prev = self.live.get(s).copied();
        let fresh = !self.moving.contains(&s) && prev.is_some_and(|p| p != now);
        let started = if fresh {
            tic
        } else {
            self.moved_at.get(&s).copied().unwrap_or(tic)
        };
        for i in (0..self.fired.len()).rev() {
            let f = self.fired[i];
            if f.life != self.lives || f.tic > started {
                continue;
            }
            let ids = self.by_line.get(&f.line).cloned().unwrap_or_default();
            for ti in ids {
                let t = self.trigger(ti).clone();
                if t.effect(s).is_none() || !self.heads_for(&t, s, was, now) {
                    continue;
                }
                let key = (self.tried_key(f.line), s);
                if started - f.tic <= SURE_TICS {
                    if self.doubts.remove(&key).is_some() {
                        self.events.push(format!(
                            "tic {tic}: line {} does move sector {s}: sure now",
                            f.line
                        ));
                    }
                } else if !self.doubts.contains_key(&key)
                    && !self.learned.get(&ti).is_some_and(|l| l.contains(&s))
                {
                    self.doubts.insert(key, tic);
                }
                if let Some(hit) = self.nav.floor_hit_after(&t, s) {
                    let hurts = hit != 0;
                    if self
                        .changed_floor
                        .insert(s, hit)
                        .is_none_or(|was| (was != 0) != hurts)
                    {
                        self.events.push(format!(
                            "tic {tic}: sector {s}'s floor {}",
                            if hurts {
                                "turns to something that hurts"
                            } else {
                                "is safe now"
                            }
                        ));
                    }
                }
                if self.learned.entry(ti).or_default().insert(s) {
                    self.events.push(format!(
                        "tic {tic}: learned line {} ({}) moves sector {s}",
                        f.line, t.info.description
                    ));
                    self.whole_stair(ti, &t, tic);
                }
                return;
            }
        }
        // Nothing she set off moves it this way: it's what the last of a kind dying moves, if this
        // is that.
        for ti in 0..self.nav.triggers.len() {
            let t = self.trigger(ti).clone();
            if t.line < 0
                && t.line > EXIT_FLOOR
                && t.effect(s).is_some()
                && self.heads_for(&t, s, was, now)
                && self.learned.entry(ti).or_default().insert(s)
            {
                self.events.push(format!(
                    "tic {tic}: learned that to {} moves sector {s}",
                    t.info.description
                ));
            }
        }
    }

    /// Is sector s, seen go from `was` to `now`, on its way to where t puts it (or, for a line
    /// whose move comes back, back to rest)?
    fn heads_for(&self, t: &TriggerLine, s: usize, was: Option<Heights>, now: Heights) -> bool {
        let Some(was) = was else { return true };
        if t.shift_from.is_some() || t.relevant.is_some() {
            return true; // (where it goes depends on how things are: no telling)
        }
        let mut targets: Vec<Heights> = t.effect(s).map(<[Heights]>::to_vec).unwrap_or_default();
        if !t.permanent() {
            targets.push(self.nav.movers.initial[s]);
        }
        let off =
            |h: Heights, at: Heights| (at.floor - h.floor).abs() + (at.ceiling - h.ceiling).abs();
        targets
            .iter()
            .any(|&h| off(h, now) < off(h, was) - 0.5 || off(h, now) < 0.5)
    }

    pub fn doubtful(&self, line: usize, s: usize) -> bool {
        self.doubts.contains_key(&(self.tried_key(line), s))
    }

    /// A line in doubt pressed again: what it's put down to moving, moving right after (sure),
    /// already where it puts it (no telling, but it fits: kept), or neither (unlearned).
    pub(super) fn check_doubts(&mut self, tic: i64) {
        let checking = std::mem::take(&mut self.checking);
        let mut still = Vec::new();
        for (f, sectors) in checking {
            let left: BTreeSet<usize> = sectors
                .iter()
                .copied()
                .filter(|&s| self.doubts.contains_key(&(self.tried_key(f.line), s)))
                .collect();
            if left.is_empty() {
                continue;
            }
            if tic - f.tic < TAKE_TICS {
                still.push((f, left));
                continue;
            }
            for s in left {
                let key = (self.tried_key(f.line), s);
                let mut fits = false;
                for &ti in self.by_line.get(&f.line).into_iter().flatten() {
                    let t = self.trigger(ti);
                    if let Some(hs) = t
                        .effect(s)
                        .filter(|_| t.shift_from.is_none() && t.relevant.is_none())
                    {
                        fits = fits
                            || self.seen_heights.get(&s).is_some_and(|seen| {
                                hs.iter().any(|h| {
                                    (seen.floor - h.floor).abs() + (seen.ceiling - h.ceiling).abs()
                                        < 0.5
                                })
                            });
                    }
                }
                if fits
                    || self.running(s, tic)
                    || self.seen_tic.get(&s).copied().unwrap_or(-1) < f.tic
                {
                    continue; // (still in doubt: no telling from here)
                }
                self.doubts.remove(&key);
                for &ti in self.by_line.get(&f.line).into_iter().flatten() {
                    if let Some(l) = self.learned.get_mut(&ti) {
                        l.remove(&s);
                    }
                }
                self.events.push(format!(
                    "tic {tic}: line {} doesn't move sector {s} after all",
                    f.line
                ));
            }
        }
        self.checking = still;
    }

    pub(super) fn detect_fired(
        &mut self,
        last: &Observation,
        obs: &Observation,
        controls: Option<&Controls>,
    ) {
        // walking over
        for (_, li) in self.lines_along(last.pos(), obs.pos()) {
            let Some(info) = self.info.get(&li) else {
                continue;
            };
            if !info.trigger.walked() || info.category == Category::Teleport {
                continue; // (going through a teleport is seen by where she ends up)
            }
            self.fire(li, obs.tic, How::Walk);
        }
        let Some(c) = controls else { return };
        let a = last.angle.to_radians();
        let heights = &obs.heights;
        if c.use_ {
            // The game turns her, then traces the use: by where she faces after the tic's turn.
            let facing = last.angle - c.turn;
            if crate::debug::on().use_press {
                self.events.push(format!(
                    "tic {}: use at ({:.0}, {:.0}) facing {facing:.1} (turned {:.1}): hits {:?}",
                    obs.tic,
                    last.x,
                    last.y,
                    c.turn,
                    self.use_hits(last.pos(), facing, heights)
                ));
            }
            if let Some(li) = self.use_hits(last.pos(), facing, heights) {
                // Any line with a special stops the trace, a walk-over one too; only one that's used goes
                // off. A locked one without its key does nothing, and the game says so: something to come
                // back to with the key.
                let info = self.info[&li].clone();
                if info.trigger.used() && self.front(li, last.x, last.y) {
                    if info.lock.satisfied(last.keys.unwrap_or_default()) {
                        self.fire(li, obs.tic, How::Use);
                    } else if obs.message.as_deref().is_some_and(needs_key)
                        && !self.locked.contains(&li)
                    {
                        self.locked.insert(li);
                        self.events.push(format!(
                            "tic {}: line {li} is locked (\"{}\")",
                            obs.tic,
                            obs.message.as_deref().unwrap_or("")
                        ));
                    }
                }
            }
        }
        if c.attack {
            let z = last
                .z
                .unwrap_or_else(|| heights[self.sector_at(last)].floor)
                + SHOT_HEIGHT;
            let end = (last.x + SHOT_RANGE * a.cos(), last.y + SHOT_RANGE * a.sin());
            for (_, li) in self.lines_along(last.pos(), end) {
                if self.info.get(&li).is_some_and(|i| i.trigger.shot()) {
                    self.fire(li, obs.tic, How::Shoot);
                }
                match self.opening(li, heights) {
                    Some((lo, hi)) if lo < z && z < hi => {}
                    _ => break,
                }
            }
        }
    }

    /// A once-only line used this life (its special's gone: the game clears it).
    pub(super) fn spent(&self, li: usize) -> bool {
        self.info.get(&li).is_some_and(|i| i.trigger.once())
            && self
                .fired
                .iter()
                .any(|f| f.line == li && f.life == self.lives)
    }

    pub(super) fn fire(&mut self, li: usize, tic: i64, how: How) {
        let Some(info) = self.info.get(&li).cloned() else {
            return;
        };
        if info.trigger.once()
            && self
                .fired
                .iter()
                .any(|f| f.line == li && f.life == self.lives)
        {
            return; // (once only: nothing happens again this life)
        }
        if matches!(info.trigger, Trigger::SR | Trigger::GR)
            && info.category != Category::Crusher
            && self
                .by_line
                .get(&li)
                .into_iter()
                .flatten()
                .any(|&ti| self.trigger(ti).permanent())
        {
            // Doom turns a switch down on a sector that's still busy. A repeatable one that would leave
            // it so for good, wrongly taken to have worked, misleads the planner.
            let busy: Vec<usize> = self
                .targets(li)
                .into_iter()
                .filter(|&t| self.running(t, tic))
                .collect();
            if let Some(b) = busy.first() {
                self.events.push(format!(
                    "tic {tic}: {} line {li} ({}): sector {b} is busy, so nothing",
                    how.name(),
                    info.description
                ));
                return;
            }
        }
        let f = Fired {
            line: li,
            tic,
            how,
            life: self.lives,
        };
        self.fired.push(f);
        // What it does, by the heights as she knows them now.
        let now: States = self
            .nav
            .movers
            .initial
            .iter()
            .enumerate()
            .map(|(s, &h)| vec![self.seen_heights.get(&s).copied().unwrap_or(h)])
            .collect();
        let ids: Vec<usize> = self.by_line.get(&li).cloned().unwrap_or_default();
        for ti in ids {
            let effects = self.nav.effects_now(self.trigger(ti), &now);
            self.fired_rest.insert(
                (ti, tic),
                effects
                    .iter()
                    .filter_map(|(s, _)| now[*s].first().map(|&h| (*s, h)))
                    .collect(),
            );
            self.fired_effects.insert((ti, tic), effects);
        }
        self.events.push(format!(
            "tic {tic}: {} line {li} ({})",
            how.name(),
            info.description
        ));
        let tk = self.tried_key(li);
        let doubted: BTreeSet<usize> = self
            .doubts
            .keys()
            .filter(|(k, _)| *k == tk)
            .map(|(_, s)| *s)
            .collect();
        // Watched to see if it does what it's in doubt of: once a life, and only pressed when what
        // it acts on has been still a while. That's the go it gets either way.
        if !doubted.is_empty()
            && self.rechecked.insert((tk, self.lives))
            && self
                .targets(li)
                .iter()
                .all(|t| tic - self.moved_at.get(t).copied().unwrap_or(-1_000_000_000) >= BUSY_TICS)
        {
            self.checking.push((f, doubted));
        }
        if info.trigger.once() {
            let targets = self.targets(li);
            if targets
                .iter()
                .any(|t| tic - self.moved_at.get(t).copied().unwrap_or(-1_000_000_000) < BUSY_TICS)
            {
                let then: BTreeMap<usize, Heights> = targets
                    .iter()
                    .filter_map(|&t| self.seen_heights.get(&t).map(|&h| (t, h)))
                    .collect();
                self.taking.push((f, targets, then));
            }
        }
    }

    /// Is sector s, as far as she can tell, still in a move that comes back (a door open on its
    /// timer, a lift away): moved lately, by a line of hers that doesn't leave it for good (or
    /// by she doesn't know what), and not back where it started?
    fn running(&self, s: usize, tic: i64) -> bool {
        if tic - self.moved_at.get(&s).copied().unwrap_or(-1_000_000_000) >= BUSY_TICS {
            return false;
        }
        let init = self.nav.movers.initial[s];
        let seen = self.seen_heights.get(&s).copied().unwrap_or(init);
        if (seen.floor - init.floor).abs() < 0.5 && (seen.ceiling - init.ceiling).abs() < 0.5 {
            return false;
        }
        let mut last: Option<(i64, usize)> = None;
        for g in self.fired.iter().filter(|g| g.life == self.lives) {
            for &ti in self.by_line.get(&g.line).into_iter().flatten() {
                if self.trigger(ti).effect(s).is_some() && last.is_none_or(|(lt, _)| g.tic >= lt) {
                    last = Some((g.tic, ti));
                }
            }
        }
        last.is_none_or(|(_, ti)| !self.trigger(ti).permanent())
    }

    /// The sectors a line acts on, by the game's rule: those with its tag; a door line with
    /// none, the door behind it.
    pub(super) fn targets(&self, li: usize) -> BTreeSet<usize> {
        let m = &self.nav.map;
        let l = &m.linedefs[li];
        if l.tag != 0 {
            return m
                .sectors
                .iter()
                .filter(|s| s.tag == l.tag)
                .map(|s| s.index)
                .collect();
        }
        m.back_sector(l).into_iter().collect()
    }

    /// Once-only lines set off on sectors that were busy: if nothing has moved since, the game
    /// turned them down.
    pub(super) fn took(&mut self, tic: i64) {
        let taking = std::mem::take(&mut self.taking);
        let mut still = Vec::new();
        for (f, targets, then) in taking {
            // From the tic it's seen pressed: a door moves in the tic of the press, and is seen in the
            // same look as the press is.
            if targets.iter().any(|&t| {
                self.moved_at.get(&t).copied().unwrap_or(-1) >= f.tic
                    && self.moved_its_way(&f, t, then.get(&t))
            }) {
                continue; // (it took)
            }
            if tic - f.tic < TAKE_TICS {
                still.push((f, targets, then));
                continue;
            }
            if self.already_so(&f, &targets) {
                continue; // (it took: what it moves was already where it puts it)
            }
            if let Some(i) = self.fired.iter().position(|g| *g == f) {
                self.fired.remove(i);
            }
            self.events.push(format!(
                "tic {tic}: line {} didn't take (what it moves was busy): not used up",
                f.line
            ));
        }
        self.taking = still;
    }

    /// Has sector s moved, since line f was set off, the way f moves it: nearer where f puts it
    /// than it was then (`then`)? A sector busy with another mover moves too, its own way: the
    /// press was turned down by the game (EV_DoFloor skips a sector already moving) and isn't
    /// used up. Where f puts it, or where s was, not known: any move counts.
    fn moved_its_way(&self, f: &Fired, s: usize, then: Option<&Heights>) -> bool {
        let (Some(&then), Some(&now)) = (then, self.seen_heights.get(&s)) else {
            return true;
        };
        let ends: Vec<Heights> = self
            .by_line
            .get(&f.line)
            .into_iter()
            .flatten()
            .filter_map(|&ti| self.fired_effects.get(&(ti, f.tic)))
            .flat_map(|e| {
                e.iter()
                    .filter(|(t, _)| *t == s)
                    .flat_map(|(_, hs)| hs.iter().copied())
            })
            .collect();
        if ends.is_empty() {
            return true;
        }
        let off = |h: Heights| {
            ends.iter()
                .map(|e| (h.floor - e.floor).abs() + (h.ceiling - e.ceiling).abs())
                .fold(f64::INFINITY, f64::min)
        };
        off(now) < off(then) - 0.5
    }

    /// Was everything line f moves left where it is for good by the last line she set off that
    /// moves it? Not if that was a door that closes again, or she doesn't know what moved it;
    /// and only for a line with a tag.
    fn already_so(&self, f: &Fired, targets: &BTreeSet<usize>) -> bool {
        if self.nav.map.linedefs[f.line].tag == 0 {
            return false;
        }
        for &t in targets {
            let mut last: Option<(i64, usize)> = None;
            for g in &self.fired {
                if g == f || g.life != self.lives || g.tic > f.tic {
                    continue;
                }
                for &ti in self.by_line.get(&g.line).into_iter().flatten() {
                    if self.trigger(ti).effect(t).is_some()
                        && last.is_none_or(|(lt, _)| g.tic >= lt)
                    {
                        last = Some((g.tic, ti));
                    }
                }
            }
            if last.is_none_or(|(_, ti)| !self.trigger(ti).permanent()) {
                return false;
            }
        }
        true
    }
}
