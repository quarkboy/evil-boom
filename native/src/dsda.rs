// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! DSDA-Doom, in-process: the game's own C code built as a library (`dsda/evil_api.c` and
//! DSDA-Doom's sources; see `dsda/README.md`), run a tic at a time, its state read straight
//! out of memory. One game a process (the engine is all globals).

use libloading::Library;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_ulong, CString};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Thing flags (`mobj_t.flags`) the adapter looks at.
pub const MF_SHOOTABLE: u64 = 0x4;
pub const MF_CORPSE: u64 = 0x100000;

/// A thing as the game has it (`evil_mobj`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Mobj {
    pub id: c_int,
    pub kind: c_int,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub momx: f64,
    pub momy: f64,
    pub momz: f64,
    pub angle: f64,
    pub radius: f64,
    pub height: f64,
    pub health: c_int,
    pub flags: u64,
    /// a missile: the type of what fired it (-1: none)
    pub owner_type: c_int,
    /// its state now (an index into the game's states: what it's doing)
    pub state: c_int,
    /// a monster: it's after the player
    pub after_her: c_int,
    /// a monster: tics into its missile attack (-1: not attacking)
    pub attack: c_int,
}

/// The player (`evil_player`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub angle: f64,
    pub momx: f64,
    pub momy: f64,
    pub momz: f64,
    pub health: c_int,
    pub armor: c_int,
    pub readyweapon: c_int,
    pub pendingweapon: c_int,
    /// by weapontype_t: fist, pistol, shotgun, chaingun, missile, plasma, bfg, chainsaw, SSG
    pub weaponowned: [c_int; 9],
    /// clip, shell, cell, missile
    pub ammo: [c_int; 4],
    /// blue card, yellow card, red card, blue skull, yellow skull, red skull
    pub cards: [c_int; 6],
    pub ironfeet: c_int,
    pub playerstate: c_int,
    pub damagecount: c_int,
    pub allmap: c_int,
    /// when the game last gave her a message (-1: never)
    pub message_tic: c_int,
    /// ...and what it said (nul-terminated)
    pub message: [c_char; 96],
    /// 0 none, 1 green, 2 blue
    pub armortype: c_int,
    pub backpack: c_int,
    /// berserk this level
    pub strength: c_int,
}

/// Where the game is (`evil_status`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Status {
    pub gametic: c_int,
    pub gamestate: c_int,
    pub gameaction: c_int,
    pub exited: c_int,
    pub secret: c_int,
    pub items: c_int,
    pub total_items: c_int,
    pub kills: c_int,
    pub total_kills: c_int,
    pub secrets: c_int,
    pub total_secrets: c_int,
}

/// (`evil_tic`'s: a weapon press for the very gun its number says, not turned into the slot's
/// best one)
const EVIL_EXACT: i32 = 0x100;

/// A tic's command for the player, as a ticcmd has it.
#[derive(Clone, Copy, Default, Debug)]
pub struct TicCmd {
    /// -50..50 (a run)
    pub forward: i32,
    /// -50..50, positive to the right
    pub side: i32,
    /// angleturn: the turn <<16, positive to the left
    pub turn: i32,
    pub attack: bool,
    pub use_: bool,
    /// switch to this weapon slot (1-7), 0 for none
    pub weapon: i32,
    /// the very gun the press is for (`Controls::exact`)
    pub exact: Option<i32>,
}

const BT_ATTACK: i32 = 1;
const BT_USE: i32 = 2;
const BT_CHANGE: i32 = 4;
const BT_WEAPONSHIFT: i32 = 3;

/// Initial capacity for bulk engine queries; grown when the level needs more.
const INITIAL_QUERY_CAPACITY: usize = 1024;

type InitFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int;
type NewGameFn = unsafe extern "C" fn(c_int, c_int, c_int, c_int);
type GodFn = unsafe extern "C" fn(c_int);
type SeedFn = unsafe extern "C" fn(c_int, c_ulong);
type TicFn = unsafe extern "C" fn(c_int, c_int, c_int, c_int);
type WarpFn = unsafe extern "C" fn(f64, f64, f64) -> c_int;
type DrawFn = unsafe extern "C" fn(*mut u8, c_int) -> c_int;
type StatusFn = unsafe extern "C" fn(*mut Status);
type SectorsFn = unsafe extern "C" fn(*mut f64, c_int) -> c_int;
type MobjsFn = unsafe extern "C" fn(*mut Mobj, c_int) -> c_int;
type PlayerFn = unsafe extern "C" fn(*mut PlayerState);
type PowerFn = unsafe extern "C" fn(c_int, c_int);
type SoundFn = unsafe extern "C" fn(c_int);
type SpawnFn = unsafe extern "C" fn(c_int, f64, f64, f64, c_int) -> c_int;
type PelletsFn = unsafe extern "C" fn(*mut c_int);
type GiveFn = unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int);
type ClearFn = unsafe extern "C" fn() -> c_int;
type ResetIdsFn = unsafe extern "C" fn();
type HitsFn = unsafe extern "C" fn(*mut c_int, c_int) -> c_int;
type SoundsFn = unsafe extern "C" fn(*mut c_int, c_int) -> c_int;
type SfxNameFn = unsafe extern "C" fn(c_int, *mut c_char, c_int) -> c_int;
type MobjSoundsFn = unsafe extern "C" fn(c_int, *mut c_int) -> c_int;
type GrabSoundFn = unsafe extern "C" fn(*mut i16, c_int) -> c_int;
type SoundRateFn = unsafe extern "C" fn() -> c_int;
type VolumeFn = unsafe extern "C" fn(c_int, c_int, c_int);
type GetVolumeFn = unsafe extern "C" fn(*mut c_int);
type PresentFn = unsafe extern "C" fn(c_int);
type PlainFn = unsafe extern "C" fn();
type MenuFn = unsafe extern "C" fn(c_int, c_int);
type ScreenFn = unsafe extern "C" fn(*mut c_int);
type TitleFn = unsafe extern "C" fn(*mut c_char, c_int) -> c_int;
type DrawAtFn = unsafe extern "C" fn(*mut u8, c_int, f64) -> c_int;
type IconFn = unsafe extern "C" fn(*const u32, c_int, c_int);

