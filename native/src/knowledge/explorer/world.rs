//! Constructing the navigable world from learned heights, triggers and exclusions.

use super::{Explorer, ViewKey, JUMP_MISSES, STUCK_TIMES};
use crate::geom::Point;
use crate::knowledge::model::How;
use crate::movers::cycles_back;
use crate::nav::Heights;
use crate::navigator::{add_height, Restriction, States, TriggerLine, View};
use crate::specials::{Category, Key, Trigger};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// a crusher heard stopping this long ago, and still since, has been stopped (a crusher turns at
/// each end at once: still at all is a stop)
const CRUSHER_STOPPED: i64 = 35;

impl Explorer {
    /// The heights the planner should take: as last seen where seen, else as at the start; and
    /// where she set something off she knows the effect of and hasn't looked since, as it'll
    /// have left it (a floor lowered out of sight is down).
    pub fn states(&self) -> States {
        let k = &self.knowledge;
        let nav = &self.nav;
        let now = self.now();
        let mut out: States = nav.movers.initial.iter().enumerate().map(|(s, &h)| vec![k.seen_heights.get(&s).copied().unwrap_or(h)]).collect();
        let known: BTreeMap<i64, (TriggerLine, BTreeSet<usize>)> = k.known_triggers().into_iter().map(|(t, s)| (t.line, (t, s))).collect();
        for f in &k.fired {
            if f.life != k.lives {
                continue; // (this life's: the level's started again)
            }
            let Some((t, sectors)) = known.get(&(f.line as i64)) else { continue };
            // (as worked out when it went off; else from the heights as they're believed now)
            let effects = k.fired_effects.get(&(t.id, f.tic)).cloned().unwrap_or_else(|| nav.effects_now(t, &out));
            for &s in sectors {
                let Some(hs) = effects.iter().find(|(x, _)| *x == s).map(|(_, hs)| hs.clone()).filter(|hs| !hs.is_empty()) else { continue };
                if t.permanent() && k.doubtful(f.line, s) {
                    continue; // (in doubt that it's gone for good: as seen, not built on)
                }
                // (not seen since the move should have finished: as it'll have left it; seen
                // after, as seen. Stairs rise slowly: seen part way up is not the top)
                let h = hs[0];
                let timing = (t.line >= 0).then(|| nav.movers.timing(t.info.special, &t.info.description, s, h));
                let done = f.tic + timing.map_or(0, |tm| tm.travel as i64);
                // (what comes back is only as moved till it's due back: MAP23's timed door)
                let back = done + timing.and_then(|tm| tm.hold.map(|hold| (hold + tm.travel) as i64)).unwrap_or(0);
                if !t.permanent() && timing.is_some_and(|tm| tm.hold.is_some()) && now > back {
                    // (back at rest, as far as she knows, whatever had it before: a door opened
                    // for good, then pressed open again, shuts after its wait: E2M4's door 48)
                    if cycles_back(t.info.special, &t.info.description) && k.seen_tic.get(&s).copied().unwrap_or(-1) < back {
                        out[s] = vec![nav.movers.initial[s]];
                    }
                    continue;
                }
                if k.seen_tic.get(&s).copied().unwrap_or(-1) < done {
                    if t.permanent() {
                        out[s] = Vec::new();
                    }
                    for x in hs {
                        add_height(&mut out[s], x);
                    }
                }
            }
        }
        // a lift or a timed door she knows comes back to where it rests, wherever she last saw
        // it (E3M3); not a crusher stopped where it is (a crusher-stop line leaves it there till
        // something starts it again: E2M4's, stopped low across the way, waited at for the hour)
        for (t, sectors) in known.values() {
            if !t.permanent() {
                for &s in sectors {
                    if t.info.category == Category::Crusher && k.stopped_for(s, now, CRUSHER_STOPPED) {
                        continue;
                    }
                    add_height(&mut out[s], nav.movers.initial[s]);
                }
            }
        }
        // a slot narrower than the box is walked over at its sides' floor, however deep she saw
        // it (E4M1), at each floor the sides can be at (MAP08's moat)
        for &tr in nav.trenches.keys() {
            let lo = nav.trench_floor(tr, |o| out[o].iter().map(|h| h.floor).fold(f64::INFINITY, f64::min));
            let hi = nav.trench_floor(tr, |o| out[o].iter().map(|h| h.floor).fold(f64::NEG_INFINITY, f64::max));
            let floors: Vec<f64> = if lo == hi { vec![lo] } else { vec![lo, hi] };
            let mut hs: Vec<Heights> = Vec::new();
            for h in &out[tr] {
                for &floor in &floors {
                    add_height(&mut hs, Heights { floor, ceiling: h.ceiling });
                }
            }
            out[tr] = hs;
        }
        out
    }

