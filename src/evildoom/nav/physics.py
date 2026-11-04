"""A minimal player-movement simulator on the level geometry, for checking jumps without the
game engine.

It follows ZDoom 2.8.1 (what ViZDoom runs) per tic, in floating point:
  P_MovePlayer: turn, then thrust along the new facing; only 1/256 of it off the ground
      (sv_aircontrol).
  P_XYMovement: move in sub-steps of at most radius - 1; each sub-step is P_TryMove (the box
      may not straddle a one-sided or blocking line or a solid thing; the opening must be
      tall enough; a step up of at most 24 above the *current* z is allowed, even in the air,
      which is how a falling player lands on a ledge's lip); a blocked move slides along the
      wall (SlideMove: the rest of the move along it becomes the velocity), or against a
      solid thing (y alone, else x alone, the dropped part of the velocity lost). Friction
      only on the ground (including while the box hangs over a ledge).
  P_ZMovement: z += velz; above the floor, gravity (2 on the first tic off a ledge, then 1);
      at or below it, land.
Things it leaves out (monsters, items, sector specials, slopes, 3D floors, exact fixed-point
rounding) don't matter for run-off jumps. Line crossings are only watched for teleports.

`PlayerSim` has the bits of VizDoomEngine's interface that the jump check uses.
"""
import math

from evildoom.motor.follower import Controls, Observation
from evildoom.wad.mapdata import ML_BLOCKING, ML_BLOCKPLAYERS, NO_SIDEDEF
from evildoom.wad.specials import Category, classify_line

from .navmesh import _cells

RADIUS = 16.0
HEIGHT = 56.0
MAX_STEP = 24.0
THRUST = 1.5625 / 50     # per unit of forward/strafe input (50 = running: forwardmove 50 << 8)
AIR_CONTROL = 1 / 256
FRICTION = 0.90625
STOP_SPEED = 0x1000 / 65536
GRAVITY = 1.0
MAX_SUBSTEP = RADIUS - 1


class Teleported(Exception):
    """The player crossed a teleport line (the sim doesn't follow it)."""


