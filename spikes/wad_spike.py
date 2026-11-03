"""WAD parsing spike: dump a map's interactive features and plot them.

Usage: python spikes/wad_spike.py <wad name or path> <map> [--no-plot]
  e.g. python spikes/wad_spike.py doom E1M1
       python spikes/wad_spike.py rerelease/sigil.wad E5M1

Map lumps are self-contained, so a PWAD can be parsed without its IWAD.
"""
import sys
from collections import Counter
from pathlib import Path

from evildoom.config import resolve_wad
from evildoom.wad.mapdata import ML_SECRET, MapData, load_map
from evildoom.wad.specials import Category, Key, Lock, Trigger, classify_line, classify_sector
from evildoom.wad.wad import Wad

OUT = Path(__file__).resolve().parent.parent / "out"

PLAYER_START = 1
KEY_COLORS = {"red": "#d62728", "blue": "#1f77b4", "yellow": "#e6b800"}
CATEGORY_STYLE = {  # color, linewidth
    Category.DOOR: ("#8c564b", 2.5),
    Category.LIFT: ("#ff7f0e", 2.5),
    Category.FLOOR: ("#2ca02c", 2.0),
    Category.CEILING: ("#2ca02c", 2.0),
    Category.STAIRS: ("#2ca02c", 2.0),
    Category.CRUSHER: ("#7f7f7f", 2.5),
    Category.EXIT: ("#e377c2", 3.5),
    Category.SECRET_EXIT: ("#9467bd", 3.5),
    Category.TELEPORT: ("#17becf", 3.0),
    Category.LIGHT: ("#bcbd22", 1.0),
    Category.OTHER: ("#bcbd22", 1.0),
}


def lock_color(lock: Lock) -> str:
    color = lock.value.split("_")[0]
    return KEY_COLORS.get(color, "black")


def affected_sectors(m: MapData, line) -> list[int]:
    """Door-style (D1/DR) specials act on the sector behind the line; others act on tagged sectors."""
    info = classify_line(line.special)
    if info.trigger in (Trigger.D1, Trigger.DR):
        back = m.back_sector(line)
        return [back] if back is not None else []
    return [s.index for s in m.tagged_sectors(line.tag)]


def sector_centroid(m: MapData, sector: int) -> tuple[float, float]:
    pts = [m.vertexes[v] for l in m.sector_lines(sector) for v in (l.v1, l.v2)]
    return sum(p.x for p in pts) / len(pts), sum(p.y for p in pts) / len(pts)


def summarize(m: MapData) -> None:
    print(f"== {m.name}: {len(m.things)} things, {len(m.linedefs)} lines, "
          f"{len(m.sidedefs)} sides, {len(m.sectors)} sectors, "
          f"{len(m.subsectors)} subsectors, {len(m.segs)} segs")

    starts = [t for t in m.things if t.type == PLAYER_START]
    for t in starts:
        print(f"player 1 start: ({t.x}, {t.y}) facing {t.angle}")
    keys = [t for t in m.things if t.type in Key._value2member_map_]
    print("keys:", [(Key(t.type).name, t.x, t.y) for t in keys] or "none")

    specials = [(l, classify_line(l.special)) for l in m.linedefs if l.special]
    print(f"\n{len(specials)} special lines, by category:",
          dict(Counter(info.category.value for _, info in specials)))
    print(f"\n{'line':>5} {'trig':4} {'category':12} {'lock':10} {'tag':>4} {'mon':3}  "
          f"description -> affected sectors")
    for line, info in specials:
        if info.category is Category.LIGHT:
            continue
        targets = affected_sectors(m, line)
        print(f"{line.index:>5} {info.trigger.value:4} {info.category.value:12} "
              f"{info.lock.value if info.lock is not Lock.NONE else '':10} {line.tag:>4} "
              f"{'yes' if info.monsters else '':3}  {info.description} -> {targets}")

    unclassified = Counter(i.special for _, i in specials
                           if i.description.startswith("unclassified"))
    if unclassified:
        print("\nunclassified specials:", dict(unclassified))

    secrets = [s.index for s in m.sectors if classify_sector(s.special).secret]
    hurt = Counter(classify_sector(s.special).damage for s in m.sectors)
    hurt.pop(0, None)
    print(f"\nsecret sectors: {secrets}")
    print(f"damaging sectors by damage: {dict(hurt)}")
    hidden = [l.index for l in m.linedefs if l.flags & ML_SECRET]
    print(f"lines flagged secret (hidden on automap): {len(hidden)}")


def plot(m: MapData, path: Path) -> None:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(figsize=(16, 16))
    ax.set_aspect("equal")
    ax.set_title(f"{m.name}: doors/locks/lifts/exits/teleports; dashed = trigger -> target")

    secret_sectors = {s.index for s in m.sectors if classify_sector(s.special).secret}
    for s in secret_sectors:
        cx, cy = sector_centroid(m, s)
        ax.plot(cx, cy, marker="$S$", color="#9467bd", markersize=12)

    for line in m.linedefs:
        a, b = m.vertexes[line.v1], m.vertexes[line.v2]
        info = classify_line(line.special)
        if info is None or info.category in (Category.LIGHT, Category.OTHER):
            color, width = ("black", 1.0) if not line.two_sided else ("#bbbbbb", 0.6)
        elif info.lock is not Lock.NONE:
            color, width = lock_color(info.lock), 4.0
        else:
            color, width = CATEGORY_STYLE[info.category]
        ax.plot([a.x, b.x], [a.y, b.y], color=color, linewidth=width, solid_capstyle="round")

        if info and info.category not in (Category.LIGHT, Category.OTHER):
            mx, my = (a.x + b.x) / 2, (a.y + b.y) / 2
            if info.trigger not in (Trigger.D1, Trigger.DR):
                ax.annotate(info.trigger.value, (mx, my), fontsize=6, color=color)
                for sec in affected_sectors(m, line):
                    cx, cy = sector_centroid(m, sec)
                    ax.plot([mx, cx], [my, cy], linestyle="--", linewidth=0.7, color=color)

    for t in m.things:
        if t.type in Key._value2member_map_:
            key = Key(t.type)
            marker = "*" if "SKULL" in key.name else "D"
            ax.plot(t.x, t.y, marker=marker, color=KEY_COLORS[key.color], markersize=14,
                    markeredgecolor="black")
        elif t.type == PLAYER_START:
            ax.plot(t.x, t.y, marker="^", color="lime", markersize=14, markeredgecolor="black")

    path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(path, dpi=110, bbox_inches="tight")
    print(f"\nplot written to {path}")


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    wad = Wad(resolve_wad(args[0]))
    if len(args) < 2:
        print(f"{wad.path.name} ({wad.kind}) maps:", " ".join(wad.map_names()))
        return
    m = load_map(wad, args[1])
    summarize(m)
    if "--no-plot" not in sys.argv:
        plot(m, OUT / f"{wad.path.stem}_{m.name}.png")


if __name__ == "__main__":
    main()
