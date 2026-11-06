# DSDA-Doom in-process (spike, 2026-11-05)

Evil's future engine: DSDA-Doom's own C code, built as a shared library with `evil_api.c`
added, run a tic at a time from Rust, its state read straight from memory. See
`docs/DECISIONS.md` ("DSDA-Doom spike").

## Build (Windows, MSYS2 mingw64)

```sh
pacman -S --needed mingw-w64-x86_64-{SDL2,SDL2_mixer,libsndfile,libzip,zlib,cmake,ninja,gcc}
git clone https://github.com/kraflab/dsda-doom.git third_party/dsda-doom   # (not in git: GPL, ours is the patch)
cd third_party/dsda-doom && git apply ../../dsda/dsda-doom.patch && cd prboom2
export PATH=/c/msys64/mingw64/bin:$PATH
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release -DENABLE_PACKAGING=OFF \
  -DWITH_FLUIDSYNTH=OFF -DWITH_IMAGE=OFF -DWITH_MAD=OFF -DWITH_PORTMIDI=OFF \
  -DWITH_VORBISFILE=OFF -DWITH_XMP=OFF
cmake --build build --target dsda-evil      # -> build/libdsda-evil.dll
```

The patch: a library target (`dsda-evil`) beside the game's, and one exported wrapper in
`d_main.c` (`D_DoomMainSetupForEvil`: the setup without the main loop).

## Run the spike

```sh
cd dsda/spike && cargo build --release
export PATH=/c/msys64/mingw64/bin:$PATH SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy
cd ../../third_party/dsda-doom/prboom2/build
../../../../dsda/spike/target/release/dsda-spike.exe ./libdsda-evil.dll "<path to DOOM.WAD>" 20000
```

(Run from the build directory: the game looks for `dsda-doom.wad` beside it.)
