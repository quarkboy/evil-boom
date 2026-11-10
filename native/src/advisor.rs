//! A decision model's say in what she does: small questions put to it as she plays (which of
//! these to shoot first; what stance to fight in), answered a moment later, the rules deciding
//! meanwhile and whenever it has nothing sure to say. The model is a classifier of TypeSafe's
//! "System One" kind (hosted Jev; an open-weight tev1 behind llama.cpp's server speaks the same):
//! a state in words, a question with its choices, a share of belief back for each choice.
//!
//! Off unless a run asks (`--advisor [FILE]`, the settings: `config/jev.json`, overlaid by
//! `config/jev.local.json` for the key). What an answer does is the player's
//! (`tactical/player.rs`): a target forced a while (`Combat::forced_target`), a stance's numbers
//! put over the tuning (`Player::set_tuning`). Each answer taken is in the journal.
//!
//! An answer comes `latency_tics` after its question, however fast the run is going: a run
//! played flat out waits for the reply and then holds it till its tic, so it plays as a run
//! at the game's own pace would with the model that slow, and the same every time the model
//! says the same. Watched at the game's pace (`live`), it's taken when it's come and its tic has.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use serde_json::{json, Value};

/// A question for the model.
#[derive(Clone, Debug)]
pub struct Question {
    /// which decision: "target", "stance"
    pub point: &'static str,
    pub tic: i64,
    /// how things stand, in words (short: the model's time goes by the words)
    pub state: String,
    pub instructions: String,
    /// each choice: its key, and what it means
    pub options: Vec<(String, String)>,
}

/// The model's answer, when it's sure enough.
#[derive(Clone, Debug)]
pub struct Answer {
    pub point: &'static str,
    /// the tic it was asked
    pub asked: i64,
    pub choice: String,
    /// its share of belief in that choice
    pub share: f64,
}

/// Something that answers: each choice's share of belief.
pub trait Backend: Send {
    fn answer(&mut self, q: &Question) -> Result<Vec<(String, f64)>, String>;
}

/// A stance: what it's called to the model, and its numbers over the tuning (any groups and
/// fields, as a tuning file).
#[derive(Clone, Debug)]
pub struct Stance {
    pub name: String,
    pub says: String,
    pub tuning: Value,
}

/// The settings (`config/jev.json`).
#[derive(Clone, Debug)]
pub struct Config {
    /// "systemone" (the API below), or "first" (no model: the first choice, at once sure; for
    /// trying the plumbing)
    pub backend: String,
    pub base_url: String,
    /// the model's name (none for llama.cpp's server)
    pub model: String,
    pub api_key: String,
    pub api_key_env: String,
    /// tics from a question to its answer (Jev: about 180 ms, 7 tics)
    pub latency_tics: i64,
    /// a target asked this often at most (tics), with two or more to choose from
    pub target_every: i64,
    /// a stance asked this often at most, in a fight
    pub stance_every: i64,
    /// an answer holds this long with none after it
    pub hold_tics: i64,
    /// an answer with less belief than this is left: the rules'
    pub min_share: f64,
    pub stances: Vec<Stance>,
}

impl Config {
    /// The settings file, overlaid by its local twin (FILE.local.json) if there is one.
    pub fn load(path: &Path) -> Result<Config, String> {
        let read = |p: &Path| -> Result<Value, String> {
            serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))
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
        let n = |k: &str, d: i64| v.get(k).and_then(Value::as_i64).unwrap_or(d);
        let stances = v
            .get("stances")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(name, st)| Stance {
                        name: name.clone(),
                        says: st.get("says").and_then(Value::as_str).unwrap_or(name).to_string(),
                        tuning: st.get("tuning").cloned().unwrap_or_else(|| json!({})),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Config {
            backend: s("backend", "systemone"),
            base_url: s("base_url", ""),
            model: s("model", ""),
            api_key: s("api_key", ""),
            api_key_env: s("api_key_env", "JEV_API_KEY"),
            latency_tics: n("latency_tics", 7),
            target_every: n("target_every", 35),
            stance_every: n("stance_every", 105),
            hold_tics: n("hold_tics", 105),
            min_share: v.get("min_share").and_then(Value::as_f64).unwrap_or(0.5),
            stances,
        })
    }

    fn backend(&self) -> Result<Box<dyn Backend>, String> {
        match self.backend.as_str() {
            "first" => Ok(Box::new(First)),
            "systemone" => {
                let key = if self.api_key.is_empty() { std::env::var(&self.api_key_env).unwrap_or_default() } else { self.api_key.clone() };
                if self.base_url.is_empty() {
                    return Err("the advisor's settings name no base_url".into());
                }
                Ok(Box::new(SystemOne { url: self.base_url.clone(), model: self.model.clone(), key }))
            }
            other => Err(format!("the advisor's backend: \"systemone\" or \"first\", not \"{other}\"")),
        }
    }
}

