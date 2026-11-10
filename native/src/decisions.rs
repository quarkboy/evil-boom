//! A journal of what she decided and on what: a record a decision (which target, which move of
//! the fight's, a retreat begun, which objective), with the options weighed where the rule had
//! any, and how things stood (the fight's assessment of that tic). For looking at why she did
//! what she did, and for a decision model later: what it'd be shown, and what the rules chose.
//!
//! Off unless a run asks (`--decisions`): then a line of JSON a record, beside the run's log
//! (`out/explore/<IWAD>_<MAP>.decisions.jsonl`).

use serde::Serialize;

/// One decision.
#[derive(Clone, Debug, Serialize)]
pub struct Record {
    pub tic: i64,
    /// which kind of decision: "target", "move", "retreat", "objective"
    pub point: &'static str,
    /// what was chosen (a `Move`'s name, a monster and its id, an objective's label)
    pub chosen: String,
    /// as the log has it, where that says more
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// what was weighed, each with its score as the rule had it (a target's: higher better; an
    /// objective's: its weighed way there, shorter better); empty where the rule is a chain of
    /// conditions with nothing to compare
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<(String, f64)>,
    /// how things stood (`Assessment::summary`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

impl Record {
    pub fn new(tic: i64, point: &'static str, chosen: String) -> Record {
        Record { tic, point, chosen, detail: String::new(), options: Vec::new(), state: None }
    }
}

/// The records so far (none kept unless it's on).
#[derive(Clone, Debug, Default)]
pub struct Journal {
    pub on: bool,
    pub records: Vec<Record>,
}

impl Journal {
    /// Note a decision, if anyone's asked: `record` isn't worked out otherwise.
    pub fn note(&mut self, record: impl FnOnce() -> Record) {
        if self.on {
            self.records.push(record());
        }
    }

    /// A line of JSON a record.
    pub fn lines(&self) -> String {
        self.records.iter().filter_map(|r| serde_json::to_string(r).ok()).map(|l| l + "\n").collect()
    }
}
