// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Orders and options for directed play.
//!
//! What a commander can talk about (`list`): areas (`places`), doors and switches not yet tried,
//! items seen, monsters remembered, exits seen; each with a stable id and a line saying what it is,
//! which way and how far. Names are the commander's own, per map.
//!
//! An order is carried out with the explorer's own objectives: going to a thing is one objective;
//! exploring an area or a way is as many as it takes (the explorer's options inside it, best
//! first: walls to see, openings to look into, doors to try, barrels to clear) till none are left.
//! When an order ends, how is reported, and she waits for the next.

use std::collections::{BTreeMap, VecDeque};

use super::names::{a, called, some, the};
use super::places::{Area, Places, SMALL};
use crate::domain::{heading, norm, Observation};
use crate::geom::{dist, Point};
use crate::knowledge::explorer::{Ident, Kind, Objective};
use crate::play::Agent;
use crate::tactical::tuning::Directed;

mod ask;
mod death;
mod events;
mod goal;
mod instinct;
mod kills;
mod listings;
mod look;
mod moments;
mod news;
mod open;
mod orders;
mod plans;
mod policy;
mod powers;
mod restart;
mod situation;
mod tactics;
pub mod team;
mod view;
mod why;

pub use ask::{About, Question};
pub use events::{Event, Weight};
pub use look::{Listing, Pick};
pub use moments::{skill_name, Level};
pub use policy::{help_for as policy_help_for, Policy};
pub use team::{Mate, Team};

/// Her standing settings the stream can lock, so they're not hers to change (`--lock`, the
/// console): `doom_policy`'s, then `doom_goal`'s.
pub const LOCKABLE: &[&str] = &[
    "stance", "save", "risk", "powerups", "explore", "instinct", "finish", "prefer",
];
pub use situation::Situation;

/// a name at most this long (refused, not cut: a name she didn't choose is one she won't use)
const NAME_MAX: usize = 32;

/// exploring a way: what's this far either side of it
const EXPLORE_CONE: f64 = 50.0;

/// a switch pressed: watched this long (tics) for what it did
const PRESS_WATCH: i64 = 3 * 35;

/// waiting on a floor that hurts: off it tried this often (tics)...
const OFF_AGAIN: i64 = 35;
/// how long nothing's fought her before a paused explore order goes on (tics)
const RESUME_CALM: i64 = 2 * 35;
/// how far (along the way there) she goes for a pickup by herself, waiting: about the room
/// she's in (waiting, she doesn't really go anywhere)
const PICKUP_RANGE: f64 = 256.0;
/// the nearest pickups tried for a way there, waiting
const PICKUP_TRIES: usize = 4;
/// the same lock's message told again only after this long (tics)
const LOCK_AGAIN: i64 = 10 * 35;
/// exits, switches and doors newly hers to go to, and what's newly in reach, are looked for this
/// often (tics): the safe ones on the second, the risky at `NEWS_RISKY_AT` into it (both
/// multiples of the five tics `events` looks at all)
const NEWS_EVERY: i64 = 35;
/// ...and with nothing new known, this often all the same (a door open now that wasn't)
const NEWS_ANYWAY: i64 = 10 * 35;
const NEWS_RISKY_AT: i64 = 15;
/// an item listed that counts for the level's items tally (the rest are just pickups)
const COUNTS: &str = " (tally)";
/// ...to the best of this many places
const OFF_TRIES: usize = 6;

/// What a list is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Areas,
    Doors,
    Switches,
    Items,
    Monsters,
    Exits,
}

impl Topic {
    pub fn parse(s: &str) -> Option<Topic> {
        Some(match s.to_lowercase().as_str() {
            "areas" | "places" | "rooms" => Topic::Areas,
            "doors" => Topic::Doors,
            "switches" | "buttons" => Topic::Switches,
            "items" | "keys" => Topic::Items,
            "monsters" | "enemies" => Topic::Monsters,
            "exits" | "exit" => Topic::Exits,
            _ => return None,
        })
    }
}

/// One thing in a list.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub what: String,
    pub at: Point,
}

/// What she's told to do.
#[derive(Clone, Debug)]
pub enum Order {
    /// go to (or try, or pick up) something listed, by id or name; risky: whatever the bot
    /// would rather not cross on its own (a floor that hurts), if it's a way there that won't kill
    /// her
    Go(String, bool),
    /// explore an area (by id or name) till there's nothing more known to do in it; and search
    /// its walls for secrets this thoroughly (0 not, 3 every suspicious one: slow, so hers to ask
    /// for)
    ExploreArea(String, u8),
    /// explore that way (degrees, 0 east, 90 north) till there's nothing more known that way;
    /// the search as for an area
    ExploreWay(f64, u8),
    /// out by an exit she knows
    Exit,
    /// stop and wait
    Stop,
}

/// What's happened, for whoever gave the orders.
#[derive(Clone, Debug)]
pub struct Report {
    pub tic: i64,
    /// an order's outcome: done (false: couldn't)
    pub ok: bool,
    pub text: String,
    /// what it is (and so how much it matters: `Event::weight`)
    pub event: Event,
}

impl Report {
    /// Worth a reaction, not just to know (Neuro's `silent: false`).
    pub fn notable(&self) -> bool {
        self.event.weight() != Weight::Quiet
    }

    /// To be told whatever else has been: never made silent to keep the moments a minute down.
    pub fn urgent(&self) -> bool {
        self.event.weight() == Weight::Urgent
    }
}

/// An order being carried out.
enum Doing {
    /// one objective: its id, what it's called
    Once(String, String),
    /// objectives inside an area, one after another (and its walls searched, that thoroughly)
    Area(String, String, u8),
    /// objectives that way, one after another (the search as for an area)
    Way(f64, u8),
    /// off a floor that hurts, her own doing (not news)
    Off,
    /// a pickup she takes by herself, waiting (not news)
    Pickup,
    /// no order for a while (`idle_auto`): carrying on by herself, the explorer choosing, till an
    /// order comes or there's nothing left but to leave
    Auto,
}

