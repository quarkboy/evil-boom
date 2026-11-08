# Evil DOOM

A DOOM integration for Evil Neuro (and Neuro-sama) using the
[Neuro SDK](https://github.com/VedalAI/neuro-sdk). Evil gives the orders; a fast classifier and
a deterministic bot carry them out in real time.

See [docs/DECISIONS.md](docs/DECISIONS.md) for the architecture and decision log.

## Layout

Evil is one Rust crate, `native/` (package `evildoom`: a library and the `evil-boom` binary), running
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
  sweep, play, viewer, cli, main  the `evil-boom` command: sweeps, single maps, watching her play
native/tests/                  against the real IWADs and the game (skipped when missing)
dsda/                          the engine patch and its build notes
docs/
```

For the internal module boundaries, tuning locations, and repeatable gameplay/performance
comparison procedure, see [the Rust review notes](docs/RUST_REVIEW.md).

## Setup

Evil builds and runs on Linux (native or WSL) and on Windows. Two parts: the engine (DSDA-Doom
with `dsda/dsda-doom.patch`, built as the `dsda-evil` shared library) and Evil itself (Rust).

### Linux (Fedora; native or WSL)

```sh
sudo dnf install gcc gcc-c++ cmake ninja-build git SDL2-devel SDL2_mixer-devel \
  libsndfile-devel libzip-devel zlib-ng-compat-devel ffmpeg-free
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh      # Rust, if not yet

tools/build-engine.sh                         # -> third_party/dsda-doom/prboom2/build/libdsda-evil.so
cd native && cargo build --release --target-dir ../out/build        # -> out/build/release/evil-boom
```

(Other distributions: the same libraries' development packages.) The watcher's window works under
X11 and Wayland, WSLg included.

### Windows (MSYS2 MinGW64 for the engine)

In MSYS2's **MINGW64** shell:

```sh
pacman -S --needed git mingw-w64-x86_64-{SDL2,SDL2_mixer,libsndfile,libzip,zlib,cmake,ninja,gcc}
tools/build-engine.sh                         # -> third_party/dsda-doom/prboom2/build/libdsda-evil.dll
```

Then, from any shell with Rust (rustup):

```sh
cd native
cargo build --release --target-dir ../out/build                     # -> out\build\release\evil-boom.exe
```

At run time Evil finds the engine's own DLLs (SDL2 and the rest) in `C:\msys64\mingw64\bin`, or
wherever `EVIL_MINGW_BIN` says. `evil-boom record` wants ffmpeg: on the path, or `winget install
ffmpeg`. A running `evil-boom.exe` can't be overwritten: rename it aside before rebuilding.

### Both

Tests: `cargo test --release --target-dir ../out/build` from `native/`. More on the engine build
(what the patch does, the steps by hand): [dsda/README.md](dsda/README.md).

WADs are not included. Evil looks for Steam's layout (`base/DOOM.WAD`, `base/doom2/DOOM2.WAD`,
`base/tnt/TNT.WAD`, `base/plutonia/PLUTONIA.WAD`):

- Linux: in `~/.local/share/evildoom`. Copy Steam's `base/` folder there (under WSL, from
  `/mnt/c/Program Files (x86)/Steam/steamapps/common/Ultimate Doom/base`: reading WADs through
  `/mnt/c` is slow).
- Windows: in the Steam install, `C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom`.

`EVILDOOM_DOOM_DIR` overrides the folder on either.

## Running

```sh
evil-boom sweep doom doom2 --jobs 8              # every map, a line each, then a summary
evil-boom sweep doom E1M1 E1M2 doom2 MAP01       # some maps
evil-boom run-map doom2 MAP14                    # one map; its log goes to out/explore
evil-boom watch doom E1M3                        # watch her play, with what she knows beside the game
evil-boom dump-nav doom E1M1                     # the level's analysis
evil-boom sector doom E1M5 99 121                # sectors: heights, lines, things
evil-boom drive doom E2M4 --from out/runs/DOOM_E2M4_expert.run@1418 --script cmds.txt
                                            # take her over at a moment of a recorded run
```

See [docs/DEBUGGING.md](docs/DEBUGGING.md) for how to trace a failed run.

For tracing a run: `run-map ... --snapshot S --focus X,Y,ZOOM` (a picture of the run at S, the
map zoomed on a place), `--from FILE@S` (play a recorded run back to S, then the bot as it is
now), and `EVIL_DEBUG_OBJECTIVES=from-to` (every option she weighed, those tics).

Run options: `--lives N` (unlimited unless given), `--max-seconds S` (all lives together),
`--seed N|random` (0 unless given), `--peek` (she knows the whole map), `--secrets` (out only with
every secret), `--expert` (every item, then out), `--stall S` (the 100% runs: over after S game
seconds with nothing new), `--fight SKILL` (monsters in), `--mortal` (no god mode).

## License

Evil DOOM is free software, licensed under the **GNU General Public License, version 3 or (at
your option) any later version** (GPL-3.0-or-later); see [LICENSE](LICENSE). The engine it is
built on and with is GPL-2.0-or-later, which allows its use under version 3:

- **DSDA-Doom** (<https://github.com/kraflab/dsda-doom>, GPL-2.0-or-later; from PrBoom+ and id
  Software's Doom source). It isn't included here: `dsda/README.md` says how to fetch it (we use
  commit `eb8eb98`, v0.30.1). Our changes to it are `dsda/dsda-doom.patch`, and `dsda/evil_api.c`
  is compiled into it; both are ours under GPL-3.0-or-later. DSDA-Doom's own code keeps its
  license.
- The movement simulator (`native/src/jumps/physics.rs`) follows ZDoom 2.8.1's player movement
  code function by function, itself derived from id Software's Doom source (GPL).

If you distribute built binaries (`evil-boom` with `libdsda-evil`), the GPL requires that you also
make the complete corresponding source available: this repository, plus DSDA-Doom at that commit
with the patch applied.

No game data is included. You need your own copy of the DOOM / DOOM II IWADs; WAD files are
never committed (see `.gitignore`). DOOM is a trademark of id Software; this project is not
affiliated with or endorsed by id Software, ZeniMax or Bethesda.
