"""Execute a planned level route tic by tic.

Engine-independent: each tic the follower gets an Observation (position, angle, live sector
heights) and returns Controls. Live heights decide whether the next portal on the path can be
crossed right now. If not, the follower opens the door in front of it, or waits for the lift
or door to arrive, and as a last resort uses a nearby trigger for that sector.

Movement is meant to look like a person playing, since it's watched on stream:
- The view turns at a keyboard-like rate (easing in and out, at most MAX_TURN deg/tic) towards
  a point well ahead on the path, so it swings smoothly into corners instead of snapping to
  each waypoint.
- Movement is decoupled from facing: forward and strafe are mixed so the player goes where the
  path goes while the view is still turning (a little strafing round corners). The combined
  input never exceeds plain running (no straferun speed boost), and moving far off the facing
  direction costs speed, so it doesn't moonwalk.
- Speed is planned along the path (slower into sharp corners, stopping at goals), and a
  velocity controller works against DOOM's momentum instead of letting it slide into walls.
- Off course (fell into a pit, wandered over a teleporter) or stuck, it replans the rest of
  the step from where it is, using switches/lifts to get back if it has to.
"""
import math
from bisect import bisect_right
from dataclasses import dataclass, field, replace

from evildoom.nav.reach import (FRICTION, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS, RUN_THRUST,
                                apply_trigger,
                                HeightStates, Jump, Navigator,
                                _closest_on_segment, _closest_point, _line_mid, _segment_distance,
                                _segments_cross, can_cross)
from evildoom.nav.walkthrough import Step, plan_from, steps_from_legs
from evildoom.wad.specials import Key, Trigger

TICRATE = 35
REACHED = 12.0           # arrival radius for points we pass through
FINAL_REACHED = 10.0     # ... and for points we stop at
USE_EVERY = 15           # tics between use presses while waiting at a door
RUNUP_TOLERANCE = 6.0    # how exactly to hit a jump run-up point
STOP_ACTIONS = ("use", "shoot", "exit", "face", "kill")

# Turning, roughly a keyboard player's (vanilla's fast keyboard turn is 7 deg/tic).
MAX_TURN = 8.0           # degrees per tic
TURN_ACCEL = 1.0         # change in turn rate per tic
# Moving.
MAX_MOVE = 50.0          # forward/strafe input magnitude: plain running, no straferun boost
FULL_SPEED = RUN_THRUST * 0.97 / (1 - FRICTION)   # ~16 units/tic, with a little in reserve
BRAKE = 1.5              # deceleration assumed when planning speed (units/tic per tic)
TRACK_AHEAD = 32.0       # steer towards the path this far ahead of us
LOOK_AHEAD = 128.0       # ... and look this far ahead
STRAFE_FREE = 40.0       # moving up to this far off the facing direction costs no speed
CORNER_SPEEDS = ((0, FULL_SPEED), (20, FULL_SPEED), (45, 11.0), (90, 6.5), (135, 4.5),
                 (180, 3.0))   # (turn in degrees, speed to take it at)
CORNER_SPAN = 48.0       # turns within this distance count as one corner
CORNER_CUT = 6.0         # how close to a waypoint before steering on past it
CARROT_STEPS = (72.0, 56.0, 40.0, 28.0)   # look-aheads to try when steering (longest first)
BLOCK_CHECK_DIST = 48.0  # how close to a lift/mover before live heights are checked
DOOR_CHECK_DIST = 128.0  # ... and to a door (so we slow down and walk up to it)
DOOR_STANDOFF = 36.0     # where to stand to press a door
# Getting stuck and getting lost.
STUCK_TICS = TICRATE     # no progress for this long while trying to move: back off and retry
UNSTICK_TICS = 12
UNSTICKS_BEFORE_REPLAN = 2
STRAY = 40.0             # off the path and outside its polys by this much: off course
OFF_COURSE = 96.0        # this far from the path anywhere: off course
OFF_COURSE_TICS = 10     # ... for this long
MAX_REPLANS = 6          # per step
MAX_TOTAL_REPLANS = 30   # per route (replans that add steps restart the per-step count)
GIVE_UP_TICS = 60 * TICRATE  # one step taking this long: give up


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


def _heading(a, b) -> float:
    return math.degrees(math.atan2(b[1] - a[1], b[0] - a[0]))


def _corner_speed(turn: float) -> float:
    for (a0, v0), (a1, v1) in zip(CORNER_SPEEDS, CORNER_SPEEDS[1:]):
        if turn <= a1:
            return v0 + (v1 - v0) * (turn - a0) / (a1 - a0)
    return CORNER_SPEEDS[-1][1]


# --- path geometry --------------------------------------------------------------------------

