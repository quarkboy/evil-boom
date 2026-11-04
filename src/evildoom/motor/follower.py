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

from evildoom.nav.movers import DOOR_CLOSE_30
from evildoom.nav.navmesh import _cells, _inside
from evildoom.nav.reach import (FRICTION, ITEM_RADIUS, MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS,
                                RUN_THRUST, TELEPORTERS_FOR_PLAYERS, HeightStates, Jump,
                                Navigator, _closest_on_segment, _closest_point, _line_mid,
                                _segment_distance, _segments_cross, apply_trigger, can_cross)
from evildoom.nav.walkthrough import Expect, Step, plan_from, steps_from_legs
from evildoom.wad.specials import Category, Key, Trigger

TICRATE = 35
REACHED = 12.0           # arrival radius for points we pass through
FINAL_REACHED = 10.0     # ... and for points we stop at
PICKUP_REACHED = 6.0     # ... and for pickup spots (they can be near the edge of touching range)
RUN_REAIM = 4            # tics between re-aims while running over sinking floors
RUN_TURN = 10.0          # degrees a tic a player turns while running across them
RUN_AHEAD_SINK = 4       # how much lower a sinking floor is assumed by the time we leave it
BARREL_IN_WAY = 40       # a barrel this close when stuck is what's in the way
BARREL_SAFE = 150        # back off this far before shooting one (blast radius 128)
BARREL_GIVE_UP = 8 * TICRATE
JUMP_HURRY = 2 * TICRATE  # a jump goes without settling if what it needs runs out sooner
JUMP_CEILING_SLACK = 16.0  # how much lower than planned a ceiling can be for a jump
CROSS_REST_WAIT = 8 * TICRATE  # longest wait for a lift to come back before crossing its line
SETTLE_WAIT = 10 * TICRATE  # longest wait for things round a switch to settle before pressing
DROP_CHECK_DIST = 48     # how far ahead to look for crossings that became one-way drops
USE_EVERY = 15           # tics between use presses while waiting at a door
RUNUP_TOLERANCE = 6.0    # how exactly to hit a jump run-up point
STOP_ACTIONS = ("use", "shoot", "exit", "face", "kill", "endure")

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
PRESS_TRIES = 3          # presses (creeping closer) before giving up on a switch
PRESS_RANGE = 56.0     # how near a door to press it (use reaches 64)
SETTLE_MAX = 0   # don't stand still for moves to finish: some floors are hopped onto rising
MAX_TOTAL_REPLANS = 30   # per route (replans that add steps restart the per-step count)
GIVE_UP_TICS = 60 * TICRATE  # one step taking this long: give up


@dataclass
class Observation:
    x: float
    y: float
    angle: float                                  # degrees, 0 = east, counter-clockwise
    heights: dict[int, tuple[float, float]]       # live sector (floor, ceiling)
    tic: int = 0
    keys: frozenset | None = None                 # keys held, if the engine can tell
    z: float | None = None                        # height of the player's feet
    weapon: int | None = None                     # weapon slot held, if the engine can tell
    barrels: tuple | None = None                  # where explosive barrels are now, if known


@dataclass
class Controls:
    forward: float = 0.0   # -50..50
    strafe: float = 0.0    # positive = right
    turn: float = 0.0      # degrees, positive = right (clockwise)
    use: bool = False
    attack: bool = False
    run: bool = True
    pistol: bool = False   # switch to the pistol (slot 2)


@dataclass
class Watch:
    """A trigger's expected effect being checked as it plays out."""
    expect: Expect
    since: int                  # tic the trigger fired
    line: int
    reached: int | None = None  # tic it got there
    late: bool = False          # logged as late
    over: bool = False          # its hold window has run out
    step: int = -1              # index of the step that fired it
    used: bool = False          # its closing has already prompted a replan


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


