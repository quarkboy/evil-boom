//! Updating visible geometry, hearing movers and feeling damaging floors.

use super::{
    segment_point_distance, Heard, How, Knowledge, EXPECTED_FOR, EXPECTED_RADIUS, HEAR_BEARING, HEAR_DISTANCE, HEAR_RANGE, NEAR_SOUND, TELEPORT_JUMP,
    TELEPORT_NEAR,
};
use crate::geom::dist;
use crate::motor::{Controls, Observation};
use crate::nav::Heights;
use crate::sight::View as SightView;
use crate::specials::Category;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

/// unhurt on a floor this long: it doesn't hurt
const FELT_TICS: i64 = 70;

/// a hurting floor hurts on every 32nd tic of the level's clock...
const FLOOR_PERIOD: i64 = 32;

/// ...so hurt twice on it a multiple of that apart, this soon: it's the floor
const FLOOR_CONFIRM: i64 = 3 * FLOOR_PERIOD;

/// a radiation suit lasts a minute
const SUIT_TICS: i64 = 60 * 35;

/// sounds this close together from the same way are one
const TICS_SAME_SOUND: i64 = 4;

/// floors that look like they'd hurt (liquids): every frame of their animations, as the game
/// cycles them (SLIME09-12 one animation: MAP15's pit floored with SLIME12, taken for dry ground)
const HAZARD_FLATS: [&str; 7] = ["NUKAGE", "LAVA", "BLOOD", "SLIME0", "SLIME10", "SLIME11", "SLIME12"];

impl Knowledge {
    fn see_subsector(&mut self, ss: usize) {
        let nav = Rc::clone(&self.nav);
        self.seen_polys.extend(nav.mesh.subsector_polys(ss));
    }

    pub(super) fn map_line(&mut self, li: usize) {
        self.mapped.insert(li);
        let subs: Vec<usize> = self.line_subsectors.get(&li).into_iter().flatten().copied().collect();
        for ss in subs {
            self.see_subsector(ss);
        }
        let (f, b) = self.nav.map.sides(li);
        for s in [f, b].into_iter().flatten() {
            if self.sectors.insert(s) {
                let sec = &self.nav.map.sectors[s];
                self.seen_heights.entry(s).or_insert(Heights { floor: sec.floor, ceiling: sec.ceiling });
            }
        }
    }

    /// Take in a tic: what's in view now, what got set off (`controls`: what she did on the tic
    /// that led here).
    pub fn update(&mut self, obs: &Observation, controls: Option<&Controls>) {
        let nav = Rc::clone(&self.nav);
        let m = &nav.map;
        let heights: Vec<(f64, f64)> = obs.heights.iter().map(|h| (h.floor, h.ceiling)).collect();
        let seen = self.sight.view(&SightView { x: obs.x, y: obs.y, angle: obs.angle, width: self.width, heights: &heights });
        for &ss in &seen.subsectors {
            self.see_subsector(ss);
        }
        let mut in_view: BTreeSet<usize> = BTreeSet::new();
        for &li in &seen.through {
            // (not on the automap, but the floor across it is in view)
            let (f, b) = m.sides(li);
            for s in [f, b].into_iter().flatten() {
                in_view.insert(s);
                self.sectors.insert(s);
            }
        }
        for &li in &seen.drawn {
            let (f, b) = m.sides(li);
            in_view.extend([f, b].into_iter().flatten());
            if !self.mapped.contains(&li) {
                self.map_line(li);
            }
        }
        if let Some(poly) = nav.mesh.poly_at(obs.pos()) {
            let sector = nav.sector_of_poly(poly);
            self.visited.insert(poly);
            let on_floor = obs.z.is_none_or(|z| z <= obs.heights[sector].floor + 1.0);
            self.feel(if on_floor { Some(sector) } else { None }, obs);
            in_view.insert(sector);
            self.sectors.insert(sector);
        }
        if let Some(last) = &self.last {
            if let (Some(now), Some(before)) = (obs.suits, last.suits) {
                if now > before {
                    self.suit_until = Some(obs.tic + SUIT_TICS);
                    self.events.push(format!("tic {}: radiation suit on", obs.tic));
                }
            }
        }
        if let Some(last) = self.last.take() {
            if dist(last.pos(), obs.pos()) > TELEPORT_JUMP {
                self.teleported(&last, obs);
            } else {
                // (the lines her middle went over)
                let (a, b) = (last.pos(), obs.pos());
                let near = crate::geom::cells_around(a, b, 1.0).filter_map(|c| nav.mesh.line_grid.get(&c)).flatten();
                let over: Vec<usize> = near.copied().filter(|&li| {
                    let (p, q) = m.line_seg(li);
                    crate::geom::segments_cross(a, b, p, q)
                }).collect();
                self.crossed.extend(over);
                self.detect_fired(&last, obs, controls);
            }
            self.last = Some(last);
        }
        for &s in &in_view {
            self.seen_tic.insert(s, obs.tic);
            let now = obs.heights[s];
            self.first_heights.entry(s).or_insert(now);
            if self.seen_heights.get(&s).is_some_and(|&h| h != now) {
                self.saw_move(s, obs.tic, now);
            }
            self.seen_heights.insert(s, now);
        }
        self.hear(obs, &in_view);
        self.took(obs.tic);
        self.check_doubts(obs.tic);
        self.last = Some(obs.clone());
    }

