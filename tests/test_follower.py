"""Integration test: drive a planned route in DSDA-Doom (no monsters) to the exit.

DSDA-Doom is one game a process, so each test plays in a process of its own."""
import multiprocessing
from concurrent.futures import ProcessPoolExecutor

import pytest

from evildoom.config import resolve_wad
from evildoom.engine.dsda_engine import LIBRARY

DOOM = resolve_wad("doom")
pytestmark = [pytest.mark.skipif(not DOOM.exists(), reason="DOOM.WAD not installed"),
              pytest.mark.skipif(not LIBRARY.exists(), reason="DSDA-Doom library not built")]

KNOCKED_TO = (2160, -2432)  # a side room off E1M1's route


def _drive(knock: bool) -> tuple[bool, int, list[str]]:
    """Plan E1M1 and follow the plan (`knock`: warped off the route 6s in, like being blasted
    into a side room). (exited, replans, the follower's log)."""
    from evildoom.engine.dsda_engine import DsdaEngine
    from evildoom.motor.follower import TICRATE, Controls, RouteFollower
    from evildoom.nav.reach import Navigator
    from evildoom.nav.walkthrough import plan_level
    from evildoom.wad.mapdata import load_map
    from evildoom.wad.wad import Wad

    m = load_map(Wad(DOOM), "E1M1")
    nav = Navigator(m)
    start = next(t for t in m.things if t.type == 1)
    plan = plan_level(nav, (start.x, start.y))
    assert plan.complete
    engine = DsdaEngine(DOOM, "E1M1")
    engine.start()
    follower = RouteFollower(nav, plan.steps)
    knocked = not knock
    while not engine.finished and not follower.failed and engine.tic < 120 * TICRATE:
        if not knocked and engine.tic >= 6 * TICRATE:
            engine.game.warp(*KNOCKED_TO, 0.0)
            knocked = True
            engine.step(Controls())
            continue
        engine.step(follower.act(engine.observe()))
    return engine.exited, follower.replans, follower.log.events


def _in_own_process(knock: bool):
    with ProcessPoolExecutor(1, mp_context=multiprocessing.get_context("spawn")) as pool:
        return pool.submit(_drive, knock).result()


def test_bot_exits_e1m1():
    exited, _, events = _in_own_process(knock=False)
    assert exited, "\n".join(events)


def test_bot_recovers_after_being_knocked_off_course():
    """The follower must notice it's off the route, replan from where it is and still exit."""
    exited, replans, events = _in_own_process(knock=True)
    assert exited, "\n".join(events)
    assert replans >= 1, "\n".join(events)
