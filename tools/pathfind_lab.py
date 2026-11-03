"""Pathfinding lab: play a level yourself and send the bot to anything reachable.

  python tools/pathfind_lab.py doom E1M3
  python tools/pathfind_lab.py doom2 MAP01 --monsters

Left: the game. You play with the keyboard (and mouse, once you click the view).
Right: buttons for every item and switch reachable from where you stand right now, using live
sector heights and the keys you hold (doors you can open count as passable). Click one and the
bot walks there from wherever you are; for switches it stops facing the switch so you can
press it yourself. Your controls are locked while the bot drives; Cancel (or Esc) stops it.

Keys: W/S move, A/D strafe, Left/Right turn, E or Space use, Ctrl or left mouse fire,
      Shift walk (run is the default), click the view to capture the mouse, Esc releases it,
      R restarts the level after it ends.
"""
import argparse
import math
import sys
from dataclasses import dataclass

import pygame

from evildoom.config import resolve_wad
from evildoom.engine.vizdoom_engine import VizDoomEngine
from evildoom.motor.follower import TICRATE, Controls, RouteFollower
from evildoom.nav.reach import Navigator
from evildoom.nav.walkthrough import Step
from evildoom.wad.mapdata import load_map
from evildoom.wad.specials import Category, Key, Trigger
from evildoom.wad.wad import Wad

GAME_W, GAME_H = 800, 600
PANEL_W = 380
ROW_H = 22
MINIMAP_H = 230
PICKUP_CATEGORIES = {"Weapon", "Ammo", "Health", "Armor", "Key", "Powerup"}
BONUSES = {"HealthBonus", "ArmorBonus"}
KEY_NAMES = {"BlueCard": Key.BLUE_CARD, "YellowCard": Key.YELLOW_CARD, "RedCard": Key.RED_CARD,
             "RedSkull": Key.RED_SKULL, "YellowSkull": Key.YELLOW_SKULL,
             "BlueSkull": Key.BLUE_SKULL}
SWITCH_TRIGGERS = {Trigger.S1, Trigger.SR, Trigger.G1, Trigger.GR}
CATEGORY_COLORS = {"Key": (255, 220, 60), "Weapon": (120, 200, 255), "Powerup": (220, 120, 255),
                   "Armor": (120, 255, 140), "Health": (255, 120, 120), "Ammo": (200, 200, 200),
                   "Switch": (255, 170, 60), "Exit": (255, 90, 200)}
BG, FG, DIM = (24, 24, 28), (230, 230, 230), (140, 140, 150)


@dataclass
class Target:
    label: str
    kind: str                       # an object category, "Switch" or "Exit"
    goal: tuple[float, float]
    distance: float
    trigger: object = None          # TriggerLine for switches