/// A fight under way: since when, what it's cost so far, what's been in it.
struct Fight {
    since: i64,
    /// when nothing was last fighting her (None: something is)
    calm_since: Option<i64>,
    kills_at_start: i32,
    health_lost: f64,
    armor_lost: f64,
    /// rounds gone, by pool: bullets, shells, rockets, cells
    spent: [i32; 4],
    /// the most of each kind fighting her at once
    against: BTreeMap<&'static str, usize>,
    /// asked her about it, once it turned bad
    asked: bool,
    /// fire held for a reason that lasts: which, and since when; and those told her this fight
    held: Option<(&'static str, i64)>,
    held_told: Vec<&'static str>,
}

/// What she had a tic ago.
struct Had {
    health: f64,
    armor: f64,
    ammo: [i32; 4],
}

/// the ammo pools, by a gun's slot that uses each, and what its rounds are called
const POOLS: [(i32, &str); 4] = [(2, "bullets"), (3, "shells"), (5, "rockets"), (6, "cells")];
/// health lost in this long (tics)...
const HURT_WINDOW: i64 = 3 * 35;
/// ...this much of it, is told at once, not left for a mark to be passed
const HURT_FAST: f64 = 25.0;
/// ...at most this often
const HURT_AGAIN: i64 = 5 * 35;
/// on her own, what she's doing is said at most this often (tics)
const AUTO_SAY_EVERY: i64 = 8 * 35;
/// a fight's summary is worth a reaction when it cost this much health, or this many died
const FIGHT_NOTABLE_HEALTH: f64 = 20.0;
const FIGHT_NOTABLE_KILLS: i32 = 3;
/// ...and worth a line at all when two or more died, or it cost this much health
const FIGHT_WORTH_HEALTH: f64 = 5.0;

/// Her say in the fight (`Director::fight`): each holds till its time's up, then the rules have
/// it back, so one forgotten can't go on costing her.
#[derive(Default)]
struct Tactics {
    /// shot first: one monster (its id) or the nearest of a kind (its name); what it was called;
    /// till when
    target: Option<(Aim, String, i64)>,
    /// the gun to use (the game's number: `gun_named`), till when
    gun: Option<(i32, i64)>,
    hold_fire: Option<i64>,
    /// a stance taken (its name), till when
    stance: Option<(&'static str, i64)>,
}

impl Tactics {
    /// Any of her say in the fight still holding.
    fn any(&self, tic: i64) -> bool {
        self.target.as_ref().is_some_and(|t| t.2 > tic)
            || self.gun.is_some_and(|g| g.1 > tic)
            || self.hold_fire.is_some_and(|t| t > tic)
            || self.stance.is_some_and(|s| s.1 > tic)
    }
}

/// What a target order is of.
enum Aim {
    One(i32),
    Kind(&'static str),
}

/// a tactic holds as long as the clocks say (`Directed::tactic`) unless she says, and at most
/// this long (seconds)
const TACTIC_MOST: f64 = 60.0;
/// a stance (`Directed::stance`), longer
const STANCE_MOST: f64 = 180.0;

/// The stances she can order, with what each puts over the tuning (the advisor's, as
/// config/jev.json has them: a first guess, not tuned).
fn stance(name: &str) -> Option<(&'static str, serde_json::Value)> {
    // (for a while, with `doom_fight`: bold or careful; standing ones are her policy's)
    let name = match name {
        "bold" | "push" | "aggressive" => "bold",
        "careful" | "cautious" => "careful",
        // (to get past a fight: a while, not a standing choice)
        "avoid" | "sneak" | "past" => "avoid",
        _ => return None,
    };
    Some((name, policy::stance_tuning(name)?))
}

/// A monster's kind from what she'd call it ("mancubus", "imps", "the archvile").
fn monster_kind(word: &str) -> Option<&'static str> {
    let w: String = word
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    let w = w.trim_prefix("the").trim_suffix('s');
    Some(match w {
        "zombieman" | "zombie" | "trooper" | "formerhuman" => "Zombieman",
        "shotgunguy" | "shotgunner" | "sergeant" => "ShotgunGuy",
        "chaingunguy" | "chaingunner" | "heavyweapondude" => "ChaingunGuy",
        "doomimp" | "imp" => "DoomImp",
        "demon" | "pinky" => "Demon",
        "spectre" => "Spectre",
        "lostsoul" | "soul" => "LostSoul",
        "cacodemon" | "caco" => "Cacodemon",
        "hellknight" | "knight" => "HellKnight",
        "baronofhell" | "baron" => "BaronOfHell",
        "arachnotron" | "spider" => "Arachnotron",
        "painelemental" | "elemental" => "PainElemental",
        "revenant" | "skeleton" => "Revenant",
        "fatso" | "mancubu" | "mancubus" | "mancubi" => "Fatso",
        "archvile" | "vile" => "Archvile",
        "spidermastermind" | "mastermind" | "spiderdemon" => "SpiderMastermind",
        "cyberdemon" | "cyber" => "Cyberdemon",
        "wolfensteins" | "ss" | "nazi" => "WolfensteinSS",
        _ => return None,
    })
}

/// A gun's slot from what she'd call it.
fn gun_named(word: &str) -> Option<i32> {
    let w: String = word
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    // (the game's numbers: the plain shotgun and the super are two guns, the fist and the
    // chainsaw two)
    Some(match w.as_str() {
        // ("berserk fist" is what her status calls it)
        "fist" | "fists" | "berserk" | "berserkfist" | "punch" => 0,
        "chainsaw" | "saw" => 7,
        "pistol" => 1,
        "shotgun" | "sg" => 2,
        "supershotgun" | "ssg" | "super" | "doublebarrel" => 8,
        "chaingun" | "cg" => 3,
        "rocketlauncher" | "rocket" | "rockets" | "rl" | "launcher" => 4,
        "plasmarifle" | "plasma" | "plasmagun" => 5,
        "bfg" | "bfg9000" => 6,
        _ => return None,
    })
}

impl Director {
    /// One of her settings changed (`setting`, `value` by name): applied from the next tic, and
    /// what she's told; yolo asked first (a question: she's sure, or not). Err: no such setting
    /// or value.
    pub fn set_policy(
        &mut self,
        obs: &Observation,
        setting: &str,
        value: &str,
    ) -> Result<String, String> {
        let v = policy::value(setting, value)
            .ok_or_else(|| format!("{setting}: no \"{value}\". {}", policy::help_for(setting)))?;
        if setting == "stance" && v == "yolo" && self.policy.stance != "yolo" {
            let asked = self.ask(
                obs,
                "Yolo: you fight everything head on, never fall back, never keep clear of anything. Sure?".into(),
                vec![
                    ask::Choice {
                        label: "yes, yolo".into(),
                        then: ask::Then::Policy("stance", "yolo"),
                    },
                    ask::Choice {
                        label: "no".into(),
                        then: ask::Then::Nothing,
                    },
                ],
                1,
                true,
            );
            return asked.ok_or_else(|| "Not now: answer what's asked first.".into());
        }
        Ok(self.policy_to(setting, v))
    }

    /// A setting taken (already checked), to be applied; what she's told.
    pub(super) fn policy_to(&mut self, setting: &str, v: &'static str) -> String {
        match setting {
            "stance" => self.policy.stance = v,
            "save" => self.policy.save = v,
            "risk" => self.policy.risk = v,
            "explore" => self.policy.explore = v,
            _ => self.policy.powerups = v,
        }
        self.policy_due = true;
        format!("Your policy: {}.", self.policy.line())
    }

    /// The gun in her hands, as she'd call it.
    /// Her calls for the viewers (the stream's column): her stance (one taken for a while, with
    /// what's left of it), what she's said in the fight that still holds, and any setting not at
    /// its default.
    pub fn calls(&self, tic: i64) -> Vec<String> {
        let left = |until: i64| format!("{} s", ((until - tic).max(0) + 34) / 35);
        let mut out: Vec<String> = self
            .goal_label()
            .map(|g| format!("Goal: {g}"))
            .into_iter()
            .collect();
        out.push(match self.tactics.stance {
            Some((name, until)) if until > tic => format!("Stance: {name} ({})", left(until)),
            _ => format!("Stance: {}", self.policy.stance),
        });
        if let Some((_, name, until)) = self.tactics.target.as_ref().filter(|t| t.2 > tic) {
            out.push(format!("Target: {name} ({})", left(*until)));
        }
        if let Some((gun, until)) = self.tactics.gun.filter(|g| g.1 > tic) {
            out.push(format!("Gun: {} ({})", gun_called(gun), left(until)));
        }
        if let Some(until) = self.tactics.hold_fire.filter(|&t| t > tic) {
            out.push(format!("Holding fire ({})", left(until)));
        }
        let changed: Vec<String> = self
            .policy
            .changed()
            .split(", ")
            .filter(|c| !c.is_empty() && !c.starts_with("stance"))
            .map(str::to_string)
            .collect();
        if !changed.is_empty() {
            out.push(changed.join(" · "));
        }
        out
    }