def _run_controls(angle: float, direction: float) -> Controls:
    """Full thrust towards world `direction` (running and strafing as needed), turning to
    face it."""
    d = _norm(direction - angle)
    rad = math.radians(d)
    return Controls(forward=50.0 * math.cos(rad), strafe=-50.0 * math.sin(rad),
                    turn=max(-RUN_TURN, min(RUN_TURN, -d)))


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
        self._lift_sectors = {s for t in nav.triggers if not t.permanent for s in t.effects}
        self.keys = set(keys or ())
        self.states = {s: set(h) for s, h in (states or nav.initial_states()).items()}
        self.replan_level = replan_level
        self.log = FollowerLog()
        self.motion = Motion()
        self.step_i = -1
        self.done = False
        self.failed = False
        self.replans = 0
        self.fired: frozenset = frozenset()  # one-shot stair switches used (for replans)
        self._drops_ok: set = set()  # unexpected drops already replanned for (this step)
        self._edge_wait = False  # holding at an edge for a sector to rise to it (logged once)
        self._settle_logged = -1  # step whose settle wait has been logged
        self._cross_ready = False  # this cross step's lift is at rest (or we've stopped waiting)
        self._cross_wait_logged = False
        self._goal_tic = 0  # when we got to the step's goal
        self._last_pos: tuple[float, float] | None = None
        self._sprint = None  # (jump, heading, start tic) while executing a run-jump
        self._run = None     # running over sinking floors (_run_step)
        self._barrel = None  # (where, since tic) of a barrel in the way being shot (_shoot_barrel)
        # Floors that sink once stepped on (a walk-over line on their own edge lowers them:
        # MAP08's exit run over the nukage): a jump off one goes at once, without settling.
        self._sinking = set()
        for t in nav.triggers:
            if t.line >= 0 and t.info.trigger in (Trigger.W1, Trigger.WR):
                line = nav.map.linedefs[t.line]
                for sec in {nav.map.front_sector(line), nav.map.back_sector(line)}:
                    if sec in t.effects and any(h[0] < nav.movers.real[sec][0]
                                                for h in t.effects[sec]):
                        self._sinking.add(sec)
        self._lineup_tic: int | None = None  # when we arrived at a jump's run-up
        self._brake_until = -1               # after landing a jump, until we've stopped
        self._jump_wait = None
        self._reopening = False              # waiting out a "close, wait 30s, open" door
        self._watches: list[Watch] = []      # what fired triggers should be doing
        self._settle: list[Expect] = []      # moves we're waiting on before the next step
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
        subs = step.substeps()
        if self.phase == "settle":
            sub = ", ".join(f"wait for sector {e.sector}" for e in self._settle)
        elif self.phase == "walk":
            sub = subs[0]
        else:
            sub = subs[1] if len(subs) > 1 else step.action
        return f"step {self.step_i + 1}/{len(self.steps)}: {sub} ({step.label})"

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
                effects = done.trigger.effects_now(self.states)
                self.states = apply_trigger(self.states, done.trigger)
                if done.trigger.permanent:
                    # Gone for good, risen or lowered: replans mustn't count on the old height
                    # (MAP29: one routed over a floor at where it had risen from a minute before)
                    for sec, hs in effects.items():
                        self.states[sec] = set(hs)
                if done.trigger.once and done.trigger.cumulative:
                    self.fired = self.fired | {done.trigger.line}
                for e in done.expects:  # watch what it should do, against the timings
                    self._watches.append(Watch(e, tic, done.trigger.line, step=self.step_i))
            if done.key is not None:
                self.keys.add(done.key)
            # Sub-step: let a quick permanent move the next path needs finish first.
            settle = [e for e in done.expects
                      if e.needed and e.hold is None and e.travel <= SETTLE_MAX]
            if settle:
                self._settle, self._settle_tic, self.phase = settle, tic, "settle"
                return
        self._advance(tic)

    def _advance(self, tic: int) -> None:
        self.step_i += 1
        self._step_replans = 0
        self._drops_ok = set()
        self._cross_ready = self._cross_wait_logged = False
        if self.step_i >= len(self.steps):
            self.done = True
            self.log.add(tic, "route finished")
            return
        step = self.steps[self.step_i]
        self.log.add(tic, f"step {self.step_i + 1}/{len(self.steps)}: "
                          + " -> ".join(step.substeps()))
        self._step_tic = tic
        self._crossed = False
        self._item = self._item_of(step)
        self._press_tries = 0
        self._before: dict | None = None
        self._cross_tic: int | None = None
        self._height_wait: int | None = None
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
                # carry on from the landing point, then the waypoints after the takeoff (one
                # at the landing point itself is dropped; a nearby one can be where the step
                # ends: E4M6's switch is 26 units past the landing, and a track of just that
                # point is "already there")
                cur = [jump.dest]
                land = next((k for k in range(i + 2, len(pts))
                             if pts[k] is not None and math.dist(pts[k], jump.dest) < 32), None)
                i = land if land is not None else i + 2
                if i < len(pts) and pts[i] is not None and math.dist(pts[i], jump.dest) < 4:
                    i += 1
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
        if self.nav.trenches:
            # Slots too narrow to stand in, at the floor beside them, as planned
            # (Navigator._fill_trenches): otherwise the real floor reads as a step to wait out.
            heights = dict(obs.heights)
            for t in self.nav.trenches:
                if t in heights:
                    f = self.nav.trench_floor(t, lambda o: obs.heights.get(o, (math.inf,))[0])
                    heights[t] = (f, heights[t][1])
            obs = replace(obs, heights=heights)
        self.motion.observe(obs)
        self._live = obs.heights   # (corner cutting keeps off ledges as they are now)
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
        self._check_watches(obs)
        if obs.keys is not None:  # the engine knows what we hold
            self.keys |= set(obs.keys)
        if self.phase == "walk":
            return self._walk(obs)
        if self.phase == "fetch":
            return self._fetch(obs)
        if self.phase == "settle":
            return self._settle_step(obs)
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
        if self._run is not None:
            return self._run_step(obs)
        if self._barrel is not None:
            return self._shoot_barrel(obs)
        step = self.steps[self.step_i]
        t = step.trigger
        if step.action == "cross" and t is not None and t.info.trigger is Trigger.WR \
                and not t.permanent and not self._cross_ready:
            # A lift line to cross for its window: if the lift's already away (set off on the
            # way, MAP09: a jump landing on the line), crossing now does nothing and the
            # window is shorter than planned. Wait for it to come back first.
            init = self.nav.movers.initial
            away = [x for x in t.effects if (live := obs.heights.get(x)) is not None
                    and abs(live[0] - init[x][0]) > 0.5]
            if away and obs.tic - self._step_tic < CROSS_REST_WAIT:
                if not self._cross_wait_logged:
                    self.log.add(obs.tic, f"sector {away[0]} already away: waiting for it to "
                                          f"come back before crossing line {t.line}")
                    self._cross_wait_logged = True
                self._progress_tic = obs.tic
                return self.motion.drive(obs, (0.0, 0.0), obs.angle)
            self._cross_ready = True
        if obs.tic < self._brake_until:
            if self.motion.speed > 1.0:  # just landed a jump: stop first
                self._progress_tic = obs.tic
                return self.motion.drive(obs, (0.0, 0.0), self._look)
            self._brake_until = -1
        seg = self.segments[self.seg_i]
        track = seg.track
        pos = (obs.x, obs.y)
        self.k, self.s, self.cross = track.project(pos, self.s)

        if self._lost(obs):
            if not self._replan(obs, "off course"):
                return self._fail(obs, "off course and no way back")
            return self._walk(obs)

        self._speed_cap = math.inf
        if self._unexpected_drop(obs):
            return self._walk(obs)
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
        if step.action == "pickup" and obs.keys is not None and step.key in obs.keys:
            self.phase = "act"  # got it (maybe on the way): done
            return self._act_at_goal(obs)
        arrive = FINAL_REACHED if stop else (PICKUP_REACHED if step.action == "pickup" else REACHED)
        if seg.end == "goal" and (to_end < arrive or self._touching_item(pos)):
            self._reset_progress(obs.tic)
            if step.action == "pickup" and obs.keys is not None and self._item is not None:
                self.phase = "fetch"  # at the spot but not holding it: go and get it
                return self._fetch(obs)
            self.phase = "act"
            return self._act_at_goal(obs)

        # Progress and getting unstuck.
        if self.s > self._best_s + 2:
            if self._best_s != -math.inf:
                self._unsticks = 0  # got past whatever snagged us
            self._best_s, self._progress_tic = self.s, obs.tic
        if obs.tic < self._unstick_until:
            return self._unstick(obs)
        if obs.tic - self._progress_tic > STUCK_TICS and step.action == "use" \
                and step.trigger is not None and step.trigger.line >= 0 \
                and seg is self.segments[-1] and self._line_distance(pos, step.trigger.line) <= PRESS_RANGE:
            # Can't get to the spot, but the switch is in reach from here (MAP27's, past a
            # closed door the press opens first): press from here.
            self._reset_progress(obs.tic)
            self.phase = "act"
            return self._act_at_goal(obs)
        if obs.tic - self._progress_tic > STUCK_TICS:
            self._unsticks += 1
            if self._unsticks > UNSTICKS_BEFORE_REPLAN:
                if not self._replan(obs, "stuck"):
                    return self._fail(obs, "stuck")
                return self._walk(obs)
            barrel = next((b for b in obs.barrels or () if math.dist(b, pos) < BARREL_IN_WAY), None)
            if barrel is not None:
                # Wedged against a barrel (MAP23 has rows of them across corridors): blow
                # it up from a safe distance, as a player would.
                self.log.add(obs.tic, f"barrel in the way at ({barrel[0]:.0f}, {barrel[1]:.0f}): "
                                      f"shooting it")
                self._barrel = (barrel, obs.tic)
                return self._shoot_barrel(obs)
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
        if seg.end == "jump":  # run-ups are often right at a ledge: come in gently, slowly
            # over the last stretch (braking does nothing between steps down: MAP17's run-up
            # at the foot of a stair was overshot into the pit beyond)
            rest = track.length - self.s
            if self._steps_down_to_end(obs, track):
                speed = min(speed, 0.06 * rest + 1.5, 6.0 if rest < 128 else FULL_SPEED)
            else:
                speed = min(speed, 0.12 * rest + 1.5)
        if math.dist(pos, carrot) < 1:
            return self.motion.drive(obs, (0.0, 0.0), self._look_angle(obs, seg))
        return self.motion.move(obs, _heading(pos, carrot), speed, self._look_angle(obs, seg))

    def _item_of(self, step: Step):
        """Where the item a pickup step is for lies (the nearest such thing to its goal)."""
        if step.action != "pickup" or step.key is None:
            return None
        goal = next((p for p in reversed(step.path.waypoints) if p is not None), None)
        things = [(t.x, t.y) for t in self.nav.map.things if t.type == step.key.value]
        return min(things, key=lambda p: math.dist(p, goal)) if things and goal else None

    def _touching_item(self, pos) -> bool:
        """Close enough that the boxes overlap: the item is picked up, wherever the spot was
        (spots by ledges can be ones the box can't quite reach)."""
        return self._item is not None and \
            max(abs(pos[0] - self._item[0]), abs(pos[1] - self._item[1])) < \
            PLAYER_RADIUS + ITEM_RADIUS - 1

    def _carrot(self, pos, track: Track) -> tuple[float, float]:
        """Point to steer for: as far ahead on the track (up to CARROT_MAX) as the straight
        line there keeps the player's box clear, so corners are rounded where there's room;
        otherwise the next waypoint exactly."""
        for ahead in CARROT_STEPS:
            q = track.point_at(self.s + ahead)
            # (A path that doubles back can put a point far along it right next to us.)
            along = min(ahead, track.length - self.s)
            if math.dist(pos, q) < 0.6 * along:
                continue
            if math.dist(pos, q) < 1 or self._chord_clear(pos, q):
                return q
        return track.carrot(self.k, self.s, TRACK_AHEAD)

    def _chord_clear(self, p, q) -> bool:
        """Can the player's box move straight from p to q without touching a wall, solid thing
        or (uncrossed) ledge edge? Edges we're already close to only need not get closer."""
        for e1, e2 in self.nav._walls_near(p, q, 2 * PLAYER_RADIUS, self._crossing_lines,
                                           getattr(self, "_live", None)):
            length = max(math.dist(e1, e2), 1e-6)
            c, s = abs(e2[0] - e1[0]) / length, abs(e2[1] - e1[1]) / length
            reach = PLAYER_RADIUS * (c + s) + 2   # the square box's extent across this edge
            d = _segment_distance(p, q, e1, e2)
            if d < reach and d < _segment_distance(p, p, e1, e2) - 0.5:
                return False
        # Nor across a teleport line from its front (cutting a corner over one teleports us).
        m = self.nav.map
        for cell in _cells(min(p[0], q[0]), min(p[1], q[1]), max(p[0], q[0]), max(p[1], q[1])):
            for li in self.nav.mesh._line_grid.get(cell, ()):
                line = m.linedefs[li]
                if line.special not in TELEPORTERS_FOR_PLAYERS or li in self._crossing_lines:
                    continue
                v1, v2 = (m.vertexes[line.v1].x, m.vertexes[line.v1].y), \
                    (m.vertexes[line.v2].x, m.vertexes[line.v2].y)
                in_front = (v2[0] - v1[0]) * (p[1] - v1[1]) - (v2[1] - v1[1]) * (p[0] - v1[0]) < 0
                if in_front and _segment_distance(p, q, v1, v2) < 1:
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
        """Direction to back off in: away from the nearest walls/obstacles and ledges (MAP29:
        backing off a stair wall walked it off the other side into the pit), else backwards."""
        pos = (obs.x, obs.y)
        ax = ay = 0.0
        for a, b in self.nav._walls_near(pos, pos, 40, self._crossing_lines, live=obs.heights):
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

    def _standing(self, obs: Observation):
        """(poly, point in it) of the floor we're actually standing on. DOOM holds the box up
        on the highest floor under any of it, so the centre can be out over a pit beside a
        walkway narrower than the box (MAP24's 24-wide ones) with nothing wrong: then it's
        the walkway's poly, at the bit of it under the box nearest the centre."""
        mesh = self.nav.mesh
        here = mesh.poly_at(obs.x, obs.y)
        if obs.z is None or here is None or abs(obs.heights[here.sector][0] - obs.z) < 0.5:
            return here, (obs.x, obs.y)
        r = PLAYER_RADIUS - 1
        best = None
        for dx in (-r, -r / 2, 0, r / 2, r):
            for dy in (-r, -r / 2, 0, r / 2, r):
                q = (obs.x + dx, obs.y + dy)
                p = mesh.poly_at(*q)
                if p is not None and abs(obs.heights[p.sector][0] - obs.z) < 0.5 \
                        and _inside(p.vertices, *q):
                    d = math.hypot(dx, dy)
                    if best is None or d < best[0]:
                        best = (d, p, q)
        return (best[1], best[2]) if best else (here, (obs.x, obs.y))

    def _lost(self, obs: Observation) -> bool:
        """Off the planned path (fell into a pit, pushed, unexpected teleport) for a while."""
        if obs.tic < self._settle_until:
            return False
        mesh = self.nav.mesh
        here = self._standing(obs)[0]
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
        nav, pos, step = self.nav, self._standing(obs)[1], self.steps[self.step_i]
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
                             max_expansions=200, max_seconds=3.0, fired=self.fired)
            if route is not None:
                new = steps_from_legs(route.legs, step, nav, states)
                self.steps[self.step_i:self.step_i + 1] = new
                self.log.add(obs.tic, f"{why} at ({obs.x:.0f}, {obs.y:.0f}): back on track "
                                      f"via {len(new) - 1} extra step(s)")
            elif self.replan_level:
                plan = plan_from(nav, pos, self.keys, states, max_seconds=10.0, fired=self.fired,
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
        # The heights the jump was found with must be there, and not still moving (a floor
        # that's still rising under the run-up).
        # (If it has stopped somewhere else it's not coming: jump from there anyway.)
        for sector, h in jump.requires:
            live = obs.heights.get(sector)
            # (a ceiling a little lower is fine: MAP09's door just starting to close as the lift
            # brings us up for the jump through it)
            if live is None or abs(live[0] - h[0]) > 0.5 or live[1] > h[1] + 0.5                     or live[1] < h[1] - JUMP_CEILING_SLACK:
                if self._jump_wait is None or self._jump_wait[:2] != (sector, live):
                    if self._jump_wait is None or self._jump_wait[0] != sector:
                        self.log.add(obs.tic, f"waiting for sector {sector} to reach "
                                              f"{h} before jumping (now {live})")
                    self._jump_wait = (sector, live, obs.tic)
                # still moving recently (a lift pauses ~3s at the bottom before coming back)
                still_for = 5 * TICRATE if sector in self._lift_sectors else TICRATE
                if obs.tic - self._jump_wait[2] < still_for:
                    self._lineup_tic = None
                    self._progress_tic = obs.tic
                    spot = jump.runup
                    here = self._standing(obs)[0]
                    if here is not None and here.sector == sector:
                        # Riding it (we set it off on the way to the run-up: MAP06): stand
                        # in the middle, clear of anything over the edges, or it can't come
                        # back up with us and goes down again.
                        spot = here.center
                    hold = ((spot[0] - obs.x) * 0.3, (spot[1] - obs.y) * 0.3)
                    return self.motion.drive(obs, hold, heading)
                if live is None or live[0] < h[0] - 0.5:
                    # Stopped lower than the jump needs (taking off higher is fine): it
                    # won't make it from here, so find another way with things as they are.
                    self._jump_wait = None
                    if self._replan(obs, f"sector {sector} at {live}, too low for the jump",
                                    live_only=True):
                        return self.motion.drive(obs, (0.0, 0.0), heading)
                    return self._fail(obs, "jump takeoff too low and no other way")
        # As exactly as the engine check did it: on the spot, square on, standing still
        # (some jumps only just make it). Settling longer than a second: go anyway.
        settled = math.dist((obs.x, obs.y), jump.runup) < 1.5 and abs(diff) <= 0.75 \
            and self.motion.speed < 0.3 and abs(self.motion.rate) < 0.5
        # Something the jump needs is about to go (MAP09: the door it goes through starts
        # closing a moment after the lift brings us up to it): no settling, go once facing.
        needs = {sec for sec, _h in jump.requires}
        if any(w.expect.sector in needs and w.reached is not None and not w.over
               and w.expect.hold is not None
               and w.reached + w.expect.hold - obs.tic < JUMP_HURRY for w in self._watches) \
                and math.dist((obs.x, obs.y), jump.runup) < 6 and abs(diff) <= 3:
            settled = True
        if self._lineup_tic is None:
            self._lineup_tic = obs.tic
        if settled or (obs.tic - self._lineup_tic > TICRATE and abs(diff) <= 1.5
                       and self.motion.speed < 0.5):
            self._lineup_tic = None
            self._sprint = (jump, heading, obs.tic)
            self.log.add(obs.tic, f"run-jump from ({jump.takeoff[0]:.0f}, {jump.takeoff[1]:.0f})")
            return self._sprint_jump(obs)
        hold = ((jump.runup[0] - obs.x) * 0.3, (jump.runup[1] - obs.y) * 0.3)
        self._progress_tic = obs.tic
        return self.motion.drive(obs, hold, heading)

    def _shoot_barrel(self, obs: Observation) -> Controls:
        """Back off out of its blast (128 units) and shoot the barrel that's in the way, then
        carry on along the path."""
        where, since = self._barrel
        pos = (obs.x, obs.y)
        if not any(math.dist(b, where) < 8 for b in obs.barrels or ()) \
                or obs.tic - since > BARREL_GIVE_UP:
            self._barrel = None
            self._reset_progress(obs.tic)
            self.log.add(obs.tic, "barrel gone" if obs.tic - since <= BARREL_GIVE_UP
                         else "couldn't shoot the barrel")
            return self._walk(obs)
        self._progress_tic = obs.tic
        look = _heading(pos, where)
        d = math.dist(pos, where)
        if d < BARREL_SAFE and obs.tic - since < 3 * TICRATE:
            away = math.atan2(pos[1] - where[1], pos[0] - where[0])
            return self.motion.drive(obs, (math.cos(away) * 8, math.sin(away) * 8), look)
        controls = self.motion.drive(obs, (0.0, 0.0), look)
        if obs.weapon == 1:
            controls.pistol = True
        elif abs(_norm(look - obs.angle)) < 2:
            controls.attack = True
        return controls

    def _run_step(self, obs: Observation) -> Controls:
        """Running over floors that sink once stepped on (MAP08's exit: platforms in nukage
        that go down as you land, with a turn in the middle). There's no stopping to line up
        a jump: keep running, and every few tics pick, with the movement sim from where we are
        and how fast we're going, the heading that lands on the floor nearest the goal along
        the way. Over once we're on a floor that stays put; then path on from there."""
        here = self.nav.mesh.poly_at(obs.x, obs.y)
        on_ground = obs.z is None or here is None or obs.z <= obs.heights[here.sector][0] + 0.5
        if on_ground and here is not None and here.sector not in self._sinking:
            self._run = None
            self.log.add(obs.tic, f"ran across to ({obs.x:.0f}, {obs.y:.0f}) sector "
                                  f"{here.sector}")
            if not self._replan(obs, "after the run"):
                return self._fail(obs, "nowhere to go after the run")
            return self._walk(obs)
        if on_ground and (obs.tic >= self._run["next"] or here.sector != self._run["sector"]):
            heading = self._run_heading(obs, here.sector)
            if heading is None:
                self._run = None
                self.log.add(obs.tic, "no heading off the sinking floor lands anywhere")
                if not self._replan(obs, "sinking floor", live_only=True):
                    return self._fail(obs, "stranded on a sinking floor")
                return self._walk(obs)
            self._run.update(heading=heading, sector=here.sector, next=obs.tic + RUN_REAIM)
        self.motion.rate = 0.0
        return _run_controls(obs.angle, self._run["heading"])

    def _run_heading(self, obs: Observation, sector: int) -> float | None:
        """The direction to push (from a fan all round: pushing back against our momentum
        may be what keeps us on) whose simulated run lands, off this floor, on the floor
        whose path to the step goal is shortest."""
        from evildoom.nav.physics import PlayerSim   # (physics imports this module)
        nav = self.nav
        goal = next(p for p in reversed(self.steps[self.step_i].path.waypoints)
                    if p is not None)
        heights = dict(obs.heights)
        f, c = heights[sector]
        heights[sector] = (f - RUN_AHEAD_SINK, c)   # it keeps going down meanwhile
        sim = PlayerSim(nav, heights)
        live = {s: {h} for s, h in obs.heights.items()}
        doors = nav.local_doors(self.keys)
        going = math.degrees(math.atan2(self.motion.disp[1], self.motion.disp[0])) \
            if math.hypot(*self.motion.disp) > 2 else obs.angle
        cost_from: dict[int, float | None] = {}
        best = None
        for off in range(-180, 180, 15):
            heading = going + off
            sim.place(obs.x, obs.y, obs.angle)
            sim.vx, sim.vy = self.motion.disp
            sim.z = obs.z if obs.z is not None else sim.floorz
            land = None
            try:
                for _ in range(2 * TICRATE):
                    sim.step(_run_controls(sim.angle, heading))
                    q = nav.mesh.poly_at(sim.x, sim.y)
                    if q is not None and q.sector != sector and sim.z <= sim.floorz \
                            and abs(sim.floorz - heights[q.sector][0]) < 0.5:
                        land = q
                        break
            except Exception:
                continue
            if land is None or nav.damage.get(land.sector) \
                    or heights[land.sector][0] < f - MAX_STEP:
                continue  # into the nukage (or nowhere)
            if land.sector not in cost_from:
                path = nav.find_path((sim.x, sim.y), goal, live, doors)
                cost_from[land.sector] = path.cost if path is not None else None
            cost = cost_from[land.sector]
            if cost is not None and (best is None or (cost, abs(off)) < best[:2]):
                best = (cost, abs(off), heading)
        return best[2] if best else None

    def _sprint_jump(self, obs: Observation) -> Controls:
        """Full speed in a straight line, exactly as the engine check did it: only up to the
        edge for a soft jump (then coast: it lands slower, for narrow strips), otherwise
        through the flight. On touchdown on the landing sector stop sprinting so the landing
        is braked at once."""
        jump, heading, since = self._sprint
        ux, uy = math.cos(math.radians(heading)), math.sin(math.radians(heading))
        flown = (obs.x - jump.takeoff[0]) * ux + (obs.y - jump.takeoff[1]) * uy
        here = self.nav.mesh.poly_at(obs.x, obs.y)
        floor = obs.heights[here.sector][0] if here is not None else None
        # Landed only on the sector the jump was checked to land on, as the check had it: a
        # jump can skim another floor on the way (E4M2: the far side of the door it takes off
        # from), and stopping there drops us short.
        landed = flown > 0 and here is not None and obs.z is not None and \
            here.sector == self.nav.mesh.polys[jump.dest_poly].sector and abs(obs.z - floor) < 1
        if landed or flown >= math.dist(jump.takeoff, jump.dest) - 8 \
                or obs.tic - since > 4 * TICRATE:
            self._sprint = None
            land = self.nav.mesh.poly_at(obs.x, obs.y)
            self.log.add(obs.tic, f"landed at ({obs.x:.0f}, {obs.y:.0f}) sector "
                                  f"{land.sector if land else None}")
            self.seg_i = min(self.seg_i + 1, len(self.segments) - 1)
            self._enter_segment(obs.tic)
            self._look = heading
            if land is not None and land.sector in self._sinking:
                # Going down under us: no settling, run on across (_run_step).
                self.log.add(obs.tic, f"sector {land.sector} sinking: running on")
                self._run = {"heading": heading, "sector": None, "next": obs.tic}
                return self._run_step(obs)
            # Landing spots can be small: shed the sprint speed before going on.
            self._brake_until = obs.tic + 2 * TICRATE
            return self._walk(obs)
        diff = _norm(heading - obs.angle)
        self.motion.rate = 0.0
        push = flown < (0 if jump.soft else math.dist(jump.takeoff, jump.dest) - 8)
        # Keep on the jump's line till takeoff (MAP27's clears a ledge corner by nothing: 2
        # units of drift on the run-up and the box caught it). The check ran it dead straight.
        strafe = 0.0
        if flown < 0:
            side = (obs.x - jump.takeoff[0]) * uy - (obs.y - jump.takeoff[1]) * ux
            strafe = max(-15.0, min(15.0, -4.0 * side))  # (side: how far right of the line)
        return Controls(forward=50.0 if push else 0.0, strafe=strafe,
                        turn=max(-3.0, min(3.0, -diff)))

    # --- doors and lifts in the way --------------------------------------------------------

    def _unexpected_drop(self, obs: Observation) -> bool:
        """A crossing just ahead that's become a one-way drop the plan didn't mean to take
        (MAP08: walking to the red key goes over a line that sinks the bridge back into the
        nukage; walking on would be into the pit). Stop and replan with the heights as they
        are, which finds the switch that raises it again. True if it replanned."""
        mesh, path = self.nav.mesh, self.steps[self.step_i].path
        planned = {n.split("into sector ")[1] for n in path.steps if "one-way drop" in n}
        for j in range(self.poly_i, min(self.poly_i + 4, len(path.polys) - 1)):
            cur, nxt = mesh.polys[path.polys[j]], mesh.polys[path.polys[j + 1]]
            if cur.sector == nxt.sector:
                continue
            portal = next((mesh.portals[pi] for pi in cur.portals
                           if mesh.portals[pi].other(cur.index) == nxt.index), None)
            if portal is None or math.dist(_closest_on_segment((obs.x, obs.y), portal),
                                           (obs.x, obs.y)) > DROP_CHECK_DIST:
                return False
            a, b = obs.heights[cur.sector], obs.heights[nxt.sector]
            key = (cur.sector, nxt.sector, a, b)
            if str(nxt.sector) in planned or key in self._drops_ok \
                    or not can_cross(a, b) or can_cross(b, a):
                continue
            if not any(can_cross(hb, ha) for ha in self.states.get(cur.sector, ())
                       for hb in self.states.get(nxt.sector, ())):
                continue  # one-way in the plan as well
            coming = next((w for w in self._watches if w.expect.sector == nxt.sector
                           and w.reached is None and can_cross(a, w.expect.heights)
                           and can_cross(w.expect.heights, a)
                           and obs.tic - w.since < 1.5 * w.expect.travel + 10 * TICRATE), None)
            if coming is not None:
                # Still on its way to where the plan has it (MAP29: a floor that takes half a
                # minute to rise to the walkway): wait at the edge for it.
                if not self._edge_wait:
                    self.log.add(obs.tic, f"waiting for sector {nxt.sector} to reach "
                                          f"{coming.expect.heights}")
                    self._edge_wait = True
                self._speed_cap = 0.0
                self._progress_tic = self._step_tic = obs.tic
                return False
            self._drops_ok.add(key)
            return self._replan(obs, f"unexpected drop into sector {nxt.sector}",
                                live_only=True)
        return False

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

    def _line_distance(self, pos, li: int) -> float:
        m = self.nav.map
        line = m.linedefs[li]
        a = (m.vertexes[line.v1].x, m.vertexes[line.v1].y)
        b = (m.vertexes[line.v2].x, m.vertexes[line.v2].y)
        return math.dist(pos, _closest_point(pos, a, b))

    def _use_intercepted(self, pos, target) -> bool:
        """Does the use trace from pos to target cross a line with a special on the way (a
        press would set that off instead)? Lines ending at target don't count."""
        m, grid = self.nav.map, self.nav.mesh._line_grid
        for li in {li for c in _cells(*pos, *target) for li in grid.get(c, ())}:
            line = m.linedefs[li]
            if not line.special:
                continue
            a = (m.vertexes[line.v1].x, m.vertexes[line.v1].y)
            b = (m.vertexes[line.v2].x, m.vertexes[line.v2].y)
            if math.dist(_closest_point(target, a, b), target) < 1:
                continue  # the line being pressed
            if _segments_cross(pos, target, a, b):
                return True
        return False

    def _door_line_point(self, sector: int, pos):
        """Nearest point (a little in from its ends) on a use line that opens this door."""
        m, best = self.nav.map, None
        for t in self.nav.triggers:
            if t.info.trigger in (Trigger.DR, Trigger.D1) and sector in t.effects:
                line = m.linedefs[t.line]
                a = (m.vertexes[line.v1].x, m.vertexes[line.v1].y)
                b = (m.vertexes[line.v2].x, m.vertexes[line.v2].y)
                length = max(math.dist(a, b), 1e-6)
                k = min(0.5, 8 / length)
                q = _closest_point(pos, (a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k),
                                   (b[0] + (a[0] - b[0]) * k, b[1] + (a[1] - b[1]) * k))
                if best is None or math.dist(pos, q) < math.dist(pos, best):
                    best = q
        return best

    def _steps_down_to_end(self, obs: Observation, track: Track) -> bool:
        """Does the last 128 units of the track go down any steps (live floors)? Braking
        does nothing while dropping between them."""
        mesh, floors = self.nav.mesh, []
        start = max(0.0, track.length - 128)
        for k in range(9):
            q = mesh.poly_at(*track.point_at(start + (track.length - start) * k / 8))
            if q is not None:
                floors.append(obs.heights[q.sector][0])
        return any(b < a for a, b in zip(floors, floors[1:]))

    def _reopens(self, sector: int, live) -> bool:
        """Is `sector` shut by a "close, wait 30s, open" door special (it opens again by
        itself)?"""
        return any(t.info.special in DOOR_CLOSE_30 and sector in t.effects
                   and any(abs(live[1] - h[1]) < 1 for h in t.effects[sector])
                   for t in self.nav.triggers)

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
        # the part of the doorway in front of us (doors can be wide), or the gate door's line;
        # for a door, its use line (the face with the door special: MAP12's diagonal blue
        # door isn't the side the path goes in by)
        door_point = use_point or (self._door_line_point(sector, pos) if opener else None) \
            or _closest_on_segment(pos, portal)
        if opener:
            # how far to the door, along the path where it goes round corners
            track = self.segments[self.seg_i].track
            along = track.project(door_point, self.s, DOOR_CHECK_DIST + 64)[1] - self.s
            dist = max(math.dist(pos, door_point), along)
            here = self.nav.mesh.poly_at(*pos)
            # (Use reaches 64: close enough to press, whatever the planner's list of
            # door-user polys says, which keeps a margin.)
            if use_point is None and (here is None or here.index not in self.doors[sector][1]) \
                    and math.dist(pos, door_point) > PRESS_RANGE:
                # Not somewhere the door can be pressed from (something else in the way of the
                # use trace, e.g. another switch line): keep walking up to it.
                self._speed_cap = max(1.0, 0.3 * dist)
                return None
            standoff = DOOR_STANDOFF
            if self._use_intercepted(pos, door_point):
                # Another switch line between us and the door would take the press (E3M9: a
                # door-close switch just in front of the red door): go right up to the door.
                standoff = PLAYER_RADIUS + 2
            if dist > standoff + 4:
                # Closed door ahead: keep following the path, slowing to stop in front of it.
                self._speed_cap = max(1.0, min(math.sqrt(2 * BRAKE * (dist - standoff)),
                                               0.3 * (dist - standoff) + 1))
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
            back = PLAYER_RADIUS + 2
            spot = (door_point[0] + nx * back, door_point[1] + ny * back)
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
                        # The nearest bit of it (a long lift edge's middle can be out of use
                        # range: MAP29), stepping up to it if need be.
                        m = self.nav.map
                        line = m.linedefs[t.line]
                        la = (m.vertexes[line.v1].x, m.vertexes[line.v1].y)
                        lb = (m.vertexes[line.v2].x, m.vertexes[line.v2].y)
                        length = max(math.dist(la, lb), 1e-6)
                        k = ((pos[0] - la[0]) * (lb[0] - la[0]) + (pos[1] - la[1]) * (lb[1] - la[1])) \
                            / length ** 2
                        k = min(max(k, min(0.5, 8 / length)), max(0.5, 1 - 8 / length))
                        point = (la[0] + (lb[0] - la[0]) * k, la[1] + (lb[1] - la[1]) * k)
                        look = _heading(pos, point)
                        if math.dist(pos, point) > PRESS_RANGE:
                            gap = math.dist(pos, point) - PRESS_RANGE + 4
                            h = math.radians(look)
                            want = (math.cos(h) * min(4.0, 0.3 * gap), math.sin(h) * min(4.0, 0.3 * gap))
                        else:
                            use = abs(_norm(look - obs.angle)) < 10 and \
                                obs.tic - self._last_use >= USE_EVERY
                    break
        controls = self.motion.drive(obs, want, look)
        controls.use = use
        if use:
            self._last_use = obs.tic
        # Nothing to press here: it may need a walk-over line crossing again (a blazing lift
        # that went back up), so replan from the live heights after a few seconds; at once
        # if we know the window we were counting on has run out.
        missed = not opener and waited == TICRATE // 2 and self._window_over(sector)
        if waited == 0:
            self._reopening = self._reopens(sector, live[sector])
            if self._reopening:
                self.log.add(obs.tic, f"sector {sector} shut for 30s: waiting for it to reopen")
        if self._reopening:
            # A "close, wait 30s, open" door we walked into the trigger of (E1M6): nothing to
            # do but wait (till it's open, not just till it starts opening), as a player would;
            # no point replanning every few seconds. Spin round lazily meanwhile.
            if waited > 33 * TICRATE:
                self._reopening = False
                if not self._replan(obs, f"sector {sector} not reopening"):
                    return self._fail(obs, f"waited too long for sector {sector} to reopen")
            return self.motion.drive(obs, want, (obs.angle + 10) % 360)
        # Still on its way to where the plan has it (MAP29's floor that takes half a minute to
        # rise to the walkway): nothing to replan, just wait.
        coming = any(w.expect.sector == sector and w.reached is None
                     and obs.tic - w.since < 1.5 * w.expect.travel + 10 * TICRATE
                     for w in self._watches)
        if coming and not opener:
            self._wait_tic = obs.tic - min(waited, 4 * TICRATE)  # (keeps "waited" short)
            return self.motion.drive(obs, want, look)
        if missed or not opener and waited == 5 * TICRATE and not switch_nearby:
            why = f"missed sector {sector}'s window" if missed else f"sector {sector} not coming"
            if self._replan(obs, why, live_only=True):
                return self.motion.drive(obs, (0.0, 0.0), look)
        if waited > 15 * TICRATE:
            if not self._replan(obs, f"sector {sector} not opening"):
                return self._fail(obs, f"waited too long for sector {sector}: live "
                                       f"{live[cur.sector]} -> {live[sector]}")
            return self.motion.drive(obs, (0.0, 0.0), look)
        return controls

    # --- acting at the end of a step ------------------------------------------------------

    def _settle_step(self, obs: Observation) -> Controls:
        """Wait (standing still) for the sectors the next path needs to finish moving."""
        def at(e) -> bool:
            live = obs.heights.get(e.sector)
            return live is not None and abs(live[0] - e.heights[0]) < 1 \
                and abs(live[1] - e.heights[1]) < 1
        waiting = [e for e in self._settle if not at(e)]
        took = (obs.tic - self._settle_tic) / TICRATE
        if not waiting:
            self.log.add(obs.tic, f"sectors {[e.sector for e in self._settle]} in place after "
                                  f"{took:.1f}s")
        elif obs.tic - self._settle_tic > max(1.5 * e.travel for e in waiting) + TICRATE:
            e = waiting[0]
            self.log.add(obs.tic, f"sector {e.sector} didn't reach {e.heights} in {took:.1f}s "
                                  f"(expected {e.travel / TICRATE:.1f}s, it's at "
                                  f"{obs.heights.get(e.sector)}); going on")
        else:
            self._progress_tic = obs.tic
            return self.motion.drive(obs, (0.0, 0.0), self._look)
        self._settle = []
        self._advance(obs.tic)
        if self.done:
            return Controls()
        return self._walk(obs)

    def _check_watches(self, obs: Observation) -> None:
        """Compare what fired triggers are doing with what the plan expected, and log it:
        arrivals, lateness, and when a temporary state (lift down, door open) runs out."""
        for w in self._watches:
            e = w.expect
            live = obs.heights.get(e.sector)
            if live is None or w.over:
                continue
            at = abs(live[0] - e.heights[0]) < 1 and abs(live[1] - e.heights[1]) < 1
            if w.reached is None:
                if at:
                    w.reached = obs.tic
                    took = (obs.tic - w.since) / TICRATE
                    if e.needed or took > 1.5 * e.travel / TICRATE + 1:
                        self.log.add(obs.tic, f"sector {e.sector} reached {e.heights} after "
                                              f"{took:.1f}s (expected {e.travel / TICRATE:.1f}s)")
                elif not w.late and obs.tic - w.since > 1.5 * e.travel + 2 * TICRATE:
                    w.late = True
                    self.log.add(obs.tic, f"sector {e.sector} hasn't reached {e.heights} "
                                          f"{(obs.tic - w.since) / TICRATE:.1f}s after line "
                                          f"{w.line} (expected {e.travel / TICRATE:.1f}s); "
                                          f"it's at {live}")
            elif e.hold is not None and (not at and obs.tic - w.reached > e.hold
                                         or obs.tic - w.reached > e.hold + e.travel):
                w.over = True
                if e.needed:
                    self.log.add(obs.tic, f"sector {e.sector}'s window is over (line {w.line})")

    def _window_over(self, sector: int) -> bool:
        """Did the temporary state the previous step set up for this sector (lift down, door
        open) run out? Each closing only counts once."""
        for w in self._watches:
            if w.over and not w.used and w.expect.sector == sector \
                    and w.step == self.step_i - 1:
                w.used = True
                return True
        return False

    def _effect_started(self, obs: Observation, t) -> bool:
        """Has the trigger done something? A sector it moves has moved since we fired it, or
        is already at a height it moves to (a door that was open anyway)."""
        if not t.effects:
            return True  # nothing we can check
        for s, hs in t.effects.items():
            live = obs.heights.get(s)
            if live is None:
                continue
            was = (self._before or {}).get(s)
            if was is not None and (abs(live[0] - was[0]) > 0.5 or abs(live[1] - was[1]) > 0.5):
                return True
            if any(abs(live[0] - h[0]) < 1 and abs(live[1] - h[1]) < 1 for h in hs):
                return True
        return False

    def _fetch(self, obs: Observation) -> Controls:
        """At the pickup spot without the item: walk straight at it for a moment."""
        step = self.steps[self.step_i]
        if obs.keys is not None and step.key in obs.keys:
            self.phase = "act"
            return self._act_at_goal(obs)
        if obs.tic - self._progress_tic > 3 * TICRATE:
            self.log.add(obs.tic, f"couldn't pick up {step.key.name} at ({obs.x:.0f}, "
                                  f"{obs.y:.0f}); carrying on")
            self.phase = "act"
            return self._act_at_goal(obs)
        heading = _heading((obs.x, obs.y), self._item)
        speed = min(6.0, 0.3 * math.dist((obs.x, obs.y), self._item) + 1)
        return self.motion.move(obs, heading, speed, heading)

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

    def _at_shot_height(self, obs: Observation, step: Step) -> bool:
        """Is the floor we're on at a height where a level shot hits the gun line? (Shots
        go straight out at SHOT_HEIGHT; some switches are hit from a lift at the right
        height.) Waits at most a few seconds, then lets the shot be tried anyway."""
        here = self.nav.mesh.poly_at(obs.x, obs.y)
        shot = self.nav.shot_floors.get((step.trigger.line, here.index)) if here else None
        if shot is None:
            return True
        floor = obs.heights[here.sector][0]
        if shot[0] <= floor <= shot[1]:
            return True
        if self._height_wait is None:
            self._height_wait = obs.tic
            self.log.add(obs.tic, f"waiting to be at the right height to shoot line "
                                  f"{step.trigger.line} (floor {floor:.0f}, needs "
                                  f"{shot[0]:.0f}..{shot[1]:.0f})")
        return obs.tic - self._height_wait > 6 * TICRATE

    def _carry_on(self, obs: Observation) -> Controls:
        """Right after finishing a step: whatever comes next (a settle sub-step or walking)."""
        if self.done or self.failed:
            return Controls()
        if self.phase == "settle":
            return self._settle_step(obs)
        return self._walk(obs)

    def _act_at_goal(self, obs: Observation) -> Controls:
        step = self.steps[self.step_i]
        if step.action == "endure":
            # An exit floor: stand on it while it hurts us down to 10 health, which ends the
            # level (E1M8). Nothing to do but wait.
            self._step_tic = self._progress_tic = obs.tic
            return self.motion.drive(obs, (0.0, 0.0), self._look)
        if step.action == "cross" and not self._crossed and step.trigger is not None:
            return self._walk_across(obs)
        if step.action == "cross" and step.trigger is not None \
                and step.trigger.info.category is not Category.EXIT:
            if self._cross_tic is None:
                self._before = {s: obs.heights.get(s) for s in step.trigger.effects}
                self._cross_tic = obs.tic
            if not self._effect_started(obs, step.trigger):
                if obs.tic - self._cross_tic < TICRATE:
                    return self.motion.drive(obs, (0.0, 0.0), self._look)
                self.log.add(obs.tic, f"crossing line {step.trigger.line} didn't do anything")
        if step.action in ("pickup", "cross"):
            self._next_step(obs.tic)
            return self._carry_on(obs)
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
        if step.action == "shoot":
            t = 0.5  # (a shot reaches: the middle, where it's widest to hit)
        aim = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
        look = _heading(pos, aim)
        err = abs(_norm(look - obs.angle))
        span = abs(_norm(_heading(pos, a) - _heading(pos, b)))
        # Hold still (brake) while turning to face the line.
        end = self.segments[-1].track.pts[-1]
        if self._press_tries:  # missed: stand a bit closer each time
            d = math.dist(end, aim)
            k = min(8 * self._press_tries, max(d - PLAYER_RADIUS - 4, 0)) / max(d, 1e-6)
            end = (end[0] + (aim[0] - end[0]) * k, end[1] + (aim[1] - end[1]) * k)
        hold = ((end[0] - obs.x) * 0.2, (end[1] - obs.y) * 0.2)
        controls = self.motion.drive(obs, hold, look)
        facing = err < (min(4.0, span / 3) if step.action == "shoot" else 4)
        if step.action == "face":  # walk up to it and look at it; leave pressing to a human
            if facing:
                self.log.add(obs.tic, f"facing line {step.trigger.line}")
                self._next_step(obs.tic)
            return controls
        t = step.trigger
        if step.action == "use" and self._action_tic is None and t is not None \
                and t.shift_from is not None:
            # Stairs don't build while their first step is moving (MAP21: it's also a lift):
            # wait till it's back up where the stairs left it.
            s0 = t.shift_from[0]
            top = max(h[0] for h in self.states[s0])
            live = obs.heights.get(s0)
            if live is not None and live[0] < top - 0.5:
                self._progress_tic = self._step_tic = obs.tic
                return controls
        if step.action in ("use", "shoot") and self._action_tic is None and t is not None \
                and t.recompute is not None:
            # Where it takes things to depends on what's round it when pressed: wait for the
            # lifts and doors among those to be where the plan has them (MAP09: a floor going
            # down to the lowest round it, pressed with the lift next to it still down, went
            # all the way down).
            init = self.nav.movers.initial
            off = [x for x in t.relevant
                   if (live := obs.heights.get(x)) is not None
                   and (h := self.nav._rep(self.states[x], init[x])) is not None
                   and (abs(live[0] - h[0]) > 0.5 or abs(live[1] - h[1]) > 0.5)]
            if off and self._settle_logged != self.step_i:
                self.log.add(obs.tic, f"waiting for sector {off[0]} to settle before "
                                      f"line {t.line}")
                self._settle_logged, self._goal_tic = self.step_i, obs.tic
            if off and obs.tic - self._goal_tic < SETTLE_WAIT:
                self._progress_tic = self._step_tic = obs.tic
                return controls
        nxt = self.steps[self.step_i + 1] if self.step_i + 1 < len(self.steps) else None
        if step.action in ("use", "shoot") and self._action_tic is None and nxt is not None \
                and nxt.action == "cross" and nxt.trigger is not None \
                and nxt.trigger.info.trigger is Trigger.WR and not nxt.trigger.permanent:
            # The next step crosses a lift line for its window: if that lift is away (set off
            # on the way, MAP09), wait for it now, before this press starts a timer of its own
            # (the door the lift ride leads through) that the wait would use up.
            init = self.nav.movers.initial
            away = [x for x in nxt.trigger.effects if (live := obs.heights.get(x)) is not None
                    and abs(live[0] - init[x][0]) > 0.5]
            if away and obs.tic - self._step_tic < CROSS_REST_WAIT:
                if not self._cross_wait_logged:
                    self.log.add(obs.tic, f"sector {away[0]} already away: waiting for it to "
                                          f"come back before line {t.line if t else '?'}")
                    self._cross_wait_logged = True
                self._progress_tic = obs.tic
                return controls
        if step.action == "shoot" and self._action_tic is None and obs.weapon == 1:
            # A fist or chainsaw (picked up on the way: the game switches to it) doesn't set
            # off a gun line (MAP19): out with the pistol first.
            controls.pistol = True
            self._progress_tic = obs.tic
            return controls
        if step.action == "shoot" and self._action_tic is None and facing \
                and not self._at_shot_height(obs, step):
            self._progress_tic = obs.tic  # waiting for the floor (a lift) to bring us level
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
                self._before = {s: obs.heights.get(s) for s in step.trigger.effects}
                tries = f" (try {self._press_tries + 1})" if self._press_tries else ""
                self.log.add(obs.tic, f"{step.action} line {step.trigger.line}{tries}")
            # Did it do anything? (An exit ends the level; otherwise something should move.)
            if since > 8 and step.action != "exit" and self._effect_started(obs, step.trigger):
                self._next_step(obs.tic)
            elif since > (2 * TICRATE if step.action == "exit" else 25):
                self._press_tries += 1
                if self._press_tries < PRESS_TRIES:
                    self.log.add(obs.tic, f"line {step.trigger.line} didn't do anything, "
                                          f"trying again closer")
                    self._action_tic = None
                else:
                    self.log.add(obs.tic, f"line {step.trigger.line} didn't work after "
                                          f"{PRESS_TRIES} tries; carrying on")
                    self._next_step(obs.tic)
        elif obs.tic - self._progress_tic > 3 * TICRATE:
            return self._fail(obs, f"could not face line {step.trigger.line}")
        return controls
