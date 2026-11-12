#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# package-linux.sh VERSION [OUT]: a Linux release, out/release/evil-boom-VERSION-linux-x86_64.tar.gz
# (or under OUT), from the built program and engine (tools/build-engine.sh, then cargo build
# --release --target-dir ../out/build; BIN=PATH for another build of the program).
#
# The folder: evil-boom, the engine (libdsda-evil.so, dsda-doom.wad), config/, lib/ with the
# engine's libraries (SDL2, SDL2_mixer, libsndfile and its codecs, libzip, ...), found through the
# engine's RUNPATH ($ORIGIN/lib) so the release runs on distributions with other versions of
# them. Left to the system: the C and C++ runtimes and the graphics and X11 stack, which must
# match the machine. Needs patchelf.
set -euo pipefail
cd "$(dirname "$0")/../.."
version=${1:?usage: tools/release/package-linux.sh VERSION [OUT]}
out=${2:-out/release}
name=evil-boom-$version-linux-x86_64
dir=$out/$name
bin=${BIN:-out/build/release/evil-boom}
engine=third_party/dsda-doom/prboom2/build
rm -rf "$dir"
mkdir -p "$dir/lib" "$dir/config"
cp "$bin" "$dir/evil-boom"
cp "$engine/libdsda-evil.so" "$engine/dsda-doom.wad" "$dir/"
for f in config/*.json; do
  case "$f" in *.local.json) ;; *) cp "$f" "$dir/config/" ;; esac
done
# (the system's own: the C and C++ runtimes, the graphics and X11 stack, the sound servers)
system='^(linux-vdso|ld-linux|libc\.|libm\.|libmvec|libdl\.|librt\.|libpthread|libresolv|libgcc_s|libstdc\+\+|libGL|libGLU|libGLX|libOpenGL|libGLdispatch|libEGL|libX|libxcb|libdrm|libwayland|libasound|libpulse|libdbus|libsystemd|libudev)'
ldd "$engine/libdsda-evil.so" | awk '$3 ~ /^\// {print $1, $3}' | while read -r so path; do
  if ! grep -Eq "$system" <<<"$so"; then
    cp -L "$path" "$dir/lib/$so"
  fi
done
patchelf --set-rpath '$ORIGIN/lib' "$dir/libdsda-evil.so"
for f in "$dir"/lib/*; do
  chmod u+w "$f"
  patchelf --set-rpath '$ORIGIN' "$f"
done
cp LICENSE THIRD_PARTY.md README.md "$dir/"
cp tools/release/RELEASE.md "$dir/"
tar -C "$out" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz: $(ls "$dir/lib" | wc -l) libraries bundled"
