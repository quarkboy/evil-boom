"""The native movement simulator against positions recorded from ViZDoom."""
import math

import pytest

import evildoom.engine.jump_check as jc
from evildoom.config import resolve_wad
from evildoom.nav.reach import Navigator
from evildoom.wad.mapdata import load_map
from evildoom.wad.wad import Wad

DOOM2 = resolve_wad("doom2")
pytestmark = pytest.mark.skipif(not DOOM2.exists(), reason="DOOM2.WAD not installed")


@pytest.fixture(scope="module")
def map13():
    return Navigator(load_map(Wad(DOOM2), "MAP13"))


def test_soft_jump_follows_the_engine(map13):
    """Coasting off the ledge from (1703,-3032): ViZDoom's trace, tic by tic (on the ledge,
    in the air, and the landing against a wall), to the hundredth of a unit."""
    nav = map13
    j = next(j for j in nav.jumps if j.key == "1703,-3032>1643,-3136>1534,-3303")
    trace = nav.jump_lab().trace((sorted(j.src_polys), j.dest_poly, j.dest, j.runup, j.takeoff,
                                  sorted((s, tuple(map(float, h))) for s, h in j.requires)), True)
    engine = {5: (1692.66, -3049.93, 400), 17: (1629.11, -3160.07, 400),
              25: (1586.90, -3233.23, 356), 31: (1555.25, -3288.11, 281),
              33: (1545.34, -3303.74, 304), 40: (1541.07, -3303.74, 304)}
    for i, (x, y, z) in engine.items():
        sx, sy, sz, _angle = trace[i]
        assert math.dist((sx, sy), (x, y)) < 0.01, i
        assert abs(sz - z) < 0.5, i


def test_jump_results_match_the_engine(map13):
    """MAP13 is one where the simulator and the engine check agree on every jump."""
    results = jc.simulate_jumps(map13)
    assert results["1703,-3032>1643,-3136>1534,-3303"] == jc.OK
    assert results["1700,-3048>1660,-3008>1525,-2873"] == jc.OK
