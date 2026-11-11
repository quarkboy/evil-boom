//! The bot's score: every level from a pistol start on one life (mortal), on the chosen skills
//! (HMP and UV unless told), each K times on random seeds; a row a run, kept as a TSV, then the
//! report (a run with nothing new for 5 game minutes is over: STALLED, as good as stuck). A run scores 1 for getting out, else half the share of the level's monsters killed
//! (partial credit: how far into the level's fight she got; the user's measure); a level's score
//! is its runs' mean, the bot's the mean over levels and skills, every level weighing the same
//! (not an episode's, where a level late in it only counts when everything before it went
//! right). Arenas are no part of it (the user's: too artificial). How consistently a level's
//! beaten is reported beside it: its runs' spread (the variance of their points), and the levels
//! beaten every time.
//!
//! Comparing two builds or two tunings: `--like FILE` plays the very runs of an earlier set (its
//! levels, skills and seeds). A run is the same every time on the same seed, so runs a change
//! doesn't touch come out the same and only the ones it does touch differ: the report of two
//! such sets gives the difference run for run, far finer than two sets on seeds of their own
//! (whose noise is about 0.008). `--tuning FILE` and `--bar NAMES` are passed to every run.
//!
//!   evil-boom score [doom|doom2 [maps...]]... [--skills 3,4] [--seeds K] [--jobs N] [--out FILE]
//!                   [--like FILE] [--tuning FILE] [--bar NAMES]
//!   evil-boom score-report FILE...    the report of one or more score TSVs (more: how far apart)
//!   evil-boom score-watch FILE [--total N]   a small window showing a score's progress as its
//!                                     TSV fills (`score` opens one itself unless --no-window)

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// One run: the level, skill and seed, and how it went.
#[derive(Clone, Debug)]
pub struct Row {
    pub wad: String,
    pub map: String,
    pub skill: u32,
    pub seed: u32,
    pub outcome: String,
    pub seconds: f64,
    pub kills: (u32, u32),
    pub items: (u32, u32),
    pub lines: (u32, u32),
    pub sectors: (u32, u32),
    /// times she died before the run ended (a run of several lives: out with this many deaths
    /// is out on the next life)
    pub deaths: u32,
}

impl Row {
    pub fn exited(&self) -> bool {
        matches!(
            self.outcome.as_str(),
            "EXITED" | "EXPERT" | "SECRETS" | "COMPLETE"
        )
    }

    /// A run's points: 1 out, else half the share of monsters killed.
    pub fn points(&self) -> f64 {
        if self.exited() {
            1.0
        } else {
            PARTIAL * self.kill_share()
        }
    }

    pub fn kill_share(&self) -> f64 {
        if self.kills.1 == 0 {
            0.0
        } else {
            // (at most all of them: monsters a level makes as it goes (a pain elemental's lost
            // souls, a spawner's) count as kills but not in its total; MAP30 UV, 1.6 of it)
            (f64::from(self.kills.0) / f64::from(self.kills.1)).min(1.0)
        }
    }

    const HEADER: &'static str = "wad\tmap\tskill\tseed\toutcome\tseconds\tkills\tkills_total\titems\titems_total\tlines\tlines_total\tsectors\tsectors_total\tdeaths";

