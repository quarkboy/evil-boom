//! How long she keeps at a monster she's getting nowhere with: one ledger for every such clock.
//! An effort (closing in on what's out of reach, standing with no shot, hitting it and not
//! hurting it, walking at it and getting no nearer) is kept at for so long, the clock set back
//! by progress where there's a measure of it, started afresh after a break where the effort's one
//! that comes and goes; out of patience, the monster's given up on a while, whichever effort it
//! was.

use std::collections::BTreeMap;

/// What she's trying, at one monster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effort {
    /// closing in on it, out of the gun's reach
    Closing,
    /// standing to fight it with no shot at it
    NoShot,
    /// at it in melee: its health is the measure
    MeleeUnhurt,
    /// closing in to hit it in melee: how near she's got is the measure
    MeleeNoNearer,
}

#[derive(Clone, Copy, Debug)]
struct Trying {
    /// since when, or since the last progress
    since: i64,
    /// when last at it
    last: i64,
    /// the measure's best yet
    best: f64,
}

/// The ledger.
#[derive(Clone, Debug, Default)]
pub struct Patience {
    trying: BTreeMap<(Effort, i32), Trying>,
    /// given up on, by id, till when
    given_up: BTreeMap<i32, i64>,
}

impl Patience {
    /// She's at `effort` on monster `id` this tic: is she out of patience with it? Kept at for
    /// `limit` tics. `measure`: (what it's at now, how much less than its best yet counts as
    /// progress): progress sets the clock back. `stale`: not at it for longer than this, the
    /// clock starts afresh. Out of patience, the effort's forgotten (the next starts afresh):
    /// the caller gives the monster up.
    pub fn spent(
        &mut self,
        effort: Effort,
        id: i32,
        tic: i64,
        limit: i64,
        measure: Option<(f64, f64)>,
        stale: Option<i64>,
    ) -> bool {
        let fresh = Trying {
            since: tic,
            last: tic,
            best: measure.map_or(0.0, |m| m.0),
        };
        let mut t = self
            .trying
            .get(&(effort, id))
            .copied()
            .filter(|t| stale.is_none_or(|s| tic - t.last <= s))
            .unwrap_or(fresh);
        if let Some((now, _)) = measure.filter(|&(now, gain)| now < t.best - gain) {
            t.since = tic;
            t.best = now;
        }
        if tic - t.since > limit {
            self.trying.remove(&(effort, id));
            return true;
        }
        t.last = tic;
        self.trying.insert((effort, id), t);
        false
    }

    /// The effort's got somewhere, or is over: its clock from the start next time.
    pub fn rest(&mut self, effort: Effort, id: i32) {
        self.trying.remove(&(effort, id));
    }

    /// Give id up till `until`.
    pub fn give_up(&mut self, id: i32, until: i64) {
        self.given_up.insert(id, until);
    }

    /// Is id given up on, at `tic`?
    pub fn given_up(&self, id: i32, tic: i64) -> bool {
        self.given_up.get(&id).is_some_and(|&until| tic < until)
    }
}