/// No model: the first choice, sure of it.
struct First;

impl Backend for First {
    fn answer(&mut self, q: &Question) -> Result<Vec<(String, f64)>, String> {
        Ok(q.options.iter().enumerate().map(|(i, (k, _))| (k.clone(), if i == 0 { 1.0 } else { 0.0 })).collect())
    }
}

/// TypeSafe's System One API (`POST base_url`: a state, questions by name; llama.cpp's server
/// answers the same without a model named).
struct SystemOne {
    url: String,
    model: String,
    key: String,
}

impl Backend for SystemOne {
    fn answer(&mut self, q: &Question) -> Result<Vec<(String, f64)>, String> {
        let criteria: serde_json::Map<String, Value> = q.options.iter().map(|(k, says)| (k.clone(), json!(says))).collect();
        let mut body = json!({"state": q.state, "questions": {q.point: {"type": "choice", "instructions": q.instructions, "criteria": criteria}}});
        if !self.model.is_empty() {
            body["model"] = json!(self.model);
        }
        let mut req = ureq::post(&self.url).timeout(Duration::from_secs(20));
        if !self.key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.key));
        }
        let resp: Value = match req.send_json(body) {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
            Err(ureq::Error::Status(code, r)) => return Err(format!("{code}: {}", r.into_string().unwrap_or_default().chars().take(300).collect::<String>())),
            Err(e) => return Err(e.to_string()),
        };
        shares(resp.get("answers").unwrap_or(&resp).get(q.point).unwrap_or(&Value::Null), &q.options).ok_or_else(|| format!("no answer read from {}", resp.to_string().chars().take(300).collect::<String>()))
    }
}

/// Each choice's share out of an answer, however it's laid out: a map of choice to share (bare,
/// or under "probabilities", "scores" or "options"), or only the choice made (then sure of it,
/// or as sure as a "confidence" or "probability" beside it says).
fn shares(answer: &Value, options: &[(String, String)]) -> Option<Vec<(String, f64)>> {
    let of = |m: &Value| -> Option<Vec<(String, f64)>> {
        let out: Vec<(String, f64)> = options.iter().filter_map(|(k, _)| Some((k.clone(), m.get(k)?.as_f64()?))).collect();
        (!out.is_empty()).then_some(out)
    };
    if let Some(found) = of(answer).or_else(|| ["probabilities", "scores", "options"].iter().find_map(|k| of(answer.get(k)?))) {
        return Some(found);
    }
    let chosen = answer.as_str().or_else(|| ["choice", "answer", "value"].iter().find_map(|k| answer.get(k)?.as_str()))?;
    let sure = ["confidence", "probability"].iter().find_map(|k| answer.get(k)?.as_f64()).unwrap_or(1.0);
    options.iter().any(|(k, _)| k == chosen).then(|| vec![(chosen.to_string(), sure)])
}