    pub fn holding(&self, obs: &Observation) -> &'static str {
        obs.ready.map_or("?", gun_called)
    }
}

/// A gun by the game's number, as she'd call it.
fn gun_called(weapon: i32) -> &'static str {
    match weapon {
        0 => "fist",
        1 => "pistol",
        2 => "shotgun",
        3 => "chaingun",
        4 => "rocket launcher",
        5 => "plasma rifle",
        6 => "BFG",
        7 => "chainsaw",
        8 => "super shotgun",
        _ => "?",
    }
}

/// An order waiting its turn.
struct Queued {
    order: Order,
    /// pressed on with mid-fight
    rush: bool,
    /// after the order in hand, not instead of it
    then: bool,
}

/// with no action from her this long (tics), she carries on by instinct (2026-11-11:
/// a bot standing still waiting on her looks dead on stream; 6 s at first, then 3, now at
/// once: an action of hers takes over, and she's asked what next as instinct takes up;
/// `doom_policy instinct off` has her wait)
pub const IDLE_AUTO: i64 = 0;
pub use crate::console::CONSOLE_NAME;

/// a go to an area whose middle she has no way to: at most this many of its unexplored spots,
/// and this many of its other known spots, tried for one she has
const AREA_TRIES: usize = 6;

#[derive(Default)]
pub struct Director {
    /// orders to carry out
    queue: VecDeque<Queued>,
    /// an explore order an errand broke into (a go, in the middle of it): gone on with once
    /// the errand's done
    resume: Option<Order>,
    doing: Option<Doing>,
    /// the objectives taken for the order in hand: not taken again for it (the explorer may go on
    /// offering one it counts as done, a wall "not seen yet" taken every tic, stood on)
    taken: std::collections::BTreeSet<Taken>,
    /// id -> her name for it (this map)
    names: BTreeMap<String, String>,
    places: Option<Places>,
    pub reports: Vec<Report>,
    /// (co-op) the others in the game (`team.rs`), and her status's line of them
    pub team: Option<Team>,
    team_line: String,
    /// what's been told already: exits, switches and doors, items, monsters (ids), the health
    /// mark passed, keys and weapons held, dead
    told_exits: std::collections::BTreeSet<String>,
    /// the explorer's rankings asked for this tic (`open::ranked`)
    open_kept: std::cell::RefCell<Vec<open::OpenKept>>,
    /// what she knew, and when, as the news last listed what's open (safe, risky)
    news_listed: [((u64, usize), i64); 2],
    /// her own plans told her this life (`plans.rs`)
    plans_told: Vec<crate::tactical::player::Plan>,
    told_items: std::collections::BTreeSet<i32>,
    /// items seen she couldn't get to (told when she can), and those told so
    out_of_reach: std::collections::BTreeSet<i32>,
    told_reachable: std::collections::BTreeSet<i32>,
    told_monsters: std::collections::BTreeSet<i32>,
    health_mark: Option<i32>,
    had_keys: Option<crate::specials::Keys>,
    had_weapons: Option<Vec<i32>>,
    told_dead: bool,
    /// when she last tried getting off a floor that hurts
    off_tried: i64,
    /// told she's on a floor that hurts with no known way off (till she's off it)
    told_trapped: bool,
    /// an explore order paused for a fight (she fights; it goes on once it's over); and told so
    /// (once an order: the status says the rest)
    paused: bool,
    told_paused: bool,
    /// since when nothing's been fighting her (None: something is)
    calm_since: Option<i64>,
    /// the areas the last list found nothing more to explore in, and not where she is
    done_areas: std::collections::BTreeSet<String>,
    /// a switch pressed: its line, what it's called, and till when to watch for what it did (what
    /// it moves is learned seeing it move, a moment after)
    pressed: Option<(usize, String, i64)>,
    /// her say in the fight, while each holds
    tactics: Tactics,
    /// with no order this long (tics) she carries on by herself (None: she waits, as long as
    /// it takes); and since when she's had none
    pub idle_auto: Option<i64>,
    /// a moment to ask her what next, on with what she's at meanwhile ("step": instinct's taken
    /// up something new; "done": instinct's taken up after an order of hers; "fight": a fight
    /// worth telling of is over); whoever speaks for her takes it
    pub next_due: Option<&'static str>,
    /// who's at the stream console, as she's told of what they do (`console_name`)
    pub console_name: String,
    idle_since: Option<i64>,
    /// what she's last said she's doing by herself
    told_auto: String,
    /// the level's 100% told her
    told_complete: bool,
    /// when the level's done, if she's said (`doom_goal`'s `finish`), and what she'd have
    /// first meanwhile (its `prefer`)
    goal: Option<goal::Goal>,
    prefer: Option<goal::Prefer>,
    /// ...which has no say just now: she has enough of it (`has_enough`)
    prefer_lull: bool,
    told_auto_tic: Option<i64>,
    /// on her own she found nothing more to do: not taken up again till an order's been given
    auto_spent: bool,
    /// an order of hers begun since she was last on instinct: instinct taking up after it is
    /// the moment to ask her what next
    had_order: bool,
    /// how often it's suggested its pick this wait for an order
    nudged: u8,
    /// kills and blasts being gathered to tell (`kills.rs`); and what the game's runner gives
    /// it each tic, as it gives the tally: the hits on her since the last, and the monsters
    /// lying dead in pieces
    kills: kills::Kills,
    pub hits: Vec<crate::dsda::Hit>,
    pub gibbed: Vec<i32>,
    /// the level, from the runner; and the moments being kept track of (`moments.rs`)
    pub level: Level,
    moments: moments::Moments,
    /// the level again hers to ask for (the console's to say; set by the runner), and asked for
    /// (for the runner to do); progress lately, (what it was, since when), and her told it's hers
    pub restart_allowed: bool,
    pub restart_wanted: bool,
    /// the ultra instinct is fighting for her now (the watch's to say each tic), and the
    /// seconds of it she has left where it's earned by dying (None: no limit to it)
    pub ultra: Option<Option<f64>>,
    /// questions to her (`ask.rs`), and how long one waits for her answer before its default
    /// (None: as long as it takes)
    asking: ask::Asking,
    pub answer_wait: Option<i64>,
    /// directed play's clocks, as the run's tuning has them (`set_clocks`)
    pub clocks: Directed,
    /// powers running, (what, till when, told it's ending); each ammo pool last tic (`powers.rs`)
    running: Vec<(&'static str, i64, bool)>,
    pools_had: Option<[i32; 4]>,
    /// what hurt her lately, (tic, what, health): her death's story (`death.rs`)
    hurt: VecDeque<(i64, String, i32)>,
    /// how things stand this tic (`situation.rs`): what tells her reads it
    pub now: Situation,
    /// her standing settings (`policy.rs`), and whether they're yet to be applied (a change, a
    /// new level: a new player)
    pub policy: Policy,
    policy_due: bool,
    /// the last resorts she's let her instinct take, this level
    resorts: std::collections::BTreeSet<crate::knowledge::explorer::Resort>,
    progress: Option<((usize, [usize; 6]), i64)>,
    told_restart: bool,
    /// told, this while with nothing new, where a secret's likeliest
    told_search: bool,
    /// the fight she's in, for its summary when it's over; and what she had last tic (what a
    /// fight cost is the falls from tic to tic, so a medikit taken in the middle doesn't hide it)
    fight: Option<Fight>,
    was: Option<Had>,
    /// her health lately, (tic, health): for telling a fast loss while there's time to act on it
    health_lately: VecDeque<(i64, f64)>,
    told_hurt: Option<i64>,
    told_secrets: Option<i32>,
    /// the game's last word on a lock, and when (told again only after a while: she presses
    /// a locked door more than once)
    told_lock: Option<(String, i64)>,
    /// the level's tally (kills, items, secrets: got, of), as a source port shows it; set each tic
    /// by whoever runs the game (it's the engine's, not in what she observes)
    pub tally: Tally,
}

/// The level's tally: kills, items, secrets, each (got, of).
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    pub kills: (i32, i32),
    pub items: (i32, i32),
    pub secrets: (i32, i32),
}

