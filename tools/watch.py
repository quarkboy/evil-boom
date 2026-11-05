"""Watch the explorer play a map in real time, with what it knows beside the game.

  python tools/watch.py doom E1M1
  python tools/watch.py doom2 MAP02 --speed 2

Left: the game as the player sees it, and the explorer's log. Right: the map as Evil knows it
(mapped lines, known things, the route being followed, the objectives open to her), and the
status panel: what she's doing, the objectives she could pick, what she has found.

Keys: space pause, +/- speed, A show the whole map (faint: what she doesn't know), F follow
the player on the map, mouse wheel zoom the map, Esc quit.
"""
import argparse
import math
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

import pygame

from evildoom.config import resolve_wad
from evildoom.engine.jump_check import keep_verified
from evildoom.engine.vizdoom_engine import VizDoomEngine
from evildoom.knowledge.explorer import Explorer
from evildoom.knowledge.model import Knowledge
from evildoom.motor.follower import TICRATE
from evildoom.nav.reach import Navigator
from evildoom.wad.mapdata import ML_DONTDRAW, load_map
from evildoom.wad.specials import Category, Key, Lock
from evildoom.wad.things import PICKUPS
from evildoom.wad.wad import Wad

W, H = 1600, 900
GAME = pygame.Rect(0, 0, 800, 600)
LOG = pygame.Rect(0, 600, 800, 300)
MAP = pygame.Rect(800, 0, 800, 520)
PANEL = pygame.Rect(800, 520, 800, 380)
VIEW_SIZE = "640x480"

BG = (12, 12, 16)
DIM = (120, 120, 130)
TEXT = (220, 220, 220)
HEAD = (255, 210, 120)
KIND_COLOUR = {"exit": (255, 80, 255), "key": (255, 255, 0), "switch": (0, 220, 255),
               "door": (0, 160, 255), "frontier": (120, 255, 120), "visit": (80, 180, 80),
               "recheck": (200, 160, 60)}
ITEM_COLOUR = {"key": (255, 255, 0), "weapon": (255, 140, 0), "ammo": (180, 140, 90),
               "health": (80, 220, 80), "armor": (80, 160, 255), "powerup": (220, 120, 255)}
KEY_COLOUR = {"blue": (60, 120, 255), "yellow": (255, 230, 40), "red": (255, 50, 50)}


def key_colour(name: str):
    return next((c for k, c in KEY_COLOUR.items() if k in name.lower()), (255, 255, 0))


