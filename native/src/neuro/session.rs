//! The game's side of the Neuro integration, a tic at a time: her actions checked, answered at
//! once (before they're carried out: Neuro waits for the answer) and turned into orders; what
//! happens told as context (notable things not silent); her state now and then; and, when the bot's
//! been waiting on her a while, a forced decision (the SDK's way for a game blocked on her).

use serde_json::{json, Value};

use super::link::{Incoming, Link};
use crate::control::directed::{bearing, Director, Hints, Order, Topic, NAME_MAX};
use crate::motor::Observation;
use crate::play::Agent;

/// waiting on her this long (tics) with nothing to do: a decision forced
const IDLE_FORCE: i64 = 6 * 35;
/// her state told (silently) at most this often, when it's changed
const STATUS_EVERY: i64 = 20 * 35;
/// a list in an answer: at most this many lines
const LIST_MAX: usize = 15;
/// exploring searches this thoroughly as it goes (1: the likeliest few walls; `doom_search` for
/// more)
const EXPLORE_SECRETS: u8 = 1;
/// at most this many moments (not silent) a minute: past it, what's not urgent is told
/// silently (her context and her attention are what's being spent)
const MOMENTS_A_MINUTE: usize = 6;
const MINUTE: i64 = 60 * 35;
/// a question unanswered this long (tics): a decision forced
const ASK_FORCE: i64 = 3 * 35;
/// the rules told again (silently) this often, or after this many characters told her since,
/// whichever comes first: her context rolls, and the rules shouldn't roll out of it
const BRIEF_EVERY: i64 = 10 * MINUTE;
const BRIEF_CHARS: usize = 30_000;

pub struct Session {
    link: Link,
    /// Neuro's address, for connecting again
    url: String,
    /// connected now, and who to (the startup's acknowledgement)
    connected: bool,
    who: String,
    character: Character,
    pub director: Director,
    /// a force out, not yet answered by an action; the question last forced
    forced: bool,
    forced_question: Option<u32>,
    /// when she last had nothing to do (or the last force or action)
    idle_since: i64,
    /// when her status was last told, and its gist then (`Director::status_gist`)
    told_status: (i64, String),
    shown: usize,
    /// when the moments lately were told, for the budget
    loud: std::collections::VecDeque<i64>,
    /// told the rules on connecting
    briefed: bool,
    /// when the rules were last told, and the characters told her since (context and results)
    briefed_at: i64,
    told_chars: usize,
    /// her status, a second old at most: what doom_status answers off a level
    last_status: String,
}

/// Who's playing, by the `characterId` of the startup's acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Character {
    /// not said yet (or a server that doesn't say)
    Unknown,
    Neuro,
    Evil,
    /// someone else, by the id given
    Other(String),
}

impl Character {
    pub fn of(id: &str) -> Character {
        match id.trim().to_lowercase().as_str() {
            "" => Character::Unknown,
            "neuro" => Character::Neuro,
            "evil" => Character::Evil,
            other => Character::Other(other.to_string()),
        }
    }
}

