#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# tools/coop/run.sh NAME [PLAYERS] [SKILL]: the co-op test. A fixed set of levels and seeds, each
# played headless by PLAYERS bots (4 unless told) on SKILL (3), mortal, the hour's cap; a line a
# run, and the totals, into out/coop/NAME/ (put the date in NAME).
#
# Counted from each run's COOP lines and the players' logs (out/watch/coop/): out by the exit or
# not, game time, deaths, the damage players took from each other (friendly fire), the times one
# was stuck, and presses that did nothing (a switch already used, pressed again).
#
# BIN: out/build/release/evil-boom unless given (a side build while the main one's in use).
# JOBS: runs at once (8 unless told).
set -u
cd "$(dirname "$0")/../.."
name=${1:?usage: tools/coop/run.sh NAME [PLAYERS] [SKILL]}
players=${2:-4}
skill=${3:-3}
bin=${BIN:-out/build/release/evil-boom}
jobs=${JOBS:-8}
dir=out/coop/$name
mkdir -p "$dir"
runs=(
  "doom E1M1 1" "doom E1M3 2" "doom E1M5 3" "doom E2M2 4"
  "doom2 MAP01 5" "doom2 MAP02 6" "doom2 MAP07 7" "doom2 MAP12 8"
)
one() {
  # shellcheck disable=SC2086
  set -- $1
  local wad=$1 map=$2 seed=$3 out="$dir/$2-s$3.txt"
  timeout 3000 "$bin" watch "$wad" "$map" --players "$players" --fight "$skill" --mortal \
    --headless --seed "$seed" --max-seconds 3600 > "$out.raw" 2>&1
  grep -E '^(COOP|  player)' "$out.raw" > "$out"
  rm -f "$out.raw"
  cat out/watch/coop/"$map"-s"$seed"-p*.log > "$dir/$map-s$seed.logs" 2>/dev/null
}
export -f one
export bin dir players skill
printf '%s\n' "${runs[@]}" | xargs -P "$jobs" -I{} bash -c 'one "$@"' _ {}
python3 - "$dir" <<'EOF'
import re, sys, pathlib
d = pathlib.Path(sys.argv[1])
rows, tot = [], dict(exits=0, secs=0.0, deaths=0, ff=0, stuck=0, nothing=0, runs=0)
for f in sorted(d.glob('*.txt')):
    text = f.read_text()
    logs = (d / (f.stem + '.logs'))
    log = logs.read_text() if logs.exists() else ''
    m = re.search(r'^COOP (\S+) (\S+) (\d+)s game', text, re.M)
    if not m:
        rows.append(f'{f.stem:14} FAILED'); continue
    out, secs = m.group(2), int(m.group(3))
    deaths = sum(int(x) for x in re.findall(r'deaths (\d+)', text))
    ff = sum(int(x) for x in re.findall(r'hit for (\d+)(?: \(and \d+ armor\))? by (?:a barrel set off by )?DoomPlayer', log))
    stuck = len(re.findall(r'stuck at', log))
    nothing = len(re.findall(r'did nothing', log))
    rows.append(f'{f.stem:14} {out:8} {secs:5}s  deaths {deaths:3}  friendly fire {ff:5}  stuck {stuck:4}  presses for nothing {nothing:3}')
    tot['runs'] += 1; tot['exits'] += out == 'EXITED'; tot['secs'] += secs
    tot['deaths'] += deaths; tot['ff'] += ff; tot['stuck'] += stuck; tot['nothing'] += nothing
rows.append(f"ALL: exits {tot['exits']}/{tot['runs']}, {tot['secs']:.0f}s game, deaths {tot['deaths']}, "
            f"friendly fire {tot['ff']}, stuck {tot['stuck']}, presses for nothing {tot['nothing']}")
(d / 'summary.txt').write_text('\n'.join(rows) + '\n')
print('\n'.join(rows))
EOF
