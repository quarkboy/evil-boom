//! Target selection, aiming and the trigger, line of fire and blast safety (the weapon choice:
//! `arsenal`).

use super::moves::HeldFire;
use super::style::Style;
use super::{mon, segment_point_distance, Combat, BARREL_CLEAR};
use crate::motor::follower::blast_gap;
use crate::geom::{dist, Point};
use crate::los::Body;
use crate::motor::{heading, norm, Observation, Thing};
use crate::navigator::PLAYER_HEIGHT;
use crate::tactical::bestiary::{monster, weapon_in, Attack};

/// degrees a tic, turning to aim
const AIM_TURN: f64 = 18.0;

const TURN_ACCEL: f64 = 5.0;

/// a barrel this near a monster (its chain all clear of her) is shot instead
const BARREL_NEAR: f64 = 96.0;
/// a barrel's blast is clear of her this far off (its 128 and her radius, and a little), by
/// `blast_gap`...
const BARREL_SAFE: f64 = 150.0;
/// ...and sets off another this near (its blast and the other's radius)
const BARREL_CHAIN: f64 = 138.0;
/// a barrel's radius
const BARREL_RADIUS: f64 = 10.0;
/// her missiles' radius, about (a rocket's 11, plasma's 13)
const MISSILE_RADIUS: f64 = 13.0;

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

/// a weapon switch asked for: give it this long before asking again
pub(super) const SWITCH_WAIT: i64 = 40;

/// tics from the trigger to the rocket (S_MISSILE1, then A_FireMissile)
pub(super) const ROCKET_DELAY: f64 = 8.0;
/// the BFG's spray: this far from her, this far either side of the ball's way (A_BFGSpray)
const BFG_REACH: f64 = 1024.0;
const BFG_CONE: f64 = 45.0;
/// the BFG's ball leaves this long after the trigger
pub(super) const BFG_DELAY: i64 = 30;
/// with a boss in sight, others this far off aren't shot (left to it)
const LEAVE_TO_BOSS: f64 = 384.0;
/// a rocket's speed, a tic
const ROCKET_SPEED: f64 = 20.0;

/// a rocket's radius: it moves as a box this far each way along x and y, not a circle
const ROCKET_RADIUS: f64 = 11.0;
/// kept clear beyond the rocket's box each side of its way (a miss is better than a near thing)
const ROCKET_MARGIN: f64 = 8.0;
/// a rocket's bottom over her feet as it comes out, and its height
const ROCKET_BOTTOM: f64 = 32.0;
const ROCKET_HEIGHT: f64 = 8.0;
/// a rocket going off further than this from her doesn't reach her (its blast 128, her radius 16)
const ROCKET_CLEAR: f64 = 160.0;
/// autoaim looks this far for something to aim at, and this many degrees to either side when
/// there's nothing straight ahead (ANG90/16, 1<<26)
const AUTOAIM_RANGE: f64 = 1024.0;
const AUTOAIM_SPREAD: f64 = 5.625;
/// plasma's radius
const PLASMA_RADIUS: f64 = 13.0;
/// autoaim's line goes out from her feet + this (her middle and 8: P_AimLineAttack's shootz), as
/// does a projectile's middle (it comes out at her feet + 32, 8 high)
const AIM_HEIGHT: f64 = 36.0;

/// with a hitscanner about, a monster she'd be longer than this many seconds killing isn't put
/// first (`priority`)
const KILL_FIRST: f64 = 8.0;

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

