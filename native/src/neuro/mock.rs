// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A stand-in Neuro server (`evil-boom neuro-mock`): the game connects to it as it would to Neuro.
//! What the game sends (registrations, context, forces with their state, results) is written to a
//! log, readable; actions are sent from a file as lines are added to it, one a line:
//! `doom_explore {"target": "north"}`, `doom_list {"what": "areas"}`, `doom_exit`; and
//! `wait-for TEXT`: the lines after it held till the game says something with TEXT in it.
//! Whoever writes the file plays Evil, by hand or by script.

use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::Message;

pub fn serve(port: u16, actions: &Path) -> Result<(), String> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let log_path = actions.with_extension("log");
    if let Some(dir) = actions.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if !actions.exists() {
        std::fs::write(actions, "").map_err(|e| e.to_string())?;
    }
    let mut log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let mut say = |line: String| {
        println!("{line}");
        let _ = writeln!(log, "{line}");
    };
    say(format!(
        "mock Neuro on ws://127.0.0.1:{port}; actions from {}, this log in {}",
        actions.display(),
        log_path.display()
    ));
    // (actions already in the file are for this session: sent from its start)
    let mut read = 0usize;
    let mut next_id = 1;
    let mut waiting: Option<String> = None;
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| e.to_string())?;
        let mut ws = tungstenite::accept(stream).map_err(|e| e.to_string())?;
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .map_err(|e| e.to_string())?;
        say("game connected".into());
        if let Err(why) = (|| -> Result<(), String> {
            loop {
                match ws.read() {
                    Ok(Message::Text(t)) => {
                        let line = summary(t.as_str());
                        // (the acknowledgement Neuro's server gives: who's playing)
                        if line.starts_with("STARTUP") {
                            ws.send(Message::text(hello().to_string()))
                                .map_err(|e| e.to_string())?;
                        }
                        if waiting.as_deref().is_some_and(|w| line.contains(w)) {
                            waiting = None;
                        }
                        say(line);
                    }
                    Ok(Message::Close(_)) => return Err("closed".into()),
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e) => return Err(e.to_string()),
                }
                let bytes = std::fs::read(actions).unwrap_or_default();
                if bytes.len() > read && waiting.is_none() {
                    let text = String::from_utf8_lossy(&bytes[read..]).to_string();
                    // (a line at a time, so a wait-for holds what's after it)
                    let upto = text.find('\n').map_or(0, |i| i + 1);
                    read += upto;
                    for line in text[..upto]
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    {
                        if let Some(w) = line.strip_prefix("wait-for ") {
                            say(format!("(waiting for \"{w}\")"));
                            waiting = Some(w.to_string());
                            continue;
                        }
                        let (name, data) = line
                            .split_once(' ')
                            .map_or((line, None), |(n, d)| (n, Some(d.trim())));
                        let id = format!("m{next_id}");
                        next_id += 1;
                        let mut d = json!({ "id": id, "name": name });
                        if let Some(data) = data {
                            d["data"] = Value::String(data.to_string());
                        }
                        say(format!("EVIL -> {name} {}  [{id}]", data.unwrap_or("")));
                        ws.send(Message::text(
                            json!({ "command": "action", "data": d }).to_string(),
                        ))
                        .map_err(|e| e.to_string())?;
                    }
                }
            }
        })() {
            say(format!("game gone ({why}); waiting for it again"));
        }
    }
    Ok(())
}

/// The startup's acknowledgement, as Evil.
pub fn hello() -> Value {
    json!({ "command": "startup", "data": { "session": { "characterId": "evil", "displayName": "Evil Neuro" } } })
}

/// A message from the game, readable.
fn summary(text: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return format!("?? {text}");
    };
    let d = &v["data"];
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("");
    match v["command"].as_str().unwrap_or("") {
        "startup" => format!("STARTUP game \"{}\"", v["game"].as_str().unwrap_or("")),
        "actions/register" => {
            let names: Vec<&str> = d["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["name"].as_str())
                .collect();
            format!("REGISTER {}", names.join(", "))
        }
        "context" => format!(
            "CONTEXT{} {}",
            if d["silent"].as_bool() == Some(true) {
                " (silent)"
            } else {
                ""
            },
            s("message")
        ),
        "action/result" => format!(
            "RESULT [{}] {} {}",
            s("id"),
            if d["success"].as_bool() == Some(true) {
                "ok"
            } else {
                "FAILED"
            },
            s("message")
        ),
        "actions/force" => {
            let names: Vec<&str> = d["action_names"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            format!(
                "FORCE ({}) {}\n  options: {}\n{}",
                s("priority"),
                s("query"),
                names.join(", "),
                s("state")
            )
        }
        c => format!("{c} {d}"),
    }
}
