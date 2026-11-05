"""Check the navigator's run-off jumps and keep only the ones that work.

By default each candidate is tried in the movement simulator (nav/physics.py, a port of ZDoom's
player movement: ~98% agreement with the engine, a few ms a jump). With engine=True it is tried
in ViZDoom instead (slow, cached). Either way the test is the same: warp to its run-up, line up, sprint along the planned
line, and check that the player touches down on the predicted sector (standing on its floor)
without being teleported away first, and can brake to a stop there. A "soft" sprint (full speed
only up to the edge, then coasting) is tried first: it lands slower, which narrow strips need. Jumps that need moved heights (a lowered fence, a raised
floor) get those set up first by firing the trigger that produces them; when that isn't possible
or reliable (lifts and timed doors) the jump is kept but marked unverified.

Results are cached per map in out/jumpcache, keyed by Jump.key, so only new jumps are tested.
"""
import json
import math
from dataclasses import replace
from pathlib import Path

from evildoom.motor.follower import TICRATE, Controls, Motion, _norm
from evildoom.nav.reach import MAX_STEP, Jump, Navigator, TriggerLine
from evildoom.nav.physics import PlayerSim, Teleported
from evildoom.wad.specials import Category, Trigger

from .vizdoom_engine import VizDoomEngine

CACHE_DIR = Path(__file__).resolve().parents[3] / "out" / "jumpcache"
VERSION = 5   # bump to invalidate cached results when the test procedure changes
OK, FAIL, UNVERIFIED = "ok", "fail", "unverified"
SOFT = "soft"   # works coasting from the edge (Jump.soft)
SLID = "slid"   # lands, but slides off the landing sector


def _cache_path(iwad: Path, map_name: str, pwad: Path | None) -> Path:
    name = f"{Path(iwad).stem}_{Path(pwad).stem + '_' if pwad else ''}{map_name}.json"
    return CACHE_DIR / name


def verify_jumps(nav: Navigator, iwad: str | Path, map_name: str, pwad: str | Path | None = None,
                 use_cache: bool = True, log=None) -> dict[str, str]:
    """Return {jump key: "ok" | "soft" | "slid" | "fail" | "unverified"} for every jump."""
    path = _cache_path(Path(iwad), map_name, Path(pwad) if pwad else None)
    cached: dict[str, str] = {}
    if use_cache and path.exists():
        data = json.loads(path.read_text())
        if data.get("version") == VERSION:
            cached = data["results"]
    todo = [j for j in nav.jumps if j.key not in cached]
    if todo:
        engine = VizDoomEngine(iwad, map_name, pwad=pwad)
        engine.game.add_game_args("+sv_cheats 1")
        engine.start()
        path.parent.mkdir(parents=True, exist_ok=True)

        def save():
            path.write_text(json.dumps({"version": VERSION, "results": cached}, indent=0))
        try:
            for i, j in enumerate(todo, 1):
                try:
                    cached[j.key] = _test_jump(nav, engine, j)
                except _LevelEnded:
                    cached[j.key] = UNVERIFIED  # e.g. the sprint crossed an exit line
                if i % 25 == 0:
                    save()
                    if log:
                        log(f"  verified {i}/{len(todo)} jumps")
        finally:
            engine.close()
            save()
    return {j.key: cached[j.key] for j in nav.jumps}


def simulate_jumps(nav: Navigator) -> dict[str, str]:
    """verify_jumps' results from the movement simulator instead of the game: the same
    test, a few ms a jump (it agrees with the engine on ~98% of them)."""
    sim = PlayerSim(nav)
    out = {}
    for j in nav.jumps:
        r = _test_jump(nav, sim, j)
        if r in (SOFT, OK):
            # made the one way that works with a little slack every way round (coasting from
            # the edge if that does, else sprinting all the way), or not at all
            r = next((SOFT if soft else OK for soft in ((True, False) if r == SOFT else (False,))
                      if _works(nav, sim, j, soft)
                      and all(_works(nav, sim, v, soft, reach=True) for v in _variants(nav, j))), FAIL)
        elif r != FAIL and not _robust(nav, sim, j):
            r = FAIL
        out[j.key] = r
    return out


