//! Every number in the tactical layer that's a judgement (how near is too near, how long before
//! giving up, what a thing's worth) rather than a fact about the game, in one place: a `Tuning`,
//! owned by the player and read through `tune` wherever a rule needs one. The defaults are the
//! values the rules were written with. A file (`--tuning FILE`, JSON, any part of it) changes
//! them for a run; a decision model can swap a whole `Tuning` in as she plays
//! (`Player::set_tuning`). Facts about the engine (a rocket's speed, a blast's reach) stay
//! `const`s where they're used.
//!
//! Distances are map units, times tics (35 a second) unless a field says seconds.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tuning {
    pub senses: Senses,
    pub engage: Engage,
    pub dodge: Dodge,
    pub barrels: Barrels,
    pub steer: Steer,
    pub hide: Hide,
    pub aim: Aim,
    pub guns: Guns,
    pub errands: Errands,
    pub retreat: Retreat,
    pub deaths: Deaths,
}

impl Tuning {
    /// The defaults with a file's values over them (JSON: any groups, any fields of them).
    pub fn load(path: &std::path::Path) -> Result<Tuning, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// A run's: the file's if one's given, else the defaults.
    pub fn from(path: Option<&std::path::Path>) -> Result<std::rc::Rc<Tuning>, String> {
        Ok(std::rc::Rc::new(path.map_or_else(|| Ok(Tuning::default()), Tuning::load)?))
    }

    /// These numbers with some others over them (`over`: any groups and fields, as a file has
    /// them): a stance's.
    pub fn with(&self, over: &serde_json::Value) -> Result<Tuning, String> {
        let mut all = serde_json::to_value(self).map_err(|e| e.to_string())?;
        for (group, fields) in over.as_object().into_iter().flatten() {
            let into = all.get_mut(group).and_then(serde_json::Value::as_object_mut).ok_or_else(|| format!("no tuning group called {group}"))?;
            for (field, value) in fields.as_object().into_iter().flatten() {
                if !into.contains_key(field) {
                    return Err(format!("no tuning field called {group}.{field}"));
                }
                into.insert(field.clone(), value.clone());
            }
        }
        serde_json::from_value(all).map_err(|e| e.to_string())
    }

