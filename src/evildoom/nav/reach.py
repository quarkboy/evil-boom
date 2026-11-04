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
import hashlib
import heapq
import inspect
import itertools
import json
import math
import time
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

from evildoom.wad.mapdata import (ML_BLOCKING, ML_BLOCKMONSTERS, ML_BLOCKPLAYERS, NO_SIDEDEF,
                                  MapData)
from evildoom.wad.specials import (Category, Key, LineSpecial, Lock, Trigger, classify_line,
                                   classify_sector)
from evildoom.wad.things import SHOOTABLE_OBSTACLES, SOLID_DECORATIONS

from .movers import Heights, MoverModel
from .navmesh import NavMesh, Point, _cells

PLAYER_HEIGHT = 56
MAX_STEP = 24
PLAYER_RADIUS = 16
ITEM_RADIUS = 20
PICKUP_MARGIN = 2  # how far inside touching range a pickup spot must be
# Paths keep this far from portal ends (wall corners). DOOM collides with a square box of
# half-width 16, whose corner is ~22.6 from its centre, so 16 snags on diagonal approaches.
PATH_CLEARANCE = 24
WAYPOINT_CLEARANCE = 18  # minimum distance from a waypoint to any wall line
THING_CLEARANCE = 6  # extra room left round solid things when detouring past them
SHOT_HEIGHT = 36  # bullets leave the player at z + 28 + 8, and level: no aiming up or down
MIN_PORTAL = 4.0  # narrower portals are nodebuilder slivers, never the real way through
LEG_SPEED = 12.0  # units/tic assumed when timing legs against lifts and doors (full run ~16)
BOX_SLACK = 0.25  # the box must clear walls by this much: no one threads a gap exactly 32 wide
USE_REACH = 48  # use traces 64 from the player centre; keep a margin
TELEPORT_DEST = 14
RUN_THRUST = 1.5625   # forwardmove 50 << 11, per tic (vanilla)
FRICTION = 0.90625    # ORIG_FRICTION, applied on the ground only
GRAVITY = 1.0
MAX_RUNUP = 256
JUMP_ANGLES = (0, -30, 30, -45, 45, -60, 60)  # degrees off the edge normal
NAV_CACHE = Path(__file__).resolve().parents[3] / "out" / "navcache"
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


