//! A run as its inputs: every tic's controls, the restarts, and what the explorer put down in
//! the knowledge itself (the journal). Game and bot are deterministic, so a run played back from
//! these comes to the same state at any tic, the game and what she knows of it, without her
//! thinking: the way to try changes from one moment of a run (run-map --from).
//!
//! The file is text, a line an entry, the tic first:
//!   c TIC FORWARD STRAFE TURN USE ATTACK RUN WEAPON   (the controls sent that tic)
//!   r TIC WHY                                         (the level again)
//!   n TIC tried LINE KEYS | stuck POLY TIMES | missed JUMP TIMES | trap POLY,POLY,... |
//!         pressed X Y LINE                            (the journal)

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use crate::knowledge::model::Note;
use crate::motor::Controls;
use crate::specials::Keys;

#[derive(Clone, Debug)]
pub enum Entry {
    Tic(i64, Controls),
    Restart(i64, String),
    Note(i64, Note),
}

#[derive(Clone, Debug, Default)]
pub struct Recording {
    pub entries: Vec<Entry>,
}

impl Recording {
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let mut out = String::new();
        for e in &self.entries {
            let _ = match e {
                Entry::Tic(t, c) => writeln!(
                    out,
                    "c {t} {:?} {:?} {:?} {} {} {} {}{}",
                    c.forward,
                    c.strafe,
                    c.turn,
                    u8::from(c.use_),
                    u8::from(c.attack),
                    u8::from(c.run),
                    c.weapon,
                    // (an exact press only where there was one: recordings before it read alike)
                    c.exact.map_or(String::new(), |g| format!(" {g}"))
                ),
                Entry::Restart(t, why) => writeln!(out, "r {t} {why}"),
                Entry::Note(t, n) => match n {
                    Note::TriedWall(li, keys) => writeln!(out, "n {t} tried {li} {}", keys.0),
                    Note::Stuck(p, times) => writeln!(out, "n {t} stuck {p} {times}"),
                    Note::MissedJump(key, times) => writeln!(out, "n {t} missed {key} {times}"),
                    Note::Trap(polys) => writeln!(
                        out,
                        "n {t} trap {}",
                        polys
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    Note::FailedPress(at, li) => {
                        writeln!(out, "n {t} pressed {:?} {:?} {li}", at.0, at.1)
                    }
                },
            };
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn read(path: &Path) -> Result<Recording, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut entries = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let bad = || format!("{}:{}: can't read {line:?}", path.display(), i + 1);
            let w: Vec<&str> = line.split_whitespace().collect();
            let (Some(kind), Some(tic)) = (w.first(), w.get(1).and_then(|t| t.parse::<i64>().ok()))
            else {
                return Err(bad());
            };
            let f = |k: usize| w.get(k).and_then(|v| v.parse::<f64>().ok()).ok_or_else(bad);
            let n = |k: usize| {
                w.get(k)
                    .and_then(|v| v.parse::<usize>().ok())
                    .ok_or_else(bad)
            };
            let int = |k: usize| w.get(k).and_then(|v| v.parse::<i32>().ok()).ok_or_else(bad);
            let flag = |k: usize| Ok::<bool, String>(n(k)? != 0);
            entries.push(match *kind {
                "c" => Entry::Tic(
                    tic,
                    Controls {
                        forward: f(2)?,
                        strafe: f(3)?,
                        turn: f(4)?,
                        use_: flag(5)?,
                        attack: flag(6)?,
                        run: flag(7)?,
                        weapon: int(8)?,
                        exact: w.get(9).and_then(|v| v.parse().ok()),
                    },
                ),
                "r" => Entry::Restart(tic, w[2..].join(" ")),
                "n" => Entry::Note(
                    tic,
                    match w.get(2).copied() {
                        Some("tried") => {
                            Note::TriedWall(n(3)?, Keys(u8::try_from(n(4)?).map_err(|_| bad())?))
                        }
                        Some("stuck") => Note::Stuck(n(3)?, int(4)?),
                        Some("missed") => {
                            Note::MissedJump(w.get(3).ok_or_else(bad)?.to_string(), int(4)?)
                        }
                        Some("trap") => Note::Trap(
                            w.get(3)
                                .map(|v| {
                                    v.split(',')
                                        .filter_map(|p| p.parse().ok())
                                        .collect::<BTreeSet<usize>>()
                                })
                                .ok_or_else(bad)?,
                        ),
                        Some("pressed") => Note::FailedPress((f(3)?, f(4)?), n(5)?),
                        _ => return Err(bad()),
                    },
                ),
                _ => return Err(bad()),
            });
        }
        Ok(Recording { entries })
    }
}
