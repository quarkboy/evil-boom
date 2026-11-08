# Notes for Claude sessions on this repo

Evil DOOM: a bot that plays DOOM / DOOM II for Evil Neuro, live on stream (Neuro SDK
integration later). Evil is all Rust (`native/`, the `evil` binary) running DSDA-Doom
in-process (`dsda/`: the patch and `evil_api.c`; the DSDA source itself is fetched into
`third_party/`, not committed). Decisions and their reasons: `docs/DECISIONS.md` (keep it
updated). How to trace a failed run: `docs/DEBUGGING.md` (read it before debugging).

## How the user wants the work done

- **Human, not TAS.** Recoveries, missed lift windows, wasted time are fine if she finishes.
  Fix failures, not inefficiency.
- **Trace, then fix.** Take a concrete failure (a death, a stuck) back through the logs to the
  decision that caused it. Work out the game mechanics involved (check vanilla/DSDA source, the
  map's lines with `evil sector`), the narrowest rule that covers it, and everything else that
  rule touches. No first-idea heuristics; no guessed fixes. Say what was considered.
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
- **Builds:** the user runs `out/build/release/evil.exe` (`evil` on Linux), often with the viewer
  open. After a change: `cargo build --release --target-dir ../out/build` from `native/`. On
  Windows the running exe can't be overwritten: rename it aside first (`evil.old.<time>.exe`).
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
  `evil episode doom E1M1 --fight 2 --mortal --seeds 8` (and E2M1, E3M1), and
  `evil episode doom2 MAP01 --fight 2 --mortal --seeds 16`; for combat changes also
  `evil arena arenas/combat.txt --seeds 8` and `arenas/doom2.txt`. Compare with the numbers
  in the last commits' messages; within a few tenths of a level is seed noise.
- Sweeps (when asked): 8 parallel jobs; one sweep type at a time, expert first; the next only
  with no per-level regressions so far.

## Where things stand (2026-11-08)

Last bot change e3627ef. Doom 1 on HNTR, one life, 8 seeds: E1 3.6 levels (ends at E1M5),
E2 2.8 with 2 of 8 through (E2M2 stuck), E3 4.0 (all die in E3M5, crushers). Doom II 7.9
(MAP12 stuck 7/16, MAP08 deaths: the Cyberdemon/Baron room). Open items: berserk isn't observed
(no fist tactics, out-of-ammo retreat ignores it); deliberate infighting only for heavies; several
missile-firing monsters can still keep her dodging.

## Next: the Linux port (WSL Fedora 45, VS Code over WSL)

The user is moving to a new laptop and wants this done there. Windows-only bits to fix:

- `native/src/config.rs`: the default WAD folder is the Windows Steam path
  (`EVILDOOM_DOOM_DIR` overrides it).
- `native/src/engine.rs`: `EVIL_MINGW_BIN` defaults to `C:\msys64\mingw64\bin`; the library is
  `libdsda-evil.dll` on Windows (Linux: `libdsda-evil.so`; check the loading in `dsda.rs`, which
  has Windows DLL-directory handling).
- `dsda/README.md`: the DSDA build uses MSYS2's pacman. Fedora: `dnf install` SDL2-devel,
  SDL2_mixer-devel, libsndfile-devel, libzip-devel, zlib-devel, cmake, ninja-build, gcc, g++;
  then clone kraflab/dsda-doom at `eb8eb98` into `third_party/dsda-doom` and `git apply
  dsda/dsda-doom.patch` (the patch adds the `dsda-evil` library target).
- `native/src/viewer.rs` `find_ffmpeg`: winget fallback (Linux: ffmpeg on PATH, `dnf install
  ffmpeg-free` or RPM Fusion's ffmpeg).
- The viewer under WSLg: untested.
- Default `--jobs` follows the CPU count; re-measure the sweet spot on the new machine.
- `.vscode/extensions.json` is already set up for WSL.
