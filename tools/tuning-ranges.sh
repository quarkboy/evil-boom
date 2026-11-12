#!/usr/bin/env bash
# tuning-ranges.sh [PHASE...]: how far each judgement number can move before she plays worse.
#
# One number at a time, each pushed to values well past what any stance or setting uses, every
# value played on the quarter set (out/score/quarter.tsv: 272 runs, ~4.5 min) with the current
# build, compared run for run with the defaults on the same runs. Results in
# out/score/ranges/summary.tsv (phase, number, value, default, score, change, its standard
# error, exits, health lost a minute), one line a value, added to as each one finishes; a
# run's TSV kept as out/score/ranges/<number>=<value>.tsv. Values already in the summary are
# skipped, so a stopped sweep picks up where it was.
#
# Phases (see docs/TUNING_RANGES.md): A the stances' numbers, B avoid's, C pickups and routes,
# D guns and cover. No phase given: all of them, A first.
set -u
cd "$(dirname "$0")/.."
B=out/build/release/evil-boom
D=out/score/ranges
mkdir -p "$D"
S=$D/summary.tsv
[ -f "$S" ] || printf 'phase\tnumber\tvalue\tdefault\tscore\tchange\tse\texits\tlost_per_min\n' > "$S"

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

phases=("$@")
[ ${#phases[@]} -eq 0 ] && phases=(A B C D)

# The defaults on the quarter set, once (the comparison for every value).
if [ ! -f "$D/default.tsv" ]; then
  echo "defaults on the quarter set..."
  $B score --like out/score/quarter.tsv --jobs 12 --out "$D/default.tsv" >/dev/null 2>&1
fi

for phase in "${phases[@]}"; do
  while read -r p number default values; do
    [ "$p" = "$phase" ] || continue
    group=${number%%.*}
    field=${number#*.}
    for v in $values; do
      name="$number=$v"
      if grep -q "^$phase	$number	$v	" "$S"; then
        continue
      fi
      echo "$name..."
      printf '{"%s": {"%s": %s}}\n' "$group" "$field" "$v" > "$D/$name.json"
      $B score --like out/score/quarter.tsv --jobs 12 --tuning "$D/$name.json" --out "$D/$name.tsv" >/dev/null 2>&1
      line=$($B score-report "$D/default.tsv" "$D/$name.tsv" | grep "run for run")
      change=$(echo "$line" | sed -E 's/.*set 1 ([-+0-9.]+), standard error ([0-9.]+).*/\1/')
      se=$(echo "$line" | sed -E 's/.*standard error ([0-9.]+).*/\1/')
      stats=$(python3 - "$D/$name.tsv" <<'PY'
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
)
      IFS=$'\t' read -r score exits lost <<< "$stats"
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$phase" "$number" "$v" "$default" "$score" "$change" "$se" "$exits" "$lost" >> "$S"
      printf '  %s: %s (%s, s.e. %s), %s exits\n' "$name" "$score" "$change" "$se" "$exits"
    done
  done <<< "$PLAN"
done
echo "summary: $S"
