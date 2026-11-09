//! Orders and options for directed play.
//!
//! What a commander can talk about (`list`): areas (`places`), doors and switches not yet tried,
//! items seen, monsters remembered, exits seen; each with a stable id and a line saying what it is,
//! which way and how far. Names are the commander's own, per map.
//!
//! An order is carried out with the explorer's own objectives: going to a thing is one objective;
//! exploring an area or a way is as many as it takes (the explorer's options inside it, best
//! first: walls to see, openings to look into, doors to try, barrels to clear) till none are left.
//! When an order ends, how is reported, and she waits for the next.

use std::collections::{BTreeMap, VecDeque};

use super::places::{Area, Places, SMALL};
use crate::geom::{dist, Point};
use crate::knowledge::explorer::{Ident, Kind, Objective};
use crate::motor::{heading, norm, Observation};
use crate::play::Agent;

/// a name at most this long (refused, not cut: a name she didn't choose is one she won't use)
pub const NAME_MAX: usize = 32;

/// exploring a way: what's this far either side of it
const EXPLORE_CONE: f64 = 50.0;

/// What a list is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Areas,
    Doors,
    Items,
    Monsters,
    Exits,
}

impl Topic {
    pub fn parse(s: &str) -> Option<Topic> {
        Some(match s.to_lowercase().as_str() {
            "areas" | "places" | "rooms" => Topic::Areas,
            "doors" | "switches" => Topic::Doors,
            "items" | "keys" => Topic::Items,
            "monsters" | "enemies" => Topic::Monsters,
            "exits" | "exit" => Topic::Exits,
            _ => return None,
        })
    }
}

/// One thing in a list.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub what: String,
    pub at: Point,
}

/// What she's told to do.
#[derive(Clone, Debug)]
pub enum Order {
    /// go to (or try, or pick up) something listed, by id or name
    Go(String),
    /// explore an area (by id or name) till there's nothing more known to do in it
    ExploreArea(String),
    /// explore that way (degrees, 0 east, 90 north) till there's nothing more known that way
    ExploreWay(f64),
    /// out by an exit she knows
    Exit,
    /// stop and wait
    Stop,
}

/// What's happened, for whoever gave the orders.
#[derive(Clone, Debug)]
pub struct Report {
    pub tic: i64,
    /// an order's outcome: done (false: couldn't)
    pub ok: bool,
    pub text: String,
    /// worth a reaction (a boss sighted, a key, nearly dead), not just to know (Neuro's
    /// `silent: false`)
    pub notable: bool,
}

/// An order being carried out.
enum Doing {
    /// one objective
    Once(String),
    /// objectives inside an area, one after another
    Area(String, String),
    /// objectives that way, one after another
    Way(f64),
}

#[derive(Default)]
pub struct Director {
    queue: VecDeque<Order>,
    doing: Option<Doing>,
    /// the objectives taken for the order in hand: not taken again for it (the explorer may go on
    /// offering one it counts as done: E1M1, a wall "not seen yet" taken every tic, stood on)
    taken: std::collections::BTreeSet<Ident>,
    /// id -> her name for it (this map)
    names: BTreeMap<String, String>,
    places: Option<Places>,
    pub reports: Vec<Report>,
    /// what's been told already: exits, items, monsters (ids), the health mark passed, keys and
    /// weapons held, dead
    told_exits: std::collections::BTreeSet<String>,
    told_items: std::collections::BTreeSet<i32>,
    told_monsters: std::collections::BTreeSet<i32>,
    health_mark: Option<i32>,
    had_keys: Option<crate::specials::Keys>,
    had_weapons: Option<Vec<i32>>,
    told_dead: bool,
}

impl Director {
    pub fn new() -> Director {
        Director::default()
    }

    /// Directed play on: the explorer chooses nothing of its own from now.
    pub fn take_over(agent: &mut Agent) {
        let x = agent.x_mut();
        x.directed = true;
        x.current = None;
        x.follower = None;
    }

