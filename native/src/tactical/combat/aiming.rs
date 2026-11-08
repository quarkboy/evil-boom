//! Target selection, weapon choice, line of fire and blast safety.

use super::{ammo_cost, mon, segment_point_distance, Combat, BARREL_CLEAR, ENGAGE_RANGE, SLOTS};
use crate::geom::{dist, Point};
use crate::los::Body;
use crate::motor::{heading, norm, Observation, Thing, TICRATE};
use crate::navigator::PLAYER_HEIGHT;
use crate::tactical::bestiary::{monster, weapon_in, Attack, Weapon};

/// degrees a tic, turning to aim
const AIM_TURN: f64 = 18.0;

const TURN_ACCEL: f64 = 5.0;

/// a barrel this near a monster (and well clear of her) is shot instead
const BARREL_NEAR: f64 = 96.0;

/// getting clear of barrels: at most this long at a time...
const BARREL_TICS: i64 = 2 * 35;

/// ...then not again for this long (E1M7: pressed against a wall for ever)
const BARREL_REST: i64 = 3 * 35;

/// shots leave her this far above her feet...
const SHOT_HEIGHT: f64 = 36.0;

/// ...and find what they're aimed at only this steep
const AUTOAIM: f64 = 100.0 / 160.0;

/// a biter seen this lately near her rules out rockets
const ROCKET_WARY: i64 = 3 * 35;

/// something that shoots back from anywhere: shot at past a gun's reach while this much lands
const FAR_HITS: f64 = 0.2;

/// a weapon switch asked for: give it this long before asking again
const SWITCH_WAIT: i64 = 40;

/// an arch-vile winding up: its kill time counts this much...
const VILE_KILL_WEIGHT: f64 = 0.35;
/// ...and its fire going off unflinched this many seconds' worth
const VILE_MISSED: f64 = 8.0;
/// its pain chance (10/256 a hit)
const VILE_PAIN: f64 = 10.0 / 256.0;

/// pain rolls a second worth twice a gun's kill time, against an arch-vile
const FLINCH_ROLLS: f64 = 6.0;

/// tics from the trigger to the rocket (S_MISSILE1, then A_FireMissile)
const ROCKET_DELAY: f64 = 8.0;

/// a rocket's radius: it moves as a box this far each way along x and y, not a circle
const ROCKET_RADIUS: f64 = 11.0;
/// kept clear beyond the rocket's box each side of its way
const ROCKET_MARGIN: f64 = 2.0;

/// a pain elemental nearer than this isn't shot with rockets (its souls come out in front)
const PAIN_NEAR: f64 = 512.0;

/// What's to be killed first, beyond its damage: an arch-vile raises the rest, a pain elemental
/// keeps making more; a lost soul's only worth it once it's near.
fn priority(name: &str, d: f64) -> f64 {
    match name {
        "Archvile" => 3.0,
        "PainElemental" => 2.5,
        "LostSoul" if d > 320.0 => 0.4,
        _ => 1.0,
    }
}

/// tics to the first shot of a gun already up
const FIRST_SHOT: f64 = 4.0;

/// seconds a weapon switch costs
const SWITCH_COST: f64 = 0.8;

/// units a second she closes in or backs off by, fighting (strafing as she goes)
const GET_THERE: f64 = 250.0;

/// How near to t she'd fight it from with w, and how far: the gun's own (its spread, its blast)
/// and what t does up close (claws, bites; a cyberdemon's rockets too quick to dodge).
pub fn window(w: &Weapon, t: &Thing) -> (f64, f64) {
    let s = standoff(t.name);
    (w.keep.0.max(s), w.keep.1.max(s + 256.0))
}

