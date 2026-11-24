// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Listing and ranking objectives in the currently reachable world.

use super::cut_off::Reach;
use super::{compass, Explorer, Focus, GoingBack, Hooks, Kind, Objective, Suspecting, WHILE_OPEN};
use crate::domain::Observation;
use crate::geom::{dist, segment_distance, Point};
use crate::knowledge::model::Heard;
use crate::nav::can_cross;
use crate::navigator::{States, TeleEdge, TriggerLine, View, EXIT_FLOOR, MAX_STEP, PLAYER_HEIGHT};
use crate::specials::{classify_line, Key, Keys, Trigger};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Below this health, healing she's seen is worth seeking even without a blocked route.
const HEAL_BELOW: f64 = 50.0;

/// For this many tics after a route cost too much, seek healing or a suit to afford it.
const SHORT_TICS: i64 = 60 * 35;

/// bullets and shells below these, she tops up from the ammo she's seen
const LOW_BULLETS: i32 = 10;

const LOW_SHELLS: i32 = 4;

/// what gives bullets or shells for tapping walls and gun lines
const TAP_AMMO: [&str; 7] = [
    "Clip", "ClipBox", "Shell", "ShellBox", "Backpack", "Chaingun", "Shotgun",
];

/// a threshold's spot: this far over the line
const THRESHOLD_IN: f64 = 8.0;
/// How far over a line never walked over she makes for.
const SEAM_IN: f64 = 32.0;

/// the expert's run: no goal this near a walk-over exit line (she'd slide over)
const EXIT_MARGIN: f64 = 64.0;

/// a wall suspected this much is tried when she's this near where she'd stand to (not only when
/// there's nothing else to do)
pub(super) const NEAR_SUSPECT: f64 = 6.0;
/// the secrets left her goal: the walls searched as thoroughly as a search asked for is by default
const SEARCH_AS: u8 = 2;
/// what she needs is far off from this (as the crow flies): the walls round her first...
const SUPPLY_FAR: f64 = 1024.0;
/// ...those within this of her (and no further than half way to it)...
const SEARCH_NEAR: f64 = 768.0;
/// ...searched this hard (the most a search for secrets goes to)
const SEARCH_HARD: u8 = 3;

/// As she passes it, that is: a hall's width off, not only at her elbow.
const NEAR_WALL: f64 = 448.0;

/// a repeatable switch whose effect wasn't seen: another go after this
const REPRESS_AFTER: i64 = 60 * 35;

/// a switch she knows, this near her when there's nothing left to do, is pressed again
const SWITCH_AT_HAND: f64 = 256.0;
/// ...once what moved since she last pressed it has been still this long (tics)
const MOVED_STILL: i64 = 35;

/// a sound is worth going to look into for this long
const NOISE_TICS: i64 = 60 * 35;

/// a sound this soon after she set off a line she doesn't know...
const CAUSE_TICS: i64 = 35;

/// ...is gone to look at first, for this long (a timed door shuts)
const URGENT_TICS: i64 = 6 * 35;

/// how fast she's taken to get places while a door she opened holds (units a tic: below a run)
const OPEN_SPEED: f64 = 10.0;

/// a noise is worth walking towards if it gets her this much nearer
const HEAR_NEARER: f64 = 128.0;

/// a frontier's look-out: this far back from the way into the unseen
const FRONTIER_BACK: f64 = 24.0;

/// a room's walked once nowhere in it is further than this from her steps
const COVER_GAP: f64 = 96.0;

impl Explorer {
    /// How what she'd be offered (the saturated reach) gets her to `goal`: the flood fill's way
    /// there, at all the heights the triggers she could fire give (`drive reach`); or, `planner`,
    /// as the planner's first check has it (no use-doors among the triggers, keys she knows of).
    pub fn reach_chain(
        &mut self,
        obs: &Observation,
        goal: Point,
        planner: bool,
    ) -> Option<Vec<String>> {
        let nav = Rc::clone(&self.nav);
        let view = self.view();
        let here = nav.poly_at(obs.pos())?;
        let to = nav.poly_at(goal)?;
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        let spent = self.spent_once();
        let mut key_items: Vec<(Key, Vec<(usize, Point)>)> = Vec::new();
        if planner {
            for th in self.knowledge.known_things() {
                if let Some(kk) = Key::from_thing(th.kind) {
                    key_items.push((kk, view.pickup_spots(th.x, th.y, &states)));
                }
            }
        }
        let actions: Vec<&TriggerLine> = view
            .triggers
            .iter()
            .filter(|t| {
                !t.effects.is_empty()
                    && !t.linedef().is_some_and(|li| spent.contains(&li))
                    && !(planner && t.info.trigger == Trigger::DR)
            })
            .collect();
        let (_, all, keys_all) = view.saturate(here, &states, keys, &key_items, &actions);
        view.reach_chain(here, to, &all, Some(&view.local_doors(keys_all)))
    }

    /// What she could go and do, for showing (a watcher's map): what her last choice had to
    /// choose from, kept from then on. Nothing is listed for the asking. A listing changes
    /// what her next choice goes by (what she can reach, the walls she suspects, as of the
    /// moment it's made), and one made between choices, once a second for the picture, sent
    /// the watched run another way than the same run unwatched (MAP21 and MAP23 of
    /// tools/eachway.sh's 24). Empty till her first choice after the first asking.
    pub fn objectives_shown(&mut self) -> Vec<Objective> {
        self.show_options = true;
        self.shown.clone()
    }

    /// What she could go and do now.
    pub fn objectives(
        &mut self,
        obs: &Observation,
        risky: bool,
        hooks: &mut dyn Hooks,
    ) -> Vec<Objective> {
        let t0 = Instant::now();
        let mut out = self.list_objectives(obs, risky, hooks);
        // (and what she'd take on to another level, weighed against the exit when it's next)
        let tic = obs.tic;
        for o in std::mem::take(&mut self.stock_aside) {
            if self.fresh(&o, tic) && !hooks.dangerous(obs, &o) {
                out.push(o);
            }
        }
        if self.knowledge.mortal {
            // Mortal: nowhere under a crusher to stop at just to look or walk round. What's to be
            // picked up or pressed there is still done, passing through.
            let crushers = self.crusher_sectors();
            out.retain(|o| {
                matches!(
                    o.kind,
                    Kind::Item
                        | Kind::Key
                        | Kind::Switch
                        | Kind::Door
                        | Kind::Exit
                        | Kind::Teleporter
                        | Kind::Secret
                ) || !self
                    .nav
                    .poly_at(o.goal)
                    .is_some_and(|p| crushers.contains(&self.nav.sector_of_poly(p)))
            });
        }
        self.stats.listed += 1;
        self.stats.listed_secs += t0.elapsed().as_secs_f64();
        if self.show_options && !risky {
            self.shown.clone_from(&out);
        }
        out
    }

