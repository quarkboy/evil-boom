"""Plan a level, drive it in ViZDoom with no monsters while recording a demo, then replay it
in a window at real-time speed (or a multiple of it).

  python spikes/play_route.py doom E1M1                  # plan, record, watch at 1x
  python spikes/play_route.py doom2 MAP01 --speed 2      # watch at 2x
  python spikes/play_route.py doom E1M1 --replay-only    # rewatch out/DOOM_E1M1.lmp
  python spikes/play_route.py doom E1M1 --no-replay      # just record

The demo is written to out/<WAD>_<MAP>.lmp.
"""
import argparse
import time
from pathlib import Path

from evildoom.config import resolve_wad
from evildoom.engine.jump_check import keep_verified
from evildoom.engine.vizdoom_engine import VizDoomEngine
from evildoom.motor.follower import TICRATE, RouteFollower
from evildoom.nav.reach import Navigator
from evildoom.nav.walkthrough import plan_level
from evildoom.wad.mapdata import load_map
from evildoom.wad.wad import Wad

OUT = Path(__file__).resolve().parent.parent / "out"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("wad", help="IWAD short name (doom, doom2, tnt, plutonia) or path")
    ap.add_argument("map", help="map name, e.g. E1M1 or MAP01")
    ap.add_argument("--speed", type=float, default=1.0, help="replay speed multiple")
    ap.add_argument("--replay-only", action="store_true", help="just replay the saved demo")
    ap.add_argument("--no-replay", action="store_true", help="record without watching")
    ap.add_argument("--max-minutes", type=float, default=15, help="game-time limit for the run")
    ap.add_argument("--god", action="store_true",
                    help="god mode for the run (tests pathfinding, not survival)")
    ap.add_argument("--size", default="1024x768",
                    help="replay window size, e.g. 800x600, 1280x720, 1920x1080")
    args = ap.parse_args()
    iwad = resolve_wad(args.wad)
    map_name = args.map.upper()
    demo = OUT / f"{iwad.stem}_{map_name}.lmp"

    if args.replay_only and not demo.exists():
        ap.error(f"no demo at {demo}; run without --replay-only first")
    if not args.replay_only:
        m = load_map(Wad(iwad), map_name)
        nav = Navigator(m)
        counts = keep_verified(nav, iwad, map_name, log=print)
        print(f"jumps checked in the engine: {counts}")
        start = next(t for t in m.things if t.type == 1)
        t0 = time.perf_counter()
        plan = plan_level(nav, (start.x, start.y))
        print(f"planned {len(plan.steps)} steps in {time.perf_counter() - t0:.1f}s"
              + ("" if plan.complete else f" -- INCOMPLETE: {plan.problem}"))
        for i, step in enumerate(plan.steps, 1):
            print(f"  {i:2}. {step.action:6} {step.label}")
        if not plan.steps:
            return

        engine = VizDoomEngine(iwad, map_name)
        if args.god:
            engine.game.add_game_args("+sv_cheats 1")
        engine.start(record=demo)
        if args.god:
            engine.game.send_game_command("god")
        follower = RouteFollower(nav, plan.steps)
        max_tics = int(args.max_minutes * 60 * TICRATE)
        while not engine.finished and not follower.done and not follower.failed \
                and engine.tic < max_tics:
            engine.step(follower.act(engine.observe()))
        exited, died = engine.exited, engine.dead
        # let the demo run a moment past the end of the route
        for _ in range(TICRATE):
            if engine.finished:
                break
            engine.step(follower.act(engine.observe()))
        engine.close()

        print("\nrun log:")
        for e in follower.log.events:
            print("  " + e)
        result = ("EXITED the level" if exited else
                  "DIED" if died else
                  "FAILED" if follower.failed else
                  "route done but level not exited" if follower.done else "timed out")
        print(f"\n{result} after {engine.tic / TICRATE:.1f}s of game time; demo: {demo}")

    if not args.no_replay:
        print(f"replaying {demo.name} at {args.speed}x ... (close the window to stop)")
        VizDoomEngine(iwad, map_name, visible=True, size=args.size).replay(demo, args.speed)


if __name__ == "__main__":
    main()
