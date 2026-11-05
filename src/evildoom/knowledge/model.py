"""What Evil knows about the level: only what she has seen and what she has learned by doing.

Seeing follows the automap: a linedef is mapped once the renderer draws any part of it
(evildoom_native.Sight), and the sectors either side of a mapped line are known, with what's
in them. What lines do is learned by using them: a line Evil sets off (pressing it, walking
over it, shooting it) is fired, and its effect on a sector is learned when she sees that
sector move, then or later. What a line *is* can be told on sight from its special type (not
its textures, which custom WADs draw however they like): switches and use-doors, exits, the
keys a lock needs, teleporters.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field

import evildoom_native

from evildoom.motor.follower import Controls, Observation
from evildoom.nav.reach import Navigator, TriggerLine
from evildoom.wad.mapdata import ML_MAPPED
from evildoom.wad.specials import Category, Lock, Trigger

USE_RANGE = 64.0     # P_UseLines
SHOT_RANGE = 2048.0  # far enough for any gun line worth shooting
SHOT_HEIGHT = 32.0   # above the feet (P_LineAttack's shootz, near enough)
VIEW_WIDTH = 320     # screen columns the view is worked out for (the engine's width)

USED = (Trigger.S1, Trigger.SR, Trigger.D1, Trigger.DR)
WALKED = (Trigger.W1, Trigger.WR)
SHOT = (Trigger.G1, Trigger.GR)


@dataclass
class Fired:
    """A line Evil set off."""
    line: int
    tic: int
    how: str  # "use", "walk", "shoot"


@dataclass
class Knowledge:
    nav: Navigator
    width: int = VIEW_WIDTH
    mapped: set[int] = field(default_factory=set)       # linedefs seen
    sectors: set[int] = field(default_factory=set)      # sectors seen (either side of a mapped line)
    seen_heights: dict[int, tuple[float, float]] = field(default_factory=dict)  # when last seen
    seen_tic: dict[int, int] = field(default_factory=dict)  # sector -> last tic in view
    fired: list[Fired] = field(default_factory=list)
    learned: dict[int, set[int]] = field(default_factory=dict)  # trigger index -> sectors whose effect is known
    teleports: dict[int, tuple[float, float]] = field(default_factory=dict)  # used line -> where it took us
    visited: set[int] = field(default_factory=set)      # polys stood in
    events: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        m = self.nav.map
        self._sight = evildoom_native.Sight(m)
        self._by_line: dict[int, list[int]] = {}
        for i, t in enumerate(self.nav.triggers):
            if t.line >= 0:
                self._by_line.setdefault(t.line, []).append(i)
        from evildoom.wad.specials import classify_line
        self._info = {i: info for i, line in enumerate(m.linedefs)
                      if (info := classify_line(line.special)) is not None}
        self._last: Observation | None = None
        self._teleport_from: int | None = None
        for i, line in enumerate(m.linedefs):
            if line.flags & ML_MAPPED:
                self._map_line(i)

    # --- seeing -------------------------------------------------------------------------

    def _map_line(self, li: int) -> None:
        m = self.nav.map
        self.mapped.add(li)
        line = m.linedefs[li]
        for s in (m.front_sector(line), m.back_sector(line)):
            if s is not None and s not in self.sectors:
                self.sectors.add(s)
                self.seen_heights.setdefault(s, (m.sectors[s].floor, m.sectors[s].ceiling))

    def _heights_list(self, obs: Observation) -> list[tuple[float, float]]:
        m = self.nav.map
        hs = [(float(s.floor), float(s.ceiling)) for s in m.sectors]
        for s, h in obs.heights.items():
            if s < len(hs):
                hs[s] = (float(h[0]), float(h[1]))
        return hs

    def update(self, obs: Observation, controls: Controls | None = None) -> None:
        """Take in a tic: what's in view now, what got set off (`controls`: what Evil did
        on the tic that led here)."""
        m = self.nav.map
        heights = self._heights_list(obs)
        drawn, through = self._sight.drawn(obs.x, obs.y, obs.angle, self.width, heights)
        in_view: set[int] = set()
        for li in through:
            # (not on the automap, but the floor across it is in view)
            line = m.linedefs[li]
            for s in (m.front_sector(line), m.back_sector(line)):
                if s is not None:
                    in_view.add(s)
                    if s not in self.sectors:
                        self.sectors.add(s)
        for li in drawn:
            line = m.linedefs[li]
            for s in (m.front_sector(line), m.back_sector(line)):
                if s is not None:
                    in_view.add(s)
            if li not in self.mapped:
                self._map_line(li)
        poly = self.nav.mesh.poly_at(obs.x, obs.y)
        if poly is not None:
            self.visited.add(poly.index)
            in_view.add(poly.sector)
            if poly.sector not in self.sectors:
                self.sectors.add(poly.sector)
        last = self._last
        if last is not None:
            self._detect_fired(last, obs, controls, heights)
            if self._teleport_from is not None and math.dist((last.x, last.y), (obs.x, obs.y)) > 64:
                self.teleports[self._teleport_from] = (obs.x, obs.y)
                self.events.append(f"tic {obs.tic}: teleport line {self._teleport_from} goes to "
                                   f"({obs.x:.0f}, {obs.y:.0f})")
                self._teleport_from = None
        for s in in_view:
            self.seen_tic[s] = obs.tic
            now = heights[s]
            if self.seen_heights.get(s, now) != now:
                self._saw_move(s, obs.tic)
            self.seen_heights[s] = now
        self._last = obs

    def _saw_move(self, s: int, tic: int) -> None:
        """Sector s is seen at new heights: put it down to the latest line fired that moves
        it."""
        for f in reversed(self.fired):
            for ti in self._by_line.get(f.line, ()):
                t = self.nav.triggers[ti]
                if s in t.effects:
                    known = self.learned.setdefault(ti, set())
                    if s not in known:
                        known.add(s)
                        self.events.append(f"tic {tic}: learned line {f.line} ({t.info.description}) "
                                           f"moves sector {s}")
                    return

    # --- doing --------------------------------------------------------------------------

    def _lines_along(self, a, b) -> list[tuple[float, int]]:
        """Linedefs the segment a-b crosses, as (fraction along, line), nearest first."""
        m, grid = self.nav.map, self.nav.mesh._line_grid
        from evildoom.nav.navmesh import _cells
        seen, out = set(), []
        for cell in _cells(a[0], a[1], b[0], b[1]):
            for li in grid.get(cell, ()):
                if li in seen:
                    continue
                seen.add(li)
                line = m.linedefs[li]
                p, q = m.vertexes[line.v1], m.vertexes[line.v2]
                t = _intersect(a, b, (p.x, p.y), (q.x, q.y))
                if t is not None:
                    out.append((t, li))
        out.sort()
        return out

    def _front(self, li: int, x: float, y: float) -> bool:
        m = self.nav.map
        line = m.linedefs[li]
        a, b = m.vertexes[line.v1], m.vertexes[line.v2]
        return (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x) <= 0

    def _opening(self, li: int, heights) -> tuple[float, float] | None:
        m = self.nav.map
        line = m.linedefs[li]
        f, b = m.front_sector(line), m.back_sector(line)
        if f is None or b is None:
            return None
        return max(heights[f][0], heights[b][0]), min(heights[f][1], heights[b][1])

    def _detect_fired(self, last: Observation, obs: Observation, controls: Controls | None,
                      heights) -> None:
        m = self.nav.map
        # walking over
        for _, li in self._lines_along((last.x, last.y), (obs.x, obs.y)):
            info = self._special(li)
            if info is None or info.trigger not in WALKED:
                continue
            if info.category is Category.TELEPORT:
                if not self._front(li, last.x, last.y):
                    continue
                self._teleport_from = li
            self._fire(li, obs.tic, "walk")
        if controls is None:
            return
        a = math.radians(last.angle)
        if controls.use:
            # P_UseLines: the first line along the use trace with a special, unless something
            # shut is in the way first (only from its front side)
            end = (last.x + USE_RANGE * math.cos(a), last.y + USE_RANGE * math.sin(a))
            for _, li in self._lines_along((last.x, last.y), end):
                info = self._special(li)
                if info is not None and info.trigger in USED:
                    if self._front(li, last.x, last.y):
                        self._fire(li, obs.tic, "use")
                    break
                opening = self._opening(li, heights)
                if opening is None or opening[1] <= opening[0]:
                    break
        if controls.attack:
            z = (last.z if last.z is not None else heights[self._sector_at(last)][0]) + SHOT_HEIGHT
            end = (last.x + SHOT_RANGE * math.cos(a), last.y + SHOT_RANGE * math.sin(a))
            for _, li in self._lines_along((last.x, last.y), end):
                info = self._special(li)
                if info is not None and info.trigger in SHOT:
                    self._fire(li, obs.tic, "shoot")
                opening = self._opening(li, heights)
                if opening is None or not opening[0] < z < opening[1]:
                    break

    def _sector_at(self, obs: Observation) -> int:
        poly = self.nav.mesh.poly_at(obs.x, obs.y)
        return poly.sector if poly is not None else 0

    def _special(self, li: int):
        return self._info.get(li)

    def _fire(self, li: int, tic: int, how: str) -> None:
        info = self._special(li)
        if info.trigger in (Trigger.W1, Trigger.S1, Trigger.G1, Trigger.D1) and \
                any(f.line == li for f in self.fired):
            return  # (once only: nothing happens again)
        self.fired.append(Fired(li, tic, how))
        self.events.append(f"tic {tic}: {how} line {li} ({info.description})")

    # --- what Evil can tell -------------------------------------------------------------

    def interactables(self) -> list[int]:
        """Mapped lines she can tell are switches or use-doors (or gun targets), not used
        yet."""
        used = {f.line for f in self.fired}
        out = []
        for li in sorted(self.mapped):
            info = self._special(li)
            if info is not None and info.trigger in USED + SHOT and li not in used:
                out.append(li)
        return out

    def exits(self) -> list[int]:
        """Exit lines she can tell are there: mapped ones, or walk-over ones in a sector she
        has seen (they're seldom drawn themselves; the exit room is)."""
        m = self.nav.map
        out = []
        for i, info in self._info.items():
            line = m.linedefs[i]
            if info.category not in (Category.EXIT, Category.SECRET_EXIT):
                continue
            sides = {m.front_sector(line), m.back_sector(line)} - {None}
            if i in self.mapped or (info.trigger in WALKED and sides & self.sectors):
                out.append(i)
        return out

    def lock(self, li: int) -> Lock:
        """The keys a line she has seen needs (told on sight)."""
        info = self._special(li)
        return info.lock if info is not None else Lock.NONE

    def teleporters(self) -> list[int]:
        """Teleport lines she can tell are there (mapped, or in a sector she has seen)."""
        m = self.nav.map
        out = []
        for i, info in self._info.items():
            line = m.linedefs[i]
            if info.category is not Category.TELEPORT:
                continue
            sides = {m.front_sector(line), m.back_sector(line)} - {None}
            if i in self.mapped or sides & self.sectors:
                out.append(i)
        return out

    def known_things(self) -> list:
        """The map's things in sectors she has seen (at skill 3)."""
        out = []
        for t in self.nav.map.things:
            poly = self.nav.mesh.poly_at(t.x, t.y)
            if poly is not None and poly.sector in self.sectors and t.in_skill(3):
                out.append(t)
        return out

    def unexplained_since(self) -> int | None:
        """The last tic a line was fired whose effect hasn't been seen at all, if any."""
        explained = {self.nav.triggers[i].line for i, s in self.learned.items() if s}
        tics = [f.tic for f in self.fired if f.line not in explained]
        return max(tics) if tics else None

    def known_polys(self) -> set[int]:
        return {p.index for p in self.nav.mesh.polys if p is not None and p.sector in self.sectors}

    def known_triggers(self) -> list[tuple[TriggerLine, set[int]]]:
        """Triggers whose effect on some sectors she has learned, with those sectors."""
        return [(self.nav.triggers[i], s) for i, s in self.learned.items() if s]

    def summary(self) -> str:
        m = self.nav.map
        return (f"{len(self.mapped)}/{len(m.linedefs)} lines, {len(self.sectors)}/{len(m.sectors)} "
                f"sectors, {len(self.fired)} fired, {sum(map(len, self.learned.values()))} effects "
                f"learned, {len(self.interactables())} untried switches/doors, "
                f"{len(self.exits())} exits seen")


def _intersect(a, b, p, q) -> float | None:
    """Where along a-b it crosses p-q (0..1), or None."""
    rx, ry = b[0] - a[0], b[1] - a[1]
    sx, sy = q[0] - p[0], q[1] - p[1]
    den = rx * sy - ry * sx
    if den == 0:
        return None
    t = ((p[0] - a[0]) * sy - (p[1] - a[1]) * sx) / den
    u = ((p[0] - a[0]) * ry - (p[1] - a[1]) * rx) / den
    return t if 0 <= t <= 1 and 0 <= u <= 1 else None