impl Tally {
    /// From the engine's (items, kills, secrets).
    pub fn of(t: [(i32, i32); 3]) -> Tally {
        Tally {
            items: t[0],
            kills: t[1],
            secrets: t[2],
        }
    }

    fn line(&self) -> String {
        let f = |(got, of): (i32, i32)| format!("{got}/{of}");
        format!(
            "kills {}, items {}, secrets {}",
            f(self.kills),
            f(self.items),
            f(self.secrets)
        )
    }

    fn complete(&self) -> bool {
        [self.kills, self.items, self.secrets]
            .iter()
            .all(|(got, of)| got >= of)
    }
}

impl Director {
    pub fn new() -> Director {
        Director {
            answer_wait: Some(Directed::tics(Directed::default().answer_wait)),
            idle_auto: Some(IDLE_AUTO),
            next_due: None,
            console_name: CONSOLE_NAME.into(),
            policy_due: true,
            ..Director::default()
        }
    }

    /// The run's clocks for directed play (its tuning's `directed`); how long a question waits
    /// with them (0: as long as it takes).
    pub fn set_clocks(&mut self, c: &Directed) {
        self.clocks = c.clone();
        self.answer_wait = (c.answer_wait > 0.0).then(|| Directed::tics(c.answer_wait));
    }

    /// Directed play on: the explorer chooses nothing of its own from now.
    pub fn take_over(agent: &mut Agent) {
        let x = agent.x_mut();
        x.directed = true;
        x.current = None;
        x.follower = None;
    }

    pub fn order(&mut self, o: Order) {
        self.order_with(o, false, false);
    }

    /// An order rushed (`rush`): pressed on with mid-fight, shooting and dodging on the way, not
    /// stopped for the fight (still getting clear of what claws, and away when nearly dead).
    pub fn order_rushed(&mut self, o: Order, rush: bool) {
        self.order_with(o, rush, false);
    }

    /// An order, rushed or not; `then`: once the order in hand is done, not instead of it (an
    /// order given without it replaces the one in hand, which is said).
    pub fn order_with(&mut self, order: Order, rush: bool, then: bool) {
        // (an order in place of what she's doing settles what's asked; one queued after it
        // doesn't)
        if !then {
            self.ordered();
        }
        self.queue.push_back(Queued { order, rush, then });
    }

    /// The sweep of area `id` is next in the queue, to come after what's in hand (the second
    /// half of a rushed sweep).
    fn sweep_queued_for(&self, id: &str) -> bool {
        matches!(
            self.queue.front(),
            Some(Queued { order: Order::ExploreArea(r, _), then: true, .. }) if self.resolve(r) == id
        )
    }

    /// Doing nothing, waiting for an order?
    pub fn idle(&self, agent: &Agent) -> bool {
        self.queue.is_empty()
            && self.doing.is_none()
            && self.pressed.is_none()
            && agent.x().current.is_none()
    }

    /// A new map: names, orders, places and what's been told are the old map's.
    pub fn new_map(&mut self) {
        self.goal_new_map();
        self.overtaken("a new level");
        self.plans_told.clear();
        // (a level's tics start again from nothing: no time kept from the old level's clock
        // is left to be measured against the new one's. What's timed across levels, the
        // questions a minute, is put back by the old level's length)
        self.questions_new_map(self.moments.last_tic);
        self.off_tried = 0;
        self.told_trapped = false;
        self.paused = false;
        self.told_paused = false;
        self.calm_since = None;
        self.next_due = None;
        self.had_order = false;
        self.nudged = 0;
        self.told_auto.clear();
        self.told_auto_tic = None;
        self.told_hurt = None;
        self.told_lock = None;
        self.hits.clear();
        self.gibbed.clear();
        self.policy_due = true;
        self.hurt.clear();
        self.running.clear();
        self.pools_had = None;
        self.resorts.clear();
        self.queue.clear();
        self.resume = None;
        self.tactics = Tactics::default();
        self.auto_spent = false;
        self.doing = None;
        self.pressed = None;
        self.idle_since = None;
        self.taken.clear();
        self.done_areas.clear();
        self.had_keys = None;
        self.names.clear();
        self.places = None;
        self.told_exits.clear();
        self.fresh_ranking();
        self.news_listed = [((0, 0), 0); 2];
        self.told_complete = false;
        self.told_items.clear();
        self.out_of_reach.clear();
        self.told_reachable.clear();
        self.told_monsters.clear();
        self.health_mark = None;
        self.told_dead = false;
        self.fight = None;
        self.was = None;
        self.health_lately.clear();
        self.told_secrets = None;
        self.kills = kills::Kills::default();
        self.moments = moments::Moments::default();
        self.progress = None;
        self.told_restart = false;
        self.told_search = false;
        self.restart_wanted = false;
    }

    /// The level again from its start (a death, a restart): her orders and her say in the fight
    /// are the attempt before's; the fight, her health's marks and what's killed start afresh.
    fn new_life(&mut self) {
        self.overtaken("the level begun again");
        self.moments.new_life();
        // (a gun before any key: told once a level, not each life of it)
        self.plans_told
            .retain(|p| *p == crate::tactical::player::Plan::GunBeforeKey);
        self.hurt.clear();
        self.running.clear();
        self.pools_had = None;
        self.queue.clear();
        self.resume = None;
        self.tactics = Tactics::default();
        self.auto_spent = false;
        self.doing = None;
        self.pressed = None;
        self.idle_since = None;
        self.taken.clear();
        self.told_dead = false;
        self.fight = None;
        self.was = None;
        self.health_lately.clear();
        self.health_mark = None;
        self.kills = kills::Kills::default();
    }

