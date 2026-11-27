// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A co-op or deathmatch game watched (`watch --players N`, 2 to 4; `--deathmatch`): one game
//! with up to four players in it, each a bot of her own, every one's view on the screen at once.
//!
//! Deathmatch, Doom's classic (`--deathmatch`, or `--altdeath` for its second kind): random
//! starts, weapons left lying, a death a rebirth at a start. The others are opponents: among the
//! things she sees, to be fought as a monster is (`Opponent`), their missiles not hers; nothing's
//! shared. The exit's kept back; with nothing left to do she walks the level round again. Over
//! at `--frags N` (20 unless told; 0: no limit) or `--minutes M` (10; 0: none): the level's
//! ended there, and Doom's own tally of frags comes up. Monsters in or not, as `--fight` says.
//!
//! Each player is an `Agent` as a single-player watch has one. In co-op what each learns of the
//! level is shared, each knows where the others are (`Observation::mates`: given way to, not
//! shot through), and jobs are left to whoever took them up first.
//!
//! Directed (`--neuro URL`, co-op only): each player has a Neuro of her own, a `Session` as a
//! single-player watch has, on URL's port and the ones after it (or a URL each). Her director
//! is told of the others (`control/directed/team.rs`): where each is, a death, a key. Headless,
//! it's played at the game's own pace, each one's log written as it goes. Doom's own
//! co-op rules play: each at their own start, the things only a netgame has, keys and weapons
//! left lying for the others, a death a rebirth at the start with a pistol (she presses on a
//! second later, and goes on knowing what she knew), and one of them out by the exit ends the
//! level for all.
//!
//! The window: two players, their views side by side and each one's log under it; three or four,
//! their views two by two and a column beside them with the whole map (where each of them is,
//! and the lines any of them has seen) and a card for each.
//!
//! Keys: space pause, N a tic while paused, +/- speed, M mute, Esc quit.

mod console;
mod human;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use super::canvas::*;
use super::{brightened, palettes, window, Rect, WatchOptions, H, W};
use crate::config::resolve_wad;
use crate::domain::{Controls, Observation, TICRATE};
use crate::dsda::WindowEvent;
use crate::engine::{Engine, Options};
use crate::knowledge::model::Knowledge;
use crate::map::{MapData, ML_DONTDRAW};
use crate::navigator::Navigator;
use crate::neuro::session::Session;
use crate::play::Agent;
use crate::tactical::tuning::Tuning;
use crate::wad::Wad;

/// The most players a game has (Doom's).
pub const MAX_PLAYERS: usize = 4;

/// A dead player presses on to be born again this long after dying (tics).
const REBIRTH_WAIT: i64 = 35;

/// A job left to another player and taken up again within this long (tics) is hers to do too.
const LEFT_AGAIN: i64 = 10 * 35;

/// (a deathmatch) nothing to do this long, she walks the level round again (tics)
const PATROL_AFTER: i64 = 3 * 35;

/// Each player's colour, as Doom's co-op has them (green, indigo, brown, red).
const COLOURS: [Rgb; MAX_PLAYERS] = [
    (80, 220, 80),
    (150, 150, 255),
    (210, 160, 90),
    (240, 80, 80),
];

/// One of the players.
struct Seat {
    agent: Agent,
    /// the last tic's controls (for the agent's next) and observation
    controls: Option<Controls>,
    obs: Option<Observation>,
    /// secrets found so far, by the game's tally of hers
    secrets_seen: i32,
    /// dead since this tic (None: alive)
    dead_since: Option<i64>,
    deaths: i32,
    /// her lines set off so far that the others have been told of (`tell_used`)
    told: usize,
    /// (a deathmatch) with nothing to do since this tic
    idle_since: Option<i64>,
    /// her view as last drawn: (width, height, palette indices, the palette)
    frame: Option<(usize, usize, Vec<u8>, usize)>,
    /// the job she last left to another, and when: back at it for want of anything else, it's
    /// hers alongside them (not left again and again, tic by tic)
    left_to_mate: Option<(Claim, i64)>,
    /// (directed) the hits on her since her director was last told, and the keys she had
    hits: Vec<crate::dsda::Hit>,
    keys_had: crate::specials::Keys,
}

struct Coop {
    w: Wad,
    m: Arc<MapData>,
    nav: Rc<Navigator>,
    map_name: String,
    engine: Engine,
    seats: Vec<Seat>,
    /// (co-op directed by Neuro) each player's own link, kept from level to level: the
    /// first's, or (a person playing, player 1) the second's on
    neuro: Vec<Session>,
    /// the seat the first of `neuro` directs (1 with a person playing, else 0)
    neuro_from: usize,
    /// (`--human`) a person playing, player 1, at the window's keyboard and mouse
    human: Option<human::Human>,
    /// PLAYPAL's palettes, made ours (brightened as the watch's are)
    palettes: Vec<Vec<u32>>,
    /// the tic the level began at (its time is from here)
    began: i64,
    /// the game's seed (the logs are named by it)
    seed: Option<u32>,
    /// 0 co-op, 1 deathmatch, 2 altdeath
    deathmatch: i32,
    /// a deathmatch over at this many frags (0: no limit), or this much game time (tics; 0: none)
    frag_limit: i32,
    time_limit: i64,
    speed: f64,
    paused: bool,
    sound: bool,
    muted: bool,
    sound_on: bool,
    /// the stream console's link (`--control`)
    console: Option<console::Link>,
    /// a deathmatch's styles, hunting and fighting, as given (for the report)
    styles: (Vec<String>, Vec<String>),
}

/// The seat a person plays (`--human`): player 1, the game's own (whose the sound is).
const HUMAN_SEAT: usize = 0;

