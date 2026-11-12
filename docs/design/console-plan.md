# The stream console: plan

> **Status:** built (2026-11-10 to 11-12); the branch it mentions was merged. How to use the console: [../CONSOLE.md](../CONSOLE.md).

Date: 2026-11-10. Branch `console` (kept apart from the bot's work on `main`).

The streamer's controls for a live show: start and stop the game, restart a level, change the
WAD, map, skill and who's directing her, reconnect to Neuro, see what's happening, step in. Not
Evil-facing and not on stream (an overlay page for OBS may come later, from the same server).
neuro-integration-plan.md section 7 put it in its own stage; its rules carried over from there:
the console never changes the engine directly (commands are applied by the game at a tic
boundary), it binds safely and keeps its credentials server-side, and a developer's ground-truth
view never reaches anything model-facing.

## Decisions (the maintainer's, 2026-11-10)

| Question | Choice | Why |
|---|---|---|
| The page | plain HTML/CSS/JS, one file baked into the binary (`include_str!`), no build step | The browser side can't be Rust without a WASM toolchain; everything with logic stays in Rust |
| Where it runs | a **supervisor process**, `evil-boom console`, which starts the game as a child | DSDA is one game a process: a WAD change (Doom to Doom II) or an engine crash needs a new process, and the console stays up through both |
| Who reaches it | the LAN too, with a token | the streamer's phone or second PC |
| Across the web | nothing in the repo: put behind a tunnel or reverse proxy of the stream's own; the console made to sit behind one (section below) | someone running the stream remotely |
| Doom's own screens | all of them: tally and finale, title and demo loop, the menu driven on screen, the melt | the show should look like Doom, not a bot harness |

## Shape

```
browser(s) --HTTP, polled, token--> evil-boom console          (supervisor)
                                                   | settings and token: config/console.local.json (git-ignored)
                                                   | starts/stops the game, keeps its stdout/stderr
                                                   v
                              TCP 127.0.0.1:<any>, JSON lines both ways
                                                   v
                                   evil-boom watch <wad> <map> ... --control ADDR   (the game)
                                   DSDA, the bot, the Neuro link, the viewer window (OBS captures it)
```

- **The game process is the existing `watch`**, with `--control ADDR`: it connects back to the
  supervisor, sends its state a few times a second and its log as it grows, and applies commands
  at the top of a frame. Without `--control` nothing changes, and sweeps, `score` and `run-map`
  never touch any of this (the score stays exactly as it is).
- **Settings that need a new process** (WAD, skill, monsters, seed, who directs her, the Neuro
  URL at first) are the supervisor's: changed on the page, applied by restarting the child.
  Everything else (pause, speed, mute, restart the level, another map, reconnect) is a command to
  the running game.
- **The Neuro link stays in the game process**, beside the director whose orders and names it
  serves. A game restart is then a reconnect for Neuro (startup and the actions again); her
  context survives it (the SDK's). If rehearsals show that's disruptive, the link moves to the
  supervisor and the game talks to it.
- **The HTTP side**: `tiny_http` (synchronous, small, threads, the codebase's style; no TLS
  features). `GET /` the page; `GET /api/poll?since=N` how things stand and the log after line N
  (the page polls it 2-3 times a second: simpler than server-sent events, which `tiny_http`'s
  chunk buffering would have needed raw socket writes for, and plenty for a page or two);
  `POST /api/game/{start,stop,restart}`; `POST /api/cmd` a command; `POST /api/settings`.
- **Access**: `--bind` (default `0.0.0.0:7878`). Every API call carries the token (`?t=` in the
  page's URL, which the page passes on), **from 127.0.0.1 too**: without it, any web page open in
  the streamer's own browser could post to the console. The token's made once and kept in
  `config/console.local.json` (git-ignored), and the URLs with it printed at start. Plain HTTP:
  fine on a home LAN; never port-forwarded (across the web: behind a tunnel, below). Under WSL2 the LAN
  reaches it only with mirrored networking or a port proxy on Windows.
- **Muting**: the game starts with its sound and, if the settings say so, muted by a command once
  it's connected (not `--mute`, which takes the sound away for the whole run).

## The protocol (game <-> supervisor)

JSON a line. The game's first line says who it is (`hello`: protocol version, pid, commit,
WAD, map); after that:

| From the game | When |
|---|---|
| `state`: map, game state (level, intermission, ...), tic, health, armor, weapon, ammo, keys, tally, lives left, paused, speed, muted, mode, Neuro (URL, connected, who), her current order | 4 a second |
| `log`: new log lines | as they come |
| `event`: died, exited, level started | as they happen |

| To the game | Does |
|---|---|
| `pause`, `resume`, `speed` x, `mute` on/off | the watcher's own controls |
| `restart_level` | the level again from the start (memory of the life before kept, as a death keeps it) |
| `goto_map` map, carry | another level of the WAD in the same process (built): made ready first, so a level that isn't there leaves her playing on, told why; a pistol start unless `carry`; one a frame, the commands after it the next frame's. The game's hello lists the WAD's levels for the page's picker ("Next level" is the next in that list) |
| `neuro_reconnect` | drop the link and connect again (the director, her orders and names, kept) |
| `say` text | context to Evil from the streamer (later: with the maintainer's wording rules) |
| `order` action, data | an action as if from Evil (an override for when she's stuck; logged as the streamer's) |
| `quit` | close cleanly |

## Stages

1. **Skeleton** (built on this branch, 2026-11-10): the supervisor and its page; settings form;
   start, stop and restart of the game; the game's state and log live on the page; pause, speed,
   mute, restart the level, reconnect to Neuro. The child's stdout and stderr kept and shown (a
   crash's last words). Tested by hand through the API: each command, a stop, a `kill -9` of the
   game (shown as `signal: 9`), a restart, a refused token, refused settings and commands.
   Not yet: the headless watch (the console drives the windowed one, which OBS captures).
2. **Operator controls** (built, 2026-11-10): another level of the same WAD; lives and god mode
   live; a line to Evil (context, not silent, "The streamer: ..."); an order as hers (through her
   own checks; she's told quietly of an order, not of a look); the traffic with Neuro is the
   log's "Evil and Neuro" filter; the WAD only by its short name (never a path). Sound (the
   maintainer's): master, SFX and music, each a remote control. SFX and music are DSDA's own settings
   (0-15, its Sound menu's, not saved to its config file); DSDA has no master, so the engine
   patch adds one, a gain on the final mix in its audio callback (music and SFX are in it by
   then), 0-100. The levels are kept in the console's settings and given to each game as it
   connects. Left: the GPT
   stand-in's settings and its log on the page.
3. **Robustness** (built, 2026-11-10): a game that ends neither asked to nor cleanly (a signal, a
   non-zero code) is started again after 2 s, at the level she was on (a pistol start, the same
   WAD's); three crashes in two minutes and it's left for the streamer, said so. A clean end (the
   episode over, the window closed) isn't restarted. A connected game silent 45 s, or one not
   connected 90 s after it started, is killed as hung, and so restarted (45 s: a level change
   works out the level's analysis in the game's loop; not yet measured on the biggest maps). The
   page's settings have "Restart it if it crashes" (on). Everything the page shows also goes to
   `out/console/<commit>-<time>.log`. Tested: kill -9 at MAP05 (back at MAP05, a new pid); three
   in a row (the third left); SIGSTOP (killed as hung after 45 s, restarted). Left: the console's
   own restart finding a game still running (today a restarted console starts a new one).
4. **Doom's own screens** (built, 2026-11-10; the maintainer's: "we might need to port more of the
   engine, like the normal menu screen", then "skip the demo loop: stay on the title card").
   `watch --present` (`--title`: held at the title card till the console's Begin); the console's
   settings "Doom's own screens" and "Wait at the title card" (both on). The engine patch
   (`evil_present`, `evil_title`, `evil_menu`, `evil_wipe_tic`, `evil_screen`, `evil_exit`) draws
   every game state as D_Display does (the tally, the finale, the title page, the menu over them,
   the HUD's messages), keeps the menu up, holds the move on to the next level for the caller,
   and runs the melt a tic at a time with the game held, as Doom holds it (its random numbers
   kept from the play's). `viewer/present.rs` says when, a tic at a time:
   - the title card, held (no demos); Begin walks the menu as a player does (New Game, the
     episode for Doom, the skill a move at a time, each with the menu's sound) and the level's
     started as any is; a level that's not an episode's first: from the title straight in;
   - the tally and the finales run as the game runs them: she presses use 2.5 s after the tally's
     counted, 6 s after a Doom II text is all shown (Doom's go on by themselves); then the next
     level, started as an episode's is; the episode's end (Doom's end picture, Doom II's cast)
     stays on, "the end";
   - a death watched 3 s (the view sinking), then the level again with a life spent, or, with none
     left, the title card ("over");
   - "Exit level" / "Secret exit" on the page: out by the game's own exit (the tally and all).
   - DSDA's speedrunning clock on the tally ("Precise Intermission Time") off while presenting.
   Seen (pictures from the page's View): the title card, the skill menu with the skull on the
   chosen skill, the melt into the level, MAP01's tally and "Entering Underhalls", MAP06's tally
   and its text then MAP07, E1M8's tally and the episode's text, MAP30's text and the cast, a
   death melting into the title card, a death and the level again with a life spent.
   Presented games aren't the same games as unpresented ones on a fixed seed (a new game seeds
   its dice with the game's tic count, `rngseed += I_GetRandomTimeSeed() + gametic`, and the title
   card and the menu add tics), but they're the same as each other (seed 3, three times: the
   same death at tic 6290). The maintainer's: fine, streams play random seeds. Unpresented play is
   unchanged by the patch: one binary on the old engine and the new, three levels, the same
   results to the second.
   **The window for the stream** (built, 2026-11-10; the maintainer's: "a new mode for the watcher
   window for streaming itself ... the game isn't natively 16:9, so there is room on the sides
   ... figure out something interesting to add in there"): `watch --stream [right|left]`, the
   console's settings "Window: for the stream" (the default) and "Column: right/left" (moved live
   when saved). 1920x1080, titled "Evil BOOM" for OBS: the game's frame at 4:3 (1440x1080, its
   pixels scaled as they are) and a 480 column for the viewers (`viewer/stream.rs`): the map and
   the level's name (the automap's title: `evil_level_title`), skill and level time; a map of what
   she's seen following her (the watcher's own map drawing, now drawn into any box:
   `MapView`/`draw_map_in`), her route and objectives; NOW, what she's doing in the viewers'
   words (fighting what, by the names players know; else her order when Evil directs her, else
   "exploring", "searching for secrets", "picking up the clip", ...); what Evil said lately; the
   tally as bars; the run (levels done, deaths, game time); what's happened lately (the game's
   messages, levels done, deaths, secrets, Evil's orders; repeats counted, not repeated).
   Nothing the game's own status bar shows (health, ammo, keys). The console's View has
   "Stream": the window at half size, as OBS gets it. Seen: both sides, MAP01 and MAP02 on UV
   mid-fight ("Fighting a Shotgun Guy" with him on screen).
   On a 1080-line monitor the window's scaled down to fit, and OBS captures it at that size
   (scaled up again in OBS); a monitor with more lines, or a smaller stream size, if that shows.
   **The console's View** (the maintainer's: "the minimap view with current objectives, like the
   watcher"): the game sends the watcher's own drawing twice a second, the right side of its
   window (the map as she knows it, her route, the objectives, the panel under it) and the
   game's frame (an indexed PNG of the engine's frame, ~70 KB), made into PNGs on a thread of
   their own; the page shows either or both, fetching a picture only when it's shown and new.
5. **Across the web** (below): the hardening left to do.
6. **Later**: an overlay page for OBS (her state, what Evil last said); Neuro's lifecycle actions
   (start, restart, next level) allowed or not from the console; a session's settings saved as
   named presets.

## Across the web: a trusted assistant running the stream remotely

The console can be reachable from anywhere, but nothing for that is in the repo: it goes behind
a tunnel or reverse proxy the stream already runs. What the console does to sit behind someone
else's tunnel or proxy:

- It serves at a hostname's root or under a path of theirs (`https://.../doom/`): the page
  calls the API by paths relative to itself, the server takes what's from `/api/` on as the
  call, and `/doom` is redirected to `/doom/`.
- `--bind 127.0.0.1:PORT` when the tunnel's on the same machine: the tunnel is the one way in.
- The token still needed on every call, whatever login the tunnel has in front.

Done for being reached from outside (2026-11-10):

- The WAD only by its short name (never a path on the PC).
- The Neuro URL only one of `neuro_urls` in `config/console.local.json` (default
  `ws://127.0.0.1:8000`), edited in the file on the PC, never from the page: otherwise the page
  could point the PC's WebSocket at any host on its network.
- Each action in the log with who sent it: `Cf-Access-Authenticated-User-Email` when a tunnel's
  login gives it, else the caller's address (the volume not logged: a slider sends many). The
  name is to be trusted only when the console's bound to 127.0.0.1 behind the tunnel: on the LAN
  anyone could send the header. Checking Access's signed `Cf-Access-Jwt-Assertion` would make it
  trustworthy anywhere: later, if wanted.
- `evil-boom console --new-token`: a new token, the old one refused.
- A `console.local.json` that can't be read stops the console with the reason rather than being
  written over (it's the streamer's settings and token).

Not done: slowing down wrong tokens. A 128-bit token can't be guessed by trying, and behind a
tunnel every caller is 127.0.0.1, so a lockout would lock everyone out.

## Keeping the branch mergeable

The bot's work goes on on `main` in `viewer.rs`, `main.rs` and the fight's files. The console's
code is in its own module (`native/src/console/`); the hooks elsewhere are kept small (a field and
a call in the watcher's loop, an option in `watch`'s parsing, a reconnect in the Neuro session).
Rebased on `main` as it moves; merged when a stage is tested.

The engine is changed on this branch too (`evil_volume`, `evil_get_volume`, the master's gain in
`i_sound.c`), so the worktree has its own copy of `third_party/` (not the main checkout's, which
the other session's sweeps load), built by `tools/build-engine.sh` there. On merging, the engine
on `main` is rebuilt the same way, with no sweep running. The Rust side takes the new calls as
optional: with an engine built before them, the binary runs and the page says there's no volume
control.

## Testing

- The protocol and the HTTP layer: unit tests (commands parsed, bad lines dropped, the token
  checked).
- A smoke test by hand each stage: `evil-boom console`, the page from another device, start a
  game, each control, kill the game process from outside and watch the console notice.
- Presentation: parity on fixed seeds (the bot's level play unchanged by it), then watched.
- Nothing here runs in `score`: a change on this branch should score exactly 0.0000 different
  (`score --like`) when it's merged; one run of a few levels to confirm, not a sweep.

## Evil starts the game (2026-11-11)

Settings: **Evil starts the game** (on by default), **Levels she may start** (empty: the map
above; `all`; or a list), **Skills she may choose** (empty: the skill above; or a list),
**Play she may choose** (as "Whole episode" above, one level, on through the episode, or
either). They go to the game as `--choices` when it starts, and as the `choices` command
whenever they're saved (no restart needed). With Doom's own screens and "Wait at the title
card", she's offered a game at the title; with any, again when what she chose is over (a
level played by itself, at its tally; an episode, once its ending's text is told), and at a
game over. The streamer's Begin still starts the console's own game. Two more switches, on by
default: **Evil goes on to the next level** (playing on through, the tally screen waits for
her `doom_game next`, 30 s at most) and **Evil may restart a level** (hers after 90 s with
nothing new).

## No lives (2026-11-11)

The Lives setting and the Set-lives control are gone (the maintainer's: lives were for benchmarking;
on stream it's the original game, again and again till it's turned off). The console starts
its game with `--lives unlimited`.

## Evil's side on the console (2026-11-12)

The game's state carries `evil` (protocol 4) whenever Neuro or a file directs her: her goal, her
settings in a line and what she does with no action, the stream column's calls, her open question
(its choices, its default, the seconds till it, whether it's one being done), `doom_why` (fresh
with each state), the danger, the last 40 lines told her (marked if told to be heard), her last 40
actions with their answers (the streamer's marked), a minute's actions, words told and moments,
and the streamer's last action's answer in full. The page's Evil card shows it all and acts on it
as hers (said to be the streamer's): a button for each of the question's choices, her goal set or
dropped, her settings applied, Look, Status, Why and the lists, the fight's hold fire, fire,
retreat and back to reflex, stop. The small card beside it keeps the line to her and any action
by hand.

## The page's layout (2026-11-12)

Two columns of sections (one on a narrow screen). Each section folds to its title (a click on it),
moves within its column or to the other (its grip, dragged), and opens in a window of its own
(its button: the page with `only=<section>`, live as the main page is; in the main page it folds
to its title, back when the window's closed or by "Bring back"). The arrangement and what's folded
are kept in the browser; "Reset layout" in the header puts them back.
