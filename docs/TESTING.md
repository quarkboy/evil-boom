# Testing Evil BOOM

There are four kinds of check, from seconds to a quarter of an hour:

| check | what it shows | needs | time |
|---|---|---|---|
| format, lint, unit tests | the code is tidy and its pieces work | nothing | seconds |
| integration tests | WAD reading, planning and movement against the real game | IWADs, the engine | a minute |
| parity | a change meant to alter nothing really didn't | IWADs, the engine | half a minute |
| the score | how well she plays, and whether a change made it better or worse | IWADs, the engine | ~17 minutes |

`cargo` commands run from `native/`; everything else from the repository's root, where
`evil-boom` stands for `out/build/release/evil-boom` (`.exe` on Windows; see
[BUILDING.md](BUILDING.md)).

## Format, lint, unit tests

```sh
cargo fmt --check
cargo clippy --release --all-targets --target-dir ../out/clippy -- -D warnings
cargo test --release --lib --target-dir ../out/build
```

All three must be clean before a commit (no warnings, no `#[allow]`s). CI runs them on Linux
and Windows.

## Integration tests

```sh
cargo test --release --target-dir ../out/build
```

`native/tests/` reads the real IWADs, plans levels, checks the jump simulator against the
engine and follows routes in the game. Each test skips itself when the IWADs or the engine
library aren't there, so it passes without them. That's why CI can run it, and also why a pass
there proves little. Run them locally with the game data installed.

## Parity: a change that should change nothing

A refactor, a rename or a comment pass must leave her play exactly as it was. Every run is
deterministic on its seed, so 24 fixed-seed runs compared line for line are proof enough:

```sh
tools/parity.sh save out/build/release/evil-boom   # with the build before the change
# ... make the change, build again (to another folder if the old binary is in use) ...
tools/parity.sh check out/check/release/evil-boom  # "all 24 runs the same", or which differ
```

For a change to comments only, `tools/dev/code_only.py --check` proves it without running
anything: it strips the comments from every changed `.rs` file and compares the code with the
last commit's.

## The score: how well she plays

```sh
evil-boom score --jobs 12 --out out/score/NAME.tsv
```

plays every level of DOOM (E1-E4) and DOOM II, from a pistol start, on one life, on Hurt Me
Plenty and Ultra-Violence, with 8 random seeds each: 1,088 runs, about 17 minutes at 12 jobs.
A run scores 1 for reaching the exit, otherwise half its share of the kills; the score is the
mean. `score-report FILE` gives the full report: by skill, the levels beaten every time, each
level's spread.

Two sets on different seeds differ by about ±0.01 from luck alone, so changes are compared on
**the same runs** instead:

```sh
# once: a baseline, with the build you start from
evil-boom score --jobs 12 --out out/score/base.tsv
# for each change: the baseline's very runs (its levels, skills and seeds), compared run for run
BIN=out/check/release/evil-boom tools/score/step.sh NAME
```

`step.sh` plays the baseline's runs with the new build, then prints the difference run for run
with its standard error, how many runs came out the same to the tic, the levels that moved most,
and both sets' summaries. A change that touches nothing scores exactly 0.0000; one that touches a
few runs moves only those. A change is kept if it's no worse within the noise (about two standard
errors). Its set then becomes the new baseline (copy it to `out/score/base.tsv`).

`out/score/NAME.stats.jsonl` keeps each run's counts beside it (what hurt her and whether she
knew of it, turns, alerts). `tools/score/screen.py base NAME...` summarises them a line a set.

Runs are deterministic per build, but a different machine can still diverge on floating-point
details. Make your own baseline rather than comparing with someone else's sets.

### Quicker: the quarter sets

`tools/score/quarter.tsv` and `quarter-b.tsv` are two fixed sets of 272 runs each (two seeds a
level-skill), about 4.5 minutes each. Use them to screen ideas and tuning before a full score:

```sh
evil-boom score --like tools/score/quarter.tsv --jobs 12 --out out/score/q-NAME.tsv
```

Two quarter sets that agree are better evidence than one. Values that look good on one alone
often don't hold.

### Tuning files

Every judgement number in the fight is in `native/src/tactical/tuning.rs`. `evil-boom tuning`
prints them all as JSON; edit a copy and give it to any run with `--tuning FILE`, a score
included (`step.sh NAME --tuning FILE`). `tools/score/tuning-ranges.sh` sweeps each number
across a range on both quarter sets.

## One level at a time

For a movement or exploration fix, play the level alone, on several seeds, with a good loadout so
the fight doesn't confound it:

```sh
evil-boom run-map doom2 MAP14 --seed 3 --carry 287,200,50,0,20,1,100,100,1
```

Each run writes its log to `out/explore/<IWAD>_<MAP>.txt` (kept as well under a stamped name),
which [DEBUGGING.md](DEBUGGING.md) explains how to read. For a fight, use `--fight SKILL
--mortal --lives 1` and several seeds: the fight is where randomness matters.

## Arenas

`arenas/*.txt` set up fights on real maps: a place, monsters, a loadout, a time limit.

```sh
evil-boom arena arenas/combat.txt --jobs 12 --seeds 8    # every scenario, 8 seeds each
evil-boom arena-watch arenas/combat.txt NAME              # one, watched
```

They're for developing a fight idea. Judge the idea by the score: arenas are too artificial to
decide by.

## The Neuro integration

Testing Evil's control layer without Neuro (a mock server, a language model stand-in, Gary) is
in [NEURO.md](NEURO.md).

## Held out

Don't tune on TNT: Evilution or The Plutonia Experiment. They're kept back as unseen maps, to
check that what she learned on DOOM and DOOM II holds elsewhere.
