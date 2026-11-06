"""The expert's run: every item that counts towards the tally picked up, then the exit (no
monsters). The explorer's own way round (it explores everything anyway, with the exit held
back till nothing else is left: Explorer.exit_last), with the items she has seen and can get to
offered as things to do (Explorer.wanted). In vanilla a second computer area map can't be
picked up (the power's had already), though it counts: such a level can't be 100%."""
from __future__ import annotations

from evildoom.knowledge.explorer import Explorer, Objective
from evildoom.motor.follower import Observation

ALLMAP = 2026  # the computer area map's thing type


def most_items(m, skill: int) -> int:
    """How many fewer than the level's item total is the best there is: every computer area map
    after the first (vanilla won't let a second be picked up, though each counts: Doom II's
    MAP27, two in mirrored alcoves, tops out at 98%)."""
    maps = sum(1 for th in m.things if th.type == ALLMAP and th.in_skill(skill))
    return max(0, maps - 1)

# what counts towards the items tally (MF_COUNTITEM in the game's tables)
COUNTED = {"HealthBonus", "ArmorBonus", "Soulsphere", "Megasphere", "Berserk", "Allmap",
           "Infrared", "InvulnerabilitySphere", "BlurSphere"}


class Completionist:
    """Hooks an explorer up to collect every counted item before it leaves."""

    def __init__(self, explorer: Explorer, restarts: int = 0):
        self.explorer = explorer
        explorer.exit_last = True
        explorer.wanted = self._wanted
        explorer.gone = self._gone
        explorer.before_exit = self._again
        self.restarts = restarts  # (levels again she may ask for)
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

    def _again(self, obs: Observation) -> bool:
        """Items she's seen she can't get to now, but could from the start of the level, as
        far as she knows (E1M4's ledge: a floor that rises to it only with her on it, set off
        walking past): rather the level again (while she may) than out without them."""
        if self.restarts <= 0:
            return False
        ex = self.explorer
        mesh, known = ex.nav.mesh, ex.knowledge.known_polys()
        left = [t for t in obs.things or () if t.name in COUNTED and not (t.name == "Allmap" and obs.allmap)
                and (p := mesh.poly_at(t.x, t.y)) is not None and p.index in known]
        if not left:
            return False
        start = next(((t.x, t.y) for t in ex.nav.map.things if t.type == 1), None)
        if start is None:
            return False
        view = ex.view(fresh=True)
        route = view.plan(start, [(t.x, t.y) for t in left], frozenset(), view.initial_states(),
                          max_expansions=2000, max_seconds=10.0)
        if route is None:
            return False
        self.restarts -= 1
        return True

    def _gone(self, obs: Observation, obj: Objective) -> bool:
        return obj.kind == "item" and obj.thing not in {t.id for t in obs.things or ()}
