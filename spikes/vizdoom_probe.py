"""Probe ViZDoom against local IWADs: does it load, and what state does it expose?

Usage: python spikes/vizdoom_probe.py <iwad_path> <map> [scenario_pwad]
"""
import sys
from collections import Counter

import vizdoom as vzd


def probe(iwad: str, map_name: str, pwad: str | None = None) -> None:
    game = vzd.DoomGame()
    game.set_doom_game_path(iwad)
    if pwad:
        game.set_doom_scenario_path(pwad)
    game.set_doom_map(map_name)
    game.set_window_visible(False)
    game.set_mode(vzd.Mode.PLAYER)
    game.set_objects_info_enabled(True)
    game.set_sectors_info_enabled(True)
    game.set_labels_buffer_enabled(True)
    game.set_automap_buffer_enabled(True)
    game.set_available_game_variables([
        vzd.GameVariable.HEALTH, vzd.GameVariable.ARMOR,
        vzd.GameVariable.POSITION_X, vzd.GameVariable.POSITION_Y,
        vzd.GameVariable.ANGLE, vzd.GameVariable.SELECTED_WEAPON,
    ])
    game.set_available_buttons([vzd.Button.MOVE_FORWARD])
    game.set_episode_timeout(0)
    game.init()
    game.new_episode()
    game.advance_action(5)

    state = game.get_state()
    print(f"loaded {iwad} {map_name} ok, tic={state.tic}")
    print("game vars (health, armor, x, y, angle, weapon):", list(state.game_variables))
    cats = Counter(o.category for o in state.objects)
    print(f"objects: {len(state.objects)} by category: {dict(cats)}")
    names = Counter(o.name for o in state.objects)
    print("object names:", dict(names.most_common(40)))
    lines = sum(len(s.lines) for s in state.sectors)
    print(f"sectors: {len(state.sectors)}, sector lines: {lines}, "
          f"blocking: {sum(l.is_blocking for s in state.sectors for l in s.lines)}")
    s0 = state.sectors[0]
    print(f"sector[0]: id={s0.id} floor={s0.floor_height} ceil={s0.ceiling_height}")
    print("visible labels:", [(l.object_name, l.object_category) for l in state.labels])
    game.close()


if __name__ == "__main__":
    probe(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)
