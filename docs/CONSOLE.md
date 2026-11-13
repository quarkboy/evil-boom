# The stream console

`evil-boom console` is a web page for whoever runs the stream: start and stop the game, choose
the level and who's playing, pause, change speed and volume, restart a level, and see and act
on what Evil is doing. The game runs as its child process, in the window the stream captures.

```sh
evil-boom console                       # serves on 0.0.0.0:7878 and prints the page's URL with its token
evil-boom console --bind 127.0.0.1:7878 # this machine only
evil-boom console --new-token           # a new token; the old one stops working
```

Open the printed URL (it carries `?t=TOKEN`). Every request needs the token, from this machine
too: otherwise any web page open in the streamer's browser could control the game.

## How it's put together

```
browser(s)  --HTTP, polled, token-->  evil-boom console            (the supervisor)
                                          | settings and token: config/console.local.json
                                          | starts and stops the game, keeps its output
                                          v   TCP on 127.0.0.1, JSON lines both ways
                                      evil-boom watch ... --control ADDR   (the game)
                                      DSDA-Doom, the bot, the Neuro link, the window OBS captures
```

Settings that need a new game process (the WAD, skill, seed, who directs her, the Neuro URL, the
window's size, the game's rendering size, vsync, full screen)
apply when the game is next started. Everything else (pause, speed, volume, restart the level,
another map) goes to the running game at once. A game that crashes or hangs can be restarted
automatically, at the level she was on.

## Settings

Kept in `config/console.local.json` (git-ignored; made on first start), edited from the page:

| setting | |
|---|---|
| `wad`, `map`, `skill` | where she starts (an IWAD by short name only; skill 0 = no monsters) |
| `mortal` | damage hurts her (off: god mode, for rehearsing) |
| `seed` | a fixed seed for testing; none = random each start |
| `episode` | on through the episode, level after level |
| `mode` | who directs her: `bot`, `neuro` (at `neuro_url`), `gpt` (the stand-in) |
| `muted`, `sfx_volume`, `music_volume`, `master_volume` | the game's sound |
| `present`, `title_wait` | Doom's own screens; the game waits at the title card until begun from the page |
| `layout`, `panel_side` | `stream` (16:9, the game and a viewers' column) or `watcher` (the game, the map and her log) |
| `window_size`, `vsync`, `fullscreen` | the game's window: its size (`1280x720`; empty: the picture's own, 1920x1080 for the stream, scaled to fit), frames at the screen's refresh, the whole screen (borderless) |
| `render_size` | the size the game itself is drawn at (empty: 640x480); the stream's game is 1440x1080 of its picture |
| `evil_starts`, `evil_maps`, `evil_skills`, ... | a game hers to start: which levels and skills she's offered |
| `auto_restart` | restart a game that crashes or hangs |

`neuro_urls` (in the same file, edited only there) lists the Neuro servers the page may choose
from: the page can never point the game at an arbitrary host.

## The page

Two columns of sections (one on a narrow screen): the game and its controls, the level, the
sound, the log, and Evil's card. Evil's card shows her goal, her settings, an open question with
its choices and countdown, `doom_why`, the last lines told her and her last actions. It can act
for her: answer the question, set a goal, change a setting, look, ask why, take over the fight.
Actions from the page are marked as the streamer's.

Each section folds to its title, can be dragged to the other column, and can open in a window of
its own (live, like the main page). The column split can be dragged too. The layout is kept in
the browser; *Reset layout* puts it back.

## Reaching it from elsewhere

- **On the LAN:** the default bind (`0.0.0.0:7878`) with the token. Plain HTTP: fine at home,
  never port-forwarded. Under WSL2 the LAN reaches it only with mirrored networking or a port
  proxy on Windows.
- **Across the web:** put it behind a tunnel or reverse proxy (it works at a hostname's root or
  under a path, `https://.../doom/`), bound to `127.0.0.1` so the tunnel is the only way in. The
  token is still needed on every call. Each action is logged with who sent it: the
  `Cf-Access-Authenticated-User-Email` header when a Cloudflare Access login gives it, else the
  caller's address. Only trust that name when bound to 127.0.0.1 behind the tunnel.

The design and its stages are in [design/console-plan.md](design/console-plan.md).
