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
    /// (the game keeps pointers into its arguments: they live as long as it does)
    _argv: Vec<CString>,
    _argv_ptrs: Vec<*mut c_char>,
    /// (last: the functions above point into it)
    _lib: Library,
}

impl Dsda {
    /// Load the library and set the game up (`args` as for dsda-doom.exe: -iwad and so on);
    /// `dll_dirs`: where else its own libraries are (Windows: MSYS2's SDL2 and the rest).
    /// Once a process.
    pub fn load(lib: &Path, dll_dirs: &[&Path], args: &[String]) -> Result<Self, String> {
        if LOADED.swap(true, Ordering::SeqCst) {
            return Err("DSDA-Doom is already loaded in this process (one game a process)".into());
        }
        let lib = open_library(lib, dll_dirs).map_err(|e| format!("loading {}: {e}", lib.display()))?;
        let sym = |name: &[u8]| -> Result<*const (), String> {
            // SAFETY: as above; the pointer is only turned into the matching fn type.
            unsafe { lib.get::<*const ()>(name) }.map(|s| *s).map_err(|e| format!("{}: {e}", String::from_utf8_lossy(name)))
        };
        // SAFETY: each symbol is the function of that name in evil_api.c, with this signature.
        let (init_fn, new_game_fn, god_fn, seed_fn, tic_fn, warp_fn, draw_fn, status_fn, sectors_fn, mobjs_fn, player_fn) = unsafe {
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
        let power_fn = sym(b"evil_power").ok().map(|p| unsafe { std::mem::transmute::<*const (), PowerFn>(p) });
        let argv: Vec<CString> =
            std::iter::once("evil".to_string()).chain(args.iter().cloned()).map(CString::new).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
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
            _argv: argv,
            _argv_ptrs: argv_ptrs,
            _lib: lib,
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

    pub fn god(&self, on: bool) {
        // SAFETY: as above.
        unsafe { (self.god_fn)(c_int::from(on)) }
    }

    /// The random numbers from the next new game on: from `seed` (the same run every time,
    /// given the same commands), or from the clock as the game has them (None: a real run).
    pub fn seed(&self, seed: Option<u32>) {
        // SAFETY: as above.
        unsafe { (self.seed_fn)(c_int::from(seed.is_some()), c_ulong::from(seed.unwrap_or(0))) }
    }

    pub fn tic(&self, cmd: TicCmd) {
        let mut buttons = 0;
        if cmd.attack {
            buttons |= BT_ATTACK;
        }
        if cmd.use_ {
            buttons |= BT_USE;
        }
        if (1..=7).contains(&cmd.weapon) {
            buttons |= BT_CHANGE | ((cmd.weapon - 1) << BT_WEAPONSHIFT);
        }
        // SAFETY: as above.
        unsafe { (self.tic_fn)(cmd.forward.clamp(-50, 50), cmd.side.clamp(-50, 50), cmd.turn.clamp(-32768, 32767), buttons) }
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
        let mut buf = vec![0u8; 3840 * 2160];
        // SAFETY: the game writes at most `max` bytes into the buffer it's given.
        let wh = unsafe { (self.draw_fn)(buf.as_mut_ptr(), buf.len() as c_int) };
        if wh <= 0 {
            return None;
        }
        let (w, h) = ((wh >> 16) as usize, (wh & 0xFFFF) as usize);
        buf.truncate(w * h);
        Some((w, h, buf))
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
        let mut n = unsafe { (self.sectors_fn)(flat.as_mut_ptr(), (flat.len() / 2) as c_int) } as usize;
        if 2 * n > flat.len() {
            flat.resize(2 * n, 0.0);
            // SAFETY: as above, with room for all.
            n = unsafe { (self.sectors_fn)(flat.as_mut_ptr(), n as c_int) } as usize;
        }
        out.clear();
        out.extend(flat[..2 * n].as_chunks::<2>().0.iter().map(|&[floor, ceiling]| (floor, ceiling)));
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
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: a nul-terminated wide string that outlives the call.
        unsafe { AddDllDirectory(wide.as_ptr()) };
    }
    // SAFETY: the library is dsda/evil_api.c built with DSDA-Doom; the functions taken from it
    // have exactly the C signatures they're given here.
    let lib = unsafe { libloading::os::windows::Library::load_with_flags(lib, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR) }?;
    Ok(lib.into())
}

/// The library (its own libraries where the system's loader looks).
#[cfg(not(windows))]
fn open_library(lib: &Path, _dll_dirs: &[&Path]) -> Result<Library, libloading::Error> {
    // SAFETY: as above.
    unsafe { Library::new(lib) }
}