/// A co-op game of `o.players` watched in a window (or played through headless).
pub fn watch(wad: &str, map_name: &str, o: &WatchOptions) -> Result<(), String> {
    let unsupported = [
        (
            o.directed.is_some() || o.gpt.is_some(),
            "a directed script or the GPT stand-in (--neuro is)",
        ),
        (
            o.neuro.is_some() && o.deathmatch > 0,
            "Neuro in a deathmatch",
        ),
        (o.present, "--present"),
        (o.stream.is_some(), "--stream"),
        (
            o.ultra || o.ultra_after.is_some() || o.tape.is_some(),
            "the ultra instinct or --tape",
        ),
        (o.practice, "--known"),
        (o.expert || o.secrets || o.complete, "the 100% runs"),
    ];
    if let Some((_, what)) = unsupported.iter().find(|(is, _)| *is) {
        return Err(format!("--players: not yet with {what}"));
    }
    if o.human && (o.players < 2 || o.headless) {
        return Err("--human: with --players N (2 to 4), in the window".into());
    }
    if o.deathmatch > 0 && o.players < 2 {
        return Err("--deathmatch: two players at least (--players N)".into());
    }
    let sound = o.sound && !o.headless;
    let mut coop = Coop::new(wad, map_name, o, sound)?;
    if o.headless {
        return coop.headless(o);
    }
    coop.show(o)
}

impl Coop {
    fn new(wad: &str, map_name: &str, o: &WatchOptions, sound: bool) -> Result<Coop, String> {
        let iwad = resolve_wad(wad);
        let w = Wad::open(&iwad)?;
        let map_name = map_name.to_uppercase();
        let (m, nav) = load(&w, &map_name, o)?;
        let skill = if o.fight > 0 { o.fight } else { 3 };
        let mut engine = Engine::new(
            &iwad,
            &map_name,
            &Options {
                skill,
                nomonsters: o.fight == 0,
                objects: true,
                drawn: !o.headless,
                sound,
                capture: false,
                seed: o.seed,
                resolution: o.render,
                soundfont: o.soundfont.clone(),
                music_wad: None,
                record: false,
            },
        )?;
        engine.set_players(o.players);
        engine.set_mode(o.deathmatch);
        engine.carry.clone_from(&o.carry);
        engine.start();
        if !o.mortal || o.charmed {
            engine.set_god(true);
        }
        let seats = seats(&nav, &map_name, o)?;
        if o.deathmatch > 0 {
            println!(
                "deathmatch on {map_name}: {} players, to {} frags or {} minutes",
                o.players,
                if o.frag_limit > 0 {
                    o.frag_limit.to_string()
                } else {
                    "no".into()
                },
                if o.minutes > 0.0 {
                    o.minutes.to_string()
                } else {
                    "no".into()
                }
            );
        }
        let mut neuro = Vec::new();
        if let Some(spec) = o.neuro.as_deref() {
            let clocks = Tuning::from(o.tuning.as_deref())?.directed.clone();
            let directed = o.players - usize::from(o.human);
            for (k, url) in neuro_urls(spec, directed)?.iter().enumerate() {
                let i = k + usize::from(o.human);
                let mut n = Session::new(url, &clocks);
                n.set_locked(&o.locked);
                if o.instinct_off {
                    n.set_instinct(false);
                }
                n.set_choices(&no_game());
                println!("player {}: directed by Neuro at {url}", i + 1);
                neuro.push(n);
            }
        }
        let palettes = palettes(&w).iter().map(|p| brightened(p)).collect();
        let began = engine.tic;
        let console = match o.control.as_deref() {
            Some(addr) => Some(console::Link::new(super::console::Console::connect(
                addr, wad, &map_name, &w,
            )?)),
            None => None,
        };
        Ok(Coop {
            w,
            m,
            nav,
            map_name,
            engine,
            seats,
            neuro,
            neuro_from: usize::from(o.human),
            human: o.human.then(|| human::Human::new(o.mouse_sens)),
            palettes,
            began,
            seed: o.seed,
            deathmatch: o.deathmatch,
            frag_limit: o.frag_limit,
            time_limit: (o.minutes * 60.0 * TICRATE) as i64,
            speed: o.speed,
            paused: false,
            sound,
            muted: false,
            sound_on: sound,
            console,
            styles: (o.hunt_style.clone(), o.fight_style.clone()),
        })
    }