impl Combat {
    pub(super) fn pick_target(&mut self, obs: &Observation) -> Option<Thing> {
        let pos = obs.pos();
        let mut best: Option<(f64, Thing)> = None;
        let against = self.against_her();
        let hitscanners: Vec<i32> = against.iter().filter(|t| mon(t.name).attack == Attack::Hitscan).map(|t| t.id).collect();
        for t in against {
            let info = mon(t.name);
            let d = dist((t.x, t.y), pos);
            // (one she'd be long at, with another that's hitscan about, isn't put first: the time
            // it'd take is the hitscanner's to shoot her in. MAP11 UV from a pistol start: a pain
            // elemental the target, 16 s of pistol, kept clear of while shotgun guys shot her dead)
            let long = f64::from(info.health) / self.firepower_on(obs, Some(&info)).max(1.0);
            let first = if long > KILL_FIRST && hitscanners.iter().any(|&h| h != t.id) { 1.0 } else { priority(t.name, d) };
            let mut s = (info.threat + 1.0) * first / (d + 192.0) * if Some(t.id) == self.target { 1.6 } else { 1.0 };
            let off = norm(heading(pos, (t.x, t.y)) - obs.angle).abs();
            s *= 1.0 - off.min(150.0) / 300.0; // (rather what's in front than what's behind)
            if !self.can_hit(obs, &t, None) {
                s *= 0.3; // (rather what a shot gets to)
            }
            if best.as_ref().is_none_or(|(bs, _)| s > *bs) {
                best = Some((s, t));
            }
        }
        // (the BFG up: the one whose line puts the most in its spray, forty lines from her a
        // quarter turn wide along the ball's way, 1024 out; the ball goes to the first on her
        // line, the spray the way it went)
        if obs.weapon == Some(7) {
            let me = obs.pos();
            let in_cone = |c: &Self, m: &Thing| {
                let at = heading(me, (m.x, m.y));
                c.visible.iter().filter(|o| dist((o.x, o.y), me) < BFG_REACH && crate::motor::norm(heading(me, (o.x, o.y)) - at).abs() < BFG_CONE).map(|o| mon(o.name).threat.max(0.5)).sum::<f64>()
            };
            // (of those she has a shot at, in the spray's reach: not one she can't hit, stood at
            // for an hour, E3M4)
            let shots: Vec<Thing> = self.visible.clone().into_iter().filter(|m| mon(m.name).attack != Attack::None && dist((m.x, m.y), me) < BFG_REACH).collect();
            let mut hittable = Vec::new();
            for m in shots {
                if self.can_hit(obs, &m, None) {
                    hittable.push(m);
                }
            }
            if let Some(m) = hittable.into_iter().max_by(|a, b| in_cone(self, a).total_cmp(&in_cone(self, b))) {
                best = Some((0.0, m));
            }
        }
        self.target = best.as_ref().map(|(_, t)| t.id);
        best.map(|(_, t)| t)
    }