class Track:
    """A waypoint polyline with arc-length lookups and a speed profile."""

    def __init__(self, pts: list[tuple[float, float]]):
        self.pts = pts
        self.cum = [0.0]
        for a, b in zip(pts, pts[1:]):
            self.cum.append(self.cum[-1] + math.dist(a, b))
        self.length = self.cum[-1]
        turns = [0.0] * len(pts)
        for i in range(1, len(pts) - 1):
            if math.dist(pts[i - 1], pts[i]) > 1e-6 and math.dist(pts[i], pts[i + 1]) > 1e-6:
                turns[i] = _norm(_heading(pts[i], pts[i + 1]) - _heading(pts[i - 1], pts[i]))
        # How sharp the corner starting at each vertex is: several small turns close together
        # are one big one.
        self.corner = [0.0] * len(pts)
        for i in range(1, len(pts) - 1):
            total, j = 0.0, i
            while j < len(pts) - 1 and self.cum[j] - self.cum[i] <= CORNER_SPAN:
                total += turns[j]
                j += 1
            self.corner[i] = max(abs(total), abs(turns[i]))

    def project(self, p, s_now: float, ahead: float = 64.0) -> tuple[int, float, float]:
        """(segment index, arc length, distance) of the closest point on the track between a
        little behind `s_now` and `ahead` past it, so progress can't skip ahead to a later
        part of the track that passes nearby (paths that double back, through walls)."""
        if len(self.pts) == 1:
            return 0, 0.0, math.dist(p, self.pts[0])
        lo, hi = s_now - 32, s_now + ahead
        best = None
        for k in range(len(self.pts) - 1):
            if self.cum[k + 1] < lo:
                continue
            if self.cum[k] > hi:
                break
            a, b = self.pts[k], self.pts[k + 1]
            seg = self.cum[k + 1] - self.cum[k]
            q = _closest_point(p, a, b)
            s = self.cum[k] + math.dist(a, q)
            if s > hi or s < lo:  # clamp to the window
                s = min(max(s, lo, self.cum[k]), hi, self.cum[k + 1])
                t = (s - self.cum[k]) / seg if seg > 1e-9 else 0.0
                q = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
            d = math.dist(p, q)
            if best is None or d < best[2] - 1e-6:
                best = (k, s, d)
        return best or (len(self.pts) - 2, self.length, math.dist(p, self.pts[-1]))

    def carrot(self, k: int, s: float, ahead: float) -> tuple[float, float]:
        """Point to steer for: `ahead` along the track, but never past the next waypoint
        (cutting the inside of a corner snags the player's square box on it) until we're
        nearly there."""
        nxt = k + 1
        while nxt < len(self.pts) - 1 and self.cum[nxt] - s < CORNER_CUT:
            nxt += 1
        return self.point_at(min(s + ahead, self.cum[min(nxt, len(self.pts) - 1)]))

    def point_at(self, s: float) -> tuple[float, float]:
        if s <= 0 or len(self.pts) == 1:
            return self.pts[0]
        if s >= self.length:
            return self.pts[-1]
        k = min(bisect_right(self.cum, s) - 1, len(self.pts) - 2)
        a, b = self.pts[k], self.pts[k + 1]
        seg = max(self.cum[k + 1] - self.cum[k], 1e-9)
        t = (s - self.cum[k]) / seg
        return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)

    def speed_limit(self, s: float, end_speed: float) -> float:
        """Fastest we can go at arc length `s` and still slow down for what's ahead."""
        rest = max(self.length - s, 0.0)
        limit = math.sqrt(end_speed ** 2 + 2 * BRAKE * rest)
        if end_speed == 0:
            limit = min(limit, 0.3 * rest + 0.5)  # creep the last bit to stop exactly
        for i in range(1, len(self.pts) - 1):
            ahead = self.cum[i] - s
            if ahead < -CORNER_SPAN / 2:
                continue
            if ahead > 320:
                break
            v = _corner_speed(self.corner[i])
            limit = min(limit, math.sqrt(v * v + 2 * BRAKE * max(ahead, 0.0)))
        return limit


@dataclass
class Segment:
    """Part of a step's path walked without a break. `end` says what happens at its last
    point: "teleport" (walk through the line), "jump" (line up and sprint) or "goal"."""
    track: Track
    end: str
    jump: Jump | None = None
    look: Track | None = None      # the track plus where to look past its end


# --- the low-level controller -----------------------------------------------------------------

class Motion:
    """Turns a wanted movement (per-tic displacement) and a wanted view angle into Controls,
    turning smoothly and compensating for momentum."""

    def __init__(self):
        self.rate = 0.0                 # current turn rate, degrees/tic counter-clockwise
        self.disp = (0.0, 0.0)          # last tic's displacement
        self._last: tuple[float, float] | None = None

    def observe(self, obs: Observation) -> None:
        pos = (obs.x, obs.y)
        if self._last is not None:
            d = (pos[0] - self._last[0], pos[1] - self._last[1])
            self.disp = d if math.hypot(*d) < 64 else (0.0, 0.0)   # teleports kill momentum
        self._last = pos

    @property
    def speed(self) -> float:
        return math.hypot(*self.disp)

    def turn_towards(self, obs: Observation, angle: float | None) -> float:
        """Turn rate for this tic: accelerate/decelerate smoothly to land on `angle`."""
        err = 0.0 if angle is None else _norm(angle - obs.angle)
        want = math.copysign(min(MAX_TURN, math.sqrt(2 * TURN_ACCEL * abs(err)), abs(err)), err)
        self.rate = max(self.rate - TURN_ACCEL, min(self.rate + TURN_ACCEL, want))
        if abs(err) < 0.25 and abs(self.rate) <= TURN_ACCEL:
            self.rate = err
        return self.rate

    def drive(self, obs: Observation, want: tuple[float, float], look: float | None,
              cap: float = MAX_MOVE) -> Controls:
        rate = self.turn_towards(obs, look)
        facing = math.radians(obs.angle + rate)   # thrust uses the angle after this tic's turn
        # Next tic's displacement is FRICTION * this one's + thrust (on the ground).
        ax, ay = want[0] - FRICTION * self.disp[0], want[1] - FRICTION * self.disp[1]
        cx, cy = ax / RUN_THRUST * MAX_MOVE, ay / RUN_THRUST * MAX_MOVE
        mag = math.hypot(cx, cy)
        if mag > cap:
            cx, cy = cx * cap / mag, cy * cap / mag
        cos, sin = math.cos(facing), math.sin(facing)
        return Controls(forward=cx * cos + cy * sin, strafe=cx * sin - cy * cos, turn=-rate)

    def move(self, obs: Observation, direction: float, speed: float, look: float | None,
             cap: float = MAX_MOVE) -> Controls:
        """Go `speed` units/tic towards world angle `direction`, slower when that's far off
        where we're looking (a little strafing is fine; running sideways isn't)."""
        off = abs(_norm(direction - obs.angle))
        if off > 90:
            speed *= max(0.15, 0.45 - (off - 90) / 60 * 0.3)
        elif off > STRAFE_FREE:
            speed *= 1 - (off - STRAFE_FREE) / (90 - STRAFE_FREE) * 0.55
        rad = math.radians(direction)
        return self.drive(obs, (math.cos(rad) * speed, math.sin(rad) * speed), look, cap)