    /// A tic: each player sees the level and decides (a dead one presses on to be born again),
    /// then the tic's played with all their controls.
    fn tic(&mut self) {
        // (what each is going for, as she'd say: left to her by the others. Kept up to date
        // as each decides this tic, so the one after her knows what she's just taken up)
        let mut going: Vec<Option<Claim>> = self.seats.iter().map(claim_of).collect();
        // (two at the same job already, taken up the same tic or before they knew: the one
        // further from it leaves it to the other)
        if self.deathmatch == 0 {
            for i in 0..self.seats.len() {
                let Some(mine) = going[i] else { continue };
                if !shareable(mine.0) {
                    continue;
                }
                let tic = self.engine.tic;
                if self.seats[i]
                    .left_to_mate
                    .is_some_and(|(c, t)| same_job(c, mine) && tic - t < LEFT_AGAIN)
                {
                    continue;
                }
                let Some(pos_i) = self.seats[i].obs.as_ref().map(Observation::pos) else {
                    continue;
                };
                let nearer = (0..self.seats.len()).find(|&j| {
                    j != i
                        && going[j].is_some_and(|theirs| same_job(mine, theirs))
                        && self.seats[j].obs.as_ref().is_some_and(|o| {
                            let (dj, di) = (
                                crate::geom::dist(o.pos(), mine.1),
                                crate::geom::dist(pos_i, mine.1),
                            );
                            dj < di || (dj == di && j < i)
                        })
                });
                if let (Some(j), Some(obs)) = (nearer, self.seats[i].obs.clone()) {
                    self.seats[i].agent.x_mut().leave_to_mate(&obs, j);
                    self.seats[i].left_to_mate = Some((mine, tic));
                    going[i] = None;
                }
            }
        }
        // (directed: each where she was last tic, for the others' directors)
        let players = self.seats.len();
        let mates: Vec<crate::control::directed::Mate> = if self.neuro.is_empty() {
            Vec::new()
        } else {
            self.seats
                .iter()
                .enumerate()
                .filter_map(|(j, s)| {
                    let obs = s.obs.as_ref()?;
                    Some(crate::control::directed::Mate {
                        player: j,
                        pos: obs.pos(),
                        alive: s.dead_since.is_none(),
                        health: obs.health.unwrap_or(0.0) as i32,
                        fighting: j
                            .checked_sub(self.neuro_from)
                            .and_then(|k| self.neuro.get(k))
                            .is_some_and(|n| n.director.now.fighting()),
                    })
                })
                .collect()
        };
        // (what each has done the others are told of: who, what)
        let mut news: Vec<(usize, String)> = Vec::new();
        let mut cs = Vec::with_capacity(self.seats.len());
        let from = self.neuro_from;
        for (i, s) in self.seats.iter_mut().enumerate() {
            self.engine.as_player(i);
            let tic = self.engine.tic;
            let person = i == HUMAN_SEAT && self.human.is_some();
            if self.engine.dead() {
                let newly = s.dead_since.is_none();
                let since = *s.dead_since.get_or_insert_with(|| {
                    s.deaths += 1;
                    s.agent
                        .x_mut()
                        .log
                        .push(format!("tic {tic}: died (death {})", s.deaths));
                    tic
                });
                if let Some(n) = i.checked_sub(from).and_then(|k| self.neuro.get_mut(k)) {
                    if newly {
                        n.director.died(tic, false, &std::mem::take(&mut s.hits));
                        news.push((i, format!("{} died.", who(i))));
                    }
                    let said = n.dead(tic);
                    s.agent.x_mut().log.extend(said);
                }
                if newly && person {
                    news.push((i, format!("{} died.", who(i))));
                }
                // (the use key, pressed and let go: Doom wants it pressed again for each go; a
                // person's own, when they press it)
                let press = tic - since >= REBIRTH_WAIT && (tic - since) % 2 == 0;
                cs.push(match self.human.as_mut().filter(|_| person) {
                    Some(h) => h.controls(None),
                    None => Controls {
                        use_: press,
                        ..Controls::default()
                    },
                });
                s.controls = None;
                continue;
            }
            let reborn = s.dead_since.take().is_some();
            if reborn {
                // (born again at her start: the level goes on as it is, she knowing what she
                // knew of it)
                s.agent.reborn();
                s.agent
                    .x_mut()
                    .log
                    .push(format!("tic {tic}: born again at the start"));
                if i.checked_sub(from).is_some_and(|k| k < self.neuro.len()) {
                    crate::control::directed::Director::take_over(&mut s.agent);
                }
            }
            // (a deathmatch: nothing left to do a while, the level walked round again, for whoever's
            // about and whatever's back)
            if self.deathmatch > 0 {
                if s.agent.x().current.is_some() {
                    s.idle_since = None;
                } else if tic - *s.idle_since.get_or_insert(tic) > PATROL_AFTER {
                    s.idle_since = None;
                    s.agent.x_mut().knowledge.visited.clear();
                    s.agent
                        .x_mut()
                        .log
                        .push(format!("tic {tic}: nothing to do: round the level again"));
                }
            }
            let obs = self.engine.observe();
            // (a person: their own controls, nothing of the bot's; their keys told the others)
            if let Some(h) = self.human.as_mut().filter(|_| person) {
                let keys = obs.keys.unwrap_or_default();
                for k in keys.iter().filter(|&k| !s.keys_had.has(k)) {
                    news.push((
                        i,
                        format!(
                            "{} got the {}.",
                            who(i),
                            crate::control::directed::key_name(k)
                        ),
                    ));
                }
                s.keys_had = keys;
                cs.push(h.controls(Some(&obs)));
                s.obs = Some(obs);
                continue;
            }
            // (what the others are at, as of their choices this tic: hers and her director's
            // to leave to them)
            if self.deathmatch == 0 {
                s.agent.x_mut().claimed = going
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .filter_map(|(_, g)| *g)
                    .collect();
            }
            s.secrets_seen =
                crate::run::secret_found(&self.engine, &mut s.agent, &obs, s.secrets_seen);
            if let Some(n) = i.checked_sub(from).and_then(|k| self.neuro.get_mut(k)) {
                // (told as she's back, what she had and can take again: her last look alive,
                // `s.obs`, says what that was)
                if reborn {
                    n.director.reborn(&s.agent, &obs, s.obs.as_ref());
                }
                let keys = obs.keys.unwrap_or_default();
                for k in keys.iter().filter(|&k| !s.keys_had.has(k)) {
                    news.push((
                        i,
                        format!(
                            "{} got the {}.",
                            who(i),
                            crate::control::directed::key_name(k)
                        ),
                    ));
                }
                s.keys_had = keys;
                let d = &mut n.director;
                d.tally = crate::control::directed::Tally::of(self.engine.tally());
                d.level = crate::control::directed::Level {
                    map: self.map_name.clone(),
                    title: self.engine.level_title(),
                    skill: self.engine.skill(),
                };
                d.restart_allowed = false;
                d.hits = std::mem::take(&mut s.hits);
                d.gibbed = self.engine.gibbed();
                let team = crate::control::directed::Team {
                    me: i,
                    human: self.human.is_some().then_some(HUMAN_SEAT),
                    players,
                    mates: mates.iter().filter(|m| m.player != i).cloned().collect(),
                };
                d.set_team(&s.agent, &obs, team);
                let said = n.tic(&mut s.agent, &obs);
                s.agent.x_mut().log.extend(said);
            }
            let c = s.agent.act(&obs, s.controls.as_ref());
            going[i] = claim_of(s);
            s.obs = Some(obs);
            cs.push(c);
        }
        let tic = self.engine.tic;
        for (from, text) in news {
            for (k, n) in self.neuro.iter_mut().enumerate() {
                if k + self.neuro_from != from {
                    n.director.mate_news(tic, text.clone());
                }
            }
        }
        self.engine.step_all(&cs);
        let directed = !self.neuro.is_empty();
        for (i, s) in self.seats.iter_mut().enumerate() {
            self.engine.as_player(i);
            for h in self.engine.take_hits() {
                if directed {
                    s.hits.push(h);
                }
                let armor = if h.armor > 0 {
                    format!(" (and {} armor)", h.armor)
                } else {
                    String::new()
                };
                s.agent.x_mut().log.push(format!(
                    "tic {}: hit for {}{armor} by {}",
                    self.engine.tic,
                    h.health,
                    h.from()
                ));
            }
            if !self.engine.dead() {
                s.controls = Some(cs[i]);
            }
        }
        self.engine.as_player(0);
        if self.deathmatch == 0 {
            self.tell_used();
            self.share_knowledge();
        } else if !self.engine.exited() && self.match_over().is_some() {
            // (the frags or the time up: the level ended, as an exit ends it, Doom's tally next)
            self.engine.exit_level(false);
        }
    }

