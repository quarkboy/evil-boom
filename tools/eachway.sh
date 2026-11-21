#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# eachway.sh [BINARY]: does a run come out the same whichever way it's played?
#
# The program plays a level by more than one loop: `run-map` (the sweep's, what every test
# measures) and the watcher's (`watch`, here with no window: what's streamed and directed).
# On one life they play the same tics, so the same seed must end the same way: the outcome,
# the kills, the items, and the game time to within the second each rounds it by. tools/parity.sh
# plays only the first; this plays each of the same 24 runs both ways and says which differ.
# For any change to either loop, or to what they share (native/src/run.rs).
#
# With lives to spend the two differ on purpose and aren't compared: the sweep begins the level
# again when she's stranded, the watcher only when she dies.
#
# Six pairs at a time unless JOBS says: a search that runs out of its time gives up sooner on a
# loaded machine, and a run that turns on one comes out differently for that alone (MAP21 here
# did, at twelve).
#
# BINARY: out/build/release/evil-boom unless given.
set -u
cd "$(dirname "$0")/.."
root=$PWD
bin=$(realpath "${1:-out/build/release/evil-boom}")
work=$root/out/eachway/work.$$
mkdir -p "$work"
carry="--carry 287,200,50,0,20,1,100,100,1"
runs=(
  "doom E1M1 3 1" "doom E1M3 4 2" "doom E1M5 3 3" "doom E1M7 4 4"
  "doom E2M1 3 5" "doom E2M2 3 6" "doom E2M4 3 7 $carry" "doom E2M6 4 8"
  "doom E3M3 3 9" "doom E3M5 4 10" "doom E3M7 3 11 $carry" "doom E4M1 3 12"
  "doom E4M5 4 13 $carry" "doom E4M7 3 14" "doom2 MAP01 4 15" "doom2 MAP02 3 16"
  "doom2 MAP06 3 17" "doom2 MAP07 3 18" "doom2 MAP09 3 19 $carry" "doom2 MAP12 4 20"
  "doom2 MAP19 3 21 $carry" "doom2 MAP21 1 22" "doom2 MAP23 1 23" "doom2 MAP24 3 24 $carry"
)
# "OUTCOME kills a/b items c/d secs" of a result line (the sweep's or the watcher's)
told() {
  sed -E 's/^(RESULT |[A-Z0-9]+: )//; s/^([A-Z]+) kills ([0-9]+\/[0-9]+), items ([0-9]+\/[0-9]+).* ([0-9]+)s game.*/\1 kills \2 items \3 \4/'
}
one() {
  # shellcheck disable=SC2086
  set -- $1
  local wad=$1 map=$2 skill=$3 seed=$4
  shift 4
  local opts="--fight $skill --mortal --lives 1 --seed $seed --max-seconds 900"
  local a b dir=$work/$map.$seed
  # shellcheck disable=SC2086
  a=$(EVILBOOM_ROOT=$root "$bin" run-map "$wad" "$map" $opts "$@" 2>/dev/null | grep -a '^RESULT ' | told)
  # (the watcher keeps its log in its root's out/watch: a root a run, side by side)
  mkdir -p "$dir" && ln -sfn "$root/third_party" "$dir/third_party"
  # shellcheck disable=SC2086
  b=$(cd "$dir" && SDL_VIDEODRIVER=dummy EVILBOOM_ROOT=$dir "$bin" watch "$wad" "$map" $opts --headless "$@" 2>/dev/null | grep -a "^$map: [A-Z]" | tail -1 | told)
  local as=${a##* } bs=${b##* }
  if [ -z "$a" ] || [ -z "$b" ]; then
    echo "$wad $map skill $skill seed $seed: FAILED (run-map: ${a:-nothing}; watch: ${b:-nothing})"
  elif [ "${a% *}" = "${b% *}" ] && [ $((as - bs)) -ge -1 ] && [ $((as - bs)) -le 1 ]; then
    echo "$wad $map skill $skill seed $seed: same"
  else
    echo "$wad $map skill $skill seed $seed: DIFFER (run-map: $a; watch: $b)"
  fi
}
export -f one told
export bin root work
out=$(printf '%s\n' "${runs[@]}" | xargs -P "${JOBS:-6}" -I{} bash -c 'one "$@"' _ {} | sort)
find "$work" -mindepth 1 -delete 2>/dev/null
rmdir "$work" 2>/dev/null
bad=$(grep -vc ': same$' <<< "$out")
if [ "$(wc -l <<< "$out")" -ne "${#runs[@]}" ] || [ "$bad" -ne 0 ]; then
  grep -v ': same$' <<< "$out" | sed 's/^/  /'
  echo "$bad of ${#runs[@]} runs come out differently by run-map and by the watcher"
  exit 1
fi
echo "all ${#runs[@]} runs the same by run-map and by the watcher"
