//! DSDA-Doom as Evil's engine: the game in-process (`dsda`), a tic at a time, its state read
//! into Observations. One game a process: DSDA-Doom is all globals, so sweeps give each map a
//! process of its own.

use crate::config;
use crate::dsda::{self, Dsda, Mobj, TicCmd};
use crate::motor::{Controls, Observation, Thing};
use crate::nav::Heights;
use crate::specials::{Key, Keys};
use crate::tactical::bestiary::category;
use std::path::{Path, PathBuf};

/// (the level loads on the first, then a few idle tics)
pub const WARMUP_TICS: i64 = 8;

/// Where the game's library and its own WAD are built.
pub fn build_dir() -> PathBuf {
    config::root().join("third_party").join("dsda-doom").join("prboom2").join("build")
}

pub fn library() -> PathBuf {
    build_dir().join(if cfg!(windows) { "libdsda-evil.dll" } else { "libdsda-evil.so" })
}

/// (episode, map) from a map name: E1M1 or MAP01.
pub fn episode_map(name: &str) -> Result<(i32, i32), String> {
    let n = name.to_uppercase();
    let bad = || format!("not a map name: {name}");
    if let Some(rest) = n.strip_prefix("MAP") {
        return Ok((1, rest.parse().map_err(|_| bad())?));
    }
    let b = n.as_bytes();
    if b.len() == 4 && b[0] == b'E' && b[2] == b'M' && b[1].is_ascii_digit() && b[3].is_ascii_digit() {
        return Ok((i32::from(b[1] - b'0'), i32::from(b[3] - b'0')));
    }
    Err(bad())
}

pub struct Options {
    pub skill: i32,
    pub nomonsters: bool,
    /// things are reported (monsters, items: the fight and the 100% runs need them)
    pub objects: bool,
    /// frames are drawn (for watching)
    pub drawn: bool,
    /// the game's random numbers from this (testing); None: the clock, as in a real game
    pub seed: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Options { skill: 3, nomonsters: true, objects: false, drawn: false, seed: None }
    }
}

pub struct Engine {
    pub game: Dsda,
    pub map_name: String,
    pub iwad: PathBuf,
    episode: i32,
    map: i32,
    skill: i32,
    nomonsters: bool,
    objects: bool,
    pub tic: i64,
    god: bool,
    suits: i32,
    ironfeet: i32,
    message_tic: i32,
    sectors: Vec<(f64, f64)>,
    mobjs: Vec<Mobj>,
}

impl Engine {
    pub fn new(iwad: &Path, map_name: &str, o: &Options) -> Result<Engine, String> {
        let (episode, map) = episode_map(map_name)?;
        set_env_default("DOOMWADDIR", &build_dir());
        let data = config::out_dir().join("dsda");
        let _ = std::fs::create_dir_all(&data);
        set_env_default("DOOMDATADIR", &data);
        // (SDL's dummy drivers while the game sets itself up: no sound device, and drawn or not
        // nothing goes to a window of its own)
        std::env::set_var("SDL_AUDIODRIVER", "dummy");
        if !o.drawn {
            std::env::set_var("SDL_VIDEODRIVER", "dummy");
        }
        // straight into the level (no title screen: its attract-mode demo took her commands)
        let warp: Vec<String> = if map_name.to_uppercase().starts_with("MAP") { vec![map.to_string()] } else { vec![episode.to_string(), map.to_string()] };
        let mut args: Vec<String> =
            ["-iwad", &iwad.to_string_lossy(), "-nosound", "-nomusic", "-nomouse", "-skill", &o.skill.to_string()].iter().map(|s| s.to_string()).collect();
        if !o.drawn {
            args.push("-nodraw".into());
        }
        args.push("-warp".into());
        args.extend(warp);
        if o.nomonsters {
            args.push("-nomonsters".into());
        }
        let dirs = dll_dirs();
        let game = Dsda::load(&library(), &dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>(), &args)?;
        game.seed(o.seed);
        Ok(Engine {
            game,
            map_name: map_name.to_uppercase(),
            iwad: iwad.to_path_buf(),
            episode,
            map,
            skill: o.skill,
            nomonsters: o.nomonsters,
            objects: o.objects,
            tic: 0,
            god: false,
            suits: 0,
            ironfeet: 0,
            message_tic: -1,
            sectors: Vec::new(),
            mobjs: Vec::new(),
        })
    }

    pub fn start(&mut self) {
        self.new_game();
        self.tic = WARMUP_TICS;
    }

    /// The level from the start again (`keep_time`: the tic count carries on).
    pub fn restart(&mut self, keep_time: bool) {
        let tic = self.tic;
        self.new_game();
        self.tic = if keep_time { tic + WARMUP_TICS } else { WARMUP_TICS };
    }

