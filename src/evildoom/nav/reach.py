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
from dataclasses import dataclass, field

from evildoom.wad.mapdata import ML_BLOCKING, ML_BLOCKPLAYERS, MapData
from evildoom.wad.specials import (Category, Key, LineSpecial, Lock, Trigger, classify_line,
                                   classify_sector)

from .movers import Heights, MoverModel
from .navmesh import NavMesh, Point

PLAYER_HEIGHT = 56
MAX_STEP = 24
PLAYER_RADIUS = 16
ITEM_RADIUS = 20
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
    """One leg of a planned route: walk `path`, then activate `trigger` (None = final leg)."""
    trigger: TriggerLine | None
    path: Path


@dataclass
class Route:
    legs: list[Leg]
    states: HeightStates  # predicted heights once the route is done


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
                    activators = touching_front.get(line.index, set())
                case Trigger.W1 | Trigger.WR:
                    activators = crossing.get(line.index, set()) \
                        | touching_any.get(line.index, set())
                case _:  # gun triggers: approximated as "standing next to it"
                    activators = touching_any.get(line.index, set())
            sectors = self._affected(line.index, info)
            out.append(TriggerLine(line.index, info, sectors,
                                   self.movers.effects(line.special, sectors), activators))
        return out + self._boss_triggers()

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
        key_things = [t for t in self.map.things if t.type in Key._value2member_map_]
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
                   states: HeightStates | None = None, max_expansions: int = 300
                   ) -> Route | None:
        """Plan a route that may need remote triggers (switches, walk-over lines) first.

        Best-first search over (triggers used, where we're standing). A trigger is worth
        trying if using it opens up new area *as seen from where we'd stand afterwards* (some
        triggers sit past one-way drops and strand the player). Nodes that can reach the goal
        come first, then the ones with the most reachable area, so it behaves greedily until it
        hits a dead end, then backtracks. Doors opened by use are handled inside find_path.
        `states` defaults to the map's initial heights; at runtime pass the live heights.
        """
        mesh = self.mesh
        doors = self.local_doors(set(keys))
        remote = [t for t in self.triggers
                  if t.info.trigger not in (Trigger.D1, Trigger.DR) and t.effects
                  and t.info.lock.satisfied(set(keys))]
        goal_poly, start_poly = mesh.poly_at(*goal), mesh.poly_at(*start)
        if goal_poly is None or start_poly is None:
            return None

        @dataclass
        class Node:
            pos: Point
            states: HeightStates
            fired: tuple[int, ...]
            legs: list[Leg]
            reach: set[int]
            pending: tuple[Point, HeightStates, TriggerLine] | None = None  # leg to path

        states0 = {s: set(h) for s, h in (states or self.initial_states()).items()}
        if goal_poly.index not in self._upper_bound(start_poly.index, states0, doors, remote):
            return None  # unreachable even if every reachable trigger fired: don't search
        counter = itertools.count()
        root = Node(start, states0, (), [], self.reachable(start_poly.index, states0, doors))
        heap = [((False, 0, 0), next(counter), root)]
        seen: set[tuple[frozenset, frozenset]] = set()
        expansions = 0
        while heap and expansions < max_expansions:
            _, _, node = heapq.heappop(heap)
            sig = (frozenset(node.fired), frozenset(node.reach))
            if sig in seen:
                continue
            seen.add(sig)
            if node.pending:  # path the leg that got us here, now that we've picked this node
                prev_pos, prev_states, t = node.pending
                path = self.find_path(prev_pos, node.pos, prev_states, doors)
                if path is None:
                    continue
                node.legs = node.legs + [Leg(t, path)]
            expansions += 1
            if goal_poly.index in node.reach:
                path = self.find_path(node.pos, goal, node.states, doors)
                if path:
                    return Route(node.legs + [Leg(None, path)], node.states)
            for t in remote:
                if t.line in node.fired or not t.activators & node.reach:
                    continue
                trial = {s: set(h) for s, h in node.states.items()}
                for sector, hs in t.effects.items():
                    trial[sector].update(hs)
                target = self._activation_point(t, node.pos, node.reach)
                target_poly = mesh.poly_at(*target)
                if target_poly is None:
                    continue
                after = self.reachable(target_poly.index, trial, doors)
                if not after - node.reach:
                    continue
                child = Node(target, trial, node.fired + (t.line,), node.legs, after,
                             (node.pos, node.states, t))
                priority = (goal_poly.index not in after, -len(after), len(child.fired))
                heapq.heappush(heap, (priority, next(counter), child))
        return None

    def _upper_bound(self, start: int, states: HeightStates, doors: Doors,
                     triggers: list[TriggerLine]) -> set[int]:
        """Everything reachable if every trigger we can get to were fired (in any order)."""
        states = {s: set(h) for s, h in states.items()}
        fired: set[int] = set()
        while True:
            reach = self.reachable(start, states, doors)
            new = [t for t in triggers if t.line not in fired and t.activators & reach]
            if not new:
                return reach
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
        return points

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


def _closest_on_segment(p: Point, portal) -> Point:
    """Closest point to p on the portal, kept PLAYER_RADIUS away from its ends if wide enough."""
    (x1, y1), (x2, y2) = portal.p1, portal.p2
    length = math.dist(portal.p1, portal.p2)
    if length <= 2 * PLAYER_RADIUS:
        return portal.mid
    t = ((p[0] - x1) * (x2 - x1) + (p[1] - y1) * (y2 - y1)) / (length * length)
    margin = PLAYER_RADIUS / length
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
    if length <= 2 * PLAYER_RADIUS:
        return portal.mid, portal.mid
    t = PLAYER_RADIUS / length
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
