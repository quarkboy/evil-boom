# Evil BOOM

A DOOM bot built to be directed by an AI streamer, live. Evil BOOM plays DOOM and DOOM II the
way a person at the screen would. It explores from what it has seen, learns what switches do by
pressing them, fights by what it can see and hear, and finds the exit. Through the
[Neuro SDK](https://github.com/VedalAI/neuro-sdk) an AI (Evil Neuro, on stream) gives it
orders at the speed of thought, while the bot handles the 35-tics-a-second part: moving,
aiming, dodging, shooting.

It runs the real game: [DSDA-Doom](https://github.com/kraflab/dsda-doom), built as a library
and stepped a tic at a time from Rust.

## What it does

- **Plays levels on its own**, from a pistol start: it plans routes through keys, lifts, timed
  doors, crushers, teleporters and run-jumps, from what it has seen of the map. It can carry on
  through an episode with what it has, on a set number of lives.
- **Fights by what it knows**: it dodges missiles, ducks out of a hitscanner's sight, picks a gun
  and a distance for each monster, clears barrels, and knows when to run.
- **Has an ultra instinct** for the levels that keep killing her: after a few deaths on a level,
  a death is undone a few seconds back and the fight is searched for her in the game itself,
  for a limited time. It's a setting, off or on, in the console and on the command line.
- **Takes orders** from an AI through the Neuro SDK, from a language-model stand-in, or from a
  script: go there, fight that, hold fire, set a goal, change how careful she is. It tells the AI
  what happened in short, second-person lines, and asks it the choices that are the AI's to make
  (leave with things left? take the risky way?), each with a default and a clock. Between orders
  she carries on by instinct, unless told to wait.
- **Lets the AI run the game too**: start it at the title card, choose the level and skill, move
  on from the tally, restart a level that's going nowhere. Whoever runs the stream decides which
  of these, and which of her settings, are hers to change.
- **Runs a stream:** a 16:9 window with Doom's own screens (title, menu, tally, finales) and a
  column for the viewers (map, what she's doing, her calls, the tally) whose parts can be turned
  on and off; and a web console for whoever runs the stream to set up, start, steer and stop the
  game, see what Evil is told and does, and act in her place.
- **Plays other maps:** PWADs in the DOOM format load over the IWAD (`--pwad`). Boom and MBF21
  map sets aren't supported yet.
- **Comes with its own test bench:** headless runs and sweeps, set-piece fights (arenas), a
  three-lives test that every change to her play is judged by, a parity check for changes meant
  to change nothing, a mock Neuro server, and a watch window that shows what she knows and why
  she's doing what she's doing.

## Download

Linux and Windows packages, ready to run, are on the
[releases page](https://github.com/quarkboy/evil-boom/releases). You need your own DOOM or
DOOM II IWADs; `RELEASE.md` inside the package says where they go.

## Quick start (from source)

You need your own DOOM / DOOM II IWADs (Steam's, for one). Then, on Linux:

```sh
tools/build-engine.sh                                           # the engine (DSDA-Doom, patched)
(cd native && cargo build --release --target-dir ../out/build)  # Evil
out/build/release/evil-boom watch doom E1M1 --fight 3 --mortal  # watch her play E1M1 on Hurt Me Plenty
out/build/release/evil-boom console                             # the stream console: open the address it prints
```

Windows builds the engine in MSYS2 and Evil with rustup: see [docs/BUILDING.md](docs/BUILDING.md)
for both platforms, the packages to install and where the WADs go. `evil-boom help` lists every
command.

To have an AI direct her, point the game at a Neuro SDK server
(`--neuro ws://127.0.0.1:8000`, or the console's "Directed by" setting);
[docs/NEURO.md](docs/NEURO.md) has her actions and how to try it without Neuro.

## Music

Without anything added, the music plays through the OPL synth, as a Sound Blaster played it.
For the original's sound, from your own copies:

- With the DOOM + DOOM II re-release on Steam, `--music sc55` (or the console's Music setting)
  plays its recordings of the soundtrack on a Roland SC-55, and `--music idkfa` Andrew
  Hulshult's IDKFA.
- Patch93's SC-55 SoundFont, saved as `soundfonts/SC-55.sf2`, plays every song those don't
  have. It isn't included, its licensing being unclear: search the web for it.

[docs/BUILDING.md](docs/BUILDING.md#music-optional) says where each goes.

## Documentation

| | |
|---|---|
| [docs/BUILDING.md](docs/BUILDING.md) | building on Linux and Windows, WADs, environment variables, making a release |
| [docs/USAGE.md](docs/USAGE.md) | the commands: watching, headless runs, custom WADs, recording |
| [docs/NEURO.md](docs/NEURO.md) | the Neuro SDK integration: the actions, and testing it without Neuro |
| [docs/CONSOLE.md](docs/CONSOLE.md) | the stream console |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | how it works, and where things are in the code |
| [docs/TESTING.md](docs/TESTING.md) | tests, parity checks, and the three-lives test that judges every change |
| [docs/DEBUGGING.md](docs/DEBUGGING.md) | tracing a failed run back to its cause |
| [docs/design/](docs/design/README.md) | the design history: decisions, plans, what was tried (WARNING! very vibey) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | how changes are made and judged |

## Use of AI in the development of this project

AI coding assistants were used heavily in the development of this project, from planning, testing, and writing and cleaning up code.  The primary code was written by Claude Opus 5.5 (medium), and later by Fable 5.2 (medium).  Care was taken to cleanup comments, maintain logical code structure, and multiple cleanup and refactoring rounds have made a sprawling codebase somewhat nice, ultimately.  All development direction and manual testing was done by humans with a good knowledge of DOOM.  AI Agents were used to attempt to debug stuck issues in multiple levels at once, but ultimately the vast majority was vibe coded old school style:  ask for a feature plan, evaluate the plan with the human, ask for changes, develop a smoke test, develop the full feature, run multiple scoring evaluations (or test in stream mode), and continue to iterate.  This has resulted in a pretty clean, straightforward codebase that is not overly engineered (except maybe the optional logging).

## License

Evil BOOM is free software under the **GNU General Public License, version 3 or later**
([LICENSE](LICENSE)). It's built on DSDA-Doom (GPL-2.0-or-later, used under version 3), fetched
and patched at build time rather than included. [THIRD_PARTY.md](THIRD_PARTY.md) lists every
component and its license. If you distribute binaries, the GPL requires that you also offer the
complete corresponding source: this repository at that version, plus DSDA-Doom at the pinned
commit with `dsda/dsda-doom.patch` applied.

No game data is included. DOOM is a trademark of id Software; this project isn't affiliated with
or endorsed by id Software, ZeniMax Media or Bethesda. Evil Neuro and Neuro-sama are Vedal's;
this is an unofficial fan project, not affiliated with or endorsed by Vedal.

## To do

- Finish more levels, more consistently, the harder IWADs (TNT, Plutonia) above all.
- Play more realistically still: everything decided by what she's seen, heard and learned. The
  Icon of Sin mode, for one, plans from the whole map.
- Community WADs: Boom and MBF21 map sets, not just DOOM-format ones.
- A person in the game with her, co-op or deathmatch; and two bots, Evil and Neuro.
- Experimental: the advisor (`--advisor`), a decision model with a say in the bot's choices.
- A custom status bar for the stream.