    pub(super) fn aim_and_fire(&mut self, obs: &Observation, t: &Thing) -> (f64, bool, i32) {
        let pos = obs.pos();
        let d = dist((t.x, t.y), pos);
        let rockets = Self::held(obs).contains(&5) && Self::ammo(obs, 5) > 0;
        let blocked = rockets && !self.rocket_lane_clear(obs, t);
        if blocked {
            self.rocket_blocked = Some((t.id, obs.tic));
        }
        // (fought up close, the weapon for it from the start, not whatever's best from here
        // while she closes in: E3M4, at a demon 299 off with the rocket launcher up, the fist
        // brought up 46 off)
        let preferred = self.preferred_gun(obs, t);
        let slot = if Style::of(&preferred, t) == Style::Melee { preferred.slot } else { self.weapon_for(obs, t, d) };
        let mut weapon = 0;
        let held = obs.weapon.unwrap_or(2);
        if slot != held && obs.tic - self.switch_tic > SWITCH_WAIT {
            weapon = slot;
            self.switch_tic = obs.tic;
            self.log.push(format!("tic {}: switching to {slot} (holding {held}) for {} {d:.0} off", obs.tic, t.name));
        }
        let w = weapon_in(if (1..=7).contains(&held) { held } else { 2 }, obs);
        // lead a moving target with a missile weapon
        let (mut ax, mut ay) = (t.x, t.y);
        if !w.hitscan && w.speed > 0.0 {
            let tof = d / w.speed;
            let (vx, vy) = self.moving(t);
            ax += vx * tof;
            ay += vy * tof;
        }
        let aim = heading(pos, (ax, ay));
        let info = mon(t.name);
        let tolerance = 1.5f64.max((info.radius * 0.7).atan2(d.max(1.0)).to_degrees());
        let on = norm(aim - obs.angle).abs() < tolerance;
        let held_fire = if !Self::in_reach(&w, t, d) {
            Some(HeldFire::OutOfReach)
        } else if held == 5 && (!self.rocket_safe(obs, t) || blocked) {
            Some(HeldFire::RocketNotSafe)
        } else if held == 6 && !self.projectile_reaches(obs, t, pos, PLASMA_RADIUS) {
            Some(HeldFire::PlasmaBlocked)
        } else if !mon(t.name).boss && d > LEAVE_TO_BOSS && self.visible.iter().any(|b| b.id != t.id && mon(b.name).boss) {
            // (a boss in sight: the others left to it, not near her; its fire lands on them and
            // they turn on it, her ammo kept for it. The user's: infighting, E3M8 from a pistol
            // start, 1400 of 3000-odd damage's worth of ammo spent on two cacodemons and a baron,
            // out of ammo with the spider mastermind at 1000-2000)
            Some(HeldFire::LeftToBoss)
        } else if !self.can_hit(obs, t, None) {
            Some(HeldFire::NoShot)
        } else if Self::barrel_in_line(obs, &w, t) {
            Some(HeldFire::BarrelInLine)
        } else {
            None
        };
        if held_fire != self.held_fire {
            if let Some(h) = held_fire {
                self.log.push(format!("tic {}: holding fire at {} {}, {d:.0} off: {}", obs.tic, t.name, t.id, h.label()));
            }
            self.held_fire = held_fire;
        }
        (aim, on && held_fire.is_none(), weapon)
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
        if self.invulnerable(obs.tic) {
            return true;
        }
        // (from where she is and from where she'll be when it comes out: the launcher lets
        // it go 8 tics after the trigger, and she'd run up beside the pillar by then)
        let now = obs.pos();
        let then = (now.0 + self.velocity.0 * ROCKET_DELAY, now.1 + self.velocity.1 * ROCKET_DELAY);
        let centre = self.z(obs) + AIM_HEIGHT;
        let mid = t.z + mon(t.name).height / 2.0;
        // (the way to it, and the way she faces: it goes where she faces, and she fires within a
        // couple of degrees of it; the arena's crates, a corner clipped 3 units off the target's
        // way, at full health, in her face)
        // (only as far as its blast could come back to her: beyond, something in the way is a
        // miss, no harm; checked all the way, she held fire at a cyberdemon till it had her)
        // (and the way the game will send it: autoaim swings it to whatever's first on her way,
        // or 5.6 degrees off it, and down or up at it: into a ledge's lip firing down into a pit,
        // a railing it'd fly over level; the user's)
        // (and further, running after it: she gains on its blast as it flies, at 20 a tic to her
        // 16, so something 600 out is a blast 120 from her; MAP06, shooting on the way at an imp
        // 1500 off, the route running her on after the rocket, twice, dead)
        let after = {
            let d = dist((t.x, t.y), now).max(1.0);
            let gain = (self.velocity.0 * (t.x - now.0) + self.velocity.1 * (t.y - now.1)) / d;
            1.0 - (gain / ROCKET_SPEED).clamp(0.0, 0.8)
        };
        [now, then].into_iter().all(|pos| {
            let d = dist((t.x, t.y), pos).max(1.0);
            let len = d.min(ROCKET_CLEAR / after + ROCKET_MARGIN);
            let to = ((mid - centre) / d).clamp(-AUTOAIM, AUTOAIM);
            let (h, seen, middle) = self.autoaim(obs, pos, obs.angle);
            [heading(pos, (t.x, t.y)), obs.angle].into_iter().all(|h| self.rocket_flies(obs, pos, h, len, to, (ROCKET_RADIUS, ROCKET_MARGIN)))
                && [seen, middle].into_iter().all(|slope| self.rocket_flies(obs, pos, h, len, slope, (ROCKET_RADIUS, ROCKET_MARGIN)))
        })
    }

