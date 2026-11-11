//! The survey of a WAD's secrets (`evil-boom secret-survey`), for the wall rubric:
//! which lines must be pressed or shot to get into each secret sector, and how the rubric scores
//! them, and every other wall she's mapped, as she sees them once she's explored all she can
//! without searching (the moment a player starts looking for what's hidden).
//!
//! Which lines reveal a secret, from the map alone: every sector a trigger moves counts as open
//! (doors up, floors down: any way through), keys as had; a line pressed or shot reveals secret
//! sector S if S can be got to from the start with every trigger set off but not with every one
//! but that line, and the exit can be got to without it (not the way on, which every secret past
//! it needs too). Got to: through two-sided lines that don't block, with room to stand and a step
//! up of 24 at most (either side open: any), and teleporters. Jumps, lifts' timing and crushers
//! aren't weighed: a secret only got to that way is counted apart.
//!
//! Each map runs in a process of its own (the engine is one a process), rows on stdout; the
//! parent gathers them into out/survey/<WAD>.tsv and the statistics.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::knowledge::model::{Signs, Wall};
use crate::map::{MapData, ML_BLOCKING, ML_SECRET};
use crate::navigator::{Navigator, MAX_STEP, PLAYER_HEIGHT};
use crate::specials::{classify_line, Category, Trigger};
use crate::sweep::{setup, RunOptions, Setup};

/// the most game time a map's given to run out of things to do
const SURVEY_SECONDS: f64 = 1500.0;

/// the thresholds the statistics are given for
const THRESHOLDS: [f64; 9] = [0.0, 1.0, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0];

/// The sectors a trigger line moves (the tagged ones; a door's own line, its back sector).
fn moved(m: &MapData, li: usize) -> Vec<usize> {
    let l = &m.linedefs[li];
    if l.tag != 0 {
        return m
            .sectors
            .iter()
            .filter(|s| s.tag == l.tag)
            .map(|s| s.index)
            .collect();
    }
    m.sides(li).1.into_iter().collect()
}

/// A line that moves something she can set off: its trigger and what it moves.
fn movers(m: &MapData) -> Vec<(usize, Trigger, Vec<usize>)> {
    (0..m.linedefs.len())
        .filter_map(|li| {
            let info = classify_line(m.linedefs[li].special)?;
            let moves = matches!(
                info.category,
                Category::Door
                    | Category::LockedDoor
                    | Category::Lift
                    | Category::Floor
                    | Category::Ceiling
                    | Category::Crusher
                    | Category::Stairs
            );
            (moves && info.players).then(|| (li, info.trigger, moved(m, li)))
        })
        .collect()
}

/// The sectors got to from `start` with the sectors in `open` open (see the module's notes).
fn reach(
    m: &MapData,
    start: usize,
    open: &BTreeSet<usize>,
    teleports: &[(usize, usize)],
) -> BTreeSet<usize> {
    let mut seen = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(a) = queue.pop_front() {
        let mut next: Vec<usize> = teleports
            .iter()
            .filter(|(from, _)| *from == a)
            .map(|(_, to)| *to)
            .collect();
        for (li, l) in m.linedefs.iter().enumerate() {
            if l.flags & ML_BLOCKING != 0 {
                continue;
            }
            let (Some(f), Some(b)) = m.sides(li) else {
                continue;
            };
            let c = if f == a {
                b
            } else if b == a {
                f
            } else {
                continue;
            };
            let (sa, sc) = (&m.sectors[a], &m.sectors[c]);
            let room = sa.ceiling.min(sc.ceiling) - sa.floor.max(sc.floor) >= PLAYER_HEIGHT
                && sc.floor - sa.floor <= MAX_STEP;
            if open.contains(&a) || open.contains(&c) || room {
                next.push(c);
            }
        }
        for c in next {
            if seen.insert(c) {
                queue.push_back(c);
            }
        }
    }
    seen
}

/// What the map says of its secrets: (each secret sector, the lines pressed or shot that reveal
/// it), and the secret sectors got to with nothing pressed, and those not got to at all.
pub struct Secrets {
    pub reveals: BTreeMap<usize, Vec<usize>>,
    pub walk_in: Vec<usize>,
    pub unreached: Vec<usize>,
    /// got to, but by more than one way (no one line needed)
    pub either: Vec<usize>,
}

