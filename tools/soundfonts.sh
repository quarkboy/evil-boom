#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# soundfonts.sh [DIR]: the SoundFonts Evil offers for the game's music, fetched into DIR
# (soundfonts/ unless given), each checked against its hash. Free to redistribute: their
# licenses are in soundfonts/licenses/. Ones already there and right are left as they are.
set -euo pipefail
cd "$(dirname "$0")/.."
dir=${1:-soundfonts}
mkdir -p "$dir"
# name, URL, sha256
fonts=(
  "GeneralUser-GS.sf2 https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/684543d5e5efaef08d02be50dcda8d552478fa60/GeneralUser-GS.sf2 9575028c7a1f589f5770fccc8cff2734566af40cd26ed836944e9a5152688cfe"
  "MuseScore_General.sf3 https://ftp.osuosl.org/pub/musescore/soundfont/MuseScore_General/MuseScore_General.sf3 5b85b6c2c61d10b2b91cddd41efcce7b25cd31c8271d511c73afafbef20b6fa3"
)
sum() { if command -v sha256sum > /dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
for f in "${fonts[@]}"; do
  read -r name url hash <<< "$f"
  if [[ -f $dir/$name && $(sum "$dir/$name") == "$hash" ]]; then
    echo "$name: there"
    continue
  fi
  curl -sSfL -o "$dir/$name.part" "$url"
  if [[ $(sum "$dir/$name.part") != "$hash" ]]; then
    rm -f "$dir/$name.part"
    echo "$name: not the file expected (hash differs)" >&2
    exit 1
  fi
  mv "$dir/$name.part" "$dir/$name"
  echo "$name: fetched"
done