class Lab:
    def __init__(self, wad: str, map_name: str, monsters: bool):
        iwad = resolve_wad(wad)
        self.map = load_map(Wad(iwad), map_name)
        self.nav = Navigator(self.map)
        self.engine = VizDoomEngine(iwad, map_name, nomonsters=not monsters,
                                    size=f"{GAME_W}x{GAME_H}", rgb=True, objects=True)
        self.engine.start()
        self.follower: RouteFollower | None = None
        self.status = "idle - click a target, or play"
        self.targets: list[Target] = []
        self.scroll = 0
        self.show_bonuses = False
        self.mouse_captured = False
        self.path_points: list[tuple[float, float]] = []
        self.map_keys = {Key(t.type) for t in self.map.things
                         if t.type in Key._value2member_map_ and t.in_skill(3)}

        pygame.init()
        pygame.display.set_caption(f"Evil DOOM pathfinding lab - {map_name}")
        self.screen = pygame.display.set_mode((GAME_W + PANEL_W, GAME_H))
        self.font = pygame.font.SysFont("consolas", 14)
        self.small = pygame.font.SysFont("consolas", 12)
        self.clock = pygame.time.Clock()
        self._minimap_setup()

    # --- world queries --------------------------------------------------------------------

    def keys_held(self, state) -> set[Key]:
        present = {KEY_NAMES[o.name] for o in state.objects if o.name in KEY_NAMES}
        return self.map_keys - present

    def refresh_targets(self) -> None:
        state = self.engine.state()
        if state is None:
            return
        obs = self.engine.observe()
        nav, pos = self.nav, (obs.x, obs.y)
        here = nav.mesh.poly_at(*pos)
        if here is None:
            return
        live = {s: {h} for s, h in obs.heights.items()}
        doors = nav.local_doors(self.keys_held(state))
        reach = nav.reachable(here.index, live, doors)
        targets = []
        for o in state.objects:
            if o.category not in PICKUP_CATEGORIES:
                continue
            if o.name in BONUSES and not self.show_bonuses:
                continue
            spots = [s for p, s in nav.pickup_spots(o.position_x, o.position_y, live)
                     if p in reach]
            if spots:
                goal = min(spots, key=lambda s: math.dist(s, pos))
                targets.append(Target(o.name, o.category, goal, math.dist(goal, pos)))
        for t in nav.triggers:
            if t.line < 0 or not t.activators & reach:
                continue
            is_exit = t.info.category in (Category.EXIT, Category.SECRET_EXIT)
            if t.info.trigger not in SWITCH_TRIGGERS and not is_exit:
                continue
            goal = nav._activation_point(t, pos, reach)
            kind = "Exit" if is_exit else "Switch"
            targets.append(Target(f"{t.info.trigger.value} line {t.line}: {t.info.description}",
                                  kind, goal, math.dist(goal, pos), t))
        self.targets = sorted(targets, key=lambda t: t.distance)
        self.scroll = min(self.scroll, max(0, len(self.targets) - 1))

    def go_to(self, target: Target) -> None:
        state = self.engine.state()
        obs = self.engine.observe()
        live = {s: {h} for s, h in obs.heights.items()}
        doors = self.nav.local_doors(self.keys_held(state))
        path = self.nav.find_path((obs.x, obs.y), target.goal, live, doors)
        if path is None:
            self.status = f"no path to {target.label}"
            return
        action = "face" if target.trigger is not None else "pickup"
        step = Step(path, action, target.trigger, label=target.label)
        self.follower = RouteFollower(self.nav, [step])
        self.path_points = [w for w in path.waypoints if w]
        notes = f" ({'; '.join(path.steps)})" if path.steps else ""
        self.status = f"bot -> {target.label}{notes}"

    def cancel(self, why: str = "cancelled") -> None:
        self.follower = None
        self.status = why
        self.refresh_targets()

    # --- input ----------------------------------------------------------------------------

    def human_controls(self) -> Controls:
        k = pygame.key.get_pressed()
        c = Controls()
        c.forward = 50.0 * (k[pygame.K_w] - k[pygame.K_s]) + 50.0 * (k[pygame.K_UP] - k[pygame.K_DOWN])
        c.forward = max(-50.0, min(50.0, c.forward))
        c.strafe = 40.0 * (k[pygame.K_d] - k[pygame.K_a])
        c.turn = 6.0 * (k[pygame.K_RIGHT] - k[pygame.K_LEFT])
        if self.mouse_captured:
            c.turn += pygame.mouse.get_rel()[0] * 0.25
            c.attack = pygame.mouse.get_pressed()[0]
        c.use = bool(k[pygame.K_e] or k[pygame.K_SPACE])
        c.attack = c.attack or bool(k[pygame.K_LCTRL] or k[pygame.K_RCTRL])
        c.run = not (k[pygame.K_LSHIFT] or k[pygame.K_RSHIFT])
        return c

    def set_capture(self, on: bool) -> None:
        self.mouse_captured = on
        pygame.event.set_grab(on)
        pygame.mouse.set_visible(not on)
        pygame.mouse.get_rel()

    def handle_click(self, x: int, y: int) -> None:
        if x < GAME_W:
            if self.follower is None:
                self.set_capture(True)
            return
        for rect, action in self.buttons:
            if rect.collidepoint(x, y):
                action()
                return

    # --- drawing --------------------------------------------------------------------------

    def _minimap_setup(self) -> None:
        xs = [v.x for v in self.map.vertexes]
        ys = [v.y for v in self.map.vertexes]
        self.mm_rect = pygame.Rect(GAME_W + 10, GAME_H - MINIMAP_H - 10, PANEL_W - 20, MINIMAP_H)
        self.mm_scale = min(self.mm_rect.w / (max(xs) - min(xs) or 1),
                            self.mm_rect.h / (max(ys) - min(ys) or 1)) * 0.95
        self.mm_origin = (min(xs), max(ys))

    def to_minimap(self, x: float, y: float) -> tuple[int, int]:
        return (int(self.mm_rect.x + 4 + (x - self.mm_origin[0]) * self.mm_scale),
                int(self.mm_rect.y + 4 + (self.mm_origin[1] - y) * self.mm_scale))

    def button(self, rect: pygame.Rect, text: str, action, color=FG, fill=(50, 50, 60)) -> None:
        pygame.draw.rect(self.screen, fill, rect, border_radius=3)
        self.screen.blit(self.font.render(text, True, color), (rect.x + 6, rect.y + 3))
        self.buttons.append((rect, action))

    def draw(self, state) -> None:
        self.screen.fill(BG)
        self.buttons = []
        if state is not None:
            buf = state.screen_buffer
            frame = pygame.image.frombuffer(buf.tobytes(), (buf.shape[1], buf.shape[0]), "RGB")
            self.screen.blit(frame, (0, 0))
        else:
            self.screen.blit(self.font.render("level ended - press R to restart", True, FG),
                             (20, GAME_H // 2))
        if self.follower is not None:
            pygame.draw.rect(self.screen, (255, 60, 60), (0, 0, GAME_W, GAME_H), 3)

        x0 = GAME_W + 10
        self.screen.blit(self.font.render(f"{self.map.name}  -  pathfinding lab", True, FG),
                         (x0, 8))
        for i, line in enumerate(_wrap(self.status, 48)[:3]):
            self.screen.blit(self.small.render(line, True, (255, 210, 120)), (x0, 28 + i * 14))
        y = 74
        self.button(pygame.Rect(x0, y, 80, 20), "Cancel", lambda: self.cancel(),
                    fill=(110, 40, 40) if self.follower else (50, 50, 60))
        self.button(pygame.Rect(x0 + 88, y, 80, 20), "Refresh", self.refresh_targets)
        self.button(pygame.Rect(x0 + 176, y, 180, 20),
                    f"Bonuses: {'shown' if self.show_bonuses else 'hidden'}", self.toggle_bonuses)
        y += 28
        self.screen.blit(self.small.render(f"{len(self.targets)} reachable targets (wheel to "
                                           f"scroll)", True, DIM), (x0, y))
        y += 16
        list_bottom = self.mm_rect.y - 8
        rows = (list_bottom - y) // ROW_H
        for t in self.targets[self.scroll:self.scroll + rows]:
            color = CATEGORY_COLORS.get(t.kind, FG)
            text = f"{t.distance:5.0f}  {t.label}"[:46]
            self.button(pygame.Rect(x0, y, PANEL_W - 20, ROW_H - 2), text,
                        lambda t=t: self.go_to(t), color=color, fill=(38, 38, 46))
            y += ROW_H

        self.draw_minimap(state)
        pygame.display.flip()

    def draw_minimap(self, state) -> None:
        pygame.draw.rect(self.screen, (12, 12, 14), self.mm_rect)
        m = self.map
        for line in m.linedefs:
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            color = (90, 90, 100) if line.two_sided else (200, 200, 210)
            pygame.draw.line(self.screen, color, self.to_minimap(a.x, a.y),
                             self.to_minimap(b.x, b.y))
        for t in self.targets:
            pygame.draw.circle(self.screen, CATEGORY_COLORS.get(t.kind, FG),
                               self.to_minimap(*t.goal), 2)
        if self.follower is not None and len(self.path_points) > 1:
            pygame.draw.lines(self.screen, (255, 60, 60), False,
                              [self.to_minimap(*p) for p in self.path_points], 2)
        if state is not None:
            obs = self.engine.observe()
            px, py = self.to_minimap(obs.x, obs.y)
            a = math.radians(obs.angle)
            pygame.draw.circle(self.screen, (80, 255, 80), (px, py), 4)
            pygame.draw.line(self.screen, (80, 255, 80), (px, py),
                             (px + 10 * math.cos(a), py - 10 * math.sin(a)), 2)

    def toggle_bonuses(self) -> None:
        self.show_bonuses = not self.show_bonuses
        self.refresh_targets()

    # --- main loop ------------------------------------------------------------------------

    def run(self) -> None:
        self.refresh_targets()
        refresh_every = TICRATE
        while True:
            for event in pygame.event.get():
                if event.type == pygame.QUIT:
                    return
                if event.type == pygame.KEYDOWN and event.key == pygame.K_ESCAPE:
                    if self.follower is not None:
                        self.cancel()
                    else:
                        self.set_capture(False)
                if event.type == pygame.KEYDOWN and event.key == pygame.K_r \
                        and self.engine.finished:
                    self.engine.restart()
                    self.cancel("level restarted")
                if event.type == pygame.MOUSEBUTTONDOWN and event.button == 1:
                    if not (self.mouse_captured and event.pos[0] < GAME_W):
                        self.handle_click(*event.pos)
                if event.type == pygame.MOUSEWHEEL:
                    self.scroll = max(0, min(len(self.targets) - 1, self.scroll - event.y * 3))

            if not self.engine.finished:
                if self.follower is not None:
                    controls = self.follower.act(self.engine.observe())
                    if self.follower.done:
                        self.cancel(f"arrived: {self.status.removeprefix('bot -> ')}")
                    elif self.follower.failed:
                        self.cancel("bot gave up: " + self.follower.log.events[-1])
                    if self.mouse_captured:
                        pygame.mouse.get_rel()  # discard mouse motion while the bot drives
                else:
                    controls = self.human_controls()
                self.engine.step(controls)
                if self.follower is None and self.engine.tic % refresh_every == 0:
                    self.refresh_targets()
            self.draw(None if self.engine.finished else self.engine.state())
            self.clock.tick(TICRATE)


def _wrap(text: str, width: int) -> list[str]:
    words, lines, cur = text.split(), [], ""
    for w in words:
        if len(cur) + len(w) + 1 > width:
            lines.append(cur)
            cur = w
        else:
            cur = f"{cur} {w}".strip()
    return lines + [cur] if cur else lines


def main() -> None:
    ap = argparse.ArgumentParser(description="Interactive pathfinding test bench")
    ap.add_argument("wad", help="IWAD short name (doom, doom2, tnt, plutonia) or path")
    ap.add_argument("map", help="map name, e.g. E1M1 or MAP01")
    ap.add_argument("--monsters", action="store_true", help="keep monsters (default: none)")
    args = ap.parse_args()
    lab = Lab(args.wad, args.map.upper(), args.monsters)
    try:
        lab.run()
    finally:
        lab.engine.close()
        pygame.quit()


if __name__ == "__main__":
    sys.exit(main())
