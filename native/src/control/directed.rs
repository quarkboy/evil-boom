//! Orders and options for directed play.
//!
//! The options are the explorer's own objectives (what it would choose among), each with a stable
//! id: its kind and what it's about (a thing, a line, the poly it's at), not its place in a list,
//! since the ranking changes as she goes. Names are the commander's own, for ids, kept per map.
//! An order is turned into one of those objectives and given to the explorer (`Explorer::take`);
//! when it ends, how is reported, and she waits for the next.

use std::collections::{BTreeMap, VecDeque};

use crate::geom::{dist, Point};
use crate::knowledge::explorer::{Kind, Objective};
use crate::motor::{heading, norm, Observation};
use crate::play::Agent;

/// a name at most this long (refused, not cut: a name she didn't choose is one she won't use)
pub const NAME_MAX: usize = 32;

/// exploring a way: options this far either side of it
const EXPLORE_CONE: f64 = 50.0;

/// Something she can be told to go for or do.
#[derive(Clone, Debug)]
pub struct Choice {
    pub id: String,
    pub kind: Kind,
    /// the explorer's own label ("try door (line 9)", "pick up Shotgun")
    pub label: String,
    pub at: Point,
    /// risky: something about makes it dangerous to go for now
    pub risky: bool,
    obj: Objective,
}

/// What she's told to do.
#[derive(Clone, Debug)]
pub enum Order {
    /// go for one of the options (by id or name)
    Go(String),
    /// the best of the ways to explore, roughly that way (degrees, 0 east, 90 north)
    Explore(f64),
    /// out by an exit she knows
    Exit,
    /// stop what she's doing and wait
    Stop,
}

/// What's happened, for whoever gave the orders.
#[derive(Clone, Debug)]
pub struct Report {
    pub tic: i64,
    pub ok: bool,
    pub text: String,
}

