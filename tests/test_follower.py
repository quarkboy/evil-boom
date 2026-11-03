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
    exited = engine.exited  # finished *and* alive
    engine.close()
    assert exited, "\n".join(follower.log.events)


def test_bot_recovers_after_being_knocked_off_course():
    """Warp the player off the route mid-way (like being blasted into a side room): the
    follower must notice, replan from where it is and still exit."""
    from evildoom.engine.vizdoom_engine import VizDoomEngine
    from evildoom.motor.follower import Controls

    m = load_map(Wad(DOOM), "E1M1")
    nav = Navigator(m)
    start = next(t for t in m.things if t.type == 1)
    plan = plan_level(nav, (start.x, start.y))
    engine = VizDoomEngine(DOOM, "E1M1")
    engine.game.add_game_args("+sv_cheats 1")
    engine.start()
    follower = RouteFollower(nav, plan.steps)
    knocked = False
    while not engine.finished and not follower.failed and engine.tic < 120 * TICRATE:
        if not knocked and engine.tic >= 6 * TICRATE:
            engine.game.send_game_command("warp 2160 -2432")  # a side room off the route
            knocked = True
            engine.step(Controls())
            continue
        engine.step(follower.act(engine.observe()))
    exited = engine.exited
    engine.close()
    assert exited, "\n".join(follower.log.events)
    assert follower.replans >= 1, "\n".join(follower.log.events)
