// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! DSDA-Doom as Evil's engine: the game in-process (`dsda`), a tic at a time, its state read
//! into Observations. One game a process: DSDA-Doom is all globals, so sweeps give each map a
//! process of its own.

use crate::config;
use crate::dsda::{self, Dsda, Mobj, TicCmd};
use crate::motor::{Controls, Heard, Observation, Sound, Thing};
use crate::nav::Heights;
use crate::specials::{Key, Keys};
use crate::tactical::bestiary::category;
use std::path::{Path, PathBuf};

/// (the level loads on the first, then a few idle tics)
pub const WARMUP_TICS: i64 = 8;

/// Where the game's library and its own WAD are built.
pub fn build_dir() -> PathBuf {
    config::engine_dir()
}

pub fn library() -> PathBuf {
    build_dir().join(config::ENGINE_LIBRARY)
}

/// (episode, map) from a map name: E1M1 or MAP01.
pub fn episode_map(name: &str) -> Result<(i32, i32), String> {
    let n = name.to_uppercase();
    let bad = || format!("not a map name: {name}");
    if let Some(rest) = n.strip_prefix("MAP") {
        return Ok((1, rest.parse().map_err(|_| bad())?));
    }
    let b = n.as_bytes();
    if b.len() == 4
        && b[0] == b'E'
        && b[2] == b'M'
        && b[1].is_ascii_digit()
        && b[3].is_ascii_digit()
    {
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
        Options {
            skill: 3,
            nomonsters: true,
            objects: false,
            drawn: false,
            sound: false,
            capture: false,
            seed: None,
        }
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
    /// which way the face looked for each hit since she last looked (`Observation::hit_sides`)
    hit_sides: Vec<i32>,
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
        let warp: Vec<String> = if map_name.to_uppercase().starts_with("MAP") {
            vec![map.to_string()]
        } else {
            vec![episode.to_string(), map.to_string()]
        };
        let mut args: Vec<String> = [
            "-iwad",
            &iwad.to_string_lossy(),
            "-nomouse",
            "-skill",
            &o.skill.to_string(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pwads = crate::config::pwads();
        if !pwads.is_empty() {
            args.push("-file".into());
            args.extend(pwads.iter().map(|p| p.to_string_lossy().to_string()));
        }
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
        let game = Dsda::load(
            &library(),
            &dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
            &args,
        )?;
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
            hit_sides: Vec::new(),
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
        let weapons = p
            .weaponowned
            .iter()
            .enumerate()
            .filter(|(_, &o)| o != 0)
            .fold(0, |m, (i, _)| m | (1 << i));
        let [clip, shell, cell, rocket] = p.ammo;
        crate::dsda::Loadout {
            weapons,
            clip,
            shell,
            cell,
            rocket,
            backpack: p.backpack != 0,
            health: p.health,
            armor: p.armor,
            armor_type: p.armortype,
            ready: Some(p.readyweapon),
        }
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
        // (things numbered afresh, as the level's first start numbered them: a monster is the
        // same one, by its number, each time the level's begun again; what she remembers of it
        // from a life before is of it)
        self.game.reset_ids();
        self.new_game();
        self.tic = if keep_time {
            tic + WARMUP_TICS
        } else {
            WARMUP_TICS
        };
    }

    /// Her bullets and pellets so far (see `Game::pellets`).
    pub fn pellets(&self) -> Option<[i32; 7]> {
        self.game.pellets()
    }

    fn new_game(&mut self) {
        self.game
            .new_game(self.skill, self.episode, self.map, self.nomonsters);
        self.suits = 0;
        self.ironfeet = 0;
        self.message_tic = -1;
        for _ in 0..WARMUP_TICS {
            self.game.tic(TicCmd::default());
        }
        if let Some(l) = &self.carry {
            self.game.give(l);
            if crate::debug::on().carry {
                eprintln!(
                    "carried in: {l:?}; in her hands now, the game's weapon {}",
                    self.game.player().readyweapon
                );
            }
        }
        if self.god {
            self.game.god(true);
        }
    }

    /// Monsters in the level or not, from the next start of it (a look round an empty level
    /// first, then the level itself).
    pub fn set_monsters(&mut self, on: bool) {
        self.nomonsters = !on;
    }

    /// Damage does nothing to her (kept across restarts).
    /// The sound on or off for now (paused, or not at the game's own speed); with it up only.
    pub fn sound(&self, on: bool) {
        self.game.sound(on);
    }

    /// Doom's own screens drawn and run as the game runs them (the stream's); false: an engine
    /// without them.
    pub fn present(&self, on: bool) -> bool {
        self.game.present(on)
    }

    /// The title card, held there.
    pub fn title(&self) {
        self.game.title();
    }

    /// A tic of the melt between screens, the game held meanwhile (not a game tic).
    pub fn wipe_tic(&self) {
        self.game.wipe_tic();
    }

    /// The menu driven as a player drives it: 0 open, 1 the skull to item `arg`, 2 choose, 3 shut.
    pub fn menu(&self, action: i32, arg: i32) {
        self.game.menu(action, arg);
    }

    /// The level left by its exit (`secret`: the secret one), as a switch leaves it: the tally and
    /// what follows as the game has them. False: an engine without it.
    pub fn exit_level(&self, secret: bool) -> bool {
        self.game.exit(secret)
    }

    /// The level's title as the automap has it ("level 7: dead simple", "E1M1: Hangar").
    pub fn level_title(&self) -> Option<String> {
        self.game.level_title()
    }

    /// A finale's text, as much as is on screen yet (see `Dsda::finale_text`).
    pub fn finale_text(&self) -> Option<String> {
        self.game.finale_text()
    }

    /// The screen as it stands (Doom's own screens); None from an engine without them.
    pub fn screen(&self) -> Option<crate::dsda::Screen> {
        self.game.screen()
    }

    /// The loudness: sfx, music (0-15, the game's own settings), master (0-100). None from an
    /// engine built without it.
    pub fn volume(&self) -> Option<[i32; 3]> {
        self.game.volume()
    }

    /// The loudness set (None: that one left as it is).
    pub fn set_volume(&self, sfx: Option<i32>, music: Option<i32>, master: Option<i32>) {
        self.game.set_volume(sfx, music, master);
    }

    /// A tic's worth of the game's sound (with `capture`): 16-bit stereo, interleaved, and its
    /// sample rate. None without capture.
    pub fn grab_tic_sound(&self) -> Option<(u32, Vec<i16>)> {
        let rate = self.game.sound_rate()?;
        self.game
            .grab_sound((rate as usize).div_ceil(crate::motor::TICRATE as usize))
    }

    pub fn god(&self) -> bool {
        self.god
    }

    pub fn set_god(&mut self, on: bool) {
        self.god = on;
        self.game.god(on);
    }

    /// The level's tally so far: items, kills, secrets, each (got, of).
    /// The monsters lying dead in pieces, by id: killed by a hit that took them further below
    /// nothing than their whole health (vanilla `P_KillMobj`), of the kinds that have the
    /// frames for it. From the things as last observed.
    pub fn gibbed(&self) -> Vec<i32> {
        /// the monsters with an "extreme death" (`xdeathstate`)
        const GIBS: [&str; 5] = [
            "Zombieman",
            "ShotgunGuy",
            "ChaingunGuy",
            "DoomImp",
            "WolfensteinSS",
        ];
        self.mobjs
            .iter()
            .filter(|m| m.flags & dsda::MF_CORPSE != 0)
            .filter(|m| {
                dsda::type_name(m.kind)
                    .filter(|n| GIBS.contains(n))
                    .and_then(crate::tactical::bestiary::monster)
                    .is_some_and(|i| m.health < -i.health)
            })
            .map(|m| m.id)
            .collect()
    }

    /// The skill played, 1-5.
    pub fn skill(&self) -> i32 {
        self.skill
    }

    /// The skill for the next level begun (1-5).
    pub fn set_skill(&mut self, skill: i32) {
        self.skill = skill;
    }

    pub fn tally(&self) -> [(i32, i32); 3] {
        let s = self.game.status();
        [
            (s.items, s.total_items),
            (s.kills, s.total_kills),
            (s.secrets, s.total_secrets),
        ]
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

    /// What each sound is and what makes it, from the game's own table of things: asked of the
    /// game once a process.
    fn sound_book(&self) -> &'static SoundBook {
        static BOOK: std::sync::OnceLock<SoundBook> = std::sync::OnceLock::new();
        BOOK.get_or_init(|| SoundBook::new(&self.game))
    }

    pub fn observe(&mut self) -> Observation {
        let p = self.game.player();
        self.game.sectors(&mut self.sectors);
        let heights: Vec<Heights> = self
            .sectors
            .iter()
            .map(|&(floor, ceiling)| Heights { floor, ceiling })
            .collect();
        self.game.things(&mut self.mobjs);
        let mut barrels = Vec::new();
        let mut barrels_lit = Vec::new();
        let mut things = self.objects.then(Vec::new);
        for m in &self.mobjs {
            let name = dsda::type_name(m.kind);
            // (a barrel going off is a barrel till it's gone: hit, it blows 15 tics later, its
            // death frames, and until then it's the most dangerous one there; left out at once,
            // it's taken for gone and walked past as it blows)
            if name == Some("ExplosiveBarrel") {
                barrels.push((m.x, m.y));
                if m.health <= 0 {
                    barrels_lit.push((m.x, m.y));
                }
            }
            if m.kind == 0
                || m.flags & dsda::MF_CORPSE != 0
                || (m.flags & dsda::MF_SHOOTABLE != 0 && m.health <= 0)
            {
                continue;
            }
            let Some(name) = name else { continue };
            if let Some(things) = things.as_mut() {
                let category = category(name);
                if !matches!(name, "DoomPlayer" | "BossTarget") && category != "Other" {
                    things.push(Thing {
                        id: m.id,
                        name,
                        category,
                        x: m.x,
                        y: m.y,
                        z: m.z,
                        vx: m.momx,
                        vy: m.momy,
                        vz: m.momz,
                        angle: m.angle,
                        health: m.health,
                        owner: m.owner_type,
                        state: m.state,
                        after_her: m.after_her != 0,
                        attack: m.attack,
                    });
                }
            }
        }
        if p.ironfeet > self.ironfeet {
            self.suits += 1; // (a suit picked up: its time went up)
        }
        self.ironfeet = p.ironfeet;
        let message = (p.message_tic != self.message_tic).then(|| {
            self.message_tic = p.message_tic;
            let text: Vec<u8> = p
                .message
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            String::from_utf8_lossy(&text).into_owned()
        });
        let names = self.game.sfx_names();
        let sounds: Vec<Sound> = self
            .game
            .sounds()
            .into_iter()
            .map(
                |[_tic, sfx, id, kind, own, volume, separation, x, y, chasing]| {
                    let name = usize::try_from(sfx)
                        .ok()
                        .and_then(|i| names.get(i))
                        .copied()
                        .unwrap_or("");
                    let (what, kinds) =
                        self.sound_book()
                            .classify(name, (kind >= 0).then_some(kind), chasing != 0);
                    (name, what, kinds, id, kind, own, volume, separation, x, y)
                },
            )
            .map(
                |(name, what, kinds, id, kind, own, volume, separation, x, y)| Sound {
                    name,
                    what,
                    kinds,
                    own: own != 0,
                    volume,
                    separation,
                    from: (kind >= 0).then(|| (id, dsda::type_name(kind).unwrap_or(""))),
                    at: (f64::from(x), f64::from(y)),
                },
            )
            .collect();
        let keys: Keys = Key::ENGINE_ORDER
            .iter()
            .zip(p.cards)
            .filter(|(_, held)| *held != 0)
            .map(|(k, _)| *k)
            .collect();
        let [clip, shell, cell, rocket] = p.ammo;
        let weapons: Vec<i32> = (0..9)
            .filter(|&w| p.weaponowned[w as usize] != 0)
            .map(dsda::weapon_slot)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Observation {
            x: p.x,
            y: p.y,
            angle: p.angle,
            heights,
            tic: self.tic,
            keys: Some(keys),
            z: Some(p.z),
            weapon: Some(dsda::weapon_slot(p.readyweapon)),
            ready: Some(p.readyweapon),
            owned: Some((0..9).filter(|&w| p.weaponowned[w as usize] != 0).collect()),
            barrels: Some(barrels),
            barrels_lit,
            health: Some(f64::from(p.health)),
            suits: Some(self.suits),
            things,
            weapons: Some(weapons),
            ammo: Some(vec![
                (1, 0),
                (2, clip),
                (3, shell),
                (4, clip),
                (5, rocket),
                (6, cell),
                (7, cell),
            ]),
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
            hit_sides: std::mem::take(&mut self.hit_sides),
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
            exact: c.exact,
        });
        self.tic += 1;
        let hits = self.game.hits();
        self.hit_sides
            .extend(hits.iter().filter(|h| !h.own).filter_map(|h| h.side));
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

    /// The frame drawn `between` (0 to 1) the tic before and this one (a watcher's window, at
    /// the screen's rate: things where they'd be between two tics).
    pub fn frame_at(&self, between: f64) -> Option<(usize, usize, Vec<u8>)> {
        self.game.draw_at(between)
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
    let bin = std::env::var_os("EVIL_MINGW_BIN")
        .map_or_else(|| PathBuf::from(r"C:\msys64\mingw64\bin"), PathBuf::from);
    if bin.is_dir() {
        vec![bin]
    } else {
        Vec::new()
    }
}

/// A sound's family: its name less the number the game's variants of it end in ("dspodth2":
/// "dspodth"; the game picks one of the three at random).
fn family(name: &str) -> &str {
    name.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// What each kind of thing sounds like, and so what a sound heard is.
struct SoundBook {
    /// per thing type: its sounds' families (seeing her or fired, attacking, pain, dying or
    /// going off, wandering; "" none)
    of: std::collections::BTreeMap<i32, [&'static str; 5]>,
    /// (family, which of the five) -> the kinds that make it
    makers: std::collections::BTreeMap<(&'static str, usize), &'static [&'static str]>,
    /// each kind alone, as a list of one
    alone: std::collections::BTreeMap<&'static str, &'static [&'static str]>,
}

impl SoundBook {
    fn new(game: &dsda::Dsda) -> SoundBook {
        let names = game.sfx_names();
        let mut of = std::collections::BTreeMap::new();
        let mut makers: std::collections::BTreeMap<(&'static str, usize), Vec<&'static str>> =
            std::collections::BTreeMap::new();
        let mut alone: std::collections::BTreeMap<&'static str, &'static [&'static str]> =
            std::collections::BTreeMap::new();
        let mut kind = 0;
        while let Some(sounds) = game.mobj_sounds(kind) {
            if let Some(thing) = dsda::type_name(kind) {
                let families = sounds.map(|s| {
                    usize::try_from(s)
                        .ok()
                        .and_then(|i| names.get(i))
                        .map_or("", |n| family(n))
                });
                for (role, f) in families.iter().enumerate().filter(|(_, f)| !f.is_empty()) {
                    let list = makers.entry((f, role)).or_default();
                    if !list.contains(&thing) {
                        list.push(thing);
                    }
                }
                of.insert(kind, families);
                alone.insert(thing, &*Box::leak(vec![thing].into_boxed_slice()));
            }
            kind += 1;
        }
        let makers = makers
            .into_iter()
            .map(|(k, v)| (k, &*Box::leak(v.into_boxed_slice())))
            .collect();
        SoundBook { of, makers, alone }
    }

    /// What a sound is and the kinds that make it, by its name and the type of thing it came
    /// from (None: no thing: a boss's cry, heard everywhere, or a door, a switch, her own).
    fn classify(
        &self,
        name: &str,
        from: Option<i32>,
        chasing: bool,
    ) -> (Heard, &'static [&'static str]) {
        let f = family(name);
        let made = |role: usize| self.makers.get(&(f, role)).copied();
        let Some(kind) = from else {
            // (no thing: a boss's cry, if any kind dies or wakes with it)
            return made(3)
                .map(|k| (Heard::Death, k))
                .or_else(|| made(0).map(|k| (Heard::Sight, k)))
                .unwrap_or((Heard::Other, &[]));
        };
        let thing = dsda::type_name(kind).unwrap_or("");
        let missile = crate::tactical::bestiary::missile(thing).is_some()
            && crate::tactical::bestiary::monster(thing).is_none();
        // (blown apart: the game's one sound for it, whatever died: A_XScream)
        if f == "dsslop" {
            return (Heard::Death, self.alone.get(thing).copied().unwrap_or(&[]));
        }
        let role = self
            .of
            .get(&kind)
            .and_then(|fs| [3, 0, 2, 1, 4].into_iter().find(|&r| fs[r] == f));
        match role {
            Some(0) if missile => (Heard::Launch, made(0).unwrap_or(&[])),
            Some(3) if missile => (Heard::Impact, made(3).unwrap_or(&[])),
            Some(0) => (Heard::Sight, made(0).unwrap_or(&[])),
            Some(1) => (Heard::Attack, made(1).unwrap_or(&[])),
            Some(2) => (Heard::Pain, made(2).unwrap_or(&[])),
            Some(3) => (Heard::Death, made(3).unwrap_or(&[])),
            Some(_) => (Heard::Active, made(4).unwrap_or(&[])),
            // (a monster's sound that's none of its five: what its attack plays itself, a
            // zombieman's pistol, a chaingunner's burst. The kind is the game's word for what
            // made it: a shotgun's blast is a sergeant's, a chaingunner's or a spider's by ear)
            // (...unless it's made as it walks: a cyberdemon's hoof, a spider's legs)
            None if crate::tactical::bestiary::monster(thing).is_some() && chasing => (
                Heard::Walking,
                self.alone.get(thing).copied().unwrap_or(&[]),
            ),
            None if crate::tactical::bestiary::monster(thing).is_some() => {
                (Heard::Attack, self.alone.get(thing).copied().unwrap_or(&[]))
            }
            None => (Heard::Other, &[]),
        }
    }
}
