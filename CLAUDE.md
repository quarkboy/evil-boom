# Notes for AI coding assistants

Evil BOOM is a Doom bot for an AI streamer to direct live, in Rust (`native/`, the `evil-boom`
program), running DSDA-Doom in-process (`dsda/`: our patch and `evil_api.c`; DSDA-Doom's
source is fetched into `third_party/`, not committed).

Read these first. They hold for you as for any contributor:

- [CONTRIBUTING.md](CONTRIBUTING.md): how she should play and how changes are judged (trace,
  then fix; general rules, never level-specific; measured by the score).
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the map of the code, and where a change goes.
- [docs/DEBUGGING.md](docs/DEBUGGING.md): how to trace a failed run. Read it before debugging.
- [docs/TESTING.md](docs/TESTING.md): the checks, the score, parity, run-for-run comparisons.
- [docs/design/](docs/design/README.md): why things are as they are, what was tried and
  dropped. Check there before proposing something that may have been tried.

## Working here

- **Trace before you fix, and say what you considered.** A fix comes with the failure it was
  traced from, the mechanics checked (engine source, `evil-boom sector`), the rule, and what
  else the rule touches. Don't make guessed fixes.
- **Look it up** (the Doom Wiki, secret lists) before calling anything unreachable or broken. The
  Doom Wiki and Fandom refuse automated fetches: ask the maintainer to paste the page.
- **Measure every change to play** with `tools/score/step.sh` on the baseline's runs, and give
  the before and after numbers in the commit message. Ask before running other full sweeps;
  up to 8 chosen levels or seeds per test is fine. Size `--jobs` to the machine.
- **A change meant to change nothing** gets `tools/parity.sh` (or `tools/dev/code_only.py
  --check` for comments) before it's committed.
- **Don't rebuild a binary in use.** Sweeps, episodes, the score and the console start the
  program afresh for each map, so a rebuild under them mixes two builds in one result. Build
  side builds into another folder (`--target-dir ../out/check`) and give it to the scripts
  (`BIN=...`). On Windows a running `.exe` can't be overwritten at all: rename it aside first.
- **Commit to `main`** when a change is tested and no worse, one change per commit, the message
  saying what failed, why, the rule, and the numbers. `cargo fmt` first; clippy clean.
- **Keep the design notes current:** a decision worth keeping goes in `docs/design/` (the
  decision log's latest chapter, or the plan it belongs to).
- **Everything Evil reads is terse** and in the second person. Her context is precious.
- **Realism:** the fight and the exploring go by what she has seen, heard and learned. Don't add
  an omniscient shortcut without saying so and measuring what it buys.
- **Held out:** don't test or tune on TNT or Plutonia.
- **No subagents or worktree helpers** without the maintainer's OK.

## Where the fight's code is

`tactical/combat/tree.rs` is the fight, a node a decision (see its header).
`combat/arsenal.rs` picks the weapon; `combat/style.rs` says how to fight with it. Retreats are
`Retreat`/`RetreatKind` in `tactical/player.rs`. New behaviour goes in as a node, a style or a
rule in `best_gun`, not as another case in a chain.

A judgement number goes in `tactical/tuning.rs` (read as `self.tune.<group>.<field>`). How
things stand this tic is `Combat::now` (`combat/assess.rs`). What the rest of her does about a
fight decision goes by its `Move` (`combat/moves.rs`), never by its log label.
