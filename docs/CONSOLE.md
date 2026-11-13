# The stream console

`evil-boom console` is a web page for whoever runs the stream (Vedal, say, or a trusted mod; Evil is the streamer): start and stop the game, choose
the level and who's playing, pause, change speed and volume, restart a level, and see and act
on what Evil is doing. The game runs as its child process, in the window the stream captures.

```sh
evil-boom console                       # serves on 0.0.0.0:7878 and prints the page's URL with its token
evil-boom console --bind 127.0.0.1:7878 # this machine only
evil-boom console --new-token           # a new token; the old one stops working
```

Open the printed URL (it carries `?t=TOKEN`). Every request needs the token, from this machine
too: otherwise any web page open in that person's browser could control the game.

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

Kept in `config/console.local.json` (git-ignored; made on first start), edited from the page's
Settings section, in five groups. A change is used when the game's next started (Start saves the
form's changes first).

| on the page | in the file | |
|---|---|---|
| **Game:** WAD, Level, Skill | `wad`, `map`, `skill` | where she starts (an IWAD by short name only; skill 0 = no monsters) |
| Seed (testing) | `seed` | a fixed seed, which plays the same each time; empty: random each start |
| She can die | `mortal` | off: nothing hurts her from the start (the Game card's God mode switches it while she plays) |
| Go on through the episode | `episode` | out by an exit, on to the next level with what she has |
| Restart the game if it crashes or hangs | `auto_restart` | at the level she was on |
| **Title card:** Doom's own screens | `present` | the title card, the menu, the melt, the tally, the finales |
| At the title card | `title_wait`, `evil_starts` | go straight in through the menu; wait for Begin; or Evil chooses the game (Neuro or GPT directing), Begin still starting it |
| **Who directs her:** Directed by, Neuro URL | `mode`, `neuro_url` | `bot`, `neuro` (at `neuro_url`), `gpt` (the stand-in) |
| Your name, as she's told it | `console_name` | who she's told says or does what comes from the page ("Vedal: ...", "Vedal did doom_go ..."); Vedal by default |
| Levels, Skills, Play she may choose | `evil_maps`, `evil_skills`, `evil_play` | what Evil's offered when she chooses the game (skills 1-4: never Nightmare) |
| She goes on to the next level herself; She may restart a level | `evil_next`, `evil_restart` | hers while she directs: from the tally (else it goes on after 30 s); after 90 s with nothing new |
| **Window:** Layout | `layout` | `stream` (16:9, the game and a viewers' column) or `watcher` (the game, the map and her log) |
| Window resolution, Vsync, Full screen | `window_size`, `vsync`, `fullscreen` | the window's size (empty: the picture's own, 1920x1080 for the stream, scaled to fit), frames at the screen's refresh, the whole screen (borderless) |
| Game rendering resolution | `render_size` | the size the game itself is drawn at (empty: 640x480; 320x200 as in 1994, stretched to 4:3 as its screens were); the stream's game is 1440x1080 of its picture |
| **Sound:** Music | `music` | the songs as recorded, from your own Steam copy of the DOOM + DOOM II re-release: `sc55` (the original soundtrack on a Roland SC-55) or `idkfa` (Andrew Hulshult's); empty: all as MIDI |
| MIDI synth | `soundfont` | for the songs not recorded: empty for the default (`soundfonts/SC-55.sf2` if it's there, else OPL), `opl`, or a SoundFont in `soundfonts/` by name |
| Start muted | `muted`, and the Sound section's `sfx_volume`, `music_volume`, `master_volume` | the game's sound |

Settings that apply only under others are shown only then: the Neuro URL for Neuro, Evil's for
Neuro or GPT, the title card's choice with Doom's own screens, the column for the stream.

`neuro_urls` (in the same file, edited only there) lists the Neuro servers the page may choose
from: the page can never point the game at an arbitrary host.

## The page

Two columns of sections (one on a narrow screen): the game and its controls, the level, the
sound, the log, and Evil's card. Evil's card shows her goal, her settings, an open question with
its choices and countdown, `doom_why`, the last lines told her and her last actions. It can act
for her: answer the question, set a goal, change a setting, look, ask why, take over the fight.
Actions from the page are marked as yours, and she's told of them by the name in Settings (`console_name`, Vedal by default). Below her settings, the stream's own: how
soon a decision's forced on her when she's waited with nothing to do (6 s) or left a question
unanswered (3 s), 0 for never; kept in `config/console.local.json` (`force_idle`, `force_ask`)
for the next game.

The Stream column section sets the stream window's column: its side, which of its items it
shows (the logo and a box for chat at the top, the level, the map, now, Evil's calls, a question she's been asked with
its choices and clock, Evil says, the tally, the run, lately; those left shown move up), and
how tall the chat box and the map are. The chat box is at the top, so it never moves: lay OBS's
chat over it, where the panel says it is (x, y, width, height in the 1920x1080 picture, scaled
with the window). Each change shows at once, no Save needed, and is kept for the next game
(`panel_side`, `column_hide`, `chat_height`, `map_height`).

The View section's pictures (the map as she knows it, the game, the stream's window) are made
by the game only while a page shows them: its View section unfolded and the page in sight. OBS
has its own preview; with the section folded, they cost the game nothing.

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