/// The watcher's window, the game's SDL's (evil_window_*): none in a library from before it.
struct WindowFns {
    open:
        unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, c_int, *mut c_int) -> c_int,
    show: unsafe extern "C" fn(*const u32, c_int, c_int),
    events: unsafe extern "C" fn(*mut c_int, c_int) -> c_int,
    title: unsafe extern "C" fn(*const c_char),
    close: unsafe extern "C" fn(),
}

/// The watcher's window as it's asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSpec {
    /// its size (width, height); none: the picture's own
    pub size: Option<(usize, usize)>,
    /// a frame shown at the screen's refresh (else when it's ready, kept to the refresh by the
    /// clock)
    pub vsync: bool,
    /// the whole screen, the desktop's resolution, borderless
    pub fullscreen: bool,
}

impl Default for WindowSpec {
    fn default() -> WindowSpec {
        WindowSpec {
            size: None,
            vsync: true,
            fullscreen: false,
        }
    }
}

/// The largest window.
pub const MAX_WINDOW: (usize, usize) = (7680, 4320);
/// The largest the game's drawn (what `draw_at` has room for).
pub const MAX_RENDER: (usize, usize) = (3840, 2880);

impl WindowSpec {
    /// "1280x720" read, 320x200 at the least and `max` at the most.
    pub fn parse_size(s: &str, max: (usize, usize)) -> Result<(usize, usize), String> {
        let bad = || {
            format!(
                "a size: WIDTHxHEIGHT, 320x200 to {}x{} ({s:?})",
                max.0, max.1
            )
        };
        let (w, h) = s.trim().split_once(['x', 'X']).ok_or_else(bad)?;
        let (w, h): (usize, usize) = (
            w.trim().parse().map_err(|_| bad())?,
            h.trim().parse().map_err(|_| bad())?,
        );
        if !(320..=max.0).contains(&w) || !(200..=max.1).contains(&h) {
            return Err(bad());
        }
        Ok((w, h))
    }
}

/// The watcher's window as it opened.
#[derive(Clone, Copy, Debug)]
pub struct WindowInfo {
    /// showing a frame waits for the screen's refresh
    pub vsync: bool,
    /// the screen's rate (Hz), 0 unknown
    pub refresh: i32,
}

/// What happened at the watcher's window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// a key pressed: its SDL keycode
    Key(i32),
    /// the wheel turned, up positive
    Wheel(i32),
    /// shut, or the program asked to quit
    Close,
}

/// The screen as the game has it (evil_screen): Doom's own screens, the stream's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    /// 0 a level, 1 the tally, 2 a finale, 3 the title
    pub state: i32,
    /// a melt under way or due (the game held: evil_wipe_tic, not a tic)
    pub wiping: bool,
    pub menu: bool,
    /// the move on to the next level held, for the caller to start it
    pub held: bool,
    /// waiting for a press (the tally counted; Doom II's finale text all shown)
    pub waiting: bool,
    /// the menu's current item
    pub item: i32,
}

static LOADED: AtomicBool = AtomicBool::new(false);

pub struct Dsda {
    /// The C API writes interleaved floor/ceiling values. Keep its staging buffer across tics.
    sector_buffer: RefCell<Vec<f64>>,
    new_game_fn: NewGameFn,
    god_fn: GodFn,
    seed_fn: SeedFn,
    tic_fn: TicFn,
    warp_fn: WarpFn,
    draw_fn: DrawFn,
    status_fn: StatusFn,
    sectors_fn: SectorsFn,
    mobjs_fn: MobjsFn,
    player_fn: PlayerFn,
    /// (none in a library from before it was added)
    power_fn: Option<PowerFn>,
    sound_fn: Option<SoundFn>,
    /// (arenas; none in an older library)
    spawn_fn: Option<SpawnFn>,
    give_fn: Option<GiveFn>,
    ready_fn: Option<unsafe extern "C" fn(c_int)>,
    clear_fn: Option<ClearFn>,
    reset_ids_fn: Option<ResetIdsFn>,
    hits_fn: Option<HitsFn>,
    pellets_fn: Option<PelletsFn>,
    sounds_fn: Option<SoundsFn>,
    sfx_name_fn: Option<SfxNameFn>,
    mobj_sounds_fn: Option<MobjSoundsFn>,
    grab_sound_fn: Option<GrabSoundFn>,
    sound_rate_fn: Option<SoundRateFn>,
    volume_fn: Option<VolumeFn>,
    get_volume_fn: Option<GetVolumeFn>,
    present_fn: Option<PresentFn>,
    title_fn: Option<PlainFn>,
    wipe_tic_fn: Option<PlainFn>,
    menu_fn: Option<MenuFn>,
    screen_fn: Option<ScreenFn>,
    exit_fn: Option<PresentFn>,
    level_title_fn: Option<TitleFn>,
    /// (none in a library from before it was added)
    finale_text_fn: Option<TitleFn>,
    draw_at_fn: Option<DrawAtFn>,
    window_fns: Option<WindowFns>,
    /// (none in a library from before it was added)
    window_icon_fn: Option<IconFn>,
    /// what a frame's drawn into, kept across frames
    draw_buffer: RefCell<Vec<u8>>,
    /// (the game keeps pointers into its arguments: they live as long as it does)
    _argv: Vec<CString>,
    _argv_ptrs: Vec<*mut c_char>,
    /// (last: the functions above point into it)
    /// (never unloaded: the game's threads, SDL's sound one among them, go on calling into it
    /// after the last of ours has done with it. Unloaded when a recorded episode ended, with
    /// the video still being closed: the sound thread's next call was into nothing, a segfault,
    /// the sound never put with the video)
    _lib: std::mem::ManuallyDrop<Library>,
}

impl Drop for Dsda {
    fn drop(&mut self) {
        // (the watcher's window, if one's open, shut with the game it shows)
        self.window_close();
    }
}