# --- following a route ------------------------------------------------------------------------

class RouteFollower:
    def __init__(self, nav: Navigator, steps: list[Step], keys: set[Key] | None = None,
                 states: HeightStates | None = None, replan_level: bool = True):
        """`keys` held and `states` (the heights the world can take, as the planner models it)
        at the start; they're kept up to date as steps complete, for replanning.
        `replan_level`: if a step's goal can't be reached any more, plan the rest of the level
        again (otherwise give up)."""
        self.nav = nav
        self.steps = list(steps)
        self.doors = nav.local_doors(set(Key))  # the plan only routes through doors we can open
        self.keys = set(keys or ())
        self.states = {s: set(h) for s, h in (states or nav.initial_states()).items()}
        self.replan_level = replan_level
        self.log = FollowerLog()
        self.motion = Motion()
        self.step_i = -1
        self.done = False
        self.failed = False
        self.replans = 0
        self._last_pos: tuple[float, float] | None = None
        self._sprint = None  # (jump, heading, start tic) while executing a run-jump
        self._look = None    # last view angle we were aiming for
        self._next_step(0)

    # --- what we're doing (for display) -------------------------------------------------

    @property
    def objective(self) -> str:
        if self.done:
            return "route finished"
        if self.failed:
            return "gave up"
        step = self.steps[self.step_i]
        return f"step {self.step_i + 1}/{len(self.steps)}: {step.action} - {step.label}"

    def planned_path(self, pos: tuple[float, float] | None = None) -> list:
        """The rest of the current step's path from `pos` (None marks a teleport/jump)."""
        if self.done or self.failed:
            return []
        out = [pos] if pos else []
        for i in range(self.seg_i, len(self.segments)):
            pts = self.segments[i].track.pts
            out += pts[self.k + 1:] if i == self.seg_i else pts
            if i + 1 < len(self.segments):
                out.append(None)
        return out

    # --- step / segment bookkeeping ----------------------------------------------------

    def _next_step(self, tic: int) -> None:
        if 0 <= self.step_i < len(self.steps):  # remember what the finished step changed
            done = self.steps[self.step_i]
            if done.trigger is not None:
                self.states = apply_trigger(self.states, done.trigger)
            if done.key is not None:
                self.keys.add(done.key)
        self.step_i += 1
        self._step_replans = 0
        if self.step_i >= len(self.steps):
            self.done = True
            self.log.add(tic, "route finished")
            return
        step = self.steps[self.step_i]
        self.log.add(tic, f"step {self.step_i + 1}/{len(self.steps)}: walk, then "
                          f"{step.action} ({step.label})")
        self._step_tic = tic
        self._crossed = False
        self._start_step(tic)

    def _start_step(self, tic: int) -> None:
        self.segments = self._split(self.steps[self.step_i])
        self.seg_i = 0
        self.poly_i = 0
        self.phase = "walk"
        path = self.steps[self.step_i].path
        self._path_polys = set(path.polys)
        # Ledge edges the path means to cross (any others count as walls when cutting corners).
        mesh = self.nav.mesh
        self._crossing_lines = frozenset(
            mesh.portals[pi].linedef for a, b in zip(path.polys, path.polys[1:])
            for pi in mesh.polys[a].portals
            if mesh.portals[pi].other(a) == b and mesh.portals[pi].linedef is not None)
        self._off_since: int | None = None
        self._unsticks = 0
        self._unstick_until = -1
        self._enter_segment(tic)

    def _enter_segment(self, tic: int) -> None:
        self.k, self.s, self.cross = 0, 0.0, 0.0
        self._settle_until = tic + OFF_COURSE_TICS
        self._reset_progress(tic)

    def _split(self, step: Step) -> list[Segment]:
        """Waypoint segments split at teleports and jump run-ups."""
        pts = step.path.waypoints
        raw: list[tuple[list, str, Jump | None]] = []
        cur: list = []
        i = 0
        while i < len(pts):
            p = pts[i]
            if p is None:
                if len(cur) >= 2:  # carry on a little past the line so we actually cross it
                    (ax, ay), (bx, by) = cur[-2], cur[-1]
                    d = max(math.dist(cur[-2], cur[-1]), 1)
                    cur[-1] = (bx + (bx - ax) / d * 24, by + (by - ay) / d * 24)
                raw.append((cur, "teleport", None))
                cur = []
                i += 1
                continue
            cur.append(p)
            jump = next((j for j in step.path.jumps if math.dist(p, j.runup) < 1), None)
            if jump is not None:
                raw.append((cur, "jump", jump))
                cur = []
                # carry on from the landing point (the waypoint after the takeoff)
                land = next((k for k in range(i + 2, len(pts))
                             if pts[k] is not None and math.dist(pts[k], jump.dest) < 32), None)
                i = land if land is not None else i + 2
                continue
            i += 1
        raw.append((cur, "goal", None))
        segs = []
        for n, (seg_pts, end, jump) in enumerate(raw):
            if not seg_pts:
                seg_pts = [jump.dest if jump else
                           next(p for p in reversed(pts) if p is not None)]
            tail = self._look_tail(step, seg_pts, end, jump, last=n == len(raw) - 1)
            segs.append(Segment(Track(seg_pts), end, jump, Track(seg_pts + tail)))
        return segs

    def _look_tail(self, step: Step, pts, end: str, jump: Jump | None, last: bool) -> list:
        """Where to look once the segment's own points run out."""
        if end == "jump":
            return [jump.takeoff]
        if end == "teleport":
            if len(pts) >= 2:
                h = math.radians(_heading(pts[-2], pts[-1]))
                return [(pts[-1][0] + math.cos(h) * 64, pts[-1][1] + math.sin(h) * 64)]
            return []
        if step.trigger is not None and step.trigger.line >= 0 and step.action in STOP_ACTIONS:
            return [_line_mid(self.nav.map, step.trigger.line)]
        nxt = self.steps[self.step_i + 1] if self.step_i + 1 < len(self.steps) else None
        if nxt is not None:  # rolling on into the next step: look where it goes
            return [p for p in nxt.path.waypoints[1:6] if p is not None][:3]
        return []

    def _reset_progress(self, tic: int) -> None:
        self._best_s = -math.inf
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
        self.motion.observe(obs)
        pos = (obs.x, obs.y)
        if self._last_pos and math.dist(pos, self._last_pos) > 64:
            if self.seg_i + 1 < len(self.segments) and self.segments[self.seg_i].end == "teleport":
                self.log.add(obs.tic, "teleported")
                self.seg_i += 1
                self._enter_segment(obs.tic)
            else:
                self.log.add(obs.tic, f"moved unexpectedly to ({obs.x:.0f}, {obs.y:.0f})")
                self._settle_until = obs.tic  # judge where we are straight away
        if self._last_pos and self.steps[self.step_i].action == "cross" and not self._crossed:
            a, b = self._trigger_line()
            self._crossed = _segments_cross(self._last_pos, pos, a, b)
        self._last_pos = pos
        if obs.tic - self._step_tic > GIVE_UP_TICS:
            return self._fail(obs, f"step {self.step_i + 1} taking too long")
        if self.phase == "walk":
            return self._walk(obs)
        return self._act_at_goal(obs)

    def _fail(self, obs: Observation, why: str) -> Controls:
        self.failed = True
        self.log.add(obs.tic, f"FAILED at ({obs.x:.0f}, {obs.y:.0f}): {why} "
                              f"(step {self.step_i + 1}: {self.steps[self.step_i].label})")
        return Controls()

    # --- walking ------------------------------------------------------------------------

    def _walk(self, obs: Observation) -> Controls:
        if self._sprint is not None:
            return self._sprint_jump(obs)
        seg = self.segments[self.seg_i]
        track = seg.track
        pos = (obs.x, obs.y)
        self.k, self.s, self.cross = track.project(pos, self.s)

        if self._lost(obs):
            if not self._replan(obs, "off course"):
                return self._fail(obs, "off course and no way back")
            return self._walk(obs)

        self._speed_cap = math.inf
        blocked = self._check_blocked(obs)
        if blocked is not None:
            return blocked

        end = track.pts[-1]
        step = self.steps[self.step_i]
        last = self.seg_i == len(self.segments) - 1
        stop = seg.end == "jump" or (last and step.action in STOP_ACTIONS)
        # Near the end only counts once we've come along the track (paths double back, e.g. to
        # walk onto a teleport pad from its front side).
        to_end = math.dist(pos, end) if track.length - self.s < 48 else math.inf
        if seg.end == "jump" and to_end <= RUNUP_TOLERANCE:
            return self._line_up(obs, seg.jump)
        if seg.end == "goal" and to_end < (FINAL_REACHED if stop else REACHED):
            self.phase = "act"
            self._reset_progress(obs.tic)
            return self._act_at_goal(obs)

        # Progress and getting unstuck.
        if self.s > self._best_s + 2:
            if self._best_s != -math.inf:
                self._unsticks = 0  # got past whatever snagged us
            self._best_s, self._progress_tic = self.s, obs.tic
        if obs.tic < self._unstick_until:
            return self._unstick(obs)
        if obs.tic - self._progress_tic > STUCK_TICS:
            self._unsticks += 1
            if self._unsticks > UNSTICKS_BEFORE_REPLAN:
                if not self._replan(obs, "stuck"):
                    return self._fail(obs, "stuck")
                return self._walk(obs)
            self.log.add(obs.tic, f"stuck at ({obs.x:.0f}, {obs.y:.0f}), backing off")
            self._unstick_until = obs.tic + UNSTICK_TICS
            self._progress_tic = obs.tic + UNSTICK_TICS
            self._unstick_dir = self._away_from_walls(obs)
            return self._unstick(obs)

        # Follow the track: steer for a point a little ahead, look further ahead.
        carrot = self._carrot(pos, track)
        if seg.end == "teleport" and to_end < REACHED:  # keep going through the line
            carrot = seg.look.pts[-1]
        end_speed = 0.0 if stop else (FULL_SPEED if seg.end == "teleport" else
                                      self._roll_speed(seg))
        speed = min(FULL_SPEED, track.speed_limit(self.s, end_speed), self._speed_cap)
        if stop:
            speed = min(speed, 0.3 * to_end + 0.5)
        if math.dist(pos, carrot) < 1:
            return self.motion.drive(obs, (0.0, 0.0), self._look_angle(obs, seg))
        return self.motion.move(obs, _heading(pos, carrot), speed, self._look_angle(obs, seg))

    def _carrot(self, pos, track: Track) -> tuple[float, float]:
        """Point to steer for: as far ahead on the track (up to CARROT_MAX) as the straight
        line there keeps the player's box clear, so corners are rounded where there's room;
        otherwise the next waypoint exactly."""
        for ahead in CARROT_STEPS:
            q = track.point_at(self.s + ahead)
            if math.dist(pos, q) < 1 or self._chord_clear(pos, q):
                return q
        return track.carrot(self.k, self.s, TRACK_AHEAD)

    def _chord_clear(self, p, q) -> bool:
        """Can the player's box move straight from p to q without touching a wall, solid thing
        or (uncrossed) ledge edge? Edges we're already close to only need not get closer."""
        for e1, e2 in self.nav._walls_near(p, q, 2 * PLAYER_RADIUS, self._crossing_lines):
            length = max(math.dist(e1, e2), 1e-6)
            c, s = abs(e2[0] - e1[0]) / length, abs(e2[1] - e1[1]) / length
            reach = PLAYER_RADIUS * (c + s) + 2   # the square box's extent across this edge
            d = _segment_distance(p, q, e1, e2)
            if d < reach and d < _segment_distance(p, p, e1, e2) - 0.5:
                return False
        return True

    def _look_angle(self, obs: Observation, seg: Segment) -> float:
        """View angle: towards a point well ahead on the path (or what we're heading for),
        holding the previous one when that point is too close to give a stable direction."""
        look = seg.look
        s = look.project((obs.x, obs.y), self.s)[1] if look.length > seg.track.length else self.s
        point = look.point_at(s + LOOK_AHEAD)
        if math.dist((obs.x, obs.y), point) >= 40 or self._look is None:
            if math.dist((obs.x, obs.y), point) > 1:
                self._look = _heading((obs.x, obs.y), point)
            elif self._look is None:
                self._look = obs.angle
        return self._look

    def _roll_speed(self, seg: Segment) -> float:
        """Speed to pass the end of a step we don't stop at, given where the next one goes."""
        look = seg.look.pts
        n = len(seg.track.pts)
        if n < 2 or len(look) <= n:
            return FULL_SPEED * 0.6
        turn = abs(_norm(_heading(look[n - 1], look[n]) - _heading(look[n - 2], look[n - 1])))
        return _corner_speed(turn)

    # --- recovering ---------------------------------------------------------------------

    def _away_from_walls(self, obs: Observation) -> float:
        """Direction to back off in: away from the nearest walls/obstacles, else backwards."""
        pos = (obs.x, obs.y)
        ax = ay = 0.0
        for a, b in self.nav._walls_near(pos, pos, 40):
            q = _closest_point(pos, a, b)
            d = math.dist(pos, q)
            if 1e-6 < d < 40:
                ax += (pos[0] - q[0]) / d * (40 - d)
                ay += (pos[1] - q[1]) / d * (40 - d)
        track = self.segments[self.seg_i].track
        back = _heading(track.point_at(self.s + TRACK_AHEAD), pos)
        if math.hypot(ax, ay) < 1e-6:
            return back
        away = math.degrees(math.atan2(ay, ax))
        # Mostly sideways off what we're snagged on, a bit backwards.
        return _heading((0, 0), (math.cos(math.radians(away)) * 2 + math.cos(math.radians(back)),
                                 math.sin(math.radians(away)) * 2 + math.sin(math.radians(back))))

    def _unstick(self, obs: Observation) -> Controls:
        seg = self.segments[self.seg_i]
        return self.motion.move(obs, self._unstick_dir, 5.0, self._look_angle(obs, seg))

    def _lost(self, obs: Observation) -> bool:
        """Off the planned path (fell into a pit, pushed, unexpected teleport) for a while."""
        if obs.tic < self._settle_until:
            return False
        mesh = self.nav.mesh
        here = mesh.poly_at(obs.x, obs.y)
        off = self.cross > OFF_COURSE
        if not off and (here is None or here.index not in self._path_polys):
            if self.cross > STRAY:
                off = True
            else:  # beside the path but somewhere we can't step back up from
                q = mesh.poly_at(*self.segments[self.seg_i].track.point_at(self.s))
                if here is not None and q is not None and here.sector != q.sector and \
                        obs.heights[here.sector][0] < obs.heights[q.sector][0] - MAX_STEP:
                    off = True
        if not off:
            self._off_since = None
            return False
        if self._off_since is None:
            self._off_since = obs.tic
        return obs.tic - self._off_since >= OFF_COURSE_TICS

    def world_states(self, obs: Observation) -> HeightStates:
        """Heights the world can take as far as replanning is concerned: what the plan has
        unlocked so far, plus whatever we can see right now."""
        states = {s: set(h) for s, h in self.states.items()}
        for s, h in obs.heights.items():
            states.setdefault(s, set()).add(h)
        return states

    def _replan(self, obs: Observation, why: str, live_only: bool = False) -> bool:
        """Re-path the current step from here. If its goal can't be walked to, plan a way
        there with triggers (lifts, switches out of a pit); failing that, replan the level.
        `live_only`: plan with only the heights as they are now, not everything the plan has
        unlocked (a lift that went back up has to be called again)."""
        self.replans += 1
        self._step_replans += 1
        if self._step_replans > MAX_REPLANS or self.replans > MAX_TOTAL_REPLANS:
            self.log.add(obs.tic, f"{why}: replanned too often")
            return False
        nav, pos, step = self.nav, (obs.x, obs.y), self.steps[self.step_i]
        goal = next(p for p in reversed(step.path.waypoints) if p is not None)
        states = {s: {h} for s, h in obs.heights.items()} if live_only else \
            self.world_states(obs)
        path = nav.find_path(pos, goal, states, nav.local_doors(self.keys))
        if path is not None:
            self.steps[self.step_i] = replace(step, path=path)
            self.log.add(obs.tic, f"{why} at ({obs.x:.0f}, {obs.y:.0f}): new path to "
                                  f"({goal[0]:.0f}, {goal[1]:.0f})")
        else:
            route = nav.plan(pos, [goal], frozenset(self.keys), states, None,
                             max_expansions=200, max_seconds=3.0)
            if route is not None:
                new = steps_from_legs(route.legs, step)
                self.steps[self.step_i:self.step_i + 1] = new
                self.log.add(obs.tic, f"{why} at ({obs.x:.0f}, {obs.y:.0f}): back on track "
                                      f"via {len(new) - 1} extra step(s)")
            elif self.replan_level:
                plan = plan_from(nav, pos, self.keys, states, max_seconds=10.0,
                                 max_expansions=1000)
                if not plan.complete:
                    self.log.add(obs.tic, f"{why}: no way to the step goal or the exit")
                    return False
                self.steps[self.step_i:] = plan.steps
                self.log.add(obs.tic, f"{why} at ({obs.x:.0f}, {obs.y:.0f}): replanned the "
                                      f"rest of the level ({len(plan.steps)} steps)")
            else:
                self.log.add(obs.tic, f"{why}: no way to the step goal")
                return False
        self._sprint = None
        self._start_step(obs.tic)
        return True

    # --- run-off jumps ---------------------------------------------------------------------

    def _line_up(self, obs: Observation, jump: Jump) -> Controls:
        """At the run-up: settle on the exact spot facing the takeoff, then start sprinting."""
        heading = _heading(jump.runup, jump.takeoff)
        diff = _norm(heading - obs.angle)
        if abs(diff) <= 1.5 and self.motion.speed < 0.5 and abs(self.motion.rate) < 1:
            self._sprint = (jump, heading, obs.tic)
            self.log.add(obs.tic, f"run-jump from ({jump.takeoff[0]:.0f}, {jump.takeoff[1]:.0f})")
            return self._sprint_jump(obs)
        hold = ((jump.runup[0] - obs.x) * 0.3, (jump.runup[1] - obs.y) * 0.3)
        self._progress_tic = obs.tic
        return self.motion.drive(obs, hold, heading)

    def _sprint_jump(self, obs: Observation) -> Controls:
        """Full speed in a straight line until we've covered the planned flight distance."""
        jump, heading, since = self._sprint
        ux, uy = math.cos(math.radians(heading)), math.sin(math.radians(heading))
        flown = (obs.x - jump.takeoff[0]) * ux + (obs.y - jump.takeoff[1]) * uy
        if flown >= math.dist(jump.takeoff, jump.dest) - 8 or obs.tic - since > 4 * TICRATE:
            self._sprint = None
            land = self.nav.mesh.poly_at(obs.x, obs.y)
            self.log.add(obs.tic, f"landed at ({obs.x:.0f}, {obs.y:.0f}) sector "
                                  f"{land.sector if land else None}")
            self.seg_i = min(self.seg_i + 1, len(self.segments) - 1)
            self._enter_segment(obs.tic)
            self._look = heading
            return self._walk(obs)
        diff = _norm(heading - obs.angle)
        self.motion.rate = 0.0
        return Controls(forward=50.0, turn=max(-3.0, min(3.0, -diff)))

    # --- doors and lifts in the way --------------------------------------------------------

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
            dist = math.dist(near, (obs.x, obs.y))
            if dist > DOOR_CHECK_DIST:
                return None
            if nxt.sector != cur.sector and \
                    not can_cross(obs.heights[cur.sector], obs.heights[nxt.sector]):
                # Look past the immediate crossing only for doors (they hide behind thin
                # door tracks). For lifts and other movers, waiting before boarding is wrong:
                # step on and ride, and the crossing off it opens up on the way.
                door = nxt.sector in self.doors
                # (Past the immediate crossing anyway if the box already reaches it.)
                if (j > self.poly_i and not door and dist >= 2 * PLAYER_RADIUS) \
                        or (not door and dist > BLOCK_CHECK_DIST):
                    return None
                return cur, nxt.sector, portal, None
            # The square box reaches into sectors past a narrow strip before it can drop into
            # the strip: a low ceiling there must clear the floor we're standing on now.
            floor_now = obs.heights[here.sector][0]
            f, c = obs.heights[nxt.sector]
            if j > self.poly_i and dist < 2 * PLAYER_RADIUS and nxt.sector != here.sector \
                    and c - max(f, floor_now) < PLAYER_HEIGHT:
                return cur, nxt.sector, portal, None
            # Something beside the way that makes it too narrow right now (key bars, a raised
            # block): open it if it's a door we can use, else wait like for a mover.
            if portal.index in self.nav._portal_gates:
                live = {s: {h} for s, h in obs.heights.items()}
                blockers = self.nav.gate_blockers(portal.index, cur.index, live)
                if blockers and not self.nav.portal_fits_without(portal.index, blockers):
                    door = self._gate_door(blockers, {cur.index, here.index}, (obs.x, obs.y))
                    if door is not None:
                        return cur, door[0], portal, door[1]
                    if j == self.poly_i and dist < BLOCK_CHECK_DIST:
                        line = self.nav.map.linedefs[blockers[0]]
                        sides = {self.nav.map.front_sector(line), self.nav.map.back_sector(line)}
                        return cur, next(iter(sides - {cur.sector}), cur.sector), portal, None
        return None

    def _gate_door(self, lines, near: set[int], pos):
        """(door sector, point to press) for a usable door among the sectors of these lines."""
        m = self.nav.map
        sectors = {s for li in lines for s in (m.front_sector(m.linedefs[li]),
                                                m.back_sector(m.linedefs[li]))}
        for t in self.nav.triggers:
            if t.info.trigger in (Trigger.DR, Trigger.D1) and sectors & set(t.effects) \
                    and near & t.activators and t.info.lock.satisfied(set(Key)):
                line = m.linedefs[t.line]
                a, b = m.vertexes[line.v1], m.vertexes[line.v2]
                return (next(iter(sectors & set(t.effects))),
                        _closest_point(pos, (a.x, a.y), (b.x, b.y)))
        return None

    def _check_blocked(self, obs: Observation) -> Controls | None:
        """If a crossing just ahead can't be made with live heights, deal with it."""
        blocked = self._next_blocked(obs)
        if blocked is None:
            self._wait_tic = None
            return None
        cur, sector, portal, use_point = blocked
        live = obs.heights
        pos = (obs.x, obs.y)
        opener = sector in self.doors and (use_point is not None
                                           or cur.index in self.doors[sector][1])
        # the part of the doorway in front of us (doors can be wide), or the gate door's line
        door_point = use_point or _closest_on_segment(pos, portal)
        if opener:
            # how far to the door, along the path where it goes round corners
            track = self.segments[self.seg_i].track
            along = track.project(door_point, self.s, DOOR_CHECK_DIST + 64)[1] - self.s
            dist = max(math.dist(pos, door_point), along)
            if dist > DOOR_STANDOFF + 4:
                # Closed door ahead: keep following the path, slowing to stop in front of it.
                self._speed_cap = max(1.0, min(math.sqrt(2 * BRAKE * (dist - DOOR_STANDOFF)),
                                               0.3 * (dist - DOOR_STANDOFF) + 1))
                return None

        if self._wait_tic is None:
            self._wait_tic = obs.tic
            kind = "door" if sector in self.doors else "mover"
            self.log.add(obs.tic, f"waiting at {kind} (sector {sector})")
        waited = obs.tic - self._wait_tic
        self._progress_tic = obs.tic  # waiting is not being stuck
        # Pressing use on a moving door reverses it, so only press once things are still.
        if live[sector] != self._mover_heights:
            self._mover_heights, self._mover_still_since = live[sector], obs.tic
        still = obs.tic - self._mover_still_since >= USE_EVERY
        look = _heading(pos, door_point)
        want, use = (0.0, 0.0), False
        if not opener:
            # Wait squarely at the edge we're crossing. Half on something else (often a ledge
            # under a low ceiling), a rising lift can't carry us up and goes back down.
            (x1, y1), (x2, y2) = portal.p1, portal.p2
            length = max(math.dist(portal.p1, portal.p2), 1e-6)
            nx, ny = (y2 - y1) / length, -(x2 - x1) / length
            if (pos[0] - door_point[0]) * nx + (pos[1] - door_point[1]) * ny < 0:
                nx, ny = -nx, -ny   # normal pointing back to our side
            spot = (door_point[0] + nx * (PLAYER_RADIUS + 2), door_point[1] + ny * (PLAYER_RADIUS + 2))
            gap = math.dist(pos, spot)
            if gap > 1:
                speed = min(4.0, 0.3 * gap)
                h = math.radians(_heading(pos, spot))
                want = (math.cos(h) * speed, math.sin(h) * speed)
        if opener:
            # Use traces 64 units straight ahead and stops at the first wall: face the middle
            # of the door, then press.
            facing = abs(_norm(look - obs.angle)) < 10
            use = facing and still and obs.tic - self._last_use >= USE_EVERY
        switch_nearby = False
        if not opener:
            # Lift/door not coming by itself: use a trigger for it we're standing next to.
            here = self.nav.mesh.poly_at(*pos)
            near = {cur.index} | ({here.index} if here else set())
            involved = {cur.sector, sector} | ({here.sector} if here else set())
            for t in self.nav.triggers:
                if t.info.trigger in (Trigger.SR, Trigger.S1, Trigger.DR, Trigger.D1) \
                        and involved & set(t.sectors) and near & t.activators:
                    switch_nearby = True
                    if waited > 3 * TICRATE and still:
                        look = _heading(pos, _line_mid(self.nav.map, t.line))
                        use = abs(_norm(look - obs.angle)) < 10 and \
                            obs.tic - self._last_use >= USE_EVERY
                    break
        controls = self.motion.drive(obs, want, look)
        controls.use = use
        if use:
            self._last_use = obs.tic
        # Nothing to press here: it may need a walk-over line crossing again (a blazing lift
        # that went back up), so replan from the live heights after a few seconds.
        if waited == 5 * TICRATE and not opener and not switch_nearby:
            if self._replan(obs, f"sector {sector} not coming", live_only=True):
                return self.motion.drive(obs, (0.0, 0.0), look)
        if waited > 15 * TICRATE:
            if not self._replan(obs, f"sector {sector} not opening"):
                return self._fail(obs, f"waited too long for sector {sector}: live "
                                       f"{live[cur.sector]} -> {live[sector]}")
            return self.motion.drive(obs, (0.0, 0.0), look)
        return controls

    # --- acting at the end of a step ------------------------------------------------------

    def _trigger_line(self):
        m = self.nav.map
        line = m.linedefs[self.steps[self.step_i].trigger.line]
        a, b = m.vertexes[line.v1], m.vertexes[line.v2]
        return (a.x, a.y), (b.x, b.y)

    def _walk_across(self, obs: Observation) -> Controls:
        """At the end of a walk-over step without having crossed its line yet: step over it."""
        pos = (obs.x, obs.y)
        a, b = self._trigger_line()
        q = _closest_point(pos, a, b)
        length = max(math.dist(a, b), 1e-6)
        nx, ny = (b[1] - a[1]) / length, -(b[0] - a[0]) / length
        side = (pos[0] - q[0]) * nx + (pos[1] - q[1]) * ny
        sign = -1 if side > 0 else 1
        # aim 24 units past the line, at the nearest point along it (not too near its ends)
        t = max(0.15, min(0.85, ((q[0] - a[0]) * (b[0] - a[0]) + (q[1] - a[1]) * (b[1] - a[1]))
                         / length ** 2)) if length > 64 else 0.5
        on = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
        target = (on[0] + sign * nx * 24, on[1] + sign * ny * 24)
        if obs.tic - self._progress_tic > 3 * TICRATE:
            self.log.add(obs.tic, f"could not cross line {self.steps[self.step_i].trigger.line}")
            self._crossed = True  # give up on it and carry on; replanning will catch problems
        heading = _heading(pos, target)
        return self.motion.move(obs, heading, min(8.0, 0.3 * math.dist(pos, target) + 2), heading)

    def _act_at_goal(self, obs: Observation) -> Controls:
        step = self.steps[self.step_i]
        if step.action == "cross" and not self._crossed and step.trigger is not None:
            return self._walk_across(obs)
        if step.action in ("pickup", "cross"):
            self._next_step(obs.tic)
            return self._walk(obs) if not self.done else Controls()
        if step.action == "kill":
            self.log.add(obs.tic, "skipping boss kill (no monsters in this run)")
            self._next_step(obs.tic)
            return Controls()

        # Aim at the nearest part of the line (a little in from its ends): the use trace is a
        # 64-unit ray, and from off to one side the line's middle can be out of its reach.
        a, b = self._trigger_line()
        length = max(math.dist(a, b), 1e-6)
        pos = (obs.x, obs.y)
        t = ((pos[0] - a[0]) * (b[0] - a[0]) + (pos[1] - a[1]) * (b[1] - a[1])) / length ** 2
        t = min(max(t, min(0.5, 8 / length)), max(0.5, 1 - 8 / length))
        aim = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
        look = _heading(pos, aim)
        err = abs(_norm(look - obs.angle))
        # Hold still (brake) while turning to face the line.
        end = self.segments[-1].track.pts[-1]
        hold = ((end[0] - obs.x) * 0.2, (end[1] - obs.y) * 0.2)
        controls = self.motion.drive(obs, hold, look)
        facing = err < 4
        if step.action == "face":  # walk up to it and look at it; leave pressing to a human
            if facing:
                self.log.add(obs.tic, f"facing line {step.trigger.line}")
                self._next_step(obs.tic)
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
            if since > 8:
                self._next_step(obs.tic)
        elif obs.tic - self._progress_tic > 3 * TICRATE:
            return self._fail(obs, f"could not face line {step.trigger.line}")
        return controls
