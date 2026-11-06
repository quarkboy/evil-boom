"""DSDA-Doom as Evil's engine: the game in-process (evildoom_native.DsdaGame over the library
built from dsda/evil_api.c and DSDA-Doom's sources), a tic at a time, nothing drawn.

Same interface as VizDoomEngine (start, observe, step, restart, finished/dead/exited, god),
so the explorer and the fight run on either. One game a process: DSDA-Doom is all globals,
so sweeps give each map a process of its own."""
from __future__ import annotations

import os
import re
import sys
from pathlib import Path

from evildoom.motor.follower import Controls, Observation, Thing
from evildoom.tactical.bestiary import category
from evildoom.wad.specials import Key

ROOT = Path(__file__).resolve().parents[3]
BUILD = ROOT / "third_party" / "dsda-doom" / "prboom2" / "build"
LIBRARY = BUILD / ("libdsda-evil.dll" if sys.platform == "win32" else "libdsda-evil.so")
MINGW_BIN = Path(os.environ.get("EVIL_MINGW_BIN", r"C:\msys64\mingw64\bin"))  # (SDL2 and the rest)
WARMUP_TICS = 8  # (as with ViZDoom: the level loads on the first, then a few idle tics)
KEYS = (Key.BLUE_CARD, Key.YELLOW_CARD, Key.RED_CARD, Key.BLUE_SKULL, Key.YELLOW_SKULL, Key.RED_SKULL)
SKIP_THINGS = {"DoomPlayer", "BossTarget"}


def _episode_map(map_name: str) -> tuple[int, int]:
    m = re.fullmatch(r"E(\d)M(\d)", map_name.upper())
    if m:
        return int(m.group(1)), int(m.group(2))
    m = re.fullmatch(r"MAP(\d\d)", map_name.upper())
    if m:
        return 1, int(m.group(1))
    raise ValueError(f"not a map name: {map_name}")


class DsdaEngine:
    def __init__(self, iwad: str | Path, map_name: str, *, skill: int = 3, nomonsters: bool = True,
                 objects: bool = False, library: str | Path = LIBRARY):
        import evildoom_native
        if sys.platform == "win32" and MINGW_BIN.is_dir():
            os.add_dll_directory(str(MINGW_BIN))  # (Python doesn't look in PATH for a DLL's DLLs)
        os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
        os.environ.setdefault("SDL_AUDIODRIVER", "dummy")
        os.environ.setdefault("DOOMWADDIR", str(BUILD))  # (dsda-doom.wad, beside the library)
        data = ROOT / "out" / "dsda"  # (DSDA-Doom's own files: config, per-WAD data)
        data.mkdir(parents=True, exist_ok=True)
        os.environ.setdefault("DOOMDATADIR", str(data))
        self.game = evildoom_native.DsdaGame(str(library), [
            "-iwad", str(iwad), "-nodraw", "-nosound", "-nomusic", "-skill", str(skill)]
            + ["-nomonsters"] * nomonsters)  # (as the game sets it: a new game resets it from this)
        self.map_name = map_name
        self.episode, self.map = _episode_map(map_name)
        self.skill = skill
        self.nomonsters = nomonsters
        self.objects = objects
        self.tic = 0
        self._god = False
        self._suits = 0
        self._ironfeet = 0

    def start(self) -> None:
        self._new_game()
        self.tic = WARMUP_TICS

    def restart(self, keep_time: bool = False) -> None:
        """The level from the start again (`keep_time`: the tic count carries on)."""
        tic = self.tic
        self._new_game()
        self.tic = tic + WARMUP_TICS if keep_time else WARMUP_TICS

    def _new_game(self) -> None:
        self.game.new_game(self.skill, self.episode, self.map, self.nomonsters)
        self._suits = self._ironfeet = 0
        for _ in range(WARMUP_TICS):
            self.game.tic(0, 0, 0, False, False, 0)
        if self._god:
            self.game.god(True)

    def god(self) -> None:
        """Damage does nothing to her (kept across restarts)."""
        self._god = True
        self.game.god(True)

    @property
    def exited(self) -> bool:
        return self.game.status()[2]

    @property
    def dead(self) -> bool:
        return self.game.player()["dead"]

    @property
    def finished(self) -> bool:
        """The level was exited or the player died."""
        return self.exited or self.dead

    def observe(self) -> Observation:
        p = self.game.player()
        heights = dict(enumerate(self.game.sectors()))
        things = barrels = None
        raw = self.game.things()
        barrels = tuple((t[2], t[3]) for t in raw if t[1] == "ExplosiveBarrel")
        if self.objects:
            things = tuple(Thing(i, name, category(name), x, y, z, vx, vy, vz, angle)
                           for i, name, x, y, z, vx, vy, vz, angle in raw
                           if name not in SKIP_THINGS and category(name) != "Other")
        if p["ironfeet"] > self._ironfeet:
            self._suits += 1  # (a suit picked up: its time went up)
        self._ironfeet = p["ironfeet"]
        ammo = dict(p["ammo"])
        return Observation(p["x"], p["y"], p["angle"], heights, self.tic,
                           frozenset(k for k, held in zip(KEYS, p["keys"]) if held), p["z"],
                           p["weapon"], barrels, float(p["health"]), self._suits, things,
                           frozenset(p["weapons"]), ammo, float(p["armor"]), p["ssg"],
                           (p["momx"], p["momy"]))

    def step(self, c: Controls) -> None:
        scale = 1.0 if c.run else 0.5
        self.game.tic(c.forward * scale, c.strafe * scale, c.turn, c.attack, c.use, c.weapon)
        self.tic += 1

    def close(self) -> None:
        pass  # (the process ends with the game)