    /// Each player's frags: the others she's killed, less the times she's killed herself.
    fn frags(&self) -> Vec<i32> {
        let f = self.engine.frags();
        (0..self.seats.len())
            .map(|i| {
                (0..self.seats.len())
                    .map(|j| if i == j { -f[i][j] } else { f[i][j] })
                    .sum()
            })
            .collect()
    }

    /// A deathmatch's end: why, if it's over.
    fn match_over(&self) -> Option<String> {
        let frags = self.frags();
        let top = frags.iter().copied().max().unwrap_or(0);
        if self.frag_limit > 0 && top >= self.frag_limit {
            return Some(format!("{} frags", top));
        }
        if self.time_limit > 0 && self.engine.tic - self.began >= self.time_limit {
            return Some("the time's up".into());
        }
        None
    }

    /// What each has come to know of the level, known to all (`Knowledge::share_from`).
    fn share_knowledge(&mut self) {
        let n = self.seats.len();
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let (to, from) = if i < j {
                    let (a, b) = self.seats.split_at_mut(j);
                    (&mut a[i], &b[0])
                } else {
                    let (a, b) = self.seats.split_at_mut(i);
                    (&mut b[0], &a[j])
                };
                to.agent
                    .x_mut()
                    .knowledge
                    .share_from(&from.agent.x().knowledge);
            }
        }
    }

    /// What each has set off of the level's once-only lines, told the others (as players say
    /// "I hit the switch"): used up for them too.
    fn tell_used(&mut self) {
        let mut said: Vec<(usize, usize)> = Vec::new();
        for (i, s) in self.seats.iter_mut().enumerate() {
            let k = &s.agent.x().knowledge;
            for f in &k.fired[s.told.min(k.fired.len())..] {
                if k.once_only(f.line) {
                    said.push((i, f.line));
                }
            }
            s.told = k.fired.len();
        }
        for (from, line) in said {
            for (i, s) in self.seats.iter_mut().enumerate() {
                if i != from && s.agent.x_mut().knowledge.mates_used.insert(line) {
                    s.agent.x_mut().log.push(format!(
                        "tic {}: player {} set off line {line}: used",
                        self.engine.tic,
                        from + 1
                    ));
                }
            }
        }
    }

    /// The level's out by its exit: on to the next one (the episode played on), each player
    /// with what she got out with. False: no next level, or not playing on.
    fn next_level(&mut self, o: &WatchOptions) -> Result<bool, String> {
        if !o.episode {
            return Ok(false);
        }
        let Some(next) = crate::run::next_map(&self.map_name, self.engine.secret_exit()) else {
            return Ok(false);
        };
        let (m, nav) = load(&self.w, &next, o)?;
        let carried: Vec<_> = (0..self.seats.len())
            .map(|i| {
                self.engine.as_player(i);
                self.engine.loadout()
            })
            .collect();
        self.engine.as_player(0);
        self.engine.carry = None;
        self.engine.goto(&next)?;
        for (i, l) in carried.iter().enumerate() {
            self.engine.as_player(i);
            self.engine.game.give(l);
        }
        self.engine.as_player(0);
        let deaths: Vec<i32> = self.seats.iter().map(|s| s.deaths).collect();
        for n in &mut self.neuro {
            n.director.new_map();
        }
        self.seats = seats(&nav, &next, o)?;
        for (s, d) in self.seats.iter_mut().zip(deaths) {
            s.deaths = d;
        }
        if let Some(k) = self.console.as_mut() {
            k.new_level();
        }
        println!("{} done: on to {next}", self.map_name);
        (self.m, self.nav, self.map_name) = (m, nav, next);
        self.began = self.engine.tic;
        Ok(true)
    }

    /// Played through without a window, as fast as it goes: a line when the level's over (or
    /// the time's up), each player's part in it.
    fn headless(&mut self, o: &WatchOptions) -> Result<(), String> {
        let start = Instant::now();
        // (directed by Neuro: at the game's own pace, she being on the clock; or as asked)
        let paced = o.paced || !self.neuro.is_empty();
        let speed = if o.paced && o.speed > 0.0 {
            o.speed
        } else {
            1.0
        };
        let (clock, tic0) = (Instant::now(), self.engine.tic);
        loop {
            let cap = (o.max_seconds * TICRATE) as i64;
            while !self.engine.exited() && self.engine.tic - self.began < cap {
                if self.engine.tic % 9 == 0 && self.console_frame() {
                    self.report(start, true);
                    return Ok(());
                }
                self.tic();
                if paced {
                    let due = (self.engine.tic - tic0) as f64 / (TICRATE * speed);
                    let ahead = due - clock.elapsed().as_secs_f64();
                    if ahead > 0.0 {
                        std::thread::sleep(std::time::Duration::from_secs_f64(ahead));
                    }
                }
                if !self.neuro.is_empty() && self.engine.tic % (5 * 35) == 0 {
                    self.write_logs();
                }
            }
            self.report(start, false);
            if !self.engine.exited() || !self.next_level(o)? {
                return Ok(());
            }
        }
    }

    /// The level's over (out by its exit, or the time's up): whoever directs each told.
    fn level_over(&mut self) {
        let tic = self.engine.tic;
        let exited = self.engine.exited();
        for (n, s) in self
            .neuro
            .iter_mut()
            .zip(self.seats.iter_mut().skip(self.neuro_from))
        {
            if exited {
                n.director.level_done();
            }
            let said = n.between(tic);
            s.agent.x_mut().log.extend(said);
        }
    }

    /// Each one's log, to read afterwards (or follow: written as it goes when she's directed):
    /// out/watch/coop/MAP[-sSEED]-pN.log.
    fn write_logs(&self) {
        let dir = crate::config::out_dir().join("watch").join("coop");
        let _ = std::fs::create_dir_all(&dir);
        for (i, s) in self.seats.iter().enumerate() {
            let seed = self.seed.map_or_else(String::new, |s| format!("-s{s}"));
            let path = dir.join(format!("{}{seed}-p{}.log", self.map_name, i + 1));
            if let Err(e) = std::fs::write(&path, s.agent.x().log.join("\n") + "\n") {
                println!("{}: {e}", path.display());
            }
        }
    }

    /// The level's end told, and each one's part in it (`stopped`: the window shut mid-level).
    fn report(&mut self, start: Instant, stopped: bool) {
        self.level_over();
        let secs = (self.engine.tic - self.began) as f64 / TICRATE;
        let outcome = if self.engine.exited() {
            "EXITED"
        } else if stopped {
            "STOPPED"
        } else {
            "TIMEOUT"
        };
        let kind = if self.deathmatch > 0 {
            "DEATHMATCH"
        } else {
            "COOP"
        };
        println!(
            "{kind} {} {outcome} {:.0}s game, {:.0}s real, {} players",
            self.map_name,
            secs,
            start.elapsed().as_secs_f64(),
            self.seats.len()
        );
        if self.deathmatch > 0 {
            let frags = self.frags();
            let f = self.engine.frags();
            for i in 0..self.seats.len() {
                let by: Vec<String> = (0..self.seats.len())
                    .map(|j| format!("{}", f[i][j]))
                    .collect();
                let hunts = self.seats[i]
                    .agent
                    .player()
                    .is_some_and(|p| p.hunt_state_on());
                let style = |v: &[String]| v.get(i).or_else(|| v.last()).cloned();
                let styles: Vec<String> = [style(&self.styles.0), style(&self.styles.1)]
                    .into_iter()
                    .flatten()
                    .collect();
                let styles = if styles.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", styles.join(" "))
                };
                println!(
                    "  player {}{}{styles}: frags {} (kills of players 1-{}: {}) deaths {}",
                    i + 1,
                    if hunts { " (hunts)" } else { "" },
                    frags[i],
                    self.seats.len(),
                    by.join(" "),
                    self.seats[i].deaths
                );
            }
        }
        for i in 0..self.seats.len() {
            self.engine.as_player(i);
            let t = self.engine.tally();
            let p = self.engine.game.player();
            let s = &self.seats[i];
            println!(
                "  player {}: kills {}/{} items {}/{} secrets {}/{} health {} armor {} deaths {}",
                i + 1,
                t[1].0,
                t[1].1,
                t[0].0,
                t[0].1,
                t[2].0,
                t[2].1,
                p.health,
                p.armor,
                s.deaths
            );
        }
        self.engine.as_player(0);
        self.write_logs();
    }

    fn kind(&self) -> &'static str {
        if self.deathmatch > 0 {
            "deathmatch"
        } else {
            "co-op"
        }
    }

    fn show(&mut self, o: &WatchOptions) -> Result<(), String> {
        let title = format!("evil-boom {}: {}", self.kind(), self.map_name);
        let mut window = window::Window::open(&self.engine.game, &title, (W, H), o.window)?;
        let mut canvas = Canvas {
            px: vec![0; W * H],
            width: W,
            clip: (0, 0, W, H),
            text_scale: 1.0,
        };
        let mut frames = super::frames::Frames::from_env();
        let (mut last, mut owed) = (Instant::now(), 0.0);
        let real = Instant::now();
        let mut reported = false;
        'frames: loop {
            if self.console_frame() {
                break 'frames;
            }
            let mut esc = false;
            for event in window::Window::events(&self.engine.game) {
                use window::key;
                let WindowEvent::Key(k) = event else {
                    match event {
                        WindowEvent::Close => break 'frames,
                        WindowEvent::Wheel(by) => {
                            if let Some(h) = self.human.as_mut() {
                                h.wheel(by);
                            }
                        }
                        WindowEvent::Key(_) => {}
                    }
                    continue;
                };
                // (a person playing: Space is theirs to use with, Esc lets the mouse go first;
                // P pauses)
                if self.human.is_some() {
                    match k {
                        key::ESCAPE => esc = true,
                        key::P | key::PAUSE => self.paused = !self.paused,
                        key::M => self.muted = !self.muted,
                        _ => {}
                    }
                    continue;
                }
                match k {
                    key::ESCAPE => break 'frames,
                    key::SPACE => self.paused = !self.paused,
                    key::N | key::RIGHT if self.paused && !self.engine.exited() => self.tic(),
                    key::EQUALS | key::KP_PLUS => self.speed = (self.speed * 2.0).min(16.0),
                    key::MINUS | key::KP_MINUS => self.speed = (self.speed / 2.0).max(0.25),
                    key::M => self.muted = !self.muted,
                    _ => {}
                }
            }
            if let Some(h) = self.human.as_mut()
                && matches!(h.frame(&self.engine.game, esc), human::Asked::Quit)
            {
                break 'frames;
            }
            let sound =
                self.sound && !self.muted && !self.paused && (self.speed - 1.0).abs() < 1e-9;
            if sound != self.sound_on {
                self.engine.sound(sound);
                self.sound_on = sound;
            }
            let (now, tic0) = (Instant::now(), self.engine.tic);
            owed += (now - last).as_secs_f64() * TICRATE * self.speed;
            last = now;
            owed = f64::min(owed, 8.0); // (don't race to catch up after a slow plan)
            let cap = (o.max_seconds * TICRATE) as i64;
            while owed >= 1.0 && !self.paused && !self.engine.exited() {
                if self.engine.tic - self.began >= cap {
                    break;
                }
                self.tic();
                owed -= 1.0;
                // (each one's log as it goes: a window shut mid-level leaves what led up to it)
                if self.engine.tic % (5 * 35) == 0 {
                    self.write_logs();
                }
            }
            if self.engine.exited() {
                if !reported {
                    self.report(real, false);
                    reported = true;
                }
                if self.next_level(o)? {
                    reported = false;
                    window::Window::set_title(
                        &self.engine.game,
                        &format!("evil-boom {}: {}", self.kind(), self.map_name),
                    );
                }
            }
            if self.paused {
                owed = 0.0;
            }
            let between = if self.paused || self.engine.exited() {
                1.0
            } else {
                owed.clamp(0.0, 1.0)
            };
            if self.engine.tic != tic0 || !self.paused || self.seats[0].frame.is_none() {
                for i in 0..self.seats.len() {
                    self.seats[i].frame = self.engine.frame_of(i, between);
                }
            }
            let stepped = Instant::now();
            self.draw(&mut canvas);
            let painted = Instant::now();
            window.show(&self.engine.game, &canvas.px, W, H);
            self.console_views(&canvas.px);
            if let Some(f) = frames.as_mut() {
                let picture = self.seats[0].frame.as_ref().map_or(&[][..], |f| &f.2[..]);
                f.frame(now, stepped, painted, self.engine.tic - tic0, picture);
                if f.done() {
                    let png = crate::config::out_dir().join("watch").join("frames.png");
                    if let Err(e) = super::save_png_sized(&png, &canvas.px, W, H) {
                        println!("{}: {e}", png.display());
                    }
                    break;
                }
            }
        }
        // (shut mid-level: where things stood, the frags too, and each one's log)
        if !reported {
            self.report(real, true);
        }
        Ok(())
    }

    // --- The picture ---------------------------------------------------------------------------

    /// The window: the views, and the logs (two players) or the map and the cards (three, four).
    fn draw(&self, c: &mut Canvas) {
        c.clip = (0, 0, W, H);
        c.fill((0, 0, W, H), BG);
        let n = self.seats.len();
        if self.human.is_some() {
            // (a person playing: their view large, 4:3; the others' small down the side, the
            // map under them if there's room)
            let (vw, vh) = (H * 4 / 3, H);
            self.draw_view(c, HUMAN_SEAT, (0, 0, vw, vh));
            let col = (vw, 0, W - vw, H);
            let small = col.2 * 3 / 4;
            let mut y = 0;
            for i in (0..n).filter(|&i| i != HUMAN_SEAT) {
                self.draw_view(c, i, (col.0, y, col.2, small));
                y += small;
            }
            if H - y >= 150 {
                self.draw_overview(c, (col.0, y, col.2, H - y));
            }
            if let Some(h) = &self.human
                && !h.grabbed()
            {
                let note = "click to play  (Esc: let the mouse go; again: quit)";
                let w = note.len() * 8 * 2 + 16;
                let x = (H * 4 / 3).saturating_sub(w) / 2;
                c.fill((x, 34, w, 26), (0, 0, 0));
                c.text(note, (x + 8, 37), HEAD, 2);
            }
        } else if n <= 2 {
            for i in 0..n {
                let x = i * (W / 2);
                self.draw_view(c, i, (x, 0, W / 2, 600));
                self.draw_card(c, i, (x, 600, W / 2, H - 600), true);
            }
        } else {
            let (vw, vh) = (600, 450);
            for i in 0..n {
                self.draw_view(c, i, ((i % 2) * vw, (i / 2) * vh, vw, vh));
            }
            let col = (2 * vw, 0, W - 2 * vw, H);
            let map = (col.0, 0, col.2, 300);
            self.draw_overview(c, map);
            let card_h = (H - map.3) / n;
            for i in 0..n {
                self.draw_card(c, i, (col.0, map.3 + i * card_h, col.2, card_h), false);
            }
        }
    }

    /// Player i's view, scaled into r, her name over it.
    fn draw_view(&self, c: &mut Canvas, i: usize, r: Rect) {
        c.clip = r;
        let s = &self.seats[i];
        match &s.frame {
            Some((fw, fh, px, pal)) => {
                let palette = self.palettes.get(*pal).unwrap_or(&self.palettes[0]);
                for y in 0..r.3 {
                    let sy = y * fh / r.3;
                    let row = (r.1 + y) * W + r.0;
                    for x in 0..r.2 {
                        let sx = x * fw / r.2;
                        if let Some(&p) = px.get(sy * fw + sx) {
                            c.px[row + x] = palette[usize::from(p)];
                        }
                    }
                }
            }
            None => {
                c.text("starting...", (r.0 + 20, r.1 + 20), TEXT, 2);
            }
        }
        // (her name, in her colour, and a frame round the view)
        let name = if i == HUMAN_SEAT && self.human.is_some() {
            format!("Player {} (you)", i + 1)
        } else {
            format!("Player {}", i + 1)
        };
        c.fill((r.0, r.1, 8 * name.len() * 2 + 12, 22), (0, 0, 0));
        c.text(&name, (r.0 + 6, r.1 + 3), COLOURS[i], 2);
        if s.dead_since.is_some() {
            c.text(
                "DEAD",
                (r.0 + r.2 / 2 - 32, r.1 + r.3 / 3),
                (255, 60, 60),
                4,
            );
        } else if self.engine.exited() && self.deathmatch > 0 {
            let frags = self.frags();
            let top = frags.iter().copied().max().unwrap_or(0);
            let won = frags[i] == top;
            c.text(
                if won { "WINNER" } else { "MATCH OVER" },
                (r.0 + r.2 / 2 - if won { 48 } else { 80 }, r.1 + r.3 / 3),
                HEAD,
                4,
            );
        } else if self.engine.exited() {
            c.text("EXITED", (r.0 + r.2 / 2 - 48, r.1 + r.3 / 3), HEAD, 4);
        }
        for (x0, y0, w, h) in [
            (r.0, r.1, r.2, 2),
            (r.0, r.1 + r.3 - 2, r.2, 2),
            (r.0, r.1, 2, r.3),
            (r.0 + r.2 - 2, r.1, 2, r.3),
        ] {
            c.fill((x0, y0, w, h), COLOURS[i]);
        }
        c.clip = (0, 0, W, H);
    }

    /// What player i is doing, and (`log`) the end of her log, into r.
    fn draw_card(&self, c: &mut Canvas, i: usize, r: Rect, log: bool) {
        c.clip = r;
        c.fill(r, (20, 20, 26));
        c.fill((r.0, r.1, 4, r.3), COLOURS[i]);
        let s = &self.seats[i];
        let x = r.0 + 12;
        let mut y = c.text(&format!("Player {}", i + 1), (x, r.1 + 6), COLOURS[i], 2);
        if let Some(obs) = &s.obs {
            let gun = obs.weapon.map_or(String::new(), |w| format!(", gun {w}"));
            let frags = if self.deathmatch > 0 {
                format!("frags {}  ", self.frags()[i])
            } else {
                String::new()
            };
            let line = format!(
                "{frags}health {:.0}  armor {:.0}{gun}  deaths {}",
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0),
                s.deaths
            );
            y = c.text(&line, (x, y + 2), TEXT, 1);
        }
        let doing = if s.dead_since.is_some() {
            "dead: back in a moment".to_string()
        } else {
            s.agent
                .x()
                .current
                .as_ref()
                .map_or_else(|| "looking round".to_string(), |o| o.label.clone())
        };
        let (width, _) = c.fits(1, r.2 - 24, r.3);
        y = c.text(
            &clip(&format!("doing: {doing}"), width),
            (x, y + 2),
            HEAD,
            1,
        );
        // (the end of her log, as much as fits: a few lines on a card, more under a view)
        let room = (r.1 + r.3).saturating_sub(y + 4) / c.glyph(1).max(1);
        let rows = if log { room } else { room.min(4) };
        let lines: Vec<&String> = s.agent.x().log.iter().rev().take(rows).collect();
        for l in lines.into_iter().rev() {
            y = c.text(&clip(l, width), (x, y), DIM, 1);
        }
        c.clip = (0, 0, W, H);
    }

    /// The whole map in r: the lines any of them has seen bright, the rest faint, and each
    /// player where she is, facing the way she faces.
    fn draw_overview(&self, c: &mut Canvas, r: Rect) {
        c.clip = r;
        c.fill(r, (8, 8, 12));
        let m = &self.m;
        let xs = m.vertexes.iter().map(|v| v.x);
        let ys = m.vertexes.iter().map(|v| v.y);
        let (x0, x1) = (
            xs.clone().fold(f64::INFINITY, f64::min),
            xs.fold(f64::NEG_INFINITY, f64::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f64::INFINITY, f64::min),
            ys.fold(f64::NEG_INFINITY, f64::max),
        );
        let pad = 10.0;
        let scale = ((r.2 as f64 - 2.0 * pad) / (x1 - x0).max(1.0))
            .min((r.3 as f64 - 2.0 * pad) / (y1 - y0).max(1.0));
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let at = |x: f64, y: f64| {
            (
                r.0 as f64 + r.2 as f64 / 2.0 + (x - cx) * scale,
                r.1 as f64 + r.3 as f64 / 2.0 - (y - cy) * scale,
            )
        };
        for (li, l) in m.linedefs.iter().enumerate() {
            if l.flags & ML_DONTDRAW != 0 {
                continue;
            }
            let (a, b) = (&m.vertexes[l.v1], &m.vertexes[l.v2]);
            let seen = self
                .seats
                .iter()
                .any(|s| s.agent.x().knowledge.mapped.contains(&li));
            let colour = if seen { (170, 170, 180) } else { (45, 45, 55) };
            c.line(at(a.x, a.y), at(b.x, b.y), colour, 1);
        }
        for (i, s) in self.seats.iter().enumerate() {
            let Some(obs) = &s.obs else { continue };
            let p = at(obs.x, obs.y);
            let a = obs.angle.to_radians();
            c.circle(p, 4.0, COLOURS[i], None);
            c.line(p, (p.0 + 9.0 * a.cos(), p.1 - 9.0 * a.sin()), COLOURS[i], 2);
        }
        c.text(&self.map_name, (r.0 + 6, r.1 + 4), HEAD, 1);
        c.clip = (0, 0, W, H);
    }
}

