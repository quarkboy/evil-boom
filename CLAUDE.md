# Notes for Claude sessions on this repo

Evil DOOM: a bot that plays DOOM / DOOM II for Evil Neuro, live on stream (Neuro SDK
integration later). Evil is all Rust (`native/`, the `evil-boom` binary) running DSDA-Doom
in-process (`dsda/`: the patch and `evil_api.c`; the DSDA source itself is fetched into
`third_party/`, not committed). Decisions and their reasons: `docs/DECISIONS.md` (keep it
updated). How to trace a failed run: `docs/DEBUGGING.md` (read it before debugging).

## How the user wants the work done

- **Human, not TAS.** Recoveries, missed lift windows, wasted time are fine if she finishes.
  Fix failures, not inefficiency.
- **Trace, then fix.** Take a concrete failure (a death, a stuck) back through the logs to the
  decision that caused it. Work out the game mechanics involved (check vanilla/DSDA source, the
  map's lines with `evil-boom sector`), the narrowest rule that covers it, and everything else that
  rule touches. No first-idea heuristics; no guessed fixes. Say what was considered.
- **General, never level-specific.** Every behaviour rule comes from the game's mechanics and what
  she can observe, so it holds on any map or WAD: no map names, sector or line numbers, or
  coordinates in a rule. The goal is a robust player bot an AI can later control, not one tuned
  to Doom/Doom II's maps; the failing map is only the test case for the rule.
- **Look it up** (DoomWiki, secret lists) before calling anything unreachable or broken. DoomWiki
  and Fandom refuse automated fetches: ask the user to paste the page, they're happy to.
- **No rules that depend on lives left.** Lives are a test setting; the goal is everything on one
  life. Risk rules must hold on any life.
- **Exploration paradigm:** she plans only from what she has seen (the automap rule), learns
  effects by using/seeing them. Keep the omniscient planner underneath; knowledge filters it.
- **Combat: survivable first, realistic later.** Escape routes, retreats, unsafe zones, mid-fight
  pickups now; awareness limited to sight/sound later. Don't add new omniscient shortcuts casually.
- **Barrels:** any barrel she's seen is cleared (no monster in sight), from out of every blast of
  its chain; shotgun within ~320 with 12+ shells, else single pistol taps (first shot is
  accurate). Walls/switches shot with pistol taps.
- **Rust only**, idiomatic, no `#[allow]`s, no warnings: `cargo clippy --release --all-targets`
  clean. Don't run `cargo fmt` (it reformats ~74 files; the code isn't fmt-formatted).
- **Commit to `main`** when a change is tested and no worse. Commit messages say what failed,
  why, what the rule is, and the before/after numbers.
- **Builds:** Linux (Fedora 45 under WSL, VS Code over WSL) is the main machine now; Windows
  still builds. The binary is `out/build/release/evil-boom` (`evil-boom.exe` on Windows); the
  user often runs it with the viewer open. After a change: `cargo build --release --target-dir
  ../out/build` from `native/`. Engine: `tools/build-engine.sh` (Linux, or MSYS2 MINGW64). On
  Windows the running exe can't be overwritten: rename it aside first
  (`evil-boom.old.<time>.exe`). On Linux don't rebuild while a sweep or episode runs: they
  start the binary again for each map. WADs: `~/.local/share/evildoom/base/...` (Steam layout).
- **Seeds:** real play uses random seeds; a fixed seed (`--seed N`, default 0) is for testing.
  Try several seeds wherever RNG matters (any fight; shot barrels). No-monster, no-barrel runs are
  seed-independent: one run per code version is enough.
- Don't test on TNT or Plutonia: they're held out for real-world testing. Doom and Doom II only.

## Testing

