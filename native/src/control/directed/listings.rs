// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What she can ask for: the lists by topic, a place or a thing found by what she calls it,
//! its description, the names she gives.

use super::*;

impl Director {
    pub(super) fn places(&mut self, agent: &Agent) -> &Places {
        self.places
            .get_or_insert_with(|| Places::build(&agent.x().nav))
    }

    /// A list of what she knows of, in her own order: what the explorer would do first first
    /// (an area by the best thing left to explore in it), the rest after, nearest first.
    pub fn list(&mut self, agent: &mut Agent, obs: &Observation, topic: Topic) -> Vec<Entry> {
        let pos = obs.pos();
        let open = match topic {
            Topic::Monsters => Vec::new(),
            Topic::Exits => self.open_unheld(agent, obs),
            _ => self.open_objectives(agent, obs),
        };
        let mut out: Vec<Entry> = match topic {
            Topic::Areas => {
                let walls = self.walls_to_search(agent, obs, 3);
                let known = agent.x().knowledge.known_polys();
                let nav = std::rc::Rc::clone(&agent.x().nav);
                let here = nav.poly_at(pos).map(|p| nav.sector_of_poly(p));
                let places = self.places(agent);
                let mut seen: BTreeMap<String, &Area> = BTreeMap::new();
                for p in &known {
                    let a = places.area_of(nav.sector_of_poly(*p));
                    if a.size >= SMALL {
                        seen.insert(a.id.clone(), a);
                    }
                }
                let mut done = std::collections::BTreeSet::new();
                let entries = seen
                    .values()
                    .map(|a| {
                        let inside = |o: &&Objective| {
                            nav.poly_at(o.goal)
                                .is_some_and(|p| a.sectors.contains(&nav.sector_of_poly(p)))
                        };
                        let left = open.iter().filter(|o| explores(o)).filter(inside).count();
                        let hiding = by_thoroughness(walls.iter().filter(inside));
                        let mut what = Places::sketch(a);
                        let in_it = here.is_some_and(|s| a.sectors.contains(&s));
                        // (walls a search would press as it's asked for by default, thorough 2:
                        // whether it's worth one, not how many; one of every wall, 3, there is
                        // everywhere)
                        let unsearched = hiding[1] > 0;
                        if in_it {
                            what.push_str(", here");
                        } else if left == 0 && !unsearched {
                            done.insert(a.id.clone());
                        }
                        what.push_str(&match (left, unsearched) {
                            (0, true) => ", explored, not searched for secrets".to_string(),
                            (0, false) => ", explored".to_string(),
                            (n, _) => format!(", {n} to explore"),
                        });
                        // (where a secret's likeliest: the walls even the lightest search
                        // presses, which few areas have)
                        if hiding[0] > 0 {
                            what.push_str(&format!(", {} likely secret walls", hiding[0]));
                        }
                        Entry {
                            id: a.id.clone(),
                            what,
                            at: a.center,
                        }
                    })
                    .collect();
                self.done_areas = done;
                entries
            }
            Topic::Doors => entries_of(&open, &[Kind::Door]),
            Topic::Switches => {
                let mut out = entries_of(&open, &[Kind::Switch]);
                for e in &mut out {
                    let effect =
                        e.id.strip_prefix("switch-l")
                            .and_then(|l| l.parse().ok())
                            .and_then(|l| self.effect_of(agent, obs, l));
                    let lock = std::mem::take(&mut e.what);
                    e.what = [effect.unwrap_or_else(|| "effect unknown".to_string()), lock]
                        .into_iter()
                        .filter(|w| !w.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ");
                }
                out
            }
            Topic::Exits => entries_of(&open, &[Kind::Exit]),
            Topic::Items => {
                let known = agent.x().knowledge.known_polys();
                let nav = &agent.x().nav;
                obs.things
                    .iter()
                    .flatten()
                    .filter(|t| {
                        matches!(
                            t.category,
                            "Weapon" | "Ammo" | "Health" | "Armor" | "Key" | "Powerup"
                        )
                    })
                    .filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
                    .map(|t| Entry {
                        id: format!(
                            "{}-t{}",
                            if t.category == "Key" { "key" } else { "item" },
                            t.id
                        ),
                        what: if crate::knowledge::completion::Completionist::counted(t.name, obs) {
                            format!("{}{COUNTS}", called(t.name))
                        } else {
                            called(t.name).to_string()
                        },
                        at: (t.x, t.y),
                    })
                    .collect()
            }
            Topic::Monsters => agent
                .player()
                .map(|p| p.known_monsters())
                .unwrap_or_default()
                .into_iter()
                .map(|(id, name, at, tic, awake)| {
                    let when = (obs.tic - tic) as f64 / 35.0;
                    let seen = if when < 1.0 {
                        "in sight".to_string()
                    } else {
                        format!("seen {when:.0}s ago")
                    };
                    Entry {
                        id: format!("monster-{id}"),
                        what: format!(
                            "{}, {seen}{}",
                            called(name),
                            if awake { "" } else { ", asleep" }
                        ),
                        at,
                    }
                })
                .collect(),
        };
        let rank = self.ranks(agent, &open);
        let place = |e: &Entry| rank.get(&e.id).copied().unwrap_or(usize::MAX);
        out.sort_by(|a, b| {
            place(a)
                .cmp(&place(b))
                .then(dist(pos, a.at).total_cmp(&dist(pos, b.at)))
        });
        out
    }

    /// Where each thing listable stands in the explorer's order (`open`, best first): a door, a
    /// switch, an item by its own objective; an area by the first thing to explore in it.
    pub(super) fn ranks(&mut self, agent: &Agent, open: &[Objective]) -> BTreeMap<String, usize> {
        let mut rank = BTreeMap::new();
        for (i, o) in open.iter().enumerate() {
            rank.entry(id_of(o)).or_insert(i);
            if explores(o)
                && let Some(area) = self.area_at(agent, o.goal)
            {
                rank.entry(area).or_insert(i);
            }
        }
        rank
    }

    /// The area a spot is in, by id.
    pub(super) fn area_at(&mut self, agent: &Agent, p: Point) -> Option<String> {
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let sector = nav.sector_of_poly(nav.poly_at(p)?);
        Some(self.places(agent).area_of(sector).id.clone())
    }

    /// A list as she's shown it: areas with nothing more to explore left out (unless `all`), but
    /// where she is and those she's named kept; and how many were left out.
    pub fn brief(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        topic: Topic,
        all: bool,
    ) -> (Vec<Entry>, usize) {
        let entries = self.list(agent, obs, topic);
        if all || topic != Topic::Areas {
            return (entries, 0);
        }
        let n = entries.len();
        let kept: Vec<Entry> = entries
            .into_iter()
            .filter(|e| !self.done_areas.contains(&e.id) || self.names.contains_key(&e.id))
            .collect();
        let hidden = n - kept.len();
        (kept, hidden)
    }

    /// What a reference (an id or one of her names) is, among everything listable now.
    pub fn find(&mut self, agent: &mut Agent, obs: &Observation, r: &str) -> Option<Entry> {
        let id = self.resolve(r);
        [
            Topic::Areas,
            Topic::Doors,
            Topic::Switches,
            Topic::Items,
            Topic::Exits,
            Topic::Monsters,
        ]
        .into_iter()
        .find_map(|t| self.list(agent, obs, t).into_iter().find(|e| e.id == id))
        .or_else(|| {
            // (or one of the explorer's spots, as suggested: somewhere to look again, a noise)
            self.open_objectives(agent, obs)
                .into_iter()
                .find(|o| id_of(o) == id)
                .map(|o| Entry {
                    id: id.clone(),
                    what: look::why(&o),
                    at: o.goal,
                })
        })
    }

    /// Is something listed under that id or name an area?
    pub fn is_area(&mut self, agent: &Agent, r: &str) -> bool {
        let id = self.resolve(r);
        self.places(agent).by_id(&id).is_some()
    }

    /// One line for an entry: its id, her name for it, what it is, which way and how far.
    pub fn describe(&self, e: &Entry, obs: &Observation) -> String {
        let pos = obs.pos();
        let d = dist(pos, e.at);
        let name = self
            .names
            .get(&e.id)
            .map_or(String::new(), |n| format!(" \"{n}\""));
        let way = if d < 64.0 {
            "here".to_string()
        } else {
            format!("{} {:.0}", compass(heading(pos, e.at)), d)
        };
        let what = if e.what.is_empty() {
            String::new()
        } else {
            format!(": {}", e.what)
        };
        format!("{}{name}{what} ({way})", e.id)
    }

    /// Something by id in a report: the id, her name for it, a thing's kind (no more: she has the
    /// list for the rest).
    pub(super) fn tag(&self, id: &str, obs: &Observation) -> String {
        let name = self
            .names
            .get(id)
            .map_or(String::new(), |n| format!(" \"{n}\""));
        let thing = id
            .split_once("-t")
            .and_then(|(_, n)| n.parse::<i32>().ok())
            .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n))
            .map_or(String::new(), |t| format!(" ({})", called(t.name)));
        format!("{id}{name}{thing}")
    }

    /// The id a reference means: one of her names, else itself.
    pub fn resolve(&self, r: &str) -> String {
        let r = r.trim();
        self.names
            .iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(r))
            .map_or_else(|| r.to_string(), |(id, _)| id.clone())
    }

    /// Her name for an id; refused (with why) when too long, empty, or used for something else.
    pub fn name(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a name can't be empty".into());
        }
        if name.chars().count() > NAME_MAX {
            return Err(format!(
                "\"{name}\" is too long: {NAME_MAX} characters at most"
            ));
        }
        if let Some((other, _)) = self
            .names
            .iter()
            .find(|(i, n)| n.eq_ignore_ascii_case(name) && *i != id)
        {
            return Err(format!("\"{name}\" is already the name of {other}"));
        }
        self.names.insert(id.to_string(), name.to_string());
        Ok(())
    }
}