/// A reply as the backend's thread sends it: the question's kind and tic, and each choice's share.
type Reply = (&'static str, i64, Result<Vec<(String, f64)>, String>);

/// A question out, its answer to come.
struct Asked {
    point: &'static str,
    tic: i64,
    /// its answer, once in: the choice and its share, or nothing sure
    reply: Option<Option<(String, f64)>>,
}

/// The questions out and the answers due.
pub struct Advisor {
    pub config: Config,
    /// watched at the game's own pace: an answer's taken when it's come, never waited for
    pub live: bool,
    out: Sender<Question>,
    back: Receiver<Reply>,
    asked: VecDeque<Asked>,
    /// what went wrong, for the log (the player's takes them)
    pub errors: Vec<String>,
}

impl Advisor {
    pub fn new(config: Config, live: bool) -> Result<Advisor, String> {
        let mut backend = config.backend()?;
        let (out, questions) = channel::<Question>();
        let (answers, back) = channel();
        std::thread::spawn(move || {
            for q in questions {
                if answers.send((q.point, q.tic, backend.answer(&q))).is_err() {
                    break;
                }
            }
        });
        Ok(Advisor { config, live, out, back, asked: VecDeque::new(), errors: Vec::new() })
    }

    /// Is a question of this kind out, unanswered or its answer not yet due?
    pub fn waiting(&self, point: &str) -> bool {
        self.asked.iter().any(|a| a.point == point)
    }

    /// Put a question (one of a kind at a time: a second is dropped).
    pub fn ask(&mut self, q: Question) {
        if self.waiting(q.point) || q.options.len() < 2 {
            return;
        }
        self.asked.push_back(Asked { point: q.point, tic: q.tic, reply: None });
        if self.out.send(q).is_err() {
            self.asked.pop_back();
        }
    }

    fn take(&mut self, got: Reply) {
        let (point, tic, result) = got;
        let best = match result {
            Ok(shares) => shares.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)).filter(|(_, s)| *s >= self.config.min_share),
            Err(e) => {
                self.errors.push(format!("the advisor, asked for a {point}: {e}"));
                None
            }
        };
        if let Some(a) = self.asked.iter_mut().find(|a| a.point == point && a.tic == tic) {
            a.reply = Some(best);
        }
    }

    /// The answers whose tic has come (asked `latency_tics` ago or more), sure ones only.
    pub fn due(&mut self, tic: i64) -> Vec<Answer> {
        while let Ok(got) = self.back.try_recv() {
            self.take(got);
        }
        let mut out = Vec::new();
        while let Some(first) = self.asked.front() {
            if tic - first.tic < self.config.latency_tics {
                break;
            }
            if first.reply.is_none() {
                if self.live {
                    break; // (not in yet: next tic)
                }
                // (flat out: its tic's come, so wait for it, as the game's own pace would have)
                match self.back.recv_timeout(Duration::from_secs(30)) {
                    Ok(got) => {
                        self.take(got);
                        continue;
                    }
                    Err(_) => {
                        self.errors.push("the advisor gave no answer in 30 s: left".into());
                        self.asked.pop_front();
                        continue;
                    }
                }
            }
            let Some(a) = self.asked.pop_front() else { break };
            if let Some(Some((choice, share))) = a.reply {
                out.push(Answer { point: a.point, asked: a.tic, choice, share });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{shares, Advisor, Config, Question};
    use serde_json::json;

    fn options() -> Vec<(String, String)> {
        vec![("a".into(), "the first".into()), ("b".into(), "the second".into())]
    }

    #[test]
    fn shares_are_read_however_laid_out() {
        assert_eq!(shares(&json!({"a": 0.2, "b": 0.8}), &options()), Some(vec![("a".into(), 0.2), ("b".into(), 0.8)]));
        assert_eq!(shares(&json!({"probabilities": {"a": 0.9, "b": 0.1}}), &options()), Some(vec![("a".into(), 0.9), ("b".into(), 0.1)]));
        assert_eq!(shares(&json!({"choice": "b", "confidence": 0.7}), &options()), Some(vec![("b".into(), 0.7)]));
        assert_eq!(shares(&json!("a"), &options()), Some(vec![("a".into(), 1.0)]));
        assert_eq!(shares(&json!({"choice": "zzz"}), &options()), None);
    }

    #[test]
    fn an_answer_comes_when_its_tic_does() {
        let config = Config {
            backend: "first".into(),
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
            api_key_env: String::new(),
            latency_tics: 7,
            target_every: 35,
            stance_every: 105,
            hold_tics: 105,
            min_share: 0.5,
            stances: Vec::new(),
        };
        let mut advisor = Advisor::new(config, false).unwrap();
        advisor.ask(Question { point: "target", tic: 100, state: String::new(), instructions: String::new(), options: options() });
        assert!(advisor.waiting("target"));
        assert!(advisor.due(106).is_empty());
        let got = advisor.due(107);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].choice.as_str(), got[0].asked), ("a", 100));
        assert!(!advisor.waiting("target"));
    }
}