    /// As a file would give it (the defaults: a file to start from).
    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// What she notices, and how long she goes on taking it for so.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Senses {
    /// half the view, degrees: monsters in line of sight inside it are seen...
    pub fov: f64,
    /// a barrel this near is noticed whichever way she faces
    pub near: f64,
    /// a monster this near is seen whichever way she faces: 0, none is (a monster behind her
    /// she can't see; it was 384, and can be set back to measure what that was worth)
    pub see_near: f64,
    /// A recently seen monster can still be noticed outside the usual field of view.
    pub recent_sight_tics: i64,
    /// Movement from the first sighting that counts as waking up (units, units/tic, degrees).
    pub wake_distance: f64,
    pub wake_speed: f64,
    pub wake_turn: f64,
    /// a monster's way of going judged over this many tics (a walker's steps come every 2-4)...
    pub track_tics: i64,
    /// ...and over at least this many
    pub track_min: i64,
    /// a monster seen after another, not her, this long: they're fighting each other: leave them
    pub infight_tics: i32,
    /// health lost lately: over this long (tics)
    pub damage_window: i64,
    /// a ceiling seen coming down this lately, this low over its floor, is a crusher at work
    pub crush_tics: i64,
    /// a crusher come down lower than she stands this lately is at work (tics: a slow one's stroke,
    /// down and up, is some 5 s)
    pub active_crusher: i64,
    pub crush_gap: f64,
    /// a floor seen coming up this lately with this little room left over it, likewise
    pub floor_crush_gap: f64,
    /// invulnerability's last this long isn't counted on
    pub invulnerable_spare: i64,
    /// a monster turned to something within this many degrees is taken to be after it: after
    /// her unless it's turned to another monster in sight and not to her (a monster steps in
    /// eight directions, 45 degrees apart)
    pub facing: f64,
    /// a sound's taken for a monster she knows of out of sight if where she last saw it is
    /// within this of how far off the sound was (by its loudness)...
    pub heard_fit: f64,
    /// ...and on the side it came from, within this much of the balance between her ears (of
    /// 96 either way)
    pub heard_side: f64,
    /// a monster out of sight is firing if its kind's attack was heard this lately (tics: a
    /// chaingun's shots come four apart)
    pub heard_firing: i64,
    /// a death seen is followed by its cry within this (tics): that cry isn't another's
    pub cry_due: i64,
    /// a monster out of her sight is still in her mind, where she last saw it, for this long
    /// (tics): counted in the threat, kept clear of, not something to shoot
    pub memory_tics: i64,
    /// one she knows is about and can't see counts this many times its threat (the user's: one
    /// you know is round you and don't know where is the bigger threat)
    pub unseen_threat: f64,
    /// a shot fired, a cry on seeing her or an attack is looked round for if it's this loud or
    /// louder (1-127: 127 within 160 units, 100 at about 380, 64 at about 680, 1 at 1200)
    pub heed_volume: i32,
    /// ...and a monster's wandering noise, this loud or louder (128: never; there's always one
    /// somewhere, but one this loud is close)
    pub heed_active_volume: i32,
    /// instinct (the user's allowance: "cheating a little, but our goal is good streaming
    /// content"): a monster she was looking at is still known where it is, as it is, for this
    /// long after something comes between them (tics; 0: not at all)
    pub instinct_tics: i64,
    /// a monster gone from her sight is taken to have gone on the way it was going for this
    /// long (tics; 0: taken to be where she last saw it): where she looks for it and what she
    /// keeps clear of
    pub reckon_tics: i64,
    /// a monster she remembers, out of her sight, is looked round for this often (tics; 0:
    /// never): where she last saw it, the nearest first; there, it's seen again; gone, she knows
    /// it's moved (the user's: knowing isn't enough, she has to look)
    pub check_every: i64,
    /// ...one within this of her
    pub check_within: f64,
    /// walking a route with nothing to shoot, a look to either side this often (tics; 0: never)
    pub glance_every: i64,
    /// ...this far round (degrees)
    pub glance_angle: f64,
    /// a monster she knows of from before this life (a life before, the level known), not seen
    /// yet in this one, counts in her odds when it's this near where she stands
    pub expect_range: f64,
    /// a monster heard walking (the few that make steps: a cyberdemon, a spider mastermind, an
    /// arachnotron) is kept track of by its steps while it's out of her sight
    pub track_steps: bool,
    /// something heard, a hit or a bump is looked round for this long at most (tics)
    pub alert_tics: i64,
    /// turned to within this many degrees of it, she's looked
    pub looked: f64,
}

impl Default for Senses {
    fn default() -> Self {
        Senses {
            fov: 55.0,
            near: 384.0,
            see_near: 0.0,
            recent_sight_tics: 35,
            wake_distance: 16.0,
            wake_speed: 0.5,
            wake_turn: 1.0,
            track_tics: 12,
            track_min: 6,
            infight_tics: 18,
            damage_window: 5 * 35,
            crush_tics: 2 * 35,
            active_crusher: 6 * 35,
            crush_gap: 120.0,
            floor_crush_gap: 72.0,
            invulnerable_spare: 3 * 35,
            facing: 45.0,
            heard_fit: 256.0,
            heard_side: 40.0,
            heard_firing: 12,
            cry_due: 35,
            memory_tics: 10 * 35,
            unseen_threat: 1.5,
            instinct_tics: 17,
            reckon_tics: 0,
            check_every: 0,
            check_within: 512.0,
            glance_every: 0,
            glance_angle: 90.0,
            heed_volume: 1,
            heed_active_volume: 128,
            expect_range: 512.0,
            track_steps: true,
            alert_tics: 70,
            looked: 10.0,
        }
    }
}

