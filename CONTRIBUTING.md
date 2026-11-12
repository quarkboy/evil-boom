# Contributing to Evil BOOM

Thanks for your interest. Evil BOOM is a Doom bot meant to be directed live by an AI streamer,
so it has to play like a person, hold up on any map, and never surprise the stream. These are
the rules changes are judged by. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the map of the
code; [docs/TESTING.md](docs/TESTING.md) has the commands.

## How she should play

- **Human, not a speedrun.** Recoveries, a missed lift, wasted time are all fine if she finishes.
  Fix failures, not inefficiency.
- **What a player could know.** She plans only from what she has seen (the automap rule) and
  learns what switches and lines do by using them or seeing them work. The fight goes by what
  she has seen and heard, not by the game's state. The omniscient planner stays underneath for
  "how do I get there", filtered through what she knows. Don't add new omniscient shortcuts. A
  little is tolerated where it plays clearly better on stream, and it's documented when it is.
- **No rules that depend on lives left.** Lives are a test setting; the goal is everything on
  one life.

## How changes are made

- **Trace, then fix.** Take a concrete failure (a death, a stuck run) back through the logs to
  the decision that caused it ([docs/DEBUGGING.md](docs/DEBUGGING.md)). Work out the game
  mechanics involved: check the engine's source and the map's lines with `evil-boom sector`.
  Then find the narrowest rule that covers the failure, and check everything else that rule
  touches. Guessed fixes and first-idea heuristics tend to cost more than they save.
- **General, never level-specific.** Every rule comes from the game's mechanics and what she can
  observe, so it holds on any map or WAD. No map names, sector or line numbers, or coordinates
  in a rule. The failing map is only the test case.
- **Look it up** (the Doom Wiki, secret lists) before calling something unreachable or broken.
- **Measured by the score.** A change to how she plays is scored on the baseline's very runs
  (`tools/score/step.sh`) and kept if it's no worse within the noise. A change meant to change
  nothing is checked with `tools/parity.sh`, or `tools/dev/code_only.py --check` for comments.
  See [docs/TESTING.md](docs/TESTING.md).
- **Several seeds** wherever randomness matters: any fight, shot barrels. Runs without monsters
  or barrels don't depend on the seed.
- **TNT and Plutonia are held out.** Develop and tune on DOOM and DOOM II only; the other two
  check that it generalises.

## Code

- **Rust**, idiomatic, rustfmt's defaults. `cargo fmt --check` and
  `cargo clippy --release --all-targets -- -D warnings` must be clean. No `#[allow]`.
- **Where things go:**
  - Fight behaviour is a node in `tactical/combat/tree.rs`, a style, or a rule in `best_gun`,
    not another case in a chain.
  - A number that's a judgement (how near, how long, what it's worth) goes in
    `tactical/tuning.rs`. A fact about the engine stays a `const` where it's used.
  - How things stand this tic is `Combat::now`: rules read it rather than working it out again.
  - What the rest of the bot does about a fight decision goes by its `Move`, never by its log
    label.
- **Comments say why**, in plain sentences: what the rule is for, what game mechanic it rests
  on, what it was measured at if it's a tuning choice.
- **Everything Evil reads is short** and in the second person ("You found a secret"). It's read
  aloud, and a model's context is precious.
- Each source file starts with `SPDX-License-Identifier: GPL-3.0-or-later` and its copyright.

## Commits and pull requests

- One change per commit, tested and no worse.
- The message says what failed, why, what the rule is, and the before and after numbers (the
  score, run for run, with its standard error).
- Design decisions worth keeping go in [docs/design/](docs/design/README.md).

## The engine

The engine is DSDA-Doom at a pinned commit plus our patch (`dsda/dsda-doom.patch`) and
`dsda/evil_api.c`. Keep the patch small. To change it, edit in `third_party/dsda-doom` and write
the patch out again ([docs/BUILDING.md](docs/BUILDING.md)).

## License

By contributing you agree that your contribution is licensed under GPL-3.0-or-later, the
project's license.
