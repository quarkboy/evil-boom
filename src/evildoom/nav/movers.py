"""Predict the floor/ceiling heights a special can move sectors to.

Targets mirror the vanilla source (p_doors.c, p_plats.c, p_floor.c, p_ceilng.c) and are
computed from the map's *initial* heights of neighboring sectors. That's exact for most stock
maps; chains of movers that depend on each other's moved heights are approximated.

Boom generalized movers are not predicted yet (stock maps don't use them).
"""
import math
from collections.abc import Callable
from dataclasses import dataclass

from evildoom.wad.mapdata import ML_TWOSIDED, MapData

Heights = tuple[int, int]  # (floor, ceiling)

DOOR_OPEN = {1, 2, 4, 26, 27, 28, 29, 31, 32, 33, 34, 46, 61, 63, 86, 90, 99, 103, 105, 106,
             108, 109, 111, 112, 114, 115, 117, 118, 133, 134, 135, 136, 137}
DOOR_CLOSE = {3, 16, 42, 50, 75, 76, 107, 110, 113, 116}
LIFT = {10, 21, 62, 88, 120, 121, 122, 123}
PERPETUAL_LIFT = {53, 87}
FLOOR_LOWEST = {23, 37, 38, 60, 82, 84}
FLOOR_HIGHEST = {19, 45, 83, 102}
FLOOR_TURBO = {36, 70, 71, 98}
FLOOR_LOWEST_CEIL = {5, 24, 64, 91, 101}
FLOOR_LOWEST_CEIL_CRUSH = {55, 56, 65, 94}
FLOOR_NEXT_HIGHER = {18, 20, 22, 47, 68, 69, 95, 119, 128, 129, 130, 131, 132}
FLOOR_RAISE = {14: 32, 15: 24, 58: 24, 59: 24, 66: 24, 67: 32, 92: 24, 93: 24, 140: 512}
FLOOR_SHORTEST_TEX = {30, 96}   # raise by the shortest lower texture around the sector
STAIRS = {7: 8, 8: 8, 100: 16, 127: 16}
CEIL_TO_FLOOR = {41, 43}
CEIL_TO_FLOOR_8 = {44, 72}
CEIL_HIGHEST = {40}
CRUSHER = {6, 25, 49, 73, 77, 141}


TICRATE = 35


@dataclass(frozen=True)
class Timing:
    """How a mover gets to its target: `travel` tics to arrive (from the initial heights),
    then it stays `hold` tics before heading back (None: it stays for good)."""
    travel: int
    hold: int | None = None


# vanilla speeds in units/tic (p_spec.h, p_plats.c): doors 2 (blazing 8); lifts 4 (blazing 8),
# perpetual lifts 1, the "change texture" raises 1/2 (they're plats, not floors); floors and
# ceilings 1 (turbo/fast 4), stairs 1/4 (turbo 4)
DOOR_SPEED, DOOR_BLAZE = 2, 8
PLAT_SPEED, PLAT_BLAZE = 4, 8
PERPETUAL_SPEED = 1
PLAT_CHANGE_SPEED = 0.5
PLAT_CHANGE = {14, 15, 20, 22, 47, 66, 67, 68, 95}  # raiseAndChange, raiseToNearestAndChange
FLOOR_SPEED, FLOOR_FAST = 1, 4
DOOR_WAIT = 150          # VDOORWAIT
PLAT_WAIT = 3 * TICRATE  # PLATWAIT
DOOR_CLOSE_30 = {16, 76}  # close, wait 30 s, open


# Specials whose target depends on the heights round them (or their own) when they fire
DEPENDS_ON_HEIGHTS = (FLOOR_LOWEST | FLOOR_HIGHEST | FLOOR_TURBO | FLOOR_LOWEST_CEIL
                      | FLOOR_LOWEST_CEIL_CRUSH | FLOOR_NEXT_HIGHER | set(FLOOR_RAISE)
                      | FLOOR_SHORTEST_TEX | CEIL_HIGHEST
                      # (a door opens to the lowest ceiling round it then, less 4: MAP14's, by a
                      # crusher stopped low, opened 42 high and she couldn't get under it)
                      | DOOR_OPEN)


