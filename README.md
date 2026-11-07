# Evil DOOM

A DOOM integration for Evil Neuro (and Neuro-sama) using the
[Neuro SDK](https://github.com/VedalAI/neuro-sdk). Evil gives the orders; a fast classifier and
a deterministic bot carry them out in real time.

See [docs/DECISIONS.md](docs/DECISIONS.md) for the architecture and decision log.

## Layout

Evil is one Rust crate, `native/` (package `evildoom`: a library and the `evil` binary), running
DSDA-Doom in-process.

```
native/src/
  wad, map, specials, things   WAD and map lumps, line/sector specials, things (engine-independent)
  mesh, nav, plan, movers      the navmesh, movement rules, the planner over moving sectors
  jumps                        drops and running jumps, simulated with the game's physics
  navigator, walkthrough       a level's analysis, views restricted to what she knows, routes
  knowledge                    what Evil has seen (the explorer, the 100%-items run)
  motor                        35 Hz control: following a route
  tactical                     monsters, sound, combat, the player with monsters about
  engine, dsda                 DSDA-Doom in-process (third_party/dsda-doom + dsda/dsda-doom.patch)
  sweep, play, viewer, cli, main  the `evil` command: sweeps, single maps, watching her play
native/tests/                  against the real IWADs and the game (skipped when missing)
dsda/                          the engine patch and its build notes
docs/
```

For the internal module boundaries, tuning locations, and repeatable gameplay/performance
comparison procedure, see [the Rust review notes](docs/RUST_REVIEW.md).

## Setup

The engine: DSDA-Doom with `dsda/dsda-doom.patch` applied, built as the `dsda-evil` shared
library (on Windows with MSYS2 MinGW64):

```sh
export PATH=/c/msys64/mingw64/bin:$PATH
cmake --build third_party/dsda-doom/prboom2/build --target dsda-evil
```

Then Evil:

```sh
cd native
cargo build --release
cargo test --release
```

WADs are not included. By default Evil looks in the Steam install
(`C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom`); override with the
`EVILDOOM_DOOM_DIR` environment variable.

## Running

```sh
evil sweep doom doom2 --jobs 8              # every map, a line each, then a summary
evil sweep doom E1M1 E1M2 doom2 MAP01       # some maps
evil run-map doom2 MAP14                    # one map; its log goes to out/explore
evil watch doom E1M3                        # watch her play, with what she knows beside the game
evil dump-nav doom E1M1                     # the level's analysis
evil sector doom E1M5 99 121                # sectors: heights, lines, things
evil drive doom E2M4 --from out/runs/DOOM_E2M4_expert.run@1418 --script cmds.txt
                                            # take her over at a moment of a recorded run
```

For tracing a run: `run-map ... --snapshot S --focus X,Y,ZOOM` (a picture of the run at S, the
map zoomed on a place), `--from FILE@S` (play a recorded run back to S, then the bot as it is
now), and `EVIL_DEBUG_OBJECTIVES=from-to` (every option she weighed, those tics).

Run options: `--lives N` (unlimited unless given), `--max-seconds S` (all lives together),
`--seed N|random` (0 unless given), `--peek` (she knows the whole map), `--secrets` (out only with
every secret), `--expert` (every item, then out), `--stall S` (the 100% runs: over after S game
seconds with nothing new), `--fight SKILL` (monsters in), `--mortal` (no god mode).
