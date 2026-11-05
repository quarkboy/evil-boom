"""Playing a level on what Evil knows: the objectives open to her now, and an explorer that
picks one, plans a way there through the known map, and follows it.

Objectives (what Evil will choose between; the explorer is the stand-in that chooses for
now, nearest first):
- the exit, once she has seen it and can get to it;
- keys she has seen;
- switches and doors she has seen and not tried (what they do is learned by trying);
- the edge of the known map: places she can get to next to ones she hasn't seen.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field, replace

from evildoom.knowledge.model import Knowledge
from evildoom.motor.follower import TICRATE, Controls, Observation, RouteFollower
from evildoom.nav.reach import HeightStates, Navigator, TriggerLine
from evildoom.nav.walkthrough import Step, _action_for, steps_from_legs
from evildoom.wad.specials import Category, Key

PLAN_SECONDS = 5.0
GIVE_UP_TICS = 20 * TICRATE   # an objective not done by then is dropped for a while
RETRY_TICS = 30 * TICRATE     # ...and offered again after this
LOOK_TICS = TICRATE           # a turn on the spot, at the edge of the known map
KEEP_REACH = 0.75             # an objective mustn't leave less than this share reachable


@dataclass
class Objective:
    kind: str  # "exit", "key", "switch", "door", "frontier", "visit", "recheck"
    goal: tuple[float, float]
    label: str
    trigger: TriggerLine | None = None
    key: Key | None = None
    reveals: frozenset = frozenset()  # frontier: the unknown sectors it looks into

    @property
    def ident(self) -> tuple:
        return (self.kind, self.trigger.line if self.trigger else None, self.key,
                self.reveals or None)


@dataclass
class Explorer:
    nav: Navigator
    knowledge: Knowledge = None
    current: Objective | None = None
    follower: RouteFollower | None = None
    log: list[str] = field(default_factory=list)
    _dropped: dict = field(default_factory=dict)  # objective ident -> tic dropped
    _since: int = 0
    _exits_seen: int = 0
    _look_until: int | None = None

    def __post_init__(self) -> None:
        if self.knowledge is None:
            self.knowledge = Knowledge(self.nav)
        self._view = None
        self._view_key = None

    # --- the known world ----------------------------------------------------------------

    def states(self) -> HeightStates:
        """The heights the planner should take: as last seen where seen, else as at the start
        (those sectors can't be gone into anyway)."""
        k = self.knowledge
        return {s: {tuple(k.seen_heights[s])} if s in k.seen_heights else {h}
                for s, h in enumerate(self.nav.movers.initial)}

    def view(self) -> Navigator:
        """The navigator as far as Evil knows (rebuilt when what she knows changes)."""
        k = self.knowledge
        key = (len(k.sectors), sum(map(len, k.learned.values())), len(k.teleports))
        if self._view is None or key != self._view_key:
            triggers = []
            for t, sectors in k.known_triggers():
                effects = {s: hs for s, hs in t.effects.items() if s in sectors}
                recompute = t.recompute
                if recompute is not None:
                    recompute = (lambda f, keep: lambda st: {s: h for s, h in f(st).items()
                                                             if s in keep})(recompute, sectors)
                known = replace(t, effects=effects, recompute=recompute)
                for extra, value in vars(t).items():  # (set after construction: `relevant`)
                    known.__dict__.setdefault(extra, value)
                triggers.append(known)
            keys_at: dict = {}
            for th in k.known_things():
                if th.type in Key._value2member_map_:
                    keys_at.setdefault(Key(th.type), []).append((th.x, th.y))
            self._view = self.nav.restricted_to(k.known_polys(), set(k.teleports), triggers, keys_at)
            self._view_key = key
        return self._view

    # --- objectives ---------------------------------------------------------------------

    def objectives(self, obs: Observation) -> list[Objective]:
        """What Evil could go and do now."""
        nav, k, view = self.nav, self.knowledge, self.view()
        pos = (obs.x, obs.y)
        here = nav.mesh.poly_at(*pos)
        if here is None:
            return []
        keys = set(obs.keys or ())
        states = self.states()
        # (what she could get to using what she's learned: the planner works out how)
        actions = [t for t in view.triggers if t.effects]
        reach, states_all, _ = view._saturate(here.index, states, frozenset(keys), {}, actions)
        out: list[Objective] = []
        by_line = {t.line: t for t in nav.triggers if t.line >= 0}
        for li in k.exits():
            t = by_line.get(li)
            if t is not None and t.activators & reach and t.info.lock.satisfied(keys):
                out.append(Objective("exit", nav._activation_point(t, pos, reach),
                                     f"exit (line {li})", trigger=t))
        for th in k.known_things():
            if th.type in Key._value2member_map_ and Key(th.type) not in keys:
                spots = [(p, pt) for p, pt in view.pickup_spots(th.x, th.y, states) if p in reach]
                if spots:
                    pt = min(spots, key=lambda s: math.dist(s[1], pos))[1]
                    out.append(Objective("key", pt, f"pick up {Key(th.type).name}", key=Key(th.type)))
        for li in k.interactables():
            t = by_line.get(li)
            if t is None or not t.activators & reach or not t.info.lock.satisfied(keys):
                continue
            kind = "door" if t.info.category in (Category.DOOR, Category.LOCKED_DOOR) else "switch"
            out.append(Objective(kind, nav._activation_point(t, pos, reach),
                                 f"try {kind} (line {li})", trigger=t))
        out += self._frontier(reach, pos)
        out += self._recheck(reach, pos)
        out += self._unvisited(reach, pos)
        tic = obs.tic
        out = [o for o in out if tic - self._dropped.get(o.ident, -10**9) > RETRY_TICS]
        # (not anywhere that would cut her off from most of what she can get to now, as far as
        # she knows: pits, one-way drops; where she stands may itself be a spot she can't get
        # back to, so it's the share of the rest that counts)
        doors = view.local_doors(keys)
        back: dict[int, bool] = {}

        def returnable(o: Objective) -> bool:
            if o.kind == "exit":
                return True
            p = nav.mesh.poly_at(*o.goal)
            if p is None:
                return False
            if p.index not in back:
                after = view.reachable(p.index, states_all, doors)
                back[p.index] = here.index in after or len(after & reach) >= KEEP_REACH * len(reach)
            return back[p.index]
        return [o for o in out if returnable(o)]

    def _frontier(self, reach: set[int], pos) -> list[Objective]:
        """Known polys she can get to next to polys she hasn't seen: one per unknown sector
        bordered, at the reachable poly nearest her."""
        mesh, k = self.nav.mesh, self.knowledge
        best: dict[int, tuple[float, tuple]] = {}
        for pi in reach:
            poly = mesh.polys[pi]
            for portal in poly.portals:
                other = mesh.polys[mesh.portals[portal].other(pi)]
                if other.sector in k.sectors:
                    continue
                d = math.dist(poly.center, pos)
                if other.sector not in best or d < best[other.sector][0]:
                    best[other.sector] = (d, poly.center)
        return [Objective("frontier", c, f"look into sector {s}", reveals=frozenset({s}))
                for s, (_, c) in best.items()]

    def _unvisited(self, reach: set[int], pos) -> list[Objective]:
        """Known sectors she can get to but hasn't stood in (walking round finds the walk-over
        lines nobody can see), one per sector, at its reachable poly nearest her."""
        k, mesh = self.knowledge, self.nav.mesh
        stood = {mesh.polys[p].sector for p in k.visited}
        best: dict[int, tuple[float, tuple]] = {}
        for pi in reach:
            poly = mesh.polys[pi]
            if poly.sector in stood or self.nav.damage[poly.sector]:
                continue  # (not wading through nukage just to look)
            d = math.dist(poly.center, pos)
            if poly.sector not in best or d < best[poly.sector][0]:
                best[poly.sector] = (d, poly.center)
        return [Objective("visit", c, f"walk into sector {s}", reveals=frozenset({s}))
                for s, (_, c) in best.items()]

    def _recheck(self, reach: set[int], pos) -> list[Objective]:
        """Since a press whose effect wasn't seen: the known sectors round the reachable area
        not seen since (a door that opened out of sight, a floor that came down), one per
        sector, at the reachable poly next to it nearest her."""
        k, mesh = self.knowledge, self.nav.mesh
        since = k.unexplained_since()
        if since is None:
            return []
        reach_sectors = {mesh.polys[p].sector for p in reach}
        best: dict[int, tuple[float, tuple]] = {}
        for pi in reach:
            poly = mesh.polys[pi]
            for portal in poly.portals:
                s = mesh.polys[mesh.portals[portal].other(pi)].sector
                if s in reach_sectors or s not in k.sectors or k.seen_tic.get(s, -1) > since:
                    continue
                d = math.dist(poly.center, pos)
                if s not in best or d < best[s][0]:
                    best[s] = (d, poly.center)
        return [Objective("recheck", c, f"look at sector {s} again", reveals=frozenset({s}))
                for s, (_, c) in best.items()]

    def choose(self, obs: Observation) -> Objective | None:
        """The explorer's pick: the exit if it can get there, else the nearest objective
        (keys and untried switches a little ahead of the edge of the map)."""
        options = self.objectives(obs)
        exits = [o for o in options if o.kind == "exit"]
        if exits:
            return exits[0]
        weight = {"key": 0.6, "switch": 0.8, "door": 0.8, "frontier": 1.0, "visit": 1.5,
                  "recheck": 3.0}
        pos = (obs.x, obs.y)
        options.sort(key=lambda o: math.dist(o.goal, pos) * weight[o.kind])
        return options[0] if options else None

    def route_to(self, obj: Objective, obs: Observation) -> list[Step] | None:
        """Steps to get to an objective and do it, through what's known (using switches
        she has learned on the way if need be)."""
        view = self.view()
        keys = frozenset(obs.keys or ())
        states = self.states()
        k = self.knowledge
        key_items: dict = {}
        for th in k.known_things():
            if th.type in Key._value2member_map_:
                key_items.setdefault(Key(th.type), []).extend(view.pickup_spots(th.x, th.y, states))
        route = view.plan((obs.x, obs.y), [obj.goal], keys, states, key_items,
                          max_expansions=2000, max_seconds=PLAN_SECONDS)
        if route is None:
            return None
        if obj.kind in ("frontier", "recheck", "visit"):
            last = Step(None, "goto", label=obj.label)
        elif obj.kind == "key":
            last = Step(None, "pickup", key=obj.key, label=obj.label)
        else:
            action = _action_for(obj.trigger)
            last = Step(None, "exit" if obj.kind == "exit" and action == "use" else action,
                        obj.trigger, label=obj.label)
        return steps_from_legs(route.legs, last, view, states)

    # --- playing ------------------------------------------------------------------------

    def act(self, obs: Observation, last: Controls | None = None) -> Controls:
        self.knowledge.update(obs, last)
        if self.current is not None and self.follower is not None and self.follower.done:
            look = self._arrived(obs)
            if look is not None:
                return look
        if self._need_new(obs):
            self._next(obs)
        if self.follower is None:
            return Controls(turn=10.0)  # nothing to do: look round
        return self.follower.act(obs)

    def _seen_to(self, obj: Objective) -> bool:
        """Has what the objective was to look at been seen?"""
        k = self.knowledge
        if obj.kind == "frontier":
            return obj.reveals <= k.sectors
        if obj.kind == "recheck":
            return all(k.seen_tic.get(s, -1) >= self._since for s in obj.reveals)
        if obj.kind == "visit":
            return obj.reveals <= {self.nav.mesh.polys[p].sector for p in k.visited}
        return False

    def _arrived(self, obs: Observation) -> Controls | None:
        """The route to the objective is done: check a press went off, and look round where
        there's something to look for (what can be seen from there, what a switch did).
        Controls while still looking, else None (and a new objective's wanted)."""
        obj = self.current
        if obj.kind in ("switch", "door") and not any(
                f.line == obj.trigger.line and f.tic >= self._since for f in self.knowledge.fired):
            self._drop(obj, obs, "pressed, but nothing went off")
        elif obj.kind in ("frontier", "switch", "recheck") and not self._seen_to(obj):
            if self._look_until is None:
                self._look_until = obs.tic + LOOK_TICS
            if obs.tic < self._look_until:
                return Controls(turn=360.0 / LOOK_TICS)
            if obj.kind != "switch" and not self._seen_to(obj):
                self._drop(obj, obs, "nothing to see from there")
        elif obj.kind == "visit" and not self._seen_to(obj):
            self._drop(obj, obs, "got there but not in it")
        self._look_until = None
        self.follower = None
        return None

    def _need_new(self, obs: Observation) -> bool:
        f, obj = self.follower, self.current
        if f is None or obj is None or f.done or f.failed:
            if f is not None and f.failed and obj is not None:
                why = next((str(ev) for ev in reversed(f.log.events) if "FAILED" in str(ev)), "")
                self._drop(obj, obs, "failed: " + why.split("] ", 1)[-1])
            return True
        if obj.kind == "frontier" and obj.reveals <= self.knowledge.sectors:
            return True  # seen it on the way
        if obj.kind != "exit" and len(self.knowledge.exits()) > self._exits_seen:
            return True  # an exit came into view
        if obs.tic - self._since > GIVE_UP_TICS:
            self._drop(obj, obs, "took too long")
            return True
        return False

    def _drop(self, obj: Objective, obs: Observation, why: str) -> None:
        self._dropped[obj.ident] = obs.tic
        self.log.append(f"tic {obs.tic}: dropped '{obj.label}' ({why})")

    def _next(self, obs: Observation) -> None:
        self.follower = None
        for _ in range(6):
            obj = self.choose(obs)
            if obj is None:
                if self.current is not None:
                    self.log.append(f"tic {obs.tic}: nothing left to do")
                self.current = None
                return
            steps = self.route_to(obj, obs)
            if steps:
                self.current, self._since = obj, obs.tic
                self._exits_seen = len(self.knowledge.exits())
                self.follower = RouteFollower(self.view(), steps, keys=set(obs.keys or ()),
                                              states=self.states(), replan_level=False,
                                              tic=obs.tic)
                self.log.append(f"tic {obs.tic}: {obj.label} ({len(steps)} steps) | "
                                f"{self.knowledge.summary()}")
                return
            self._drop(obj, obs, "no route")
        self.current = None
