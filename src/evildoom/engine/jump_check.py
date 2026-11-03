"""Verify the navigator's run-off jumps in ViZDoom and keep only the ones that work.

The jump finder simulates DOOM physics, but small errors decide whether a jump clears a gap, so
each candidate is tried in the real engine: warp to its run-up, line up, sprint along the planned
line, and check that the player touches down on the predicted sector (standing on its floor)
without being teleported away first. Jumps that need moved heights (a lowered fence, a raised
floor) get those set up first by firing the trigger that produces them; when that isn't possible
or reliable (lifts and timed doors) the jump is kept but marked unverified.

Results are cached per map in out/jumpcache, keyed by Jump.key, so only new jumps are tested.
"""
import json
import math
from pathlib import Path

import vizdoom as vzd

from evildoom.motor.follower import TICRATE, Controls, _norm
from evildoom.nav.reach import Jump, Navigator, TriggerLine
from evildoom.wad.specials import Category, Trigger

from .vizdoom_engine import VizDoomEngine

CACHE_DIR = Path(__file__).resolve().parents[3] / "out" / "jumpcache"
VERSION = 1   # bump to invalidate cached results when the test procedure changes
OK, FAIL, UNVERIFIED = "ok", "fail", "unverified"


def _cache_path(iwad: Path, map_name: str, pwad: Path | None) -> Path:
    name = f"{Path(iwad).stem}_{Path(pwad).stem + '_' if pwad else ''}{map_name}.json"
    return CACHE_DIR / name


def verify_jumps(nav: Navigator, iwad: str | Path, map_name: str, pwad: str | Path | None = None,
                 use_cache: bool = True, log=None) -> dict[str, str]:
    """Return {jump key: "ok" | "fail" | "unverified"} for every jump the navigator has."""
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


def keep_verified(nav: Navigator, iwad: str | Path, map_name: str,
                  pwad: str | Path | None = None, log=None) -> dict[str, int]:
    """Drop the jumps the engine disproves (unverified ones are kept). Returns counts."""
    results = verify_jumps(nav, iwad, map_name, pwad, log=log)
    nav.set_jumps([j for j in nav.jumps if results[j.key] != FAIL])
    counts = {OK: 0, FAIL: 0, UNVERIFIED: 0}
    for r in results.values():
        counts[r] += 1
    return counts


# --- one test -----------------------------------------------------------------------------

class _LevelEnded(Exception):
    """The episode ended mid-test (an exit line, or death)."""


def _step(engine: VizDoomEngine, controls: Controls) -> None:
    engine.step(controls)
    if engine.finished:
        raise _LevelEnded

def _test_jump(nav: Navigator, engine: VizDoomEngine, j: Jump) -> str:
    engine.restart()
    engine.game.send_game_command("god")
    engine.game.send_game_command("give all")
    for sector, heights in j.requires:
        if heights == nav.movers.initial[sector]:
            continue  # already like that at the start
        if not _establish(nav, engine, sector, heights):
            return UNVERIFIED
    engine.game.send_game_command(f"warp {j.runup[0]:.0f} {j.runup[1]:.0f}")
    for _ in range(4):
        _step(engine, Controls())
    o = engine.observe()
    if math.dist((o.x, o.y), j.runup) > 4:
        return UNVERIFIED  # couldn't stand at the run-up (e.g. it's on a moving sector)
    heading = math.degrees(math.atan2(j.takeoff[1] - j.runup[1], j.takeoff[0] - j.runup[0]))
    for _ in range(20):
        diff = _norm(heading - engine.observe().angle)
        if abs(diff) < 1:
            break
        _step(engine, Controls(turn=max(-25, min(25, -diff))))
    flight = math.dist(j.runup, j.takeoff) + math.dist(j.takeoff, j.dest)
    ux, uy = math.cos(math.radians(heading)), math.sin(math.radians(heading))
    want = nav.mesh.polys[j.dest_poly].sector
    last = (o.x, o.y)
    for i in range(4 * TICRATE + 20):
        o = engine.observe()
        if math.dist((o.x, o.y), last) > 64:
            return FAIL  # teleported somewhere else
        last = (o.x, o.y)
        here = nav.mesh.poly_at(o.x, o.y)
        z = engine.game.get_game_variable(vzd.GameVariable.POSITION_Z)
        if here is not None and here.sector == want and abs(z - o.heights[want][0]) < 1:
            return OK
        flown = (o.x - j.runup[0]) * ux + (o.y - j.runup[1]) * uy
        if flown >= flight - 8 and i > 4 * TICRATE:
            break
        _step(engine, Controls(forward=50 if flown < flight - 8 else 0,
                             turn=max(-3, min(3, -_norm(heading - o.angle)))))
    return FAIL


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

