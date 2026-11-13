#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# package-windows.sh VERSION [OUT]: a Windows release, out/release/evil-boom-VERSION-windows-x86_64.zip
# (or under OUT), from the built program and engine. Run in MSYS2's MINGW64 shell, after
# tools/build-engine.sh and the Rust build (BIN=PATH for the program; out/build/release/evil-boom.exe
# unless given).
#
# The folder: evil-boom.exe, the engine (libdsda-evil.dll, dsda-doom.wad), the MinGW-w64 DLLs the
# engine needs (found beside it when it's loaded), each one's package licenses in licenses/,
# config/. Needs zip (pacman -S zip).
set -euo pipefail
cd "$(dirname "$0")/../.."
version=${1:?usage: tools/release/package-windows.sh VERSION [OUT]}
out=${2:-out/release}
name=evil-boom-$version-windows-x86_64
dir=$out/$name
bin=${BIN:-out/build/release/evil-boom.exe}
engine=third_party/dsda-doom/prboom2/build
rm -rf "$dir"
mkdir -p "$dir/config" "$dir/licenses"
# (the folder for a SoundFont of the player's own, and how to get one: none is included)
mkdir -p "$dir/soundfonts"
cp soundfonts/README.md "$dir/soundfonts/"
cp "$bin" "$dir/evil-boom.exe"
cp "$engine/libdsda-evil.dll" "$engine/dsda-doom.wad" "$dir/"
for f in config/*.json; do
  case "$f" in *.local.json) ;; *) cp "$f" "$dir/config/" ;; esac
done
# (every DLL from MinGW-w64 the engine loads, directly or not; Windows' own stay Windows')
ldd "$engine/libdsda-evil.dll" | awk '$3 ~ /^\/mingw64\// {print $3}' | sort -u | while read -r dll; do
  cp "$dll" "$dir/"
  pkg=$(pacman -Qqo "$dll")
  if [ -d "/mingw64/share/licenses/$pkg" ] && [ ! -d "$dir/licenses/$pkg" ]; then
    cp -r "/mingw64/share/licenses/$pkg" "$dir/licenses/$pkg"
  fi
done
cp LICENSE THIRD_PARTY.md README.md "$dir/"
cp tools/release/RELEASE.md "$dir/"
(cd "$out" && rm -f "$name.zip" && zip -qr "$name.zip" "$name")
echo "$out/$name.zip: $(ls "$dir"/*.dll | wc -l) DLLs"
