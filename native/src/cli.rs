// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Parsing run options and forwarding them to sweep subprocesses.

use evilboom::run::RunOptions;

/// What's left once every option a command knows has been read is names (a WAD, maps, a file):
/// anything there that begins `--` is an option nobody knew, a misspelling as a rule, and is
/// said so, not taken for a name.
pub(crate) fn no_flags(names: &[String]) -> Result<(), String> {
    match names.iter().find(|a| a.starts_with("--")) {
        Some(a) => Err(format!(
            "unknown option {a} (evil-boom help options, or help and the command's name)"
        )),
        None => Ok(()),
    }
}

/// The run options among `args`: (options, jobs, the rest as they were).
pub(crate) fn run_options(args: &[String]) -> Result<(RunOptions, usize, Vec<String>), String> {
    let mut o = RunOptions::default();
    let mut jobs = 8;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a.as_str() {
            "--lives" => {
                // (`unlimited`: as the game itself, again and again: the stream's)
                let v = value("--lives")?;
                o.lives = if v == "unlimited" {
                    i32::MAX
                } else {
                    v.parse().map_err(|_| "--lives: a number, or unlimited")?
                };
                o.lives_given = true;
            }
            "--stranded" => {
                o.stranded = value("--stranded")?
                    .parse()
                    .map_err(|_| "--stranded: a number")?;
            }
            "--snapshot" => {
                o.snapshot = Some(
                    value("--snapshot")?
                        .parse()
                        .map_err(|_| "--snapshot: game seconds")?,
                )
            }
            "--stall" => {
                o.stall_seconds = value("--stall")?
                    .parse()
                    .map_err(|_| "--stall: game seconds")?
            }
            "--from" => {
                let v = value("--from")?;
                let (file, at) = v.rsplit_once('@').ok_or("--from: FILE@SECONDS")?;
                o.from = Some((
                    std::path::PathBuf::from(file),
                    at.parse().map_err(|_| "--from: FILE@SECONDS")?,
                ));
            }
            "--focus" => {
                let v: Vec<f64> = value("--focus")?
                    .split(',')
                    .filter_map(|x| x.trim().parse().ok())
                    .collect();
                let [x, y, z] = v.as_slice() else {
                    return Err("--focus: X,Y,ZOOM".into());
                };
                o.focus = Some((*x, *y, *z));
            }
            "--max-seconds" => {
                o.max_seconds = value("--max-seconds")?
                    .parse()
                    .map_err(|_| "--max-seconds: a number")?
            }
            "--seed" => {
                let v = value("--seed")?;
                o.seed = if v == "random" {
                    None
                } else {
                    Some(v.parse().map_err(|_| "--seed: a number or 'random'")?)
                };
            }
            "--jobs" => jobs = value("--jobs")?.parse().map_err(|_| "--jobs: a number")?,
            "--fight" => {
                o.fight = value("--fight")?
                    .parse()
                    .map_err(|_| "--fight: a skill, 1-5")?
            }
            "--expert" => o.expert = true,
            "--peek" => o.peek = true,
            "--secrets" => o.secrets = true,
            "--mortal" => o.mortal = true,
            "--charmed" => {
                o.mortal = true;
                o.charmed = true;
            }
            "--endless-bullets" => o.endless_bullets = true,
            "--keys" => o.keys = true,
            "--demo" => o.demo = Some(std::path::PathBuf::from(value("--demo")?)),
            "--complete" => o.complete = true,
            "--stall-out" => o.stall_out = true,
            "--decisions" => o.decisions = true,
            "--known" => o.known = true,
            "--bar" => o.bar = Some(value("--bar")?),
            "--advisor" => o.advisor = Some(std::path::PathBuf::from(value("--advisor")?)),
            "--tuning" => o.tuning = Some(std::path::PathBuf::from(value("--tuning")?)),
            "--tape" => o.tape = Some(std::path::PathBuf::from(value("--tape")?)),
            "--ultra" => o.ultra = true,
            "--ultra-after" => {
                o.ultra_after = Some(
                    value("--ultra-after")?
                        .parse()
                        .map_err(|_| "--ultra-after N".to_string())?,
                );
            }
            "--ultra-back" => {
                o.ultra_back = value("--ultra-back")?
                    .parse()
                    .ok()
                    .filter(|x: &f64| *x >= 1.0 && *x <= 20.0)
                    .ok_or("--ultra-back S (1 to 20)")?;
            }
            "--ultra-speed" => {
                o.ultra_speed = value("--ultra-speed")?
                    .parse()
                    .ok()
                    .filter(|x: &f64| *x > 0.0 && *x <= 1.0)
                    .ok_or("--ultra-speed X (over 0, up to 1)")?;
            }
            "--workers" => {
                o.workers = value("--workers")?
                    .parse()
                    .map_err(|_| "--workers N".to_string())?;
            }
            "--carry" => o.carry = Some(parse_carry(&value("--carry")?)?),
            _ => rest.push(a.clone()),
        }
    }
    if o.demo.is_some() && !o.lives_given {
        o.lives = 1; // (a demo is one life: the level begun again isn't in it)
    }
    if o.complete {
        // (all of it: the items run and the secrets run together, with the monsters in)
        o.expert = true;
        o.secrets = true;
        if o.fight == 0 {
            o.fight = 4;
        }
    }
    Ok((o, jobs, rest))
}

