"""The expert's run: every item that counts towards the tally picked up, then the exit (no
monsters). The explorer's own way round (it explores everything anyway, with the exit held
back till nothing else is left: Explorer.exit_last), with the items she has seen and can get to
offered as things to do (Explorer.wanted). In vanilla a second computer area map can't be
picked up (the power's had already), though it counts: such a level can't be 100%."""
from __future__ import annotations

from evildoom.knowledge.explorer import Explorer, Objective
from evildoom.motor.follower import Observation

# what counts towards the items tally (MF_COUNTITEM in the game's tables)
COUNTED = {"HealthBonus", "ArmorBonus", "Soulsphere", "Megasphere", "Berserk", "Allmap",
           "Infrared", "InvulnerabilitySphere", "BlurSphere"}


class Completionist:
    """Hooks an explorer up to collect every counted item before it leaves."""

    def __init__(self, explorer: Explorer):
        self.explorer = explorer
        explorer.exit_last = True
        explorer.wanted = self._wanted
        explorer.gone = self._gone
        self._present: set[int] = set()

    def _wanted(self, obs: Observation, reach: set[int], pos) -> list[Objective]:
        self._present = {t.id for t in obs.things or ()}
        mesh = self.explorer.nav.mesh
        known = self.explorer.knowledge.known_polys()
        out = []
        for t in obs.things or ():
            if t.name not in COUNTED or (t.name == "Allmap" and obs.allmap):
                continue
            poly = mesh.poly_at(t.x, t.y)
            if poly is None or poly.index not in known or poly.index not in reach:
                continue
            out.append(Objective("item", self.explorer._stand_spot((t.x, t.y), reach),
                                 f"pick up {t.name}", thing=t.id))
        return out

    def _gone(self, obs: Observation, obj: Objective) -> bool:
        return obj.kind == "item" and obj.thing not in {t.id for t in obs.things or ()}