    /// The navigator as far as she knows (rebuilt when what she knows changes). `fresh`: for the
    /// level from its start (what's spent this life works again; exits open).
    pub fn view(&mut self) -> Rc<View> {
        self.view_for(false)
    }

    pub fn view_for(&mut self, fresh: bool) -> Rc<View> {
        if fresh {
            return self.fresh_view();
        }
        let key = self.view_key(false);
        if let Some((v, vk)) = &self.view {
            if *vk == key && self.banned_jumps() == *v.banned_jumps() {
                return Rc::clone(v);
            }
        }
        let view = Rc::new(self.build_view(false));
        self.view = Some((Rc::clone(&view), key));
        view
    }

    /// The navigator as she knows the level from its start (what's spent this life works
    /// again; exits open).
    pub fn fresh_view(&self) -> Rc<View> {
        Rc::new(self.build_view(true))
    }

    /// The once-only lines she's used this life: spent, to the planner (E3M4's door; E2M1's
    /// door switch 288, pressed once from the alcove: the room behind is had then or not at all).
    pub(super) fn spent_once(&self) -> BTreeSet<usize> {
        let k = &self.knowledge;
        k.fired.iter().filter(|f| f.life == k.lives && self.nav.triggers.iter().any(|t| t.line == f.line as i64 && t.once())).map(|f| f.line).collect()
    }

    /// Sectors a door or lift she pressed open this life holds open for now (as `states` has
    /// them), and the tic each starts back. (Pressed: what she opened to go through; not a lift
    /// she walked onto, which takes her away from where she was: E2M4's lift 66.)
    pub(super) fn open_for_now(&self, tic: i64) -> BTreeMap<usize, i64> {
        let (k, nav) = (&self.knowledge, &self.nav);
        let known: BTreeMap<i64, (TriggerLine, BTreeSet<usize>)> = k.known_triggers().into_iter().map(|(t, s)| (t.line, (t, s))).collect();
        let mut out: BTreeMap<usize, i64> = BTreeMap::new();
        for f in k.fired.iter().filter(|f| f.life == k.lives && f.how == How::Use) {
            let Some((t, sectors)) = known.get(&(f.line as i64)) else { continue };
            if t.permanent() || t.line < 0 || !cycles_back(t.info.special, &t.info.description) {
                continue;
            }
            for (s, hs) in t.effects.iter().filter(|(s, _)| sectors.contains(s)) {
                let Some(&h) = hs.first() else { continue };
                let timing = nav.movers.timing(t.info.special, &t.info.description, *s, h);
                let Some(hold) = timing.hold else { continue };
                let until = f.tic + (timing.travel + hold) as i64;
                if tic < until {
                    let e = out.entry(*s).or_insert(until);
                    *e = (*e).max(until);
                }
            }
        }
        out
    }

    pub(super) fn spent(&self, fresh: bool) -> (BTreeSet<usize>, BTreeSet<usize>) {
        let k = &self.knowledge;
        let once_used = |pred: &dyn Fn(Trigger, Category) -> bool| -> BTreeSet<usize> {
            if fresh {
                return BTreeSet::new();
            }
            k.fired.iter().filter(|f| f.life == k.lives && k.special(f.line).is_some_and(|i| pred(i.trigger, i.category))).map(|f| f.line).collect()
        };
        // (once-only door lines used this life open nothing again: MAP19's door)
        let spent_doors = once_used(&|t, _| t == Trigger::D1);
        // (and once-only teleporters used this life send no one anywhere: MAP27's)
        let spent_teleports = once_used(&|t, c| t == Trigger::W1 && c == Category::Teleport);
        (spent_doors, spent_teleports)
    }