/// The actions, as registered (descriptions and schemas Neuro reads as they are).
pub fn actions() -> Vec<Value> {
    let s = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    // (short: every word here is in Evil's context for the whole game)
    vec![
        json!({ "name": "doom_look", "description": "What's round you: the room, what it leads to, monsters, items, doors and switches in sight, sounds, a suggestion." }),
        json!({ "name": "doom_list", "description": "List known areas, doors, switches, items, monsters or exits, with ids, the likeliest first. Areas: only unexplored, current and named ones unless all; a/b/c walls to search: at secrets 1/2/3.", "schema": s(json!({ "what": { "type": "string", "enum": ["areas", "doors", "switches", "items", "monsters", "exits"] }, "all": { "type": "boolean" } }), &["what"]) }),
        json!({ "name": "doom_press", "description": "Press a switch. Exploring never presses them. rush, then: as for doom_go.", "schema": s(json!({ "ref": { "type": "string" }, "rush": { "type": "boolean" }, "then": { "type": "boolean" } }), &["ref"]) }),
        json!({ "name": "doom_inspect", "description": "What something is and where.", "schema": s(json!({ "ref": { "type": "string" } }), &["ref"]) }),
        json!({ "name": "doom_go", "description": "Go to an area, item, door, switch or exit. risky: cross hurting floors if survivable. sure: exit even with things left. rush: keep going mid-fight, shooting and dodging on the way. then: after the current action, not instead (exploring resumes after a go anyway).", "schema": s(json!({ "ref": { "type": "string" }, "risky": { "type": "boolean" }, "sure": { "type": "boolean" }, "rush": { "type": "boolean" }, "then": { "type": "boolean" } }), &["ref"]) }),
        json!({ "name": "doom_explore", "description": "Explore an area or a direction (N, NE, ...), the likeliest secret walls too; paused while you're fighting.", "schema": s(json!({ "target": { "type": "string" }, "then": { "type": "boolean" } }), &["target"]) }),
        json!({ "name": "doom_search", "description": "Search an area or a direction for secrets: thorough 2 (most suspicious walls, default) or 3 (every one, slow).", "schema": s(json!({ "target": { "type": "string" }, "thorough": { "type": "integer", "enum": [2, 3] }, "then": { "type": "boolean" } }), &["target"]) }),
        json!({ "name": "doom_name", "description": format!("Name something (up to {NAME_MAX} characters); the name then works as its id."), "schema": s(json!({ "ref": { "type": "string" }, "name": { "type": "string" } }), &["ref", "name"]) }),
        json!({ "name": "doom_exit", "description": "Leave the level. sure: even with things left.", "schema": s(json!({ "sure": { "type": "boolean" } }), &[]) }),
        json!({ "name": "doom_fight", "description": "Your fight, for a while (10 s unless seconds; then by reflex again). target: ref a monster id or kind, shot first. weapon: ref a gun, or auto. retreat: to cover. hold_fire / fire. bold / careful / avoid (keep clear: to get past) / steady: how you fight. auto: all by reflex again.", "schema": s(json!({ "do": { "type": "string", "enum": ["target", "weapon", "retreat", "hold_fire", "fire", "bold", "careful", "avoid", "steady", "auto"] }, "ref": { "type": "string" }, "seconds": { "type": "integer" } }), &["do"]) }),
        json!({ "name": "doom_policy", "description": format!("Standing settings; give any, or none to hear them. {} idle: seconds with no action before you carry on by instinct (0: you wait). hints: suggestions, off, on_stall or always. answers: seconds a question waits before its default (0: till you answer).", crate::control::directed::policy_help()), "schema": s(json!({ "stance": { "type": "string", "enum": ["careful", "steady", "bold", "avoid", "yolo"] }, "save": { "type": "string", "enum": ["cells", "none"] }, "risk": { "type": "string", "enum": ["careful", "normal", "bold"] }, "powerups": { "type": "string", "enum": ["need", "now"] }, "idle": { "type": "integer" }, "hints": { "type": "string", "enum": ["off", "on_stall", "always"] }, "answers": { "type": "integer" } }), &[]) }),
        json!({ "name": "doom_game", "description": "The game itself, when it's yours. start: a game, when you're offered one (map, skill 1-5, play single or continuous). next: on to the next level from the tally screen. restart: this level from its start, after a while with nothing new.", "schema": s(json!({ "do": { "type": "string", "enum": ["start", "next", "restart"] }, "map": { "type": "string" }, "skill": { "type": "integer" }, "play": { "type": "string", "enum": ["single", "continuous"] } }), &["do"]) }),
        json!({ "name": "doom_answer", "description": "Answer a question ([ask q3] ...): id, and choice by its letter.", "schema": s(json!({ "id": { "type": "string" }, "choice": { "type": "string" } }), &["id", "choice"]) }),
        json!({ "name": "doom_stop", "description": "Stop and wait." }),
        json!({ "name": "doom_status", "description": "Health, armor, weapons, ammo, keys, tally, what you're doing, what you're fighting." }),
    ]
}

/// The rules, told on connecting and at each level. Everything she's told is told to her as
/// the one playing ("you found a secret"), never of a player she directs ("she found").
const BRIEFING: &str = "You're playing Doom. Moving, fighting and taking spare health, armor and ammo you do by reflex; you choose where you go. \
Ids come from doom_list; your names work as ids. \"NE 400\": north-east of you, 400 units away. (tally): counts for the items %. An action done, you wait for the next; with none for 6 s, you carry on by instinct (doom_policy idle).";

