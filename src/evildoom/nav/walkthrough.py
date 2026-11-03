"""Plan a whole level as a sequence of steps a bot can execute."""
import time
from dataclasses import dataclass, field

from evildoom.wad.specials import Category, Key, Trigger

from .reach import Navigator, Path, Point, TriggerLine


@dataclass
class Step:
    """Walk `path`, then do `action` ("use", "shoot", "cross", "pickup", "exit", "kill", or
    "face": stop facing the trigger line without pressing it)."""
    path: Path
    action: str
    trigger: TriggerLine | None = None
    key: Key | None = None
    label: str = ""


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


def plan_level(nav: Navigator, start: Point, max_seconds: float = 60.0,
               max_expansions: int = 5000) -> LevelPlan:
    """From the map's initial heights, finish the level: one search (Navigator.plan) whose
    goal is any exit, using switches and picking up keys in whatever order works."""
    start_poly = nav.mesh.poly_at(*start)
    stages = nav.progression(start_poly.index)
    if not any(t.info.category is Category.EXIT for s in stages for t in s.exits):
        return LevelPlan(problem="no route to the exit (not reachable even in principle)")
    # Stand where progression says the player can actually get to.
    reach = stages[-1].reached
    exits = {}
    for t in nav.triggers:
        if t.info.category is Category.EXIT and t.activators & reach:
            exits[nav._activation_point(t, start, reach)] = t
    t0 = time.perf_counter()
    route = nav.plan(start, list(exits), frozenset(), None, nav.key_items(),
                     max_expansions=max_expansions, max_seconds=max_seconds)
    if route is None:
        timed_out = time.perf_counter() - t0 >= max_seconds
        return LevelPlan(problem="no route to the exit"
                         + (f" (search gave up after {max_seconds:.0f}s)" if timed_out else ""))
    steps = []
    for leg in route.legs:
        if leg.trigger:
            steps.append(Step(leg.path, _action_for(leg.trigger), leg.trigger,
                              label=f"line {leg.trigger.line}: {leg.trigger.info.description}"))
        elif leg.key:
            steps.append(Step(leg.path, "pickup", key=leg.key, label=f"pick up {leg.key.name}"))
        else:
            exit_t = exits[route.goal]
            action = _action_for(exit_t)
            steps.append(Step(leg.path, "exit" if action == "use" else action, exit_t,
                              label=f"exit via line {exit_t.line}"))
    return LevelPlan(steps=steps, complete=True)
