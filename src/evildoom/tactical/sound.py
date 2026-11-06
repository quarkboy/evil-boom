"""Where a shot is heard: Doom's P_NoiseAlert. The noise floods out from the shooter's sector
through every two-sided line with an opening (a shut door stops it), and through at most one
line marked to block sound (it stops at the second). A monster in a sector it reached wakes
when it next looks about (unless it's deaf: an "ambush" monster wakes only on sight).

This is how sound works in the game, the same everywhere: what Evil knows. The monsters she
counts as woken are only the ones she's seen."""
from __future__ import annotations

from evildoom.wad.mapdata import ML_SOUNDBLOCK, MapData


class Hearing:
    def __init__(self, m: MapData):
        # sector -> (the sector on the other side, the line's index, blocks sound)
        self.links: list[list[tuple[int, bool]]] = [[] for _ in m.sectors]
        for line in m.linedefs:
            if not line.two_sided:
                continue
            a, b = m.front_sector(line), m.back_sector(line)
            if a is None or b is None or a == b:
                continue
            block = bool(line.flags & ML_SOUNDBLOCK)
            self.links[a].append((b, block))
            self.links[b].append((a, block))
        self._cache: tuple | None = None

    def heard(self, sector: int, heights: list[tuple[float, float]]) -> set[int]:
        """The sectors a shot from `sector` is heard in, the floors and ceilings as they are."""
        key = (sector, tuple(heights))
        if self._cache is not None and self._cache[0] == key:
            return self._cache[1]
        # (P_RecursiveSound: a sector is gone into again only if it's reached through fewer
        # sound-blocking lines than before)
        best: dict[int, int] = {sector: 0}
        stack = [(sector, 0)]
        while stack:
            s, blocks = stack.pop()
            if best.get(s, 2) < blocks:
                continue
            for other, block in self.links[s]:
                f = max(heights[s][0], heights[other][0])
                c = min(heights[s][1], heights[other][1])
                if c - f <= 0:
                    continue  # (a shut door)
                b = blocks + 1 if block else blocks
                if b >= 2 or best.get(other, 2) <= b:
                    continue
                best[other] = b
                stack.append((other, b))
        out = set(best)
        self._cache = (key, out)
        return out