impl Session {
    pub fn new(url: &str) -> Session {
        Session {
            link: Link::start(url, actions()),
            url: url.to_string(),
            connected: false,
            who: String::new(),
            character: Character::Unknown,
            director: Director::new(),
            forced: false,
            forced_question: None,
            idle_since: 0,
            told_status: (i64::MIN / 2, String::new()),
            shown: 0,
            loud: std::collections::VecDeque::new(),
            briefed: false,
            briefed_at: 0,
            told_chars: 0,
            last_status: String::new(),
        }
    }

    /// Told her (context): counted, for telling the rules again.
    fn tell(&mut self, text: &str, silent: bool) {
        self.told_chars += text.len();
        self.link.context(text, silent);
    }

    /// The rules told her (silently), and the count since started again.
    fn brief(&mut self, tic: i64) {
        self.link.context(BRIEFING, true);
        self.briefed = true;
        self.briefed_at = tic;
        self.told_chars = 0;
    }

    /// An action's result sent: counted as what's told her.
    fn answer(&mut self, id: &str, ok: bool, msg: &str) {
        self.told_chars += msg.len();
        self.link.result(id, ok, msg);
    }

    /// A tic: what's come in handled; what's happened told; a decision forced if she's waited.
    /// The lines for the log.
    pub fn tic(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        let mut log = Vec::new();
        while let Ok(i) = self.link.incoming.try_recv() {
            match i {
                Incoming::Connected => {
                    log.push("neuro: connected".into());
                    self.connected = true;
                    self.forced = false;
                    self.idle_since = obs.tic;
                    if !self.briefed {
                        self.brief(obs.tic);
                    }
                    // (her context survives a reconnect: a short status, not everything again)
                    let status = self.director.status(agent, obs);
                    self.tell(&status, true);
                    self.told_status = (obs.tic, self.director.status_gist(agent, obs));
                    // (a question open: asked again, what went out before may not have reached
                    // her; forced again when it's due)
                    if let Some(line) = self.director.question().map(|q| q.line(obs.tic)) {
                        self.tell(&line, false);
                        self.forced_question = None;
                    }
                }
                Incoming::Hello { character, display } => {
                    log.push(format!("neuro: playing with {display} ({character})"));
                    self.who = format!("{display} ({character})");
                    self.character = Character::of(&character);
                }
                Incoming::Lost(why) => {
                    self.connected = false;
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
                    log.push(format!(
                        "<< {}{}",
                        if ok { "" } else { "FAILED: " },
                        msg.lines().next().unwrap_or("")
                    ));
                    self.answer(&id, ok, &msg);
                }
            }
        }
        if self.connected
            && (obs.tic - self.briefed_at >= BRIEF_EVERY || self.told_chars >= BRIEF_CHARS)
        {
            self.brief(obs.tic);
        }
        self.director.tick(agent, obs);
        if obs.tic % 35 == 0 {
            self.last_status = self.director.status(agent, obs);
        }
        self.tell_news(obs.tic, &mut log);
        // (her status again when what she'd act on has changed: not a shell spent, nor a
        // monster's bearing)
        let gist = self.director.status_gist(agent, obs);
        if gist != self.told_status.1 && obs.tic - self.told_status.0 >= STATUS_EVERY {
            let status = self.director.status(agent, obs);
            self.tell(&status, true);
            self.told_status = (obs.tic, gist);
        }
        // (a question unanswered a few seconds: a decision forced, the question its state; never
        // one that can't wait, its moment gone by the time a force is answered)
        // (no force while the link's down: it'd go out stale when it's back, and a reconnect
        // asks again anyway)
        if let Some(q) = self
            .director
            .question()
            .filter(|q| !q.now && self.connected)
        {
            if self.forced_question != Some(q.id) && obs.tic - q.opened >= ASK_FORCE {
                self.forced_question = Some(q.id);
                self.forced = true;
                self.link.force(
                    &q.line(obs.tic),
                    &format!("Answer q{}?", q.id),
                    &[
                        "doom_answer",
                        "doom_go",
                        "doom_explore",
                        "doom_exit",
                        "doom_stop",
                    ],
                    "high",
                );
                log.push(format!("neuro: q{} unanswered: decision forced", q.id));
            }
        }
        // (waiting on her: after a while, a decision forced; one at a time)
        if !self.director.idle(agent) {
            self.idle_since = obs.tic;
        } else if self.connected && !self.forced && obs.tic - self.idle_since >= IDLE_FORCE {
            let state = self.state(agent, obs);
            self.link.force(
                &state,
                "You're waiting. What next?",
                &[
                    "doom_explore",
                    "doom_search",
                    "doom_go",
                    "doom_exit",
                    "doom_look",
                    "doom_list",
                    "doom_inspect",
                    "doom_name",
                    "doom_status",
                ],
                "medium",
            );
            self.forced = true;
            log.push("neuro: waiting on her: decision forced".into());
        }
        log
    }

