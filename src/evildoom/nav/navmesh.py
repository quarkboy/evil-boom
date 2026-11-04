"""Navigation mesh built from the map's BSP.

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


def _cross(ox, oy, dx, dy, px, py) -> float:
    """Cross product of direction (dx, dy) from origin with point. < 0 = right/front side."""
    return dx * (py - oy) - dy * (px - ox)


def clip_front(poly: list[Point], ox, oy, dx, dy, front: bool) -> list[Point]:
    """Sutherland-Hodgman: keep the part of a convex polygon on one side of a line."""
    sign = -1 if front else 1
    out: list[Point] = []
    n = len(poly)
    for i in range(n):
        a, b = poly[i], poly[(i + 1) % n]
        da = sign * _cross(ox, oy, dx, dy, *a)
        db = sign * _cross(ox, oy, dx, dy, *b)
        if da >= -EPS:
            out.append(a)
        if (da > EPS and db < -EPS) or (da < -EPS and db > EPS):
            t = da / (da - db)
            out.append((a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])))
    return out


def area(poly: list[Point]) -> float:
    return abs(sum(a[0] * b[1] - b[0] * a[1]
                   for a, b in zip(poly, poly[1:] + poly[:1]))) / 2


def centroid(poly: list[Point]) -> Point:
    a = cx = cy = 0.0
    for p, q in zip(poly, poly[1:] + poly[:1]):
        c = p[0] * q[1] - q[0] * p[1]
        a += c
        cx += (p[0] + q[0]) * c
        cy += (p[1] + q[1]) * c
    if abs(a) < 1e-9:
        return sum(p[0] for p in poly) / len(poly), sum(p[1] for p in poly) / len(poly)
    return cx / (3 * a), cy / (3 * a)


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
        """`obstacles`: (x, y, half-width) boxes cut out of the mesh, so it only covers where
        the player's centre can be (solid things, grown by the player's half-width)."""
        if not m.nodes and len(m.subsectors) > 1:
            raise ValueError(f"{m.name}: no usable BSP nodes (extended node formats unsupported)")
        self.map = m
        self.polys: list[NavPoly | None] = []
        self.portals: list[Portal] = []
        self._pieces: dict[int, list[int]] = {}   # subsector -> its polys, where cut up
        self._line_grid: dict[tuple[int, int], list[int]] = defaultdict(list)
        for line in m.linedefs:
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            for cell in _cells(a.x, a.y, b.x, b.y):
                self._line_grid[cell].append(line.index)
        self._build_polys()
        if obstacles:
            self._cut_obstacles(obstacles)
        self._attach_lines()
        self._build_portals()

    def _cut_obstacles(self, boxes: list[tuple[float, float, float]]) -> None:
        """Remove axis-aligned boxes from the polys. A cut poly is replaced by the convex
        pieces outside the box (left, right, then below and above the box between those);
        poly indices beyond the subsector count are the extra pieces."""
        def outside(vs: list[Point], box) -> list[list[Point]]:
            x0, x1, y0, y1 = box
            # clip_front(..., front=True) keeps x > ox for a line pointing +y, and y < oy for
            # a line pointing +x
            left = clip_front(vs, x0, 0, 0, 1, False)
            rest = clip_front(vs, x0, 0, 0, 1, True)
            right = clip_front(rest, x1, 0, 0, 1, True)
            middle = clip_front(rest, x1, 0, 0, 1, False)
            below = clip_front(middle, 0, y0, 1, 0, True)
            above = clip_front(middle, 0, y1, 1, 0, False)
            return [p for p in (left, right, below, above) if len(p) >= 3 and area(p) >= 1.0]

        rects = [(x - r, x + r, y - r, y + r) for x, y, r in boxes]
        for ss in range(len(self.polys)):
            poly = self.polys[ss]
            if poly is None:
                continue
            xs = [v[0] for v in poly.vertices]
            ys = [v[1] for v in poly.vertices]
            hits = [b for b in rects if b[0] < max(xs) and b[1] > min(xs)
                    and b[2] < max(ys) and b[3] > min(ys)]
            if not hits:
                continue
            pieces = [poly.vertices]
            for box in hits:
                pieces = [q for p in pieces for q in outside(p, box)]
            group = []
            for n, p in enumerate(pieces):
                i = ss if n == 0 else len(self.polys)
                piece = NavPoly(i, poly.sector, p, centroid(p))
                if n == 0:
                    self.polys[ss] = piece
                else:
                    self.polys.append(piece)
                group.append(i)
            if not pieces:
                self.polys[ss] = None
            self._pieces[ss] = group

    # --- construction -------------------------------------------------------------------

    def _build_polys(self) -> None:
        m = self.map
        xs = [v.x for v in m.vertexes]
        ys = [v.y for v in m.vertexes]
        pad = 64
        bounds = [(min(xs) - pad, min(ys) - pad), (max(xs) + pad, min(ys) - pad),
                  (max(xs) + pad, max(ys) + pad), (min(xs) - pad, max(ys) + pad)]
        cells: dict[int, list[Point]] = {}

        def descend(child: int, poly: list[Point]) -> None:
            if child & NF_SUBSECTOR:
                cells[child & ~NF_SUBSECTOR] = poly
                return
            node = m.nodes[child]
            descend(node.front_child, clip_front(poly, node.x, node.y, node.dx, node.dy, True))
            descend(node.back_child, clip_front(poly, node.x, node.y, node.dx, node.dy, False))

        if m.nodes:
            descend(len(m.nodes) - 1, bounds)
        else:
            cells[0] = bounds

        for i, ss in enumerate(m.subsectors):
            poly = cells.get(i, [])
            segs = m.segs[ss.first_seg:ss.first_seg + ss.seg_count]
            for seg in segs:
                a, b = m.vertexes[seg.v1], m.vertexes[seg.v2]
                if len(poly) >= 3:
                    poly = clip_front(poly, a.x, a.y, b.x - a.x, b.y - a.y, True)
            if len(poly) < 3 or area(poly) < 1.0:
                self.polys.append(None)
                continue
            line = m.linedefs[segs[0].linedef]
            side = line.back if segs[0].direction else line.front
            nav = NavPoly(i, m.sidedefs[side].sector, poly, centroid(poly))
            for seg in segs:
                (nav.lines_back if seg.direction else nav.lines_front).add(seg.linedef)
            self.polys.append(nav)

    def _attach_lines(self) -> None:
        """Attach linedefs to every poly with an edge lying along them, not just the polys that
        own their segs: nodebuilders sometimes put a seg in a zero-area subsector on a
        partition line, leaving the real neighbouring area without it."""
        m = self.map
        for poly in self.polys:
            if poly is None:
                continue
            vs = poly.vertices
            for p, q in zip(vs, vs[1:] + vs[:1]):
                for li in self.lines_on_edge(p, q):
                    line = m.linedefs[li]
                    a, b = m.vertexes[line.v1], m.vertexes[line.v2]
                    front = _cross(a.x, a.y, b.x - a.x, b.y - a.y, *poly.center) < 0
                    (poly.lines_front if front else poly.lines_back).add(li)

    def lines_on_edge(self, p: Point, q: Point) -> list[int]:
        """Linedefs collinear with and overlapping the segment p-q."""
        m = self.map
        candidates = {li for cell in _cells(*p, *q) for li in self._line_grid.get(cell, ())}
        out = []
        for li in candidates:
            line = m.linedefs[li]
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            if _shared_segment((a.x, a.y), (b.x, b.y), p, q) is not None:
                out.append(li)
        return out

    def _build_portals(self) -> None:
        """Find pairs of polygon edges that are collinear and overlapping."""
        grid: dict[tuple[int, int], list[tuple[int, Point, Point]]] = defaultdict(list)
        for poly in self.polys:
            if poly is None:
                continue
            vs = poly.vertices
            for p, q in zip(vs, vs[1:] + vs[:1]):
                if math.dist(p, q) < 1.0:
                    continue
                for cell in _cells(*p, *q):
                    grid[cell].append((poly.index, p, q))

        seen: set[tuple[int, int]] = set()
        for bucket in grid.values():
            for i, (pa, a1, a2) in enumerate(bucket):
                for pb, b1, b2 in bucket[i + 1:]:
                    if pa == pb or (min(pa, pb), max(pa, pb)) in seen:
                        continue
                    overlap = _shared_segment(a1, a2, b1, b2)
                    if overlap is None:
                        continue
                    seen.add((min(pa, pb), max(pa, pb)))
                    self._add_portal(pa, pb, *overlap)

    def _add_portal(self, a: int, b: int, p1: Point, p2: Point) -> None:
        """Add the portal(s) for a shared edge: one per two-sided linedef it covers (lines can
        differ in flags and specials), or a single open portal for a plain BSP split."""
        m = self.map
        pa, pb = self.polys[a], self.polys[b]
        sectors = {pa.sector, pb.sector}
        pieces: list[tuple[Point, Point, int | None]] = []
        for li in self.lines_on_edge(p1, p2):
            line = m.linedefs[li]
            if line.two_sided and {m.front_sector(line), m.back_sector(line)} == sectors:
                v1, v2 = m.vertexes[line.v1], m.vertexes[line.v2]
                overlap = _shared_segment(p1, p2, (v1.x, v1.y), (v2.x, v2.y))
                if overlap:
                    pieces.append((*overlap, li))
        if not pieces:
            pieces = [(p1, p2, None)]
        for q1, q2, linedef in pieces:
            portal = Portal(len(self.portals), a, b, q1, q2, linedef)
            self.portals.append(portal)
            pa.portals.append(portal.index)
            pb.portals.append(portal.index)

    # --- queries ------------------------------------------------------------------------

    def poly_at(self, x: float, y: float) -> NavPoly | None:
        """Walk the BSP like R_PointInSubsector."""
        m = self.map
        if not m.nodes:
            return self.polys[0]
        child = len(m.nodes) - 1
        while not child & NF_SUBSECTOR:
            node = m.nodes[child]
            front = _cross(node.x, node.y, node.dx, node.dy, x, y) < 0
            child = node.front_child if front else node.back_child
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


def _shared_segment(a1: Point, a2: Point, b1: Point, b2: Point,
                    tol: float = 2.0) -> tuple[Point, Point] | None:
    """Overlap of two collinear segments, or None. The tolerance absorbs nodebuilders
    rounding split vertices to integer coordinates (up to ~1 unit off the original line)."""
    dx, dy = a2[0] - a1[0], a2[1] - a1[1]
    length = math.hypot(dx, dy)
    if length < 1e-6:
        return None
    for p in (b1, b2):
        if abs(_cross(a1[0], a1[1], dx, dy, *p)) / length > tol:
            return None
    ux, uy = dx / length, dy / length
    t_b1 = (b1[0] - a1[0]) * ux + (b1[1] - a1[1]) * uy
    t_b2 = (b2[0] - a1[0]) * ux + (b2[1] - a1[1]) * uy
    lo, hi = max(0.0, min(t_b1, t_b2)), min(length, max(t_b1, t_b2))
    if hi - lo < 1.0:
        return None
    return (a1[0] + ux * lo, a1[1] + uy * lo), (a1[0] + ux * hi, a1[1] + uy * hi)
