// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The game's side of the Neuro integration, a tic at a time: her actions checked, answered at
//! once (before they're carried out: Neuro waits for the answer) and turned into orders; what
//! happens told as context (notable things not silent); her state now and then; and, when the bot's
//! been waiting on her a while, a forced decision (the SDK's way for a game blocked on her).

use serde_json::{json, Value};

use super::link::{Incoming, Link};
use crate::control::directed::{bearing, Director, Order, Topic};
use crate::control::game::Choices;
use crate::domain::Observation;
use crate::play::Agent;
use crate::tactical::tuning::Directed;

// (how long she's waited for, and how often she's put to: the tuning's `directed`, the console
// changing some as it runs: `set_forcing`)
/// a list in an answer: at most this many lines
const LIST_MAX: usize = 15;
const MINUTE: i64 = 60 * 35;
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
    /// the force out, if one is (`Forcing`: one at a time); the question last forced
    forced: Forcing,
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
    /// for the stream console: what she's been told lately (tic, text, not silent), her actions
    /// lately, the console's last answer (its number); the tic last seen
    told_lines: std::collections::VecDeque<(i64, String, bool)>,
    exchanges: std::collections::VecDeque<Exchange>,
    reply: Option<(u64, String, bool, String)>,
    now_tic: i64,
    /// the actions registered with Neuro now (`sync_actions`): only those that make sense where
    /// she is, changed only when that changes (a few times a level)
    registered: std::collections::BTreeSet<&'static str>,
    /// how long (tics) she's waited with nothing to do, and a question's gone unanswered, before
    /// a decision's forced on her (0: never); the console's to change
    idle_force: i64,
    ask_force: i64,
    /// after a look or a list, her status at most so often, the moments a minute (the tuning's)
    after_look: i64,
    status_every: i64,
    moments: usize,
    /// asked what next while she's on with something (`Directed::next_force`: at most this often, 0
    /// never): the moment waiting to be asked about ("step", "fight"), and when she was last
    /// asked or last acted
    next_force: i64,
    /// ...and without a word aloud (the SDK's `silent`: it comes often)
    next_silent: bool,
    /// her talking taken as over this long (tics) after its last part with no end heard
    speech_stale: i64,
    next_due: Option<&'static str>,
    next_at: i64,
    /// the settings not hers to change (the stream's say; of `LOCKABLE`): each stays as it is
    /// (instinct: on), isn't in her schema, and is refused if sent anyway; an action with all
    /// its settings locked isn't registered at all. `policy_stale`, `goal_stale`: that action
    /// registered with another schema than it has now
    locked: std::collections::BTreeSet<&'static str>,
    policy_stale: bool,
    goal_stale: bool,
    /// the action being carried out is the console's (the stream's own hand: no lock on it)
    console_hand: bool,
    /// the games hers to start and what of the game is hers (the watch's; doom_game's schema
    /// made from them), and doom_game registered with older ones
    choices: Option<Choices>,
    game_stale: bool,
    /// the game's paused (the watcher's, the console's: as the watch has it)
    pub paused: bool,
    /// off a level: since when a game's been hers to start, and when a force for it went out
    /// (the wall's clock: the game's doesn't run there)
    off_since: Option<std::time::Instant>,
    off_forced: Option<std::time::Instant>,
    /// she's mid-sentence (`speech_finished`, not the final one yet): since when it was last
    /// heard of
    speaking: Option<i64>,
}

/// The force out, if one is. Neuro handles one at a time, and a second sent meanwhile "will
/// cause problems" (the SDK's specification): so none is sent while one's out. It's over when
/// she answers it and that's accepted (one refused, the server puts the force to her again by
/// itself: it's still out), or when it's gone unanswered so long the server must have dropped
/// it (its retries spent, or a result of ours it gave up waiting for: it doesn't say).
///
/// Which of her actions is its answer the protocol doesn't say, and one of its own done of her
/// own accord often comes first (begun before the force reached her; against Gary, three
/// forces in four): only the answer to the question forced is surely it. Another of its
/// actions accepted, the force is still out for `settle` more, for the answer to come.
struct Forcing {
    out: Option<Out>,
    timeout: i64,
    settle: i64,
}

struct Out {
    at: i64,
    names: Vec<&'static str>,
    /// over at this tic: one of its actions accepted that may not have been its answer
    over: Option<i64>,
}

impl Forcing {
    fn new(timeout: i64, settle: i64) -> Forcing {
        Forcing {
            out: None,
            timeout,
            settle,
        }
    }

    fn out(&self) -> bool {
        self.out.is_some()
    }

    fn sent(&mut self, tic: i64, names: Vec<&'static str>) {
        self.out = Some(Out {
            at: tic,
            names,
            over: None,
        });
    }

    /// An action of hers come in, accepted or not. `surely`: it can only be the force's answer
    /// (the question forced, answered by its id; or no clock runs to wait by).
    fn answered(&mut self, name: &str, surely: bool, ok: bool, tic: i64) {
        let Some(out) = self.out.as_mut() else {
            return;
        };
        if !out.names.contains(&name) {
            return; // (not this force's: something she did of her own accord meanwhile)
        }
        if !ok {
            out.at = tic; // (put to her again by the server: waited for afresh)
        } else if surely || self.settle == 0 {
            self.out = None;
        } else if out.over.is_none() {
            out.over = Some(tic + self.settle);
        }
    }

    /// The clock's tick. True once: unanswered past the timeout (0: never), and so no longer
    /// out. (Settled meanwhile: no longer out either, and nothing to say.)
    fn expire(&mut self, tic: i64) -> bool {
        let Some(out) = self.out.as_ref() else {
            return false;
        };
        if out.over.is_some_and(|t| tic >= t) {
            self.out = None;
            return false;
        }
        let over = self.timeout > 0 && tic - out.at >= self.timeout;
        if over {
            self.out = None;
        }
        over
    }

    fn clear(&mut self) {
        self.out = None;
    }

    /// The clock gone back (a new level).
    fn back(&mut self, by: i64) {
        if let Some(out) = self.out.as_mut() {
            out.at -= by;
            out.over = out.over.map(|t| t - by);
        }
    }
}

/// The question a `doom_answer` names (`{"id":"q4",...}`: "4"), if it's readable.
fn answered_id(data: Option<&str>) -> Option<String> {
    let v: Value = serde_json::from_str(data?).ok()?;
    let id = &v["id"];
    let id = id
        .as_str()
        .map(str::to_string)
        .or_else(|| id.as_u64().map(|n| n.to_string()))?;
    Some(id.trim().trim_start_matches(['q', 'Q']).to_string())
}

/// Every action's name, as `actions` has them.
const ALL_ACTIONS: &[&str] = &[
    "doom_look",
    "doom_query",
    "doom_go",
    "doom_exit",
    "doom_fight",
    "doom_policy",
    "doom_game",
    "doom_answer",
    "doom_status",
    "doom_goal",
];

/// Off a level (the title, a game over, the tally, a finale): the game itself, her answers, her
/// status, and what stands across levels (her settings, a goal).
const OFF_LEVEL: &[&str] = &[
    "doom_game",
    "doom_answer",
    "doom_status",
    "doom_policy",
    "doom_goal",
];

/// The game paused (by whoever's at the stream): what doesn't move her is answered as ever
/// (asking, answering, settings); what would is told it's paused.
const PAUSED: &[&str] = &[
    "doom_look",
    "doom_query",
    "doom_status",
    "doom_answer",
    "doom_policy",
    "doom_goal",
];

/// An action and what came back, for the stream console.
struct Exchange {
    tic: i64,
    name: String,
    data: String,
    ok: bool,
    reply: String,
    /// given from the stream console, as hers
    by_console: bool,
}

/// lines told and actions kept for the stream console
const KEPT_LINES: usize = 40;

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
    actions_timed(&Directed::default())
}

