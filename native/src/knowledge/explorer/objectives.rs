//! Listing and ranking objectives in the currently reachable world.

use super::{compass, Explorer, Hooks, Kind, Objective, Suspecting, KEEP_REACH, WHILE_OPEN};
use crate::geom::{dist, segment_distance, Point};
use crate::knowledge::model::Heard;
use crate::motor::Observation;
use crate::movers::cycles_back;
use crate::nav::can_cross;
use crate::navigator::{add_height, TeleEdge, TriggerLine, EXIT_FLOOR, MAX_STEP, PLAYER_HEIGHT};
use crate::specials::{classify_line, Category, Key, Trigger};
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
const TAP_AMMO: [&str; 7] = ["Clip", "ClipBox", "Shell", "ShellBox", "Backpack", "Chaingun", "Shotgun"];

/// a threshold's spot: this far over the line
const THRESHOLD_IN: f64 = 8.0;
/// How far over a line never walked over she makes for.
const SEAM_IN: f64 = 32.0;

/// the expert's run: no goal this near a walk-over exit line (she'd slide over)
const EXIT_MARGIN: f64 = 64.0;

/// a wall suspected this much is tried when she's this near where she'd stand to (not only when
/// there's nothing else to do)
const NEAR_SUSPECT: f64 = 6.0;

const NEAR_WALL: f64 = 128.0;

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

