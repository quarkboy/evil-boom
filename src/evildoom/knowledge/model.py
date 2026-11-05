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
HEAR_RANGE = 1200.0  # S_CLIPPING_DIST: sounds further off aren't heard at all
HEAR_BEARING = 15.0  # how well she can tell where a sound came from: degrees...
HEAR_DISTANCE = 64.0  # ...and units
TELEPORT_JUMP = 64.0   # further than this in a tic is a teleport
TELEPORT_NEAR = 48.0   # ...through a teleport line this near where she was
NEAR_SOUND = 128.0       # a sound this near is something she'll see anyway
EXPECTED_RADIUS = 128.0  # a sound this near a known mover's is that mover
FELT_TICS = 70          # unhurt on a floor this long: it doesn't hurt
SUIT_TICS = 60 * 35     # a radiation suit lasts a minute
TICS_SAME_SOUND = 4   # sounds this close together from the same way are one

# Floors that look like they'd hurt (liquids): nukage, lava, blood, the slimes.
HAZARD_FLATS = ("NUKAGE", "LAVA", "BLOOD", "SLIME0")

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
class Heard:
    """A sound Evil heard: roughly which way and how far, not what made it."""
    tic: int
    what: str             # "mover" (a floor or ceiling starting or stopping); later: monsters
    bearing: float        # degrees, anticlockwise from east, from where she was
    distance: float
    origin: tuple[float, float]  # where that puts it (an estimate)
    seen: bool = False    # what made it was in view anyway