pub fn secrets(m: &MapData, nav: &Navigator) -> Secrets {
    let at = |x: f64, y: f64| nav.mesh.poly_at((x, y)).map(|p| nav.sector_of_poly(p));
    let start = m
        .things
        .iter()
        .find(|t| t.kind == 1)
        .and_then(|t| at(t.x, t.y))
        .unwrap_or(0);
    let secret: Vec<usize> = m
        .sectors
        .iter()
        .filter(|s| s.special == 9 || (s.special >= 32 && s.special & 0x80 != 0))
        .map(|s| s.index)
        .collect();
    // (teleporters: from either side of the line to the destination in a sector it tags)
    let mut teleports = Vec::new();
    for (li, l) in m.linedefs.iter().enumerate() {
        let Some(info) = classify_line(l.special) else {
            continue;
        };
        if info.category != Category::Teleport || l.tag == 0 {
            continue;
        }
        let dest = m
            .things
            .iter()
            .filter(|t| t.kind == 14)
            .filter_map(|t| at(t.x, t.y))
            .find(|&s| m.sectors[s].tag == l.tag);
        let (f, b) = m.sides(li);
        if let Some(d) = dest {
            teleports.extend([f, b].into_iter().flatten().map(|s| (s, d)));
        }
    }
    let lines = movers(m);
    let pressed = |t: &Trigger| {
        matches!(
            t,
            Trigger::S1 | Trigger::SR | Trigger::G1 | Trigger::GR | Trigger::D1 | Trigger::DR
        )
    };
    let open: BTreeSet<usize> = lines
        .iter()
        .flat_map(|(_, _, s)| s.iter().copied())
        .collect();
    let by_walking: BTreeSet<usize> = lines
        .iter()
        .filter(|(_, t, _)| !pressed(t))
        .flat_map(|(_, _, s)| s.iter().copied())
        .collect();
    let all = reach(m, start, &open, &teleports);
    let walked = reach(m, start, &by_walking, &teleports);
    let exits: BTreeSet<usize> = (0..m.linedefs.len())
        .filter(|&li| {
            classify_line(m.linedefs[li].special).is_some_and(|i| i.category == Category::Exit)
        })
        .flat_map(|li| {
            let (f, b) = m.sides(li);
            [f, b].into_iter().flatten()
        })
        .collect();
    // (a sector only a press or a shot opens, kept shut: the secrets it cuts off are revealed
    // by every line that opens it, whichever she'd use; a door with a line either side, say)
    let mut reveals: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &x in open.difference(&by_walking) {
        let mut shut = open.clone();
        shut.remove(&x);
        let without = reach(m, start, &shut, &teleports);
        // (the way on: the exit cut off without it)
        if exits.iter().any(|e| all.contains(e)) && !exits.iter().any(|e| without.contains(e)) {
            continue;
        }
        let openers: Vec<usize> = lines
            .iter()
            .filter(|(_, t, s)| pressed(t) && s.contains(&x))
            .map(|(li, _, _)| *li)
            .collect();
        for &s in secret
            .iter()
            .filter(|s| all.contains(s) && !without.contains(s))
        {
            let r = reveals.entry(s).or_default();
            for &li in &openers {
                if !r.contains(&li) {
                    r.push(li);
                }
            }
        }
    }
    let walk_in: Vec<usize> = secret
        .iter()
        .copied()
        .filter(|s| walked.contains(s))
        .collect();
    let unreached: Vec<usize> = secret
        .iter()
        .copied()
        .filter(|s| !all.contains(s))
        .collect();
    let either: Vec<usize> = secret
        .iter()
        .copied()
        .filter(|s| all.contains(s) && !walked.contains(s) && !reveals.contains_key(s))
        .collect();
    Secrets {
        reveals,
        walk_in,
        unreached,
        either,
    }
}

