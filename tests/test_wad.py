"""Tests against the real Ultimate Doom IWAD; skipped when it isn't installed."""
import pytest

from evildoom.config import resolve_wad
from evildoom.wad.mapdata import load_map
from evildoom.wad.specials import Category, classify_line, classify_sector
from evildoom.wad.wad import Wad

DOOM = resolve_wad("doom")
pytestmark = pytest.mark.skipif(not DOOM.exists(), reason="DOOM.WAD not installed")


@pytest.fixture(scope="module")
def wad():
    return Wad(DOOM)


def test_lists_all_maps(wad):
    names = wad.map_names()
    assert len(names) == 36 and names[0] == "E1M1" and names[-1] == "E4M9"


def test_e1m1(wad):
    m = load_map(wad, "E1M1")
    assert (len(m.linedefs), len(m.sectors)) == (486, 88)
    start = next(t for t in m.things if t.type == 1)
    assert (start.x, start.y) == (1056, -3616)  # matches ViZDoom's reported spawn
    exits = [l.index for l in m.linedefs
             if l.special and classify_line(l.special).category is Category.EXIT]
    assert exits == [326]
    secrets = [s.index for s in m.sectors if classify_sector(s.special).secret]
    assert secrets == [83, 86, 87]
    assert m.subsectors and m.segs


def test_every_map_parses(wad):
    for name in wad.map_names():
        m = load_map(wad, name)
        assert m.linedefs and m.sectors and m.things