def _works(nav: Navigator, sim, j, soft: bool, reach: bool = False) -> bool:
    """Does the jump stick, made soft (or not)? With `reach`: does it get there at all
    (sliding on after touching down is all right, not falling short: what a little off
    changes is mostly whether it stops on the landing or slides a bit, E4M6)."""
    heading = _prepare(nav, sim, j)
    if heading is None:
        return reach  # (a variant that can't be set up doesn't count against the jump)
    result, at = _fly(nav, sim, j, heading, soft)
    if reach and result == "missed" and math.dist(at, j.runup) < 4:
        return True  # (nor one that can't get going: its run-up backed into a wall, say)
    return result in (("stuck", "slid") if reach else ("stuck",))


def _variants(nav: Navigator, j) -> list:
    """The jump a little off how the check makes it: half a unit to either side of its line,
    and from a run-up ROBUST_RUNUP further back or nearer (taking off a little faster or
    slower). Nobody lines up to a fraction of a unit, so a jump has to survive these."""
    (rx, ry), (tx, ty) = j.runup, j.takeoff
    length = math.hypot(tx - rx, ty - ry)
    if length < 1e-6:
        return []
    ux, uy = (tx - rx) / length, (ty - ry) / length
    px, py = -uy, ux
    out = []
    for s in (ROBUST_SHIFT, -ROBUST_SHIFT):
        out.append(replace(j, runup=(rx + px * s, ry + py * s), takeoff=(tx + px * s, ty + py * s),
                           dest=(j.dest[0] + px * s, j.dest[1] + py * s)))
    for d in (-ROBUST_RUNUP, ROBUST_RUNUP):
        if length - d > 1:
            out.append(replace(j, runup=(rx + ux * d, ry + uy * d)))
    return [v for v in out if nav.mesh.poly_at(*v.runup) is not None]


ROBUST_SHIFT = 0.5  # a jump has to work this far to either side of its line too (1 was too strict: E4M6, MAP24)
ROBUST_RUNUP = 2.0  # ...and get there from a run-up this much longer or shorter (4 lost E4M2's jumps onto narrow ledges)


def _robust(nav: Navigator, sim, j) -> bool:
    """Does the jump still work a unit to either side? Nobody stands to a fraction of a
    unit: MAP09's jump through a doorway off a lift glanced off the jamb from 0.4 units over
    from where the check stood. (3 units was too strict: E4M2's and E4M6's needed jumps.)"""
    (rx, ry), (tx, ty) = j.runup, j.takeoff
    length = math.hypot(tx - rx, ty - ry)
    if length < 1e-6:
        return True
    px, py = -(ty - ry) / length, (tx - rx) / length
    for s in (ROBUST_SHIFT, -ROBUST_SHIFT):
        shifted = replace(j, runup=(rx + px * s, ry + py * s), takeoff=(tx + px * s, ty + py * s),
                          dest=(j.dest[0] + px * s, j.dest[1] + py * s))
        if nav.mesh.poly_at(*shifted.runup) is None or _test_jump(nav, sim, shifted) == FAIL:
            return False
    return True


def keep_verified(nav: Navigator, iwad: str | Path, map_name: str,
                  pwad: str | Path | None = None, log=None,
                  engine: bool = False) -> dict[str, int]:
    """Drop the jumps the check disproves (unverified ones are kept). Returns counts. The
    check runs in the movement simulator, or with `engine` in ViZDoom (slow; cached)."""
    if engine:
        results = verify_jumps(nav, iwad, map_name, pwad, log=log)
    else:
        results = simulate_jumps(nav)
    # One that slides off its landing is only kept when there's no better way across: one
    # that stops there, or one that runs straight across it (the strip isn't for stopping on).
    sticks = {(frozenset(j.src_polys), nav.mesh.polys[j.dest_poly].sector)
              for j in nav.jumps if results[j.key] in (OK, SOFT) or "@" in results[j.key]}
    keep = []
    for j in nav.jumps:
        r = results[j.key]
        if "@" in r:
            # run straight across the strip it lands on, to where the engine stopped
            x, y = map(float, r.split("@")[1].split(","))
            end = nav.mesh.poly_at(x, y)
            if end is not None:
                keep.append(replace(j, dest_poly=end.index, dest=(x, y), through=True))
        r = r.split("@")[0]
        if r == FAIL or r == SLID and (frozenset(j.src_polys),
                                     nav.mesh.polys[j.dest_poly].sector) in sticks:
            continue
        j.soft = r == SOFT
        keep.append(j)
    nav.set_jumps(keep)
    counts = {OK: 0, SOFT: 0, SLID: 0, FAIL: 0, UNVERIFIED: 0}
    for r in results.values():
        counts[r.split("@")[0]] += 1
    return counts