    pub fn order(&mut self, o: Order) {
        self.queue.push_back(o);
    }

    /// Doing nothing, waiting for an order?
    pub fn idle(&self, agent: &Agent) -> bool {
        self.queue.is_empty() && self.doing.is_none() && agent.x().current.is_none()
    }

    /// A new map: names, orders, places and what's been told are the old map's.
    pub fn new_map(&mut self) {
        self.queue.clear();
        self.doing = None;
        self.names.clear();
        self.places = None;
        self.told_exits.clear();
        self.told_items.clear();
        self.told_monsters.clear();
        self.health_mark = None;
        self.told_dead = false;
    }

    /// Her state in a line: health, armor, weapons, ammo, keys, what she's doing.
    pub fn status(&self, agent: &Agent, obs: &Observation) -> String {
        let weapons: Vec<&str> = obs.weapons.iter().flatten().map(|&w| weapon_name(w, obs)).collect();
        let ammo = |slot: i32| obs.ammo.iter().flatten().find(|(s, _)| *s == slot).map_or(0, |(_, n)| *n);
        let mut pools = Vec::new();
        for (slot, what) in [(2, "bullets"), (3, "shells"), (5, "rockets"), (6, "cells")] {
            if ammo(slot) > 0 {
                pools.push(format!("{} {what}", ammo(slot)));
            }
        }
        let keys: Vec<String> = obs.keys.unwrap_or_default().iter().map(|k| key_name(k).to_string()).collect();
        let doing = match &self.doing {
            Some(Doing::Once(w)) => format!("going: {w}"),
            Some(Doing::Area(_, w)) => format!("exploring {w}"),
            Some(Doing::Way(b)) => format!("exploring {}", compass(*b)),
            None if agent.x().current.is_some() => "busy".to_string(),
            None => "waiting for orders".to_string(),
        };
        format!(
            "Health {:.0}, armor {:.0}; weapons: {}; ammo: {}; keys: {}; {doing}",
            obs.health.unwrap_or(0.0),
            obs.armor.unwrap_or(0.0),
            if weapons.is_empty() { "none".to_string() } else { weapons.join(", ") },
            if pools.is_empty() { "none".to_string() } else { pools.join(", ") },
            if keys.is_empty() { "none".to_string() } else { keys.join(", ") }
        )
    }

