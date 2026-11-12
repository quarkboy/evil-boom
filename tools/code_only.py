#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""code_only.py [--check [REV]] [FILE...]: Rust source with its comments taken out.

With FILEs: prints each one's code alone (comments gone, runs of whitespace as one space).
With --check: every .rs file changed against REV (HEAD unless given) must have the same code
as it had there, comments aside; says which don't, and exits 1 if any. For a pass that's meant
to touch comments only: the proof that it did.
"""
import subprocess
import sys


def strip(src: str) -> str:
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        two = src[i:i + 2]
        if two == "//":
            while i < n and src[i] != "\n":
                i += 1
        elif two == "/*":
            depth, i = 1, i + 2
            while i < n and depth:
                if src[i:i + 2] == "/*":
                    depth, i = depth + 1, i + 2
                elif src[i:i + 2] == "*/":
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif c == '"' or (c in "rb" and _raw_or_byte(src, i)):
            j = _string_end(src, i)
            out.append(src[i:j])
            i = j
        elif c == "'":
            j = _char_end(src, i)
            out.append(src[i:j])
            i = j
        else:
            out.append(c)
            i += 1
    return " ".join("".join(out).split())


def _raw_or_byte(src: str, i: int) -> bool:
    if i and (src[i - 1].isalnum() or src[i - 1] == "_"):
        return False
    j = i
    if src[j] == "b":
        j += 1
    if j < len(src) and src[j] == "r":
        j += 1
        while j < len(src) and src[j] == "#":
            j += 1
    return j < len(src) and src[j] == '"' and j > i


def _string_end(src: str, i: int) -> int:
    j = i
    if src[j] == "b":
        j += 1
    hashes = 0
    raw = src[j] == "r"
    if raw:
        j += 1
        while src[j] == "#":
            hashes, j = hashes + 1, j + 1
    j += 1  # the opening quote
    close = '"' + "#" * hashes
    while j < len(src):
        if not raw and src[j] == "\\":
            j += 2
        elif src.startswith(close, j):
            return j + len(close)
        else:
            j += 1
    return j


def _char_end(src: str, i: int) -> int:
    # a char literal ('a', '\n', '\u{1F600}') or a lifetime ('a): only the first has a closing quote
    if src[i + 1:i + 2] == "\\":
        j = src.find("'", i + 2)
        return j + 1 if j != -1 else i + 1
    if src[i + 2:i + 3] == "'":
        return i + 3
    return i + 1


def main() -> int:
    args = sys.argv[1:]
    if args and args[0] == "--check":
        rev = args[1] if len(args) > 1 else "HEAD"
        changed = subprocess.run(["git", "diff", "--name-only", rev, "--", "*.rs"], capture_output=True, text=True, check=True).stdout.split()
        bad = []
        for path in changed:
            was = subprocess.run(["git", "show", f"{rev}:{path}"], capture_output=True, text=True)
            try:
                now = open(path, encoding="utf-8").read()
            except FileNotFoundError:
                bad.append(f"{path}: gone")
                continue
            if was.returncode != 0:
                bad.append(f"{path}: new")
            elif strip(was.stdout) != strip(now):
                bad.append(f"{path}: its code changed")
        print(f"{len(changed)} files changed against {rev}; {len(changed) - len(bad)} in their comments only")
        for b in bad:
            print("  " + b)
        return 1 if bad else 0
    for path in args:
        print(strip(open(path, encoding="utf-8").read()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