    fn tsv(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{:.0}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.wad,
            self.map,
            self.skill,
            self.seed,
            self.outcome,
            self.seconds,
            self.kills.0,
            self.kills.1,
            self.items.0,
            self.items.1,
            self.lines.0,
            self.lines.1,
            self.sectors.0,
            self.sectors.1,
            self.deaths
        )
    }

    fn from_tsv(line: &str) -> Option<Row> {
        let f: Vec<&str> = line.split('\t').collect();
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<u32>().ok());
        Some(Row {
            wad: f.first()?.to_string(),
            map: f.get(1)?.to_string(),
            skill: n(2)?,
            seed: n(3)?,
            outcome: f.get(4)?.to_string(),
            seconds: f.get(5)?.parse().ok()?,
            kills: (n(6)?, n(7)?),
            items: (n(8)?, n(9)?),
            lines: (n(10)?, n(11)?),
            sectors: (n(12)?, n(13)?),
            deaths: n(14).unwrap_or(0),
        })
    }

    /// From a run-map's RESULT line ("EXITED items 4/9, 81s game, ...; 342/370 lines, 58/59
    /// sectors, ...").
    fn from_result(wad: &str, map: &str, skill: u32, seed: u32, result: &str) -> Row {
        let words: Vec<&str> = result
            .split([' ', ',', ';'])
            .filter(|w| !w.is_empty())
            .collect();
        let pair = |w: &str| {
            let (a, b) = w.split_once('/')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        };
        let before = |what: &str| {
            words
                .iter()
                .position(|w| *w == what)
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| pair(words[i]))
                .unwrap_or((0, 0))
        };
        let seconds = words
            .iter()
            .position(|w| *w == "game")
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| words[i].trim_end_matches('s').parse().ok())
            .unwrap_or(0.0);
        Row {
            wad: wad.to_string(),
            map: map.to_string(),
            skill,
            seed,
            outcome: words.first().copied().unwrap_or("ERROR").to_string(),
            seconds,
            kills: words
                .iter()
                .position(|w| *w == "kills")
                .and_then(|i| words.get(i + 1))
                .and_then(|w| pair(w))
                .unwrap_or((0, 0)),
            items: words
                .iter()
                .position(|w| *w == "items")
                .and_then(|i| words.get(i + 1))
                .and_then(|w| pair(w))
                .unwrap_or((0, 0)),
            lines: before("lines"),
            sectors: before("sectors"),
            deaths: words
                .iter()
                .position(|w| *w == "deaths")
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| words[i].parse().ok())
                .unwrap_or(0),
        }
    }
}

/// A run that doesn't get out scores this times the share of monsters killed.
const PARTIAL: f64 = 0.5;

/// A seed from the clock and a count (splitmix64): a new one each run.
fn random_seed(count: u64) -> u32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut z = t ^ count.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) & 0x7FFF_FFFF) as u32
}

