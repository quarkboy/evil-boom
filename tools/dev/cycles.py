#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""cycles.py LOG...: where she goes round in circles, in runs' logs (out/explore/*.txt).

Reads each log's changes of state (the fight's move, the explorer's objective, the follower's
"stuck", "off course", "moved unexpectedly") and finds stretches where the same two or three
states come round again and again in quick succession: a to-and-fro between rules, each right
by itself. Prints them longest first: the log, when, how long, how many changes, the states.

    python3 tools/dev/cycles.py out/explore/DOOM_E1M7.txt
    python3 tools/dev/cycles.py --min 12 out/explore/*.s1.*.txt

--min N: the fewest changes that make a stretch worth printing (10); --gap T: changes further
apart than this (tics, 150) aren't the same stretch; --states K: at most this many states going
round (3)."""
import re
import sys
from collections import Counter

FIGHT = re.compile(r"^\s*\[fight\] tic (\d+): (.*)$")
OBJECTIVE = re.compile(r"^tic (\d+): (.*?) \(\d+ steps, ")
FOLLOWER = re.compile(r"^\s*\[\s*([\d.]+)s\] (stuck at|off course at|moved unexpectedly)")
# (what the fight says that isn't a move of hers: what she saw, heard, was hit by, held fire for)
NOT_A_MOVE = re.compile(
    r"^(saw |heard |hurt |hit |holding fire|switching to|.* dead|.* isn't where|no cover|got there|"
    r"can't get at|\(|a shot|dodging \w+ \d|fight over)")


def state(text):
    """A line's state: its words without the numbers, the target's name kept."""
    text = re.sub(r"\(target (\w+) \d+,.*\)", r"[\1]", text)
    text = re.sub(r"\(line \d+.*?\)", "(line)", text)
    text = re.sub(r"-?\d+(\.\d+)?", "#", text)
    return text.strip()


def events(path):
    out = []
    with open(path, errors="replace") as f:
        for line in f:
            line = line.rstrip("\n")
            # (a watcher's live log has its own clock in front)
            line = re.sub(r"^\[\s*[\d.]+s\] ", "", line)
            if m := FIGHT.match(line):
                if not NOT_A_MOVE.match(m.group(2)):
                    out.append((int(m.group(1)), "fight: " + state(m.group(2))))
            elif m := OBJECTIVE.match(line):
                out.append((int(m.group(1)), "goes to: " + state(m.group(2))))
            elif m := FOLLOWER.match(line):
                out.append((int(float(m.group(1)) * 35), "route: " + m.group(2).replace(" at", "")))
    return out


def stretches(ev, least, gap, most_states):
    """Maximal stretches of changes no further apart than `gap`, between at most `most_states`
    states, each of them come round at least three times."""
    found, i = [], 0
    while i < len(ev):
        seen, j = Counter(), i
        while j < len(ev) and (j == i or ev[j][0] - ev[j - 1][0] <= gap):
            if ev[j][1] not in seen and len(seen) == most_states:
                break
            seen[ev[j][1]] += 1
            j += 1
        # (changes, not the same state said again)
        changes = sum(1 for k in range(i + 1, j) if ev[k][1] != ev[k - 1][1])
        going_round = [s for s, n in seen.items() if n >= 3]
        if changes >= least and len(going_round) >= 2:
            found.append((ev[i][0], ev[j - 1][0], changes, seen.most_common()))
            i = j
        else:
            i += 1
    return found


def main():
    args, least, gap, most = sys.argv[1:], 10, 150, 3
    paths = []
    while args:
        a = args.pop(0)
        if a == "--min":
            least = int(args.pop(0))
        elif a == "--gap":
            gap = int(args.pop(0))
        elif a == "--states":
            most = int(args.pop(0))
        else:
            paths.append(a)
    if not paths:
        sys.exit(__doc__)
    rows = []
    for p in paths:
        for a, b, changes, seen in stretches(events(p), least, gap, most):
            rows.append((b - a, p, a, b, changes, seen))
    rows.sort(reverse=True)
    for span, p, a, b, changes, seen in rows:
        name = p.rsplit("/", 1)[-1]
        print(f"{name}: tic {a} to {b} ({span / 35:.0f} s), {changes} changes")
        for s, n in seen:
            print(f"    {n:4} x {s}")
    kinds = Counter()
    for span, _, _, _, _, seen in rows:
        kinds[" <-> ".join(sorted(s for s, n in seen if n >= 3))] += span
    print(f"\n{len(rows)} stretches; by what goes round, seconds in all:")
    for k, span in kinds.most_common(25):
        print(f"  {span / 35:6.0f} s  {k}")


if __name__ == "__main__":
    main()