    fn new_game(&mut self) {
        self.game.new_game(self.skill, self.episode, self.map, self.nomonsters);
        self.suits = 0;
        self.ironfeet = 0;
        self.message_tic = -1;
        for _ in 0..WARMUP_TICS {
            self.game.tic(TicCmd::default());
        }
        if self.god {
            self.game.god(true);
        }
    }

    /// Damage does nothing to her (kept across restarts).
    pub fn set_god(&mut self) {
        self.god = true;
        self.game.god(true);
    }

    /// The level's tally so far: items, kills, secrets, each (got, of).
    pub fn tally(&self) -> [(i32, i32); 3] {
        let s = self.game.status();
        [(s.items, s.total_items), (s.kills, s.total_kills), (s.secrets, s.total_secrets)]
    }

    pub fn exited(&self) -> bool {
        self.game.status().exited != 0
    }

    pub fn secret_exit(&self) -> bool {
        self.game.status().secret != 0
    }

    pub fn dead(&self) -> bool {
        self.game.player().playerstate != 0
    }

    /// The level was left or she died.
    pub fn finished(&self) -> bool {
        self.exited() || self.dead()
    }

    pub fn observe(&mut self) -> Observation {
        let p = self.game.player();
        self.game.sectors(&mut self.sectors);
        let heights: Vec<Heights> = self.sectors.iter().map(|&(floor, ceiling)| Heights { floor, ceiling }).collect();
        self.game.things(&mut self.mobjs);
        let mut barrels = Vec::new();
        let mut things = self.objects.then(Vec::new);
        for m in &self.mobjs {
            if m.kind == 0 || m.flags & dsda::MF_CORPSE != 0 || (m.flags & dsda::MF_SHOOTABLE != 0 && m.health <= 0) {
                continue;
            }
            let Some(name) = dsda::type_name(m.kind) else { continue };
            if name == "ExplosiveBarrel" {
                barrels.push((m.x, m.y));
            }
            if let Some(things) = things.as_mut() {
                let category = category(name);
                if !matches!(name, "DoomPlayer" | "BossTarget") && category != "Other" {
                    things.push(Thing { id: m.id, name, category, x: m.x, y: m.y, z: m.z, vx: m.momx, vy: m.momy, vz: m.momz, angle: m.angle });
                }
            }
        }
        if p.ironfeet > self.ironfeet {
            self.suits += 1; // (a suit picked up: its time went up)
        }
        self.ironfeet = p.ironfeet;
        let message = (p.message_tic != self.message_tic).then(|| {
            self.message_tic = p.message_tic;
            let text: Vec<u8> = p.message.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
            String::from_utf8_lossy(&text).into_owned()
        });
        let keys: Keys = Key::ENGINE_ORDER.iter().zip(p.cards).filter(|(_, held)| *held != 0).map(|(k, _)| *k).collect();
        let [clip, shell, cell, rocket] = p.ammo;
        let weapons: Vec<i32> =
            (0..9).filter(|&w| p.weaponowned[w as usize] != 0).map(dsda::weapon_slot).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        Observation {
            x: p.x,
            y: p.y,
            angle: p.angle,
            heights,
            tic: self.tic,
            keys: Some(keys),
            z: Some(p.z),
            weapon: Some(dsda::weapon_slot(p.readyweapon)),
            barrels: Some(barrels),
            health: Some(f64::from(p.health)),
            suits: Some(self.suits),
            things,
            weapons: Some(weapons),
            ammo: Some(vec![(1, 0), (2, clip), (3, shell), (4, clip), (5, rocket), (6, cell), (7, cell)]),
            armor: Some(f64::from(p.armor)),
            super_shotgun: Some(p.weaponowned[8] != 0),
            velocity: Some((p.momx, p.momy)),
            allmap: Some(p.allmap != 0),
            message,
        }
    }

    pub fn step(&mut self, c: &Controls) {
        let scale = if c.run { 1.0 } else { 0.5 };
        self.game.tic(TicCmd {
            forward: (c.forward * scale).round() as i32,
            side: (c.strafe * scale).round() as i32,
            turn: (-c.turn * 65536.0 / 360.0).round() as i32,
            attack: c.attack,
            use_: c.use_,
            weapon: c.weapon,
        });
        self.tic += 1;
    }

    /// The game's frame as drawn (the view and status bar): (width, height, palette indices).
    pub fn frame(&self) -> Option<(usize, usize, Vec<u8>)> {
        self.game.draw()
    }
}

fn set_env_default(var: &str, value: &Path) {
    if std::env::var_os(var).is_none() {
        std::env::set_var(var, value);
    }
}

/// Where the library's own DLLs (SDL2 and the rest) are, on Windows: MSYS2's (EVIL_MINGW_BIN,
/// or C:\msys64\mingw64\bin).
fn dll_dirs() -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let bin = std::env::var_os("EVIL_MINGW_BIN").map_or_else(|| PathBuf::from(r"C:\msys64\mingw64\bin"), PathBuf::from);
    if bin.is_dir() {
        vec![bin]
    } else {
        Vec::new()
    }
}
