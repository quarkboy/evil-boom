//! Which weapon: what she has to fight with (each slot as the game brings it up, the ammo for
//! it), whether she's armed, what she can do with it all (firepower, outmatched), and the gun
//! she'd fight a monster with, every rule for choosing it in `best_gun`.

use super::style::window;
use super::aiming::SWITCH_WAIT;
use super::{mon, Combat, LOW_HEALTH, SLOTS};
use crate::geom::dist;
use crate::motor::{Observation, Thing, TICRATE};
use crate::tactical::bestiary::{melee_at, punchable, weapon, weapon_in, Attack, Weapon, CHAINSAW};

/// The launcher, its way blocked, kept (waited with) when the next gun would take this many
/// times as long.
const BLOCKED_ROCKET_WORSE: f64 = 3.0;
/// ...and not the launcher this long (tics) after its way was blocked
const BLOCKED_KEEP: i64 = 70;

/// the big guns' ammo this much dearer, kept for a boss about
const BOSS_SAVE: f64 = 10.0;

/// ...unless killing what's in sight would take longer than this (s), or more ammo than she
/// has: then get past it
const OUTMATCHED: f64 = 30.0;

/// of what she shoots that would land (the spread allowed for), this much does (her ammo's
/// worth, roughly)
const AMMO_LANDS: f64 = 0.9;

/// ...outmatched only clearly short of it (fifty rockets for a cyberdemon will about do)
const SHORT_BY: f64 = 0.8;

/// fewer shots than this for the guns held: supplies come first
const SHORT_OF_AMMO: i32 = 15;

/// tics to the first shot of a gun already up
const FIRST_SHOT: f64 = 4.0;

/// seconds a weapon switch costs
const SWITCH_COST: f64 = 0.8;

/// units a second she closes in or backs off by, fighting (strafing as she goes)
const GET_THERE: f64 = 250.0;

/// an arch-vile winding up: its kill time counts this much...
const VILE_KILL_WEIGHT: f64 = 0.35;

/// ...and its fire going off unflinched this many seconds' worth
const VILE_MISSED: f64 = 8.0;

/// its pain chance (10/256 a hit)
const VILE_PAIN: f64 = 10.0 / 256.0;

/// pain rolls a second worth twice a gun's kill time, against an arch-vile
const FLINCH_ROLLS: f64 = 6.0;

/// What using up ammo costs, in seconds of fighting a unit, and below how much the cost goes up.
fn ammo_cost(slot: i32) -> (f64, f64) {
    match slot {
        2 | 4 => (0.01, 20.0),
        3 => (0.05, 8.0),
        5 => (0.3, 4.0),
        6 => (0.015, 40.0),
        _ => (0.015, 80.0),
    }
}

impl Combat {
    pub(super) fn held(obs: &Observation) -> Vec<i32> {
        obs.weapons.clone().unwrap_or_else(|| vec![1, 2])
    }

    pub(super) fn ammo(obs: &Observation, slot: i32) -> i32 {
        obs.ammo.iter().flatten().find(|(s, _)| *s == slot).map_or(0, |(_, n)| *n)
    }

