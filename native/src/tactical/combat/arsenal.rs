//! Which weapon: what she has to fight with (each slot as the game brings it up, the ammo for
//! it), whether she's armed, what she can do with it all (firepower, outmatched), and the gun
//! she'd fight a monster with, every rule for choosing it in `best_gun`.

use super::steering::CLAW_REACH;
use super::style::window;
use super::{mon, Combat, SLOTS};
use crate::geom::dist;
use crate::motor::{Observation, Thing, TICRATE};
use crate::tactical::bestiary::{
    claw_max, melee_at, punchable, weapon, weapon_in, Attack, Monster, Weapon, CHAINSAW,
};

/// tics to the first shot of a gun already up
const FIRST_SHOT: f64 = 4.0;

/// the BFG's spray reaches this far from her
const BFG_SPRAY: f64 = 1024.0;

/// units a second she closes in or backs off by, fighting (strafing as she goes)
const GET_THERE: f64 = 250.0;

/// its pain chance (10/256 a hit)
const VILE_PAIN: f64 = 10.0 / 256.0;

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

/// Her odds against some monsters, with what she has: the numbers every "is this too much for
/// her" comes down to. What's too much is the asker's line (`too_much`): 30 s of killing is
/// outmatched in a fight; 15 s is enough to leave something standing about alone.
#[derive(Clone, Copy, Debug)]
pub struct Odds {
    /// nothing can hurt her (invulnerable): whatever it is, now's the time
    pub unhurt: bool,
    /// seconds to kill them all, with her best gun that has ammo
    pub kill_secs: f64,
    /// seconds she'd last toe to toe, as she is: her health and her armor against what they'd
    /// do to her a second (a lot: they'd do nothing). What she can take is weighed, not only her
    /// guns.
    pub last_secs: f64,
    /// berserk, and all of them things she'd punch: the fist will do, ammo or none
    pub fists: bool,
    /// all her ammo wouldn't kill them
    pub ammo_short: bool,
}

impl Odds {
    /// Longer than `secs` to kill, or longer than she'd last at it, or more than her ammo's
    /// worth?
    pub fn too_much(&self, secs: f64) -> bool {
        if self.unhurt {
            return false;
        }
        self.kill_secs > secs
            || (!self.fists && (self.kill_secs > self.last_secs || self.ammo_short))
    }
}

impl Combat {
    pub(super) fn held(obs: &Observation) -> Vec<i32> {
        obs.weapons.clone().unwrap_or_else(|| vec![1, 2])
    }

    pub(super) fn ammo(obs: &Observation, slot: i32) -> i32 {
        obs.ammo
            .iter()
            .flatten()
            .find(|(s, _)| *s == slot)
            .map_or(0, |(_, n)| *n)
    }

    /// Damage a second she can do with the best gun she has ammo for (the fist's, else: as a
    /// plain fist, berserk or not, the chainsaw or not; the berserk fist counts against what it'd
    /// punch, `too_much`).
    pub fn firepower(&self, obs: &Observation) -> f64 {
        self.firepower_on(obs, None)
    }

