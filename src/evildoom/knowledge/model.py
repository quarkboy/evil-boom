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
EXPECTED_FOR = 8 * 35    # ...which she set off at most this many tics before (a door closing
#                          behind her, a lift coming back): later it moved by itself
TAKE_TICS = 35          # a once-only line set off moves nothing in this long...
BUSY_TICS = 200         # ...where it acts on what moved this lately (a door open, waiting to
#                          close): it didn't take (Doom leaves a switch on a busy sector unused)
FELT_TICS = 70          # unhurt on a floor this long: it doesn't hurt
FLOOR_PERIOD = 32       # a hurting floor hurts on every 32nd tic of the level's clock...
FLOOR_CONFIRM = 3 * FLOOR_PERIOD  # ...so hurt twice on it a multiple of that apart, this soon: it's the floor
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
    life: int = 1  # which try at the level (a one-shot line works again in the next)


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
    seen_polys: set[int] = field(default_factory=set)   # polys seen into (or in front of a mapped line)
    heard: list[Heard] = field(default_factory=list)
    felt: dict[int, bool] = field(default_factory=dict)  # sector stood on -> did it hurt
    missed_jumps: dict = field(default_factory=dict)  # run-jump key -> times it came down off its landing
    # sector -> does it hurt now, for floors seen moved by a line that changes their type
    # (this life: the level starting again puts them back)
    changed_floor: dict[int, bool] = field(default_factory=dict)
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
        self._hurt_on: tuple[int, int] | None = None   # (sector, tic) last hurt standing on it
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
        self._moved_at: dict[int, int] = {}    # sector -> tic it was last heard moving
        self._taking: list[tuple[Fired, set[int]]] = []  # once-only lines set off: did they take?
        # per linedef: the subsectors its segs bound (the floor in front of it)
        self._line_subsectors: dict[int, set[int]] = {}
        for ss, sub in enumerate(m.subsectors):
            for seg in m.segs[sub.first_seg:sub.first_seg + sub.seg_count]:
                self._line_subsectors.setdefault(seg.linedef, set()).add(ss)
        for i, line in enumerate(m.linedefs):
            if line.flags & ML_MAPPED:
                self._map_line(i)

    # --- seeing -------------------------------------------------------------------------

    def _see_subsector(self, ss: int) -> None:
        self.seen_polys.update(self.nav.mesh.subsector_polys(ss))

    def _map_line(self, li: int) -> None:
        m = self.nav.map
        self.mapped.add(li)
        for ss in self._line_subsectors.get(li, ()):
            self._see_subsector(ss)
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
        drawn, through, subsectors = self._sight.drawn(obs.x, obs.y, obs.angle, self.width, heights)
        for ss in subsectors:
            self._see_subsector(ss)
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
        self._took(obs.tic)
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
                self._moved_at[s] = obs.tic
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
                    hurts = self.nav.floor_after(t, s)
                    if hurts is not None and self.changed_floor.get(s) != hurts:
                        self.changed_floor[s] = hurts
                        self.events.append(f"tic {tic}: sector {s}'s floor "
                                           + ("turns to something that hurts" if hurts else "is safe now"))
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
            # shut is in the way first (only from its front side). A once-only line used this
            # life has no special any more: the trace goes past it (E2M7's switch behind a door
            # switch used before: pressed, it went off, and she never knew)
            end = (last.x + USE_RANGE * math.cos(a), last.y + USE_RANGE * math.sin(a))
            for _, li in self._lines_along((last.x, last.y), end):
                info = self._special(li)
                # (any line with a special stops it, a walk-over one too: E2M4's door behind
                # one; only one that's used goes off)
                if info is not None and not self._spent(li):
                    if info.trigger in USED and self._front(li, last.x, last.y):
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
        # the rest of its pad goes there too (no need to try every side of it)
        for other in self.teleport_pad(li):
            self.teleports[other] = (obs.x, obs.y)

    def teleport_pad(self, li: int) -> frozenset[int]:
        """Teleport line `li` and those joined to it end to end (sharing vertices, one after
        another) with the same tag, so the same destination: the sides of one pad."""
        m = self.nav.map
        tag = m.linedefs[li].tag
        same = [o for o, info in self._info.items()
                if info.category is Category.TELEPORT and info.players and m.linedefs[o].tag == tag]
        pad, todo = {li}, [li]
        while todo:
            line = m.linedefs[todo.pop()]
            ends = {line.v1, line.v2}
            for o in same:
                if o not in pad and ends & {m.linedefs[o].v1, m.linedefs[o].v2}:
                    pad.add(o)
                    todo.append(o)
        return frozenset(pad)

    def _sector_at(self, obs: Observation) -> int:
        poly = self.nav.mesh.poly_at(obs.x, obs.y)
        return poly.sector if poly is not None else 0

    def _special(self, li: int):
        return self._info.get(li)

    def _spent(self, li: int) -> bool:
        """A once-only line used this life (its special's gone: the game clears it)."""
        info = self._special(li)
        return info is not None and info.trigger in (Trigger.W1, Trigger.S1, Trigger.G1, Trigger.D1)             and any(f.line == li and f.life == self.lives for f in self.fired)

    def _fire(self, li: int, tic: int, how: str) -> None:
        info = self._special(li)
        if info.trigger in (Trigger.W1, Trigger.S1, Trigger.G1, Trigger.D1) and \
                any(f.line == li and f.life == self.lives for f in self.fired):
            return  # (once only: nothing happens again, this life; MAP08's trap went off again
            #        in the next and wasn't noticed: the planner took the line for spent)
        f = Fired(li, tic, how, self.lives)
        self.fired.append(f)
        self.events.append(f"tic {tic}: {how} line {li} ({info.description})")
        if info.trigger in (Trigger.W1, Trigger.S1, Trigger.G1, Trigger.D1):
            targets = self._targets(li)
            if any(tic - self._moved_at.get(t, -10**9) < BUSY_TICS for t in targets):
                self._taking.append((f, targets))

    def _targets(self, li: int) -> set[int]:
        """The sectors a line acts on, by the game's rule: those with its tag; a door line
        with none, the door behind it."""
        m = self.nav.map
        line = m.linedefs[li]
        if line.tag:
            return {i for i, sec in enumerate(m.sectors) if sec.tag == line.tag}
        back = m.back_sector(line)
        return {back} if back is not None else set()

    def _took(self, tic: int) -> None:
        """Once-only lines set off on sectors that were busy: if nothing's moved since, the
        game turned them down (a switch on a door still open from another stays unused:
        E3M4's door 152, opened by its door switch, then the switch to open it for good
        pressed while it waited; it closed, and the switch still worked)."""
        still = []
        for f, targets in self._taking:
            if any(self._moved_at.get(t, -1) > f.tic for t in targets):
                continue  # (it took)
            if tic - f.tic < TAKE_TICS:
                still.append((f, targets))
                continue
            if self._already_so(f, targets):
                continue  # (it took: what it moves was already where it puts it, so nothing moved)
            self.fired.remove(f)
            self.events.append(f"tic {tic}: line {f.line} didn't take (what it moves was busy): not used up")
        self._taking = still

    def _already_so(self, f: Fired, targets: set[int]) -> bool:
        """Was everything line f moves left where it is for good, by the last line she set off
        that moves it (a door opened to stay: then nothing's running on it, and f went off but
        had nothing to do: E3M7's walk-over line opening a door another had just opened for
        good)? Not if that was a door that closes again (still running on its timer: Doom turned
        f down, E3M4's) or she doesn't know what moved it. Only for a line with a tag: a door's
        own once-only line on a door already open stays unused (MAP28's, used later)."""
        if not self.nav.map.linedefs[f.line].tag:
            return False
        for t in targets:
            last = None
            for g in self.fired:
                if g is f or g.life != self.lives or g.tic > f.tic:
                    continue
                for ti in self._by_line.get(g.line, ()):
                    if t in self.nav.triggers[ti].effects and (last is None or g.tic >= last[0]):
                        last = (g.tic, self.nav.triggers[ti])
            if last is None or not last[1].permanent:
                return False
        return True

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
        """The map's things in sectors she has seen (at the skill played)."""
        out = []
        for t in self.nav.map.things:
            poly = self.nav.mesh.poly_at(t.x, t.y)
            if poly is not None and poly.sector in self.sectors and t.in_skill(self.nav.skill):
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
        self.changed_floor.clear()
        self._last = None
        self._standing = None
        self._hurt_on = None
        self._live = {}
        self._moving = set()
        self._moved_at = {}
        self._taking = []
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
            # (a floor hurts on the level's clock, every 32 tics: hurt again on the same floor
            # a multiple of that later, it's the floor; once alone may be a monster's shot,
            # which isn't the floor's doing: fights had her think floors hurt)
            prev = self._hurt_on
            if prev is not None and prev[0] == s and 0 < obs.tic - prev[1] <= FLOOR_CONFIRM \
                    and (obs.tic - prev[1]) % FLOOR_PERIOD == 0:
                self._learn_floor(s, True, obs.tic)
            self._hurt_on = (s, obs.tic)
        elif obs.tic - max(self._standing[1], self._hurt_on[1] if self._hurt_on and self._hurt_on[0] == s else 0) \
                > FELT_TICS and self.felt.get(s) is not False \
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
        if s in self.changed_floor:  # (seen changed since: MAP08's walkway gone to nukage)
            return self.changed_floor[s]
        felt = self.felt.get(s)
        return self.looks_damaging(s) if felt is None else felt

    def expected(self, h: Heard) -> bool:
        """A sound from something she knows moves and set off a moment before (the door she
        just went through closing behind her, a lift coming back), or from right beside her:
        nothing new. One she set off long before moving again is news: E1M6's door that shut
        on her opened again 30 s later, and she'd given up."""
        if h.distance <= NEAR_SOUND:
            return True
        fired_at: dict[int, int] = {}
        for f in self.fired:
            fired_at[f.line] = f.tic
        for t, sectors in self.known_triggers():
            tic = fired_at.get(t.line)
            if tic is None or not 0 <= h.tic - tic <= EXPECTED_FOR:
                continue
            if any(math.dist(self._sound_origin[s], h.origin) <= EXPECTED_RADIUS
                   for s in sectors if s in self._sound_origin):
                return True
        return False

    def known_polys(self) -> set[int]:
        """Where she can plan to go: the parts of the map she's seen into or stood in (not a
        whole room for seeing its door: MAP24's paths beelined across rooms not looked into)."""
        return self.seen_polys | self.visited

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
