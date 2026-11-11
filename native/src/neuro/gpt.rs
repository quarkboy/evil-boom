//! A language model as Evil (`evil-boom neuro-gpt`): a stand-in Neuro server the game connects to
//! as it would to Neuro, with an OpenAI model choosing the actions through the Responses API.
//!
//! The game's registered actions are the model's function tools (all of them every time; a forced
//! decision narrows what it may call with an allowed-tools choice, the list itself unchanged). What
//! the game says (the briefing, reports, status) goes in as user items; a forced decision asks it to
//! choose now (the force's state given for that request only, being ephemeral); each call's result
//! goes back as its output; after a look (list, inspect, status) it may act again, a few steps at
//! most. Notable news (not silent) may prompt it too. What it writes rather than does is what Evil
//! says, logged. Its output items (reasoning included) are kept in the conversation as returned.
//!
//! Configuration: config/neuro-gpt.json (committed: endpoint, model, reasoning, persona, ...),
//! overlaid by config/neuro-gpt.local.json (not committed: the API key, anything local).

use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::Message;

/// The stand-in's settings (the file's fields; the local file's override them).
#[derive(Clone, Debug)]
pub struct Config {
    /// the API's base ("https://api.openai.com/v1")
    pub base_url: String,
    pub model: String,
    /// the key itself (the local file), or the environment variable it's in
    pub api_key: String,
    pub api_key_env: String,
    /// reasoning effort ("none", "low", "medium", ...), if any
    pub reasoning_effort: Option<String>,
    /// sampling temperature, if any (reasoning models may refuse one)
    pub temperature: Option<f64>,
    /// who she is and how she plays (the instructions)
    pub persona: String,
    /// conversation items kept (the oldest dropped)
    pub history: usize,
    /// actions in a row after a look (list, inspect, status) before waiting again
    pub max_steps: usize,
    /// notable news prompts her to act, not only forced decisions
    pub act_on_news: bool,
}

impl Config {
    /// The settings file, overlaid by its local twin (FILE.local.json) if there is one.
    pub fn load(path: &Path) -> Result<Config, String> {
        let read = |p: &Path| -> Result<Value, String> {
            serde_json::from_str(
                &std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?,
            )
            .map_err(|e| format!("{}: {e}", p.display()))
        };
        let mut v = read(path)?;
        let local = path.with_extension("local.json");
        if local.exists() {
            if let (Some(base), Value::Object(over)) = (v.as_object_mut(), read(&local)?) {
                for (k, x) in over {
                    base.insert(k, x);
                }
            }
        }
        let s = |k: &str, d: &str| v.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let n = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).unwrap_or(d);
        Ok(Config {
            base_url: s("base_url", "https://api.openai.com/v1"),
            model: s("model", ""),
            api_key: s("api_key", ""),
            api_key_env: s("api_key_env", "OPENAI_API_KEY"),
            reasoning_effort: v
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::to_string),
            temperature: v.get("temperature").and_then(Value::as_f64),
            persona: s("persona", ""),
            history: n("history", 80.0) as usize,
            max_steps: n("max_steps", 4.0) as usize,
            act_on_news: v
                .get("act_on_news")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        })
    }

    fn key(&self) -> String {
        if self.api_key.is_empty() {
            std::env::var(&self.api_key_env).unwrap_or_default()
        } else {
            self.api_key.clone()
        }
    }
}

/// What the WebSocket side tells the model's side.
enum Event {
    Actions(Vec<Value>),
    Context {
        message: String,
        silent: bool,
    },
    Force {
        state: String,
        query: String,
        names: Vec<String>,
    },
    Result {
        id: String,
        success: bool,
        message: String,
    },
    Gone,
}

/// What the model's side asks the WebSocket side to send: an action (id, name, data).
type Send = (String, String, Option<String>);

/// What the stand-in has said (its text, not its actions), for a game in the same program to show
/// (`take_said`): Neuro's own words go to the stream, never to the game, but the stand-in's can.
static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// What the stand-in has said since last asked.
pub fn take_said() -> Vec<String> {
    SAID.lock()
        .map(|mut s| std::mem::take(&mut *s))
        .unwrap_or_default()
}

pub fn serve(port: u16, config_path: &Path) -> Result<(), String> {
    let config = configured(config_path)?;
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    run(&listener, &config)
}

