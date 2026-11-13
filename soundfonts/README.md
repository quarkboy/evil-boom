# SoundFonts

The game's music is MIDI. Played as Doom first played it, through the OPL synth (a Sound Blaster's
FM), it needs nothing here. A SoundFont plays it with sampled instruments instead, through
FluidSynth: chosen with `watch --soundfont NAME`, or in the console's settings.

`tools/soundfonts.sh` fetches two, free to redistribute, and checks them:

| file | | license |
|---|---|---|
| `GeneralUser-GS.sf2` | GeneralUser GS v2.0.3 by S. Christian Collins: a General MIDI and GS bank, well liked for game music | its own, permissive ([licenses/GeneralUser-GS.txt](licenses/GeneralUser-GS.txt)) |
| `MuseScore_General.sf3` | MuseScore General v0.2: Fluid R3 by Frank Wen, adapted by Michael Cowgill and S. Christian Collins | MIT ([licenses/MuseScore_General.md](licenses/MuseScore_General.md)) |

Any other `.sf2` or `.sf3` put in this folder is offered too (a Roland Sound Canvas one, say).
Those are yours: nothing here but the two above, and their licenses, goes into git or a
release.