/// The nearest she'd fight a monster from, whatever the gun.
pub fn standoff(name: &str) -> f64 {
    match name {
        "Cyberdemon" | "SpiderMastermind" => 640.0,
        "Fatso" => 400.0,
        "Arachnotron" | "Revenant" | "PainElemental" => 256.0,
        "Demon" | "Spectre" | "HellKnight" | "BaronOfHell" | "Cacodemon" => 160.0,
        "DoomImp" | "LostSoul" => 128.0,
        _ => 0.0,
    }
}

impl Combat {
    pub(super) fn pick_target(&mut self, obs: &Observation) -> Option<Thing> {
        let pos = obs.pos();
        let mut best: Option<(f64, Thing)> = None;
        for t in self.against_her() {
            let info = mon(t.name);
            let d = dist((t.x, t.y), pos);
            let mut s = (info.threat + 1.0) * priority(t.name, d) / (d + 192.0) * if Some(t.id) == self.target { 1.6 } else { 1.0 };
            let off = norm(heading(pos, (t.x, t.y)) - obs.angle).abs();
            s *= 1.0 - off.min(150.0) / 300.0; // (rather what's in front than what's behind)
            if !self.can_hit(obs, &t, None) {
                s *= 0.3; // (rather what a shot gets to)
            }
            if best.as_ref().is_none_or(|(bs, _)| s > *bs) {
                best = Some((s, t));
            }
        }
        self.target = best.as_ref().map(|(_, t)| t.id);
        best.map(|(_, t)| t)
    }

    pub(super) fn aim_and_fire(&mut self, obs: &Observation, t: &Thing) -> (f64, bool, i32) {
        let pos = obs.pos();
        let d = dist((t.x, t.y), pos);
        let slot = self.weapon_for(obs, t, d);
        let mut weapon = 0;
        let held = obs.weapon.unwrap_or(2);
        if slot != held && obs.tic - self.switch_tic > SWITCH_WAIT {
            weapon = slot;
            self.switch_tic = obs.tic;
            self.log.push(format!("tic {}: switching to {slot} (holding {held}) for {} {d:.0} off", obs.tic, t.name));
        }
        let w = weapon_in(if (1..=7).contains(&held) { held } else { 2 }, obs.super_shotgun);
        // lead a moving target with a missile weapon
        let (mut ax, mut ay) = (t.x, t.y);
        if !w.hitscan && w.speed > 0.0 {
            let tof = d / w.speed;
            ax += t.vx * tof;
            ay += t.vy * tof;
        }
        let aim = heading(pos, (ax, ay));
        let info = mon(t.name);
        let tolerance = 1.5f64.max((info.radius * 0.7).atan2(d.max(1.0)).to_degrees());
        let on = norm(aim - obs.angle).abs() < tolerance;
        let held_fire = if !Self::in_reach(&w, t, d) {
            "out of reach"
        } else if held == 5 && (!self.rocket_safe(obs, t) || !self.rocket_lane_clear(obs, t)) {
            "rocket not safe"
        } else if !self.can_hit(obs, t, None) {
            "no shot (too steep, or something in the way)"
        } else {
            ""
        };
        if held_fire != self.held_fire {
            if !held_fire.is_empty() {
                self.log.push(format!("tic {}: holding fire at {} {}, {d:.0} off: {held_fire}", obs.tic, t.name, t.id));
            }
            self.held_fire = held_fire.to_string();
        }
        (aim, on && held_fire.is_empty(), weapon)
    }

    /// Close enough to shoot at with w: inside its reach, or (at something that hurts her from
    /// anywhere) as far as enough of a shot still lands.
    pub fn in_reach(w: &Weapon, t: &Thing, d: f64) -> bool {
        let info = mon(t.name);
        if d >= ENGAGE_RANGE {
            return false;
        }
        if d < w.reach + if w.slot == 1 { info.radius } else { 0.0 } {
            return true;
        }
        info.attack == Attack::Hitscan && w.slot != 1 && w.hits(info.radius, d) >= FAR_HITS
    }