/// ...with the times her say in a fight holds as `c` has them.
pub fn actions_timed(c: &Directed) -> Vec<Value> {
    let (tactic, stance) = (c.tactic, c.stance);
    let s = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    // (short: every word here is in Evil's context for the whole game)
    vec![
        json!({ "name": "doom_look", "description": "What's round you: the room, what it leads to, monsters, items, doors and switches in sight, sounds." }),
        json!({ "name": "doom_query", "description": "What you know of. all: the areas, doors, switches, items, monsters or exits you know of, with ids, the first to take first (monsters: nearest first), 15 at most. ref: one thing by its id or name: what it is and where. include_explored: areas with nothing left in them too.", "schema": s(json!({ "all": { "type": "string", "enum": ["areas", "doors", "switches", "items", "monsters", "exits"] }, "ref": { "type": "string" }, "include_explored": { "type": "boolean" } }), &[]) }),
        json!({ "name": "doom_go", "description": "Go to an area, item, door, switch or exit: a thing is picked up, pressed or opened. explore: none (just go); or the area (or a compass direction, N, NE...) gone over as you get there: sweep, secrets (its suspicious walls too) or everything (every wall: slow); it pauses while you fight. risky: cross hurting floors if survivable. rush: keep going mid-fight, shooting and dodging on the way (to an area, till you're in it). queue_up: after what you're doing, not instead.", "schema": s(json!({ "ref": { "type": "string" }, "explore": { "type": "string", "enum": ["none", "sweep", "secrets", "everything"] }, "risky": { "type": "boolean" }, "rush": { "type": "boolean" }, "queue_up": { "type": "boolean" } }), &["ref"]) }),
        json!({ "name": "doom_exit", "description": "Leave the level by the nearest exit. With things left you're asked first." }),
        json!({ "name": "doom_fight", "description": format!("Your say in the fight, for a while; give any together. target: a monster id or kind to shoot first ({tactic:.0}s; none ends it). weapon: a gun you hold, used while it has a shot ({tactic:.0}s; auto ends it). fire: hold, or free ({tactic:.0}s). override_stance: how hard you fight, over your policy's ({stance:.0}s). retreat: fall back to cover now. cancel: drop all of these. seconds: a different length. None given: what's holding now."), "schema": s(json!({ "target": { "type": "string" }, "weapon": { "type": "string" }, "fire": { "type": "string", "enum": ["hold", "free"] }, "override_stance": { "type": "string", "enum": ["bold", "careful", "avoid", "steady"] }, "retreat": { "type": "boolean" }, "cancel": { "type": "boolean" }, "seconds": { "type": "integer" } }), &[]) }),
        json!({ "name": "doom_policy", "description": "Your settings, kept till you change them. Give any to change them; none to hear them all and what each does.", "schema": s(json!({ "stance": { "type": "string", "enum": ["careful", "steady", "bold", "avoid", "yolo"] }, "save": { "type": "string", "enum": ["cells", "none"] }, "risk": { "type": "string", "enum": ["careful", "normal", "bold"] }, "powerups": { "type": "string", "enum": ["need", "now"] }, "explore": { "type": "string", "enum": ["normal", "methodical", "speedy"] }, "instinct": { "type": "string", "enum": ["on", "off"] } }), &[]) }),
        json!({ "name": "doom_game", "description": "The game itself, when it's yours. start: a game, when you're offered one (map; skill 1 easiest to 4 hardest; play single level or continuous). next: on to the next level from the tally screen. restart: this level from its start, after a while with nothing new.", "schema": s(json!({ "do": { "type": "string", "enum": ["start", "next", "restart"] }, "map": { "type": "string" }, "skill": { "type": "integer", "enum": [1, 2, 3, 4] }, "play": { "type": "string", "enum": ["single", "continuous"] } }), &["do"]) }),
        json!({ "name": "doom_answer", "description": "Answer a question ([ask q3] ...): id, and choice by its letter.", "schema": s(json!({ "id": { "type": "string" }, "choice": { "type": "string" } }), &["id", "choice"]) }),
        json!({ "name": "doom_status", "description": "Your current status." }),
        json!({ "name": "doom_goal", "description": "Two standing aims; neither plays for you: each puts what it's after first in what's suggested to you and in what you do by instinct. finish: when the level's done: out (by the first exit), kills, secrets or completionist (all of it: slow, asked first), each then out; none: you're asked. prefer: what comes first meanwhile: health (while under 100), ammo (while short of a cyberdemon's worth), hunt (the monsters you know of), unexplored (new ground), or none. Neither given: what's set.", "schema": s(json!({ "finish": { "type": "string", "enum": ["none", "out", "kills", "secrets", "completionist"] }, "prefer": { "type": "string", "enum": ["none", "health", "ammo", "hunt", "unexplored"] } }), &[]) }),
    ]
}

/// `doom_policy`'s word on instinct, in what it answers with nothing given: left out when it
/// isn't hers to turn off.
const INSTINCT_HELP: &str = " instinct: on (an action done, you carry on by yourself and are asked what next), off (you stand and wait).";

pub use crate::control::directed::LOCKABLE;
const POLICY_SETTINGS: &[&str] = &["stance", "save", "risk", "powerups", "explore", "instinct"];
const GOAL_SETTINGS: &[&str] = &["finish", "prefer"];

const GOAL_FINISH: &str = "finish: when the level's done: out (by the first exit), kills, secrets or completionist (all of it: slow, asked first), each then out; none: you're asked.";
const GOAL_PREFER: &str = "prefer: what comes first meanwhile: health (while under 100), ammo (while short of a cyberdemon's worth), hunt (the monsters you know of), unexplored (new ground), or none.";

/// The schema an action's data is mended against: hers as registered, the locked settings out
/// of it; the console's, whole. A lock is on her, not on the page: mended against hers, a
/// locked setting the console applied was dropped as a field the schema hasn't got, and the
/// setting stayed as it was.
fn mending_schema(
    mut all: Vec<Value>,
    locked: &std::collections::BTreeSet<&'static str>,
    console: bool,
    name: &str,
) -> Option<Value> {
    if !console {
        without(&mut all, locked);
    }
    all.into_iter()
        .find(|a| a["name"] == name)
        .and_then(|a| a.get("schema").cloned())
}

/// The actions without the settings locked: out of `doom_policy`'s and `doom_goal`'s schemas,
/// and `doom_goal`'s description of the one that's left; either action gone with nothing left
/// of it.
fn without(all: &mut Vec<Value>, locked: &std::collections::BTreeSet<&'static str>) {
    if locked.is_empty() {
        return;
    }
    for (name, settings) in [
        ("doom_policy", POLICY_SETTINGS),
        ("doom_goal", GOAL_SETTINGS),
    ] {
        if settings.iter().all(|k| locked.contains(k)) {
            all.retain(|a| a["name"] != name);
            continue;
        }
        let Some(a) = all.iter_mut().find(|a| a["name"] == name) else {
            continue;
        };
        if let Some(props) = a["schema"]["properties"].as_object_mut() {
            props.retain(|k, _| !locked.contains(k.as_str()));
        }
        if name == "doom_goal" && settings.iter().any(|k| locked.contains(k)) {
            let (one, what) = if locked.contains("finish") {
                ("prefer", GOAL_PREFER)
            } else {
                ("finish", GOAL_FINISH)
            };
            a["description"] = json!(format!(
                "A standing aim; it doesn't play for you: it puts what it's after first in what's suggested to you and in what you do by instinct. {what} Not given: what's set."
            ));
            debug_assert!(a["schema"]["properties"][one].is_object());
        }
    }
}

/// The rules, told on connecting and at each level. Everything she's told is told to her as
/// the one playing ("you found a secret"), never of a player she directs ("she found").
const BRIEFING: &str = "You're playing Doom. Moving, fighting and taking spare health, armor and ammo you do by reflex; you choose where you go. \
Ids come from doom_query; your names work as ids. \"NE 400\": north-east of you, 400 units away. (tally): counts for the items %. An action done, you carry on by instinct, not leaving the level, and you're asked what next (doom_policy instinct).";