/// Whom she fights, from where, and when she leaves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Engage {
    /// closing in on something out of reach this long without getting it in reach: left...
    pub close_give_up: i64,
    /// ...for this long
    pub given_up_tics: i64,
    /// an awake monster out of sight this long is still fought (where it was)
    pub lost_tics: i64,
    /// outmatched by an awake monster this near: keeping clear of it, not getting past
    pub keep_clear: f64,
    /// no shooting at anything further
    pub range: f64,
    /// a clawing monster nearer than this: back off from it
    pub close: f64,
    /// this much threat in sight: stand and fight rather than walk on
    pub stand_threat: f64,
    /// monsters standing about (or by where she's going) she'd take longer than this to kill: left
    /// be
    pub leave_be: f64,
    /// ...or below this health, anything in sight
    pub low_health: f64,
    /// how near a monster with a gun or missiles has to be to count as fighting her (`engaged`)
    pub engaged_ranged: f64,
    /// ...a lost soul, that charges from a distance
    pub engaged_charge: f64,
    /// ...one that only claws or bites
    pub engaged_melee: f64,
    /// punching or sawing: this much inside the weapon's reach, centre to centre, she stops closing
    /// in (it steps about). Not more: up against a demon she's 46 off. (MAP22 HNTR: berserk, stood
    /// 68-77 off a chaingunner, inside the old 64-past-her-front-plus-its-radius, every punch short
    /// of its centre, 16 of its bullets to 10 health)
    pub melee_slack: f64,
    /// closing in to hit it in melee, no nearer by this much (units)...
    pub melee_gain: f64,
    /// ...in this long (tics): it's out of her reach, given up a while
    pub melee_stall: i64,
    /// a monster that only claws or charges, in sight of her this long (tics) with her staying
    /// where she is and it getting no nearer, can't get at her (behind bars, across a gap)...
    pub cant_reach_tics: i64,
    /// ...and counts this much as a target while it can't (others first)
    pub cant_reach_first: f64,
    /// how far past touching a monster still stands in her way
    pub errand_contact: f64,
    /// with something to shoot from where she is, and her route about to take her onto ground
    /// she's never been on: she stands and fights it from here (the user's: fight from the
    /// corridor she knows, not out in the room)
    pub hold_known: bool,
    /// something that shoots back from anywhere: shot at past a gun's reach while this much lands
    pub far_hits: f64,
}

impl Default for Engage {
    fn default() -> Self {
        Engage {
            close_give_up: 20 * 35,
            given_up_tics: 30 * 35,
            lost_tics: 3 * 35,
            keep_clear: 1024.0,
            range: 2048.0,
            close: 160.0,
            stand_threat: 6.0,
            leave_be: 15.0,
            low_health: 35.0,
            engaged_ranged: 1536.0,
            engaged_charge: 768.0,
            engaged_melee: 384.0,
            melee_slack: 12.0,
            melee_gain: 32.0,
            melee_stall: 3 * 35,
            cant_reach_tics: 140,
            cant_reach_first: 0.2,
            errand_contact: 16.0,
            hold_known: false,
            far_hits: 0.2,
        }
    }
}

/// Getting out of a missile's way, and moving about as she shoots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dodge {
    /// a missile arriving sooner than this is dodged...
    pub within_tics: i64,
    /// ...if it would pass nearer than the two radii and this
    pub margin: f64,
    /// a dodge along a missile's way (straight from it or at it) counts this much less than across it
    pub along: f64,
    /// a clear dodge's worth for going the way she's going (an errand), all the way: as much as
    /// room after it (24)
    pub toward: f64,
    /// a strafe one way lasts between these
    pub strafe_tics: (i64, i64),
    /// radians off straight at it she'll veer, closing in, when straight on isn't clear
    pub close_veer: [f64; 2],
}

impl Default for Dodge {
    fn default() -> Self {
        Dodge {
            within_tics: 35,
            margin: 12.0,
            along: 24.0,
            toward: 24.0,
            strafe_tics: (18, 40),
            close_veer: [0.35, 0.7],
        }
    }
}

/// Barrels: kept clear of, shot at monsters, cleared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Barrels {
    /// with something shooting in sight, keep this far from barrels
    pub keep_clear: f64,
    /// a barrel this near a monster (its chain all clear of her) is shot instead
    pub by_monster: f64,
    /// getting clear of barrels: at most this long at a time...
    pub clear_tics: i64,
    /// ...then not again for this long (E1M7: pressed against a wall for ever)
    pub clear_rest: i64,
    /// barrels this near her (as the crow flies) are cleared...
    pub clear_within: f64,
    /// ...from a place she can get to this near one (the follower finds where to shoot from)...
    pub spot: f64,
    /// ...with this much for the pistol, or this many shells for the shotgun
    pub bullets: i32,
    pub shells: i32,
}

impl Default for Barrels {
    fn default() -> Self {
        Barrels {
            keep_clear: 160.0,
            by_monster: 96.0,
            clear_tics: 2 * 35,
            clear_rest: 3 * 35,
            clear_within: 1024.0,
            spot: 256.0,
            bullets: 10,
            shells: 12,
        }
    }
}