    pub(super) fn blocked(&self) -> BTreeSet<usize> {
        self.knowledge.stuck_at.iter().filter(|(_, &n)| n >= STUCK_TIMES).map(|(&p, _)| p).collect()
    }

    pub(super) fn view_key(&self, fresh: bool) -> ViewKey {
        let k = &self.knowledge;
        let (spent_doors, spent_teleports) = self.spent(fresh);
        ViewKey {
            sectors: k.sectors.len(),
            seen_polys: k.seen_polys.len(),
            visited: k.visited.len(),
            learned: k.learned.values().map(BTreeSet::len).sum(),
            teleports: k.teleports.len(),
            spent_doors,
            spent_teleports,
            exits_open: fresh || self.exits_open,
            blocked: self.blocked(),
            difficult: self.difficult,
        }
    }

    fn build_view(&self, fresh: bool) -> View {
        let k = &self.knowledge;
        let (spent_doors, spent_teleports) = self.spent(fresh);
        let exits_open = fresh || self.exits_open;
        let blocked = self.blocked();
        let nav = &self.nav;
        let mut triggers = Vec::new();
        for (t, sectors) in k.known_triggers() {
            if t.linedef().is_some_and(|li| spent_doors.contains(&li)) {
                continue;
            }
            // (and the slots beside them: walked at their sides' floor, they never move to be
            // seen to; MAP08's moat)
            let mut keep: BTreeSet<usize> = sectors.clone();
            for (s, _) in &t.effects {
                if nav.trenches.get(s).is_some_and(|around| around.iter().any(|x| sectors.contains(x))) {
                    keep.insert(*s);
                }
            }
            let mut known = t.clone();
            known.effects.retain(|(s, _)| keep.contains(s));
            known.keep = Some(keep);
            triggers.push(known);
        }
        let mut keys_at: Vec<(Key, Vec<Point>)> = Vec::new();
        for th in k.known_things() {
            if let Some(kk) = Key::from_thing(th.kind) {
                match keys_at.iter_mut().find(|(x, _)| *x == kk) {
                    Some((_, v)) => v.push((th.x, th.y)),
                    None => keys_at.push((kk, vec![(th.x, th.y)])),
                }
            }
        }
        let mut polys: BTreeSet<usize> = k.known_polys().difference(&blocked).copied().collect();
        if !exits_open {
            polys.retain(|p| !self.pads.0.contains(p));
        }
        let teleports: BTreeSet<usize> = k.teleports.keys().copied().filter(|l| !spent_teleports.contains(l)).collect();
        let banned = if fresh { BTreeSet::new() } else { self.banned_jumps() };
        self.nav.restricted_to(Restriction { polys, teleports, spent: spent_teleports }, triggers, keys_at, banned)
    }

    /// The run-jumps left out: those missed too often (and, unless with nothing else to do,
    /// those missed at all).
    pub(super) fn banned_jumps(&self) -> BTreeSet<usize> {
        let limit = if self.difficult { JUMP_MISSES } else { 1 };
        let missed = &self.knowledge.missed_jumps;
        // (and strafing runs, but when there's nothing else to do: for where a run straight
        // won't go, not a way round; MAP13's, taken as one, led her off from what was left)
        self.nav
            .jumps
            .iter()
            .enumerate()
            .filter(|(_, j)| missed.get(&j.key()).copied().unwrap_or(0) >= limit || (j.strafe && !self.strafe_ok) || self.lapsing(&j.key()))
            .map(|(i, _)| i)
            .collect()
    }

    /// Is the jump one left out for coming too late for something timed (and she's learned
    /// nothing since that might change it)?
    fn lapsing(&self, key: &str) -> bool {
        let (jumps, learned) = &self.lapsing_jumps;
        *learned == self.knowledge.learned.values().map(BTreeSet::len).sum::<usize>() && jumps.contains(key)
    }

    pub(super) fn set_jumps(&mut self, difficult: bool) {
        self.difficult = difficult;
        self.view = None; // (it holds the old edges)
    }
}
