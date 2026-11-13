# Using Evil BOOM

`evil-boom` is one program with many commands. Run it from the repository's root (its logs go to
`out/`). A WAD is given by short name (`doom`, `doom2`, `tnt`, `plutonia`) or by path, a map as
`E1M1` or `MAP01`.

## Watching her play

```sh
evil-boom watch doom E1M1                       # one level, in a window beside what she knows
evil-boom watch doom2 MAP01 --episode           # on from that level through the episode
evil-boom watch doom E1M3 --fight 4 --mortal    # Ultra-Violence, and she can die
```

In the window: space pause, `+`/`-` speed, `A` the whole map, `F` follow her, `M` mute, `L`
Evil's log, `T` text size, the mouse wheel zooms, Esc quits. The game's sound plays at normal
speed (off while paused or sped up).

`watch` options:

| option | |
|---|---|
| `--speed X` | play speed (1 = real time) |
| `--all` | the whole map shown, not only what she knows |
| `--episode` | carry on to the next level when she exits |
| `--mute` | no sound for the whole run |
| `--present` | Doom's own screens: the title, the menu, the melt, the tally, the finales, death |
| `--stream [right\|left]` | the 16:9 window for a stream: the game on one side, a column for the viewers on the other |
| `--window-size WxH` | the window's size (`1280x720`); the picture, 1920x1080 for the stream, is scaled to fit |
| `--no-vsync` | frames shown when ready rather than at the screen's refresh (kept to it by the clock) |
| `--fullscreen` | the whole screen, at the desktop's resolution, borderless |
| `--headless` | no window at all |
| `--directed FILE` | directed play: orders read from FILE as lines are added (see [NEURO.md](NEURO.md)) |
| `--neuro URL` | directed by Neuro over its WebSocket (`ws://127.0.0.1:8000`) |
| `--gpt [CONFIG]` | directed by the language-model stand-in (`config/neuro-gpt.json`) |

## Run options

These work with `watch`, `run-map`, `sweep`, `episode`, `score` and the rest:

| option | |
|---|---|
| `--fight SKILL` | monsters in, at that skill (1-5); without it, no monsters |
| `--mortal` | she can die (otherwise she can't be killed) |
| `--lives N` | lives (unlimited unless given) |
| `--seed N\|random` | the game's random numbers (0 unless given; real play uses `random`) |
| `--max-seconds S` | give up after S game seconds, all lives together (3600) |
| `--expert` | every item, then out |
| `--secrets` | out only with every secret found |
| `--complete` | every kill, item and secret, then out (`--fight 4 --expert --secrets`) |
| `--peek` | she knows the whole map from the start |
| `--carry H,A,...` | the loadout she starts with (see [DEBUGGING.md](DEBUGGING.md)) |
| `--tuning FILE` | the fight's numbers from FILE (`evil-boom tuning` prints the defaults) |
| `--snapshot S` | stop after S seconds and draw the window to `out/watch.png` (with `--focus X,Y,ZOOM`) |
| `--from FILE@S` | replay a recorded run to S seconds, then the bot as it is now |
| `--pwad FILE` | a PWAD over the IWAD (repeatable; any command) |
| `--known` | the level known beforehand: she looks round it first with no monsters, then plays it knowing the map and where every monster starts |
| `--stall S` | the 100% runs: over after S game seconds with nothing new collected or learned (300) |
| `--decisions` | keep the journal of her decisions, written beside the log |
| `--bar NAMES` | moves and retreats she isn't to make, by name |
| `--advisor [FILE]` | ask a decision model as she plays (`config/jev.json`; see `native/src/advisor.rs`) |

## Custom WADs

```sh
evil-boom watch doom2 MAP01 --pwad wads/mm/MM.WAD --pwad wads/mm/MMMUS.WAD --episode
```

PWADs are loaded over the IWAD in order, as the game's `-file`. Each map comes from the last one
that has it, and logs are named after the first (`MM_MAP01.txt`). Only maps in the DOOM format
work (not UDMF or Hexen format). Maps built for co-op play are often much harder alone.

## Headless runs

```sh
evil-boom run-map doom2 MAP14 --fight 3 --mortal        # one level; its log in out/explore/
evil-boom sweep doom doom2 --jobs 8                      # every level, a line each, a summary
evil-boom sweep doom E1M1 E1M2 doom2 MAP01               # some levels
evil-boom episode doom E1M1 --fight 3 --mortal --seeds 8 # an episode on one life, 8 seeds side by side
evil-boom score --jobs 12                                # the score (see TESTING.md)
evil-boom score-report out/score/A.tsv out/score/B.tsv   # compare two score sets
```

`run-map` writes everything about a run to `out/explore/<IWAD>_<MAP>.txt` and keeps a stamped
copy; [DEBUGGING.md](DEBUGGING.md) explains how to read it.

## Recording

```sh
evil-boom record doom E1M1 out/e1m1.mp4 --view game --fight 3 --mortal
```

An MP4 (H.264, through `ffmpeg`), played as fast as it goes; directed runs record in real time,
with the game's sound. `--view game|full|hud`, `--size WxH`, `--speed N`, `--episode`, `--crf N`,
`--preset P`, `--ffmpeg PATH`.

## The stream console

`evil-boom console` serves a web page to start, steer and stop the game from a browser: see
[CONSOLE.md](CONSOLE.md).

## Looking inside a level

```sh
evil-boom dump-nav doom E1M1            # the level's analysis: the navigation mesh, jumps, the plan
evil-boom sector doom E1M5 99 121       # sectors as the map has them: heights, lines, things, jumps
evil-boom follow doom E1M1              # plan the level and follow the plan
evil-boom secret-survey doom E1M1 E1M2  # how each secret is found, and how the wall search rates it
evil-boom drive doom E2M4 --script s.txt     # drive her by hand (see DEBUGGING.md)
evil-boom tuning                         # the fight's numbers, as JSON
```

## Arenas

```sh
evil-boom arena arenas/combat.txt --jobs 12 --seeds 8   # every fight in the file, 8 times each
evil-boom arena-one arenas/combat.txt NAME --seed 3     # one, once; its log in out/arena/
evil-boom arena-watch arenas/combat.txt NAME            # one, watched (N steps a tic)
```

The arena file format is described at the top of `native/src/arena.rs`.

## For the Neuro integration

```sh
evil-boom neuro-mock [--port N] [FILE]   # a stand-in Neuro server: actions sent from FILE
evil-boom neuro-gpt [--port N]           # a language model as Evil, as a Neuro server
```

See [NEURO.md](NEURO.md).
