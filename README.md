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
  doors, crushers, teleporters and run-jumps, from what it has seen of the map.
- **Fights by what it knows**: it dodges missiles, ducks out of a hitscanner's sight, picks a gun
  and a distance for each monster, clears barrels, and knows when to run.
- **Takes orders** from an AI through the Neuro SDK, or from a script: go there, explore,
  search for secrets, fight that, hold fire, set a goal. It tells the AI what happened in short,
  second-person lines. Optionally, with no orders for a few seconds it carries on by instinct.
- **Runs a stream:** a 16:9 stream window with Doom's own screens and a customizable HUD, and a web console for whoever
  runs the stream to start, steer and stop the game.
  **Debugging and testing** there are numerous tools and scripts developed for testing new code effectiveness, Neuro SDK behavior, and setting up quality comparisons, and a special watch screen for debugging and recording play sessions.

## Quick start

You need your own DOOM / DOOM II IWADs (Steam's, for one). Then, on Linux:

```sh
tools/build-engine.sh                                         # the engine (DSDA-Doom, patched)
(cd native && cargo build --release --target-dir ../out/build)  # Evil
out/build/release/evil-boom watch doom E1M1 --fight 3 --mortal  # watch her play E1M1 on HMP
```

Windows builds the engine in MSYS2 and Evil with rustup: see [docs/BUILDING.md](docs/BUILDING.md)
for both platforms, the packages to install and where the WADs go.

## Music

For the original's sound, from your own copies: with the DOOM + DOOM II re-release on Steam,
`--music sc55` (or the console's Music setting) plays its recordings of the soundtrack on a
Roland SC-55; and Patch93's SC-55 SoundFont (Not included due to ambiguous licensing, google for it), saved as `soundfonts/SC-55.sf2`, plays the songs
those don't have. [docs/BUILDING.md](docs/BUILDING.md#music-optional) says where each goes.
Without them it's the OPL synth, as a Sound Blaster played it.

## Documentation

| | |
|---|---|
| [docs/BUILDING.md](docs/BUILDING.md) | building on Linux and Windows, WADs, environment variables |
| [docs/USAGE.md](docs/USAGE.md) | the commands: watching, headless runs, custom WADs, recording |
| [docs/NEURO.md](docs/NEURO.md) | the Neuro SDK integration: the actions, and testing it without Neuro |
| [docs/CONSOLE.md](docs/CONSOLE.md) | the stream console |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | how it works, and where things are in the code |
| [docs/TESTING.md](docs/TESTING.md) | tests, parity checks, and the score that judges every change |
| [docs/DEBUGGING.md](docs/DEBUGGING.md) | tracing a failed run back to its cause |
| [docs/design/](docs/design/README.md) | the design history: decisions, plans, what was tried <- WARNING! very vibey> |
| [CONTRIBUTING.md](CONTRIBUTING.md) | how changes are made and judged |

## Use of AI in the development of this project

AI coding assistants were used heavily in the development of this project, from planning, testing, and writing and cleaning up code.  The primary code was written by Claude Opus 5.5 (medium), and later by Fable 5.2 (medium).  Care was taken to cleanup comments, maintain logical code structure, and multiple cleanup and refactoring rounds have made a sprawling codebase somewhat nice, ultimately.  All development direction and manual testing was done by humans with a good knowledge of DOOM.  AI Agents were used to attempt to debug stuck issues in multiple levels at once, but ultimately the vast majority was vibe coded old school style:  ask for a feature plan, evaluate the plan with the human, ask for changes, develop a smoke test, develop the full feature, run multiple scoring evaluations (or test in strea mode), and continue to iterate.  This has resulted in a pretty clean, straightforward codebase that is not overly engineered (except maybe the optional logging).

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