/// A level's map and its analysis.
fn load(w: &Wad, name: &str, o: &WatchOptions) -> Result<(Arc<MapData>, Rc<Navigator>), String> {
    crate::engine::episode_map(name)?;
    let m = Arc::new(crate::wad::load_level(w, name)?);
    let skill = if o.fight > 0 { o.fight } else { 3 };
    let mut nav = Navigator::new(Arc::clone(&m), skill)?;
    nav.keep_verified();
    Ok((m, Rc::new(nav)))
}

/// Player `i`'s numbers: the run's, a deathmatch's styles over them (`--hunt-style`,
/// `--fight-style`: one for all, or one a player, the last for the rest).
fn style_tuning(base: &Rc<Tuning>, o: &WatchOptions, i: usize) -> Result<Rc<Tuning>, String> {
    let pick = |v: &[String]| v.get(i).or_else(|| v.last()).cloned();
    let mut t = (**base).clone();
    if o.deathmatch > 0 {
        if let Some(h) = pick(&o.hunt_style).and_then(|n| crate::tactical::styles::hunt_style(&n)) {
            t = t.with(&h)?;
        }
        if let Some(f) = pick(&o.fight_style).and_then(|n| crate::tactical::styles::fight_style(&n))
        {
            t = t.with(&f)?;
        }
    }
    Ok(Rc::new(t))
}