impl Dsda {
    /// Load the library and set the game up (`args` as for dsda-doom.exe: -iwad and so on);
    /// `dll_dirs`: where else its own libraries are (Windows: MSYS2's SDL2 and the rest).
    /// Once a process.
    pub fn load(lib: &Path, dll_dirs: &[&Path], args: &[String]) -> Result<Self, String> {
        if LOADED.swap(true, Ordering::SeqCst) {
            return Err("DSDA-Doom is already loaded in this process (one game a process)".into());
        }
        let lib =
            open_library(lib, dll_dirs).map_err(|e| format!("loading {}: {e}", lib.display()))?;
        let sym = |name: &[u8]| -> Result<*const (), String> {
            // SAFETY: as above; the pointer is only turned into the matching fn type.
            unsafe { lib.get::<*const ()>(name) }
                .map(|s| *s)
                .map_err(|e| format!("{}: {e}", String::from_utf8_lossy(name)))
        };
        // SAFETY: each symbol is the function of that name in evil_api.c, with this signature.
        let (
            init_fn,
            new_game_fn,
            god_fn,
            seed_fn,
            tic_fn,
            warp_fn,
            draw_fn,
            status_fn,
            sectors_fn,
            mobjs_fn,
            player_fn,
        ) = unsafe {
            (
                std::mem::transmute::<*const (), InitFn>(sym(b"evil_init")?),
                std::mem::transmute::<*const (), NewGameFn>(sym(b"evil_new_game")?),
                std::mem::transmute::<*const (), GodFn>(sym(b"evil_god")?),
                std::mem::transmute::<*const (), SeedFn>(sym(b"evil_seed")?),
                std::mem::transmute::<*const (), TicFn>(sym(b"evil_tic")?),
                std::mem::transmute::<*const (), WarpFn>(sym(b"evil_warp")?),
                std::mem::transmute::<*const (), DrawFn>(sym(b"evil_draw")?),
                std::mem::transmute::<*const (), StatusFn>(sym(b"evil_get_status")?),
                std::mem::transmute::<*const (), SectorsFn>(sym(b"evil_sectors")?),
                std::mem::transmute::<*const (), MobjsFn>(sym(b"evil_mobjs")?),
                std::mem::transmute::<*const (), PlayerFn>(sym(b"evil_player_state")?),
            )
        };
        // SAFETY: as above, evil_power's signature; optional, an older library hasn't it.
        let power_fn = sym(b"evil_power")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), PowerFn>(p) });
        // SAFETY: as above (evil_sound(int)).
        let sound_fn = sym(b"evil_sound")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), SoundFn>(p) });
        // SAFETY: as above, evil_spawn's and evil_give's signatures; optional.
        let spawn_fn = sym(b"evil_spawn")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), SpawnFn>(p) });
        let give_fn = sym(b"evil_give")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), GiveFn>(p) });
        // SAFETY: the symbol, if the library has it, is `void evil_ready(int)`.
        let ready_fn = sym(b"evil_ready")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), unsafe extern "C" fn(c_int)>(p) });
        // SAFETY: as above, evil_clear_items's signature; optional.
        let clear_fn = sym(b"evil_clear_items")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), ClearFn>(p) });
        // SAFETY: as above, evil_reset_ids's signature; optional.
        let reset_ids_fn = sym(b"evil_reset_ids")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), ResetIdsFn>(p) });
        // SAFETY: as above, evil_hits_since's signature; optional.
        let hits_fn = sym(b"evil_hits_since")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), HitsFn>(p) });
        // SAFETY: as above, evil_pellets_so_far(int*); optional.
        let pellets_fn = sym(b"evil_pellets_so_far")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), PelletsFn>(p) });
        // SAFETY: as above, evil_sounds_since's and evil_sfx_name's signatures; optional.
        let sounds_fn = sym(b"evil_sounds_since")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), SoundsFn>(p) });
        let mobj_sounds_fn = sym(b"evil_mobj_sounds")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), MobjSoundsFn>(p) });
        let sfx_name_fn = sym(b"evil_sfx_name")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), SfxNameFn>(p) });
        // SAFETY: as above, evil_grab_sound's signature; optional.
        let grab_sound_fn = sym(b"evil_grab_sound")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), GrabSoundFn>(p) });
        // SAFETY: as above, evil_sound_rate's signature; optional.
        let sound_rate_fn = sym(b"evil_sound_rate")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), SoundRateFn>(p) });
        // SAFETY: as above, evil_volume's and evil_get_volume's signatures; optional.
        let volume_fn = sym(b"evil_volume")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), VolumeFn>(p) });
        let get_volume_fn = sym(b"evil_get_volume")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), GetVolumeFn>(p) });
        // SAFETY: as above, the presentation's calls' signatures (evil_present, evil_title,
        // evil_wipe_tic, evil_menu, evil_screen); optional.
        let present_fn = sym(b"evil_present")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), PresentFn>(p) });
        let title_fn = sym(b"evil_title")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), PlainFn>(p) });
        let wipe_tic_fn = sym(b"evil_wipe_tic")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), PlainFn>(p) });
        let menu_fn = sym(b"evil_menu")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), MenuFn>(p) });
        let screen_fn = sym(b"evil_screen")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), ScreenFn>(p) });
        // SAFETY: as above, evil_exit(int); optional.
        let exit_fn = sym(b"evil_exit")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), PresentFn>(p) });
        // SAFETY: as above, evil_level_title(char*, int); optional.
        let level_title_fn = sym(b"evil_level_title")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), TitleFn>(p) });
        let finale_text_fn = sym(b"evil_finale_text")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), TitleFn>(p) });
        // SAFETY: as above, evil_draw_at's signature; optional.
        let draw_at_fn = sym(b"evil_draw_at")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), DrawAtFn>(p) });
        // SAFETY: as above, the evil_window_* functions' signatures, all or none; optional.
        let window_fns = (|| -> Result<WindowFns, String> {
            Ok(unsafe {
                WindowFns {
                    open: std::mem::transmute::<
                        *const (),
                        unsafe extern "C" fn(
                            *const c_char,
                            c_int,
                            c_int,
                            c_int,
                            c_int,
                            c_int,
                            *mut c_int,
                        ) -> c_int,
                    >(sym(b"evil_window_open")?),
                    show: std::mem::transmute::<
                        *const (),
                        unsafe extern "C" fn(*const u32, c_int, c_int),
                    >(sym(b"evil_window_show")?),
                    events: std::mem::transmute::<
                        *const (),
                        unsafe extern "C" fn(*mut c_int, c_int) -> c_int,
                    >(sym(b"evil_window_events")?),
                    title: std::mem::transmute::<*const (), unsafe extern "C" fn(*const c_char)>(
                        sym(b"evil_window_title")?,
                    ),
                    close: std::mem::transmute::<*const (), unsafe extern "C" fn()>(sym(
                        b"evil_window_close",
                    )?),
                }
            })
        })()
        .ok();
        // SAFETY: as above, evil_window_icon's signature; optional.
        let window_icon_fn = sym(b"evil_window_icon")
            .ok()
            .map(|p| unsafe { std::mem::transmute::<*const (), IconFn>(p) });
        let argv: Vec<CString> = std::iter::once("evil".to_string())
            .chain(args.iter().cloned())
            .map(CString::new)
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let mut argv_ptrs: Vec<*mut c_char> = argv.iter().map(|a| a.as_ptr().cast_mut()).collect();
        // SAFETY: argv and its pointers live in the returned value, as long as the game.
        unsafe { init_fn(argv_ptrs.len() as c_int, argv_ptrs.as_mut_ptr()) };
        Ok(Self {
            sector_buffer: RefCell::new(vec![0.0; 2 * INITIAL_QUERY_CAPACITY]),
            new_game_fn,
            god_fn,
            seed_fn,
            tic_fn,
            warp_fn,
            draw_fn,
            status_fn,
            sectors_fn,
            mobjs_fn,
            player_fn,
            power_fn,
            sound_fn,
            spawn_fn,
            give_fn,
            ready_fn,
            clear_fn,
            reset_ids_fn,
            hits_fn,
            pellets_fn,
            sounds_fn,
            sfx_name_fn,
            mobj_sounds_fn,
            grab_sound_fn,
            sound_rate_fn,
            volume_fn,
            get_volume_fn,
            present_fn,
            title_fn,
            wipe_tic_fn,
            menu_fn,
            screen_fn,
            exit_fn,
            level_title_fn,
            finale_text_fn,
            draw_at_fn,
            window_fns,
            window_icon_fn,
            draw_buffer: RefCell::new(Vec::new()),
            _argv: argv,
            _argv_ptrs: argv_ptrs,
            _lib: std::mem::ManuallyDrop::new(lib),
        })
    }

    /// A new game on the map (Doom: episode 1-4, map 1-9; Doom II: episode 1, map 1-32);
    /// skill 1-5. It starts on the next tic.
    pub fn new_game(&self, skill: i32, episode: i32, map: i32, no_monsters: bool) {
        // SAFETY: plain values into the game; see `load`.
        unsafe { (self.new_game_fn)(skill, episode, map, c_int::from(no_monsters)) }
    }

    /// Give her power `power` (0 invulnerability .. 5 the light amplification visor) for `tics`;
    /// false if the library hasn't the call.
    pub fn power(&self, power: i32, tics: i32) -> bool {
        let Some(f) = self.power_fn else { return false };
        // SAFETY: plain values into the game; see `load`.
        unsafe { f(power, tics) };
        true
    }

    /// The game's sound for the next `samples` sample frames, mixed now (16-bit stereo,
    /// interleaved), and its sample rate; None without -evilsoundcap (or the call).
    pub fn grab_sound(&self, samples: usize) -> Option<(u32, Vec<i16>)> {
        let f = self.grab_sound_fn?;
        let mut out = vec![0i16; samples * 2];
        // SAFETY: out holds 2 * samples shorts, all the call writes; see `load`.
        let rate = unsafe { f(out.as_mut_ptr(), c_int::try_from(samples).ok()?) };
        u32::try_from(rate)
            .ok()
            .filter(|&r| r > 0)
            .map(|r| (r, out))
    }

    /// The sound's sample rate (None without the call, or no sound).
    pub fn sound_rate(&self) -> Option<u32> {
        let f = self.sound_rate_fn?;
        // SAFETY: no arguments; see `load`.
        u32::try_from(unsafe { f() }).ok().filter(|&r| r > 0)
    }

    /// The loudness: the game's sfx and music volumes (0-15) and the master over the mix (0-100).
    /// None without the call.
    pub fn volume(&self) -> Option<[i32; 3]> {
        let f = self.get_volume_fn?;
        let mut out = [0 as c_int; 3];
        // SAFETY: room for the three it writes; see `load`.
        unsafe { f(out.as_mut_ptr()) };
        Some(out)
    }

    /// Doom's own screens on or off (the presentation: see evil_api.c). False: the library
    /// hasn't them.
    pub fn present(&self, on: bool) -> bool {
        let Some(f) = self.present_fn else {
            return false;
        };
        // SAFETY: a plain value into the game; see `load`.
        unsafe { f(c_int::from(on)) };
        true
    }

    /// The title card, held.
    pub fn title(&self) {
        if let Some(f) = self.title_fn {
            // SAFETY: no arguments; see `load`.
            unsafe { f() };
        }
    }

    /// A tic of the melt, the game held.
    pub fn wipe_tic(&self) {
        if let Some(f) = self.wipe_tic_fn {
            // SAFETY: no arguments; see `load`.
            unsafe { f() };
        }
    }

    /// The menu driven: 0 open, 1 the skull to item `arg`, 2 choose, 3 shut.
    pub fn menu(&self, action: i32, arg: i32) {
        if let Some(f) = self.menu_fn {
            // SAFETY: plain values into the game; see `load`.
            unsafe { f(action, arg) };
        }
    }

    /// The level left by its exit (`secret`: the secret one), as a switch leaves it. False: the
    /// library hasn't the call.
    pub fn exit(&self, secret: bool) -> bool {
        let Some(f) = self.exit_fn else { return false };
        // SAFETY: a plain value into the game; see `load`.
        unsafe { f(c_int::from(secret)) };
        true
    }

    /// The level's title as the automap has it ("level 7: dead simple"); None without the call
    /// or outside a level.
    pub fn level_title(&self) -> Option<String> {
        let f = self.level_title_fn?;
        let mut buf = vec![0 as c_char; 128];
        // SAFETY: the buffer's size given; it writes at most that, ended; see `load`.
        let n = unsafe { f(buf.as_mut_ptr(), buf.len() as c_int) };
        let bytes: Vec<u8> = buf
            .iter()
            .take(usize::try_from(n).ok()?)
            .map(|&c| c as u8)
            .collect();
        Some(String::from_utf8_lossy(&bytes).into_owned()).filter(|t| !t.is_empty())
    }

    /// The finale's text while one shows it (an episode's end, Doom II's story between levels),
    /// as much as it's typed out on screen yet; None without the call, with no finale up, or
    /// none of it shown yet.
    pub fn finale_text(&self) -> Option<String> {
        let f = self.finale_text_fn?;
        let mut buf = vec![0 as c_char; 4096];
        // SAFETY: the buffer's size given; it writes at most that, ended; see `load`.
        let n = unsafe { f(buf.as_mut_ptr(), buf.len() as c_int) };
        let bytes: Vec<u8> = buf
            .iter()
            .take(usize::try_from(n).ok()?)
            .map(|&c| c as u8)
            .collect();
        Some(String::from_utf8_lossy(&bytes).into_owned()).filter(|t| !t.trim().is_empty())
    }

    /// The screen as it stands; None without the call.
    pub fn screen(&self) -> Option<Screen> {
        let f = self.screen_fn?;
        let mut o = [0 as c_int; 6];
        // SAFETY: room for the six it writes; see `load`.
        unsafe { f(o.as_mut_ptr()) };
        Some(Screen {
            state: o[0],
            wiping: o[1] != 0,
            menu: o[2] != 0,
            held: o[3] != 0,
            waiting: o[4] != 0,
            item: o[5],
        })
    }

    /// The loudness set (None: that one left as it is); no effect without the call.
    pub fn set_volume(&self, sfx: Option<i32>, music: Option<i32>, master: Option<i32>) {
        if let Some(f) = self.volume_fn {
            let v = |x: Option<i32>, top: i32| x.map_or(-1, |x| x.clamp(0, top));
            // SAFETY: plain values into the game; see `load`.
            unsafe { f(v(sfx, 15), v(music, 15), v(master, 100)) };
        }
    }

    /// Sound on or off (with the game's sound up: a watcher's); no effect without the call.
    pub fn sound(&self, on: bool) {
        if let Some(f) = self.sound_fn {
            // SAFETY: a plain value into the game; see `load`.
            unsafe { f(c_int::from(on)) };
        }
    }

    /// Spawn a thing of `kind` (its type number) on the floor at (x, y), facing `angle`;
    /// `ambush`: deaf. Its id, or None (no room, or no such call in the library).
    pub fn spawn(&self, kind: i32, x: f64, y: f64, angle: f64, ambush: bool) -> Option<i32> {
        let f = self.spawn_fn?;
        // SAFETY: plain values into the game; see `load`.
        let id = unsafe { f(kind, x, y, angle, c_int::from(ambush)) };
        (id >= 0).then_some(id)
    }

    /// Number things from 1 again (as a new process does); false if the library hasn't the call.
    pub fn reset_ids(&self) -> bool {
        let Some(f) = self.reset_ids_fn else {
            return false;
        };
        // SAFETY: no arguments; see `load`.
        unsafe { f() };
        true
    }

    /// The sounds the game's started since the last call, as she'd hear them (none in a library
    /// without the call): ten numbers each, as the engine's `evil_sound_t` has them.
    pub fn sounds(&self) -> Vec<[i32; 10]> {
        let Some(f) = self.sounds_fn else {
            return Vec::new();
        };
        const MAX: usize = 256;
        let mut raw = [0 as c_int; 10 * MAX];
        // SAFETY: room for MAX sounds of ten ints each, as the call is told; see `load`.
        let n = unsafe { f(raw.as_mut_ptr(), MAX as c_int) }.clamp(0, MAX as c_int) as usize;
        raw.as_chunks::<10>().0.iter().take(n).copied().collect()
    }

    /// The sounds a kind of thing makes, by number (0: none): seeing her (a missile: fired),
    /// attacking, in pain, dying (a missile: going off), wandering. None past the last kind, or
    /// in a library without the call.
    pub fn mobj_sounds(&self, kind: i32) -> Option<[i32; 5]> {
        let f = self.mobj_sounds_fn?;
        let mut out = [0 as c_int; 5];
        // SAFETY: room for the five ints the call writes; see `load`.
        (unsafe { f(kind, out.as_mut_ptr()) } != 0).then_some(out)
    }

    /// The game's sounds' names, by number (the first is none's, empty): asked of the game once
    /// a process, kept for good.
    pub fn sfx_names(&self) -> &'static [&'static str] {
        static NAMES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
        NAMES.get_or_init(|| {
            let mut names: Vec<&'static str> = vec![""];
            let Some(f) = self.sfx_name_fn else {
                return names;
            };
            loop {
                let mut buf = [0 as c_char; 32];
                // SAFETY: a buffer of 32 bytes, as the call is told; it ends what it writes with a 0.
                if unsafe { f(names.len() as c_int, buf.as_mut_ptr(), 32) } == 0 {
                    break;
                }
                let text: Vec<u8> = buf
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                names.push(Box::leak(
                    String::from_utf8_lossy(&text).into_owned().into_boxed_str(),
                ));
            }
            names
        })
    }

    /// The hits on her since the last call (none in a library without the call).
    /// Her bullets and pellets so far, by the game's own count: fired, those that hit a thing
    /// that can be shot, those of them a barrel, those that hit nothing after one of the same
    /// tic had killed; and her shots (a tic's pellets), and those of them with no pellet on a
    /// thing; and the pellets fired with the trigger held since the shot before. For
    /// measuring; None in an older library.
    pub fn pellets(&self) -> Option<[i32; 7]> {
        let f = self.pellets_fn?;
        let mut raw = [0 as c_int; 7];
        // SAFETY: room for the seven ints it writes; see `load`.
        unsafe { f(raw.as_mut_ptr()) };
        Some(raw)
    }

    pub fn hits(&self) -> Vec<Hit> {
        let Some(f) = self.hits_fn else {
            return Vec::new();
        };
        const MAX: usize = 64;
        let mut raw = [0 as c_int; 8 * MAX];
        // SAFETY: room for MAX hits of eight ints each, as the call is told; see `load`.
        let n = unsafe { f(raw.as_mut_ptr(), MAX as c_int) }.clamp(0, MAX as c_int) as usize;
        raw.as_chunks::<8>()
            .0
            .iter()
            .take(n)
            .map(
                |&[health, armor, inflictor, source, own, cause, side, source_id]| Hit {
                    side: (side != 2).then_some(side),
                    source_id: (source_id > 0).then_some(source_id),
                    health,
                    armor,
                    inflictor: (inflictor >= 0).then_some(inflictor),
                    source: (source >= 0).then_some(source),
                    own: own != 0,
                    cause: match cause {
                        0 => HitCause::Thing,
                        1 => HitCause::Crusher,
                        2 => HitCause::Floor,
                        _ => HitCause::Other,
                    },
                },
            )
            .collect()
    }

    /// Take every pickup off the level: how many went (None: no such call in the library).
    pub fn clear_items(&self) -> Option<i32> {
        let f = self.clear_fn?;
        // SAFETY: no arguments; see `load`.
        Some(unsafe { f() })
    }

    /// Give her a loadout (see `Loadout`); false if the library hasn't the call.
    pub fn give(&self, l: &Loadout) -> bool {
        let Some(f) = self.give_fn else { return false };
        // SAFETY: plain values into the game; see `load`.
        unsafe {
            f(
                l.weapons,
                l.clip,
                l.shell,
                l.cell,
                l.rocket,
                c_int::from(l.backpack),
                l.health,
                l.armor,
                l.armor_type,
            )
        };
        // (and the weapon she had up, if it's said and the library can: else the pistol the game
        // starts her with, till she changes it herself)
        if let (Some(ready), Some(w)) = (self.ready_fn, l.ready) {
            // SAFETY: a plain value into the game; see `load`.
            unsafe { ready(w) };
        }
        true
    }

    pub fn god(&self, on: bool) {
        // SAFETY: as above.
        unsafe { (self.god_fn)(c_int::from(on)) }
    }

    /// The random numbers from the next new game on: from `seed` (the same run every time,
    /// given the same commands), or from the clock as the game has them (None: a real run).
    pub fn seed(&self, seed: Option<u32>) {
        // SAFETY: as above.
        unsafe {
            (self.seed_fn)(
                c_int::from(seed.is_some()),
                c_ulong::from(seed.unwrap_or(0)),
            )
        }
    }

    pub fn tic(&self, cmd: TicCmd) {
        let mut buttons = 0;
        if cmd.attack {
            buttons |= BT_ATTACK;
        }
        if cmd.use_ {
            buttons |= BT_USE;
        }
        if let Some(gun) = cmd.exact.filter(|g| (0..=8).contains(g)) {
            buttons |= BT_CHANGE | EVIL_EXACT | (gun << BT_WEAPONSHIFT);
        } else if (1..=7).contains(&cmd.weapon) {
            buttons |= BT_CHANGE | ((cmd.weapon - 1) << BT_WEAPONSHIFT);
        }
        // SAFETY: as above.
        unsafe {
            (self.tic_fn)(
                cmd.forward.clamp(-50, 50),
                cmd.side.clamp(-50, 50),
                cmd.turn.clamp(-32768, 32767),
                buttons,
            )
        }
    }

    /// Put the player at (x, y) facing `angle` (degrees), still, on the floor (for checking
    /// jumps in the game itself). False if something's in the way.
    pub fn warp(&self, x: f64, y: f64, angle: f64) -> bool {
        // SAFETY: plain values into the game; see `load`.
        unsafe { (self.warp_fn)(x, y, angle) != 0 }
    }

    /// Draw the frame as the game would and get it as palette indices, a row at a time:
    /// (width, height, pixels); None when drawing is off (-nodraw).
    pub fn draw(&self) -> Option<(usize, usize, Vec<u8>)> {
        self.draw_at(1.0)
    }

    /// The frame drawn `between` (0 to 1) the tic before and this one, 1 the tic as it is (a
    /// library without evil_draw_at: as it is).
    pub fn draw_at(&self, between: f64) -> Option<(usize, usize, Vec<u8>)> {
        let mut buf = self.draw_buffer.borrow_mut();
        buf.resize(MAX_RENDER.0 * MAX_RENDER.1, 0);
        let (out, max) = (buf.as_mut_ptr(), buf.len() as c_int);
        // SAFETY: the game writes at most `max` bytes into the buffer it's given.
        let wh = unsafe {
            match self.draw_at_fn {
                Some(f) => f(out, max, between),
                None => (self.draw_fn)(out, max),
            }
        };
        if wh <= 0 {
            return None;
        }
        let (w, h) = ((wh >> 16) as usize, (wh & 0xFFFF) as usize);
        Some((w, h, buf[..w * h].to_vec()))
    }

    /// The watcher's window opened for a picture `w` by `h`, as `spec` asks.
    pub fn window_open(
        &self,
        title: &str,
        w: usize,
        h: usize,
        spec: WindowSpec,
    ) -> Result<WindowInfo, String> {
        let f = self
            .window_fns
            .as_ref()
            .ok_or("this engine has no window of its own (rebuild it: tools/build-engine.sh)")?;
        let title = CString::new(title).map_err(|e| e.to_string())?;
        let mut out = [0 as c_int; 2];
        // SAFETY: a string the call only reads, and room for the two values it writes.
        let (ww, wh) = spec.size.unwrap_or((0, 0));
        let flags = c_int::from(spec.vsync) | (c_int::from(spec.fullscreen) << 1);
        let ok = unsafe {
            (f.open)(
                title.as_ptr(),
                w as c_int,
                h as c_int,
                ww as c_int,
                wh as c_int,
                flags,
                out.as_mut_ptr(),
            )
        };
        if ok == 0 {
            return Err("the window didn't open (see the game's log)".into());
        }
        Ok(WindowInfo {
            vsync: out[0] != 0,
            refresh: out[1],
        })
    }

    /// A picture shown in the window (0RGB, `w` by `h`): it waits for the screen's refresh
    /// where the window has vsync.
    pub fn window_show(&self, px: &[u32], w: usize, h: usize) {
        if let Some(f) = &self.window_fns {
            if px.len() >= w * h {
                // SAFETY: the call reads `w * h` pixels, which `px` has.
                unsafe { (f.show)(px.as_ptr(), w as c_int, h as c_int) };
            }
        }
    }

    /// What's happened at the window since the last call.
    pub fn window_events(&self) -> Vec<WindowEvent> {
        let Some(f) = &self.window_fns else {
            return Vec::new();
        };
        let mut out = [0 as c_int; 128];
        // SAFETY: room for 64 pairs, as many as it's told it may write.
        let n = unsafe { (f.events)(out.as_mut_ptr(), 64) }.clamp(0, 64) as usize;
        out[..2 * n]
            .chunks(2)
            .filter_map(|e| match e[0] {
                1 => Some(WindowEvent::Key(e[1])),
                2 => Some(WindowEvent::Wheel(e[1])),
                3 => Some(WindowEvent::Close),
                _ => None,
            })
            .collect()
    }

    pub fn window_title(&self, title: &str) {
        if let (Some(f), Ok(t)) = (&self.window_fns, CString::new(title)) {
            // SAFETY: a string the call only reads.
            unsafe { (f.title)(t.as_ptr()) };
        }
    }

    /// The window's icon (ARGB, `w` by `h`).
    pub fn window_icon(&self, px: &[u32], w: usize, h: usize) {
        if let Some(f) = self.window_icon_fn.filter(|_| px.len() >= w * h) {
            // SAFETY: the call reads `w * h` pixels, which `px` has, and keeps none of them.
            unsafe { f(px.as_ptr(), w as c_int, h as c_int) };
        }
    }

    pub fn window_close(&self) {
        if let Some(f) = &self.window_fns {
            // SAFETY: no arguments; closing a window that isn't open does nothing.
            unsafe { (f.close)() };
        }
    }

    pub fn status(&self) -> Status {
        let mut s = Status::default();
        // SAFETY: the game fills the struct it's given.
        unsafe { (self.status_fn)(&mut s) };
        s
    }

    /// Every sector's (floor, ceiling) now.
    pub fn sectors(&self, out: &mut Vec<(f64, f64)>) {
        let mut flat = self.sector_buffer.borrow_mut();
        // SAFETY: the game writes at most `max` pairs into the buffer it's given.
        let mut n =
            unsafe { (self.sectors_fn)(flat.as_mut_ptr(), (flat.len() / 2) as c_int) } as usize;
        if 2 * n > flat.len() {
            flat.resize(2 * n, 0.0);
            // SAFETY: as above, with room for all.
            n = unsafe { (self.sectors_fn)(flat.as_mut_ptr(), n as c_int) } as usize;
        }
        out.clear();
        out.extend(
            flat[..2 * n]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[floor, ceiling]| (floor, ceiling)),
        );
    }

    /// Every thing about now.
    pub fn things(&self, out: &mut Vec<Mobj>) {
        if out.len() < INITIAL_QUERY_CAPACITY {
            out.resize(INITIAL_QUERY_CAPACITY, Mobj::default());
        }
        loop {
            // SAFETY: the game writes at most `max` things into the buffer it's given.
            let n = unsafe { (self.mobjs_fn)(out.as_mut_ptr(), out.len() as c_int) } as usize;
            if n <= out.len() {
                out.truncate(n);
                return;
            }
            out.resize(n, Mobj::default());
        }
    }

    pub fn player(&self) -> PlayerState {
        // SAFETY: the struct is plain numbers: all zeros is a valid one.
        let mut p: PlayerState = unsafe { std::mem::zeroed() };
        // SAFETY: the game fills the struct it's given.
        unsafe { (self.player_fn)(&mut p) };
        p
    }
}

