#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# one.sh NAME BIN WAD MAP SKILL SEED: one run of the three-lives test (run.sh), its row to
# out/lives/NAME.rows/. Each run has a folder of its own to write in, removed after. OPTS: more
# options for the run (a mode under test, as `OPTS=--keys`).
set -u
name=$1; bin=$2; wad=$3; map=$4; skill=$5; seed=$6
top=$(cd "$(dirname "$0")/../.." && pwd)
work=$top/out/lives/work/$name/$map.$skill.$seed
row=$top/out/lives/$name.rows/$map.$skill.$seed
[ -s "$row" ] && exit 0
mkdir -p "$work" "$(dirname "$row")"
ln -sfn "$top/third_party" "$work/third_party"
# shellcheck disable=SC2086
EVILBOOM_ROOT=$work timeout 1800 "$bin" run-map "$wad" "$map" --fight "$skill" --seed "$seed" \
  --mortal --lives "${LIVES:-3}" ${OPTS:-} > "$work/stdout.txt" 2>&1
line=$(grep -a "^RESULT" "$work/stdout.txt" | tail -1)
[ -n "$line" ] || line="RESULT ERROR"
printf '%s\t%s\t%s\t%s\t%s\n' "$wad" "$map" "$skill" "$seed" "$line" > "$row"
rm -rf "$work"
