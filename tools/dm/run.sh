#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# tools/dm/run.sh NAME: the hunting bot against the one before it, in deathmatches of two. Each
# level and seed is played twice, the hunter player 1 and then player 2 (the other as bots were
# before: `--hunt N`), to 20 frags or 10 minutes, no monsters; a line a run into
# out/dm/NAME/, then the frags of each side, run for run (put the date in NAME).
#
# BIN: out/build/release/evil-boom unless given. JOBS: runs at once (8). SEEDS: 1 to 8 unless
# given. MAPS: the levels (WAD:MAP ...). HUNT=none plays both as before, HUNT=all both hunting
# (how often they meet, in frags a minute, with hunting and without).
set -u
cd "$(dirname "$0")/../.."
name=${1:?usage: tools/dm/run.sh NAME}
bin=${BIN:-out/build/release/evil-boom}
jobs=${JOBS:-8}
seeds=${SEEDS:-"1 2 3 4 5 6 7 8"}
maps=${MAPS:-"doom2:MAP01 doom2:MAP07 doom2:MAP11 doom2:MAP16 doom:E1M1"}
dir=out/dm/$name
mkdir -p "$dir"
runs=()
for m in $maps; do
  for s in $seeds; do
    if [ "${HUNT:-}" = none ] || [ "${HUNT:-}" = all ]; then
      runs+=("$m $s $HUNT")
    else
      runs+=("$m $s 1" "$m $s 2")
    fi
  done
done
one() {
  # shellcheck disable=SC2086
  set -- $1
  local wad=${1%%:*} map=${1#*:} seed=$2 hunt=$3 out="$dir/${1#*:}-s$2-h$3.txt"
  [ -s "$out" ] && return
  timeout 1200 "$bin" watch "$wad" "$map" --players 2 --deathmatch --mortal --headless \
    --seed "$seed" --hunt "$hunt" --max-seconds 700 2>&1 | grep -E '^(DEATHMATCH|  player [0-9]( \(hunts\))?: frags)' > "$out.tmp"
  mv "$out.tmp" "$out"
}
export -f one
export bin dir
printf '%s\n' "${runs[@]}" | xargs -P "$jobs" -I{} bash -c 'one "$@"' _ {}
python3 - "$dir" <<'PY'
import glob, math, re, sys
d = sys.argv[1]
rows = []
for f in sorted(glob.glob(d + "/*.txt")):
    t = open(f).read()
    m = re.search(r"DEATHMATCH (\S+) (\S+) (\d+)s game", t)
    p = re.findall(r"player (\d)( \(hunts\))?: frags (-?\d+)", t)
    if not m or len(p) < 2:
        print("incomplete:", f); continue
    secs = int(m.group(3))
    hunt = [x for x in p if x[1]]; old = [x for x in p if not x[1]]
    rows.append((f.split("/")[-1], secs, [int(x[2]) for x in hunt], [int(x[2]) for x in old]))
if not rows:
    sys.exit()
mins = sum(r[1] for r in rows) / 60
allfr = sum(sum(r[2]) + sum(r[3]) for r in rows)
print(f"{len(rows)} matches, {mins:.0f} min of play, {allfr} frags: {allfr / max(mins, 1e-9):.2f} a minute")
paired = [(r[2][0], r[3][0]) for r in rows if r[2] and r[3]]
if paired:
    diffs = [h - o for h, o in paired]
    n = len(diffs); mean = sum(diffs) / n
    se = math.sqrt(sum((x - mean) ** 2 for x in diffs) / (n - 1) / n) if n > 1 else 0
    wins = sum(1 for h, o in paired if h > o); losses = sum(1 for h, o in paired if h < o)
    print(f"hunter against the bot before: frags {sum(h for h, _ in paired)} to {sum(o for _, o in paired)}; "
          f"a match {mean:+.2f} (s.e. {se:.2f}); won {wins}, lost {losses}, drawn {n - wins - losses}")
    by = {}
    for r in rows:
        if r[2] and r[3]:
            k = r[0].split("-")[0]; a = by.setdefault(k, [0, 0]); a[0] += r[2][0]; a[1] += r[3][0]
    print("by level (hunter-before): " + ", ".join(f"{k} {v[0]}-{v[1]}" for k, v in sorted(by.items())))
PY
