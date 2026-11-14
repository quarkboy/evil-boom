#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""cycles.py [options] LOG...: where she goes round in circles, in runs' logs (out/explore/*.txt).

Reads each log's changes of state (the fight's move, the explorer's objective, the follower's
"stuck", "off course", "moved unexpectedly") and finds stretches where the same few states come
round again and again in quick succession: a to-and-fro between rules, each right by itself.

Prints a catalogue: by what goes round (the two states that come round most, monsters' names
left out), how many stretches in how many runs, the seconds in all and the longest, how fast it
turns, what she killed and was hurt meanwhile, which monsters, and the longest one to look at.
With --list, every stretch, longest first.

    python3 tools/dev/cycles.py out/cycles/*.txt
    python3 tools/dev/cycles.py --list out/explore/DOOM_E1M7.txt

--min N     the fewest changes that make a stretch (10)
--gap T     changes further apart than this (tics, 150) aren't the same stretch
--states K  at most this many states going round (4)
--join T    stretches of the same two states this near each other (tics, 350) are one
--tsv FILE  the stretches as a table too"""
import re
import sys
from collections import Counter, defaultdict

FIGHT = re.compile(r"^\s*\[fight\] tic (\d+): (.*)$")
OBJECTIVE = re.compile(r"^tic (\d+): (.*?) \(\d+ steps, ")
FOLLOWER = re.compile(r"^\s*\[\s*([\d.]+)s\] (stuck at|off course at|moved unexpectedly)")
DEAD = re.compile(r"^\w+ \d+ dead")
HURT = re.compile(r"^hurt (\d+)")
TARGET = re.compile(r"\(target (\w+) \d+,")
# (what the fight says that isn't a move of hers: what she saw, heard, was hit by, held fire for)
NOT_A_MOVE = re.compile(
    r"^(saw |heard |hurt |hit |holding fire|switching to|.* dead|.* isn't where|no cover|got there|"
    r"can't get at|\(|a shot|dodging \w+ \d|fight over)")
# (a monster's or a missile's name, as the logs have them: two capitals run together, or one of
# the plain ones)
NAME = re.compile(
    r"\b(?:[A-Z][a-z]+(?:[A-Z][a-z]*)+|Zombieman|Demon|Spectre|Cacodemon|Fatso|Revenant|"
    r"Cyberdemon|Archvile|Arachnotron|Rocket|Shotgun|Chaingun|Stimpack|Medikit|Clip|Shell|"
    r"Backpack|Soulsphere|Megasphere|Berserk|Chainsaw|Cell|Shellbox|Clipbox)\b")


def state(text):
    """A line's state: its words without the numbers or the names."""
    text = re.sub(r"\(target .*\)", "", text)
    text = re.sub(r"\(line \d+.*?\)", "", text)
    text = re.sub(r"to \(-?\d+, -?\d+\)", "", text)
    text = NAME.sub("X", text)
    text = re.sub(r"-?\d+(\.\d+)?", "#", text)
    # (an errand's a detail of the move, not another move)
    text = re.sub(r" (on the way to|towards the|for its) X", "", text)
    return re.sub(r"\s+", " ", text).strip(" :,")


def events(path):
    """(tic, state, target) for each change; and the tics of her kills and what hurt her."""
    out, kills, hurts = [], [], []
    with open(path, errors="replace") as f:
        for line in f:
            line = line.rstrip("\n")
            # (a watcher's live log has its own clock in front)
            line = re.sub(r"^\[\s*[\d.]+s\] ", "", line)
            if m := FIGHT.match(line):
                tic, text = int(m.group(1)), m.group(2)
                if DEAD.match(text):
                    kills.append(tic)
                elif h := HURT.match(text):
                    hurts.append((tic, int(h.group(1))))
                if not NOT_A_MOVE.match(text):
                    t = TARGET.search(text)
                    out.append((tic, "fight: " + state(text), t.group(1) if t else ""))
            elif m := OBJECTIVE.match(line):
                out.append((int(m.group(1)), "goes to: " + state(m.group(2)), ""))
            elif m := FOLLOWER.match(line):
                tic = int(float(m.group(1)) * 35)
                out.append((tic, "route: " + m.group(2).replace(" at", ""), ""))
    return out, kills, hurts


def stretches(ev, least, gap, most_states):
    """Maximal stretches of changes no further apart than `gap`, between at most `most_states`
    states, two of them at least come round three times: (first, last, changes, states)."""
    found, i = [], 0
    while i < len(ev):
        seen, j = Counter(), i
        while j < len(ev) and (j == i or ev[j][0] - ev[j - 1][0] <= gap):
            if ev[j][1] not in seen and len(seen) == most_states:
                break
            seen[ev[j][1]] += 1
            j += 1
        changes = sum(1 for k in range(i + 1, j) if ev[k][1] != ev[k - 1][1])
        if changes >= least and sum(1 for n in seen.values() if n >= 3) >= 2:
            found.append([i, j - 1, changes, seen])
            i = j
        else:
            i += 1
    return found


def pair(seen):
    """What goes round: the two states that come round most."""
    return tuple(sorted(s for s, _ in seen.most_common(2)))


def joined(found, ev, join):
    """Stretches of the same two states near each other, as one."""
    out = []
    for s in found:
        last = out[-1] if out else None
        if last and pair(last[3]) == pair(s[3]) and ev[s[0]][0] - ev[last[1]][0] <= join:
            last[1], last[2], last[3] = s[1], last[2] + s[2], last[3] + s[3]
        else:
            out.append(s)
    return out


def main():
    args, least, gap, most, join, listing, tsv = sys.argv[1:], 10, 150, 4, 350, False, None
    paths = []
    while args:
        a = args.pop(0)
        if a == "--min":
            least = int(args.pop(0))
        elif a == "--gap":
            gap = int(args.pop(0))
        elif a == "--states":
            most = int(args.pop(0))
        elif a == "--join":
            join = int(args.pop(0))
        elif a == "--tsv":
            tsv = args.pop(0)
        elif a == "--list":
            listing = True
        else:
            paths.append(a)
    if not paths:
        sys.exit(__doc__)
    rows = []
    for p in paths:
        ev, kills, hurts = events(p)
        name = p.rsplit("/", 1)[-1].removesuffix(".txt")
        for i, j, changes, seen in joined(stretches(ev, least, gap, most), ev, join):
            a, b = ev[i][0], ev[j][0]
            rows.append({
                "run": name, "from": a, "to": b, "seconds": (b - a) / 35, "changes": changes,
                "pair": pair(seen), "states": seen.most_common(),
                "kills": sum(1 for t in kills if a <= t <= b),
                "hurt": sum(h for t, h in hurts if a <= t <= b),
                "targets": Counter(t for _, _, t in ev[i:j + 1] if t),
            })
    rows.sort(key=lambda r: -r["seconds"])
    if tsv:
        with open(tsv, "w") as f:
            f.write("run\tfrom_tic\tto_tic\tseconds\tchanges\tkills\thurt\tstate_a\tstate_b\ttargets\n")
            for r in rows:
                f.write("\t".join(str(x) for x in (
                    r["run"], r["from"], r["to"], f"{r['seconds']:.1f}", r["changes"], r["kills"],
                    r["hurt"], r["pair"][0], r["pair"][-1], ",".join(r["targets"]))) + "\n")
    if listing:
        for r in rows:
            print(f"{r['run']}: tic {r['from']} to {r['to']} ({r['seconds']:.0f} s), "
                  f"{r['changes']} changes, {r['kills']} kills, hurt {r['hurt']}")
            for s, n in r["states"]:
                print(f"    {n:4} x {s}")
        return
    by = defaultdict(list)
    for r in rows:
        by[r["pair"]].append(r)
    total = sum(r["seconds"] for r in rows)
    print(f"{len(paths)} runs, {len(rows)} stretches, {total:.0f} s in all "
          f"({len({r['run'] for r in rows})} runs have one)\n")
    for p, rs in sorted(by.items(), key=lambda kv: -sum(r["seconds"] for r in kv[1])):
        secs = sum(r["seconds"] for r in rs)
        changes = sum(r["changes"] for r in rs)
        worst = max(rs, key=lambda r: r["seconds"])
        targets = Counter()
        for r in rs:
            targets.update(r["targets"])
        print(" <-> ".join(p))
        print(f"    {len(rs)} stretches in {len({r['run'] for r in rs})} runs; {secs:.0f} s in all, "
              f"the longest {worst['seconds']:.0f} s; {changes / max(secs, 1):.1f} changes a second")
        print(f"    meanwhile: {sum(r['kills'] for r in rs)} kills, hurt {sum(r['hurt'] for r in rs)}"
              f" ({sum(r['hurt'] for r in rs) / max(secs, 1) * 60:.0f} a minute)")
        print("    at: " + ", ".join(f"{t} {n}" for t, n in targets.most_common(5)))
        print(f"    longest: {worst['run']} tic {worst['from']} to {worst['to']}\n")


if __name__ == "__main__":
    main()
