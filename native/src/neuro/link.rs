//! The WebSocket to Neuro, on its own thread: connecting (and reconnecting), `startup` and the
//! actions registered each time, messages in and out over channels. The game never waits on it.

use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

/// The game's name to Neuro: always the same (the SDK's rule).
pub const GAME: &str = "Doom";

/// What comes in from the link.
#[derive(Debug)]
pub enum Incoming {
    /// connected (again): startup sent, the actions registered; the session sends what it should
    Connected,
    /// the startup acknowledgement: who's playing ("evil", "Evil Neuro")
    Hello { character: String, display: String },
    /// an action: its id, name, and data (a JSON string, unchecked)
    Action {
        id: String,
        name: String,
        data: Option<String>,
    },
    /// lost the connection (it's tried again)
    Lost(String),
}

pub struct Link {
    out: Sender<Value>,
    pub incoming: Receiver<Incoming>,
}

impl Link {
    /// Connect to `url` (ws://host:port) on a thread of its own; `actions` registered on each
    /// connect.
    pub fn start(url: &str, actions: Vec<Value>) -> Link {
        let (out, out_rx) = channel::<Value>();
        let (in_tx, incoming) = channel::<Incoming>();
        let url = url.to_string();
        thread::spawn(move || run(&url, &actions, &out_rx, &in_tx));
        Link { out, incoming }
    }

    /// Send a message (command and data; the game's name added).
    pub fn send(&self, command: &str, data: Option<Value>) {
        let mut m = json!({ "command": command, "game": GAME });
        if let Some(d) = data {
            m["data"] = d;
        }
        let _ = self.out.send(m);
    }

    pub fn context(&self, message: &str, silent: bool) {
        self.send(
            "context",
            Some(json!({ "message": message, "silent": silent })),
        );
    }

    pub fn result(&self, id: &str, success: bool, message: &str) {
        self.send(
            "action/result",
            Some(json!({ "id": id, "success": success, "message": message })),
        );
    }

    pub fn force(&self, state: &str, query: &str, actions: &[&str], priority: &str) {
        self.send("actions/force", Some(json!({ "state": state, "query": query, "ephemeral_context": true, "priority": priority, "action_names": actions })));
    }
}

fn run(url: &str, actions: &[Value], out: &Receiver<Value>, incoming: &Sender<Incoming>) {
    loop {
        match connect(url) {
            Ok(mut ws) => {
                let hello = [
                    json!({ "command": "startup", "game": GAME }),
                    json!({ "command": "actions/register", "game": GAME, "data": { "actions": actions } }),
                ];
                if hello
                    .iter()
                    .all(|m| ws.send(Message::text(m.to_string())).is_ok())
                    && incoming.send(Incoming::Connected).is_ok()
                {
                    let why = serve(&mut ws, out, incoming);
                    if incoming.send(Incoming::Lost(why)).is_err() {
                        return; // (the game's gone)
                    }
                }
            }
            Err(e) => {
                if incoming.send(Incoming::Lost(e)).is_err() {
                    return;
                }
            }
        }
        // (what was waiting to go out is for a connection that's gone: dropped, the session
        // syncs on reconnecting)
        while out.try_recv().is_ok() {}
        thread::sleep(Duration::from_secs(2));
    }
}

fn connect(url: &str) -> Result<WebSocket<TcpStream>, String> {
    let hostport = url
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap_or("localhost:8000")
        .to_string();
    let stream = TcpStream::connect(&hostport).map_err(|e| format!("{hostport}: {e}"))?;
    stream.set_nodelay(true).ok();
    let (ws, _) = tungstenite::client(url, stream).map_err(|e| e.to_string())?;
    // (short reads, so what's to go out isn't held up by waiting for what comes in)
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(10)))
        .map_err(|e| e.to_string())?;
    Ok(ws)
}

/// The connection's life: what's to go out sent, what comes in passed on. Why it ended.
fn serve(
    ws: &mut WebSocket<TcpStream>,
    out: &Receiver<Value>,
    incoming: &Sender<Incoming>,
) -> String {
    loop {
        loop {
            match out.try_recv() {
                Ok(m) => {
                    if let Err(e) = ws.send(Message::text(m.to_string())) {
                        return e.to_string();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return "the game's gone".into(),
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                if let Some(i) = parse(t.as_str()) {
                    if incoming.send(i).is_err() {
                        return "the game's gone".into();
                    }
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

/// A message from Neuro (malformed or unknown ones dropped, as the server drops ours).
fn parse(text: &str) -> Option<Incoming> {
    let v: Value = serde_json::from_str(text).ok()?;
    let data = v.get("data");
    match v.get("command")?.as_str()? {
        "action" => {
            let d = data?;
            Some(Incoming::Action {
                id: d.get("id")?.as_str()?.to_string(),
                name: d.get("name")?.as_str()?.to_string(),
                data: d.get("data").and_then(Value::as_str).map(str::to_string),
            })
        }
        "startup" => {
            let s = data?.get("session")?;
            Some(Incoming::Hello {
                character: s
                    .get("characterId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                display: s
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        }
        _ => None,
    }
}
