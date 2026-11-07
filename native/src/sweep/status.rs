//! Optional live timing and planner-search diagnostics.

use crate::knowledge::explorer::Explorer;
use crate::motor::{Observation, TICRATE};

/// The live status of a run, for watching it as it goes: with `EVIL_STATUS` set to a directory,
/// a line each game second to `<dir>/<IWAD>_<MAP>.status` (game and real seconds, the think
/// time that second and its slowest think, the routes planned and failed so far and their
/// seconds, what she's doing), and the log of any think longer than `SLOW_THINK`.
pub(super) struct Status {
    file: std::fs::File,
    /// every plan search, a line each: budget, expansions, how it ended, seconds, game tic
    searches: std::fs::File,
    secs: f64,
    slowest: f64,
}

impl Drop for Status {
    fn drop(&mut self) {
        crate::stats::collect_searches(false);
    }
}

/// a think longer than this (seconds) has its log written to the status
const SLOW_THINK: f64 = 1.0;

impl Status {
    pub(super) fn open(iwad: &str, map: &str) -> Option<Status> {
        let dir = std::env::var_os("EVIL_STATUS")?;
        let dir = std::path::Path::new(&dir);
        let file = std::fs::File::create(dir.join(format!("{iwad}_{map}.status"))).ok()?;
        let searches = std::fs::File::create(dir.join(format!("{iwad}_{map}.searches"))).ok()?;
        crate::stats::collect_searches(true);
        Some(Status { file, searches, secs: 0.0, slowest: 0.0 })
    }

    pub(super) fn think(&mut self, ob: &Observation, took: f64, x: &Explorer, log_n: usize, real: f64) {
        use std::io::Write;
        self.secs += took;
        self.slowest = self.slowest.max(took);
        for s in crate::stats::take_searches() {
            let _ = writeln!(self.searches, "{},{},{:?},{:.4},{}", s.budget, s.expansions, s.ended, s.secs, ob.tic);
        }
        if took > SLOW_THINK {
            let _ = writeln!(self.file, "  slow think at tic {}: {:.1}s", ob.tic, took);
            for l in &x.log[log_n.min(x.log.len())..] {
                let _ = writeln!(self.file, "    {}", l.chars().take(200).collect::<String>());
            }
        }
        if ob.tic % 35 == 0 {
            let s = x.stats;
            let doing = x.current.as_ref().map_or("-", |o| o.label.as_str());
            let _ = writeln!(
                self.file,
                "{:7.1}s game {:6.1}s real | think {:5.2}s (slowest {:5.2}s) | routes {} ({:.1}s), failed {} ({:.1}s), known none {} | lists {} ({:.1}s, saturate {:.1}s) | {}",
                ob.tic as f64 / TICRATE,
                real,
                self.secs,
                self.slowest,
                s.routed,
                s.routed_secs,
                s.failed,
                s.failed_secs,
                s.known_none,
                s.listed,
                s.listed_secs,
                s.saturate_secs,
                doing
            );
            let _ = self.file.flush();
            self.secs = 0.0;
            self.slowest = 0.0;
        }
    }
}
