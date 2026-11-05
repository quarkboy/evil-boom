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
from evildoom.tactical.bestiary import AMMO_ITEMS, ARMOR_ITEMS, HEALTH_ITEMS, MONSTERS, WEAPON_ITEMS
from evildoom.tactical.combat import OUTMATCHED, Combat

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


class Player:
    def __init__(self, nav: Navigator, knowledge: Knowledge | None = None):
        self.nav = nav
        self.explorer = Explorer(nav, knowledge)
        self.combat = Combat(nav)
        self.explorer.wanted = self._wanted
        self.explorer.last_resort = self._hunts
        self.explorer.gone = self._gone
        self.explorer.dangerous = self._dangerous
        self._present: set[int] = set()
        self._fighting = False
        self._retreat: RouteFollower | None = None
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

    def act(self, obs: Observation, last: Controls | None = None) -> Controls:
        self._present = {t.id for t in obs.things or ()}
        self.combat.observe(obs)
        for line in self.combat.log:
            self.explorer.log.append("  [fight] " + line)
        self.combat.log.clear()
        f = self.explorer.follower
        busy = f is not None and (f._sprint is not None or f._run is not None
                                  or f._lineup_tic is not None)
        d = self.combat.decide(obs, busy)
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
        return [t for t in self.combat.visible if MONSTERS[t.name].attack in ("hitscan", "missile", "fire")]

    def _retreating(self, obs: Observation) -> RouteFollower | None:
        """The way back to cover she's on, if she's falling back: hurt, with something that
        shoots in sight, to the nearest place she's been that nothing in sight can see (and
        on from there, the explorer's way: to the health she needs)."""
        if self._shooters():
            self._shooters_tic = obs.tic
        r = self._retreat
        if r is not None:
            if r.done or r.failed or obs.tic - self._shooters_tic > CLEAR_TICS:
                self.explorer.log.append(f"  [fight] tic {obs.tic}: "
                                         + ("in cover" if r.done else "retreat over"))
                self._retreat = None
                if self.explorer.follower is not None:
                    self.explorer.follower.resume(obs.tic)
            return self._retreat
        health = obs.health if obs.health is not None else 100.0
        if health >= RETREAT_HEALTH or not self._shooters() or obs.tic - self._retreat_tic < RETREAT_AGAIN:
            return None
        self._retreat_tic = obs.tic
        self._retreat = self._to_cover(obs)
        return self._retreat

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
                need = 1.0 if WEAPON_ITEMS[t.name] not in held or t.name == "SuperShotgun" else 0.0
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
            if short and (t.name in WEAPON_ITEMS or t.name in AMMO_ITEMS):
                why = " (out of ammo)"
            elif health < HURT and t.name in HEALTH_ITEMS and HEALTH_ITEMS[t.name][0] >= 10:
                why = " (hurt)"
            out.append(Objective("item", goal, f"pick up {t.name}{why}", thing=t.id, urgent=bool(why)))
        return out

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
