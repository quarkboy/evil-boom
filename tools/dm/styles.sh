#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# tools/dm/styles.sh NAME hunt|fight A B: one deathmatch style against another (tactical/styles.rs),
# head to head in deathmatches of two, each level and seed played with A as player 1 and then
# as player 2, both hunting, to 20 frags or 10 minutes, no monsters; A's frags less B's, run for
# run, into out/dm/NAME/ (put the date in NAME). BIN, JOBS, SEEDS, MAPS as tools/dm/run.sh.
set -u
cd "$(dirname "$0")/../.."
name=${1:?usage: tools/dm/styles.sh NAME hunt|fight A B}
axis=${2:?hunt or fight}; a=${3:?style A}; b=${4:?style B}
bin=${BIN:-out/build/release/evil-boom}
jobs=${JOBS:-8}
seeds=${SEEDS:-"1 2 3 4"}
maps=${MAPS:-"doom2:MAP01 doom2:MAP07 doom2:MAP11 doom2:MAP16 doom:E1M1"}
dir=out/dm/$name
mkdir -p "$dir"
for m in $maps; do for s in $seeds; do echo "$m $s $a,$b 1"; echo "$m $s $b,$a 2"; done; done |
  xargs -P "$jobs" -L1 sh -c '
    wad=${0%%:*}; map=${0#*:}; out='"$dir"'/$map-s$1-a$3.txt
    [ -s "$out" ] && exit 0
    timeout 1200 '"$bin"' watch "$wad" "$map" --players 2 --deathmatch --mortal --headless \
      --seed "$1" --'"$axis"'-style "$2" --max-seconds 700 2>&1 | grep -E "^  player [0-9].*: frags" > "$out.tmp"
    mv "$out.tmp" "$out"'
python3 -I - "$dir" "$a" "$b" <<'PY'
import glob, math, re, sys, collections
d, a, b = sys.argv[1], sys.argv[2], sys.argv[3]
diffs, by = [], collections.defaultdict(list)
for f in sorted(glob.glob(d + "/*.txt")):
    seat = int(re.search(r"-a(\d)\.txt$", f).group(1))
    fr = {int(p): int(n) for p, n in re.findall(r"player (\d)[^:]*: frags (-?\d+)", open(f).read())}
    if len(fr) < 2:
        print("incomplete:", f); continue
    x = fr[seat] - fr[3 - seat]
    diffs.append(x); by[f.split("/")[-1].split("-")[0]].append(x)
n = len(diffs)
if n < 2:
    sys.exit()
mu = sum(diffs) / n
se = math.sqrt(sum((x - mu) ** 2 for x in diffs) / (n - 1) / n)
print(f"{a} against {b}, {n} matches: {mu:+.2f} frags a match (s.e. {se:.2f}); won {sum(x > 0 for x in diffs)}, lost {sum(x < 0 for x in diffs)}")
print("  by level: " + ", ".join(f"{k} {sum(v)/len(v):+.1f}" for k, v in sorted(by.items())))
PY
