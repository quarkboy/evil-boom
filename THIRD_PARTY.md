# Third-party software and credits

Evil BOOM is licensed under the GNU General Public License, version 3 or (at your option) any
later version (GPL-3.0-or-later): see [LICENSE](LICENSE). This file lists what it's built on and
with, under which licenses. All of them are compatible with GPL-3.0.

## DSDA-Doom (the engine)

- <https://github.com/kraflab/dsda-doom>, GPL-2.0-or-later (descended from PrBoom+, Boom and id
  Software's Doom source). Used under GPL-3.0.
- Not included in this repository. `tools/build-engine.sh` fetches it at commit `eb8eb98`
  (v0.30.1), applies `dsda/dsda-doom.patch` and builds it as the `dsda-evil` library, with
  `dsda/evil_api.c` compiled in. The patch and `evil_api.c` are ours, GPL-3.0-or-later.
- Inside it, besides its GPL-2.0-or-later files: Nuked OPL3 (LGPL-2.1-or-later), Raven
  Software's Heretic and Hexen code (released under the GPL), and a few public-domain and
  BSD-style files (`md5.c`, `win_opendir.c`, `scanner.cpp`, `gl_sky.c`).
- `dsda-doom.wad`, the engine's own resource file (menus, fonts, a few sprites and sounds), is
  built from DSDA-Doom's `prboom2/data/` folder and distributed with it under its license, as
  DSDA-Doom's own releases do.

The engine links these libraries at run time (they're part of a binary release, not this
repository): SDL2 and SDL2_mixer (zlib), libsndfile (LGPL-2.1-or-later), libzip (BSD-3-Clause),
zlib (zlib), FluidSynth (LGPL-2.1-or-later; the Linux release's built from its source, v2.6.2,
unmodified, without its own audio drivers), GLib (LGPL-2.1-or-later), and libogg and libvorbis
(BSD-3-Clause).

## Code written from other ports' behaviour

These are our own Rust, written to behave like the functions named. No code text was copied.

- `native/src/jumps/physics.rs`, the player-movement simulator, follows the ZDoom family's
  movement functions (P_XYMovement, P_TryMove, FSlide::SlideMove, P_ZMovement and others) as in
  GZDoom 3.0 (<https://github.com/ZDoom/gzdoom>, GPL-3.0-or-later). Those functions are id
  Software's (GPL) with ZDoom's changes.
- `native/src/los.rs` follows Doom's P_CheckSight (id Software's, GPL; DSDA-Doom's
  `p_sight.c`).
