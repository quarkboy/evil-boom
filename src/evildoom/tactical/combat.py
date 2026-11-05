"""Fighting: which monsters Evil knows are there, which to shoot at and with what, getting out
of the way of what's flying at her, and backing off from what would claw her.

`Combat.decide` runs every tic and says how much of the controls the fight needs:
- nothing (no monster she knows is about): the explorer has them;
- the aim (a monster in sight that isn't pressing): she carries on along her route, turning to
  shoot as she goes, as a player runs and guns;
- everything (something's flying at her, something's close, there are too many, she's low):
  she stands and fights, strafing and dodging, or backs off; the route waits.

What she knows of monsters follows what she can see: a monster is noticed when it's in line
of sight in front of her (or near, or she's being hurt), and then remembered where last seen
(the hunt for what's left, on maps that open only once a boss is dead).
"""
from __future__ import annotations

import math
from dataclasses import dataclass

import evildoom_native

from evildoom.motor.follower import TICRATE, Controls, Observation, Thing
from evildoom.nav.reach import MAX_STEP, PLAYER_HEIGHT, PLAYER_RADIUS, Navigator
from evildoom.tactical.bestiary import MISSILES, MONSTERS, WEAPONS

FOV = 55.0               # half the view, degrees: monsters in line of sight inside it are seen...
NEAR = 384.0             # ...and any this near, in any direction (heard)
HURT_TICS = 2 * TICRATE  # being hurt: everything in line of sight is noticed for this long
FORGET_TICS = 20 * TICRATE  # out of sight this long: not "about" any more (still remembered)
ENGAGE_RANGE = 2048.0    # no shooting at anything further
AIM_TURN = 18.0          # degrees a tic, turning to aim
TURN_ACCEL = 5.0         # change in aim turn rate a tic
DODGE_TICS = 30          # a missile arriving sooner than this is dodged
DODGE_MARGIN = 10.0      # ...if it would pass nearer than the two radii and this
CLOSE = 160.0            # a clawing monster nearer than this: back off from it
STAND_THREAT = 6.0       # this much threat in sight: stand and fight rather than walk on
OUTMATCHED = 15.0        # ...unless killing what's in sight would take longer than this (s):
#                          then get past it (on the way to something better to fight with)
SHORT_OF_AMMO = 15       # fewer shots than this for the guns held: supplies come first
LOW_HEALTH = 35.0        # ...or below this health, anything in sight
STRAFE_TICS = (18, 40)   # a strafe one way lasts between these
SWITCH_WAIT = 40         # a weapon switch asked for: give it this long before asking again


@dataclass
class Seen:
    """A monster she knows of."""
    id: int
    name: str
    x: float
    y: float
    z: float
    tic: int          # last seen
    first: int        # first seen

    @property
    def pos(self) -> tuple[float, float]:
        return (self.x, self.y)


@dataclass
class Decision:
    takeover: bool          # the fight needs the movement too
    aim: float | None       # angle to turn to (degrees), if fighting at all
    fire: bool = False
    weapon: int = 0         # slot to switch to (0: keep)
    move: tuple[float, float] | None = None  # world direction to run in (unit), if moving
    why: str = ""