    /// What she's doing on her orders, in a few words ("exploring area-0 +secrets 2, rushing").
    pub fn doing(&self, agent: &Agent) -> String {
        let doing = match &self.doing {
            Some(Doing::Once(_, w)) => format!("going to {w}"),
            Some(Doing::Area(_, w, secrets)) => format!(
                "exploring {w}{}{}",
                with_secrets(*secrets),
                if self.paused {
                    " (paused for the fight)"
                } else {
                    ""
                }
            ),
            Some(Doing::Way(b, secrets)) => format!(
                "exploring {}{}{}",
                compass(*b),
                with_secrets(*secrets),
                if self.paused {
                    " (paused for the fight)"
                } else {
                    ""
                }
            ),
            Some(Doing::Off) => "getting off a hurting floor".to_string(),
            Some(Doing::Pickup) => "picking up nearby".to_string(),
            Some(Doing::Auto) => format!(
                "{}{}",
                self.goal_label().map_or_else(
                    || "by instinct".to_string(),
                    |g| format!("for your goal ({g})")
                ),
                agent
                    .x()
                    .current
                    .as_ref()
                    .map_or(String::new(), |o| format!(": {}", plain(o.kind, &o.label)))
            ),
            None if agent.x().current.is_some() => "busy".to_string(),
            None => "waiting".to_string(),
        };
        if agent.x().press_on {
            format!("{doing}, rushing")
        } else {
            doing
        }
    }

    /// Her state in a line: health, armor, weapons, ammo, keys, what she's doing.
    pub fn status(&self, agent: &Agent, obs: &Observation) -> String {
        let doing = self.doing(agent);
        // (in a fight: what's in sight, the nearest; an order then is held up by it unless rushed)
        // (...and what the fight's doing, and that she isn't shooting when she isn't and it'll
        // last: she has no picture of it, and can't ask why of what she doesn't know)
        let doing = match fighting(&engaged(agent, obs), obs) {
            Some(what) => {
                match self.not_shooting(agent) {
                    // (not "shooting on the way, not shooting")
                    Some(why) => format!(
                        "{doing}; fighting {what}: {}; not shooting: {why}",
                        self.now.doing().replace(", shooting on the way", "")
                    ),
                    None => format!("{doing}; fighting {what}: {}", self.now.doing()),
                }
            }
            None => doing,
        };
        // (a plan of her own in force: what she's about, beyond the errand in hand)
        let plan = self
            .plan_words(agent, obs)
            .map_or(String::new(), |p| format!("; {p}"));
        // (the ultra instinct fighting for her: she'd not know it from the fight's own words)
        let ultra = match self.ultra {
            Some(Some(secs)) => format!("; Ultra Instinct on, {secs:.0} s left"),
            Some(None) => "; Ultra Instinct on".to_string(),
            None => String::new(),
        };
        let team = if self.team_line.is_empty() {
            String::new()
        } else {
            format!("; {}", self.team_line)
        };
        // (short: what Evil reads costs her context)
        format!(
            "{}; {}; {doing}{plan}{ultra}{team}",
            kit(obs),
            self.tally.line()
        )
    }

    /// What in her status she'd act on, to tell whether it's changed: health and armor by tens,
    /// her weapons and keys, what she's doing and whether she's fighting, the tally. (Not each
    /// round spent, nor where what's fighting her has got to.)
    pub fn status_gist(&self, agent: &Agent, obs: &Observation) -> String {
        format!(
            "{} {} {:?} {:?} {} {} {} {:?} {:?} {}",
            (self.now.health / 10.0).floor(),
            (self.now.armor / 10.0).floor(),
            obs.weapons,
            obs.keys.unwrap_or_default().iter().count(),
            self.doing(agent),
            self.now.fighting(),
            self.tally.line(),
            self.now.fighting().then(|| self.now.doing()),
            self.not_shooting(agent),
            self.ultra.is_some()
        )
    }