    /// Where the game sends a rocket fired from `pos` facing `facing` (P_SpawnPlayerMissile's
    /// autoaim, P_AimLineAttack): the first monster on that way within 1024, else on the way 5.6
    /// degrees to one side, then the other: the rocket's way swung to it, climbing or dipping to
    /// it (to its middle: the game aims at what's seen of it, above a railing higher, so its
    /// middle is the nearer of the two to what's in between); none, straight on and level.
    /// Also the slope to its middle (the game aims at what's seen of it, but a near thing's best
    /// judged both ways: for a rocket by her, either way clear).
    pub(super) fn autoaim(&mut self, obs: &Observation, pos: Point, facing: f64) -> (f64, f64, f64) {
        let shootz = self.z(obs) + AIM_HEIGHT;
        self.refresh_tic_cache(obs);
        for off in [0.0, AUTOAIM_SPREAD, -AUTOAIM_SPREAD] {
            let h = facing + off;
            let (ux, uy) = (h.to_radians().cos(), h.to_radians().sin());
            let mut on: Vec<(f64, &Thing)> = obs
                .things
                .iter()
                .flatten()
                .filter(|m| monster(m.name).is_some() && m.health > 0)
                .filter_map(|m| {
                    let (dx, dy) = (m.x - pos.0, m.y - pos.1);
                    let along = dx * ux + dy * uy;
                    let side = (dx * uy - dy * ux).abs();
                    (along > 0.0 && along < AUTOAIM_RANGE && side < mon(m.name).radius).then_some((along, m))
                })
                .collect();
            on.sort_by(|a, b| a.0.total_cmp(&b.0));
            // (the first it sees any of, aimed at the middle of what it sees: over a railing, the
            // part above it; the game's way, P_AimLineAttack)
            for (along, m) in on {
                let (lo, hi) = (m.z, m.z + mon(m.name).height);
                let seen: Vec<f64> = (0..)
                    .map(|k| lo + 4.0 * f64::from(k))
                    .take_while(|&z| z <= hi)
                    .filter(|&z| {
                        let eye = Body { x: pos.0, y: pos.1, z: shootz - 3.0, height: 4.0 };
                        self.los.clear(eye, Body { x: m.x, y: m.y, z: z - 0.5, height: 1.0 }, &self.heights_now)
                    })
                    .collect();
                if let (Some(&bottom), Some(&top)) = (seen.first(), seen.last()) {
                    let slope = |z: f64| ((z - shootz) / along.max(1.0)).clamp(-AUTOAIM, AUTOAIM);
                    return (h, slope((bottom + top) / 2.0), slope((lo + hi) / 2.0));
                }
            }
        }
        (facing, 0.0, 0.0)
    }

