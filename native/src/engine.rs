// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! DSDA-Doom as Evil's engine: the game in-process (`dsda`), a tic at a time, its state read
//! into Observations. One game a process: DSDA-Doom is all globals, so sweeps give each map a
//! process of its own.

use crate::bestiary::category;
use crate::config;
use crate::domain::{Controls, Heard, Observation, Sound, Thing};
use crate::dsda::{self, Dsda, Mobj, TicCmd};
use crate::nav::Heights;
use crate::specials::{Key, Keys};
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

pub use crate::map::episode_map;

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
    /// drawn at this size (width, height); None: the game's own setting (640x480)
    pub resolution: Option<(usize, usize)>,
    /// the music played by FluidSynth with this SoundFont; None: by the OPL synth
    pub soundfont: Option<PathBuf>,
    /// the songs recorded (crate::music's WAD of them); the rest played as MIDI
    pub music_wad: Option<PathBuf>,
    /// the run recorded as a demo (`Engine::record_end` writes it): the level's first start
    /// is the recording's, and there's no second
    pub record: bool,
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
            resolution: None,
            soundfont: None,
            music_wad: None,
            record: false,
        }
    }
}

/// (`Engine::record`)
#[derive(Clone, Copy, PartialEq, Eq)]
enum Recording {
    No,
    Wanted,
    On,
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
    /// a demo's being recorded, or is to be from the level's start (`Options::record`)
    record: Recording,
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
    /// the tic of the last of those (`Observation::hit_tic`)
    hit_tic: Option<i64>,
    /// a co-op game's players (1: a single-player game), and the one the calls about "the
    /// player" are about (`as_player`): what this side keeps of each of the others is kept
    /// here meanwhile, the one's own in the fields above
    players: usize,
    me: usize,
    seats: Vec<Seat>,
}

/// What this side keeps of a player (the suits, the message, the hits), for each of a co-op
/// game's players but the one the engine's calls are about just now.
#[derive(Clone, Debug, Default)]
struct Seat {
    suits: i32,
    ironfeet: i32,
    message_tic: i32,
    hits: Vec<crate::dsda::Hit>,
    hit_sides: Vec<i32>,
    hit_tic: Option<i64>,
}

