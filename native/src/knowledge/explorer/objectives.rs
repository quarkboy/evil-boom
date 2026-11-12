//! Listing and ranking objectives in the currently reachable world.

use super::cut_off::Reach;
use super::{compass, Explorer, GoingBack, Hooks, Kind, Objective, Suspecting, WHILE_OPEN};
use crate::geom::{dist, segment_distance, Point};
use crate::knowledge::model::Heard;
use crate::motor::Observation;
use crate::nav::can_cross;
use crate::navigator::{States, TeleEdge, TriggerLine, View, EXIT_FLOOR, MAX_STEP};
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

/// As she passes it, that is: a hall's width off, not only at her elbow.
const NEAR_WALL: f64 = 448.0;

/// a repeatable switch whose effect wasn't seen: another go after this
const REPRESS_AFTER: i64 = 60 * 35;

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

    /// What she could go and do now.
    pub fn objectives(
        &mut self,
        obs: &Observation,
        risky: bool,
        hooks: &mut dyn Hooks,
    ) -> Vec<Objective> {
        let t0 = Instant::now();
        let mut out = self.list_objectives(obs, risky, hooks);
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
        if crate::debug::on().reach == Some(obs.tic) {
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
        out.extend(self.exits(&by_line, &reach, keys, pos));
        out.extend(self.keys_seen(&view, &states, &reach, keys, pos));
        out.extend(self.ammo_for_tapping(obs, &reach));
        out.extend(self.untried(&by_line, &reach, keys, pos));
        let again = self.pressed_again(obs, &by_line, &reach, keys, pos);
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
        }
        out.extend(self.teleporters(&reach, pos));
        out.extend(self.frontier(&reach, pos));
        out.extend(self.recheck(&reach, pos));
        out.extend(self.unfinished(&reach, pos));
        out.extend(self.unvisited(&reach, pos));
        out.extend(self.healing(obs, &reach));
        out.extend(self.spare_pickups(obs, &reach));
        out.extend(self.get_off(obs, &reach, risky));
        out.extend(self.secrets_again(&reach, pos));
        out.extend(self.noises(&reach, pos, obs.tic));
        out.extend(hooks.wanted(self, obs, &reach, pos));
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
        // With nothing else left, the ways between rooms never walked over (walk-over lines are
        // thresholds), and last of all those onto floors that hurt.
        let first = self.usable(obs, hooks, &mut sift, out);
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
                if let Some(t) = by_line
                    .get(&li)
                    .filter(|t| !t.activators.is_disjoint(reach) && t.info.lock.satisfied(keys))
                {
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
                if let Some(spot) = self.touch_spot((t.x, t.y), reach) {
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

    /// The switches and doors she can tell are there and hasn't tried, that she can get to and
    /// has the key for.
    fn untried(
        &self,
        by_line: &BTreeMap<usize, &TriggerLine>,
        reach: &BTreeSet<usize>,
        keys: Keys,
        pos: Point,
    ) -> Vec<Objective> {
        let nav = &self.nav;
        let k = &self.knowledge;
        let mut out: Vec<Objective> = Vec::new();
        for li in k.interactables() {
            let Some(t) = by_line
                .get(&li)
                .filter(|t| !t.activators.is_disjoint(reach) && t.info.lock.satisfied(keys))
            else {
                continue;
            };
            let kind = if t.info.category.is_door() {
                Kind::Door
            } else {
                Kind::Switch
            };
            let mut o = Objective::new(
                kind,
                nav.activation_point(t, pos, reach),
                format!(
                    "try {} (line {li})",
                    if kind == Kind::Door { "door" } else { "switch" }
                ),
            );
            o.trigger = Some((*t).clone());
            out.push(o);
        }
        out
    }

    /// Repeatable switches pressed a while ago whose effect she never saw are worth another go
    /// once there's nothing else, each by what it works; and those whose effect she has put
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
        let doubted: BTreeSet<crate::knowledge::model::Mechanism> =
            k.doubts.keys().map(|(m, _)| *m).collect();
        let mut again = Vec::new();
        for &(li, tic) in &last_press {
            let Some(t) = by_line.get(&li) else { continue };
            if !k.mapped.contains(&li) || !matches!(t.info.trigger, Trigger::SR | Trigger::GR) {
                continue;
            }
            let unsure = doubted.contains(&k.tried_key(li))
                && !k.rechecked.contains(&(k.tried_key(li), k.lives));
            if learned_lines.contains(&(li as i64)) && !unsure {
                continue;
            }
            if (obs.tic - tic < REPRESS_AFTER && !unsure)
                || t.activators.is_disjoint(reach)
                || !t.info.lock.satisfied(keys)
            {
                continue;
            }
            // Not again knowing no more than when she last set out to press it: what was missed then
            // would be missed again.
            if !unsure
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
                    "try switch again (line {li}{})",
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
                if mesh.poly_at(spot) != Some(pi) || !self.standable(spot, false) {
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
            if let (Some(f), Some(b)) = (front, back) {
                if !now[f]
                    .iter()
                    .any(|&a| now[b].iter().any(|&c| can_cross(a, c)))
                {
                    continue; // (the pad isn't down yet, as far as she's seen)
                }
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
            if let Some(spot) = self.touch_spot((t.x, t.y), reach) {
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
            if let Some(spot) = self.touch_spot((t.x, t.y), reach) {
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
    pub(super) fn travel(&self, obs: &Observation) -> impl Fn(Point) -> f64 {
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
        if risky && !self.knowledge.traps.is_empty() {
            // Not back into a trap she's been stuck in before; but in one already, it's all there is.
            let here = mesh.poly_at(obs.pos());
            let traps: Vec<&BTreeSet<usize>> = self
                .knowledge
                .traps
                .iter()
                .filter(|t| here.is_none_or(|h| !t.contains(&h)))
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
        // Keys she has seen and has no way to yet: the way to one is likeliest near it, as a
        // player who's seen a key looks round where it is. Ground to explore, and doors, switches
        // and teleporters to try, count as nearer the nearer they are to such a key.
        let held = obs.keys.unwrap_or_default();
        let wanted: Vec<Point> = self
            .knowledge
            .known_things()
            .into_iter()
            .filter_map(|th| {
                let kk = Key::from_thing(th.kind).filter(|kk| !held.has(*kk))?;
                (!options.iter().any(|o| o.key == Some(kk))).then_some((th.x, th.y))
            })
            .collect();
        let pull = |o: &Objective| {
            if wanted.is_empty()
                || !matches!(
                    o.kind,
                    Kind::Frontier
                        | Kind::Room
                        | Kind::Visit
                        | Kind::Door
                        | Kind::Switch
                        | Kind::Teleporter
                )
            {
                return 1.0;
            }
            let d = wanted
                .iter()
                .map(|&k| dist(k, o.goal))
                .fold(f64::INFINITY, f64::min);
            1.0 - KEY_PULL * (1.0 - d / KEY_PULL_RANGE).max(0.0)
        };
        let mut weighed: Vec<(f64, Objective)> = options
            .into_iter()
            .map(|o| {
                (
                    travel(o.goal) * o.kind.weight() * pull(&o) * hooks.weigh(obs, &o),
                    o,
                )
            })
            .collect();
        weighed.sort_by(|a, b| {
            (a.1.priority(), a.0)
                .partial_cmp(&(b.1.priority(), b.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // On instinct for whoever directs her (`resorts` given: never playing alone), new ground
        // first: going back over what she's done (a walk across floor she's seen, walls pressed
        // for secrets, another look, a hunt, a spare pickup more than a short walk off) waits till
        // there's nothing new left. On a stream that's her wandering finished rooms; alone, the
        // run's own order stands.
        if !self.directed && self.resorts.is_some() && self.secrets_done {
            // (all the secrets found: no wall's worth pressing for one)
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
            if self.going_back == GoingBack::Never {
                weighed.retain(|(_, o)| !back_over(o));
            } else if weighed
                .iter()
                .any(|(_, o)| o.kind != Kind::Exit && !back_over(o))
            {
                // (stable: each part in its own order)
                weighed.sort_by_key(|(_, o)| back_over(o));
            }
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

/// a key seen with no way to it yet: what could show the way counts as up to this much nearer
/// (a half) right by it...
const KEY_PULL: f64 = 0.5;
/// ...and none from this far from it
const KEY_PULL_RANGE: f64 = 1024.0;

/// on instinct for whoever directs her, a spare pickup this far off along the way or less is
/// taken with the new ground; further, it waits till there's no new ground left
const INSTINCT_PICKUP_NEAR: f64 = 512.0;

/// a point her reach gets to by no walk the travel field knows (a teleporter, a jump): counted
/// this many times its straight line
const TRAVEL_UNKNOWN: f64 = 2.0;
