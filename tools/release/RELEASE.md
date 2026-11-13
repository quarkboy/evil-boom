# Evil BOOM: binary release

A DOOM bot built to be directed by an AI streamer, running DSDA-Doom in-process. This folder is
ready to run: unpack it anywhere you can write to. Logs, recordings and caches go to `out/`
beside the program; settings are in `config/`.

## You need

- Your own DOOM and/or DOOM II IWADs. Evil looks for them in Steam's layout (`base/DOOM.WAD`,
  `base/doom2/DOOM2.WAD`):
  - Windows: Steam's install folder (`C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom`);
  - Linux: `~/.local/share/evilboom/` (copy Steam's `base/` folder there);
  - anywhere: set `EVILBOOM_DOOM_DIR` to the folder holding `base/`.
- Linux: a desktop (X11 or Wayland) with OpenGL. The other libraries the engine needs are in
  `lib/`.

## Music

The music plays through the OPL synth, as a Sound Blaster played it, unless you add one of these
from your own copies:

- the DOOM + DOOM II re-release on Steam: `--music sc55` plays its recordings of the original
  soundtrack on a Roland SC-55 (`idkfa`: Andrew Hulshult's). Evil finds it in Steam's folder;
  elsewhere, set `EVILBOOM_RERELEASE_DIR` to the folder holding its `extras.wad`.
- Patch93's SC-55 SoundFont (search the web for it), saved as `soundfonts/SC-55.sf2`: the
  default MIDI synth from then on, for every song not recorded.

The console has both as settings (Music, MIDI synth).

## Try it

```sh
evil-boom watch doom E1M1 --fight 3 --mortal        # watch her play E1M1 on Hurt Me Plenty
evil-boom watch doom2 MAP01 --fight 2 --episode     # DOOM II from MAP01, on through
evil-boom console                                    # the stream console (open the URL it prints)
```

(On Windows, `evil-boom.exe` from a command prompt in this folder; on Linux, `./evil-boom`.)

The commands and options: <https://github.com/quarkboy/evil-boom/blob/main/docs/USAGE.md>.
Everything else (building it yourself, the Neuro SDK integration, how it works) is in the
repository: <https://github.com/quarkboy/evil-boom>.

## License and source

Evil BOOM is free software under the GNU General Public License, version 3 or later
(`LICENSE`). It's built from:

- this repository at the release's tag (its source archive is attached to the release);
- DSDA-Doom at the pinned commit with Evil's patch applied (the release's
  `engine-source` archive);
- the libraries bundled here (`lib/` on Linux; the DLLs on Windows, with their licenses in
  `licenses/`), unmodified, from Ubuntu's and MSYS2's packages (FluidSynth on Linux from its own
  source, v2.6.2). Their sources are available from
  those projects, and on request from the maintainer for three years from the release.

`THIRD_PARTY.md` lists every component and its license. DOOM is a trademark of id Software; this
project isn't affiliated with or endorsed by id Software, ZeniMax Media or Bethesda. Evil Neuro
and Neuro-sama are Vedal's; this is an unofficial fan project, not affiliated with or endorsed by
Vedal.