    /// This tic's sounds, as the engine plays them: a floor or ceiling makes one from its
    /// sector's sound origin as it starts moving and as it stops, heard anywhere within
    /// HEAR_RANGE (through walls), with which way it came from and roughly how far.
    fn hear(&mut self, obs: &Observation, in_view: &BTreeSet<usize>) {
        let first = self.live.is_empty();
        for (s, &h) in obs.heights.iter().enumerate() {
            let prev = if first { None } else { self.live.get(s).copied() };
            if self.live.len() <= s {
                self.live.push(h);
            } else {
                self.live[s] = h;
            }
            let Some(prev) = prev else { continue };
            let moving = prev != h;
            if moving == self.moving.contains(&s) {
                continue;
            }
            if moving {
                self.moving.insert(s);
                self.moved_at.insert(s, obs.tic);
            } else {
                self.moving.remove(&s);
                self.stopped_at.insert(s, obs.tic);
            }
            let Some(&origin) = self.sound_origin.get(&s) else { continue };
            let d = dist(obs.pos(), origin);
            if d > HEAR_RANGE {
                continue;
            }
            // (+ 0.0: Python's round() gives an int, never -0)
            let bearing = ((origin.1 - obs.y).atan2(origin.0 - obs.x).to_degrees() / HEAR_BEARING).round_ties_even() * HEAR_BEARING + 0.0;
            let distance = HEAR_DISTANCE.max((d / HEAR_DISTANCE).round_ties_even() * HEAR_DISTANCE) + 0.0;
            let n = self.heard.len();
            if self.heard[n.saturating_sub(8)..].iter().any(|hd| hd.tic >= obs.tic - TICS_SAME_SOUND && hd.bearing == bearing && hd.distance == distance) {
                continue; // (one sound, several sectors: a door's two halves, stairs)
            }
            let est = (obs.x + distance * bearing.to_radians().cos(), obs.y + distance * bearing.to_radians().sin());
            let seen = in_view.contains(&s);
            self.heard.push(Heard { tic: obs.tic, bearing, distance, origin: est, seen });
            if !seen {
                self.events.push(format!("tic {}: heard something move, bearing {bearing:.0}, about {distance:.0} away", obs.tic));
                if moving {
                    self.heard_move(s, obs.tic);
                }
            }
        }
    }

