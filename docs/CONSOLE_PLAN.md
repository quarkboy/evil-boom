# The stream console: plan

Date: 2026-11-10. Branch `console` (kept apart from the bot's work on `main`).

The streamer's controls for a live show: start and stop the game, restart a level, change the
WAD, map, skill and who's directing her, reconnect to Neuro, see what's happening, step in. Not
Evil-facing and not on stream (an overlay page for OBS may come later, from the same server).
NEURO_INTEGRATION_PLAN.md section 7 put it in its own stage; its rules carried over from there:
the console never changes the engine directly (commands are applied by the game at a tic
boundary), it binds safely and keeps its credentials server-side, and a developer's ground-truth
view never reaches anything model-facing.

## Decisions (the user's, 2026-11-10)

| Question | Choice | Why |
|---|---|---|
| The page | plain HTML/CSS/JS, one file baked into the binary (`include_str!`), no build step | The browser side can't be Rust without a WASM toolchain; everything with logic stays in Rust |
| Where it runs | a **supervisor process**, `evil-boom console`, which starts the game as a child | DSDA is one game a process: a WAD change (Doom to Doom II) or an engine crash needs a new process, and the console stays up through both |
| Who reaches it | the LAN too, with a token | the streamer's phone or second PC |
| Across the web | nothing in the repo: added to the tunnels Vedal already has for his other consoles; the console made to sit behind one (section below) | a trusted assistant running the stream remotely (the user's, 2026-11-10) |
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
| `say` text | context to Evil from the streamer (later: with the user's wording rules) |
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
   log's "Evil and Neuro" filter; the WAD only by its short name (never a path). Left: the GPT
   stand-in's settings and its log on the page.
3. **Robustness**: a crashed game restarted on its own (with a limit on crash loops); a hung one
   (no state for some seconds) killed and restarted; a log a session in `out/console/`; the
   supervisor's own restart picking the game up where it can.
4. **Doom's own screens** (the presentation; the user's: "we might need to port more of the
   engine, like the normal menu screen"). Today the patch draws only the level (`evil_draw`
   returns nothing unless `GS_LEVEL`), shuts the menu every tic, has the melt off, and on an
   exit jumps straight to the next map (`Engine::goto` with the loadout carried). In order:
   - a. `evil_draw` for every game state: the tally (`WI_Drawer`), the finale (`F_Drawer`), the
     title pages (`D_PageDrawer`), the menu over any of them (`M_Drawer`).
   - b. The tally and finale run as the game runs them: after an exit she keeps ticking with no
     commands (a press of use to hurry it after a while, as a player does) till the game moves on
     itself (`G_WorldDone`), which also carries her loadout.
   - c. Death shown as the game shows it: the view down, held a moment, then the restart.
   - d. The title and demo loop (`D_StartTitle`, `D_AdvanceDemo`) when no game's on: the game
     process starts in it and returns to it.
   - e. The menu driven on screen: key events made up for `M_Responder` (`evil_menu_key`), so a
     new game from the console walks the real menu (new game, episode, skill) at a human pace and
     the menu itself starts it. Later Evil could choose the skill herself (a Neuro lifecycle
     action, as NEURO_INTEGRATION_PLAN.md's `doom_start` has it).
   - f. The melt, driven a tic at a time by us rather than by the clock (why it was turned off:
     on the clock, some of her commands went nowhere while it ran, a different number each time).
   All of it behind a presentation flag the console's game sets: the bot's runs, sweeps and the
   score keep today's path. Risk to check before relying on it: the demo loop plays the game and
   uses the play RNG, so a level started after it must play as one started fresh (a parity check
   on fixed seeds, `run-map` against `watch --headless`).
   Also: a game-only window for OBS (the watcher draws the game, the map and the log side by
   side; `record --view game` already has the game alone), switchable from the console.
5. **Across the web** (below): the hardening left to do.
6. **Later**: an overlay page for OBS (her state, what Evil last said); Neuro's lifecycle actions
   (start, restart, next level) allowed or not from the console; a session's settings saved as
   named presets.

## Across the web: a trusted assistant running the stream remotely

The user asked (2026-11-10) for the console to be reachable from anywhere. Settled the same day:
nothing for it in the repo. Vedal already has tunnels for his other consoles; this one is added
to them. What the console does to sit behind someone else's tunnel or proxy:

- It serves at a hostname's root or under a path of theirs (`https://.../doom/`): the page
  calls the API by paths relative to itself, the server takes what's from `/api/` on as the
  call, and `/doom` is redirected to `/doom/`.
- `--bind 127.0.0.1:PORT` when the tunnel's on the same machine: the tunnel is the one way in.
- The token still needed on every call, whatever login the tunnel has in front.

Still to do for being reached from outside (on the LAN too, mostly):

- No setting that reaches past the game: `wad` one of the known IWADs (not any path on the PC);
  the Neuro URL one of a list in the local config (otherwise the page could point the PC's
  WebSocket at any host on its network).
- Each command logged with who sent it, when the tunnel's login says (Cloudflare Access's
  `Cf-Access-Authenticated-User-Email`, for one).
- Wrong tokens slowed down (a delay that grows); the token changeable from the command line.

## Keeping the branch mergeable

The bot's work goes on on `main` in `viewer.rs`, `main.rs` and the fight's files. The console's
code is in its own module (`native/src/console/`); the hooks elsewhere are kept small (a field and
a call in the watcher's loop, an option in `watch`'s parsing, a reconnect in the Neuro session).
Rebased on `main` as it moves; merged when a stage is tested.

## Testing

- The protocol and the HTTP layer: unit tests (commands parsed, bad lines dropped, the token
  checked).
- A smoke test by hand each stage: `evil-boom console`, the page from another device, start a
  game, each control, kill the game process from outside and watch the console notice.
- Presentation: parity on fixed seeds (the bot's level play unchanged by it), then watched.
- Nothing here runs in `score`: a change on this branch should score exactly 0.0000 different
  (`score --like`) when it's merged; one run of a few levels to confirm, not a sweep.
