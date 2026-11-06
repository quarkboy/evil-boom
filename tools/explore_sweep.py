"""Run the explorer (knowing only what it has seen) through many maps at once.

  python tools/explore_sweep.py doom2               every map in DOOM2.WAD
  python tools/explore_sweep.py doom E1M1 E1M4      just these

Each map: no monsters, god mode, up to --max-seconds of game time (--fight 4 --mortal: monsters
on Ultra-Violence, and no god mode). One line per map as it
finishes; the log goes to out/explore/WAD_MAP.txt, and for a map it didn't finish, what the
omniscient planner would do from where it ended up (and what of that the explorer knew).
"""
import argparse
import math
import os
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
OUT = ROOT / "out" / "explore"
STRANDED_TICS = 60 * 35  # nothing to do this long (with lives left): start the level again


# Levels with secrets nobody can get: how many short of the tally counts as all (E4M3: two
# torch sectors no one can stand in; E4M7: one; MAP15's #9 and MAP27's #5: teleporter sectors
# the teleport line keeps everyone out of)
BROKEN_SECRETS = {"E4M3": 2, "E4M7": 1, "MAP15": 1, "MAP27": 1}


def _plan_report(lp, k, x=None, real=None) -> list[str]:
    """The omniscient plan's steps, each with what the explorer knew of it: the trigger, and
    (with the explorer `x` and the `real` heights) the way there, where it goes through what
    she doesn't know or by heights she doesn't take any sector to be at."""
    learned = {t.line for t, _ in k.known_triggers()}
    fired = {f.line for f in k.fired}
    known_polys = k.known_polys()
    believed = x.states() if x is not None else None
    out = []
    for st in lp.steps or ():
        if believed is not None and st.path is not None:
            mesh = x.nav.mesh
            unknown = [p for p in st.path.polys if p not in known_polys]
            off = sorted({mesh.polys[p].sector for p in st.path.polys
                          if (h := real.get(mesh.polys[p].sector)) is not None
                          and tuple(h) not in believed[mesh.polys[p].sector]})
            if unknown or off:
                out.append(f"    (the way: {len(unknown)} of {len(st.path.polys)} polys unknown"
                           + (f", from poly {unknown[0]} at {tuple(round(c) for c in mesh.polys[unknown[0]].center)}" if unknown else "")
                           + "".join(f"; sector {s} is {tuple(real[s])}, she takes it as {sorted(believed[s])}" for s in off)
                           + (f"; via {st.path.steps}" if st.path.steps else "") + ")")
            reach = getattr(x, "_reach", None) or set()
            cut = next((i for i, p in enumerate(st.path.polys) if p not in reach), None)
            if cut is not None and reach:
                p, prev = st.path.polys[cut], st.path.polys[cut - 1] if cut else None
                out.append(f"    (her reach ends at poly {p}, sector {mesh.polys[p].sector}, "
                           f"{tuple(round(c) for c in mesh.polys[p].center)}"
                           + (f", from sector {mesh.polys[prev].sector}" if prev is not None else "")
                           + (f"; stuck there {k.stuck_at[p]}x" if k.stuck_at.get(p) else "")
                           + ("; in a trap" if any(p in t for t in k.traps) else "") + ")")
        t = st.trigger
        if t is None:
            out.append(f"  {st.action} {st.label}")
            continue
        out.append(f"  {st.action} line {t.line} {t.info.trigger.name} {t.info.description}: "
                   f"mapped {t.line in k.mapped}, fired {t.line in fired}, learned {t.line in learned}, "
                   f"activators known {len(t.activators & known_polys)}/{len(t.activators)}, "
                   f"effect sectors known {sum(s in k.sectors for s in t.effects)}/{len(t.effects)}")
    return out