impl Session {
    pub fn new(url: &str, c: &Directed) -> Session {
        let mut director = Director::new();
        director.set_clocks(c);
        Session {
            link: Link::start(url),
            url: url.to_string(),
            connected: false,
            who: String::new(),
            character: Character::Unknown,
            director,
            forced: Forcing::new(
                Directed::tics(c.force_timeout),
                Directed::tics(c.force_settle),
            ),
            forced_question: None,
            idle_since: 0,
            told_status: (i64::MIN / 2, String::new()),
            shown: 0,
            loud: std::collections::VecDeque::new(),
            briefed: false,
            briefed_at: 0,
            told_chars: 0,
            last_status: String::new(),
            told_lines: std::collections::VecDeque::new(),
            exchanges: std::collections::VecDeque::new(),
            reply: None,
            now_tic: 0,
            registered: std::collections::BTreeSet::new(),
            idle_force: Directed::tics(c.idle_force),
            ask_force: Directed::tics(c.ask_force),
            next_force: Directed::tics(c.next_force),
            next_silent: c.next_silent,
            speech_stale: Directed::tics(c.speech_stale),
            after_look: Directed::tics(c.after_look),
            status_every: Directed::tics(c.status_every),
            moments: c.moments_a_minute,
            next_due: None,
            next_at: 0,
            locked: std::collections::BTreeSet::new(),
            policy_stale: false,
            goal_stale: false,
            console_hand: false,
            choices: None,
            game_stale: false,
            paused: false,
            off_since: None,
            off_forced: None,
            speaking: None,
        }
    }

    /// Told her (context): counted, for telling the rules again; kept for the stream console.
    fn tell(&mut self, text: &str, silent: bool) {
        self.told_chars += text.len();
        self.link.context(text, silent);
        self.told_lines
            .push_back((self.now_tic, text.to_string(), !silent));
        while self.told_lines.len() > KEPT_LINES {
            self.told_lines.pop_front();
        }
    }

    /// An action and its answer, kept for the stream console.
    fn keep(&mut self, name: &str, data: Option<&str>, ok: bool, reply: &str, by_console: bool) {
        self.exchanges.push_back(Exchange {
            tic: self.now_tic,
            name: name.to_string(),
            data: data.unwrap_or("").to_string(),
            ok,
            reply: reply.to_string(),
            by_console,
        });
        while self.exchanges.len() > KEPT_LINES {
            self.exchanges.pop_front();
        }
    }

    /// For the stream console: what she's been told lately, her actions lately, a minute's
    /// worth (her actions, words told, moments), the console's last answer.
    pub fn console_lines(&self) -> crate::console::EvilState {
        use crate::console::{ActionLine, EvilState, ToldLine};
        let secs = |tic: i64| tic as f64 / 35.0;
        let lately = |tic: i64| self.now_tic - tic <= MINUTE;
        EvilState {
            told: self
                .told_lines
                .iter()
                .map(|(tic, text, loud)| ToldLine {
                    seconds: secs(*tic),
                    text: text.clone(),
                    loud: *loud,
                })
                .collect(),
            actions: self
                .exchanges
                .iter()
                .map(|e| ActionLine {
                    seconds: secs(e.tic),
                    name: e.name.clone(),
                    data: e.data.clone(),
                    ok: e.ok,
                    reply: e.reply.clone(),
                    by_console: e.by_console,
                })
                .collect(),
            per_minute: [
                self.exchanges
                    .iter()
                    .filter(|e| !e.by_console && lately(e.tic))
                    .count() as u32,
                self.told_lines
                    .iter()
                    .filter(|(tic, _, _)| lately(*tic))
                    .map(|(_, t, _)| t.split_whitespace().count() as u32)
                    .sum(),
                self.loud.len() as u32,
            ],
            reply: self.reply.clone(),
            forcing: [
                self.idle_force as f64 / 35.0,
                self.ask_force as f64 / 35.0,
                self.next_force as f64 / 35.0,
            ],
            instinct_hers: !self.locked.contains("instinct"),
            locked: self.locked.iter().map(ToString::to_string).collect(),
            next_silent: self.next_silent,
            answer_wait: self.director.answer_wait.map_or(0.0, |t| t as f64 / 35.0),
            ..EvilState::default()
        }
    }

    /// How soon a decision's forced on her, in seconds (the console's): waiting with nothing to
    /// do, a question unanswered, asked what next while she's on with something (at most that
    /// often); 0 never, None as it was. (Kept to an hour at most.)
    pub fn set_forcing(&mut self, idle: Option<f64>, ask: Option<f64>, next: Option<f64>) {
        if let Some(s) = next {
            self.next_force = (s.clamp(0.0, 3600.0) * 35.0).round() as i64;
        }
        let tics = |s: f64| (s.clamp(0.0, 3600.0) * 35.0).round() as i64;
        if let Some(s) = idle {
            self.idle_force = tics(s);
        }
        if let Some(s) = ask {
            self.ask_force = tics(s);
        }
    }

    /// The time now. The clock gone back (a new level: its tics start from nothing): what's
    /// timed from the old clock is put back as far, each as long ago as it was. Else she isn't
    /// pushed, asked what next or told a moment aloud till this level's as old as the last was.
    fn clock(&mut self, tic: i64) {
        if tic < self.now_tic {
            let back = self.now_tic - tic;
            self.idle_since -= back;
            self.forced.back(back);
            self.speaking = self.speaking.map(|t| t - back);
            self.next_at -= back;
            self.briefed_at -= back;
            self.told_status.0 -= back;
            self.loud.iter_mut().for_each(|t| *t -= back);
            self.told_lines.iter_mut().for_each(|l| l.0 -= back);
            self.exchanges.iter_mut().for_each(|e| e.tic -= back);
        }
        self.now_tic = tic;
    }

    /// She's talking: the last heard of what she's saying wasn't its end, and that not long
    /// ago (an end that never came isn't waited for for ever).
    fn speaking(&self, tic: i64) -> bool {
        self.speaking.is_some_and(|t| tic - t < self.speech_stale)
    }

    /// Whether she's asked what next without a word aloud (the console's).
    pub fn set_next_silent(&mut self, silent: bool) {
        self.next_silent = silent;
    }

    /// How long a question waits for her answer before its default's taken, in seconds (the
    /// console's, not hers: she can't know how long her answers take); 0: till she answers.
    pub fn set_answer_wait(&mut self, secs: f64) {
        let secs = secs.clamp(0.0, 3600.0);
        self.director.answer_wait = (secs > 0.0).then_some((secs * 35.0).round() as i64);
    }

    /// The settings not hers to change, from now (the stream's say; names of `LOCKABLE`,
    /// others passed over): each stays as it is, leaves her action's schema, and is refused
    /// if sent anyway. Instinct locked is instinct on.
    pub fn set_locked<S: AsRef<str>>(&mut self, names: &[S]) {
        let new: std::collections::BTreeSet<&'static str> = LOCKABLE
            .iter()
            .copied()
            .filter(|l| names.iter().any(|n| n.as_ref().trim() == *l))
            .collect();
        if new.contains("instinct") {
            self.director.idle_auto = Some(crate::control::directed::IDLE_AUTO);
        }
        let changed = |of: &[&str]| {
            of.iter()
                .any(|k| new.contains(k) != self.locked.contains(k))
        };
        let (policy, goal) = (changed(POLICY_SETTINGS), changed(GOAL_SETTINGS));
        self.policy_stale |= policy;
        self.goal_stale |= goal;
        self.locked = new;
    }

    /// The settings not hers to change now.
    pub fn locked(&self) -> Vec<String> {
        self.locked.iter().map(ToString::to_string).collect()
    }

    /// Is this setting hers to change now? (The console's own hand always may.)
    fn hers(&self, setting: &str) -> bool {
        self.console_hand || !self.locked.contains(setting)
    }