/// One map (in this process): played to its first "nothing else to do", then a row per wall she
/// has mapped and per revealing line, on stdout ("ROW\t" first).
pub fn survey_map(wad: &str, map: &str) -> Result<(), String> {
    let o = RunOptions {
        fight: 0,
        mortal: false,
        lives: 1,
        max_seconds: SURVEY_SECONDS,
        things: true,
        ..RunOptions::default()
    };
    let Setup {
        m,
        nav,
        mut e,
        mut agent,
        map_name,
        ..
    } = setup(wad, map, &o, false, None)?;
    let sec = secrets(&m, &nav);
    let mut revealing: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (s, lines) in &sec.reveals {
        for &li in lines {
            revealing.entry(li).or_default().push(*s);
        }
    }
    // (a revealing line she set out to press, as what, and the rubric's view of it just then:
    // found by her own means, the score she'd have given it)
    let mut taken: BTreeMap<usize, (String, Wall)> = BTreeMap::new();
    agent.x_mut().exit_held_back = true; // (everything else before the exit)
    let mut c = None;
    let mut last = e.observe();
    let mut secrets_seen = 0;
    while (e.tic as f64) < SURVEY_SECONDS * crate::motor::TICRATE
        && !e.finished()
        && agent.x().out_of_options.is_none()
    {
        let ob = e.observe();
        let found = e.tally()[2].0;
        if found > secrets_seen {
            if let Some(p) = nav.mesh.poly_at(ob.pos()) {
                agent.x_mut().found_secret(nav.sector_of_poly(p), ob.tic);
            }
        }
        secrets_seen = found;
        let ctl = agent.act(&ob, c.as_ref());
        if let Some(o) = &agent.x().current {
            let target = o.wall.or_else(|| {
                o.trigger
                    .as_ref()
                    .and_then(|t| usize::try_from(t.line).ok())
            });
            if let Some(li) =
                target.filter(|li| revealing.contains_key(li) && !taken.contains_key(li))
            {
                let how = format!("taken as {:?}", o.kind).to_lowercase();
                let wall = agent
                    .x()
                    .wall_survey(&ob)
                    .into_iter()
                    .find(|(l, _)| *l == li)
                    .map_or(Wall::Skip("unmapped"), |(_, w)| w);
                taken.insert(li, (how, wall));
            }
        }
        e.step(&ctl);
        c = Some(ctl);
        last = ob;
    }
    let ran_out = agent.x().out_of_options.is_some();
    let tally = e.tally()[2];
    let x = agent.x();
    let k = &x.knowledge;
    let walls: BTreeMap<usize, Wall> = x.wall_survey(&last).into_iter().collect();
    let pressed: BTreeSet<usize> = k.fired.iter().map(|f| f.line).collect();
    let list = |v: &[usize]| {
        v.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    println!(
        "MAP\t{map_name}\t{}\t{}\t{}\t{}\t{}\t{}/{}\t{:.0}",
        sec.reveals.len() + sec.walk_in.len() + sec.unreached.len() + sec.either.len(),
        list(&sec.reveals.keys().copied().collect::<Vec<_>>()),
        list(&sec.walk_in),
        list(&sec.either),
        list(&sec.unreached),
        tally.0,
        tally.1,
        if ran_out {
            e.tic as f64 / crate::motor::TICRATE
        } else {
            -1.0
        }
    );
    let lines: BTreeSet<usize> = walls.keys().chain(revealing.keys()).copied().collect();
    for li in lines {
        let l = &m.linedefs[li];
        let trigger =
            classify_line(l.special).map_or("-".to_string(), |i| format!("{:?}", i.trigger));
        let view = |w: Option<&Wall>| match w {
            None => ("unmapped".to_string(), None),
            Some(Wall::Skip(why)) => ((*why).to_string(), None),
            Some(Wall::Suspect { signs, .. }) => ("suspect".to_string(), Some(*signs)),
        };
        // (what it hides, as the search counts walls: one for each place behind)
        let behind = match walls.get(&li) {
            Some(Wall::Suspect {
                behind: Some(crate::knowledge::model::Behind::Sector(s)),
                ..
            }) => format!("s{s}"),
            _ => format!("l{li}"),
        };
        let (status, signs) = if let Some((how, w)) = taken.get(&li) {
            let (st, signs) = view(Some(w));
            (format!("{how}: {st}"), signs)
        } else if pressed.contains(&li) {
            ("pressed in passing".to_string(), None)
        } else {
            view(walls.get(&li))
        };
        println!(
            "ROW\t{map_name}\t{li}\t{}\t{trigger}\t{}\t{}\t{status}\t{:.1}\t{}\t{:.1}\t{behind}",
            revealing.get(&li).map_or(String::new(), |s| list(s)),
            u8::from(l.flags & ML_SECRET != 0),
            u8::from(k.looks_interactive(li)),
            signs.map_or(0.0, |s| s.score()),
            signs.map_or(String::new(), |s| s
                .named()
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join(",")),
            signs.map_or(f64::NAN, |s| s.secret_score())
        );
    }
    Ok(())
}

/// A row read back.
struct Row {
    map: String,
    line: usize,
    reveals: String,
    trigger: String,
    secret_flag: bool,
    interactive: bool,
    status: String,
    score: f64,
    signs: Vec<String>,
    /// the secret search's score (NaN: not weighed)
    secret_score: f64,
}

/// The maps, a process each (`jobs` at once), gathered: the rows to out/survey/<WAD>.tsv, the
/// statistics to stdout and out/survey/<WAD>.txt.
pub fn survey(wad: &str, maps: &[String], jobs: usize) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut outputs: BTreeMap<String, String> = BTreeMap::new();
    for chunk in maps.chunks(jobs.max(1)) {
        let children: Vec<(String, std::process::Child)> = chunk
            .iter()
            .map(|mp| {
                let child = Command::new(&exe)
                    .args(["secret-survey", wad, mp, "--one"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                Ok((mp.clone(), child))
            })
            .collect::<Result<_, String>>()?;
        for (mp, child) in children {
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            eprintln!("  {mp} done");
            outputs.insert(mp, String::from_utf8_lossy(&out.stdout).to_string());
        }
    }
    let mut rows = Vec::new();
    let mut maps_lines = Vec::new();
    for text in outputs.values() {
        for l in text.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            match f.first() {
                Some(&"MAP") => maps_lines.push(f[1..].join("\t")),
                Some(&"ROW") if f.len() >= 11 => rows.push(Row {
                    map: f[1].into(),
                    line: f[2].parse().unwrap_or(0),
                    reveals: f[3].into(),
                    trigger: f[4].into(),
                    secret_flag: f[5] == "1",
                    interactive: f[6] == "1",
                    status: f[7].into(),
                    score: f[8].parse().unwrap_or(0.0),
                    signs: f[9]
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split('=').next().unwrap_or("").to_string())
                        .collect(),
                    secret_score: f[10].parse().unwrap_or(f64::NAN),
                }),
                _ => {}
            }
        }
    }
    let stem = crate::config::resolve_wad(wad)
        .file_stem()
        .map_or_else(|| "WAD".into(), |s| s.to_string_lossy().to_uppercase());
    let dir = crate::config::out_dir().join("survey");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut tsv = String::from("map\tline\treveals\ttrigger\tml_secret\tlooks_interactive\tstatus\tscore\tsigns\tsecret_score\tbehind\n");
    for text in outputs.values() {
        for l in text.lines().filter_map(|l| l.strip_prefix("ROW\t")) {
            tsv.push_str(l);
            tsv.push('\n');
        }
    }
    std::fs::write(dir.join(format!("{stem}.tsv")), tsv).map_err(|e| e.to_string())?;
    let report = report(&maps_lines, &rows);
    print!("{report}");
    let mut f =
        std::fs::File::create(dir.join(format!("{stem}.txt"))).map_err(|e| e.to_string())?;
    f.write_all(report.as_bytes()).map_err(|e| e.to_string())?;
    println!(
        "\nrows: {}\nreport: {}",
        dir.join(format!("{stem}.tsv")).display(),
        dir.join(format!("{stem}.txt")).display()
    );
    Ok(())
}

