"""Nav spike: key/switch progression stages and a height-aware path to the exit.

Usage: python spikes/nav_spike.py <wad> <map> [--no-plot]
"""
import sys
from pathlib import Path

from evildoom.config import resolve_wad
from evildoom.nav.reach import MAX_STEP, Navigator, can_cross
from evildoom.wad.mapdata import load_map
from evildoom.wad.specials import Category, Key
from evildoom.wad.wad import Wad

OUT = Path(__file__).resolve().parent.parent / "out"
STAGE_COLORS = ["#a6d96a", "#fdae61", "#74add1", "#f46d43", "#abd9e9", "#d73027", "#fee090"]


def describe(nav: Navigator, stages) -> None:
    total = sum(1 for p in nav.mesh.polys if p)
    for i, st in enumerate(stages):
        keys = ", ".join(sorted(k.name for k in st.keys)) or "no keys"
        print(f"\n-- stage {i} [{keys}]: {len(st.reached)}/{total} polys reachable")
        for t in st.fired:
            if t.info.category is Category.DOOR and t.info.trigger.value in ("DR", "D1"):
                continue  # plain doors are noise
            lock = f" ({t.info.lock.value})" if t.info.lock.value != "none" else ""
            print(f"   fires line {t.line:>4} {t.info.trigger.value} "
                  f"{t.info.description}{lock} -> sectors {t.sectors}")
        new_keys = st.keys_available - st.keys
        if new_keys:
            print("   picks up:", ", ".join(sorted(k.name for k in new_keys)))
        if st.exits:
            print("   EXIT reachable:", [f"line {t.line} ({t.info.category.value})"
                                         for t in st.exits])


def one_way_portals(nav: Navigator, states):
    """Portals passable in exactly one direction under the given states."""
    out = []
    for portal in nav.mesh.portals:
        ab = nav.passable(portal.index, portal.a, states)
        ba = nav.passable(portal.index, portal.b, states)
        if ab != ba:
            out.append((portal, portal.a if ab else portal.b))
    return out


def plot(nav: Navigator, stages, legs, states, out: Path) -> None:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.patches import Polygon

    m, mesh = nav.map, nav.mesh
    fig, ax = plt.subplots(figsize=(16, 16))
    ax.set_aspect("equal")
    stage_of = {}
    for i, st in enumerate(stages):
        for p in st.reached:
            stage_of.setdefault(p, i)
    for poly in mesh.polys:
        if poly is None:
            continue
        stage = stage_of.get(poly.index)
        color = STAGE_COLORS[stage % len(STAGE_COLORS)] if stage is not None else "#dddddd"
        ax.add_patch(Polygon(poly.vertices, closed=True, facecolor=color,
                             edgecolor="white", linewidth=0.2))
    for line in m.linedefs:
        if not line.two_sided:
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            ax.plot([a.x, b.x], [a.y, b.y], color="black", linewidth=1)

    for portal, frm in one_way_portals(nav, states):
        fx, fy = mesh.polys[frm].center
        mx, my = portal.mid
        ax.annotate("", xy=(mx + (mx - fx) * 0.15, my + (my - fy) * 0.15), xytext=(mx, my),
                    arrowprops=dict(arrowstyle="->", color="#7b3294", lw=1.2))

    for tp in nav.teleports:
        a, b = m.vertexes[m.linedefs[tp.line].v1], m.vertexes[m.linedefs[tp.line].v2]
        ax.plot([(a.x + b.x) / 2, tp.dest[0]], [(a.y + b.y) / 2, tp.dest[1]],
                color="#17becf", linestyle=":", linewidth=1)

    cmap = plt.get_cmap("autumn")
    for i, leg in enumerate(legs):
        color = cmap(i / max(len(legs) - 1, 1))
        seg = []
        for p in leg.path.waypoints + [None]:
            if p is None:
                if seg:
                    ax.plot([q[0] for q in seg], [q[1] for q in seg], color=color,
                            linewidth=2.5, marker="o", markersize=3)
                seg = []
            else:
                seg.append(p)
        end = [w for w in leg.path.waypoints if w][-1]
        ax.annotate(f"{i + 1}", end, fontsize=11, fontweight="bold", color="black",
                    bbox=dict(boxstyle="circle", fc="white", ec=color))

    labels = [f"stage {i}: " + (", ".join(sorted(k.name for k in st.keys)) or "no keys")
              for i, st in enumerate(stages)]
    for i, label in enumerate(labels):
        ax.plot([], [], color=STAGE_COLORS[i % len(STAGE_COLORS)], linewidth=8, label=label)
    ax.plot([], [], color="#dddddd", linewidth=8, label="never reachable")
    ax.plot([], [], color="#7b3294", label="one-way drop (arrow = allowed direction)")
    ax.plot([], [], color="red", linewidth=2.5, label="walkthrough legs (numbered, red -> yellow)")
    ax.legend(loc="upper left", fontsize=9)
    ax.set_title(f"{m.name}: reachability by key state, height-aware path to exit")
    ax.autoscale_view()
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out, dpi=110, bbox_inches="tight")
    print(f"\nplot written to {out}")


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    wad = Wad(resolve_wad(args[0]))
    m = load_map(wad, args[1])
    nav = Navigator(m)
    start = next(t for t in m.things if t.type == 1)
    start_poly = nav.mesh.poly_at(start.x, start.y)
    print(f"{m.name}: {sum(1 for p in nav.mesh.polys if p)} polys, {len(nav.mesh.portals)} "
          f"portals, {len(nav.triggers)} triggers, {len(nav.teleports)} teleporters")

    stages = nav.progression(start_poly.index)
    describe(nav, stages)
    states = nav.final_states

    legs = walkthrough(nav, (start.x, start.y), stages)

    drops = one_way_portals(nav, states)
    print(f"\none-way portals: {len(drops)} (drops > {MAX_STEP})")
    if "--no-plot" not in sys.argv:
        plot(nav, stages, legs, states, OUT / f"{wad.path.stem}_{m.name}_nav.png")