pub fn run(args: &[String]) -> Result<(), String> {
    let (mut skills, mut seeds, mut jobs, mut out) = (vec![3u32, 4], 8u32, 12usize, None::<String>);
    let mut window = true;
    let (mut like, mut passed) = (None::<String>, Vec::<String>::new());
    let mut lives = 1u32;
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1);
        match args[i].as_str() {
            "--skills" => {
                skills = v
                    .ok_or("--skills 3,4")?
                    .split(',')
                    .map(|s| s.parse().map_err(|_| "--skills 3,4".to_string()))
                    .collect::<Result<_, _>>()?;
                i += 2;
            }
            "--seeds" => {
                seeds = v.and_then(|v| v.parse().ok()).ok_or("--seeds K")?;
                i += 2;
            }
            "--jobs" => {
                jobs = v.and_then(|v| v.parse().ok()).ok_or("--jobs N")?;
                i += 2;
            }
            "--out" => {
                out = Some(v.ok_or("--out FILE")?.clone());
                i += 2;
            }
            "--like" => {
                like = Some(v.ok_or("--like FILE")?.clone());
                i += 2;
            }
            "--lives" => {
                let n: u32 = v.and_then(|v| v.parse().ok()).ok_or("--lives N")?;
                // (several lives: the hour a run has is for one; so many hours, up to four)
                passed.extend([
                    "--lives".to_string(),
                    n.to_string(),
                    "--max-seconds".to_string(),
                    (3600 * n.clamp(1, 4)).to_string(),
                ]);
                lives = n;
                i += 2;
            }
            "--known" => {
                passed.push("--known".into());
                i += 1;
            }
            flag @ ("--tuning" | "--bar") => {
                passed.push(flag.to_string());
                passed.push(v.ok_or_else(|| format!("{flag}: a value"))?.clone());
                i += 2;
            }
            "--no-window" => {
                window = false;
                i += 1;
            }
            t => {
                let lower = t.to_lowercase();
                if lower == "doom" || lower == "doom2" || lower.ends_with(".wad") {
                    groups.push((t.to_string(), Vec::new()));
                } else if let Some(g) = groups.last_mut() {
                    g.1.push(t.to_uppercase());
                } else {
                    return Err(format!("{t}: name a WAD before its maps"));
                }
                i += 1;
            }
        }
    }
    // (the very runs of an earlier set: its levels, skills and seeds, in its order)
    let like: Option<Vec<Row>> = match like {
        Some(p) => {
            if !groups.is_empty() {
                return Err("--like FILE plays that set's levels: name none".into());
            }
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
            Some(text.lines().skip(1).filter_map(Row::from_tsv).collect())
        }
        None => None,
    };
    if groups.is_empty() && like.is_none() {
        groups = vec![("doom".into(), Vec::new()), ("doom2".into(), Vec::new())];
    }
    let mut levels: Vec<(String, String)> = Vec::new();
    for (w, maps) in groups {
        let maps = if maps.is_empty() {
            crate::wad::Wad::open(&crate::config::resolve_wad(&w))?.map_names()
        } else {
            maps
        };
        levels.extend(maps.into_iter().map(|m| (w.clone(), m)));
    }
    // (each level's runs spread over the whole sweep, not one level's all at once: the long ones
    // don't come in a lump at the end)
    let mut count = 0u64;
    let mut todo: VecDeque<(String, String, u32, u32)> = VecDeque::new();
    for _ in 0..seeds {
        for &skill in &skills {
            for (w, m) in &levels {
                count += 1;
                todo.push_back((w.clone(), m.clone(), skill, random_seed(count)));
            }
        }
    }
    if let Some(rows) = like {
        todo = rows
            .into_iter()
            .map(|r| (r.wad, r.map, r.skill, r.seed))
            .collect();
    }
    let total = todo.len();
    let path = out.map_or_else(
        || {
            crate::config::out_dir()
                .join("score")
                .join(format!("score.{}.tsv", crate::config::run_stamp()))
        },
        std::path::PathBuf::from,
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(file, "{}", Row::HEADER).map_err(|e| e.to_string())?;
    let mut stats =
        std::fs::File::create(path.with_extension("stats.jsonl")).map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    // (the progress window, its own process: a failure to open one costs the score nothing)
    if window && cfg!(feature = "viewer") {
        let _ = Command::new(&me)
            .arg("score-watch")
            .arg(&path)
            .args(["--total", &total.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    let mut running: Vec<((String, String, u32, u32), Child)> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    while !todo.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some(job) = todo.pop_front() else { break };
            let child = Command::new(&me)
                .args([
                    "run-map",
                    &job.0,
                    &job.1,
                    "--fight",
                    &job.2.to_string(),
                    "--mortal",
                    "--seed",
                    &job.3.to_string(),
                    "--stall-out",
                ])
                .args(if lives == 1 {
                    &["--lives", "1"][..]
                } else {
                    &[]
                })
                .args(&passed)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            running.push((job, child));
        }
        let Some(i) = running
            .iter_mut()
            .position(|(_, c)| c.try_wait().ok().flatten().is_some())
        else {
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        };
        let ((w, m, skill, seed), mut child) = running.swap_remove(i);
        let lines: Vec<String> = child
            .stdout
            .take()
            .map(BufReader::new)
            .map(|r| r.lines().map_while(Result::ok).collect())
            .unwrap_or_default();
        let line = lines
            .iter()
            .find(|l| l.starts_with("RESULT "))
            .cloned()
            .unwrap_or_else(|| "RESULT ERROR".into());
        // (the run's counts, a line of JSON a run beside the TSV: FILE.stats.jsonl)
        if let Some(st) = lines.iter().find_map(|l| l.strip_prefix("STATS ")) {
            let _ = writeln!(stats, "{{\"wad\":\"{w}\",\"map\":\"{m}\",\"skill\":{skill},\"seed\":{seed},\"stats\":{st}}}");
        }
        let row = Row::from_result(&w, &m, skill, seed, line.trim_start_matches("RESULT "));
        writeln!(file, "{}", row.tsv()).map_err(|e| e.to_string())?;
        println!(
            "[{:4}/{total}] {w} {m} skill {skill} seed {seed}: {} {:.0}s",
            rows.len() + 1,
            row.outcome,
            row.seconds
        );
        rows.push(row);
    }
    println!("\nruns in {}\n", path.display());
    print!("{}", report(&[rows]));
    Ok(())
}

pub fn report_files(paths: &[String]) -> Result<(), String> {
    let mut sets = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
        sets.push(
            text.lines()
                .skip(1)
                .filter_map(Row::from_tsv)
                .collect::<Vec<_>>(),
        );
    }
    print!("{}", report(&sets));
    Ok(())
}

type Key = (String, String, u32);

/// A level on a skill: its runs, exits, mean points, the points' variance, and the mean kill
/// share.
#[derive(Clone, Copy, Default)]
struct Level {
    runs: u32,
    exits: u32,
    points: f64,
    spread: f64,
    kills: f64,
}

