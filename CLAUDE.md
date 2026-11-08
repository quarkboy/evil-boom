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
- **Barrels:** shotgun within ~320 with 12+ shells, else single pistol taps (first shot is
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

- **No full sweeps without asking.** Up to 8 chosen levels (or seeds) per test is fine.
- For a movement/exploration fix: that level alone, a good loadout so combat doesn't confound it
  (`--carry 287,200,50,0,20,1,100,100,1`; format in docs/DEBUGGING.md), several seeds, one at a
  time (each run-map overwrites `out/explore/<IWAD>_<MAP>.txt`). Then an episode or pistol-start
  check.
- Regression set used lately (HNTR = `--fight 2`, `--mortal`, one life):
  `evil-boom episode doom E1M1 --fight 2 --mortal --seeds 8` (and E2M1, E3M1), and
  `evil-boom episode doom2 MAP01 --fight 2 --mortal --seeds 16`; for combat changes also
  `evil-boom arena arenas/combat.txt --seeds 8` and `arenas/doom2.txt`. Compare with the numbers
  in the last commits' messages; within a few tenths of a level is seed noise.
- **Parity checks** (a refactor meant to change nothing): one run of one episode's levels
  (e.g. `episode doom E1M1 --seeds 1` with the regression flags) compared line for line with
  the old build's; runs are deterministic, so that's enough. Not the whole suite (the user's).
- Sweeps (when asked): 12 parallel jobs (`--jobs 12` on this laptop; episodes and arenas too);
  one sweep type at a time, expert first; the next only
  with no per-level regressions so far.

## Where the fight's code is

`tactical/combat/tree.rs` is the fight, a node a decision (see its header); `combat/arsenal.rs`
which weapon; `combat/style.rs` how to fight with it; retreats are `Retreat`/`RetreatKind` in
`tactical/player.rs`. New behaviour goes in as a node (or a style, a rule in `best_gun`), not as
another case in a chain.

## Where things stand (2026-11-08)

Last bot change: crushers in every phase. HNTR, one life, 8 seeds: E1 4.1 (1/8 through; the rest
end at E1M3-E1M5, E1M5 with her own barrel blasts), E2 4.2 (3/8 through; E2M2 stuck/died), E3 5.6
(three to E3M8), Doom II 8.6 (16 seeds; MAP12 stuck, MAP08 deaths: the Cyberdemon/Baron room);
berserk arena 14/32. Open items: barrels (the user: shoot any barrel she sees, from clear of the
chain's blast; E1M5's deaths are her own barrel blasts); two or more revenants with berserk;
deliberate infighting only for heavies; several missile-firing monsters can still keep her dodging.

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