/// Whether player `i` (0-based) hunts the others in a deathmatch (`--hunt`).
fn hunts(o: &WatchOptions, i: usize) -> bool {
    o.hunt.as_ref().is_none_or(|h| h.contains(&i))
}

/// The players for a level, each in it afresh as a watch starts her.
fn seats(nav: &Rc<Navigator>, map_name: &str, o: &WatchOptions) -> Result<Vec<Seat>, String> {
    let base = Tuning::from(o.tuning.as_deref())?;
    // (each player's own, a deathmatch's styles over the run's numbers)
    let tunes: Vec<Rc<Tuning>> = (0..o.players)
        .map(|i| style_tuning(&base, o, i))
        .collect::<Result<_, _>>()?;
    let going_on = o.episode
        && (crate::run::next_map(map_name, false).is_some()
            || crate::run::next_map(map_name, true).is_some());
    Ok((0..o.players)
        .map(|i| {
            let mut knowledge = Knowledge::new(Rc::clone(nav), o.mortal, !o.peek);
            knowledge.charmed = o.charmed;
            // (a gun the level put down stays when picked up: co-op's, and classic deathmatch's;
            // altdeath's goes, and comes back)
            knowledge.coop = o.deathmatch != 2;
            // (a deathmatch: the fight hers whether the monsters are in or not, the others being
            // what she fights; the exit not for her, it'd end the match)
            let fight = if o.deathmatch > 0 && o.fight == 0 {
                3
            } else {
                o.fight
            };
            let mut agent = Agent::new(nav, knowledge, fight, false, o.lives, Rc::clone(&tunes[i]));
            if let Some(p) = agent.player_mut() {
                p.set_going_on(going_on && o.deathmatch == 0);
                if o.deathmatch != 2 {
                    p.set_coop();
                }
            }
            if o.deathmatch > 0 {
                agent.x_mut().exit_held_back = true;
                if let Some(p) = agent.player_mut() {
                    if hunts(o, i) {
                        p.set_hunting(o.fight > 0);
                    }
                    if o.deathmatch == 2 {
                        p.set_items_return();
                    }
                }
            }
            if o.keys {
                agent.keys_only();
            }
            if o.neuro.is_some() && !(o.human && i == HUMAN_SEAT) {
                crate::control::directed::Director::take_over(&mut agent);
            }
            Seat {
                agent,
                controls: None,
                obs: None,
                secrets_seen: 0,
                dead_since: None,
                deaths: 0,
                told: 0,
                idle_since: None,
                frame: None,
                hits: Vec::new(),
                keys_had: crate::specials::Keys::default(),
                left_to_mate: None,
            }
        })
        .collect())
}