/// What the Rust side of the engine keeps that a saved game doesn't (`Engine::save`).
#[derive(Clone, Debug)]
pub struct Mark {
    pub tic: i64,
    suits: i32,
    ironfeet: i32,
    message_tic: i32,
    hits: usize,
    hit_sides: Vec<i32>,
    hit_tic: Option<i64>,
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
                crate::dsda::set_env("SDL_AUDIODRIVER", None);
            }
        } else {
            crate::dsda::set_env("SDL_AUDIODRIVER", Some("dummy".as_ref()));
        }
        if !o.drawn {
            crate::dsda::set_env("SDL_VIDEODRIVER", Some("dummy".as_ref()));
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
        // (the recorded music's WAD first: a PWAD's own songs stay its own)
        let music = o.music_wad.iter().filter(|_| o.sound || o.capture);
        let files: Vec<PathBuf> = music.cloned().chain(crate::config::pwads()).collect();
        if !files.is_empty() {
            args.push("-file".into());
            args.extend(files.iter().map(|p| p.to_string_lossy().to_string()));
        }
        if let Some(sf) = o.soundfont.as_ref().filter(|_| o.sound || o.capture) {
            args.extend([
                "-evilsoundfont".to_string(),
                sf.to_string_lossy().to_string(),
            ]);
        }
        if o.capture {
            args.push("-evilsoundcap".into()); // (a dummy device: mixed for her, not played)
        } else if !o.sound {
            args.extend(["-nosound".to_string(), "-nomusic".to_string()]);
        }
        if !o.drawn {
            args.push("-nodraw".into());
        } else if let Some((w, h)) = o.resolution {
            // (w: windowed, the game's own window that nobody sees)
            args.extend(["-geom".to_string(), format!("{w}x{h}w")]);
        }
        if o.record {
            // (the game won't record without being told which game it's playing as; this is
            // the one it plays as anyway, untold)
            args.extend(["-complevel".to_string(), "21".to_string()]);
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
            record: if o.record {
                Recording::Wanted
            } else {
                Recording::No
            },
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
            hit_tic: None,
            players: 1,
            me: 0,
            seats: vec![Seat::default()],
        })
    }

    /// The players in the games begun from now on (1 to 4): more than one, a co-op game, each
    /// player a bot of her own (`as_player`, `step_all`). Before `start`.
    pub fn set_players(&mut self, n: usize) {
        let n = n.clamp(1, 4);
        self.as_player(0);
        self.players = n;
        self.seats = vec![Seat::default(); n];
        self.game.set_players(n);
    }

    pub fn players(&self) -> usize {
        self.players
    }

    /// The player the engine's calls are about from now on (`observe`, `dead`, `loadout`,
    /// `take_hits`, `set_god`, ...): 0, the first, unless told.
    pub fn as_player(&mut self, i: usize) {
        if i == self.me || i >= self.players {
            return;
        }
        let mine = Seat {
            suits: self.suits,
            ironfeet: self.ironfeet,
            message_tic: self.message_tic,
            hits: std::mem::take(&mut self.hits),
            hit_sides: std::mem::take(&mut self.hit_sides),
            hit_tic: self.hit_tic,
        };
        self.seats[self.me] = mine;
        let theirs = std::mem::take(&mut self.seats[i]);
        self.suits = theirs.suits;
        self.ironfeet = theirs.ironfeet;
        self.message_tic = theirs.message_tic;
        self.hits = theirs.hits;
        self.hit_sides = theirs.hit_sides;
        self.hit_tic = theirs.hit_tic;
        self.me = i;
        self.game.as_player(i);
    }

    /// The player the engine's calls are about (`as_player`).
    pub fn me(&self) -> usize {
        self.me
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
            .filter(|&(_, &o)| o != 0)
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
        match self.record {
            // (the level begun and its recording with it; a second start isn't part of a demo)
            Recording::Wanted
                if self
                    .game
                    .record(self.skill, self.episode, self.map, self.nomonsters) =>
            {
                self.record = Recording::On;
            }
            _ => {
                self.record = Recording::No;
                self.game
                    .new_game(self.skill, self.episode, self.map, self.nomonsters);
            }
        }
        let me = self.me;
        for i in 0..self.players {
            self.as_player(i);
            self.suits = 0;
            self.ironfeet = 0;
            self.message_tic = -1;
        }
        self.as_player(me);
        for _ in 0..WARMUP_TICS {
            self.game.tic(TicCmd::default());
        }
        for i in 0..self.players {
            self.as_player(i);
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
        self.as_player(me);
    }

    /// The demo being recorded, ended and written to `path` (see `Dsda::record_end`): the tics
    /// in it. An error if none was (the library hasn't the call, or the level was begun again).
    pub fn record_end(&mut self, path: &Path) -> Result<i32, String> {
        if self.record != Recording::On {
            return Err(
                "no demo was being recorded (an engine library from before it: \
                        tools/build-engine.sh; or the level was begun a second time)"
                    .into(),
            );
        }
        self.record = Recording::No;
        self.game
            .record_end(path)
            .ok_or_else(|| "the game had no recording to end".to_string())
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

    /// Which of PLAYPAL's palettes the last frame drawn is shown in (`Dsda::palette`).
    pub fn palette(&self) -> usize {
        self.game.palette()
    }

    /// How wide the view's drawn, in degrees (Doom's own: 90). None from an engine built
    /// without it.
    pub fn fov(&self) -> Option<i32> {
        self.game.fov()
    }

    /// The view's width set (60 to 120 degrees): only what's drawn, the game plays the same.
    pub fn set_fov(&self, degrees: i32) {
        self.game.set_fov(degrees);
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
            .grab_sound((rate as usize).div_ceil(crate::domain::TICRATE as usize))
    }

    pub fn god(&self) -> bool {
        self.god
    }

    /// God mode, for every player there is.
    pub fn set_god(&mut self, on: bool) {
        self.god = on;
        let me = self.me;
        for i in 0..self.players {
            self.as_player(i);
            self.game.god(on);
        }
        self.as_player(me);
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
                    .and_then(crate::bestiary::monster)
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
            hit_tic: self.hit_tic,
            mates: self.mates(),
            me: self.me,
        }
    }

    /// A co-op game's other players, where they are (none alone).
    fn mates(&self) -> Vec<crate::domain::Mate> {
        let others: Vec<usize> = (0..self.players).filter(|&j| j != self.me).collect();
        let mates = others
            .into_iter()
            .map(|j| {
                self.game.as_player(j);
                let p = self.game.player();
                crate::domain::Mate {
                    index: j,
                    x: p.x,
                    y: p.y,
                    z: p.z,
                    angle: p.angle,
                    vx: p.momx,
                    vy: p.momy,
                    alive: p.playerstate == 0 && p.health > 0,
                }
            })
            .collect();
        self.game.as_player(self.me);
        mates
    }

    /// The game saved into `slot`, with what this side keeps of it (`Mark`): put back by
    /// `restore`, the same controls then play the same tics. None: the library can't.
    pub fn save(&mut self, slot: i32) -> Option<Mark> {
        self.game.save(slot)?;
        Some(Mark {
            tic: self.tic,
            suits: self.suits,
            ironfeet: self.ironfeet,
            message_tic: self.message_tic,
            hits: self.hits.len(),
            hit_sides: self.hit_sides.clone(),
            hit_tic: self.hit_tic,
        })
    }

    /// The game put back as `slot` has it, and this side as `mark` (the one `save` gave for
    /// it). The hits of the tics played since are forgotten: they didn't happen.
    pub fn restore(&mut self, slot: i32, mark: &Mark) -> bool {
        if !self.game.restore(slot) {
            return false;
        }
        self.tic = mark.tic;
        self.suits = mark.suits;
        self.ironfeet = mark.ironfeet;
        self.message_tic = mark.message_tic;
        self.hits.truncate(mark.hits);
        self.hit_sides.clone_from(&mark.hit_sides);
        self.hit_tic = mark.hit_tic;
        true
    }

    /// The game saved in `slot` as bytes for another process (`take`), and what this side
    /// keeps of it that the other needs: the tic, the suits, the suit on, the message's tic.
    pub fn hand(&self, slot: i32, mark: &Mark) -> Option<(Vec<u8>, [i64; 4])> {
        let bytes = self.game.frame_get(slot)?;
        Some((
            bytes,
            [
                mark.tic,
                i64::from(mark.suits),
                i64::from(mark.ironfeet),
                i64::from(mark.message_tic),
            ],
        ))
    }

    /// A game another process handed over (`hand`), into `slot` here: its `Mark`, for
    /// `restore`. The hits on her are each process's own.
    pub fn take(&mut self, slot: i32, bytes: &[u8], kept: [i64; 4]) -> Option<Mark> {
        if !self.game.frame_put(slot, bytes) {
            return None;
        }
        Some(Mark {
            tic: kept[0],
            suits: kept[1] as i32,
            ironfeet: kept[2] as i32,
            message_tic: kept[3] as i32,
            hits: 0,
            hit_sides: Vec::new(),
            hit_tic: None,
        })
    }

    /// The player and the things as the game has them now, read and no more (no sounds taken,
    /// no message, nothing kept): for looking at a tic played ahead.
    pub fn peek(&mut self) -> (crate::dsda::PlayerState, &[Mobj]) {
        let p = self.game.player();
        self.game.things(&mut self.mobjs);
        (p, &self.mobjs)
    }

    pub fn step(&mut self, c: &Controls) {
        self.game.tic(ticcmd(c));
        self.tic += 1;
        self.note_hits();
    }

    /// A co-op game's tic: each player's controls (`cs[i]` player i's; one short, standing
    /// still), the hits on each kept for them. The engine's calls are about the player they
    /// were about before.
    pub fn step_all(&mut self, cs: &[Controls]) {
        for i in 1..self.players {
            self.game
                .stage(i, cs.get(i).map(ticcmd).unwrap_or_default());
        }
        self.game.tic(cs.first().map(ticcmd).unwrap_or_default());
        self.tic += 1;
        let me = self.me;
        for i in 0..self.players {
            self.as_player(i);
            self.note_hits();
        }
        self.as_player(me);
    }

    /// The hits on her the tic just played, kept (`take_hits`, `Observation::hit_sides`).
    fn note_hits(&mut self) {
        let hits = self.game.hits();
        let sides = self.hit_sides.len();
        self.hit_sides
            .extend(hits.iter().filter(|h| !h.own).filter_map(|h| h.side));
        if self.hit_sides.len() > sides {
            self.hit_tic = Some(self.tic);
        }
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

    /// Player `i`'s view as `frame_at` draws the first's, their status bar with it, and the
    /// palette it's in (a co-op watch's).
    pub fn frame_of(&self, i: usize, between: f64) -> Option<(usize, usize, Vec<u8>, usize)> {
        self.game.draw_as(i, between)
    }
}

/// A tic's controls as the game's command.
fn ticcmd(c: &Controls) -> TicCmd {
    let scale = if c.run { 1.0 } else { 0.5 };
    TicCmd {
        forward: (c.forward * scale).round() as i32,
        side: (c.strafe * scale).round() as i32,
        turn: (-c.turn * 65536.0 / 360.0).round() as i32,
        attack: c.attack,
        use_: c.use_,
        weapon: c.weapon,
        exact: c.exact,
    }
}

fn set_env_default(var: &str, value: &Path) {
    if std::env::var_os(var).is_none() {
        crate::dsda::set_env(var, Some(value.as_ref()));
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
        let missile =
            crate::bestiary::missile(thing).is_some() && crate::bestiary::monster(thing).is_none();
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
            None if crate::bestiary::monster(thing).is_some() && chasing => (
                Heard::Walking,
                self.alone.get(thing).copied().unwrap_or(&[]),
            ),
            None if crate::bestiary::monster(thing).is_some() => {
                (Heard::Attack, self.alone.get(thing).copied().unwrap_or(&[]))
            }
            None => (Heard::Other, &[]),
        }
    }
}
