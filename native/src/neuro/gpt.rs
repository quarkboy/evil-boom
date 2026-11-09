//! A language model as Evil (`evil-boom neuro-gpt`): a stand-in Neuro server the game connects to
//! as it would to Neuro, with a model (any OpenAI-compatible chat API) choosing the actions.
//!
//! The game's registered actions are the model's tools. What the game says (the briefing, reports,
//! status) goes in as messages; a forced decision asks it to choose now, among the forced actions
//! (the force's state taken out again after, being ephemeral); the result of each action goes back
//! to it, and after a look (list, inspect, status) it may act again, a few steps at most. Notable
//! news (not silent) may prompt it too. What it writes rather than does is what Evil says, logged.
//!
//! Configuration: config/neuro-gpt.json (committed: endpoint, model, persona, ...), overlaid by
//! config/neuro-gpt.local.json (not committed: the API key, or anything to change locally).

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
    /// an OpenAI-compatible API ("https://api.openai.com/v1", "http://localhost:11434/v1" for Ollama)
    pub base_url: String,
    pub model: String,
    /// the key itself (the local file), or the environment variable it's in
    pub api_key: String,
    pub api_key_env: String,
    pub temperature: f64,
    /// who she is and how she plays (the system prompt)
    pub persona: String,
    /// messages kept (the oldest dropped)
    pub history: usize,
    /// actions in a row after a look (list, inspect, status) before waiting again
    pub max_steps: usize,
    /// notable news prompts her to act, not only forced decisions
    pub act_on_news: bool,
}

impl Config {
    /// The settings file, overlaid by its local twin (FILE.local.json) if there is one.
    pub fn load(path: &Path) -> Result<Config, String> {
        let read = |p: &Path| -> Result<Value, String> { serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display())) };
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
            temperature: n("temperature", 0.7),
            persona: s("persona", ""),
            history: n("history", 60.0) as usize,
            max_steps: n("max_steps", 4.0) as usize,
            act_on_news: v.get("act_on_news").and_then(Value::as_bool).unwrap_or(true),
        })
    }

    fn key(&self) -> String {
        if self.api_key.is_empty() { std::env::var(&self.api_key_env).unwrap_or_default() } else { self.api_key.clone() }
    }
}

/// What the WebSocket side tells the model's side.
enum Event {
    Actions(Vec<Value>),
    Context { message: String, silent: bool },
    Force { state: String, query: String, names: Vec<String> },
    Result { id: String, success: bool, message: String },
    Gone,
}

/// What the model's side asks the WebSocket side to send: an action (id, name, data).
type Send = (String, String, Option<String>);

