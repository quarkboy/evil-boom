"""Execute a planned level route tic by tic.

Engine-independent: each tic the follower gets an Observation (position, angle, live sector
heights) and returns Controls. Live heights decide whether the next portal on the path can be
crossed right now. If not, the follower opens the door in front of it, or waits for the lift
or door to arrive, and as a last resort uses a nearby trigger for that sector.
"""
import math
from dataclasses import dataclass, field

from evildoom.nav.reach import Navigator, _closest_on_segment, can_cross
from evildoom.nav.walkthrough import Step
from evildoom.wad.specials import Key, Trigger

TICRATE = 35
REACHED = 12.0          # waypoint arrival radius
FINAL_REACHED = 10.0
BLOCK_CHECK_DIST = 48.0  # how close to a portal before live heights are checked
MAX_TURN = 25.0          # degrees per tic
USE_EVERY = 15           # tics between use presses while waiting at a door
STUCK_TICS = 2 * TICRATE
GIVE_UP_TICS = 15 * TICRATE


@dataclass
class Observation:
    x: float
    y: float
    angle: float                                  # degrees, 0 = east, counter-clockwise
    heights: dict[int, tuple[float, float]]       # live sector (floor, ceiling)
    tic: int = 0


@dataclass
class Controls:
    forward: float = 0.0   # -50..50
    strafe: float = 0.0    # positive = right
    turn: float = 0.0      # degrees, positive = right (clockwise)
    use: bool = False
    attack: bool = False
    run: bool = True


@dataclass
class FollowerLog:
    events: list[str] = field(default_factory=list)

    def add(self, tic: int, msg: str) -> None:
        self.events.append(f"[{tic / TICRATE:6.1f}s] {msg}")


def _norm(deg: float) -> float:
    return (deg + 180) % 360 - 180