    /// The rules told her (silently), and the count since started again.
    fn brief(&mut self, tic: i64) {
        // (instinct not hers to turn off: its setting not named)
        if !self.locked.contains("instinct") {
            self.link.context(BRIEFING, true);
        } else {
            self.link
                .context(&BRIEFING.replace(" (doom_policy instinct)", ""), true);
        }
        // (co-op: who she is in the game, and that the others are players)
        if let Some(t) = &self.director.team {
            self.link.context(&t.brief(), true);
        }
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
    /// What's come from Neuro: connections, actions (done, and answered).
    fn receive(&mut self, agent: &mut Agent, obs: &Observation, log: &mut Vec<String>) {
        while let Ok(i) = self.link.incoming.try_recv() {
            match i {
                Incoming::Connected => {
                    log.push("neuro: connected".into());
                    self.connected = true;
                    // (a new connection has nothing registered: what fits now, below)
                    self.registered.clear();
                    self.game_stale = false;
                    self.forced.clear();
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
                    self.forced.clear();
                    self.speaking = None;
                }
                Incoming::Speech { last } => self.speaking = (!last).then_some(obs.tic),
                Incoming::Action { id, name, data } => {
                    if matches!(
                        name.as_str(),
                        "doom_go" | "doom_exit" | "doom_answer" | "doom_goal" | "doom_game"
                    ) {
                        self.idle_since = obs.tic;
                        // (she's just had her say: not asked what next on top of it)
                        self.next_due = None;
                        self.next_at = obs.tic;
                    } else {
                        // (a look, a list, her status, a setting: time to act on it, no more.
                        // One after another doesn't put off her being asked)
                        self.idle_since = self
                            .idle_since
                            .max(obs.tic - self.idle_force + self.after_look);
                    }
                    // (the question forced, answered by its id: the force's answer for sure)
                    let surely = name == "doom_answer"
                        && self.forced_question.is_some_and(|q| {
                            answered_id(data.as_deref()).is_some_and(|id| id == q.to_string())
                        });
                    let (ok, msg) = self.action(agent, obs, &name, data.as_deref());
                    self.forced.answered(&name, surely, ok, obs.tic);
                    log.push(format!(">> {name} {}", data.as_deref().unwrap_or("")));
                    log.push(format!(
                        "<< {}{}",
                        if ok { "" } else { "FAILED: " },
                        msg.lines().next().unwrap_or("")
                    ));
                    self.keep(&name, data.as_deref(), ok, &msg, false);
                    self.answer(&id, ok, &msg);
                }
            }
        }
    }

    /// The game paused: what she sends answered, nothing else (no tic of the director,
    /// nothing told her, no decision forced on her). Her actions stay registered as they
    /// were: a pause is the stream's, and short.
    pub fn tic_paused(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        let mut log = Vec::new();
        self.receive(agent, obs, &mut log);
        log
    }

    /// The game paused or not, as it now is (played on: her wait for an action starts
    /// afresh).
    pub fn set_paused(&mut self, on: bool, tic: i64) {
        if self.paused && !on {
            self.idle_since = tic;
            self.forced.clear();
        }
        self.paused = on;
    }

    /// The lines for the log.
    pub fn tic(&mut self, agent: &mut Agent, obs: &Observation) -> Vec<String> {
        self.clock(obs.tic);
        // (a level: what was forced off one, a game to start, is over with)
        if self.off_forced.take().is_some() {
            self.forced.clear();
        }
        self.off_since = None;
        let mut log = Vec::new();
        crate::debug::timed("neuro.receive", || self.receive(agent, obs, &mut log));
        if self.connected
            && (obs.tic - self.briefed_at >= BRIEF_EVERY || self.told_chars >= BRIEF_CHARS)
        {
            self.brief(obs.tic);
        }
        crate::debug::timed("director.tick", || self.director.tick(agent, obs));
        if obs.tic % 35 == 0 {
            self.last_status =
                crate::debug::timed("director.status", || self.director.status(agent, obs));
        }
        if self.forced.expire(obs.tic) {
            log.push("neuro: the force went unanswered: taken as dropped".into());
        }
        // (a question that can't wait: forced at once, by how much it matters, its default
        // done already and her answer switching it while it's open. She may answer in half
        // a second or in two: either's inside its few. With another force out it can't be:
        // then it's told her aloud as before, and that's all)
        let urgent = self
            .director
            .question()
            .filter(|q| q.now && self.connected && self.forced_question != Some(q.id))
            .filter(|_| !self.forced.out())
            .map(|q| (q.id, q.line(obs.tic), q.priority, q.about.clone()));
        // (its line, new this tic, quietly then: the force says it, and one prompt's enough)
        self.tell_news(obs.tic, urgent.is_some(), &mut log);
        if let Some((id, line, priority, about)) = urgent {
            self.forced_question = Some(id);
            let names: &[&str] = if about == crate::control::directed::About::Fight {
                &["doom_answer", "doom_fight"]
            } else {
                &["doom_answer"]
            };
            self.force(
                obs.tic,
                &line,
                &format!("q{id}: switch it?"),
                names,
                priority,
            );
            log.push(format!("neuro: q{id} can't wait: forced ({priority})"));
        }
        // (her status again when what she'd act on has changed: not a shell spent, nor a
        // monster's bearing)
        let gist = self.director.status_gist(agent, obs);
        if gist != self.told_status.1 && obs.tic - self.told_status.0 >= self.status_every {
            let status = self.director.status(agent, obs);
            self.tell(&status, true);
            self.told_status = (obs.tic, gist);
        }
        // (a question unanswered a few seconds: a decision forced, the question its state; never
        // one that can't wait, its moment gone by the time a force is answered)
        // (no force while the link's down: it'd go out stale when it's back, and a reconnect
        // asks again anyway)
        // (one force at a time: with another out, this one waits for it)
        if let Some((id, line)) = self
            .director
            .question()
            .filter(|q| !q.now && self.connected)
            .filter(|q| self.forced_question != Some(q.id))
            .filter(|q| self.ask_force > 0 && obs.tic - q.opened >= self.ask_force)
            .map(|q| (q.id, q.line(obs.tic)))
            && !self.forced.out()
        {
            self.forced_question = Some(id);
            self.force(
                obs.tic,
                &line,
                &format!("Answer q{id}?"),
                &["doom_answer", "doom_go", "doom_exit"],
                "high",
            );
            log.push(format!("neuro: q{id} unanswered: decision forced"));
        }
        // (waiting on her: after a while, a decision forced; one at a time)
        if !self.director.idle(agent) {
            self.idle_since = obs.tic;
        } else if self.connected
            && !self.forced.out()
            && self.idle_force > 0
            && obs.tic - self.idle_since >= self.idle_force
        {
            let state = self.state(agent, obs);
            self.force(
                obs.tic,
                &state,
                "You're waiting. What next?",
                &[
                    "doom_go",
                    "doom_exit",
                    "doom_look",
                    "doom_query",
                    "doom_status",
                ],
                "medium",
            );
            log.push("neuro: waiting on her: decision forced".into());
        }
        // (on with something by herself, instinct's new step or after a fight: asked what
        // next, a question with carrying on its default; she goes on meanwhile. Not mid-fight, not over
        // a question, not while she's waiting: that's the force above)
        if let Some(due) = self.director.next_due.take() {
            // (instinct's first step after her order is that order's end still)
            if !(due == "step" && self.next_due.is_some()) {
                self.next_due = Some(due);
            }
        }
        if self.director.idle(agent) {
            self.next_due = None;
        }
        if let Some(due) = self.next_due.filter(|_| {
            self.connected
                && !self.forced.out()
                // (mid-sentence: asked when she's done. It'd wait for her anyway, and its
                // few seconds for an answer would be gone by then)
                && !self.speaking(obs.tic)
                && self.next_force > 0
                && (self.next_due == Some("done") && agent.x().current.is_some()
                    || self.next_due != Some("done") && obs.tic - self.next_at >= self.next_force)
                && !self.director.now.fighting()
                && self.director.question().is_none()
        }) {
            // (what she's at, without whose choice it was: "going to open a door")
            let doing = self.director.doing(agent);
            let what = match doing.split_once(": ") {
                Some((by, what)) if by == "by instinct" || by.starts_with("for your goal") => what,
                _ if doing == "by instinct" || doing.starts_with("for your goal") => "exploring",
                _ => doing.as_str(),
            };
            let text = if due == "fight" {
                format!("Fight over. Before it: {what}. Carry on, or something else?")
            } else {
                let mut c = what.chars();
                let first = c
                    .next()
                    .map_or(String::new(), |f| f.to_uppercase().collect());
                format!("{first}{}. Carry on, or something else?", c.as_str())
            };
            self.next_due = None;
            self.next_at = obs.tic;
            let Some(query) = self.director.ask_next(obs, text) else {
                return log;
            };
            // (put to her here: not forced again as a question left unanswered)
            self.forced_question = self.director.question().map(|q| q.id);
            // (short: it comes often, and carrying on needs none of the lists; a look or a
            // list is among her answers)
            let mut state = format!("## Status\n{}\n", self.director.status(agent, obs));
            if let Some(p) = self.director.pick_line(agent, obs) {
                state.push_str(&format!("\nSuggested: {p}.\n"));
            }
            state.push_str("\nUnsure? doom_look, or doom_query.\n");
            self.force_as(
                obs.tic,
                &state,
                &query,
                &[
                    "doom_answer",
                    "doom_go",
                    "doom_exit",
                    "doom_look",
                    "doom_query",
                ],
                "low",
                self.next_silent,
            );
            log.push(format!("neuro: asked what next ({due}): {query}"));
        }
        let wanted = self.level_actions(agent);
        self.sync_actions(wanted, &mut log);
        log
    }

    /// In a level: every action but the game's own (only where she may restart the level), and
    /// leaving only once she's seen an exit in this life (as stocking up waits for).
    fn level_actions(&self, agent: &Agent) -> std::collections::BTreeSet<&'static str> {
        let exit = agent.x().exit_seen_this_life();
        ALL_ACTIONS
            .iter()
            .copied()
            .filter(|&a| match a {
                "doom_exit" => exit,
                "doom_game" => self.director.restart_allowed,
                _ => true,
            })
            .collect()
    }