    fn list_objectives(
        &mut self,
        obs: &Observation,
        risky: bool,
        hooks: &mut dyn Hooks,
    ) -> Vec<Objective> {
        let nav = Rc::clone(&self.nav);
        let view = self.view();
        let pos = obs.pos();
        // From the floor she's on: on a lip, not the drop her middle is over.
        let Some(here) = nav.poly_at(self.standing_point(obs)) else {
            return Vec::new();
        };
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        // What she could get to using what she has learned: the planner works out how.
        let spent = self.spent_once();
        let actions: Vec<&TriggerLine> = view
            .triggers
            .iter()
            .filter(|t| !t.effects.is_empty() && !t.linedef().is_some_and(|li| spent.contains(&li)))
            .collect();
        let t_sat = Instant::now();
        let (reach, states_all, _) = view.saturate(here, &states, keys, &[], &actions);
        self.stats.saturate_secs += t_sat.elapsed().as_secs_f64();
        let lap = crate::debug::lap("explorer.reach", t_sat);
        // (EVIL_DEBUG_REACH=<tic>: the first time it's worked out at or after that tic)
        static DUMPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if crate::debug::on().reach.is_some_and(|t| obs.tic >= t)
            && !DUMPED.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let sectors: BTreeSet<usize> = reach.iter().map(|&p| nav.sector_of_poly(p)).collect();
            self.log.push(format!(
                "tic {}: reach from poly {here} (sector {}): {} polys, sectors {sectors:?}",
                obs.tic,
                nav.sector_of_poly(here),
                reach.len()
            ));
            for &s in &crate::debug::on().heights {
                self.log.push(format!(
                    "  sector {s}: really at {:?}, last seen at {:?} (tic {:?}), believed {:?}",
                    obs.heights.get(s),
                    self.knowledge.seen_heights.get(&s),
                    self.knowledge.seen_tic.get(&s),
                    states.get(s)
                ));
            }
            for &p in &reach {
                let poly = nav.mesh.poly(p);
                self.log.push(format!(
                    "  poly {p}: {:?}",
                    poly.vertices
                        .iter()
                        .map(|v| (v.0.round(), v.1.round()))
                        .collect::<Vec<_>>()
                ));
                for &pi in &nav.mesh.poly(p).portals {
                    let portal = &nav.mesh.portals[pi];
                    let o = portal.other(p);
                    if !reach.contains(&o) {
                        self.log.push(format!(
                            "    (poly {o}: {:?})",
                            nav.mesh
                                .poly(o)
                                .vertices
                                .iter()
                                .map(|v| (v.0.round(), v.1.round()))
                                .collect::<Vec<_>>()
                        ));
                        self.log.push(format!("    out of reach past the portal ({:.0}, {:.0})-({:.0}, {:.0}) (line {:?}), {:.0} wide, into sector {}", portal.p1.0, portal.p1.1, portal.p2.0, portal.p2.1, portal.linedef, dist(portal.p1, portal.p2), nav.sector_of_poly(o)));
                    }
                }
            }
        }
        self.reach = reach.clone();
        self.reach_states = states_all.clone();
        // What she can get to only while a door or lift she has pressed open is open: go now, as a
        // player goes through a door before it shuts, if she's near enough to make it. Only for
        // somewhere she's never walked: where she's been can wait.
        let open = self.open_for_now(obs.tic);
        let while_open: BTreeSet<usize> = if open.is_empty() {
            BTreeSet::new()
        } else {
            let mut shut = states.clone();
            for &s in open.keys() {
                shut[s] = vec![nav.movers.initial[s]];
            }
            let (reach_shut, _, _) = view.saturate(here, &shut, keys, &[], &actions);
            let beyond: BTreeSet<usize> = reach.difference(&reach_shut).copied().collect();
            if beyond.is_disjoint(&self.knowledge.visited) {
                beyond
            } else {
                BTreeSet::new()
            }
        };
        let closes = open.values().copied().min().unwrap_or(obs.tic);
        let mut out: Vec<Objective> = Vec::new();
        let by_line: BTreeMap<usize, &TriggerLine> = nav
            .triggers
            .iter()
            .filter_map(|t| t.linedef().map(|li| (li, t)))
            .collect();
        let k = &self.knowledge;
        // Where she can set off an exit from, as things are.
        let exit_polys: BTreeSet<usize> = k
            .exits()
            .into_iter()
            .filter_map(|li| by_line.get(&li).filter(|t| t.info.lock.satisfied(keys)))
            .flat_map(|t| {
                t.activators
                    .intersection(&reach)
                    .copied()
                    .collect::<Vec<_>>()
            })
            .collect();
        let lap = crate::debug::lap("explorer.before_gather", lap);
        let glap = lap;
        out.extend(self.exits(&by_line, &reach, keys, pos));
        let glap = crate::debug::lap("g.exits", glap);
        out.extend(self.keys_seen(&view, &states, &reach, keys, pos));
        let glap = crate::debug::lap("g.keys_seen", glap);
        out.extend(self.ammo_for_tapping(obs, &reach));
        let glap = crate::debug::lap("g.ammo_for_tapping", glap);
        out.extend(self.untried(&by_line, &reach, keys, pos, (&view, &states, &states_all)));
        let glap = crate::debug::lap("g.untried", glap);
        let again = self.pressed_again(obs, &by_line, &reach, keys, pos);
        let glap = crate::debug::lap("g.pressed_again", glap);
        if self.suspecting != Suspecting::No {
            let panic = self.suspecting == Suspecting::EveryWall;
            out.extend(self.walls(obs, &reach, panic));
        } else {
            // A wall she strongly suspects, right by her, is tried there and then, as a player in front
            // of it would, not left for when there's nothing else.
            let near: Vec<Objective> = self
                .suspected(obs, &reach, false)
                .into_iter()
                .filter(|w| {
                    w.0 >= NEAR_SUSPECT && dist(w.2, pos) <= NEAR_WALL && self.standable(w.2, false)
                })
                .map(|(score, li, stand, mid)| {
                    super::selection::wall_objective(score, li, stand, mid, false)
                })
                .collect();
            out.extend(near);
            // The secrets left are what she's after, and the way out's known: the walls a search
            // for them would press (its score, as asked for by default), not only the strong
            // ones by her. In with the rest, not left for when there's nothing else: that's after
            // every look again, every line stepped over and every switch pressed once more.
            if self.searching_secrets() {
                let search = self.suspected_walls(obs, SEARCH_AS);
                for o in search {
                    if !out.iter().any(|w| w.kind == Kind::Wall && w.wall == o.wall) {
                        out.push(o);
                    }
                }
            }
        }
        let glap = crate::debug::lap("g.walls", glap);
        out.extend(self.teleporters(&reach, pos));
        let glap = crate::debug::lap("g.teleporters", glap);
        out.extend(self.frontier(&reach, pos));
        let glap = crate::debug::lap("g.frontier", glap);
        out.extend(self.recheck(&reach, pos));
        let glap = crate::debug::lap("g.recheck", glap);
        out.extend(self.unfinished(&reach, pos));
        let glap = crate::debug::lap("g.unfinished", glap);
        out.extend(self.unvisited(&reach, pos));
        let glap = crate::debug::lap("g.unvisited", glap);
        out.extend(self.healing(obs, &reach));
        let glap = crate::debug::lap("g.healing", glap);
        out.extend(self.spare_pickups(obs, &reach));
        let glap = crate::debug::lap("g.spare_pickups", glap);
        out.extend(self.get_off(obs, &reach, risky));
        let glap = crate::debug::lap("g.get_off", glap);
        out.extend(self.secrets_again(&reach, pos));
        let glap = crate::debug::lap("g.secrets_again", glap);
        out.extend(self.noises(&reach, pos, obs.tic));
        let glap = crate::debug::lap("g.noises", glap);
        let _ = glap;
        let lap = crate::debug::lap("explorer.gather", lap);
        out.extend(hooks.wanted(self, obs, &reach, pos));
        // Out of ammo, and the nearest thing she needs (health, ammo) a long way off: the walls
        // round her first, searched as hard as a search for secrets goes. What a level hides
        // is mostly supplies, and a wall by her is seconds where the walk is minutes (the
        // maintainer's, watching Doom II MAP31: out of ammo, round and round between a
        // stimpack and a clip a corridor of doors apart, past walls with things behind them).
        if self.suspecting == Suspecting::No && hooks.unarmed(obs) {
            let far = out
                .iter()
                .filter(|o| o.kind == Kind::Item && (o.needed || o.urgent))
                .map(|o| dist(o.goal, pos))
                .min_by(f64::total_cmp)
                .filter(|&d| d > SUPPLY_FAR);
            if let Some(d) = far {
                let within = (d / 2.0).min(SEARCH_NEAR);
                for mut o in self.suspected_walls(obs, SEARCH_HARD) {
                    if dist(o.goal, pos) <= within
                        && !out.iter().any(|w| w.kind == Kind::Wall && w.wall == o.wall)
                    {
                        o.needed = true;
                        out.push(o);
                    }
                }
            }
        }
        if !while_open.is_empty() {
            for o in out.iter_mut().filter(|o| {
                o.kind != Kind::Exit && nav.poly_at(o.goal).is_some_and(|p| while_open.contains(&p))
            }) {
                o.urgent = true;
                o.worth = Some((closes - obs.tic) as f64 * OPEN_SPEED);
                o.label = format!("{}{WHILE_OPEN}", o.label);
            }
        }
        // Not anywhere that would cut her off from most of what she can get to now, as far as she
        // knows (`cut_off.rs`).
        let now = Reach {
            here,
            keys,
            polys: &reach,
            states: &states,
            states_all: &states_all,
        };
        let back = self.way_back(&nav, &view, now, &by_line);
        let mut sift = self.sift(obs, risky, back, exit_polys);
        let lap = crate::debug::lap("explorer.back_and_sift", lap);
        // With nothing else left, the ways between rooms never walked over (walk-over lines are
        // thresholds), and last of all those onto floors that hurt.
        // (what's wanted only to take on to another level, set aside: what's to do now, and what
        // comes when there's nothing, as they'd be without it; given back after, `objectives`)
        let supplies = self.focus == super::Focus::Supplies;
        let (aside, out): (Vec<Objective>, Vec<Objective>) = out.into_iter().partition(|o| {
            o.stock_only && !(supplies && o.thing.is_none_or(|t| self.focus_things.contains(&t)))
        });
        self.stock_aside = aside;
        let first = self.usable(obs, hooks, &mut sift, out);
        crate::debug::lap("explorer.usable", lap);
        if !first.is_empty() {
            return first;
        }
        let t1 = self.thresholds(&reach, false);
        let x = self.usable(obs, hooks, &mut sift, t1);
        if !x.is_empty() {
            return x;
        }
        let t2 = self.thresholds(&reach, true);
        let x = self.usable(obs, hooks, &mut sift, t2);
        if !x.is_empty() {
            return x;
        }
        let x = self.usable(obs, hooks, &mut sift, self.seams(&reach, pos));
        if !x.is_empty() {
            return x;
        }
        let x = self.usable(obs, hooks, &mut sift, again);
        if !x.is_empty() {
            return x;
        }
        let lr = hooks.last_resort(self, obs, &reach, pos);
        self.usable(obs, hooks, &mut sift, lr)
    }

    /// The exits she can set off from where she can get to, with the keys she has: exit lines,
    /// and floors that have ended the level under her before. None while the exit is held back
    /// (`exits_offered`).
    fn exits(
        &self,
        by_line: &BTreeMap<usize, &TriggerLine>,
        reach: &BTreeSet<usize>,
        keys: Keys,
        pos: Point,
    ) -> Vec<Objective> {
        let nav = &self.nav;
        let k = &self.knowledge;
        let mut out: Vec<Objective> = Vec::new();
        if self.exits_offered {
            for li in k.exits() {
                // (not a line she's found she can't get over from this ground: `note_misses`)
                let barred = self.uncrossable.get(&li);
                if let Some(t) = by_line.get(&li).filter(|t| {
                    t.info.lock.satisfied(keys)
                        && t.activators
                            .iter()
                            .any(|p| reach.contains(p) && barred.is_none_or(|g| !g.contains(p)))
                }) {
                    let mut o = Objective::new(
                        Kind::Exit,
                        nav.activation_point(t, pos, reach),
                        format!("exit (line {li})"),
                    );
                    o.trigger = Some((*t).clone());
                    out.push(o);
                }
            }
            // A floor that has ended the level under her before: stood on again.
            for &sec in &k.exit_floors {
                if let Some(t) = nav
                    .triggers
                    .iter()
                    .find(|t| t.line == EXIT_FLOOR - sec as i64 && !t.activators.is_disjoint(reach))
                {
                    let mut o = Objective::new(
                        Kind::Exit,
                        nav.activation_point(t, pos, reach),
                        format!("exit (the floor of sector {sec})"),
                    );
                    o.trigger = Some(t.clone());
                    out.push(o);
                }
            }
        }
        out
    }

    /// The keys she has seen and doesn't hold, each from the nearest spot she can get to that
    /// touches it.
    fn keys_seen(
        &self,
        view: &View,
        states: &States,
        reach: &BTreeSet<usize>,
        keys: Keys,
        pos: Point,
    ) -> Vec<Objective> {
        let k = &self.knowledge;
        let mut out: Vec<Objective> = Vec::new();
        for th in k.known_things() {
            let Some(kk) = Key::from_thing(th.kind).filter(|kk| !keys.has(*kk)) else {
                continue;
            };
            let spots: Vec<Point> = view
                .pickup_spots(th.x, th.y, states)
                .into_iter()
                .filter(|(p, _)| reach.contains(p))
                .map(|(_, pt)| pt)
                .collect();
            if let Some(pt) =
                spots
                    .into_iter()
                    .reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a })
            {
                let mut o = Objective::new(Kind::Key, pt, format!("pick up {}", kk.name()));
                o.key = Some(kk);
                out.push(o);
            }
        }
        out
    }

    /// Out of rounds to tap a wall or a gun line with: the ammo she has seen, as a player tops
    /// up.
    fn ammo_for_tapping(&self, obs: &Observation, reach: &BTreeSet<usize>) -> Vec<Objective> {
        let nav = &self.nav;
        let k = &self.knowledge;
        let mut out: Vec<Objective> = Vec::new();
        let rounds = |slot: i32| crate::motor::follower::tap_rounds(obs, slot);
        if obs.ammo.is_some() && rounds(2).max(rounds(4)) < LOW_BULLETS && rounds(3) < LOW_SHELLS {
            let known = k.known_polys();
            for t in obs
                .things
                .iter()
                .flatten()
                .filter(|t| TAP_AMMO.contains(&t.name))
            {
                if !nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)) {
                    continue;
                }
                if let Some(spot) = self.touch_spot((t.x, t.y), t.z, reach) {
                    let mut o = Objective::new(
                        Kind::Item,
                        spot,
                        format!("pick up {} (for the gun)", t.name),
                    );
                    o.thing = Some(t.id);
                    out.push(o);
                }
            }
        }
        out
    }

    /// A door she opens again and again by pressing it, standing open as she last saw it, that
    /// she has learned another line opens to stay: not one to try. Pressed open, it shuts after
    /// its wait (EV_VerticalDoor: a new open-wait-close on a door already up) and stays shut
    /// behind her: TNT MAP32's line 824, its door opened for good by a switch.
    ///
    /// Only with the other line learned. Any such door standing open (a monster come through
    /// it, mostly) left untried moved 151 of the score's 1,088 runs, somewhat for the worse; and
    /// a door pressed once that opens and stays (31 to 34, 118) can't be shut by a press at all
    /// (E3M7's line 835, where the try is what takes her over a floor that hurts to it).
    pub(super) fn stands_open(&self, t: &TriggerLine) -> bool {
        let k = &self.knowledge;
        let open = |h: &crate::nav::Heights| h.ceiling - h.floor >= PLAYER_HEIGHT;
        let opened_to_stay = |s: usize| {
            k.learned.iter().any(|(&i, sectors)| {
                let other = &self.nav.triggers[i];
                sectors.contains(&s)
                    && other.line != t.line
                    && other.permanent()
                    && other.effect(s).is_some_and(|hs| hs.iter().any(open))
            })
        };
        t.info.category.is_door()
            && t.info.trigger == Trigger::DR
            && !t.sectors.is_empty()
            && t.sectors
                .iter()
                .all(|&s| k.seen_heights.get(&s).is_some_and(open) && opened_to_stay(s))
    }

    /// The switches and doors she can tell are there and hasn't tried, that she can get to and
    /// has the key for.
    ///
    /// Lines that work the same thing (`tried_key`) and join end to end are one of them: the
    /// facets of a round lift's face, a wide door drawn in pieces. One to try, pressed from
    /// the nearest place any of its lines can be pressed from; a line of it given up is passed
    /// over for one given up fewer times, so she goes round the lot, not back
    /// to the facet in the corner that can't be pressed (Doom II MAP14's line 537), and not
    /// to every facet in turn as so many things to do.
    fn untried(
        &self,
        by_line: &BTreeMap<usize, &TriggerLine>,
        reach: &BTreeSet<usize>,
        keys: Keys,
        pos: Point,
        (view, states, states_all): (&View, &States, &States),
    ) -> Vec<Objective> {
        let nav = &self.nav;
        let k = &self.knowledge;
        // (one of them to try, as a line by itself: from ground it's worked from as she
        // believes things stand now. The floor its face is seen from or a shot hits it from;
        // and, as the planner has it for a line she knows (`View::activators_now`), nothing
        // shut between that she can't open, room for her box where she'd stand, a far side
        // she can step onto. Held only to the first, a lift's face was "tried" from beside a
        // block 72 high that had to come down before she could stand anywhere but in line
        // with it: Doom E4M1, three presses along the face in seven runs of eight.)
        // As things could stand, that is, with what she knows how to bring about
        // (`states_all`): a door worked from a floor a line she's learned raises first is
        // one to try, and the route sees to the line. (Doom E3M4's line 509, behind 491.)
        let doors = view.local_doors(keys);
        let to_try = |li: usize| -> Option<Objective> {
            let t = by_line
                .get(&li)
                .filter(|t| t.info.lock.satisfied(keys))
                .map(|t| {
                    let mut t = (*t).clone();
                    let now = view.activators_now(&t, states_all, Some(&doors));
                    t.activators = view
                        .floors_now(&t, states)
                        .intersection(&now)
                        .copied()
                        .collect();
                    t
                })
                .filter(|t| !t.activators.is_disjoint(reach))?;
            let t = &t;
            let kind = if t.info.category.is_door() {
                Kind::Door
            } else {
                Kind::Switch
            };
            if kind == Kind::Door && self.stands_open(t) {
                return None;
            }
            let mut o = Objective::new(
                kind,
                nav.activation_point(t, pos, reach),
                format!(
                    "try {} (line {li})",
                    if kind == Kind::Door { "door" } else { "switch" }
                ),
            );
            o.trigger = Some((*t).clone());
            Some(o)
        };
        let mut found: Vec<(usize, Objective)> = k
            .interactables()
            .into_iter()
            .filter_map(|li| to_try(li).map(|o| (li, o)))
            .collect();
        // Which of them are one: the same thing worked, joined by their ends (through any
        // line of the map that works it too, seen or not: a facet she hasn't made out yet
        // doesn't make two of a face).
        let ends = |li: usize| {
            let (a, b) = nav.map.line_seg(li);
            [a, b]
        };
        let known = found.len();
        let mut group: Vec<usize> = (0..known).collect();
        for i in 0..known {
            if group[i] != i {
                continue;
            }
            let key = k.tried_key(found[i].0);
            let mut points: Vec<Point> = ends(found[i].0).to_vec();
            let mut alike: Vec<usize> = (0..nav.map.linedefs.len())
                .filter(|&li| {
                    li != found[i].0 && nav.map.linedefs[li].special != 0 && k.tried_key(li) == key
                })
                .collect();
            let mut joined: Vec<usize> = Vec::new();
            loop {
                let before = alike.len();
                alike.retain(|&li| {
                    let e = ends(li);
                    let join = e.iter().any(|p| points.contains(p));
                    if join {
                        points.extend(e);
                        joined.push(li);
                    }
                    !join
                });
                if alike.len() == before {
                    break;
                }
            }
            for j in i + 1..known {
                if group[j] == j && joined.contains(&found[j].0) {
                    group[j] = i;
                }
            }
            // ...and the lot is pressed at any of its lines, made out or not: one facet of a
            // lift's face seen, the face is seen, and the one she made out may be the one that
            // can't be pressed (MAP14's line 375, at the end of its arc by the wall: pressed
            // from before it nothing goes off, five times in eleven minutes, and the lift never
            // learned; the facet next to it works).
            for li in joined {
                if found.iter().all(|(l, _)| *l != li)
                    && let Some(o) = to_try(li)
                {
                    found.push((li, o));
                    group.push(i);
                }
            }
        }
        // Of each lot, the line given up the fewest times, the nearest of those: a press that
        // did nothing isn't made again before the lines beside it have had theirs. (Not "given
        // up lately": the half minute that lasts was over by the time she came back for the
        // lift, and the dead facet, nearest, was pressed again: eighteen times in one run.)
        let rank = |o: &Objective| {
            (
                self.drop_count.get(&o.ident()).copied().unwrap_or(0),
                dist(o.goal, pos),
            )
        };
        let mut best: BTreeMap<usize, Objective> = BTreeMap::new();
        for (g, (_, o)) in group.into_iter().zip(found) {
            match best.get(&g) {
                Some(b) if rank(b) <= rank(&o) => {}
                _ => {
                    best.insert(g, o);
                }
            }
        }
        best.into_values().collect()
    }

    /// Repeatable switches pressed a while ago whose effect she never saw are worth another go
    /// once there's nothing else, each by what it works (a door she can open again and again
    /// among them: pressed as it stood open, by a monster come through it, it only shut, and
    /// she's none the wiser what's behind it); and those whose effect she has put
    /// down to them on timing alone, unsure, are pressed again and watched, once a life.
    fn pressed_again(
        &self,
        obs: &Observation,
        by_line: &BTreeMap<usize, &TriggerLine>,
        reach: &BTreeSet<usize>,
        keys: Keys,
        pos: Point,
    ) -> Vec<Objective> {
        let nav = &self.nav;
        let k = &self.knowledge;
        let learned_lines: BTreeSet<i64> = k
            .learned
            .iter()
            .filter(|(_, s)| !s.is_empty())
            .map(|(&i, _)| nav.triggers[i].line)
            .collect();
        let mut last_by_key: BTreeMap<crate::knowledge::model::Mechanism, i64> = BTreeMap::new();
        for f in &k.fired {
            last_by_key.insert(k.tried_key(f.line), f.tic);
        }
        let mut last_press: Vec<(usize, i64)> = Vec::new();
        for f in &k.fired {
            let tic = last_by_key[&k.tried_key(f.line)];
            match last_press.iter_mut().find(|(l, _)| *l == f.line) {
                Some(e) => e.1 = tic,
                None => last_press.push((f.line, tic)),
            }
        }
        // And the other lines of what a press worked, ones she's mapped and never pressed
        // themselves: the same thing, to be had from where they are. Two pillars that go up
        // and down together, a switch on each: the one pressed with them down already did
        // nothing she saw, and up on the other, its switch in front of her the only way off,
        // it was "tried" (E1M5: stranded there, and again every go). Not where she learned
        // what the press does from any line of it: a door's other face is no news (MAP14: a
        // hundred and fifty doors over again, an hour of it).
        let pressed: BTreeSet<usize> = last_press.iter().map(|p| p.0).collect();
        // (a boss's death works floors with no line: -1)
        let learned_keys: BTreeSet<crate::knowledge::model::Mechanism> = learned_lines
            .iter()
            .filter(|&&li| li >= 0)
            .map(|&li| k.tried_key(li as usize))
            .collect();
        let twins: Vec<(usize, i64)> = k
            .mapped
            .iter()
            .copied()
            .filter(|li| !pressed.contains(li) && by_line.contains_key(li))
            .filter(|&li| !learned_keys.contains(&k.tried_key(li)))
            .filter_map(|li| last_by_key.get(&k.tried_key(li)).map(|&tic| (li, tic)))
            .collect();
        let doubted: BTreeSet<crate::knowledge::model::Mechanism> =
            k.doubts.keys().map(|(m, _)| *m).collect();
        let mut again = Vec::new();
        for &(li, tic) in last_press.iter().chain(&twins) {
            let twin = !pressed.contains(&li);
            let Some(t) = by_line.get(&li) else { continue };
            if !k.mapped.contains(&li)
                || !matches!(t.info.trigger, Trigger::SR | Trigger::GR | Trigger::DR)
            {
                continue;
            }
            let unsure = doubted.contains(&k.tried_key(li))
                && !k.rechecked.contains(&(k.tried_key(li), k.lives));
            // (down to the last resort, the ones she knows the working of as well, and no wait:
            // a switch at hand, not a door and not a walk away. Shut in with one, she presses
            // it; she doesn't go round the level pressing what she knows)
            // And only if something has moved since she last pressed it, and has stopped:
            // pressed again into the same stillness it does the same nothing, and into a
            // floor on its way the game ignores it. Something the press didn't itself set
            // going: a lift's switch moves its lift every time, and by its own doing was
            // news for ever (E4M1: stood at one, pressed 337 times in half an hour).
            let own: BTreeSet<usize> = t.effects.iter().map(|(s, _)| *s).collect();
            let at_hand = self.known_again
                && !t.info.category.is_door()
                && dist(nav.activation_point(t, pos, reach), pos) <= SWITCH_AT_HAND
                && k.last_moved_but(&own, tic)
                    .is_some_and(|m| m > tic && obs.tic - m >= MOVED_STILL);
            if learned_lines.contains(&(li as i64)) && !unsure && !at_hand {
                continue;
            }
            // (a twin isn't waited for: it was never pressed from there)
            if (obs.tic - tic < REPRESS_AFTER && !unsure && !twin && !at_hand)
                || t.activators.is_disjoint(reach)
                || !t.info.lock.satisfied(keys)
            {
                continue;
            }
            // Not again knowing no more than when she last set out to press it: what was missed then
            // would be missed again.
            if !unsure
                && !at_hand
                && self
                    .pressed_knowing
                    .get(&li)
                    .is_some_and(|known| *known == k.known_summary())
            {
                continue;
            }
            let mut o = Objective::new(
                Kind::Switch,
                nav.activation_point(t, pos, reach),
                format!(
                    "try {} again (line {li}{})",
                    if t.info.category.is_door() {
                        "door"
                    } else {
                        "switch"
                    },
                    if unsure { ", unsure what it does" } else { "" }
                ),
            );
            o.trigger = Some((*t).clone());
            again.push(o);
        }
        again
    }

    /// Known polys she can get to next to polys she hasn't seen into: per sector those are in,
    /// at the reachable poly nearest her (done once she's seen into any of them).
    pub(super) fn frontier(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let mesh = &self.nav.mesh;
        let known = self.knowledge.known_polys();
        let mut best: BTreeMap<usize, (f64, Point)> = BTreeMap::new();
        let mut order: Vec<usize> = Vec::new();
        let mut unseen: BTreeMap<usize, BTreeSet<i64>> = BTreeMap::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            for &q in &poly.portals {
                let portal = &mesh.portals[q];
                let other = portal.other(pi);
                if known.contains(&other) {
                    continue;
                }
                let os = mesh.poly(other).sector;
                unseen.entry(os).or_default().insert(other as i64);
                // Look from the way in, just this side of it: from the poly's middle it can be round a
                // corner.
                let mid = (
                    (portal.p1.0 + portal.p2.0) / 2.0,
                    (portal.p1.1 + portal.p2.1) / 2.0,
                );
                let back = dist(mid, poly.center);
                let k = if back > 0.0 {
                    1.0f64.min(FRONTIER_BACK / back)
                } else {
                    0.0
                };
                let mut spot = (
                    mid.0 + (poly.center.0 - mid.0) * k,
                    mid.1 + (poly.center.1 - mid.1) * k,
                );
                if mesh.poly_at(spot) != Some(pi) {
                    spot = poly.center;
                }
                let d = dist(spot, pos);
                match best.get(&os) {
                    Some((bd, _)) if d >= *bd => {}
                    Some(_) => {
                        best.insert(os, (d, spot));
                    }
                    None => {
                        order.push(os);
                        best.insert(os, (d, spot));
                    }
                }
            }
        }
        order
            .into_iter()
            .map(|s| {
                let mut o = Objective::new(
                    Kind::Frontier,
                    self.stand_spot(best[&s].1, reach),
                    format!("look into sector {s}"),
                );
                o.reveals = unseen[&s].clone();
                o
            })
            .collect()
    }

    /// Reachable polys she hasn't stood in across a line from ones she has: just over the line
    /// (all a walk-over line needs; a strip's middle may have no room for the box).
    fn thresholds(&self, reach: &BTreeSet<usize>, hurting: bool) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let mut out = Vec::new();
        for &pi in reach.difference(&k.visited) {
            let poly = mesh.poly(pi);
            if k.damaging(poly.sector) != hurting {
                continue;
            }
            for &q in &poly.portals {
                let portal = &mesh.portals[q];
                let Some(li) = portal
                    .linedef
                    .filter(|_| k.visited.contains(&portal.other(pi)))
                else {
                    continue;
                };
                let mid = (
                    (portal.p1.0 + portal.p2.0) / 2.0,
                    (portal.p1.1 + portal.p2.1) / 2.0,
                );
                let d = dist(mid, poly.center);
                if d < 1e-6 {
                    continue;
                }
                let spot = (
                    mid.0 + (poly.center.0 - mid.0) / d * THRESHOLD_IN,
                    mid.1 + (poly.center.1 - mid.1) / d * THRESHOLD_IN,
                );
                // (standable as she last saw things, or as a line she knows leaves the place:
                // a pedestal under a low roof that a switch lowers for three seconds is in her
                // reach by the switch, and has room on it only then. TNT MAP05's, whose edge is
                // the line that brings the blue card's lift down)
                if mesh.poly_at(spot) != Some(pi)
                    || !(self.standable(spot, false) || self.standable_moved(spot, poly.sector))
                {
                    continue; // (a sliver by a wall or shut door the box can't stand in)
                }
                let mut o = Objective::new(Kind::Visit, spot, format!("step over line {li}"));
                o.reveals = BTreeSet::from([pi as i64]);
                out.push(o);
                break;
            }
        }
        out
    }

    /// Lines she has stood on both sides of and never walked over, between sectors with nothing
    /// to tell them apart as she first saw them (floor, ceiling, flats, light: a split only a
    /// trigger or a mover needs; one of them moved since, a ledge now, still counts): over each,
    /// to the side away from her.
    fn seams(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let sectors = &self.nav.map.sectors;
        let alike = |a: usize, b: usize| {
            let (x, y) = (&sectors[a], &sectors[b]);
            let (hx, hy) = (k.first_heights.get(&a), k.first_heights.get(&b));
            hx.is_some()
                && hx == hy
                && x.floor_tex == y.floor_tex
                && x.ceiling_tex == y.ceiling_tex
                && x.light == y.light
        };
        let mut done: BTreeSet<usize> = BTreeSet::new();
        let mut out = Vec::new();
        for &pi in reach.intersection(&k.visited) {
            for &q in &mesh.poly(pi).portals {
                let portal = &mesh.portals[q];
                let other = portal.other(pi);
                let Some(li) = portal
                    .linedef
                    .filter(|li| !k.crossed.contains(li) && !done.contains(li))
                else {
                    continue;
                };
                let (a, b) = (mesh.poly(pi).sector, mesh.poly(other).sector);
                if a == b || !k.visited.contains(&other) || !reach.contains(&other) || !alike(a, b)
                {
                    continue;
                }
                let mid = (
                    (portal.p1.0 + portal.p2.0) / 2.0,
                    (portal.p1.1 + portal.p2.1) / 2.0,
                );
                // The far side from her: from where she is, over it.
                let far = [pi, other]
                    .into_iter()
                    .max_by(|&x, &y| {
                        dist(mesh.poly(x).center, pos).total_cmp(&dist(mesh.poly(y).center, pos))
                    })
                    .unwrap_or(other);
                let d = dist(mid, mesh.poly(far).center);
                if d < 1e-6 {
                    continue;
                }
                // In far enough that getting there is going over it: not done within arrival of it.
                let c = mesh.poly(far).center;
                let at = |depth: f64| {
                    (
                        mid.0 + (c.0 - mid.0) / d * depth,
                        mid.1 + (c.1 - mid.1) / d * depth,
                    )
                };
                let Some(spot) = [SEAM_IN, THRESHOLD_IN]
                    .map(at)
                    .into_iter()
                    .find(|&p| mesh.poly_at(p) == Some(far) && self.standable(p, false))
                else {
                    continue;
                };
                done.insert(li);
                let mut o = Objective::new(Kind::Visit, spot, format!("walk over line {li}"));
                o.reveals = BTreeSet::from([far as i64]);
                // The line is marked tried once the trip is over, gone over or not: a spot over it can be
                // got to round it, and from the far side the next one's spot is back again, for ever.
                o.wall = Some(li);
                out.push(o);
            }
        }
        out
    }

    /// Teleporters she has seen and not been through: walk over one (from the side that sends
    /// you) to find out where it goes. One per pad, over its side nearest her.
    pub(super) fn teleporters(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, nav) = (&self.knowledge, &self.nav);
        let mut best: Vec<(BTreeSet<usize>, bool, Objective)> = Vec::new();
        let now = self.states();
        let heights = self.roomiest();
        for li in k.teleporters() {
            if k.teleports.contains_key(&li) {
                continue;
            }
            let src: BTreeSet<usize> = nav
                .tele_from
                .iter()
                .enumerate()
                .filter(|(_, es)| {
                    es.iter()
                        .any(|e| matches!(e, TeleEdge::Teleport(i) if nav.teleports[*i].line == li))
                })
                .map(|(p, _)| p)
                .filter(|p| reach.contains(p))
                .collect();
            if src.is_empty() {
                continue;
            }
            let (front, back) = nav.map.sides(li);
            if let (Some(f), Some(b)) = (front, back)
                && !now[f]
                    .iter()
                    .any(|&a| now[b].iter().any(|&c| can_cross(a, c)))
            {
                continue; // (the pad isn't down yet, as far as she's seen)
            }
            let (a, b) = nav.map.line_seg(li);
            let length = dist(a, b).max(1.0);
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            // the front side (the one that teleports), a little back from the line
            let mut spot = (
                mid.0 + (b.1 - a.1) / length * 24.0,
                mid.1 - (b.0 - a.0) / length * 24.0,
            );
            if !nav.poly_at(spot).is_some_and(|p| reach.contains(&p)) {
                spot = src
                    .iter()
                    .map(|&p| nav.poly_center(p))
                    .reduce(|x, y| if dist(y, mid) < dist(x, mid) { y } else { x })
                    .unwrap_or(spot);
            }
            // Somewhere she fits first: not up against something standing in front of the pad when
            // another side of it is clear.
            let fits = self.stands_at(spot, &heights);
            let Some(info) = classify_line(nav.map.linedefs[li].special) else {
                continue;
            };
            let t = TriggerLine {
                id: usize::MAX,
                line: li as i64,
                info,
                sectors: Vec::new(),
                effects: Vec::new(),
                activators: src,
                shift_from: None,
                relevant: None,
                keep: None,
            };
            let pad = k.teleport_pad(li);
            let mut o = Objective::new(
                Kind::Teleporter,
                spot,
                format!("try teleporter (line {li})"),
            );
            o.trigger = Some(t);
            o.pad = pad.clone();
            match best.iter_mut().find(|(p, _, _)| *p == pad) {
                Some((_, bf, b)) if (fits, -dist(spot, pos)) > (*bf, -dist(b.goal, pos)) => {
                    (*bf, *b) = (fits, o)
                }
                Some(_) => {}
                None => best.push((pad, fits, o)),
            }
        }
        best.into_iter().map(|(_, _, o)| o).collect()
    }

    /// Sounds she heard but couldn't see the source of, lately: go to the reachable spot nearest
    /// where it seemed to come from (if that's nearer than here) and look.
    pub(super) fn noises(&self, reach: &BTreeSet<usize>, pos: Point, tic: i64) -> Vec<Objective> {
        let mesh = &self.nav.mesh;
        let mut out = Vec::new();
        for h in &self.knowledge.heard {
            if tic - h.tic > NOISE_TICS || !self.worth_a_look(h) || self.noise_seen(h) {
                continue;
            }
            let urgent = self.urgent(h, tic);
            let Some(best) = reach.iter().copied().reduce(|a, b| {
                if dist(mesh.poly(b).center, h.origin) < dist(mesh.poly(a).center, h.origin) {
                    b
                } else {
                    a
                }
            }) else {
                continue;
            };
            // Where the box can stand.
            let spot = self.stand_spot(mesh.poly(best).center, reach);
            if dist(spot, h.origin) > dist(pos, h.origin) - HEAR_NEARER {
                continue;
            }
            let mut o = Objective::new(
                Kind::Noise,
                spot,
                format!(
                    "check a noise {} (~{:.0}){}",
                    compass(h.bearing),
                    h.distance,
                    if urgent { " right away" } else { "" }
                ),
            );
            o.reveals = BTreeSet::from([h.tic]);
            o.heard = Some(*h);
            o.urgent = urgent;
            out.push(o);
        }
        out
    }

    /// A sound heard just after she set off a line she doesn't know the effect of: go and look
    /// now, while it's still moving.
    pub(super) fn urgent(&self, h: &Heard, tic: i64) -> bool {
        let k = &self.knowledge;
        if tic - h.tic >= URGENT_TICS || h.seen {
            return false;
        }
        let n = k.fired.len();
        let lines: BTreeSet<i64> = k.fired[n.saturating_sub(8)..]
            .iter()
            .filter(|f| (0..=CAUSE_TICS).contains(&(h.tic - f.tic)))
            .map(|f| f.line as i64)
            .collect();
        self.nav
            .triggers
            .iter()
            .any(|t| lines.contains(&t.line) && k.learned.get(&t.id).is_none_or(BTreeSet::is_empty))
    }

    /// Has where a sound seemed to come from been in view since?
    pub(super) fn noise_seen(&self, h: &Heard) -> bool {
        self.nav.poly_at(h.origin).is_some_and(|p| {
            self.knowledge
                .seen_tic
                .get(&self.nav.sector_of_poly(p))
                .copied()
                .unwrap_or(-1)
                > h.tic
        })
    }

    /// Is the box (with the checks' slack) at p over a line that teleports her or ends the level?
    fn by_trip_line(&self, p: Point) -> bool {
        let (m, mesh) = (&self.nav.map, &self.nav.mesh);
        let r = crate::geom::PLAYER_RADIUS + 0.25;
        crate::geom::cells_around(p, p, r + 1.0).any(|c| {
            mesh.line_grid.get(&c).into_iter().flatten().any(|&li| {
                let sp = m.linedefs[li].special;
                let trips = crate::navigator::TELEPORTERS_FOR_PLAYERS.contains(&sp)
                    || crate::specials::classify_line(sp)
                        .is_some_and(|i| i.category.is_exit() && i.trigger.walked());
                trips && crate::geom::swept_box_hits(p, p, r, m.line_seg(li))
            })
        })
    }

    /// Parts of known rooms she can get to but hasn't walked (walking round finds the walk-over
    /// lines nobody can see): per sector, the reachable poly furthest from anywhere she's stood,
    /// if far enough to be worth it; and every room stepped into once.
    fn unvisited(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let stood: Vec<Point> = k.visited.iter().map(|&p| mesh.poly(p).center).collect();
        let entered: BTreeSet<usize> = k.visited.iter().map(|&p| mesh.poly(p).sector).collect();
        let mut best: Vec<(usize, f64, usize)> = Vec::new();
        let mut nearest: Vec<(usize, f64, usize)> = Vec::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            if k.visited.contains(&pi) || k.damaging(poly.sector) {
                continue; // (not wading through nukage just to look)
            }
            if !self.standable(poly.center, false) || self.by_trip_line(poly.center) {
                // Not a strip too narrow for the box (by a shut door, beside a teleport pad): no way to it
                // but squeezed past the edge, and a whole search spent finding that out.
                continue;
            }
            if !entered.contains(&poly.sector) {
                let d = dist(poly.center, pos);
                match nearest.iter_mut().find(|e| e.0 == poly.sector) {
                    Some(e) if d < e.1 => *e = (poly.sector, d, pi),
                    Some(_) => {}
                    None => nearest.push((poly.sector, d, pi)),
                }
            }
            let gap = stood
                .iter()
                .map(|&c| dist(poly.center, c))
                .fold(f64::INFINITY, f64::min);
            if gap < COVER_GAP {
                continue;
            }
            match best.iter_mut().find(|e| e.0 == poly.sector) {
                Some(e) if gap > e.1 => *e = (poly.sector, gap, pi),
                Some(_) => {}
                None => best.push((poly.sector, gap, pi)),
            }
        }
        for (s, _, pi) in nearest {
            if !best.iter().any(|e| e.0 == s) {
                best.push((s, 0.0, pi));
            }
        }
        best.into_iter()
            .map(|(s, _, pi)| {
                let mut o = Objective::new(
                    Kind::Visit,
                    mesh.poly(pi).center,
                    format!("walk across sector {s}"),
                );
                o.reveals = BTreeSet::from([pi as i64]);
                o
            })
            .collect()
    }

    /// The sectors crushers move (as their lines' specials say, the ones she's seen).
    fn crusher_sectors(&self) -> BTreeSet<usize> {
        self.nav
            .triggers
            .iter()
            .filter(|t| {
                t.info.category == crate::specials::Category::Crusher
                    && !t.info.description.contains("stop")
            })
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .collect()
    }

    /// Is she down on a floor she takes to hurt?
    pub(super) fn on_hurting_floor(&self, obs: &Observation) -> bool {
        let Some(p) = self.nav.poly_at(obs.pos()) else {
            return false;
        };
        let s = self.nav.sector_of_poly(p);
        self.knowledge.damaging(s)
            && obs
                .z
                .is_some_and(|z| (z - obs.heights[s].floor).abs() < 1.0)
            && !self.knowledge.protected(obs.tic)
    }

    /// (mortal runs) Down on a floor that hurts: off it, to the nearest floor that doesn't that
    /// she can get to. Waded in on purpose for a risky objective there, she looks round where
    /// she waded to before wading back: getting off is then only one of the risky options, by
    /// how far, while what she has left after paying for the way back (as much again as it has
    /// cost her since) is over the margin.
    fn get_off(&self, obs: &Observation, reach: &BTreeSet<usize>, risky: bool) -> Vec<Objective> {
        if !self.knowledge.mortal || !self.on_hurting_floor(obs) {
            return Vec::new();
        }
        let health = obs.health.unwrap_or(100.0);
        let meant = self.waded_dead_end
            || self
                .waded
                .is_some_and(|h| health - (h - health) > super::routing::HEALTH_MARGIN);
        if meant && !risky {
            return Vec::new();
        }
        let mesh = &self.nav.mesh;
        let pos = obs.pos();
        let best = reach
            .iter()
            .copied()
            .filter(|&p| {
                !self.knowledge.damaging(mesh.poly(p).sector)
                    && self.standable(mesh.poly(p).center, false)
            })
            .min_by(|&a, &b| {
                dist(mesh.poly(a).center, pos).total_cmp(&dist(mesh.poly(b).center, pos))
            });
        best.map(|p| {
            let mut o = Objective::new(
                Kind::Visit,
                mesh.poly(p).center,
                "get off the floor that hurts",
            );
            o.reveals = BTreeSet::from([p as i64]);
            o.urgent = !meant;
            o
        })
        .into_iter()
        .collect()
    }

    /// (mortal runs) Health she's seen, to go for when she's low or a way cost more than she had
    /// to spare; and a radiation suit, when one did (floors that hurt are all that costs).
    pub(super) fn healing(&self, obs: &Observation, reach: &BTreeSet<usize>) -> Vec<Objective> {
        if !self.knowledge.mortal {
            return Vec::new();
        }
        let health = obs.health.unwrap_or(100.0);
        let short = self
            .short_of_health
            .is_some_and(|t| obs.tic - t < SHORT_TICS);
        let known = self.knowledge.known_polys();
        let mut out = Vec::new();
        for t in obs.things.iter().flatten() {
            // What heals her past 100 counts as an item, had anyway: these only below it.
            let heals = matches!(t.name, "Stimpack" | "Medikit")
                && health < 100.0
                && (short || health < HEAL_BELOW);
            let suit = t.name == "RadSuit"
                && (short || self.on_hurting_floor(obs))
                && !self.knowledge.protected(obs.tic);
            if !heals && !suit {
                continue;
            }
            if !self
                .nav
                .poly_at((t.x, t.y))
                .is_some_and(|p| known.contains(&p))
            {
                continue;
            }
            if let Some(spot) = self.touch_spot((t.x, t.y), t.z, reach) {
                let why = if suit {
                    "for the floors that hurt"
                } else {
                    "to heal"
                };
                let mut o = Objective::new(Kind::Item, spot, format!("pick up {} ({why})", t.name));
                o.thing = Some(t.id);
                o.urgent = health < HEAL_BELOW;
                out.push(o);
            }
        }
        out
    }

    /// The pickups whoever directs her wants had when safe (`spares`), seen and to be got to.
    fn spare_pickups(&self, obs: &Observation, reach: &BTreeSet<usize>) -> Vec<Objective> {
        if self.spares.is_empty() {
            return Vec::new();
        }
        let known = self.knowledge.known_polys();
        let mut out = Vec::new();
        for t in obs
            .things
            .iter()
            .flatten()
            .filter(|t| self.spares.contains(&t.id))
        {
            if !self
                .nav
                .poly_at((t.x, t.y))
                .is_some_and(|p| known.contains(&p))
            {
                continue;
            }
            if let Some(spot) = self.touch_spot((t.x, t.y), t.z, reach) {
                let mut o = Objective::new(Kind::Item, spot, format!("pick up {}", t.name));
                o.thing = Some(t.id);
                out.push(o);
            }
        }
        out
    }

    /// How far she'd walk to a point (`travel`'s, a function of the goal): the shortest way through
    /// the polys of her reach from where she stands, between them at the portals' middles, no step
    /// up more than she can climb (at the heights she knows: a lift or door at any it can be). A
    /// point that's not in it (a teleporter's way, a jump) counts its straight line twice over.
    pub(super) fn travel(&self, obs: &Observation) -> impl Fn(Point) -> f64 + use<> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let mesh = Arc::clone(&self.nav.mesh);
        let start_pt = self.standing_point(obs);
        let states = self.states();
        let floors = |p: usize| states[mesh.poly(p).sector].iter().map(|h| h.floor);
        let mut best: BTreeMap<usize, (f64, Point)> = BTreeMap::new();
        if let Some(start) = mesh.poly_at(start_pt) {
            let mut heap = BinaryHeap::from([(Reverse(0u64), start)]);
            best.insert(start, (0.0, start_pt));
            while let Some((Reverse(bits), p)) = heap.pop() {
                let (d, at) = best[&p];
                if f64::from_bits(bits) > d {
                    continue;
                }
                let high = floors(p).fold(f64::NEG_INFINITY, f64::max);
                for &pi in &mesh.poly(p).portals {
                    let portal = &mesh.portals[pi];
                    let q = portal.other(p);
                    if !self.reach.contains(&q)
                        || floors(q).fold(f64::INFINITY, f64::min) - high > MAX_STEP
                    {
                        continue;
                    }
                    let mid = (
                        (portal.p1.0 + portal.p2.0) / 2.0,
                        (portal.p1.1 + portal.p2.1) / 2.0,
                    );
                    let nd = d + dist(at, mid);
                    if best.get(&q).is_none_or(|&(od, _)| nd < od) {
                        best.insert(q, (nd, mid));
                        heap.push((Reverse(nd.to_bits()), q));
                    }
                }
            }
        }
        move |goal: Point| match mesh.poly_at(goal).and_then(|p| best.get(&p)) {
            Some(&(d, at)) => d + dist(at, goal),
            None => TRAVEL_UNKNOWN * dist(start_pt, goal),
        }
    }

    /// The tally went up with her in sector `s`: a secret there.
    pub fn found_secret(&mut self, s: usize, tic: i64) {
        if self.secret_sectors.insert(s) {
            self.log.push(format!("tic {tic}: a secret in sector {s}"));
        }
        self.secrets_had.insert(s);
    }

    /// The secrets run, the level again: back into the sectors she found secrets in before.
    /// The tally starts again from 0, and knowing the place, nothing else sends her back there.
    fn secrets_again(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        if !self.secrets_mode {
            return Vec::new();
        }
        let mesh = &self.nav.mesh;
        let mut out = Vec::new();
        for &s in self.secret_sectors.difference(&self.secrets_had) {
            let nearest = reach
                .iter()
                .copied()
                .filter(|&p| mesh.poly(p).sector == s && self.standable(mesh.poly(p).center, false))
                .min_by(|&a, &b| {
                    dist(mesh.poly(a).center, pos).total_cmp(&dist(mesh.poly(b).center, pos))
                });
            if let Some(p) = nearest {
                let mut o = Objective::new(
                    Kind::Secret,
                    mesh.poly(p).center,
                    format!("back into secret sector {s}"),
                );
                o.reveals = BTreeSet::from([s as i64]);
                out.push(o);
            }
        }
        out
    }

    /// On instinct for whoever directs her, the secrets left what they're after and a way out
    /// known (till then the way on is what walls are pressed for, in their turn).
    fn searching_secrets(&self) -> bool {
        self.secret_search
            && !self.directed
            && self.resorts.is_some()
            && !self.secrets_done
            && !self.knowledge.exits().is_empty()
    }

    /// Since a press whose effect wasn't seen: the known sectors round the reachable area not
    /// seen since, one per sector, at the reachable poly next to it nearest her.
    fn recheck(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let Some(since) = k.unexplained_since() else {
            return Vec::new();
        };
        let reach_sectors: BTreeSet<usize> = reach.iter().map(|&p| mesh.poly(p).sector).collect();
        let mut best: Vec<(usize, f64, Point)> = Vec::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            for &q in &poly.portals {
                let s = mesh.poly(mesh.portals[q].other(pi)).sector;
                if reach_sectors.contains(&s)
                    || !k.sectors.contains(&s)
                    || k.seen_tic.get(&s).copied().unwrap_or(-1) > since
                {
                    continue;
                }
                let d = dist(poly.center, pos);
                match best.iter_mut().find(|e| e.0 == s) {
                    Some(e) if d < e.1 => *e = (s, d, poly.center),
                    Some(_) => {}
                    None => best.push((s, d, poly.center)),
                }
            }
        }
        best.into_iter()
            .map(|(s, _, c)| {
                let mut o = Objective::new(
                    Kind::Recheck,
                    self.stand_spot(c, reach),
                    format!("look at sector {s} again"),
                );
                o.reveals = BTreeSet::from([s as i64]);
                o
            })
            .collect()
    }

    /// The objectives open, the explorer's way round: the exit first if she can get there, then
    /// the locks her keys open, then by distance as the crow flies (weighted by kind).
    pub fn ranked(
        &mut self,
        obs: &Observation,
        risky: bool,
        hooks: &mut dyn Hooks,
    ) -> Vec<Objective> {
        let mut options = self.objectives(obs, risky, hooks);
        let mesh = Arc::clone(&self.nav.mesh);
        if !self.knowledge.traps.is_empty() {
            // Not back into a trap she's been stuck in before, for a key lying there or anything
            // else; but in one already, it's all there is. Nor is it a trap still with a key she
            // didn't hold then: the door it opens may be the way out. (Plutonia MAP22: the red
            // card's hall, shut till the yellow card's switch.)
            let here = mesh.poly_at(obs.pos());
            let keys = obs.keys.unwrap_or_default();
            let traps: Vec<&BTreeSet<usize>> = self
                .knowledge
                .traps
                .iter()
                .filter(|(t, held)| here.is_none_or(|h| !t.contains(&h)) && keys.0 & !held.0 == 0)
                .map(|(t, _)| t)
                .collect();
            options.retain(|o| {
                !traps
                    .iter()
                    .any(|trap| mesh.poly_at(o.goal).is_some_and(|p| trap.contains(&p)))
            });
        }
        if !self.exits_offered {
            // Nor anywhere she'd slide over one getting there.
            let segs = self.pads.1.clone();
            options.retain(|o| {
                segs.iter()
                    .all(|&(a, b)| segment_distance(a, b, o.goal, o.goal) >= EXIT_MARGIN)
            });
        }
        // Near as she'd walk it, not as the crow flies: a goal close in a straight line can be many
        // times that along the way.
        let travel = self.travel(obs);
        let mut weighed: Vec<(f64, Objective)> = options
            .into_iter()
            .map(|o| {
                (
                    travel(o.goal) * o.kind.weight() * hooks.weigh(obs, &o) * o.nearer(),
                    o,
                )
            })
            .collect();
        weighed.sort_by(|a, b| {
            (a.1.tier(), a.0)
                .partial_cmp(&(b.1.tier(), b.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // On instinct for whoever directs her (`resorts` given: never playing alone), new ground
        // first: going back over what she's done (a walk across floor she's seen, walls pressed
        // for secrets, another look, a hunt, a spare pickup more than a short walk off) waits till
        // there's nothing new left. On a stream that's her wandering finished rooms; alone, the
        // run's own order stands.
        if !self.directed
            && self.resorts.is_some()
            && self.secrets_done
            && !self.knowledge.exits().is_empty()
        {
            // (all the secrets found, and a way out known: no wall's worth pressing for one.
            // With no way out known a wall may be the way on, secret or not: E2M1's exit is
            // behind one, and with its four secrets found first she never pressed it, and
            // stood with nothing left to do)
            weighed.retain(|(_, o)| o.kind != Kind::Wall);
        }
        if !self.directed && self.resorts.is_some() && self.going_back != GoingBack::AsItComes {
            let back_over = |o: &Objective| {
                !o.urgent
                    && !o.needed
                    && match o.kind {
                        Kind::Visit | Kind::Wall | Kind::Recheck | Kind::Hunt => true,
                        Kind::Item => travel(o.goal) > INSTINCT_PICKUP_NEAR,
                        _ => false,
                    }
            };
            if self.going_back == GoingBack::Never || self.new_ground_only {
                weighed.retain(|(_, o)| !back_over(o));
            } else if weighed
                .iter()
                .any(|(_, o)| o.kind != Kind::Exit && !back_over(o))
            {
                // (stable: each part in its own order)
                weighed.sort_by_key(|(_, o)| back_over(o));
            }
        }
        // (the secrets left what she's after: new ground, things to pick up and to kill first,
        // as they come; then the walls a search would press, the likeliest before the rest,
        // each lot nearest first; and only then a walk over floor she's seen or another look)
        if self.searching_secrets() {
            let likely = crate::knowledge::model::THOROUGHNESS[0].0;
            weighed.sort_by_key(|(_, o)| match o.kind {
                _ if o.urgent || o.needed => 0,
                Kind::Wall if o.suspicion >= likely => 1,
                Kind::Wall => 2,
                Kind::Visit | Kind::Recheck => 3,
                _ => 0,
            });
        }
        // (a goal of whoever directs her: what it's after first, on instinct for them)
        if !self.directed && self.resorts.is_some() && self.focus != Focus::None {
            let focus = self.focus;
            let things = &self.focus_things;
            weighed.sort_by_key(|(_, o)| !focus.wants(o, things));
        }
        if self
            .debug_tics
            .is_some_and(|(a, b)| (a..=b).contains(&obs.tic))
        {
            for (d, o) in &weighed {
                self.log.push(format!(
                    "  (ranked) '{}' weighed {d:.0}, priority {}, needed {}, urgent {}",
                    o.label,
                    o.priority(),
                    o.needed,
                    o.urgent
                ));
            }
        }
        weighed.into_iter().map(|(_, o)| o).collect()
    }
}

/// on instinct for whoever directs her, a spare pickup this far off along the way or less is
/// taken with the new ground; further, it waits till there's no new ground left
const INSTINCT_PICKUP_NEAR: f64 = 512.0;

/// a point her reach gets to by no walk the travel field knows (a teleporter, a jump): counted
/// this many times its straight line
const TRAVEL_UNKNOWN: f64 = 2.0;
