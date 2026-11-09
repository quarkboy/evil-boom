//! The game's side of the Neuro integration, a tic at a time: her actions checked, answered at
//! once (before they're carried out: Neuro waits for the answer) and turned into orders; what
//! happens told as context (notable things not silent); her state now and then; and, when the bot's
//! been waiting on her a while, a forced decision (the SDK's way for a game blocked on her).

use serde_json::{json, Value};

use super::link::{Incoming, Link};
use crate::control::directed::{bearing, Director, Order, Topic, NAME_MAX};
use crate::motor::Observation;
use crate::play::Agent;

/// waiting on her this long (tics) with nothing to do: a decision forced
const IDLE_FORCE: i64 = 6 * 35;
/// her state told (silently) at most this often, when it's changed
const STATUS_EVERY: i64 = 20 * 35;
/// a list in an answer: at most this many lines
const LIST_MAX: usize = 15;

pub struct Session {
    link: Link,
    pub director: Director,
    /// a force out, not yet answered by an action
    forced: bool,
    /// when she last had nothing to do (or the last force or action)
    idle_since: i64,
    told_status: (i64, String),
    shown: usize,
    /// told the rules on connecting
    briefed: bool,
}

/// The actions, as registered (descriptions and schemas Neuro reads as they are).
pub fn actions() -> Vec<Value> {
    let s = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    // (short: every word here is in Evil's context for the whole game, the user's)
    vec![
        json!({ "name": "doom_list", "description": "List known areas, doors, switches, items, monsters or exits, with ids. Areas: only unexplored, current and named ones unless all; a/b/c walls to search: at secrets 1/2/3.", "schema": s(json!({ "what": { "type": "string", "enum": ["areas", "doors", "switches", "items", "monsters", "exits"] }, "all": { "type": "boolean" } }), &["what"]) }),
        json!({ "name": "doom_press", "description": "Press a switch. Exploring never presses them. rush: as for doom_go.", "schema": s(json!({ "ref": { "type": "string" }, "rush": { "type": "boolean" } }), &["ref"]) }),
        json!({ "name": "doom_inspect", "description": "What something is and where.", "schema": s(json!({ "ref": { "type": "string" } }), &["ref"]) }),
        json!({ "name": "doom_go", "description": "Go to an area, item, door, switch or exit. risky: cross hurting floors if survivable. sure: exit even with things left. rush: keep going mid-fight, shooting and dodging on the way.", "schema": s(json!({ "ref": { "type": "string" }, "risky": { "type": "boolean" }, "sure": { "type": "boolean" }, "rush": { "type": "boolean" } }), &["ref"]) }),
        json!({ "name": "doom_explore", "description": "Explore an area or a direction (N, NE, ...); paused while she's fighting. secrets: also search its walls, 1 the likeliest few, 2 most, 3 every suspicious one (slow).", "schema": s(json!({ "target": { "type": "string" }, "secrets": { "type": "integer", "enum": [0, 1, 2, 3] } }), &["target"]) }),
        json!({ "name": "doom_name", "description": format!("Name something (up to {NAME_MAX} characters); the name then works as its id."), "schema": s(json!({ "ref": { "type": "string" }, "name": { "type": "string" } }), &["ref", "name"]) }),
        json!({ "name": "doom_exit", "description": "Leave the level. sure: even with things left.", "schema": s(json!({ "sure": { "type": "boolean" } }), &[]) }),
        json!({ "name": "doom_stop", "description": "Stop and wait." }),
        json!({ "name": "doom_status", "description": "Health, armor, weapons, ammo, keys, tally, current order, what she's fighting." }),
    ]
}

/// The rules, told on connecting and at each level.
const BRIEFING: &str = "You direct a Doom player. She moves, fights and takes spare health, armor and ammo herself; you choose where she goes. \
Ids come from doom_list; your names work as ids. \"NE 400\": north-east, 400 units away. (tally): counts for the items %. She waits for orders when done.";

impl Session {
    pub fn new(url: &str) -> Session {
        Session { link: Link::start(url, actions()), director: Director::new(), forced: false, idle_since: 0, told_status: (i64::MIN / 2, String::new()), shown: 0, briefed: false }
    }