/// The stand-in in the background, on a free port, for a game in the same program to connect to
/// (`watch --gpt`: one program). Its URL. The configuration's checked first, so a missing key
/// stops it before the game starts.
pub fn start(config_path: &Path) -> Result<String, String> {
    let config = configured(config_path)?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("a local port: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    thread::spawn(move || {
        if let Err(e) = run(&listener, &config) {
            eprintln!("GPT stand-in stopped: {e}");
        }
    });
    Ok(format!("ws://127.0.0.1:{port}"))
}

/// The configuration, with a key.
fn configured(config_path: &Path) -> Result<Config, String> {
    let config = Config::load(config_path)?;
    if config.key().is_empty() {
        return Err(format!(
            "no API key: put \"api_key\" in {}, or set {}",
            config_path.with_extension("local.json").display(),
            config.api_key_env
        ));
    }
    Ok(config)
}

/// Serving games on `listener`, one after another, as they connect.
fn run(listener: &TcpListener, config: &Config) -> Result<(), String> {
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let log_path = crate::config::out_dir().join("neuro").join("gpt.log");
    std::fs::create_dir_all(log_path.parent().unwrap_or(Path::new(".")))
        .map_err(|e| e.to_string())?;
    let log = std::sync::Arc::new(std::sync::Mutex::new(
        crate::config::Log::create(&log_path, "").map_err(|e| e.to_string())?,
    ));
    let say = {
        let log = std::sync::Arc::clone(&log);
        move |line: String| {
            println!("{line}");
            if let Ok(mut f) = log.lock() {
                let _ = writeln!(f, "{line}");
            }
        }
    };
    say(format!(
        "GPT stand-in for Evil on ws://127.0.0.1:{port}: {} (reasoning {}); log in {}",
        config.model,
        config.reasoning_effort.as_deref().unwrap_or("default"),
        log_path.display()
    ));
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| e.to_string())?;
        let mut ws = tungstenite::accept(stream).map_err(|e| e.to_string())?;
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .map_err(|e| e.to_string())?;
        say("game connected".into());
        let (ev_tx, ev_rx) = channel::<Event>();
        let (act_tx, act_rx) = channel::<Send>();
        let brain = {
            let config = config.clone();
            let say = say.clone();
            thread::spawn(move || Brain::new(config, act_tx, say).run(&ev_rx))
        };
        let why = pump(&mut ws, &ev_tx, &act_rx, &say);
        let _ = ev_tx.send(Event::Gone);
        let _ = brain.join();
        say(format!("game gone ({why}); waiting for it again"));
    }
    Ok(())
}