    /// Damage a second she can do with the best gun she has ammo for (the fist's, else: as a
    /// plain fist, berserk or not, the chainsaw or not; the berserk fist counts against what it'd
    /// punch, `too_much`).
    pub fn firepower(&self, obs: &Observation) -> f64 {
        let held = Self::held(obs);
        SLOTS
            .iter()
            .filter(|s| held.contains(s))
            .map(|&s| if s == 1 { weapon(1) } else { weapon_in(s, obs) })
            .filter(|w| w.ammo_per_shot == 0 || Self::ammo(obs, w.slot) >= w.ammo_per_shot)
            .map(|w| w.dps())
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Would killing what's in sight take too long with what she has (a room of barons with the
    /// pistol: MAP08 at pistol start), or more ammo than she has (a cyberdemon, ten rockets)?
    pub fn outmatched(&self, obs: &Observation) -> bool {
        let near: Vec<&'static str> = self.visible.iter().filter(|t| dist((t.x, t.y), obs.pos()) < 1200.0).map(|t| t.name).collect();
        self.too_much(obs, &near, OUTMATCHED)
    }

    /// Would these take her longer than `secs` to kill, or more than her ammo's worth?
    pub fn too_much(&self, obs: &Observation, names: &[&str], secs: f64) -> bool {
        let hp: f64 = names.iter().map(|n| f64::from(mon(n).health)).sum();
        // (berserk, against nothing but what she'd punch: the fist, ammo or none)
        if Self::fists_for(obs, names) {
            return hp / self.firepower(obs).max(weapon_in(1, obs).dps()) > secs;
        }
        if hp / self.firepower(obs) > secs {
            return true;
        }
        // (what each kind of ammo's worth, with the best gun she has for it; bosses take no blast)
        let held = Self::held(obs);
        let boss = names.iter().any(|n| mon(n).boss);
        let radius = names.iter().map(|n| mon(n).radius).fold(20.0, f64::max);
        let mut worth = 0.0;
        for pool in [&[2, 4][..], &[3], &[5], &[6, 7]] {
            let best = pool
                .iter()
                .filter(|s| held.contains(s))
                .map(|&s| {
                    let w = weapon_in(s, obs);
                    let per = if boss && s == 5 { w.damage / 2.0 } else { w.damage };
                    f64::from(Self::ammo(obs, s) / w.ammo_per_shot.max(1)) * per * w.hits(radius, 384.0)
                })
                .fold(0.0, f64::max);
            worth += best;
        }
        let melee = held.contains(&1) && self.firepower(obs) == weapon(1).dps();
        !melee && worth * AMMO_LANDS < SHORT_BY * hp
    }

    /// Can any gun she has ammo for get to t from here?
    pub fn reachable(&self, obs: &Observation, t: &Thing) -> bool {
        let held = Self::held(obs);
        let d = dist((t.x, t.y), obs.pos());
        SLOTS.iter().any(|&s| {
            let w = weapon_in(s, obs);
            held.contains(&s) && Self::ammo(obs, s) >= w.ammo_per_shot && Self::in_reach(&w, t, d)
        })
    }

    /// Has she a gun with ammo for it (more than the fist), or her fists berserk with nothing
    /// after her she wouldn't punch?
    pub fn armed(&self, obs: &Observation) -> bool {
        self.guns_armed(obs) || Self::fists_for(obs, &self.against_her().iter().map(|t| t.name).collect::<Vec<_>>())
    }

    /// Has she a gun with ammo for it (more than slot 1's)?
    pub(super) fn guns_armed(&self, obs: &Observation) -> bool {
        let held = Self::held(obs);
        SLOTS.iter().any(|&s| s != 1 && held.contains(&s) && Self::ammo(obs, s) >= weapon_in(s, obs).ammo_per_shot)
    }

    /// Berserk, and these all things she'd punch?
    fn fists_for(obs: &Observation, names: &[&str]) -> bool {
        obs.berserk == Some(true) && Self::held(obs).contains(&1) && names.iter().all(|n| punchable(n))
    }

    /// Next to nothing left to shoot with (the fist doesn't count).
    pub fn short_of_ammo(&self, obs: &Observation) -> bool {
        self.shots(obs) < SHORT_OF_AMMO
    }

    fn shots(&self, obs: &Observation) -> i32 {
        let held = Self::held(obs);
        let shots: i32 = SLOTS
            .iter()
            .filter(|&&s| held.contains(&s) && s != 4 && s != 7)
            .map(|&s| (s, crate::tactical::bestiary::weapon(s)))
            .filter(|(_, w)| w.ammo_per_shot > 0)
            .map(|(s, w)| Self::ammo(obs, s) / w.ammo_per_shot)
            .sum();
        shots
    }

    /// The gun that'd see t off soonest from here, counting the switch and what its ammo is worth.
    pub(super) fn weapon_for(&self, obs: &Observation, t: &Thing, d: f64) -> i32 {
        self.best_gun(obs, t, |_| d)
    }

    /// The gun to have up with no fight on (and nothing else asking for one: a barrel tapped
    /// with the pistol, then the pistol kept, the next fight begun with it, MAP02): the super
    /// shotgun or the shotgun, the chaingun, plasma, whichever first has a few shots in it; the
    /// pistol only for want of any. Not the rocket launcher or the BFG (round a corner into
    /// something close), nor the fist. None: what she has up will do.
    pub fn ready_gun(&self, obs: &Observation) -> Option<i32> {
        let held = Self::held(obs);
        let want = [(3, 4), (4, 20), (6, 40), (2, 1)].into_iter().find(|&(slot, shots)| {
            let w = weapon_in(slot, obs);
            held.contains(&slot) && Self::ammo(obs, slot) >= w.ammo_per_shot * shots
        })?;
        (obs.weapon != Some(want.0) && obs.tic - self.switch_tic > SWITCH_WAIT).then_some(want.0)
    }

    /// The gun she'd rather fight t with, wherever she stood: each gun judged from the nearest
    /// place in its window (and the way there): the super shotgun for hell knights, closing in,
    /// not the pistol from where she happens to be (all 50 shells kept to the end, dead).
    pub(super) fn preferred_gun(&self, obs: &Observation, t: &Thing) -> Weapon {
        let d = dist((t.x, t.y), obs.pos());
        let slot = self.best_gun(obs, t, |w| {
            let (near, far) = window(w, t);
            d.clamp(near, far)
        });
        weapon_in(slot, obs)
    }

    /// The gun that'd see t off soonest, each judged at the distance `at` gives for it (and the
    /// time to get there).
    fn best_gun(&self, obs: &Observation, t: &Thing, at: impl Fn(&Weapon) -> f64) -> i32 {
        let here = dist((t.x, t.y), obs.pos());
        let held = Self::held(obs);
        let info = mon(t.name);
        let crowd = self.visible.len();
        // (a boss seen and still about, t not it: the big guns' ammo kept for it, as a player
        // would; E3M8, 138 cells on two cacodemons, the spider mastermind left to 22 rockets)
        let saving = !info.boss && self.remaining().iter().any(|m| m.id != t.id && mon(m.name).boss);
        let (mut best, mut cost) = (obs.weapon.unwrap_or(2), f64::INFINITY);
        let mut blocked = None;
        for &slot in &SLOTS {
            if !held.contains(&slot) {
                continue;
            }
            let w = weapon_in(slot, obs);
            let have = Self::ammo(obs, slot);
            if (w.ammo_per_shot > 0 && have < w.ammo_per_shot) || (slot == 5 && !self.rocket_safe(obs, t)) {
                continue;
            }
            let d = at(&w);
            if slot == 1 && !Self::in_reach(&w, t, d) {
                continue; // (not the fist for being free when nothing reaches: as Astra's branch has it)
            }
            if slot == 1 && ((w == CHAINSAW && !melee_at(&w, t.name)) || (obs.health.unwrap_or(100.0) < LOW_HEALTH && w != weapon(1))) && self.guns_armed(obs) {
                // (not the saw at what it won't keep flinching, with a gun to hand: a cacodemon's
                // bites, a baron's claws come through it; nor the saw or the berserk fist at
                // anything, hurt: E3M4, at 14 health, up to a demon with the saw, one bite)
                continue;
            }
            let mut c = self.time_to_kill(&w, t, d) + (d - here).abs() / GET_THERE;
            if w.ammo_per_shot > 0 {
                let shots = (f64::from(info.health) / (w.damage_on(&info) * w.hits(info.radius, d)).max(1.0)).max(1.0);
                let (per, low) = ammo_cost(slot);
                let kept = if saving && slot >= 5 { BOSS_SAVE } else { 1.0 };
                c += shots * f64::from(w.ammo_per_shot) * per * kept * (low / f64::from(have)).max(1.0);
            }
            if info.attack == Attack::Fire && t.attack >= 0 {
                // (winding up its fire: what counts is the chance of a flinch before it goes off,
                // the shots that land in time, after the switch, the reload, the launch, the
                // flight; Astra's branch's reckoning)
                let chance = self.interrupt_chance(obs, &w, t, d, f64::from(super::steering::VILE_BLAST - t.attack));
                c = VILE_KILL_WEIGHT * c + VILE_MISSED * (1.0 - chance);
            } else if info.attack == Attack::Fire {
                // (an arch-vile flinches out of its fire one hit in 25: the super shotgun's 20
                // pellets, plasma's stream, the chaingun's, stun it best)
                c /= 1.0 + w.flinch_rolls(info.radius, d) / FLINCH_ROLLS;
            }
            if slot == 7 && (info.boss || crowd >= 4) {
                c *= 0.3;
            }
            if Some(slot) != obs.weapon {
                c += SWITCH_COST;
            }
            if slot == 5 && self.rocket_blocked.is_some_and(|(id, tic)| id == t.id && obs.tic - tic < BLOCKED_KEEP) {
                blocked = Some(c);
                continue;
            }
            if c < cost {
                best = slot;
                cost = c;
            }
        }
        // (its way blocked close by: another gun rather than a rocket held, the user's, but one
        // that'll do; not the pistol at a Cyberdemon, out of cells behind cover: switched to it
        // each time the cover was in the rocket's way, 3/8 drawn at full health)
        match blocked {
            Some(r) if cost > BLOCKED_ROCKET_WORSE * r => 5,
            _ => best,
        }
    }

    /// The chance a shot at t with w makes it flinch within `window` tics: the shots that land
    /// in time (a switch first, what's left of the reload, the launch, the flight), each hit
    /// a roll of its pain chance.
    fn interrupt_chance(&self, obs: &Observation, w: &Weapon, t: &Thing, d: f64, window: f64) -> f64 {
        if w.ammo_per_shot == 0 {
            return 0.0;
        }
        let ready = if Some(w.slot) != obs.weapon {
            SWITCH_WAIT as f64
        } else {
            self.last_shot.filter(|&(s, _)| s == w.slot).map_or(0.0, |(_, tic)| (tic as f64 + w.cycle - obs.tic as f64).max(0.0))
        };
        let launch = match w.slot {
            3 => 3.0,
            5 => 8.0,
            7 => 46.0,
            _ => 0.0,
        };
        let flight = if w.hitscan { 0.0 } else { d / w.speed.max(1.0) };
        let left = window - ready - launch - flight;
        if left < 0.0 {
            return 0.0;
        }
        let shots = ((left / w.cycle).floor() + 1.0).min(f64::from(Self::ammo(obs, w.slot) / w.ammo_per_shot));
        let rolls = w.pellets * w.hits(mon(t.name).radius, d);
        1.0 - (1.0 - VILE_PAIN).powf(shots * rolls)
    }

    /// Seconds of shooting it'd take (a lot, out of the gun's reach: anything else first).
    pub(super) fn time_to_kill(&self, w: &Weapon, t: &Thing, d: f64) -> f64 {
        let info = mon(t.name);
        if !Self::in_reach(w, t, d) {
            return 1e4 + d;
        }
        let per_shot = w.damage_on(&info) * w.hits(info.radius, d);
        // (the first shot goes at once, the rest a reload apart: a zombieman 65 off, one shotgun
        // blast, was counted a whole reload and lost to two pistol shots, MAP02)
        let shots = (f64::from(info.health) / per_shot.max(0.1)).max(1.0);
        ((shots - 1.0) * w.cycle + FIRST_SHOT) / TICRATE
    }
}
