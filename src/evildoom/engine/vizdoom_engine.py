"""ViZDoom adapter: Controls in, Observations out, with demo recording and playback."""
import sys
import time
from pathlib import Path

import vizdoom as vzd

if sys.platform == "win32":
    # ViZDoom sleeps between tics; at Windows' default 15.6 ms timer resolution every tic
    # costs a whole timer tick (only ~2x real time). 1 ms makes it ~3x faster.
    import ctypes
    ctypes.windll.winmm.timeBeginPeriod(1)

from evildoom.motor.follower import TICRATE, Controls, Observation
from evildoom.wad.specials import Key

BUTTONS = [vzd.Button.MOVE_FORWARD_BACKWARD_DELTA, vzd.Button.MOVE_LEFT_RIGHT_DELTA,
           vzd.Button.TURN_LEFT_RIGHT_DELTA, vzd.Button.USE, vzd.Button.ATTACK,
           vzd.Button.SPEED]
VARIABLES = [vzd.GameVariable.POSITION_X, vzd.GameVariable.POSITION_Y,
             vzd.GameVariable.ANGLE, vzd.GameVariable.HEALTH, vzd.GameVariable.POSITION_Z]
WARMUP_TICS = 8  # ViZDoom ignores turn input for the first few tics of an episode
KEY_OBJECTS = {"BlueCard": Key.BLUE_CARD, "YellowCard": Key.YELLOW_CARD, "RedCard": Key.RED_CARD,
               "BlueSkull": Key.BLUE_SKULL, "YellowSkull": Key.YELLOW_SKULL,
               "RedSkull": Key.RED_SKULL}


class VizDoomEngine:
    def __init__(self, iwad: str | Path, map_name: str, *, pwad: str | Path | None = None,
                 skill: int = 3, nomonsters: bool = True, visible: bool = False,
                 size: str = "320x240", rgb: bool = False, objects: bool = False):
        """`size` is a ViZDoom resolution like "1024x768". Avoid 1280x960: it segfaults
        ViZDoom 1.3.2 on init (other sizes, including 1920x1080, are fine).
        `rgb` makes the screen buffer H x W x 3 RGB (for drawing it ourselves). The live
        object list is always on: keys held are worked out from key items that disappear
        (`objects` is kept for callers that want the list for other things)."""
        game = vzd.DoomGame()
        game.set_doom_game_path(str(iwad))
        if pwad:
            game.set_doom_scenario_path(str(pwad))
        game.set_doom_map(map_name)
        game.set_doom_skill(skill)
        game.set_mode(vzd.Mode.PLAYER)
        game.set_episode_timeout(0)
        game.set_window_visible(visible)
        game.set_screen_resolution(getattr(vzd.ScreenResolution, f"RES_{size.upper()}"))
        game.set_render_hud(True)
        game.set_render_crosshair(False)
        game.set_sectors_info_enabled(True)
        game.set_objects_info_enabled(True)
        if rgb:
            game.set_screen_format(vzd.ScreenFormat.RGB24)
        game.set_available_buttons(BUTTONS)
        game.set_available_game_variables(VARIABLES)
        if nomonsters:
            game.add_game_args("-nomonsters")
        # ViZDoom never times out ZDoom's "A secret is revealed!" message: it stays on screen
        # for the rest of the level (worst in demo replay). Turn it off.
        game.add_game_args("+cl_showsecretmessage 0")
        self.game = game
        self.tic = 0
        self._keys_at_start: dict[str, int] | None = None

    def start(self, record: str | Path | None = None) -> None:
        self.game.init()
        if record:
            Path(record).parent.mkdir(parents=True, exist_ok=True)
            self.game.new_episode(str(record))
        else:
            self.game.new_episode()
        self.game.make_action([0] * len(BUTTONS), WARMUP_TICS)
        self.tic = WARMUP_TICS
        self._keys_at_start = None

    @property
    def finished(self) -> bool:
        """The episode is over: the level was exited *or the player died*."""
        return self.game.is_episode_finished()

    @property
    def dead(self) -> bool:
        return self.game.is_player_dead()

    @property
    def exited(self) -> bool:
        return self.finished and not self.dead

    def observe(self) -> Observation:
        state = self.game.get_state()
        x, y, angle, _health, z = state.game_variables
        heights = {s.id: (s.floor_height, s.ceiling_height) for s in state.sectors}
        return Observation(float(x), float(y), float(angle), heights, self.tic,
                           self._keys_held(state), float(z))

    def _keys_held(self, state) -> frozenset:
        """Keys picked up so far: key items present at the start that have gone."""
        now: dict[str, int] = {}
        for o in state.objects:
            if o.name in KEY_OBJECTS:
                now[o.name] = now.get(o.name, 0) + 1
        if self._keys_at_start is None:
            self._keys_at_start = now
        return frozenset(KEY_OBJECTS[n] for n, c in self._keys_at_start.items()
                         if now.get(n, 0) < c)

    def state(self):
        """The raw ViZDoom GameState (screen buffer, objects, ...), or None after the end."""
        return self.game.get_state()

    def restart(self) -> None:
        self.game.new_episode()
        self.game.make_action([0] * len(BUTTONS), WARMUP_TICS)
        self.tic = WARMUP_TICS
        self._keys_at_start = None

    def step(self, c: Controls) -> None:
        self.game.make_action([c.forward, c.strafe, c.turn, float(c.use), float(c.attack),
                               float(c.run)])
        self.tic += 1

    def close(self) -> None:
        self.game.close()

    def replay(self, demo: str | Path, speed: float = 1.0) -> None:
        """Play a recorded demo back in a window at (a multiple of) real-time speed."""
        self.game.set_window_visible(True)
        self.game.set_render_weapon(True)
        self.game.init()
        self.game.replay_episode(str(demo))
        frame = 1.0 / (TICRATE * speed)
        next_t = time.perf_counter()
        tics = 0
        try:
            while not self.game.is_episode_finished():
                self.game.advance_action()
                tics += 1
                if tics % (10 * TICRATE) == 0:
                    print(f"  {tics / TICRATE:.0f}s of game time", flush=True)
                next_t += frame
                delay = next_t - time.perf_counter()
                if delay > 0:
                    time.sleep(delay)
                elif delay < -0.5:  # fell behind (window dragged etc.): don't sprint to catch up
                    next_t = time.perf_counter()
        except vzd.ViZDoomIsNotRunningException:
            print("  window closed")
            return
        print(f"  demo finished ({tics / TICRATE:.1f}s of game time)")
        self.game.close()