    /// What's happened since last told: one message, not silent if any of it's notable (and
    /// the moments a minute not over).
    fn tell_news(&mut self, tic: i64, log: &mut Vec<String>) {
        let news: Vec<_> = self.director.reports[self.shown..].to_vec();
        self.shown = self.director.reports.len();
        if !news.is_empty() {
            let text: Vec<String> = news.iter().map(|r| r.text.clone()).collect();
            while self.loud.front().is_some_and(|&t| tic - t >= MINUTE) {
                self.loud.pop_front();
            }
            // (a moment over the budget told quietly; one that's urgent, always, and not
            // counted against it: answers to her actions don't crowd out what she should see)
            let urgent = news.iter().any(crate::control::directed::Report::urgent);
            let moment = news
                .iter()
                .any(|r| r.event.weight() == crate::control::directed::Weight::Moment);
            let within = moment && self.loud.len() < MOMENTS_A_MINUTE;
            if within {
                self.loud.push_back(tic);
            }
            let notable = urgent || within;
            log.extend(
                text.iter()
                    .map(|t| format!("<< {}{t}", if notable { "(!) " } else { "" })),
            );
            self.tell(&text.join("\n"), !notable);
        }
    }

    /// Off a level (the title, a game over, the end of what she chose, the tally screen): what
    /// comes in handled, `doom_game` by `act` (checked: (success, message));
    /// any other action answered `idle`: why there's no level to do it in. What's been told
    /// since, passed on. The lines for the log.
    pub fn off_level(
        &mut self,
        tic: i64,
        idle: &str,
        mut act: impl FnMut(&Value) -> (bool, String),
    ) -> Vec<String> {
        let mut log = Vec::new();
        while let Ok(i) = self.link.incoming.try_recv() {
            match i {
                Incoming::Connected => {
                    log.push("neuro: connected".into());
                    self.connected = true;
                    self.forced = false;
                    if !self.briefed {
                        self.brief(tic);
                    }
                }
                Incoming::Hello { character, display } => {
                    log.push(format!("neuro: playing with {display} ({character})"));
                    self.who = format!("{display} ({character})");
                    self.character = Character::of(&character);
                }
                Incoming::Lost(why) => {
                    self.connected = false;
                    if self.briefed {
                        log.push(format!("neuro: connection lost ({why}); trying again"));
                    }
                    self.forced = false;
                }
                Incoming::Action { id, name, data } => {
                    let args: Value = data
                        .as_deref()
                        .and_then(|d| serde_json::from_str(d).ok())
                        .unwrap_or(Value::Null);
                    let (args, notes) = mend(&name, args);
                    let (ok, msg) = if name == "doom_game" {
                        let (ok, msg) = act(&args);
                        (ok, noted(msg, &notes))
                    } else if name == "doom_status" && !self.last_status.is_empty() {
                        // (her status still stands off a level: asked, given, with why there's
                        // nothing to do)
                        (true, format!("{idle} Last: {}", self.last_status))
                    } else {
                        (false, idle.to_string())
                    };
                    log.push(format!(">> {name} {}", data.as_deref().unwrap_or("")));
                    log.push(format!("<< {}{msg}", if ok { "" } else { "FAILED: " }));
                    self.answer(&id, ok, &msg);
                }
            }
        }
        if self.connected
            && (tic - self.briefed_at >= BRIEF_EVERY || self.told_chars >= BRIEF_CHARS)
        {
            self.brief(tic);
        }
        self.tell_news(tic, &mut log);
        log
    }

    /// Between levels (the tally, a finale): what the director's been told of the screens,
    /// passed on; the lines for the log. (Her actions wait for the next level.)
    pub fn between(&mut self, tic: i64) -> Vec<String> {
        let mut log = Vec::new();
        self.tell_news(tic, &mut log);
        log
    }

