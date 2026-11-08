#!/usr/bin/env bash
# Build the engine: DSDA-Doom at the pinned commit, dsda/dsda-doom.patch applied, as the
# dsda-evil shared library (and dsda-doom.wad beside it) in
# third_party/dsda-doom/prboom2/build/.
#
# Linux: run from any shell (the packages: see dsda/README.md).
# Windows: run from MSYS2's MINGW64 shell (the packages: see dsda/README.md).
#
#   tools/build-engine.sh            fetch if missing, patch if not yet, configure, build
#   tools/build-engine.sh --clean    the same from a fresh checkout and build folder
set -euo pipefail

COMMIT=eb8eb98
REPO=https://github.com/kraflab/dsda-doom.git
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC=$ROOT/third_party/dsda-doom
PATCH=$ROOT/dsda/dsda-doom.patch

case "$(uname -s)" in
  Linux*) platform=linux; lib=libdsda-evil.so ;;
  MINGW64*) platform=windows; lib=libdsda-evil.dll ;;
  MSYS*|MINGW32*|UCRT64*|CLANG64*)
    echo "run this from MSYS2's MINGW64 shell (the libraries Evil loads are mingw64's)" >&2; exit 1 ;;
  *) echo "unsupported platform: $(uname -s)" >&2; exit 1 ;;
esac

if [[ ${1:-} == --clean ]]; then
  rm -rf "$SRC"
fi

if [[ ! -d $SRC/.git ]]; then
  git clone "$REPO" "$SRC"
fi
cd "$SRC"
if git apply --reverse --check "$PATCH" 2>/dev/null; then
  echo "patch already applied"
else
  git checkout -q "$COMMIT"
  git apply "$PATCH"
fi

cmake_args=(-S prboom2 -B prboom2/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DENABLE_PACKAGING=OFF
  -DWITH_FLUIDSYNTH=OFF -DWITH_IMAGE=OFF -DWITH_MAD=OFF -DWITH_PORTMIDI=OFF
  -DWITH_VORBISFILE=OFF -DWITH_XMP=OFF)
if [[ $platform == linux ]]; then
  # (DSDA prefers package configs; Fedora's zlib-ng one names a libz.a that isn't installed)
  cmake_args+=(-DCMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF)
fi
cmake "${cmake_args[@]}"
cmake --build prboom2/build --target dsda-evil

ls -l "prboom2/build/$lib" prboom2/build/dsda-doom.wad