    /// She moved further in a tic than anyone can walk: a teleport, the one nearest where she was.
    fn teleported(&mut self, last: &Observation, obs: &Observation) {
        let m = Arc::clone(&self.nav.map);
        let mut best: Option<(f64, usize)> = None;
        for (&li, info) in &self.info {
            if info.category != Category::Teleport || !info.players {
                continue;
            }
            let (a, b) = m.line_seg(li);
            let d = segment_point_distance(a, b, last.pos());
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, li));
            }
        }
        let Some((_, li)) = best.filter(|(d, _)| *d <= TELEPORT_NEAR) else {
            self.events.push(format!("tic {}: moved to ({:.0}, {:.0}) all at once", obs.tic, obs.x, obs.y));
            return;
        };
        self.fire(li, obs.tic, How::Walk);
        if !self.teleports.contains_key(&li) {
            self.events.push(format!("tic {}: teleport line {li} goes to ({:.0}, {:.0})", obs.tic, obs.x, obs.y));
        }
        // (the rest of its pad goes there too)
        for other in self.teleport_pad(li) {
            self.teleports.insert(other, obs.pos());
        }
    }

    pub(super) fn sector_at(&self, obs: &Observation) -> usize {
        self.nav.mesh.poly_at(obs.pos()).map_or(0, |p| self.nav.sector_of_poly(p))
    }

    /// What standing on sector s's floor (None: not on a floor) tells her about it.
    fn feel(&mut self, s: Option<usize>, obs: &Observation) {
        let Some(s) = s else {
            self.standing = None;
            return;
        };
        if !self.mortal || self.immune {
            if !self.felt.contains_key(&s) {
                self.learn_floor(s, self.nav.damage[s] != 0, obs.tic);
            }
            return;
        }
        if self.standing.is_none_or(|(x, _)| x != s) {
            self.standing = Some((s, obs.tic));
        }
        let lost = self.last.as_ref().and_then(|last| match (obs.health, last.health) {
            (Some(now), Some(before)) if now < before => Some(before - now),
            _ => None,
        });
        if let Some(lost) = lost {
            // (a floor hurts on the level's clock, every 32 tics, and the same each time: hurt
            // again on the same floor a multiple of that later, by as much, it's the floor; once
            // alone may be a monster's shot, and so may two: E1M7 UV, a zombieman's 15 and a
            // shotgunner's 9 on a plain floor 96 tics apart, taken for the floor, and the room
            // the whole level goes through refused)
            if let Some((ps, pt, pl)) = self.hurt_on {
                let gap = obs.tic - pt;
                if ps == s && 0 < gap && gap <= FLOOR_CONFIRM && gap % FLOOR_PERIOD == 0 && (lost - pl).abs() < 0.5 {
                    self.learn_floor(s, true, obs.tic);
                }
            }
            self.hurt_on = Some((s, obs.tic, lost));
        } else {
            // (only time on it unprotected counts: under a suit no floor hurts, so the clock
            // starts when the suit runs out, not when she stepped on)
            let since = self.standing.map_or(obs.tic, |x| x.1)
                .max(self.hurt_on.filter(|h| h.0 == s).map_or(0, |h| h.1))
                .max(self.suit_until.unwrap_or(0));
            if obs.tic - since > FELT_TICS && self.felt.get(&s) != Some(&false) {
                self.learn_floor(s, false, obs.tic);
            }
        }
    }

    fn learn_floor(&mut self, s: usize, hurts: bool, tic: i64) {
        if self.felt.get(&s) == Some(&hurts) {
            return;
        }
        self.felt.insert(s, hurts);
        if hurts != self.looks_damaging(s) {
            self.events.push(format!("tic {tic}: sector {s} {}", if hurts { "hurts after all" } else { "doesn't hurt after all" }));
        }
    }

    /// Is she wearing a radiation suit (one picked up in the last minute)?
    pub fn protected(&self, tic: i64) -> bool {
        self.suit_until.is_some_and(|u| tic < u)
    }

    /// A floor that looks like it'd hurt (its flat is a liquid).
    pub fn looks_damaging(&self, s: usize) -> bool {
        let looks = self.looks.get_or_init(|| {
            self.nav
                .map
                .sectors
                .iter()
                .map(|sec| {
                    let tex = sec.floor_tex.to_uppercase();
                    HAZARD_FLATS.iter().any(|h| tex.starts_with(h))
                })
                .collect()
        });
        looks[s]
    }

    /// The floors that would hurt her now, as she takes them (`damaging`), by sector: none under
    /// a radiation suit. For the fight's moves: where it doesn't back off to or dodge onto.
    pub fn hazards(&self, tic: i64) -> Vec<bool> {
        let suited = self.protected(tic);
        (0..self.nav.map.sectors.len()).map(|s| !suited && self.damaging(s)).collect()
    }

    /// Does she take sector s's floor to hurt: as found out by standing on it, else by its look.
    pub fn damaging(&self, s: usize) -> bool {
        if let Some(&hit) = self.changed_floor.get(&s) {
            return hit != 0; // (seen changed since: MAP08's walkway gone to nukage)
        }
        self.felt.get(&s).copied().unwrap_or_else(|| self.looks_damaging(s))
    }

    /// What a hit from sector s's floor would do, as she'd reckon it: what the map says, but for
    /// one it doesn't (one that only looks like it hurts) the most a floor of the same look has
    /// done to her, `worst` if she's felt none. (E4M1 HMP: a door pressed from a nukage trench
    /// that doesn't hurt, beside a pit of the same nukage she'd waded at 5 a hit, reckoned at 20
    /// a hit, 27 at 37 health: refused, nothing else to do for the hour.)
    /// One seen changed to a floor that hurts: its model's hit, as the map says. (E3M7 UV: the
    /// blue key's alcove ringed by a floor gone down into the blood beside it, 10 a hit, reckoned
    /// at 20 by its old look.)
    pub fn floor_hit(&self, s: usize, worst: i32) -> i32 {
        if let Some(&hit) = self.changed_floor.get(&s).filter(|&&hit| hit > 0) {
            return hit;
        }
        let said = self.nav.damage[s];
        if said > 0 {
            return said;
        }
        let tex = &self.nav.map.sectors[s].floor_tex;
        let sectors = &self.nav.map.sectors;
        self.felt
            .iter()
            .filter(|&(&f, &hurt)| hurt && sectors[f].floor_tex.eq_ignore_ascii_case(tex))
            .map(|(&f, _)| self.nav.damage[f])
            .filter(|&d| d > 0)
            .max()
            .unwrap_or(worst)
    }

    /// A sound from something she knows moves and set off a moment before (a door closing behind
    /// her, a lift coming back), or from right beside her: nothing new. One she set off long
    /// before moving again is news (E1M6's door that shut on her opened again 30 s later).
    pub fn expected(&self, h: &Heard) -> bool {
        if h.distance <= NEAR_SOUND {
            return true;
        }
        let mut fired_at: BTreeMap<usize, i64> = BTreeMap::new();
        for f in &self.fired {
            fired_at.insert(f.line, f.tic);
        }
        for (t, sectors) in self.known_triggers() {
            let Some(&tic) = t.linedef().and_then(|li| fired_at.get(&li)) else { continue };
            if !(0..=EXPECTED_FOR).contains(&(h.tic - tic)) {
                continue;
            }
            if sectors.iter().filter_map(|s| self.sound_origin.get(s)).any(|&o| dist(o, h.origin) <= EXPECTED_RADIUS) {
                return true;
            }
        }
        false
    }
}