    /// The link dropped and made again (the stream console's): what she's been told, her orders
    /// and names kept; the old link's thread ends as it finds its channels gone.
    pub fn reconnect(&mut self) {
        self.link = Link::start(&self.url, actions());
        self.connected = false;
        self.forced = false;
    }

    /// Which of the twins is playing, once the startup's been acknowledged: for what's worth
    /// telling one differently from the other.
    pub fn character(&self) -> &Character {
        &self.character
    }

    /// (Neuro's address, connected, who's playing)
    pub fn link_state(&self) -> (&str, bool, &str) {
        (&self.url, self.connected, &self.who)
    }

    /// Her state and options, for a forced decision (Markdown, no top-level heading).
    fn state(&mut self, agent: &mut Agent, obs: &Observation) -> String {
        let mut s = format!("## Status\n{}\n", self.director.status(agent, obs));
        let mut marked = false;
        for (topic, title, n) in [
            (Topic::Areas, "Areas", 8),
            (Topic::Exits, "Exits", 3),
            (Topic::Doors, "Doors", 5),
            (Topic::Switches, "Switches", 5),
            (Topic::Items, "Items", 6),
        ] {
            let l = self.director.listing(agent, obs, topic, false, n);
            if l.lines.is_empty() && l.hidden == 0 {
                continue;
            }
            s.push_str(&format!("\n## {title}\n"));
            for e in &l.lines {
                s.push_str(&format!("- {e}\n"));
            }
            if l.hidden > 0 {
                s.push_str(&format!("- +{} explored\n", l.hidden));
            }
            marked |= l.lines.iter().any(|e| e.contains("(suggested"));
        }
        // (the bot's pick, if no line above was it)
        if let Some(p) = self.director.pick_line(agent, obs).filter(|_| !marked) {
            s.push_str(&format!("\nSuggested: {p}.\n"));
        }
        s
    }

    /// A line from the streamer (the stream console's), told her as it is: not silent, it's for her
    /// to hear.
    pub fn say(&mut self, text: &str) {
        self.tell(&format!("The streamer: {text}"), false);
    }

    /// An action of hers given by the streamer (the stream console's, for when she's stuck):
    /// checked and carried out as hers is, the answer back to the streamer. An order (not a look:
    /// a list, an inspection, her status) she's told of, quietly, with what came of it: her orders
    /// may have changed under her.
    pub fn streamer_action(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        name: &str,
        data: Option<&str>,
    ) -> (bool, String) {
        let (ok, msg) = self.action(agent, obs, name, data);
        if !matches!(
            name,
            "doom_list" | "doom_look" | "doom_inspect" | "doom_status"
        ) {
            let first = msg.lines().next().unwrap_or("");
            self.tell(
                &format!(
                    "The streamer did {name} {}: {}{first}",
                    data.unwrap_or(""),
                    if ok { "" } else { "failed: " }
                ),
                true,
            );
            self.idle_since = obs.tic;
        }
        (ok, msg)
    }

    /// An action checked, carried out (an order queued), answered: (success, message).
    fn action(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        name: &str,
        data: Option<&str>,
    ) -> (bool, String) {
        let args: Value = match data.map(serde_json::from_str::<Value>) {
            None => Value::Null,
            Some(Ok(v)) => v,
            Some(Err(_)) => return (false, "The action's data wasn't valid JSON.".into()),
        };
        let (mut args, mut notes) = mend(name, args);
        // (a place as lists give it, "SW 305", for an id: what's listed nearest there)
        if let Some(r) = args.get("ref").and_then(Value::as_str).map(str::to_string) {
            if let Some(id) = self.director.at_position(agent, obs, &r) {
                notes.push(format!("{r} read as {id}"));
                args["ref"] = Value::String(id);
            }
        }
        let (ok, msg) = self.act(agent, obs, name, &args);
        (ok, noted(msg, &notes))
    }