class Watch:
    def __init__(self, iwad: Path, map_name: str, speed: float, snapshot: float | None = None):
        self.map_name = map_name
        self.snapshot = snapshot
        self.m = load_map(Wad(iwad), map_name)
        self.nav = Navigator(self.m)
        keep_verified(self.nav, iwad, map_name)
        self.engine = VizDoomEngine(iwad, map_name, size=VIEW_SIZE, rgb=True)
        self.engine.game.add_game_args("+sv_cheats 1")
        self.engine.start()
        self.engine.game.send_game_command("god")
        width = int(VIEW_SIZE.split("x")[0])
        self.x = Explorer(self.nav, Knowledge(self.nav, width=width))
        self.speed = speed
        self.paused = False
        self.show_all = False
        self.follow = False
        self.zoom = 1.0
        self.controls = None
        self.obs = None
        self.options = []
        self.options_tic = -10**9
        xs = [v.x for v in self.m.vertexes]
        ys = [v.y for v in self.m.vertexes]
        self.bounds = (min(xs), min(ys), max(xs), max(ys))
        pygame.init()
        pygame.display.set_caption(f"evil-doom explorer: {map_name}")
        self.screen = pygame.display.set_mode((W, H))
        self.font = pygame.font.SysFont("consolas", 14)
        self.small = pygame.font.SysFont("consolas", 12)
        self.big = pygame.font.SysFont("consolas", 18, bold=True)
        self.clock = pygame.time.Clock()
        self.finished_at = None

    # --- running ------------------------------------------------------------------------

    def run(self) -> None:
        last_frame = time.perf_counter()
        owed = 0.0
        while True:
            for ev in pygame.event.get():
                if ev.type == pygame.QUIT or (ev.type == pygame.KEYDOWN and ev.key == pygame.K_ESCAPE):
                    self.close()
                    return
                if ev.type == pygame.KEYDOWN:
                    if ev.key == pygame.K_SPACE:
                        self.paused = not self.paused
                    elif ev.key in (pygame.K_PLUS, pygame.K_EQUALS, pygame.K_KP_PLUS):
                        self.speed = min(self.speed * 2, 16)
                    elif ev.key in (pygame.K_MINUS, pygame.K_KP_MINUS):
                        self.speed = max(self.speed / 2, 0.25)
                    elif ev.key == pygame.K_a:
                        self.show_all = not self.show_all
                    elif ev.key == pygame.K_f:
                        self.follow = not self.follow
                if ev.type == pygame.MOUSEWHEEL:
                    self.zoom = max(1.0, min(self.zoom * (1.25 if ev.y > 0 else 0.8), 20.0))
            now = time.perf_counter()
            owed += (now - last_frame) * TICRATE * self.speed
            last_frame = now
            owed = min(owed, 8)  # (don't race to catch up after a slow plan)
            while owed >= 1 and not self.paused and not self.engine.finished:
                self.tic()
                owed -= 1
            if self.paused:
                owed = 0
            self.draw()
            if self.snapshot is not None and self.obs is not None                     and (self.obs.tic >= self.snapshot * TICRATE or self.engine.finished):
                pygame.image.save(self.screen, "out/watch.png")
                self.close()
                return
            self.clock.tick(60)

    def tic(self) -> None:
        e = self.engine
        self.obs = e.observe()
        self.state = e.state()
        self.controls = self.x.act(self.obs, self.controls)
        e.step(self.controls)
        if e.finished and self.finished_at is None:
            self.finished_at = e.tic
        if self.obs.tic - self.options_tic >= TICRATE:
            self.options = self.x.objectives(self.obs)
            self.options_tic = self.obs.tic

    def close(self) -> None:
        self.engine.close()
        pygame.quit()

    # --- drawing ------------------------------------------------------------------------

    def text(self, s: str, pos, colour=TEXT, font=None) -> int:
        f = font or self.font
        self.screen.blit(f.render(s, True, colour), pos)
        return pos[1] + f.get_linesize()

    def draw(self) -> None:
        self.screen.fill(BG)
        self.draw_game()
        self.draw_log()
        self.draw_map()
        self.draw_panel()
        pygame.display.flip()

    def draw_game(self) -> None:
        st = getattr(self, "state", None)
        if st is None or st.screen_buffer is None:
            self.text("starting...", (20, 20))
            return
        buf = st.screen_buffer  # H x W x 3
        surf = pygame.surfarray.make_surface(buf.swapaxes(0, 1))
        self.screen.blit(pygame.transform.scale(surf, GAME.size), GAME.topleft)
        if self.engine.finished:
            label = "EXITED" if self.engine.exited else "GAME OVER"
            self.text(label, (GAME.x + 20, GAME.y + 20), (255, 255, 0), self.big)
        if self.paused:
            self.text("PAUSED", (GAME.right - 90, GAME.y + 10), (255, 255, 0), self.big)

    def draw_log(self) -> None:
        pygame.draw.rect(self.screen, (20, 20, 26), LOG)
        y = self.text("Log", (LOG.x + 8, LOG.y + 4), HEAD, self.big)
        lines = [l for l in self.x.log[-40:]]
        k_events = self.x.knowledge.events[-40:]
        merged = sorted(lines + k_events, key=lambda s: int(s.split(":")[0].split()[-1])
                        if s.startswith("tic ") else 0)[-15:]
        for line in merged:
            colour = (255, 140, 140) if "dropped" in line else (140, 220, 255) if "learned" in line \
                else TEXT
            y = self.text(line.split(" | ")[0][:110], (LOG.x + 8, y), colour, self.small)

    def to_screen(self, p):
        x0, y0, x1, y1 = self.bounds
        pad = 12
        scale = min((MAP.w - 2 * pad) / max(x1 - x0, 1), (MAP.h - 2 * pad) / max(y1 - y0, 1))
        scale *= self.zoom
        if self.follow and self.obs is not None:
            cx, cy = self.obs.x, self.obs.y
        else:
            cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        return (MAP.centerx + (p[0] - cx) * scale, MAP.centery - (p[1] - cy) * scale)

    def draw_map(self) -> None:
        pygame.draw.rect(self.screen, (8, 8, 10), MAP)
        self.screen.set_clip(MAP)
        m, k = self.m, self.x.knowledge
        heights = self.obs.heights if self.obs else {}
        for li, line in enumerate(m.linedefs):
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            mapped = li in k.mapped and not line.flags & ML_DONTDRAW
            if not mapped and not self.show_all:
                continue
            f, bk = m.front_sector(line), m.back_sector(line)
            if not mapped:
                colour = (45, 45, 55)
            elif bk is None:
                colour = (220, 60, 60)
            else:
                hf, hb = heights.get(f), heights.get(bk)
                if hf and hb and hf[0] != hb[0]:
                    colour = (190, 130, 70)
                elif hf and hb and hf[1] != hb[1]:
                    colour = (220, 220, 80)
                else:
                    colour = (70, 70, 70)
            pygame.draw.line(self.screen, colour, self.to_screen((a.x, a.y)), self.to_screen((b.x, b.y)), 1)
        # what lines are, as far as she can tell
        fired = {f.line for f in k.fired}
        marks = [(li, KIND_COLOUR["switch"] if li not in fired else (90, 140, 90)) for li in k.interactables()]
        marks += [(li, (90, 140, 90)) for li in fired if li in k.mapped]
        marks += [(li, KIND_COLOUR["exit"]) for li in k.exits()]
        marks += [(li, (255, 255, 255)) for li in k.teleporters()]
        for li, colour in marks:
            line = m.linedefs[li]
            a, b = m.vertexes[line.v1], m.vertexes[line.v2]
            pygame.draw.line(self.screen, colour, self.to_screen((a.x, a.y)), self.to_screen((b.x, b.y)), 3)
        # things she knows about
        for th in k.known_things():
            item = PICKUPS.get(th.type)
            if item is None:
                continue
            name, cat = item
            p = self.to_screen((th.x, th.y))
            if cat == "key":
                pygame.draw.circle(self.screen, key_colour(name), p, 6)
                pygame.draw.circle(self.screen, (255, 255, 255), p, 6, 1)
            else:
                pygame.draw.circle(self.screen, ITEM_COLOUR[cat], p, 2)
        # objectives open to her, the current one, the route
        for o in self.options:
            p = self.to_screen(o.goal)
            c = KIND_COLOUR.get(o.kind, TEXT)
            pygame.draw.line(self.screen, c, (p[0] - 4, p[1] - 4), (p[0] + 4, p[1] + 4), 1)
            pygame.draw.line(self.screen, c, (p[0] - 4, p[1] + 4), (p[0] + 4, p[1] - 4), 1)
        obs = self.obs
        f = self.x.follower
        if obs is not None and f is not None:
            pts = f.planned_path((obs.x, obs.y))
            run = []
            for p in pts + [None]:
                if p is None:
                    if len(run) > 1:
                        pygame.draw.lines(self.screen, (60, 255, 60), False, [self.to_screen(q) for q in run], 2)
                    run = []
                else:
                    run.append(p)
        cur = self.x.current
        if cur is not None:
            p = self.to_screen(cur.goal)
            pygame.draw.circle(self.screen, KIND_COLOUR.get(cur.kind, TEXT), p, 8, 2)
        if obs is not None:
            p = self.to_screen((obs.x, obs.y))
            a = math.radians(obs.angle)
            tip = (p[0] + 10 * math.cos(a), p[1] - 10 * math.sin(a))
            left = (p[0] + 6 * math.cos(a + 2.5), p[1] - 6 * math.sin(a + 2.5))
            right = (p[0] + 6 * math.cos(a - 2.5), p[1] - 6 * math.sin(a - 2.5))
            pygame.draw.polygon(self.screen, (255, 255, 255), [tip, left, right])
        self.screen.set_clip(None)
        self.text("map as known" + ("  (+ unknown, faint)" if self.show_all else "") +
                  ("  [follow]" if self.follow else ""), (MAP.x + 8, MAP.y + 4), DIM, self.small)

    def draw_panel(self) -> None:
        pygame.draw.rect(self.screen, (20, 20, 26), PANEL)
        k, x, obs = self.x.knowledge, self.x, self.obs
        left, right = PANEL.x + 8, PANEL.x + 410
        tic = obs.tic if obs else 0
        state = "EXITED" if self.engine.exited else "paused" if self.paused else "exploring"
        y = self.text(f"{self.map_name}  {tic / TICRATE:6.1f}s  x{self.speed:g}  {state}",
                      (left, PANEL.y + 4), HEAD, self.big)
        cur = x.current
        y = self.text("Now: " + (cur.label if cur else "-"), (left, y + 2),
                      KIND_COLOUR.get(cur.kind, TEXT) if cur else TEXT)
        if x.follower is not None:
            y = self.text("     " + x.follower.objective[:55], (left, y), DIM, self.small)
        m = self.m
        y = self.text(f"seen {len(k.mapped)}/{len(m.linedefs)} lines, {len(k.sectors)}/{len(m.sectors)} "
                      f"sectors; {len(k.fired)} fired", (left, y + 2), DIM, self.small)
        y = self.text("Objectives open", (left, y + 6), HEAD)
        pos = (obs.x, obs.y) if obs else (0, 0)
        for o in sorted(self.options, key=lambda o: math.dist(o.goal, pos))[:14]:
            mark = ">" if cur is not None and o.ident == cur.ident else " "
            y = self.text(f"{mark} {o.kind:8} {o.label[:30]:30} {math.dist(o.goal, pos):5.0f}",
                          (left, y), KIND_COLOUR.get(o.kind, TEXT), self.small)
        # what she has found
        y = self.text("Found", (right, PANEL.y + 4), HEAD, self.big)
        held = set(obs.keys or ()) if obs else set()
        keys, items = [], {}
        for th in k.known_things():
            item = PICKUPS.get(th.type)
            if item is None:
                continue
            name, cat = item
            if cat == "key":
                keys.append(name + (" (held)" if Key(th.type) in held else ""))
            else:
                items.setdefault(cat, []).append(name)
        y = self.text("Keys: " + (", ".join(keys) or "none seen"), (right, y + 2), (255, 255, 0), self.small)
        for li in k.exits():
            y = self.text(f"Exit: line {li}", (right, y), KIND_COLOUR["exit"], self.small)
        for li in k.teleporters():
            where = k.teleports.get(li)
            y = self.text(f"Teleporter: line {li}" + (f" -> ({where[0]:.0f}, {where[1]:.0f})" if where else " (not used)"),
                          (right, y), (255, 255, 255), self.small)
        untried = k.interactables()
        locked = [li for li in untried if k.lock(li) is not Lock.NONE]
        y = self.text(f"Untried switches/doors: {len(untried)}" +
                      (f" ({len(locked)} locked: " + ", ".join(sorted({k.lock(li).value for li in locked})) + ")"
                       if locked else ""), (right, y), KIND_COLOUR["switch"], self.small)
        learned = k.known_triggers()
        y = self.text(f"Learned: {len(learned)} lines", (right, y), (140, 220, 255), self.small)
        for t, secs in learned[-6:]:
            y = self.text(f"  line {t.line}: {t.info.description[:28]} -> sectors {sorted(secs)}"[:60],
                          (right, y), (140, 220, 255), self.small)
        for cat in ("weapon", "health", "armor", "ammo", "powerup"):
            names = items.get(cat, [])
            if names:
                summary = ", ".join(f"{n} x{names.count(n)}" if names.count(n) > 1 else n
                                    for n in sorted(set(names)))
                y = self.text(f"{cat.capitalize()}: {summary}"[:62], (right, y), ITEM_COLOUR[cat], self.small)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("wad", help="doom, doom2, or a WAD path")
    ap.add_argument("map", help="e.g. E1M1, MAP02")
    ap.add_argument("--speed", type=float, default=1.0, help="game speed (1 = real time)")
    ap.add_argument("--snapshot", type=float, metavar="SECONDS",
                    help="save the window to out/watch.png after this much game time, and quit")
    a = ap.parse_args()
    Watch(resolve_wad(a.wad), a.map.upper(), a.speed, a.snapshot).run()


if __name__ == "__main__":
    main()
