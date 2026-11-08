# DSDA-Doom in-process (spike, 2026-11-05)

Evil's future engine: DSDA-Doom's own C code, built as a shared library with `evil_api.c`
added, run a tic at a time from Rust, its state read straight from memory. See
`docs/DECISIONS.md` ("DSDA-Doom spike").

## Build

`tools/build-engine.sh` does it all (Linux, or MSYS2's MINGW64 shell on Windows): clones
kraflab/dsda-doom into `third_party/dsda-doom` if missing (not in git: GPL, ours is the patch),
checks out `eb8eb98`, applies the patch, configures and builds the `dsda-evil` target into
`third_party/dsda-doom/prboom2/build/` (`libdsda-evil.so` or `.dll`, and `dsda-doom.wad`).
`--clean` starts over from a fresh clone. The packages it needs are in the top README's Setup.

By hand, it's:

```sh
git clone https://github.com/kraflab/dsda-doom.git third_party/dsda-doom
cd third_party/dsda-doom && git checkout eb8eb98 && git apply ../../dsda/dsda-doom.patch
cmake -S prboom2 -B prboom2/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DENABLE_PACKAGING=OFF \
  -DWITH_FLUIDSYNTH=OFF -DWITH_IMAGE=OFF -DWITH_MAD=OFF -DWITH_PORTMIDI=OFF \
  -DWITH_VORBISFILE=OFF -DWITH_XMP=OFF \
  -DCMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF      # Linux only
cmake --build prboom2/build --target dsda-evil
```

`CMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF` (Linux): DSDA turns it on, and then finds Fedora's
zlib-ng CMake config first, which names a static `libz.a` that isn't installed (configuring
stops). With it off, CMake's own FindZLIB finds the shared one (SDL2 is still found by its
config, as DSDA asks for it explicitly).

To change the patch: edit in `third_party/dsda-doom`, then from there
`git diff --abbrev=7 > ../../dsda/dsda-doom.patch`.

The patch: a library target (`dsda-evil`) beside the game's, put beside `dsda-doom.wad` (which
it depends on), and one exported wrapper in `d_main.c` (`D_DoomMainSetupForEvil`: the setup
without the main loop).

## Run the spike

```sh
cd dsda/spike && cargo build --release
export SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy
export PATH=/c/msys64/mingw64/bin:$PATH                     # Windows (MSYS2) only
cd ../../third_party/dsda-doom/prboom2/build
../../../../dsda/spike/target/release/dsda-spike ./libdsda-evil.so "<path to DOOM.WAD>" 20000
# Windows: dsda-spike.exe ./libdsda-evil.dll
```

(Run from the build directory: the game looks for `dsda-doom.wad` beside it.)