    /// Leaving now: what she'd leave behind, as a question to be sure (None: nothing left, the
    /// tally complete and nothing more known to do). An exit asks first, for now. (None too with
    /// no exit known: the order says so.)
    pub fn exit_check(&mut self, agent: &mut Agent, obs: &Observation) -> Option<String> {
        // (the exits too while they're held back from her instinct: leaving's asked either way)
        let open = self.open_unheld(agent, obs);
        if !open.iter().any(|o| o.kind == Kind::Exit) {
            return None;
        }
        let count = |f: &dyn Fn(&Objective) -> bool| {
            open.iter()
                .filter(|o| f(o))
                .map(id_of)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        let explore = count(&|o| explores(o));
        let switches = count(&|o| o.kind == Kind::Switch);
        let doors = count(&|o| o.kind == Kind::Door);
        // (the items tally's own apart from the rest: "12 items seen" beside "items 5/6" reads as
        // a contradiction; ammo and health she was full of are pickups, not the tally's)
        let seen = self.list(agent, obs, Topic::Items);
        let counted = seen.iter().filter(|e| e.what.ends_with(COUNTS)).count();
        let pickups = seen.len() - counted;
        let mut left = Vec::new();
        for (n, one, many) in [
            (explore, "to explore", "to explore"),
            (switches, "switch", "switches"),
            (doors, "door", "doors"),
            (counted, "tally item", "tally items"),
            (pickups, "other pickup", "other pickups"),
        ] {
            if n > 0 {
                left.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        if left.is_empty() && self.tally.complete() {
            return None;
        }
        let known = if left.is_empty() {
            String::new()
        } else {
            format!("{}; ", left.join(", "))
        };
        Some(format!("Still here: {known}{}.", self.tally.line()))
    }

    /// Each tic: the order in hand carried on (its next objective, or its end reported); the next
    /// order taken up.
    pub fn tick(&mut self, agent: &mut Agent, obs: &Observation) {
        let hits = self.hits.clone();
        self.remember_hits(obs.tic, &hits);
        use crate::debug::timed;
        timed("director.take_stock", || self.take_stock(agent, obs));
        // (her exploring style, for when she's on instinct; and whether there's a secret left
        // to press walls for)
        agent.x_mut().going_back = self.policy.going_back();
        agent.x_mut().secrets_done = self.secrets_done();
        agent.x_mut().secret_search = matches!(
            self.goal,
            Some(goal::Goal::Completionist | goal::Goal::Secrets)
        ) && !self.secrets_done();
        timed("director.check_complete", || {
            self.check_complete(agent, obs)
        });
        // (her policy on her numbers: on a change, and on each new level's player)
        if self.policy_due
            && let Some(p) = agent.player_mut()
        {
            self.policy_due = false;
            if let Err(e) = p.set_policy(&self.policy.tuning()) {
                self.tell(
                    obs,
                    Event::Outcome,
                    format!("Your policy couldn't be applied: {e}."),
                );
            }
        }

        timed("director.events", || self.events(agent, obs));
        timed("director.kills", || self.kills(agent, obs));
        timed("director.more_moments", || self.more_moments(agent, obs));
        timed("director.watch_progress", || {
            self.watch_progress(agent, obs)
        });
        self.powers(obs);
        timed("director.plans", || self.plans(agent, obs));
        self.held_fire_news(agent, obs);
        timed("director.questions", || self.questions(agent, obs));
        // (a jump that costs is asked about, and she waits at its line-up while it is)
        let x = agent.x_mut();
        x.jump_asked = true;
        x.jump_held = self
            .question()
            .is_some_and(|q| matches!(q.about, About::Jump(_)));
        timed("director.apply_tactics", || self.apply_tactics(agent, obs));
        // (...once she's seen a way out this life: till then a sphere or a berserk she's no
        // need of is left for when she might, as without the goal. One known only from a life
        // before isn't yet: she has the level to get through again)
        let everything =
            self.goal == Some(goal::Goal::Completionist) && agent.x().exit_seen_this_life();
        let preferred = self.preferred_things(obs);
        agent.x_mut().spares = obs
            .things
            .iter()
            .flatten()
            // (her goal everything: what the items tally counts too, need it or not: a bonus
            // at 200 is still one of the level's items)
            .filter(|t| {
                spare(t.name, obs)
                    // (what she prefers, health or ammo: hers to have gone for, wanted or not)
                    || preferred.contains(&t.id)
                    || (everything
                        && crate::knowledge::completion::Completionist::counted(t.name, obs))
            })
            .map(|t| t.id)
            .collect();
        if let Some((line, what, until)) = self.pressed.clone() {
            let effect = self.effect_of(agent, obs, line);
            if effect.is_some() || obs.tic >= until {
                self.pressed = None;
                self.report(
                    obs,
                    true,
                    Event::Outcome,
                    match effect {
                        Some(w) => format!("Pressed {what}: {w}."),
                        None => format!("Pressed {what}: no change seen."),
                    },
                );
            }
        }
        // (the next order: at once, in place of the one in hand, unless it was given to come
        // after it)
        let in_hand = matches!(
            self.doing,
            Some(Doing::Once(..) | Doing::Area(..) | Doing::Way(..))
        ) || self.pressed.is_some();
        if let Some(Queued { order: o, rush, .. }) = self
            .queue
            .front()
            .is_some_and(|q| !q.then || !in_hand)
            .then(|| self.queue.pop_front())
            .flatten()
        {
            // (a go with the sweep of the same area queued after it, a rushed sweep: with no
            // way there the sweep's dropped with it, one failure told, not two)
            let went = match &o {
                Order::Go(r, _) => Some(self.resolve(r)),
                _ => None,
            };
            self.fresh_ranking();
            timed("director.start", || self.start(agent, obs, o));
            if let Some(id) = went
                && self.doing.is_none()
                && self.sweep_queued_for(&id)
            {
                self.queue.pop_front();
            }
            // (a single target only: a sweep of a room, rushed, is a run about it under fire)
            agent.x_mut().press_on = rush && matches!(self.doing, Some(Doing::Once(..)));
            return;
        }
        if timed("director.explore_paused", || {
            self.explore_paused(agent, obs)
        }) {
            return;
        }
        if timed("director.goal_tick", || self.goal_tick(agent, obs)) {
            return;
        }
        if matches!(self.doing, Some(Doing::Auto)) {
            timed("director.auto", || self.auto(agent, obs));
            return;
        }
        let ended = agent.x_mut().ended.take();
        if agent.x().current.is_some() {
            return;
        }
        // (no order, nothing in hand: since when; and after `idle_auto` of it, on by herself)
        let waiting = matches!(self.doing, None | Some(Doing::Off | Doing::Pickup))
            && self.queue.is_empty()
            && self.resume.is_none()
            && self.pressed.is_none();
        if !waiting {
            self.idle_since = None;
            self.nudged = 0;
        } else if self.idle_since.is_none() {
            self.idle_since = Some(obs.tic);
        }
        if let (Some(limit), Some(since)) = (self.idle_auto, self.idle_since) {
            // (not again once she's found nothing more to do by herself, till an order's come)
            if obs.tic - since >= limit && obs.health.unwrap_or(0.0) > 0.0 && !self.auto_spent {
                self.go_instinct(agent);
                // (her goal's things first from its first choice, not its second)
                self.goal_tick(agent, obs);
                if std::mem::take(&mut self.had_order) {
                    self.next_due = Some("done");
                }
                // (at once, it's not news: what she takes up is told, and she's asked)
                if limit > 0 {
                    self.report(
                        obs,
                        true,
                        Event::OnInstinct,
                        format!(
                            "No action for {} s: on by instinct till your next.",
                            limit / 35
                        ),
                    );
                }
                return;
            }
        }
        if let Some(since) = self.idle_since {
            timed("director.nudge", || self.nudge(agent, obs, obs.tic - since));
        }
        if self.doing.is_none() || matches!(self.doing, Some(Doing::Off | Doing::Pickup)) {
            agent.x_mut().press_on = false; // (her own doing: no order's rush)
        }
        // (an errand done, or come to nothing: on with the exploring it broke into)
        if self.doing.is_none()
            && self.queue.is_empty()
            && self.pressed.is_none()
            && let Some(order) = self.resume.take()
        {
            // (the bot's own going on, not hers: nothing asked is settled by it)
            self.queue.push_back(Queued {
                order,
                rush: false,
                then: false,
            });
            return;
        }
        let _orders = crate::debug::Timed::new("director.orders");
        match self.doing.take() {
            None => {
                // (waiting: never on a floor that hurts)
                if self.queue.is_empty() && obs.tic - self.off_tried >= OFF_AGAIN {
                    self.off_tried = obs.tic;
                    if self.off_hurting_floor(agent, obs) {
                        self.doing = Some(Doing::Off);
                    } else if self.pick_up_nearby(agent, obs) {
                        self.doing = Some(Doing::Pickup);
                    }
                }
            }
            // (one picked up: the next straight away, if there's one, not a tic waiting between)
            Some(Doing::Pickup) => {
                if self.pick_up_nearby(agent, obs) {
                    self.doing = Some(Doing::Pickup);
                }
            }
            Some(Doing::Off) => {}
            // (handled above: never taken here)
            Some(Doing::Auto) => self.doing = Some(Doing::Auto),
            Some(Doing::Once(id, what)) => {
                let (ok, why, pressed_nothing) = ended
                    .map_or((false, "stopped".to_string(), false), |e| {
                        (e.ok, e.plain, e.pressed)
                    });
                // (a switch pressed: what it did told when she's seen it, or after a while)
                // (pressed and nothing seen to change is pressed all the same: what it moved may be
                // out of sight, across an arena; not "couldn't")
                let pressed = ok || pressed_nothing;
                match id
                    .strip_prefix("switch-l")
                    .and_then(|l| l.parse::<usize>().ok())
                    .filter(|_| pressed)
                {
                    Some(l) => self.pressed = Some((l, id.clone(), obs.tic + PRESS_WATCH)),
                    // (got to an area whose sweep comes next: the sweep says so, not a "done"
                    // in the middle of the one order of hers)
                    None if ok && self.sweep_queued_for(&id) => {}
                    None => {
                        // (not got to: its sweep's dropped with it)
                        if self.sweep_queued_for(&id) {
                            self.queue.pop_front();
                        }
                        self.report(
                            obs,
                            ok,
                            Event::Outcome,
                            if ok {
                                format!("Done: {what}.")
                            } else {
                                format!("Couldn't get to {what}: {why}.")
                            },
                        )
                    }
                }
            }
            Some(Doing::Area(id, what, secrets)) => {
                if self.next_in(agent, obs, secrets, |d, a, o| d.in_area(a, &id, o)) {
                    self.doing = Some(Doing::Area(id, what, secrets));
                } else {
                    let found = self.found(agent, obs, |d, a, o| d.in_area(a, &id, o));
                    self.report(
                        obs,
                        true,
                        Event::Outcome,
                        format!("Explored {what}{}.{found}", searched(secrets)),
                    );
                }
            }
            Some(Doing::Way(b, secrets)) => {
                let pos = obs.pos();
                let fits = move |_: &Self, _: &Agent, o: &Objective| {
                    dist(pos, o.goal) > 64.0 && norm(heading(pos, o.goal) - b).abs() < EXPLORE_CONE
                };
                if self.next_in(agent, obs, secrets, fits) {
                    self.doing = Some(Doing::Way(b, secrets));
                } else {
                    let found = self.found(agent, obs, fits);
                    self.report(
                        obs,
                        true,
                        Event::Outcome,
                        format!("Explored {}{}.{found}", compass(b), searched(secrets)),
                    );
                }
            }
        }
    }

    /// An explore order and a fight: paused while something's fighting her (the objective in
    /// hand dropped, the fight free to move her; what it shows her she maps all the same), on
    /// again after RESUME_CALM with nothing fighting her. True while paused. (A sweep of look-at
    /// spots under fire never ends, rushed or not.)
    fn explore_paused(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        // (fighting: something is, and the fight has her legs; one whose move lets the route
        // go on, something out of reach shooting at her, say, is gone on past, as on her own)
        let goes_on = agent
            .player()
            .and_then(|p| p.combat().last_move)
            .is_some_and(crate::tactical::combat::Move::route_goes_on);
        let fighting = !engaged(agent, obs).is_empty() && !goes_on;
        if fighting {
            self.calm_since = None;
        } else if self.calm_since.is_none() {
            self.calm_since = Some(obs.tic);
        }
        if !matches!(self.doing, Some(Doing::Area(..) | Doing::Way(..))) {
            self.paused = false;
            return false;
        }
        if fighting && !self.paused {
            self.paused = true;
            let x = agent.x_mut();
            if let Some(cur) = x.current.take() {
                self.taken.remove(&taken_key(&cur)); // (to be taken again, if it's still to do)
            }
            x.follower = None;
            x.ended = None;
            // (told once: in an arena it goes on and off, each time a word in her context)
            if !self.told_paused {
                self.told_paused = true;
                self.report(
                    obs,
                    true,
                    Event::Begun,
                    "Exploring paused while you're fighting; it goes on after.".into(),
                );
            }
        }
        if self.paused && self.calm_since.is_some_and(|t| obs.tic - t >= RESUME_CALM) {
            self.paused = false;
        }
        self.paused
    }

    /// Everything the level counts done (every kill, item and secret), with a way out known:
    /// told her once, and asked to leave if she's on instinct (leaving the default: there's
    /// nothing left that counts).
    fn check_complete(&mut self, agent: &mut Agent, obs: &Observation) {
        // (once a second while there's no exit to tell her of: each look is two listings)
        if self.told_complete || !self.tally.complete() || obs.tic % NEWS_EVERY != 0 {
            return;
        }
        let exits = self.objectives_of(agent, obs, &[Kind::Exit]);
        let Some(first) = exits.first() else {
            return;
        };
        self.told_complete = true;
        let line = self.describe(first, obs);
        self.tell(
            obs,
            Event::LevelComplete,
            format!("Level 100%: every kill, item and secret. Exit: {line}."),
        );
        // (a goal of hers says what's next itself: everything's turns into leaving)
        if matches!(self.doing, Some(Doing::Auto))
            && self.question().is_none()
            && self.goal.is_none()
        {
            let known: Vec<(String, String)> = exits
                .iter()
                .map(|e| (e.id.clone(), self.describe(e, obs)))
                .collect();
            self.end_auto(agent);
            self.doing = None;
            self.auto_spent = true;
            self.ask_complete(obs, &known);
        }
    }

    /// An action's news: begun, done, couldn't (`ok`), as `event` has it.
    fn report(&mut self, obs: &Observation, ok: bool, event: Event, text: String) {
        self.reports.push(Report {
            tic: obs.tic,
            ok,
            text,
            event,
        });
    }
}

/// What she has: "Health 100, armor 0; pistol; 50/200 bullets; no keys" (each pool with the
/// most it holds, a backpack's doubling in it).
fn kit(obs: &Observation) -> String {
    // (every gun she has, the plain shotgun and the super apart; the fist only as a berserk's)
    let mut weapons: Vec<&str> = Vec::new();
    if obs.berserk == Some(true) {
        weapons.push("berserk fist");
    }
    for gun in [7, 1, 2, 8, 3, 4, 5, 6] {
        if obs.owned.as_ref().is_some_and(|o| o.contains(&gun)) {
            weapons.push(gun_called(gun));
        }
    }
    let packed = obs.backpack == Some(true);
    let pools: Vec<String> = POOLS
        .iter()
        .filter_map(|&(slot, what)| {
            let n = obs
                .ammo
                .iter()
                .flatten()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n);
            let most = crate::bestiary::ammo_max(slot) * if packed { 2.0 } else { 1.0 };
            (n > 0).then(|| format!("{n}/{most:.0} {what}"))
        })
        .collect();
    let keys: Vec<&str> = obs.keys.unwrap_or_default().iter().map(key_name).collect();
    let list = |v: Vec<String>, none: &str| {
        if v.is_empty() {
            none.to_string()
        } else {
            v.join(", ")
        }
    };
    format!(
        "Health {:.0}, armor {:.0}; {}; {}; {}",
        obs.health.unwrap_or(0.0),
        obs.armor.unwrap_or(0.0),
        list(
            weapons.iter().map(ToString::to_string).collect(),
            "no weapons"
        ),
        list(pools, "no ammo"),
        list(keys.iter().map(ToString::to_string).collect(), "no keys"),
    )
}

/// The objectives of those kinds among `open`, as entries, in its order.
fn entries_of(open: &[Objective], kinds: &[Kind]) -> Vec<Entry> {
    open.iter()
        .filter(|o| kinds.contains(&o.kind))
        .map(|o| {
            let info = o.trigger.as_ref().map(|t| &t.info);
            let what = match info {
                Some(i) if i.category == crate::specials::Category::SecretExit => {
                    "secret exit".to_string()
                }
                Some(i) => lock_name(i.lock).map_or(String::new(), |k| format!("needs {k}")),
                None => String::new(),
            };
            Entry {
                id: id_of(o),
                what,
                at: o.goal,
            }
        })
        .collect()
}

/// An objective as the order in hand remembers taking it: the explorer's ident, and the wall for
/// pressing one (every wall has the same ident: an explore with secrets took one and ended).
type Taken = (Ident, Option<usize>);

fn taken_key(o: &Objective) -> Taken {
    (o.ident(), o.wall)
}

/// An objective's stable id: its kind, and what it's about (a thing, a line, else its spot).
fn id_of(o: &Objective) -> String {
    let kind = match o.kind {
        Kind::Exit => "exit",
        Kind::Key => "key",
        Kind::Switch => "switch",
        Kind::Door => "door",
        Kind::Item => "item",
        Kind::Teleporter => "teleporter",
        _ => "spot",
    };
    if let Some(t) = o.thing {
        format!("{kind}-t{t}")
    } else if let Some(l) = o.trigger.as_ref().map(|t| t.line).filter(|&l| l >= 0) {
        format!("{kind}-l{l}")
    } else if let Some(w) = o.wall {
        format!("{kind}-l{w}")
    } else {
        format!("{kind}-{:.0}x{:.0}", o.goal.0, o.goal.1)
    }
}

/// A gun by name, as the game numbers them (`dsda::weapon_slot` gives its slot).
fn weapon_name(slot: i32, obs: &Observation) -> &'static str {
    match slot {
        1 if obs.chainsaw == Some(true) => "chainsaw",
        1 => "fist",
        2 => "pistol",
        3 if obs.super_shotgun == Some(true) => "super shotgun",
        3 => "shotgun",
        4 => "chaingun",
        5 => "rocket launcher",
        6 => "plasma rifle",
        7 => "BFG",
        _ => "?",
    }
}

pub fn key_name(k: crate::specials::Key) -> &'static str {
    use crate::specials::Key;
    match k {
        Key::BlueCard => "blue keycard",
        Key::YellowCard => "yellow keycard",
        Key::RedCard => "red keycard",
        Key::BlueSkull => "blue skull key",
        Key::YellowSkull => "yellow skull key",
        Key::RedSkull => "red skull key",
    }
}

