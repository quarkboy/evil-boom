"""Run the bot through many maps at once (one process per map) and report which exit.

  python tools/sweep.py doom2                 every map in DOOM2.WAD
  python tools/sweep.py doom E1M1 E1M2 E4M9   just these
  python tools/sweep.py doom2 doom --jobs 10  both games, 10 maps at a time

Each map: plan the level, then follow the plan in ViZDoom in god mode, no monsters (bosses
are killed with a console command when the plan needs them dead), up to --max-seconds of
game time. One line per map as it finishes; the full run log goes to out/sweep/WAD_MAP.txt.
"""
import argparse
import os
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "out" / "sweep"


def run_map(wad: str, map_name: str, max_seconds: float) -> tuple[str, str, str]:
    """(map, outcome, detail) for one map; writes its log file."""
    from evildoom.config import resolve_wad
    from evildoom.engine.jump_check import keep_verified
    from evildoom.engine.vizdoom_engine import VizDoomEngine
    from evildoom.motor.follower import TICRATE, Controls, RouteFollower
    from evildoom.nav.reach import Navigator
    from evildoom.nav.walkthrough import plan_level
    from evildoom.wad.mapdata import load_map
    from evildoom.wad.wad import Wad

    iwad = resolve_wad(wad)
    lines = []
    t0 = time.perf_counter()
    m = load_map(Wad(iwad), map_name)
    nav = Navigator(m)
    keep_verified(nav, iwad, map_name)
    s = next(t for t in m.things if t.type == 1)
    t1 = time.perf_counter()
    plan = plan_level(nav, (s.x, s.y))
    t2 = time.perf_counter()
    lines.append(f"navigator {t1 - t0:.1f}s, plan {t2 - t1:.1f}s: "
                 f"{'complete' if plan.complete else 'INCOMPLETE ' + plan.problem}")
    lines += [f"  {i:2}. " + " -> ".join(st.substeps()) for i, st in enumerate(plan.steps, 1)]
    if not plan.complete:
        outcome, detail = "NO PLAN", plan.problem
    else:
        bosses = any(st.action == "kill" for st in plan.steps)
        e = VizDoomEngine(iwad, map_name, nomonsters=not bosses)
        e.game.add_game_args("+sv_cheats 1")
        e.start()
        if not any(st.action == "endure" for st in plan.steps):  # (an exit floor has to hurt)
            e.game.send_game_command("god")
        if bosses:
            e.game.send_game_command("kill monsters")
        f = RouteFollower(nav, plan.steps)
        while not e.finished and not f.done and not f.failed and e.tic < max_seconds * TICRATE:
            e.step(f.act(e.observe()))
        # (a walk-over exit goes off as the route finishes; the level ends a tic or so later)
        grace = e.tic + 2 * TICRATE
        while f.done and not e.finished and e.tic < grace:
            e.step(Controls())
        exited, game_s = e.exited, e.tic / TICRATE
        e.close()
        lines += [str(ev) for ev in f.log.events]
        if exited:
            outcome, detail = "EXITED", f"{game_s:.0f}s"
        else:
            last = next((str(ev) for ev in reversed(f.log.events) if "FAILED" in str(ev)), "")
            outcome = "FAILED"
            detail = f"{game_s:.0f}s " + (last.split("] ", 1)[-1] if last else "timed out")
    lines.append(f"RESULT {outcome} {detail}")
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{iwad.stem.upper()}_{map_name}.txt").write_text("\n".join(lines) + "\n")
    return map_name, outcome, detail


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("targets", nargs="+", help="WAD short names (doom, doom2) and/or map names")
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2),
                    help="maps at a time (default: half the CPU threads)")
    ap.add_argument("--max-seconds", type=float, default=300, help="game time per map")
    args = ap.parse_args()

    from evildoom.config import resolve_wad
    from evildoom.wad.wad import Wad

    groups: list[tuple[str, list[str]]] = []  # (wad, maps named after it)
    for t in args.targets:
        if t.lower() in ("doom", "doom2", "tnt", "plutonia") or t.lower().endswith(".wad"):
            groups.append((t, []))
        elif not groups:
            ap.error(f"{t}: name a WAD before its maps")
        else:
            groups[-1][1].append(t.upper())
    work = [(w, mp) for w, maps in groups
            for mp in (maps or Wad(resolve_wad(w)).map_names())]
    print(f"{len(work)} maps, {args.jobs} at a time", flush=True)
    t0 = time.perf_counter()
    results = []
    with ProcessPoolExecutor(max_workers=args.jobs) as pool:
        futures = {pool.submit(run_map, w, mp, args.max_seconds): (w, mp) for w, mp in work}
        for fut in as_completed(futures):
            w, mp = futures[fut]
            try:
                _, outcome, detail = fut.result()
            except Exception as exc:  # a crash in one map shouldn't stop the sweep
                outcome, detail = "ERROR", f"{type(exc).__name__}: {exc}"
            results.append((w, mp, outcome, detail))
            print(f"[{len(results):3}/{len(work)}] {w} {mp}: {outcome} {detail}", flush=True)
    exited = sum(1 for r in results if r[2] == "EXITED")
    print(f"\n{exited}/{len(results)} exited in {time.perf_counter() - t0:.0f}s")
    for w, mp, outcome, detail in sorted(results):
        if outcome != "EXITED":
            print(f"  {w} {mp}: {outcome} {detail}")


if __name__ == "__main__":
    sys.exit(main())
