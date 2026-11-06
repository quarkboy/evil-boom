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
from evildoom.tactical.bestiary import MISSILES, MONSTERS, WEAPONS, Weapon, weapon_in

FOV = 55.0               # half the view, degrees: monsters in line of sight inside it are seen...
NEAR = 384.0             # ...and any this near, in any direction (heard)
HURT_TICS = 2 * TICRATE  # being hurt: everything in line of sight is noticed for this long
FORGET_TICS = 20 * TICRATE  # out of sight this long: not "about" any more (still remembered)
ENGAGE_RANGE = 2048.0    # no shooting at anything further
AIM_TURN = 18.0          # degrees a tic, turning to aim
TURN_ACCEL = 5.0         # change in aim turn rate a tic
DODGE_TICS = 35          # a missile arriving sooner than this is dodged
DODGE_MARGIN = 12.0      # ...if it would pass nearer than the two radii and this (her going on
#                          as she's going); then it's dodged till it's gone by
DODGE_WAYS = 16          # ways tried, getting out of the way
THRUST = 50 * 2048 / 65536  # a running player's push a tic (units/tic, a tic)
FRICTION = 0xE800 / 0x10000  # what's left of her speed after a tic
CLOSE = 160.0            # a clawing monster nearer than this: back off from it
STAND_THREAT = 6.0       # this much threat in sight: stand and fight rather than walk on
OUTMATCHED = 15.0        # ...unless killing what's in sight would take longer than this (s):
#                          then get past it (on the way to something better to fight with)
SHORT_OF_AMMO = 15       # fewer shots than this for the guns held: supplies come first
LOW_HEALTH = 35.0        # ...or below this health, anything in sight
BARREL_CLEAR = 160.0     # with something shooting in sight, keep this far from barrels
BARREL_NEAR = 96.0       # a barrel this near a monster (and well clear of her) is shot instead
BARREL_TICS = 2 * TICRATE    # getting clear of barrels: at most this long at a time...
BARREL_REST = 3 * TICRATE    # ...then not again for this long (E1M7: pressed against a wall for ever)
STRAFE_TICS = (18, 40)   # a strafe one way lasts between these
SHOT_HEIGHT = 36.0       # shots leave her this far above her feet (half her height, and 8)...
AUTOAIM = 100 / 160      # ...and find what they're aimed at only this steep (up or down): any
#                          steeper, or with a ledge's lip in the way, they go out level
ROCKET_WARY = 3 * TICRATE  # a biter seen this lately near her (round a corner, maybe) rules out
#                            rockets: it'd be in the blast before the rocket got anywhere
FAR_HITS = 0.2           # something that shoots back from anywhere (zombies, chaingunners): shot
#                          at past a gun's reach while this much of a shot lands
SWITCH_WAIT = 40         # a weapon switch asked for: give it this long before asking again
SWITCH_COST = 0.8        # seconds a weapon switch costs (down and up again)
# what using up ammo costs, in seconds of fighting, a unit (rockets are worth keeping for what
# needs them), and below how much of it the cost goes up
AMMO_COST = {2: 0.01, 3: 0.05, 4: 0.01, 5: 0.3, 6: 0.015, 7: 0.015}
AMMO_LOW = {2: 20, 3: 8, 4: 20, 5: 4, 6: 40, 7: 80}


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
        self._held_fire = ""
        self._heights_for: Observation | None = None
        self._heights_now: list[tuple[float, float]] = []
        self._hit_cache: dict[tuple[int, float], bool] = {}
        self._dodging: set[int] = set()     # missiles being dodged (till they've gone by)
        self._dodge_way: tuple[float, float] | None = None
        self._last_pos: tuple[float, float] | None = None
        self._velocity = (0.0, 0.0)
        self._barrel_way: tuple[float, float] | None = None  # getting clear of barrels this way
        self._barrel_since = 0
        self._barrel_rest_until = -10**9

    def restart(self) -> None:
        """A new life: the level from the start (what's been seen is remembered: it'll be
        there again)."""
        self.alive.clear()
        self.visible = []
        self.target = None
        self._health = None
        self._hurt_tic = -10**9
        self._mode = ""
        self._dodging.clear()
        self._last_pos = None
        self._velocity = (0.0, 0.0)

    # --- what's about -------------------------------------------------------------------

    def observe(self, obs: Observation) -> None:
        pos = (obs.x, obs.y)
        if obs.velocity is not None:
            self._velocity = obs.velocity
        elif self._last_pos is not None:
            self._velocity = (pos[0] - self._last_pos[0], pos[1] - self._last_pos[1])
        self._last_pos = pos
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
        guns = [weapon_in(slot, obs.super_shotgun) for slot in WEAPONS if slot in held]
        return max(w.dps for w in guns if not w.ammo_per_shot or ammo.get(w.slot, 0) >= w.ammo_per_shot)

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
                    if slot in held and w.ammo_per_shot and slot not in (4, 7))  # (2's bullets, 6's cells)
        return shots < SHORT_OF_AMMO

    # --- deciding -----------------------------------------------------------------------

    def decide(self, obs: Observation, busy: bool = False) -> Decision | None:
        """What the fight needs this tic (None: nothing). `busy`: the route is in the middle of
        something that wants the controls (a run-up, a jump): only a missile about to hit
        takes them away."""
        pos = (obs.x, obs.y)
        missiles = self._incoming(obs)
        missile = missiles[0] if missiles else None
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
            shot = self._barrel_shot(obs)
            if shot is not None and obs.weapon in (2, 3, 4):
                # (MAP23: the barrels round the mancubus do what the pistol can't)
                aim = _heading(pos, shot)
                d = math.dist(shot, pos)
                fire = abs(_norm(aim - obs.angle)) < max(0.75, math.degrees(math.atan2(8, d))) \
                    and d < weapon_in(obs.weapon, obs.super_shotgun).reach \
                    and self._shot_clear(obs, shot, self._floor_at(obs, shot), 32.0)
        health = obs.health if obs.health is not None else 100.0
        threat = self.threats(obs)
        outmatched = self.outmatched(obs)
        close = [t for t in self.visible if MONSTERS[t.name].attack in ("melee", "charge", "fire")
                 or MONSTERS[t.name].melee]
        close = [t for t in close if math.dist((t.x, t.y), pos) < CLOSE + MONSTERS[t.name].radius]
        move = None
        why = ""
        shooting = any(MONSTERS[t.name].attack in ("hitscan", "missile") for t in self.visible)
        barrel = min(obs.barrels or (), key=lambda b: math.dist(b, pos), default=None)
        if missile is not None:
            move = self._dodge(obs, missiles)
            why = f"dodging {missile.name}"
        elif not busy and shooting and barrel is not None and math.dist(barrel, pos) < BARREL_CLEAR + 64                 and (away := self._from_barrels(obs, barrel)) is not None:
            # (MAP23: a mancubus's fireball set off the barrels she stood by, at full health;
            # once clear, she can set them off on it)
            move = away
            why = "away from a barrel"
        elif busy:
            why = "shooting on the way"
        elif target is not None and not self.can_hit(obs, target) \
                and len(self.visible) == 1 and math.dist((target.x, target.y), pos) < 512 \
                and (way := self._shot_way(obs, target)) is not None:
            move = way
            why = f"stepping for a shot at {target.name}"
        elif close:
            nearest = min(close, key=lambda t: math.dist((t.x, t.y), pos))
            move = self._away_from(obs, (nearest.x, nearest.y))
            why = f"backing off from {nearest.name}"
        elif outmatched:
            why = "getting past"  # (shooting on the way, the route going on)
        elif threat >= STAND_THREAT or health < LOW_HEALTH or self._blocking(obs, target):
            move = self._strafe_dir(obs, aim)
            why = "standing and fighting"
            gun = weapon_in(obs.weapon if obs.weapon in WEAPONS else 2, obs.super_shotgun)
            if target is not None and not self.in_reach(gun, target, math.dist((target.x, target.y), pos)):
                move = self._closing_in(obs, target, move)
                why = f"closing in on {target.name}"
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
            if not self.can_hit(obs, t):
                s *= 0.3  # (rather what a shot gets to)
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
        w = weapon_in(held if held in WEAPONS else 2, obs.super_shotgun)
        # lead a moving target with a missile weapon
        ax, ay = t.x, t.y
        if not w.hitscan and w.speed:
            tof = d / w.speed
            ax, ay = t.x + t.vx * tof, t.y + t.vy * tof
        aim = _heading(pos, (ax, ay))
        info = MONSTERS[t.name]
        tolerance = max(1.5, math.degrees(math.atan2(info.radius * 0.7, max(d, 1.0))))
        on = abs(_norm(aim - obs.angle)) < tolerance
        held_fire = ("out of reach" if not self.in_reach(w, t, d)
                     else "rocket not safe" if held == 5 and not self._rocket_safe(obs, t)
                     else "no shot (too steep, or something in the way)" if not self.can_hit(obs, t)
                     else "")
        if held_fire != self._held_fire:
            if held_fire:
                self.log.append(f"tic {obs.tic}: holding fire at {t.name} {t.id}, {d:.0f} off: {held_fire}")
            self._held_fire = held_fire
        return aim, on and not held_fire, weapon

    @staticmethod
    def in_reach(w: Weapon, t: Thing, d: float, pos: tuple[float, float] | None = None) -> bool:
        """Close enough to shoot at with w: inside its reach, or (at something that hurts her
        from anywhere, so not shooting back is worse than the ammo) as far as enough of a
        shot still lands."""
        info = MONSTERS[t.name]
        if d >= ENGAGE_RANGE:
            return False
        if d < w.reach + (info.radius if w.slot == 1 else 0):
            return True
        return info.attack == "hitscan" and w.slot != 1 and w.hits(info.radius, d) >= FAR_HITS

    def can_hit(self, obs: Observation, t: Thing, height: float | None = None) -> bool:
        """Would a shot at t get to it, as Doom aims one: from her gun, at any part of t the
        aim reaches (not too steep: right under a ledge it isn't) and nothing's in the way of
        (a ledge's lip, a low wall); else out level, past it unless it's level with her
        (shotgun blasts into the lip of the ledge the imp stands on)."""
        h = MONSTERS[t.name].height if height is None else height
        self._heights(obs)  # (a new tic: what was worked out for the last is cleared)
        key = (t.id, h)
        if key not in self._hit_cache:
            self._hit_cache[key] = self._shot_clear(obs, (t.x, t.y), t.z, h)
        return self._hit_cache[key]

    def _shot_clear(self, obs: Observation, at: tuple[float, float], z: float, h: float) -> bool:
        gun = self._z(obs) + SHOT_HEIGHT
        d = max(1.0, math.dist(at, (obs.x, obs.y)))
        lo, hi = max(z, gun - AUTOAIM * d), min(z + h, gun + AUTOAIM * d)
        if hi <= lo:
            if not z < gun < z + h:
                return False
            lo, hi = gun - 1, gun + 1  # (out level: it only hits what's level with it)
        me = (obs.x, obs.y, gun - 0.75 * PLAYER_HEIGHT, PLAYER_HEIGHT)  # (the eye's at 0.75)
        return self.los.visible(me, [(at[0], at[1], lo, hi - lo)], self._heights(obs))[0]

    def _shot_way(self, obs: Observation, t: Thing) -> tuple[float, float] | None:
        """Where to step for a shot at t she can see but can't hit: back from it when it's
        above her (the slope too steep, or a lip in the way that's less in the way from
        further back), towards it when it's below; None if neither way is safe."""
        away = self._away_from(obs, (t.x, t.y))
        if t.z <= self._z(obs) and away is not None:
            return (-away[0], -away[1]) if self._safe(obs, (-away[0], -away[1]), 64.0) else None
        return away

    def _barrel_shot(self, obs: Observation) -> tuple[float, float] | None:
        """A barrel she can see right by a monster, well clear of her: shoot that."""
        pos = (obs.x, obs.y)
        barrels = [b for b in obs.barrels or () if math.dist(b, pos) > BARREL_CLEAR + 64
                   and any(math.dist(b, (t.x, t.y)) < BARREL_NEAR + MONSTERS[t.name].radius
                           for t in self.visible)]
        if not barrels:
            return None
        heights = self._heights(obs)
        me = (obs.x, obs.y, self._z(obs), PLAYER_HEIGHT)
        floor = self._floor_at
        seen = self.los.visible(me, [(b[0], b[1], floor(obs, b), 32.0) for b in barrels], heights)
        open_ = [b for b, v in zip(barrels, seen) if v]
        return min(open_, key=lambda b: math.dist(b, pos)) if open_ else None

    def _floor_at(self, obs: Observation, p: tuple[float, float]) -> float:
        poly = self.nav.mesh.poly_at(*p)
        return obs.heights.get(poly.sector, (0.0, 0.0))[0] if poly is not None else 0.0

    def _from_barrels(self, obs: Observation, nearest: tuple[float, float]) -> tuple[float, float] | None:
        """The way to go to get clear of the barrels by her: the way she's going already if
        it's still safe and clearing them (no dithering), else a new one; None once she's
        clear (or can't get so)."""
        pos = (obs.x, obs.y)
        if obs.tic < self._barrel_rest_until:
            return None
        if self._barrel_way is None:
            self._barrel_since = obs.tic
        elif obs.tic - self._barrel_since > BARREL_TICS:
            self._barrel_way = None
            self._barrel_rest_until = obs.tic + BARREL_REST
            return None
        if math.dist(nearest, pos) >= BARREL_CLEAR + (64 if self._barrel_way is not None else 0):
            self._barrel_way = None
            return None
        way = self._barrel_way
        if way is not None and self._safe(obs, way, 48.0) and self._clear_of_barrels(obs, way):
            return way
        way = self._away_from(obs, nearest)
        self._barrel_way = way if way is not None and self._clear_of_barrels(obs, way) else None
        return self._barrel_way

    def _clear_of_barrels(self, obs: Observation, d: tuple[float, float]) -> bool:
        """Does going that way (a second's run) leave her further from every barrel near her?"""
        pos = (obs.x, obs.y)
        ahead = (pos[0] + d[0] * 96, pos[1] + d[1] * 96)
        near = [b for b in obs.barrels or () if math.dist(b, pos) < BARREL_CLEAR]
        return all(math.dist(b, ahead) > math.dist(b, pos) for b in near)

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
        # (E1M9: dead at full health, a demon 83 away: it came round a corner into the way)
        others += [(m.x, m.y, MONSTERS[m.name].radius + 128) for m in self.remaining()
                   if m.id != t.id and obs.tic - m.tic < ROCKET_WARY
                   and MONSTERS[m.name].attack in ("melee", "charge")]
        for x, y, r in others:
            if math.dist((x, y), pos) < clear + r:
                return False
            if _segment_point_distance(pos, (t.x, t.y), (x, y)) < r + 12 \
                    and math.dist((x, y), pos) < math.dist((t.x, t.y), pos):
                return False
        return True

    def _weapon_for(self, obs: Observation, t: Thing, d: float) -> int:
        """The gun that'd see t off soonest, counting the switch to it and what its ammo is
        worth (rockets aren't for zombiemen, unless that's all she has)."""
        held = obs.weapons or frozenset({1, 2})
        ammo = obs.ammo or {}
        info = MONSTERS[t.name]
        crowd = len(self.visible)
        best, cost = obs.weapon or 2, math.inf
        for slot in WEAPONS:
            if slot not in held:
                continue
            w = weapon_in(slot, obs.super_shotgun)
            have = ammo.get(slot, 0)
            if w.ammo_per_shot and have < w.ammo_per_shot or slot == 5 and not self._rocket_safe(obs, t):
                continue
            c = self._time_to_kill(w, t, d)
            if w.ammo_per_shot:
                shots = max(1.0, info.health / max(1.0, w.damage * w.hits(info.radius, d)))
                c += shots * w.ammo_per_shot * AMMO_COST[slot] * max(1.0, AMMO_LOW[slot] / have)
            if slot == 7 and (info.boss or crowd >= 4):
                c *= 0.3
            if slot != obs.weapon:
                c += SWITCH_COST
            if c < cost:
                best, cost = slot, c
        return best

    def _time_to_kill(self, w: Weapon, t: Thing, d: float) -> float:
        """Seconds of shooting it'd take (a lot, out of the gun's reach: anything else first)."""
        info = MONSTERS[t.name]
        if not self.in_reach(w, t, d):
            return 1e4 + d
        per_shot = w.damage * w.hits(info.radius, d)
        return max(1.0, info.health / max(per_shot, 0.1)) * w.cycle / TICRATE

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

    def _incoming(self, obs: Observation) -> list[Thing]:
        """The missiles that'd hit her, soonest first: any that would if she went on as she's
        going, and any she's getting out of the way of that haven't gone by yet."""
        pos = (obs.x, obs.y)
        z = self._z(obs)
        vel = self._velocity
        out = []
        for t in obs.things or ():
            m = MISSILES.get(t.name)
            if m is None or (t.name == "LostSoul" and math.hypot(t.vx, t.vy) < 8):
                continue
            # (relative to her, as she's going)
            rvx, rvy = t.vx - vel[0], t.vy - vel[1]
            v2 = rvx * rvx + rvy * rvy
            if v2 < 1.0:
                continue
            rx, ry = pos[0] - t.x, pos[1] - t.y
            when = (rx * rvx + ry * rvy) / v2
            if not 0 < when < DODGE_TICS:
                continue
            miss = math.hypot(t.x + rvx * when - pos[0], t.y + rvy * when - pos[1])
            hz = t.z + t.vz * when
            if not z - 24 < hz < z + PLAYER_HEIGHT + 24:
                continue
            if t.id not in self._dodging and miss > self._clearance(m):
                continue
            if t.id not in self._dodging:
                self._dodging.add(t.id)
                self.log.append(f"tic {obs.tic}: dodging {t.name} {t.id} "
                                f"(due in {when:.0f} tics, {miss:.0f} off)")
            out.append((when, t))
        here = {t.id for _, t in out}
        self._dodging &= here
        if not here:
            self._dodge_way = None
        return [t for _, t in sorted(out, key=lambda w: w[0])]

    @staticmethod
    def _clearance(m) -> float:
        """How far a missile's middle has to pass from hers to miss (and its blast)."""
        return PLAYER_RADIUS + m.radius + DODGE_MARGIN + (64 if m.splash else 0)

    def _dodge(self, obs: Observation, missiles: list[Thing]) -> tuple[float, float] | None:
        """The way to run that keeps her furthest from all of them as they go by (her speed
        now carrying on, a running player's push and Doom's friction), where the floor's safe;
        the way she's going already when it's as good (no dithering)."""
        pos = (obs.x, obs.y)
        best, score = None, -math.inf
        ways = [(math.cos(2 * math.pi * i / DODGE_WAYS), math.sin(2 * math.pi * i / DODGE_WAYS))
                for i in range(DODGE_WAYS)]
        if self._dodge_way is not None:
            ways.append(self._dodge_way)
        for way in ways:
            if not self._safe(obs, way, 64.0):
                continue
            s = min(self._miss_by(pos, self._velocity, way, t) - self._clearance(MISSILES[t.name])
                    for t in missiles)
            s = min(s, 48.0)  # (clear is clear: then the rest decides)
            if self._dodge_way is not None:
                s += 6.0 * (way[0] * self._dodge_way[0] + way[1] * self._dodge_way[1])
            if s > score:
                best, score = way, s
        self._dodge_way = best
        return best

    @staticmethod
    def _miss_by(pos, vel, way, t: Thing) -> float:
        """How near t comes to her as it goes by, if she runs `way` from now."""
        x, y = pos
        vx, vy = vel
        nearest = math.inf
        for k in range(1, DODGE_TICS + 1):
            vx, vy = vx + THRUST * way[0], vy + THRUST * way[1]
            x, y = x + vx, y + vy
            vx, vy = vx * FRICTION, vy * FRICTION
            d = math.hypot(t.x + t.vx * k - x, t.y + t.vy * k - y)
            nearest = min(nearest, d)
            if d > nearest + 64:
                break  # (gone by)
        return nearest

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

    def _closing_in(self, obs: Observation, t: Thing, strafe: tuple[float, float] | None
                    ) -> tuple[float, float] | None:
        """Towards t, zig-zagging (the strafe's way and forwards), as far as it's safe."""
        a = math.atan2(t.y - obs.y, t.x - obs.x)
        ahead = (math.cos(a), math.sin(a))
        ways = [ahead]
        if strafe is not None:
            mixed = (ahead[0] + strafe[0], ahead[1] + strafe[1])
            n = math.hypot(*mixed)
            if n > 0.1:
                ways.insert(0, (mixed[0] / n, mixed[1] / n))
        return next((w for w in ways if self._safe(obs, w, 64.0)), strafe)

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
        """Every sector's floor and ceiling now (made once a tic: it's asked for a lot)."""
        if self._heights_for is not obs:
            hs = [(float(s.floor), float(s.ceiling)) for s in self.nav.map.sectors]
            for s, h in obs.heights.items():
                if s < len(hs):
                    hs[s] = (float(h[0]), float(h[1]))
            self._heights_for, self._heights_now = obs, hs
            self._hit_cache.clear()
        return self._heights_now

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
