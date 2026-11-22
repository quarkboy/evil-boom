# Testing Evil BOOM

There are four kinds of check, from seconds to ten minutes:

| check | what it shows | needs | time |
|---|---|---|---|
| format, lint, unit tests | the code is tidy and its pieces work | nothing | seconds |
| integration tests | WAD reading, planning and movement against the real game | IWADs, the engine | a minute |
| parity | a change meant to alter nothing really didn't | IWADs, the engine | half a minute |
| the three-lives test | how well she plays, and whether a change made it better or worse | IWADs, the engine | ~9 minutes |

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

What CI does run without the game data: the unit tests, the WAD reader's on broken bytes, and
`tests/synthetic.rs`, which plans a level made in memory (`evilboom::test_support`: a WAD's
bytes from lumps, a one-room level with the walls it's given). A new test that needs a level
and not a particular one goes there.

## Parity: a change that should change nothing

A refactor, a rename or a comment pass must leave her play exactly as it was. Every run is
deterministic on its seed, so 24 fixed-seed runs compared line for line are proof enough:

```sh
tools/parity.sh save out/build/release/evil-boom   # with the build before the change
# ... make the change, build again (to another folder if the old binary is in use) ...
tools/parity.sh check out/check/release/evil-boom  # "all 24 runs the same", or which differ
```

A run that fails, or gives no result, is `FAILED`: then nothing is saved and nothing compared,
and the script says which (exit 2). A baseline is only ever 24 runs that ran.

For a change to comments only, `tools/dev/code_only.py --check` proves it without running
anything: it strips the comments from every changed `.rs` file and compares the code with the
last commit's.

`tools/eachway.sh` plays the same 24 runs twice over, by `run-map` and by the watcher with no
window, on one life, and says which come out differently: the two loops play the same tics, so
none should. For a change to either loop or to what they share (`run.rs`). `tools/parity.sh`
plays only the first.

`tools/demos.sh` records nine runs as demos (`run-map --demo`), plays each back with the
`dsda-doom` program itself, and compares the game's own tally with what the run reported: a
run of hers is a demo anyone's DSDA-Doom plays, and stays one. For a change to the engine
boundary, to how a tic's command is made, or to how a level is begun.

`tools/dev/cycles.py` says whether any two top-level modules have come to depend on each
other (none do; CI runs it): a module moved or a `use` added goes through it.

## The three-lives test: how well she plays

```sh
BIN=out/check/release/evil-boom tools/lives/run.sh NAME
```

plays every level of DOOM (E1-E4) and DOOM II from a pistol start with **three lives** (dead,
the level again knowing what she learned), on Hurt Me Plenty and Ultra-Violence, on the four
seeds of `tools/lives/seeds.txt`: 544 runs, about nine minutes at 14 jobs (`JOBS=N`). A row a
run goes to `out/lives/NAME.rows/`; a run already there isn't played again, so the script keeps what NAME was played with (the build's hash, the lives, seeds and options, in `out/lives/NAME.stamp`) and refuses a NAME holding anything else. Put the date in the name (`1122-thing`). Then the report
(`tools/lives/report.py NAME...`), a line a set:

