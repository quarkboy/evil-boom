"""Evil playing with monsters about: the explorer finds the way, the fight (Combat) takes the
controls when it needs them, and between them she goes for what she needs to keep fighting
(weapons, ammo, health, armour she has seen) and, once there's nothing else, hunts down the
monsters she knows are left (some maps open only when they're dead: E1M8's barons)."""
from __future__ import annotations

import math

from evildoom.knowledge.explorer import Explorer, Objective
from evildoom.knowledge.model import Knowledge
from evildoom.motor.follower import TICRATE, Controls, Observation, RouteFollower
from evildoom.nav.reach import PLAYER_HEIGHT, Navigator
from evildoom.nav.walkthrough import Step
from evildoom.tactical.bestiary import (AMMO_ITEMS, ARMOR_ITEMS, HEALTH_ITEMS, MONSTERS, SUPER_SHOTGUN,
                                        WEAPON_ITEMS, weapon_in)
from evildoom.tactical.combat import OUTMATCHED, Combat, _segment_point_distance

# most of each ammo she can carry (without a backpack), by weapon slot
AMMO_MAX = {2: 200, 3: 50, 4: 200, 5: 50, 6: 300, 7: 300}
ITEM_RANGE = 1536.0   # items further than this (as the crow flies) only when they're needed badly
HUNT_NEAR = 512.0     # a monster's last known spot: hunted from the reachable poly nearest it, this near
HURT = 50.0           # below this health, health items come first
DANGER_NEAR = 640.0   # monsters this near an objective's spot count against going there
RETREAT_HEALTH = 40.0  # below this, with something shooting at her: back off out of its sight
COVER_RANGE = 768.0    # cover looked for among the places she's been this near
COVER_SPOTS = 48       # ...the nearest this many of them
CLEAR_TICS = TICRATE   # a retreat is over once nothing that shoots has been in sight this long
RETREAT_AGAIN = 3 * TICRATE  # after a retreat that found no cover, not again for this long
SURROUNDED_NEAR = 640.0  # monsters this near...
SURROUNDED_COUNT = 3     # ...this many of them...
SURROUNDED_SPREAD = 160.0  # ...spread round her this far (degrees): get out, to somewhere better
OUT_TICS = 4 * TICRATE   # getting out: at most this long (then the fight goes on from there)
# monsters whose attacks, landing on others, set them fighting (and that are worth not facing)
HEAVIES = {"Cyberdemon", "SpiderMastermind", "Fatso", "Arachnotron", "Revenant", "BaronOfHell", "HellKnight"}
SPECIES = {"HellKnight": "BaronOfHell"}  # (barons' and knights' fireballs don't hurt each other)
BAIT_HEAVY = 8.0         # a heavy taking longer than this (s) to kill with what she has: let others
BAIT_RANGE = 768.0       # ...take its shots: from a place she's been this near...
BAIT_AGAIN = 8 * TICRATE  # ...and not again for this long
VILE_KILL = 2.0          # an arch-vile she'd take longer than this (s) to kill: into cover
SPOT_RANGE = 1024.0      # somewhere better looked for among the places she's been this near
PISTOL_ONLY = 25.0       # firepower (damage a second) no better than this: the pistol's, about
RUN_SPEED = 400.0      # units a second she gets along a route, about (turns, doors, lifts)
GUN_WORTH = 10.0       # seconds a better gun is worth going for anyway (the fights to come)...
#                        ...and the fighting time it'd save with what she knows is still about