- **The score is the measure of progress** (the user's, 2026-11-09; it replaces the old
  regression set of episodes, a Doom II HNTR sweep and arenas): `evil-boom score` plays all 68
  levels (Doom E1-E4, Doom II) from a pistol start, one life, mortal, on HMP and UV, 8 random seeds
  each (1088 runs, ~17 min at `--jobs 12`; a progress window pops up). A run scores 1 for an exit,
  else half its kill share; the score is the mean over level-skills. Consistency counts too: the
  levels beaten on every run, and each level's spread. `score-report A.tsv B.tsv` compares sets
  (TSVs in `out/score/`). Its own noise: about +-0.008 a measurement (seven sets of near-identical
  builds: 0.428-0.448), so one set against another needs 0.02-0.025 to be real; for smaller
  effects, two sets a build; a single level's points move about +-0.07 by seed alone. Judge a change by it; commit
  messages give the before/after score.
- **Arenas aren't part of the score** (the user's: too artificial). Use them while developing a
  fight idea, not to judge it.
- **No other full sweeps without asking.** Up to 8 chosen levels (or seeds) per test is fine.
- For a movement/exploration fix: that level alone, a good loadout so combat doesn't confound it
  (`--carry 287,200,50,0,20,1,100,100,1`; format in docs/DEBUGGING.md), several seeds, one at a
  time (each run-map overwrites `out/explore/<IWAD>_<MAP>.txt`), then the score.
- **Parity checks** (a refactor meant to change nothing): a handful of `run-map`s on fixed seeds
  compared line for line with the old build's; runs are deterministic, so that's enough.
- 12 parallel jobs (`--jobs 12` on this laptop).

## Where the fight's code is

`tactical/combat/tree.rs` is the fight, a node a decision (see its header); `combat/arsenal.rs`
which weapon; `combat/style.rs` how to fight with it; retreats are `Retreat`/`RetreatKind` in
`tactical/player.rs`. New behaviour goes in as a node (or a style, a rule in `best_gun`), not as
another case in a chain.

## Where things stand (2026-11-09)

Last bot changes: the level campaign (2026-11-09): a 10-minute pass (rounds 1-14) and a
20-minute pass (rounds A-N) of worktree agents over every pistol-start level failing on UV, each
round merged, regression-checked and committed (details per round in DECISIONS.md). Seed-0
single-level exits, campaign start -> now: ITYTD 52 -> 55, HNTR 45 -> 49, HMP 21 -> 29, UV 14 -> 13
of 68. Then the score replaced the regression set, and a stuck round (8 agents on the most-stalled
levels) took stalls from 69 to 45 a measurement.

Score (all 68 levels, HMP+UV, 8 seeds): 0.437 (HMP 0.54, UV 0.33; exits 34%, kills 50%; 18 of 136
level-skills beaten every run, 60 never; 45 stalled runs). A MAP22 HMP run thought 679 s real for
758 game s: the planner, to trace. Paused before the 40-minute pass (the
user's); the larger changes waiting first: the route's speed cut kept under the fight's aim
(`Motion::go`, crawling backwards over lava), ammo running out early on UV, rockets held at packed
crowds, the MAP30 brain (the user: she may know of it), closed door sectors taken as passable
(E4M6).

## The Linux port (done 2026-11-08)

Same results as on Windows (E1 3.6 levels, Doom II 7.9, the same per-seed endings). What it took:
the default WAD folder per platform (`config.rs`), the patch putting the `.so` beside
`dsda-doom.wad` (and building the WAD with it), `CMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF` for
Fedora's zlib-ng, minifb 0.27 -> 0.28 (0.27 segfaulted on X11 with AspectRatioStretch and
corrupted the heap on Wayland). Setup for both platforms: README. minifb is built X11-only (its
Wayland window has no decorations under WSLg); SDL catches SIGTERM, so close the watcher from its
window. The watchers (`watch`, `arena-watch`) play the game's sound by default (`--mute`, M key;
off while paused or not at x1); everything else is silent. Tests use `--jobs 12` (the user's call
for this laptop). Linux baselines (HNTR, one life): E1 3.6, E2 2.8 (2/8 through), E3 3.9, Doom II
7.9; arenas combat 120/128, doom2 227/240.