| measure | what it is |
|---|---|
| exit | she got out within her lives |
| progress | 1 for an exit, else the share of the level's sectors she saw |
| seen | that share, exit or not |
| damage | the health she took off the monsters over all they had, every life together (as the program counts it: a run's first twenty game minutes) |
| deaths | lives lost |
| secs | game seconds, every life together |
| stood | the run went its whole hour: stranded on her last life, or going round |

No one number is the verdict. Exits and progress say how far she gets; damage says how the
fights went where she didn't get out; time is travel, and travel is risk; a run that stood
out the hour is the worst of outcomes and is listed by name.

Changes are compared on **the same runs**:

```sh
# once: a baseline, with the build you start from
BIN=out/build/release/evil-boom tools/lives/run.sh base
# for each change: the same runs with its build, then each measure run for run beside the baseline
BIN=out/check/release/evil-boom tools/lives/run.sh NAME base
```

The comparison gives each measure's difference run for run with its standard error, the exits
by skill, the levels whose exits moved, and the runs that stood out the hour. A change that
touches nothing reproduces the baseline's rows exactly. A change is kept if it's no worse
within the noise (about two standard errors) and adds no run that stands out the hour; its
set then becomes the baseline (move its folder to `out/lives/base.rows`).

Runs are deterministic per build, but a different machine can still diverge on floating-point
details. Make your own baseline rather than comparing with someone else's sets. And on a
quiet machine: under load the clock's backstops fire differently and runs change.

Other seeds (`SEEDS="1 2 3"`) or lives (`LIVES=1`) make another test, not this one: say so
with the numbers.

## Retired: the one-death score

Until 2026-11-20 a change was judged by `evil-boom score` (and `tools/score/step.sh`): every
level on one life, 8 seeds a skill, 1,088 runs; 1 for an exit, else half the share of kills.
It was given up because one death ended a run: three knife-edge levels (E3M7, E4M7, E4M8)
decided it whatever was changed, and a change that made her play sooner or better elsewhere
read as a loss. Numbers in commit messages and the design notes before that date are its. The
program and the scripts are kept, for reading those and for the tuning sweeps; nothing new is
judged by them. What follows is how it worked.

### The score

```sh
evil-boom score --jobs 12 --out out/score/NAME.tsv
```

plays every level of DOOM (E1-E4) and DOOM II, from a pistol start, on one life, on Hurt Me
Plenty and Ultra-Violence, with 8 random seeds each: 1,088 runs, about 17 minutes at 12 jobs.
A run scores 1 for reaching the exit, otherwise half its share of the kills; the score is the
mean. One life is one death: stranded (nothing left to do and no way on, a trap she couldn't
know of), she begins the level again knowing what she learned, up to three times a run
(`--stranded N`; the hour is for all of it), as she would on stream within the minute. A loop
is still a stall: 300 s with nothing new ends the run. `score-report FILE` gives the full report: by skill, the levels beaten every time, each
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
with its standard error, how many runs came out with the same outcome, seconds and kills (not
"the same run": for that the recordings are compared), the levels that moved most, and both
sets' summaries. A change that touches nothing scores exactly 0.0000; one that touches a few
runs moves only those. A change is kept if it's no worse within the noise (about two standard
errors). Its set then becomes the new baseline (copy it to `out/score/base.tsv`).

A run that never ran (a level not there, the program fallen over) is no run she lost: `score`
says which, keeps what the program said in `NAME.errors/`, leaves the run's row as `ERROR` (in
no tally and no pair), and fails; `step.sh` then stops with no comparison. A report over a set
with such rows, lines that aren't runs, a run twice, or runs with no partner in the other set
says so (`NOTE ...`, `NOT A WHOLE PAIRING`).

**The fight score** is printed beside it (`fight score`, and `fight score, run for run`): how she
fared in each run's first twenty minutes, by more than out or not. Out within them is 1; else the
health the monsters lost (to whatever: her, each other, a crusher) less what she and her armor
took, as a share of all the health the level's monsters had, three quarters of it for dying in
that time, and never less than nothing. A run that lasts longer counts as it stood at twenty minutes.
It moves for a fight better or worse fought where the score, mostly exits, only moves when a run
tips over: the finer measure for changes to the fight and to how she moves in one. The score
stays what a change is kept by. (Sets scored before 2026-11-14 don't have its numbers.)

`out/score/NAME.stats.jsonl` keeps each run's counts beside it (what hurt her and whether she
knew of it, turns, alerts). `tools/score/screen.py base NAME...` summarises them a line a set.

Runs are deterministic per build, but a different machine can still diverge on floating-point
details. Make your own baseline rather than comparing with someone else's sets.

### The quarter sets

`tools/score/quarter.tsv` and `quarter-b.tsv` are two fixed sets of 272 runs each (two seeds a
level-skill), about 4.5 minutes each. Use them to screen ideas and tuning before a full score:

```sh
evil-boom score --like tools/score/quarter.tsv --jobs 12 --out out/score/q-NAME.tsv
```

Two quarter sets that agree are better evidence than one. Values that look good on one alone
often don't hold.

### Tuning files (still as they were)

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

They're for developing a fight idea. Judge the idea by the three-lives test: arenas are too
artificial to decide by.

## The Neuro integration

Testing Evil's control layer without Neuro (a mock server, a language model stand-in, Gary) is
in [NEURO.md](NEURO.md).

## Held out

Don't tune on TNT: Evilution or The Plutonia Experiment. They're kept back as unseen maps, to
check that what she learned on DOOM and DOOM II holds elsewhere.
