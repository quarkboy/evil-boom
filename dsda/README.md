# The engine boundary

Evil runs [DSDA-Doom](https://github.com/kraflab/dsda-doom) in-process: the game's own C code,
built as a shared library (`dsda-evil`), stepped a tic at a time from Rust, its state read
straight from memory.

- `dsda-doom.patch`: our changes to DSDA-Doom at commit `eb8eb98` (v0.30.1). It adds a library
  target beside the game's (placed next to `dsda-doom.wad`, which the game loads) and an exported
  setup without the main loop. It also adds the hooks the bot needs: sounds as they start, hits
  on the player, and the game's screens.
- `evil_api.c`: compiled into the library; the boundary the Rust side calls
  (`native/src/dsda.rs`). It covers running a tic with given controls, reading the player, the
  things, sectors and lines, and the game's sounds and screens (the title card, the menu, the
  tally, the finales, the melt). It also does testing aids: a fixed seed, god mode, powers,
  warping.

DSDA-Doom's source isn't in this repository; `tools/build-engine.sh` fetches it at the pinned
commit, applies the patch and builds the library. How, on Linux and Windows, and how to change
the patch: [docs/BUILDING.md](../docs/BUILDING.md). Why this engine:
[docs/design/decisions/02-engine.md](../docs/design/decisions/02-engine.md).

`dsda-doom.patch` and `evil_api.c` are GPL-3.0-or-later; DSDA-Doom is GPL-2.0-or-later, used under
version 3 ([THIRD_PARTY.md](../THIRD_PARTY.md)).
