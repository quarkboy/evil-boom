"""Integration test: drive a planned route in ViZDoom (no monsters) to the exit."""
import pytest

from evildoom.config import resolve_wad
from evildoom.motor.follower import TICRATE, RouteFollower
from evildoom.nav.reach import Navigator
from evildoom.nav.walkthrough import plan_level
from evildoom.wad.mapdata import load_map
from evildoom.wad.wad import Wad

DOOM = resolve_wad("doom")
vizdoom = pytest.importorskip("vizdoom")
pytestmark = pytest.mark.skipif(not DOOM.exists(), reason="DOOM.WAD not installed")


def test_bot_exits_e1m1():
    from evildoom.engine.vizdoom_engine import VizDoomEngine

    m = load_map(Wad(DOOM), "E1M1")
    nav = Navigator(m)
    start = next(t for t in m.things if t.type == 1)
    plan = plan_level(nav, (start.x, start.y))
    assert plan.complete

    engine = VizDoomEngine(DOOM, "E1M1")
    engine.start()
    follower = RouteFollower(nav, plan.steps)
    while not engine.finished and not follower.failed and engine.tic < 120 * TICRATE:
        engine.step(follower.act(engine.observe()))
    exited = engine.finished
    engine.close()
    assert exited, "\n".join(follower.log.events)