/// What to give her in an arena: weapons by weapontype bit (fist 0, pistol 1, shotgun 2,
/// chaingun 3, rocket launcher 4, plasma 5, BFG 6, chainsaw 7, super shotgun 8), ammo, the
/// backpack, health, armor (type 1 green, 2 blue).
#[derive(Clone, Debug)]
pub struct Loadout {
    pub weapons: i32,
    pub clip: i32,
    pub shell: i32,
    pub cell: i32,
    pub rocket: i32,
    pub backpack: bool,
    pub health: i32,
    pub armor: i32,
    pub armor_type: i32,
    /// the weapon in her hands (the game's numbering, as `weapons`' bits); None: as the game
    /// starts her
    pub ready: Option<i32>,
}

/// What hurt her, as the game has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitCause {
    /// a thing (a monster's attack, a missile, a barrel going up)
    Thing,
    /// a ceiling coming down on her
    Crusher,
    /// a floor that hurts
    Floor,
    /// the level otherwise (a death line, a telefrag)
    Other,
}

/// A hit on her: what it took and where it came from.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub health: i32,
    pub armor: i32,
    /// the thing type that hit her (a missile, a barrel, the monster itself for a bullet or a bite)
    pub inflictor: Option<i32>,
    /// whose it was (the monster that fired it, whoever set the barrel off)
    pub source: Option<i32>,
    /// her own (a rocket of hers too close, a barrel she shot)
    pub own: bool,
    pub cause: HitCause,
    /// where the status bar's face looks for it: -1 her left, 0 straight at it (within 45
    /// degrees of her facing), 1 her right (behind is one side or the other); None: no one's
    pub side: Option<i32>,
    /// whose it was, by its id: for measuring a run (was what hit her something she knew
    /// of?), never hers to know
    pub source_id: Option<i32>,
}

