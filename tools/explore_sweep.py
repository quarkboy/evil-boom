"""Run the explorer (knowing only what it has seen) through many maps at once.

  python tools/explore_sweep.py doom2               every map in DOOM2.WAD
  python tools/explore_sweep.py doom E1M1 E1M4      just these

Each map: no monsters, god mode, up to --max-seconds of game time. One line per map as it
finishes; the log goes to out/explore/WAD_MAP.txt, and for a map it didn't finish, what the
omniscient planner would do from where it ended up (and what of that the explorer knew).
"""
import argparse
import os
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
OUT = ROOT / "out" / "explore"


def run_map(wad: str, map_name: str, max_seconds: float, mortal: bool = False,
            lives: int = 1) -> tuple[str, str, str]:
    from evildoom.config import resolve_wad
    from evildoom.engine.jump_check import keep_verified
    from evildoom.engine.vizdoom_engine import VizDoomEngine
    from evildoom.knowledge.explorer import Explorer
    from evildoom.motor.follower import TICRATE
    from evildoom.nav.reach import Navigator
    from evildoom.nav.walkthrough import plan_from
    from evildoom.wad.mapdata import load_map
    from evildoom.wad.wad import Wad

    iwad = resolve_wad(wad)
    m = load_map(Wad(iwad), map_name)
    nav = Navigator(m)
    keep_verified(nav, iwad, map_name)
    e = VizDoomEngine(iwad, map_name)
    e.game.add_game_args("+sv_cheats 1")
    e.start()
    if not mortal:
        e.game.send_game_command("god")
    from evildoom.knowledge.model import Knowledge
    x = Explorer(nav, Knowledge(nav, mortal=mortal))
    c = obs = None
    t0 = time.perf_counter()
    deaths = 0
    while e.tic < max_seconds * TICRATE:
        if e.finished:
            if e.dead and deaths + 1 < lives:
                deaths += 1
                e.restart(keep_time=True)
                x.restart()
                c = None
                continue
            break
        obs = e.observe()
        c = x.act(obs, c)
        e.step(c)
    exited, died, game_s = e.exited, e.finished and e.dead, e.tic / TICRATE
    k = x.knowledge
    lines = list(x.log) + ["", "knowledge events:"] + k.events + ["", k.summary()]
    if not exited and obs is not None:
        lines.append(f"ended at ({obs.x:.0f}, {obs.y:.0f}), objective: "
                     f"{x.current.label if x.current else '-'}")
        lp = plan_from(nav, (obs.x, obs.y), set(obs.keys or ()),
                       {s: {h} for s, h in obs.heights.items()}, max_seconds=30)
        lines.append(f"omniscient plan from there: {'ok' if lp.complete else lp.problem}")
        learned = {t.line for t, _ in k.known_triggers()}
        fired = {f.line for f in k.fired}
        known_polys = k.known_polys()
        for st in lp.steps:
            t = st.trigger
            if t is None:
                lines.append(f"  {st.action} {st.label}")
                continue
            lines.append(f"  {st.action} line {t.line} {t.info.trigger.name} {t.info.description}: "
                         f"mapped {t.line in k.mapped}, fired {t.line in fired}, learned {t.line in learned}, "
                         f"activators known {len(t.activators & known_polys)}/{len(t.activators)}, "
                         f"effect sectors known {sum(s in k.sectors for s in t.effects)}/{len(t.effects)}")
    e.close()
    outcome = "EXITED" if exited else "DIED" if died else "STUCK"
    detail = f"{game_s:.0f}s game, {time.perf_counter() - t0:.0f}s real, {deaths} deaths; {k.summary()}"
    lines.append(f"RESULT {outcome} {detail}")
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{iwad.stem.upper()}_{map_name}.txt").write_text("\n".join(lines) + "\n")
    return map_name, outcome, detail


def main() -> None:
    from evildoom.config import resolve_wad
    from evildoom.wad.wad import Wad
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("targets", nargs="+", help="WAD short names (doom, doom2) and/or map names")
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    ap.add_argument("--max-seconds", type=float, default=900)
    ap.add_argument("--mortal", action="store_true", help="no god mode: floors hurt")
    ap.add_argument("--lives", type=int, default=1,
                    help="tries at a map (dying starts it again, knowing what was learned)")
    a = ap.parse_args()
    groups: list[tuple[str, list[str]]] = []  # (wad, maps named after it)
    for t in a.targets:
        if t.lower() in ("doom", "doom2") or t.lower().endswith(".wad"):
            groups.append((t, []))
        elif not groups:
            ap.error(f"{t}: name a WAD before its maps")
        else:
            groups[-1][1].append(t.upper())
    jobs = [(w, mp) for w, maps in groups for mp in (maps or Wad(resolve_wad(w)).map_names())]
    results = []
    with ProcessPoolExecutor(a.jobs) as pool:
        futs = {pool.submit(run_map, w, n, a.max_seconds, a.mortal, a.lives): (w, n) for w, n in jobs}
        for i, f in enumerate(as_completed(futs), 1):
            w, n = futs[f]
            try:
                _, outcome, detail = f.result()
            except Exception as ex:  # noqa: BLE001 (report and go on)
                outcome, detail = "ERROR", repr(ex)[:200]
            results.append((w, n, outcome))
            print(f"[{i:3}/{len(jobs)}] {w} {n}: {outcome} {detail}", flush=True)
    done = sum(o == "EXITED" for *_, o in results)
    print(f"\n{done}/{len(results)} exited")


if __name__ == "__main__":
    main()