/// Which way her legs go in a fight: what's looked at round her and what each thing counts for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Steer {
    /// where she'd be, about, after half a second running a way
    pub look: f64,
    /// monsters' bodies looked for this far along a way
    pub body_look: f64,
    /// room to run looked for this far
    pub room: f64,
    /// walls looked for round where she'd be this far
    pub open: f64,
    /// what each counts for, a way scored: open ground round where she'd be
    pub w_open: f64,
    /// room to run that way
    pub w_room: f64,
    /// nearer a monster than it's kept at (her target: the gun's window)
    pub w_near: f64,
    /// further from her target than the gun's window
    pub w_far: f64,
    /// across the line to her target, when it shoots, not along it
    pub w_across: f64,
    /// monsters spread further round her after it
    pub w_surround: f64,
    /// the way she was already going
    pub w_keep: f64,
    /// in sight of more of them (by threat)
    pub w_exposed: f64,
    /// off a floor that hurts (twice this: out from under a crusher)
    pub w_off_floor: f64,
    /// nearer a barrel than `barrel_keep`
    pub w_barrel: f64,
    /// towards an errand (health, a gun, ammo)
    pub w_toward: f64,
    /// a way that takes her onto ground she's never been on, in a fight (0: not counted): the
    /// user's, from watching: she dodges and backs off into rooms she's never been in and wakes
    /// what's in them; where she's been is safe
    pub w_new_ground: f64,
    /// a way she isn't looking along and hasn't looked along for `unlooked_tics`: she doesn't
    /// know what's there (0: not counted)
    pub w_unlooked: f64,
    pub unlooked_tics: i64,
    /// what she knows is about and would have out of her view from there, facing her target
    /// (by threat; 0: not counted): the user's, keep as many in sight as she can
    pub w_out_of_view: f64,
    /// with anything shooting in sight, kept this far from a barrel's middle (its blast and a margin)
    pub barrel_keep: f64,
    /// tics before a shotgun's ready that she comes back out for the shot
    pub reload_back: f64,
    /// still this long pushing one way: not that way for a while
    pub stuck_tics: i64,
}

impl Default for Steer {
    fn default() -> Self {
        Steer {
            look: 160.0,
            body_look: 48.0,
            room: 384.0,
            open: 192.0,
            w_open: 2.0,
            w_room: 2.0,
            w_near: 1.0,
            w_far: 0.6,
            w_across: 1.2,
            w_surround: 1.5,
            w_keep: 0.8,
            w_exposed: 1.5,
            w_off_floor: 10.0,
            w_barrel: 4.0,
            w_toward: 1.5,
            w_new_ground: 4.0,
            w_unlooked: 0.0,
            unlooked_tics: 70,
            w_out_of_view: 0.0,
            barrel_keep: 192.0,
            reload_back: 12.0,
            stuck_tics: 6,
        }
    }
}

/// Out of sight of what can't be dodged: an arch-vile, a homing missile, a hitscanner's burst.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hide {
    /// a hiding place this deep in an arch-vile's shadow, along the way there
    pub shadow: f64,
    /// a blast shelter (something low between her and the arch-vile) only this near...
    pub shelter_near: f64,
    /// ...and only with this few tics left (and the way there)
    pub shelter_late: f64,
    /// a homing missile dodged this long (tics) is going round her: shaken off round a corner...
    pub shake_after: i64,
    /// ...no further off than this straight off, or this by the mesh
    pub shake_reach: f64,
    pub shake_far: f64,
    /// ducking from a hitscan shooter only in a fight longer than this (s)...
    pub duck_fight: f64,
    /// ...with this many tics to get out of its sight, once it's firing (it fires on for as long as
    /// it sees her: any way out of its sight shortens the burst; 8, E3M8, nowhere in 84, a burst of
    /// 120 tics stood in)...
    pub duck_firing: f64,
    /// ...off to the spot with this many tics to spare before its first shot...
    pub duck_late: f64,
    /// ...held there this long past its first shot (its fire breaks off at the next refire, 9 tics
    /// at most, once it can't see her)
    pub duck_hold: i64,
}

impl Default for Hide {
    fn default() -> Self {
        Hide {
            shadow: 48.0,
            shelter_near: 96.0,
            shelter_late: 16.0,
            shake_after: 3 * 35,
            shake_reach: 384.0,
            shake_far: 1536.0,
            duck_fight: 4.0,
            duck_firing: 24.0,
            duck_late: 3.0,
            duck_hold: 12,
        }
    }
}

