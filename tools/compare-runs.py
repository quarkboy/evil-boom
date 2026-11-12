# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""Compare deterministic gameplay and elapsed time between two release builds.

Run from the project root, without other sweeps/builds running:
    python tools/compare-runs.py BASELINE_EXE CANDIDATE_EXE --output out/comparison

Uses the normal engine/WAD configuration and navcache. Existing run/log files for the
selected maps are overwritten by evil; copies of every trial are kept under --output.
Wall time includes startup and end-of-run reporting. Timing is secondary to exact replay
equality: a different control, restart or journal entry fails the comparison.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import time


CASES = [
    ("doom2", "MAP13", []),
    ("doom2", "MAP17", []),
    ("doom2", "MAP24", []),
    ("doom2", "MAP29", []),
    ("doom2", "MAP26", ["--expert"]),
    ("doom2", "MAP23", ["--mortal"]),
    ("doom", "E2M4", ["--expert", "--mortal"]),
    ("doom", "E1M3", ["--secrets"]),
    ("doom2", "MAP01", ["--fight", "4", "--mortal"]),
]


def run(executable, wad, map_name, flags, destination, root, seconds):
    destination.mkdir(parents=True)
    suffix = "".join("_" + mode for mode in ("fight", "expert", "secrets", "peek") if "--" + mode in flags)
    if "--from" in flags:
        suffix += "_branch"
    recording = root / "out/runs" / f"{wad.upper()}_{map_name}{suffix}.run"
    log = root / "out/explore" / f"{wad.upper()}_{map_name}.txt"
    command = [str(executable), "run-map", wad, map_name, "--seed", "0", "--max-seconds", str(seconds), "--lives", "2", *flags]
    started = time.perf_counter()
    with (destination / "stdout.txt").open("wb") as output:
        subprocess.run(command, cwd=root, stdout=output, stderr=subprocess.STDOUT, check=True, timeout=900)
    elapsed = time.perf_counter() - started
    shutil.copyfile(recording, destination / "recording.run")
    shutil.copyfile(log, destination / "explorer.log")
    data = recording.read_bytes()
    return {"seconds": elapsed, "sha256": hashlib.sha256(data).hexdigest(), "controls": data.count(b"\nc ") + data.startswith(b"c ")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--seconds", type=int, default=600)
    args = parser.parse_args()
    if args.repeats < 1 or args.seconds < 1:
        parser.error("repeats and seconds must be positive")
    root = Path(os.environ.get("EVILBOOM_ROOT") or os.environ.get("EVILDOOM_ROOT") or Path(__file__).resolve().parents[1]).resolve()
    builds = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    if not all(p.is_file() for p in builds.values()):
        parser.error("both executables must exist")
    # Refuse to mix results from different versions in a previous comparison directory.
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for wad, map_name, flags in CASES:
        case = "_".join([wad, map_name, *[f.removeprefix("--") for f in flags]])
        trials = {name: [] for name in builds}
        for repeat in range(args.repeats):
            # Alternate order to reduce a systematic warm-cache/thermal advantage.
            order = list(builds) if repeat % 2 == 0 else list(reversed(builds))
            for name in order:
                trial = run(builds[name], wad, map_name, flags, args.output / case / f"{repeat}-{name}", root, args.seconds)
                trials[name].append(trial)
                print(f"{case} {name} {repeat + 1}: {trial['seconds']:.3f}s", flush=True)
        hashes = {trial["sha256"] for group in trials.values() for trial in group}
        if len(hashes) != 1:
            (args.output / "mismatch.json").write_text(json.dumps({"case": case, "trials": trials}, indent=2))
            raise SystemExit(f"Gameplay mismatch: {case}; compare retained recording.run files")
        medians = {name: statistics.median(t["seconds"] for t in group) for name, group in trials.items()}
        result = {"case": case, "median_seconds": medians, "speedup_percent": 100 * (1 - medians["candidate"] / medians["baseline"]), "trials": trials}
        results.append(result)
        (args.output / "results.json").write_text(json.dumps(results, indent=2))
        print(f"  identical recordings; {result['speedup_percent']:+.1f}%", flush=True)


if __name__ == "__main__":
    main()
