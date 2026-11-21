#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""cycles.py: do any two of the program's top-level modules depend on each other?

A module depends on another when anything under it names `crate::<the other>` (comments left
out). Two that name each other can't be told apart as layers, tested apart or ever split into
crates; there were eight such pairs once (docs/design/code-review-2026-11-21.md, M1) and there
are none now. Run from anywhere; exits 1 if there's a pair, saying where each names the other.
The program's own entry points (main, lib, cli, help) are above everything and left out.
"""
import collections
import glob
import os
import re
import sys

SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "native", "src")


def main():
    deps = collections.defaultdict(set)
    where = collections.defaultdict(lambda: collections.defaultdict(set))
    for path in glob.glob(os.path.join(SRC, "**", "*.rs"), recursive=True):
        rel = os.path.relpath(path, SRC)
        top = rel.split(os.sep)[0].removesuffix(".rs")
        if top in ("main", "lib", "cli", "help"):
            continue
        text = re.sub(r"(?m)^\s*//.*$", "", open(path).read())
        for m in re.finditer(r"\bcrate::([a-z_0-9]+)", text):
            other = m.group(1)
            if other != top:
                deps[top].add(other)
                where[top][other].add(rel)
    pairs = [(a, b) for a in sorted(deps) for b in sorted(deps[a]) if a < b and a in deps.get(b, ())]
    for a, b in pairs:
        print(f"{a} <-> {b}: {a} in {sorted(where[a][b])[:3]}, {b} in {sorted(where[b][a])[:3]}")
    print(f"{len(pairs)} pairs of modules that depend on each other")
    sys.exit(1 if pairs else 0)


if __name__ == "__main__":
    main()