/// A heading (degrees, 0 east, 90 north) as a compass point.
pub fn compass(h: f64) -> &'static str {
    const POINTS: [&str; 8] = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"];
    POINTS[((h.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

/// A compass point (n, ne, east, south-west, ...) as a heading.
pub fn bearing(s: &str) -> Option<f64> {
    // (a way as lists give it, its distance after: "SE 160" is that way)
    let s = s
        .trim()
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')
        .to_lowercase()
        .replace(['-', ' ', '_'], "");
    Some(match s.as_str() {
        "e" | "east" => 0.0,
        "ne" | "northeast" => 45.0,
        "n" | "north" => 90.0,
        "nw" | "northwest" => 135.0,
        "w" | "west" => 180.0,
        "sw" | "southwest" => 225.0,
        "s" | "south" => 270.0,
        "se" | "southeast" => 315.0,
        _ => return None,
    })
}

/// A way (and a distance) given where an id was wanted: said so.
fn place_not_id(r: &str) -> Option<String> {
    bearing(r).map(|_| {
        format!("\"{r}\" is a way, not an id: nothing listed there; ids come from doom_query.")
    })
}

/// Is o one of exploring's? Not out by an exit or a teleporter, not after a noise or a monster;
/// nor a switch: pressing one is hers to decide, it may set off anything (exploring finds them and
/// says so); nor pressing walls for secrets (a search asked for adds its own).
fn explores(o: &Objective) -> bool {
    !matches!(
        o.kind,
        Kind::Exit | Kind::Teleporter | Kind::Noise | Kind::Recheck | Kind::Hunt | Kind::Switch
    ) && !is_search(o)
}

/// The walls among `walls` (a search's, their suspicion the secret score) to search at each
/// thoroughness, 1 to 3.
fn by_thoroughness<'a>(walls: impl Iterator<Item = &'a Objective>) -> [usize; 3] {
    let mut n = [0; 3];
    for o in walls {
        for (count, (press, _)) in n.iter_mut().zip(crate::knowledge::model::THOROUGHNESS) {
            if o.suspicion >= press {
                *count += 1;
            }
        }
    }
    n
}

/// An exploring order's end, its search said: ", secrets 2 too".
fn searched(secrets: u8) -> String {
    // (1 is what every exploring does: not said)
    if secrets > 1 {
        format!(", secrets {secrets} too")
    } else {
        String::new()
    }
}

/// Searching for secrets: pressing a wall she suspects, back into a secret sector.
fn is_search(o: &Objective) -> bool {
    matches!(o.kind, Kind::Wall | Kind::Secret)
}

fn with_secrets(secrets: u8) -> String {
    if secrets > 1 {
        format!(" +secrets {secrets}")
    } else {
        String::new()
    }
}

/// A pickup she takes by herself whenever it's safe: bonuses below 200, health that heals her
/// without going to waste, ammo with room for all of it.
fn spare(name: &str, obs: &Observation) -> bool {
    use crate::bestiary::{ammo_item, ammo_max, health_item};
    let health = obs.health.unwrap_or(100.0);
    match name {
        "HealthBonus" => health < 200.0,
        "ArmorBonus" => obs.armor.unwrap_or(0.0) < 200.0,
        "Stimpack" | "Medikit" => health_item(name)
            .is_some_and(|(amount, cap)| health + f64::from(amount) <= f64::from(cap)),
        "Backpack" => false,
        _ => ammo_item(name).is_some_and(|(slots, amount)| {
            let slot = slots[0];
            let have = obs
                .ammo
                .iter()
                .flatten()
                .find(|(s, _)| *s == slot)
                .map_or(0, |(_, n)| *n);
            let max = ammo_max(slot) * if obs.backpack == Some(true) { 2.0 } else { 1.0 };
            f64::from(have + amount) <= max
        }),
    }
}

/// What a lock needs, as she'd say it (None: nothing).
fn lock_name(l: crate::specials::Lock) -> Option<&'static str> {
    use crate::specials::Lock;
    Some(match l {
        Lock::None => return None,
        Lock::Red | Lock::RedCard | Lock::RedSkull => "red key",
        Lock::Blue | Lock::BlueCard | Lock::BlueSkull => "blue key",
        Lock::Yellow | Lock::YellowCard | Lock::YellowSkull => "yellow key",
        Lock::Any => "any key",
        _ => "all keys",
    })
}

/// The monsters fighting her now (`Combat::engaged`: able to hurt her from where they are).
/// What she's after, in the viewers' words (not the objective's own: "try the wall (line 8)").
pub fn plain(kind: Kind, label: &str) -> String {
    match kind {
        Kind::Exit => "heading for the exit".into(),
        Kind::Key => "going for a key".into(),
        Kind::Switch => "going to press a switch".into(),
        Kind::Door => "going to open a door".into(),
        Kind::Frontier | Kind::Room | Kind::Visit | Kind::Recheck => "exploring".into(),
        Kind::Noise => "checking out a noise".into(),
        Kind::Teleporter => "taking a teleporter".into(),
        Kind::Item => label.strip_prefix("pick up ").map_or_else(
            || "picking something up".into(),
            |what| {
                // (the thing by what she calls it; the explorer's reason after it kept)
                let (thing, why) = what.split_once(' ').map_or((what, ""), |(t, w)| (t, w));
                let why = if why.is_empty() {
                    String::new()
                } else {
                    format!(" {why}")
                };
                format!("picking up the {}{why}", called(thing))
            },
        ),
        Kind::Hunt => "hunting".into(),
        Kind::Wall => "searching for secrets".into(),
        Kind::Secret => "going back to a secret".into(),
        Kind::Barrel => "clearing a barrel".into(),
    }
}

/// What's fighting her, in a phrase: how many of each kind, and where the nearest is
/// ("2 Fatso, 1 Arachnotron (NW 650)"). None: nothing is.
fn fighting(against: &[crate::domain::Thing], obs: &Observation) -> Option<String> {
    let pos = obs.pos();
    let near = against
        .iter()
        .map(|t| (t.x, t.y))
        .min_by(|a, b| dist(pos, *a).total_cmp(&dist(pos, *b)))?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for t in against {
        *counts.entry(t.name).or_insert(0) += 1;
    }
    let what: Vec<String> = counts.iter().map(|(n, c)| some(*c, n)).collect();
    Some(format!(
        "{} ({} {:.0})",
        what.join(", "),
        compass(heading(pos, near)),
        dist(pos, near)
    ))
}

fn engaged(agent: &Agent, obs: &Observation) -> Vec<crate::domain::Thing> {
    agent
        .player()
        .map(|p| p.combat().engaged(obs))
        .unwrap_or_default()
}