impl Hit {
    /// Where it came from, for the log and the tallies: "ShotgunGuy", "Cyberdemon's Rocket",
    /// "her own Rocket", "a barrel set off by her", "a crusher", "the floor".
    pub fn from(&self) -> String {
        let name = |k: i32| type_name(k).map_or_else(|| format!("thing type {k}"), str::to_string);
        match self.cause {
            HitCause::Crusher => return "a crusher".into(),
            HitCause::Floor => return "the floor".into(),
            HitCause::Other => return "the level".into(),
            HitCause::Thing => {}
        }
        let barrel = type_of("ExplosiveBarrel");
        match (self.inflictor, self.source) {
            (Some(i), s) if Some(i) == barrel => match s {
                _ if self.own => "a barrel set off by her".into(),
                Some(s) if Some(s) != barrel => format!("a barrel set off by {}", name(s)),
                _ => "a barrel".into(),
            },
            (Some(i), _) if self.own => format!("her own {}", name(i)),
            (Some(i), Some(s)) if i != s => format!("{}'s {}", name(s), name(i)),
            (Some(i), _) => name(i),
            (None, Some(s)) => name(s),
            (None, None) => "something".into(),
        }
    }
}

/// The type number of a thing Evil knows by `name` (`type_name` backwards).
pub fn type_of(name: &str) -> Option<i32> {
    (0..200).find(|&k| type_name(k) == Some(name))
}

