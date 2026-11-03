"""Height-aware passability, key/switch progression and A* over the nav mesh.

Heights are kept separate from topology. A `HeightStates` maps each sector to the set of
(floor, ceiling) states it can be in:
- At runtime: one state per sector, the live heights reported by the engine.
- For planning: the initial state plus every state a reachable mover can put it in. This
  "ever possible" union deliberately over-approximates (a raised floor doesn't erase the
  lowered one), which is what we want for "can this area be reached at all in this key state".

A portal from sector A to sector B is passable if some pair of states allows it, using
vanilla player movement rules: step up <= 24, any drop, opening >= 56 (player height).
"""
import heapq
import itertools
import math
import time
from collections import defaultdict
from dataclasses import dataclass, field

from evildoom.wad.mapdata import (ML_BLOCKING, ML_BLOCKMONSTERS, ML_BLOCKPLAYERS, NO_SIDEDEF,
                                  MapData)
from evildoom.wad.specials import (Category, Key, LineSpecial, Lock, Trigger, classify_line,
                                   classify_sector)
from evildoom.wad.things import SOLID_DECORATIONS

from .movers import Heights, MoverModel
from .navmesh import NavMesh, Point, _cells

PLAYER_HEIGHT = 56
MAX_STEP = 24
PLAYER_RADIUS = 16
ITEM_RADIUS = 20
# Paths keep this far from portal ends (wall corners). DOOM collides with a square box of
# half-width 16, whose corner is ~22.6 from its centre, so 16 snags on diagonal approaches.
PATH_CLEARANCE = 24
WAYPOINT_CLEARANCE = 18  # minimum distance from a waypoint to any wall line
USE_REACH = 48  # use traces 64 from the player centre; keep a margin
TELEPORT_DEST = 14
RUN_THRUST = 1.5625   # forwardmove 50 << 11, per tic (vanilla)
FRICTION = 0.90625    # ORIG_FRICTION, applied on the ground only
GRAVITY = 1.0
MAX_RUNUP = 256
JUMP_ANGLES = (0, -30, 30, -45, 45, -60, 60)  # degrees off the edge normal
JUMPS_PER_TARGET = 4  # candidates kept per (takeoff poly, landing sector)
DAMAGE_COST = 4.0  # path cost multiplier on damaging floors
TELEPORTERS_FOR_PLAYERS = {39, 97, 174, 195, 207, 208, 209, 210}
TELEPORTERS_FOR_MONSTERS = {39, 97, 125, 126, 207, 208}  # walk-over ones monsters trigger

BARON, CYBERDEMON, SPIDER, MANCUBUS, ARACHNOTRON, KEEN = 3003, 16, 7, 67, 68, 72
MONSTER_HEIGHTS = {BARON: 64, CYBERDEMON: 110, SPIDER: 100, MANCUBUS: 64, ARACHNOTRON: 64,
                   KEEN: 72}
MONSTER_NAMES = {BARON: "Barons of Hell", CYBERDEMON: "Cyberdemons",
                 SPIDER: "Spider Masterminds", MANCUBUS: "Mancubi",
                 ARACHNOTRON: "Arachnotrons", KEEN: "Commander Keens"}
# map -> (monster type, tag, special whose effect it mimics (0 = none), category)
BOSS_RULES = {
    "E1M8": [(BARON, 666, 38, Category.FLOOR)],       # lower floor to lowest
    "E2M8": [(CYBERDEMON, 0, 0, Category.EXIT)],
    "E3M8": [(SPIDER, 0, 0, Category.EXIT)],
    "E4M6": [(CYBERDEMON, 666, 109, Category.DOOR)],  # blazing door open
    "E4M8": [(SPIDER, 666, 38, Category.FLOOR)],
    "MAP07": [(MANCUBUS, 666, 38, Category.FLOOR),
              (ARACHNOTRON, 667, 30, Category.FLOOR)],  # raise to shortest texture
}

HeightStates = dict[int, set[Heights]]
Doors = dict[int, tuple[set[Heights], set[int]]]
KeyItems = dict[Key, list[tuple[int, Point]]]  # key -> (poly, point) pickup spots


def can_cross(a: Heights, b: Heights) -> bool:
    (fa, ca), (fb, cb) = a, b
    return fb - fa <= MAX_STEP and min(ca, cb) - max(fa, fb) >= PLAYER_HEIGHT


@dataclass
class TriggerLine:
    line: int
    info: LineSpecial
    sectors: list[int]
    effects: dict[int, list[Heights]]
    activators: set[int]        # polys from which it can be activated (use/shoot/cross)


@dataclass
class Teleport:
    line: int
    src_polys: set[int]
    dest_poly: int
    dest: Point


@dataclass
class Jump(Teleport):
    """A run-off jump: sprint from `runup` through `takeoff` (a ledge edge) and land at
    `dest`. Treated as a one-way edge like a teleport (`line` is -1)."""
    runup: Point = (0.0, 0.0)
    takeoff: Point = (0.0, 0.0)
    speed: float = 0.0
    requires: frozenset = frozenset()  # (sector, heights) moved states the jump depends on

    def usable(self, states: "HeightStates") -> bool:
        return all(h in states[s] for s, h in self.requires)

    @property
    def key(self) -> str:
        """Stable identifier (for caching engine verification results)."""
        r, t, d = self.runup, self.takeoff, self.dest
        return f"{r[0]:.0f},{r[1]:.0f}>{t[0]:.0f},{t[1]:.0f}>{d[0]:.0f},{d[1]:.0f}"


@dataclass
class Stage:
    keys: frozenset[Key]
    reached: set[int]
    fired: list[TriggerLine]
    keys_available: set[Key]
    exits: list[TriggerLine]


@dataclass
class Path:
    polys: list[int]
    waypoints: list[Point | None]   # None marks a teleport (position jump)
    cost: float
    steps: list[str] = field(default_factory=list)   # human-readable notes (teleports, drops)
    jumps: list["Jump"] = field(default_factory=list)  # run-off jumps, in order


@dataclass
class Leg:
    """One leg of a planned route: walk `path`, then activate `trigger` or pick up `key`
    (neither = the final leg, ending at the goal)."""
    trigger: TriggerLine | None
    path: Path
    key: Key | None = None


@dataclass
class Route:
    legs: list[Leg]
    states: HeightStates            # predicted heights once the route is done
    keys: frozenset = frozenset()   # keys held at the end
    goal: Point | None = None       # which goal it reached


