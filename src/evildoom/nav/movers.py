"""Predict the floor/ceiling heights a special can move sectors to.

Targets mirror the vanilla source (p_doors.c, p_plats.c, p_floor.c, p_ceilng.c) and are
computed from the map's *initial* heights of neighboring sectors. That's exact for most stock
maps; chains of movers that depend on each other's moved heights are approximated.

Boom generalized movers are not predicted yet (stock maps don't use them).
"""
from collections.abc import Callable

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
FLOOR_SHORTEST_TEX = {30, 96}   # needs texture heights; approximated
STAIRS = {7: 8, 8: 8, 100: 16, 127: 16}
CEIL_TO_FLOOR = {41, 43}
CEIL_TO_FLOOR_8 = {44, 72}
CEIL_HIGHEST = {40}
CRUSHER = {6, 25, 49, 73, 77, 141}


class MoverModel:
    def __init__(self, m: MapData, neighbors: Callable[[int], set[int]]):
        self.map = m
        self.neighbors = neighbors
        self.initial: list[Heights] = [(s.floor, s.ceiling) for s in m.sectors]

    def _nfloors(self, s: int) -> list[int]:
        return [self.initial[n][0] for n in self.neighbors(s)]

    def _nceils(self, s: int) -> list[int]:
        return [self.initial[n][1] for n in self.neighbors(s)]

    def effects(self, special: int, sectors: list[int]) -> dict[int, list[Heights]]:
        """Heights each affected sector can take after the special fires.

        Returns several states for movers that oscillate (lifts, perpetual platforms).
        Doors that close again on their own (open-wait-close) report only the open state;
        the closed state is already the sector's initial state.
        """
        if special in STAIRS:
            return self._stairs(sectors, STAIRS[special])
        out = {}
        for s in sectors:
            states = self._sector_effect(special, s)
            if states:
                out[s] = states
        return out

    def _sector_effect(self, special: int, s: int) -> list[Heights]:
        floor, ceil = self.initial[s]
        nf, nc = self._nfloors(s), self._nceils(s)
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
            return [(floor + 64, ceil)]  # TODO: read texture heights from TEXTURE1/2
        if special in CEIL_TO_FLOOR:
            return [(floor, floor)]
        if special in CEIL_TO_FLOOR_8:
            return [(floor, floor + 8)]
        if special in CEIL_HIGHEST:
            return [(floor, max(nc + [ceil]))]
        if special in CRUSHER:
            return [(floor, floor + 8)]
        return []

    def _stairs(self, sectors: list[int], step: int) -> dict[int, list[Heights]]:
        """EV_BuildStairs: raise the tagged sector by one step, then follow two-sided lines
        whose front sector is the current step into back sectors with the same floor flat."""
        m = self.map
        out: dict[int, list[Heights]] = {}
        for start in sectors:
            sec = start
            height = self.initial[sec][0] + step
            texture = m.sectors[sec].floor_tex
            out[sec] = [(height, self.initial[sec][1])]
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
                out[sec] = [(height, self.initial[sec][1])]
        return out