fn tally(rows: &[Row]) -> BTreeMap<Key, Level> {
    let mut by: BTreeMap<Key, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by.entry((r.wad.clone(), r.map.clone(), r.skill))
            .or_default()
            .push(r);
    }
    by.into_iter()
        .map(|(k, rs)| {
            let n = rs.len() as f64;
            let points = rs.iter().map(|r| r.points()).sum::<f64>() / n;
            let spread = rs
                .iter()
                .map(|r| (r.points() - points).powi(2))
                .sum::<f64>()
                / n;
            let level = Level {
                runs: rs.len() as u32,
                exits: rs.iter().filter(|r| r.exited()).count() as u32,
                points,
                spread,
                kills: rs.iter().map(|r| r.kill_share()).sum::<f64>() / n,
            };
            (k, level)
        })
        .collect()
}

/// Over levels (on one skill, or all): (score, exit share, kill share, spread, beaten every run,
/// never beaten, levels).
fn summary(
    t: &BTreeMap<Key, Level>,
    skill: Option<u32>,
) -> (f64, f64, f64, f64, usize, usize, usize) {
    let ls: Vec<&Level> = t
        .iter()
        .filter(|(k, _)| skill.is_none_or(|s| k.2 == s))
        .map(|(_, l)| l)
        .collect();
    let n = ls.len().max(1) as f64;
    let mean = |f: &dyn Fn(&Level) -> f64| ls.iter().map(|l| f(l)).sum::<f64>() / n;
    (
        mean(&|l| l.points),
        mean(&|l| f64::from(l.exits) / f64::from(l.runs.max(1))),
        mean(&|l| l.kills),
        mean(&|l| l.spread),
        ls.iter().filter(|l| l.exits == l.runs).count(),
        ls.iter().filter(|l| l.exits == 0).count(),
        ls.len(),
    )
}

pub fn report(sets: &[Vec<Row>]) -> String {
    let mut out = String::new();
    let tallies: Vec<_> = sets.iter().map(|rows| tally(rows)).collect();
    let skills: Vec<u32> = {
        let mut s: Vec<u32> = tallies.iter().flat_map(|t| t.keys().map(|k| k.2)).collect();
        s.sort_unstable();
        s.dedup();
        s
    };
    // (per level: each set's exits/runs and points)
    let mut keys: Vec<Key> = tallies.iter().flat_map(|t| t.keys().cloned()).collect();
    keys.sort();
    keys.dedup();
    out += "level          skill  per set: exits/runs points\n";
    for k in &keys {
        let cells: Vec<String> = tallies
            .iter()
            .map(|t| {
                t.get(k).map_or_else(
                    || "    -     ".into(),
                    |l| format!("{:2}/{:<2} {:.2}", l.exits, l.runs, l.points),
                )
            })
            .collect();
        out += &format!("{:5} {:6} {:5}  {}\n", k.0, k.1, k.2, cells.join("   "));
    }
    out += "\n";
    for (i, t) in tallies.iter().enumerate() {
        let runs: u32 = t.values().map(|l| l.runs).sum();
        out += &format!("set {} ({runs} runs):\n", i + 1);
        for s in skills.iter().map(|&s| Some(s)).chain([None]) {
            let (score, exits, kills, spread, all, none, n) = summary(t, s);
            let name = s.map_or_else(|| "all    ".to_string(), |s| format!("skill {s}"));
            out += &format!(
                "  {name}: SCORE {score:.3} (exits {exits:.3}, kills {kills:.3}); spread {spread:.3}; of {n} levels beaten every run {all}, never {none}\n"
            );
        }
    }
    if tallies.len() > 1 {
        // (how far apart the sets are: the score's own noise, and each level's)
        let scores: Vec<f64> = tallies.iter().map(|t| summary(t, None).0).collect();
        let m = scores.iter().sum::<f64>() / scores.len() as f64;
        let sd = (scores.iter().map(|s| (s - m).powi(2)).sum::<f64>() / (scores.len() - 1) as f64)
            .sqrt();
        out += &format!("\nscore over the sets: mean {m:.3}, standard deviation {sd:.3}\n");
        let mut level_var = 0.0;
        let mut counted = 0;
        let mut apart: Vec<(f64, &Key)> = Vec::new();
        for k in &keys {
            let ps: Vec<f64> = tallies
                .iter()
                .filter_map(|t| t.get(k))
                .map(|l| l.points)
                .collect();
            if ps.len() < 2 {
                continue;
            }
            let pm = ps.iter().sum::<f64>() / ps.len() as f64;
            let v = ps.iter().map(|p| (p - pm).powi(2)).sum::<f64>() / (ps.len() - 1) as f64;
            level_var += v;
            counted += 1;
            apart.push((v.sqrt(), k));
        }
        out += &format!(
            "a level's points between sets: standard deviation {:.3} on average\n",
            (level_var / f64::from(counted.max(1))).sqrt()
        );
        apart.sort_by(|a, b| b.0.total_cmp(&a.0));
        out += "furthest apart:";
        for (sd, k) in apart.iter().take(10) {
            out += &format!(" {} {}/{} {:.2};", k.0, k.1, k.2, sd);
        }
        out += "\n";
    }
    out += &by_life(sets);
    out += &paired(sets);
    out
}