    /// Would a shot at t get to it, as Doom aims one (not too steep, nothing in the way); else out
    /// level, past it unless it's level with her.
    pub fn can_hit(&mut self, obs: &Observation, t: &Thing, height: Option<f64>) -> bool {
        let h = height.unwrap_or_else(|| mon(t.name).height);
        self.refresh_tic_cache(obs);
        let key = (t.id, h.to_bits());
        if let Some(&v) = self.hit_cache.get(&key) {
            return v;
        }
        let v = self.shot_clear(obs, (t.x, t.y), t.z, h);
        self.hit_cache.insert(key, v);
        v
    }

    pub(super) fn shot_clear(&mut self, obs: &Observation, at: Point, z: f64, h: f64) -> bool {
        self.shot_clear_from(obs, obs.pos(), at, z, h)
    }

    /// A rocket's way to t clear its whole width (MAP32: one fired past a pillar's corner beside
    /// her, the line from her eye clear, the rocket's side not: 86 off her).
    fn rocket_lane_clear(&mut self, obs: &Observation, t: &Thing) -> bool {
        // (from where she is and from where she'll be when it comes out: the launcher lets
        // it go 8 tics after the trigger, and she'd run up beside the pillar by then)
        let now = obs.pos();
        let then = (now.0 + self.velocity.0 * ROCKET_DELAY, now.1 + self.velocity.1 * ROCKET_DELAY);
        let h = mon(t.name).height;
        [now, then].into_iter().all(|pos| {
            let d = dist((t.x, t.y), pos).max(1.0);
            let (ux, uy) = ((t.x - pos.0) / d, (t.y - pos.1) / d);
            // (the box's reach to the side of its way: its radius flying along an axis, 1.4
            // times that on the diagonal; MAP08: one fired out of an alcove at 115 degrees,
            // the jamb's corner 14.5 off its way, the box 14.6 wide there, in her face)
            let half = ROCKET_RADIUS * (ux.abs() + uy.abs()) + ROCKET_MARGIN;
            let side = (-uy * half, ux * half);
            [1.0, -1.0].into_iter().all(|k| {
                let o = (side.0 * k, side.1 * k);
                self.shot_clear_from(obs, (pos.0 + o.0, pos.1 + o.1), (t.x + o.0, t.y + o.1), t.z, h)
            })
        })
    }

    fn shot_clear_from(&mut self, obs: &Observation, from: Point, at: Point, z: f64, h: f64) -> bool {
        let gun = self.z(obs) + SHOT_HEIGHT;
        let d = dist(at, from).max(1.0);
        let (mut lo, mut hi) = (z.max(gun - AUTOAIM * d), (z + h).min(gun + AUTOAIM * d));
        if hi <= lo {
            if !(z < gun && gun < z + h) {
                return false;
            }
            (lo, hi) = (gun - 1.0, gun + 1.0); // (out level: it only hits what's level with it)
        }
        let me = Body { x: from.0, y: from.1, z: gun - 0.75 * PLAYER_HEIGHT, height: PLAYER_HEIGHT }; // (the eye's at 0.75)
        self.refresh_tic_cache(obs);
        self.los.clear(me, Body { x: at.0, y: at.1, z: lo, height: hi - lo }, &self.heights_now)
    }

    /// Where to step for a shot at t she can see but can't hit: back from it when it's above
    /// her, towards it when it's below; None if neither way is safe.
    pub(super) fn shot_way(&mut self, obs: &Observation, t: &Thing) -> Option<Point> {
        let away = self.away_from(obs, (t.x, t.y));
        if t.z <= self.z(obs) {
            if let Some(a) = away {
                let toward = (-a.0, -a.1);
                return self.safe(obs, toward, 64.0).then_some(toward);
            }
        }
        away
    }