/// (an objective that would cut her off from others: after them)
const LATER: &str = " (after what it cuts off)";

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
    pub fn reach_chain(&mut self, obs: &Observation, goal: Point, planner: bool) -> Option<Vec<String>> {
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
            .filter(|t| !t.effects.is_empty() && !t.linedef().is_some_and(|li| spent.contains(&li)) && !(planner && t.info.trigger == Trigger::DR))
            .collect();
        let (_, all, keys_all) = view.saturate(here, &states, keys, &key_items, &actions);
        view.reach_chain(here, to, &all, Some(&view.local_doors(keys_all)))
    }

    /// What she could go and do now.
    pub fn objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> Vec<Objective> {
        let t0 = Instant::now();
        let mut out = self.list_objectives(obs, risky, hooks);
        if self.knowledge.mortal {
            // (mortal: nowhere under a crusher to stop at just to look or walk round; what's to be
            // picked up or pressed there, passed through. E4M7's walls of sectors 226 and 230,
            // looked at from under them, crushed)
            let crushers = self.crusher_sectors();
            out.retain(|o| {
                matches!(o.kind, Kind::Item | Kind::Key | Kind::Switch | Kind::Door | Kind::Exit | Kind::Teleporter | Kind::Secret)
                    || !self.nav.poly_at(o.goal).is_some_and(|p| crushers.contains(&self.nav.sector_of_poly(p)))
            });
        }
        self.stats.listed += 1;
        self.stats.listed_secs += t0.elapsed().as_secs_f64();
        out
    }

    fn list_objectives(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> Vec<Objective> {
        let nav = Rc::clone(&self.nav);
        let view = self.view();
        let pos = obs.pos();
        // (from the floor she's on: on a lip, not the drop her middle's over)
        let Some(here) = nav.poly_at(self.standing_point(obs)) else { return Vec::new() };
        let keys = obs.keys.unwrap_or_default();
        let states = self.states();
        // (what she could get to using what she's learned: the planner works out how)
        let spent = self.spent_once();
        let actions: Vec<&TriggerLine> = view.triggers.iter().filter(|t| !t.effects.is_empty() && !t.linedef().is_some_and(|li| spent.contains(&li))).collect();
        let t_sat = Instant::now();
        let (reach, states_all, _) = view.saturate(here, &states, keys, &[], &actions);
        self.stats.saturate_secs += t_sat.elapsed().as_secs_f64();
        self.reach = reach.clone();
        self.reach_states = states_all.clone();
        // (what she can get to only while a door or lift she's pressed open is open: go now, as a
        // player goes through a door before it shuts, if near enough to make it: E2M1's door 44,
        // whose only other switch is on the far side. Only somewhere she's never walked: back
        // where she's been can wait, there'll be a way or she'll find one)
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
        let by_line: BTreeMap<usize, &TriggerLine> = nav.triggers.iter().filter_map(|t| t.linedef().map(|li| (li, t))).collect();
        let k = &self.knowledge;
        // (where she can set off an exit from, as things are)
        let exit_polys: BTreeSet<usize> = k
            .exits()
            .into_iter()
            .filter_map(|li| by_line.get(&li).filter(|t| t.info.lock.satisfied(keys)))
            .flat_map(|t| t.activators.intersection(&reach).copied().collect::<Vec<_>>())
            .collect();
        if self.exits_open {
            for li in k.exits() {
                if let Some(t) = by_line.get(&li).filter(|t| !t.activators.is_disjoint(&reach) && t.info.lock.satisfied(keys)) {
                    let mut o = Objective::new(Kind::Exit, nav.activation_point(t, pos, &reach), format!("exit (line {li})"));
                    o.trigger = Some((*t).clone());
                    out.push(o);
                }
            }
            // (a floor she's had end the level under her: stood on again)
            for &sec in &k.exit_floors {
                if let Some(t) = nav.triggers.iter().find(|t| t.line == EXIT_FLOOR - sec as i64 && !t.activators.is_disjoint(&reach)) {
                    let mut o = Objective::new(Kind::Exit, nav.activation_point(t, pos, &reach), format!("exit (the floor of sector {sec})"));
                    o.trigger = Some(t.clone());
                    out.push(o);
                }
            }
        }
        for th in k.known_things() {
            let Some(kk) = Key::from_thing(th.kind).filter(|kk| !keys.has(*kk)) else { continue };
            let spots: Vec<Point> = view.pickup_spots(th.x, th.y, &states).into_iter().filter(|(p, _)| reach.contains(p)).map(|(_, pt)| pt).collect();
            if let Some(pt) = spots.into_iter().reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a }) {
                let mut o = Objective::new(Kind::Key, pt, format!("pick up {}", kk.name()));
                o.key = Some(kk);
                out.push(o);
            }
        }
        // (out of rounds to tap a wall or a gun line with: the ammo she's seen, as a player tops up;
        // E2M4's late walls, shot at with an empty pistol, its door that reopens to a shot never)
        let rounds = |slot: i32| crate::motor::follower::tap_rounds(obs, slot);
        if obs.ammo.is_some() && rounds(2).max(rounds(4)) < LOW_BULLETS && rounds(3) < LOW_SHELLS {
            let known = k.known_polys();
            for t in obs.things.iter().flatten().filter(|t| TAP_AMMO.contains(&t.name)) {
                if !nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)) {
                    continue;
                }
                if let Some(spot) = self.touch_spot((t.x, t.y), &reach) {
                    let mut o = Objective::new(Kind::Item, spot, format!("pick up {} (for the gun)", t.name));
                    o.thing = Some(t.id);
                    out.push(o);
                }
            }
        }
        for li in k.interactables() {
            let Some(t) = by_line.get(&li).filter(|t| !t.activators.is_disjoint(&reach) && t.info.lock.satisfied(keys)) else { continue };
            let kind = if t.info.category.is_door() { Kind::Door } else { Kind::Switch };
            let mut o =
                Objective::new(kind, nav.activation_point(t, pos, &reach), format!("try {} (line {li})", if kind == Kind::Door { "door" } else { "switch" }));
            o.trigger = Some((*t).clone());
            out.push(o);
        }
        // repeatable switches pressed a while ago whose effect she never saw: worth another go
        // once there's nothing else (E3M4); by what it works (MAP08's switch, pressed "again"
        // every few seconds, set off the one in front each time); and those whose effect she's
        // put down to them on timing alone, unsure: pressed again and watched, once a life
        let learned_lines: BTreeSet<i64> = k.learned.iter().filter(|(_, s)| !s.is_empty()).map(|(&i, _)| nav.triggers[i].line).collect();
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
        let doubted: BTreeSet<crate::knowledge::model::Mechanism> = k.doubts.keys().map(|(m, _)| *m).collect();
        let mut again = Vec::new();
        for &(li, tic) in &last_press {
            let Some(t) = by_line.get(&li) else { continue };
            if !k.mapped.contains(&li) || !matches!(t.info.trigger, Trigger::SR | Trigger::GR) {
                continue;
            }
            let unsure = doubted.contains(&k.tried_key(li)) && !k.rechecked.contains(&(k.tried_key(li), k.lives));
            if learned_lines.contains(&(li as i64)) && !unsure {
                continue;
            }
            if (obs.tic - tic < REPRESS_AFTER && !unsure) || t.activators.is_disjoint(&reach) || !t.info.lock.satisfied(keys) {
                continue;
            }
            // (not again knowing no more than when last set out to press it: what was missed then
            // is missed again; MAP19's lift switch at the bottom of a pit, pressed every minute,
            // each time just before she'd have counted herself stranded there)
            if !unsure && self.pressed_knowing.get(&li).is_some_and(|known| *known == k.known_summary()) {
                continue;
            }
            let mut o = Objective::new(
                Kind::Switch,
                nav.activation_point(t, pos, &reach),
                format!("try switch again (line {li}{})", if unsure { ", unsure what it does" } else { "" }),
            );
            o.trigger = Some((*t).clone());
            again.push(o);
        }
        if self.suspecting != Suspecting::No {
            let panic = self.suspecting == Suspecting::Panic;
            out.extend(self.walls(obs, &reach, panic));
        } else {
            // (a wall she strongly suspects, right by her: tried there and then, as a player in
            // front of it would, not left for when there's nothing else; MAP05's yellow door
            // under the exit sign, passed by and left for a recheck round that never ended)
            let near: Vec<Objective> = self
                .suspected(obs, &reach, false)
                .into_iter()
                .filter(|w| w.0 >= NEAR_SUSPECT && dist(w.2, pos) <= NEAR_WALL && self.standable(w.2, false))
                .map(|(score, li, stand, mid)| super::selection::wall_objective(score, li, stand, mid, false))
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
            for o in out.iter_mut().filter(|o| o.kind != Kind::Exit && nav.poly_at(o.goal).is_some_and(|p| while_open.contains(&p))) {
                o.urgent = true;
                o.worth = Some((closes - obs.tic) as f64 * OPEN_SPEED);
                o.label = format!("{}{WHILE_OPEN}", o.label);
            }
        }
        // (not anywhere that would cut her off from most of what she can get to now, as far as
        // she knows; the way back as it'll be: a door that closes again shut, unless something
        // opens it for good, but for one she can open from where she'll be: MAP12's door 31; a
        // lift the same, at rest unless she can call it from there: MAP27's lift 156, whose only
        // switch is on it, over the room below)
        let doors = view.local_doors(keys);
        let known_triggers = self.knowledge.known_triggers();
        // (not where a door or lift that comes back was set off since one that stays: E2M4's
        // door 48, open for good, pressed again from outside, shuts behind her)
        let last_fired = |pred: &dyn Fn(&TriggerLine) -> bool, s: usize| -> i64 {
            let lines: BTreeSet<i64> = known_triggers.iter().filter(|(t, secs)| pred(t) && secs.contains(&s)).map(|(t, _)| t.line).collect();
            self.knowledge.fired.iter().filter(|f| f.life == self.knowledge.lives && lines.contains(&(f.line as i64))).map(|f| f.tic).max().unwrap_or(-1)
        };
        let for_good: BTreeSet<usize> = known_triggers
            .iter()
            .filter(|(t, _)| t.permanent())
            .flat_map(|(_, s)| s.iter().copied())
            .filter(|&s| last_fired(&|t| !t.permanent() && cycles_back(t.info.special, &t.info.description), s) <= last_fired(&|t| t.permanent(), s))
            .collect();
        let mut states_back = states_all.clone();
        let mut openers: Vec<(TriggerLine, BTreeSet<usize>)> = Vec::new();
        for (t, secs) in &known_triggers {
            let comes_back = t.info.category.is_door() || (t.info.category == Category::Lift && cycles_back(t.info.special, &t.info.description));
            if !t.permanent() && comes_back && !t.linedef().is_some_and(|li| spent.contains(&li)) {
                let shut: BTreeSet<usize> = secs.difference(&for_good).copied().collect();
                for &s in &shut {
                    states_back[s] = vec![nav.movers.initial[s]];
                }
                openers.push((t.clone(), shut));
            }
        }
        let way_back = |start: usize| -> BTreeSet<usize> {
            // (opening on the way the doors she can from there: a switch or a walk-over line on
            // the far side too: MAP19's door)
            let mut st = states_back.clone();
            loop {
                let after = view.reachable(start, &st, Some(&doors));
                let more: Vec<&BTreeSet<usize>> = openers
                    .iter()
                    .filter(|(t, secs)| {
                        !t.activators.is_disjoint(&after) && t.info.lock.satisfied(keys) && secs.iter().any(|&x| st[x] == vec![nav.movers.initial[x]])
                    })
                    .map(|(_, secs)| secs)
                    .collect();
                let before = st.clone();
                for secs in more {
                    for &x in secs {
                        for &h in &states_all[x] {
                            add_height(&mut st[x], h);
                        }
                    }
                }
                if st == before {
                    return after; // (nothing more opens)
                }
            }
        };
        // (what of where she can get to now she couldn't get back to from a poly)
        let mut afters: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        // (and whether where she'd be left is still open to finding a way out: an edge onto
        // somewhere she's never seen, or a sector there she's never set foot in but the goal's,
        // where a walk-over line can be: no way out known isn't no way out. E1M4's nukage pit,
        // its switch for the lift out in an alcove she'd not seen into; its room of bonuses, the
        // lines round the pad in the middle opening its doors for good)
        let known_polys = self.knowledge.known_polys();
        let entered: BTreeSet<usize> = self.knowledge.visited.iter().map(|&q| nav.sector_of_poly(q)).collect();
        // (lines she's pressed or set off, and sectors she believes shut: a door's face she's not
        // tried is a way out to try, MAP04's hidden door shutting behind her)
        let pressed: BTreeSet<usize> = self.knowledge.fired.iter().map(|f| f.line).chain(self.knowledge.tried_walls.keys().copied()).collect();
        let shut = |sec: usize| states[sec].iter().all(|h| h.ceiling - h.floor < PLAYER_HEIGHT);
        let untried: Vec<&TriggerLine> = self.knowledge.interactables().iter().filter_map(|li| by_line.get(li).copied()).collect();
        let mut lost_from = |p: usize| -> (BTreeSet<usize>, bool) {
            let after = afters.entry(p).or_insert_with(|| way_back(p));
            if after.contains(&here) {
                return (BTreeSet::new(), true);
            }
            let goal_sector = nav.sector_of_poly(p);
            let unseen_edge = after.iter().any(|&q| nav.mesh.poly(q).portals.iter().any(|&pi| !known_polys.contains(&nav.mesh.portals[pi].other(q))));
            let unwalked = after.iter().map(|&q| nav.sector_of_poly(q)).any(|sec| sec != goal_sector && !entered.contains(&sec));
            let untried_door = after.iter().any(|&q| {
                nav.mesh.poly(q).portals.iter().any(|&pi| {
                    let portal = &nav.mesh.portals[pi];
                    let other = nav.sector_of_poly(portal.other(q));
                    shut(other) && portal.linedef.is_some_and(|li| !pressed.contains(&li))
                })
            });
            // (or a switch or door not tried yet that she could press from there: E4M9's pit of
            // bonuses, its lift out a face of plain support at the south end, as `shuts_in` has it)
            let untried_switch = untried.iter().any(|t| !t.activators.is_disjoint(after));
            (reach.difference(after).copied().collect(), unseen_edge || unwalked || untried_door || untried_switch)
        };
        let tic = obs.tic;
        // (an item the level put somewhere has a way back out from it, if not one she knows yet:
        // nobody makes a pit with a prize and no way out; not one a monster dropped, which can
        // fall anywhere. The user's rule, from E4M9's pit of bonuses)
        let placed = |o: &Objective| {
            o.kind == Kind::Item
                && o.thing
                    .and_then(|id| obs.things.iter().flatten().find(|t| t.id == id))
                    .is_some_and(|t| nav.map.things.iter().any(|m| (m.x - t.x).abs() < 1.0 && (m.y - t.y).abs() < 1.0))
        };
        // (no switch pressed from a floor that hurts while there's floor that doesn't to get to:
        // what a switch does is anyone's guess, and shut in there is dead, a suit on or not (it
        // runs out). E2M2: seed 2, an 80 drop into a nukage pit for one; seed 1, waded in with a
        // suit on; both presses raised the pit's one low way out, sealed in, dead. Shut in already,
        // no floor that doesn't in reach, it's hers to try: E1M4's lift switch; and with nothing
        // else at all to do, walls and all, as `harm_ok` has it)
        // A floor that doesn't hurt only through a door that shuts again by itself, with nothing
        // on her side to open it, is no way out either: shut in once it shuts, whatever she does
        // (E1M9: a nukage room behind a door walk-over lines outside open for a few seconds,
        // the lift out of it a switch inside, never pressed while the door stood open; once
        // shut, too far a wade back in for her health)
        let safe = |polys: &BTreeSet<usize>| polys.iter().any(|&p| !self.knowledge.damaging(nav.sector_of_poly(p)));
        let stays_shut = self.knowledge.damaging(nav.sector_of_poly(here)) && !safe(&way_back(here));
        // (nor is the dry floor she waded off from a dead end: there was nothing to do there,
        // so a switch down here is hers to try, before going back to stand on it. E1M9 HMP: an
        // island in a nukage pit, nothing left on it, waded through a door the island's lines
        // open into the room with the lift out; straight back to the island, too low for the
        // wade again, the lift's switch never pressed)
        let shut_in = self.harm_ok || self.waded_dead_end || stays_shut || !safe(&reach);
        let switch_in_harm = |this: &Self, o: &Objective| o.kind == Kind::Switch && !shut_in && nav.poly_at(o.goal).is_some_and(|p| this.knowledge.damaging(nav.sector_of_poly(p)));
        // (down in a pit whose floor hurts, no floor that doesn't in reach: every second there
        // costs health, so a switch or door she can see, a way out someone built, before a guess
        // at a plain wall. E4M8: a missed jump into the lava, three walls tried by the lift's
        // switch, dead on the way to it)
        let in_pit = self.knowledge.mortal && self.on_hurting_floor(obs) && (stays_shut || self.waded_dead_end || !safe(&reach));
        let mut usable = |this: &Self, hooks: &mut dyn Hooks, objectives: Vec<Objective>| -> Vec<Objective> {
            let mut objectives: Vec<Objective> = objectives.into_iter().filter(|o| !switch_in_harm(this, o)).collect();
            if in_pit && objectives.iter().any(|o| matches!(o.kind, Kind::Switch | Kind::Door)) {
                objectives.retain(|o| o.kind != Kind::Wall);
            }
            if risky {
                // nothing safe left: anything, one-way drops and nukage and all; but not for what
                // little it's worth: a sound (MAP09: a noise far off took her down into a pit), a
                // place seen already looked at again, a monster gone from where it was last seen
                // (the health a way costs against what it's for, as a player weighs it: nothing new
                // to find there, the floor's hits for nothing); and never away from
                // every way out there is, as far as she knows (MAP24's secret ledge over the
                // nukage pit: the level given up for a look)
                return objectives
                    .into_iter()
                    .filter(|o| !matches!(o.kind, Kind::Noise | Kind::Recheck | Kind::Hunt))
                    .filter(|o| {
                        exit_polys.is_empty()
                            || o.kind == Kind::Exit
                            || placed(o)
                            || nav.poly_at(o.goal).map(&mut lost_from).is_none_or(|(g, open)| open || !exit_polys.is_subset(&g))
                    })
                    .map(|o| Objective { label: format!("{} (risky)", o.label), risky: true, ..o })
                    .filter(|o| this.fresh(o, tic))
                    .collect::<Vec<_>>()
                    .into_iter()
                    // (risky as to the way there, not as to what's there: MAP08's box of rockets in
                    // the cyberdemon's room, the last thing to do, walked in for at 137 health)
                    // (items only: they're to be had or not; the ways on, through it if need be:
                    // MAP06's spider's hall)
                    .filter(|o| o.kind != Kind::Item || !hooks.dangerous(obs, o))
                    .collect();
            }
            let mut kept: Vec<Objective> = Vec::new();
            let mut lost: Vec<BTreeSet<usize>> = Vec::new();
            for o in objectives {
                if !this.fresh(&o, tic) || hooks.dangerous(obs, &o) {
                    continue;
                }
                let cut = nav.poly_at(o.goal).map(&mut lost_from);
                let open = cut.as_ref().is_none_or(|c| c.1) || placed(&o);
                let gone = cut.map(|c| c.0);
                // (worth the risk, however far she can't get back from: the exit, a teleporter,
                // where it leads is the unknown; a key, it opens the way)
                // (and ammo or a gun the level put there when she's next to out of it: nothing she
                // can't get back to is worth much without, and there's a way out from where the
                // level put it. E2M5 on UV: six shots, the clips she'd seen past doors she knew no
                // way back through, left for walls by a baron she couldn't kill; cornered, dead)
                let risk_it = matches!(o.kind, Kind::Exit | Kind::Teleporter | Kind::Key) || (placed(&o) && hooks.worth_cut_off(obs, &o));
                // (most of what she can get to, and never the way out, as far as she knows: MAP24's
                // nukage pit, where a wall was worth a one-way drop by the rest of it)
                let keeps = gone.as_ref().is_some_and(|g| {
                    (reach.len() - g.len()) as f64 >= KEEP_REACH * reach.len() as f64 && (exit_polys.is_empty() || open || !exit_polys.is_subset(g))
                });
                if risk_it || keeps {
                    lost.push(gone.unwrap_or_default());
                    kept.push(o);
                }
            }
            // what would cut her off from where there's more to do, as far as she knows, after
            // that (as a player finishes a room before dropping out of it: E2M1's, with the
            // teleporter in it, left by its door, whose switches are spent or inside)
            let goals: Vec<Option<usize>> = kept.iter().map(|o| nav.poly_at(o.goal)).collect();
            for (i, o) in kept.iter_mut().enumerate() {
                if o.kind == Kind::Exit || lost[i].is_empty() {
                    continue;
                }
                if goals.iter().enumerate().any(|(j, g)| j != i && g.is_some_and(|g| lost[i].contains(&g))) {
                    o.later = true;
                    o.label = format!("{}{LATER}", o.label);
                }
            }
            kept
        };
        // with nothing else left, the ways between rooms never walked over (walk-over lines are
        // thresholds: E4M7's), and last of all, those onto floors that hurt (E4M2's door opener
        // is on lava)
        let first = usable(self, hooks, out);
        if !first.is_empty() {
            return first;
        }
        let t1 = self.thresholds(&reach, false);
        let x = usable(self, hooks, t1);
        if !x.is_empty() {
            return x;
        }
        let t2 = self.thresholds(&reach, true);
        let x = usable(self, hooks, t2);
        if !x.is_empty() {
            return x;
        }
        let x = usable(self, hooks, self.seams(&reach, pos));
        if !x.is_empty() {
            return x;
        }
        let x = usable(self, hooks, again);
        if !x.is_empty() {
            return x;
        }
        let lr = hooks.last_resort(self, obs, &reach, pos);
        usable(self, hooks, lr)
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
                // look from the way in, just this side of it (from the poly's middle it can be
                // round a corner: E2M4)
                let mid = ((portal.p1.0 + portal.p2.0) / 2.0, (portal.p1.1 + portal.p2.1) / 2.0);
                let back = dist(mid, poly.center);
                let k = if back > 0.0 { 1.0f64.min(FRONTIER_BACK / back) } else { 0.0 };
                let mut spot = (mid.0 + (poly.center.0 - mid.0) * k, mid.1 + (poly.center.1 - mid.1) * k);
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
                let mut o = Objective::new(Kind::Frontier, self.stand_spot(best[&s].1, reach), format!("look into sector {s}"));
                o.reveals = unseen[&s].clone();
                o
            })
            .collect()
    }

    /// Reachable polys she hasn't stood in across a line from ones she has: just over the line
    /// (all a walk-over line needs; a strip's middle may have no room for the box: E4M7's).
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
                let Some(li) = portal.linedef.filter(|_| k.visited.contains(&portal.other(pi))) else { continue };
                let mid = ((portal.p1.0 + portal.p2.0) / 2.0, (portal.p1.1 + portal.p2.1) / 2.0);
                let d = dist(mid, poly.center);
                if d < 1e-6 {
                    continue;
                }
                let spot = (mid.0 + (poly.center.0 - mid.0) / d * THRESHOLD_IN, mid.1 + (poly.center.1 - mid.1) / d * THRESHOLD_IN);
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
    /// trigger or a mover needs; one of them moved since, a ledge now, still one):
    /// over each, to the side away from her. (MAP12: a W1 floor raise between two slime sectors,
    /// each walked into across other lines, the way on)
    fn seams(&self, reach: &BTreeSet<usize>, pos: Point) -> Vec<Objective> {
        let (k, mesh) = (&self.knowledge, &self.nav.mesh);
        let sectors = &self.nav.map.sectors;
        let alike = |a: usize, b: usize| {
            let (x, y) = (&sectors[a], &sectors[b]);
            let (hx, hy) = (k.first_heights.get(&a), k.first_heights.get(&b));
            hx.is_some() && hx == hy && x.floor_tex == y.floor_tex && x.ceiling_tex == y.ceiling_tex && x.light == y.light
        };
        let mut done: BTreeSet<usize> = BTreeSet::new();
        let mut out = Vec::new();
        for &pi in reach.intersection(&k.visited) {
            for &q in &mesh.poly(pi).portals {
                let portal = &mesh.portals[q];
                let other = portal.other(pi);
                let Some(li) = portal.linedef.filter(|li| !k.crossed.contains(li) && !done.contains(li)) else { continue };
                let (a, b) = (mesh.poly(pi).sector, mesh.poly(other).sector);
                if a == b || !k.visited.contains(&other) || !reach.contains(&other) || !alike(a, b) {
                    continue;
                }
                let mid = ((portal.p1.0 + portal.p2.0) / 2.0, (portal.p1.1 + portal.p2.1) / 2.0);
                // (the far side from her: from where she is, over it)
                let far = [pi, other].into_iter().max_by(|&x, &y| dist(mesh.poly(x).center, pos).total_cmp(&dist(mesh.poly(y).center, pos))).unwrap_or(other);
                let d = dist(mid, mesh.poly(far).center);
                if d < 1e-6 {
                    continue;
                }
                // (in far enough that getting there is going over: not done within arrival of it)
                let c = mesh.poly(far).center;
                let at = |depth: f64| (mid.0 + (c.0 - mid.0) / d * depth, mid.1 + (c.1 - mid.1) / d * depth);
                let Some(spot) = [SEAM_IN, THRESHOLD_IN].map(at).into_iter().find(|&p| mesh.poly_at(p) == Some(far) && self.standable(p, false)) else { continue };
                done.insert(li);
                let mut o = Objective::new(Kind::Visit, spot, format!("walk over line {li}"));
                o.reveals = BTreeSet::from([far as i64]);
                // (the line, tried once the trip's over, gone over or not: a spot over it can be
                // got to round it, and from the far side the next one's spot back; E1M5, two of
                // them in turn for the hour)
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
                .filter(|(_, es)| es.iter().any(|e| matches!(e, TeleEdge::Teleport(i) if nav.teleports[*i].line == li)))
                .map(|(p, _)| p)
                .filter(|p| reach.contains(p))
                .collect();
            if src.is_empty() {
                continue;
            }
            let (front, back) = nav.map.sides(li);
            if let (Some(f), Some(b)) = (front, back) {
                if !now[f].iter().any(|&a| now[b].iter().any(|&c| can_cross(a, c))) {
                    continue; // (the pad isn't down yet, as far as she's seen)
                }
            }
            let (a, b) = nav.map.line_seg(li);
            let length = dist(a, b).max(1.0);
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            // the front side (the one that teleports), a little back from the line
            let mut spot = (mid.0 + (b.1 - a.1) / length * 24.0, mid.1 - (b.0 - a.0) / length * 24.0);
            if !nav.poly_at(spot).is_some_and(|p| reach.contains(&p)) {
                spot = src.iter().map(|&p| nav.poly_center(p)).reduce(|x, y| if dist(y, mid) < dist(x, mid) { y } else { x }).unwrap_or(spot);
            }
            // (somewhere she fits first: not up against a torch in front of the pad, MAP11's,
            // when another side of it is clear)
            let fits = self.stands_at(spot, &heights);
            let Some(info) = classify_line(nav.map.linedefs[li].special) else { continue };
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
            let mut o = Objective::new(Kind::Teleporter, spot, format!("try teleporter (line {li})"));
            o.trigger = Some(t);
            o.pad = pad.clone();
            match best.iter_mut().find(|(p, _, _)| *p == pad) {
                Some((_, bf, b)) if (fits, -dist(spot, pos)) > (*bf, -dist(b.goal, pos)) => (*bf, *b) = (fits, o),
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
            let Some(best) = reach.iter().copied().reduce(|a, b| if dist(mesh.poly(b).center, h.origin) < dist(mesh.poly(a).center, h.origin) { b } else { a })
            else {
                continue;
            };
            // (where the box can stand: MAP05's)
            let spot = self.stand_spot(mesh.poly(best).center, reach);
            if dist(spot, h.origin) > dist(pos, h.origin) - HEAR_NEARER {
                continue;
            }
            let mut o = Objective::new(
                Kind::Noise,
                spot,
                format!("check a noise {} (~{:.0}){}", compass(h.bearing), h.distance, if urgent { " right away" } else { "" }),
            );
            o.reveals = BTreeSet::from([h.tic]);
            o.heard = Some(*h);
            o.urgent = urgent;
            out.push(o);
        }
        out
    }

    /// A sound heard just after she set off a line she doesn't know the effect of: go and look
    /// now, while it's still moving (MAP19's line 443).
    pub(super) fn urgent(&self, h: &Heard, tic: i64) -> bool {
        let k = &self.knowledge;
        if tic - h.tic >= URGENT_TICS || h.seen {
            return false;
        }
        let n = k.fired.len();
        let lines: BTreeSet<i64> =
            k.fired[n.saturating_sub(8)..].iter().filter(|f| (0..=CAUSE_TICS).contains(&(h.tic - f.tic))).map(|f| f.line as i64).collect();
        self.nav.triggers.iter().any(|t| lines.contains(&t.line) && k.learned.get(&t.id).is_none_or(BTreeSet::is_empty))
    }

    /// Has where a sound seemed to come from been in view since?
    pub(super) fn noise_seen(&self, h: &Heard) -> bool {
        self.nav.poly_at(h.origin).is_some_and(|p| self.knowledge.seen_tic.get(&self.nav.sector_of_poly(p)).copied().unwrap_or(-1) > h.tic)
    }

    /// Is the box (with the checks' slack) at p over a line that teleports her or ends the level?
    fn by_trip_line(&self, p: Point) -> bool {
        let (m, mesh) = (&self.nav.map, &self.nav.mesh);
        let r = crate::geom::PLAYER_RADIUS + 0.25;
        crate::geom::cells_around(p, p, r + 1.0).any(|c| {
            mesh.line_grid.get(&c).into_iter().flatten().any(|&li| {
                let sp = m.linedefs[li].special;
                let trips = crate::navigator::TELEPORTERS_FOR_PLAYERS.contains(&sp)
                    || crate::specials::classify_line(sp).is_some_and(|i| i.category.is_exit() && i.trigger.walked());
                trips && crate::geom::swept_box_hits(p, p, r, m.line_seg(li))
            })
        })
    }

    /// Parts of known rooms she can get to but hasn't walked (walking round finds the walk-over
    /// lines nobody can see): per sector, the reachable poly furthest from anywhere she's stood,
    /// if far enough to be worth it; and every room stepped into once (E1M4's door opener).
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
                // (an 8-wide strip by a shut door, E1M4; a strip the box's width beside a teleport
                // pad, MAP26's, the box over the pad's edge there and no way to it but squeezed
                // past that edge: a whole search, 20000 steps, for nothing, every life)
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
            let gap = stood.iter().map(|&c| dist(poly.center, c)).fold(f64::INFINITY, f64::min);
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
                let mut o = Objective::new(Kind::Visit, mesh.poly(pi).center, format!("walk across sector {s}"));
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
            .filter(|t| t.info.category == crate::specials::Category::Crusher && !t.info.description.contains("stop"))
            .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
            .collect()
    }

    /// Is she down on a floor she takes to hurt?
    pub(super) fn on_hurting_floor(&self, obs: &Observation) -> bool {
        let Some(p) = self.nav.poly_at(obs.pos()) else { return false };
        let s = self.nav.sector_of_poly(p);
        self.knowledge.damaging(s) && obs.z.is_some_and(|z| (z - obs.heights[s].floor).abs() < 1.0) && !self.knowledge.protected(obs.tic)
    }

    /// (mortal runs) Down on a floor that hurts: off it, to the nearest floor that doesn't she
    /// can get to (the user's: "if you fall in, head for the nearest floor that doesn't hurt").
    /// Waded in on purpose, for a risky objective there, she looks round where she waded to
    /// before wading back: off it only as one of the risky options, by how far, while what
    /// she has left after paying for the way back (as much again as it's cost her since) is
    /// over the margin. (E1M9: through a door the nukage's walk-over lines open for a few
    /// seconds, into a nukage room whose only way on is a lift's switch inside: the look she
    /// went for done in the doorway, straight back out every time, the switch never seen)
    fn get_off(&self, obs: &Observation, reach: &BTreeSet<usize>, risky: bool) -> Vec<Objective> {
        if !self.knowledge.mortal || !self.on_hurting_floor(obs) {
            return Vec::new();
        }
        let health = obs.health.unwrap_or(100.0);
        let meant = self.waded_dead_end || self.waded.is_some_and(|h| health - (h - health) > super::routing::HEALTH_MARGIN);
        if meant && !risky {
            return Vec::new();
        }
        let mesh = &self.nav.mesh;
        let pos = obs.pos();
        let best = reach
            .iter()
            .copied()
            .filter(|&p| !self.knowledge.damaging(mesh.poly(p).sector) && self.standable(mesh.poly(p).center, false))
            .min_by(|&a, &b| dist(mesh.poly(a).center, pos).total_cmp(&dist(mesh.poly(b).center, pos)));
        best.map(|p| {
            let mut o = Objective::new(Kind::Visit, mesh.poly(p).center, "get off the floor that hurts");
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
        let short = self.short_of_health.is_some_and(|t| obs.tic - t < SHORT_TICS);
        let known = self.knowledge.known_polys();
        let mut out = Vec::new();
        for t in obs.things.iter().flatten() {
            // (what heals her past 100 counts as an item, had anyway: these only below it)
            let heals = matches!(t.name, "Stimpack" | "Medikit") && health < 100.0 && (short || health < HEAL_BELOW);
            let suit = t.name == "RadSuit" && (short || self.on_hurting_floor(obs)) && !self.knowledge.protected(obs.tic);
            if !heals && !suit {
                continue;
            }
            if !self.nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)) {
                continue;
            }
            if let Some(spot) = self.touch_spot((t.x, t.y), reach) {
                let why = if suit { "for the floors that hurt" } else { "to heal" };
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
        for t in obs.things.iter().flatten().filter(|t| self.spares.contains(&t.id)) {
            if !self.nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)) {
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
                    if !self.reach.contains(&q) || floors(q).fold(f64::INFINITY, f64::min) - high > MAX_STEP {
                        continue;
                    }
                    let mid = ((portal.p1.0 + portal.p2.0) / 2.0, (portal.p1.1 + portal.p2.1) / 2.0);
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

    /// The secrets run, the level again: back into the sectors she found secrets in before
    /// (the tally starts again; knowing the place, nothing else sends her back: MAP19's room
    /// behind door 558, opened in her first life, never stepped into in the one that got 8/9).
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
                .min_by(|&a, &b| dist(mesh.poly(a).center, pos).total_cmp(&dist(mesh.poly(b).center, pos)));
            if let Some(p) = nearest {
                let mut o = Objective::new(Kind::Secret, mesh.poly(p).center, format!("back into secret sector {s}"));
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
        let Some(since) = k.unexplained_since() else { return Vec::new() };
        let reach_sectors: BTreeSet<usize> = reach.iter().map(|&p| mesh.poly(p).sector).collect();
        let mut best: Vec<(usize, f64, Point)> = Vec::new();
        for &pi in reach {
            let poly = mesh.poly(pi);
            for &q in &poly.portals {
                let s = mesh.poly(mesh.portals[q].other(pi)).sector;
                if reach_sectors.contains(&s) || !k.sectors.contains(&s) || k.seen_tic.get(&s).copied().unwrap_or(-1) > since {
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
                let mut o = Objective::new(Kind::Recheck, self.stand_spot(c, reach), format!("look at sector {s} again"));
                o.reveals = BTreeSet::from([s as i64]);
                o
            })
            .collect()
    }

    /// The objectives open, the explorer's way round: the exit first if she can get there, then
    /// the locks her keys open, then by distance as the crow flies (weighted by kind).
    pub fn ranked(&mut self, obs: &Observation, risky: bool, hooks: &mut dyn Hooks) -> Vec<Objective> {
        let mut options = self.objectives(obs, risky, hooks);
        let mesh = Arc::clone(&self.nav.mesh);
        if risky && !self.knowledge.traps.is_empty() {
            // (not back into a trap she's been stuck in before: MAP18's moat; but in one already,
            // it's all there is: MAP21's pit)
            let here = mesh.poly_at(obs.pos());
            let traps: Vec<&BTreeSet<usize>> = self.knowledge.traps.iter().filter(|t| here.is_none_or(|h| !t.contains(&h))).collect();
            options.retain(|o| !traps.iter().any(|trap| mesh.poly_at(o.goal).is_some_and(|p| trap.contains(&p))));
        }
        if !self.exits_open {
            // (nor anywhere she'd slide over one getting there: MAP24's pad, 38 off a goal)
            let segs = self.pads.1.clone();
            options.retain(|o| segs.iter().all(|&(a, b)| segment_distance(a, b, o.goal, o.goal) >= EXIT_MARGIN));
        }
        // (near as she'd walk it, not as the crow flies: the user's, E2M2, a wall 224 off as the
        // crow flies and 4287 along the way, taken first; a tenth of the objectives with ways four
        // times their straight line or more, a quarter of all the walking)
        let travel = self.travel(obs);
        let mut weighed: Vec<(f64, Objective)> = options.into_iter().map(|o| (travel(o.goal) * o.kind.weight() * hooks.weigh(obs, &o), o)).collect();
        weighed.sort_by(|a, b| (a.1.priority(), a.0).partial_cmp(&(b.1.priority(), b.0)).unwrap_or(std::cmp::Ordering::Equal));
        if self.debug_tics.is_some_and(|(a, b)| (a..=b).contains(&obs.tic)) {
            for (d, o) in &weighed {
                self.log.push(format!("  (ranked) '{}' weighed {d:.0}, priority {}, needed {}, urgent {}", o.label, o.priority(), o.needed, o.urgent));
            }
        }
        weighed.into_iter().map(|(_, o)| o).collect()
    }
}

/// a point her reach gets to by no walk the travel field knows (a teleporter, a jump): counted
/// this many times its straight line
const TRAVEL_UNKNOWN: f64 = 2.0;