def walkthrough(nav: Navigator, start, stages):
    """From the map's initial heights, finish the level: at each step go for the exit if a
    route exists, otherwise fetch the cheapest key we can plan a route to. Each goal is
    planned with plan_route, so the switches/walk triggers it needs are included."""
    m = nav.map
    all_polys = set(range(len(nav.mesh.polys)))
    exits = [t for t in nav.triggers if t.info.category is Category.EXIT and t.activators]
    keys_left = {}
    for t in m.things:
        if t.type in Key._value2member_map_:
            keys_left.setdefault(Key(t.type), (t.x, t.y))

    print("\n== walkthrough from initial heights")
    pos, keys, states, legs = start, set(), None, []
    while True:
        options = [(f"use exit (line {t.line})", nav._activation_point(t, pos, all_polys), None)
                   for t in exits]
        plans = [(label, goal, key, nav.plan_route(pos, goal, keys, states))
                 for label, goal, key in options]
        plans = [p for p in plans if p[3]]
        if not plans:
            heights = states or nav.initial_states()
            plans = [(f"pick up {key.name}", spot, key, nav.plan_route(pos, spot, keys, states))
                     for key, (x, y) in keys_left.items()
                     for _, spot in nav.pickup_spots(x, y, heights)]
            plans = [p for p in plans if p[3]]
        if not plans:
            print("  FAILED to plan: exit"
                  + (f" (keys still missing: {sorted(k.name for k in keys_left)})"
                     if keys_left else ""))
            return legs
        label, goal, key, route = min(plans, key=lambda p: sum(l.path.cost for l in p[3].legs))
        for leg in route.legs:
            p = leg.path
            what = (f"then {leg.trigger.info.trigger.value} line {leg.trigger.line} "
                    f"({leg.trigger.info.description}) -> sectors {leg.trigger.sectors}"
                    if leg.trigger else f"then {label}")
            print(f"  walk {p.cost:6.0f} units, {len(p.polys):3} polys; {what}")
            for note in p.steps:
                print(f"      {note}")
            legs.append(leg)
        pos, states = goal, route.states
        if key is None:
            return legs
        keys.add(key)
        del keys_left[key]


if __name__ == "__main__":
    main()