@dataclass
class Knowledge:
    nav: Navigator
    width: int = VIEW_WIDTH
    # does she get hurt (no god mode): then a floor's harm is learned from her health, else
    # from standing on it
    mortal: bool = False
    mapped: set[int] = field(default_factory=set)       # linedefs seen
    sectors: set[int] = field(default_factory=set)      # sectors seen (either side of a mapped line)
    seen_heights: dict[int, tuple[float, float]] = field(default_factory=dict)  # when last seen
    seen_tic: dict[int, int] = field(default_factory=dict)  # sector -> last tic in view
    fired: list[Fired] = field(default_factory=list)
    learned: dict[int, set[int]] = field(default_factory=dict)  # trigger index -> sectors whose effect is known
    teleports: dict[int, tuple[float, float]] = field(default_factory=dict)  # used line -> where it took us
    visited: set[int] = field(default_factory=set)      # polys stood in
    heard: list[Heard] = field(default_factory=list)
    felt: dict[int, bool] = field(default_factory=dict)  # sector stood on -> did it hurt
    suit_until: int | None = None  # radiation suit on until this tic
    lives: int = 1                 # tries at the level so far
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
        self._standing: tuple[int, int] | None = None  # (sector, since tic) on its floor
        # where each sector's sounds come from: the middle of its bounding box (P_GroupLines)
        box: dict[int, list[float]] = {}
        for line in m.linedefs:
            for sec in (m.front_sector(line), m.back_sector(line)):
                if sec is None:
                    continue
                b = box.setdefault(sec, [math.inf, math.inf, -math.inf, -math.inf])
                for v in (m.vertexes[line.v1], m.vertexes[line.v2]):
                    b[0], b[1] = min(b[0], v.x), min(b[1], v.y)
                    b[2], b[3] = max(b[2], v.x), max(b[3], v.y)
        self._sound_origin = {sec: ((b[0] + b[2]) / 2, (b[1] + b[3]) / 2) for sec, b in box.items()}
        # lines that get drawn when looked at (not two-sided ones alike on both sides)
        def drawable(line) -> bool:
            f, b = m.front_sector(line), m.back_sector(line)
            if f is None or b is None:
                return True
            sf, sb = m.sectors[f], m.sectors[b]
            return (sf.floor, sf.ceiling, sf.floor_tex, sf.ceiling_tex, sf.light) != \
                (sb.floor, sb.ceiling, sb.floor_tex, sb.ceiling_tex, sb.light) \
                or m.sidedefs[line.front].middle not in ("-", "")
        self._drawable = [i for i, line in enumerate(m.linedefs) if drawable(line)]
        self._live: dict[int, tuple[float, float]] = {}  # every sector's heights last tic
        self._moving: set[int] = set()
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
            on_floor = obs.z is None or obs.z <= heights[poly.sector][0] + 1
            self._feel(poly.sector if on_floor else None, obs)
            in_view.add(poly.sector)
            if poly.sector not in self.sectors:
                self.sectors.add(poly.sector)
        last = self._last
        if last is not None and obs.suits is not None and last.suits is not None and obs.suits > last.suits:
            self.suit_until = obs.tic + SUIT_TICS
            self.events.append(f"tic {obs.tic}: radiation suit on")
        if last is not None:
            if math.dist((last.x, last.y), (obs.x, obs.y)) > TELEPORT_JUMP:
                self._teleported(last, obs)
            else:
                self._detect_fired(last, obs, controls, heights)
        for s in in_view:
            self.seen_tic[s] = obs.tic
            now = heights[s]
            if self.seen_heights.get(s, now) != now:
                self._saw_move(s, obs.tic)
            self.seen_heights[s] = now
        self._hear(obs, in_view)
        self._last = obs

    # --- hearing ------------------------------------------------------------------------

    def _hear(self, obs: Observation, in_view: set[int]) -> None:
        """The sounds of this tic, as the engine plays them: a floor or ceiling makes a sound
        from its sector's sound origin as it starts moving and as it stops, heard anywhere
        within HEAR_RANGE (Doom's sounds go through walls), with which way it came from (the
        stereo) and roughly how far (how loud)."""
        for s, h in obs.heights.items():
            prev = self._live.get(s)
            self._live[s] = h
            if prev is None:
                continue
            moving = prev != h
            if moving == (s in self._moving):
                continue
            if moving:
                self._moving.add(s)
            else:
                self._moving.discard(s)
            origin = self._sound_origin.get(s)
            if origin is None:
                continue
            d = math.dist((obs.x, obs.y), origin)
            if d > HEAR_RANGE:
                continue
            bearing = math.degrees(math.atan2(origin[1] - obs.y, origin[0] - obs.x))
            bearing = round(bearing / HEAR_BEARING) * HEAR_BEARING
            dist = max(HEAR_DISTANCE, round(d / HEAR_DISTANCE) * HEAR_DISTANCE)
            if any(hd.tic >= obs.tic - TICS_SAME_SOUND and hd.bearing == bearing and hd.distance == dist
                   for hd in self.heard[-8:]):
                continue  # (one sound, several sectors: a door's two halves, stairs)
            est = (obs.x + dist * math.cos(math.radians(bearing)),
                   obs.y + dist * math.sin(math.radians(bearing)))
            self.heard.append(Heard(obs.tic, "mover", bearing, dist, est, s in in_view))
            if s not in in_view:
                self.events.append(f"tic {obs.tic}: heard something move, bearing {bearing:.0f}, "
                                   f"about {dist:.0f} away")

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
                continue  # (going through is seen by where she ends up: _teleported)
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

    def _teleported(self, last: Observation, obs: Observation) -> None:
        """She moved further in a tic than anyone can walk: a teleporter, the one nearest
        where she was."""
        m = self.nav.map
        best = None
        for li, info in self._info.items():
            if info.category is not Category.TELEPORT or not info.players:
                continue
            line = m.linedefs[li]
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            d = _segment_point_distance((a.x, a.y), (b.x, b.y), (last.x, last.y))
            if best is None or d < best[0]:
                best = (d, li)
        if best is None or best[0] > TELEPORT_NEAR:
            self.events.append(f"tic {obs.tic}: moved to ({obs.x:.0f}, {obs.y:.0f}) all at once")
            return
        li = best[1]
        self._fire(li, obs.tic, "walk")
        if li not in self.teleports:
            self.events.append(f"tic {obs.tic}: teleport line {li} goes to ({obs.x:.0f}, {obs.y:.0f})")
        self.teleports[li] = (obs.x, obs.y)

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
        yet (exits are told apart: see exits)."""
        used = {self.mechanism(f.line) for f in self.fired}
        out = []
        for li in sorted(self.mapped):
            info = self._special(li)
            if info is not None and info.trigger in USED + SHOT and self.mechanism(li) not in used \
                    and info.category not in (Category.EXIT, Category.SECRET_EXIT):
                out.append(li)
        return out

    def mechanism(self, li: int) -> tuple:
        """What a line works, as far as she can tell: lines with the same special and tag
        work the same thing (the faces of one set of bars), as do door lines on the same
        door; using one is trying them all."""
        m = self.nav.map
        line = m.linedefs[li]
        info = self._special(li)
        if info is not None and info.trigger in (Trigger.D1, Trigger.DR):
            side = m.back_sector(line)
            return ("door", side if side is not None else li)
        if line.tag:
            return ("tag", line.special, line.tag)
        return ("line", li)

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
            if info.category is not Category.TELEPORT or not info.players:
                continue  # (not monster-only ones)
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
        """The last tic a line was first fired whose effect hasn't been seen at all, if any
        (setting it off again, a walk-over line crossed time and again, tells nothing new)."""
        explained = {self.nav.triggers[i].line for i, s in self.learned.items() if s}
        first: dict[int, int] = {}
        for f in self.fired:
            first.setdefault(f.line, f.tic)
        tics = [tic for line, tic in first.items() if line not in explained]
        return max(tics) if tics else None

    def unseen_walls(self) -> dict[int, list[int]]:
        """Known sectors not closed on the automap yet: sector -> its lines that would be
        drawn if looked at (walls, steps, windows) but haven't been."""
        m = self.nav.map
        out: dict[int, list[int]] = {}
        for li in self._drawable:
            if li in self.mapped:
                continue
            line = m.linedefs[li]
            for s in (m.front_sector(line), m.back_sector(line)):
                if s is not None and s in self.sectors:
                    out.setdefault(s, []).append(li)
        return out

    def restart(self) -> None:
        """The level started again (after dying): what she knows of the map stays (what she
        has seen, what lines do, which floors hurt, where teleporters go, where she's walked,
        what she's tried); what was so this time goes. The level is as at the start again,
        so the heights she knows go back to those (where she saw them so: else as seen)."""
        m = self.nav.map
        for s in self.sectors:
            self.seen_heights[s] = (float(m.sectors[s].floor), float(m.sectors[s].ceiling))
        self.suit_until = None
        self._last = None
        self._standing = None
        self._live = {}
        self._moving = set()
        self.lives += 1
        self.events.append(f"--- life {self.lives}: the level from the start, knowing what she knew")

    def _feel(self, s: int | None, obs: Observation) -> None:
        """What standing on sector s's floor (None: not on a floor) tells her about it: it
        hurts if her health drops there, it doesn't if she's stood there a while unhurt
        (floors hurt every 32 tics). Without being hurt at all (god mode): it's as the map
        has it, found out by standing on it."""
        if s is None:
            self._standing = None
            return
        if not self.mortal:
            if s not in self.felt:
                self._learn_floor(s, bool(self.nav.damage[s]), obs.tic)
            return
        last = self._last
        if self._standing is None or self._standing[0] != s:
            self._standing = (s, obs.tic)
        if last is not None and obs.health is not None and last.health is not None \
                and obs.health < last.health:
            self._learn_floor(s, True, obs.tic)
        elif obs.tic - self._standing[1] > FELT_TICS and self.felt.get(s) is not False \
                and not self.protected(obs.tic):
            self._learn_floor(s, False, obs.tic)

    def _learn_floor(self, s: int, hurts: bool, tic: int) -> None:
        if self.felt.get(s) == hurts:
            return
        self.felt[s] = hurts
        if hurts != self.looks_damaging(s):
            self.events.append(f"tic {tic}: sector {s} " + ("hurts after all" if hurts else "doesn't hurt after all"))

    def protected(self, tic: int) -> bool:
        """Is she wearing a radiation suit (one picked up in the last minute)?"""
        return self.suit_until is not None and tic < self.suit_until

    def looks_damaging(self, s: int) -> bool:
        """A floor that looks like it would hurt (its flat is a liquid)."""
        return self.nav.map.sectors[s].floor_tex.upper().startswith(HAZARD_FLATS)

    def damaging(self, s: int) -> bool:
        """Does she take sector s's floor to hurt: as found out by standing on it, else by
        its look."""
        felt = self.felt.get(s)
        return self.looks_damaging(s) if felt is None else felt

    def expected(self, h: Heard) -> bool:
        """A sound from something she knows moves (a door or lift she's learned: the one she
        just went through closing behind her), or from right beside her: nothing new."""
        if h.distance <= NEAR_SOUND:
            return True
        movers = {s for _, sectors in self.known_triggers() for s in sectors}
        return any(math.dist(self._sound_origin[s], h.origin) <= EXPECTED_RADIUS
                   for s in movers if s in self._sound_origin)

    def known_polys(self) -> set[int]:
        return {p.index for p in self.nav.mesh.polys if p is not None and p.sector in self.sectors}

    def known_triggers(self) -> list[tuple[TriggerLine, set[int]]]:
        """Triggers whose effect on some sectors she has learned, with those sectors; also the
        other lines of the same mechanism (the switch on the other side of the same door) as
        far as they're mapped, which she takes to do the same."""
        by_mechanism: dict[tuple, set[int]] = {}
        for i, sectors in self.learned.items():
            t = self.nav.triggers[i]
            if sectors and t.line >= 0:
                by_mechanism.setdefault(self.mechanism(t.line), set()).update(sectors)
        out = []
        for i, t in enumerate(self.nav.triggers):
            if t.line < 0:
                continue
            if i in self.learned and self.learned[i]:
                out.append((t, self.learned[i]))
            elif t.line in self.mapped and (same := by_mechanism.get(self.mechanism(t.line))):
                if same & set(t.effects):
                    out.append((t, same & set(t.effects)))
        return out

    def summary(self) -> str:
        m = self.nav.map
        return (f"{len(self.mapped)}/{len(m.linedefs)} lines, {len(self.sectors)}/{len(m.sectors)} "
                f"sectors, {len(self.fired)} fired, {sum(map(len, self.learned.values()))} effects "
                f"learned, {len(self.interactables())} untried switches/doors, "
                f"{len(self.exits())} exits seen")


def _segment_point_distance(a, b, p) -> float:
    dx, dy = b[0] - a[0], b[1] - a[1]
    l2 = dx * dx + dy * dy
    t = 0.0 if l2 == 0 else max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2))
    return math.dist(p, (a[0] + t * dx, a[1] + t * dy))


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