/// Her hands: turning, the trigger, which target first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Aim {
    /// degrees a tic, turning to aim
    pub aim_turn: f64,
    pub turn_accel: f64,
    /// her hand, whoever's turning her (the fight's aim, a route, a look): no faster than this
    /// a tic (degrees)...
    pub hand_turn: f64,
    /// ...and its rate changed by no more than this from one tic to the next (0: no limit): no
    /// turn one way and back the next tic (the user's: a jitter no hand makes)
    pub hand_accel: f64,
    /// a biter seen this lately near her rules out rockets
    pub rocket_wary: i64,
    /// a weapon switch asked for: give it this long before asking again
    pub switch_wait: i64,
    /// with a boss in sight, others this far off aren't shot (left to it)
    pub leave_to_boss: f64,
    /// kept clear beyond the rocket's box each side of its way (a miss is better than a near thing)
    pub rocket_margin: f64,
    /// with a hitscanner about, a monster she'd be longer than this many seconds killing isn't put
    /// first (`priority`)
    pub kill_first: f64,
    /// a pain elemental nearer than this isn't shot with rockets (its souls come out in front)
    pub pain_near: f64,
}

impl Default for Aim {
    fn default() -> Self {
        Aim {
            aim_turn: 18.0,
            turn_accel: 5.0,
            hand_turn: 18.0,
            hand_accel: 10.0,
            rocket_wary: 3 * 35,
            switch_wait: 40,
            leave_to_boss: 384.0,
            rocket_margin: 8.0,
            kill_first: 8.0,
            pain_near: 512.0,
        }
    }
}

/// Which gun, and whether what she has is enough.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Guns {
    /// The launcher, its way blocked, kept (waited with) when the next gun would take this many
    /// times as long.
    pub blocked_rocket_worse: f64,
    /// ...and not the launcher this long (tics) after its way was blocked
    pub blocked_keep: i64,
    /// the big guns' ammo this much dearer, kept for a boss about
    pub boss_save: f64,
    /// killing what's in sight would take longer than this (s), or more ammo than she has:
    /// outmatched (got past, kept clear of)
    pub outmatched: f64,
    /// of what she shoots that would land (the spread allowed for), this much does (her ammo's
    /// worth, roughly)
    pub ammo_lands: f64,
    /// ...outmatched only clearly short of it (fifty rockets for a cyberdemon will about do)
    pub short_by: f64,
    /// fewer shots than this for the guns held: supplies come first
    pub short_of_ammo: i32,
    /// seconds a weapon switch costs
    pub switch_cost: f64,
    /// the gun in hand counts as taking this share of its time (another must be that much better)
    pub held_keep: f64,
    /// an arch-vile winding up: its kill time counts this much...
    pub vile_kill_weight: f64,
    /// ...and its fire going off unflinched this many seconds' worth
    pub vile_missed: f64,
    /// pain rolls a second worth twice a gun's kill time, against an arch-vile
    pub flinch_rolls: f64,
}

impl Default for Guns {
    fn default() -> Self {
        Guns {
            blocked_rocket_worse: 3.0,
            blocked_keep: 70,
            boss_save: 10.0,
            outmatched: 30.0,
            ammo_lands: 0.9,
            short_by: 0.8,
            short_of_ammo: 15,
            switch_cost: 0.8,
            held_keep: 0.85,
            vile_kill_weight: 0.35,
            vile_missed: 8.0,
            flinch_rolls: 6.0,
        }
    }
}