class PlayerSim:
    def __init__(self, nav, heights: dict[int, tuple[float, float]] | None = None):
        self.nav = nav
        self.m = nav.map
        self.mesh = nav.mesh
        self.heights = dict(heights) if heights is not None else dict(enumerate(nav.movers.initial))
        start = next(t for t in self.m.things if t.type == 1)
        self.start_angle = float(start.angle)
        self._things = nav._obstacle_grid()
        self._teleports = set()
        for line in self.m.linedefs:
            info = classify_line(line.special) if line.special else None
            if info is not None and info.category is Category.TELEPORT:
                self._teleports.add(line.index)
        self.finished = False
        self.place(start.x, start.y)

    # --- the bits of the engine interface the jump check uses ---------------------------------

    def place(self, x: float, y: float, angle: float | None = None) -> None:
        """Stand still at (x, y) on the floor (like the "warp" console command)."""
        self.x, self.y = float(x), float(y)
        self.angle = self.start_angle if angle is None else angle
        self.vx = self.vy = self.vz = 0.0
        ok, floor, ceil, *_ = self._check(self.x, self.y, -math.inf)
        self.floorz, self.ceilz = floor, ceil
        self.z = floor
        self.tic = 0

    def observe(self) -> Observation:
        return Observation(self.x, self.y, self.angle, self.heights, self.tic, frozenset(),
                           self.z)

    def step(self, c: Controls) -> None:
        # P_MovePlayer
        self.angle = (self.angle - c.turn) % 360
        onground = self.z <= self.floorz
        if c.forward or c.strafe:
            k = THRUST * (1 if onground else AIR_CONTROL)
            a = math.radians(self.angle)
            self.vx += k * (c.forward * math.cos(a) + c.strafe * math.sin(a))
            self.vy += k * (c.forward * math.sin(a) - c.strafe * math.cos(a))
        oldfloorz = self.floorz
        self._xy_move(bool(c.forward or c.strafe))
        self._z_move(oldfloorz)
        self.tic += 1

    # --- movement -----------------------------------------------------------------------------

    def _xy_move(self, pushing: bool) -> None:
        if self.vx == 0 and self.vy == 0:
            return
        speed = max(abs(self.vx), abs(self.vy))
        steps = 1 + int(speed // MAX_SUBSTEP) if speed > MAX_SUBSTEP else 1
        for _ in range(steps):
            dx, dy = self.vx / steps, self.vy / steps
            self._thing_hit = False
            if not self._try_move(self.x + dx, self.y + dy):
                if self._thing_hit:
                    # Sliding against another actor (P_XYMovement): y alone, else x alone,
                    # and the dropped part of the velocity is gone for good (E4M2: a barrel
                    # clipping a coasting jump stops it dead sideways).
                    if self._try_move(self.x, self.y + dy):
                        self.vx = 0.0
                    elif self._try_move(self.x + dx, self.y):
                        self.vy = 0.0
                    else:
                        self.vx = self.vy = 0.0
                else:
                    self._slide(dx, dy, steps)
                if self.vx == 0 and self.vy == 0:
                    break
        # friction
        if self.z > self.floorz:
            return  # none in the air (ZDoom's "keep sliding off a step" is for corpses only)
        if abs(self.vx) < STOP_SPEED and abs(self.vy) < STOP_SPEED and not pushing:
            self.vx = self.vy = 0.0
        else:
            self.vx *= FRICTION
            self.vy *= FRICTION

    def _slide(self, dx: float, dy: float, steps: int) -> None:
        """Blocked: FSlide::SlideMove. Go as far as we fit towards the wall in the way (less
        a 1/32 fudge), then the rest of the move projected along the wall, which also becomes
        the velocity (so a glancing hit keeps little speed); up to two walls. With no wall to
        slide along (a corner, or a third wall): y alone, else x alone, velocity untouched."""
        for _ in range(2):
            line = self._blocking
            if line is None:
                break
            lo, hi = 0.0, 1.0
            for _ in range(8):
                mid = (lo + hi) / 2
                if self._fits(self.x + dx * mid, self.y + dy * mid) is not None:
                    lo = mid
                else:
                    hi = mid
            frac = lo - 1 / 32
            if frac > 0 and not self._try_move(self.x + dx * frac, self.y + dy * frac):
                break
            rest = 1 - max(frac, 0) - 1 / 32 if frac > 0 else 1 - lo
            if rest <= 0:
                return
            a, b = self.m.vertexes[line.v1], self.m.vertexes[line.v2]
            lx, ly = b.x - a.x, b.y - a.y
            n = math.hypot(lx, ly) or 1.0
            lx, ly = lx / n, ly / n
            along = (dx * lx + dy * ly) * rest
            dx, dy = along * lx, along * ly
            self.vx, self.vy = dx * steps, dy * steps
            self._blocking = None
            if self._try_move(self.x + dx, self.y + dy):
                return
        if not self._try_move(self.x, self.y + dy):
            self._try_move(self.x + dx, self.y)

    def _fits(self, x: float, y: float):
        """(floorz, ceilingz) if the box can move to (x, y) from where it is, else None
        (and _blocking is the line in the way, if there is one)."""
        ok, floor, ceil, line, floor_line, ceil_line = self._check(x, y, self.z)
        self._blocking = line
        if not ok:
            return None
        if ceil - floor < HEIGHT or ceil - self.z < HEIGHT:
            self._blocking = ceil_line
            return None
        if floor - self.z > MAX_STEP:
            self._blocking = floor_line
            return None
        return floor, ceil

    def _try_move(self, x: float, y: float) -> bool:
        fit = self._fits(x, y)
        if fit is None:
            return False
        floor, ceil = fit
        self._crossings(x, y)
        if self.z < floor:  # step up (also how a falling player lands on a lip)
            self.z = floor
            if self.vz < 0:
                self.vz = 0.0
        self.x, self.y, self.floorz, self.ceilz = x, y, floor, ceil
        return True

    def _z_move(self, oldfloorz: float) -> None:
        self.z += self.vz
        if self.z > self.floorz:
            if self.vz == 0 and oldfloorz > self.floorz and self.z == oldfloorz:
                self.vz -= 2 * GRAVITY
            else:
                self.vz -= GRAVITY
        if self.z <= self.floorz:
            self.z = self.floorz
            if self.vz < 0:
                self.vz = 0.0
        if self.z + HEIGHT > self.ceilz:
            self.z = self.ceilz - HEIGHT
            if self.vz > 0:
                self.vz = 0.0

    # --- geometry -----------------------------------------------------------------------------

    def _center_floor(self) -> float:
        p = self.mesh.poly_at(self.x, self.y)
        return self.heights[p.sector][0] if p is not None else -math.inf

    def _check(self, x: float, y: float, z: float):
        """P_CheckPosition for the box at (x, y): (ok, floorz, ceilingz, blocking line or
        None, the line that set floorz, the one that set ceilingz). floorz/ceilingz: the sector
        under the centre, raised/lowered by every two-sided line the box straddles."""
        p = self.mesh.poly_at(x, y)
        if p is None:
            return False, 0.0, 0.0, None, None, None
        floor, ceil = self.heights[p.sector]
        floor_line = ceil_line = None
        r = RADIUS
        x0, y0, x1, y1 = x - r, y - r, x + r, y + r
        m = self.m
        seen = set()
        for cell in _cells(x0, y0, x1, y1):
            for li in self.mesh._line_grid.get(cell, ()):
                if li in seen:
                    continue
                seen.add(li)
                line = m.linedefs[li]
                a, b = m.vertexes[line.v1], m.vertexes[line.v2]
                if max(a.x, b.x) <= x0 or min(a.x, b.x) >= x1 \
                        or max(a.y, b.y) <= y0 or min(a.y, b.y) >= y1:
                    continue
                if not _box_straddles(a.x, a.y, b.x, b.y, x0, y0, x1, y1):
                    continue
                if line.back == NO_SIDEDEF or line.flags & (ML_BLOCKING | ML_BLOCKPLAYERS):
                    return False, floor, ceil, line, None, None
                fa = self.heights[m.front_sector(line)]
                fb = self.heights[m.back_sector(line)]
                if max(fa[0], fb[0]) > floor:
                    floor, floor_line = max(fa[0], fb[0]), line
                if min(fa[1], fb[1]) < ceil:
                    ceil, ceil_line = min(fa[1], fb[1]), line
        for cell in _cells(x0, y0, x1, y1):
            for (ax, ay), (bx, by) in self._things.get(cell, ()):
                if max(ax, bx) > x0 and min(ax, bx) < x1 and max(ay, by) > y0 \
                        and min(ay, by) < y1:
                    self._thing_hit = True
                    return False, floor, ceil, None, None, None
        return True, floor, ceil, None, floor_line, ceil_line

    def _crossings(self, x: float, y: float) -> None:
        if not self._teleports:
            return
        # As P_TryMove's spechit: a line the box (at the new spot) touches fires when the
        # centre goes from its front to its back side of the *infinite* line, so walking past
        # the end of a teleport line can fire it (E4M3: past the corner of a teleporter pad).
        m = self.m
        r = RADIUS
        x0, y0, x1, y1 = x - r, y - r, x + r, y + r
        for cell in _cells(x0, y0, x1, y1):
            for li in self.mesh._line_grid.get(cell, ()):
                if li not in self._teleports:
                    continue
                line = m.linedefs[li]
                a, b = m.vertexes[line.v1], m.vertexes[line.v2]
                if max(a.x, b.x) <= x0 or min(a.x, b.x) >= x1 \
                        or max(a.y, b.y) <= y0 or min(a.y, b.y) >= y1 \
                        or not _box_straddles(a.x, a.y, b.x, b.y, x0, y0, x1, y1):
                    continue
                s0 = (b.x - a.x) * (self.y - a.y) - (b.y - a.y) * (self.x - a.x)
                s1 = (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
                if s0 < 0 <= s1:  # front (right of v1->v2) to back: teleports only fire so
                    raise Teleported


def _box_straddles(ax, ay, bx, by, x0, y0, x1, y1) -> bool:
    """P_BoxOnLineSide == -1: the box has corners on both sides of the (infinite) line."""
    dx, dy = bx - ax, by - ay
    sides = set()
    for cx, cy in ((x0, y0), (x1, y0), (x1, y1), (x0, y1)):
        c = dx * (cy - ay) - dy * (cx - ax)
        sides.add(c > 0)
        if len(sides) == 2:
            return True
    return False