    /// Her actions, `doom_game` as what's offered her has it.
    fn actions(&self) -> Vec<Value> {
        let mut all = self.all_actions();
        without(&mut all, &self.locked);
        all
    }

    /// ...with the settings locked from her still in them.
    fn all_actions(&self) -> Vec<Value> {
        let mut all = actions_timed(&self.director.clocks);
        if let (Some(c), Some(g)) = (
            &self.choices,
            all.iter_mut().find(|a| a["name"] == "doom_game"),
        ) {
            *g = game_action(c);
        }
        all
    }

    /// An action's data mended against its registered schema (`repair`), and the notes of
    /// fields taken under another name.
    fn mend(&self, name: &str, args: Value) -> (Value, Vec<String>) {
        let schema = mending_schema(self.all_actions(), &self.locked, self.console_hand, name);
        super::repair::repair(args, schema.as_ref())
    }

    /// What's hers of the game, as the watch has it now (changed: `doom_game` registered
    /// again with its new schema).
    pub fn set_choices(&mut self, c: &Choices) {
        if self.choices.as_ref() != Some(c) {
            self.choices = Some(c.clone());
            self.game_stale = true;
        }
    }

    /// The actions registered with Neuro made `wanted`: those no longer wanted unregistered, the
    /// new ones registered (a change slows her answers a moment: only when where she is changes).
    fn sync_actions(
        &mut self,
        wanted: std::collections::BTreeSet<&'static str>,
        log: &mut Vec<String>,
    ) {
        if !self.connected {
            return;
        }
        // (an action with every setting locked has nothing left to offer her)
        let mut wanted = wanted;
        for (name, settings) in [
            ("doom_policy", POLICY_SETTINGS),
            ("doom_goal", GOAL_SETTINGS),
        ] {
            if settings.iter().all(|k| self.locked.contains(k)) {
                wanted.remove(name);
            }
        }
        // (what's hers of the game changed: doom_game again, with its new schema)
        if std::mem::take(&mut self.game_stale) && self.registered.remove("doom_game") {
            self.link.send(
                "actions/unregister",
                Some(json!({ "action_names": ["doom_game"] })),
            );
        }
        if std::mem::take(&mut self.goal_stale) && self.registered.remove("doom_goal") {
            self.link.send(
                "actions/unregister",
                Some(json!({ "action_names": ["doom_goal"] })),
            );
        }
        if std::mem::take(&mut self.policy_stale) && self.registered.remove("doom_policy") {
            self.link.send(
                "actions/unregister",
                Some(json!({ "action_names": ["doom_policy"] })),
            );
        }
        if wanted == self.registered {
            return;
        }
        let gone: Vec<&str> = self.registered.difference(&wanted).copied().collect();
        let new: Vec<Value> = self
            .actions()
            .into_iter()
            .filter(|a| {
                a["name"]
                    .as_str()
                    .is_some_and(|n| wanted.contains(n) && !self.registered.contains(n))
            })
            .collect();
        if !gone.is_empty() {
            self.link
                .send("actions/unregister", Some(json!({ "action_names": gone })));
            log.push(format!("neuro: unregistered {}", gone.join(", ")));
        }
        if !new.is_empty() {
            let names: Vec<&str> = new.iter().filter_map(|a| a["name"].as_str()).collect();
            log.push(format!("neuro: registered {}", names.join(", ")));
            self.link
                .send("actions/register", Some(json!({ "actions": new })));
        }
        self.registered = wanted;
    }

    /// A decision forced, of those of `names` registered now.
    /// (One at a time: the callers look at `forced.out()` first.)
    fn force(&mut self, tic: i64, state: &str, query: &str, names: &[&str], priority: &str) {
        self.force_as(tic, state, query, names, priority, false);
    }

    /// ...`silent`: picked without a word aloud.
    fn force_as(
        &mut self,
        tic: i64,
        state: &str,
        query: &str,
        names: &[&str],
        priority: &str,
        silent: bool,
    ) {
        let names: Vec<&'static str> = self
            .registered
            .iter()
            .copied()
            .filter(|n| names.contains(n))
            .collect();
        if !names.is_empty() {
            self.link.force(state, query, &names, priority, silent);
            self.forced.sent(tic, names);
        }
    }