class MoverModel:
    def __init__(self, m: MapData, neighbors: Callable[[int], set[int]]):
        self.map = m
        self.neighbors = neighbors
        # The map's heights, which movers go by; `initial` is the planner's view of where the
        # player can stand, which can differ (Navigator._fill_trenches).
        self.real: list[Heights] = [(s.floor, s.ceiling) for s in m.sectors]
        self.initial: list[Heights] = list(self.real)

    def _at(self, s: int, heights: dict | None) -> Heights:
        return heights[s] if heights and s in heights else self.real[s]

    def _nfloors(self, s: int, heights: dict | None = None) -> list[int]:
        return [self._at(n, heights)[0] for n in self.neighbors(s)]

    def _nceils(self, s: int, heights: dict | None = None) -> list[int]:
        return [self._at(n, heights)[1] for n in self.neighbors(s)]

    def effects(self, special: int, sectors: list[int],
                heights: dict | None = None) -> dict[int, list[Heights]]:
        """Heights each affected sector can take after the special fires.

        Returns several states for movers that oscillate (lifts, perpetual platforms).
        Doors that close again on their own (open-wait-close) report only the open state;
        the closed state is already the sector's initial state.
        """
        if special in STAIRS:
            return self._stairs(sectors, STAIRS[special])
        out = {}
        for s in sectors:
            states = self._sector_effect(special, s, heights)
            if states:
                out[s] = states
        return out

    def _sector_effect(self, special: int, s: int, heights: dict | None = None) -> list[Heights]:
        """`heights`: sectors' heights now, where not the map's (the target is worked out
        when the special fires, from what's round it then)."""
        floor, ceil = self._at(s, heights)
        nf, nc = self._nfloors(s, heights), self._nceils(s, heights)
        if special in DOOR_OPEN:
            return [(floor, min(nc, default=ceil) - 4)]
        if special in DOOR_CLOSE:
            return [(floor, floor)]
        if special in LIFT:
            return [(min(nf + [floor]), ceil)]
        if special in PERPETUAL_LIFT:
            return [(min(nf + [floor]), ceil), (max(nf + [floor]), ceil)]
        if special in FLOOR_LOWEST:
            return [(min(nf + [floor]), ceil)]
        if special in FLOOR_HIGHEST:
            return [(max(nf, default=-500), ceil)]
        if special in FLOOR_TURBO:
            dest = max(nf, default=-500)
            return [(dest + 8 if dest != floor else dest, ceil)]
        if special in FLOOR_LOWEST_CEIL or special in FLOOR_LOWEST_CEIL_CRUSH:
            dest = min(min(nc, default=ceil), ceil)
            return [(dest - 8 if special in FLOOR_LOWEST_CEIL_CRUSH else dest, ceil)]
        if special in FLOOR_NEXT_HIGHER:
            higher = [f for f in nf if f > floor]
            return [(min(higher) if higher else floor, ceil)]
        if special in FLOOR_RAISE:
            return [(floor + FLOOR_RAISE[special], ceil)]
        if special in FLOOR_SHORTEST_TEX:
            return [(floor + self.shortest_lower_texture(s), ceil)]
        if special in CEIL_TO_FLOOR:
            return [(floor, floor)]
        if special in CEIL_TO_FLOOR_8:
            return [(floor, floor + 8)]
        if special in CEIL_HIGHEST:
            return [(floor, max(nc + [ceil]))]
        if special in CRUSHER:
            return [(floor, floor + 8)]
        return []

    def timing(self, special: int, description: str, s: int, target: Heights) -> Timing:
        """How long the special takes to move sector s from its initial heights to `target`,
        and how long it stays there (approximate: vanilla speeds, starting from rest)."""
        floor, ceil = self.initial[s]
        fast = "blazing" in description or "turbo" in description or "fast" in description
        moved = max(abs(target[0] - floor), abs(target[1] - ceil))
        if special in DOOR_OPEN or special in DOOR_CLOSE:
            speed = DOOR_BLAZE if fast else DOOR_SPEED
            if special in DOOR_CLOSE_30:
                hold = 30 * TICRATE
            else:
                hold = DOOR_WAIT if "wait" in description else None
        elif special in LIFT:
            speed, hold = (PLAT_BLAZE if fast else PLAT_SPEED), PLAT_WAIT
        elif special in PERPETUAL_LIFT:
            speed, hold = PERPETUAL_SPEED, PLAT_WAIT
        elif special in PLAT_CHANGE:
            speed, hold = PLAT_CHANGE_SPEED, None
        elif special in STAIRS:
            speed, hold = (FLOOR_FAST if fast else FLOOR_SPEED / 4), None
        elif special in CRUSHER:
            speed, hold = (2 if fast else 1), 0
        else:
            speed, hold = (FLOOR_FAST if fast else FLOOR_SPEED), None
        return Timing(int(math.ceil(moved / speed)) + 1, hold)

    def shortest_lower_texture(self, s: int) -> int:
        """Height of the shortest lower texture on the sector's two-sided lines (both sides).

        Missing textures ("-") are skipped, as in ZDoom/ViZDoom; vanilla counted texture #0
        there instead. Falls back to 64 if there's no texture data or nothing to measure.
        """
        m = self.map
        heights = []
        for line in m.sector_lines(s):
            if not line.two_sided:
                continue
            for side in (line.front, line.back):
                tex = m.sidedefs[side].lower
                if tex and tex != "-" and tex in m.texture_heights:
                    heights.append(m.texture_heights[tex])
        return min(heights) if heights else 64

    def _stairs(self, sectors: list[int], step: int) -> dict[int, list[Heights]]:
        """EV_BuildStairs: raise the tagged sector by one step, then follow two-sided lines
        whose front sector is the current step into back sectors with the same floor flat."""
        m = self.map
        out: dict[int, list[Heights]] = {}
        for start in sectors:
            sec = start
            height = self.real[sec][0] + step
            texture = m.sectors[sec].floor_tex
            out[sec] = [(height, self.real[sec][1])]
            while True:
                nxt = None
                for line in m.sector_lines(sec):
                    if not line.flags & ML_TWOSIDED or m.front_sector(line) != sec:
                        continue
                    back = m.back_sector(line)
                    if back is None or m.sectors[back].floor_tex != texture:
                        continue
                    height += step
                    if back in out:
                        continue
                    nxt = back
                    break
                if nxt is None:
                    break
                sec = nxt
                out[sec] = [(height, self.real[sec][1])]
        return out