def run_map(wad: str, map_name: str, max_seconds: float, mortal: bool = False,
            lives: int = 1, fight: int = 0, engine: str = "vizdoom",
            expert: bool = False, peek: bool = False, secrets: bool = False,
            seed: int | None = 0) -> tuple[str, str, str]:
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
    skill = fight or 3
    nav = Navigator(m, skill=skill)
    keep_verified(nav, iwad, map_name)
    if engine == "dsda":
        from evildoom.engine.dsda_engine import DsdaEngine
        e = DsdaEngine(iwad, map_name, skill=skill, nomonsters=not fight, objects=bool(fight) or expert,
                       seed=seed)
    else:
        e = VizDoomEngine(iwad, map_name, skill=skill, nomonsters=not fight, objects=bool(fight),
                          headless=True)
    e.start()
    if not mortal:
        e.god()
    from evildoom.knowledge.model import Knowledge
    if fight:
        from evildoom.tactical.player import Player
        x = Player(nav, Knowledge(nav, mortal=mortal, perceive=not peek))
    else:
        x = Explorer(nav, Knowledge(nav, mortal=mortal, perceive=not peek))
        if expert:
            from evildoom.knowledge.completion import Completionist
            Completionist(x, restarts=lives - 1)
    # the secrets run: out only with every secret there is (the tally's count, as the automap
    # shows a player: how many, not where); till then, everything else first, the exit last
    secrets_short = BROKEN_SECRETS.get(map_name, 0)
    if secrets:
        x.exit_last = True
    c = obs = None
    t0 = time.perf_counter()
    deaths = 0
    idle_since = None  # since when she's had nothing to do
    spent = {"engine": 0.0, "observe": 0.0, "think": 0.0}  # seconds, where the time goes
    while e.tic < max_seconds * TICRATE:
        stranded = idle_since is not None and e.tic - idle_since > STRANDED_TICS \
                or getattr(x, "restart_wanted", False)
        if e.finished and e.dead and fight and obs is not None:
            seen = ", ".join(f"{t.name} {math.dist((t.x, t.y), (obs.x, obs.y)):.0f} away"
                             for t in x.combat.visible)
            x.log.append(f"tic {e.tic}: killed: health {obs.health:.0f}, armor {obs.armor:.0f}, "
                         f"weapon {obs.weapon}, weapons {sorted(obs.weapons or ())}, ammo {obs.ammo}; "
                         f"in sight: {seen or 'nothing'}; doing: {x.current.label if x.current else '-'}")
        if getattr(x, "restart_wanted", False) and deaths + 1 >= lives:
            # (no level again to be had: out with what she has)
            x.restart_wanted = False
            x.before_exit = None
            stranded = False
        if e.finished or (stranded and deaths + 1 < lives):
            # dead, or stranded with nothing to do (a trap she couldn't know of: MAP12's
            # floor that drops into a pit): the level again, knowing what she learned
            if (e.dead or stranded) and deaths + 1 < lives:
                deaths += 1
                why = "died" if e.dead else "for what's left" if getattr(x, "restart_wanted", False) else "stranded"
                x.log.append(f"tic {e.tic}: {why}: starting again")
                if why == "stranded" and obs is not None:
                    # (a trap she walked into, or a way on she couldn't see?)
                    lp = plan_from(nav, (obs.x, obs.y), set(obs.keys or ()),
                                   {s: {h} for s, h in obs.heights.items()}, max_seconds=20)
                    x.log.append(f"  stranded at ({obs.x:.0f}, {obs.y:.0f}), objective "
                                 f"{x.current.label if x.current else '-'}; omniscient plan from there: "
                                 f"{'ok' if lp.complete else lp.problem}")
                    x.log.extend("  " + line for line in _plan_report(lp, x.knowledge, x, obs.heights))
                if hasattr(x, "restart_wanted"):
                    x.restart_wanted = False

                e.restart(keep_time=True)
                x.restart()
                c = None
                idle_since = None
                continue
            break
        if secrets and e.tic % TICRATE == 0:
            got, of = e.tally()["secrets"]
            x.exit_last = got < of - secrets_short
        t1 = time.perf_counter()
        obs = e.observe()
        t2 = time.perf_counter()
        c = x.act(obs, c)
        t3 = time.perf_counter()
        if x.current is None:
            idle_since = e.tic if idle_since is None else idle_since
        else:
            idle_since = None
        e.step(c)
        spent["observe"] += t2 - t1
        spent["think"] += t3 - t2
        spent["engine"] += time.perf_counter() - t3
    exited, died, game_s = e.exited, e.finished and e.dead, e.tic / TICRATE
    k = x.knowledge
    lines = list(x.log) + ["", "knowledge events:"] + k.events + ["", k.summary()]
    if k.doubts:
        lines.append("in doubt: " + ", ".join(f"{key} moves sector {s} (since tic {tic})"
                                              for (key, s), tic in sorted(k.doubts.items(), key=str)))
    if not exited and obs is not None:
        lines.append(f"ended at ({obs.x:.0f}, {obs.y:.0f}), objective: "
                     f"{x.current.label if x.current else '-'}")
        lp = plan_from(nav, (obs.x, obs.y), set(obs.keys or ()),
                       {s: {h} for s, h in obs.heights.items()}, max_seconds=30)
        lines.append(f"omniscient plan from there: {'ok' if lp.complete else lp.problem}")
        lines += _plan_report(lp, k, x, obs.heights)
    tally = e.tally() if hasattr(e, "tally") else None
    if expert and obs is not None and obs.things is not None:
        from evildoom.knowledge.completion import COUNTED
        known = x.knowledge.known_polys()
        for t in obs.things:
            if t.name in COUNTED:
                poly = nav.mesh.poly_at(t.x, t.y)
                seen = poly is not None and poly.index in known
                lines.append(f"left: {t.name} at ({t.x:.0f}, {t.y:.0f}), sector "
                             f"{poly.sector if poly else None}, {'seen' if seen else 'never seen'}")
    e.close()
    outcome = "EXITED" if exited else "DIED" if died else "STUCK"
    if secrets and exited and tally is not None and tally["secrets"][0] >= tally["secrets"][1] - secrets_short:
        outcome = "SECRETS"  # (every secret there is to find, then out)
    if expert and exited and tally is not None:
        from evildoom.knowledge.completion import most_items
        short = most_items(m, skill)
        if tally["items"][0] >= tally["items"][1] - short:
            outcome = "EXPERT"  # (every item there is to get, then out)
    real = time.perf_counter() - t0
    lines.append("time: " + ", ".join(f"{k} {v:.1f}s" for k, v in spent.items())
                 + f", other {real - sum(spent.values()):.1f}s; {e.tic / max(real, 1e-9):.0f} tics/s")
    detail = f"{game_s:.0f}s game, {time.perf_counter() - t0:.0f}s real, {deaths} deaths; {k.summary()}"
    if tally is not None:
        detail = f"items {tally['items'][0]}/{tally['items'][1]}, " + detail
        if secrets:
            detail = f"secrets {tally['secrets'][0]}/{tally['secrets'][1]}, " + detail
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
    ap.add_argument("--max-seconds", type=float, default=3600, help="game seconds before a map counts as stuck (an hour: slow is fine, only stuck is a failure)")
    ap.add_argument("--mortal", action="store_true", help="no god mode: floors hurt")
    ap.add_argument("--fight", type=int, default=0, metavar="SKILL",
                    help="monsters in, at this skill (4: Ultra-Violence), and Evil fights them")
    ap.add_argument("--engine", choices=("vizdoom", "dsda"), default="vizdoom",
                    help="dsda: DSDA-Doom in-process (a process a map: it's one game a process)")
    ap.add_argument("--peek", action="store_true",
                    help="every switch and door known on sight from the map data (no secrets to find)")
    ap.add_argument("--expert", action="store_true",
                    help="every item that counts picked up, then the exit (DSDA-Doom, no monsters)")
    ap.add_argument("--secrets", action="store_true",
                    help="every secret there is found (going by the tally's count), then the exit (DSDA-Doom)")
    ap.add_argument("--lives", type=int, default=1,
                    help="tries at a map (dying starts it again, knowing what was learned)")
    ap.add_argument("--seed", default="0",
                    help="the game's random numbers from this seed, the same run every time (a "
                         "number), or from the clock as in a real game ('random')")
    a = ap.parse_args()
    seed = None if a.seed == "random" else int(a.seed)
    groups: list[tuple[str, list[str]]] = []  # (wad, maps named after it)
    for t in a.targets:
        if t.lower() in ("doom", "doom2") or t.lower().endswith(".wad"):
            groups.append((t, []))
        elif not groups:
            ap.error(f"{t}: name a WAD before its maps")
        else:
            groups[-1][1].append(t.upper())
    jobs = [(w, mp) for w, maps in groups for mp in (maps or Wad(resolve_wad(w)).map_names())]
    # (the same run every time: set and dict orders that depend on hashing don't vary between
    # processes; the workers inherit it)
    os.environ.setdefault("PYTHONHASHSEED", "0")
    results = []
    with ProcessPoolExecutor(a.jobs, max_tasks_per_child=1 if a.engine == "dsda" else None) as pool:
        futs = {pool.submit(run_map, w, n, a.max_seconds, a.mortal, a.lives, a.fight, a.engine, a.expert,
                           a.peek, a.secrets, seed): (w, n)
                for w, n in jobs}
        for i, f in enumerate(as_completed(futs), 1):
            w, n = futs[f]
            try:
                _, outcome, detail = f.result()
            except Exception as ex:  # noqa: BLE001 (report and go on)
                outcome, detail = "ERROR", repr(ex)[:200]
            results.append((w, n, outcome))
            print(f"[{i:3}/{len(jobs)}] {w} {n}: {outcome} {detail}", flush=True)
    done = sum(o in ("EXITED", "EXPERT", "SECRETS") for *_, o in results)
    print(f"\n{done}/{len(results)} exited")
    if a.expert:
        print(f"{sum(o == 'EXPERT' for *_, o in results)}/{len(results)} with every item")
    if a.secrets:
        print(f"{sum(o == 'SECRETS' for *_, o in results)}/{len(results)} with every secret")


if __name__ == "__main__":
    main()