fn report(maps: &[String], rows: &[Row]) -> String {
    let mut out = String::new();
    let mut say = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    say("== Secrets by map (secret sectors: revealed by a press or shot of their own / walked into from the start / reached once the way on's open, no press of their own / not reached on these terms (jumps, lifts' timing, ...); found by the snapshot; when she ran out of things to do)".into());
    let count = |s: &str| s.split(',').filter(|x| !x.is_empty()).count();
    let mut totals = [0usize; 5];
    for m in maps {
        let f: Vec<&str> = m.split('\t').collect();
        if f.len() < 8 {
            continue;
        }
        let n = [
            f[1].parse().unwrap_or(0),
            count(f[2]),
            count(f[3]),
            count(f[4]),
            count(f[5]),
        ];
        for (t, v) in totals.iter_mut().zip(n) {
            *t += v;
        }
        let when = if f[7] == "-1" {
            "never".to_string()
        } else {
            format!("{}s", f[7])
        };
        say(format!("{:5} {:2} secrets: {:2} pressed/shot, {:2} walked, {:2} way on, {:2} unreached; found {}; out of things to do at {when}", f[0], n[0], n[1], n[2], n[3], n[4], f[6]));
    }
    say(format!(
        "all   {:2} secrets: {:2} pressed/shot, {:2} walked, {:2} way on, {:2} unreached",
        totals[0], totals[1], totals[2], totals[3], totals[4]
    ));
    let revealing: Vec<&Row> = rows.iter().filter(|r| !r.reveals.is_empty()).collect();
    say(String::new());
    say(format!("== The {} lines pressed or shot to reveal a secret (map line trigger flags status score signs)", revealing.len()));
    for r in &revealing {
        say(format!(
            "{:5} {:5} {:3} {:11} {:24} {:4.1} {}  (sector {})",
            r.map,
            r.line,
            r.trigger,
            format!(
                "{}{}",
                if r.secret_flag { "ml_secret " } else { "" },
                if r.interactive { "looks" } else { "" }
            ),
            r.status,
            r.score,
            r.signs.join(" "),
            r.reveals
        ));
    }
    let mut by_status: BTreeMap<(&str, bool), usize> = BTreeMap::new();
    for r in &revealing {
        *by_status
            .entry((r.status.as_str(), r.interactive))
            .or_default() += 1;
    }
    say(String::new());
    say(
        "== Their status when she ran out of things to do (looks: a switch or door texture)".into(),
    );
    for ((st, looks), n) in &by_status {
        say(format!(
            "  {st:24} {:6} {n}",
            if *looks { "looks" } else { "hidden" }
        ));
    }
    // (the walls the rubric weighs: revealing against the rest, hidden ones)
    let hidden_secret: Vec<&Row> = revealing
        .iter()
        .copied()
        .filter(|r| r.status.ends_with("suspect"))
        .collect();
    let others: Vec<&Row> = rows
        .iter()
        .filter(|r| r.reveals.is_empty() && r.status == "suspect")
        .collect();
    say(String::new());
    say(format!("== Score thresholds: of the {} revealing lines the rubric weighs, and the {} other walls it weighs", hidden_secret.len(), others.len()));
    say("  score>=   revealing   others   share of those at or over it that reveal".into());
    for t in THRESHOLDS {
        let a = hidden_secret.iter().filter(|r| r.score >= t).count();
        let b = others.iter().filter(|r| r.score >= t).count();
        say(format!(
            "  {t:5.1}   {a:4} ({:3.0}%)   {b:6}   {:.1}%",
            100.0 * a as f64 / hidden_secret.len().max(1) as f64,
            100.0 * a as f64 / (a + b).max(1) as f64
        ));
    }
    say(String::new());
    say(format!(
        "== The secret search's score (secret_score; pressed, shot at thoroughness 1-3: {:?})",
        crate::knowledge::model::THOROUGHNESS
    ));
    say("  score>=   revealing   others   share of those at or over it that reveal".into());
    for t in [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0] {
        let a = hidden_secret.iter().filter(|r| r.secret_score >= t).count();
        let b = others.iter().filter(|r| r.secret_score >= t).count();
        say(format!(
            "  {t:5.1}   {a:4} ({:3.0}%)   {b:6}   {:.1}%",
            100.0 * a as f64 / hidden_secret.len().max(1) as f64,
            100.0 * a as f64 / (a + b).max(1) as f64
        ));
    }
    say(String::new());
    say("== Signs: how often each is on a revealing line and on another wall (lift: the first over the second)".into());
    let names = Signs::default().named_all();
    for n in names {
        let a = hidden_secret
            .iter()
            .filter(|r| r.signs.iter().any(|s| s == n))
            .count();
        let b = others
            .iter()
            .filter(|r| r.signs.iter().any(|s| s == n))
            .count();
        let (pa, pb) = (
            a as f64 / hidden_secret.len().max(1) as f64,
            b as f64 / others.len().max(1) as f64,
        );
        say(format!(
            "  {n:13} revealing {a:3} ({:3.0}%)   others {b:6} ({:4.1}%)   lift {}",
            100.0 * pa,
            100.0 * pb,
            if pb > 0.0 {
                format!("{:.1}", pa / pb)
            } else {
                "-".into()
            }
        ));
    }
    let skipped: BTreeMap<&str, usize> = rows
        .iter()
        .filter(|r| r.reveals.is_empty() && r.status != "suspect")
        .fold(BTreeMap::new(), |mut acc, r| {
            *acc.entry(r.status.as_str()).or_default() += 1;
            acc
        });
    say(String::new());
    say("== The other walls she's mapped the rubric doesn't weigh, and why".into());
    for (st, n) in skipped {
        say(format!("  {st:24} {n}"));
    }
    out
}
