//! The bot's own opinion, and her eyes.
//!
//! The pick: what she'd do next left to herself (the explorer's best objective she has a way
//! to; leaving is never its choice while anything else is left, as on her own), under the id a
//! commander would order it by, with why in a few words. Lists mark it, a failed order says what
//! would do instead, and with no order for a while it's suggested (`Hints`).
//!
//! The look: what's round her now as a player's eyes give it: the room, what it leads to, the
//! monsters, pickups, doors, switches and barrels in sight, what she's heard, and the pick.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    called, compass, entries_of, explores, id_of, lock_name, some, weapon_name, Director, Entry,
    Places, Topic, SMALL,
};
use crate::geom::{dist, Point};
use crate::knowledge::explorer::{Kind, Objective};
use crate::los::Body;
use crate::motor::{heading, Observation};
use crate::navigator::{MAX_STEP, PLAYER_HEIGHT};
use crate::play::Agent;

/// the pick: the best this many of the explorer's objectives tried for a way there
const PICK_TRIES: usize = 6;
/// with no order this long (tics), the pick is suggested, quietly...
const NUDGE_QUIET: i64 = 30 * 35;
/// ...and this long, once more, to be heard
const NUDGE_LOUD: i64 = 60 * 35;
/// a look: this many of each kind of thing at most, the nearest
const LOOK_MAX: usize = 6;
/// ...nothing further off than this (a door across an open level isn't "round her")
const LOOK_FAR: f64 = 2048.0;
/// a monster seen within this (tics) is in sight; within the second, about though out of sight
const IN_SIGHT: i64 = 35;
const LATELY: i64 = 30 * 35;
/// a key's objective is where she'd touch it from: the key itself is within this of it
const KEY_NEAR: f64 = 64.0;
/// things of a kind this near each other are in one place, for the look
const SAME_PLACE: f64 = 64.0;
/// how tall a pickup and a barrel are, for whether she sees one
const PICKUP_HEIGHT: f64 = 16.0;
const BARREL_HEIGHT: f64 = 42.0;

/// How much of its own opinion the bot offers unasked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hints {
    /// none: lists in its order still, no pick, no suggestions
    Off,
    /// its pick where she asks (a list, a look, a decision forced), what would do when an order
    /// fails, and a suggestion once she's had no order for a while
    #[default]
    OnStall,
    /// and a suggestion as soon as she's waiting
    Always,
}

impl Hints {
    pub fn parse(s: &str) -> Option<Hints> {
        Some(match s {
            "off" => Hints::Off,
            "on_stall" => Hints::OnStall,
            "always" => Hints::Always,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Hints::Off => "off",
            Hints::OnStall => "on_stall",
            Hints::Always => "always",
        }
    }
}

/// What the bot would do next, as a commander would order it.
#[derive(Clone, Debug)]
pub struct Pick {
    /// what to go to, press or explore
    pub id: String,
    pub why: String,
    pub at: Point,
}

/// A list as she reads it.
pub struct Listing {
    /// a line each, the pick's marked
    pub lines: Vec<String>,
    /// the pick, when it's none of the lines
    pub pick: Option<String>,
    /// explored areas left out
    pub hidden: usize,
    /// how many there were to list
    pub known: usize,
}

impl Director {
    /// What she'd do next left to herself, if anything (and hints aren't off).
    pub fn pick(&mut self, agent: &mut Agent, obs: &Observation) -> Option<Pick> {
        if self.hints == Hints::Off {
            return None;
        }
        let open = self.open_unheld(agent, obs);
        // (not a pickup she takes by herself on the way or while she waits: nothing to choose)
        let spares = agent.x().spares.clone();
        let choice = open
            .iter()
            .filter(|o| !matches!(o.kind, Kind::Exit | Kind::Barrel))
            .filter(|o| o.thing.is_none_or(|t| !spares.contains(&t)))
            .take(PICK_TRIES)
            .find(|o| agent.x_mut().route_to(o, obs).is_some());
        match choice {
            Some(o) => Some(self.pick_of(agent, obs, o, why(o))),
            // (nothing else she has a way to: the way out, if she knows one)
            None => open
                .iter()
                .find(|o| o.kind == Kind::Exit)
                .map(|o| self.pick_of(agent, obs, o, "nothing else left in reach".into())),
        }
    }