    /// A barrel she can see right by a monster, well clear of her: shoot that.
    pub(super) fn barrel_shot(&mut self, obs: &Observation) -> Option<Point> {
        let pos = obs.pos();
        let barrels: Vec<Point> = obs
            .barrels
            .iter()
            .flatten()
            .copied()
            .filter(|&b| dist(b, pos) > BARREL_CLEAR + 64.0 && self.visible.iter().any(|t| dist(b, (t.x, t.y)) < BARREL_NEAR + mon(t.name).radius))
            .collect();
        if barrels.is_empty() {
            return None;
        }
        self.refresh_tic_cache(obs);
        let me = Body { x: obs.x, y: obs.y, z: self.z(obs), height: PLAYER_HEIGHT };
        barrels
            .into_iter()
            .filter(|&b| self.los.clear(me, Body { x: b.0, y: b.1, z: self.floor_at(obs, b), height: 32.0 }, &self.heights_now))
            .reduce(|a, b| if dist(b, pos) < dist(a, pos) { b } else { a })
    }

    pub(super) fn floor_at(&self, obs: &Observation, p: Point) -> f64 {
        self.nav.poly_at(p).and_then(|q| obs.height(self.nav.sector_of_poly(q))).map_or(0.0, |h| h.floor)
    }

    /// The way to go to get clear of the barrels by her: the way she's going already if still
    /// safe and clearing them, else a new one; None once clear (or she can't get so).
    pub(super) fn away_from_barrels(&mut self, obs: &Observation, nearest: Point) -> Option<Point> {
        let pos = obs.pos();
        if obs.tic < self.barrel_rest_until {
            return None;
        }
        if self.barrel_way.is_none() {
            self.barrel_since = obs.tic;
        } else if obs.tic - self.barrel_since > BARREL_TICS {
            self.barrel_way = None;
            self.barrel_rest_until = obs.tic + BARREL_REST;
            return None;
        }
        if dist(nearest, pos) >= BARREL_CLEAR + if self.barrel_way.is_some() { 64.0 } else { 0.0 } {
            self.barrel_way = None;
            return None;
        }
        if let Some(way) = self.barrel_way {
            if self.safe(obs, way, 48.0) && self.clear_of_barrels(obs, way) {
                return Some(way);
            }
        }
        let way = self.away_from(obs, nearest);
        self.barrel_way = way.filter(|&w| self.clear_of_barrels(obs, w));
        self.barrel_way
    }

    /// Does going that way (a second's run) leave her further from every barrel near her?
    fn clear_of_barrels(&self, obs: &Observation, d: Point) -> bool {
        let pos = obs.pos();
        let ahead = (pos.0 + d.0 * 96.0, pos.1 + d.1 * 96.0);
        obs.barrels.iter().flatten().filter(|&&b| dist(b, pos) < BARREL_CLEAR).all(|&b| dist(b, ahead) > dist(b, pos))
    }

    /// Is a rocket at t sure not to blow up by her (E1M9's died at 88 health, nothing in sight)?
    fn rocket_safe(&self, obs: &Observation, t: &Thing) -> bool {
        let pos = obs.pos();
        let clear = crate::tactical::bestiary::weapon(5).splash + 48.0;
        if dist((t.x, t.y), pos) < clear + mon(t.name).radius {
            return false;
        }
        // (where they'll be when it gets there: a revenant closing in met it 71 off her)
        let me_then = (pos.0 + self.velocity.0 * ROCKET_DELAY, pos.1 + self.velocity.1 * ROCKET_DELAY);
        for o in self.visible.iter() {
            let tof = ROCKET_DELAY + dist((o.x, o.y), pos) / 20.0;
            let then = (o.x + o.vx * tof, o.y + o.vy * tof);
            if dist(then, me_then) < clear + mon(o.name).radius && dist(then, me_then) < dist((o.x, o.y), pos) {
                return false;
            }
        }
        if t.name == "PainElemental" && dist((t.x, t.y), pos) < PAIN_NEAR {
            return false; // (the souls it spits out come between: E... dead at 90 health, one 28 off)
        }
        let mut others: Vec<(f64, f64, f64)> = self.visible.iter().filter(|o| o.id != t.id).map(|o| (o.x, o.y, mon(o.name).radius)).collect();
        // (and anything close in front she'd see, seen yet or not: a soul just spat out)
        others.extend(
            obs.things
                .iter()
                .flatten()
                .filter(|o| o.id != t.id && monster(o.name).is_some() && dist((o.x, o.y), pos) < clear * 2.0)
                .map(|o| (o.x, o.y, mon(o.name).radius)),
        );
        others.extend(obs.barrels.iter().flatten().map(|b| (b.0, b.1, 10.0)));
        // (E1M9: dead at full health, a demon 83 away: it came round a corner into the way)
        others.extend(
            self.remaining()
                .iter()
                .filter(|m| m.id != t.id && obs.tic - m.tic < ROCKET_WARY && matches!(mon(m.name).attack, Attack::Melee | Attack::Charge))
                .map(|m| (m.x, m.y, mon(m.name).radius + 128.0)),
        );
        for (x, y, r) in others {
            if dist((x, y), pos) < clear + r {
                return false;
            }
            if segment_point_distance(pos, (t.x, t.y), (x, y)) < r + 12.0 && dist((x, y), pos) < dist((t.x, t.y), pos) {
                return false;
            }
        }
        true
    }