class Combat:
    def __init__(self, nav: Navigator):
        self.nav = nav
        self.los = evildoom_native.LineOfSight(nav.map)
        self.known: dict[int, Seen] = {}       # every monster seen, by id
        self.alive: dict[int, Thing] = {}      # every monster about now (as the engine has it)
        self.visible: list[Thing] = []         # monsters she can see now
        self.target: int | None = None
        self.kills = 0
        self.log: list[str] = []
        self._health: float | None = None
        self._hurt_tic = -10**9
        self._switch_tic = -10**9
        self._strafe = 1.0
        self._strafe_until = 0
        self._rate = 0.0
        self._mode = ""
        self._dodging: int | None = None  # missile being dodged

    def restart(self) -> None:
        """A new life: the level from the start (what's been seen is remembered: it'll be
        there again)."""
        self.alive.clear()
        self.visible = []
        self.target = None
        self._health = None
        self._hurt_tic = -10**9
        self._mode = ""

    # --- what's about -------------------------------------------------------------------

    def observe(self, obs: Observation) -> None:
        if obs.health is not None:
            if self._health is not None and obs.health < self._health:
                self._hurt_tic = obs.tic
            self._health = obs.health
        things = obs.things or ()
        monsters = {t.id: t for t in things if t.name in MONSTERS and t.name != "CommanderKeen"}
        for i in self.alive:
            if i not in monsters and i in self.known:
                self.kills += 1
                self.log.append(f"tic {obs.tic}: {self.known[i].name} {i} dead")
        self.alive = monsters
        heights = self._heights(obs)
        me = (obs.x, obs.y, self._z(obs), PLAYER_HEIGHT)
        hurt = obs.tic - self._hurt_tic < HURT_TICS
        near = [t for t in monsters.values()
                if math.dist((t.x, t.y), (obs.x, obs.y)) < ENGAGE_RANGE]
        clear = self.los.visible(me, [(t.x, t.y, t.z, MONSTERS[t.name].height) for t in near],
                                 heights) if near else []
        self.visible = []
        for t, ok in zip(near, clear):
            if not ok:
                continue
            d = math.dist((t.x, t.y), (obs.x, obs.y))
            off = abs(_norm(math.degrees(math.atan2(t.y - obs.y, t.x - obs.x)) - obs.angle))
            if off < FOV or d < NEAR or hurt or t.id in self.known and obs.tic - self.known[t.id].tic < TICRATE:
                self.visible.append(t)
                s = self.known.get(t.id)
                if s is None:
                    self.known[t.id] = Seen(t.id, t.name, t.x, t.y, t.z, obs.tic, obs.tic)
                    self.log.append(f"tic {obs.tic}: saw {t.name} {t.id} at ({t.x:.0f}, {t.y:.0f})")
                else:
                    s.x, s.y, s.z, s.tic = t.x, t.y, t.z, obs.tic

    def remaining(self) -> list[Seen]:
        """Monsters seen that are still about (to hunt down)."""
        return [s for i, s in self.known.items() if i in self.alive]

    def threats(self, obs: Observation) -> float:
        return sum(MONSTERS[t.name].threat for t in self.visible)

    def firepower(self, obs: Observation) -> float:
        """Damage a second she can do with the best gun she has ammo for (the fist's, else)."""
        held = obs.weapons or frozenset({1, 2})
        ammo = obs.ammo or {}
        return max(w.dps for slot, w in WEAPONS.items()
                   if slot in held and (not w.ammo_per_shot or ammo.get(slot, 0) >= w.ammo_per_shot))

    def outmatched(self, obs: Observation) -> bool:
        """Would killing what's in sight take too long with what she has (a room of barons
        with the pistol: MAP08 at pistol start)?"""
        pos = (obs.x, obs.y)
        hp = sum(MONSTERS[t.name].health for t in self.visible if math.dist((t.x, t.y), pos) < 1200)
        return hp / self.firepower(obs) > OUTMATCHED

    def short_of_ammo(self, obs: Observation) -> bool:
        """Next to nothing left to shoot with (the fist doesn't count)."""
        held = obs.weapons or frozenset({1, 2})
        ammo = obs.ammo or {}
        shots = sum(ammo.get(slot, 0) // w.ammo_per_shot for slot, w in WEAPONS.items()
                    if slot in held and w.ammo_per_shot and slot != 4)  # (4 shares 2's bullets)
        return shots < SHORT_OF_AMMO

    # --- deciding -----------------------------------------------------------------------

    def decide(self, obs: Observation, busy: bool = False) -> Decision | None:
        """What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
        something that wants the controls (a run-up, a jump): only a missile about to hit
        takes them away."""
        pos = (obs.x, obs.y)
        missile = self._incoming(obs)
        target = self._pick_target(obs)
        if target is None and missile is None:
            if self._mode:
                self.log.append(f"tic {obs.tic}: fight over ({self.kills} kills)")
            self._mode = ""
            self.target = None
            return None
        aim = fire = None
        weapon = 0
        if target is not None:
            aim, fire, weapon = self._aim_and_fire(obs, target)
        health = obs.health if obs.health is not None else 100.0
        threat = self.threats(obs)
        outmatched = self.outmatched(obs)
        close = [t for t in self.visible if MONSTERS[t.name].attack in ("melee", "charge", "fire")
                 or MONSTERS[t.name].melee]
        close = [t for t in close if math.dist((t.x, t.y), pos) < CLOSE + MONSTERS[t.name].radius]
        move = None
        why = ""
        if missile is not None:
            move = self._dodge(obs, missile)
            why = f"dodging {missile.name}"
        elif busy:
            why = "shooting on the way"
        elif close:
            nearest = min(close, key=lambda t: math.dist((t.x, t.y), pos))
            move = self._away_from(obs, (nearest.x, nearest.y))
            why = f"backing off from {nearest.name}"
        elif outmatched:
            why = "getting past"  # (shooting on the way, the route going on)
        elif threat >= STAND_THREAT or health < LOW_HEALTH or self._blocking(obs, target):
            move = self._strafe_dir(obs, aim)
            why = "standing and fighting"
        else:
            why = "shooting on the way"
        takeover = not busy and why not in ("shooting on the way", "getting past") or missile is not None
        if why != self._mode:
            self.log.append(f"tic {obs.tic}: {why}"
                            + (f" (target {target.name} {target.id}, {threat:.0f} threat)" if target else ""))
            self._mode = why
        return Decision(takeover, aim, bool(fire), weapon, move, why)

    def controls(self, obs: Observation, d: Decision, base: Controls | None = None) -> Controls:
        """The controls for a decision: over the route's (`base`: only the aim and the trigger
        are the fight's), or all of them."""
        turn = self._turn(obs, d.aim)
        if d.takeover or base is None:
            c = Controls(turn=turn)
            if d.move is not None:
                facing = obs.angle - turn  # (thrust goes the way faced after this tic's turn)
                rel = math.radians(_norm(math.degrees(math.atan2(d.move[1], d.move[0])) - facing))
                c.forward, c.strafe = 50.0 * math.cos(rel), -50.0 * math.sin(rel)
        else:
            c = Controls(forward=base.forward, strafe=base.strafe, turn=turn, use=base.use,
                         run=base.run, weapon=base.weapon)
            if d.aim is None:
                c.turn = base.turn
            else:
                # (the route's way in the world, whichever way she's turned to aim)
                way = obs.angle - base.turn + math.degrees(math.atan2(-base.strafe, base.forward))
                rel = math.radians(_norm(way - (obs.angle - turn)))
                mag = math.hypot(base.forward, base.strafe)
                c.forward, c.strafe = mag * math.cos(rel), -mag * math.sin(rel)
        c.attack = d.fire or (base is not None and base.attack and not d.takeover)
        if d.weapon:
            c.weapon = d.weapon
        return c

    # --- targets, weapons, aim ----------------------------------------------------------

    def _pick_target(self, obs: Observation) -> Thing | None:
        pos = (obs.x, obs.y)
        best, score = None, -math.inf
        for t in self.visible:
            info = MONSTERS[t.name]
            d = max(64.0, math.dist((t.x, t.y), pos))
            s = (info.threat + 1) / d * (1.6 if t.id == self.target else 1.0)
            off = abs(_norm(_heading(pos, (t.x, t.y)) - obs.angle))
            s *= 1.0 - min(off, 150) / 300  # (rather what's in front than what's behind)
            if s > score:
                best, score = t, s
        self.target = best.id if best is not None else None
        return best

    def _aim_and_fire(self, obs: Observation, t: Thing) -> tuple[float, bool, int]:
        pos = (obs.x, obs.y)
        d = math.dist((t.x, t.y), pos)
        slot = self._weapon_for(obs, t, d)
        weapon = 0
        held = obs.weapon
        if slot != held and obs.tic - self._switch_tic > SWITCH_WAIT:
            weapon = slot
            self._switch_tic = obs.tic
        w = WEAPONS.get(held or 2, WEAPONS[2])
        # lead a moving target with a missile weapon
        ax, ay = t.x, t.y
        if not w.hitscan and w.speed:
            tof = d / w.speed
            ax, ay = t.x + t.vx * tof, t.y + t.vy * tof
        aim = _heading(pos, (ax, ay))
        info = MONSTERS[t.name]
        tolerance = max(1.5, math.degrees(math.atan2(info.radius * 0.7, max(d, 1.0))))
        on = abs(_norm(aim - obs.angle)) < tolerance
        ok = d < min(ENGAGE_RANGE, w.reach) and (held != 5 or self._rocket_safe(obs, t))
        return aim, on and ok, weapon

    def _rocket_safe(self, obs: Observation, t: Thing) -> bool:
        """Is a rocket at t sure not to blow up by her: t well clear, nothing else near her or
        in the way (a monster stepping in, a barrel: E1M9's died at 88 health, nothing in
        sight)?"""
        pos = (obs.x, obs.y)
        clear = WEAPONS[5].splash + 48
        if math.dist((t.x, t.y), pos) < clear + MONSTERS[t.name].radius:
            return False
        others = [(o.x, o.y, MONSTERS[o.name].radius) for o in self.visible if o.id != t.id]
        others += [(b[0], b[1], 10.0) for b in obs.barrels or ()]
        for x, y, r in others:
            if math.dist((x, y), pos) < clear + r:
                return False
            if _segment_point_distance(pos, (t.x, t.y), (x, y)) < r + 12                     and math.dist((x, y), pos) < math.dist((t.x, t.y), pos):
                return False
        return True

    def _weapon_for(self, obs: Observation, t: Thing, d: float) -> int:
        held = obs.weapons or frozenset({1, 2})
        ammo = obs.ammo or {}
        info = MONSTERS[t.name]
        crowd = len(self.visible)
        best, value = 2, -math.inf
        for slot, w in WEAPONS.items():
            if slot not in held or (w.ammo_per_shot and ammo.get(slot, 0) < w.ammo_per_shot):
                continue
            v = w.dps
            lo, hi = w.best_range
            if d < lo:
                v *= 0.05
            elif d > hi:
                v *= 0.35
            if slot == 3 and d > 300:
                v *= 0.6
            if slot in (5, 6, 7) and info.health <= 100 and crowd < 3:
                v *= 0.25  # (keep the big ammo for what needs it)
            if slot == 7 and not (info.boss or crowd >= 4):
                v *= 0.2
            if slot == 1:
                v *= 1.0 if d < 64 and info.attack == "hitscan" else 0.02
            if slot == 5 and not self._rocket_safe(obs, t):
                v *= 0.02
            if slot == obs.weapon:
                v *= 1.3  # (switching takes a moment)
            if v > value:
                best, value = slot, v
        return best

    def _turn(self, obs: Observation, aim: float | None) -> float:
        """This tic's turn (Controls.turn: positive is clockwise) towards `aim`."""
        if aim is None:
            self._rate = 0.0
            return 0.0
        err = _norm(aim - obs.angle)
        want = math.copysign(min(AIM_TURN, math.sqrt(2 * TURN_ACCEL * abs(err)), abs(err)), err)
        self._rate = max(self._rate - TURN_ACCEL, min(self._rate + TURN_ACCEL, want))
        if abs(err) < 1.0:
            self._rate = err
        return -self._rate

    # --- moving -------------------------------------------------------------------------

    def _incoming(self, obs: Observation) -> Thing | None:
        """The missile that'll hit her soonest, if one will."""
        pos = (obs.x, obs.y)
        z = self._z(obs)
        best, soonest = None, math.inf
        for t in obs.things or ():
            m = MISSILES.get(t.name)
            if m is None or (t.name == "LostSoul" and math.hypot(t.vx, t.vy) < 8):
                continue
            v2 = t.vx * t.vx + t.vy * t.vy
            if v2 < 1.0:
                continue
            rx, ry = pos[0] - t.x, pos[1] - t.y
            when = (rx * t.vx + ry * t.vy) / v2
            if not 0 < when < DODGE_TICS:
                continue
            cx, cy = t.x + t.vx * when - pos[0], t.y + t.vy * when - pos[1]
            hz = t.z + t.vz * when
            if math.hypot(cx, cy) > PLAYER_RADIUS + m.radius + DODGE_MARGIN + (64 if m.splash else 0):
                continue
            if not z - 16 < hz < z + PLAYER_HEIGHT + 16:
                continue
            if when < soonest:
                best, soonest = t, when
        return best

    def _dodge(self, obs: Observation, m: Thing) -> tuple[float, float] | None:
        """Sideways out of a missile's way: to the side she's already on of its line, else the
        other, wherever the floor's safe."""
        pos = (obs.x, obs.y)
        speed = math.hypot(m.vx, m.vy)
        ux, uy = m.vx / speed, m.vy / speed
        side = (pos[0] - m.x) * -uy + (pos[1] - m.y) * ux
        first = (-uy, ux) if side >= 0 else (uy, -ux)
        if self._dodging != m.id:
            self._dodging = m.id
            self.log.append(f"tic {obs.tic}: dodging {m.name} {m.id}")
        # (or, failing both, aslant away from where it came from)
        for d in (first, (-first[0], -first[1]), (first[0] * 0.7 + ux * 0.7, first[1] * 0.7 + uy * 0.7)):
            if self._safe(obs, d, 64.0):
                return d
        return None

    def _away_from(self, obs: Observation, what: tuple[float, float]) -> tuple[float, float] | None:
        """Back off from `what`: straight away if the floor's safe that way, else as near that
        as is."""
        away = math.atan2(obs.y - what[1], obs.x - what[0])
        for off in (0, 30, -30, 60, -60, 90, -90):
            a = away + math.radians(off)
            d = (math.cos(a), math.sin(a))
            if self._safe(obs, d, 64.0):
                return d
        return None

    def _strafe_dir(self, obs: Observation, aim: float | None) -> tuple[float, float] | None:
        """Side to side across the line of fire, a second or so each way, where it's safe."""
        if obs.tic >= self._strafe_until:
            self._strafe = -self._strafe
            lo, hi = STRAFE_TICS
            self._strafe_until = obs.tic + lo + (obs.tic * 7919) % (hi - lo)
        if aim is None:
            return None
        a = math.radians(aim + 90 * self._strafe)
        d = (math.cos(a), math.sin(a))
        if self._safe(obs, d, 48.0):
            return d
        self._strafe = -self._strafe
        d = (-d[0], -d[1])
        return d if self._safe(obs, d, 48.0) else None

    def _blocking(self, obs: Observation, t: Thing | None) -> bool:
        """A monster near and in front: walking on would walk into it."""
        if t is None:
            return False
        d = math.dist((t.x, t.y), (obs.x, obs.y))
        return d < 320 and abs(_norm(_heading((obs.x, obs.y), (t.x, t.y)) - obs.angle)) < 60

    def _safe(self, obs: Observation, d: tuple[float, float], length: float) -> bool:
        """Is it all right to go `length` that way: floor all the way, no drop or step up she
        couldn't come back from, nothing that hurts, room to stand?"""
        mesh = self.nav.mesh
        here = mesh.poly_at(obs.x, obs.y)
        if here is None:
            return False
        live = obs.heights
        floor = live.get(here.sector, (0.0, 0.0))[0]
        n = max(2, int(length // 16))
        for i in range(1, n + 1):
            q = (obs.x + d[0] * length * i / n, obs.y + d[1] * length * i / n)
            poly = mesh.poly_at(*q)
            if poly is None:
                return False
            f, c = live.get(poly.sector, (0.0, 0.0))
            if abs(f - floor) > MAX_STEP or c - f < PLAYER_HEIGHT or self.nav.damage[poly.sector]:
                return False
            floor = f
        return True

    # --- helpers ------------------------------------------------------------------------

    def _heights(self, obs: Observation) -> list[tuple[float, float]]:
        hs = [(float(s.floor), float(s.ceiling)) for s in self.nav.map.sectors]
        for s, h in obs.heights.items():
            if s < len(hs):
                hs[s] = (float(h[0]), float(h[1]))
        return hs

    def _z(self, obs: Observation) -> float:
        if obs.z is not None:
            return obs.z
        poly = self.nav.mesh.poly_at(obs.x, obs.y)
        return obs.heights.get(poly.sector, (0.0, 0.0))[0] if poly is not None else 0.0


def _heading(a: tuple[float, float], b: tuple[float, float]) -> float:
    return math.degrees(math.atan2(b[1] - a[1], b[0] - a[0]))


def _norm(a: float) -> float:
    return (a + 180.0) % 360.0 - 180.0


def _segment_point_distance(a: tuple[float, float], b: tuple[float, float], p: tuple[float, float]) -> float:
    dx, dy = b[0] - a[0], b[1] - a[1]
    n = dx * dx + dy * dy
    u = 0.0 if n == 0 else max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / n))
    return math.dist(p, (a[0] + dx * u, a[1] + dy * u))
