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
type SoundFn = unsafe extern "C" fn(c_int);
type SpawnFn = unsafe extern "C" fn(c_int, f64, f64, f64, c_int) -> c_int;
type GiveFn = unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int);
type ClearFn = unsafe extern "C" fn() -> c_int;
type ResetIdsFn = unsafe extern "C" fn();
type HitsFn = unsafe extern "C" fn(*mut c_int, c_int) -> c_int;
type SoundsFn = unsafe extern "C" fn(*mut c_int, c_int) -> c_int;
type SfxNameFn = unsafe extern "C" fn(c_int, *mut c_char, c_int) -> c_int;
type MobjSoundsFn = unsafe extern "C" fn(c_int, *mut c_int) -> c_int;
type GrabSoundFn = unsafe extern "C" fn(*mut i16, c_int) -> c_int;
type SoundRateFn = unsafe extern "C" fn() -> c_int;

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
    clear_fn: Option<ClearFn>,
    reset_ids_fn: Option<ResetIdsFn>,
    hits_fn: Option<HitsFn>,
    sounds_fn: Option<SoundsFn>,
    sfx_name_fn: Option<SfxNameFn>,
    mobj_sounds_fn: Option<MobjSoundsFn>,
    grab_sound_fn: Option<GrabSoundFn>,
    sound_rate_fn: Option<SoundRateFn>,
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
        // SAFETY: as above (evil_sound(int)).
        let sound_fn = sym(b"evil_sound").ok().map(|p| unsafe { std::mem::transmute::<*const (), SoundFn>(p) });
        // SAFETY: as above, evil_spawn's and evil_give's signatures; optional.
        let spawn_fn = sym(b"evil_spawn").ok().map(|p| unsafe { std::mem::transmute::<*const (), SpawnFn>(p) });
        let give_fn = sym(b"evil_give").ok().map(|p| unsafe { std::mem::transmute::<*const (), GiveFn>(p) });
        // SAFETY: as above, evil_clear_items's signature; optional.
        let clear_fn = sym(b"evil_clear_items").ok().map(|p| unsafe { std::mem::transmute::<*const (), ClearFn>(p) });
        // SAFETY: as above, evil_reset_ids's signature; optional.
        let reset_ids_fn = sym(b"evil_reset_ids").ok().map(|p| unsafe { std::mem::transmute::<*const (), ResetIdsFn>(p) });
        // SAFETY: as above, evil_hits_since's signature; optional.
        let hits_fn = sym(b"evil_hits_since").ok().map(|p| unsafe { std::mem::transmute::<*const (), HitsFn>(p) });
        // SAFETY: as above, evil_sounds_since's and evil_sfx_name's signatures; optional.
        let sounds_fn = sym(b"evil_sounds_since").ok().map(|p| unsafe { std::mem::transmute::<*const (), SoundsFn>(p) });
        let mobj_sounds_fn = sym(b"evil_mobj_sounds").ok().map(|p| unsafe { std::mem::transmute::<*const (), MobjSoundsFn>(p) });
        let sfx_name_fn = sym(b"evil_sfx_name").ok().map(|p| unsafe { std::mem::transmute::<*const (), SfxNameFn>(p) });
        // SAFETY: as above, evil_grab_sound's signature; optional.
        let grab_sound_fn = sym(b"evil_grab_sound").ok().map(|p| unsafe { std::mem::transmute::<*const (), GrabSoundFn>(p) });
        // SAFETY: as above, evil_sound_rate's signature; optional.
        let sound_rate_fn = sym(b"evil_sound_rate").ok().map(|p| unsafe { std::mem::transmute::<*const (), SoundRateFn>(p) });
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
            sound_fn,
            spawn_fn,
            give_fn,
            clear_fn,
            reset_ids_fn,
            hits_fn,
            sounds_fn,
            sfx_name_fn,
            mobj_sounds_fn,
            grab_sound_fn,
            sound_rate_fn,
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

    /// The game's sound for the next `samples` sample frames, mixed now (16-bit stereo,
    /// interleaved), and its sample rate; None without -evilsoundcap (or the call).
    pub fn grab_sound(&self, samples: usize) -> Option<(u32, Vec<i16>)> {
        let f = self.grab_sound_fn?;
        let mut out = vec![0i16; samples * 2];
        // SAFETY: out holds 2 * samples shorts, all the call writes; see `load`.
        let rate = unsafe { f(out.as_mut_ptr(), c_int::try_from(samples).ok()?) };
        u32::try_from(rate).ok().filter(|&r| r > 0).map(|r| (r, out))
    }

    /// The sound's sample rate (None without the call, or no sound).
    pub fn sound_rate(&self) -> Option<u32> {
        let f = self.sound_rate_fn?;
        // SAFETY: no arguments; see `load`.
        u32::try_from(unsafe { f() }).ok().filter(|&r| r > 0)
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
        let Some(f) = self.reset_ids_fn else { return false };
        // SAFETY: no arguments; see `load`.
        unsafe { f() };
        true
    }

    /// The sounds the game's started since the last call, as she'd hear them (none in a library
    /// without the call): nine numbers each, as the engine's `evil_sound_t` has them.
    pub fn sounds(&self) -> Vec<[i32; 9]> {
        let Some(f) = self.sounds_fn else { return Vec::new() };
        const MAX: usize = 256;
        let mut raw = [0 as c_int; 9 * MAX];
        // SAFETY: room for MAX sounds of nine ints each, as the call is told; see `load`.
        let n = unsafe { f(raw.as_mut_ptr(), MAX as c_int) }.clamp(0, MAX as c_int) as usize;
        raw.as_chunks::<9>().0.iter().take(n).copied().collect()
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
            let Some(f) = self.sfx_name_fn else { return names };
            loop {
                let mut buf = [0 as c_char; 32];
                // SAFETY: a buffer of 32 bytes, as the call is told; it ends what it writes with a 0.
                if unsafe { f(names.len() as c_int, buf.as_mut_ptr(), 32) } == 0 {
                    break;
                }
                let text: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
                names.push(Box::leak(String::from_utf8_lossy(&text).into_owned().into_boxed_str()));
            }
            names
        })
    }

    /// The hits on her since the last call (none in a library without the call).
    pub fn hits(&self) -> Vec<Hit> {
        let Some(f) = self.hits_fn else { return Vec::new() };
        const MAX: usize = 64;
        let mut raw = [0 as c_int; 7 * MAX];
        // SAFETY: room for MAX hits of seven ints each, as the call is told; see `load`.
        let n = unsafe { f(raw.as_mut_ptr(), MAX as c_int) }.clamp(0, MAX as c_int) as usize;
        raw.as_chunks::<7>()
            .0
            .iter()
            .take(n)
            .map(|&[health, armor, inflictor, source, own, cause, side]| Hit {
                side: (side != 2).then_some(side),
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
            })
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
        unsafe { f(l.weapons, l.clip, l.shell, l.cell, l.rocket, c_int::from(l.backpack), l.health, l.armor, l.armor_type) };
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