def apply_trigger(states: "HeightStates", t: "TriggerLine") -> "HeightStates":
    """The heights the world can take after `t` fires. Normally its targets are added to
    what's possible (lifts and timed doors come back; you can step onto a rising floor on its
    way up). A floor that drops for good is replaced, though: the old height is gone (MAP07
    can't be jumped from the lowered ledge)."""
    out = {s: set(h) for s, h in states.items()}
    for s, hs in t.effects.items():
        if t.permanent and max(h[0] for h in hs) < min(h[0] for h in out[s]):
            out[s] = set(hs)
        else:
            out[s].update(hs)
    return out


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

    @property
    def permanent(self) -> bool:
        """Its moves stay put (floors, stairs, doors that stay open), unlike lifts, crushers
        and doors that close again: once fired, the old heights are gone."""
        return self.info.category not in (Category.LIFT, Category.CRUSHER) \
            and "wait" not in self.info.description


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
    requires: frozenset = frozenset()  # (sector, heights) of movable sectors it depends on
    soft: bool = False  # engine-checked: sprint only to the edge, then coast (narrow landings)
    through: bool = False  # sprint straight across a strip too narrow to stop on, to `dest`

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
    squeezes: int = 0   # spots where the player's box may not fit (see find_path)


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
        # Solid decorations (pillars, lamps, trees) are cut out of the mesh, grown by the
        # player's half-width: the mesh is where the player's centre can be, so A* goes round
        # rows of them and never through gaps the box can't fit. They never move; moving
        # *sectors* are handled by heights. Barrels aren't cut: they can be shot away (a row of
        # them sealing a corridor in MAP23), so they stay soft obstacles that paths detour round.
        self.mesh = mesh or NavMesh(m, [
            (t.x, t.y, SOLID_DECORATIONS[t.type] + PLAYER_RADIUS) for t in m.things
            if t.type in SOLID_DECORATIONS and t.type not in SHOOTABLE_OBSTACLES
            and t.in_skill(3)])
        self.movers = MoverModel(m, self.mesh.neighbors_of_sector)
        self.damage = {s.index: classify_sector(s.special).damage for s in m.sectors}
        self.strict_fit = False  # True: find_path refuses paths the box (maybe) can't get through
        #                          (the planner instead prefers paths with fewer squeezes)
        # (switch line, poly) -> sectors that must be open for the use trace (activators_now)
        self.use_gates: dict[tuple[int, int], frozenset[int]] = {}
        # (gun line, poly) -> floors to stand on there for a level shot to hit it
        self.shot_floors: dict[tuple[int, int], tuple[float, float]] = {}
        self.triggers = self._find_triggers()
        self.teleports = self._find_teleports()
        self.tele_from: dict[int, list[Teleport]] = {}
        for tp in self.teleports:
            for p in tp.src_polys:
                self.tele_from.setdefault(p, []).append(tp)
        self.portal_fits = self._portal_widths()
        self.jumps: list[Jump] = []
        self.set_jumps(self._find_jumps_cached())

    def set_jumps(self, jumps: list["Jump"]) -> None:
        """Replace the jump edges (e.g. with only the engine-verified ones). Jumps that need
        a lift or timed door at a height it only passes through or waits at are left out:
        no stock level needs a jump off something that's about to move."""
        for p, edges in self.tele_from.items():
            self.tele_from[p] = [e for e in edges if not isinstance(e, Jump)]
        temporary = self._temporary_heights()
        self.jumps = [j for j in jumps if not any(r in temporary for r in j.requires)]
        for j in self.jumps:
            for p in j.src_polys:
                self.tele_from.setdefault(p, []).append(j)

    def _temporary_heights(self) -> set[tuple[int, Heights]]:
        """(sector, heights) that only movers which come back (lifts, timed doors, crushers)
        produce: the sector won't stay like that."""
        if not hasattr(self, "_temporary"):
            # (perpetual lifts keep cycling: both their heights come round again and again)
            lasting = {(s, h) for t in self.triggers
                       if t.permanent or "perpetual" in t.info.description
                       for s, hs in t.effects.items() for h in hs}
            lasting |= {(s, h) for s, h in enumerate(self.movers.initial)}
            self._temporary = {(s, h) for t in self.triggers if not t.permanent
                               for s, hs in t.effects.items() for h in hs} - lasting
        return self._temporary

    def edge_ok(self, tp: Teleport, states: HeightStates) -> bool:
        """Can this teleport/jump edge be taken with these heights? A walk-over teleport only
        fires if the player can actually step over its line (MAP08 has one in front of a
        closed niche); a jump needs the moved heights it was found with."""
        if isinstance(tp, Jump):
            return tp.usable(states)
        line = self.map.linedefs[tp.line]
        f, b = self.map.front_sector(line), self.map.back_sector(line)
        if f is None or b is None:
            return False
        return any(can_cross(fh, bh) for fh in states[f] for bh in states[b])

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

    def _find_jumps_cached(self) -> list[Jump]:
        """_find_jumps, from out/navcache when nothing it depends on has changed: the map, the
        mesh, the heights sectors can take, which portals the box fits, and the jump finder's
        own code and constants (most of building a Navigator otherwise)."""
        h = hashlib.sha256()
        m = self.map
        for part in (m.vertexes, m.linedefs, m.sidedefs, m.sectors, m.things,
                     [(p.sector, p.vertices) if p else None for p in self.mesh.polys],
                     [(q.a, q.b, q.p1, q.p2, q.linedef) for q in self.mesh.portals],
                     self.portal_fits, sorted({s: sorted(hs) for t in self.triggers
                                              for s, hs in t.effects.items()}.items()),
                     self.movers.initial,
                     (MAX_STEP, PLAYER_RADIUS, PLAYER_HEIGHT, RUN_THRUST, FRICTION, GRAVITY,
                      MAX_RUNUP, JUMPS_PER_TARGET, JUMP_ANGLES)):
            h.update(repr(part).encode())
        for fn in (Navigator._find_jumps, Navigator._try_jump, Navigator._aim_angles,
                   Navigator._static_walls, Navigator._static_wall_grid,
                   Navigator._box_hits_static_wall, _box_hits_segment, _segments_cross):
            h.update(inspect.getsource(fn).encode())
        path = NAV_CACHE / f"jumps_{h.hexdigest()[:24]}.json"
        if path.exists():
            return [Jump(d["line"], set(d["src"]), d["dest_poly"], tuple(d["dest"]),
                         tuple(d["runup"]), tuple(d["takeoff"]), d["speed"],
                         frozenset((s, tuple(hs)) for s, hs in d["requires"]))
                    for d in json.loads(path.read_text())]
        jumps = self._find_jumps()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps([
            {"line": j.line, "src": sorted(j.src_polys), "dest_poly": j.dest_poly,
             "dest": j.dest, "runup": j.runup, "takeoff": j.takeoff, "speed": j.speed,
             "requires": sorted([s, list(hs)] for s, hs in j.requires)} for j in jumps]))
        return jumps

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
        # Sectors that can move are required at the heights used even when those are the
        # initial ones: a takeoff floor that's been lowered for good can't be jumped from.
        if not hasattr(self, "_moving"):
            self._moving = {s for t in self.triggers for s in t.effects}
        moving = self._moving
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
                    requires = frozenset((s, hh) for s, hh in chosen.items()
                                         if hh != init[s] or s in moving)
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
                    requires = frozenset((s, hh) for s, hh in chosen.items()
                                         if hh != init[s] or s in moving)
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
        r = PLAYER_RADIUS + BOX_SLACK  # a gap exactly 32 wide can't be threaded
        m = self.map
        moving = {s for t in self.triggers for s in t.effects}
        ledges = self._drop_lines()  # walls only from below: judged per approach, like movers
        fits = []
        # Lines of moving sectors that would make a portal too narrow when they block (a
        # raised block beside a 24-wide strip): checked against heights in passable().
        self._portal_gates: dict[int, list[tuple[int, tuple[Point, Point], tuple[int, int]]]] = {}
        self._gate_fits: dict[tuple[int, frozenset], bool] = {}
        self._static_grid_for_width = grid
        for portal in self.mesh.portals:
            (x1, y1), (x2, y2) = portal.p1, portal.p2
            box = (min(x1, x2) - r, min(y1, y2) - r, max(x1, x2) + r, max(y1, y2) + r)
            # The portal's own line is judged by flags/heights in passable(), not here.
            walls = {e for cell in _cells(*box) for e, line in grid.get(cell, ())
                     if line is None or line != portal.linedef}
            # Slivers (BSP leftovers a unit or so wide, e.g. along MAP16's lift edge) are never
            # the real way through: where it's open there's a wide portal next to them.
            ok = portal.width >= MIN_PORTAL and _box_fits_somewhere(portal, r, walls)
            fits.append(ok)
            if not ok:
                continue
            gates = {}
            for cell in _cells(*box):
                for li in self.mesh._line_grid.get(cell, ()):
                    line = m.linedefs[li]
                    f, b = m.front_sector(line), m.back_sector(line)
                    if li != portal.linedef and f is not None and b is not None and \
                            (f in moving or b in moving or li in ledges):
                        v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                        # Only lines across the portal narrow it; one alongside it (a thin
                        # BSP strip next to a door) matters for the next crossing instead.
                        lx, ly = v2.x - v1.x, v2.y - v1.y
                        cos = abs(lx * (x2 - x1) + ly * (y2 - y1)) / max(
                            math.hypot(lx, ly) * math.hypot(x2 - x1, y2 - y1), 1e-9)
                        if cos < 0.87:
                            gates[li] = ((v1.x, v1.y), (v2.x, v2.y)), (f, b)
            if gates and not _box_fits_somewhere(portal, r, walls | {e for e, _ in gates.values()}):
                self._portal_gates[portal.index] = [(li, e, s) for li, (e, s) in gates.items()]
        return fits

    def gate_blockers(self, portal_index: int, frm: int, states: HeightStates,
                      doors: Doors | None = None) -> list[int]:
        """Nearby moving-sector lines that block the box at this portal, coming from poly
        `frm`: the box can't overlap the sector on their far side (too high a step, or no
        headroom). Doors we can open from here (key bars beside a corridor) count as open."""
        gates = self._portal_gates.get(portal_index)
        if not gates:
            return []
        sector = self.mesh.polys[frm].sector
        blocking = []
        here = states[sector] | (doors[sector][0] if doors and sector in doors else set())
        for li, _edge, sectors in gates:
            for s in sectors:
                if s == sector:
                    continue
                hs = states[s]
                if doors and s in doors and frm in doors[s][1]:
                    hs = hs | doors[s][0]
                if not any(can_cross(ha, hb) for ha in here for hb in hs):
                    blocking.append(li)
                    break
        return blocking

    def _fits_now(self, portal_index: int, frm: int, states: HeightStates,
                  doors: Doors | None) -> bool:
        """Does the box fit through the portal from poly `frm`, given which nearby
        moving-sector lines block (gate_blockers)?"""
        return self.portal_fits_without(portal_index,
                                        self.gate_blockers(portal_index, frm, states, doors))

    def portal_fits_without(self, portal_index: int, blocking) -> bool:
        """Does the box fit through the portal with these gate lines (indices) as walls?"""
        if not blocking:
            return True
        gates = self._portal_gates[portal_index]
        key = (portal_index, frozenset(blocking))
        if key not in self._gate_fits:
            portal = self.mesh.portals[portal_index]
            (x1, y1), (x2, y2) = portal.p1, portal.p2
            r = PLAYER_RADIUS + BOX_SLACK
            grid = self._static_grid_for_width
            walls = {e for cell in _cells(min(x1, x2) - r, min(y1, y2) - r,
                                          max(x1, x2) + r, max(y1, y2) + r)
                     for e, line in grid.get(cell, ())
                     if line is None or line != portal.linedef}
            walls |= {e for li, e, _ in gates if li in key[1]}
            self._gate_fits[key] = _box_fits_somewhere(portal, r, walls)
        return self._gate_fits[key]

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
                case _:  # gun triggers: shoot from anywhere with a view of the line...
                    candidates = touching_any.get(line.index, set())
                    a, b = self.map.vertexes[line.v1], self.map.vertexes[line.v2]
                    points = [(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
                              for t in (0.25, 0.5, 0.75)]
                    for point in points:
                        candidates = candidates | self._line_of_sight_polys(
                            point, ignore=line.index)
                    # ...where a level shot (no aiming up or down) actually hits it
                    activators = set()
                    for pi in candidates:
                        ranges = [r for point in points
                                  if (r := self._shot_floors(self.mesh.polys[pi].center,
                                                             point, line.index))]
                        if ranges:
                            activators.add(pi)
                            self.shot_floors[(line.index, pi)] = max(
                                ranges, key=lambda r: r[1] - r[0])
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
            vs = poly.vertices
            # nearest part of the poly (its edges, not just its corners: big polys run past
            # short lines)
            if not any(_segment_distance(vs[i], vs[(i + 1) % len(vs)], a, b) <= USE_REACH
                       for i in range(len(vs))):
                continue
            # The use trace stops at the first line with no opening: a switch behind closed
            # bars (MAP32's exit, opened by killing the Keens) can only be pressed through them
            # once they're open. Record which sectors must be open (use_gates).
            near = _spot_in_poly(poly, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2), inset=1.0)
            length = max(math.dist(a, b), 1e-6)
            inner = (a[0] + (b[0] - a[0]) * min(0.45, 2 / length),   # aim a little inside
                     a[1] + (b[1] - a[1]) * min(0.45, 2 / length))   # the line's ends, which
            inner2 = (b[0] + (a[0] - b[0]) * min(0.45, 2 / length),  # other walls share
                      b[1] + (a[1] - b[1]) * min(0.45, 2 / length))
            gates = [g for p in (near, poly.center)
                     if (g := self._use_gates(p, _closest_point(p, inner, inner2),
                                              line.index)) is not None]
            if gates:
                out.add(poly.index)
                best = min(gates, key=len)
                if best:
                    self.use_gates[(line.index, poly.index)] = best
        return out

    def _use_gates(self, p: Point, q: Point, target: int) -> frozenset | None:
        """Sectors closed at the map's initial heights that the use trace from p to q (on the
        target line) passes, or None if a line that never opens is in the way."""
        m, init = self.map, self.movers.initial
        if not hasattr(self, "_may_move"):  # tagged sectors and door sectors
            self._may_move = {s.index for s in m.sectors if s.tag} | {
                m.back_sector(ln) for ln in m.linedefs
                if (i := classify_line(ln.special)) and i.trigger in (Trigger.D1, Trigger.DR)}
        gates = set()
        for cell in _cells(min(p[0], q[0]), min(p[1], q[1]), max(p[0], q[0]), max(p[1], q[1])):
            for li in self.mesh._line_grid.get(cell, ()):
                if li == target:
                    continue
                line = m.linedefs[li]
                f, b = m.front_sector(line), m.back_sector(line)
                closed = f is None or b is None or \
                    min(init[f][1], init[b][1]) - max(init[f][0], init[b][0]) <= 0
                # The trace also stops at (and uses) the first other line with a use action.
                # One-shot ones lose their action once used (and the plan uses them first).
                usable = (i := classify_line(line.special)) is not None and \
                    i.trigger in (Trigger.SR, Trigger.DR)
                if not closed and not usable:
                    continue
                v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                # touching counts: slipping exactly through a corner isn't a clear view
                if _segment_distance(p, q, (v1.x, v1.y), (v2.x, v2.y)) >= 0.5:
                    continue
                if usable:
                    return None
                movable = [s for s in (f, b) if s is not None and s in self._may_move
                           and init[s][1] - init[s][0] <= 0]
                if f is None or b is None or not movable:
                    return None
                gates.update(movable)
        return frozenset(gates)

    def activators_now(self, t: TriggerLine, states: HeightStates,
                       doors: Doors | None = None) -> set[int]:
        """The trigger's activator polys whose use trace isn't blocked by a closed sector
        with these heights (with `doors`, ones we can open count as open), and for gun
        lines, where the floor can be at a height a level shot hits the line from."""
        if t.line < 0:
            return t.activators
        out = set()
        for p in t.activators:
            if not all(any(h[1] - h[0] > 0 for h in states[s]) or (doors and s in doors)
                       for s in self.use_gates.get((t.line, p), ())):
                continue
            shot = self.shot_floors.get((t.line, p))
            if shot is not None and not any(shot[0] <= h[0] <= shot[1]
                                            for h in states[self.mesh.polys[p].sector]):
                continue
            out.add(p)
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

    def _shot_floors(self, p: Point, q: Point, target: int) -> tuple[float, float] | None:
        """Floors to stand on at p so a level shot hits line `target` at q: bullets leave at
        SHOT_HEIGHT above the feet and go straight (no aiming up or down), so every two-sided
        line on the way must be open at that height, and the target hit where it's solid.
        (Map's initial heights.) None if no floor works."""
        m, init = self.map, self.movers.initial
        lo, hi = -math.inf, math.inf   # shot heights that get through
        for cell in _cells(min(p[0], q[0]), min(p[1], q[1]), max(p[0], q[0]), max(p[1], q[1])):
            for li in self.mesh._line_grid.get(cell, ()):
                if li == target:
                    continue
                line = m.linedefs[li]
                v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                if not _segments_cross(p, q, (v1.x, v1.y), (v2.x, v2.y)):
                    continue
                f, b = m.front_sector(line), m.back_sector(line)
                if f is None or b is None:
                    return None
                lo = max(lo, init[f][0], init[b][0])
                hi = min(hi, init[f][1], init[b][1])
        line = m.linedefs[target]
        f, b = m.front_sector(line), m.back_sector(line)
        if b is None:  # a wall: anywhere between its sector's floor and ceiling
            lo, hi = max(lo, init[f][0]), min(hi, init[f][1])
        else:  # two-sided: only its upper or lower part is solid
            opening = (max(init[f][0], init[b][0]), min(init[f][1], init[b][1]))
            below = (lo, min(hi, opening[0]))
            above = (max(lo, opening[1]), hi)
            lo, hi = below if below[1] - below[0] >= above[1] - above[0] else above
        if hi - lo < 1:
            return None
        return lo - SHOT_HEIGHT, hi - SHOT_HEIGHT

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
        if not self._statically_passable(portal_index, frm):
            return False
        to = mesh.portals[portal_index].other(frm)
        sa, sb = mesh.polys[frm].sector, mesh.polys[to].sector
        if not self._fits_now(portal_index, frm, states, doors):
            return False
        return sa == sb or self._heights_passable(portal_index, frm, sa, sb, states, doors)

    def _landable(self, li: int, high: int, top: float) -> bool:
        """Can the player drop off line `li` from sector `high`, whose floor is at `top`? DOOM
        keeps you on the highest floor anywhere under your box, so you only fall once the
        whole box is past the ledge, and up there it must fit (not straddle a wall, nor an
        opening too low for someone at `top`). A gap exactly as wide as the box can't be
        dropped into (MAP15's 32-wide strip by a low opening: you're stopped a hair short).
        Only walls and ceilings are checked (the map's initial ceilings): floors past the
        ledge may have moved, and a teleport line just past it means you never land (MAP13)."""
        key = (li, high, top)
        cache = self.__dict__.setdefault("_landable_cache", {})
        if key not in cache:
            from .physics import PlayerSim  # (physics imports the motor, which imports us)
            if not hasattr(self, "_probe"):
                self._probe = PlayerSim(self)
            m = self.map
            line = m.linedefs[li]
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            dx, dy = b.x - a.x, b.y - a.y
            length = math.hypot(dx, dy) or 1.0
            nx, ny = -dy / length, dx / length  # left of v1->v2: the back side
            if m.back_sector(line) == high:
                nx, ny = -nx, -ny
            clear = PLAYER_RADIUS * (abs(nx) + abs(ny)) + 0.5  # box fully past the line
            ok = False
            steps = max(2, int(length // 8))
            for i in range(1, steps):
                t = i / steps
                for d in (clear, clear + 4, clear + 8, clear + 16, clear + 32):
                    x, y = a.x + dx * t + nx * d, a.y + dy * t + ny * d
                    fits, _floor, ceil, *_ = self._probe._check(x, y, top)
                    if fits and ceil - top >= PLAYER_HEIGHT:
                        ok = True
                        break
                if ok:
                    break
            cache[key] = ok
        return cache[key]

    def reachable(self, start: int, states: HeightStates,
                  doors: Doors | None = None) -> set[int]:
        # The planner's inner loop (millions of portal checks a level): the parts of
        # passable() that don't depend on heights are worked out once (_static_adjacency).
        adj = self._static_adjacency()
        gates = self._portal_gates
        seen = {start}
        todo = [start]
        while todo:
            poly = todo.pop()
            for pi, to, sa, sb in adj[poly]:
                if to in seen:
                    continue
                if pi in gates and not self._fits_now(pi, poly, states, doors):
                    continue
                if sa != sb and not self._heights_passable(pi, poly, sa, sb, states, doors):
                    continue
                seen.add(to)
                todo.append(to)
            for tp in self.tele_from.get(poly, ()):
                if tp.dest_poly not in seen and self.edge_ok(tp, states):
                    seen.add(tp.dest_poly)
                    todo.append(tp.dest_poly)
        return seen

    def _static_adjacency(self) -> list[list[tuple[int, int, int, int]]]:
        """Per poly: (portal, poly beyond, its sector, the far sector) for the portals that
        aren't ruled out whatever the heights (blocking lines, teleport lines walked from
        the front, gaps the box never fits)."""
        if not hasattr(self, "_adj"):
            mesh = self.mesh
            self._adj = [[] for _ in mesh.polys]
            for poly in mesh.polys:
                if poly is None:
                    continue
                for pi in poly.portals:
                    if self._statically_passable(pi, poly.index):
                        to = mesh.portals[pi].other(poly.index)
                        self._adj[poly.index].append((pi, to, poly.sector, mesh.polys[to].sector))
        return self._adj

    def _statically_passable(self, portal_index: int, frm: int) -> bool:
        mesh = self.mesh
        portal = mesh.portals[portal_index]
        if portal.linedef is not None:
            line = self.map.linedefs[portal.linedef]
            if line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                return False
            # Walking over a teleport line from its front side teleports you: that's the
            # teleport edge (tele_from), not a walk. (Back to front is an ordinary crossing.)
            if line.special in TELEPORTERS_FOR_PLAYERS and \
                    portal.linedef in mesh.polys[frm].lines_front:
                return False
        return bool(self.portal_fits[portal_index])

    def _heights_passable(self, portal_index: int, frm: int, sa: int, sb: int,
                          states: HeightStates, doors: Doors | None) -> bool:
        """The height part of passable(): a step or drop we can make, with headroom."""
        hs_a, hs_b = states[sa], states[sb]
        if doors:
            if sa in doors:
                hs_a = hs_a | doors[sa][0]
            if sb in doors and frm in doors[sb][1]:
                hs_b = hs_b | doors[sb][0]
        linedef = self.mesh.portals[portal_index].linedef
        return any(can_cross(ha, hb) and (ha[0] - hb[0] <= MAX_STEP or linedef is None
                                          or self._landable(linedef, sa, ha[0]))
                   for ha in hs_a for hb in hs_b)

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
        # Touching needs the boxes to overlap strictly; keep a margin for where the bot stops.
        reach = PLAYER_RADIUS + ITEM_RADIUS - PICKUP_MARGIN
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
            # if the line blocks or the item's side is too high to step onto, else a little
            # past it), then check the boxes overlap.
            blocks = portal.linedef is not None and \
                self.map.linedefs[portal.linedef].flags & (ML_BLOCKING | ML_BLOCKPLAYERS)
            steppable = any(can_cross(ho, hh) for ho in states[other.sector]
                            for hh in states[home.sector])
            back = PLAYER_RADIUS + 1 if blocks or not steppable else 8
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
        """A* over polys (see _find_path_once). If the smoothed path has a stretch the
        player's box can't get through (a 32-wide strip inside one poly, a row of pillars),
        search again avoiding the polys there. If every way squeezes: the least squeezed one,
        with `Path.squeezes` set (the box check has false alarms, so the planner ranks these
        below clean paths rather than refusing them), or None with `strict_fit`."""
        best, banned = None, set()   # (squeeze points, path) of the least bad attempt
        for _ in range(3):
            self._squeezes: list[Point] = []
            path = self._find_path_once(start, goal, states, doors, banned)
            if path is None:
                break
            if not self._squeezes:
                return path
            if best is None or len(self._squeezes) < best[0]:
                best = (len(self._squeezes), path)
            # ban the path's own polys where it squeezes (not where it starts or ends)
            new = set()
            for pi in path.polys[1:-1]:
                vs = self.mesh.polys[pi].vertices
                if any(_segment_distance(vs[i], vs[(i + 1) % len(vs)], q, q) <= PLAYER_RADIUS
                       for q in self._squeezes for i in range(len(vs))):
                    new.add(pi)
            new -= banned
            if not new:
                break
            banned |= new
        if best is None or self.strict_fit:
            return None
        best[1].squeezes = best[0]
        return best[1]

    def _find_path_once(self, start: Point, goal: Point, states: HeightStates,
                        doors: Doors | None, banned: set[int]) -> Path | None:
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
                if self.edge_ok(tp, states):
                    moves.append((tp.dest_poly, tp.dest, tp))
            for nxt, point, via in moves:
                if nxt in banned:
                    continue
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
        self._smooth_states = states  # box fitting judges moving sectors at these heights
        try:
            points = self._smooth(start, goal, chain)
        finally:
            self._smooth_states = None
        return Path(polys, points, cost,
                    self._annotate(chain, states, doors),
                    [via for _, via in chain if isinstance(via, Jump)])

    def _timed_path(self, start: Point, goal: Point, states: HeightStates, doors: Doors | None,
                    fired: TriggerLine | None) -> Path | None:
        """find_path for the leg right after `fired` went off at `start`. A temporary state it
        made (lift down, door open for a while) can only be used if the path gets to that
        sector before it ends, at running speed; otherwise path without it."""
        path = self.find_path(start, goal, states, doors)
        if path is None or fired is None or fired.line < 0:
            return path
        temporary = self._temporary_heights()
        late = []
        arrive = self._arrival_tics(path)
        for j in path.jumps:  # a jump needs its heights when we take off
            src = self.mesh.poly_at(*j.takeoff)
            if src is not None and src.sector in arrive:
                for s, _h in j.requires:
                    arrive[s] = min(arrive.get(s, math.inf), arrive[src.sector])
        for s, hs in fired.effects.items():
            if fired.permanent:
                # A floor moving for good: where it was is only there until it gets where
                # it's going (E4M2: a jump planned over sector 30 at its old height, which it
                # rises from in 0.3 s; the bot came down on it, well short).
                timing = self.movers.timing(fired.info.special, fired.info.description, s,
                                            hs[0])
                if s in arrive and arrive[s] > timing.travel:
                    late += [(s, h) for h in states[s] if h not in hs]
                continue
            for h in hs:
                if (s, h) not in temporary or s not in arrive:
                    continue
                timing = self.movers.timing(fired.info.special, fired.info.description, s, h)
                if arrive[s] > timing.travel + (timing.hold or 0):
                    late.append((s, h))
        if not late:
            return path
        slower = {s: set(hs) for s, hs in states.items()}
        for s, h in late:
            if len(slower[s]) > 1:
                slower[s].discard(h)
        return self.find_path(start, goal, slower, doors)

    def _arrival_tics(self, path: Path) -> dict[int, float]:
        """Roughly when (tics from the start) a path first gets into each sector it crosses:
        distance along it at a little under full running speed, plus a moment to get going."""
        out: dict[int, float] = {}
        dist, prev = 0.0, None

        def note(q: Point, d: float) -> None:
            poly = self.mesh.poly_at(*q)
            if poly is not None and poly.sector not in out:
                out[poly.sector] = 10 + d / LEG_SPEED
        for p in path.waypoints:
            if p is None:
                prev = None
                continue
            if prev is None:
                note(p, dist)
            else:  # sample along the segment: doors and the like lie between waypoints
                seg = math.dist(prev, p)
                n = max(1, int(seg // 8))
                for i in range(1, n + 1):
                    note((prev[0] + (p[0] - prev[0]) * i / n, prev[1] + (p[1] - prev[1]) * i / n),
                         dist + seg * i / n)
                dist += seg
            prev = p
        for poly in path.polys:  # anything missed (a sliver between samples)
            out.setdefault(self.mesh.polys[poly].sector, 10 + path.cost / LEG_SPEED)
        return out

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

    @property
    def _key_positions(self) -> dict[Key, list[Point]]:
        """Where each key (at skill 3) is."""
        if not hasattr(self, "_keys_at"):
            self._keys_at: dict[Key, list[Point]] = {}
            for t in self.map.things:
                if t.type in Key._value2member_map_ and t.in_skill(3):
                    self._keys_at.setdefault(Key(t.type), []).append((t.x, t.y))
        return self._keys_at

    def plan(self, start: Point, goals: list[Point], keys: set[Key] = frozenset(),
             states: HeightStates | None = None, key_items: KeyItems | None = None,
             max_expansions: int = 300, max_seconds: float = 5.0,
             trace: list | None = None,
             goal_triggers: dict | None = None) -> Route | None:
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
        initial heights; at runtime pass the live heights. `goal_triggers` maps goal points to
        the switch used there: the goal only counts once that switch can be pressed from it
        (nothing closed in the way, see activators_now).
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
            temp: frozenset = frozenset()  # temporary heights the last trigger made (a lift
            #                                down for 3s): only the next leg can use them
            squeezes: int = 0   # spots on the route so far where the box may not fit
            rank: tuple = ()    # the rest of its priority (after goal reach and squeezes)

        temporary = self._temporary_heights()

        def expire(states: HeightStates, temp: frozenset) -> HeightStates:
            """The heights once a node's temporary states have run out."""
            if not temp:
                return states
            out = {s: set(h) for s, h in states.items()}
            for s, h in temp:
                if len(out[s]) > 1:
                    out[s].discard(h)
            return out

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
        # Paths where the player's box may not fit (Path.squeezes) aren't refused (the check has
        # false alarms) but rank below clean ones: priority is (can't reach a goal *or*
        # squeezed, squeezes so far, rank...), so any clean node comes before a squeezed one
        # (E4M9: a squeezed walk to the key beat pressing the switches that open the way). A
        # squeezed route to a goal is kept as `fallback` and returned once nothing ahead in
        # the queue could do better.
        heap = [((False, 0, False, False, 0, 0), next(counter), root)]
        seen: set[tuple] = set()
        expansions = 0
        fallback: tuple[tuple, Route] | None = None
        deadline = time.perf_counter() + max_seconds
        while heap and expansions < max_expansions and time.perf_counter() < deadline:
            priority, _, node = heapq.heappop(heap)
            if fallback is not None and priority[:2] >= fallback[0]:
                return fallback[1]
            sig = (node.added, node.keys, frozenset(node.reach))
            if sig in seen:
                continue
            seen.add(sig)
            if node.pending:  # path the leg that got us here, now that we've picked this node
                prev_pos, prev_states, prev_keys, t, key = node.pending
                path = self._timed_path(prev_pos, node.pos, prev_states, doors_for(prev_keys),
                                        node.legs[-1].trigger if node.legs else None)
                if path is None:
                    continue
                node.legs = node.legs + [Leg(t, path, key)]
                node.pending = None
                if path.squeezes:  # back in the queue, behind cleaner nodes
                    node.squeezes += path.squeezes
                    seen.discard(sig)
                    heapq.heappush(heap, ((True, node.squeezes) + node.rank,
                                          next(counter), node))
                    continue
            expansions += 1
            if trace is not None:  # debugging: what the search actually explores
                last = node.legs[-1] if node.legs else None
                trace.append((len(node.reach), sorted(k.name for k in node.keys),
                              (f"line {last.trigger.line}" if last and last.trigger else
                               f"key {last.key.name}" if last and last.key else "start")))
            doors = doors_for(node.keys)
            for gp in sorted(goal_polys.keys() & node.reach,
                             key=lambda p: math.dist(node.pos, goal_polys[p])):
                gt = (goal_triggers or {}).get(goal_polys[gp])
                if gt is not None and gp not in self.activators_now(gt, node.states):
                    continue  # e.g. the exit switch is still behind closed bars
                path = self._timed_path(node.pos, goal_polys[gp], node.states, doors,
                                        node.legs[-1].trigger if node.legs else None)
                if path:
                    route = Route(node.legs + [Leg(None, path)], node.states, node.keys,
                                  goal_polys[gp])
                    total = node.squeezes + path.squeezes
                    if not total:
                        return route
                    if fallback is None or (True, total) < fallback[0]:
                        fallback = ((True, total), route)

            def push(target: Point, trial: HeightStates, added: frozenset, ks: frozenset,
                     t: TriggerLine | None, key: Key | None, temp: frozenset = frozenset(),
                     after: set[int] | None = None) -> None:
                target_poly = mesh.poly_at(*target)
                if target_poly is None:
                    return
                if after is None:  # (what's reachable from target, if already worked out)
                    after = self.reachable(target_poly.index, trial, doors_for(ks))
                child = Node(target, trial, added, ks, node.legs, after, node.steps + 1,
                             (node.pos, node.states, node.keys, t, key), temp,
                             squeezes=node.squeezes)
                progress = bool(after - node.reach) or key is not None
                relevant = (t is not None and t.line in rel_t) or (key is not None and key in rel_k)
                child.rank = (not relevant, not progress, -len(after), child.steps)
                priority = (bool(child.squeezes) or not goal_polys.keys() & after,
                            child.squeezes) + child.rank
                heapq.heappush(heap, (priority, next(counter), child))

            # Triggers: one candidate per distinct world change, the closest line producing it.
            options: dict[frozenset, tuple[float, TriggerLine, Point]] = {}
            for t in actions:
                # Usable if a door we can open is all that's in the way, but stand where no
                # door needs opening when there's such a spot (the path then opens it).
                usable = self.activators_now(t, node.states) & node.reach or \
                    self.activators_now(t, node.states, doors) & node.reach
                if not usable or not t.info.lock.satisfied(set(node.keys)):
                    continue
                new = frozenset((s, h) for s, hs in t.effects.items() for h in hs
                                if h not in node.states[s])
                if not new:
                    continue  # already fired, or a duplicate of something that was
                target = self._activation_point(t, node.pos, usable)
                d = math.dist(node.pos, target)
                if new not in options or d < options[new][0]:
                    options[new] = (d, t, target, usable)
            for new, (_, t, target, usable) in options.items():
                base = expire(node.states, node.temp)
                trial = apply_trigger(base, t)
                temp = frozenset((s, h) for s, hs in t.effects.items() for h in hs
                                 if (s, h) in temporary and h not in base[s])
                if t.permanent:
                    # Once a floor has risen for good its old height is gone too: only the
                    # next leg can still hop on while it moves (E4M2: the plan walked from a
                    # pit onto sector 3 at its old floor long after it had risen).
                    temp |= {(s, h) for s, hs in t.effects.items() for h in trial[s]
                             if h not in hs}
                # Where we stand to use it matters (a door pressed from the pit below it is
                # useless): try the default spot and a few others it can be used from, keep
                # the one leaving the most reachable.
                by_sector: dict[int, int] = {}  # one candidate per sector it's usable from
                for p in sorted(usable,
                                key=lambda p: math.dist(mesh.polys[p].center, target)):
                    by_sector.setdefault(mesh.polys[p].sector, p)
                near = _line_mid(self.map, t.line) if t.line >= 0 else target
                spots = [target] + [_spot_in_poly(mesh.polys[p], near) if t.line >= 0
                                    else mesh.polys[p].center
                                    for p in list(by_sector.values())[:8]]
                doors_k = doors_for(node.keys)
                # A spot inside what another reaches can't reach more than it: skip it.
                best, best_reach, covered = spots[0], None, set()
                for sp in spots:
                    p = mesh.poly_at(*sp)
                    if p is None or p.index in covered:
                        continue
                    r = self.reachable(p.index, trial, doors_k)
                    covered |= r
                    if best_reach is None or len(r) > len(best_reach):
                        best, best_reach = sp, r
                push(best, trial, (node.added - node.temp) | new, node.keys, t, None, temp,
                     best_reach)
            # Keys: for each key not yet held, the reachable pickup spot that leaves the most
            # reachable afterwards (some spots are slivers you can't walk on from).
            for key, spots in key_items.items():
                if key in node.keys:
                    continue
                # Spots as things stand at this node (key_items is judged against every
                # height, so a key on a pedestal that hasn't been lowered yet would look
                # reachable from beside it: MAP04's blue card).
                if key in self._key_positions:
                    spots = [s for x, y in self._key_positions[key]
                             for s in self.pickup_spots(x, y, node.states)]
                here = [(p, pt) for p, pt in spots if p in node.reach]
                if here:
                    ks = node.keys | {key}
                    _, spot = max(here, key=lambda s: (
                        len(self.reachable(s[0], node.states, doors_for(ks))),
                        -math.dist(node.pos, s[1])))
                    push(spot, expire(node.states, node.temp), node.added - node.temp, ks, None,
                         key)
        return fallback[1] if fallback else None

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
                          if self.edge_ok(tp, states_all)]
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
        """Where to stand to use a switch, or where to walk to so a walk-over line is crossed.
        A switch spot the player's box can't occupy (barrels in front of E1M9's switch) is
        moved straight back from the line, as far as use range allows."""
        spot = self._activation_spot(t, pos, reach)
        if t.line < 0 or t.info.trigger not in (Trigger.S1, Trigger.SR, Trigger.D1, Trigger.DR):
            return spot
        if not self._box_hits(spot, spot):
            return spot
        line = self.map.linedefs[t.line]
        a, b = self.map.vertexes[line.v1], self.map.vertexes[line.v2]
        q = _closest_point(spot, (a.x, a.y), (b.x, b.y))
        d = math.dist(spot, q)
        if d < 1:
            return spot
        ux, uy = (spot[0] - q[0]) / d, (spot[1] - q[1]) / d
        for back in range(int(d) + 4, USE_REACH + 13, 4):  # use traces reach 64
            p = (q[0] + ux * back, q[1] + uy * back)
            poly = self.mesh.poly_at(*p)
            if poly is not None and poly.index in reach and not self._box_hits(p, p):
                return p
        return spot

    def _activation_spot(self, t: TriggerLine, pos: Point, reach: set[int]) -> Point:
        """The spot before box checks (see _activation_point)."""
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
        # Otherwise the spot nearest the line in any poly it can be used from (big polys only
        # qualify by their nearest part, so their centres can be out of reach).
        best = None
        for pi in t.activators & reach:
            spot = _spot_in_poly(mesh.polys[pi], (mx, my))
            if mesh.poly_at(*spot) is not None and mesh.poly_at(*spot).index == pi:
                d = math.dist(spot, _closest_point(spot, (a.x, a.y), (b.x, b.y)))
                if best is None or d < best[0]:
                    best = (d, spot)
        return best[1] if best else mesh.polys[min(t.activators & reach)].center

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
        crossed |= {via.line for _, via in chain
                    if isinstance(via, Teleport) and not isinstance(via, Jump)}
        # ...but not walk-over lines that shut a sector right beside them (MAP23's "close door
        # 30s" line): keep clear of those, or the closing door pins the player's box.
        crossed -= self._shutting_lines()
        pinned = {p for _, via in chain if isinstance(via, Jump) for p in (via.runup, via.takeoff)}
        out: list[Point | None] = []
        run: list[Point] = []
        for p in points + [None]:
            if p is None:
                if run:
                    out += self._straighten(self._box_fits_path(
                        self._push_from_corners(run, frozenset(crossed), pinned),
                        frozenset(crossed), pinned), frozenset(crossed), pinned,
                        {mesh.polys[p].sector for p, _ in chain})
                    run = []
                out.append(None)
            else:
                run.append(p)
        return out[:-1]

    def _solid_things(self) -> list[tuple[float, float, float]]:
        """(x, y, radius) of solid decorations present at skill 3."""
        if not hasattr(self, "_solids"):
            self._solids = [(t.x, t.y, SOLID_DECORATIONS[t.type]) for t in self.map.things
                            if t.type in SOLID_DECORATIONS and t.in_skill(3)]
        return self._solids

    # --- making paths fit the player's square box ---------------------------------------------

    def _static_edges(self, a: Point, b: Point, pad: float, exempt: frozenset | None = None):
        """Static walls and solid-thing box edges near the segment a-b. With `exempt` (lines
        the path means to cross), also lines that block at the map's initial heights: closed
        sectors that some trigger moves, and ledges seen from below (from a's floor)."""
        grid = self._static_wall_grid()
        seen: set = set()
        cells = list(_cells(min(a[0], b[0]) - pad, min(a[1], b[1]) - pad,
                            max(a[0], b[0]) + pad, max(a[1], b[1]) + pad))
        for cell in cells:
            for e in grid.get(cell, ()):
                if e not in seen:
                    seen.add(e)
                    yield e
        if exempt is None:
            return
        m, init = self.map, self.movers.initial
        walls, drops = self._wall_lines(), self._drop_lines()
        here = self.mesh.poly_at(*a)
        states = getattr(self, "_smooth_states", None)
        if here is None:
            ours = None
        elif states is not None:  # as high as we can be:
            # the player stands on the highest floor anywhere under the box, of those within
            # a step of the floor under its centre (E4M9: a box in a 32-wide gap between two
            # 64-high blocks isn't standing on them, so their edges are walls; it can't get up)
            reach_up = max(h[0] for h in states[here.sector]) + MAX_STEP
            under = {here.sector} | {
                p.sector for dx in (-PLAYER_RADIUS, PLAYER_RADIUS)
                for dy in (-PLAYER_RADIUS, PLAYER_RADIUS)
                if (p := self.mesh.poly_at(a[0] + dx, a[1] + dy)) is not None}
            ours = max(h[0] for s_ in under for h in states[s_] if h[0] <= reach_up)
        else:
            ours = init[here.sector][0]
        for cell in cells:
            for li in self.mesh._line_grid.get(cell, ()):
                if li in seen or li in exempt:
                    continue
                line = m.linedefs[li]
                f, bk = m.front_sector(line), m.back_sector(line)
                if line.special in TELEPORTERS_FOR_PLAYERS:
                    # A teleport line the path doesn't mean to take: keep the box off it (it
                    # fires when the box touches it and the centre crosses its extension).
                    seen.add(li)
                    v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                    yield (v1.x, v1.y), (v2.x, v2.y)
                    continue
                if li not in walls and li not in drops:
                    # Up on a ledge, an opening whose ceiling is below our head is a wall
                    # too, even if it's fine from the floor below (MAP15: next to the drop
                    # off the 32-wide strip, sector 186's ceiling is 176 under the strip).
                    if f is None or bk is None or ours is None:
                        continue
                    top = max(min(hf[1], hb[1]) for hf in (states[f] if states else [init[f]])
                              for hb in (states[bk] if states else [init[bk]]))
                    if top - ours >= PLAYER_HEIGHT:
                        continue
                    seen.add(li)
                    v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                    yield (v1.x, v1.y), (v2.x, v2.y)
                    continue
                seen.add(li)
                if li in walls and states is not None and f is not None and bk is not None \
                        and any(can_cross(hf, hb) or can_cross(hb, hf)
                                for hf in states[f] for hb in states[bk]):
                    continue  # closed at the start, but open by the time we walk this path
                if li not in walls:  # a ledge: only a wall if we're below it
                    if states is not None:
                        top = min(max(hf[0], hb[0]) for hf in states[f] for hb in states[bk])
                    else:
                        top = max(init[f][0], init[bk][0])
                    if ours is None or ours + MAX_STEP >= top:
                        continue
                v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                yield (v1.x, v1.y), (v2.x, v2.y)

    def _ledges_crossed(self, a: Point, b: Point, exempt: frozenset) -> bool:
        """Does a-b walk off a ledge (a one-way drop line) other than the exempt ones?"""
        m, drops = self.map, self._drop_lines()
        for cell in _cells(min(a[0], b[0]), min(a[1], b[1]), max(a[0], b[0]), max(a[1], b[1])):
            for li in self.mesh._line_grid.get(cell, ()):
                if li in drops and li not in exempt:
                    line = m.linedefs[li]
                    v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                    if _segments_cross(a, b, (v1.x, v1.y), (v2.x, v2.y)):
                        return True
        return False

    def _box_hits(self, p: Point, q: Point, ignore=frozenset(),
                  exempt: frozenset | None = None) -> set:
        """Edges (see _static_edges) the player's box touches moving straight from p to q."""
        r = PLAYER_RADIUS + BOX_SLACK
        return {e for e in self._static_edges(p, q, PLAYER_RADIUS + 1, exempt)
                if e not in ignore and _swept_box_hits(p, q, r, e)}

    def _box_fits_path(self, pts: list[Point], exempt: frozenset,
                       pinned: set[Point] = frozenset()) -> list[Point]:
        """The funnel and corner pushes keep round clearances from portal ends and vertices,
        but DOOM's player is a 32x32 square that also hits diagonal walls, pillars and solid
        things (a path squeezed between a barrel and a pillar's corner doesn't fit). Check
        each segment with the swept box and detour locally round what it would hit."""
        if len(pts) < 2:
            return pts
        # Interior waypoints the box can't occupy are dropped; detours reconnect neighbours.
        pts = [p for i, p in enumerate(pts)
               if i == 0 or i == len(pts) - 1 or p in pinned or not self._box_hits(p, p, exempt=exempt)]
        out = [pts[0]]
        for b in pts[1:]:
            a = out[-1]
            # edges the ends already touch don't count
            ignore = self._box_hits(a, a, exempt=exempt) | self._box_hits(b, b, exempt=exempt)
            if self._box_hits(a, b, ignore, exempt):
                detour = self._detour(a, b, exempt, ignore)
                if detour:
                    out += detour
                elif hasattr(self, "_squeezes"):  # tell find_path to look for another way
                    self._squeezes += self._squeeze_points(a, b, ignore, exempt)
            out.append(b)
        return out

    def _straighten(self, pts: list[Point], exempt: frozenset,
                    pinned: set[Point] = frozenset(),
                    sectors: set[int] | None = None) -> list[Point]:
        """Last pass: from each waypoint go straight to the farthest later one the player's
        box reaches without touching a wall or walking off a ledge, never skipping a pinned
        point (jump run-ups/takeoffs). Removes the back-and-forth jitter that corner pushes,
        nudges and detours can leave (MAP02's bars), and any needless corners. Shortcuts
        stay over the route's `sectors` (not across a lift that's only sometimes low), and
        over sectors the stretch they replace went over (E4M2: the route started in a lava
        pit, so a shortcut later on cut straight across it from one rim to the other)."""
        if len(pts) < 3:
            return pts
        over = [self._sectors_along(pts[k], pts[k + 1]) for k in range(len(pts) - 1)]
        out, i = [pts[0]], 0
        while i < len(pts) - 1:
            a = pts[i]
            best = i + 1
            for j in range(min(len(pts) - 1, i + 10), i + 1, -1):
                if any(p in pinned for p in pts[i + 1:j]):
                    continue
                b = pts[j]
                allowed = set().union(*over[i:j])
                if sectors is not None:
                    allowed &= sectors
                if not self._within_sectors(a, b, allowed):
                    continue
                ignore = self._box_hits(a, a, exempt=exempt) | self._box_hits(b, b, exempt=exempt)
                if not self._box_hits(a, b, ignore, exempt) \
                        and not self._ledges_crossed(a, b, exempt):
                    best = j
                    break
            out.append(pts[best])
            i = best
        return out

    def _sectors_along(self, a: Point, b: Point) -> set[int]:
        """Sectors under the segment a-b (sampled every 8 units)."""
        n = max(1, int(math.dist(a, b) // 8))
        out = set()
        for i in range(n + 1):
            poly = self.mesh.poly_at(a[0] + (b[0] - a[0]) * i / n,
                                     a[1] + (b[1] - a[1]) * i / n)
            if poly is not None:
                out.add(poly.sector)
        return out

    def _within_sectors(self, a: Point, b: Point, sectors: set[int] | None) -> bool:
        """Does the segment a-b stay over the given sectors (sampled every 8 units)?"""
        if sectors is None:
            return True
        n = max(1, int(math.dist(a, b) // 8))
        for i in range(n + 1):
            poly = self.mesh.poly_at(a[0] + (b[0] - a[0]) * i / n,
                                     a[1] + (b[1] - a[1]) * i / n)
            if poly is None or poly.sector not in sectors:
                return False
        return True

    def _squeeze_points(self, a: Point, b: Point, ignore, exempt: frozenset) -> list[Point]:
        """Points along a-b where the player's box doesn't fit."""
        length = math.dist(a, b)
        if length < 1:
            return []
        ux, uy = (b[0] - a[0]) / length, (b[1] - a[1]) / length
        out = []
        for t in range(0, int(length) + 1, 8):
            p = (a[0] + ux * t, a[1] + uy * t)
            if self._box_hits(p, p, ignore, exempt):
                out.append(p)
        return out[::max(1, len(out) // 6)]

    def _detour(self, a: Point, b: Point, exempt: frozenset, ignore) -> list[Point] | None:
        """Points to go via from a to b so the box fits: the shortest route through spots just
        diagonally off the corners of nearby walls and solid things. Only the stretch of a-b
        that doesn't fit is rerouted (the rest may run through tight spots of its own)."""
        length = math.dist(a, b)
        if length < 1:
            return None
        ux, uy = (b[0] - a[0]) / length, (b[1] - a[1]) / length
        bad = [t for t in range(0, int(length) + 1, 8)
               if self._box_hits((a[0] + ux * t, a[1] + uy * t),
                                 (a[0] + ux * min(t + 8, length), a[1] + uy * min(t + 8, length)),
                                 ignore, exempt)]
        if not bad:
            return None
        t0, t1 = bad[0] - 40, bad[-1] + 48
        a2 = (a[0] + ux * t0, a[1] + uy * t0) if t0 > 0 else a
        b2 = (a[0] + ux * t1, a[1] + uy * t1) if t1 < length else b
        if (a2 != a and self._box_hits(a2, a2, exempt=exempt)) or \
                (b2 != b and self._box_hits(b2, b2, exempt=exempt)):
            a2, b2 = a, b
        nodes, seen = [a2, b2], set()
        for e in self._static_edges(a2, b2, 96, exempt):
            for v in e:
                if v in seen or _segment_distance(a2, b2, v, v) > 96:
                    continue
                seen.add(v)
                # with some room to spare, or only just clear (passages exactly 32 wide)
                for d in (PLAYER_RADIUS + THING_CLEARANCE, PLAYER_RADIUS + 2 * BOX_SLACK):
                    for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                        c = (v[0] + sx * d, v[1] + sy * d)
                        if self.mesh.poly_at(*c) is not None and \
                                not self._box_hits(c, c, exempt=exempt):
                            nodes.append(c)

        def ok(p: Point, q: Point) -> bool:
            skip = ignore if p in (a, a2) or q in (b, b2) else frozenset()
            return not self._box_hits(p, q, skip, exempt) and not self._ledges_crossed(p, q, exempt)
        # A* over the candidate spots, checking edges lazily.
        limit = 3 * math.dist(a2, b2) + 192
        best, prev = {0: 0.0}, {}
        todo = [(math.dist(a2, b2), 0.0, 0)]
        expansions = 0
        while todo and expansions < 150:
            _, cost, i = heapq.heappop(todo)
            if i == 1:
                break
            if cost > best[i]:
                continue
            expansions += 1
            for j in range(1, len(nodes)):
                c = cost + math.dist(nodes[i], nodes[j])
                if j != i and c < best.get(j, math.inf) and \
                        c + math.dist(nodes[j], b2) <= limit and ok(nodes[i], nodes[j]):
                    best[j], prev[j] = c, i
                    heapq.heappush(todo, (c + math.dist(nodes[j], b2), c, j))
        if 1 not in prev:
            return None
        chain, i = [], prev[1]
        while i != 0:
            chain.append(nodes[i])
            i = prev[i]
        chain.reverse()
        return ([a2] if a2 != a else []) + chain + ([b2] if b2 != b else [])

    def _shutting_lines(self) -> set[int]:
        """Walk-over lines that close (no headroom) a sector on either side of themselves."""
        if not hasattr(self, "_shutting"):
            m = self.map
            self._shutting = set()
            for t in self.triggers:
                if t.line < 0 or t.info.trigger not in (Trigger.W1, Trigger.WR):
                    continue
                line = m.linedefs[t.line]
                sides = {m.front_sector(line), m.back_sector(line)}
                if any(s in sides and any(h[1] - h[0] < PLAYER_HEIGHT for h in hs)
                       for s, hs in t.effects.items()):
                    self._shutting.add(t.line)
        return self._shutting

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

    def _trap_lines(self) -> set[int]:
        """Walk-over trigger lines that close a sector to below the player's height (doors
        shutting, ceilings coming down): crossing one by accident can shut the way ahead
        (E1M6: line 206 closes door 187 for 30 s, just past it on the way to the blue key)."""
        if not hasattr(self, "_traps"):
            self._traps = {t.line for t in self.triggers
                           if t.line >= 0 and t.info.trigger in (Trigger.W1, Trigger.WR)
                           and any(h[1] - h[0] < PLAYER_HEIGHT
                                   for hs in t.effects.values() for h in hs)}
        return self._traps

    def _teleport_lines(self) -> set[int]:
        """Lines that teleport the player when walked over (from the front)."""
        if not hasattr(self, "_teleport_set"):
            self._teleport_set = {line.index for line in self.map.linedefs
                                  if line.special in TELEPORTERS_FOR_PLAYERS}
        return self._teleport_set

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

    def _walls_near(self, a: Point, b: Point, pad: float, exempt: frozenset | None = None,
                    live: dict[int, Heights] | None = None):
        """Wall lines and solid-thing box edges near the segment a-b. With `exempt` given
        (path smoothing: the lines the path crosses), ledge edges count as walls too, and
        the exempt lines (doors it opens, ledges it drops off) don't. With `live` heights
        (the follower), lines that are walls or ledges *now* count as well: a floor that has
        risen makes new ledges (E4M2: sector 3 rose out of the lava; cutting a corner by it
        dropped the bot back in)."""
        m, mesh, obstacles = self.map, self.mesh, self._obstacle_grid()
        walls = self._wall_lines()
        if exempt is not None:  # the lines the path crosses (doors it opens) aren't walls
            # (teleport lines it doesn't mean to take are: touching one can fire it)
            walls = (walls | self._drop_lines() | self._teleport_lines()) - exempt
        seen: set = set()
        for cell in _cells(min(a[0], b[0]) - pad, min(a[1], b[1]) - pad,
                           max(a[0], b[0]) + pad, max(a[1], b[1]) + pad):
            for li in mesh._line_grid.get(cell, ()):
                if live is not None and li not in walls and li not in seen \
                        and (exempt is None or li not in exempt):
                    line = m.linedefs[li]
                    f, bk = m.front_sector(line), m.back_sector(line)
                    if f is not None and bk is not None and \
                            not (can_cross(live[f], live[bk]) and can_cross(live[bk], live[f])):
                        seen.add(li)
                        v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                        yield (v1.x, v1.y), (v2.x, v2.y)
                        continue
                if li in walls and li not in seen:
                    seen.add(li)
                    line = m.linedefs[li]
                    states = getattr(self, "_smooth_states", None)
                    f, bk = m.front_sector(line), m.back_sector(line)
                    if states is not None and f is not None and bk is not None and \
                            li in self._wall_lines() and \
                            any(can_cross(hf, hb) or can_cross(hb, hf)
                                for hf in states[f] for hb in states[bk]):
                        continue  # closed at the start, open at the heights planned for
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
            for a, b in self._walls_near(p, p, WAYPOINT_CLEARANCE + 6, exempt):
                q = _closest_point(p, a, b)
                d = math.dist(p, q)
                need = _box_clearance(a, b)
                if d < need and (worst is None or d - need < worst[0]):
                    worst = (d - need, q, a, b, need)
            if worst is None:
                return p
            _, q, a, b, need = worst
            d = math.dist(p, q)
            if d > 1e-6:
                ux, uy = (p[0] - q[0]) / d, (p[1] - q[1]) / d
            else:  # on the line: step to the side the poly is on
                length = max(math.dist(a, b), 1e-6)
                ux, uy = -(b[1] - a[1]) / length, (b[0] - a[0]) / length
                if self.mesh.poly_at(p[0] + ux, p[1] + uy) is None:
                    ux, uy = -ux, -uy
            moved = (q[0] + ux * need, q[1] + uy * need)
            # Never push a waypoint through a wall or into another sector (MAP02: pushed off
            # a key bar, through the corridor's corner into the void behind it).
            here, there = self.mesh.poly_at(*p), self.mesh.poly_at(*moved)
            if there is None or (here is not None and there.sector != here.sector) \
                    or any(_segments_cross(p, moved, e1, e2)
                           for e1, e2 in self._walls_near(p, moved, 0, exempt)):
                return p
            p = moved
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


def _segment_hits_box(a: Point, b: Point, cx: float, cy: float, r: float) -> bool:
    """Does segment a-b pass through the open axis-aligned box of half-width r at (cx, cy)?
    (Slab clipping.)"""
    t0, t1 = 0.0, 1.0
    for p, d, lo, hi in ((a[0], b[0] - a[0], cx - r, cx + r), (a[1], b[1] - a[1], cy - r, cy + r)):
        if abs(d) < 1e-9:
            if p <= lo or p >= hi:
                return False
            continue
        u0, u1 = (lo - p) / d, (hi - p) / d
        if u0 > u1:
            u0, u1 = u1, u0
        t0, t1 = max(t0, u0), min(t1, u1)
        if t0 >= t1:
            return False
    return True


def _spot_in_poly(poly, target: Point, inset: float = PLAYER_RADIUS + 2) -> Point:
    """The point of the poly nearest `target`, moved `inset` towards the poly's centre (or
    to the centre if the poly is smaller than that)."""
    vs = poly.vertices
    best = min((_closest_point(target, vs[i], vs[(i + 1) % len(vs)]) for i in range(len(vs))),
               key=lambda q: math.dist(q, target))
    cx, cy = poly.center
    d = math.dist(best, (cx, cy))
    if d <= inset:
        return poly.center
    return (best[0] + (cx - best[0]) * inset / d, best[1] + (cy - best[1]) * inset / d)


def _box_fits_somewhere(portal, r: float, walls) -> bool:
    """Is there a spot along the portal where the box of half-width r touches none of `walls`?"""
    (x1, y1), (x2, y2) = portal.p1, portal.p2
    n = max(1, int(portal.width // 4))
    return any(not any(_box_hits_segment(x1 + (x2 - x1) * i / n, y1 + (y2 - y1) * i / n, r, w)
                       for w in walls)
               for i in range(n + 1))


def _box_clearance(a: Point, b: Point) -> float:
    """How far a waypoint should be from the wall a-b: WAYPOINT_CLEARANCE, or more for
    diagonal walls, which the square player box's corner reaches further towards."""
    length = max(math.dist(a, b), 1e-6)
    c, s = abs(b[0] - a[0]) / length, abs(b[1] - a[1]) / length
    return max(WAYPOINT_CLEARANCE, PLAYER_RADIUS * (c + s) + 1)


def _swept_box_hits(p: Point, q: Point, r: float, edge: tuple[Point, Point]) -> bool:
    """Does an axis-aligned square of half-width r, moved straight from p to q, overlap the
    segment `edge`? Separating-axis test: the swept square is convex with edge normals x, y
    and the normal of p-q; add the edge's own normal."""
    e1, e2 = edge
    axes = [(1.0, 0.0), (0.0, 1.0)]
    for (x1, y1), (x2, y2) in ((p, q), (e1, e2)):
        dx, dy = x2 - x1, y2 - y1
        length = math.hypot(dx, dy)
        if length > 1e-9:
            axes.append((-dy / length, dx / length))
    for ux, uy in axes:
        ext = r * (abs(ux) + abs(uy))
        a, b = p[0] * ux + p[1] * uy, q[0] * ux + q[1] * uy
        c, d = e1[0] * ux + e1[1] * uy, e2[0] * ux + e2[1] * uy
        if max(a, b) + ext <= min(c, d) or max(c, d) <= min(a, b) - ext:
            return False
    return True


def _segment_distance(p1: Point, p2: Point, q1: Point, q2: Point) -> float:
    if _segments_cross(p1, p2, q1, q2):
        return 0.0
    return min(math.dist(p1, _closest_point(p1, q1, q2)), math.dist(p2, _closest_point(p2, q1, q2)),
               math.dist(q1, _closest_point(q1, p1, p2)), math.dist(q2, _closest_point(q2, p1, p2)))


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