pub fn serve(port: u16, config_path: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    if config.key().is_empty() && config.base_url.starts_with("https") {
        return Err(format!("no API key: put \"api_key\" in {}, or set {}", config_path.with_extension("local.json").display(), config.api_key_env));
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let log_path = crate::config::out_dir().join("neuro").join("gpt.log");
    std::fs::create_dir_all(log_path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let log = std::sync::Arc::new(std::sync::Mutex::new(std::fs::File::create(&log_path).map_err(|e| e.to_string())?));
    let say = {
        let log = std::sync::Arc::clone(&log);
        move |line: String| {
            println!("{line}");
            if let Ok(mut f) = log.lock() {
                let _ = writeln!(f, "{line}");
            }
        }
    };
    say(format!("GPT stand-in for Evil on ws://127.0.0.1:{port}: {} at {}; log in {}", config.model, config.base_url, log_path.display()));
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| e.to_string())?;
        let mut ws = tungstenite::accept(stream).map_err(|e| e.to_string())?;
        ws.get_ref().set_read_timeout(Some(Duration::from_millis(20))).map_err(|e| e.to_string())?;
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
fn pump(ws: &mut tungstenite::WebSocket<std::net::TcpStream>, ev: &Sender<Event>, acts: &Receiver<Send>, say: &impl Fn(String)) -> String {
    loop {
        while let Ok((id, name, data)) = acts.try_recv() {
            let mut d = json!({ "id": id, "name": name });
            if let Some(data) = data {
                d["data"] = Value::String(data);
            }
            if let Err(e) = ws.send(Message::text(json!({ "command": "action", "data": d }).to_string())) {
                return e.to_string();
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                let d = &v["data"];
                let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let e = match v["command"].as_str().unwrap_or("") {
                    "actions/register" => Event::Actions(d["actions"].as_array().cloned().unwrap_or_default()),
                    "context" => Event::Context { message: s("message"), silent: d["silent"].as_bool().unwrap_or(false) },
                    "actions/force" => Event::Force { state: s("state"), query: s("query"), names: d["action_names"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect() },
                    "action/result" => Event::Result { id: s("id"), success: d["success"].as_bool().unwrap_or(false), message: s("message") },
                    "startup" => {
                        say("STARTUP".into());
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
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return e.to_string(),
        }
    }
}

/// The model's side: the conversation, and when to ask the model.
struct Brain<F: Fn(String)> {
    config: Config,
    acts: Sender<Send>,
    say: F,
    tools: Vec<Value>,
    history: VecDeque<Value>,
    next_id: u64,
    /// events that came in while waiting on a result
    backlog: VecDeque<Event>,
}

/// Actions that only look: after one, she may go on to act.
const LOOKS: [&str; 3] = ["doom_list", "doom_inspect", "doom_status"];

impl<F: Fn(String)> Brain<F> {
    fn new(config: Config, acts: Sender<Send>, say: F) -> Self {
        Brain { config, acts, say, tools: Vec::new(), history: VecDeque::new(), next_id: 1, backlog: VecDeque::new() }
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
                    (self.say)(format!("REGISTER {}", a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")));
                    self.tools = a;
                }
                Event::Context { message, silent } => {
                    (self.say)(format!("GAME{} {message}", if silent { " (silent)" } else { "" }));
                    self.push(json!({ "role": "user", "content": format!("[game] {message}") }));
                    if !silent && self.config.act_on_news {
                        self.turn(events, None, None);
                    }
                }
                Event::Force { state, query, names } => {
                    (self.say)(format!("FORCE {query} [{}]", names.join(", ")));
                    self.turn(events, Some(&names), Some(format!("[game, decide now] {query}\n\n{state}")));
                }
                Event::Result { .. } => {} // (a result not waited for: a stale one)
            }
        }
    }

    /// One turn: the model asked (forced: choose among `names`), its actions sent, their results
    /// given back; after a look, asked again, a few steps at most.
    fn turn(&mut self, events: &Receiver<Event>, names: Option<&Vec<String>>, ephemeral: Option<String>) {
        let mut steps = 0;
        let mut forced = names.cloned();
        let mut tries = 0;
        loop {
            let mut messages: Vec<Value> = vec![json!({ "role": "system", "content": self.config.persona })];
            messages.extend(self.history.iter().cloned());
            if let Some(e) = &ephemeral {
                messages.push(json!({ "role": "user", "content": e }));
            }
            let tools: Vec<Value> = self
                .tools
                .iter()
                .filter(|t| forced.as_ref().is_none_or(|n| n.iter().any(|x| t["name"].as_str() == Some(x))))
                .map(|t| json!({ "type": "function", "function": { "name": t["name"], "description": t["description"], "parameters": if t["schema"].is_object() { t["schema"].clone() } else { json!({ "type": "object", "properties": {} }) } } }))
                .collect();
            let reply = match self.ask(&messages, &tools, forced.is_some()) {
                Ok(r) => r,
                Err(e) => {
                    (self.say)(format!("MODEL ERROR {e}"));
                    return;
                }
            };
            if let Some(text) = reply["content"].as_str().filter(|t| !t.trim().is_empty()) {
                (self.say)(format!("EVIL says: {}", text.trim()));
            }
            let calls = reply["tool_calls"].as_array().cloned().unwrap_or_default();
            self.push(reply.clone());
            if calls.is_empty() {
                return;
            }
            let mut looked = false;
            let mut failed = false;
            for c in &calls {
                let name = c["function"]["name"].as_str().unwrap_or("").to_string();
                let args = c["function"]["arguments"].as_str().map(str::to_string);
                let (ok, message) = self.act(events, &name, args.clone());
                (self.say)(format!("EVIL -> {name} {}\n  {} {message}", args.unwrap_or_default(), if ok { "ok" } else { "FAILED" }));
                self.push(json!({ "role": "tool", "tool_call_id": c["id"], "content": format!("{}{message}", if ok { "" } else { "Failed: " }) }));
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
    fn act(&mut self, events: &Receiver<Event>, name: &str, data: Option<String>) -> (bool, String) {
        let id = format!("g{}", self.next_id);
        self.next_id += 1;
        if self.acts.send((id.clone(), name.to_string(), data)).is_err() {
            return (false, "the game's gone".into());
        }
        loop {
            match events.recv_timeout(Duration::from_secs(20)) {
                Ok(Event::Result { id: rid, success, message }) if rid == id => return (success, message),
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

    fn push(&mut self, m: Value) {
        self.history.push_back(m);
        while self.history.len() > self.config.history {
            self.history.pop_front();
        }
        // (not starting on a tool result whose call was dropped: the API wants them paired)
        while self.history.front().is_some_and(|m| m["role"] == "tool") {
            self.history.pop_front();
        }
    }

    /// The model's reply message (content, tool_calls).
    fn ask(&self, messages: &[Value], tools: &[Value], required: bool) -> Result<Value, String> {
        let mut body = json!({ "model": self.config.model, "messages": messages, "temperature": self.config.temperature });
        if !tools.is_empty() {
            body["tools"] = json!(tools);
            body["tool_choice"] = json!(if required { "required" } else { "auto" });
        }
        let mut req = ureq::post(&format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'))).timeout(Duration::from_secs(60));
        let key = self.config.key();
        if !key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        let resp: Value = match req.send_json(body) {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
            Err(ureq::Error::Status(code, r)) => return Err(format!("{code}: {}", r.into_string().unwrap_or_default().chars().take(300).collect::<String>())),
            Err(e) => return Err(e.to_string()),
        };
        resp["choices"][0]["message"].as_object().map(|m| Value::Object(m.clone())).ok_or_else(|| format!("no reply: {}", resp.to_string().chars().take(300).collect::<String>()))
    }
}
