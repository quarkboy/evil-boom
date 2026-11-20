#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""report.py NAME [NAME...]: the three-lives test's sets (out/lives/NAME.rows), each of the
rest beside the first, run for run.

A run's measures:
  exit      1 if she got out within her lives
  progress  1 for an exit, else the share of the level's sectors she saw
  seen      that share, exit or not
  damage    the health she took off the monsters over all they had, every life together
            (as the program counts it: a run's first twenty game minutes)
  deaths    lives lost
  secs      game seconds, every life together
  stood     1 if the run went its whole hour (stranded on her last life, or going round)
See docs/TESTING.md.
"""
import collections
import glob
import math
import os
import re
import sys

TOP = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "out", "lives")
MEASURES = ["exit", "progress", "seen", "damage", "deaths", "secs", "stood"]
PAIRS = {
    "kills": r"kills (\d+)/(\d+)",
    "dealt": r"(\d+)/(\d+) dealt",
    "sectors": r"(\d+)/(\d+) sectors",
}


def share(pair):
    return min(1.0, pair[0] / max(1, pair[1]))


def load(name):
    """A set's runs: (wad, map, skill, seed) -> its measures. ERROR rows are left out."""
    runs = {}
    for path in glob.glob(os.path.join(TOP, name + ".rows", "*")):
        wad, level, skill, seed, line = open(path).read().rstrip("\n").split("\t")
        outcome = line.split()[1]
        if outcome == "ERROR":
            continue
        r = {"outcome": outcome}
        for key, pattern in PAIRS.items():
            m = re.search(pattern, line)
            r[key] = (int(m.group(1)), int(m.group(2))) if m else (0, 1)
        m = re.search(r"(\d+)s game", line)
        r["secs"] = int(m.group(1)) if m else 0
        m = re.search(r"(\d+) deaths", line)
        r["deaths"] = int(m.group(1)) if m else 0
        r["exit"] = 1.0 if outcome == "EXITED" else 0.0
        r["seen"] = share(r["sectors"])
        r["progress"] = 1.0 if r["exit"] else r["seen"]
        r["damage"] = share(r["dealt"])
        r["stood"] = 1.0 if outcome in ("STUCK", "STALLED", "TIME") else 0.0
        runs[(wad, level, skill, seed)] = r
    return runs


def main():
    names = sys.argv[1:]
    if not names:
        sys.exit(__doc__)
    sets = {n: load(n) for n in names}
    for n in names:
        if not sets[n]:
            sys.exit(f"no runs in {os.path.join(TOP, n + '.rows')}")
    base = sets[names[0]]
    print(f"{'':12} {'runs':>5} " + " ".join(f"{c:>9}" for c in MEASURES) + "   outcomes")
    for n in names:
        d = sets[n]
        keys = [k for k in base if k in d]
        outcomes = collections.Counter(d[k]["outcome"] for k in keys)
        means = " ".join(f"{sum(d[k][c] for k in keys) / max(1, len(keys)):9.3f}" for c in MEASURES)
        tally = ", ".join(f"{a} {b}" for a, b in sorted(outcomes.items()))
        print(f"{n:12} {len(keys):5} {means}   {tally}")
        if len(keys) != len(base) or len(keys) != len(d):
            print(f"  NOT A WHOLE PAIRING: {len(base)} runs in {names[0]}, {len(d)} in {n}")
    for n in names[1:]:
        d = sets[n]
        keys = [k for k in base if k in d]
        print(f"\n{n} less {names[0]}, run for run ({len(keys)} runs):")
        for c in MEASURES:
            diffs = [d[k][c] - base[k][c] for k in keys]
            mean = sum(diffs) / len(diffs)
            var = sum((v - mean) ** 2 for v in diffs) / max(1, len(diffs) - 1)
            se = math.sqrt(var / len(diffs))
            more, less = sum(v > 0 for v in diffs), sum(v < 0 for v in diffs)
            print(f"  {c:9} {mean:+9.4f} (s.e. {se:.4f})  more {more}, less {less}")
        for skill in ("3", "4"):
            kk = [k for k in keys if k[2] == skill]
            if not kk:
                continue

            def mean_of(runs, c):
                return sum(runs[k][c] for k in kk) / len(kk)

            print(
                f"  skill {skill}: exits {sum(base[k]['exit'] for k in kk):.0f} -> "
                f"{sum(d[k]['exit'] for k in kk):.0f}, "
                f"progress {mean_of(base, 'progress'):.3f} -> {mean_of(d, 'progress'):.3f}, "
                f"damage {mean_of(base, 'damage'):.3f} -> {mean_of(d, 'damage'):.3f}"
            )
        by_level = collections.defaultdict(float)
        for k in keys:
            by_level[k[1]] += d[k]["exit"] - base[k]["exit"]
        moved = " ".join(f"{m}{v:+.0f}" for m, v in sorted(by_level.items()) if v)
        print("  exits by level:", moved or "none moved")
        stood = [k for k in keys if d[k]["stood"]]
        if stood:
            print("  stood out the hour:", " ".join(f"{k[1]}/{k[2]}/{k[3]}" for k in stood))


if __name__ == "__main__":
    main()