    /// What's new since last told: an exit, things of note seen, monsters, keys and weapons got,
    /// health running low, death.
    fn events(&mut self, agent: &mut Agent, obs: &Observation) {
        let pos = obs.pos();
        let health = obs.health.unwrap_or(100.0);
        if health <= 0.0 {
            if !self.told_dead {
                self.told_dead = true;
                self.event(obs, true, "Died.".into());
            }
            return;
        }
        // (health falling past a mark, each told once till she's above it again)
        let mark = [25, 50].into_iter().find(|&m| health < f64::from(m));
        if mark != self.health_mark {
            if let Some(m) = mark.filter(|m| self.health_mark.is_none_or(|h| *m < h)) {
                self.event(obs, true, format!("Health down to {health:.0} (under {m})."));
            }
            self.health_mark = mark;
        }
        if let Some(keys) = obs.keys {
            let got: Vec<_> = keys.iter().filter(|k| self.had_keys.is_some_and(|h| !h.has(*k))).collect();
            for k in got {
                self.event(obs, true, format!("Got the {}.", key_name(k)));
            }
            self.had_keys = Some(keys);
        }
        if let Some(weapons) = &obs.weapons {
            let got: Vec<i32> = weapons.iter().copied().filter(|w| self.had_weapons.as_ref().is_some_and(|h| !h.contains(w))).collect();
            for w in got {
                self.event(obs, true, format!("Got the {}.", weapon_name(w, obs)));
            }
            self.had_weapons = Some(weapons.clone());
        }
        if obs.tic % 5 != 0 {
            return;
        }
        for e in self.objectives_of(agent, obs, &[Kind::Exit]) {
            if self.told_exits.insert(e.id.clone()) {
                let line = self.describe(&e, obs);
                self.event(obs, true, format!("Found an exit: {line}."));
            }
        }
        let known = agent.x().knowledge.known_polys();
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let held = obs.weapons.clone().unwrap_or_default();
        let mut seen = Vec::new();
        for t in obs.things.iter().flatten() {
            let notable = match t.category {
                "Key" | "Powerup" => true,
                "Weapon" => crate::tactical::bestiary::weapon_item(t.name).is_some_and(|w| !held.contains(&w)),
                "Health" | "Armor" => matches!(t.name, "Soulsphere" | "Megasphere" | "BlueArmor" | "GreenArmor" | "Berserk"),
                _ => false,
            };
            if notable && !self.told_items.contains(&t.id) && nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)) {
                self.told_items.insert(t.id);
                let id = format!("{}-t{}", if t.category == "Key" { "key" } else { "item" }, t.id);
                seen.push(self.describe(&Entry { id, what: t.name.to_string(), at: (t.x, t.y) }, obs));
            }
        }
        if !seen.is_empty() {
            self.event(obs, true, format!("Spotted: {}.", seen.join("; ")));
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
            let nearest = new.iter().map(|m| m.2).min_by(|a, b| dist(pos, *a).total_cmp(&dist(pos, *b))).unwrap_or(pos);
            let what: Vec<String> = counts.iter().map(|(n, c)| format!("{c} {n}")).collect();
            let dangerous = new.iter().any(|m| crate::tactical::bestiary::monster(m.1).is_some_and(|i| i.boss || i.threat >= 6.0));
            self.event(obs, dangerous, format!("Monsters sighted: {} (nearest {} {:.0}).", what.join(", "), compass(heading(pos, nearest)), dist(pos, nearest)));
        }
    }

    fn event(&mut self, obs: &Observation, notable: bool, text: String) {
        self.reports.push(Report { tic: obs.tic, ok: true, text, notable });
    }

    fn places(&mut self, agent: &Agent) -> &Places {
        self.places.get_or_insert_with(|| Places::build(&agent.x().nav))
    }

    /// A list of what she knows of, nearest first.
    pub fn list(&mut self, agent: &mut Agent, obs: &Observation, topic: Topic) -> Vec<Entry> {
        let pos = obs.pos();
        let mut out: Vec<Entry> = match topic {
            Topic::Areas => {
                let open = self.open_objectives(agent, obs);
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
                seen.values()
                    .map(|a| {
                        let left = open.iter().filter(|o| nav.poly_at(o.goal).is_some_and(|p| a.sectors.contains(&nav.sector_of_poly(p)))).count();
                        let mut what = Places::sketch(a);
                        if here.is_some_and(|s| a.sectors.contains(&s)) {
                            what.push_str(", where she is");
                        }
                        what.push_str(&if left == 0 { ", nothing more known to explore".to_string() } else { format!(", {left} things to explore") });
                        Entry { id: a.id.clone(), what, at: a.center }
                    })
                    .collect()
            }
            Topic::Doors => self.objectives_of(agent, obs, &[Kind::Door, Kind::Switch]),
            Topic::Exits => self.objectives_of(agent, obs, &[Kind::Exit]),
            Topic::Items => {
                let known = agent.x().knowledge.known_polys();
                let nav = &agent.x().nav;
                obs.things
                    .iter()
                    .flatten()
                    .filter(|t| matches!(t.category, "Weapon" | "Ammo" | "Health" | "Armor" | "Key" | "Powerup"))
                    .filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
                    .map(|t| Entry { id: format!("{}-t{}", if t.category == "Key" { "key" } else { "item" }, t.id), what: t.name.to_string(), at: (t.x, t.y) })
                    .collect()
            }
            Topic::Monsters => agent
                .player()
                .map(|p| p.known_monsters())
                .unwrap_or_default()
                .into_iter()
                .map(|(id, name, at, tic, awake)| {
                    let when = (obs.tic - tic) as f64 / 35.0;
                    let seen = if when < 1.0 { "in sight".to_string() } else { format!("last seen {when:.0}s ago") };
                    Entry { id: format!("monster-{id}"), what: format!("{name}, {seen}{}", if awake { "" } else { ", not stirred yet" }), at }
                })
                .collect(),
        };
        out.sort_by(|a, b| dist(pos, a.at).total_cmp(&dist(pos, b.at)));
        out
    }

    /// One line for an entry: its id, her name for it, what it is, which way and how far.
    pub fn describe(&self, e: &Entry, obs: &Observation) -> String {
        let pos = obs.pos();
        let d = dist(pos, e.at);
        let name = self.names.get(&e.id).map_or(String::new(), |n| format!(" \"{n}\""));
        let way = if d < 64.0 { "here".to_string() } else { format!("{} {:.0}", compass(heading(pos, e.at)), d) };
        format!("{}{name}: {} ({way})", e.id, e.what)
    }

    /// The id a reference means: one of her names, else itself.
    pub fn resolve(&self, r: &str) -> String {
        let r = r.trim();
        self.names.iter().find(|(_, n)| n.eq_ignore_ascii_case(r)).map_or_else(|| r.to_string(), |(id, _)| id.clone())
    }

    /// Her name for an id; refused (with why) when too long, empty, or used for something else.
    pub fn name(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a name can't be empty".into());
        }
        if name.chars().count() > NAME_MAX {
            return Err(format!("\"{name}\" is too long: {NAME_MAX} characters at most"));
        }
        if let Some((other, _)) = self.names.iter().find(|(i, n)| n.eq_ignore_ascii_case(name) && *i != id) {
            return Err(format!("\"{name}\" is already the name of {other}"));
        }
        self.names.insert(id.to_string(), name.to_string());
        Ok(())
    }

    /// Each tic: the order in hand carried on (its next objective, or its end reported); the next
    /// order taken up.
    pub fn tick(&mut self, agent: &mut Agent, obs: &Observation) {
        self.events(agent, obs);
        if let Some(o) = self.queue.pop_front() {
            self.start(agent, obs, o);
            return;
        }
        let ended = agent.x_mut().ended.take();
        if agent.x().current.is_some() {
            return;
        }
        match self.doing.take() {
            None => {}
            Some(Doing::Once(what)) => {
                let (ok, why) = ended.map_or((false, "stopped".to_string()), |e| (e.ok, e.why));
                self.report(obs, ok, if ok { format!("Done: {what}") } else { format!("Couldn't: {what} ({why})") });
            }
            Some(Doing::Area(id, what)) => {
                if !self.next_in(agent, obs, |d, a, o| d.in_area(a, &id, o)) {
                    self.report(obs, true, format!("Explored {what}: nothing more known to do there"));
                } else {
                    self.doing = Some(Doing::Area(id, what));
                }
            }
            Some(Doing::Way(b)) => {
                let pos = obs.pos();
                if !self.next_in(agent, obs, |_, _, o| dist(pos, o.goal) > 64.0 && norm(heading(pos, o.goal) - b).abs() < EXPLORE_CONE) {
                    self.report(obs, true, format!("Explored {}: nothing more known that way", compass(b)));
                } else {
                    self.doing = Some(Doing::Way(b));
                }
            }
        }
    }

    fn start(&mut self, agent: &mut Agent, obs: &Observation, o: Order) {
        // (a new order replaces the one in hand)
        if self.doing.take().is_some() {
            self.report(obs, true, "Dropped what she was doing (a new order)".into());
        }
        let x = agent.x_mut();
        x.current = None;
        x.follower = None;
        x.ended = None;
        self.taken.clear();
        match o {
            Order::Stop => self.report(obs, true, "Stopped; waiting".into()),
            Order::Exit => match self.objectives_of(agent, obs, &[Kind::Exit]).into_iter().next() {
                Some(e) => self.go(agent, obs, &e.id),
                None => self.report(obs, false, "No exit known yet".into()),
            },
            Order::Go(r) => {
                let id = self.resolve(&r);
                self.go(agent, obs, &id);
            }
            Order::ExploreArea(r) => {
                let id = self.resolve(&r);
                let Some(a) = self.places(agent).by_id(&id).cloned() else {
                    self.report(obs, false, format!("No area \"{r}\"; list the areas for their ids"));
                    return;
                };
                let what = self.describe(&Entry { id: a.id.clone(), what: Places::sketch(&a), at: a.center }, obs);
                if self.next_in(agent, obs, |d, a2, o| d.in_area(a2, &id, o)) {
                    self.report(obs, true, format!("Exploring {what}"));
                    self.doing = Some(Doing::Area(id, what));
                } else {
                    self.report(obs, true, format!("Nothing more known to explore in {what}"));
                }
            }
            Order::ExploreWay(b) => {
                let pos = obs.pos();
                if self.next_in(agent, obs, |_, _, o| dist(pos, o.goal) > 64.0 && norm(heading(pos, o.goal) - b).abs() < EXPLORE_CONE) {
                    self.report(obs, true, format!("Exploring {}", compass(b)));
                    self.doing = Some(Doing::Way(b));
                } else {
                    self.report(obs, false, format!("Nothing known to explore {}", compass(b)));
                }
            }
        }
    }

    /// Go to (try, pick up) something by id.
    fn go(&mut self, agent: &mut Agent, obs: &Observation, id: &str) {
        let obj = if let Some(a) = self.places(agent).by_id(id).cloned() {
            let x = agent.x();
            let at = x.stand_spot_for(a.center, &x.reach);
            Some(Objective::new(Kind::Visit, at, format!("go to {}", Places::sketch(&a))))
        } else if let Some(t) = id.split_once("-t").and_then(|(_, n)| n.parse::<i32>().ok()).and_then(|n| obs.things.iter().flatten().find(|t| t.id == n)) {
            let x = agent.x();
            x.touch_spot((t.x, t.y), &x.reach).map(|at| {
                let mut o = Objective::new(Kind::Item, at, format!("pick up {}", t.name));
                o.thing = Some(t.id);
                o
            })
        } else {
            self.open_objectives(agent, obs).into_iter().find(|o| id_of(o) == id)
        };
        let Some(obj) = obj else {
            self.report(obs, false, format!("Nothing \"{id}\" to go to now (done, gone, or not known); list for what there is"));
            return;
        };
        let what = self.describe(&Entry { id: id.to_string(), what: obj.label.clone(), at: obj.goal }, obs);
        if agent.x_mut().take(obs, obj) {
            self.report(obs, true, format!("Going: {what}"));
            self.doing = Some(Doing::Once(what));
        } else {
            self.report(obs, false, format!("No way to {what} from here"));
        }
    }

    /// The explorer's open objectives (best first, risky ones after).
    fn open_objectives(&self, agent: &mut Agent, obs: &Observation) -> Vec<Objective> {
        let mut out: Vec<Objective> = Vec::new();
        for risky in [false, true] {
            for o in agent.ranked(obs, risky) {
                if !out.iter().any(|p| id_of(p) == id_of(&o)) {
                    out.push(o);
                }
            }
        }
        out
    }

    fn objectives_of(&self, agent: &mut Agent, obs: &Observation, kinds: &[Kind]) -> Vec<Entry> {
        self.open_objectives(agent, obs).into_iter().filter(|o| kinds.contains(&o.kind)).map(|o| Entry { id: id_of(&o), what: o.label.clone(), at: o.goal }).collect()
    }

    /// Is objective o one of exploring area `id`'s: done standing in it? (Looking out of it from
    /// inside, into a doorway or at the next room's wall, is exploring it; standing in the next
    /// room looking back isn't: E1M1, "explore the room" through a door to the next room's edge.)
    fn in_area(&self, agent: &Agent, id: &str, o: &Objective) -> bool {
        let Some(places) = &self.places else { return false };
        let nav = &agent.x().nav;
        nav.poly_at(o.goal).is_some_and(|q| places.area_of(nav.sector_of_poly(q)).id == id)
    }

    /// The best of the explorer's objectives that `fits` (and an exploring one: not out by an exit,
    /// not off through a teleporter), taken up. False if there's none (or no way to any).
    fn next_in(&mut self, agent: &mut Agent, obs: &Observation, fits: impl Fn(&Self, &Agent, &Objective) -> bool) -> bool {
        let options: Vec<Objective> = self
            .open_objectives(agent, obs)
            .into_iter()
            .filter(|o| !matches!(o.kind, Kind::Exit | Kind::Teleporter | Kind::Noise | Kind::Recheck | Kind::Hunt))
            .filter(|o| !self.taken.contains(&o.ident()))
            .filter(|o| fits(self, agent, o))
            .collect();
        for o in options {
            let id = o.ident();
            if agent.x_mut().take(obs, o) {
                self.taken.insert(id);
                return true;
            }
            self.taken.insert(id); // (no way to it: not tried again for this order either)
        }
        false
    }

    /// An order's news: started, done, couldn't (worth a reaction when it's done or couldn't be).
    fn report(&mut self, obs: &Observation, ok: bool, text: String) {
        let notable = !text.starts_with("Going") && !text.starts_with("Exploring");
        self.reports.push(Report { tic: obs.tic, ok, text, notable });
    }
}

