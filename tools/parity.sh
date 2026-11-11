#!/usr/bin/env bash
# parity.sh save|check [BINARY]: do the same runs come out the same?
#
# A set of fixed-seed runs (a run is the same every time on its seed), each one's outcome and
# counts (its RESULT, CARRY, DAMAGE and STATS lines, the real time taken left out) written a
# line a run. `save` keeps them in out/parity/golden.txt; `check` plays them again and says
# which differ. For a change meant to alter nothing she does (a refactor, formatting): save
# with the build before it, check with the build after.
#
# BINARY: out/build/release/evil-boom unless given (out/check/release/evil-boom for a side
# build while the main one's in use).
set -u
cd "$(dirname "$0")/.."
mode=${1:-check}
bin=${2:-out/build/release/evil-boom}
dir=out/parity
mkdir -p "$dir"
carry="--carry 287,200,50,0,20,1,100,100,1"
# wad map skill seed [more options]: the fight on HMP and UV from a pistol start, levels with
# lifts, crushers, teleporters, jumps, barrels and floors that hurt, and some with a loadout
runs=(
  "doom E1M1 3 1" "doom E1M3 4 2" "doom E1M5 3 3" "doom E1M7 4 4"
  "doom E2M1 3 5" "doom E2M2 3 6" "doom E2M4 3 7 $carry" "doom E2M6 4 8"
  "doom E3M3 3 9" "doom E3M5 4 10" "doom E3M7 3 11 $carry" "doom E4M1 3 12"
  "doom E4M5 4 13 $carry" "doom E4M7 3 14" "doom2 MAP01 4 15" "doom2 MAP02 3 16"
  "doom2 MAP06 3 17" "doom2 MAP07 3 18" "doom2 MAP09 3 19 $carry" "doom2 MAP12 4 20"
  "doom2 MAP19 3 21 $carry" "doom2 MAP21 1 22" "doom2 MAP23 1 23" "doom2 MAP24 3 24 $carry"
)
one() {
  # shellcheck disable=SC2086
  set -- $1
  local wad=$1 map=$2 skill=$3 seed=$4
  shift 4
  local out
  out=$("$bin" run-map "$wad" "$map" --fight "$skill" --mortal --lives 1 --seed "$seed" --max-seconds 900 "$@" 2>/dev/null \
    | grep -E '^(RESULT|CARRY|DAMAGE|STATS) ' | sed -E 's/, [0-9]+s real//' | md5sum | cut -c1-12)
  echo "$wad $map skill $skill seed $seed $*: $out"
}
export -f one
export bin
now="$dir/now.$$.txt"
printf '%s\n' "${runs[@]}" | xargs -P 12 -I{} bash -c 'one "$@"' _ {} | sort > "$now"
if [ "$mode" = save ]; then
  mv "$now" "$dir/golden.txt"
  echo "saved $(wc -l < "$dir/golden.txt") runs from $bin"
else
  if diff -q "$dir/golden.txt" "$now" > /dev/null; then
    echo "all $(wc -l < "$now") runs the same"
    rm -f "$now"
  else
    echo "runs that differ:"
    diff "$dir/golden.txt" "$now" | grep '^[<>]' | sed 's/^</  was /; s/^>/  now /'
    rm -f "$now"
    exit 1
  fi
fi
