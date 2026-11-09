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

- **No full sweeps without asking.** Up to 8 chosen levels (or seeds) per test is fine.
- For a movement/exploration fix: that level alone, a good loadout so combat doesn't confound it
  (`--carry 287,200,50,0,20,1,100,100,1`; format in docs/DEBUGGING.md), several seeds, one at a
  time (each run-map overwrites `out/explore/<IWAD>_<MAP>.txt`). Then an episode or pistol-start
  check.
- Regression set (the user's, 2026-11-08; HNTR = `--fight 2`, `--mortal`, one life):
  `evil-boom episode doom E1M1 --fight 2 --mortal --seeds 8` (and E2M1, E3M1), Doom II as single
  levels from a pistol start, `evil-boom sweep doom2 MAP01 MAP02 MAP03 MAP04 MAP05 MAP08 MAP10
  MAP12 --fight 2 --mortal --lives 1 --seed N` for seeds 0-3 (no gimmick maps: MAP06's crushers,
  MAP07), and `evil-boom arena arenas/combat.txt --seeds 8` and `arenas/doom2.txt`. Compare with the numbers
  in the last commits' messages; within a few tenths of a level is seed noise.
- **Parity checks** (a refactor meant to change nothing): one run of one episode's levels
  (e.g. `episode doom E1M1 --seeds 1` with the regression flags) compared line for line with
  the old build's; runs are deterministic, so that's enough. Not the whole suite (the user's).
- **Staggered sets** (the user's): start each set of the regression run ~20 s after the one
  before rather than waiting for it (the load drops once a set's first seeds finish).
- Sweeps (when asked): 12 parallel jobs (`--jobs 12` on this laptop; episodes and arenas too);
  one sweep type at a time, expert first; the next only
  with no per-level regressions so far.

## Where the fight's code is

`tactical/combat/tree.rs` is the fight, a node a decision (see its header); `combat/arsenal.rs`
which weapon; `combat/style.rs` how to fight with it; retreats are `Retreat`/`RetreatKind` in
`tactical/player.rs`. New behaviour goes in as a node (or a style, a rule in `best_gun`), not as
another case in a chain.

## Where things stand (2026-11-09)

Last bot change: the ammo hold's "in her way" by the route's way, not her facing; before it, 20-minute pass round A (unarmed and shot at, to the ammo before cover; a floor hurts the same each time; a purposeful wade looked round before wading back, a self-shutting door no way out); before it, a shut-in's trigger lines past the risk cap, a rocket's way held through its launch, no ammo spent on what she can't kill; before it, level campaign rounds 1-14 (round 14: unseen walls a way out of a pit, a switch before walls in one, no fist at a killer; round 13: out of ammo to the ammo itself, harmless floors learned unprotected; round 12: unarmed the route keeps the turn, Keens fought; round 11: lining up a jump isn't a jump; round 10: a barrel chain going off kept off out of a fight too; round 9: no coasting off a ledge in a fight, a gun urgent only on a way she can take; round 8: closing in round a corner, SLIME10-12 hurt; round 7: health an errand only near, a cheap way not refused for health; round 6: a jump given up only for a body in the way, switches from a hurting floor at a dead end; round 5: a melee she can't land, a monster in an errand's way) (barrels going off, waits that aren't coming, a hurting floor's own cost; teleporters from the front, ammo walks through monsters, one-hit claws); before it, crushers by kind (slow ones hold her: no-go stopped low, set off from the edge
first; fast ones a damage cost), spent W1 teleporters walkable, the escape from under a crusher
with all of her out, no stepping for a shot in nukage (DECISIONS.md). Regression set (HNTR, one
life): E1 8.0 (8/8 through), E2 7.0 (5/8), E3 7.1 (1/8), Doom II pistol-start levels 27/32;
arenas combat 119/128, doom2 227/240. Sets swing between builds: judge across sets. Open items:
Doom II MAP08, E2M4 seed 5's pit drop (dead_end let it through), E3M9's arrival ambush, E3M6
stucks/deaths (shut in past a WR door-close line), E3M8 deaths, the stepping-for-a-shot flip at
512 off hurting floors, a stopped crusher set off by a walk-over line one sector before its edge
(E3M4: set_off_first only knows lines on the crusher's own edge).

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