- `native/src/sight.rs` follows Doom's renderer, R_RenderBSPNode, R_AddLine and
  R_ClipWallSegment (id Software's, GPL), as in GZDoom's software renderer (GPL-3.0-or-later),
  where FWallCoords comes from.
- Thing class names (`DoomImp`, `BaronBall`, ...) are the ZDoom family's names for Doom's
  things, as ViZDoom (MIT) reports them.

## Rust crates

The crates in `native/Cargo.lock` (generated from `cargo metadata`; some are only used on
other platforms), by license:

- **MIT OR Apache-2.0** (84): base64 0.22.1, bitflags 2.13.2, block-buffer 0.10.4, bumpalo 3.20.3, cc 1.11.1, cfg-if 1.0.5, chunked_transfer 1.5.0, cpufeatures 0.2.17, crc32fast 1.5.4, crypto-common 0.1.7, digest 0.10.7, displaydoc 0.2.7, fdeflate 0.3.7, find-msvc-tools 0.1.15, flate2 1.1.11, form_urlencoded 1.2.2, futures 0.3.34, futures-channel 0.3.34, futures-core 0.3.34, futures-executor 0.3.34, futures-io 0.3.34, futures-macro 0.3.34, futures-sink 0.3.34, futures-task 0.3.34, futures-util 0.3.34, getrandom 0.2.17, http 1.5.0, httparse 1.10.1, httpdate 1.0.3, idna 1.1.0, itoa 1.0.18, js-sys 0.3.107, lazy_static 1.5.1, libc 0.2.191, log 0.4.35, minifb 0.28.0, once_cell 1.21.4, percent-encoding 2.3.2, pkg-config 0.3.34, png 0.17.16, ppv-lite86 0.2.21, proc-macro2 1.0.107, quote 1.0.47, rand 0.8.8, rand_chacha 0.3.1, rand_core 0.6.4, rustls-pki-types 1.15.1, rustversion 1.0.23, serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.154, sha1 0.10.7, sha2 0.10.9, shlex 2.0.1, smallvec 1.16.5, stable_deref_trait 1.2.1, syn 2.0.119, syn 3.0.9, thiserror 1.0.69, thiserror-impl 1.0.69, tiny_http 0.12.0, tungstenite 0.24.0, typenum 1.20.1, ureq 2.12.1, url 2.5.8, utf-8 0.7.6, wasm-bindgen 0.2.130, wasm-bindgen-futures 0.4.80, wasm-bindgen-macro 0.2.130, wasm-bindgen-macro-support 0.2.130, wasm-bindgen-shared 0.2.130, web-sys 0.3.107, windows-link 0.2.1, windows-sys 0.52.0, windows-targets 0.52.6, windows_aarch64_gnullvm 0.52.6, windows_aarch64_msvc 0.52.6, windows_i686_gnu 0.52.6, windows_i686_gnullvm 0.52.6, windows_i686_msvc 0.52.6, windows_x86_64_gnu 0.52.6, windows_x86_64_gnullvm 0.52.6, windows_x86_64_msvc 0.52.6
- **Unicode-3.0** (18): icu_collections 2.4.0, icu_locale_core 2.4.0, icu_normalizer 2.4.0, icu_normalizer_data 2.4.0, icu_properties 2.4.0, icu_properties_data 2.4.0, icu_provider 2.4.0, litemap 0.8.3, potential_utf 0.1.6, tinystr 0.8.5, writeable 0.6.4, yoke 0.8.4, yoke-derive 0.8.4, zerofrom 0.1.8, zerofrom-derive 0.1.8, zerotrie 0.2.5, zerovec 0.11.9, zerovec-derive 0.11.7
- **MIT** (15): bytes 1.12.1, data-encoding 2.11.1, font8x8 0.3.1, generic-array 0.14.7, libredox 0.1.25, orbclient 0.3.55, redox_syscall 0.9.5, sdl2 0.38.0, simd-adler32 0.3.10, slab 0.4.12, synstructure 0.14.0, tokio 1.54.1, version-compare 0.1.1, x11-dl 2.21.0, zmij 1.0.23
- **Apache-2.0 OR MIT** (6): ascii 1.1.0, idna_adapter 1.2.2, pin-project-lite 0.2.17, utf8_iter 1.1.0, utf_types 1.0.0, zeroize 1.9.1
- **MIT/Apache-2.0** (6): bitflags 1.3.2, plain 0.2.3, version_check 0.9.5, winapi 0.3.9, winapi-i686-pc-windows-gnu 0.4.0, winapi-x86_64-pc-windows-gnu 0.4.0
- **ISC** (3): libloading 0.8.9, rustls-webpki 0.103.16, untrusted 0.9.0
- **BSD-2-Clause OR Apache-2.0 OR MIT** (2): zerocopy 0.8.63, zerocopy-derive 0.8.63
- **BSD-3-Clause** (2): instant 0.1.13, subtle 2.6.1
- **CDLA-Permissive-2.0** (2): webpki-roots 0.26.11, webpki-roots 1.0.11
- **MIT OR Zlib OR Apache-2.0** (2): miniz_oxide 0.8.9, miniz_oxide 0.9.1
- **Unlicense OR MIT** (2): byteorder 1.5.0, memchr 2.8.3
- **(MIT OR Apache-2.0) AND Unicode-3.0** (1): unicode-ident 1.0.27
- **0BSD OR MIT OR Apache-2.0** (1): adler2 2.0.1
- **Apache-2.0 AND ISC** (1): ring 0.17.14
- **Apache-2.0 OR ISC OR MIT** (1): rustls 0.23.46
- **Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT** (1): wasi 0.11.1+wasi-snapshot-preview1
- **Apache-2.0/MIT** (1): console_error_panic_hook 0.1.7
- **MIT AND Zlib** (1): sdl2-sys 0.38.0
- **MIT OR Apache-2.0 OR Zlib** (1): raw-window-handle 0.6.2
- **Zlib** (1): zlib-rs 0.6.8

## Not included

- No SoundFont: `soundfonts/` is for the player's own (Patch93's SC-55 SoundFont, say, which is
  made from Roland's samples).
- The recorded music (`--music sc55|idkfa`) is read from the player's own copy of the DOOM +
  DOOM II re-release (its `rerelease/extras.wad`) into `out/music/` at run time; none of it is
  in this repository or a release.
- No game data. You need your own DOOM / DOOM II IWADs. DOOM is a trademark of id Software; this
  project isn't affiliated with or endorsed by id Software, ZeniMax Media or Bethesda.
- The Neuro SDK (<https://github.com/VedalAI/neuro-sdk>): Evil BOOM speaks its WebSocket protocol
  and includes none of its code. Evil Neuro and Neuro-sama are Vedal's; this is an unofficial fan
  project, not affiliated with or endorsed by Vedal.
