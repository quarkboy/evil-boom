// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What she's told as it happens: the level's events, what's newly in reach, the moments worth
//! a word, each put on the list to send.

use super::*;

impl Director {
    /// What's new since last told: an exit, things of note seen, monsters, keys and weapons got,
    /// health running low, death.
    pub(super) fn events(&mut self, agent: &mut Agent, obs: &Observation) {
        let pos = obs.pos();
        let health = obs.health.unwrap_or(100.0);
        if health <= 0.0 {
            if !self.told_dead {
                self.told_dead = true;
                let story = self.death_story();
                self.tell(obs, Event::Died, story);
            }
            return;
        }
        // (a lock: what the game says it needs, as it says it)
        if let Some(m) = obs
            .message
            .as_deref()
            .filter(|m| crate::knowledge::model::needs_key(m))
        {
            let again = self
                .told_lock
                .as_ref()
                .is_none_or(|(t, at)| t != m || obs.tic - at >= LOCK_AGAIN);
            if again {
                self.told_lock = Some((m.to_string(), obs.tic));
                self.tell(obs, Event::Lock, format!("{}.", m.trim_end_matches('.')));
            }
        }
        // (health falling past a mark, each told once till she's above it again)
        let mark = [10, 25, 50].into_iter().find(|&m| health < f64::from(m));
        if mark != self.health_mark {
            if mark.is_some_and(|m| self.health_mark.is_none_or(|h| m < h)) {
                self.tell(
                    obs,
                    Event::HealthLow,
                    format!("Your health is down to {health:.0}."),
                );
            }
            self.health_mark = mark;
        }
        self.moments(agent, obs);
        if let Some(keys) = obs.keys {
            let got: Vec<_> = keys
                .iter()
                .filter(|k| self.had_keys.is_some_and(|h| !h.has(*k)))
                .collect();
            for k in got {
                self.tell(obs, Event::Key, format!("You got the {}.", key_name(k)));
            }
            self.had_keys = Some(keys);
        }
        if let Some(weapons) = &obs.weapons {
            let got: Vec<i32> = weapons
                .iter()
                .copied()
                .filter(|w| self.had_weapons.as_ref().is_some_and(|h| !h.contains(w)))
                .collect();
            for w in got {
                self.tell(
                    obs,
                    Event::Weapon,
                    format!("You got the {}.", weapon_name(w, obs)),
                );
            }
            self.had_weapons = Some(weapons.clone());
        }
        if obs.tic % 5 != 0 {
            return;
        }
        // (exits, switches and doors as they come into view, mid-fight too: a switch the fight
        // swung her round to see is hers to rush to, not news for an explore order's end that a
        // fight puts off)
        let (mut exits, mut switches, mut doors) = (Vec::new(), Vec::new(), Vec::new());
        // (looked for once a second, not each time here: a listing is the explorer's whole
        // reckoning of what she can do, tens of milliseconds of it on a big level well known,
        // and the game's picture waits on it. The safe ones and the risky half a second apart,
        // so that no tic has two)
        let listed = match obs.tic % NEWS_EVERY {
            0 => Some(false),
            NEWS_RISKY_AT => Some(true),
            _ => None,
        };
        // (...and only when she knows of another switch, door or exit than at the last, or
        // holds another key, or it's been a while: those are what there is to tell, and the
        // knowledge has them without the reckoning (the lines mapped that can be worked,
        // the exits). End to end, E2M2 in a window: the two a second were most of the frames
        // that came late, 16 ms each as a rule and up to 60)
        let listed = listed.filter(|&risky| {
            let k = &agent.x().knowledge;
            let now = (k.workable(), obs.keys.unwrap_or_default().iter().count());
            let last = &mut self.news_listed[usize::from(risky)];
            let due = last.0 != now || obs.tic - last.1 >= NEWS_ANYWAY;
            if due {
                *last = (now, obs.tic);
            }
            due
        });
        let open = listed.map_or_else(Vec::new, |risky| self.open_half(agent, obs, risky));
        for e in entries_of(&open, &[Kind::Exit, Kind::Switch, Kind::Door]) {
            if !self.told_exits.insert(e.id.clone()) {
                continue;
            }
            let line = self.describe(&e, obs);
            if e.id.starts_with("exit-") {
                exits.push(line);
            } else if e.id.starts_with("switch-") {
                switches.push(line);
            } else {
                doors.push(line);
            }
        }
        if !exits.is_empty() {
            self.tell(
                obs,
                Event::ExitFound,
                format!(
                    "Exit{} found: {}.",
                    if exits.len() == 1 { "" } else { "s" },
                    exits.join(", ")
                ),
            );
        }
        if !switches.is_empty() {
            self.tell(
                obs,
                Event::SwitchFound,
                format!(
                    "Switch{} found: {}.",
                    if switches.len() == 1 { "" } else { "es" },
                    switches.join(", ")
                ),
            );
        }
        if !doors.is_empty() {
            self.tell(
                obs,
                Event::DoorFound,
                format!(
                    "Door{} found: {}.",
                    if doors.len() == 1 { "" } else { "s" },
                    doors.join(", ")
                ),
            );
        }
        let known = agent.x().knowledge.known_polys();
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let held = obs.weapons.clone().unwrap_or_default();
        let mut seen = Vec::new();
        for t in obs.things.iter().flatten() {
            let notable = match t.category {
                "Key" | "Powerup" => true,
                "Weapon" => {
                    crate::bestiary::weapon_item(t.name).is_some_and(|w| !held.contains(&w))
                }
                "Health" | "Armor" => matches!(
                    t.name,
                    "Soulsphere" | "Megasphere" | "BlueArmor" | "GreenArmor" | "Berserk"
                ),
                _ => false,
            };
            if notable
                && !self.told_items.contains(&t.id)
                && nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p))
            {
                self.told_items.insert(t.id);
                let id = format!(
                    "{}-t{}",
                    if t.category == "Key" { "key" } else { "item" },
                    t.id
                );
                seen.push(self.describe(
                    &Entry {
                        id,
                        what: called(t.name).to_string(),
                        at: (t.x, t.y),
                    },
                    obs,
                ));
            }
        }
        if !seen.is_empty() {
            self.tell(
                obs,
                Event::Spotted,
                format!("Spotted: {}.", seen.join("; ")),
            );
        }
        // (by what the listing found she can get to: with it)
        if listed == Some(false) {
            self.newly_reachable(agent, obs);
        }
        // (monsters seen for the first time, together: how many of each, the nearest; worth a
        // reaction when one's dangerous)
        let new: Vec<(i32, &'static str, Point)> = agent
            .player()
            .map(|p| p.known_monsters())
            .unwrap_or_default()
            .into_iter()
            .filter(|m| !self.told_monsters.contains(&m.0))
            .map(|(id, name, at, _, _)| (id, name, at))
            .collect();
        if !new.is_empty() {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for (id, name, _) in &new {
                self.told_monsters.insert(*id);
                *counts.entry(name).or_insert(0) += 1;
            }
            let nearest = new
                .iter()
                .map(|m| m.2)
                .min_by(|a, b| dist(pos, *a).total_cmp(&dist(pos, *b)))
                .unwrap_or(pos);
            let what: Vec<String> = counts.iter().map(|(n, c)| some(*c, n)).collect();
            let dangerous = new
                .iter()
                .any(|m| crate::bestiary::monster(m.1).is_some_and(|i| i.boss || i.threat >= 6.0));
            let text = format!(
                "Monsters: {} ({} {:.0}).",
                what.join(", "),
                compass(heading(pos, nearest)),
                dist(pos, nearest)
            );
            // (a heavy one first seen is her life in danger: never kept quiet by the budget)
            self.tell(obs, Event::Sighting { dangerous }, text);
        }
    }

    /// Items she's seen and couldn't get to that she now can (a floor gone down to them, a lift,
    /// a door: whatever opened the way, seen), told: weapons on platforms a switch lowers are
    /// hers to hear can be had. Notable if any's worth a decision (a gun she hasn't, a powerup,
    /// armor, a key).
    pub(super) fn newly_reachable(&mut self, agent: &Agent, obs: &Observation) {
        let x = agent.x();
        let known = x.knowledge.known_polys();
        let held = obs.weapons.clone().unwrap_or_default();
        let mut now = Vec::new();
        let mut notable = false;
        for t in obs.things.iter().flatten().filter(|t| {
            matches!(
                t.category,
                "Weapon" | "Ammo" | "Health" | "Armor" | "Key" | "Powerup"
            )
        }) {
            if !x
                .nav
                .poly_at((t.x, t.y))
                .is_some_and(|p| known.contains(&p))
            {
                continue;
            }
            let reachable = x.touch_spot((t.x, t.y), t.z, &x.reach).is_some();
            if !reachable {
                // (each told once: a door that shuts again would tell it again and again)
                if !self.told_reachable.contains(&t.id) {
                    self.out_of_reach.insert(t.id);
                }
            } else if self.out_of_reach.remove(&t.id) {
                self.told_reachable.insert(t.id);
                let id = format!(
                    "{}-t{}",
                    if t.category == "Key" { "key" } else { "item" },
                    t.id
                );
                notable |= match t.category {
                    "Weapon" => {
                        crate::bestiary::weapon_item(t.name).is_some_and(|w| !held.contains(&w))
                    }
                    "Key" | "Powerup" | "Armor" => true,
                    _ => matches!(t.name, "Soulsphere" | "Megasphere" | "Berserk"),
                };
                now.push(self.describe(
                    &Entry {
                        id,
                        what: called(t.name).to_string(),
                        at: (t.x, t.y),
                    },
                    obs,
                ));
            }
        }
        if !now.is_empty() {
            self.tell(
                obs,
                Event::Reachable { notable },
                format!("Now reachable: {}.", now.join("; ")),
            );
        }
    }

    /// The things worth a word as they happen: a secret found; health going fast, while there's
    /// time to do something about it; a fight over, with what it cost and what it killed.
    pub(super) fn moments(&mut self, agent: &Agent, obs: &Observation) {
        let health = obs.health.unwrap_or(100.0);
        let armor = obs.armor.unwrap_or(0.0);
        let pool = |slot: i32| {
            obs.ammo
                .iter()
                .flatten()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n)
        };
        let ammo = POOLS.map(|(slot, _)| pool(slot));
        let secrets = self.tally.secrets.0;
        if self.told_secrets.is_some_and(|n| secrets > n) {
            self.tell(
                obs,
                Event::Secret,
                format!("You found a secret ({secrets}/{}).", self.tally.secrets.1),
            );
        }
        self.told_secrets = Some(secrets);
        let against = engaged(agent, obs);
        // (a fight: from the first thing fighting her till nothing has for a moment)
        if !against.is_empty() && self.fight.is_none() {
            self.fight = Some(Fight {
                since: obs.tic,
                calm_since: None,
                kills_at_start: self.tally.kills.0,
                health_lost: 0.0,
                armor_lost: 0.0,
                spent: [0; 4],
                against: BTreeMap::new(),
                asked: false,
                held: None,
                held_told: Vec::new(),
            });
        }
        if let (Some(f), Some(was)) = (self.fight.as_mut(), self.was.as_ref()) {
            f.health_lost += (was.health - health).max(0.0);
            f.armor_lost += (was.armor - armor).max(0.0);
            for (spent, (now, before)) in f.spent.iter_mut().zip(ammo.iter().zip(was.ammo)) {
                *spent += (before - now).max(0);
            }
            let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
            for t in &against {
                *counts.entry(t.name).or_insert(0) += 1;
            }
            for (name, n) in counts {
                let most = f.against.entry(name).or_insert(0);
                *most = (*most).max(n);
            }
            f.calm_since = if against.is_empty() {
                f.calm_since.or(Some(obs.tic))
            } else {
                None
            };
        }
        self.was = Some(Had {
            health,
            armor,
            ammo,
        });
        // (health going fast: told with what's doing it, once a while)
        self.health_lately.push_back((obs.tic, health));
        while self
            .health_lately
            .front()
            .is_some_and(|&(t, _)| obs.tic - t > HURT_WINDOW)
        {
            self.health_lately.pop_front();
        }
        let top = self
            .health_lately
            .iter()
            .map(|&(_, h)| h)
            .fold(health, f64::max);
        if top - health >= HURT_FAST && self.told_hurt.is_none_or(|t| obs.tic - t >= HURT_AGAIN) {
            self.told_hurt = Some(obs.tic);
            let by = fighting(&against, obs).map_or(String::new(), |w| format!(" to {w}"));
            self.tell(
                obs,
                Event::HurtFast,
                format!(
                    "You're losing health fast: {:.0} in {} s{by}. Health {health:.0}.",
                    top - health,
                    HURT_WINDOW / 35
                ),
            );
        }
        if self
            .fight
            .as_ref()
            .is_some_and(|f| f.calm_since.is_some_and(|t| obs.tic - t >= RESUME_CALM))
            && let Some(f) = self.fight.take()
        {
            let killed = self.tally.kills.0 - f.kills_at_start;
            let them: Vec<String> = f.against.iter().map(|(name, n)| some(*n, name)).collect();
            let mut cost = Vec::new();
            if f.health_lost >= 1.0 {
                cost.push(format!("{:.0} health", f.health_lost));
            }
            if f.armor_lost >= 1.0 {
                cost.push(format!("{:.0} armor", f.armor_lost));
            }
            for (n, (_, what)) in f.spent.iter().zip(POOLS) {
                if *n > 0 {
                    cost.push(format!("{n} {what}"));
                }
            }
            let notable = f.health_lost >= FIGHT_NOTABLE_HEALTH || killed >= FIGHT_NOTABLE_KILLS;
            // (one that came to little isn't a fight to tell of: a monster seen and gone, a
            // zombie shot in passing)
            if killed < 2 && f.health_lost < FIGHT_WORTH_HEALTH {
                return;
            }
            self.next_due = Some("fight");
            self.tell(
                obs,
                Event::FightOver { notable },
                format!(
                    "Fight over ({} s, against {}): {killed} killed; cost {}. Health {health:.0}.",
                    (obs.tic - f.since) / 35,
                    if them.is_empty() {
                        "something unseen".to_string()
                    } else {
                        them.join(", ")
                    },
                    if cost.is_empty() {
                        "nothing".to_string()
                    } else {
                        cost.join(", ")
                    }
                ),
            );
        }
    }

    /// News for her: what it is, in words.
    pub(super) fn tell(&mut self, obs: &Observation, event: Event, text: String) {
        self.told_at(obs.tic, event, text);
    }

    /// News for her at a tic (off a level: the screens').
    pub(super) fn told_at(&mut self, tic: i64, event: Event, text: String) {
        self.reports.push(Report {
            tic,
            ok: true,
            text,
            event,
        });
    }
}