    /// ...on m, if given (a boss takes no rocket's blast).
    pub fn firepower_on(&self, obs: &Observation, m: Option<&Monster>) -> f64 {
        let held = Self::held(obs);
        SLOTS
            .iter()
            .filter(|s| held.contains(s))
            .map(|&s| if s == 1 { weapon(1) } else { weapon_in(s, obs) })
            .filter(|w| w.ammo_per_shot == 0 || Self::ammo(obs, w.slot) >= w.ammo_per_shot)
            .map(|w| Self::dps_on(&w, m))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// A gun's damage a second on m, if given, else on anything.
    pub fn dps_on(w: &Weapon, m: Option<&Monster>) -> f64 {
        m.map_or(w.dps(), |m| w.dps() * w.damage_on(m) / w.damage.max(1.0))
    }

    /// Her odds against what's in sight: what's near, and a hitscan shooter however far, since
    /// its bullets get there. This is `now.odds`, and `now.outmatched` goes by it.
    pub(super) fn odds_in_sight(&self, obs: &Observation) -> Odds {
        let remembered = self.remembered(obs);
        let mut near: Vec<&'static str> = self
            .visible
            .iter()
            .chain(&remembered)
            .filter(|t| dist((t.x, t.y), obs.pos()) < 1200.0 || self.sniper(obs, t.name))
            .map(|t| t.name)
            .collect();
        // And what she knows from before is standing close by, not yet seen this time: what she's
        // about to walk in on.
        near.extend(
            self.expected(obs.pos(), self.tune.senses.expect_range)
                .iter()
                .map(|s| s.name),
        );
        self.odds(obs, &near)
    }

    /// A hitscan shooter she couldn't see off in a few seconds with what she has: dangerous
    /// however far it is, and a long fight. Not every hitscanner: a zombieman or a sergeant
    /// across the map is no reason to leave objectives in its sight undone.
    pub fn sniper(&self, obs: &Observation, name: &str) -> bool {
        let m = mon(name);
        m.attack == Attack::Hitscan
            && f64::from(m.health) / self.firepower(obs).max(1.0) > self.tune.hide.duck_fight
    }

    /// A hitscan shooter she'd see off in a few seconds with what she has (not a sniper). It is
    /// fought, outmatched or not, never kept clear of or walked on past: its bullets land at any
    /// range, and it's dead sooner than she'd be out of its sight.
    pub fn quick_hitscan(&self, obs: &Observation, name: &str) -> bool {
        mon(name).attack == Attack::Hitscan && !self.sniper(obs, name)
    }

    /// A sniper she'd see off before it saw her off, with the gun, health and armor she has (her
    /// ammo isn't counted: what she needs she picks up as she goes). Then it's fought, from
    /// cover, whatever else: kept clear of or walked past in its sight, its bullets get there
    /// anyway.
    pub fn takes_on(&self, obs: &Observation, name: &str) -> bool {
        let m = mon(name);
        self.sniper(obs, name)
            && f64::from(m.health) / self.firepower(obs).max(1.0)
                <= Self::lasts(obs) / m.threat.max(1.0)
    }

    /// Her odds against these, with what she has (`obs`: as she is, or as she'd be with
    /// something picked up).
    pub fn odds(&self, obs: &Observation, names: &[&str]) -> Odds {
        let hp: f64 = names.iter().map(|n| f64::from(mon(n).health)).sum();
        // Berserk, against nothing but what she'd punch: the fist, ammo or none.
        let fists = Self::fists_for(obs, names);
        let firepower = if fists {
            self.firepower(obs).max(weapon_in(1, obs).dps())
        } else {
            self.firepower(obs)
        };
        let rate: f64 = names.iter().map(|n| mon(n).threat).sum();
        Odds {
            unhurt: self.invulnerable(obs.tic),
            kill_secs: hp / firepower,
            last_secs: if rate > 0.0 {
                Self::lasts(obs) / rate
            } else {
                f64::INFINITY
            },
            fists,
            ammo_short: !fists && self.ammo_short_for(obs, names),
        }
    }

    /// Would these take her longer than `secs` to kill, longer than she'd last, or more than her
    /// ammo's worth?
    pub fn too_much(&self, obs: &Observation, names: &[&str], secs: f64) -> bool {
        self.odds(obs, names).too_much(secs)
    }

    /// Would all her ammo, with the best gun she has for each kind, fall short of killing these
    /// (with the fist her best, never: punching costs nothing)?
    pub fn ammo_short_for(&self, obs: &Observation, names: &[&str]) -> bool {
        let hp: f64 = names.iter().map(|n| f64::from(mon(n).health)).sum();
        self.ammo_short_of(obs, names, hp, f64::INFINITY)
    }

    /// ...of killing every one of these, where a shot is worth no more than the most any one of
    /// them takes: what's past its health is spent on its body, not the next one's. Two blasts
    /// of the super shotgun kill two imps, not the three their damage adds up to.
    pub fn ammo_short_for_each(&self, obs: &Observation, names: &[&str]) -> bool {
        let hp: f64 = names.iter().map(|n| f64::from(mon(n).health)).sum();
        let most = names
            .iter()
            .map(|n| f64::from(mon(n).health))
            .fold(0.0, f64::max);
        self.ammo_short_of(obs, names, hp, most)
    }

    /// ...of killing this one, with what she has put into it already taken off (as she reckons
    /// it: so many rounds at it, landing as they do at their range). One that's a few rounds
    /// short of dead is worth the rest.
    pub fn ammo_short_against(&self, obs: &Observation, t: &Thing) -> bool {
        self.ammo_short_of_killing(obs, t.id, t.name)
    }

    /// The same, by the monster's id and kind (one known of, not in sight).
    pub fn ammo_short_of_killing(&self, obs: &Observation, id: i32, name: &str) -> bool {
        // Or what she's seen it lose, to whatever: when that's more than she has put in, something
        // else has been at it.
        let lost = self
            .put_in
            .get(&id)
            .copied()
            .unwrap_or(0.0)
            .max(self.seen_hurt.get(&id).copied().map_or(0.0, f64::from));
        let hp = f64::from(mon(name).health) - lost;
        self.ammo_short_of(obs, &[name], hp.max(0.0), f64::INFINITY)
    }

    fn ammo_short_of(&self, obs: &Observation, names: &[&str], hp: f64, most: f64) -> bool {
        // What each kind of ammo is worth with the best gun she has for it. Bosses take no blast.
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
                    let per = if boss && s == 5 {
                        w.damage / 2.0
                    } else {
                        w.damage
                    };
                    f64::from(Self::ammo(obs, s) / w.ammo_per_shot.max(1))
                        * (per * w.hits(radius, 384.0)).min(most)
                })
                .fold(0.0, f64::max);
            worth += best;
        }
        let melee = held.contains(&1) && self.firepower(obs) == weapon(1).dps();
        !melee && worth * self.tune.guns.ammo_lands < self.tune.guns.short_by * hp
    }

    /// Her health as it would go under fire: armor takes its share of each hit (a third green,
    /// half blue) till it's gone (P_DamageMobj), so so much more health's worth.
    pub fn lasts(obs: &Observation) -> f64 {
        Self::lasts_with(
            obs.health.unwrap_or(100.0),
            obs.armor.unwrap_or(0.0),
            obs.armor_type.unwrap_or(0),
        )
    }

    /// ...with that health, and that many points of armor of that kind (1 green, 2 blue).
    pub fn lasts_with(health: f64, armor: f64, kind: i32) -> f64 {
        let (health, armor) = (health.max(0.0), armor.max(0.0));
        let share = match kind {
            1 => 1.0 / 3.0,
            2 => 0.5,
            _ => return health,
        };
        // All her health lost with the armor holding: health / (1 - share) taken, the armor's share
        // of it within what she has. Else the armor goes first, then health alone.
        let all = health / (1.0 - share);
        if armor >= all * share {
            all
        } else {
            health + armor
        }
    }

    /// What picking that up would add to how much she can take (`lasts`), by the game's rules
    /// for it; nothing if she can't pick it up, and less than nothing where it would lose her
    /// some. Armor is replaced whole, kind and all, whenever she has fewer points than the
    /// pickup gives (P_GiveArmor): a green armor over 60 points of blue is 100 of green, a third
    /// of each hit where it was half, and she lasts less for it; over fewer blue points than
    /// half her health it's the better. A megaarmor on full health doubles what she can take.
    pub fn pickup_gain(obs: &Observation, name: &str) -> f64 {
        let (health, armor, kind) = (
            obs.health.unwrap_or(100.0),
            obs.armor.unwrap_or(0.0),
            obs.armor_type.unwrap_or(0),
        );
        let after = match name {
            "GreenArmor" if armor < 100.0 => (health, 100.0, 1),
            "BlueArmor" if armor < 200.0 => (health, 200.0, 2),
            "Megasphere" => (200.0, armor.max(200.0), 2),
            "ArmorBonus" if armor < 200.0 => (health, armor + 1.0, kind.max(1)),
            "Soulsphere" if health < 200.0 => ((health + 100.0).min(200.0), armor, kind),
            "HealthBonus" if health < 200.0 => (health + 1.0, armor, kind),
            "Stimpack" if health < 100.0 => ((health + 10.0).min(100.0), armor, kind),
            "Medikit" if health < 100.0 => ((health + 25.0).min(100.0), armor, kind),
            "Berserk" if health < 100.0 => (100.0, armor, kind),
            _ => return 0.0,
        };
        Self::lasts_with(after.0, after.1, after.2) - Self::lasts_with(health, armor, kind)
    }

    /// Can any gun she has ammo for get to t from here?
    pub fn reachable(&self, obs: &Observation, t: &Thing) -> bool {
        let held = Self::held(obs);
        let d = dist((t.x, t.y), obs.pos());
        // By the guns she'd bring up for it. With another gun to hand the pistol is never brought
        // up for its reach (see `best_gun`), so its reach doesn't count here either: otherwise a
        // target only the pistol reaches is "in reach" and is fought with a gun that isn't.
        let other = self.other_gun(obs, t);
        SLOTS.iter().any(|&s| {
            let w = weapon_in(s, obs);
            held.contains(&s)
                && !(s == 2 && other)
                && Self::ammo(obs, s) >= w.ammo_per_shot
                && self.in_reach(&w, t, d)
        })
    }

    /// Has she a gun other than the pistol with a shot in it for t (a rocket only where it's
    /// safe)? Then the pistol stays down.
    fn other_gun(&self, obs: &Observation, t: &Thing) -> bool {
        let held = Self::held(obs);
        held.iter().any(|&s| {
            matches!(s, 3 | 4 | 6 | 7)
                && Self::ammo(obs, s) >= weapon_in(s, obs).ammo_per_shot.max(1)
        }) || (held.contains(&5) && Self::ammo(obs, 5) >= 1 && self.rocket_safe(obs, t))
    }

    /// Has she a gun with ammo for it (more than the fist), or her fists berserk with nothing
    /// after her that she wouldn't punch or can't get at?
    pub fn armed(&self, obs: &Observation) -> bool {
        let them = &self.now.about;
        self.guns_armed(obs)
            || (Self::fists_for(obs, &them.iter().map(|t| t.name).collect::<Vec<_>>())
                && !them.iter().any(|t| self.out_of_fist_reach(obs, t)))
    }

    /// Has she a gun with ammo for it (more than slot 1's)?
    pub(super) fn guns_armed(&self, obs: &Observation) -> bool {
        let held = Self::held(obs);
        SLOTS.iter().any(|&s| {
            s != 1 && held.contains(&s) && Self::ammo(obs, s) >= weapon_in(s, obs).ammo_per_shot
        })
    }

    /// Berserk, and these all things she'd punch?
    fn fists_for(obs: &Observation, names: &[&str]) -> bool {
        obs.berserk == Some(true)
            && Self::held(obs).contains(&1)
            && names.iter().all(|n| punchable(n))
    }

    /// Next to nothing left to shoot with (the fist doesn't count).
    pub(super) fn short_of_ammo(&self, obs: &Observation) -> bool {
        self.shots(obs) < self.tune.guns.short_of_ammo
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

    /// The gun to have up with no fight on and nothing else asking for one: the super shotgun or
    /// the shotgun, the chaingun, plasma, whichever first has a few shots in it; the pistol only
    /// for want of any. Not the rocket launcher or the BFG (round a corner into something
    /// close), nor the fist. `None`: what she has up will do.
    pub fn ready_gun(&self, obs: &Observation) -> Option<i32> {
        // Not with something awake she knows of still about: the next fight is with it, and the gun
        // up is the one she was fighting it with. Otherwise she'd swap guns each time it went out of
        // sight and back, a second without a shot each time.
        if self
            .remaining()
            .iter()
            .any(|s| s.awake && mon(s.name).attack != Attack::None)
        {
            return None;
        }
        let held = Self::held(obs);
        let want = [(3, 4), (4, 20), (6, 40), (2, 1)]
            .into_iter()
            .find(|&(slot, shots)| {
                let w = weapon_in(slot, obs);
                held.contains(&slot) && Self::ammo(obs, slot) >= w.ammo_per_shot * shots
            })?;
        (obs.weapon != Some(want.0) && obs.tic - self.switch_tic > self.tune.aim.switch_wait)
            .then_some(want.0)
    }

    /// The gun she'd rather fight t with, wherever she stood: each gun judged from the nearest
    /// place in its window, and the way there. So a close-range gun is chosen for what's worth
    /// closing in on, not the long-range one that happens to reach from where she is.
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
        // What the BFG's spray would take in: forty lines from her, a quarter turn wide along the
        // shot, 1024 out (A_BFGSpray). Not everything in sight.
        let me = obs.pos();
        let shot = crate::motor::heading(me, (t.x, t.y));
        let crowd = self
            .visible
            .iter()
            .filter(|m| {
                dist((m.x, m.y), me) < BFG_SPRAY
                    && crate::motor::norm(crate::motor::heading(me, (m.x, m.y)) - shot).abs() < 45.0
            })
            .count();
        // A boss seen and still about, and t isn't it: the big guns' ammo is kept for the boss.
        let saving = !info.boss
            && self
                .remaining()
                .iter()
                .any(|m| m.id != t.id && mon(m.name).boss);
        let (mut best, mut cost) = (obs.weapon.unwrap_or(2), f64::INFINITY);
        let mut blocked = None;
        // Others awake shooting at her from a distance, t aside.
        let pelted = self.now.engaged.iter().any(|m| {
            m.id != t.id
                && matches!(
                    mon(m.name).attack,
                    Attack::Hitscan | Attack::Missile | Attack::Fire
                )
        });
        // The pistol only for want of any other gun with a shot in it, never for its reach. (Taps
        // at barrels, switches and walls are the route's own choice of gun, not this.)
        let other_gun = self.other_gun(obs, t);
        for &slot in &SLOTS {
            if !held.contains(&slot) || (slot == 2 && other_gun) {
                continue;
            }
            let w = weapon_in(slot, obs);
            let have = Self::ammo(obs, slot);
            let d = at(&w);
            // A rocket's safety is judged where she'd fire it from. Further out than she is and past
            // its blast, the launcher is the gun to back off to, not one ruled out by how close t is
            // now.
            let safe_there = d > here && d >= Self::rocket_clear() + info.radius;
            if (w.ammo_per_shot > 0 && have < w.ammo_per_shot)
                || (slot == 5 && !safe_there && !self.rocket_safe(obs, t))
            {
                continue;
            }
            if slot == 1 && !self.in_reach(&w, t, d) {
                continue; // not the fist just for being free when nothing else reaches
            }
            if slot == 1 && info.attack == Attack::None && self.given_up_on(obs, t.id) {
                // Nor the fist at something harmless she's given up getting at: free to punch, the fist is
                // chosen for that, but closing in is what failed. Anything that fights back is weighed as
                // it is.
                continue;
            }
            if slot == 1 && self.guns_armed(obs) && self.out_of_fist_reach(obs, t) {
                // Nor at anything that closing in has failed to get her to, with a gun with a shot in it to
                // hand: chosen for its speed up close, the fist would walk her at it again and again.
                continue;
            }
            let health = obs.health.unwrap_or(100.0);
            // Brought up inside a claw's reach, a claw comes before the first blow: the switch, 16 tics
            // down and 16 up (LOWERSPEED, RAISESPEED), is longer than any claw's cycle.
            let claws = if obs.weapon != Some(1) && here < CLAW_REACH {
                2.0
            } else {
                1.0
            };
            if slot == 1
                && ((w == CHAINSAW && !melee_at(&w, t.name))
                    || (health < self.tune.engage.low_health && w != weapon(1))
                    || claws * claw_max(t.name) >= health)
                && self.guns_armed(obs)
            {
                // Not the saw at what it won't keep flinching, with a gun to hand: those claws and bites
                // come through it. Nor the saw or the berserk fist at anything while she's hurt. Nor any
                // fist at what one claw of could kill her.
                continue;
            }
            if slot == 1
                && melee_at(&w, t.name)
                && here > w.reach - self.tune.engage.melee_slack
                && pelted
                && self.guns_armed(obs)
            {
                // Nor going in up close, with a gun to hand, while others shoot at her: standing at one,
                // every other's shots are free. One already in reach is still hit.
                continue;
            }
            let mut c = self.time_to_kill(&w, t, d) + (d - here).abs() / GET_THERE;
            if w.ammo_per_shot > 0 {
                let shots = (f64::from(info.health)
                    / (w.damage_on(&info) * w.hits(info.radius, d)).max(1.0))
                .max(1.0);
                let (per, low) = ammo_cost(slot);
                let kept = if saving && slot >= 5 {
                    self.tune.guns.boss_save
                } else {
                    1.0
                };
                c += shots
                    * f64::from(w.ammo_per_shot)
                    * per
                    * kept
                    * (low / f64::from(have)).max(1.0);
            }
            if info.attack == Attack::Fire && t.attack >= 0 {
                // Winding up its fire: what counts is the chance of a flinch before it goes off, from the
                // shots that land in time, after the switch, the reload, the launch and the flight.
                let chance = self.interrupt_chance(
                    obs,
                    &w,
                    t,
                    d,
                    f64::from(super::steering::VILE_BLAST - t.attack),
                );
                c = self.tune.guns.vile_kill_weight * c
                    + self.tune.guns.vile_missed * (1.0 - chance);
            } else if info.attack == Attack::Fire {
                // An arch-vile flinches out of its fire one hit in 25, so many small hits stun it best: the
                // super shotgun's 20 pellets, plasma's stream, the chaingun's.
                c /= 1.0 + w.flinch_rolls(info.radius, d) / self.tune.guns.flinch_rolls;
            }
            if slot == 7 && (info.boss || crowd >= 4) {
                c *= 0.3;
            }
            if Some(slot) != obs.weapon {
                c += self.tune.guns.switch_cost;
            } else {
                // The gun in hand is kept unless another is clearly better, by a share of the whole fight,
                // not a fixed second: in a long fight near the boundary of two guns' ranges, a fixed margin
                // swaps them over and over, a second without a shot each time.
                c *= self.tune.guns.held_keep;
            }
            if slot == 5
                && self.rocket_blocked.is_some_and(|(id, tic)| {
                    id == t.id && obs.tic - tic < self.tune.guns.blocked_keep
                })
            {
                blocked = Some(c);
                continue;
            }
            if c < cost {
                best = slot;
                cost = c;
            }
        }
        // Its way blocked close by: another gun rather than a rocket held, but one that will do.
        // Not the pistol at a boss.
        match blocked {
            Some(r) if cost > self.tune.guns.blocked_rocket_worse * r => 5,
            _ => best,
        }
    }

    /// The chance a shot at t with w makes it flinch within `window` tics: the shots that land
    /// in time (a switch first, what's left of the reload, the launch, the flight), each hit
    /// a roll of its pain chance.
    fn interrupt_chance(
        &self,
        obs: &Observation,
        w: &Weapon,
        t: &Thing,
        d: f64,
        window: f64,
    ) -> f64 {
        if w.ammo_per_shot == 0 {
            return 0.0;
        }
        let ready = if Some(w.slot) != obs.weapon {
            self.tune.aim.switch_wait as f64
        } else {
            self.last_shot
                .filter(|&(s, _)| s == w.slot)
                .map_or(0.0, |(_, tic)| {
                    (tic as f64 + w.cycle - obs.tic as f64).max(0.0)
                })
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
        let shots = ((left / w.cycle).floor() + 1.0)
            .min(f64::from(Self::ammo(obs, w.slot) / w.ammo_per_shot));
        let rolls = w.pellets * w.hits(mon(t.name).radius, d);
        1.0 - (1.0 - VILE_PAIN).powf(shots * rolls)
    }

    /// Seconds of shooting it'd take (a lot, out of the gun's reach: anything else first).
    pub(super) fn time_to_kill(&self, w: &Weapon, t: &Thing, d: f64) -> f64 {
        let info = mon(t.name);
        if !self.in_reach(w, t, d) {
            return 1e4 + d;
        }
        let per_shot = w.damage_on(&info) * w.hits(info.radius, d);
        // The first shot goes at once, the rest a reload apart.
        let shots = (f64::from(info.health) / per_shot.max(0.1)).max(1.0);
        // And the launch: the rocket is out 8 tics after the trigger, the BFG's ball 30.
        let launch = match w.slot {
            5 => 8.0,
            7 => 30.0,
            _ => 0.0,
        };
        ((shots - 1.0) * w.cycle + FIRST_SHOT + launch) / TICRATE
    }
}
