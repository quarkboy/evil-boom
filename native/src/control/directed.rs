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

/// a switch pressed: watched this long (tics) for what it did
const PRESS_WATCH: i64 = 3 * 35;

/// waiting on a floor that hurts: off it tried this often (tics)...
const OFF_AGAIN: i64 = 35;
/// how long nothing's fought her before a paused explore order goes on (tics)
const RESUME_CALM: i64 = 2 * 35;
/// how far (along the way there) she goes for a pickup by herself, waiting
const PICKUP_RANGE: f64 = 1024.0;
/// the nearest pickups tried for a way there, waiting
const PICKUP_TRIES: usize = 4;
/// an item listed that counts for the level's items tally (the rest are just pickups)
const COUNTS: &str = " (tally)";
/// ...to the best of this many places
const OFF_TRIES: usize = 6;

/// What a list is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Areas,
    Doors,
    Switches,
    Items,
    Monsters,
    Exits,
}

impl Topic {
    pub fn parse(s: &str) -> Option<Topic> {
        Some(match s.to_lowercase().as_str() {
            "areas" | "places" | "rooms" => Topic::Areas,
            "doors" => Topic::Doors,
            "switches" | "buttons" => Topic::Switches,
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
    /// go to (or try, or pick up) something listed, by id or name; risky: whatever the bot
    /// would rather not cross on its own (a floor that hurts), if it's a way there that won't kill
    /// her
    Go(String, bool),
    /// explore an area (by id or name) till there's nothing more known to do in it; and search
    /// its walls for secrets this thoroughly (0 not, 3 every suspicious one: slow, so hers to ask
    /// for)
    ExploreArea(String, u8),
    /// explore that way (degrees, 0 east, 90 north) till there's nothing more known that way;
    /// the search as for an area
    ExploreWay(f64, u8),
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
    /// one objective: its id, what it's called
    Once(String, String),
    /// objectives inside an area, one after another (and its walls searched, that thoroughly)
    Area(String, String, u8),
    /// objectives that way, one after another (the search as for an area)
    Way(f64, u8),
    /// off a floor that hurts, her own doing (not news)
    Off,
    /// a pickup she takes by herself, waiting (not news)
    Pickup,
    /// no order for a while (`idle_auto`): carrying on by herself, the explorer choosing, till an
    /// order comes or there's nothing left but to leave
    Auto,
}

/// A fight under way: since when, what it's cost so far, what's been in it.
struct Fight {
    since: i64,
    /// when nothing was last fighting her (None: something is)
    calm_since: Option<i64>,
    kills_at_start: i32,
    health_lost: f64,
    armor_lost: f64,
    /// rounds gone, by pool: bullets, shells, rockets, cells
    spent: [i32; 4],
    /// the most of each kind fighting her at once
    against: BTreeMap<&'static str, usize>,
}

/// What she had a tic ago.
struct Had {
    health: f64,
    armor: f64,
    ammo: [i32; 4],
}

/// the ammo pools, by a gun's slot that uses each, and what its rounds are called
const POOLS: [(i32, &str); 4] = [(2, "bullets"), (3, "shells"), (5, "rockets"), (6, "cells")];
/// health lost in this long (tics)...
const HURT_WINDOW: i64 = 3 * 35;
/// ...this much of it, is told at once, not left for a mark to be passed
const HURT_FAST: f64 = 25.0;
/// ...at most this often
const HURT_AGAIN: i64 = 5 * 35;
/// on her own, what she's doing is said at most this often (tics)
const AUTO_SAY_EVERY: i64 = 8 * 35;
/// a fight's summary is worth a reaction when it cost this much health, or this many died
const FIGHT_NOTABLE_HEALTH: f64 = 20.0;
const FIGHT_NOTABLE_KILLS: i32 = 3;

/// An order waiting its turn.
struct Queued {
    order: Order,
    /// pressed on with mid-fight
    rush: bool,
    /// after the order in hand, not instead of it
    then: bool,
}

#[derive(Default)]
pub struct Director {
    /// orders to carry out
    queue: VecDeque<Queued>,
    /// an explore order an errand broke into (a go, in the middle of it): gone on with once
    /// the errand's done
    resume: Option<Order>,
    doing: Option<Doing>,
    /// the objectives taken for the order in hand: not taken again for it (the explorer may go on
    /// offering one it counts as done, a wall "not seen yet" taken every tic, stood on)
    taken: std::collections::BTreeSet<Taken>,
    /// id -> her name for it (this map)
    names: BTreeMap<String, String>,
    places: Option<Places>,
    pub reports: Vec<Report>,
    /// what's been told already: exits, switches and doors, items, monsters (ids), the health
    /// mark passed, keys and weapons held, dead
    told_exits: std::collections::BTreeSet<String>,
    told_items: std::collections::BTreeSet<i32>,
    /// items seen she couldn't get to (told when she can), and those told so
    out_of_reach: std::collections::BTreeSet<i32>,
    told_reachable: std::collections::BTreeSet<i32>,
    told_monsters: std::collections::BTreeSet<i32>,
    health_mark: Option<i32>,
    had_keys: Option<crate::specials::Keys>,
    had_weapons: Option<Vec<i32>>,
    told_dead: bool,
    /// when she last tried getting off a floor that hurts
    off_tried: i64,
    /// told she's on a floor that hurts with no known way off (till she's off it)
    told_trapped: bool,
    /// an explore order paused for a fight (she fights; it goes on once it's over); and told so
    /// (once an order: the status says the rest)
    paused: bool,
    told_paused: bool,
    /// since when nothing's been fighting her (None: something is)
    calm_since: Option<i64>,
    /// the areas the last list found nothing more to explore in, and not where she is
    done_areas: std::collections::BTreeSet<String>,
    /// a switch pressed: its line, what it's called, and till when to watch for what it did (what
    /// it moves is learned seeing it move, a moment after)
    pressed: Option<(usize, String, i64)>,
    /// with no order this long (tics) she carries on by herself (None: she waits, as long as
    /// it takes); and since when she's had none
    pub idle_auto: Option<i64>,
    idle_since: Option<i64>,
    /// what she's last said she's doing by herself
    told_auto: String,
    told_auto_tic: Option<i64>,
    /// the fight she's in, for its summary when it's over; and what she had last tic (what a
    /// fight cost is the falls from tic to tic, so a medikit taken in the middle doesn't hide it)
    fight: Option<Fight>,
    was: Option<Had>,
    /// her health lately, (tic, health): for telling a fast loss while there's time to act on it
    health_lately: VecDeque<(i64, f64)>,
    told_hurt: Option<i64>,
    told_secrets: Option<i32>,
    /// the level's tally (kills, items, secrets: got, of), as a source port shows it; set each tic
    /// by whoever runs the game (it's the engine's, not in what she observes)
    pub tally: Tally,
}

/// The level's tally: kills, items, secrets, each (got, of).
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    pub kills: (i32, i32),
    pub items: (i32, i32),
    pub secrets: (i32, i32),
}

impl Tally {
    /// From the engine's (items, kills, secrets).
    pub fn of(t: [(i32, i32); 3]) -> Tally {
        Tally {
            items: t[0],
            kills: t[1],
            secrets: t[2],
        }
    }

    fn line(&self) -> String {
        let f = |(got, of): (i32, i32)| format!("{got}/{of}");
        format!(
            "kills {}, items {}, secrets {}",
            f(self.kills),
            f(self.items),
            f(self.secrets)
        )
    }

    fn complete(&self) -> bool {
        [self.kills, self.items, self.secrets]
            .iter()
            .all(|(got, of)| got >= of)
    }
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
        self.order_with(o, false, false);
    }

    /// An order rushed (`rush`): pressed on with mid-fight, shooting and dodging on the way, not
    /// stopped for the fight (still getting clear of what claws, and away when nearly dead).
    pub fn order_rushed(&mut self, o: Order, rush: bool) {
        self.order_with(o, rush, false);
    }

    /// An order, rushed or not; `then`: once the order in hand is done, not instead of it (an
    /// order given without it replaces the one in hand, which is said).
    pub fn order_with(&mut self, order: Order, rush: bool, then: bool) {
        self.queue.push_back(Queued { order, rush, then });
    }

    /// Doing nothing, waiting for an order?
    pub fn idle(&self, agent: &Agent) -> bool {
        self.queue.is_empty()
            && self.doing.is_none()
            && self.pressed.is_none()
            && agent.x().current.is_none()
    }

    /// A new map: names, orders, places and what's been told are the old map's.
    pub fn new_map(&mut self) {
        self.queue.clear();
        self.resume = None;
        self.doing = None;
        self.names.clear();
        self.places = None;
        self.told_exits.clear();
        self.told_items.clear();
        self.out_of_reach.clear();
        self.told_reachable.clear();
        self.told_monsters.clear();
        self.health_mark = None;
        self.told_dead = false;
        self.fight = None;
        self.was = None;
        self.health_lately.clear();
        self.told_secrets = None;
    }

    /// What she's doing on her orders, in a few words ("exploring area-0 +secrets 2, rushing").
    pub fn doing(&self, agent: &Agent) -> String {
        let doing = match &self.doing {
            Some(Doing::Once(_, w)) => format!("going to {w}"),
            Some(Doing::Area(_, w, secrets)) => format!(
                "exploring {w}{}{}",
                with_secrets(*secrets),
                if self.paused {
                    " (paused for the fight)"
                } else {
                    ""
                }
            ),
            Some(Doing::Way(b, secrets)) => format!(
                "exploring {}{}{}",
                compass(*b),
                with_secrets(*secrets),
                if self.paused {
                    " (paused for the fight)"
                } else {
                    ""
                }
            ),
            Some(Doing::Off) => "getting off a hurting floor".to_string(),
            Some(Doing::Pickup) => "picking up nearby".to_string(),
            Some(Doing::Auto) => format!(
                "on her own (no order){}",
                agent
                    .x()
                    .current
                    .as_ref()
                    .map_or(String::new(), |o| format!(": {}", o.label))
            ),
            None if agent.x().current.is_some() => "busy".to_string(),
            None => "waiting".to_string(),
        };
        if agent.x().press_on {
            format!("{doing}, rushing")
        } else {
            doing
        }
    }

    /// Her state in a line: health, armor, weapons, ammo, keys, what she's doing.
    pub fn status(&self, agent: &Agent, obs: &Observation) -> String {
        let weapons: Vec<&str> = obs
            .weapons
            .iter()
            .flatten()
            .map(|&w| weapon_name(w, obs))
            .collect();
        let ammo = |slot: i32| {
            obs.ammo
                .iter()
                .flatten()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n)
        };
        let mut pools = Vec::new();
        for (slot, what) in [(2, "bullets"), (3, "shells"), (5, "rockets"), (6, "cells")] {
            if ammo(slot) > 0 {
                pools.push(format!("{} {what}", ammo(slot)));
            }
        }
        let keys: Vec<String> = obs
            .keys
            .unwrap_or_default()
            .iter()
            .map(|k| key_name(k).to_string())
            .collect();
        let doing = self.doing(agent);
        // (in a fight: what's in sight, the nearest; an order then is held up by it unless rushed)
        let doing = match fighting(&engaged(agent, obs), obs) {
            Some(what) => format!("{doing}; fighting {what}"),
            None => doing,
        };
        // (short: what Evil reads costs her context)
        format!(
            "Health {:.0}, armor {:.0}; {}; {}; {}; {}; {doing}",
            obs.health.unwrap_or(0.0),
            obs.armor.unwrap_or(0.0),
            if weapons.is_empty() {
                "no weapons".to_string()
            } else {
                weapons.join(", ")
            },
            if pools.is_empty() {
                "no ammo".to_string()
            } else {
                pools.join(", ")
            },
            if keys.is_empty() {
                "no keys".to_string()
            } else {
                keys.join(", ")
            },
            self.tally.line()
        )
    }