class Player:
    def __init__(self, nav: Navigator, knowledge: Knowledge | None = None):
        self.nav = nav
        self.explorer = Explorer(nav, knowledge)
        self.combat = Combat(nav)
        self.explorer.wanted = self._wanted
        self.explorer.last_resort = self._hunts
        self.explorer.gone = self._gone
        self.explorer.dangerous = self._dangerous
        self.explorer.interrupt = self._interrupt
        self._offered: set[int] = set()  # better guns she's turned aside for already
        self._present: set[int] = set()
        self._fighting = False
        self._retreat: RouteFollower | None = None
        self._retreat_why = ""
        self._bait_tic = -10**9
        self._retreat_tic = -10**9       # when the last retreat started or found no cover
        self._shooters_tic = -10**9      # when something that shoots was last in sight

    @property
    def knowledge(self) -> Knowledge:
        return self.explorer.knowledge

    @property
    def log(self) -> list[str]:
        return self.explorer.log

    @property
    def current(self) -> Objective | None:
        return self.explorer.current

    def restart(self) -> None:
        self.explorer.restart()
        self.combat.restart()
        self._fighting = False
        self._retreat = None
        self.combat.hold_fire = False

    def act(self, obs: Observation, last: Controls | None = None) -> Controls:
        self._present = {t.id for t in obs.things or ()}
        self.combat.observe(obs)
        for line in self.combat.log:
            self.explorer.log.append("  [fight] " + line)
        self.combat.log.clear()
        f = self.explorer.follower
        busy = f is not None and (f._sprint is not None or f._run is not None
                                  or f._lineup_tic is not None)
        d = self.combat.decide(obs, busy, "" if busy else self._errand(obs))
        retreat = None if busy else self._retreating(obs)
        if retreat is not None:
            self.explorer.hold(obs, last)
            c = retreat.act(obs)
            if d is not None and d.move is not None and d.why.startswith("dodging"):
                return self.combat.controls(obs, d)  # (a missile about to hit still comes first)
            return self.combat.controls(obs, d, c) if d is not None else c
        if d is not None and d.takeover:
            self.explorer.hold(obs, last)
            self._fighting = True
            return self.combat.controls(obs, d)
        self._fighting = False
        c = self.explorer.act(obs, last)
        return self.combat.controls(obs, d, c) if d is not None else c

    # --- backing off ------------------------------------------------------------------------

    def _shooters(self) -> list:
        return [t for t in self.combat.against_her() if MONSTERS[t.name].attack in ("hitscan", "missile", "fire")]

    def _retreating(self, obs: Observation) -> RouteFollower | None:
        """The way back to cover she's on, if she's falling back: hurt, with something that
        shoots in sight, to the nearest place she's been that nothing in sight can see (and
        on from there, the explorer's way: to the health she needs)."""
        if self._shooters():
            self._shooters_tic = obs.tic
        r = self._retreat
        if r is not None:
            if self._retreat_why in ("out", "bait"):
                over = obs.tic - self._retreat_tic > OUT_TICS
            else:
                over = obs.tic - self._shooters_tic > CLEAR_TICS
            if self._retreat_why == "bait" and any(self.combat.infighting(t) for t in self.combat.visible):
                over = True  # (they're at it: leave them to it)
            if r.done or r.failed or over:
                self.explorer.log.append(f"  [fight] tic {obs.tic}: "
                                         + ("got there" if r.done else f"{self._retreat_why} over"))
                self._retreat = None
                self.combat.hold_fire = False
                if self.explorer.follower is not None:
                    self.explorer.follower.resume(obs.tic)
            return self._retreat
        health = obs.health if obs.health is not None else 100.0
        if obs.tic - self._retreat_tic < RETREAT_AGAIN:
            return None
        if health < RETREAT_HEALTH and self._shooters() or self._vile(obs):
            self._retreat_tic = obs.tic
            self._retreat = self._to_cover(obs)
            self._retreat_why = "cover"
        elif self._surrounded(obs):
            self._retreat_tic = obs.tic
            self._retreat = self._out_of_it(obs)
            self._retreat_why = "out"
        elif obs.tic - self._bait_tic > BAIT_AGAIN and (bait := self._baiting(obs)) is not None:
            self._retreat_tic = self._bait_tic = obs.tic
            self._retreat = bait
            self._retreat_why = "bait"
            self.combat.hold_fire = True
        return self._retreat

    def _baiting(self, obs: Observation) -> RouteFollower | None:
        """With a heavy after her she can't see off quickly and monsters of another kind near
        it: to a place she's been where its shots at her go through them (they turn on it; it
        on them), holding fire so as not to kill the shield."""
        them = self.combat.against_her()
        fp = self.combat.firepower(obs)
        heavies = [h for h in them if h.name in HEAVIES and MONSTERS[h.name].health / fp > BAIT_HEAVY]
        if not heavies:
            return None
        k, mesh = self.explorer.knowledge, self.nav.mesh
        pos = (obs.x, obs.y)
        spots = sorted((math.dist(mesh.polys[pi].center, pos), pi) for pi in k.visited
                       if not k.damaging(mesh.polys[pi].sector)
                       and 64 < math.dist(mesh.polys[pi].center, pos) < BAIT_RANGE)[:COVER_SPOTS * 2]
        good = []
        for h in heavies:
            kind = SPECIES.get(h.name, h.name)
            shields = [b for b in self.combat.visible if b.id != h.id
                       and SPECIES.get(b.name, b.name) != kind and math.dist((b.x, b.y), (h.x, h.y)) < 1024]
            for d, pi in spots:
                c = mesh.polys[pi].center
                for b in shields:
                    if _segment_point_distance((h.x, h.y), c, (b.x, b.y)) < MONSTERS[b.name].radius + 8 \
                            and 64 < math.dist((h.x, h.y), (b.x, b.y)) < math.dist((h.x, h.y), c) - 128:
                        good.append((d, pi, h, b))
                        break
        good.sort(key=lambda g: g[0])
        for d, pi, h, b in good[:6]:
            follower = self._go_to(obs, pi, them, "bait")
            if follower is not None:
                self.explorer.log.append(f"  [fight] tic {obs.tic}: baiting {h.name} {h.id} into {b.name} {b.id}")
                return follower
        return None

    def _go_to(self, obs: Observation, pi: int, them: list, label: str) -> RouteFollower | None:
        """A way to that poly (not past monsters), if there is one."""
        mesh = self.nav.mesh
        pos = (obs.x, obs.y)
        view = self.explorer.view()
        live = {s: {h} for s, h in obs.heights.items()}
        goal = mesh.polys[pi].center
        path = view.find_path(pos, goal, live)
        if path is None:
            return None
        through = min((_segment_point_distance(a, b, (t.x, t.y)) for a, b in zip(path.waypoints, path.waypoints[1:])
                       if a and b for t in them), default=math.inf)
        if through < 96:
            return None  # (not past them)
        return RouteFollower(view, [Step(path, "goto", label=label)], keys=set(obs.keys or ()),
                             states=live, replan_level=False, tic=obs.tic)

    def _vile(self, obs: Observation) -> bool:
        """An arch-vile in sight she can't kill in a moment: out of its sight, whatever her
        health (its fire does nothing once it can't see her; MAP11: three deaths at 50-70
        health, one in sight)."""
        fp = self.combat.firepower(obs)
        return any(t.name == "Archvile" and MONSTERS[t.name].health / fp > VILE_KILL
                   for t in self.combat.against_her())

    def _surrounded(self, obs: Observation) -> bool:
        """Several monsters near her, and round her rather than all one side."""
        pos = (obs.x, obs.y)
        near = [t for t in self.combat.against_her() if math.dist((t.x, t.y), pos) < SURROUNDED_NEAR
                and MONSTERS[t.name].attack != "none"]
        return len(near) >= SURROUNDED_COUNT and _spread(pos, [(t.x, t.y) for t in near]) > SURROUNDED_SPREAD

    def _out_of_it(self, obs: Observation) -> RouteFollower | None:
        """To somewhere she's been nearby where fewer of them can see her, they're further,
        and all on one side of her (so she can keep them in front of her), by a way that
        doesn't go through them."""
        k, mesh = self.explorer.knowledge, self.nav.mesh
        pos = (obs.x, obs.y)
        them = [t for t in self.combat.visible if MONSTERS[t.name].attack != "none"]
        spots = sorted((math.dist(mesh.polys[pi].center, pos), pi) for pi in k.visited
                       if not k.damaging(mesh.polys[pi].sector)
                       and 96 < math.dist(mesh.polys[pi].center, pos) < SPOT_RANGE)[:COVER_SPOTS * 2]
        if not spots or not them:
            return None
        heights = self.combat._heights(obs)
        bodies = [(*mesh.polys[pi].center, heights[mesh.polys[pi].sector][0], PLAYER_HEIGHT) for _, pi in spots]
        seen = [0.0] * len(spots)
        for t in them:
            eye = (t.x, t.y, t.z, MONSTERS[t.name].height)
            for i, v in enumerate(self.combat.los.visible(eye, bodies, heights)):
                seen[i] += MONSTERS[t.name].threat if v else 0.0
        scored = []
        for (d, pi), sees in zip(spots, seen):
            c = mesh.polys[pi].center
            nearest = min(math.dist(c, (t.x, t.y)) for t in them)
            cost = sees + _spread(c, [(t.x, t.y) for t in them]) / 45 - min(nearest, 640) / 96 + d / 256
            scored.append((cost, pi))
        scored.sort()
        for _, pi in scored[:6]:
            follower = self._go_to(obs, pi, them, "out of it")
            if follower is not None:
                goal = mesh.polys[pi].center
                self.explorer.log.append(f"  [fight] tic {obs.tic}: surrounded by {len(them)}: getting out to "
                                         f"({goal[0]:.0f}, {goal[1]:.0f})")
                return follower
        self.explorer.log.append(f"  [fight] tic {obs.tic}: surrounded, nowhere better in reach")
        return None

    def _to_cover(self, obs: Observation) -> RouteFollower | None:
        k, mesh = self.explorer.knowledge, self.nav.mesh
        pos = (obs.x, obs.y)
        shooters = self._shooters()
        spots = sorted((math.dist(mesh.polys[pi].center, pos), pi) for pi in k.visited
                       if not k.damaging(mesh.polys[pi].sector)
                       and math.dist(mesh.polys[pi].center, pos) < COVER_RANGE)[:COVER_SPOTS]
        if not spots:
            return None
        heights = self.combat._heights(obs)
        bodies = [(*mesh.polys[pi].center, heights[mesh.polys[pi].sector][0], PLAYER_HEIGHT) for _, pi in spots]
        seen = [False] * len(spots)
        for t in shooters:
            eye = (t.x, t.y, t.z, MONSTERS[t.name].height)
            for i, v in enumerate(self.combat.los.visible(eye, bodies, heights)):
                seen[i] = seen[i] or v
        hidden = [(d, pi) for (d, pi), v in zip(spots, seen) if not v and d > 32]
        if not hidden:
            self.explorer.log.append(f"  [fight] tic {obs.tic}: no cover in reach")
            return None
        view = self.explorer.view()
        live = {s: {h} for s, h in obs.heights.items()}
        for _, pi in hidden[:4]:
            goal = mesh.polys[pi].center
            path = view.find_path(pos, goal, live)
            if path is None:
                continue
            self.explorer.log.append(f"  [fight] tic {obs.tic}: falling back to cover at "
                                     f"({goal[0]:.0f}, {goal[1]:.0f}), health {obs.health:.0f}")
            return RouteFollower(view, [Step(path, "goto", label="cover")], keys=set(obs.keys or ()),
                                 states=live, replan_level=False, tic=obs.tic)
        return None

    # --- what the explorer is offered -----------------------------------------------------

    def _wanted(self, obs: Observation, reach: set[int], pos) -> list[Objective]:
        """Items she has seen that she could use now."""
        k, mesh = self.explorer.knowledge, self.nav.mesh
        known = k.known_polys()
        health = obs.health if obs.health is not None else 100.0
        armor = obs.armor or 0.0
        held = obs.weapons or frozenset()
        ammo = obs.ammo or {}
        short = self.combat.short_of_ammo(obs)  # (then ammo and guns before anything else)
        out = []
        for t in obs.things or ():
            poly = mesh.poly_at(t.x, t.y)
            if poly is None or poly.index not in known or poly.index not in reach:
                continue
            d = math.dist((t.x, t.y), pos)
            need = 0.0  # how much it's needed (0: not at all, 1: badly)
            if t.name in WEAPON_ITEMS:
                need = 1.0 if self._new_gun(obs, t.name) else 0.0
            elif t.name in AMMO_ITEMS:
                slots, _ = AMMO_ITEMS[t.name]
                fill = [ammo.get(s, 0) / AMMO_MAX[s] for s in slots if s in held]
                if t.name == "Backpack":
                    need = 0.8
                elif fill:
                    need = max(0.0, 1.0 - min(fill) * 1.5)
            elif t.name in HEALTH_ITEMS:
                amount, cap = HEALTH_ITEMS[t.name]
                if amount >= 10 and health < cap:
                    need = min(1.0, (cap - health) / 60.0)
            elif t.name in ARMOR_ITEMS:
                amount, cap = ARMOR_ITEMS[t.name]
                if amount >= 100 and armor < cap * 0.7:
                    need = 0.6
            if need <= 0.1 or (d > ITEM_RANGE and need < 0.8):
                continue
            goal = self.explorer._stand_spot((t.x, t.y), reach)
            why = ""
            if t.name in WEAPON_ITEMS and need and self._better_gun(obs, t.name):
                why = " (a better gun)"
            elif short and (t.name in WEAPON_ITEMS or t.name in AMMO_ITEMS):
                why = " (out of ammo)"
            elif health < HURT and t.name in HEALTH_ITEMS and HEALTH_ITEMS[t.name][0] >= 10:
                why = " (hurt)"
            worth = self._gun_worth(obs, t.name) * RUN_SPEED if why == " (a better gun)" else None
            if worth is not None and d > worth:
                why = ""  # (too far to drop everything for, as the crow flies even)
            out.append(Objective("item", goal, f"pick up {t.name}{why}", thing=t.id, urgent=bool(why),
                                 worth=worth if why else None))
        return out

    @staticmethod
    def _new_gun(obs: Observation, name: str) -> bool:
        if name == "SuperShotgun":
            return not obs.super_shotgun
        return WEAPON_ITEMS[name] not in (obs.weapons or frozenset({1, 2}))

    def _better_gun(self, obs: Observation, name: str) -> bool:
        """Would that gun do more than the best she has (worth dropping what she's doing
        for: the shotgun over the pistol, the rocket launcher, ...)?"""
        if name == "Chainsaw" or not self._new_gun(obs, name):
            return False
        w = SUPER_SHOTGUN if name == "SuperShotgun" else weapon_in(WEAPON_ITEMS[name], obs.super_shotgun)
        return w.dps > self.combat.firepower(obs)

    def _gun_worth(self, obs: Observation, name: str) -> float:
        """Seconds of walking a gun is worth: what it'd save killing what she knows is still
        about (and some, for what's to come)."""
        w = SUPER_SHOTGUN if name == "SuperShotgun" else weapon_in(WEAPON_ITEMS[name], obs.super_shotgun)
        now = self.combat.firepower(obs)
        hp = sum(MONSTERS[s.name].health for s in self.combat.remaining())
        return GUN_WORTH + max(0.0, hp / now - hp / max(w.dps, now))

    def _interrupt(self, obs: Observation, obj: Objective) -> bool:
        """Something to fight with come into what she knows of the map (a better gun; ammo,
        when she's short): go for it now (once for each)."""
        if obj.urgent or obj.kind == "exit":
            return False
        t = next(iter(self._errands(obs)), None)
        if t is None:
            return False
        self._offered.add(t.id)
        self.explorer.log.append(f"  [fight] tic {obs.tic}: spotted a {t.name}")
        return True

    def _errand(self, obs: Observation) -> str:
        """What she's going for to fight with, if that's what the route's for (then the fight
        lets the route go on); or, mid-fight, with only the pistol or next to no ammo, something
        to fight with she hasn't turned aside for yet (the fight lets go a tic, for the explorer
        to take it up: MAP09's hot start, zombies about and the shotgun across the room)."""
        cur = self.explorer.current
        if cur is not None and cur.kind == "item" and cur.urgent:
            return cur.label.removeprefix("pick up ")
        if self.combat.firepower(obs) > PISTOL_ONLY and not self.combat.short_of_ammo(obs):
            return ""
        t = next(iter(self._errands(obs)), None)
        return t.name if t is not None else ""

    def _errands(self, obs: Observation):
        """Guns and ammo she knows where they are and hasn't turned aside for yet: better guns
        worth the walk, and, short of ammo, ammo for what she has (or any gun)."""
        known = self.explorer.knowledge.known_polys()
        mesh = self.nav.mesh
        pos = (obs.x, obs.y)
        short = self.combat.short_of_ammo(obs)
        held = obs.weapons or frozenset({1, 2})
        for t in obs.things or ():
            if t.id in self._offered:
                continue
            if t.name in WEAPON_ITEMS:
                if not (self._better_gun(obs, t.name) or short and self._new_gun(obs, t.name)
                        and t.name != "Chainsaw"):
                    continue
                if math.dist((t.x, t.y), pos) > self._gun_worth(obs, t.name) * RUN_SPEED:
                    continue
            elif not (short and t.name in AMMO_ITEMS and any(s in held for s in AMMO_ITEMS[t.name][0])
                      and math.dist((t.x, t.y), pos) < ITEM_RANGE):
                continue
            poly = mesh.poly_at(t.x, t.y)
            if poly is not None and poly.index in known:
                yield t

    def _hunts(self, obs: Observation, reach: set[int], pos) -> list[Objective]:
        """Monsters seen that are still about: go where each was last seen."""
        mesh = self.nav.mesh
        out = []
        for s in self.combat.remaining():
            poly = mesh.poly_at(s.x, s.y)
            if poly is None or poly.index not in reach:
                near = [pi for pi in reach if math.dist(mesh.polys[pi].center, s.pos) < HUNT_NEAR]
                if not near:
                    continue
                poly = mesh.polys[min(near, key=lambda pi: math.dist(mesh.polys[pi].center, s.pos))]
                goal = poly.center
            else:
                goal = s.pos
            goal = self.explorer._stand_spot(goal, reach)
            out.append(Objective("hunt", goal, f"hunt {s.name} {s.id}", thing=s.id))
        return out

    def _dangerous(self, obs: Observation, obj: Objective) -> bool:
        """Are there monsters she knows of by the objective's spot she couldn't take on with
        what she has? (Supplies she badly needs and the exit are worth it anyway.)"""
        if obj.urgent or obj.kind == "exit":
            return False
        hp = sum(MONSTERS[s.name].health for s in self.combat.remaining()
                 if math.dist(s.pos, obj.goal) < DANGER_NEAR)
        return hp / self.combat.firepower(obs) > OUTMATCHED

    def _gone(self, obs: Observation, obj: Objective) -> bool:
        if obj.kind == "item":
            return obj.thing not in self._present
        if obj.kind == "hunt":
            return obj.thing not in self.combat.alive
        return False


def _spread(at: tuple[float, float], points: list[tuple[float, float]]) -> float:
    """How far round `at` the points are spread (degrees: 360 less the widest gap)."""
    if len(points) < 2:
        return 0.0
    bearings = sorted(math.degrees(math.atan2(p[1] - at[1], p[0] - at[0])) % 360 for p in points)
    gaps = [b - a for a, b in zip(bearings, bearings[1:])] + [bearings[0] + 360 - bearings[-1]]
    return 360 - max(gaps)

