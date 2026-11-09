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
    vec![
        json!({ "name": "doom_list", "description": "List what you know of in the level: areas (rooms and places with something left to explore, where she is, and those you've named; set all for every area), doors not yet tried, switches not yet pressed (and what each does, if known), items, monsters, exits. Each has an id to use in other actions.", "schema": s(json!({ "what": { "type": "string", "enum": ["areas", "doors", "switches", "items", "monsters", "exits"] }, "all": { "type": "boolean" } }), &["what"]) }),
        json!({ "name": "doom_press", "description": "Press a switch (by its id or your name for it). Exploring doesn't press switches: they can do anything, so that's your call. She reports what it did, if she sees.", "schema": s(json!({ "ref": { "type": "string" } }), &["ref"]) }),
        json!({ "name": "doom_inspect", "description": "Say what one thing is, which way and how far, by its id or your name for it.", "schema": s(json!({ "ref": { "type": "string" } }), &["ref"]) }),
        json!({ "name": "doom_go", "description": "Go to a place, pick up an item, try a door or switch, or take an exit, by its id or your name for it. She does it and reports back. Set risky to go even where she'd rather not (across a floor that hurts), as long as it won't kill her.", "schema": s(json!({ "ref": { "type": "string" }, "risky": { "type": "boolean" } }), &["ref"]) }),
        json!({ "name": "doom_explore", "description": "Explore an area (by its id or your name for it) or a direction (north, south-east, ...), until there's nothing more known to explore there. Set secrets to also search for secrets: she presses (and shoots) every wall there she suspects of hiding something, which takes a while.", "schema": s(json!({ "target": { "type": "string" }, "secrets": { "type": "boolean" } }), &["target"]) }),
        json!({ "name": "doom_name", "description": format!("Give something a name of your own (up to {NAME_MAX} characters), to use instead of its id from then on."), "schema": s(json!({ "ref": { "type": "string" }, "name": { "type": "string" } }), &["ref", "name"]) }),
        json!({ "name": "doom_exit", "description": "Leave the level by the exit she knows of (ends the level)." }),
        json!({ "name": "doom_stop", "description": "Stop what she's doing and wait." }),
        json!({ "name": "doom_status", "description": "Her health, armor, weapons, ammo, keys, and what she's doing." }),
    ]
}