/// An objective's stable id: its kind, and what it's about (a thing, a line, else its spot).
fn id_of(o: &Objective) -> String {
    let kind = match o.kind {
        Kind::Exit => "exit",
        Kind::Key => "key",
        Kind::Switch => "switch",
        Kind::Door => "door",
        Kind::Item => "item",
        Kind::Teleporter => "teleporter",
        _ => "spot",
    };
    if let Some(t) = o.thing {
        format!("{kind}-t{t}")
    } else if let Some(l) = o.trigger.as_ref().map(|t| t.line).filter(|&l| l >= 0) {
        format!("{kind}-l{l}")
    } else if let Some(w) = o.wall {
        format!("{kind}-l{w}")
    } else {
        format!("{kind}-{:.0}x{:.0}", o.goal.0, o.goal.1)
    }
}

/// A weapon slot by name (as she has it: the super shotgun, the chainsaw).
fn weapon_name(slot: i32, obs: &Observation) -> &'static str {
    match slot {
        1 if obs.chainsaw == Some(true) => "chainsaw",
        1 => "fist",
        2 => "pistol",
        3 if obs.super_shotgun == Some(true) => "super shotgun",
        3 => "shotgun",
        4 => "chaingun",
        5 => "rocket launcher",
        6 => "plasma rifle",
        7 => "BFG",
        _ => "?",
    }
}

fn key_name(k: crate::specials::Key) -> &'static str {
    use crate::specials::Key;
    match k {
        Key::BlueCard => "blue keycard",
        Key::YellowCard => "yellow keycard",
        Key::RedCard => "red keycard",
        Key::BlueSkull => "blue skull key",
        Key::YellowSkull => "yellow skull key",
        Key::RedSkull => "red skull key",
    }
}

/// A heading (degrees, 0 east, 90 north) as a compass point.
pub fn compass(h: f64) -> &'static str {
    const POINTS: [&str; 8] = ["east", "north-east", "north", "north-west", "west", "south-west", "south", "south-east"];
    POINTS[((h.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

/// A compass point (n, ne, east, south-west, ...) as a heading.
pub fn bearing(s: &str) -> Option<f64> {
    let s = s.to_lowercase().replace(['-', ' ', '_'], "");
    Some(match s.as_str() {
        "e" | "east" => 0.0,
        "ne" | "northeast" => 45.0,
        "n" | "north" => 90.0,
        "nw" | "northwest" => 135.0,
        "w" | "west" => 180.0,
        "sw" | "southwest" => 225.0,
        "s" | "south" => 270.0,
        "se" | "southeast" => 315.0,
        _ => return None,
    })
}
