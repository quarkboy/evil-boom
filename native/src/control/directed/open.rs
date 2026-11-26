// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What's open to her now: the objectives by kind, what she's found, what a switch does,
//! what's in an area, what's left to search.

use super::*;

/// the explorer's ranking is kept this long at the most (tics), and with more known since, this
/// long at the least
const OPEN_KEPT_MOST: i64 = 35;
const OPEN_KEPT_LEAST: i64 = 12;

/// A ranking kept (`Director::ranked`): for which (risky, exits held back), when, what she knew.
#[derive(Clone, Debug)]
pub(super) struct OpenKept {
    key: (bool, bool),
    tic: i64,
    knows: ([usize; 4], usize, u64),
    open: Vec<Objective>,
}

impl Director {
    /// The explorer's ranking of what she can do (the safe objectives, or the risky), kept a
    /// while: its whole reckoning, tens of milliseconds on a big level well known, with the
    /// game's picture waiting on it; and a look, a list and a suggestion each ask for it, twice
    /// over. What she's told from it may be a second old (the maintainer's call: a list a
    /// moment out of date matters less than a frame held): kept while she knows no more than
    /// she did, a second at the most; with more known, a third of a second at the least. Kept
    /// apart by whether the exits are held back from it, and dropped when an order of hers
    /// begins (`fresh_ranking`).
    fn ranked(&self, agent: &mut Agent, obs: &Observation, risky: bool) -> Vec<Objective> {
        let key = (risky, agent.x().exit_held_back);
        // (co-op: what the others are at, left to them, changes it too: kept no longer then)
        let claims = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for (k, at, wall) in &agent.x().claimed {
                format!("{k:?}").hash(&mut h);
                (at.0.round() as i64, at.1.round() as i64, *wall).hash(&mut h);
            }
            h.finish()
        };
        let knows = (
            agent.x().knowledge.amount(),
            obs.keys.unwrap_or_default().iter().count(),
            claims,
        );
        if let Some(k) = self.open_kept.borrow().iter().find(|k| k.key == key) {
            let age = obs.tic - k.tic;
            if (0..OPEN_KEPT_MOST).contains(&age)
                && k.knows.2 == knows.2
                && (k.knows == knows || age < OPEN_KEPT_LEAST)
            {
                return k.open.clone();
            }
        }
        let open = crate::debug::timed("director.ranked", || agent.ranked(obs, risky));
        let mut kept = self.open_kept.borrow_mut();
        kept.retain(|k| k.key != key);
        kept.push(OpenKept {
            key,
            tic: obs.tic,
            knows,
            open: open.clone(),
        });
        open
    }

    /// What's kept of the explorer's ranking dropped: the next asked for is worked out anew.
    pub(super) fn fresh_ranking(&self) {
        self.open_kept.borrow_mut().clear();
    }

    /// The explorer's open objectives (best first, risky ones after).
    pub(super) fn open_objectives(&self, agent: &mut Agent, obs: &Observation) -> Vec<Objective> {
        let mut out: Vec<Objective> = Vec::new();
        for risky in [false, true] {
            for o in self.ranked(agent, obs, risky) {
                if !out.iter().any(|p| id_of(p) == id_of(&o)) {
                    out.push(o);
                }
            }
        }
        out
    }

    /// The explorer's objectives of those kinds as entries: doors, switches, exits (what each
    /// is: only what the id doesn't say, a key it needs, a secret exit; not the explorer's label).
    pub(super) fn objectives_of(
        &self,
        agent: &mut Agent,
        obs: &Observation,
        kinds: &[Kind],
    ) -> Vec<Entry> {
        let open = if kinds.contains(&Kind::Exit) {
            self.open_unheld(agent, obs)
        } else {
            self.open_objectives(agent, obs)
        };
        entries_of(&open, kinds)
    }

    /// One of the two listings `open_unheld` is made of: the safe objectives or the risky.
    pub(super) fn open_half(
        &self,
        agent: &mut Agent,
        obs: &Observation,
        risky: bool,
    ) -> Vec<Objective> {
        let held = agent.x().exit_held_back;
        if held {
            agent.x_mut().hold_exits(false);
        }
        let open = self.ranked(agent, obs, risky);
        if held {
            agent.x_mut().hold_exits(true);
        }
        open
    }

    /// The explorer's open objectives, the exits among them even while they're held back from
    /// it (the exits she knows are hers to hear of and to list then too: whether she may leave
    /// is one question, whether there's a way out another).
    pub(super) fn open_unheld(&self, agent: &mut Agent, obs: &Observation) -> Vec<Objective> {
        let held = agent.x().exit_held_back;
        if held {
            agent.x_mut().hold_exits(false);
        }
        let open = self.open_objectives(agent, obs);
        if held {
            agent.x_mut().hold_exits(true);
        }
        open
    }

    /// What an exploring order leaves to her, as sentences for its end ("" if nothing): the
    /// switches found and not pressed, and the walls still to search at each thoroughness.
    pub(super) fn found(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        fits: impl Fn(&Self, &Agent, &Objective) -> bool,
    ) -> String {
        let mut out = self.switches_found(agent, obs, &fits);
        let walls = self.walls_to_search(agent, obs, 3);
        let [a, b, c] = by_thoroughness(walls.iter().filter(|o| fits(self, agent, o)));
        if c > 0 {
            out.push_str(&format!(" Walls to search: {a}/{b}/{c}."));
        }
        out
    }

    /// The switches not pressed yet that `fits` (where she'd press them from), as a sentence for an
    /// exploring order's end ("" if none).
    pub(super) fn switches_found(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        fits: impl Fn(&Self, &Agent, &Objective) -> bool,
    ) -> String {
        let found: Vec<Objective> = self
            .open_objectives(agent, obs)
            .into_iter()
            .filter(|o| o.kind == Kind::Switch && fits(self, agent, o))
            .collect();
        if found.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = found
            .iter()
            .map(|o| {
                self.describe(
                    &Entry {
                        id: id_of(o),
                        what: String::new(),
                        at: o.goal,
                    },
                    obs,
                )
            })
            .collect();
        format!(" Switches: {}.", lines.join(", "))
    }

    /// What a line (a switch) does, as far as she knows: from having set it off, or another of the
    /// same mechanism. None if not known (never pressed: she's no way to know).
    pub(super) fn effect_of(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        line: usize,
    ) -> Option<String> {
        let sectors = agent
            .x()
            .knowledge
            .known_triggers()
            .into_iter()
            .find(|(t, _)| t.line == line as i64)
            .map(|(_, s)| s)?;
        if sectors.is_empty() {
            return None;
        }
        let pos = obs.pos();
        let places = self.places(agent);
        let mut areas: Vec<&Area> = sectors.iter().map(|&s| places.area_of(s)).collect();
        areas.sort_by(|a, b| a.id.cmp(&b.id));
        areas.dedup_by(|a, b| a.id == b.id);
        let wheres: Vec<String> = areas
            .iter()
            .map(|a| {
                format!(
                    "{} ({} {:.0})",
                    a.id,
                    compass(heading(pos, a.center)),
                    dist(pos, a.center)
                )
            })
            .collect();
        Some(format!("moves {}", wheres.join(", ")))
    }

    /// Is objective o one of exploring area `id`'s: done standing in it? (Looking out of it from
    /// inside, into a doorway or at the next room's wall, is exploring it; standing in the next
    /// room looking back isn't.)
    pub(super) fn in_area(&self, agent: &Agent, id: &str, o: &Objective) -> bool {
        let Some(places) = &self.places else {
            return false;
        };
        let nav = &agent.x().nav;
        nav.poly_at(o.goal)
            .is_some_and(|q| places.area_of(nav.sector_of_poly(q)).id == id)
    }

    /// Walls worth pressing for a secret, this thoroughly: none once every secret the level
    /// counts is found (whatever she's asked for: there's none left behind one).
    pub(super) fn walls_to_search(
        &self,
        agent: &mut Agent,
        obs: &Observation,
        how: u8,
    ) -> Vec<Objective> {
        if self.secrets_done() {
            return Vec::new();
        }
        agent.x_mut().suspected_walls(obs, how)
    }

    /// Where a secret's likeliest, for a search of hers: the areas with the walls the lightest
    /// search presses (none: the next lightest), most first, as "area-0 (3), area-24 (1)"; ""
    /// if there's none, or no secret left.
    pub(super) fn likely_areas(&mut self, agent: &mut Agent, obs: &Observation) -> String {
        let nav = std::rc::Rc::clone(&agent.x().nav);
        for how in [1, 2] {
            let walls = self.walls_to_search(agent, obs, how);
            let places = self.places(agent);
            let mut n: BTreeMap<String, usize> = BTreeMap::new();
            for o in &walls {
                if let Some(p) = nav.poly_at(o.goal) {
                    *n.entry(places.area_of(nav.sector_of_poly(p)).id.clone())
                        .or_default() += 1;
                }
            }
            if n.is_empty() {
                continue;
            }
            let mut areas: Vec<(String, usize)> = n.into_iter().collect();
            areas.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            return areas
                .iter()
                .take(3)
                .map(|(id, n)| format!("{id} ({n})"))
                .collect::<Vec<_>>()
                .join(", ");
        }
        String::new()
    }

    /// Every secret the level counts found (the tally, as the automap's: hers to know).
    pub fn secrets_done(&self) -> bool {
        self.tally.secrets.0 >= self.tally.secrets.1
    }

    /// The best of the explorer's objectives that `fits` (and an exploring one: not out by an exit,
    /// not off through a teleporter), taken up. False if there's none (or no way to any).
    pub(super) fn next_in(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        secrets: u8,
        fits: impl Fn(&Self, &Agent, &Objective) -> bool,
    ) -> bool {
        let open = self.open_objectives(agent, obs);
        // (the walls searched that thoroughly, not the strong ones by her the explorer offers;
        // after the rest, as a player does: what's in plain sight first)
        let walls = self.walls_to_search(agent, obs, secrets);
        let mut options: Vec<Objective> = open
            .into_iter()
            .filter(explores)
            .chain(walls)
            .filter(|o| !self.taken.contains(&taken_key(o)))
            .filter(|o| fits(self, agent, o))
            .collect();
        // (co-op: what another player's at, after the rest; the walls searched too, which aren't
        // the explorer's ranking and weren't left to her)
        options.sort_by_key(|o| agent.x().claimed_by_mate(o));
        for o in options {
            let id = taken_key(&o);
            if agent.x_mut().take(obs, o) {
                self.taken.insert(id);
                return true;
            }
            self.taken.insert(id); // (no way to it: not tried again for this order either)
        }
        false
    }
}
