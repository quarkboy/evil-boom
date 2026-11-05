"""Navigation mesh built from the map's BSP (by the native core: native/src/mesh.rs).

Each subsector is a convex region. Vanilla nodes don't store the "minisegs" along BSP
partition lines, so subsector polygons are reconstructed by clipping the map bounds down the
BSP tree (partition lines), then by the subsector's own segs (walls). Adjacent polygons share
an edge; those shared edges are the portals between them.

The mesh is purely topological/geometric. Whether a portal can be crossed depends on sector
heights at the time, which is decided elsewhere (see reach.py).
"""
import math
from collections import defaultdict
from dataclasses import dataclass, field

from evildoom.wad.mapdata import NF_SUBSECTOR, MapData

Point = tuple[float, float]
EPS = 0.01


@dataclass
class NavPoly:
    index: int          # == subsector index
    sector: int
    vertices: list[Point]
    center: Point
    lines_front: set[int] = field(default_factory=set)  # linedefs this poly touches, front side
    lines_back: set[int] = field(default_factory=set)
    portals: list[int] = field(default_factory=list)   # indices into NavMesh.portals


@dataclass
class Portal:
    index: int
    a: int              # poly index
    b: int
    p1: Point
    p2: Point
    linedef: int | None  # two-sided linedef crossed, None for an open BSP split

    @property
    def mid(self) -> Point:
        return (self.p1[0] + self.p2[0]) / 2, (self.p1[1] + self.p2[1]) / 2

    @property
    def width(self) -> float:
        return math.dist(self.p1, self.p2)

    def other(self, poly: int) -> int:
        return self.b if poly == self.a else self.a


class NavMesh:
    def __init__(self, m: MapData, obstacles: list[tuple[float, float, float]] = ()):
        """Built by the native core (evildoom_native.NativeMesh); here are its polys and
        portals, and the lookups the Python side needs. `obstacles`: (x, y, half-width) boxes
        cut out of the mesh, so it only covers where the player's centre can be (solid things,
        grown by the player's half-width)."""
        if not m.nodes and len(m.subsectors) > 1:
            raise ValueError(f"{m.name}: no usable BSP nodes (extended node formats unsupported)")
        import evildoom_native
        self.map = m
        built = evildoom_native.NativeMesh(
            [(v.x, v.y) for v in m.vertexes],
            [(l.v1, l.v2, l.flags, l.special, l.tag, l.front, l.back) for l in m.linedefs],
            [sd.sector for sd in m.sidedefs],
            [(sg.v1, sg.v2, sg.linedef, sg.direction) for sg in m.segs],
            [(ss.seg_count, ss.first_seg) for ss in m.subsectors],
            [(n.x, n.y, n.dx, n.dy, n.front_child, n.back_child) for n in m.nodes],
            [tuple(map(float, b)) for b in obstacles])
        self.polys: list[NavPoly | None] = [
            None if p is None else NavPoly(i, p[0], list(p[1]), p[2], set(p[3]), set(p[4]),
                                           list(p[5]))
            for i, p in enumerate(built.polys())]
        self.portals: list[Portal] = [Portal(i, a, b, p1, p2, li)
                                      for i, (a, b, p1, p2, li) in enumerate(built.portals())]
        self._pieces: dict[int, list[int]] = built.pieces()  # subsector -> its polys, if cut
        self._line_grid: dict[tuple[int, int], list[int]] = defaultdict(list)
        for line in m.linedefs:
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            for cell in _cells(a.x, a.y, b.x, b.y):
                self._line_grid[cell].append(line.index)

    # --- queries ------------------------------------------------------------------------

    def poly_at(self, x: float, y: float) -> NavPoly | None:
        """Walk the BSP like R_PointInSubsector."""
        nodes = self.__dict__.get("_flat_nodes")
        if nodes is None:
            # the hottest lookup there is (jump finding, path fitting, the movement sim):
            # plain tuples, the side test inlined
            nodes = self._flat_nodes = [(n.x, n.y, n.dx, n.dy, n.front_child, n.back_child)
                                        for n in self.map.nodes]
        if not nodes:
            return self.polys[0]
        child = len(nodes) - 1
        while not child & NF_SUBSECTOR:
            nx, ny, ndx, ndy, front, back = nodes[child]
            child = front if ndx * (y - ny) - ndy * (x - nx) < 0 else back
        ss = child & ~NF_SUBSECTOR
        group = self._pieces.get(ss)
        if group is None:
            return self.polys[ss]
        # cut up by obstacles: the piece holding the point (else the nearest: on an obstacle's
        # edge, or inside one, which the player's centre can't really be)
        best = None
        for i in group:
            vs = self.polys[i].vertices
            if _inside(vs, x, y):
                return self.polys[i]
            d = min(math.dist((x, y), q) for q in vs)
            if best is None or d < best[0]:
                best = (d, i)
        return self.polys[best[1]] if best else None

    def neighbors_of_sector(self, sector: int) -> set[int]:
        m = self.map
        out = set()
        for line in m.sector_lines(sector):
            for s in (m.front_sector(line), m.back_sector(line)):
                if s is not None and s != sector:
                    out.add(s)
        return out


GRID = 128


def _inside(vs: list[Point], x: float, y: float) -> bool:
    """Is the point inside (or on) the convex polygon?"""
    signs = set()
    for (ax, ay), (bx, by) in zip(vs, vs[1:] + vs[:1]):
        c = (bx - ax) * (y - ay) - (by - ay) * (x - ax)
        if abs(c) > EPS:
            signs.add(c > 0)
    return len(signs) <= 1


def _cells(x0: float, y0: float, x1: float, y1: float):
    """Grid cells covered by a segment's bounding box."""
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    for gx in range(int(x0 // GRID), int(x1 // GRID) + 1):
        for gy in range(int(y0 // GRID), int(y1 // GRID) + 1):
            yield gx, gy