    /// A tic: what's come in handled; what's happened told; a decision forced if she's waited.
    /// The lines for the log.
    pub fn tic(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        let mut log = Vec::new();
        while let Ok(i) = self.link.incoming.try_recv() {
            match i {
                Incoming::Connected => {
                    log.push("neuro: connected".into());
                    self.forced = false;
                    self.idle_since = obs.tic;
                    if !self.briefed {
                        self.link.context(BRIEFING, true);
                        self.briefed = true;
                    }
                    // (her context survives a reconnect: a short status, not everything again)
                    let status = self.director.status(agent, obs);
                    self.link.context(&status, true);
                    self.told_status = (obs.tic, status);
                }
                Incoming::Hello { character, display } => log.push(format!("neuro: playing with {display} ({character})")),
                Incoming::Lost(why) => {
                    if self.briefed {
                        log.push(format!("neuro: connection lost ({why}); trying again"));
                    }
                    self.forced = false;
                }
                Incoming::Action { id, name, data } => {
                    self.forced = false;
                    self.idle_since = obs.tic;
                    let (ok, msg) = self.action(agent, obs, &name, data.as_deref());
                    log.push(format!(">> {name} {}", data.as_deref().unwrap_or("")));
                    log.push(format!("<< {}{}", if ok { "" } else { "FAILED: " }, msg.lines().next().unwrap_or("")));
                    self.link.result(&id, ok, &msg);
                }
            }
        }
        self.director.tick(agent, obs);
        // (what's happened since: one message a tic, not silent if any of it's notable)
        let news: Vec<_> = self.director.reports[self.shown..].to_vec();
        self.shown = self.director.reports.len();
        if !news.is_empty() {
            let text: Vec<String> = news.iter().map(|r| r.text.clone()).collect();
            let notable = news.iter().any(|r| r.notable);
            log.extend(text.iter().map(|t| format!("<< {}{t}", if notable { "(!) " } else { "" })));
            self.link.context(&text.join("\n"), !notable);
        }
        let status = self.director.status(agent, obs);
        if status != self.told_status.1 && obs.tic - self.told_status.0 >= STATUS_EVERY {
            self.link.context(&status, true);
            self.told_status = (obs.tic, status.clone());
        }
        // (waiting on her: after a while, a decision forced; one at a time)
        if !self.director.idle(agent) {
            self.idle_since = obs.tic;
        } else if !self.forced && obs.tic - self.idle_since >= IDLE_FORCE {
            let state = self.state(agent, obs);
            self.link.force(&state, "She's waiting. Next order?", &["doom_explore", "doom_go", "doom_exit", "doom_list", "doom_inspect", "doom_name", "doom_status"], "medium");
            self.forced = true;
            log.push("neuro: waiting on her: decision forced".into());
        }
        log
    }

    /// Her state and options, for a forced decision (Markdown, no top-level heading).
    fn state(&mut self, agent: &mut Agent, obs: &Observation) -> String {
        let mut s = format!("## Status\n{}\n", self.director.status(agent, obs));
        for (topic, title, n) in [(Topic::Areas, "Areas", 8), (Topic::Exits, "Exits", 3), (Topic::Doors, "Doors", 5), (Topic::Switches, "Switches", 5), (Topic::Items, "Items", 6)] {
            let (list, hidden) = self.director.brief(agent, obs, topic, false);
            if list.is_empty() && hidden == 0 {
                continue;
            }
            s.push_str(&format!("\n## {title}\n"));
            for e in list.iter().take(n) {
                s.push_str(&format!("- {}\n", self.director.describe(e, obs)));
            }
            if hidden > 0 {
                s.push_str(&format!("- +{hidden} explored\n"));
            }
        }
        s
    }