/// What's worth leaving the route for, mid-fight or between fights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Errands {
    /// items further than this (as the crow flies) only when they're needed badly
    pub item_range: f64,
    /// the fight or a retreat having taken her this far from where the route last had her legs,
    /// what she was going for is thought again (0: never): something else may be the nearer now
    pub rethink_moved: f64,
    /// a pickup lying on a floor that hurts costs this much health to fetch, about (a hit or
    /// two going in and out): weighed against what it's worth, like the fire she'd take
    pub floor_cost: f64,
    /// a pickup needed this much (0-1) is taken with the doors a key opens, the nearer first,
    /// not left till after them
    pub needed: f64,
    /// a health pickup the route's going for this near is gone on to mid-fight
    pub heal_near: f64,
    /// a monster's last known spot: hunted from the reachable poly nearest it, this near
    pub hunt_near: f64,
    /// below this health, health items come first
    pub hurt: f64,
    /// monsters this near an objective's spot count against going there
    pub danger_near: f64,
    /// the way to something urgent passes nothing she can't take on closer than this (and its radius)
    pub way_clear: f64,
    /// ammo for what she has counts as this much further, outmatched by something she knows of
    pub ammo_outmatched: f64,
    /// a switch counts as this much as far, with a better gun seen that she can't get to
    pub switch_for_gun: f64,
    /// a way somewhere passes no monster nearer than this
    pub past_clear: f64,
    /// firepower (damage a second) no better than this: the pistol's, about
    pub pistol_only: f64,
    /// below this health, losing it fast enough to be dead within this long (s): health first
    pub survive_health: f64,
    pub survive_secs: f64,
    /// monsters this near the way to an item can shoot at her on it
    pub exposed_near: f64,
    /// of a monster's threat (damage a second, roughly), this much lands
    pub hits: f64,
    /// below this, whatever an item's worth, the way to it's all right
    pub cost_floor: f64,
    /// seconds a better gun is worth going for anyway (the fights to come)
    pub gun_worth: f64,
}

impl Default for Errands {
    fn default() -> Self {
        Errands {
            item_range: 1536.0,
            rethink_moved: 256.0,
            floor_cost: 20.0,
            needed: 0.8,
            heal_near: 512.0,
            hunt_near: 512.0,
            hurt: 50.0,
            danger_near: 640.0,
            way_clear: 320.0,
            ammo_outmatched: 2.0,
            switch_for_gun: 0.25,
            past_clear: 96.0,
            pistol_only: 25.0,
            survive_health: 80.0,
            survive_secs: 8.0,
            exposed_near: 768.0,
            hits: 0.5,
            cost_floor: 10.0,
            gun_worth: 10.0,
        }
    }
}

/// Backing off: cover, posts, getting out when surrounded or ambushed, away with no ammo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retreat {
    /// below this, with something shooting at her: back off out of its sight
    pub health: f64,
    /// cover looked for among the places she's been this near...
    pub cover_range: f64,
    /// a retreat is over once nothing that shoots has been in sight this long
    pub clear_tics: i64,
    /// a step this far ahead, into a firing sniper's sight from cover, held back
    pub edge_look: f64,
    /// a way across ground in sight of a hitscan shooter she can't take on counts this many times
    /// its length...
    pub exposed: f64,
    /// ...that ground judged this far from it at most...
    pub exposure_range: f64,
    /// ...and again once it's moved this far, or this long on
    pub exposure_moved: f64,
    pub exposure_again: i64,
    /// a post (cover to fight from) looked for among the places she knows of this near...
    pub post_range: f64,
    /// ...each counting for this much nearer for every unit it's further from them
    pub post_far: f64,
    /// ...with somewhere they'd see her this near it (to step out to and shoot from)
    pub post_peek: f64,
    /// on the way to one this long at most
    pub post_tics: i64,
    /// after a retreat that found no cover, not again for this long
    pub again: i64,
    /// a retreat's way, for a dodge to take: its next point at least this far on
    pub ahead: f64,
    /// monsters this near, this many of them, spread round her this far (degrees): get out
    pub surrounded_near: f64,
    pub surrounded_count: usize,
    pub surrounded_spread: f64,
    /// getting out: at most this long (then the fight goes on from there)
    pub out_tics: i64,
    /// out of ammo: off for ammo this long at most, before thinking again
    pub ammo_tics: i64,
    /// out of ammo and none known: away to somewhere she's been at most this far
    pub ammo_away: f64,
    /// a way away is no nearer than this to any monster than she is now
    pub ammo_margin: f64,
    /// a heavy taking longer than this (s) to kill with what she has: let others take its shots...
    pub bait_heavy: f64,
    /// ...from a place she's been this near...
    pub bait_range: f64,
    /// ...and not again for this long
    pub bait_again: i64,
    /// an arch-vile she'd take longer than this (s) to kill: into cover
    pub vile_kill: f64,
    /// somewhere better looked for among the places she's been this near
    pub spot_range: f64,
    /// an ambush: losing health faster than this (a second), or this many close...
    pub ambush_rate: f64,
    pub ambush_count: usize,
    pub ambush_near: f64,
    /// ...where it's tight (the mean way out round her this short a share of this)...
    pub tight: f64,
    pub open_look: f64,
    /// ...or they're spread round her this far (degrees)
    pub sides: f64,
    /// monsters woken on ground she's never been on: back this far the way she came, onto
    /// ground she knows, and let them come to her (0: not at all). The user's: standing at
    /// the edge still leaves her sides open
    pub fall_back: f64,
    /// ...and waited there for them this long, none in sight (tics)
    pub fall_back_wait: i64,
    /// this many monsters first seen within `trap_tics` of each other: a trap sprung, or a room
    /// walked in on: back out the way she came (0: never)
    pub trap_count: usize,
    pub trap_tics: i64,
    /// backing out: to somewhere on the way she came at least this far back...
    pub back_min: f64,
    /// ...that none of them see, or this far from them all; at most this long
    pub back_clear: f64,
    pub back_tics: i64,
}

