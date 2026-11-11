//! Parsing run options and forwarding them to sweep subprocesses.

use evilboom::sweep::RunOptions;

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
            "--lives" => o.lives = value("--lives")?.parse().map_err(|_| "--lives: a number")?,
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
            "--complete" => o.complete = true,
            "--stall-out" => o.stall_out = true,
            "--decisions" => o.decisions = true,
            "--known" => o.known = true,
            "--bar" => o.bar = Some(value("--bar")?),
            "--advisor" => o.advisor = Some(std::path::PathBuf::from(value("--advisor")?)),
            "--tuning" => o.tuning = Some(std::path::PathBuf::from(value("--tuning")?)),
            "--carry" => o.carry = Some(parse_carry(&value("--carry")?)?),
            _ => rest.push(a.clone()),
        }
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
    if o.fight > 0 {
        out.push("--fight".into());
        out.push(o.fight.to_string());
    }
    for (on, flag) in [
        (o.peek, "--peek"),
        (o.secrets, "--secrets"),
        (o.mortal, "--mortal"),
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
    out
}