/// Runs of several lives: the share of them out by each life (out with d deaths is out on life
/// d + 1), a set a line: how fast she learns a level.
fn by_life(sets: &[Vec<Row>]) -> String {
    let most = sets.iter().flatten().map(|r| r.deaths).max().unwrap_or(0);
    if most == 0 {
        return String::new();
    }
    let mut out = String::from("\nout by life (the share of runs out on that life or before):\n");
    for (i, rows) in sets.iter().enumerate() {
        let mut skills: Vec<u32> = rows.iter().map(|r| r.skill).collect();
        skills.sort_unstable();
        skills.dedup();
        for skill in skills.iter().map(|&s| Some(s)).chain([None]) {
            let of: Vec<&Row> = rows
                .iter()
                .filter(|r| skill.is_none_or(|s| r.skill == s))
                .collect();
            let n = of.len().max(1) as f64;
            let shares: Vec<String> = (1..=most + 1)
                .map(|life| {
                    format!(
                        "{life}: {:.3}",
                        of.iter().filter(|r| r.exited() && r.deaths < life).count() as f64 / n
                    )
                })
                .collect();
            let name = skill.map_or_else(|| "all    ".to_string(), |s| format!("skill {s}"));
            out += &format!("  set {} {name}: {}\n", i + 1, shares.join("  "));
        }
    }
    out
}

/// Two sets of the same runs (level, skill and seed: `--like`), run for run: the second's score
/// less the first's, with its standard error (the runs' differences, most of them nothing), and
/// the levels that moved most.
fn paired(sets: &[Vec<Row>]) -> String {
    let [a, b] = sets else { return String::new() };
    let key = |r: &Row| (r.wad.clone(), r.map.clone(), r.skill, r.seed);
    let first: BTreeMap<_, &Row> = a.iter().map(|r| (key(r), r)).collect();
    let pairs: Vec<(&Row, &Row)> = b
        .iter()
        .filter_map(|r| first.get(&key(r)).map(|&x| (x, r)))
        .collect();
    if pairs.len() < a.len().min(b.len()) / 2 || pairs.is_empty() {
        return String::new(); // (seeds of their own: nothing to pair)
    }
    let mut out = String::new();
    let n = pairs.len() as f64;
    let diffs: Vec<f64> = pairs.iter().map(|(x, y)| y.points() - x.points()).collect();
    let mean = diffs.iter().sum::<f64>() / n;
    let se =
        (diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0) / n).sqrt();
    let same = pairs
        .iter()
        .filter(|(x, y)| x.outcome == y.outcome && x.seconds == y.seconds && x.kills == y.kills)
        .count();
    let (better, worse) = (
        diffs.iter().filter(|&&d| d > 1e-9).count(),
        diffs.iter().filter(|&&d| d < -1e-9).count(),
    );
    out += &format!(
        "\nrun for run ({} runs on the same seeds): set 2 less set 1 {mean:+.4}, standard error {se:.4}; {same} runs the same to the tic, {better} better, {worse} worse\n",
        pairs.len()
    );
    for skill in {
        let mut s: Vec<u32> = pairs.iter().map(|(x, _)| x.skill).collect();
        s.sort_unstable();
        s.dedup();
        s
    } {
        let d: Vec<f64> = pairs
            .iter()
            .filter(|(x, _)| x.skill == skill)
            .map(|(x, y)| y.points() - x.points())
            .collect();
        out += &format!(
            "  skill {skill}: {:+.4}\n",
            d.iter().sum::<f64>() / d.len().max(1) as f64
        );
    }
    let mut by: BTreeMap<Key, (f64, u32)> = BTreeMap::new();
    for (x, y) in &pairs {
        let e = by
            .entry((x.wad.clone(), x.map.clone(), x.skill))
            .or_default();
        e.0 += y.points() - x.points();
        e.1 += 1;
    }
    let mut moved: Vec<(f64, &Key)> = by
        .iter()
        .map(|(k, &(d, c))| (d / f64::from(c.max(1)), k))
        .filter(|(d, _)| d.abs() > 1e-9)
        .collect();
    moved.sort_by(|p, q| q.0.abs().total_cmp(&p.0.abs()));
    out += "  moved most:";
    for (d, k) in moved.iter().take(12) {
        out += &format!(" {} {}/{} {d:+.2};", k.0, k.1, k.2);
    }
    out += "\n";
    out
}

