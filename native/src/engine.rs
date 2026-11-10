//! DSDA-Doom as Evil's engine: the game in-process (`dsda`), a tic at a time, its state read
//! into Observations. One game a process: DSDA-Doom is all globals, so sweeps give each map a
//! process of its own.

use crate::config;
use crate::dsda::{self, Dsda, Mobj, TicCmd};
use crate::motor::{Controls, Observation, Sound, Thing};
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
    /// the game's sound and music, out loud (a watcher's)
    pub sound: bool,
    /// the game's sound mixed for the caller to take a tic at a time (`grab_sound`), not played
    /// (a recording's)
    pub capture: bool,
    /// the game's random numbers from this (testing); None: the clock, as in a real game
    pub seed: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Options { skill: 3, nomonsters: true, objects: false, drawn: false, sound: false, capture: false, seed: None }
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
    /// what she has at the start of the level (and every start again): what she brought from
    /// the one before, playing an episode through
    pub carry: Option<crate::dsda::Loadout>,
    /// the hits on her since they were last taken (`take_hits`)
    hits: Vec<crate::dsda::Hit>,
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
        if o.sound && !o.capture {
            // (the system's own, unless one's asked for; not the dummy one set before)
            if std::env::var_os("SDL_AUDIODRIVER").is_some_and(|v| v == "dummy") {
                std::env::remove_var("SDL_AUDIODRIVER");
            }
        } else {
            std::env::set_var("SDL_AUDIODRIVER", "dummy");
        }
        if !o.drawn {
            std::env::set_var("SDL_VIDEODRIVER", "dummy");
        }
        // straight into the level (no title screen: its attract-mode demo took her commands)
        let warp: Vec<String> = if map_name.to_uppercase().starts_with("MAP") { vec![map.to_string()] } else { vec![episode.to_string(), map.to_string()] };
        let mut args: Vec<String> =
            ["-iwad", &iwad.to_string_lossy(), "-nomouse", "-skill", &o.skill.to_string()].iter().map(|s| s.to_string()).collect();
        if o.capture {
            args.push("-evilsoundcap".into()); // (a dummy device: mixed for her, not played)
        } else if !o.sound {
            args.extend(["-nosound".to_string(), "-nomusic".to_string()]);
        }
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
        if o.sound || o.capture {
            game.sound(true);
        }
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
            carry: None,
            hits: Vec::new(),
        })
    }

    pub fn start(&mut self) {
        self.new_game();
        self.tic = WARMUP_TICS;
    }

    /// What she has now, to carry into the next level as Doom does (weapons, ammo, the backpack,
    /// health, armour; not keys or powers).
    pub fn loadout(&self) -> crate::dsda::Loadout {
        let p = self.game.player();
        let weapons = p.weaponowned.iter().enumerate().filter(|(_, &o)| o != 0).fold(0, |m, (i, _)| m | (1 << i));
        let [clip, shell, cell, rocket] = p.ammo;
        crate::dsda::Loadout { weapons, clip, shell, cell, rocket, backpack: p.backpack != 0, health: p.health, armor: p.armor, armor_type: p.armortype }
    }

    /// On to another map, started as a new process would start it (things numbered afresh), with
    /// `carry` (an episode's next level).
    pub fn goto(&mut self, map_name: &str) -> Result<(), String> {
        let (episode, map) = episode_map(map_name)?;
        (self.episode, self.map) = (episode, map);
        self.map_name = map_name.to_uppercase();
        self.game.reset_ids();
        self.start();
        Ok(())
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
        if let Some(l) = &self.carry {
            self.game.give(l);
        }
        if self.god {
            self.game.god(true);
        }
    }

    /// Damage does nothing to her (kept across restarts).
    /// The sound on or off for now (paused, or not at the game's own speed); with it up only.
    pub fn sound(&self, on: bool) {
        self.game.sound(on);
    }

    /// A tic's worth of the game's sound (with `capture`): 16-bit stereo, interleaved, and its
    /// sample rate. None without capture.
    pub fn grab_tic_sound(&self) -> Option<(u32, Vec<i16>)> {
        let rate = self.game.sound_rate()?;
        self.game.grab_sound((rate as usize).div_ceil(crate::motor::TICRATE as usize))
    }

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
        let mut barrels_lit = Vec::new();
        let mut things = self.objects.then(Vec::new);
        for m in &self.mobjs {
            let name = dsda::type_name(m.kind);
            // (a barrel going off is a barrel till it's gone: hit, it blows 15 tics later, its
            // death frames, and until then it's the most dangerous one there; E1M5's, shot, left
            // out at once, taken for gone and walked past as it blew)
            if name == Some("ExplosiveBarrel") {
                barrels.push((m.x, m.y));
                if m.health <= 0 {
                    barrels_lit.push((m.x, m.y));
                }
            }
            if m.kind == 0 || m.flags & dsda::MF_CORPSE != 0 || (m.flags & dsda::MF_SHOOTABLE != 0 && m.health <= 0) {
                continue;
            }
            let Some(name) = name else { continue };
            if let Some(things) = things.as_mut() {
                let category = category(name);
                if !matches!(name, "DoomPlayer" | "BossTarget") && category != "Other" {
                    things.push(Thing { id: m.id, name, category, x: m.x, y: m.y, z: m.z, vx: m.momx, vy: m.momy, vz: m.momz, angle: m.angle, health: m.health, owner: m.owner_type, state: m.state, after_her: m.after_her != 0, attack: m.attack });
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
        let names = self.game.sfx_names();
        let sounds: Vec<Sound> = self
            .game
            .sounds()
            .into_iter()
            .map(|[_tic, sfx, id, kind, own, volume, separation, x, y]| Sound {
                name: usize::try_from(sfx).ok().and_then(|i| names.get(i)).copied().unwrap_or(""),
                own: own != 0,
                volume,
                separation,
                from: (kind >= 0).then(|| (id, dsda::type_name(kind).unwrap_or(""))),
                at: (f64::from(x), f64::from(y)),
            })
            .collect();
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
            barrels_lit,
            health: Some(f64::from(p.health)),
            suits: Some(self.suits),
            things,
            weapons: Some(weapons),
            ammo: Some(vec![(1, 0), (2, clip), (3, shell), (4, clip), (5, rocket), (6, cell), (7, cell)]),
            armor: Some(f64::from(p.armor)),
            armor_type: Some(p.armortype),
            super_shotgun: Some(p.weaponowned[8] != 0),
            chainsaw: Some(p.weaponowned[7] != 0),
            berserk: Some(p.strength != 0),
            velocity: Some((p.momx, p.momy)),
            allmap: Some(p.allmap != 0),
            backpack: Some(p.backpack != 0),
            message,
            sounds,
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
        let hits = self.game.hits();
        self.hits.extend(hits);
    }

    /// The hits on her since the last call.
    pub fn take_hits(&mut self) -> Vec<crate::dsda::Hit> {
        std::mem::take(&mut self.hits)
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