    /// Would a rocket fired from `pos` the way `h` (degrees) get `len` with nothing in its box's
    /// way, at its own height (its bottom at her feet + 32, its top + 40, climbing to `to_mid` if
    /// aimed at something there): a step as high as its bottom stops it as a wall does (the
    /// arena's 32-high steps, seen over, flown into), and the box reaches a corner its middle
    /// passes (its radius on an axis, 1.4 times it on the diagonal, and some: a miss is better
    /// than one in her face, the user's).
    /// (`shape`: the projectile's radius, and what's kept clear beyond it each side)
    pub(super) fn rocket_flies(&mut self, obs: &Observation, pos: Point, h: f64, len: f64, slope: f64, (radius, margin): (f64, f64)) -> bool {
        let (ux, uy) = (h.to_radians().cos(), h.to_radians().sin());
        let half = radius * (ux.abs() + uy.abs()) + margin;
        let foot = self.z(obs);
        let (bottom, top) = (foot + ROCKET_BOTTOM - 0.5, foot + ROCKET_BOTTOM + ROCKET_HEIGHT + 0.5);
        let rise = slope * len;
        self.refresh_tic_cache(obs);
        [0.0, 1.0, -1.0].into_iter().all(|k| {
            let from = (pos.0 - uy * half * k, pos.1 + ux * half * k);
            let to = (from.0 + ux * len, from.1 + uy * len);
            [bottom, top].into_iter().all(|z| {
                let me = Body { x: from.0, y: from.1, z: z - 3.0, height: 4.0 }; // (its eye, 0.75 up, at z)
                let at = Body { x: to.0, y: to.1, z: z + rise - 0.5, height: 1.0 };
                self.los.clear(me, at, &self.heights_now)
            })
        })
    }

    /// Would a shot of a projectile weapon (`radius`: the rocket's 11, plasma's 13) fired at t from
    /// `q`, facing it, go the way autoaim sends it and get to it, nothing in its box's way at its
    /// height? (Plasma: not wasted on a ledge's lip below her, or the floor in front of something
    /// up on a ledge; the user's.) Its middle only, not its box's sides: a side grazing a corner
    /// costs a cell at most (it steps 25 at a time, the box checked where it lands, and may get
    /// by), and holding fire for it held her out of a fight (MAP16's cover, a Spider Mastermind 700
    /// off, the sides 14 off the line clipping the cover's edges: 6/8 lost, none before).
    pub(super) fn projectile_reaches(&mut self, obs: &Observation, t: &Thing, q: Point, radius: f64) -> bool {
        let d = dist((t.x, t.y), q);
        let (h, seen, _) = self.autoaim(obs, q, heading(q, (t.x, t.y)));
        self.rocket_flies(obs, q, h, (d - mon(t.name).radius - radius).max(1.0), seen, (0.0, 0.0))
    }

    /// Plasma from here wouldn't get to t (autoaim tips it into a lip below or the floor in front of
    /// it above): the nearest step, up to 96 on (towards it first, then 30 and 60 degrees off it
    /// either side), she can take without dropping off anything (the fight's own `safe`: no drop she
    /// couldn't come back up, nothing that hurts), from where it would; the way to it.
    /// (Not for rockets: stepping for one put her by the crates' columns more than it helped,
    /// own-rocket hits in the crate arena 26 -> 44.)
    pub(super) fn plasma_spot(&mut self, obs: &Observation, t: &Thing) -> Option<Point> {
        let pos = obs.pos();
        let to = heading(pos, (t.x, t.y));
        for off in [0.0, 30.0, -30.0, 60.0, -60.0] {
            let h = (to + off).to_radians();
            let d = (h.cos(), h.sin());
            for k in 1..=6 {
                let len = 16.0 * f64::from(k);
                if !self.safe(obs, d, len) {
                    break;
                }
                let q = (pos.0 + d.0 * len, pos.1 + d.1 * len);
                if self.projectile_reaches(obs, t, q, PLASMA_RADIUS) {
                    return Some(d);
                }
            }
        }
        None
    }

