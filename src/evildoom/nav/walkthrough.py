"""Plan a whole level as a sequence of steps a bot can execute."""
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


def plan_level(nav: Navigator, start: Point) -> LevelPlan:
    """From the map's initial heights, finish the level: go for the exit if a route exists,
    otherwise fetch the cheapest key we can plan a route to, and repeat."""
    m = nav.map
    all_polys = set(range(len(nav.mesh.polys)))
    exits = [t for t in nav.triggers if t.info.category is Category.EXIT and t.activators]
    keys_left: dict[Key, tuple[int, int]] = {}
    for thing in m.things:
        if thing.type in Key._value2member_map_:
            keys_left.setdefault(Key(thing.type), (thing.x, thing.y))

    plan = LevelPlan()
    pos, keys, states = start, set(), None
    while True:
        plans = []
        for t in exits:
            goal = nav._activation_point(t, pos, all_polys)
            route = nav.plan_route(pos, goal, keys, states)
            if route:
                plans.append((route, goal, None, t))
        if not plans:
            heights = states or nav.initial_states()
            for key, (x, y) in keys_left.items():
                for _, spot in nav.pickup_spots(x, y, heights):
                    route = nav.plan_route(pos, spot, keys, states)
                    if route:
                        plans.append((route, spot, key, None))
        if not plans:
            plan.problem = "no route to the exit" + (
                f" (keys still missing: {sorted(k.name for k in keys_left)})" if keys_left else "")
            return plan
        route, goal, key, exit_t = min(plans, key=lambda p: sum(l.path.cost for l in p[0].legs))
        for leg in route.legs:
            if leg.trigger:
                plan.steps.append(Step(leg.path, _action_for(leg.trigger), leg.trigger,
                                       label=f"line {leg.trigger.line}: "
                                             f"{leg.trigger.info.description}"))
            elif key:
                plan.steps.append(Step(leg.path, "pickup", key=key, label=f"pick up {key.name}"))
            else:
                action = _action_for(exit_t)
                plan.steps.append(Step(leg.path, "exit" if action == "use" else action, exit_t,
                                       label=f"exit via line {exit_t.line}"))
        pos, states = goal, route.states
        if key is None:
            plan.complete = True
            return plan
        keys.add(key)
        del keys_left[key]
