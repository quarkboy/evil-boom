#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""profile.py FILE [--under NAME] [--top N]: where a sampled run's time went.

FILE is what a build with `--features profile` writes for EVIL_PROFILE=FILE: a line a stack,
outermost frame first, frames joined by ';', then how many samples (499 a second) saw it.
Printed: the functions by the time under them (themselves and all they call), then by the
time in themselves alone. `--under NAME`: only the stacks through a function whose name
holds NAME, and the times as shares of those. See docs/DEBUGGING.md.
"""
import re
import sys
from collections import Counter


def short(name):
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    name = re.sub(r"<([^<>]*) as [^<>]*>", r"\1", name)
    return name.replace("evilboom::", "")


def main():
    args = sys.argv[1:]
    under = args[args.index("--under") + 1] if "--under" in args else None
    top = int(args[args.index("--top") + 1]) if "--top" in args else 40
    total, inclusive, own = 0, Counter(), Counter()
    for line in open(args[0], errors="replace"):
        stack, _, count = line.rstrip("\n").rpartition(" ")
        frames = [short(f) for f in stack.split(";")]
        if under:
            at = next((i for i, f in enumerate(frames) if under in f), None)
            if at is None:
                continue
            frames = frames[at:]
        n = int(count)
        total += n
        for f in set(frames):
            inclusive[f] += n
        own[frames[-1]] += n
    print(f"{total} samples, {total / 499:.1f} s" + (f" under {under}" if under else ""))
    for title, table in (("under it", inclusive), ("in itself", own)):
        print(f"\n{title}:")
        for f, n in table.most_common(top):
            print(f"  {100 * n / max(total, 1):5.1f}%  {n / 499:7.2f} s  {f[:150]}")


main()