#[derive(Default)]
pub struct Director {
    queue: VecDeque<Order>,
    /// the order in hand: its id (or what it was) and what it's called
    active: Option<(String, String)>,
    /// id -> her name for it (this map)
    names: BTreeMap<String, String>,
    pub reports: Vec<Report>,
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
        self.queue.is_empty() && agent.x().current.is_none()
    }

    /// What's in hand: (id, what it's called).
    pub fn active(&self) -> Option<&(String, String)> {
        self.active.as_ref()
    }

    /// The options now, best first (the explorer's ranking; risky ones after).
    pub fn options(&self, agent: &mut Agent, obs: &Observation) -> Vec<Choice> {
        let mut out: Vec<Choice> = Vec::new();
        for risky in [false, true] {
            for obj in agent.ranked(obs, risky) {
                let id = id_of(agent, &obj);
                if out.iter().any(|c| c.id == id) {
                    continue;
                }
                out.push(Choice { id, kind: obj.kind, label: obj.label.clone(), at: obj.goal, risky: obj.risky, obj });
            }
        }
        out
    }

    /// One line for an option: its id, her name for it, what it is, which way and how far.
    pub fn describe(&self, c: &Choice, obs: &Observation) -> String {
        let pos = obs.pos();
        let d = dist(pos, c.at);
        let name = self.names.get(&c.id).map_or(String::new(), |n| format!(" \"{n}\""));
        let way = if d < 64.0 { "here".to_string() } else { format!("{} {:.0}", compass(heading(pos, c.at)), d) };
        format!("{}{name}: {} ({way}){}", c.id, c.label, if c.risky { ", risky" } else { "" })
    }

    /// The id a reference means: an id, or one of her names.
    pub fn resolve(&self, r: &str) -> Option<String> {
        let r = r.trim();
        self.names.iter().find(|(_, n)| n.eq_ignore_ascii_case(r)).map(|(id, _)| id.clone()).or_else(|| Some(r.to_string()))
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

    /// A new map: names and orders are the old map's.
    pub fn new_map(&mut self) {
        self.queue.clear();
        self.active = None;
        self.names.clear();
    }

    /// Each tic: the end of the order in hand reported; the next order taken up.
    pub fn tick(&mut self, agent: &mut Agent, obs: &Observation) {
        if let Some(e) = agent.x_mut().ended.take() {
            let what = self.active.take().map_or(e.label.clone(), |(_, w)| w);
            let text = if e.ok { format!("Done: {what} ({})", e.why) } else { format!("Couldn't: {what} ({})", e.why) };
            self.report(obs, e.ok, text);
        }
        let Some(o) = self.queue.pop_front() else { return };
        // (a new order replaces the one in hand)
        if let Some((_, what)) = self.active.take() {
            self.report(obs, true, format!("Dropped: {what} (a new order)"));
        }
        let x = agent.x_mut();
        x.current = None;
        x.follower = None;
        x.ended = None;
        let choice = match &o {
            Order::Stop => {
                self.report(obs, true, "Stopped; waiting".into());
                return;
            }
            Order::Go(r) => {
                let options = self.options(agent, obs);
                let id = self.resolve(r).unwrap_or_default();
                match options.into_iter().find(|c| c.id == id) {
                    Some(c) => c,
                    None => {
                        self.report(obs, false, format!("No option \"{r}\" (it may be done or gone); list the options for what there is"));
                        return;
                    }
                }
            }
            Order::Explore(bearing) => {
                let pos = obs.pos();
                let options = self.options(agent, obs);
                let way = options.into_iter().find(|c| {
                    matches!(c.kind, Kind::Frontier | Kind::Room | Kind::Wall | Kind::Door | Kind::Switch | Kind::Visit | Kind::Teleporter)
                        && dist(pos, c.at) > 64.0
                        && norm(heading(pos, c.at) - bearing).abs() < EXPLORE_CONE
                });
                match way {
                    Some(c) => c,
                    None => {
                        self.report(obs, false, format!("Nothing known to explore {}", compass(*bearing)));
                        return;
                    }
                }
            }
            Order::Exit => match self.options(agent, obs).into_iter().find(|c| c.kind == Kind::Exit) {
                Some(c) => c,
                None => {
                    self.report(obs, false, "No exit known yet".into());
                    return;
                }
            },
        };
        let what = self.describe(&choice, obs);
        if agent.x_mut().take(obs, choice.obj.clone()) {
            self.report(obs, true, format!("Going: {what}"));
            self.active = Some((choice.id, what));
        } else {
            self.report(obs, false, format!("No way to {what} from here"));
        }
    }

    fn report(&mut self, obs: &Observation, ok: bool, text: String) {
        self.reports.push(Report { tic: obs.tic, ok, text });
    }
}

/// An option's stable id: its kind, and what it's about.
fn id_of(agent: &Agent, o: &Objective) -> String {
    let kind = match o.kind {
        Kind::Exit => "exit",
        Kind::Key => "key",
        Kind::Switch => "switch",
        Kind::Door => "door",
        Kind::Frontier => "opening",
        Kind::Room => "room",
        Kind::Visit => "place",
        Kind::Noise => "noise",
        Kind::Teleporter => "teleporter",
        Kind::Recheck => "recheck",
        Kind::Item => "item",
        Kind::Hunt => "monster",
        Kind::Wall => "wall",
        Kind::Secret => "secret",
        Kind::Barrel => "barrel",
    };
    let about = if let Some(t) = o.thing {
        format!("t{t}")
    } else if let Some(l) = o.trigger.as_ref().map(|t| t.line).filter(|&l| l >= 0) {
        format!("l{l}")
    } else if let Some(w) = o.wall {
        format!("l{w}")
    } else {
        agent.x().nav.poly_at(o.goal).map_or_else(|| format!("{:.0}x{:.0}", o.goal.0, o.goal.1), |p| format!("p{p}"))
    };
    format!("{kind}-{about}")
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