/// The rules, told on connecting and at each level.
const BRIEFING: &str = "You're directing a Doom player. She moves, aims, shoots and dodges by herself; you decide where she goes and what she explores. \
Use doom_list to see what's known (areas, doors, switches, items, monsters, exits: each with an id), doom_explore to explore an area or a direction (with secrets set to search its walls for secrets too), doom_go to go to something, \
doom_press to press a switch (exploring finds switches but leaves pressing them to you), doom_name to name things your way, doom_exit to finish the level. She reports what happens and waits for you when she's done.";

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
            self.link.force(&state, "She's waiting for your next order. Where should she go, or what should she explore?", &["doom_explore", "doom_go", "doom_exit", "doom_list", "doom_inspect", "doom_name", "doom_status"], "medium");
            self.forced = true;
            log.push("neuro: waiting on her: decision forced".into());
        }
        log
    }

    /// Her state and options, for a forced decision (Markdown, no top-level heading).
    fn state(&mut self, agent: &mut Agent, obs: &Observation) -> String {
        let mut s = format!("## Status\n{}\n", self.director.status(agent, obs));
        for (topic, title, n) in [(Topic::Areas, "Areas", 8), (Topic::Exits, "Exits", 3), (Topic::Doors, "Doors not yet tried", 5), (Topic::Switches, "Switches not yet pressed", 5), (Topic::Items, "Items seen", 6)] {
            let (list, hidden) = self.director.brief(agent, obs, topic, false);
            if list.is_empty() && hidden == 0 {
                continue;
            }
            s.push_str(&format!("\n## {title}\n"));
            for e in list.iter().take(n) {
                s.push_str(&format!("- {}\n", self.director.describe(e, obs)));
            }
            if hidden > 0 {
                s.push_str(&format!("- (and {hidden} explored areas)\n"));
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
        match name {
            "doom_list" => {
                let Some(topic) = arg("what").and_then(Topic::parse) else { return (false, "List what? One of: areas, doors, items, monsters, exits.".into()) };
                let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
                let (list, hidden) = self.director.brief(agent, obs, topic, all);
                let more = if hidden > 0 { format!("\n(and {hidden} explored areas: list with all to see them)") } else { String::new() };
                if list.is_empty() {
                    return (true, format!("None known yet.{more}"));
                }
                let lines: Vec<String> = list.iter().take(LIST_MAX).map(|e| format!("- {}", self.director.describe(e, obs))).collect();
                (true, format!("{} known:\n{}{more}", list.len(), lines.join("\n")))
            }
            "doom_inspect" => {
                let Some(r) = arg("ref") else { return (false, "Inspect what? Give an id from doom_list, or your name for it.".into()) };
                match self.director.find(agent, obs, r) {
                    Some(e) => (true, self.director.describe(&e, obs)),
                    None => (false, format!("Nothing known as \"{r}\". Use doom_list for ids.")),
                }
            }
            "doom_go" => {
                let Some(r) = arg("ref") else { return (false, "Go to what? Give an id from doom_list, or your name for it.".into()) };
                let risky = args.get("risky").and_then(Value::as_bool).unwrap_or(false);
                match self.director.check_go(agent, obs, r, risky) {
                    Ok(what) => {
                        self.director.order(Order::Go(r.to_string(), risky));
                        (true, format!("On her way: {what}. She'll report when she's there or if she can't."))
                    }
                    // (not to be retried as it is: the reason says what would do)
                    Err(why) => (false, why),
                }
            }
            "doom_press" => {
                let Some(r) = arg("ref") else { return (false, "Press what? A switch's id from doom_list switches, or your name for it.".into()) };
                let id = self.director.resolve(r);
                if !id.starts_with("switch-") && !id.starts_with("door-") {
                    return (false, format!("\"{r}\" isn't a switch. doom_list switches for the switches."));
                }
                match self.director.check_go(agent, obs, r, false) {
                    Ok(what) => {
                        self.director.order(Order::Go(r.to_string(), false));
                        (true, format!("On her way to press it: {what}. She'll say what it did."))
                    }
                    Err(why) => (false, why),
                }
            }
            "doom_explore" => {
                let Some(t) = arg("target") else { return (false, "Explore what? An area's id or your name for it, or a direction (north, south-east, ...).".into()) };
                let secrets = args.get("secrets").and_then(Value::as_bool).unwrap_or(false);
                let too = if secrets { " (searching for secrets too)" } else { "" };
                if let Some(b) = bearing(t) {
                    self.director.order(Order::ExploreWay(b, secrets));
                    (true, format!("Exploring {t}{too}; she'll report when there's nothing more that way."))
                } else if self.director.is_area(agent, t) {
                    self.director.order(Order::ExploreArea(t.to_string(), secrets));
                    (true, format!("Exploring {t}{too}; she'll report when there's nothing more there."))
                } else {
                    (false, format!("\"{t}\" isn't an area or a direction. Use doom_list areas for areas, or say north, south-east, ..."))
                }
            }
            "doom_name" => {
                let (Some(r), Some(n)) = (arg("ref"), arg("name")) else { return (false, "Name what, and what as? Give ref and name.".into()) };
                let Some(e) = self.director.find(agent, obs, r) else { return (false, format!("Nothing known as \"{r}\" to name. Use doom_list for ids.")) };
                match self.director.name(&e.id, n) {
                    Ok(()) => (true, format!("{} is \"{n}\" now.", e.id)),
                    Err(why) => (false, format!("Not named: {why}. Pick another name.")),
                }
            }
            "doom_exit" => {
                if self.director.list(agent, obs, Topic::Exits).is_empty() {
                    // (not a failure to retry: nothing to do about it but explore)
                    (true, "No exit known yet: explore to find one.".into())
                } else {
                    self.director.order(Order::Exit);
                    (true, "Heading for the exit.".into())
                }
            }
            "doom_stop" => {
                self.director.order(Order::Stop);
                (true, "Stopping; she'll wait for your next order.".into())
            }
            "doom_status" => (true, self.director.status(agent, obs)),
            _ => (false, format!("There's no action \"{name}\" in Doom.")),
        }
    }
}