    /// What's happened since last told: one message, not silent if any of it's notable (and
    /// the moments a minute not over).
    /// (`asked_quietly`: a question among it is being forced this tic, and isn't told aloud too.)
    fn tell_news(&mut self, tic: i64, asked_quietly: bool, log: &mut Vec<String>) {
        let news: Vec<_> = self.director.reports[self.shown..].to_vec();
        self.shown = self.director.reports.len();
        if !news.is_empty() {
            let text: Vec<String> = news.iter().map(|r| r.text.clone()).collect();
            while self.loud.front().is_some_and(|&t| tic - t >= MINUTE) {
                self.loud.pop_front();
            }
            // (a moment over the budget told quietly; one that's urgent, always, and not
            // counted against it: answers to her actions don't crowd out what she should see)
            let urgent = news.iter().any(|r| {
                r.urgent() && !(asked_quietly && r.event == crate::control::directed::Event::Asked)
            });
            let moment = news
                .iter()
                .any(|r| r.event.weight() == crate::control::directed::Weight::Moment);
            let within = moment && self.loud.len() < self.moments;
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
    ///
    /// `offer`: a game's hers to start, and nothing happens till she does (what's on offer, in
    /// words). That's the game blocked on her, the SDK's case for a force: after
    /// `idle_force` of it one's sent, and again whenever the last has gone unanswered its
    /// timeout. (By the wall's clock: off a level the game's own doesn't run.)
    pub fn off_level(
        &mut self,
        tic: i64,
        idle: &str,
        offer: Option<&str>,
        mut act: impl FnMut(&Value) -> (bool, String),
    ) -> Vec<String> {
        self.clock(tic);
        let mut log = Vec::new();
        let now = std::time::Instant::now();
        let waited = |since: Option<std::time::Instant>, tics: i64| {
            tics > 0
                && since.is_some_and(|t| now.duration_since(t).as_secs_f64() * 35.0 >= tics as f64)
        };
        if offer.is_none() {
            self.off_since = None;
        } else if self.off_since.is_none() {
            self.off_since = Some(now);
        }
        // (a force sent here and never answered: dropped by the server by now)
        if self.forced.out() && waited(self.off_forced, self.forced.timeout) {
            self.forced.clear();
            self.off_forced = None;
            self.off_since = Some(now);
            log.push("neuro: the force went unanswered: taken as dropped".into());
        }
        while let Ok(i) = self.link.incoming.try_recv() {
            match i {
                Incoming::Connected => {
                    log.push("neuro: connected".into());
                    self.connected = true;
                    // (a new connection has nothing registered: what fits now, below)
                    self.registered.clear();
                    self.game_stale = false;
                    self.forced.clear();
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
                    self.forced.clear();
                    self.speaking = None;
                }
                Incoming::Speech { last } => self.speaking = (!last).then_some(tic),
                Incoming::Action { id, name, data } => {
                    let args: Value = data
                        .as_deref()
                        .and_then(|d| serde_json::from_str(d).ok())
                        .unwrap_or(Value::Null);
                    let (args, notes) = self.mend(&name, args);
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
                    // (no level's clock to wait by here)
                    self.forced.answered(&name, true, ok, tic);
                    // (whatever she did: waited for afresh)
                    self.off_since = offer.map(|_| now);
                    log.push(format!(">> {name} {}", data.as_deref().unwrap_or("")));
                    log.push(format!("<< {}{msg}", if ok { "" } else { "FAILED: " }));
                    self.keep(&name, data.as_deref(), ok, &msg, false);
                    self.answer(&id, ok, &msg);
                }
            }
        }
        if self.connected
            && (tic - self.briefed_at >= BRIEF_EVERY || self.told_chars >= BRIEF_CHARS)
        {
            self.brief(tic);
        }
        self.tell_news(tic, false, &mut log);
        self.sync_actions(OFF_LEVEL.iter().copied().collect(), &mut log);
        if let Some(offer) = offer
            && self.connected
            && !self.forced.out()
            && waited(self.off_since, self.idle_force)
        {
            self.force(
                tic,
                offer,
                "No game's on. Start one?",
                &["doom_game"],
                "medium",
            );
            if self.forced.out() {
                self.off_forced = Some(now);
                log.push("neuro: a game's hers to start: decision forced".into());
            }
        }
        log
    }

    /// Between levels (the tally, a finale): what the director's been told of the screens,
    /// passed on; the lines for the log. (Her actions wait for the next level.)
    pub fn between(&mut self, tic: i64) -> Vec<String> {
        self.clock(tic);
        let mut log = Vec::new();
        self.tell_news(tic, false, &mut log);
        // (the game's own not hers here: going on is the screen's)
        let wanted = OFF_LEVEL.iter().copied().filter(|&a| a != "doom_game");
        self.sync_actions(wanted.collect(), &mut log);
        log
    }

    /// (co-op) Dead a moment, to be born again where she started: what the director's been
    /// told passed on, her actions left as they are (they're hers again in a second or two;
    /// anything she sends meanwhile is answered then).
    pub fn dead(&mut self, tic: i64) -> Vec<String> {
        self.clock(tic);
        let mut log = Vec::new();
        self.tell_news(tic, false, &mut log);
        log
    }

    /// The link dropped and made again (the stream console's): what she's been told, her orders
    /// and names kept; the old link's thread ends as it finds its channels gone.
    pub fn reconnect(&mut self) {
        self.link = Link::start(&self.url);
        self.registered.clear();
        self.connected = false;
        self.forced.clear();
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

    /// A line from whoever's at the stream console, told her as it is, by their name: not
    /// silent, it's for her to hear.
    pub fn say(&mut self, text: &str) {
        let line = format!("{}: {text}", self.director.console_name);
        self.tell(&line, false);
    }

    /// An action of hers given from the stream console (for when she's stuck): checked and
    /// carried out as hers is, the answer back to the console. An order (not a look: a list, an
    /// inspection, her status) she's told of, quietly, by who gave it, with what came of it: her
    /// orders may have changed under her.
    pub fn console_action(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        name: &str,
        data: Option<&str>,
    ) -> (bool, String) {
        self.clock(obs.tic);
        self.console_hand = true;
        let (ok, msg) = self.action(agent, obs, name, data);
        self.console_hand = false;
        self.keep(name, data, ok, &msg, true);
        let n = self.reply.as_ref().map_or(1, |r| r.0 + 1);
        self.reply = Some((n, name.to_string(), ok, msg.clone()));
        if !matches!(
            name,
            "doom_query" | "doom_look" | "doom_status" | "doom_why"
        ) {
            let first = msg.lines().next().unwrap_or("");
            self.tell(
                &format!(
                    "{} did {name} {}: {}{first}",
                    self.director.console_name,
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
        // (a locked setting sent anyway: refused by name, before the mending drops it as a
        // field her schema hasn't got)
        if matches!(name, "doom_policy" | "doom_goal")
            && let Some(k) = LOCKABLE
                .iter()
                .find(|k| args.get(**k).is_some() && !self.hers(k))
        {
            return (false, format!("{k}: not yours to change."));
        }
        let (mut args, mut notes) = self.mend(name, args);
        // (a place as lists give it, "SW 305", for an id: what's listed nearest there. Not
        // for exploring or searching, whose `ref` may be a direction: that's taken as it is)
        if name == "doom_go"
            && args
                .get("explore")
                .and_then(Value::as_str)
                .is_none_or(|e| e == "none")
            && let Some(r) = args.get("ref").and_then(Value::as_str).map(str::to_string)
            && let Some(id) = self.director.at_position(agent, obs, &r)
        {
            notes.push(format!("{r} read as {id}"));
            args["ref"] = Value::String(id);
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
        if self.paused && !PAUSED.contains(&name) {
            return (
                false,
                "The game's paused by the stream: wait for it to go on.".into(),
            );
        }
        let rush = args.get("rush").and_then(Value::as_bool).unwrap_or(false);
        let then = args
            .get("queue_up")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match name {
            "doom_query" => {
                // (one thing, by its id or name: what it is and where)
                if let Some(r) = arg("ref") {
                    return match self.director.find(agent, obs, r) {
                        Some(e) => (true, self.director.describe(&e, obs)),
                        None => (false, self.director.not_found(agent, obs, r)),
                    };
                }
                let Some(topic) = arg("all").and_then(Topic::parse) else {
                    return (
                        false,
                        "all: areas, doors, switches, items, monsters or exits; or ref: an id or name."
                            .into(),
                    );
                };
                let explored = args
                    .get("include_explored")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let l = self.director.listing(agent, obs, topic, explored, LIST_MAX);
                let mut more = String::new();
                // (a list cut short says so: she isn't left thinking that's all there is)
                if l.known > l.lines.len() {
                    more.push_str(&format!(
                        "\n({} of {} shown: the first to take)",
                        l.lines.len(),
                        l.known
                    ));
                }
                if l.hidden > 0 {
                    more.push_str(&format!(
                        "\n(+{} with nothing left in them: include_explored: true)",
                        l.hidden
                    ));
                }
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
                let instinct = match arg("instinct") {
                    Some("on") => Some(true),
                    Some("off") => Some(false),
                    Some(_) => return (false, "instinct: on or off.".into()),
                    None => None,
                };
                // (her settings first: one refused, or yolo asked about, is answered at once)
                if let Some(k) = POLICY_SETTINGS
                    .iter()
                    .find(|k| arg(k).is_some() && !self.hers(k))
                {
                    return (false, format!("{k}: not yours to change."));
                }
                for setting in ["stance", "save", "risk", "powerups", "explore"] {
                    if let Some(v) = arg(setting) {
                        match self.director.set_policy(obs, setting, v) {
                            Ok(s) if s.starts_with("[ask") => return (true, s),
                            Ok(_) => {}
                            Err(why) => return (false, why),
                        }
                    }
                }
                if let Some(on) = instinct {
                    self.director.idle_auto = on.then_some(crate::control::directed::IDLE_AUTO);
                }
                let idle = match self.director.idle_auto {
                    Some(_) => "carry on by instinct when an action's done".to_string(),
                    None => "wait for the next action (instinct off)".to_string(),
                };
                let now = format!("Your policy: {}. You {idle}.", self.director.policy.line());
                // (none given: what each does with it; her action's description doesn't
                // carry that, it'd sit in her context the whole game)
                let asked = ["stance", "save", "risk", "powerups", "explore", "instinct"]
                    .iter()
                    .any(|k| args.get(*k).is_some());
                if asked {
                    (true, now)
                } else {
                    // (what's hers to set only; the console's hand hears of them all)
                    let mut help: Vec<&str> = ["stance", "save", "risk", "powerups", "explore"]
                        .into_iter()
                        .filter(|k| self.hers(k))
                        .map(crate::control::directed::policy_help_for)
                        .collect();
                    if self.hers("instinct") {
                        help.push(INSTINCT_HELP.trim());
                    }
                    (true, format!("{now}\n{}", help.join(" ")))
                }
            }
            "doom_go" => {
                let Some(r) = arg("ref") else {
                    return (
                        false,
                        "ref: an id or name, or a direction to explore.".into(),
                    );
                };
                let risky = args.get("risky").and_then(Value::as_bool).unwrap_or(false);
                // (how thoroughly it's swept as she gets there, None: just gone to. A direction
                // is nowhere to go to, only a way to explore)
                let way = bearing(r);
                let sweep = match arg("explore") {
                    None | Some("none") => way.map(|_| self.director.policy.explore_secrets()),
                    // (`normal`: its name at first, and her policy's word for the same)
                    Some("sweep" | "normal") => Some(self.director.policy.explore_secrets()),
                    Some("secrets") => Some(2),
                    Some("everything") => Some(3),
                    Some(_) => {
                        return (false, "explore: none, sweep, secrets or everything.".into())
                    }
                };
                let area = way.is_none() && self.director.is_area(agent, r);
                if let Some(secrets) = sweep.filter(|_| way.is_some() || area) {
                    // (every secret found: nothing behind a wall to search for)
                    let done = self.director.secrets_done();
                    let said = if secrets >= 2 && done {
                        let (got, of) = self.director.tally.secrets;
                        format!("OK. Every secret here is found ({got}/{of}): no walls to search.")
                    } else {
                        "OK.".to_string()
                    };
                    if let Some(b) = way {
                        self.director
                            .order_with(Order::ExploreWay(b, secrets), false, then);
                    } else if rush && !self.director.stands_in(agent, obs, r) {
                        // (rushed: to the area through whatever's in the way, then swept as
                        // any other, pausing for a fight: a rushed sweep is a run round a
                        // room under fire)
                        self.director
                            .order_with(Order::Go(r.to_string(), risky), true, then);
                        self.director.order_with(
                            Order::ExploreArea(r.to_string(), secrets),
                            false,
                            true,
                        );
                    } else {
                        self.director.order_with(
                            Order::ExploreArea(r.to_string(), secrets),
                            false,
                            then,
                        );
                    }
                    return (true, said);
                }
                if self.director.resolve(r).starts_with("exit-")
                    && let Some(q) = self.director.ask_leave(agent, obs)
                {
                    return (true, q);
                }
                // (a way that costs: asked; not to be retried as it is otherwise, the reason
                // says what would do)
                // (what she died after doing, hers to do again: done, with a word of it)
                let note = self.director.death_note(agent, obs, r);
                // (one thing isn't swept: said, so she doesn't think it was)
                let unswept = sweep.map_or("", |_| {
                    " (explore is for an area or a direction: just going.)"
                });
                match self.director.ask_go(agent, obs, r, risky, rush, then) {
                    Ok(None) => (
                        true,
                        format!(
                            "OK.{}{unswept}",
                            note.map_or(String::new(), |n| format!(" {n}"))
                        ),
                    ),
                    Ok(Some(q)) => (true, q),
                    Err(why) => (false, why),
                }
            }
            "doom_exit" => {
                if self.director.list(agent, obs, Topic::Exits).is_empty() {
                    // (not a failure to retry: nothing to do about it but explore)
                    (true, "No exit known yet.".into())
                } else if let Some(q) = self.director.ask_leave(agent, obs) {
                    // (a question, not a failure: Neuro would send it again as it was)
                    (true, q)
                } else {
                    self.director.order(Order::Exit);
                    (true, "Going to the exit.".into())
                }
            }
            "doom_fight" => {
                let seconds = args.get("seconds").and_then(Value::as_f64);
                let flag = |k: &str| args.get(k).and_then(Value::as_bool).unwrap_or(false);
                let fire = match arg("fire") {
                    Some("hold") => Some("hold_fire"),
                    Some("free") => Some("fire"),
                    Some(_) => return (false, "fire: hold or free.".into()),
                    None => None,
                };
                let stance = arg("override_stance");
                if stance.is_some_and(|s| !["bold", "careful", "avoid", "steady"].contains(&s)) {
                    return (
                        false,
                        "override_stance: bold, careful, avoid or steady.".into(),
                    );
                }
                // (each of her words in turn, as `Director::fight` has them: dropping what
                // was hers first, falling back last)
                let mut steps: Vec<(&str, Option<&str>)> = Vec::new();
                if flag("cancel") {
                    steps.push(("auto", None));
                }
                if let Some(t) = arg("target") {
                    steps.push(("target", Some(t)));
                }
                if let Some(w) = arg("weapon") {
                    steps.push(("weapon", Some(w)));
                }
                steps.extend(fire.map(|f| (f, None)));
                steps.extend(stance.map(|s| (s, None)));
                if flag("retreat") {
                    steps.push(("retreat", None));
                }
                if steps.is_empty() {
                    let holding = self.director.calls(obs.tic).join("; ");
                    return (true, format!("Holding now: {holding}."));
                }
                // (one that can't be done doesn't stop the rest: what was taken is said, and
                // what wasn't, why)
                let (mut said, mut not): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
                for (what, r) in steps {
                    match self.director.fight(agent, obs, what, r, seconds) {
                        Ok(s) => said.push(s),
                        Err(why) => not.push(why),
                    }
                }
                match (said.is_empty(), not.is_empty()) {
                    (_, true) => (true, said.join(" ")),
                    (true, false) => (false, not.join(" ")),
                    (false, false) => (false, format!("{} But: {}", said.join(" "), not.join(" "))),
                }
            }
            "doom_status" => (true, self.director.status(agent, obs)),
            // (not one of hers any more: the stream console's "Why" button asks it)
            "doom_why" => (true, self.director.why(agent, obs)),
            "doom_goal" => {
                let mut said: Vec<String> = Vec::new();
                if arg("finish").is_some() && !self.hers("finish") {
                    return (
                        false,
                        "finish: not yours to set here: you leave a level when you say (doom_exit)."
                            .into(),
                    );
                }
                if arg("prefer").is_some() && !self.hers("prefer") {
                    return (false, "prefer: not yours to change.".into());
                }
                if let Some(f) = arg("finish") {
                    match self.director.set_goal(agent, obs, f) {
                        // (completionist: asked first; what's asked is the answer)
                        Ok(s) => said.push(s),
                        Err(why) => return (false, why),
                    }
                }
                if let Some(p) = arg("prefer") {
                    match self.director.set_prefer(agent, obs, p) {
                        Ok(s) => said.push(s),
                        Err(why) => return (false, why),
                    }
                }
                if said.is_empty() {
                    return (
                        true,
                        match self.director.goal_label() {
                            Some(g) => format!("Set: {g}."),
                            None => "Nothing set: finish none, prefer none.".into(),
                        },
                    );
                }
                (true, said.join(" "))
            }
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

/// `doom_game` as `c` has it: only the moves hers, the levels (by name, up to a dozen), skills
/// and play on offer, in its schema and its description.
fn game_action(c: &Choices) -> Value {
    let skill_names: Vec<String> = c
        .skills
        .iter()
        .map(|&k| format!("{k} ({})", crate::control::directed::skill_name(k)))
        .collect();
    let plays: Vec<&str> = [("single", c.single), ("continuous", c.continuous)]
        .into_iter()
        .filter_map(|(p, on)| on.then_some(p))
        .collect();
    let mut moves: Vec<&str> = Vec::new();
    let mut said: Vec<String> = Vec::new();
    if c.offered() {
        moves.push("start");
        let play = match (c.single, c.continuous) {
            (true, true) => "play single level or continuous",
            (true, false) => "play single level",
            _ => "play continuous (on through the episode)",
        };
        let map = if c.maps.len() == 1 {
            format!("map {}", c.maps[0])
        } else {
            "map".to_string()
        };
        said.push(format!(
            "start: a game, when you're offered one ({map}; skill {}; {play}).",
            skill_names.join(" or ")
        ));
    }
    if c.next {
        moves.push("next");
        said.push("next: on to the next level from the tally screen.".into());
    }
    if c.restart {
        moves.push("restart");
        said.push("restart: this level from its start, after a while with nothing new.".into());
    }
    if moves.is_empty() {
        moves = vec!["start", "next", "restart"];
    }
    let map = if (1..=12).contains(&c.maps.len()) {
        json!({ "type": "string", "enum": c.maps })
    } else {
        json!({ "type": "string" })
    };
    let skill = if c.skills.is_empty() {
        json!({ "type": "integer", "enum": [1, 2, 3, 4] })
    } else {
        json!({ "type": "integer", "enum": c.skills })
    };
    let play = if plays.is_empty() {
        json!({ "type": "string", "enum": ["single", "continuous"] })
    } else {
        json!({ "type": "string", "enum": plays })
    };
    let description = format!("The game itself, when it's yours. {}", said.join(" "));
    json!({
        "name": "doom_game",
        "description": description.trim_end(),
        "schema": {
            "type": "object",
            "properties": { "do": { "type": "string", "enum": moves }, "map": map, "skill": skill, "play": play },
            "required": ["do"],
        },
    })
}

/// A result's message with what was mended in her data, so she gives it right next time.
fn noted(msg: String, notes: &[String]) -> String {
    if notes.is_empty() {
        msg
    } else {
        format!("{msg} ({})", notes.join(", "))
    }
}

#[cfg(test)]
mod tests {
    /// A setting locked is out of her action's schema, the rest as they were; an action with
    /// nothing left is gone.
    #[test]
    fn locked_settings_out_of_her_actions() {
        let set = |l: &[&'static str]| l.iter().copied().collect();
        let mut all = super::actions();
        super::without(&mut all, &set(&["instinct", "stance", "finish"]));
        let p = all.iter().find(|a| a["name"] == "doom_policy").unwrap();
        assert!(
            !p.to_string().contains("instinct") && !p.to_string().contains("stance"),
            "{p}"
        );
        assert!(p["schema"]["properties"]["explore"].is_object());
        // (and what each does isn't in the description at all: it's what an empty call answers)
        assert!(p["description"].as_str().unwrap().len() < 160);
        let g = all.iter().find(|a| a["name"] == "doom_goal").unwrap();
        assert!(!g.to_string().contains("finish"), "{g}");
        assert!(g["schema"]["properties"]["prefer"].is_object());
        let mut all = super::actions();
        super::without(&mut all, &set(super::LOCKABLE));
        assert!(!all
            .iter()
            .any(|a| a["name"] == "doom_policy" || a["name"] == "doom_goal"));
        assert!(all.iter().any(|a| a["name"] == "doom_go"));
    }

    /// A locked setting the console applies is kept by the mending; hers is dropped by it.
    #[test]
    fn a_lock_is_on_her_not_the_console() {
        let locked = ["stance"].into_iter().collect();
        let sent = serde_json::json!({ "stance": "bold", "risk": "normal" });
        let mend = |console: bool| {
            let schema = super::mending_schema(super::actions(), &locked, console, "doom_policy");
            crate::neuro::repair::repair(sent.clone(), schema.as_ref()).0
        };
        assert_eq!(mend(true)["stance"], "bold");
        assert!(mend(false).get("stance").is_none());
        assert_eq!(mend(false)["risk"], "normal");
    }

    /// One force at a time: out till it's answered for sure, a while after another of its own
    /// actions is accepted, or it's timed out.
    #[test]
    fn one_force_at_a_time() {
        let mut f = super::Forcing::new(30 * 35, 5 * 35);
        assert!(!f.out());
        f.sent(100, vec!["doom_answer", "doom_go"]);
        // (something else of hers meanwhile: still out)
        f.answered("doom_status", false, true, 110);
        assert!(f.out());
        // (one of its own, refused: the server asks again, still out, the wait afresh)
        f.answered("doom_go", false, false, 120);
        assert!(f.out());
        assert!(!f.expire(100 + 30 * 35));
        // (one of its own accepted, maybe of her own accord: out a while yet, then not)
        f.answered("doom_go", false, true, 130);
        f.answered("doom_go", false, true, 200);
        assert!(!f.expire(130 + 5 * 35 - 1) && f.out());
        assert!(!f.expire(130 + 5 * 35) && !f.out());
        // (the question forced answered: over at once)
        f.sent(400, vec!["doom_answer", "doom_go"]);
        f.answered("doom_answer", true, true, 410);
        assert!(!f.out());
        // (never answered: dropped after the timeout, said once)
        f.sent(500, vec!["doom_answer"]);
        assert!(!f.expire(500 + 30 * 35 - 1));
        assert!(f.expire(500 + 30 * 35));
        assert!(!f.out() && !f.expire(500 + 31 * 35));
        assert_eq!(
            super::answered_id(Some(r#"{"choice":"a","id":"q4"}"#)).as_deref(),
            Some("4")
        );
    }

    /// Every action named for registering is one `actions` has, and every one it has is named.
    #[test]
    fn all_actions_named() {
        let named: std::collections::BTreeSet<&str> = super::ALL_ACTIONS.iter().copied().collect();
        let had: Vec<String> = super::actions()
            .iter()
            .filter_map(|a| a["name"].as_str().map(str::to_string))
            .collect();
        assert_eq!(had.len(), named.len());
        assert!(had.iter().all(|n| named.contains(n.as_str())));
        assert!(super::OFF_LEVEL.iter().all(|n| named.contains(n)));
    }

    #[test]
    fn game_action_as_offered() {
        use crate::control::game::Choices;
        use serde_json::json;
        // (two levels, two skills, one level at a time, no restarting)
        let c = Choices {
            maps: vec!["E1M1".into(), "E1M2".into()],
            skills: vec![3, 4],
            single: true,
            continuous: false,
            start: true,
            next: true,
            restart: false,
        };
        let g = super::game_action(&c);
        let p = &g["schema"]["properties"];
        assert_eq!(p["do"]["enum"], json!(["start", "next"]));
        assert_eq!(p["map"]["enum"], json!(["E1M1", "E1M2"]));
        assert_eq!(p["skill"]["enum"], json!([3, 4]));
        assert_eq!(p["play"]["enum"], json!(["single"]));
        let d = g["description"].as_str().unwrap_or("");
        assert!(
            d.contains("skill 3 (Hurt me plenty) or 4 (Ultra-Violence)"),
            "{d}"
        );
        assert!(!d.contains("restart"), "{d}");
        // (a whole IWAD's levels: named in the offer, not listed in the schema)
        let all = Choices {
            maps: (1..=32).map(|n| format!("MAP{n:02}")).collect(),
            ..c
        };
        assert!(super::game_action(&all)["schema"]["properties"]["map"]
            .get("enum")
            .is_none());
    }
}
