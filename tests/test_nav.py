import pytest

from evildoom.config import resolve_wad
from evildoom.nav.navmesh import NavMesh, area
from evildoom.nav.reach import Navigator, can_cross, funnel
from evildoom.wad.mapdata import load_map
from evildoom.wad.specials import Category, Key
from evildoom.wad.wad import Wad

DOOM = resolve_wad("doom")
needs_doom = pytest.mark.skipif(not DOOM.exists(), reason="DOOM.WAD not installed")


# --- movement rules ---------------------------------------------------------------------

def test_step_up_limit():
    assert can_cross((0, 128), (24, 128))
    assert not can_cross((0, 128), (25, 128))


def test_any_drop_is_allowed_but_not_back_up():
    assert can_cross((200, 400), (0, 400))
    assert not can_cross((0, 400), (200, 400))


def test_headroom():
    assert can_cross((0, 56), (0, 56))
    assert not can_cross((0, 55), (0, 128))       # source too low
    assert not can_cross((0, 128), (16, 64))      # step up into 48 units of room
    assert not can_cross((0, 0), (0, 128))        # closed door


# --- funnel -----------------------------------------------------------------------------

def test_funnel_straight_corridor_is_one_segment():
    portals = [((x, 10.0), (x, -10.0)) for x in (10.0, 20.0, 30.0)]  # (left, right) going +x
    assert funnel((0.0, 0.0), (40.0, 0.0), portals) == [(0.0, 0.0), (40.0, 0.0)]


def test_funnel_bends_around_corner():
    # Corridor goes +x then turns +y; the inside corner is at (10, 10).
    portals = [((10.0, 10.0), (10.0, -10.0)), ((10.0, 10.0), (30.0, 10.0))]
    path = funnel((0.0, 0.0), (20.0, 40.0), portals)
    assert path == [(0.0, 0.0), (10.0, 10.0), (20.0, 40.0)]


# --- real maps ----------------------------------------------------------------------------

@pytest.fixture(scope="module")
def wad():
    return Wad(DOOM)


def _start(m):
    t = next(t for t in m.things if t.type == 1)
    return t.x, t.y


@needs_doom
def test_mesh_polys_are_found_by_bsp_lookup(wad):
    m = load_map(wad, "E1M1")
    mesh = NavMesh(m)
    polys = [p for p in mesh.polys if p]
    assert len(polys) == len(m.subsectors)
    for p in polys:
        assert area(p.vertices) > 0
        assert mesh.poly_at(*p.center) is p


@needs_doom
def test_e1m1_needs_no_keys_and_starts_with_doors_closed(wad):
    m = load_map(wad, "E1M1")
    nav = Navigator(m)
    start = _start(m)
    stages = nav.progression(nav.mesh.poly_at(*start).index)
    assert len(stages) == 1 and any(t.info.category is Category.EXIT for t in stages[0].exits)

    exit_t = next(t for t in nav.triggers if t.info.category is Category.EXIT)
    goal = nav._activation_point(exit_t, start, set(range(len(nav.mesh.polys))))
    # Every door starts closed, so there is no path without opening any...
    assert nav.find_path(start, goal, nav.initial_states()) is None
    # ...but the planner opens doors on the way.
    route = nav.plan_route(start, goal)
    assert route is not None
    assert any("open door" in note for leg in route.legs for note in leg.path.steps)


@needs_doom
def test_e1m3_exit_is_behind_the_blue_door(wad):
    m = load_map(wad, "E1M3")
    nav = Navigator(m)
    start = _start(m)
    stages = nav.progression(nav.mesh.poly_at(*start).index)
    # Both keycards are reachable without keys; the exit opens up once they're held.
    assert stages[0].keys_available == {Key.BLUE_CARD, Key.YELLOW_CARD}
    assert not any(t.info.category is Category.EXIT for t in stages[0].exits)
    assert any(t.info.category is Category.EXIT for t in stages[1].exits)

    exit_t = next(t for t in nav.triggers if t.info.category is Category.EXIT)
    goal = nav._activation_point(exit_t, start, set(range(len(nav.mesh.polys))))
    assert nav.plan_route(start, goal) is None
    assert nav.plan_route(start, goal, {Key.YELLOW_CARD}) is None
    assert nav.plan_route(start, goal, {Key.BLUE_CARD}) is not None