class Navigator:
    def __init__(self, m: MapData, mesh: NavMesh | None = None):
        self.map = m
        self.mesh = mesh or NavMesh(m)
        self.movers = MoverModel(m, self.mesh.neighbors_of_sector)
        self.damage = {s.index: classify_sector(s.special).damage for s in m.sectors}
        self.triggers = self._find_triggers()
        self.teleports = self._find_teleports()
        self.tele_from: dict[int, list[Teleport]] = {}
        for tp in self.teleports:
            for p in tp.src_polys:
                self.tele_from.setdefault(p, []).append(tp)
        self.portal_fits = self._portal_widths()
        self.jumps: list[Jump] = []
        self.set_jumps(self._find_jumps())

    def set_jumps(self, jumps: list["Jump"]) -> None:
        """Replace the jump edges (e.g. with only the engine-verified ones)."""
        for p, edges in self.tele_from.items():
            self.tele_from[p] = [e for e in edges if not isinstance(e, Jump)]
        self.jumps = list(jumps)
        for j in self.jumps:
            for p in j.src_polys:
                self.tele_from.setdefault(p, []).append(j)

    # --- run-off jumps --------------------------------------------------------------------

    def _static_wall_grid(self) -> dict[tuple[int, int], list[tuple[Point, Point]]]:
        if not hasattr(self, "_swgrid"):
            self._swgrid = defaultdict(list)
            for edge, _line in set(self._static_walls()):
                for cell in _cells(*edge[0], *edge[1]):
                    self._swgrid[cell].append(edge)
        return self._swgrid

    def _box_hits_static_wall(self, x: float, y: float) -> bool:
        """Does the player's box at (x, y) straddle a static wall or solid thing edge?"""
        r = PLAYER_RADIUS - 0.1
        grid = self._static_wall_grid()
        edges = {e for cell in _cells(x - r, y - r, x + r, y + r) for e in grid.get(cell, ())}
        return any(_box_hits_segment(x, y, r, e) for e in edges)

    def _crosses_static_wall(self, a: Point, b: Point) -> bool:
        grid = self._static_wall_grid()
        edges = {e for cell in _cells(*a, *b) for e in grid.get(cell, ())}
        return any(_segments_cross(a, b, p, q) for p, q in edges)

    def _find_jumps(self) -> list[Jump]:
        """Run-off jumps across gaps. For every ledge edge (a drop of more than MAX_STEP), take
        the straight run-up available behind it, the speed that run-up produces (vanilla thrust
        and friction), and simulate the flight: no air friction, gravity 1/tic^2, blocked by
        walls, by ledge faces more than MAX_STEP above the player, and by low ceilings. Kept if
        it lands higher than the floor right below the edge (plain drops are already edges).

        Sectors a trigger can move are allowed: each is given one of its possible heights
        (starting heights first) that makes the jump work, and any moved heights it needed are
        recorded in `Jump.requires`; the edge is only usable once the world has them."""
        mesh = self.mesh
        options: dict[int, list[Heights]] = {s: [h] for s, h in enumerate(self.movers.initial)}
        for t in self.triggers:
            for s, hs in t.effects.items():
                options[s] += [h for h in hs if h not in options[s]]
        out = []
        seen: dict[tuple[int, int], int] = {}
        for portal in mesh.portals:
            if not self.portal_fits[portal.index]:
                continue
            if portal.linedef is not None and \
                    self.map.linedefs[portal.linedef].flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                continue
            for a, b in ((portal.a, portal.b), (portal.b, portal.a)):
                sa, sb = mesh.polys[a].sector, mesh.polys[b].sector
                (x1, y1), (x2, y2) = portal.p1, portal.p2
                length = max(portal.width, 1)
                nx, ny = -(y2 - y1) / length, (x2 - x1) / length
                ca = mesh.polys[a].center
                if nx * (portal.mid[0] - ca[0]) + ny * (portal.mid[1] - ca[1]) < 0:
                    nx, ny = -nx, -ny
                # Players aim: try angles off the edge, not just straight out. Keep one jump
                # per (takeoff poly, landing sector).
                for ha in options[sa]:
                    for hb in options[sb]:
                        if ha[0] - hb[0] <= MAX_STEP:
                            continue
                        # fixed angles, plus aiming straight at nearby sectors high enough
                        # to land on (an island in a pit isn't at any fixed angle)
                        for ang in JUMP_ANGLES + tuple(self._aim_angles(
                                portal.mid, (nx, ny), hb[0] + MAX_STEP, options)):
                            c, s_ = math.cos(math.radians(ang)), math.sin(math.radians(ang))
                            d = (nx * c - ny * s_, nx * s_ + ny * c)
                            # every 16 units along the edge: gaps between bars are narrow
                            steps = max(1, int(portal.width // 16))
                            ts = sorted({0.5} | {(i + 0.5) / steps for i in range(steps)},
                                        key=lambda t: abs(t - 0.5))
                            for t in ts:
                                takeoff = (x1 + (x2 - x1) * t, y1 + (y2 - y1) * t)
                                jump = self._try_jump(a, takeoff, d, {sa: ha, sb: hb}, options,
                                                      portal.linedef)
                                if jump is None:
                                    continue
                                # keep a few varied candidates per (takeoff poly, landing
                                # sector): engine verification drops the near misses
                                key = (a, mesh.polys[jump.dest_poly].sector)
                                if seen.get(key, 0) < JUMPS_PER_TARGET:
                                    seen[key] = seen.get(key, 0) + 1
                                    out.append(jump)
                                break
        return out

    def _aim_angles(self, origin: Point, n: Point, min_floor: float,
                    options: dict[int, list[Heights]], max_dist: float = 400) -> list[float]:
        """Angles (degrees off normal n, within +-75) toward the centres of sectors within
        max_dist whose floor could be landed on (above min_floor)."""
        if not hasattr(self, "_sector_centres"):
            acc: dict[int, list[Point]] = {}
            for poly in self.mesh.polys:
                if poly is not None:
                    acc.setdefault(poly.sector, []).append(poly.center)
            self._sector_centres = {s: (sum(p[0] for p in ps) / len(ps),
                                        sum(p[1] for p in ps) / len(ps)) for s, ps in acc.items()}
        out = []
        base = math.atan2(n[1], n[0])
        for s, (cx, cy) in self._sector_centres.items():
            d = math.dist(origin, (cx, cy))
            if d > max_dist or d < 16 or max(h[0] for h in options[s]) <= min_floor:
                continue
            ang = math.degrees(math.atan2(cy - origin[1], cx - origin[0]) - base)
            ang = (ang + 180) % 360 - 180
            if abs(ang) <= 75 and all(abs(ang - a) > 5 for a in JUMP_ANGLES + tuple(out)):
                out.append(round(ang))
        return out

    def _try_jump(self, src: int, takeoff: Point, n: Point, chosen: dict[int, Heights],
                  options: dict[int, list[Heights]], edge: int | None) -> Jump | None:
        m, mesh, init = self.map, self.mesh, self.movers.initial
        chosen = dict(chosen)
        floor = chosen[mesh.polys[src].sector][0]
        below = next(h for s, h in chosen.items() if s != mesh.polys[src].sector)[0] \
            if len(chosen) > 1 else floor

        def pick(sector: int, ok) -> Heights | None:
            if sector in chosen:
                return chosen[sector] if ok(chosen[sector]) else None
            for h in options[sector]:
                if ok(h):
                    chosen[sector] = h
                    return h
            return None

        def crossed(a: Point, b: Point):
            cand = {li for cell in _cells(*a, *b) for li in mesh._line_grid.get(cell, ())}
            for li in cand:
                line = m.linedefs[li]
                v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                if _segments_cross(a, b, (v1.x, v1.y), (v2.x, v2.y)):
                    yield line

        # Run-up: back off along -n while every line we'd cross running forward is crossable
        # (in the running direction) and the box stays clear of walls.
        runup, back = 0.0, takeoff
        r = PLAYER_RADIUS - 1
        box = [(-r, -r), (0, -r), (r, -r), (r, 0), (r, r), (0, r), (-r, r), (-r, 0)]
        while runup < MAX_RUNUP:
            nxt = (takeoff[0] - n[0] * (runup + 8), takeoff[1] - n[1] * (runup + 8))
            here = mesh.poly_at(*nxt)
            # static walls only: doors and other movers are handled through their heights below
            if here is None or self._box_hits_static_wall(*nxt):
                break
            # The whole 32x32 box has to fit: DOOM stands you on the highest floor under it, so
            # a ledge more than a step up anywhere under the box blocks the run (moved heights
            # may be chosen, e.g. a lowered fence, and become requirements).
            hc = pick(here.sector, lambda h: h[1] - h[0] >= PLAYER_HEIGHT)  # room to stand
            if hc is None:
                break
            ok = True
            for dx, dy in box:
                q = mesh.poly_at(nxt[0] + dx, nxt[1] + dy)
                if q is None or pick(q.sector, lambda h, hc=hc: h[0] <= hc[0] + MAX_STEP and
                                     h[1] - max(h[0], hc[0]) >= PLAYER_HEIGHT) is None:
                    ok = False
                    break
            if not ok:
                break
            for line in crossed(nxt, back):
                f, b = m.front_sector(line), m.back_sector(line)
                if b is None or line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                    ok = False
                    break
                # which side is behind (farther from the takeoff)?
                v1 = m.vertexes[line.v1]
                v2 = m.vertexes[line.v2]
                behind_front = ((v2.x - v1.x) * (nxt[1] - v1.y)
                                - (v2.y - v1.y) * (nxt[0] - v1.x)) < 0
                far, near = (f, b) if behind_front else (b, f)
                hf = pick(far, lambda h: True)
                hn = pick(near, lambda h, hf=hf: can_cross(hf, h))
                if hn is None:
                    ok = False
                    break
            if not ok:
                break
            runup, back = runup + 8, nxt
        if runup < 16:
            return None
        v = d = 0.0
        while d < runup:
            v = v * FRICTION + RUN_THRUST
            d += v

        # Flight. DOOM stands the player on the highest floor anywhere under their 32x32 box,
        # so they keep ledge height until the box has fully left the ledge, and they land (or
        # step up) as soon as the front of the box reaches something no more than a step above
        # them. Per tic, like P_XYMovement then P_ZMovement: move, check the floors/ceilings
        # under the box, then fall (first falling tic -2, then -1 per tic).
        src_sector = mesh.polys[src].sector
        x, y = takeoff
        z, vz = float(floor), 0.0
        airborne = False
        for _ in range(120):
            nx_, ny_ = x + n[0] * v, y + n[1] * v
            for line in crossed((x, y), (nx_, ny_)):
                if line.back == NO_SIDEDEF or line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                    return None
            if self._box_hits_static_wall(nx_, ny_):
                return None  # the box clips a wall/pillar even if the centre line misses it
            under: dict[int, Heights] = {}
            for dx, dy in box + [(0, 0)]:
                q = mesh.poly_at(nx_ + dx, ny_ + dy)
                if q is None:
                    return None
                if q.sector not in under:
                    h = pick(q.sector, lambda h, z=z: h[0] <= z + MAX_STEP
                             and h[1] - max(z, h[0]) >= PLAYER_HEIGHT)
                    if h is None:
                        return None  # a wall/ledge face or a low ceiling in the way
                    under[q.sector] = h
            fmax = max(h[0] for h in under.values())
            x, y = nx_, ny_
            z += vz
            if z <= fmax:
                z, vz = fmax, 0.0
                ahead = mesh.poly_at(x + n[0] * (PLAYER_RADIUS - 1), y + n[1] * (PLAYER_RADIUS - 1))
                if ahead is not None and ahead.sector != src_sector \
                        and under.get(ahead.sector, (None,))[0] == fmax:
                    if fmax <= below + MAX_STEP:
                        return None  # just dropped to the floor below: an ordinary edge
                    on = (x + n[0] * (PLAYER_RADIUS + 8), y + n[1] * (PLAYER_RADIUS + 8))
                    land = mesh.poly_at(*on)
                    if land is None or land.sector != ahead.sector:
                        land, on = ahead, (x + n[0] * (PLAYER_RADIUS - 1),
                                           y + n[1] * (PLAYER_RADIUS - 1))
                    requires = frozenset((s, hh) for s, hh in chosen.items() if hh != init[s])
                    return Jump(-1, {src}, land.index, on, back, takeoff, v, requires)
                if airborne:
                    # Came down supported by something under the box that isn't ahead (e.g. a
                    # narrow bridge under the back half): that's a landing on it all the same.
                    support = [(dx, dy) for dx, dy in box + [(0, 0)]
                               if (q := mesh.poly_at(x + dx, y + dy)) is not None
                               and q.sector != src_sector and under[q.sector][0] == fmax]
                    if not support or fmax <= below + MAX_STEP:
                        return None
                    dx, dy = min(support, key=lambda d: abs(d[0]) + abs(d[1]))
                    on = (x + dx * 0.8, y + dy * 0.8)
                    land = mesh.poly_at(*on)
                    if land is None or under.get(land.sector, (None,))[0] != fmax:
                        return None
                    requires = frozenset((s, hh) for s, hh in chosen.items() if hh != init[s])
                    return Jump(-1, {src}, land.index, on, back, takeoff, v, requires)
            else:
                airborne = True
                vz = -2 * GRAVITY if vz == 0 else vz - GRAVITY
        return None

    # --- player width -------------------------------------------------------------------

    def _static_walls(self) -> list[tuple[tuple[Point, Point], int | None]]:
        """(segment, linedef or None) for everything that blocks the player and never changes:
        one-sided, impassable-flagged, or a height difference that can't be crossed either way
        between sectors no mover touches. Plus the box edges of solid decorations."""
        m, init = self.map, self.movers.initial
        moving = {s for t in self.triggers for s in t.effects}
        out = []
        for line in m.linedefs:
            f, b = m.front_sector(line), m.back_sector(line)
            wall = b is None or f is None or line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) \
                or (f not in moving and b not in moving
                    and not (can_cross(init[f], init[b]) or can_cross(init[b], init[f])))
            if wall:
                v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                out.append((((v1.x, v1.y), (v2.x, v2.y)), line.index))
        out += [(e, None) for edges in self._obstacle_grid().values() for e in set(edges)]
        return out

    def _portal_widths(self) -> list[bool]:
        """Whether the player's 32x32 box fits through each portal somewhere along it.

        Mirrors PIT_CheckLine: a box is blocked by a wall if it overlaps the wall's bounding box
        and has corners on both sides of the wall's line. Narrow sound channels and
        shoot-through slits fail this even though their heights are fine.
        """
        grid: dict[tuple[int, int], list] = defaultdict(list)
        for edge, line in set(self._static_walls()):
            for cell in _cells(*edge[0], *edge[1]):
                grid[cell].append((edge, line))
        r = PLAYER_RADIUS - 0.1  # touching a wall is allowed
        fits = []
        for portal in self.mesh.portals:
            (x1, y1), (x2, y2) = portal.p1, portal.p2
            # The portal's own line is judged by flags/heights in passable(), not here.
            walls = {e for cell in _cells(min(x1, x2) - r, min(y1, y2) - r,
                                          max(x1, x2) + r, max(y1, y2) + r)
                     for e, line in grid.get(cell, ())
                     if line is None or line != portal.linedef}
            n = max(1, int(portal.width // 4))
            fits.append(any(
                not any(_box_hits_segment(x1 + (x2 - x1) * i / n, y1 + (y2 - y1) * i / n, r, w)
                        for w in walls)
                for i in range(n + 1)))
        return fits

    # --- static analysis ----------------------------------------------------------------

    def initial_states(self) -> HeightStates:
        return {i: {h} for i, h in enumerate(self.movers.initial)}

    def _affected(self, line_index: int, info: LineSpecial) -> list[int]:
        line = self.map.linedefs[line_index]
        if info.trigger in (Trigger.D1, Trigger.DR):
            back = self.map.back_sector(line)
            return [back] if back is not None else []
        return [s.index for s in self.map.tagged_sectors(line.tag)]

    def _find_triggers(self) -> list[TriggerLine]:
        mesh = self.mesh
        touching_front: dict[int, set[int]] = {}
        touching_any: dict[int, set[int]] = {}
        crossing: dict[int, set[int]] = {}
        for poly in mesh.polys:
            if poly is None:
                continue
            for li in poly.lines_front:
                touching_front.setdefault(li, set()).add(poly.index)
                touching_any.setdefault(li, set()).add(poly.index)
            for li in poly.lines_back:
                touching_any.setdefault(li, set()).add(poly.index)
        for portal in mesh.portals:
            if portal.linedef is not None:
                crossing.setdefault(portal.linedef, set()).update((portal.a, portal.b))

        out = []
        for line in self.map.linedefs:
            info = classify_line(line.special)
            if info is None or not info.players or info.trigger is Trigger.NONE:
                continue
            if info.category in (Category.LIGHT, Category.OTHER, Category.TELEPORT):
                continue
            match info.trigger:
                case Trigger.S1 | Trigger.SR | Trigger.D1 | Trigger.DR:
                    activators = touching_front.get(line.index, set()) \
                        | self._polys_in_use_range(line)
                case Trigger.W1 | Trigger.WR:
                    activators = crossing.get(line.index, set()) \
                        | touching_any.get(line.index, set())
                case _:  # gun triggers: shoot from anywhere with a view of the line
                    activators = touching_any.get(line.index, set())
                    a, b = self.map.vertexes[line.v1], self.map.vertexes[line.v2]
                    for t in (0.25, 0.5, 0.75):
                        point = (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
                        activators = activators | self._line_of_sight_polys(
                            point, ignore=line.index)
            sectors = self._affected(line.index, info)
            out.append(TriggerLine(line.index, info, sectors,
                                   self.movers.effects(line.special, sectors), activators))
        self.triggers = out  # boss triggers need the line triggers (monster-usable doors)
        return out + self._boss_triggers()

    def _polys_in_use_range(self, line) -> set[int]:
        """Polys that come within USE_REACH of the line on its front side. Use traces 64 units
        from the player's centre, so a switch at the back of a shallow alcove (too shallow to
        stand in) is pressed from the room in front of it."""
        m, mesh = self.map, self.mesh
        a, b = m.vertexes[line.v1], m.vertexes[line.v2]
        a, b = (a.x, a.y), (b.x, b.y)
        out = set()
        for poly in mesh.polys:
            if poly is None:
                continue
            xs = [v[0] for v in poly.vertices]
            ys = [v[1] for v in poly.vertices]
            if min(xs) > max(a[0], b[0]) + USE_REACH or max(xs) < min(a[0], b[0]) - USE_REACH \
                    or min(ys) > max(a[1], b[1]) + USE_REACH \
                    or max(ys) < min(a[1], b[1]) - USE_REACH:
                continue
            # front side = right of v1->v2; the poly's centre must be in front of the line
            if (b[0] - a[0]) * (poly.center[1] - a[1]) - (b[1] - a[1]) * (poly.center[0] - a[0]) >= 0:
                continue
            if any(math.dist(v, _closest_point(v, a, b)) <= USE_REACH for v in poly.vertices) \
                    or math.dist(poly.center, _closest_point(poly.center, a, b)) <= USE_REACH:
                out.add(poly.index)
        return out

    def _boss_triggers(self) -> list[TriggerLine]:
        """Vanilla A_BossDeath / A_KeenDie: killing every monster of a type fires a tagged
        action on certain maps. Modelled as pseudo-triggers (negative line ids) activated from
        the monsters' positions (they have to be reachable to be killed)."""
        m, mesh = self.map, self.mesh
        name = m.name
        rules = list(BOSS_RULES.get(name, []))
        rules.append((KEEN, 666, 2, Category.DOOR))  # Keens open tag 666 on any map
        out = []
        for i, (monster, tag, special, category) in enumerate(rules):
            homes = [(t.x, t.y) for t in m.things if t.type == monster and t.in_skill(3)]
            if not homes:
                continue
            # Killable from anywhere the boss can walk or teleport to (monster closets with
            # teleporters), or anywhere with line of sight to where it starts.
            polys = set()
            for x, y in homes:
                home = mesh.poly_at(x, y)
                if home is not None:
                    polys |= self._monster_reach(home.index, MONSTER_HEIGHTS[monster])
                polys |= self._line_of_sight_polys((x, y))
            sectors = [s.index for s in m.tagged_sectors(tag)] if tag else []
            info = LineSpecial(special, Trigger.NONE, category,
                               f"kill all {MONSTER_NAMES[monster]}")
            effects = self.movers.effects(special, sectors) if sectors else {}
            out.append(TriggerLine(-1 - i, info, sectors, effects, polys))
        return out

    def _monster_reach(self, start: int, height: int) -> set[int]:
        """Polys a monster can get to from `start`: steps of at most 24 *both* ways (monsters
        don't walk off ledges), its own height of headroom, not through impassable or
        block-monster lines, plus teleporters monsters use. Doors/lifts monsters can trigger
        are assumed open/lowered."""
        m, mesh = self.map, self.mesh
        states = self.initial_states()
        for t in self.triggers:
            if t.info.monsters:
                for s, hs in t.effects.items():
                    states[s].update(hs)
        tele: dict[int, list[int]] = {}
        dests = {}
        for t in m.things:
            if t.type == TELEPORT_DEST and (p := mesh.poly_at(t.x, t.y)):
                dests.setdefault(p.sector, p.index)
        for line in m.linedefs:
            if line.special in TELEPORTERS_FOR_MONSTERS:
                dest = next((dests[s.index] for s in m.tagged_sectors(line.tag)
                             if s.index in dests), None)
                if dest is not None:
                    for p in mesh.polys:
                        if p and line.index in p.lines_front:
                            tele.setdefault(p.index, []).append(dest)

        def ok(a: Heights, b: Heights) -> bool:
            return abs(b[0] - a[0]) <= MAX_STEP and min(a[1], b[1]) - max(a[0], b[0]) >= height

        seen, todo = {start}, [start]
        while todo:
            poly = todo.pop()
            nxt = list(tele.get(poly, []))
            for pi in mesh.polys[poly].portals:
                portal = mesh.portals[pi]
                o = portal.other(poly)
                if portal.linedef is not None and \
                        m.linedefs[portal.linedef].flags & (ML_BLOCKING | ML_BLOCKMONSTERS):
                    continue
                sa, sb = mesh.polys[poly].sector, mesh.polys[o].sector
                if sa == sb or any(ok(ha, hb) for ha in states[sa] for hb in states[sb]):
                    nxt.append(o)
            for n in nxt:
                if n not in seen:
                    seen.add(n)
                    todo.append(n)
        return seen

    def _sight_blockers(self) -> dict[tuple[int, int], list[tuple[Point, Point, int]]]:
        """Grid of lines that block sight/hitscan: one-sided, or closed at the start."""
        if not hasattr(self, "_sightgrid"):
            m, init = self.map, self.movers.initial
            self._sightgrid = defaultdict(list)
            for line in m.linedefs:
                f, b = m.front_sector(line), m.back_sector(line)
                if b is None or f is None or \
                        min(init[f][1], init[b][1]) - max(init[f][0], init[b][0]) <= 0:
                    v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                    seg = ((v1.x, v1.y), (v2.x, v2.y), line.index)
                    for cell in _cells(v1.x, v1.y, v2.x, v2.y):
                        self._sightgrid[cell].append(seg)
        return self._sightgrid

    def _clear_sight(self, a: Point, b: Point, ignore: int | None = None) -> bool:
        grid = self._sight_blockers()
        segs = {s for cell in _cells(*a, *b) for s in grid.get(cell, ())}
        return not any(li != ignore and _segments_cross(a, b, p, q) for p, q, li in segs)

    def _line_of_sight_polys(self, target: Point, max_range: float = 2048,
                             ignore: int | None = None) -> set[int]:
        """Polys whose centre has an unobstructed view of `target` within hitscan range.
        Sight is blocked by one-sided lines and by sectors closed at the start (not ledges),
        except line `ignore` (e.g. the shootable line itself)."""
        return {poly.index for poly in self.mesh.polys
                if poly is not None and math.dist(poly.center, target) <= max_range
                and self._clear_sight(poly.center, target, ignore)}

    def _find_teleports(self) -> list[Teleport]:
        m, mesh = self.map, self.mesh
        dests: dict[int, tuple[int, Point]] = {}   # sector -> first teleport destination
        for t in m.things:
            if t.type == TELEPORT_DEST and (poly := mesh.poly_at(t.x, t.y)):
                dests.setdefault(poly.sector, (poly.index, (t.x, t.y)))
        out = []
        for line in m.linedefs:
            if line.special not in TELEPORTERS_FOR_PLAYERS:
                continue
            for sector in m.tagged_sectors(line.tag):
                if sector.index in dests:
                    poly, point = dests[sector.index]
                    srcs = {p.index for p in mesh.polys if p and line.index in p.lines_front}
                    out.append(Teleport(line.index, srcs, poly, point))
                    break
        return out

    # --- passability --------------------------------------------------------------------

    def local_doors(self, keys: set[Key]) -> Doors:
        """Doors the bot can open itself by pressing use (DR/D1 lines), given its keys:
        door sector -> (open heights, polys the door can be used from)."""
        out: Doors = {}
        for t in self.triggers:
            if t.info.trigger not in (Trigger.DR, Trigger.D1) or not t.info.lock.satisfied(keys):
                continue
            for sector, hs in t.effects.items():
                opened, users = out.setdefault(sector, (set(), set()))
                opened.update(hs)
                users.update(t.activators)
        return out

    def passable(self, portal_index: int, frm: int, states: HeightStates,
                 doors: Doors | None = None) -> bool:
        """Can the player cross this portal from poly `frm`? With `doors`, a door sector also
        counts as open when entered from a poly where the door can be used, or when already
        standing in it."""
        mesh = self.mesh
        portal = mesh.portals[portal_index]
        to = portal.other(frm)
        if portal.linedef is not None:
            line = self.map.linedefs[portal.linedef]
            if line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                return False
            # Walking over a teleport line from its front side teleports you: that's the
            # teleport edge (tele_from), not a walk. (Back to front is an ordinary crossing.)
            if line.special in TELEPORTERS_FOR_PLAYERS and \
                    portal.linedef in mesh.polys[frm].lines_front:
                return False
        if not self.portal_fits[portal_index]:
            return False
        sa, sb = mesh.polys[frm].sector, mesh.polys[to].sector
        if sa == sb:
            return True
        hs_a, hs_b = states[sa], states[sb]
        if doors:
            if sa in doors:
                hs_a = hs_a | doors[sa][0]
            if sb in doors and frm in doors[sb][1]:
                hs_b = hs_b | doors[sb][0]
        return any(can_cross(ha, hb) for ha in hs_a for hb in hs_b)

    def reachable(self, start: int, states: HeightStates,
                  doors: Doors | None = None) -> set[int]:
        seen = {start}
        todo = [start]
        while todo:
            poly = todo.pop()
            nxt = [self.mesh.portals[pi].other(poly) for pi in self.mesh.polys[poly].portals
                   if self.passable(pi, poly, states, doors)]
            nxt += [tp.dest_poly for tp in self.tele_from.get(poly, [])
                    if not isinstance(tp, Jump) or tp.usable(states)]
            for n in nxt:
                if n not in seen:
                    seen.add(n)
                    todo.append(n)
        return seen

    def pickup_spots(self, x: float, y: float, states: HeightStates) -> list[tuple[int, Point]]:
        """Polys (and a point in each) from which an item at (x, y) can be picked up.

        P_TouchSpecialThing only needs the (square) bounding boxes to overlap — player half-width
        16 + item half-width 20 on each axis — and the item to be between 8 below and 56 above
        the player's feet. So items on pedestals too high to step onto can be grabbed from the
        floor next to them, even across an impassable railing if the item is close enough to it.
        """
        mesh = self.mesh
        home = mesh.poly_at(x, y)
        if home is None:
            return []
        spots = [(home.index, (x, y))]
        item_floors = {f for f, _ in states[home.sector]}
        edge = [(pi, poly.index) for poly in mesh.polys if poly and poly.sector == home.sector
                for pi in poly.portals]
        reach = PLAYER_RADIUS + ITEM_RADIUS
        for pi, own in edge:
            portal = mesh.portals[pi]
            other = mesh.polys[portal.other(own)]
            if other.sector == home.sector:
                continue
            near = _closest_on_segment((x, y), portal)
            if not any(-8 <= fi - fo <= PLAYER_HEIGHT
                       for fi in item_floors for fo, _ in states[other.sector]):
                continue
            # Stand far enough back that the box fits on the other side of the line (17 units
            # if the line blocks, else a little past it), then check the boxes overlap.
            back = PLAYER_RADIUS + 1 if portal.linedef is not None and \
                self.map.linedefs[portal.linedef].flags & (ML_BLOCKING | ML_BLOCKPLAYERS) else 8
            cx, cy = other.center
            d = max(math.dist(near, (cx, cy)), 1)
            spot = (near[0] + (cx - near[0]) * back / d, near[1] + (cy - near[1]) * back / d)
            if abs(spot[0] - x) < reach and abs(spot[1] - y) < reach \
                    and mesh.poly_at(*spot) is other:
                spots.append((other.index, spot))
        return spots

    # --- progression --------------------------------------------------------------------

    def progression(self, start: int) -> list[Stage]:
        """Alternate: fire every usable trigger until nothing changes, then pick up every
        reachable key. Each key set is one stage."""
        states = self.initial_states()
        keys: set[Key] = set()
        fired: set[int] = set()
        key_things = [t for t in self.map.things
                      if t.type in Key._value2member_map_ and t.in_skill(3)]
        stages = []
        while True:
            fired_now = []
            while True:
                reached = self.reachable(start, states)
                usable = [t for t in self.triggers
                          if t.line not in fired and t.activators & reached
                          and t.info.lock.satisfied(keys)]
                if not usable:
                    break
                for t in usable:
                    fired.add(t.line)
                    fired_now.append(t)
                    for sector, hs in t.effects.items():
                        states[sector].update(hs)
            available = {Key(t.type) for t in key_things
                         if any(p in reached for p, _ in self.pickup_spots(t.x, t.y, states))}
            exits = [t for t in self.triggers if t.info.category in
                     (Category.EXIT, Category.SECRET_EXIT) and t.line in fired]
            stages.append(Stage(frozenset(keys), reached, fired_now, available, exits))
            if available <= keys:
                break
            keys |= available
        self.final_states = states
        return stages

    # --- pathfinding --------------------------------------------------------------------

    def find_path(self, start: Point, goal: Point, states: HeightStates,
                  doors: Doors | None = None) -> Path | None:
        """A* over polys. Edge cost is the distance between the points where the path enters
        successive polys (closest point on each portal), weighted up on damaging floors."""
        mesh = self.mesh
        sp, gp = mesh.poly_at(*start), mesh.poly_at(*goal)
        if sp is None or gp is None:
            return None
        use_heuristic = not self.teleports  # teleports break admissibility

        def h(poly: int) -> float:
            return math.dist(mesh.polys[poly].center, goal) if use_heuristic else 0.0

        # Nodes are polys; the point we entered a poly at is tracked for edge costs.
        best = {sp.index: 0.0}
        entry = {sp.index: start}
        came: dict[int, tuple[int, int | Teleport]] = {}
        heap = [(h(sp.index), 0.0, sp.index)]
        while heap:
            _, g, poly = heapq.heappop(heap)
            if poly == gp.index:
                break
            if g > best[poly]:
                continue
            here = entry[poly]
            mult = DAMAGE_COST if self.damage[mesh.polys[poly].sector] else 1.0
            moves: list[tuple[int, Point, int | Teleport]] = []
            for pi in mesh.polys[poly].portals:
                if self.passable(pi, poly, states, doors):
                    portal = mesh.portals[pi]
                    moves.append((portal.other(poly), _closest_on_segment(here, portal), pi))
            for tp in self.tele_from.get(poly, []):
                if not isinstance(tp, Jump) or tp.usable(states):
                    moves.append((tp.dest_poly, tp.dest, tp))
            for nxt, point, via in moves:
                cost = g + math.dist(here, point) * mult
                if nxt not in best or cost < best[nxt]:
                    best[nxt] = cost
                    entry[nxt] = point
                    came[nxt] = (poly, via)
                    heapq.heappush(heap, (cost + h(nxt), cost, nxt))
        if gp.index not in best:
            return None

        chain: list[tuple[int, int | Teleport | None]] = [(gp.index, None)]
        while chain[-1][0] != sp.index:
            prev, via = came[chain[-1][0]]
            chain[-1] = (chain[-1][0], via)
            chain.append((prev, None))
        chain.reverse()
        polys = [p for p, _ in chain]
        cost = best[gp.index] + math.dist(entry[gp.index], goal)
        return Path(polys, self._smooth(start, goal, chain), cost,
                    self._annotate(chain, states, doors),
                    [via for _, via in chain if isinstance(via, Jump)])

    def plan_route(self, start: Point, goal: Point, keys: set[Key] = frozenset(),
                   states: HeightStates | None = None, max_expansions: int = 300,
                   max_seconds: float = 5.0) -> Route | None:
        """Plan a route to one goal, using remote triggers (switches, walk-over lines) as
        needed but not collecting keys. See `plan` for how the search works."""
        return self.plan(start, [goal], keys, states, None, max_expansions, max_seconds)

    def key_items(self, states: HeightStates | None = None) -> KeyItems:
        """Every key on the map with the (poly, point) spots it can be picked up from.
        By default spots are judged against every height a sector can ever take, since keys
        on pedestals may only become reachable once something moves."""
        if states is None:
            states = self.initial_states()
            for t in self.triggers:
                for s, hs in t.effects.items():
                    states[s].update(hs)
        out: KeyItems = {}
        for t in self.map.things:
            if t.type in Key._value2member_map_ and t.in_skill(3):
                out.setdefault(Key(t.type), []).extend(self.pickup_spots(t.x, t.y, states))
        return out

    def plan(self, start: Point, goals: list[Point], keys: set[Key] = frozenset(),
             states: HeightStates | None = None, key_items: KeyItems | None = None,
             max_expansions: int = 300, max_seconds: float = 5.0,
             trace: list | None = None) -> Route | None:
        """Plan a route to any of `goals`, using remote triggers and (if `key_items` is given)
        picking up keys along the way, in whatever order works.

        Best-first search over (what the world looks like, keys held, where we're standing).
        Actions are: use a reachable trigger that would change some sector's possible heights,
        or pick up a reachable key. Each is judged from where we'd stand afterwards (some sit
        past one-way drops and strand the player). Order: nodes that can reach a goal, then
        ones that made progress (new area or a new key), then the most reachable area.
        Triggers that change the world without opening anything yet are still tried, just
        later: some only help in combination, or are needed later to get back out.

        Use-doors are handled inside find_path; stay-open (D1) doors are also actions, so that
        once opened they count as open from both sides. Search states are deduplicated by the
        heights added and keys held, not by which line produced them, and lines with identical
        effects are only tried once per node (closest first). `states` defaults to the map's
        initial heights; at runtime pass the live heights.
        """
        mesh = self.mesh
        key_items = key_items or {}
        door_cache: dict[frozenset, Doors] = {}

        def doors_for(ks: frozenset) -> Doors:
            if ks not in door_cache:
                door_cache[ks] = self.local_doors(set(ks))
            return door_cache[ks]

        goal_polys: dict[int, Point] = {}
        for g in goals:
            if (gp := mesh.poly_at(*g)) is not None:
                goal_polys.setdefault(gp.index, g)
        start_poly = mesh.poly_at(*start)
        if not goal_polys or start_poly is None:
            return None
        actions = [t for t in self.triggers if t.info.trigger is not Trigger.DR and t.effects]

        @dataclass
        class Node:
            pos: Point
            states: HeightStates
            added: frozenset            # (sector, heights) pairs added beyond the start
            keys: frozenset
            legs: list[Leg]
            reach: set[int]
            steps: int = 0
            pending: tuple | None = None  # (prev pos, prev states, prev keys, trigger, key)

        keys0 = frozenset(keys)
        states0 = {s: set(h) for s, h in (states or self.initial_states()).items()}
        bound = self._upper_bound(start_poly.index, states0, keys0, key_items, actions)
        if not goal_polys.keys() & bound:
            return None  # unreachable even if everything reachable were used: don't search
        # Point the search at the critical chain: actions the relaxed plan depends on first.
        rel_t, rel_k = self._relaxed_needs(start_poly.index, set(goal_polys), states0, keys0,
                                           key_items, actions)
        counter = itertools.count()
        root = Node(start, states0, frozenset(), keys0, [],
                    self.reachable(start_poly.index, states0, doors_for(keys0)))
        heap = [((False, False, False, 0, 0), next(counter), root)]
        seen: set[tuple] = set()
        expansions = 0
        deadline = time.perf_counter() + max_seconds
        while heap and expansions < max_expansions and time.perf_counter() < deadline:
            _, _, node = heapq.heappop(heap)
            sig = (node.added, node.keys, frozenset(node.reach))
            if sig in seen:
                continue
            seen.add(sig)
            if node.pending:  # path the leg that got us here, now that we've picked this node
                prev_pos, prev_states, prev_keys, t, key = node.pending
                path = self.find_path(prev_pos, node.pos, prev_states, doors_for(prev_keys))
                if path is None:
                    continue
                node.legs = node.legs + [Leg(t, path, key)]
            expansions += 1
            if trace is not None:  # debugging: what the search actually explores
                last = node.legs[-1] if node.legs else None
                trace.append((len(node.reach), sorted(k.name for k in node.keys),
                              (f"line {last.trigger.line}" if last and last.trigger else
                               f"key {last.key.name}" if last and last.key else "start")))
            doors = doors_for(node.keys)
            for gp in sorted(goal_polys.keys() & node.reach,
                             key=lambda p: math.dist(node.pos, goal_polys[p])):
                path = self.find_path(node.pos, goal_polys[gp], node.states, doors)
                if path:
                    return Route(node.legs + [Leg(None, path)], node.states, node.keys,
                                 goal_polys[gp])

            def push(target: Point, trial: HeightStates, added: frozenset, ks: frozenset,
                     t: TriggerLine | None, key: Key | None) -> None:
                target_poly = mesh.poly_at(*target)
                if target_poly is None:
                    return
                after = self.reachable(target_poly.index, trial, doors_for(ks))
                child = Node(target, trial, added, ks, node.legs, after, node.steps + 1,
                             (node.pos, node.states, node.keys, t, key))
                progress = bool(after - node.reach) or key is not None
                relevant = (t is not None and t.line in rel_t) or (key is not None and key in rel_k)
                priority = (not goal_polys.keys() & after, not relevant, not progress,
                            -len(after), child.steps)
                heapq.heappush(heap, (priority, next(counter), child))

            # Triggers: one candidate per distinct world change, the closest line producing it.
            options: dict[frozenset, tuple[float, TriggerLine, Point]] = {}
            for t in actions:
                if not t.activators & node.reach or not t.info.lock.satisfied(set(node.keys)):
                    continue
                new = frozenset((s, h) for s, hs in t.effects.items() for h in hs
                                if h not in node.states[s])
                if not new:
                    continue  # already fired, or a duplicate of something that was
                target = self._activation_point(t, node.pos, node.reach)
                d = math.dist(node.pos, target)
                if new not in options or d < options[new][0]:
                    options[new] = (d, t, target)
            for new, (_, t, target) in options.items():
                trial = {s: set(h) for s, h in node.states.items()}
                for s, h in new:
                    trial[s].add(h)
                # Where we stand to use it matters (a door pressed from the pit below it is
                # useless): try the default spot and a few others it can be used from, keep
                # the one leaving the most reachable.
                by_sector: dict[int, int] = {}  # one candidate per sector it's usable from
                for p in sorted(t.activators & node.reach,
                                key=lambda p: math.dist(mesh.polys[p].center, target)):
                    by_sector.setdefault(mesh.polys[p].sector, p)
                spots = [target] + [mesh.polys[p].center for p in list(by_sector.values())[:8]]
                doors_k = doors_for(node.keys)
                best = max(spots, key=lambda sp: len(self.reachable(
                    mesh.poly_at(*sp).index, trial, doors_k)) if mesh.poly_at(*sp) else -1)
                push(best, trial, node.added | new, node.keys, t, None)
            # Keys: for each key not yet held, the reachable pickup spot that leaves the most
            # reachable afterwards (some spots are slivers you can't walk on from).
            for key, spots in key_items.items():
                if key in node.keys:
                    continue
                here = [(p, pt) for p, pt in spots if p in node.reach]
                if here:
                    ks = node.keys | {key}
                    _, spot = max(here, key=lambda s: (
                        len(self.reachable(s[0], node.states, doors_for(ks))),
                        -math.dist(node.pos, s[1])))
                    push(spot, node.states, node.added, ks, None, key)
        return None

    def _upper_bound(self, start: int, states: HeightStates, keys: frozenset,
                     key_items: KeyItems, triggers: list[TriggerLine]) -> set[int]:
        """Everything reachable if every trigger we can get to were used and every key we can
        get to picked up (in any order)."""
        return self._saturate(start, states, keys, key_items, triggers)[0]

    def _saturate(self, start: int, states: HeightStates, keys: frozenset, key_items: KeyItems,
                  triggers: list[TriggerLine]) -> tuple[set[int], HeightStates, set[Key]]:
        """Fire every reachable trigger and take every reachable key until nothing changes.
        Returns (reach, heights, keys) at the fixed point."""
        states = {s: set(h) for s, h in states.items()}
        keys = set(keys)
        fired: set[int] = set()
        while True:
            reach = self.reachable(start, states, self.local_doors(keys))
            new = [t for t in triggers if t.line not in fired and t.activators & reach
                   and t.info.lock.satisfied(keys)]
            new_keys = {k for k, spots in key_items.items()
                        if k not in keys and any(p in reach for p, _ in spots)}
            if not new and not new_keys:
                return reach, states, keys
            keys |= new_keys
            for t in new:
                fired.add(t.line)
                for sector, hs in t.effects.items():
                    states[sector].update(hs)

    def _relaxed_needs(self, start: int, goals: set[int], states0: HeightStates,
                       keys0: frozenset, key_items: KeyItems,
                       triggers: list[TriggerLine]) -> tuple[set[int], set[Key]]:
        """Relaxed plan (ordering ignored): which triggers and keys a route to `goals` depends
        on. Path to the goal under the fully saturated world; every crossing that needs a moved
        height names the trigger(s) producing it, every locked door names a key; then the same
        for those triggers' activators and those keys' spots, until nothing new is needed.
        Used to point the search at the level's critical chain."""
        reach_all, states_all, keys_all = self._saturate(start, states0, keys0, key_items,
                                                         triggers)
        doors_all = self.local_doors(keys_all)
        producers: dict[tuple[int, Heights], list[TriggerLine]] = {}
        for t in triggers:
            if t.activators & reach_all:
                for s, hs in t.effects.items():
                    for h in hs:
                        producers.setdefault((s, h), []).append(t)
        mesh = self.mesh
        need_t: set[int] = set()
        need_k: set[Key] = set()
        queue = [goals]
        seen_goals: set[frozenset] = set()
        while queue and len(seen_goals) < 200:
            target = frozenset(queue.pop())
            if target in seen_goals or not target & reach_all:
                continue
            seen_goals.add(target)
            # BFS with parents under the saturated world
            parent: dict[int, tuple[int, object]] = {start: (-1, None)}
            todo, end = [start], None
            while todo:
                p = todo.pop(0)
                if p in target:
                    end = p
                    break
                moves = [(mesh.portals[pi].other(p), pi) for pi in mesh.polys[p].portals
                         if self.passable(pi, p, states_all, doors_all)]
                moves += [(tp.dest_poly, tp) for tp in self.tele_from.get(p, [])
                          if not isinstance(tp, Jump) or tp.usable(states_all)]
                for n, via in moves:
                    if n not in parent:
                        parent[n] = (p, via)
                        todo.append(n)
            if end is None:
                continue
            needed_states: set[tuple[int, Heights]] = set()
            p = end
            while parent[p][0] != -1:
                prev, via = parent[p]
                if isinstance(via, Jump):
                    needed_states |= set(via.requires)
                elif isinstance(via, int) and not self.passable(via, prev, states0):
                    sa, sb = mesh.polys[prev].sector, mesh.polys[p].sector
                    pairs = [(ha, hb) for ha in states_all[sa] for hb in states_all[sb]
                             if can_cross(ha, hb)]
                    if pairs:
                        ha, hb = min(pairs, key=lambda pr: (pr[0] not in states0[sa])
                                     + (pr[1] not in states0[sb]))
                        needed_states |= {(sa, ha)} - {(sa, h) for h in states0[sa]}
                        needed_states |= {(sb, hb)} - {(sb, h) for h in states0[sb]}
                    elif sb in doors_all:  # a use-door: which key does it need?
                        for t in self.triggers:
                            if sb in t.sectors and t.info.lock is not Lock.NONE:
                                k = next((k for k in key_items
                                          if t.info.lock.satisfied({k})), None)
                                if k is not None and k not in need_k:
                                    need_k.add(k)
                                    queue.append({sp for sp, _ in key_items[k]})
                p = prev
            for sh in needed_states:
                for t in producers.get(sh, [])[:1]:
                    if t.line not in need_t:
                        need_t.add(t.line)
                        queue.append(set(t.activators))
                        if t.info.lock is not Lock.NONE:
                            k = next((k for k in key_items if t.info.lock.satisfied({k})), None)
                            if k is not None and k not in need_k:
                                need_k.add(k)
                                queue.append({sp for sp, _ in key_items[k]})
        return need_t, need_k

    def _activation_point(self, t: TriggerLine, pos: Point, reach: set[int]) -> Point:
        """Where to stand to use a switch, or where to walk to so a walk-over line is crossed."""
        m, mesh = self.map, self.mesh
        if t.line < 0:  # boss death: go to where the monsters are
            return mesh.polys[min(t.activators & reach or t.activators)].center
        line = m.linedefs[t.line]
        a, b = m.vertexes[line.v1], m.vertexes[line.v2]
        length = max(math.dist((a.x, a.y), (b.x, b.y)), 1)
        mx, my = (a.x + b.x) / 2, (a.y + b.y) / 2
        nx, ny = (b.y - a.y) / length, -(b.x - a.x) / length  # points to the front side
        front = (mx + nx * 20, my + ny * 20)
        back = (mx - nx * 20, my - ny * 20)
        ok = [p for p in (front, back)
              if (poly := mesh.poly_at(*p)) and poly.index in reach]
        if t.info.trigger in (Trigger.W1, Trigger.WR) and ok:
            return max(ok, key=lambda p: math.dist(p, pos))  # far side: cross the line
        if t.info.trigger in (Trigger.G1, Trigger.GR):
            # anywhere with a view: the reachable shooting spot closest to where we are
            spots = [mesh.polys[p].center for p in t.activators & reach]
            if spots:
                return min(spots, key=lambda c: math.dist(c, pos))
        if front in ok:
            return front
        return mesh.polys[min(t.activators & reach)].center

    def _smooth(self, start: Point, goal: Point, chain) -> list[Point | None]:
        """String-pull through portals with the funnel algorithm, restarting at teleports.
        A None in the result marks a teleport jump between two waypoints."""
        mesh = self.mesh
        points: list[Point | None] = []
        seg_start, portals = start, []
        for i in range(1, len(chain)):
            prev_poly = chain[i - 1][0]
            poly, via = chain[i]
            if isinstance(via, Jump):  # continuous: run-up, edge, landing; no teleport marker
                points += funnel(seg_start, via.runup, portals) + [via.takeoff]
                seg_start, portals = via.dest, []
                continue
            if isinstance(via, Teleport):
                # Cross this exact line squarely: adjacent teleport lines often have different
                # destinations (E4M6's lift edge has four).
                mid = _line_mid(self.map, via.line)
                line = self.map.linedefs[via.line]
                a, b = self.map.vertexes[line.v1], self.map.vertexes[line.v2]
                length = max(math.dist((a.x, a.y), (b.x, b.y)), 1)
                approach = (mid[0] + (b.y - a.y) / length * 24, mid[1] - (b.x - a.x) / length * 24)
                points += funnel(seg_start, approach, portals) + [mid]
                points.append(None)
                seg_start, portals = via.dest, []
                continue
            portal = mesh.portals[via]
            portals.append(_oriented(portal, mesh.polys[prev_poly].center,
                                     mesh.polys[poly].center))
        points += funnel(seg_start, goal, portals)
        # Ledge edges get wall-like clearance too, except the ones this path means to cross;
        # jump run-up/takeoff points are kept exactly where they are.
        crossed = {mesh.portals[via].linedef for _, via in chain
                   if isinstance(via, int) and mesh.portals[via].linedef is not None}
        pinned = {p for _, via in chain if isinstance(via, Jump) for p in (via.runup, via.takeoff)}
        out: list[Point | None] = []
        run: list[Point] = []
        for p in points + [None]:
            if p is None:
                if run:
                    out += self._push_from_corners(run, frozenset(crossed), pinned)
                    run = []
                out.append(None)
            else:
                run.append(p)
        return out[:-1]

    def _drop_lines(self) -> set[int]:
        """Two-sided lines that can only be crossed one way (drops > MAX_STEP) at initial
        heights: falling over one by accident can strand the player."""
        if not hasattr(self, "_drops"):
            m, init = self.map, self.movers.initial
            self._drops = set()
            for line in m.linedefs:
                f, b = m.front_sector(line), m.back_sector(line)
                if f is not None and b is not None and \
                        can_cross(init[f], init[b]) != can_cross(init[b], init[f]):
                    self._drops.add(line.index)
        return self._drops

    def _wall_lines(self) -> set[int]:
        """Lines the player can't cross in either direction at initial heights."""
        if not hasattr(self, "_walls"):
            m, init = self.map, self.movers.initial
            self._walls = set()
            for line in m.linedefs:
                f, b = m.front_sector(line), m.back_sector(line)
                if b is None or f is None or line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS) \
                        or not (can_cross(init[f], init[b]) or can_cross(init[b], init[f])):
                    self._walls.add(line.index)
        return self._walls

    def _push_from_corners(self, pts: list[Point], exempt: frozenset = frozenset(),
                           pinned: set[Point] = frozenset()) -> list[Point]:
        """The funnel only keeps clear of the ends of portals it crosses; other wall corners can
        still be grazed. Insert detour waypoints PATH_CLEARANCE away from any wall (or ledge
        edge not in `exempt`) vertex that a segment passes too close to, then nudge waypoints
        (except `pinned` ones) off walls and ledges."""
        m, mesh = self.map, self.mesh
        out = [pts[0]]

        def corners_near(a: Point, b: Point) -> set[Point]:
            found = set()
            for (x1, y1), (x2, y2) in self._walls_near(a, b, PATH_CLEARANCE, exempt):
                found.add((x1, y1))
                found.add((x2, y2))
            return found

        def segment(a: Point, b: Point, depth: int) -> None:
            worst = None
            if depth < 4:
                for v in corners_near(a, b):
                    q = _closest_point(v, a, b)
                    d = math.dist(v, q)
                    if d < PATH_CLEARANCE - 1 and math.dist(v, a) > 1 and math.dist(v, b) > 1 \
                            and (worst is None or d < worst[0]):
                        worst = (d, v, q)
            if worst:
                d, v, q = worst
                if d < 1e-6:  # passing straight through the vertex: step off sideways
                    length = max(math.dist(a, b), 1e-6)
                    ux, uy = -(b[1] - a[1]) / length, (b[0] - a[0]) / length
                else:
                    ux, uy = (q[0] - v[0]) / d, (q[1] - v[1]) / d
                w = (v[0] + ux * PATH_CLEARANCE, v[1] + uy * PATH_CLEARANCE)
                wp, qp = mesh.poly_at(*w), mesh.poly_at(*q)
                # Only convex corners snag; pushing off a concave one lands on the other wall.
                if wp and qp and wp.sector == qp.sector \
                        and math.dist(w, a) > 4 and math.dist(w, b) > 4 \
                        and self._wall_clearance(w, exempt) >= WAYPOINT_CLEARANCE:
                    segment(a, w, depth + 1)
                    segment(w, b, depth + 1)
                    return
            out.append(b)

        for b in pts[1:]:
            segment(out[-1], b, 0)
        # Nudge waypoints (not the start/goal) that still hug a wall or ledge away from it.
        for i in range(1, len(out) - 1):
            if out[i] not in pinned:
                out[i] = self._nudge_off_walls(out[i], exempt)
        return out

    def _obstacle_grid(self) -> dict[tuple[int, int], list[tuple[Point, Point]]]:
        """Edges of solid decorations' collision boxes (barrels, lamps, pillars...), which
        block movement like walls but aren't in the map's lines."""
        if not hasattr(self, "_obstacles"):
            self._obstacles = defaultdict(list)
            for t in self.map.things:
                r = SOLID_DECORATIONS.get(t.type)
                if r is None or not t.in_skill(3):
                    continue
                c = [(t.x - r, t.y - r), (t.x + r, t.y - r), (t.x + r, t.y + r), (t.x - r, t.y + r)]
                for a, b in zip(c, c[1:] + c[:1]):
                    for cell in _cells(*a, *b):
                        self._obstacles[cell].append((a, b))
        return self._obstacles

    def _walls_near(self, a: Point, b: Point, pad: float, exempt: frozenset | None = None):
        """Wall lines and solid-thing box edges near the segment a-b. With `exempt` given
        (path smoothing), ledge edges count as walls too, except the exempt lines."""
        m, mesh, obstacles = self.map, self.mesh, self._obstacle_grid()
        walls = self._wall_lines()
        if exempt is not None:
            walls = walls | (self._drop_lines() - exempt)
        seen: set = set()
        for cell in _cells(min(a[0], b[0]) - pad, min(a[1], b[1]) - pad,
                           max(a[0], b[0]) + pad, max(a[1], b[1]) + pad):
            for li in mesh._line_grid.get(cell, ()):
                if li in walls and li not in seen:
                    seen.add(li)
                    line = m.linedefs[li]
                    v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                    yield (v1.x, v1.y), (v2.x, v2.y)
            for edge in obstacles.get(cell, ()):
                if edge not in seen:
                    seen.add(edge)
                    yield edge

    def _wall_clearance(self, p: Point, exempt: frozenset | None = None) -> float:
        return min((math.dist(p, _closest_point(p, a, b))
                    for a, b in self._walls_near(p, p, PATH_CLEARANCE, exempt)),
                   default=math.inf)

    def _nudge_off_walls(self, p: Point, exempt: frozenset | None = None) -> Point:
        for _ in range(4):
            worst = None
            for a, b in self._walls_near(p, p, WAYPOINT_CLEARANCE, exempt):
                q = _closest_point(p, a, b)
                d = math.dist(p, q)
                if d < WAYPOINT_CLEARANCE and (worst is None or d < worst[0]):
                    worst = (d, q, a, b)
            if worst is None:
                return p
            d, q, a, b = worst
            if d > 1e-6:
                ux, uy = (p[0] - q[0]) / d, (p[1] - q[1]) / d
            else:  # on the line: step to the side the poly is on
                length = max(math.dist(a, b), 1e-6)
                ux, uy = -(b[1] - a[1]) / length, (b[0] - a[0]) / length
                if self.mesh.poly_at(p[0] + ux, p[1] + uy) is None:
                    ux, uy = -ux, -uy
            p = (q[0] + ux * WAYPOINT_CLEARANCE, q[1] + uy * WAYPOINT_CLEARANCE)
        return p

    def _annotate(self, chain, states: HeightStates, doors: Doors | None) -> list[str]:
        mesh = self.mesh
        notes = []
        for i in range(1, len(chain)):
            frm = chain[i - 1][0]
            poly, via = chain[i]
            if isinstance(via, Jump):
                notes.append(f"run-jump off ({via.takeoff[0]:.0f}, {via.takeoff[1]:.0f}) to "
                             f"({via.dest[0]:.0f}, {via.dest[1]:.0f})")
                continue
            if isinstance(via, Teleport):
                notes.append(f"teleport via line {via.line}")
                continue
            sa, sb = mesh.polys[frm].sector, mesh.polys[poly].sector
            if sa == sb:
                continue
            if doors and sb in doors and not self.passable(via, frm, states):
                notes.append(f"open door (sector {sb})")
            elif not self.passable(via, poly, states, doors):
                drop = min(f for f, _ in states[sa]) - min(f for f, _ in states[sb])
                if drop > MAX_STEP:
                    notes.append(f"one-way drop of {drop} into sector {sb}")
        return notes


def _line_mid(m: MapData, line: int) -> Point:
    l = m.linedefs[line]
    a, b = m.vertexes[l.v1], m.vertexes[l.v2]
    return (a.x + b.x) / 2, (a.y + b.y) / 2


def _segments_cross(p1: Point, p2: Point, q1: Point, q2: Point) -> bool:
    """Do segments p1-p2 and q1-q2 properly intersect?"""
    def side(a, b, c):
        return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    d1, d2 = side(q1, q2, p1), side(q1, q2, p2)
    d3, d4 = side(p1, p2, q1), side(p1, p2, q2)
    return (d1 > 0) != (d2 > 0) and (d3 > 0) != (d4 > 0) and d1 and d2 and d3 and d4


def _box_hits_segment(cx: float, cy: float, r: float, seg: tuple[Point, Point]) -> bool:
    """Does the axis-aligned box of half-width r at (cx, cy) straddle the segment?"""
    (ax, ay), (bx, by) = seg
    if max(ax, bx) <= cx - r or min(ax, bx) >= cx + r \
            or max(ay, by) <= cy - r or min(ay, by) >= cy + r:
        return False
    dx, dy = bx - ax, by - ay
    sides = [dx * (py - ay) - dy * (px - ax)
             for px, py in ((cx - r, cy - r), (cx + r, cy - r), (cx + r, cy + r), (cx - r, cy + r))]
    return min(sides) < 0 < max(sides)


def _closest_point(p: Point, a: Point, b: Point) -> Point:
    dx, dy = b[0] - a[0], b[1] - a[1]
    length2 = dx * dx + dy * dy
    if length2 < 1e-9:
        return a
    t = max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length2))
    return a[0] + t * dx, a[1] + t * dy


def _closest_on_segment(p: Point, portal) -> Point:
    """Closest point to p on the portal, kept PATH_CLEARANCE away from its ends if wide enough."""
    (x1, y1), (x2, y2) = portal.p1, portal.p2
    length = math.dist(portal.p1, portal.p2)
    if length <= 2 * PATH_CLEARANCE:
        return portal.mid
    t = ((p[0] - x1) * (x2 - x1) + (p[1] - y1) * (y2 - y1)) / (length * length)
    margin = PATH_CLEARANCE / length
    t = min(max(t, margin), 1 - margin)
    return x1 + t * (x2 - x1), y1 + t * (y2 - y1)


def _oriented(portal, from_center: Point, to_center: Point) -> tuple[Point, Point]:
    """Return (left, right) endpoints as seen walking from one poly to the next, shrunk by the
    player radius so the smoothed path doesn't clip corners."""
    p1, p2 = portal.p1, portal.p2
    dx, dy = to_center[0] - from_center[0], to_center[1] - from_center[1]
    if dx * (p1[1] - from_center[1]) - dy * (p1[0] - from_center[0]) > 0:
        left, right = p1, p2
    else:
        left, right = p2, p1
    length = math.dist(left, right)
    if length <= 2 * PATH_CLEARANCE:
        return portal.mid, portal.mid
    t = PATH_CLEARANCE / length
    shrink = lambda a, b: (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)  # noqa: E731
    return shrink(left, right), shrink(right, left)


def _tri(a: Point, b: Point, c: Point) -> float:
    """Twice the signed area of abc; > 0 when c is left of a->b."""
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def funnel(start: Point, goal: Point, portals: list[tuple[Point, Point]]) -> list[Point]:
    """Simple stupid funnel algorithm (Mononen). Portals are (left, right) pairs in travel
    order. Returns the taut path including start and goal."""
    portals = [(start, start)] + portals + [(goal, goal)]
    path = [start]
    apex = left = right = start
    apex_i = left_i = right_i = 0
    i = 1
    while i < len(portals):
        pl, pr = portals[i]
        # Tighten the right side
        if _tri(apex, right, pr) >= 0:
            if apex == right or _tri(apex, left, pr) < 0:
                right, right_i = pr, i
            else:
                path.append(left)
                apex, apex_i = left, left_i
                left = right = apex
                left_i = right_i = apex_i
                i = apex_i + 1
                continue
        # Tighten the left side
        if _tri(apex, left, pl) <= 0:
            if apex == left or _tri(apex, right, pl) > 0:
                left, left_i = pl, i
            else:
                path.append(right)
                apex, apex_i = right, right_i
                left = right = apex
                left_i = right_i = apex_i
                i = apex_i + 1
                continue
        i += 1
    if path[-1] != goal:
        path.append(goal)
    return path