    /// Leaving now: what she'd leave behind, as a question to be sure (None: nothing left, the
    /// tally complete and nothing more known to do). An exit asks first, for now. (None too with
    /// no exit known: the order says so.)
    pub fn exit_check(&mut self, agent: &mut Agent, obs: &Observation) -> Option<String> {
        let open = self.open_objectives(agent, obs);
        if !open.iter().any(|o| o.kind == Kind::Exit) {
            return None;
        }
        let count = |f: &dyn Fn(&Objective) -> bool| {
            open.iter()
                .filter(|o| f(o))
                .map(id_of)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        let explore = count(&|o| explores(o));
        let switches = count(&|o| o.kind == Kind::Switch);
        let doors = count(&|o| o.kind == Kind::Door);
        // (the items tally's own apart from the rest: "12 items seen" beside "items 5/6" reads as
        // a contradiction; ammo and health she was full of are pickups, not the tally's)
        let seen = self.list(agent, obs, Topic::Items);
        let counted = seen.iter().filter(|e| e.what.ends_with(COUNTS)).count();
        let pickups = seen.len() - counted;
        let mut left = Vec::new();
        for (n, one, many) in [
            (explore, "to explore", "to explore"),
            (switches, "switch", "switches"),
            (doors, "door", "doors"),
            (counted, "tally item", "tally items"),
            (pickups, "other pickup", "other pickups"),
        ] {
            if n > 0 {
                left.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        if left.is_empty() && self.tally.complete() {
            return None;
        }
        let known = if left.is_empty() {
            String::new()
        } else {
            format!("{}; ", left.join(", "))
        };
        Some(format!(
            "Still here: {known}{}. Leave anyway? Resend with sure: true.",
            self.tally.line()
        ))
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
            if mark.is_some_and(|m| self.health_mark.is_none_or(|h| m < h)) {
                self.event(obs, true, format!("Health down to {health:.0}."));
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
                self.event(obs, true, format!("Got the {}.", key_name(k)));
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
                self.event(obs, true, format!("Got the {}.", weapon_name(w, obs)));
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
        for e in self.objectives_of(agent, obs, &[Kind::Exit, Kind::Switch, Kind::Door]) {
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
            self.event(
                obs,
                true,
                format!(
                    "Exit{} found: {}.",
                    if exits.len() == 1 { "" } else { "s" },
                    exits.join(", ")
                ),
            );
        }
        if !switches.is_empty() {
            self.event(
                obs,
                true,
                format!(
                    "Switch{} found: {}.",
                    if switches.len() == 1 { "" } else { "es" },
                    switches.join(", ")
                ),
            );
        }
        if !doors.is_empty() {
            self.event(
                obs,
                false,
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
                "Weapon" => crate::tactical::bestiary::weapon_item(t.name)
                    .is_some_and(|w| !held.contains(&w)),
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
                        what: t.name.to_string(),
                        at: (t.x, t.y),
                    },
                    obs,
                ));
            }
        }
        if !seen.is_empty() {
            self.event(obs, true, format!("Spotted: {}.", seen.join("; ")));
        }
        self.newly_reachable(agent, obs);
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
            let what: Vec<String> = counts.iter().map(|(n, c)| format!("{c} {n}")).collect();
            let dangerous = new.iter().any(|m| {
                crate::tactical::bestiary::monster(m.1).is_some_and(|i| i.boss || i.threat >= 6.0)
            });
            self.event(
                obs,
                dangerous,
                format!(
                    "Monsters: {} ({} {:.0}).",
                    what.join(", "),
                    compass(heading(pos, nearest)),
                    dist(pos, nearest)
                ),
            );
        }
    }