/// s cut to `width` characters.
fn clip(s: &str, width: usize) -> String {
    s.chars().take(width).collect()
}

/// "player 2 (indigo)"
fn who(i: usize) -> String {
    crate::control::directed::team::who(i)
}

/// No game of hers to start, nor a level to go on to or begin again: the co-op game's the
/// watch's.
fn no_game() -> crate::control::game::Choices {
    crate::control::game::Choices {
        maps: Vec::new(),
        skills: Vec::new(),
        single: false,
        continuous: false,
        start: false,
        next: false,
        restart: false,
    }
}

/// Each player's Neuro: as many URLs as players, comma-separated; or one, the others on the
/// ports after its (ws://127.0.0.1:8000: player 2 on 8001, ...).
fn neuro_urls(spec: &str, players: usize) -> Result<Vec<String>, String> {
    let urls: Vec<String> = spec.split(',').map(|u| u.trim().to_string()).collect();
    if urls.len() == players {
        return Ok(urls);
    }
    let [one] = &urls[..] else {
        return Err(format!(
            "--neuro: one URL, or one for each of the {players} players"
        ));
    };
    let (head, port) = one
        .rsplit_once(':')
        .and_then(|(h, p)| Some((h, p.trim_end_matches('/').parse::<u32>().ok()?)))
        .ok_or("--neuro: a URL ending in its port (ws://127.0.0.1:8000)")?;
    Ok((0..players as u32)
        .map(|k| format!("{head}:{}", port + k))
        .collect())
}

/// What a player's going for, as the others leave it to her: (kind, where, the wall).
type Claim = (
    crate::knowledge::explorer::Kind,
    crate::geom::Point,
    Option<usize>,
);

fn claim_of(s: &Seat) -> Option<Claim> {
    if s.dead_since.is_some() {
        return None;
    }
    s.agent
        .x()
        .current
        .as_ref()
        .map(|o| (o.kind, o.goal, o.wall))
}

/// A job one does for all: looking round, trying a wall, a switch or a door (not a pickup each
/// wants for herself, a key, the exit, a fight's).
fn shareable(k: crate::knowledge::explorer::Kind) -> bool {
    use crate::knowledge::explorer::Kind as K;
    matches!(
        k,
        K::Frontier
            | K::Room
            | K::Visit
            | K::Noise
            | K::Recheck
            | K::Wall
            | K::Secret
            | K::Switch
            | K::Door
    )
}

/// The same job: the same kind, at the same spot or wall.
fn same_job(a: Claim, b: Claim) -> bool {
    a.0 == b.0 && (crate::geom::dist(a.1, b.1) < 64.0 || a.2.is_some() && a.2 == b.2)
}