/// The name Evil knows a thing type by (ZDoom's class names, as ViZDoom gave them), for the
/// types Doom and Doom II have (DSDA-Doom keeps vanilla's numbering); None for decorations
/// and the like.
pub fn type_name(kind: i32) -> Option<&'static str> {
    Some(match kind {
        0 => "DoomPlayer",
        1 => "Zombieman",
        2 => "ShotgunGuy",
        3 => "Archvile",
        4 => "ArchvileFire",
        5 => "Revenant",
        6 => "RevenantTracer",
        8 => "Fatso",
        9 => "FatShot",
        10 => "ChaingunGuy",
        11 => "DoomImp",
        12 => "Demon",
        13 => "Spectre",
        14 => "Cacodemon",
        15 => "BaronOfHell",
        16 => "BaronBall",
        17 => "HellKnight",
        18 => "LostSoul",
        19 => "SpiderMastermind",
        20 => "Arachnotron",
        21 => "Cyberdemon",
        22 => "PainElemental",
        23 => "WolfensteinSS",
        24 => "CommanderKeen",
        25 => "BossBrain",
        26 => "BossEye",
        27 => "BossTarget",
        28 => "SpawnShot",
        30 => "ExplosiveBarrel",
        31 => "DoomImpBall",
        32 => "CacodemonBall",
        33 => "Rocket",
        34 => "PlasmaBall",
        35 => "BFGBall",
        36 => "ArachnotronPlasma",
        43 => "GreenArmor",
        44 => "BlueArmor",
        45 => "HealthBonus",
        46 => "ArmorBonus",
        47 => "BlueCard",
        48 => "RedCard",
        49 => "YellowCard",
        50 => "YellowSkull",
        51 => "RedSkull",
        52 => "BlueSkull",
        53 => "Stimpack",
        54 => "Medikit",
        55 => "Soulsphere",
        56 => "InvulnerabilitySphere",
        57 => "Berserk",
        58 => "BlurSphere",
        59 => "RadSuit",
        60 => "Allmap",
        61 => "Infrared",
        62 => "Megasphere",
        63 => "Clip",
        64 => "ClipBox",
        65 => "RocketAmmo",
        66 => "RocketBox",
        67 => "Cell",
        68 => "CellPack",
        69 => "Shell",
        70 => "ShellBox",
        71 => "Backpack",
        // (solid decorations, for arenas: in the way of her and of what she shoots low, 16 high
        // to a missile as all things of theirs are; MT_MISC32, MT_MISC34, MT_MISC48)
        82 => "TallGreenColumn",
        84 => "TallRedColumn",
        98 => "TechPillar",
        72 => "BFG9000",
        73 => "Chaingun",
        74 => "Chainsaw",
        75 => "RocketLauncher",
        76 => "PlasmaRifle",
        77 => "Shotgun",
        78 => "SuperShotgun",
        _ => return None,
    })
}