class RouteFollower:
    def __init__(self, nav: Navigator, steps: list[Step]):
        self.nav = nav
        self.steps = steps
        self.doors = nav.local_doors(set(Key))  # the plan only routes through doors we can open
        self.log = FollowerLog()
        self.step_i = -1
        self.done = False
        self.failed = False
        self._last_pos: tuple[float, float] | None = None
        self._next_step(0)

    # --- step / segment bookkeeping ----------------------------------------------------

    def _next_step(self, tic: int) -> None:
        self.step_i += 1
        if self.step_i >= len(self.steps):
            self.done = True
            self.log.add(tic, "route finished")
            return
        step = self.steps[self.step_i]
        self.log.add(tic, f"step {self.step_i + 1}/{len(self.steps)}: walk, then "
                          f"{step.action} ({step.label})")
        self.segments = self._split(step)
        self.seg_i, self.wp_i = 0, 1
        self.poly_i = 0
        self.phase = "walk"
        self._reset_progress(tic)

    def _split(self, step: Step) -> list[list[tuple[float, float]]]:
        """Waypoint segments split at teleports. Segments that end at a teleport line are
        extended a little so the player actually crosses it."""
        segs, cur = [], []
        for p in step.path.waypoints:
            if p is None:
                if len(cur) >= 2:
                    (ax, ay), (bx, by) = cur[-2], cur[-1]
                    d = max(math.dist(cur[-2], cur[-1]), 1)
                    cur[-1] = (bx + (bx - ax) / d * 24, by + (by - ay) / d * 24)
                segs.append(cur)
                cur = []
            else:
                cur.append(p)
        segs.append(cur)
        return segs

    def _reset_progress(self, tic: int) -> None:
        self._best_dist = math.inf
        self._progress_tic = tic
        self._wait_tic: int | None = None
        self._action_tic: int | None = None
        self._mover_heights: tuple[float, float] | None = None
        self._mover_still_since = tic
        self._last_use = -USE_EVERY

    # --- main entry ---------------------------------------------------------------------

    def act(self, obs: Observation) -> Controls:
        if self.done or self.failed:
            return Controls()
        pos = (obs.x, obs.y)
        if self._last_pos and math.dist(pos, self._last_pos) > 64 \
                and self.seg_i + 1 < len(self.segments):
            self.log.add(obs.tic, "teleported")
            self.seg_i, self.wp_i = self.seg_i + 1, 1
            self._reset_progress(obs.tic)
        self._last_pos = pos
        if self.phase == "walk":
            return self._walk(obs)
        return self._act_at_goal(obs)

    # --- walking ------------------------------------------------------------------------

    def _walk(self, obs: Observation) -> Controls:
        step = self.steps[self.step_i]
        seg = self.segments[self.seg_i]
        pos = (obs.x, obs.y)

        blocked = self._check_blocked(obs)
        if blocked is not None:
            return blocked

        if self.wp_i >= len(seg):
            if self.seg_i + 1 < len(self.segments):  # waiting for a teleport to happen
                return self._steer(obs, seg[-1], final=False)
            self.phase = "act"
            self._reset_progress(obs.tic)
            return Controls()
        target = seg[self.wp_i]
        final = self.wp_i == len(seg) - 1 and self.seg_i == len(self.segments) - 1
        dist = math.dist(pos, target)
        passed = False
        if not final and self.wp_i + 1 < len(seg) and dist < 3 * REACHED:
            # already beyond the waypoint in the direction of the next one
            nx, ny = seg[self.wp_i + 1]
            passed = (nx - target[0]) * (pos[0] - target[0]) \
                + (ny - target[1]) * (pos[1] - target[1]) > 0
        if passed or dist < (FINAL_REACHED if final else REACHED):
            self.wp_i += 1
            self._reset_progress(obs.tic)
            return self._walk(obs)

        if dist < self._best_dist - 1:
            self._best_dist, self._progress_tic = dist, obs.tic
        stalled = obs.tic - self._progress_tic
        if stalled > GIVE_UP_TICS:
            self.failed = True
            self.log.add(obs.tic, f"STUCK at ({obs.x:.0f}, {obs.y:.0f}) heading for "
                                  f"({target[0]:.0f}, {target[1]:.0f}) during step "
                                  f"{self.step_i + 1} ({step.label})")
            return Controls()
        controls = self._steer(obs, target, final)
        if stalled > STUCK_TICS:
            # wiggle sideways to get unsnagged from a corner
            controls.strafe = 30 if (obs.tic // 20) % 2 else -30
        return controls

    def _steer(self, obs: Observation, target, final: bool) -> Controls:
        dx, dy = target[0] - obs.x, target[1] - obs.y
        diff = _norm(math.degrees(math.atan2(dy, dx)) - obs.angle)
        turn = max(-MAX_TURN, min(MAX_TURN, -diff))
        dist = math.hypot(dx, dy)
        if abs(diff) > 70:
            forward = 0.0
        elif abs(diff) > 30:
            forward = 15.0
        else:
            forward = 50.0
        if final:
            forward = min(forward, max(8.0, dist))
        return Controls(forward=forward, turn=turn)

    def _face(self, obs: Observation, point) -> tuple[Controls, bool]:
        diff = _norm(math.degrees(math.atan2(point[1] - obs.y, point[0] - obs.x)) - obs.angle)
        return Controls(turn=max(-MAX_TURN, min(MAX_TURN, -diff))), abs(diff) < 10

    def _next_blocked(self, obs: Observation):
        """First crossing on the path ahead, within reach, that live heights don't allow.
        Looks past the immediate next poly: doors often sit behind thin door-track sectors."""
        mesh, path = self.nav.mesh, self.steps[self.step_i].path
        here = mesh.poly_at(obs.x, obs.y)
        if here is None:
            return None
        window = path.polys[self.poly_i:self.poly_i + 12]
        if here.index in window:
            self.poly_i += window.index(here.index)
        for j in range(self.poly_i, min(self.poly_i + 8, len(path.polys) - 1)):
            cur, nxt = mesh.polys[path.polys[j]], mesh.polys[path.polys[j + 1]]
            portal = next((mesh.portals[pi] for pi in cur.portals
                           if mesh.portals[pi].other(cur.index) == nxt.index), None)
            if portal is None:
                return None  # teleport edge
            near = _closest_on_segment((obs.x, obs.y), portal)
            if math.dist(near, (obs.x, obs.y)) > BLOCK_CHECK_DIST:
                return None
            if nxt.sector != cur.sector and \
                    not can_cross(obs.heights[cur.sector], obs.heights[nxt.sector]):
                return cur, nxt, portal
        return None

    def _check_blocked(self, obs: Observation) -> Controls | None:
        """If a crossing just ahead can't be made with live heights, deal with it."""
        blocked = self._next_blocked(obs)
        if blocked is None:
            self._wait_tic = None
            return None
        cur, nxt, portal = blocked
        live = obs.heights

        # Blocked right now.
        if self._wait_tic is None:
            self._wait_tic = obs.tic
            kind = "door" if nxt.sector in self.doors else "mover"
            self.log.add(obs.tic, f"waiting at {kind} (sector {nxt.sector})")
        waited = obs.tic - self._wait_tic
        self._progress_tic = obs.tic  # waiting is not being stuck
        # Pressing use on a moving door reverses it, so only press once things are still.
        if live[nxt.sector] != self._mover_heights:
            self._mover_heights, self._mover_still_since = live[nxt.sector], obs.tic
        still = obs.tic - self._mover_still_since >= USE_EVERY
        controls, facing = self._face(obs, portal.mid)
        if nxt.sector in self.doors and cur.index in self.doors[nxt.sector][1]:
            # Use traces 64 units straight ahead and stops at the first wall: get close and
            # square up to the middle of the door first.
            if math.dist((obs.x, obs.y), portal.mid) > 40:
                controls.forward = 12.0 if facing else 0.0
            else:
                controls.use = facing and still and obs.tic - self._last_use >= USE_EVERY
        elif waited > 3 * TICRATE and still:
            # Lift/door not coming by itself: use a trigger for it we're standing next to.
            for t in self.nav.triggers:
                if t.info.trigger in (Trigger.SR, Trigger.S1, Trigger.DR, Trigger.D1) \
                        and (nxt.sector in t.sectors or cur.sector in t.sectors) \
                        and cur.index in t.activators:
                    line = self.nav.map.linedefs[t.line]
                    a, b = self.nav.map.vertexes[line.v1], self.nav.map.vertexes[line.v2]
                    controls, facing = self._face(obs, ((a.x + b.x) / 2, (a.y + b.y) / 2))
                    controls.use = facing and obs.tic - self._last_use >= USE_EVERY
                    break
        if controls.use:
            self._last_use = obs.tic
        if waited > GIVE_UP_TICS:
            self.failed = True
            self.log.add(obs.tic, f"GAVE UP waiting for sector {nxt.sector}: live "
                                  f"{live[cur.sector]} -> {live[nxt.sector]}")
        return controls

    # --- acting at the end of a step ------------------------------------------------------

    def _act_at_goal(self, obs: Observation) -> Controls:
        step = self.steps[self.step_i]
        if step.action in ("pickup", "cross"):
            self._next_step(obs.tic)
            return Controls()
        if step.action == "kill":
            self.log.add(obs.tic, "skipping boss kill (no monsters in this run)")
            self._next_step(obs.tic)
            return Controls()

        m = self.nav.map
        line = m.linedefs[step.trigger.line]
        a, b = m.vertexes[line.v1], m.vertexes[line.v2]
        mid = ((a.x + b.x) / 2, (a.y + b.y) / 2)
        controls, facing = self._face(obs, mid)
        if step.action == "face":  # walk up to it and look at it; leave pressing to a human
            if facing:
                self.log.add(obs.tic, f"facing line {step.trigger.line}")
                self._next_step(obs.tic)
                return Controls()
            return controls
        if self._action_tic is None and facing:
            self._action_tic = obs.tic
        if self._action_tic is not None:
            since = obs.tic - self._action_tic
            if step.action in ("use", "exit"):
                controls.use = since == 0
            elif step.action == "shoot":
                controls.attack = since < 10
            if since == 0:
                self.log.add(obs.tic, f"{step.action} line {step.trigger.line}")
            if since > 12:
                self._next_step(obs.tic)
        elif obs.tic - self._progress_tic > 3 * TICRATE:
            self.failed = True
            self.log.add(obs.tic, f"could not face line {step.trigger.line}")
        return controls