/// A score's progress in a small window, read from its TSV as rows come in, till all `total` are
/// in (or the window's closed).
#[cfg(feature = "viewer")]
pub fn watch(args: &[String]) -> Result<(), String> {
    use font8x8::legacy::BASIC_LEGACY;
    use minifb::{Key, Window, WindowOptions};
    const W: usize = 460;
    const H: usize = 132;
    let file = args
        .first()
        .ok_or("usage: evil-boom score-watch FILE [--total N]")?
        .clone();
    let total: usize = args
        .iter()
        .position(|a| a == "--total")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1088);
    let mut window =
        Window::new("Evil: score", W, H, WindowOptions::default()).map_err(|e| e.to_string())?;
    window.set_target_fps(4);
    let mut buf = vec![0u32; W * H];
    let start = std::time::Instant::now();
    let mut first: Option<(std::time::Instant, usize)> = None;
    let mut done_at: Option<std::time::Instant> = None;
    let text = |buf: &mut Vec<u32>, s: &str, x0: usize, y0: usize, color: u32| {
        for (i, ch) in s.chars().enumerate() {
            let g = BASIC_LEGACY.get(ch as usize).copied().unwrap_or([0; 8]);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                            let (x, y) = (x0 + i * 16 + col * 2 + dx, y0 + row * 2 + dy);
                            if x < W && y < H {
                                buf[y * W + x] = color;
                            }
                        }
                    }
                }
            }
        }
    };
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let rows: Vec<Row> = std::fs::read_to_string(&file)
            .map(|t| t.lines().skip(1).filter_map(Row::from_tsv).collect())
            .unwrap_or_default();
        let n = rows.len();
        // (the time left from the pace since this window first saw rows come in)
        let now = std::time::Instant::now();
        let base = *first.get_or_insert((now, n));
        let rate = if n > base.1 {
            (n - base.1) as f64 / now.duration_since(base.0).as_secs_f64().max(1.0)
        } else {
            0.0
        };
        let left = if rate > 0.0 {
            (total.saturating_sub(n)) as f64 / rate
        } else {
            f64::NAN
        };
        let exits = rows.iter().filter(|r| r.exited()).count();
        let points = if n == 0 {
            0.0
        } else {
            rows.iter().map(Row::points).sum::<f64>() / n as f64
        };
        buf.fill(0x0016_1a20);
        let share = (n as f64 / total.max(1) as f64).min(1.0);
        let (bx, by, bw, bh) = (16, 16, W - 32, 26);
        for y in by..by + bh {
            for x in bx..bx + bw {
                let inside = ((x - bx) as f64) < share * bw as f64;
                buf[y * W + x] = if inside { 0x0040_a060 } else { 0x0030_343c };
            }
        }
        text(
            &mut buf,
            &format!("{n}/{total} runs  {:.0}%", share * 100.0),
            16,
            54,
            0x00e0_e0e0,
        );
        text(
            &mut buf,
            &format!("exits {exits}  points {points:.2}"),
            16,
            78,
            0x00b0_d0b0,
        );
        let eta = if left.is_finite() {
            format!("{:.0} min left", (left / 60.0).ceil())
        } else {
            format!("{:.0} s in", start.elapsed().as_secs_f64())
        };
        text(
            &mut buf,
            &if n >= total { "done".to_string() } else { eta },
            16,
            102,
            0x00a0_a0b0,
        );
        window
            .update_with_buffer(&buf, W, H)
            .map_err(|e| e.to_string())?;
        if n >= total {
            // (a moment on "done", then gone)
            if done_at.get_or_insert(now).elapsed().as_secs() >= 5 {
                break;
            }
        }
    }
    Ok(())
}
