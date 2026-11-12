#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# tuning-ranges.sh [PHASE...]: how far each judgement number can move before she plays worse.
#
# One number at a time, each pushed to values well past what any stance or setting uses, every
# value played on two quarter sets (tools/score/quarter.tsv and quarter-b.tsv: 272 runs each, two
# seeds a level-skill, the second set two others; ~4.5 min each) with the current build,
# compared run for run with the defaults on the same runs. Two, because the defaults' runs on
# one set can be lucky (careful's are, on the first: 0.472 there, 0.447 on the full set's other
# runs), and a value that changes most runs is then a fresh draw that looks worse for nothing:
# a drop on both is a fall-off. Results in out/score/ranges/summary.tsv (the first set) and
# summary.b.tsv (the second): phase, number, value, default, score, change, its standard error,
# exits, health lost a minute; one line a value, added to as each finishes; a run's TSV kept as
# out/score/ranges/<number>=<value>[.b].tsv. Values already in a summary are skipped, so a
# stopped sweep picks up where it was.
#
# Phases (see docs/TUNING_RANGES.md): A the stances' numbers, B avoid's, C pickups and routes,
# D guns and cover. No phase given: all of them, A first.
set -u
cd "$(dirname "$0")/../.."
B=out/build/release/evil-boom
D=out/score/ranges
mkdir -p "$D"
for S in "$D/summary.tsv" "$D/summary.b.tsv"; do
  [ -f "$S" ] || printf 'phase\tnumber\tvalue\tdefault\tscore\tchange\tse\texits\tlost_per_min\n' > "$S"
done
# the two sets: suffix:runs
SETS=(":tools/score/quarter.tsv" ".b:tools/score/quarter-b.tsv")

# phase number default values...
read -r -d '' PLAN <<'EOF'
A engage.stand_threat 9 0 3 6 12 18 30
A engage.low_health 50 0 20 35 70 90
A retreat.health 55 0 25 40 70 85
A guns.outmatched 20 5 10 30 45 100
B engage.keep_clear 1024 512 768 1536 2048
B engage.leave_be 15 4 8 30 60
B errands.danger_near 640 320 1024 1536
C errands.item_range 1536 512 1024 2048 3072
C errands.floor_cost 20 0 10 40 80
C errands.mega_gain 100 1 50 150 200
C errands.soul_gain 50 1 25 75 100
C errands.hurt 50 25 75
C errands.survive_health 80 50 110
D guns.boss_save 10 1 3 30
D guns.short_of_ammo 15 5 30
D retreat.cover_range 768 384 1536
EOF

# A run's score, exits and health lost a minute (tab-separated).
stats() {
  python3 - "$1" <<'PY'
import csv, json, sys
rows = list(csv.DictReader(open(sys.argv[1]), delimiter='\t'))
def pts(r):
    if r['outcome'] == 'EXITED': return 1.0
    k, t = int(r['kills']), int(r['kills_total']); return 0.5 * min(1, k / t) if t else 0.0
score = sum(map(pts, rows)) / len(rows)
exits = sum(r['outcome'] == 'EXITED' for r in rows)
lost = mins = 0.0
try:
    for l in open(sys.argv[1].replace('.tsv', '.stats.jsonl')):
        d = json.loads(l); lost += d['stats'].get('health lost', 0)
    mins = sum(float(r['seconds']) for r in rows) / 60
except OSError:
    pass
print(f"{score:.4f}\t{exits}\t{lost / mins if mins else 0:.1f}")
PY
}

phases=("$@")
[ ${#phases[@]} -eq 0 ] && phases=(A B C D)

# The defaults on each set, once (the comparison for every value).
for set in "${SETS[@]}"; do
  sfx=${set%%:*}
  runs=${set#*:}
  if [ ! -f "$D/default$sfx.tsv" ]; then
    echo "defaults on $runs..."
    $B score --like "$runs" --jobs 12 --out "$D/default$sfx.tsv" >/dev/null 2>&1
  fi
done

for phase in "${phases[@]}"; do
  while read -r p number default values; do
    [ "$p" = "$phase" ] || continue
    group=${number%%.*}
    field=${number#*.}
    for v in $values; do
      name="$number=$v"
      printf '{"%s": {"%s": %s}}\n' "$group" "$field" "$v" > "$D/$name.json"
      changes=""
      for set in "${SETS[@]}"; do
        sfx=${set%%:*}
        runs=${set#*:}
        S="$D/summary$sfx.tsv"
        if line=$(grep "^$phase	$number	$v	" "$S"); then
          changes="$changes $(echo "$line" | cut -f6)"
          continue
        fi
        echo "$name on $runs..."
        $B score --like "$runs" --jobs 12 --tuning "$D/$name.json" --out "$D/$name$sfx.tsv" >/dev/null 2>&1
        line=$($B score-report "$D/default$sfx.tsv" "$D/$name$sfx.tsv" | grep "run for run")
        change=$(echo "$line" | sed -E 's/.*set 1 ([-+0-9.]+), standard error ([0-9.]+).*/\1/')
        se=$(echo "$line" | sed -E 's/.*standard error ([0-9.]+).*/\1/')
        IFS=$'\t' read -r score exits lost <<< "$(stats "$D/$name$sfx.tsv")"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$phase" "$number" "$v" "$default" "$score" "$change" "$se" "$exits" "$lost" >> "$S"
        printf '  %s%s: %s (%s, s.e. %s), %s exits\n' "$name" "$sfx" "$score" "$change" "$se" "$exits"
        changes="$changes $change"
      done
      # (the two sets' changes and their mean: a fall-off is a drop on both)
      echo "  $name: changes$changes, mean $(echo "$changes" | awk '{ printf "%+.4f", ($1 + $2) / 2 }')"
    done
  done <<< "$PLAN"
done
echo "summaries: $D/summary.tsv, $D/summary.b.tsv"
