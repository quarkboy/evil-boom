//! Sifting a list of things to do down to those she'd go for now: nowhere that would cut her
//! off from most of what she can get to, as far as she knows; no switch on a floor that hurts
//! while there's floor that doesn't; and with nothing safe left (`risky`), nearly anything.

use super::{Explorer, Hooks, Kind, Objective, KEEP_REACH};
use crate::motor::Observation;
use crate::movers::cycles_back;
use crate::nav::Doors;
use crate::navigator::{add_height, Navigator, States, TriggerLine, View, PLAYER_HEIGHT};
use crate::specials::{Category, Keys};
use std::collections::{BTreeMap, BTreeSet};

/// on what's left till after what it would cut her off from
const LATER: &str = " (after what it cuts off)";

/// Where she can get to now, as she knows the level.
#[derive(Clone, Copy)]
pub(super) struct Reach<'a> {
    /// the poly she's on, and the keys she has
    pub here: usize,
    pub keys: Keys,
    /// the polys she can get to
    pub polys: &'a BTreeSet<usize>,
    /// the heights she believes each sector at now
    pub states: &'a States,
    /// every height each sector can come to by what she can set off
    pub states_all: &'a States,
}

/// The way back from a place, as it will be once she's there: a door that closes again is shut,
/// unless something opens it for good or she can open it from where she'll be; a lift likewise
/// is at rest unless she can call it from there.
pub(super) struct WayBack<'a> {
    nav: &'a Navigator,
    view: &'a View,
    now: Reach<'a>,
    /// the same with what comes back shut again
    states_back: States,
    doors: Doors,
    /// the doors and lifts that come back, each with the sectors it would have to open again
    openers: Vec<(TriggerLine, BTreeSet<usize>)>,
    known_polys: BTreeSet<usize>,
    /// the sectors she's set foot in
    entered: BTreeSet<usize>,
    /// the lines she has pressed or set off, and the walls she's tried
    pressed: BTreeSet<usize>,
    /// switches and doors she knows of and hasn't tried
    untried: Vec<&'a TriggerLine>,
    /// where she could get back to from each poly asked about so far
    afters: BTreeMap<usize, BTreeSet<usize>>,
}

impl WayBack<'_> {
    /// Where she can get to from `start` with what comes back shut, opening on the way the doors
    /// she can open from there: by a switch or a walk-over line on the far side too.
    fn from(&self, start: usize) -> BTreeSet<usize> {
        let nav = self.nav;
        let mut st = self.states_back.clone();
        loop {
            let after = self.view.reachable(start, &st, Some(&self.doors));
            let more: Vec<&BTreeSet<usize>> = self
                .openers
                .iter()
                .filter(|(t, secs)| {
                    !t.activators.is_disjoint(&after)
                        && t.info.lock.satisfied(self.now.keys)
                        && secs.iter().any(|&x| st[x] == vec![nav.movers.initial[x]])
                })
                .map(|(_, secs)| secs)
                .collect();
            let before = st.clone();
            for secs in more {
                for &x in secs {
                    for &h in &self.now.states_all[x] {
                        add_height(&mut st[x], h);
                    }
                }
            }
            if st == before {
                return after; // (nothing more opens)
            }
        }
    }

    /// What of where she can get to now she couldn't get back to from poly `p`; and whether
    /// where she'd be left is still open to finding a way out: an edge onto somewhere she's never
    /// seen, a sector there she's never set foot in other than the goal's (where a walk-over
    /// line can be), a door's face she hasn't tried, a switch or door not tried yet that she
    /// could press from there (as `shuts_in` has it). No way out known isn't no way out.
    fn lost_from(&mut self, p: usize) -> (BTreeSet<usize>, bool) {
        if !self.afters.contains_key(&p) {
            let after = self.from(p);
            self.afters.insert(p, after);
        }
        let nav = self.nav;
        let after = &self.afters[&p];
        if after.contains(&self.now.here) {
            return (BTreeSet::new(), true);
        }
        let shut = |sec: usize| {
            self.now.states[sec]
                .iter()
                .all(|h| h.ceiling - h.floor < PLAYER_HEIGHT)
        };
        let goal_sector = nav.sector_of_poly(p);
        let unseen_edge = after.iter().any(|&q| {
            nav.mesh
                .poly(q)
                .portals
                .iter()
                .any(|&pi| !self.known_polys.contains(&nav.mesh.portals[pi].other(q)))
        });
        let unwalked = after
            .iter()
            .map(|&q| nav.sector_of_poly(q))
            .any(|sec| sec != goal_sector && !self.entered.contains(&sec));
        let untried_door = after.iter().any(|&q| {
            nav.mesh.poly(q).portals.iter().any(|&pi| {
                let portal = &nav.mesh.portals[pi];
                let other = nav.sector_of_poly(portal.other(q));
                shut(other) && portal.linedef.is_some_and(|li| !self.pressed.contains(&li))
            })
        });
        let untried_switch = self
            .untried
            .iter()
            .any(|t| !t.activators.is_disjoint(after));
        (
            self.now.polys.difference(after).copied().collect(),
            unseen_edge || unwalked || untried_door || untried_switch,
        )
    }
}

