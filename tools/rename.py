#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""rename.py OLD NEW [--in FILE]: rename a field, method or function the compiler's way.

There's no IDE here to rename with, and a name like `pressed` or `mode` belongs to several
types: a search and replace would change them all. So: the definition is renamed by hand
first (or with --in FILE, the one `OLD` in that file that is a definition: `OLD:` in a
struct, `fn OLD`), and this then asks the compiler what no longer resolves and renames
exactly those uses, again and again till it builds. Only identifiers the compiler reports as
unknown, at the place it reports them, are touched.

Run from the repository's root; checks with `cargo check --all-targets` in native/.
"""
import json
import re
import subprocess
import sys


def check():
    out = subprocess.run(
        ["cargo", "check", "--all-targets", "--message-format=json", "--target-dir", "../out/check"],
        cwd="native", capture_output=True, text=True,
    ).stdout
    spans = []
    errors = 0
    for line in out.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        msg = m.get("message") or {}
        if msg.get("level") != "error":
            continue
        errors += 1
        for sp in msg.get("spans", []):
            if sp.get("is_primary"):
                spans.append((sp["file_name"], sp["line_start"], sp["column_start"], sp["column_end"], msg.get("message", "")))
    return errors, spans


def main():
    args = sys.argv[1:]
    old, new = args[0], args[1]
    if "--in" in args:
        path = args[args.index("--in") + 1]
        src = open(path).read()
        pat = re.compile(r"(\bfn\s+|\bpub(?:\([a-z]+\))?\s+|^\s+)" + re.escape(old) + r"(\s*[:(<])", re.M)
        found = [m for m in pat.finditer(src) if "fn" in m.group(1) or m.group(2).strip() == ":"]
        if len(found) != 1:
            sys.exit(f"{path}: {len(found)} definitions of {old}, wanted one: rename it by hand")
        m = found[0]
        src = src[:m.start()] + m.group(1) + new + m.group(2) + src[m.end():]
        open(path, "w").write(src)
    total = 0
    for _ in range(40):
        errors, spans = check()
        todo = {}
        for file, line, c0, c1, text in spans:
            if old not in text:
                continue
            todo.setdefault(file, []).append((line, c0, c1))
        if not todo:
            print(f"{old} -> {new}: {total} uses renamed; {errors} errors left")
            return 1 if errors else 0
        for file, places in todo.items():
            path = "native/" + file
            lines = open(path).read().split("\n")
            for line, c0, c1 in sorted(set(places), reverse=True):
                row = lines[line - 1]
                if row[c0 - 1:c1 - 1] == old:
                    lines[line - 1] = row[:c0 - 1] + new + row[c1 - 1:]
                    total += 1
                else:
                    # the span covers more than the name (a struct literal's field, say)
                    seg = row[c0 - 1:c1 - 1]
                    fixed = re.sub(r"\b" + re.escape(old) + r"\b", new, seg, count=1)
                    if fixed != seg:
                        lines[line - 1] = row[:c0 - 1] + fixed + row[c1 - 1:]
                        total += 1
            open(path, "w").write("\n".join(lines))
    sys.exit("gave up after 40 rounds")


if __name__ == "__main__":
    sys.exit(main())