# --- one test -----------------------------------------------------------------------------

class _LevelEnded(Exception):
    """The episode ended mid-test (an exit line, or death)."""


def _step(engine: VizDoomEngine, controls: Controls) -> None:
    engine.step(controls)
    if engine.finished:
        raise _LevelEnded

def _test_jump(nav: Navigator, engine: VizDoomEngine, j: Jump) -> str:
    """Try the jump soft (full speed only up to the edge, then coast) and, if that doesn't
    stick, pushing all the way. Landing spots can be narrow strips: touching one isn't enough,
    the player has to stop on it."""
    pushed = FAIL
    for soft in (True, False):
        heading = _prepare(nav, engine, j)
        if heading is None:
            return UNVERIFIED
        result, _ = _fly(nav, engine, j, heading, soft)
        if result == "stuck":
            return SOFT if soft else OK
        if result == "slid" and not soft:
            pushed = SLID
    if pushed in (SLID, FAIL):
        # Too narrow to stop on: maybe it's meant to be run straight across, onto the next
        # floor (MAP14's strips). Recorded as "slid@x,y" (or "fail@x,y"), where we stopped.
        heading = _prepare(nav, engine, j)
        if heading is not None:
            result, at = _fly(nav, engine, j, heading, False, through=True)
            if result == "stuck":
                return f"{pushed}@{at[0]:.0f},{at[1]:.0f}"
    return pushed


def _prepare(nav: Navigator, engine: VizDoomEngine, j: Jump) -> float | None:
    """Set up the heights the jump needs and stand at its run-up facing the takeoff; the
    heading, or None if that can't be done. `engine` may be a PlayerSim: the heights are just
    set then."""
    if isinstance(engine, PlayerSim):
        # Like _establish in the game: the trigger that makes a required height moves every
        # sector it tags (E4M6: two posts by a takeoff lower together; with only the required
        # one lowered, the box caught the other).
        heights = dict(enumerate(nav.movers.initial))
        for sector, h in j.requires:
            if h == nav.movers.initial[sector]:
                continue
            t = next((t for t in nav.triggers if t.line >= 0 and t.permanent
                      and h in t.effects.get(sector, [])), None)
            if t is not None:
                for s, hs in t.effects.items():
                    heights[s] = hs[0]
        engine.heights = heights | dict(j.requires)
        engine.place(round(j.runup[0]), round(j.runup[1]))
    else:
        engine.restart()
        engine.game.send_game_command("god")
        engine.game.send_game_command("give all")
        for sector, heights in j.requires:
            if heights == nav.movers.initial[sector]:
                continue  # already like that at the start
            if not _establish(nav, engine, sector, heights):
                return None
        engine.game.send_game_command(f"warp {j.runup[0]:.0f} {j.runup[1]:.0f}")
    for _ in range(4):
        _step(engine, Controls())
    o = engine.observe()
    if math.dist((o.x, o.y), j.runup) > 4:
        return None  # couldn't stand at the run-up (e.g. it's on a moving sector)
    heading = math.degrees(math.atan2(j.takeoff[1] - j.runup[1], j.takeoff[0] - j.runup[0]))
    for _ in range(20):
        diff = _norm(heading - engine.observe().angle)
        if abs(diff) < 1:
            break
        _step(engine, Controls(turn=max(-25, min(25, -diff))))
    return heading


def _fly(nav, engine, j: Jump, heading: float, soft: bool, through: bool = False):
    try:
        return _fly_once(nav, engine, j, heading, soft, through)
    except Teleported:  # the sim doesn't follow teleports (the engine check spots the jump)
        o = engine.observe()
        return "missed", (o.x, o.y)