impl Default for Retreat {
    fn default() -> Self {
        Retreat {
            health: 40.0,
            cover_range: 768.0,
            clear_tics: 35,
            edge_look: 48.0,
            exposed: 3.0,
            exposure_range: 2048.0,
            exposure_moved: 64.0,
            exposure_again: 35,
            post_range: 1536.0,
            post_far: 0.5,
            post_peek: 128.0,
            post_tics: 6 * 35,
            again: 3 * 35,
            ahead: 32.0,
            surrounded_near: 640.0,
            surrounded_count: 3,
            surrounded_spread: 160.0,
            out_tics: 4 * 35,
            ammo_tics: 20 * 35,
            ammo_away: 1536.0,
            ammo_margin: 16.0,
            bait_heavy: 8.0,
            bait_range: 768.0,
            bait_again: 8 * 35,
            vile_kill: 2.0,
            spot_range: 1024.0,
            ambush_rate: 6.0,
            ambush_count: 3,
            ambush_near: 512.0,
            tight: 0.45,
            open_look: 256.0,
            sides: 120.0,
            fall_back: 0.0,
            fall_back_wait: 105,
            trap_count: 0,
            trap_tics: 35,
            back_min: 128.0,
            back_clear: 768.0,
            back_tics: 6 * 35,
        }
    }
}

/// What a death teaches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Deaths {
    /// a hit this big with nothing close: a blast
    pub blast_hit: f64,
    /// died going for something this many times: not again
    pub deaths_give_up: usize,
    /// a goal this near where she's died this many times (any reason but want of ammo) waits; this
    /// many, even when urgent
    pub death_zone: f64,
    pub death_zone_count: usize,
    pub death_zone_urgent: usize,
    /// a goal this near where she died going for the same kind of thing is the same
    pub same_goal: f64,
}

impl Default for Deaths {
    fn default() -> Self {
        Deaths {
            blast_hit: 40.0,
            deaths_give_up: 2,
            death_zone: 256.0,
            death_zone_count: 2,
            death_zone_urgent: 3,
            same_goal: 96.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Tuning;

    #[test]
    fn a_file_changes_only_what_it_names() {
        let t: Tuning = serde_json::from_str(r#"{"engage": {"leave_be": 20.0}, "dodge": {"strafe_tics": [10, 30]}}"#).unwrap();
        let d = Tuning::default();
        assert_eq!(t.engage.leave_be, 20.0);
        assert_eq!(t.dodge.strafe_tics, (10, 30));
        assert_eq!(t.engage.low_health, d.engage.low_health);
        assert_eq!(t.steer, d.steer);
    }

    #[test]
    fn the_defaults_come_back_from_their_own_file() {
        let d = Tuning::default();
        assert_eq!(serde_json::from_str::<Tuning>(&d.json()).unwrap(), d);
    }

    #[test]
    fn numbers_put_over_others() {
        let d = Tuning::default();
        let t = d.with(&serde_json::json!({"retreat": {"health": 55.0}})).unwrap();
        assert_eq!(t.retreat.health, 55.0);
        assert_eq!(t.engage, d.engage);
        assert!(d.with(&serde_json::json!({"retreat": {"helth": 55.0}})).is_err());
    }

    #[test]
    fn a_misspelt_field_is_an_error() {
        assert!(serde_json::from_str::<Tuning>(r#"{"engage": {"leave_bee": 20.0}}"#).is_err());
    }
}