/// What a list of things to do is sifted by (`Explorer::usable`).
pub(super) struct Sift<'a> {
    back: WayBack<'a>,
    /// where she can set off an exit from, as things are
    exit_polys: BTreeSet<usize>,
    /// shut in on a floor that hurts, or as good as: a switch there is hers to try
    shut_in: bool,
    /// down in a pit whose floor hurts, no floor that doesn't in reach
    in_pit: bool,
    /// nothing safe is left: anything goes, nearly
    risky: bool,
}

impl Explorer {
    /// The way back as she knows it, from where she can get to now.
    pub(super) fn way_back<'a>(
        &self,
        nav: &'a Navigator,
        view: &'a View,
        now: Reach<'a>,
        by_line: &BTreeMap<usize, &'a TriggerLine>,
    ) -> WayBack<'a> {
        let spent = self.spent_once();
        let doors = view.local_doors(now.keys);
        let known_triggers = self.knowledge.known_triggers();
        // Not where a door or lift that comes back was set off since one that stays: pressed again
        // from outside, a door open for good can shut behind her.
        let last_fired = |pred: &dyn Fn(&TriggerLine) -> bool, s: usize| -> i64 {
            let lines: BTreeSet<i64> = known_triggers
                .iter()
                .filter(|(t, secs)| pred(t) && secs.contains(&s))
                .map(|(t, _)| t.line)
                .collect();
            self.knowledge
                .fired
                .iter()
                .filter(|f| f.life == self.knowledge.lives && lines.contains(&(f.line as i64)))
                .map(|f| f.tic)
                .max()
                .unwrap_or(-1)
        };
        let for_good: BTreeSet<usize> = known_triggers
            .iter()
            .filter(|(t, _)| t.permanent())
            .flat_map(|(_, s)| s.iter().copied())
            .filter(|&s| {
                last_fired(
                    &|t| !t.permanent() && cycles_back(t.info.special, &t.info.description),
                    s,
                ) <= last_fired(&|t| t.permanent(), s)
            })
            .collect();
        let mut states_back = now.states_all.clone();
        let mut openers: Vec<(TriggerLine, BTreeSet<usize>)> = Vec::new();
        for (t, secs) in &known_triggers {
            let comes_back = t.info.category.is_door()
                || (t.info.category == Category::Lift
                    && cycles_back(t.info.special, &t.info.description));
            if !t.permanent() && comes_back && !t.linedef().is_some_and(|li| spent.contains(&li)) {
                let shut: BTreeSet<usize> = secs.difference(&for_good).copied().collect();
                for &s in &shut {
                    states_back[s] = vec![nav.movers.initial[s]];
                }
                openers.push((t.clone(), shut));
            }
        }
        WayBack {
            nav,
            view,
            now,
            states_back,
            doors,
            openers,
            known_polys: self.knowledge.known_polys(),
            entered: self
                .knowledge
                .visited
                .iter()
                .map(|&q| nav.sector_of_poly(q))
                .collect(),
            pressed: self
                .knowledge
                .fired
                .iter()
                .map(|f| f.line)
                .chain(self.knowledge.tried_walls.keys().copied())
                .collect(),
            untried: self
                .knowledge
                .interactables()
                .iter()
                .filter_map(|li| by_line.get(li).copied())
                .collect(),
            afters: BTreeMap::new(),
        }
    }

    /// What this list is sifted by, as she stands.
    ///
    /// No switch is pressed from a floor that hurts while there's floor that doesn't to get to:
    /// what a switch does is anyone's guess, and shut in there is dead, suit or no suit (it runs
    /// out). Shut in already, with no floor that doesn't hurt in reach, it's hers to try; and
    /// with nothing else at all to do, walls and all, as `harm_allowed` has it. A floor that
    /// doesn't hurt reached only through a door that shuts again by itself, with nothing on her
    /// side to open it, is no way out either: she's shut in once it shuts. Nor is the dry floor
    /// she waded off from a dead end: there was nothing to do there, so a switch down here is
    /// hers to try before she goes back to stand on it.
    ///
    /// Down in a pit whose floor hurts, with no floor that doesn't in reach, every second costs
    /// health: a switch or a door she can see (a way out someone built) comes before a guess at
    /// a plain wall.
    pub(super) fn sift<'a>(
        &self,
        obs: &Observation,
        risky: bool,
        back: WayBack<'a>,
        exit_polys: BTreeSet<usize>,
    ) -> Sift<'a> {
        let nav = &self.nav;
        let safe = |polys: &BTreeSet<usize>| {
            polys
                .iter()
                .any(|&p| !self.knowledge.damaging(nav.sector_of_poly(p)))
        };
        let stays_shut = self.knowledge.damaging(nav.sector_of_poly(back.now.here))
            && !safe(&back.from(back.now.here));
        let shut_in =
            self.harm_allowed || self.waded_dead_end || stays_shut || !safe(back.now.polys);
        let in_pit = self.knowledge.mortal
            && self.on_hurting_floor(obs)
            && (stays_shut || self.waded_dead_end || !safe(back.now.polys));
        Sift {
            back,
            exit_polys,
            shut_in,
            in_pit,
            risky,
        }
    }

    /// An item the level put somewhere has a way back out from it, if not one she knows yet:
    /// nobody makes a pit with a prize and no way out. Not one a monster dropped, which can fall
    /// anywhere.
    fn placed(&self, obs: &Observation, o: &Objective) -> bool {
        o.kind == Kind::Item
            && o.thing
                .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
                .is_some_and(|t| {
                    self.nav
                        .map
                        .things
                        .iter()
                        .any(|m| (m.x - t.x).abs() < 1.0 && (m.y - t.y).abs() < 1.0)
                })
    }

    /// Those of `objectives` she'd go for now.
    pub(super) fn usable(
        &self,
        obs: &Observation,
        hooks: &mut dyn Hooks,
        sift: &mut Sift,
        objectives: Vec<Objective>,
    ) -> Vec<Objective> {
        let nav = &self.nav;
        let tic = obs.tic;
        let Sift {
            back,
            exit_polys,
            shut_in,
            in_pit,
            risky,
        } = sift;
        let switch_in_harm = |o: &Objective| {
            o.kind == Kind::Switch
                && !*shut_in
                && nav
                    .poly_at(o.goal)
                    .is_some_and(|p| self.knowledge.damaging(nav.sector_of_poly(p)))
        };
        let mut objectives: Vec<Objective> = objectives
            .into_iter()
            .filter(|o| !switch_in_harm(o))
            .collect();
        if *in_pit
            && objectives
                .iter()
                .any(|o| matches!(o.kind, Kind::Switch | Kind::Door))
        {
            objectives.retain(|o| o.kind != Kind::Wall);
        }
        if *risky {
            // Nothing safe left: anything, one-way drops and nukage and all. But not for what's worth
            // little: a sound, a place seen already looked at again, a monster gone from where it was
            // last seen. The health a way costs is weighed against what it's for. And never away from
            // every way out there is, as far as she knows.
            return objectives
                .into_iter()
                // But a sound just after she set something off, with her on a floor that hurts already:
                // what she set off is as like as not the way off it, and going to see is no new risk.
                .filter(|o| {
                    !matches!(o.kind, Kind::Noise | Kind::Recheck | Kind::Hunt)
                        || (o.kind == Kind::Noise && o.urgent && self.on_hurting_floor(obs))
                })
                .filter(|o| {
                    exit_polys.is_empty()
                        || o.kind == Kind::Exit
                        || self.placed(obs, o)
                        || nav
                            .poly_at(o.goal)
                            .map(|p| back.lost_from(p))
                            .is_none_or(|(g, open)| open || !exit_polys.is_subset(&g))
                })
                .map(|o| Objective {
                    label: format!("{} (risky)", o.label),
                    risky: true,
                    ..o
                })
                .filter(|o| self.fresh(o, tic))
                .collect::<Vec<_>>()
                .into_iter()
                // Risky as to the way there, not as to what's there. Items only: they're to be had or not.
                // The ways on go through it if need be.
                .filter(|o| o.kind != Kind::Item || !hooks.dangerous(obs, o))
                .collect();
        }
        let mut kept: Vec<Objective> = Vec::new();
        let mut lost: Vec<BTreeSet<usize>> = Vec::new();
        for o in objectives {
            if !self.fresh(&o, tic) || hooks.dangerous(obs, &o) {
                continue;
            }
            let cut = nav.poly_at(o.goal).map(|p| back.lost_from(p));
            let open = cut.as_ref().is_none_or(|c| c.1) || self.placed(obs, &o);
            let gone = cut.map(|c| c.0);
            // Worth the risk, however far she can't get back from: the exit; a teleporter (where it
            // leads is the unknown); a key (it opens the way). And ammo or a gun the level put there
            // when she's next to out of it: nothing she can't get back to is worth much without, and
            // there's a way out from where the level put it.
            let risk_it = matches!(o.kind, Kind::Exit | Kind::Teleporter | Kind::Key)
                || (self.placed(obs, &o) && hooks.worth_cut_off(obs, &o));
            // Most of what she can get to, and never the way out, as far as she knows.
            let reach = back.now.polys;
            let keeps = gone.as_ref().is_some_and(|g| {
                (reach.len() - g.len()) as f64 >= KEEP_REACH * reach.len() as f64
                    && (exit_polys.is_empty() || open || !exit_polys.is_subset(g))
            });
            if risk_it || keeps {
                lost.push(gone.unwrap_or_default());
                kept.push(o);
            }
        }
        // What would cut her off from where there's more to do, as far as she knows, comes after
        // that: as a player finishes a room before dropping out of it.
        let goals: Vec<Option<usize>> = kept.iter().map(|o| nav.poly_at(o.goal)).collect();
        for (i, o) in kept.iter_mut().enumerate() {
            if o.kind == Kind::Exit || lost[i].is_empty() {
                continue;
            }
            if goals
                .iter()
                .enumerate()
                .any(|(j, g)| j != i && g.is_some_and(|g| lost[i].contains(&g)))
            {
                o.later = true;
                o.label = format!("{}{LATER}", o.label);
            }
        }
        // Something she needs now that would cut her off still waits while there's something she
        // needs now that wouldn't.
        if kept
            .iter()
            .any(|o| o.kind == Kind::Item && o.urgent && !o.later)
        {
            for o in kept.iter_mut().filter(|o| o.kind == Kind::Item && o.later) {
                o.urgent = false;
            }
        }
        kept
    }
}
