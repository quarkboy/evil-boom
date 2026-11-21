#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# Build the engine: DSDA-Doom at the pinned commit, dsda/dsda-doom.patch applied, as the
# dsda-evil shared library (and dsda-doom.wad beside it) in
# third_party/dsda-doom/prboom2/build/.
#
# Linux: run from any shell (the packages: see dsda/README.md).
# Windows: run from MSYS2's MINGW64 shell (the packages: see dsda/README.md).
#
#   tools/build-engine.sh            fetch if missing, patch if not yet, configure, build
#   tools/build-engine.sh --clean    the same from a fresh checkout and build folder
#
# After changing the engine's source in third_party: write the patch again first
# (cd third_party/dsda-doom && git diff --abbrev=7 > ../../dsda/dsda-doom.patch), or this
# takes the tree for unpatched, fails to apply the old patch over it and builds nothing.
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
  # (the patch is on: on the pinned commit, or it's some other DSDA-Doom it happens to fit)
  head=$(git rev-parse HEAD)
  if [[ $head != "$COMMIT"* ]]; then
    echo "third_party/dsda-doom is at ${head:0:7}, not the pinned $COMMIT: run with --clean" >&2
    exit 1
  fi
  echo "patch already applied"
elif [[ -n $(git status --porcelain --untracked-files=no) ]]; then
  # (changed, and not by this patch: an older patch, or edits not written back to it. A
  # checkout over them would fail or lose them: nothing's touched)
  echo "third_party/dsda-doom has changes that aren't dsda/dsda-doom.patch (an older patch, or" >&2
  echo "edits not yet written back to it: see this script's header). Write the patch again; or" >&2
  echo "drop the changes (git -C third_party/dsda-doom checkout -- .) and run this again; or run" >&2
  echo "with --clean for a fresh checkout and a whole build." >&2
  exit 1
else
  git checkout -q "$COMMIT"
  git apply "$PATCH"
fi

cmake_args=(-S prboom2 -B prboom2/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DENABLE_PACKAGING=OFF
  -DWITH_FLUIDSYNTH=ON -DWITH_IMAGE=OFF -DWITH_MAD=OFF -DWITH_PORTMIDI=OFF
  -DWITH_VORBISFILE=ON -DWITH_XMP=OFF)
if [[ $platform == linux ]]; then
  # (DSDA prefers package configs; Fedora's zlib-ng one names a libz.a that isn't installed)
  cmake_args+=(-DCMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF)
fi
cmake "${cmake_args[@]}"
cmake --build prboom2/build --target dsda-evil

ls -l "prboom2/build/$lib" prboom2/build/dsda-doom.wad