def _fly_once(nav: Navigator, engine: VizDoomEngine, j: Jump, heading: float, soft: bool,
         through: bool = False) -> tuple[str, tuple[float, float]]:
    """Sprint (as RouteFollower does), then brake on touchdown: "stuck" if we stop on the
    landing sector, "slid" if we touched it but slid off, "missed" otherwise; and where we
    ended up. `through`: keep sprinting across the landing sector and land on whatever comes
    next (no lower than a step below it)."""
    edge = math.dist(j.runup, j.takeoff)
    flight = edge + math.dist(j.takeoff, j.dest)
    ux, uy = math.cos(math.radians(heading)), math.sin(math.radians(heading))
    want = nav.mesh.polys[j.dest_poly].sector
    src = {nav.mesh.polys[p].sector for p in j.src_polys}
    motion = Motion()
    o = engine.observe()
    last = (o.x, o.y)
    touched = None
    crossed = False   # been over the landing sector (for `through`)
    for i in range(6 * TICRATE):
        o = engine.observe()
        motion.observe(o)
        if math.dist((o.x, o.y), last) > 64:
            return "missed", last  # teleported somewhere else
        last = (o.x, o.y)
        here = nav.mesh.poly_at(o.x, o.y)
        # Only past the takeoff (as RouteFollower counts it): the landing sector can be the
        # run-up's own, on the far side of a gap (E4M3: across a teleporter pad).
        past = (o.x - j.takeoff[0]) * ux + (o.y - j.takeoff[1]) * uy > 0
        crossed = crossed or (past and here is not None and here.sector == want)
        if touched is None and past and here is not None \
                and abs(o.z - o.heights[here.sector][0]) < 1:
            if not through and here.sector == want:
                touched = i
            elif through and crossed and here.sector not in src | {want} \
                    and o.heights[here.sector][0] >= o.heights[want][0] - MAX_STEP:
                touched, want = i, here.sector
        if touched is not None:
            if motion.speed < 0.3 or i - touched > 2 * TICRATE:
                return ("stuck" if here is not None and here.sector == want else "slid"), last
            _step(engine, motion.drive(o, (0.0, 0.0), heading))
            continue
        flown = (o.x - j.runup[0]) * ux + (o.y - j.runup[1]) * uy
        if flown >= flight - 8 and i > 4 * TICRATE:
            break
        push = through or flown < (edge if soft else flight - 8)
        _step(engine, Controls(forward=50 if push else 0,
                               turn=max(-3, min(3, -_norm(heading - o.angle)))))
    return "missed", last


def _establish(nav: Navigator, engine: VizDoomEngine, sector: int, heights) -> bool:
    """Fire a trigger that leaves `sector` at `heights` and wait for it. Only permanent
    changes (floors, stairs, ceilings, stay-open doors) can be set up reliably."""
    t = next((t for t in nav.triggers if t.line >= 0 and heights in t.effects.get(sector, [])
              and t.permanent), None)
    if t is None:
        return False
    m = nav.map
    line = m.linedefs[t.line]
    a, b = m.vertexes[line.v1], m.vertexes[line.v2]
    mid = ((a.x + b.x) / 2, (a.y + b.y) / 2)
    length = max(math.dist((a.x, a.y), (b.x, b.y)), 1)
    nx, ny = (b.y - a.y) / length, -(b.x - a.x) / length   # front side
    stand = (mid[0] + nx * 32, mid[1] + ny * 32)
    engine.game.send_game_command(f"warp {stand[0]:.0f} {stand[1]:.0f}")
    for _ in range(4):
        _step(engine, Controls())
    walk = t.info.trigger in (Trigger.W1, Trigger.WR)
    shoot = t.info.trigger in (Trigger.G1, Trigger.GR)
    target = (mid[0] - nx * 32, mid[1] - ny * 32) if walk else mid
    for i in range(6 * TICRATE):
        o = engine.observe()
        f, c = o.heights[sector]
        if abs(f - heights[0]) < 1 and abs(c - heights[1]) < 1:
            return True
        if i < 2 * TICRATE:
            diff = _norm(math.degrees(math.atan2(target[1] - o.y, target[0] - o.x)) - o.angle)
            controls = Controls(turn=max(-25, min(25, -diff)))
            if abs(diff) < 4:
                controls.forward = 25 if walk else 0
                controls.use = not walk and not shoot and i % 15 == 0
                controls.attack = shoot and i % 10 == 0
            _step(engine, controls)
        else:
            _step(engine, Controls())
    return False