    /// An action, its data mended, carried out: (success, message).
    fn act(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        name: &str,
        args: &Value,
    ) -> (bool, String) {
        let arg = |k: &str| {
            args.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let sure = args.get("sure").and_then(Value::as_bool).unwrap_or(false);
        let rush = args.get("rush").and_then(Value::as_bool).unwrap_or(false);
        let then = args.get("then").and_then(Value::as_bool).unwrap_or(false);
        match name {
            "doom_list" => {
                let Some(topic) = arg("what").and_then(Topic::parse) else {
                    return (
                        false,
                        "what: areas, doors, switches, items, monsters or exits.".into(),
                    );
                };
                let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
                let l = self.director.listing(agent, obs, topic, all, LIST_MAX);
                let mut more = if l.hidden > 0 {
                    format!("\n(+{} explored; all: true lists them)", l.hidden)
                } else {
                    String::new()
                };
                if let Some(p) = &l.pick {
                    more.push_str(&format!("\nSuggested: {p}."));
                }
                if l.lines.is_empty() {
                    return (true, format!("None known yet.{more}"));
                }
                let lines: Vec<String> = l.lines.iter().map(|e| format!("- {e}")).collect();
                (true, format!("{}{more}", lines.join("\n")))
            }
            "doom_look" => (true, self.director.look(agent, obs).join("\n")),
            "doom_policy" => {
                let idle = args.get("idle").and_then(Value::as_f64);
                let answers = args.get("answers").and_then(Value::as_f64);
                let hints = match arg("hints").map(Hints::parse) {
                    Some(None) => return (false, "hints: off, on_stall or always.".into()),
                    Some(h) => h,
                    None => None,
                };
                // (her settings first: one refused, or yolo asked about, is answered at once)
                for setting in ["stance", "save", "risk", "powerups"] {
                    if let Some(v) = arg(setting) {
                        match self.director.set_policy(obs, setting, v) {
                            Ok(s) if s.starts_with("[ask") => return (true, s),
                            Ok(_) => {}
                            Err(why) => return (false, why),
                        }
                    }
                }
                if let Some(secs) = answers {
                    self.director.answer_wait = (secs > 0.0).then_some((secs * 35.0) as i64);
                }
                if let Some(secs) = idle {
                    self.director.idle_auto = (secs > 0.0).then_some((secs * 35.0) as i64);
                }
                if let Some(h) = hints {
                    self.director.hints = h;
                }
                let idle = match self.director.idle_auto {
                    Some(t) => format!("carry on by instinct after {} s with no action", t / 35),
                    None => "wait for the next action".to_string(),
                };
                let answers = match self.director.answer_wait {
                    Some(t) => format!("questions wait {} s", t / 35),
                    None => "questions wait for you".to_string(),
                };
                (
                    true,
                    format!(
                        "Your policy: {}. You {idle}; hints {}; {answers}.",
                        self.director.policy.line(),
                        self.director.hints.name()
                    ),
                )
            }
            "doom_inspect" => {
                let Some(r) = arg("ref") else {
                    return (false, "ref: an id or name.".into());
                };
                match self.director.find(agent, obs, r) {
                    Some(e) => (true, self.director.describe(&e, obs)),
                    None => (false, self.director.not_found(agent, obs, r)),
                }
            }
            "doom_go" => {
                let Some(r) = arg("ref") else {
                    return (false, "ref: an id or name.".into());
                };
                let risky = args.get("risky").and_then(Value::as_bool).unwrap_or(false);
                if self.director.resolve(r).starts_with("exit-") && !sure {
                    if let Some(q) = self.director.ask_leave(agent, obs) {
                        return (true, q);
                    }
                }
                // (a way that costs: asked; not to be retried as it is otherwise, the reason
                // says what would do)
                match self.director.ask_go(agent, obs, r, risky, rush, then) {
                    Ok(None) => (true, "OK.".into()),
                    Ok(Some(q)) => (true, q),
                    Err(why) => (false, why),
                }
            }
            "doom_press" => {
                let Some(r) = arg("ref") else {
                    return (false, "ref: a switch's id or name.".into());
                };
                let id = self.director.resolve(r);
                if !id.starts_with("switch-") && !id.starts_with("door-") {
                    return (false, format!("\"{r}\" isn't a switch."));
                }
                match self.director.check_go(agent, obs, r, false) {
                    Ok(_) => {
                        self.director
                            .order_with(Order::Go(r.to_string(), false), rush, then);
                        (true, "OK.".into())
                    }
                    Err(why) => (false, why),
                }
            }
            "doom_explore" | "doom_search" => {
                let Some(t) = arg("target") else {
                    return (false, "target: an area or a direction (N, NE, ...).".into());
                };
                // (exploring searches the likeliest few walls as it goes; a search, most of them
                // or every one)
                let secrets = if name == "doom_search" {
                    args.get("thorough")
                        .and_then(Value::as_u64)
                        .map_or(2, |n| n.clamp(2, 3) as u8)
                } else {
                    EXPLORE_SECRETS
                };
                if let Some(b) = bearing(t) {
                    self.director
                        .order_with(Order::ExploreWay(b, secrets), rush, then);
                    (true, "OK.".into())
                } else if self.director.is_area(agent, t) {
                    self.director.order_with(
                        Order::ExploreArea(t.to_string(), secrets),
                        rush,
                        then,
                    );
                    (true, "OK.".into())
                } else if self.director.resolve(t).starts_with("spot-")
                    && self.director.check_go(agent, obs, t, false).is_ok()
                {
                    // (a spot, as suggested: somewhere to go, not an area to explore)
                    self.director
                        .order_with(Order::Go(t.to_string(), false), rush, then);
                    (true, format!("OK: going to {t} (a spot, not an area)."))
                } else {
                    (false, format!("\"{t}\" isn't an area or a direction."))
                }
            }
            "doom_name" => {
                let (Some(r), Some(n)) = (arg("ref"), arg("name")) else {
                    return (false, "Give ref and name.".into());
                };
                let Some(e) = self.director.find(agent, obs, r) else {
                    return (false, self.director.not_found(agent, obs, r));
                };
                match self.director.name(&e.id, n) {
                    Ok(()) => (true, format!("{} is \"{n}\" now.", e.id)),
                    Err(why) => (false, format!("Not named: {why}.")),
                }
            }
            "doom_exit" => {
                if self.director.list(agent, obs, Topic::Exits).is_empty() {
                    // (not a failure to retry: nothing to do about it but explore)
                    (true, "No exit known yet.".into())
                } else if let Some(q) = (!sure)
                    .then(|| self.director.ask_leave(agent, obs))
                    .flatten()
                {
                    // (a question, not a failure: Neuro would send it again as it was)
                    (true, q)
                } else {
                    self.director.order(Order::Exit);
                    (true, "Going to the exit.".into())
                }
            }
            "doom_fight" => {
                let Some(what) = arg("do") else {
                    return (
                        false,
                        "do: target, weapon, hold_fire, fire, bold, careful, steady or auto."
                            .into(),
                    );
                };
                let seconds = args.get("seconds").and_then(Value::as_f64);
                match self.director.fight(agent, obs, what, arg("ref"), seconds) {
                    Ok(said) => (true, said),
                    Err(why) => (false, why),
                }
            }
            "doom_stop" => {
                self.director.order(Order::Stop);
                (true, "OK.".into())
            }
            "doom_status" => (true, self.director.status(agent, obs)),
            "doom_answer" => {
                let (Some(id), Some(choice)) = (arg("id"), arg("choice")) else {
                    return (false, "Give id and choice.".into());
                };
                match self.director.answer(agent, obs, id, choice) {
                    Ok(said) => (true, said),
                    Err(why) => (false, why),
                }
            }
            "doom_game" => match crate::control::game::GameAct::of_json(args) {
                Ok(crate::control::game::GameAct::Restart) => {
                    match self.director.ask_restart(obs.tic) {
                        Ok(said) => (true, said),
                        Err(why) => (false, why),
                    }
                }
                Ok(_) => (
                    false,
                    "You're in a level: a new game, or the next level, when it's over.".into(),
                ),
                Err(why) => (false, why),
            },
            _ => (false, format!("No action \"{name}\".")),
        }
    }
}

/// An action's data mended against its registered schema (`repair`), and the notes of fields
/// taken under another name.
fn mend(name: &str, args: Value) -> (Value, Vec<String>) {
    let schema = actions()
        .into_iter()
        .find(|a| a["name"] == name)
        .and_then(|a| a.get("schema").cloned());
    super::repair::repair(args, schema.as_ref())
}

/// A result's message with what was mended in her data, so she gives it right next time.
fn noted(msg: String, notes: &[String]) -> String {
    if notes.is_empty() {
        msg
    } else {
        format!("{msg} ({})", notes.join(", "))
    }
}