    /// An action checked, carried out (an order queued), answered: (success, message).
    fn action(&mut self, agent: &mut Agent, obs: &Observation, name: &str, data: Option<&str>) -> (bool, String) {
        let args: Value = match data.map(serde_json::from_str::<Value>) {
            None => Value::Null,
            Some(Ok(v)) => v,
            Some(Err(_)) => return (false, "The action's data wasn't valid JSON.".into()),
        };
        let arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
        let sure = args.get("sure").and_then(Value::as_bool).unwrap_or(false);
        let rush = args.get("rush").and_then(Value::as_bool).unwrap_or(false);
        match name {
            "doom_list" => {
                let Some(topic) = arg("what").and_then(Topic::parse) else { return (false, "what: areas, doors, switches, items, monsters or exits.".into()) };
                let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
                let (list, hidden) = self.director.brief(agent, obs, topic, all);
                let more = if hidden > 0 { format!("\n(+{hidden} explored; all: true lists them)") } else { String::new() };
                if list.is_empty() {
                    return (true, format!("None known yet.{more}"));
                }
                let lines: Vec<String> = list.iter().take(LIST_MAX).map(|e| format!("- {}", self.director.describe(e, obs))).collect();
                (true, format!("{}{more}", lines.join("\n")))
            }
            "doom_inspect" => {
                let Some(r) = arg("ref") else { return (false, "ref: an id or name.".into()) };
                match self.director.find(agent, obs, r) {
                    Some(e) => (true, self.director.describe(&e, obs)),
                    None => (false, format!("Nothing known as \"{r}\".")),
                }
            }
            "doom_go" => {
                let Some(r) = arg("ref") else { return (false, "ref: an id or name.".into()) };
                let risky = args.get("risky").and_then(Value::as_bool).unwrap_or(false);
                if self.director.resolve(r).starts_with("exit-") && !sure {
                    if let Some(q) = self.director.exit_check(agent, obs) {
                        return (true, q);
                    }
                }
                match self.director.check_go(agent, obs, r, risky) {
                    Ok(_) => {
                        self.director.order_rushed(Order::Go(r.to_string(), risky), rush);
                        (true, "OK.".into())
                    }
                    // (not to be retried as it is: the reason says what would do)
                    Err(why) => (false, why),
                }
            }
            "doom_press" => {
                let Some(r) = arg("ref") else { return (false, "ref: a switch's id or name.".into()) };
                let id = self.director.resolve(r);
                if !id.starts_with("switch-") && !id.starts_with("door-") {
                    return (false, format!("\"{r}\" isn't a switch."));
                }
                match self.director.check_go(agent, obs, r, false) {
                    Ok(_) => {
                        self.director.order_rushed(Order::Go(r.to_string(), false), rush);
                        (true, "OK.".into())
                    }
                    Err(why) => (false, why),
                }
            }
            "doom_explore" => {
                let Some(t) = arg("target") else { return (false, "target: an area or a direction (N, NE, ...).".into()) };
                // (0-3; true, as it was, the middle one)
                let secrets = match args.get("secrets") {
                    Some(Value::Bool(true)) => 2,
                    Some(v) => v.as_u64().map_or(0, |n| n.min(3) as u8),
                    None => 0,
                };
                if let Some(b) = bearing(t) {
                    self.director.order_rushed(Order::ExploreWay(b, secrets), rush);
                    (true, "OK.".into())
                } else if self.director.is_area(agent, t) {
                    self.director.order_rushed(Order::ExploreArea(t.to_string(), secrets), rush);
                    (true, "OK.".into())
                } else {
                    (false, format!("\"{t}\" isn't an area or a direction."))
                }
            }
            "doom_name" => {
                let (Some(r), Some(n)) = (arg("ref"), arg("name")) else { return (false, "Give ref and name.".into()) };
                let Some(e) = self.director.find(agent, obs, r) else { return (false, format!("Nothing known as \"{r}\".")) };
                match self.director.name(&e.id, n) {
                    Ok(()) => (true, format!("{} is \"{n}\" now.", e.id)),
                    Err(why) => (false, format!("Not named: {why}.")),
                }
            }
            "doom_exit" => {
                if self.director.list(agent, obs, Topic::Exits).is_empty() {
                    // (not a failure to retry: nothing to do about it but explore)
                    (true, "No exit known yet.".into())
                } else if let Some(q) = self.director.exit_check(agent, obs).filter(|_| !sure) {
                    // (a question, not a failure: Neuro would send it again as it was)
                    (true, q)
                } else {
                    self.director.order(Order::Exit);
                    (true, "Going to the exit.".into())
                }
            }
            "doom_stop" => {
                self.director.order(Order::Stop);
                (true, "OK.".into())
            }
            "doom_status" => (true, self.director.status(agent, obs)),
            _ => (false, format!("No action \"{name}\".")),
        }
    }
}