    /// The gun that'd see t off soonest from here, counting the switch and what its ammo is worth.
    fn weapon_for(&self, obs: &Observation, t: &Thing, d: f64) -> i32 {
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
            let w = weapon_in(slot, obs.super_shotgun);
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
        weapon_in(slot, obs.super_shotgun)
    }

    /// The gun that'd see t off soonest, each judged at the distance `at` gives for it (and the
    /// time to get there).
    fn best_gun(&self, obs: &Observation, t: &Thing, at: impl Fn(&Weapon) -> f64) -> i32 {
        let here = dist((t.x, t.y), obs.pos());
        let held = Self::held(obs);
        let info = mon(t.name);
        let crowd = self.visible.len();
        let (mut best, mut cost) = (obs.weapon.unwrap_or(2), f64::INFINITY);
        for &slot in &SLOTS {
            if !held.contains(&slot) {
                continue;
            }
            let w = weapon_in(slot, obs.super_shotgun);
            let have = Self::ammo(obs, slot);
            if (w.ammo_per_shot > 0 && have < w.ammo_per_shot) || (slot == 5 && !self.rocket_safe(obs, t)) {
                continue;
            }
            let d = at(&w);
            if slot == 1 && !Self::in_reach(&w, t, d) {
                continue; // (not the fist for being free when nothing reaches: as Astra's branch has it)
            }
            let mut c = self.time_to_kill(&w, t, d) + (d - here).abs() / GET_THERE;
            if w.ammo_per_shot > 0 {
                let shots = (f64::from(info.health) / (w.damage_on(&info) * w.hits(info.radius, d)).max(1.0)).max(1.0);
                let (per, low) = ammo_cost(slot);
                c += shots * f64::from(w.ammo_per_shot) * per * (low / f64::from(have)).max(1.0);
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
            if c < cost {
                best = slot;
                cost = c;
            }
        }
        best
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
    fn time_to_kill(&self, w: &Weapon, t: &Thing, d: f64) -> f64 {
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

    /// This tic's turn (positive is clockwise) towards `aim`.
    pub(super) fn turn(&mut self, obs: &Observation, aim: Option<f64>) -> f64 {
        let Some(aim) = aim else {
            self.rate = 0.0;
            return 0.0;
        };
        let err = norm(aim - obs.angle);
        let want = AIM_TURN.min((2.0 * TURN_ACCEL * err.abs()).sqrt()).min(err.abs()).copysign(err);
        self.rate = (self.rate - TURN_ACCEL).max((self.rate + TURN_ACCEL).min(want));
        if err.abs() < 1.0 {
            self.rate = err;
        }
        -self.rate
    }
}