    /// Items she's seen and couldn't get to that she now can (a floor gone down to them, a lift,
    /// a door: whatever opened the way, seen), told: weapons on platforms a switch lowers are
    /// hers to hear can be had. Notable if any's worth a decision (a gun she hasn't, a powerup,
    /// armor, a key).
    fn newly_reachable(&mut self, agent: &Agent, obs: &Observation) {
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
            let reachable = x.touch_spot((t.x, t.y), &x.reach).is_some();
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
                    "Weapon" => crate::tactical::bestiary::weapon_item(t.name)
                        .is_some_and(|w| !held.contains(&w)),
                    "Key" | "Powerup" | "Armor" => true,
                    _ => matches!(t.name, "Soulsphere" | "Megasphere" | "Berserk"),
                };
                now.push(self.describe(
                    &Entry {
                        id,
                        what: t.name.to_string(),
                        at: (t.x, t.y),
                    },
                    obs,
                ));
            }
        }
        if !now.is_empty() {
            self.event(obs, notable, format!("Now reachable: {}.", now.join("; ")));
        }
    }

    /// The things worth a word as they happen: a secret found; health going fast, while there's
    /// time to do something about it; a fight over, with what it cost and what it killed.
    fn moments(&mut self, agent: &Agent, obs: &Observation) {
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
            self.event(
                obs,
                true,
                format!("Secret found ({secrets}/{}).", self.tally.secrets.1),
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
            self.event(
                obs,
                true,
                format!(
                    "Losing health fast: {:.0} in {} s{by}. Health {health:.0}.",
                    top - health,
                    HURT_WINDOW / 35
                ),
            );
        }
        if self
            .fight
            .as_ref()
            .is_some_and(|f| f.calm_since.is_some_and(|t| obs.tic - t >= RESUME_CALM))
        {
            if let Some(f) = self.fight.take() {
                let killed = self.tally.kills.0 - f.kills_at_start;
                let them: Vec<String> = f
                    .against
                    .iter()
                    .map(|(name, n)| format!("{n} {name}"))
                    .collect();
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
                let notable =
                    f.health_lost >= FIGHT_NOTABLE_HEALTH || killed >= FIGHT_NOTABLE_KILLS;
                // (one that came to nothing, a monster seen and gone, isn't a fight to tell of)
                if killed == 0 && cost.is_empty() {
                    return;
                }
                self.event(
                    obs,
                    notable,
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
    }

    fn event(&mut self, obs: &Observation, notable: bool, text: String) {
        self.reports.push(Report {
            tic: obs.tic,
            ok: true,
            text,
            notable,
        });
    }

    fn places(&mut self, agent: &Agent) -> &Places {
        self.places
            .get_or_insert_with(|| Places::build(&agent.x().nav))
    }

    /// A list of what she knows of, nearest first.
    pub fn list(&mut self, agent: &mut Agent, obs: &Observation, topic: Topic) -> Vec<Entry> {
        let pos = obs.pos();
        let mut out: Vec<Entry> = match topic {
            Topic::Areas => {
                let open = self.open_objectives(agent, obs);
                let walls = agent.x_mut().suspected_walls(obs, 3);
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
                        if in_it {
                            what.push_str(", here");
                        } else if left == 0 {
                            done.insert(a.id.clone());
                        }
                        what.push_str(&match (left, hiding) {
                            (0, [0, 0, 0]) => ", explored, no walls to search".to_string(),
                            (0, [a, b, c]) => format!(", explored, {a}/{b}/{c} walls to search"),
                            (n, _) => format!(", {n} to explore"),
                        });
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
            Topic::Doors => self.objectives_of(agent, obs, &[Kind::Door]),
            Topic::Switches => {
                let mut out = self.objectives_of(agent, obs, &[Kind::Switch]);
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
            Topic::Exits => self.objectives_of(agent, obs, &[Kind::Exit]),
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
                            format!("{}{COUNTS}", t.name)
                        } else {
                            t.name.to_string()
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
                        what: format!("{name}, {seen}{}", if awake { "" } else { ", asleep" }),
                        at,
                    }
                })
                .collect(),
        };
        out.sort_by(|a, b| dist(pos, a.at).total_cmp(&dist(pos, b.at)));
        out
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
    fn tag(&self, id: &str, obs: &Observation) -> String {
        let name = self
            .names
            .get(id)
            .map_or(String::new(), |n| format!(" \"{n}\""));
        let thing = id
            .split_once("-t")
            .and_then(|(_, n)| n.parse::<i32>().ok())
            .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n))
            .map_or(String::new(), |t| format!(" ({})", t.name));
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

    /// Each tic: the order in hand carried on (its next objective, or its end reported); the next
    /// order taken up.
    pub fn tick(&mut self, agent: &mut Agent, obs: &Observation) {
        self.events(agent, obs);
        agent.x_mut().spares = obs
            .things
            .iter()
            .flatten()
            .filter(|t| spare(t.name, obs))
            .map(|t| t.id)
            .collect();
        if let Some((line, what, until)) = self.pressed.clone() {
            let effect = self.effect_of(agent, obs, line);
            if effect.is_some() || obs.tic >= until {
                self.pressed = None;
                self.report(
                    obs,
                    true,
                    match effect {
                        Some(w) => format!("Pressed {what}: {w}."),
                        None => format!("Pressed {what}: no change seen."),
                    },
                );
            }
        }
        // (the next order: at once, in place of the one in hand, unless it was given to come
        // after it)
        let in_hand = matches!(
            self.doing,
            Some(Doing::Once(..) | Doing::Area(..) | Doing::Way(..))
        ) || self.pressed.is_some();
        if let Some(Queued { order: o, rush, .. }) = self
            .queue
            .front()
            .is_some_and(|q| !q.then || !in_hand)
            .then(|| self.queue.pop_front())
            .flatten()
        {
            self.start(agent, obs, o);
            // (a single target only: a sweep of a room, rushed, is a run about it under fire)
            agent.x_mut().press_on = rush && matches!(self.doing, Some(Doing::Once(..)));
            return;
        }
        if self.explore_paused(agent, obs) {
            return;
        }
        if matches!(self.doing, Some(Doing::Auto)) {
            self.auto(agent, obs);
            return;
        }
        let ended = agent.x_mut().ended.take();
        if agent.x().current.is_some() {
            return;
        }
        // (no order, nothing in hand: since when; and after `idle_auto` of it, on by herself)
        let waiting = matches!(self.doing, None | Some(Doing::Off | Doing::Pickup))
            && self.queue.is_empty()
            && self.resume.is_none()
            && self.pressed.is_none();
        if !waiting {
            self.idle_since = None;
        } else if self.idle_since.is_none() {
            self.idle_since = Some(obs.tic);
        }
        if let (Some(limit), Some(since)) = (self.idle_auto, self.idle_since) {
            if obs.tic - since >= limit && obs.health.unwrap_or(0.0) > 0.0 {
                self.idle_since = None;
                self.doing = Some(Doing::Auto);
                self.told_auto.clear();
                let x = agent.x_mut();
                x.directed = false;
                x.exit_held_back = true;
                x.out_of_options = None;
                self.report(
                    obs,
                    true,
                    format!(
                        "No order for {} s: carrying on by myself (not leaving the level) till you give one.",
                        limit / 35
                    ),
                );
                return;
            }
        }
        if self.doing.is_none() || matches!(self.doing, Some(Doing::Off | Doing::Pickup)) {
            agent.x_mut().press_on = false; // (her own doing: no order's rush)
        }
        // (an errand done, or come to nothing: on with the exploring it broke into)
        if self.doing.is_none() && self.queue.is_empty() && self.pressed.is_none() {
            if let Some(order) = self.resume.take() {
                self.order_with(order, false, false);
                return;
            }
        }
        match self.doing.take() {
            None => {
                // (waiting: never on a floor that hurts)
                if self.queue.is_empty() && obs.tic - self.off_tried >= OFF_AGAIN {
                    self.off_tried = obs.tic;
                    if self.off_hurting_floor(agent, obs) {
                        self.doing = Some(Doing::Off);
                    } else if self.pick_up_nearby(agent, obs) {
                        self.doing = Some(Doing::Pickup);
                    }
                }
            }
            // (one picked up: the next straight away, if there's one, not a tic waiting between)
            Some(Doing::Pickup) => {
                if self.pick_up_nearby(agent, obs) {
                    self.doing = Some(Doing::Pickup);
                }
            }
            Some(Doing::Off) => {}
            // (handled above: never taken here)
            Some(Doing::Auto) => self.doing = Some(Doing::Auto),
            Some(Doing::Once(id, what)) => {
                let (ok, why, pressed_nothing) = ended
                    .map_or((false, "stopped".to_string(), false), |e| {
                        (e.ok, e.why, e.pressed)
                    });
                // (a switch pressed: what it did told when she's seen it, or after a while)
                // (pressed and nothing seen to change is pressed all the same: what it moved may be
                // out of sight, across an arena; not "couldn't")
                let pressed = ok || pressed_nothing;
                match id
                    .strip_prefix("switch-l")
                    .and_then(|l| l.parse::<usize>().ok())
                    .filter(|_| pressed)
                {
                    Some(l) => self.pressed = Some((l, id.clone(), obs.tic + PRESS_WATCH)),
                    None => self.report(
                        obs,
                        ok,
                        if ok {
                            format!("Done: {what}.")
                        } else {
                            format!("Couldn't: {what} ({why}).")
                        },
                    ),
                }
            }
            Some(Doing::Area(id, what, secrets)) => {
                if self.next_in(agent, obs, secrets, |d, a, o| d.in_area(a, &id, o)) {
                    self.doing = Some(Doing::Area(id, what, secrets));
                } else {
                    let found = self.found(agent, obs, |d, a, o| d.in_area(a, &id, o));
                    self.report(
                        obs,
                        true,
                        format!("Explored {what}{}.{found}", searched(secrets)),
                    );
                }
            }
            Some(Doing::Way(b, secrets)) => {
                let pos = obs.pos();
                let fits = move |_: &Self, _: &Agent, o: &Objective| {
                    dist(pos, o.goal) > 64.0 && norm(heading(pos, o.goal) - b).abs() < EXPLORE_CONE
                };
                if self.next_in(agent, obs, secrets, fits) {
                    self.doing = Some(Doing::Way(b, secrets));
                } else {
                    let found = self.found(agent, obs, fits);
                    self.report(
                        obs,
                        true,
                        format!("Explored {}{}.{found}", compass(b), searched(secrets)),
                    );
                }
            }
        }
    }

    /// An explore order and a fight: paused while something's fighting her (the objective in
    /// hand dropped, the fight free to move her; what it shows her she maps all the same), on
    /// again after RESUME_CALM with nothing fighting her. True while paused. (A sweep of look-at
    /// spots under fire never ends, rushed or not.)
    fn explore_paused(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        let fighting = !engaged(agent, obs).is_empty();
        if fighting {
            self.calm_since = None;
        } else if self.calm_since.is_none() {
            self.calm_since = Some(obs.tic);
        }
        if !matches!(self.doing, Some(Doing::Area(..) | Doing::Way(..))) {
            self.paused = false;
            return false;
        }
        if fighting && !self.paused {
            self.paused = true;
            let x = agent.x_mut();
            if let Some(cur) = x.current.take() {
                self.taken.remove(&taken_key(&cur)); // (to be taken again, if it's still to do)
            }
            x.follower = None;
            x.ended = None;
            // (told once: in an arena it goes on and off, each time a word in her context)
            if !self.told_paused {
                self.told_paused = true;
                self.report(
                    obs,
                    true,
                    "Exploring paused while she's fighting; it goes on after.".into(),
                );
            }
        }
        if self.paused && self.calm_since.is_some_and(|t| obs.tic - t >= RESUME_CALM) {
            self.paused = false;
        }
        self.paused
    }

    /// On her own for want of an order: what she's taken up is said (quietly) as it changes;
    /// with nothing left she'd do but leave, she stops and says so: leaving is an order's.
    fn auto(&mut self, agent: &mut Agent, obs: &Observation) {
        if agent.x().out_of_options.is_some() {
            self.end_auto(agent);
            self.doing = None;
            let exits = self.objectives_of(agent, obs, &[Kind::Exit]);
            self.report(
                obs,
                false,
                match exits.first() {
                    Some(e) => format!(
                        "Nothing more I can find to do here but search walls or leave ({}).",
                        self.describe(e, obs)
                    ),
                    None => "Nothing more I can find to do here, and no exit known.".to_string(),
                },
            );
            return;
        }
        // (what she's taken up, in plain words, as it changes: not each pickup and barrel on
        // the way, and not more than a line every few seconds)
        let Some((kind, phrase)) = agent
            .x()
            .current
            .as_ref()
            .map(|o| (o.kind, plain(o.kind, &o.label)))
        else {
            return;
        };
        if !matches!(kind, Kind::Item | Kind::Barrel)
            && phrase != self.told_auto
            && self
                .told_auto_tic
                .is_none_or(|t| obs.tic - t >= AUTO_SAY_EVERY)
        {
            self.event(obs, false, format!("On my own: {phrase}."));
            self.told_auto = phrase;
            self.told_auto_tic = Some(obs.tic);
        }
    }

    /// Back to her orders: the explorer chooses nothing of its own.
    fn end_auto(&mut self, agent: &mut Agent) {
        let x = agent.x_mut();
        x.directed = true;
        x.exit_held_back = false;
        x.current = None;
        x.follower = None;
        x.ended = None;
    }

    fn start(&mut self, agent: &mut Agent, obs: &Observation, o: Order) {
        // (a new order replaces the one in hand, and says so: two given together, the first
        // would be gone without a word. An errand in the middle of exploring is an errand: the
        // exploring goes on after it)
        let errand = matches!(o, Order::Go(..));
        if matches!(self.doing, Some(Doing::Auto)) {
            self.end_auto(agent);
        }
        match self.doing.take() {
            Some(Doing::Area(id, what, secrets)) if errand => {
                self.resume = Some(Order::ExploreArea(id, secrets));
                self.report(obs, true, format!("Exploring {what} goes on after this."));
            }
            Some(Doing::Way(b, secrets)) if errand => {
                self.resume = Some(Order::ExploreWay(b, secrets));
                self.report(
                    obs,
                    true,
                    format!("Exploring {} goes on after this.", compass(b)),
                );
            }
            Some(Doing::Once(_, what)) if !matches!(o, Order::Stop) => {
                self.report(obs, false, format!("Dropped for the new order: {what}."));
            }
            Some(Doing::Area(_, what, _)) if !matches!(o, Order::Stop) => {
                self.report(
                    obs,
                    false,
                    format!("Dropped for the new order: exploring {what}."),
                );
            }
            Some(Doing::Way(b, _)) if !matches!(o, Order::Stop) => {
                self.report(
                    obs,
                    false,
                    format!("Dropped for the new order: exploring {}.", compass(b)),
                );
            }
            _ => {}
        }
        if !errand {
            self.resume = None;
        }
        let x = agent.x_mut();
        x.current = None;
        x.follower = None;
        x.ended = None;
        self.taken.clear();
        self.told_paused = false;
        match o {
            Order::Stop => self.report(obs, true, "Stopped.".into()),
            Order::Exit => match self
                .objectives_of(agent, obs, &[Kind::Exit])
                .into_iter()
                .next()
            {
                Some(e) => self.go(agent, obs, &e.id, false),
                None => self.report(obs, false, "No exit known yet.".into()),
            },
            Order::Go(r, risky) => {
                let id = self.resolve(&r);
                self.go(agent, obs, &id, risky);
            }
            Order::ExploreArea(r, secrets) => {
                let id = self.resolve(&r);
                let Some(a) = self.places(agent).by_id(&id).cloned() else {
                    self.report(obs, false, format!("No area \"{r}\"."));
                    return;
                };
                let what = self.tag(&a.id, obs);
                let fits = |d: &Self, a2: &Agent, o: &Objective| d.in_area(a2, &id, o);
                if self.next_in(agent, obs, secrets, fits) {
                    self.report(
                        obs,
                        true,
                        format!("Exploring {what}{}.", with_secrets(secrets)),
                    );
                    self.doing = Some(Doing::Area(id, what, secrets));
                } else {
                    let found = self.found(agent, obs, fits);
                    self.report(
                        obs,
                        true,
                        format!(
                            "Nothing to {} in {what}.{found}",
                            if secrets > 0 {
                                "explore or search"
                            } else {
                                "explore"
                            }
                        ),
                    );
                }
            }
            Order::ExploreWay(b, secrets) => {
                let pos = obs.pos();
                let fits = move |_: &Self, _: &Agent, o: &Objective| {
                    dist(pos, o.goal) > 64.0 && norm(heading(pos, o.goal) - b).abs() < EXPLORE_CONE
                };
                if self.next_in(agent, obs, secrets, fits) {
                    self.report(
                        obs,
                        true,
                        format!("Exploring {}{}.", compass(b), with_secrets(secrets)),
                    );
                    self.doing = Some(Doing::Way(b, secrets));
                } else {
                    let found = self.found(agent, obs, fits);
                    self.report(
                        obs,
                        false,
                        format!(
                            "Nothing to {} {}.{found}",
                            if secrets > 0 {
                                "explore or search"
                            } else {
                                "explore"
                            },
                            compass(b)
                        ),
                    );
                }
            }
        }
    }

    /// Waiting on a floor that hurts: to the nearest known floor that doesn't, the way she was
    /// going rather than back (through what she came for, out the far side: an item in nukage
    /// picked up isn't stood on after, waiting for the next order). True if she's off to it.
    fn off_hurting_floor(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let pos = obs.pos();
        let Some(here) = nav.poly_at(pos) else {
            return false;
        };
        if !agent.x().knowledge.damaging(nav.sector_of_poly(here)) {
            self.told_trapped = false;
            return false;
        }
        let v = obs.velocity.unwrap_or((0.0, 0.0));
        let way = if v.0.hypot(v.1) > 1.0 {
            v.1.atan2(v.0).to_degrees()
        } else {
            obs.angle
        };
        let _ = self.open_objectives(agent, obs); // (what she can get to, up to date)
        let x = agent.x();
        let mut spots: Vec<(f64, Point)> = x
            .reach
            .iter()
            .filter(|&&p| !x.knowledge.damaging(nav.sector_of_poly(p)))
            .map(|&p| {
                let c = nav.poly_center(p);
                // (behind her counts as up to twice as far)
                let back = norm(heading(pos, c) - way).abs() / 180.0;
                (dist(pos, c) * (1.0 + back), c)
            })
            .collect();
        spots.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, c) in spots.into_iter().take(OFF_TRIES) {
            let x = agent.x();
            let at = x.stand_spot_for(c, &x.reach);
            let mut o = Objective::new(Kind::Visit, at, "off the floor that hurts");
            o.risky = true; // (in it already: the way out may wade)
            if agent.x_mut().take(obs, o) {
                return true;
            }
        }
        // (no way off she knows of: hers to hear, once, not to die waiting in silence)
        if !self.told_trapped {
            self.told_trapped = true;
            self.report(
                obs,
                false,
                format!(
                    "On a hurting floor, no known way off (health {:.0}).",
                    obs.health.unwrap_or(0.0)
                ),
            );
        }
        false
    }

    /// Waiting: the nearest pickup she takes by herself (`spare`), if it's safe: one the explorer
    /// offers (past its checks for danger and for being cut off), by a way with nothing that hurts,
    /// not far. True if she's off for it.
    fn pick_up_nearby(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        let spares = agent.x().spares.clone();
        let mut options: Vec<Objective> = self
            .open_objectives(agent, obs)
            .into_iter()
            .filter(|o| {
                o.kind == Kind::Item && !o.risky && o.thing.is_some_and(|t| spares.contains(&t))
            })
            .collect();
        let pos = obs.pos();
        options.sort_by(|a, b| dist(pos, a.goal).total_cmp(&dist(pos, b.goal)));
        for o in options.into_iter().take(PICKUP_TRIES) {
            let near = agent
                .x_mut()
                .route_to(&o, obs)
                .is_some_and(|st| crate::knowledge::explorer::route_length(&st) <= PICKUP_RANGE);
            if near && agent.x_mut().take(obs, o) {
                return true;
            }
        }
        false
    }

    /// The objective going to (trying, picking up) `id` would be; why not, if there's none.
    fn objective_for(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        id: &str,
    ) -> Result<Objective, String> {
        // (the explorer's options, which also brings what she can get to up to date)
        let open = self.open_objectives(agent, obs);
        if let Some(a) = self.places(agent).by_id(id).cloned() {
            let x = agent.x();
            let at = x.stand_spot_for(a.center, &x.reach);
            return Ok(Objective::new(
                Kind::Visit,
                at,
                format!("go to {}", Places::sketch(&a)),
            ));
        }
        if let Some(t) = id
            .split_once("-t")
            .and_then(|(_, n)| n.parse::<i32>().ok())
            .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n))
        {
            let x = agent.x();
            let at = x
                .touch_spot((t.x, t.y), &x.reach)
                .ok_or_else(|| format!("No known way to the {} yet; explore near it.", t.name))?;
            let mut o = Objective::new(Kind::Item, at, format!("pick up {}", t.name));
            o.thing = Some(t.id);
            return Ok(o);
        }
        // (or what she's doing this moment: listed a moment ago, then taken up by the order in hand
        // as she got to it; GPT-Evil's door-l148, "here" in the list, "nothing to go to" a tic later)
        let current = agent.x().current.clone().filter(|o| id_of(o) == id);
        open.into_iter()
            .find(|o| id_of(o) == id)
            .or(current)
            .ok_or_else(|| format!("Nothing \"{id}\" to go to (done, gone or unknown)."))
    }

    /// Could she go to `r` now (risky: past what she'd rather not, if it won't kill her)? What
    /// it is, or why not (and what would do it, if risky would).
    pub fn check_go(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        r: &str,
        risky: bool,
    ) -> Result<String, String> {
        let id = self.resolve(r);
        let mut obj = self.objective_for(agent, obs, &id)?;
        obj.risky = risky;
        let what = self.tag(&id, obs);
        if agent.x_mut().route_to(&obj, obs).is_some() {
            return Ok(what);
        }
        if !risky {
            obj.risky = true;
            if agent.x_mut().route_to(&obj, obs).is_some() {
                return Err(format!("The only known way to {what} crosses a hurting floor (or worse); resend with risky: true."));
            }
        }
        Err(format!("No survivable way to {what} known."))
    }

    /// Go to (try, pick up) something by id.
    fn go(&mut self, agent: &mut Agent, obs: &Observation, id: &str, risky: bool) {
        let mut obj = match self.objective_for(agent, obs, id) {
            Ok(o) => o,
            Err(why) => {
                self.report(obs, false, why);
                return;
            }
        };
        obj.risky = risky;
        let what = self.tag(id, obs);
        if agent.x_mut().take(obs, obj) {
            self.report(obs, true, format!("Going to {what}."));
            self.doing = Some(Doing::Once(id.to_string(), what));
        } else {
            self.report(obs, false, format!("No way to {what}."));
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

    /// The explorer's objectives of those kinds as entries: doors, switches, exits (what each
    /// is: only what the id doesn't say, a key it needs, a secret exit; not the explorer's label).
    fn objectives_of(&self, agent: &mut Agent, obs: &Observation, kinds: &[Kind]) -> Vec<Entry> {
        self.open_objectives(agent, obs)
            .into_iter()
            .filter(|o| kinds.contains(&o.kind))
            .map(|o| {
                let info = o.trigger.as_ref().map(|t| &t.info);
                let what = match info {
                    Some(i) if i.category == crate::specials::Category::SecretExit => {
                        "secret exit".to_string()
                    }
                    Some(i) => lock_name(i.lock).map_or(String::new(), |k| format!("needs {k}")),
                    None => String::new(),
                };
                Entry {
                    id: id_of(&o),
                    what,
                    at: o.goal,
                }
            })
            .collect()
    }

    /// What an exploring order leaves to her, as sentences for its end ("" if nothing): the
    /// switches found and not pressed, and the walls still to search at each thoroughness.
    fn found(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        fits: impl Fn(&Self, &Agent, &Objective) -> bool,
    ) -> String {
        let mut out = self.switches_found(agent, obs, &fits);
        let walls = agent.x_mut().suspected_walls(obs, 3);
        let [a, b, c] = by_thoroughness(walls.iter().filter(|o| fits(self, agent, o)));
        if c > 0 {
            out.push_str(&format!(" Walls to search: {a}/{b}/{c}."));
        }
        out
    }

    /// The switches not pressed yet that `fits` (where she'd press them from), as a sentence for an
    /// exploring order's end ("" if none).
    fn switches_found(
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
    fn effect_of(&mut self, agent: &mut Agent, obs: &Observation, line: usize) -> Option<String> {
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
    fn in_area(&self, agent: &Agent, id: &str, o: &Objective) -> bool {
        let Some(places) = &self.places else {
            return false;
        };
        let nav = &agent.x().nav;
        nav.poly_at(o.goal)
            .is_some_and(|q| places.area_of(nav.sector_of_poly(q)).id == id)
    }

    /// The best of the explorer's objectives that `fits` (and an exploring one: not out by an exit,
    /// not off through a teleporter), taken up. False if there's none (or no way to any).
    fn next_in(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        secrets: u8,
        fits: impl Fn(&Self, &Agent, &Objective) -> bool,
    ) -> bool {
        let open = self.open_objectives(agent, obs);
        // (the walls searched that thoroughly, not the strong ones by her the explorer offers;
        // after the rest, as a player does: what's in plain sight first)
        let walls = agent.x_mut().suspected_walls(obs, secrets);
        let options: Vec<Objective> = open
            .into_iter()
            .filter(explores)
            .chain(walls)
            .filter(|o| !self.taken.contains(&taken_key(o)))
            .filter(|o| fits(self, agent, o))
            .collect();
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

    /// An order's news: started, done, couldn't (worth a reaction when it's done or couldn't be).
    fn report(&mut self, obs: &Observation, ok: bool, text: String) {
        let notable = !text.starts_with("Going") && !text.starts_with("Exploring");
        self.reports.push(Report {
            tic: obs.tic,
            ok,
            text,
            notable,
        });
    }
}

/// An objective as the order in hand remembers taking it: the explorer's ident, and the wall for
/// pressing one (every wall has the same ident: an explore with secrets took one and ended).
type Taken = (Ident, Option<usize>);

fn taken_key(o: &Objective) -> Taken {
    (o.ident(), o.wall)
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
    const POINTS: [&str; 8] = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"];
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

/// Is o one of exploring's? Not out by an exit or a teleporter, not after a noise or a monster;
/// nor a switch: pressing one is hers to decide, it may set off anything (exploring finds them and
/// says so); nor pressing walls for secrets (a search asked for adds its own).
fn explores(o: &Objective) -> bool {
    !matches!(
        o.kind,
        Kind::Exit | Kind::Teleporter | Kind::Noise | Kind::Recheck | Kind::Hunt | Kind::Switch
    ) && !is_search(o)
}

/// The walls among `walls` (a search's, their suspicion the secret score) to search at each
/// thoroughness, 1 to 3.
fn by_thoroughness<'a>(walls: impl Iterator<Item = &'a Objective>) -> [usize; 3] {
    let mut n = [0; 3];
    for o in walls {
        for (count, (press, _)) in n.iter_mut().zip(crate::knowledge::model::THOROUGHNESS) {
            if o.suspicion >= press {
                *count += 1;
            }
        }
    }
    n
}

/// An exploring order's end, its search said: ", secrets 2 too".
fn searched(secrets: u8) -> String {
    if secrets > 0 {
        format!(", secrets {secrets} too")
    } else {
        String::new()
    }
}

/// Searching for secrets: pressing a wall she suspects, back into a secret sector.
fn is_search(o: &Objective) -> bool {
    matches!(o.kind, Kind::Wall | Kind::Secret)
}

fn with_secrets(secrets: u8) -> String {
    if secrets > 0 {
        format!(" +secrets {secrets}")
    } else {
        String::new()
    }
}

/// A pickup she takes by herself whenever it's safe: bonuses below 200, health that heals her
/// without going to waste, ammo with room for all of it.
fn spare(name: &str, obs: &Observation) -> bool {
    use crate::tactical::bestiary::{ammo_item, ammo_max, health_item};
    let health = obs.health.unwrap_or(100.0);
    match name {
        "HealthBonus" => health < 200.0,
        "ArmorBonus" => obs.armor.unwrap_or(0.0) < 200.0,
        "Stimpack" | "Medikit" => health_item(name)
            .is_some_and(|(amount, cap)| health + f64::from(amount) <= f64::from(cap)),
        "Backpack" => false,
        _ => ammo_item(name).is_some_and(|(slots, amount)| {
            let slot = slots[0];
            let have = obs
                .ammo
                .iter()
                .flatten()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n);
            let max = ammo_max(slot) * if obs.backpack == Some(true) { 2.0 } else { 1.0 };
            f64::from(have + amount) <= max
        }),
    }
}

/// What a lock needs, as she'd say it (None: nothing).
fn lock_name(l: crate::specials::Lock) -> Option<&'static str> {
    use crate::specials::Lock;
    Some(match l {
        Lock::None => return None,
        Lock::Red | Lock::RedCard | Lock::RedSkull => "red key",
        Lock::Blue | Lock::BlueCard | Lock::BlueSkull => "blue key",
        Lock::Yellow | Lock::YellowCard | Lock::YellowSkull => "yellow key",
        Lock::Any => "any key",
        _ => "all keys",
    })
}

/// The monsters fighting her now (`Combat::engaged`: able to hurt her from where they are).
/// What she's after, in the viewers' words (not the objective's own: "try the wall (line 8)").
pub fn plain(kind: Kind, label: &str) -> String {
    match kind {
        Kind::Exit => "heading for the exit".into(),
        Kind::Key => "going for a key".into(),
        Kind::Switch => "going to press a switch".into(),
        Kind::Door => "going to open a door".into(),
        Kind::Frontier | Kind::Room | Kind::Visit | Kind::Recheck => "exploring".into(),
        Kind::Noise => "checking out a noise".into(),
        Kind::Teleporter => "taking a teleporter".into(),
        Kind::Item => label.strip_prefix("pick up ").map_or_else(
            || "picking something up".into(),
            |what| format!("picking up the {}", what.to_lowercase()),
        ),
        Kind::Hunt => "hunting".into(),
        Kind::Wall => "searching for secrets".into(),
        Kind::Secret => "going back to a secret".into(),
        Kind::Barrel => "clearing a barrel".into(),
    }
}

/// What's fighting her, in a phrase: how many of each kind, and where the nearest is
/// ("2 Fatso, 1 Arachnotron (NW 650)"). None: nothing is.
fn fighting(against: &[crate::motor::Thing], obs: &Observation) -> Option<String> {
    let pos = obs.pos();
    let near = against
        .iter()
        .map(|t| (t.x, t.y))
        .min_by(|a, b| dist(pos, *a).total_cmp(&dist(pos, *b)))?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for t in against {
        *counts.entry(t.name).or_insert(0) += 1;
    }
    let what: Vec<String> = counts.iter().map(|(n, c)| format!("{c} {n}")).collect();
    Some(format!(
        "{} ({} {:.0})",
        what.join(", "),
        compass(heading(pos, near)),
        dist(pos, near)
    ))
}

fn engaged(agent: &Agent, obs: &Observation) -> Vec<crate::motor::Thing> {
    agent
        .player()
        .map(|p| p.combat().engaged(obs))
        .unwrap_or_default()
}
