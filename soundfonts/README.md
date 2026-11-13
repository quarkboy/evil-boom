# SoundFonts

Doom's music is MIDI. With nothing here it plays through the OPL synth, as a Sound Blaster
played it. A SoundFont plays it with sampled instruments instead (through FluidSynth); the one
to have is a Roland SC-55's, the module the music was written on.

## The SC-55 SoundFont (the default when it's here)

1. Search the web for **"Patch93's SC-55 SoundFont"** and download it (a `.sf2` file; it's made
   from Roland's samples, so it isn't ours to include).
2. Put the `.sf2` in this folder named **`SC-55.sf2`**.

That's all: from then on the MIDI plays through it unless told otherwise (`watch --soundfont
opl` for OPL; the console's MIDI synth setting).

Any other `.sf2` or `.sf3` put here can be chosen by its file name (`--soundfont NAME`, or from
the console's list). Nothing in this folder but this README goes into git or a release.

The recorded soundtrack from the Steam re-release (`--music sc55`) is better still where it has
a song: see [docs/USAGE.md](../docs/USAGE.md#music).
