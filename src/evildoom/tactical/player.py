"""Evil playing with monsters about: the explorer finds the way, the fight (Combat) takes the
controls when it needs them, and between them she goes for what she needs to keep fighting
(weapons, ammo, health, armour she has seen) and, once there's nothing else, hunts down the
monsters she knows are left (some maps open only when they're dead: E1M8's barons)."""
from __future__ import annotations

import math

from evildoom.knowledge.explorer import Explorer, Objective
from evildoom.knowledge.model import Knowledge
from evildoom.motor.follower import Controls, Observation
from evildoom.nav.reach import Navigator
from evildoom.tactical.bestiary import AMMO_ITEMS, ARMOR_ITEMS, HEALTH_ITEMS, WEAPON_ITEMS
from evildoom.tactical.combat import Combat

# most of each ammo she can carry (without a backpack), by weapon slot
AMMO_MAX = {2: 200, 3: 50, 4: 200, 5: 50, 6: 300, 7: 300}
ITEM_RANGE = 1536.0   # items further than this (as the crow flies) only when they're needed badly
HUNT_NEAR = 512.0     # a monster's last known spot: hunted from the reachable poly nearest it, this near


class Player:
    def __init__(self, nav: Navigator, knowledge: Knowledge | None = None):
        self.nav = nav
        self.explorer = Explorer(nav, knowledge)
        self.combat = Combat(nav)
        self.explorer.wanted = self._wanted
        self.explorer.last_resort = self._hunts
        self.explorer.gone = self._gone
        self._present: set[int] = set()
        self._fighting = False

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
        if d is not None and d.takeover:
            self.explorer.hold(obs, last)
            self._fighting = True
            return self.combat.controls(obs, d)
        self._fighting = False
        c = self.explorer.act(obs, last)
        return self.combat.controls(obs, d, c) if d is not None else c

    # --- what the explorer is offered -----------------------------------------------------

    def _wanted(self, obs: Observation, reach: set[int], pos) -> list[Objective]:
        """Items she has seen that she could use now."""
        k, mesh = self.explorer.knowledge, self.nav.mesh
        known = k.known_polys()
        health = obs.health if obs.health is not None else 100.0
        armor = obs.armor or 0.0
        held = obs.weapons or frozenset()
        ammo = obs.ammo or {}
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
            out.append(Objective("item", goal, f"pick up {t.name}", thing=t.id))
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

    def _gone(self, obs: Observation, obj: Objective) -> bool:
        if obj.kind == "item":
            return obj.thing not in self._present
        if obj.kind == "hunt":
            return obj.thing not in self.combat.alive
        return False
