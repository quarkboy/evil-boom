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

from evildoom.wad.mapdata import ML_BLOCKING, ML_BLOCKPLAYERS, MapData
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
DAMAGE_COST = 4.0  # path cost multiplier on damaging floors
TELEPORTERS_FOR_PLAYERS = {39, 97, 174, 195, 207, 208, 209, 210}

BARON, CYBERDEMON, SPIDER, MANCUBUS, ARACHNOTRON, KEEN = 3003, 16, 7, 67, 68, 72
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
class Stage:
    keys: frozenset[Key]
    reached: set[int]
    fired: list[TriggerLine]
    keys_available: set[Key]
    exits: list[TriggerLine]


@dataclass
class Path:
    polys: list[int]
    waypoints: list[Point | None]   # None marks a teleport jump
    cost: float
    steps: list[str] = field(default_factory=list)   # human-readable notes (teleports, drops)


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
                case _:  # gun triggers: approximated as "standing next to it"
                    activators = touching_any.get(line.index, set())
            sectors = self._affected(line.index, info)
            out.append(TriggerLine(line.index, info, sectors,
                                   self.movers.effects(line.special, sectors), activators))
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
            polys = {p.index for t in m.things
                     if t.type == monster and t.in_skill(3) and (p := mesh.poly_at(t.x, t.y))}
            if not polys:
                continue
            sectors = [s.index for s in m.tagged_sectors(tag)] if tag else []
            info = LineSpecial(special, Trigger.NONE, category,
                               f"kill all {MONSTER_NAMES[monster]}")
            effects = self.movers.effects(special, sectors) if sectors else {}
            out.append(TriggerLine(-1 - i, info, sectors, effects, polys))
        return out

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
            if self.map.linedefs[portal.linedef].flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
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
            nxt += [tp.dest_poly for tp in self.tele_from.get(poly, [])]
            for n in nxt:
                if n not in seen:
                    seen.add(n)
                    todo.append(n)
        return seen

    def pickup_spots(self, x: float, y: float, states: HeightStates) -> list[tuple[int, Point]]:
        """Polys (and a point in each) from which an item at (x, y) can be picked up.

        P_TouchSpecialThing only needs the bounding boxes to touch (player radius 16 + item
        radius 20) and the item to be between 8 below and 56 above the player's feet, so items
        on pedestals too high to step onto can be grabbed from the floor next to them.
        """
        mesh = self.mesh
        home = mesh.poly_at(x, y)
        if home is None:
            return []
        spots = [(home.index, (x, y))]
        item_floors = {f for f, _ in states[home.sector]}
        edge = [(pi, poly.index) for poly in mesh.polys if poly and poly.sector == home.sector
                for pi in poly.portals]
        for pi, own in edge:
            portal = mesh.portals[pi]
            other = mesh.polys[portal.other(own)]
            if other.sector == home.sector:
                continue
            if portal.linedef is not None and \
                    self.map.linedefs[portal.linedef].flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                continue
            near = _closest_on_segment((x, y), portal)
            if math.dist(near, (x, y)) > PLAYER_RADIUS + ITEM_RADIUS:
                continue
            if any(-8 <= fi - fo <= PLAYER_HEIGHT
                   for fi in item_floors for fo, _ in states[other.sector]):
                # stand just on the other side of the shared edge
                cx, cy = other.center
                d = max(math.dist(near, (cx, cy)), 1)
                spots.append((other.index, (near[0] + (cx - near[0]) * 8 / d,
                                            near[1] + (cy - near[1]) * 8 / d)))
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
                    self._annotate(chain, states, doors))

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
             max_expansions: int = 300, max_seconds: float = 5.0) -> Route | None:
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
        counter = itertools.count()
        root = Node(start, states0, frozenset(), keys0, [],
                    self.reachable(start_poly.index, states0, doors_for(keys0)))
        heap = [((False, False, 0, 0), next(counter), root)]
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
                priority = (not goal_polys.keys() & after, not progress, -len(after),
                            child.steps)
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
                push(target, trial, node.added | new, node.keys, t, None)
            # Keys: the closest reachable pickup spot for each key not yet held.
            for key, spots in key_items.items():
                if key in node.keys:
                    continue
                here = [pt for p, pt in spots if p in node.reach]
                if here:
                    spot = min(here, key=lambda pt: math.dist(node.pos, pt))
                    push(spot, node.states, node.added, node.keys | {key}, None, key)
        return None

    def _upper_bound(self, start: int, states: HeightStates, keys: frozenset,
                     key_items: KeyItems, triggers: list[TriggerLine]) -> set[int]:
        """Everything reachable if every trigger we can get to were used and every key we can
        get to picked up (in any order)."""
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
                return reach
            keys |= new_keys
            for t in new:
                fired.add(t.line)
                for sector, hs in t.effects.items():
                    states[sector].update(hs)

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
            if isinstance(via, Teleport):
                points += funnel(seg_start, _line_mid(self.map, via.line), portals)
                points.append(None)
                seg_start, portals = via.dest, []
                continue
            portal = mesh.portals[via]
            portals.append(_oriented(portal, mesh.polys[prev_poly].center,
                                     mesh.polys[poly].center))
        points += funnel(seg_start, goal, portals)
        out: list[Point | None] = []
        run: list[Point] = []
        for p in points + [None]:
            if p is None:
                if run:
                    out += self._push_from_corners(run)
                    run = []
                out.append(None)
            else:
                run.append(p)
        return out[:-1]

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

    def _push_from_corners(self, pts: list[Point]) -> list[Point]:
        """The funnel only keeps clear of the ends of portals it crosses; other wall corners can
        still be grazed. Insert detour waypoints PATH_CLEARANCE away from any wall vertex that
        a segment passes too close to."""
        m, mesh = self.map, self.mesh
        out = [pts[0]]

        def corners_near(a: Point, b: Point) -> set[Point]:
            found = set()
            for (x1, y1), (x2, y2) in self._walls_near(a, b, PATH_CLEARANCE):
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
                        and self._wall_clearance(w) >= WAYPOINT_CLEARANCE:
                    segment(a, w, depth + 1)
                    segment(w, b, depth + 1)
                    return
            out.append(b)

        for b in pts[1:]:
            segment(out[-1], b, 0)
        # Nudge waypoints (not the start/goal) that still hug a wall away from it.
        for i in range(1, len(out) - 1):
            out[i] = self._nudge_off_walls(out[i])
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

    def _walls_near(self, a: Point, b: Point, pad: float):
        """Wall lines and solid-thing box edges near the segment a-b."""
        m, mesh, walls, obstacles = self.map, self.mesh, self._wall_lines(), self._obstacle_grid()
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

    def _wall_clearance(self, p: Point) -> float:
        return min((math.dist(p, _closest_point(p, a, b))
                    for a, b in self._walls_near(p, p, PATH_CLEARANCE)), default=math.inf)

    def _nudge_off_walls(self, p: Point) -> Point:
        for _ in range(4):
            worst = None
            for a, b in self._walls_near(p, p, WAYPOINT_CLEARANCE):
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