    /// An objective as a pick, under the id it's listed by: something to explore by its area,
    /// a key by the key lying there, the rest by its own id.
    fn pick_of(&mut self, agent: &Agent, obs: &Observation, o: &Objective, why: String) -> Pick {
        let area = (explores(o) && o.kind != Kind::Key && o.thing.is_none() && o.trigger.is_none())
            .then(|| self.area_at(agent, o.goal))
            .flatten();
        let key = (o.kind == Kind::Key && o.thing.is_none())
            .then(|| {
                obs.things
                    .iter()
                    .flatten()
                    .filter(|t| t.category == "Key")
                    .map(|t| (dist((t.x, t.y), o.goal), t.id))
                    .filter(|(d, _)| *d <= KEY_NEAR)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, id)| format!("key-t{id}"))
            })
            .flatten();
        Pick {
            id: area.or(key).unwrap_or_else(|| id_of(o)),
            why,
            at: o.goal,
        }
    }

    /// The pick in a line ("door-l40: door not tried (N 210)").
    pub fn pick_line(&mut self, agent: &mut Agent, obs: &Observation) -> Option<String> {
        let p = self.pick(agent, obs)?;
        Some(self.describe_pick(&p, obs))
    }

    fn describe_pick(&self, p: &Pick, obs: &Observation) -> String {
        // (a thing by what it is too: its id doesn't say)
        let thing =
            p.id.split_once("-t")
                .and_then(|(_, n)| n.parse::<i32>().ok())
                .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n));
        self.describe(
            &Entry {
                id: p.id.clone(),
                what: match thing {
                    Some(t) => format!("{}, {}", called(t.name), p.why),
                    None => p.why.clone(),
                },
                at: p.at,
            },
            obs,
        )
    }

    /// A list for her to read: `max` lines at most, the pick marked with why; under the areas,
    /// given apart when it's none of them (a door, a switch).
    pub fn listing(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        topic: Topic,
        all: bool,
        max: usize,
    ) -> Listing {
        let (entries, hidden) = self.brief(agent, obs, topic, all);
        let pick = (topic != Topic::Monsters)
            .then(|| self.pick(agent, obs))
            .flatten();
        let mut marked = false;
        let lines = entries
            .iter()
            .take(max)
            .map(|e| {
                let line = self.describe(e, obs);
                match pick.as_ref().filter(|p| p.id == e.id) {
                    Some(p) => {
                        marked = true;
                        format!("{line} (suggested: {})", p.why)
                    }
                    None => line,
                }
            })
            .collect();
        Listing {
            lines,
            // (apart only under the list that asks "where next": a list of items or switches
            // without it in is answer enough)
            pick: pick
                .filter(|_| !marked && topic == Topic::Areas)
                .map(|p| self.describe_pick(&p, obs)),
            hidden,
            known: entries.len(),
        }
    }

    /// An order that couldn't get to `at`: what's untried nearest it, as a sentence ("" if
    /// nothing is, or hints are off). The way to something out of reach is most likely by what's
    /// nearest it that she hasn't tried.
    pub(super) fn toward(&mut self, agent: &mut Agent, obs: &Observation, at: Point) -> String {
        if self.hints == Hints::Off {
            return String::new();
        }
        let open = self.open_objectives(agent, obs);
        let near = open
            .iter()
            .filter(|o| !matches!(o.kind, Kind::Exit | Kind::Barrel | Kind::Item))
            .filter(|o| dist(o.goal, at) > 1.0)
            .min_by(|a, b| dist(a.goal, at).total_cmp(&dist(b.goal, at)));
        match near {
            Some(o) => {
                let p = self.pick_of(agent, obs, o, String::new());
                let e = Entry {
                    id: p.id,
                    what: String::new(),
                    at: p.at,
                };
                format!(" Nearest untried to it: {}.", self.describe(&e, obs))
            }
            None => String::new(),
        }
    }

    /// An order that found nothing to do: the pick, as a sentence ("" if none).
    pub(super) fn instead(&mut self, agent: &mut Agent, obs: &Observation) -> String {
        let Some(p) = self.pick_line(agent, obs) else {
            return String::new();
        };
        self.nudged = self.nudged.max(1); // (said: not suggested again the same moment)
        format!(" Suggested: {p}.")
    }

    /// Waiting on an order for `idle` tics: the pick suggested, once quietly, once to be heard
    /// (with hints `always`, as soon as she waits).
    pub(super) fn nudge(&mut self, agent: &mut Agent, obs: &Observation, idle: i64) {
        let at = match (self.hints, self.nudged) {
            (Hints::Always, 0) => 0,
            (Hints::OnStall, 0) => NUDGE_QUIET,
            (Hints::Always | Hints::OnStall, 1) => NUDGE_LOUD,
            _ => return,
        };
        if idle < at || obs.health.unwrap_or(0.0) <= 0.0 {
            return;
        }
        self.nudged += 1;
        let loud = self.nudged == 2;
        if let Some(p) = self.pick_line(agent, obs) {
            self.event(obs, loud, format!("Suggestion: {p}."));
        }
    }

    /// What's round her now, a line a kind of thing (none for a kind there's nothing of).
    pub fn look(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        let pos = obs.pos();
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let open = self.open_unheld(agent, obs);
        let known = agent.x().knowledge.known_polys();
        let heights: Vec<(f64, f64)> = obs.heights.iter().map(|h| (h.floor, h.ceiling)).collect();
        let floor = |p: Point| nav.poly_at(p).map(|i| heights[nav.sector_of_poly(i)].0);
        let eye = Body {
            x: pos.0,
            y: pos.1,
            z: obs.z.or_else(|| floor(pos)).unwrap_or(0.0),
            height: PLAYER_HEIGHT,
        };
        // (in her sight wherever she's facing: a look is a look round)
        let sees = |p: Point, z: Option<f64>, height: f64| {
            dist(pos, p) <= LOOK_FAR
                && z.or_else(|| floor(p)).is_some_and(|z| {
                    let to = Body {
                        x: p.0,
                        y: p.1,
                        z,
                        height,
                    };
                    nav.sight().clear(eye, to, &heights)
                })
        };
        let way = |p: Point| format!("{} {:.0}", compass(heading(pos, p)), dist(pos, p));
        let nearest = |v: &mut Vec<Entry>| {
            v.sort_by(|a, b| dist(pos, a.at).total_cmp(&dist(pos, b.at)));
        };
        let several = |title: &str, v: &[String], more: usize| {
            let more = if more > 0 {
                format!("; +{more} more")
            } else {
                String::new()
            };
            format!("{title}: {}{more}", v.join("; "))
        };
        let mut out = Vec::new();

        // Where she is, and what's left of it.
        let here = nav.poly_at(pos).map(|p| nav.sector_of_poly(p));
        let area = here.map(|s| self.places(agent).area_of(s).clone());
        let mut left: BTreeMap<String, usize> = BTreeMap::new();
        for o in open.iter().filter(|o| explores(o)) {
            if let Some(a) = self.area_at(agent, o.goal) {
                *left.entry(a).or_insert(0) += 1;
            }
        }
        let explored = |id: &str| match left.get(id) {
            Some(n) => format!("{n} to explore"),
            None => "explored".to_string(),
        };
        out.push(match &area {
            Some(a) => format!(
                "In {}: {}, {}.",
                self.tag(&a.id, obs),
                Places::sketch(a),
                explored(&a.id)
            ),
            None => "Not on any floor you know.".to_string(),
        });

        // The fight, in the assessment's own words; and the bot's own choice in it, which
        // monster with which gun (where to explore next isn't the question then).
        let mut fighting = false;
        if let Some(now) = agent
            .player()
            .map(|p| &p.combat().now)
            .filter(|now| obs.tic - now.tic <= 1 && !now.about.is_empty())
        {
            let mut how = Vec::new();
            for (is, what) in [
                (now.outmatched, "outmatched"),
                (now.surrounded, "surrounded"),
                (now.short_of_ammo, "short of ammo"),
                (now.vile, "an arch-vile"),
            ] {
                if is {
                    how.push(what);
                }
            }
            let word = if now.outmatched {
                "deadly"
            } else if !now.engaged.is_empty() {
                "dangerous"
            } else {
                "safe for now"
            };
            let danger = if how.is_empty() {
                format!("Danger: {word}.")
            } else {
                format!("Danger: {word} ({}).", how.join(", "))
            };
            fighting = !now.engaged.is_empty();
            let on = agent
                .player()
                .and_then(|p| p.combat().target)
                .and_then(|id| now.against_her.iter().find(|t| t.id == id))
                .filter(|_| fighting)
                .map_or(String::new(), |t| {
                    format!(
                        " On monster-{} ({}) with the {}.",
                        t.id,
                        called(t.name),
                        weapon_name(obs.weapon.unwrap_or(0), obs)
                    )
                });
            out.push(format!("{danger}{on}"));
        }

        // What the room leads to: the areas across its openings she's seen (a door, a step or
        // a lift between counted through), each by its nearest opening.
        if let Some(a) = &area {
            let leads = {
                let m = &nav.map;
                let places = self.places(agent);
                let known_sectors: BTreeSet<usize> =
                    known.iter().map(|&p| nav.sector_of_poly(p)).collect();
                let mid = |li: usize| {
                    let (p, q) = m.line_seg(li);
                    ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)
                };
                // (a line she's seen: what she knows of the floor reaches it, either side)
                let seen = |li: usize| {
                    let (p, q) = m.line_seg(li);
                    let d = dist(p, q).max(1.0);
                    let n = ((q.1 - p.1) / d * 8.0, (p.0 - q.0) / d * 8.0);
                    let c = mid(li);
                    [1.0, -1.0].iter().any(|k| {
                        nav.poly_at((c.0 + n.0 * k, c.1 + n.1 * k))
                            .is_some_and(|i| known.contains(&i))
                    })
                };
                // (the lines out of an area: the line, the sector this side, the one beyond)
                let out_of = |id: &str| -> Vec<(usize, usize, usize)> {
                    (0..m.linedefs.len())
                        .filter_map(|li| {
                            let (Some(f), Some(b)) = m.sides(li) else {
                                return None;
                            };
                            let (af, ab) = (&places.area_of(f).id, &places.area_of(b).id);
                            if af == ab {
                                None
                            } else if af == id {
                                Some((li, f, b))
                            } else if ab == id {
                                Some((li, b, f))
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                // (somewhere she could step or drop to, not a window's sill or a ledge above)
                let walkable = |from: usize, to: usize| {
                    let (a, b) = (heights[from], heights[to]);
                    b.0 - a.0 <= MAX_STEP && a.1.min(b.1) - a.0.max(b.0) >= PLAYER_HEIGHT
                };
                // area id (None: one she's seen nothing of) -> its nearest opening
                let mut leads: BTreeMap<Option<String>, Point> = BTreeMap::new();
                let mut lead = |to: &super::Area, at: Point| {
                    let key = to
                        .sectors
                        .iter()
                        .any(|s| known_sectors.contains(s))
                        .then(|| to.id.clone());
                    let e = leads.entry(key).or_insert(at);
                    if dist(pos, at) < dist(pos, *e) {
                        *e = at;
                    }
                };
                for (li, near, far) in out_of(&a.id) {
                    if !seen(li) {
                        continue;
                    }
                    let beyond = places.area_of(far);
                    if beyond.size >= SMALL {
                        if walkable(near, far) {
                            lead(beyond, mid(li));
                        }
                        continue;
                    }
                    for (_, _, further) in out_of(&beyond.id) {
                        let to = places.area_of(further);
                        if to.id != a.id && to.size >= SMALL {
                            lead(to, mid(li));
                        }
                    }
                }
                leads
            };
            let mut ways: Vec<(Point, String)> = leads
                .into_iter()
                .map(|(id, at)| {
                    (
                        at,
                        match id {
                            Some(id) => {
                                format!("{} ({}, {})", self.tag(&id, obs), way(at), explored(&id))
                            }
                            None => format!("not seen yet ({})", way(at)),
                        },
                    )
                })
                .collect();
            ways.sort_by(|a, b| dist(pos, a.0).total_cmp(&dist(pos, b.0)));
            if !ways.is_empty() {
                let more = ways.len().saturating_sub(LOOK_MAX);
                let ways: Vec<String> = ways.into_iter().take(LOOK_MAX).map(|w| w.1).collect();
                out.push(several("Leads to", &ways, more));
            }
        }

        // Monsters: in sight, and those about that aren't.
        let mut monsters = agent
            .player()
            .map(|p| p.known_monsters())
            .unwrap_or_default();
        monsters.retain(|m| obs.tic - m.3 < LATELY);
        monsters.sort_by(|a, b| dist(pos, a.2).total_cmp(&dist(pos, b.2)));
        let (in_sight, about): (Vec<_>, Vec<_>) =
            monsters.iter().partition(|m| obs.tic - m.3 < IN_SIGHT);
        if !in_sight.is_empty() {
            let lines: Vec<String> = in_sight
                .iter()
                .take(LOOK_MAX)
                .map(|(id, name, at, _, awake)| {
                    format!(
                        "monster-{id} {} ({}{})",
                        called(name),
                        way(*at),
                        if *awake { "" } else { ", asleep" }
                    )
                })
                .collect();
            out.push(several(
                "Monsters",
                &lines,
                in_sight.len().saturating_sub(LOOK_MAX),
            ));
        }
        if let Some((_, _, at, _, _)) = about.first() {
            out.push(format!(
                "Out of your sight, seen lately: {} (nearest {}).",
                about.len(),
                way(*at)
            ));
        }

        // Pickups in sight.
        let mut items: Vec<(Entry, &'static str)> = obs
            .things
            .iter()
            .flatten()
            .filter(|t| {
                matches!(
                    t.category,
                    "Weapon" | "Ammo" | "Health" | "Armor" | "Key" | "Powerup"
                )
            })
            .filter(|t| nav.poly_at((t.x, t.y)).is_some_and(|p| known.contains(&p)))
            .filter(|t| sees((t.x, t.y), Some(t.z), PICKUP_HEIGHT))
            .map(|t| {
                let e = Entry {
                    id: format!(
                        "{}-t{}",
                        if t.category == "Key" { "key" } else { "item" },
                        t.id
                    ),
                    what: called(t.name).to_string(),
                    at: (t.x, t.y),
                };
                (e, t.name)
            })
            .collect();
        items.sort_by(|a, b| dist(pos, a.0.at).total_cmp(&dist(pos, b.0.at)));
        // (several of a kind in one place, one entry: "item-t93: 5 backpacks (S 145)", by the
        // first's id)
        let mut grouped: Vec<(Entry, &str, usize)> = Vec::new();
        for (e, name) in items {
            match grouped
                .iter_mut()
                .find(|(g, _, _)| g.what == e.what && dist(g.at, e.at) <= SAME_PLACE)
            {
                Some((_, _, n)) => *n += 1,
                None => grouped.push((e, name, 1)),
            }
        }
        if !grouped.is_empty() {
            let lines: Vec<String> = grouped
                .iter()
                .take(LOOK_MAX)
                .map(|(e, name, n)| {
                    if *n == 1 {
                        self.describe(e, obs)
                    } else {
                        let many = Entry {
                            what: some(*n, name),
                            ..e.clone()
                        };
                        self.describe(&many, obs)
                    }
                })
                .collect();
            out.push(several(
                "Items",
                &lines,
                grouped.len().saturating_sub(LOOK_MAX),
            ));
        }

        // Doors, switches, teleporters and exits untried, in sight.
        let mut things = entries_of(
            &open,
            &[Kind::Door, Kind::Switch, Kind::Teleporter, Kind::Exit],
        );
        things.retain(|e| sees(e.at, None, PLAYER_HEIGHT));
        nearest(&mut things);
        if !things.is_empty() {
            let lines: Vec<String> = things
                .iter()
                .take(LOOK_MAX)
                .map(|e| self.describe(e, obs))
                .collect();
            out.push(several(
                "Untried",
                &lines,
                things.len().saturating_sub(LOOK_MAX),
            ));
        }

        // Barrels.
        let barrels: Vec<Point> = obs
            .barrels
            .iter()
            .flatten()
            .copied()
            .filter(|&b| sees(b, None, BARREL_HEIGHT))
            .collect();
        if let Some(&near) = barrels
            .iter()
            .min_by(|a, b| dist(pos, **a).total_cmp(&dist(pos, **b)))
        {
            out.push(format!(
                "Barrels: {}, nearest {}.",
                barrels.len(),
                way(near)
            ));
        }

        // What she's heard and not been to see.
        let heard: Vec<String> = open
            .iter()
            .filter_map(|o| o.heard.as_ref())
            .take(2)
            .map(|h| format!("about {}", way(h.origin)))
            .collect();
        if !heard.is_empty() {
            out.push(format!("Heard, not seen: {}.", heard.join("; ")));
        }

        if let Some(p) = self.pick_line(agent, obs).filter(|_| !fighting) {
            out.push(format!("Suggested: {p}."));
        }
        out
    }
}

/// Why the explorer would do an objective, in a few words.
fn why(o: &Objective) -> String {
    let lock = o.trigger.as_ref().and_then(|t| lock_name(t.info.lock));
    match o.kind {
        Kind::Exit => "the way out".into(),
        Kind::Key => "not taken yet".into(),
        Kind::Item if o.needed => "wanted".into(),
        Kind::Item => "worth the trip".into(),
        Kind::Door => lock.map_or_else(
            || "door not tried".into(),
            |k| format!("door not tried, {k} in hand"),
        ),
        Kind::Switch => "switch not pressed".into(),
        Kind::Frontier | Kind::Room | Kind::Visit => "nearest unexplored".into(),
        Kind::Recheck => "worth another look".into(),
        Kind::Noise => "a noise heard".into(),
        Kind::Teleporter => "teleporter not taken".into(),
        Kind::Hunt => "monsters left".into(),
        Kind::Wall | Kind::Secret => "a wall to search".into(),
        Kind::Barrel => "a barrel to clear".into(),
    }
}