    /// (a rocket on its way out, the way she faces now going into something near) The nearest
    /// heading to her facing with a clear lane far enough out that its blast won't reach her:
    /// the last resort once the trigger's pressed (the user's: it can't be called back; turned
    /// away from what it'd hit).
    pub(super) fn rocket_way_out(&mut self, obs: &Observation) -> Option<f64> {
        let pos = obs.pos();
        // (each way as autoaim will take it from there)
        let goes = |c: &mut Self, facing: f64| {
            let (h, seen, middle) = c.autoaim(obs, pos, facing);
            [seen, middle].into_iter().all(|slope| c.rocket_flies(obs, pos, h, ROCKET_CLEAR, slope, (ROCKET_RADIUS, ROCKET_MARGIN)))
        };
        if goes(self, obs.angle) {
            return None;
        }
        (1..=18).flat_map(|k| [f64::from(k) * 10.0, -f64::from(k) * 10.0]).map(|d| obs.angle + d).find(|&h| goes(self, h))
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

    /// A barrel she can see right by a monster, well clear of her, and the barrels it sets off in
    /// turn too: shoot that (MAP23: one by the mancubus 320 off her, its chain through the
    /// field of barrels she stood in, the blasts on her, dead in 5 s).
    pub(super) fn barrel_shot(&mut self, obs: &Observation) -> Option<Point> {
        let barrels: Vec<Point> = obs
            .barrels
            .iter()
            .flatten()
            .copied()
            .filter(|&b| dist(b, obs.pos()) > BARREL_CLEAR + 64.0 && self.visible.iter().any(|t| dist(b, (t.x, t.y)) < BARREL_NEAR + mon(t.name).radius) && Self::chain_clear(obs, b))
            .collect();
        let pos = obs.pos();
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

    /// A barrel going off (hit: it blows 15 tics later) whose blast, or its chain's, reaches her:
    /// of the chain's barrels whose blasts reach her, the one that goes off first (fewest blasts
    /// from one going off; the chain burns a barrel a blast at a time), the nearest of those, to
    /// get away from. Not the nearest barrel of all: MAP23's hall of barrels, the chain burning
    /// down it towards her, away from the nearest one by the wall and back from the next, till it
    /// got there, dead.
    pub(super) fn lit_barrel_by(obs: &Observation) -> Option<Point> {
        let pos = obs.pos();
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        let mut chain: Vec<(Point, u32)> = obs.barrels_lit.iter().map(|&l| (l, 0)).collect();
        let mut i = 0;
        while i < chain.len() {
            let (c, hops) = chain[i];
            for &x in &barrels {
                if blast_gap(c, x) < BARREL_CHAIN && !chain.iter().any(|&(y, _)| dist(x, y) < 1.0) {
                    chain.push((x, hops + 1));
                }
            }
            i += 1;
        }
        chain.into_iter().filter(|&(b, _)| blast_gap(pos, b) < BARREL_SAFE).min_by(|a, b| a.1.cmp(&b.1).then(blast_gap(a.0, pos).total_cmp(&blast_gap(b.0, pos)))).map(|(b, _)| b)
    }

    /// Would every barrel a blast at `b` sets off, in turn (each within a blast of the one
    /// before), go off clear of her? Not her distance from the one shot alone: E1M8's yard of
    /// barrels, shot from among them, all of it round her (walls not counted: rather a shot
    /// missed than a blast through a gap).
    pub(super) fn chain_clear(obs: &Observation, b: Point) -> bool {
        let pos = obs.pos();
        let barrels: Vec<Point> = obs.barrels.iter().flatten().copied().collect();
        let mut chain = vec![b];
        let mut i = 0;
        while i < chain.len() {
            let c = chain[i];
            if blast_gap(pos, c) < BARREL_SAFE {
                return false;
            }
            for &x in &barrels {
                if blast_gap(c, x) < BARREL_CHAIN && !chain.iter().any(|&y| dist(x, y) < 1.0) {
                    chain.push(x);
                }
            }
            i += 1;
        }
        true
    }

    /// A barrel on the line of a shot at t (up to t, within the gun's spread) whose chain would go
    /// off by her: a bullet takes the first thing on its line (E1M8: demons fought among the
    /// barrels, the pistol's shots at them into the barrels beside her).
    fn barrel_in_line(obs: &Observation, w: &crate::tactical::bestiary::Weapon, t: &Thing) -> bool {
        let pos = obs.pos();
        let d = dist((t.x, t.y), pos);
        // (a bullet: anywhere in the spread at that distance; a missile: what its box touches)
        let width = |along: f64| if w.hitscan { BARREL_RADIUS + along * w.spread.max(1.0).to_radians().tan() } else { BARREL_RADIUS + MISSILE_RADIUS };
        obs.barrels.iter().flatten().copied().any(|b| {
            let along = dist(b, pos);
            along < d && segment_point_distance(pos, (t.x, t.y), b) < width(along) && !Self::chain_clear(obs, b)
        })
    }

    pub(crate) fn floor_at(&self, obs: &Observation, p: Point) -> f64 {
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

    /// Does going that way (a second's run) leave her further from every barrel near her, and
    /// near no other there? (MAP23: a hall of barrels, every spot by some; away from the row
    /// beside her was into the next, back and forth between them till the mancubus's fireballs
    /// set them off, dead.)
    fn clear_of_barrels(&self, obs: &Observation, d: Point) -> bool {
        let pos = obs.pos();
        let ahead = (pos.0 + d.0 * 96.0, pos.1 + d.1 * 96.0);
        obs.barrels.iter().flatten().filter(|&&b| dist(b, pos) < BARREL_CLEAR || dist(b, ahead) < BARREL_CLEAR).all(|&b| dist(b, ahead) > dist(b, pos))
    }

    /// How far off a rocket's burst must be from her: its blast and a margin.
    pub(super) fn rocket_clear() -> f64 {
        crate::tactical::bestiary::weapon(5).splash + 48.0
    }

    /// Is a rocket at t sure not to blow up by her (E1M9's died at 88 health, nothing in sight)?
    pub(super) fn rocket_safe(&self, obs: &Observation, t: &Thing) -> bool {
        if self.invulnerable(obs.tic) {
            return true; // (her own blast can't hurt her either)
        }
        let pos = obs.pos();
        let clear = Self::rocket_clear();
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
        // (each with how far it could come into the way while the rocket's near her: none for
        // what's in sight, its blast's reach for a claw she's lost sight of)
        let mut others: Vec<(f64, f64, f64, f64)> = self.visible.iter().filter(|o| o.id != t.id).map(|o| (o.x, o.y, mon(o.name).radius, 0.0)).collect();
        // (and anything close in front she'd see, seen yet or not: a soul just spat out)
        others.extend(
            obs.things
                .iter()
                .flatten()
                .filter(|o| o.id != t.id && monster(o.name).is_some() && dist((o.x, o.y), pos) < clear * 2.0)
                .map(|o| (o.x, o.y, mon(o.name).radius, 0.0)),
        );
        others.extend(obs.barrels.iter().flatten().map(|b| (b.0, b.1, 10.0, 0.0)));
        // (E1M9: dead at full health, a demon 83 away: it came round a corner into the way)
        others.extend(
            self.remaining()
                .iter()
                .filter(|m| m.id != t.id && obs.tic - m.tic < ROCKET_WARY && matches!(mon(m.name).attack, Attack::Melee | Attack::Charge))
                .map(|m| (m.x, m.y, mon(m.name).radius, 128.0)),
        );
        // (the leeway only for coming into the way: a rocket bursts on it only there, and one
        // behind her can't get in front of it; E3M4 UV, demons all round, four rockets held
        // for every demon 237-344 off till the fist was all she had)
        for (x, y, r, pad) in others {
            if dist((x, y), pos) < clear + r {
                return false;
            }
            // (in the way only where a burst on it reaches her: one in the lane further off
            // takes the rocket itself; MAP08 HMP, twelve demons coming down a hall 950 off, no
            // rocket for the pack in its own lane, the pistol out instead)
            let near_her = dist((x, y), pos) < clear + r + pad;
            if near_her && segment_point_distance(pos, (t.x, t.y), (x, y)) < r + pad + 12.0 && dist((x, y), pos) < dist((t.x, t.y), pos) + pad {
                return false;
            }
        }
        true
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
