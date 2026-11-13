# Building Evil BOOM

Two parts, built separately:

1. **The engine:** [DSDA-Doom](https://github.com/kraflab/dsda-doom) at a pinned commit, with
   `dsda/dsda-doom.patch` applied and `dsda/evil_api.c` compiled in, built as a shared library
   (`libdsda-evil.so` on Linux, `libdsda-evil.dll` on Windows) with its resource file
   `dsda-doom.wad` beside it. `tools/build-engine.sh` does all of it.
2. **Evil:** the Rust crate in `native/`, which builds the `evil-boom` program. It loads the
   engine library at run time, so the two can be rebuilt independently.

You also need the game's data: your own **DOOM** and/or **DOOM II** IWADs (see
[WADs](#wads) below). None are included.

## Linux

Fedora:

```sh
sudo dnf install gcc gcc-c++ cmake ninja-build git SDL2-devel SDL2_mixer-devel \
  libsndfile-devel libzip-devel zlib-ng-compat-devel fluidsynth-devel libvorbis-devel
```

Debian / Ubuntu:

```sh
sudo apt install build-essential cmake ninja-build git pkg-config libsdl2-dev \
  libsdl2-mixer-dev libsndfile1-dev libzip-dev zlib1g-dev libgl-dev libglu1-mesa-dev \
  libfluidsynth-dev libvorbis-dev
```

Rust, if you don't have it: <https://rustup.rs> (`curl --proto '=https' --tlsv1.2 -sSf
https://sh.rustup.rs | sh`).

Then, from the repository's root:

```sh
tools/build-engine.sh                                    # -> third_party/dsda-doom/prboom2/build/
cd native && cargo build --release --target-dir ../out/build   # -> out/build/release/evil-boom
```

The watch window (`evil-boom watch`) is the engine's own SDL window: native on X11, Wayland and
Windows, kept to the screen's refresh. The score's small progress window (`score-watch`) uses X11
(XWayland on Wayland desktops). `evil-boom record` needs `ffmpeg` on the path.

**WSL:** works as on Linux. Keep the WADs inside the Linux file system (reading them through
`/mnt/c` is slow). The console page is reachable from the LAN only with WSL's mirrored networking
or a port proxy on Windows.

## Windows

The engine is built with MSYS2's MinGW-w64 toolchain; Evil with ordinary Rust (rustup).

1. Install [MSYS2](https://www.msys2.org/). In its **MINGW64** shell (not MSYS, UCRT64 or
   CLANG64):

   ```sh
   pacman -S --needed git mingw-w64-x86_64-{SDL2,SDL2_mixer,libsndfile,libzip,zlib,cmake,ninja,gcc,fluidsynth,libvorbis}
   cd /c/path/to/evil-boom
   tools/build-engine.sh                  # -> third_party\dsda-doom\prboom2\build\libdsda-evil.dll
   ```

2. Install Rust with [rustup](https://rustup.rs) (the default MSVC toolchain is fine; it needs
   the Visual Studio Build Tools, which rustup offers to install). From any shell:

   ```sh
   cd native
   cargo build --release --target-dir ../out/build     # -> out\build\release\evil-boom.exe
   ```

At run time Evil finds the engine's own DLLs (SDL2 and the rest) in `C:\msys64\mingw64\bin`, or
in the folder `EVIL_MINGW_BIN` names.

A running `evil-boom.exe` can't be overwritten: close it (and any console, sweep or score using
it) before rebuilding, or rename it aside first.

## The engine by hand

What `tools/build-engine.sh` does (it also takes `--clean`, to start from a fresh checkout):

```sh
git clone https://github.com/kraflab/dsda-doom.git third_party/dsda-doom
cd third_party/dsda-doom && git checkout eb8eb98 && git apply ../../dsda/dsda-doom.patch
cmake -S prboom2 -B prboom2/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DENABLE_PACKAGING=OFF \
  -DWITH_FLUIDSYNTH=ON -DWITH_IMAGE=OFF -DWITH_MAD=OFF -DWITH_PORTMIDI=OFF \
  -DWITH_VORBISFILE=ON -DWITH_XMP=OFF \
  -DCMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF      # Linux only
cmake --build prboom2/build --target dsda-evil
```

`CMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF` (Linux): DSDA-Doom turns it on, and then on Fedora finds
zlib-ng's CMake config first, which names a static `libz.a` that isn't installed. With it off,
CMake's own FindZLIB finds the shared library.

The patch adds a library target (`dsda-evil`) beside the game's, placed next to `dsda-doom.wad`,
and an exported setup function without the game's main loop. `dsda/evil_api.c` is the rest of
the boundary: running a tic, reading the game's state, sending controls.

To change the patch: edit in `third_party/dsda-doom`, then from there
`git diff --abbrev=7 > ../../dsda/dsda-doom.patch`. Write the patch out before running the build
script again: it takes a modified tree for an unpatched one and fails to apply the patch over it.

## Music (optional)

Two things make the music sound like the original, both from your own copies:

- **The recordings** from the DOOM + DOOM II re-release on Steam (the original soundtrack
  recorded on a Roland SC-55, and Andrew Hulshult's IDKFA), played with `--music sc55` or the
  console's Music setting. Evil reads them from the re-release's `rerelease/extras.wad`, found
  where Steam installed it: on Windows `C:\Program Files (x86)\Steam\steamapps\common\Ultimate
  Doom\rerelease\`; on Linux `~/.local/share/Steam/steamapps/common/Ultimate Doom/rerelease/`;
  under WSL, Windows' Steam folder through `/mnt/c`. Installed elsewhere (another Steam library,
  say), copy `extras.wad` into a `rerelease/` folder beside the WADs' `base/` (on Linux
  `~/.local/share/evilboom/rerelease/extras.wad`), or set `EVILBOOM_RERELEASE_DIR` to the folder
  holding it. The first game with them makes a music WAD of the IWAD's songs in `out/music/`
  (about a second); nothing of the re-release goes anywhere else.
- **An SC-55 SoundFont** for every song the recordings don't have (a PWAD's own, TNT's own): see
  [soundfonts/README.md](../soundfonts/README.md) (search for "Patch93's SC-55 SoundFont" and
  save it as `soundfonts/SC-55.sf2`; it's then the default).

Without either, the music plays through the OPL synth, as a Sound Blaster played it. The engine
plays SoundFonts through FluidSynth and the recordings through Ogg Vorbis, both built in by
`tools/build-engine.sh`.

## Build options

- `cargo build --release --no-default-features`: no watch window (headless runs, sweeps and the
  score still work).
- Release builds keep incremental compilation and 16 codegen units (a change rebuilds in about
  12 s instead of 110 s; the bot runs under 1% slower).
- Build into `out/` (`--target-dir ../out/build`): the tools and docs assume
  `out/build/release/evil-boom`. Any other folder works; give the scripts the binary
  (`BIN=...`).

## WADs

Evil looks for the IWADs in Steam's layout (`base/DOOM.WAD`, `base/doom2/DOOM2.WAD`, also
`base/tnt/TNT.WAD` and `base/plutonia/PLUTONIA.WAD`):

- Linux: `~/.local/share/evilboom/`. Copy Steam's `base/` folder there.
- Windows: Steam's install folder, `C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom`.
- Anywhere: set `EVILBOOM_DOOM_DIR` to the folder holding `base/`.

Commands take an IWAD by short name (`doom`, `doom2`, `tnt`, `plutonia`) or by path. PWADs go on
top with `--pwad FILE` (see [USAGE.md](USAGE.md)).

## Environment variables

| variable | what it does |
|---|---|
| `EVILBOOM_DOOM_DIR` | the folder holding the WADs (`base/...`) |
| `EVILBOOM_ROOT` | the project folder (logs go to its `out/`, the engine is found under its `third_party/`); the source checkout unless set |
| `EVIL_MINGW_BIN` | Windows: where the engine's DLLs are (`C:\msys64\mingw64\bin` unless set) |
| `EVILBOOM_PWADS` | PWADs over the IWAD (set by `--pwad`; passed on to child runs) |
| `EVIL_DEBUG_*` | tracing switches: see [DEBUGGING.md](DEBUGGING.md) |
| `OPENAI_API_KEY`, `JEV_API_KEY` | keys for the GPT stand-in and the advisor (or their `config/*.local.json`) |

## Making a release

Pushing a tag `vX.Y.Z` runs `.github/workflows/release.yml`. It builds the Linux and Windows
packages, and the engine's source archive that the GPL asks a binary release to offer, and
publishes them as a GitHub release. Started by hand (Actions, *Release*, a version), it makes a
draft instead.

The packaging scripts also run locally, after building the engine and Evil:

- `tools/release/package-linux.sh VERSION` makes a folder and `.tar.gz` with the program, the
  engine and the engine's libraries in `lib/` (SDL2, SDL2_mixer, libsndfile, libzip, ...), found
  through the engine's RUNPATH. It needs `patchelf`. The C and C++ runtimes and the graphics stack
  stay the system's. Build on an older distribution (CI uses Ubuntu 22.04) for the widest reach.
- `tools/release/package-windows.sh VERSION` (in MSYS2's MINGW64 shell) makes a `.zip` with
  the program, the engine, the MinGW-w64 DLLs it loads, and their licenses.
- `tools/release/engine-source.sh VERSION` archives the patched engine source.

A release runs from wherever it's unpacked: when the engine library is beside the program,
that folder is its root (`config/`, and `out/` for its logs).

## Troubleshooting

- **"loading ... libdsda-evil.so: cannot open"**: the engine isn't built, or Evil was built
  somewhere else and can't find the checkout (set `EVILBOOM_ROOT`).
- **CMake stops on zlib (Fedora)**: see `CMAKE_FIND_PACKAGE_PREFER_CONFIG` above.
- **The build script says "run this from MSYS2's MINGW64 shell"**: the engine must be built with
  the MINGW64 toolchain, whose DLLs Evil loads.
- **The patch doesn't apply**: `tools/build-engine.sh --clean` starts from a fresh checkout.
