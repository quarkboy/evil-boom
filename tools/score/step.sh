#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# step.sh NAME [score options...]: score a build on the baseline's very runs, and compare.
#
# Plays the runs of out/score/base.tsv (the same levels, skills and seeds) with the build, into
# out/score/NAME.tsv, then prints the run-for-run comparison (score-report), the levels that
# moved most, and the set's summary beside the baseline's (screen.py). Options after NAME go to
# `score` (a --tuning FILE, say). See docs/TESTING.md.
#
#   BIN=PATH    the build to score (out/build/release/evil-boom unless given; .exe on Windows)
#   JOBS=N      parallel runs (12 unless given)
set -eu
cd "$(dirname "$0")/../.."
name=${1:?usage: tools/score/step.sh NAME [score options...]}
shift
bin=${BIN:-out/build/release/evil-boom}
[ -x "$bin" ] || bin="$bin.exe"
base=out/score/base.tsv
[ -f "$base" ] || { echo "no $base: make a baseline first (docs/TESTING.md)" >&2; exit 1; }
"$bin" score --like "$base" --jobs "${JOBS:-12}" --out "out/score/$name.tsv" "$@" >/dev/null 2>&1
"$bin" score-report "$base" "out/score/$name.tsv" | grep -A2 "run for run" | cut -c1-150
"$bin" score-report "$base" "out/score/$name.tsv" | grep "moved most" | cut -c1-200
"$bin" score-report "out/score/$name.tsv" | tail -1
python3 tools/score/screen.py base "$name" | cut -c1-100