/// A loadout as `--carry` gives it: weapons (a bit each, by the game's numbering), clip, shell,
/// cell, rocket, backpack (0/1), health, armor, armor type.
pub(crate) fn parse_carry(v: &str) -> Result<evilboom::dsda::Loadout, String> {
    let n: Vec<i32> = v
        .split(',')
        .map(|x| x.trim().parse().map_err(|_| format!("--carry: {v}")))
        .collect::<Result<_, _>>()?;
    // (a tenth, if given: the weapon in her hands, the game's number for it)
    let (n, ready) = match n.as_slice() {
        [first @ .., r] if first.len() == 9 => (first.to_vec(), Some(*r).filter(|&r| r >= 0)),
        _ => (n, None),
    };
    let [weapons, clip, shell, cell, rocket, backpack, health, armor, armor_type] = n.as_slice()
    else {
        return Err(format!("--carry: nine numbers (or ten), not {v}"));
    };
    Ok(evilboom::dsda::Loadout {
        weapons: *weapons,
        clip: *clip,
        shell: *shell,
        cell: *cell,
        rocket: *rocket,
        backpack: *backpack != 0,
        health: *health,
        armor: *armor,
        armor_type: *armor_type,
        ready,
    })
}

/// A loadout as `--carry` takes it.
pub(crate) fn carry_arg(l: &evilboom::dsda::Loadout) -> String {
    let nine = format!(
        "{},{},{},{},{},{},{},{},{}",
        l.weapons,
        l.clip,
        l.shell,
        l.cell,
        l.rocket,
        i32::from(l.backpack),
        l.health,
        l.armor,
        l.armor_type
    );
    l.ready.map_or(nine.clone(), |r| format!("{nine},{r}"))
}

/// The options as they'd be given again (for the child processes).
pub(crate) fn option_args(o: &RunOptions) -> Vec<String> {
    let mut out = vec![
        "--lives".to_string(),
        o.lives.to_string(),
        "--max-seconds".into(),
        o.max_seconds.to_string(),
        "--stall".into(),
        o.stall_seconds.to_string(),
        "--seed".into(),
        o.seed.map_or("random".into(), |s| s.to_string()),
    ];
    if o.stranded > 0 {
        out.extend(["--stranded".to_string(), o.stranded.to_string()]);
    }
    if o.fight > 0 {
        out.push("--fight".into());
        out.push(o.fight.to_string());
    }
    for (on, flag) in [
        (o.peek, "--peek"),
        (o.secrets, "--secrets"),
        (o.mortal, "--mortal"),
        (o.charmed, "--charmed"),
        (o.endless_bullets, "--endless-bullets"),
        (o.keys, "--keys"),
        (o.expert, "--expert"),
        (o.complete, "--complete"),
        (o.stall_out, "--stall-out"),
    ] {
        if on {
            out.push(flag.into());
        }
    }
    if o.decisions {
        out.push("--decisions".into());
    }
    if o.known {
        out.push("--known".into());
    }
    if let Some(b) = &o.bar {
        out.push("--bar".into());
        out.push(b.clone());
    }
    if let Some(a) = &o.advisor {
        out.push("--advisor".into());
        out.push(a.display().to_string());
    }
    if let Some(t) = &o.tuning {
        out.push("--tuning".into());
        out.push(t.display().to_string());
    }
    // (the loadout she starts with: left out, a sweep's or an episode's runs began from a
    // pistol whatever was asked, and said nothing of it)
    if let Some(l) = &o.carry {
        out.push("--carry".into());
        out.push(carry_arg(l));
    }
    out
}
