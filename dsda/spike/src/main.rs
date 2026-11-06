//! Spike: DSDA-Doom in-process, a tic at a time, its state read straight from memory.
//! How fast does E1M3 (monsters, Hurt Me Plenty) go, reading everything Evil would each tic?
use std::ffi::{c_char, c_int, CString};
use std::time::Instant;

use libloading::{Library, Symbol};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Mobj {
    id: u64,
    kind: c_int,
    x: f64,
    y: f64,
    z: f64,
    momx: f64,
    momy: f64,
    momz: f64,
    angle: f64,
    health: c_int,
    flags: u64,
}

#[repr(C)]
#[derive(Default)]
struct Player {
    x: f64,
    y: f64,
    z: f64,
    angle: f64,
    momx: f64,
    momy: f64,
    health: c_int,
    armor: c_int,
    readyweapon: c_int,
    gametic: c_int,
    weaponowned: [c_int; 9],
    ammo: [c_int; 4],
    playerstate: c_int,
}

const MF_COUNTKILL: u64 = 0x0040_0000;

/// A small, fixed random stream (the walk is the same every run).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

type Init = unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int;
type Tic = unsafe extern "C" fn(c_int, c_int, c_int, c_int);
type Sectors = unsafe extern "C" fn(*mut f64, c_int) -> c_int;
type Mobjs = unsafe extern "C" fn(*mut Mobj, c_int) -> c_int;
type PlayerState = unsafe extern "C" fn(*mut Player);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dll = args.next().ok_or("usage: dsda-spike <libdsda-evil.dll> <DOOM.WAD> [tics]")?;
    let iwad = args.next().ok_or("usage: dsda-spike <libdsda-evil.dll> <DOOM.WAD> [tics]")?;
    let tics: usize = args.next().map(|t| t.parse()).transpose()?.unwrap_or(20_000);

    let t_load = Instant::now();
    // SAFETY: the library is ours (dsda/evil_api.c built with DSDA-Doom); its functions take
    // and fill plain C data as declared above.
    let lib = unsafe { Library::new(&dll)? };
    let (init, tic, sectors, mobjs, player_state) = unsafe {
        let init: Symbol<Init> = lib.get(b"evil_init")?;
        let tic: Symbol<Tic> = lib.get(b"evil_tic")?;
        let sectors: Symbol<Sectors> = lib.get(b"evil_sectors")?;
        let mobjs: Symbol<Mobjs> = lib.get(b"evil_mobjs")?;
        let player_state: Symbol<PlayerState> = lib.get(b"evil_player_state")?;
        (*init, *tic, *sectors, *mobjs, *player_state)
    };
    let argv: Vec<CString> = ["evil", "-iwad", &iwad, "-skill", "3", "-warp", "1", "3",
                              "-nodraw", "-nosound", "-nomusic"]
        .iter()
        .map(|a| CString::new(*a))
        .collect::<Result<_, _>>()?;
    let mut ptrs: Vec<*mut c_char> = argv.iter().map(|a| a.as_ptr().cast_mut()).collect();
    // SAFETY: argv outlives the call (the game keeps pointers into it: it lives to the end).
    unsafe { init(ptrs.len() as c_int, ptrs.as_mut_ptr()) };
    println!("loaded and set up in {:.2}s", t_load.elapsed().as_secs_f64());

    let mut heights = vec![0.0f64; 2 * 4096];
    let mut things = vec![Mobj::default(); 8192];
    let mut me = Player::default();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut stepping, mut reading) = (0.0f64, 0.0f64);
    let (mut deaths, mut was_dead) = (0, false);
    let (mut most_things, mut sum_things, mut monsters_start, mut monsters_end) = (0, 0usize, 0, 0);
    let mut turn = 0;
    let start = Instant::now();
    for i in 0..tics {
        if i % 35 == 0 {
            turn = [0, 0, 4, -4, 12, -12][rng.below(6) as usize] << 8;
        }
        let forward = if rng.below(10) < 8 { 50 } else { -25 };
        let side = [0, 0, 40, -40][rng.below(4) as usize];
        let buttons = i32::from(rng.below(10) < 3) | if i % 70 == 0 { 2 } else { 0 };
        let t0 = Instant::now();
        // SAFETY: as above.
        unsafe { tic(forward, side, turn, buttons) };
        let t1 = Instant::now();
        let (nsec, nthings) = unsafe {
            let nsec = sectors(heights.as_mut_ptr(), (heights.len() / 2) as c_int);
            let n = mobjs(things.as_mut_ptr(), things.len() as c_int);
            player_state(&mut me);
            (nsec, n as usize)
        };
        reading += t1.elapsed().as_secs_f64();
        stepping += (t1 - t0).as_secs_f64();
        let n = nthings.min(things.len());
        most_things = most_things.max(nthings);
        sum_things += nthings;
        let alive = things[..n].iter().filter(|t| t.flags & MF_COUNTKILL != 0 && t.health > 0).count();
        if i == 10 {
            monsters_start = alive;
            println!("tic {i}: {nsec} sectors, {nthings} things, {alive} monsters alive, \
                      player at ({:.0}, {:.0}), health {}", me.x, me.y, me.health);
        }
        monsters_end = alive;
        let dead = me.playerstate != 0;
        if dead && !was_dead {
            deaths += 1;
        }
        was_dead = dead;
    }
    let wall = start.elapsed().as_secs_f64();
    println!("{tics} tics in {wall:.2}s: {:.0} tics/s (stepping {:.0}%, reading {:.0}%)",
             tics as f64 / wall, 100.0 * stepping / wall, 100.0 * reading / wall);
    println!("things: {:.0} on average, {most_things} at most; monsters alive {monsters_start} \
              near the start, {monsters_end} at the end; {deaths} deaths; gametic {}",
             sum_things as f64 / tics as f64, me.gametic);
    Ok(())
}