/// The weapon slot (1-7) a weapontype_t is in.
pub fn weapon_slot(weapon: i32) -> i32 {
    match weapon {
        0 | 7 => 1, // fist, chainsaw
        1 => 2,
        2 | 8 => 3, // shotgun, super shotgun
        3 => 4,
        4 => 5,
        5 => 6,
        6 => 7,
        _ => 0,
    }
}

/// The library, its own libraries looked for in `dll_dirs` too.
#[cfg(windows)]
fn open_library(lib: &Path, dll_dirs: &[&Path]) -> Result<Library, libloading::Error> {
    use std::os::windows::ffi::OsStrExt;
    /// LoadLibraryExW: the application's folder, System32 and the folders added.
    const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;
    /// ...and the library's own folder.
    const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
    #[link(name = "kernel32")]
    extern "system" {
        fn AddDllDirectory(new_directory: *const u16) -> *mut std::ffi::c_void;
    }
    for dir in dll_dirs {
        let wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: a nul-terminated wide string that outlives the call.
        unsafe { AddDllDirectory(wide.as_ptr()) };
    }
    // SAFETY: the library is dsda/evil_api.c built with DSDA-Doom; the functions taken from it
    // have exactly the C signatures they're given here.
    let lib = unsafe {
        libloading::os::windows::Library::load_with_flags(
            lib,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        )
    }?;
    Ok(lib.into())
}

/// The library (its own libraries where the system's loader looks).
#[cfg(not(windows))]
fn open_library(lib: &Path, _dll_dirs: &[&Path]) -> Result<Library, libloading::Error> {
    // SAFETY: as above.
    unsafe { Library::new(lib) }
}
