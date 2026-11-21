#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# demos.sh [BINARY]: does a run recorded as a demo play back the same in DSDA-Doom itself?
#
# `run-map --demo FILE` has the game record the run as it records any demo. This records a
# handful of runs that exit their level (with and without `--keys`), plays each back with the
# dsda-doom program built beside the engine library (the game's own main loop, with nothing of
# Evil's in the play: -timedemo), and compares what the game's own tally says (-levelstat: the
# time, kills and items) with what the run reported. A demo that parts from the run it's of
# ends somewhere else, or not at all.
#
# For any change to the engine boundary, to how a tic's command is made, or to how a level is
# begun. The demos are left in out/demos/ to watch: dsda-doom -iwad IWAD -playdemo FILE.
#
# BINARY: out/build/release/evil-boom unless given.
set -u
cd "$(dirname "$0")/.."
root=$PWD
bin=$(realpath "${1:-out/build/release/evil-boom}")
game=$root/third_party/dsda-doom/prboom2/build/dsda-doom
[ -x "$game" ] || { echo "no dsda-doom at $game (tools/build-engine.sh builds the library only: cmake --build third_party/dsda-doom/prboom2/build --target dsda-doom)" >&2; exit 2; }
out=$root/out/demos
mkdir -p "$out/home"
# wad map skill seed [options]: runs that exit on one life
runs=(
  "doom E1M1 3 1" "doom E1M1 3 1 --keys" "doom E1M5 3 3" "doom E2M1 3 5"
  "doom E1M3 4 2 --keys" "doom E2M2 3 6 --keys" "doom2 MAP02 3 16" "doom2 MAP02 3 16 --keys"
  "doom2 MAP01 4 15 --keys"
)
one() {
  # shellcheck disable=SC2086
  set -- $1
  local wad=$1 map=$2 skill=$3 seed=$4
  shift 4
  local name=$map-s$skill-$seed${1:+-keys} run iwad said secs kills items play back
  rm -f "$out/$name.lmp"
  run=$(EVILBOOM_ROOT=$root "$bin" run-map "$wad" "$map" --fight "$skill" --mortal --seed "$seed" "$@" --demo "$out/$name" 2>/dev/null)
  iwad=$(sed -nE 's/^DEMO .* -iwad (.*) -playdemo .*/\1/p' <<< "$run")
  said=$(grep -a '^RESULT ' <<< "$run")
  if [ ! -s "$out/$name.lmp" ] || [ -z "$iwad" ] || ! grep -q '^RESULT EXITED' <<< "$said"; then
    echo "$name: FAILED (no demo, or the run didn't exit: ${said:0:60})"
    return
  fi
  kills=$(sed -E 's/.* kills ([0-9]+\/[0-9]+),.*/\1/' <<< "$said")
  items=$(sed -E 's/.* items ([0-9]+\/[0-9]+),.*/\1/' <<< "$said")
  secs=$(sed -E 's/.* ([0-9]+)s game.*/\1/' <<< "$said")
  play=$out/play.$name
  mkdir -p "$play" && rm -f "$play/levelstat.txt"
  (cd "$play" && HOME=$out/home XDG_DATA_HOME=$out/home SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy \
    DOOMWADDIR=$(dirname "$game") timeout 600 "$game" -iwad "$iwad" -timedemo "$out/$name.lmp" \
    -nodraw -nosound -nomusic -levelstat > play.txt 2>&1)
  back=$(cat "$play/levelstat.txt" 2>/dev/null)
  # (the game's tally: "E1M1 - 0:59.66 (0:59)  K: 6/6  I: 2/37  S: 1/3")
  local bk bi bs
  bk=$(sed -nE 's/.* K: ([0-9]+\/[0-9]+) .*/\1/p' <<< "$back")
  bi=$(sed -nE 's/.* I: ([0-9]+\/[0-9]+) .*/\1/p' <<< "$back")
  bs=$(sed -nE 's/.* - ([0-9]+):([0-9]+)\..*/\1 \2/p' <<< "$back" | awk '{print $1 * 60 + $2}')
  if [ "$bk" = "$kills" ] && [ "$bi" = "$items" ] && [ -n "$bs" ] && [ $((secs - bs)) -ge -1 ] && [ $((secs - bs)) -le 1 ]; then
    echo "$name: same ($back)"
  else
    echo "$name: DIFFERS (the run: kills $kills, items $items, ${secs}s; played back: ${back:-no level finished})"
  fi
}
export -f one
export bin root game out
res=$(printf '%s\n' "${runs[@]}" | xargs -P "${JOBS:-6}" -I{} bash -c 'one "$@"' _ {} | sort)
echo "$res"
bad=$(grep -vc ': same ' <<< "$res")
if [ "$bad" -ne 0 ]; then
  echo "$bad of ${#runs[@]} demos don't play back as the run they're of"
  exit 1
fi
echo "all ${#runs[@]} demos play back in dsda-doom as the runs they're of (out/demos/)"
