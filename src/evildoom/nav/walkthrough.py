"""Plan a whole level as a sequence of steps a bot can execute."""
import time
from dataclasses import dataclass, field, replace

from evildoom.wad.specials import Category, Key, Trigger

from .movers import TICRATE
from .reach import HeightStates, Navigator, Path, Point, TriggerLine


@dataclass
class Expect:
    """What a trigger step should make happen, for checking as it runs: `sector` reaches
    `heights` about `travel` tics after the action and then stays `hold` tics (None: for
    good). `needed`: the next step's path relies on it (goes over or through the sector)."""
    sector: int
    heights: tuple[int, int]
    travel: int
    hold: int | None
    needed: bool


@dataclass
class Step:
    """Walk `path`, then do `action` ("use", "shoot", "cross", "pickup", "exit", "kill", or
    "face": stop facing the trigger line without pressing it). `expects` lists what the
    action should then do to the world (see Expect)."""
    path: Path
    action: str
    trigger: TriggerLine | None = None
    key: Key | None = None
    label: str = ""
    expects: list[Expect] = field(default_factory=list)

    def substeps(self) -> list[str]:
        """The step spelled out, for display and debugging: walk, act, then what to wait
        for (permanent moves the next path needs) or beat (temporary ones)."""
        goal = next((p for p in reversed(self.path.waypoints) if p is not None), None)
        notes = f" ({'; '.join(self.path.steps)})" if self.path.steps else ""
        out = [f"walk to ({goal[0]:.0f}, {goal[1]:.0f}){notes}" if goal else "walk"]
        if self.action == "pickup":
            out.append(f"pick up {self.key.name}")
        elif self.trigger is not None and self.trigger.line >= 0:
            verb = {"use": "press", "exit": "press exit", "shoot": "shoot", "cross": "cross",
                    "face": "face"}.get(self.action, self.action)
            out.append(f"{verb} line {self.trigger.line}")
        elif self.action == "kill":
            out.append(self.label)
        for e in self.expects:
            if not e.needed:
                continue
            if e.hold is None:
                out.append(f"wait for sector {e.sector} to reach {e.heights} "
                           f"(~{e.travel / TICRATE:.1f}s)")
            else:
                out.append(f"get to sector {e.sector} within "
                           f"{(e.travel + e.hold) / TICRATE:.1f}s, before it moves back")
        return out


@dataclass
class LevelPlan:
    steps: list[Step] = field(default_factory=list)
    complete: bool = False
    problem: str = ""


def _action_for(t: TriggerLine) -> str:
    if t.line < 0:
        return "kill"
    if t.info.trigger in (Trigger.S1, Trigger.SR, Trigger.D1, Trigger.DR):
        return "use"
    if t.info.trigger in (Trigger.G1, Trigger.GR):
        return "shoot"
    return "cross"


def expects_of(nav: Navigator, t: TriggerLine, after: Path | None) -> list[Expect]:
    """What firing `t` should do, with timings; marked `needed` where the path `after`
    depends on it."""
    on_path = {nav.mesh.polys[p].sector for p in after.polys} if after else set()
    out = []
    if "perpetual" in t.info.description:
        return out  # they keep cycling: nothing to wait for or beat
    for s, hs in t.effects.items():
        h = hs[0]
        if h == nav.movers.initial[s] and len(hs) == 1:
            continue  # already like that: nothing to see
        timing = nav.movers.timing(t.info.special, t.info.description, s, h) \
            if t.line >= 0 else None
        out.append(Expect(s, h, timing.travel if timing else 0,
                          timing.hold if timing else None, s in on_path))
    return out


def steps_from_legs(legs, goal_step: Step | None = None,
                    nav: Navigator | None = None) -> list[Step]:
    """Steps for a Route's legs. The final (goal) leg becomes `goal_step` with that leg's
    path, or is dropped if `goal_step` is None. With `nav`, trigger steps get `expects`."""
    steps = []
    for i, leg in enumerate(legs[:-1]):
        if leg.trigger:
            expects = expects_of(nav, leg.trigger, legs[i + 1].path) if nav else []
            steps.append(Step(leg.path, _action_for(leg.trigger), leg.trigger,
                              label=f"line {leg.trigger.line}: {leg.trigger.info.description}",
                              expects=expects))
        elif leg.key:
            steps.append(Step(leg.path, "pickup", key=leg.key, label=f"pick up {leg.key.name}"))
    if goal_step is not None and legs:
        steps.append(replace(goal_step, path=legs[-1].path))
    return steps


def exit_goals(nav: Navigator, pos: Point, reach: set[int]) -> dict[Point, TriggerLine]:
    """Where to stand to use each exit line that can be activated from `reach`."""
    return {nav._activation_point(t, pos, reach): t for t in nav.triggers
            if t.info.category is Category.EXIT and t.activators & reach}


def _exit_step(t: TriggerLine) -> Step:
    action = _action_for(t)
    return Step(None, "exit" if action == "use" else action, t, label=f"exit via line {t.line}")


def plan_level(nav: Navigator, start: Point, max_seconds: float = 60.0,
               max_expansions: int = 5000) -> LevelPlan:
    """From the map's initial heights, finish the level: one search (Navigator.plan) whose
    goal is any exit, using switches and picking up keys in whatever order works."""
    start_poly = nav.mesh.poly_at(*start)
    stages = nav.progression(start_poly.index)
    if not any(t.info.category is Category.EXIT for s in stages for t in s.exits):
        return LevelPlan(problem="no route to the exit (not reachable even in principle)")
    # Stand where progression says the player can actually get to. One search: it prefers
    # paths the player's box surely fits through and only takes tight ones (the box check
    # has false alarms) when nothing cleaner gets there (Navigator.plan, Path.squeezes).
    return plan_from(nav, start, set(), None, stages[-1].reached, max_seconds, max_expansions)


def plan_from(nav: Navigator, pos: Point, keys: set[Key], states: HeightStates | None,
              reach: set[int] | None = None, max_seconds: float = 60.0,
              max_expansions: int = 5000) -> LevelPlan:
    """Plan the rest of the level from `pos` holding `keys`, with `states` the heights the
    world can take now (default: the map's initial heights). `reach` limits which exit
    activation spots are considered (default: anywhere)."""
    if reach is None:
        reach = {p.index for p in nav.mesh.polys if p}
    exits = exit_goals(nav, pos, reach)
    if not exits:
        return LevelPlan(problem="no exit lines")
    t0 = time.perf_counter()
    route = nav.plan(pos, list(exits), frozenset(keys), states, nav.key_items(),
                     max_expansions=max_expansions, max_seconds=max_seconds,
                     goal_triggers=exits)
    if route is None:
        timed_out = time.perf_counter() - t0 >= max_seconds
        return LevelPlan(problem="no route to the exit"
                         + (f" (search gave up after {max_seconds:.0f}s)" if timed_out else ""))
    return LevelPlan(steps=steps_from_legs(route.legs, _exit_step(exits[route.goal]), nav),
                     complete=True)
