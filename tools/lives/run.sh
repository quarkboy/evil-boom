#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# run.sh NAME [BASE]: the three-lives test of a build, and (with BASE) the comparison.
#
# Plays every level of DOOM and DOOM II from a pistol start with three lives, on Hurt Me Plenty
# and Ultra-Violence, on the seeds of tools/lives/seeds.txt (four): 544 runs, about nine
# minutes at 14 jobs. A row a run goes to out/lives/NAME.rows/ (a run already there isn't
# played again: the same build and options go on where they left off; any other is refused,
# by the stamp kept in out/lives/NAME.stamp). Name runs with the date (1122-thing): the
# folder is full of old ones. Then report.py, beside BASE's rows if given. See
# docs/TESTING.md.
#
#   BIN=PATH     the build to test (out/build/release/evil-boom unless given; .exe on Windows)
#   JOBS=N       runs at a time (12 unless given)
#   SEEDS="..."  other seeds than the file's
#   LIVES=N      other than three lives
#   OPTS="..."   more options for every run (a mode under test: OPTS=--keys)
set -eu
cd "$(dirname "$0")/../.."
name=${1:?usage: tools/lives/run.sh NAME [BASE]}
bin=${BIN:-out/build/release/evil-boom}
[ -x "$bin" ] || bin="$bin.exe"
[ -x "$bin" ] || { echo "no build at $bin (BIN=PATH)" >&2; exit 1; }
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")
# The rows on file under NAME are played no more; so they must be this build's, with these
# options, or the result is another run's. What NAME was played with is kept beside its rows
# (out/lives/NAME.stamp: the build's hash, the lives, the seeds, the options), and a NAME
# with rows of anything else, or of nothing recorded, is refused.
stamp="$(sha256sum "$bin" | cut -c1-16) lives=${LIVES:-3} seeds=$(echo ${SEEDS:-file} | tr ' ' ,) opts=${OPTS:-}"
if [ -n "$(ls "out/lives/$name.rows" 2>/dev/null | head -1)" ]; then
  was=$(cat "out/lives/$name.stamp" 2>/dev/null || echo "nothing recorded")
  if [ "$was" != "$stamp" ]; then
    echo "out/lives/$name.rows holds runs of another build or other options:" >&2
    echo "  on file: $was" >&2
    echo "  now:     $stamp" >&2
    echo "another NAME (with the date in it), or remove that folder" >&2
    exit 1
  fi
fi
mkdir -p out/lives
echo "$stamp" > "out/lives/$name.stamp"
seeds=${SEEDS:-$(grep -v '^#' tools/lives/seeds.txt)}
{
  for e in 1 2 3 4; do for m in 1 2 3 4 5 6 7 8 9; do echo "doom E${e}M${m}"; done; done
  for m in $(seq -w 1 32); do echo "doom2 MAP$m"; done
} | while read -r wad map; do
  for skill in 3 4; do for seed in $seeds; do echo "$name $bin $wad $map $skill $seed"; done; done
done | LIVES=${LIVES:-3} OPTS=${OPTS:-} xargs -P "${JOBS:-12}" -L 1 tools/lives/one.sh
errors=$(grep -l "RESULT ERROR" "out/lives/$name.rows"/* 2>/dev/null | wc -l)
[ "$errors" = 0 ] || echo "NOTE: $errors runs did not run (their rows say ERROR)" >&2
if [ $# -ge 2 ]; then python3 tools/lives/report.py "$2" "$name"; else python3 tools/lives/report.py "$name"; fi