/// The WebSocket: what comes in passed to the model's side; its actions sent.
fn pump(
    ws: &mut tungstenite::WebSocket<std::net::TcpStream>,
    ev: &Sender<Event>,
    acts: &Receiver<Send>,
    say: &impl Fn(String),
) -> String {
    loop {
        while let Ok((id, name, data)) = acts.try_recv() {
            let mut d = json!({ "id": id, "name": name });
            if let Some(data) = data {
                d["data"] = Value::String(data);
            }
            if let Err(e) = ws.send(Message::text(
                json!({ "command": "action", "data": d }).to_string(),
            )) {
                return e.to_string();
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else {
                    continue;
                };
                let d = &v["data"];
                let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let e = match v["command"].as_str().unwrap_or("") {
                    "actions/register" => {
                        Event::Actions(d["actions"].as_array().cloned().unwrap_or_default())
                    }
                    "context" => Event::Context {
                        message: s("message"),
                        silent: d["silent"].as_bool().unwrap_or(false),
                    },
                    "actions/force" => Event::Force {
                        state: s("state"),
                        query: s("query"),
                        names: d["action_names"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect(),
                    },
                    "action/result" => Event::Result {
                        id: s("id"),
                        success: d["success"].as_bool().unwrap_or(false),
                        message: s("message"),
                    },
                    "startup" => {
                        say("STARTUP".into());
                        if let Err(e) = ws.send(Message::text(super::mock::hello().to_string())) {
                            return e.to_string();
                        }
                        continue;
                    }
                    _ => continue,
                };
                if ev.send(e).is_err() {
                    return "the model's side stopped".into();
                }
            }
            Ok(Message::Close(_)) => return "closed".into(),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return e.to_string(),
        }
    }
}

/// A model's reply.
struct Reply {
    /// what it wrote (the message items' text)
    text: String,
    /// its function calls: (call id, action name, arguments as JSON text)
    calls: Vec<(String, String, Option<String>)>,
    /// its output items, as returned (kept in the conversation)
    items: Vec<Value>,
}

/// Actions that only look: after one, she may go on to act.
const LOOKS: [&str; 3] = ["doom_list", "doom_inspect", "doom_status"];

/// The model's side: the conversation, and when to ask the model.
struct Brain<F: Fn(String)> {
    config: Config,
    acts: Sender<Send>,
    say: F,
    /// the game's actions as function tools
    tools: Vec<Value>,
    history: VecDeque<Value>,
    next_id: u64,
    /// events that came in while waiting on a result
    backlog: VecDeque<Event>,
}

impl<F: Fn(String)> Brain<F> {
    fn new(config: Config, acts: Sender<Send>, say: F) -> Self {
        Brain {
            config,
            acts,
            say,
            tools: Vec::new(),
            history: VecDeque::new(),
            next_id: 1,
            backlog: VecDeque::new(),
        }
    }

    fn run(mut self, events: &Receiver<Event>) {
        loop {
            let e = match self.backlog.pop_front() {
                Some(e) => e,
                None => match events.recv() {
                    Ok(e) => e,
                    Err(_) => return,
                },
            };
            match e {
                Event::Gone => return,
                Event::Actions(a) => {
                    (self.say)(format!(
                        "REGISTER {}",
                        a.iter()
                            .filter_map(|x| x["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    // (a function tool: the action's name, description, and schema as its parameters;
                    // not strict, some parameters being optional)
                    self.tools = a
                        .iter()
                        .map(|t| {
                            let params = if t["schema"].is_object() { t["schema"].clone() } else { json!({ "type": "object", "properties": {} }) };
                            json!({ "type": "function", "name": t["name"], "description": t["description"], "parameters": params, "strict": false })
                        })
                        .collect();
                }
                Event::Context { message, silent } => {
                    (self.say)(format!(
                        "GAME{} {message}",
                        if silent { " (silent)" } else { "" }
                    ));
                    self.push(json!({ "role": "user", "content": format!("[game] {message}") }));
                    if !silent && self.config.act_on_news {
                        self.turn(events, None, None);
                    }
                }
                Event::Force {
                    state,
                    query,
                    names,
                } => {
                    (self.say)(format!("FORCE {query} [{}]", names.join(", ")));
                    self.turn(
                        events,
                        Some(&names),
                        Some(format!("[game, decide now] {query}\n\n{state}")),
                    );
                }
                Event::Result { .. } => {} // (a result not waited for: a stale one)
            }
        }
    }

    /// One turn: the model asked (forced: it must call one of `names`), its calls sent as actions,
    /// their results given back; after a look, asked again, a few steps at most.
    fn turn(
        &mut self,
        events: &Receiver<Event>,
        names: Option<&Vec<String>>,
        ephemeral: Option<String>,
    ) {
        let mut steps = 0;
        let mut forced = names.cloned();
        let mut tries = 0;
        loop {
            let mut input: Vec<Value> = self.history.iter().cloned().collect();
            if let Some(e) = &ephemeral {
                input.push(json!({ "role": "user", "content": e }));
            }
            let reply = match self.ask(input, forced.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    (self.say)(format!("MODEL ERROR {e}"));
                    return;
                }
            };
            if !reply.text.trim().is_empty() {
                (self.say)(format!("EVIL says: {}", reply.text.trim()));
                if let Ok(mut said) = SAID.lock() {
                    said.push(reply.text.trim().to_string());
                }
            }
            for item in reply.items {
                self.push(item);
            }
            if reply.calls.is_empty() {
                return;
            }
            let mut looked = false;
            let mut failed = false;
            for (call_id, name, args) in &reply.calls {
                let (ok, message) = self.act(events, name, args.clone());
                (self.say)(format!(
                    "EVIL -> {name} {}\n  {} {message}",
                    args.clone().unwrap_or_default(),
                    if ok { "ok" } else { "FAILED" }
                ));
                self.push(json!({ "type": "function_call_output", "call_id": call_id, "output": format!("{}{message}", if ok { "" } else { "Failed: " }) }));
                looked |= LOOKS.contains(&name.as_str());
                failed |= !ok;
            }
            steps += 1;
            if forced.is_some() && failed && tries < 3 {
                tries += 1; // (a forced decision that failed is asked again, as Neuro retries it)
                continue;
            }
            if !looked || steps >= self.config.max_steps {
                return;
            }
            // (after a look, she may act on what she saw: any action now, not only the forced ones)
            forced = None;
        }
    }

    /// An action sent; its result (success, message), what else comes in meanwhile kept for after.
    fn act(
        &mut self,
        events: &Receiver<Event>,
        name: &str,
        data: Option<String>,
    ) -> (bool, String) {
        let id = format!("g{}", self.next_id);
        self.next_id += 1;
        if self
            .acts
            .send((id.clone(), name.to_string(), data))
            .is_err()
        {
            return (false, "the game's gone".into());
        }
        loop {
            match events.recv_timeout(Duration::from_secs(20)) {
                Ok(Event::Result {
                    id: rid,
                    success,
                    message,
                }) if rid == id => return (success, message),
                Ok(Event::Gone) => {
                    self.backlog.push_back(Event::Gone);
                    return (false, "the game's gone".into());
                }
                Ok(other) => self.backlog.push_back(other),
                Err(RecvTimeoutError::Timeout) => return (false, "no answer from the game".into()),
                Err(RecvTimeoutError::Disconnected) => return (false, "the game's gone".into()),
            }
        }
    }

    fn push(&mut self, item: Value) {
        self.history.push_back(item);
        while self.history.len() > self.config.history {
            self.history.pop_front();
        }
        // (starting on one of the game's messages: never on a reasoning item, a call or an output
        // whose partner was dropped, which the API refuses)
        while self.history.front().is_some_and(|m| m["role"] != "user") {
            self.history.pop_front();
        }
    }

    /// A response (POST /responses): the conversation as input, the instructions, all the tools,
    /// and a choice of them: required among `forced` if forced, else up to the model.
    fn ask(&self, input: Vec<Value>, forced: Option<&[String]>) -> Result<Reply, String> {
        let mut body = json!({ "model": self.config.model, "instructions": self.config.persona, "input": input });
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.clone());
            body["tool_choice"] = match forced {
                Some(names) => {
                    json!({ "type": "allowed_tools", "mode": "required", "tools": names.iter().map(|n| json!({ "type": "function", "name": n })).collect::<Vec<_>>() })
                }
                None => json!("auto"),
            };
        }
        if let Some(e) = &self.config.reasoning_effort {
            body["reasoning"] = json!({ "effort": e });
        }
        if let Some(t) = self.config.temperature {
            body["temperature"] = json!(t);
        }
        let url = format!("{}/responses", self.config.base_url.trim_end_matches('/'));
        let resp: Value = match ureq::post(&url)
            .timeout(Duration::from_secs(180))
            .set("Authorization", &format!("Bearer {}", self.config.key()))
            .send_json(body)
        {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
            Err(ureq::Error::Status(code, r)) => {
                return Err(format!(
                    "{code}: {}",
                    r.into_string()
                        .unwrap_or_default()
                        .chars()
                        .take(500)
                        .collect::<String>()
                ))
            }
            Err(e) => return Err(e.to_string()),
        };
        let output = resp["output"].as_array().ok_or_else(|| {
            format!(
                "no output: {}",
                resp.to_string().chars().take(300).collect::<String>()
            )
        })?;
        let mut text = String::new();
        let mut calls = Vec::new();
        for o in output {
            match o["type"].as_str().unwrap_or("") {
                "message" => {
                    for c in o["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|c| c["type"] == "output_text")
                    {
                        text.push_str(c["text"].as_str().unwrap_or(""));
                    }
                }
                "function_call" => calls.push((
                    o["call_id"].as_str().unwrap_or("").to_string(),
                    o["name"].as_str().unwrap_or("").to_string(),
                    o["arguments"].as_str().map(str::to_string),
                )),
                _ => {}
            }
        }
        Ok(Reply {
            text,
            calls,
            items: output.clone(),
        })
    }
}
