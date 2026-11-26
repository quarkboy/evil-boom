// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A decision turned into this tic's controls: the turn onto the line, the tap, the burst kept.

use super::*;

impl Combat {
    /// The controls for a decision. `legs`: the fight's move has her legs (the arbiter's call);
    /// else `base`'s do (a route's, the explorer's or a retreat's), and only the aim and the
    /// trigger are the fight's. `base` is given with the fight's legs too when a route's
    /// walking under it, for what's in her way.
    pub fn controls(
        &mut self,
        obs: &Observation,
        d: &Decision,
        base: Option<&Controls>,
        legs: bool,
    ) -> Controls {
        let _timed = crate::debug::Timed::new("fight.controls");
        let turn = self.turn(obs, d.aim);
        if let Some(b) = base {
            self.route_walks(obs, b);
        }
        let mut c = match base {
            Some(b) if !legs => {
                let mut c = Controls {
                    forward: b.forward,
                    strafe: b.strafe,
                    turn,
                    use_: b.use_,
                    run: b.run,
                    weapon: b.weapon,
                    ..Controls::default()
                };
                // Nothing to shoot with: aiming fires nothing, and the route's turn is what faces a shut
                // door on her way to press it.
                if d.aim.is_none() || !self.armed(obs) || d.kind.route_faces() {
                    c.turn = b.turn;
                } else {
                    // The route's way in the world, whichever way she's turned to aim.
                    let way = obs.angle - b.turn + (-b.strafe).atan2(b.forward).to_degrees();
                    let rel = norm(way - (obs.angle - turn)).to_radians();
                    let mag = b.forward.hypot(b.strafe);
                    c.forward = mag * rel.cos();
                    c.strafe = -mag * rel.sin();
                }
                c
            }
            _ => {
                let mut c = Controls {
                    turn,
                    ..Controls::default()
                };
                let facing = obs.angle - turn; // (thrust goes the way faced after this tic's turn)
                                               // A turn about soon after the last one: she stops a moment instead, and goes
                                               // when the moment's past. To and fro inside a second is a jitter no player's
                                               // legs make; getting out of the way of something isn't held up for it.
                let mag = d.moving.map_or(0.0, |m| m.0.hypot(m.1)).max(1e-6);
                let way = d.moving.map(|m| (m.0 / mag, m.1 / mag));
                // (against the way she last went: a turn about if that was just now, or if she's
                // been held from it since)
                let since_turn = obs.tic - self.steer.legs_turned;
                let about = way.zip(self.steer.legs_way).is_some_and(|(m, (w, at))| {
                    m.0 * w.0 + m.1 * w.1 < -0.5
                        && (obs.tic - at <= DART_GAP || since_turn < self.tune.steer.dart_tics)
                });
                let held = about
                    && !d.urgent
                    && !d.kind.escaping()
                    && since_turn < self.tune.steer.dart_tics;
                if about && !held {
                    self.steer.legs_turned = obs.tic;
                }
                if let Some(w) = way.filter(|_| !held) {
                    self.steer.legs_way = Some((w, obs.tic));
                }
                if held {
                    (c.forward, c.strafe) = crate::motor::brake(obs, facing);
                } else if let Some(m) = d.moving {
                    let rel = norm(m.1.atan2(m.0).to_degrees() - facing).to_radians();
                    c.forward = 50.0 * rel.cos();
                    c.strafe = -50.0 * rel.sin();
                } else if self.coasting_off(obs) {
                    // Nowhere the fight wants her: not where the speed she has would take her either.
                    (c.forward, c.strafe) = crate::motor::brake(obs, facing);
                }
                c
            }
        };
        c.attack = d.fire || base.is_some_and(|b| b.attack && !legs);
        // The rocket launcher and the BFG don't fire again with the trigger held: it is let go a tic
        // after each press, which is the game's way to fire the next.
        if matches!(obs.weapon, Some(5 | 7)) && self.trigger.held {
            c.attack = false;
        }
        self.keep_line(obs, d, &mut c, legs || base.is_none());
        self.spare_mates(obs, &mut c);
        if d.fire {
            self.tap(obs, &mut c);
        }
        self.trigger.held = c.attack;
        // A shot begun, that is: the trigger pulled on a gun with too little in it begins none, and
        // must not be taken for one.
        if let Some(s) = obs
            .weapon
            .filter(|&s| c.attack && Self::ammo(obs, s) >= weapon_in(s, obs).ammo_per_shot)
        {
            self.trigger.last_shot = Some((s, obs.tic));
            if s == 5 {
                self.trigger.rocket_facing = Some(obs.angle);
            }
        }
        if d.weapon != 0 {
            c.weapon = d.weapon;
        }
        // (the ordered one of a slot's two, the plain shotgun or the super, the fist or the saw:
        // asked for by its own number)
        if let Some(want) = self.ordered_weapon {
            let slot = crate::dsda::weapon_slot(want);
            let chosen = if d.weapon != 0 {
                d.weapon
            } else {
                obs.weapon.unwrap_or(0)
            };
            if chosen == slot && obs.ready.is_some_and(|r| r != want) {
                c.weapon = slot;
                c.exact = Some(want);
            }
        }
        if crate::debug::on().pace && obs.tic % 10 == 0 {
            eprintln!("  fight tic {}: {:?} legs {legs}, base {:?}, out ({:.1}, {:.1}) turn {:.1}, facing {:.1}", obs.tic, d.kind, base.map(|b| (b.forward, b.strafe, b.turn)), c.forward, c.strafe, c.turn, obs.angle);
        }
        c
    }

    /// A bullet or a shell goes from where she is when it leaves the gun, a few tics after the
    /// trigger (the pistol's 4, the shotguns' 3: the frames before A_FirePistol, A_FireShotgun),
    /// not from where she pressed it. So the trigger isn't pulled when the speed she has would
    /// have her, by then, where there's no line to what she's shooting at. Her speed, not her
    /// legs: she can't stop in three tics, and not at all in the air coming down a stair. Once
    /// it's pulled, her legs are held those tics if where they're taking her has no line and
    /// where she stands has. Between shots her legs are her own: the rest of the gun's cycle is
    /// the time to get behind something. None of this applies when the move is one that can't
    /// wait (a missile coming, an arch-vile's fire).
    /// A co-op game: the trigger let go with another player where the shot would go: inside the
    /// gun's spread (and their width) short of what she's shooting at, or for the rocket and the
    /// BFG, near where it goes off.
    fn spare_mates(&mut self, obs: &Observation, c: &mut Controls) {
        /// a player's radius, and a little over for their moving
        const BODY: f64 = 16.0 + 8.0;
        /// a rocket's blast reaches this far (128), and a step over
        const BLAST: f64 = 128.0 + 32.0;
        /// the BFG's spray: 40 tracers over 90 degrees, 1024 units on from her
        const SPRAY: (f64, f64) = (45.0, 1024.0);
        if !c.attack || obs.mates.is_empty() {
            return;
        }
        let pos = obs.pos();
        let facing = obs.angle - c.turn;
        let target = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id))
            .map(|t| (t.x, t.y));
        let to_target = target.map_or(2048.0, |t| dist(pos, t));
        // (the spread either side: a bullet's, a shell's pellets; the super shotgun's twice that)
        let spread = match obs.ready {
            Some(8) => 11.2,
            Some(1..=3) => 5.6,
            _ => 0.0,
        };
        // (a missile's own width, and its time on the way: a teammate walking across its line in
        // the second a plasma ball takes over a big room is hit. Its radius, and its speed a tic)
        let missile = match obs.weapon {
            Some(5) => Some((11.0, 20.0)),
            Some(6 | 7) => Some((13.0, 25.0)),
            _ => None,
        };
        let in_line = |p: Point, d: f64, body: f64| {
            let bearing = (p.1 - pos.1).atan2(p.0 - pos.0).to_degrees();
            let off = norm(bearing - facing).abs();
            let width = (body / d.max(1.0)).atan().to_degrees();
            off <= spread + width && d < to_target + body
        };
        let in_the_way = obs.mates.iter().filter(|m| m.alive).any(|m| {
            let d = dist(pos, m.pos());
            let bearing = (m.y - pos.1).atan2(m.x - pos.0).to_degrees();
            let off = norm(bearing - facing).abs();
            let line = match missile {
                None => in_line(m.pos(), d, BODY),
                Some((radius, speed)) => {
                    let flight = d / speed;
                    let then = (m.x + m.vx * flight, m.y + m.vy * flight);
                    in_line(m.pos(), d, BODY + radius)
                        || in_line(then, dist(pos, then), BODY + radius)
                }
            };
            let blast = match obs.weapon {
                Some(5) => target.is_some_and(|t| dist(t, m.pos()) < BLAST) || d < BLAST,
                Some(7) => off <= SPRAY.0 && d < SPRAY.1,
                _ => false,
            };
            line || blast
        });
        if in_the_way {
            c.attack = false;
            *self
                .tally
                .entry("triggers not pulled: another player in the way".to_string())
                .or_insert(0.0) += 1.0;
        }
    }

    pub(super) fn keep_line(
        &mut self,
        obs: &Observation,
        d: &Decision,
        c: &mut Controls,
        legs: bool,
    ) {
        /// longer than this with no shot out, the gun isn't coming up to fire (a switch, say)
        const GIVEN_UP: i64 = 12;
        let Some(held) = obs.weapon.filter(|w| matches!(w, 2..=4) && !d.urgent) else {
            self.trigger.line_held = None;
            return;
        };
        if held == 4 {
            self.trigger.line_held = None;
            self.keep_burst(obs, c, legs);
            return;
        }
        let delay: i64 = if held == 2 { 4 } else { 3 };
        let since = self
            .trigger
            .shot_out
            .filter(|&(s, _)| s == held)
            .map_or(i64::MAX / 2, |(_, t)| obs.tic - t);
        let pressed = c.attack
            || self
                .trigger
                .last_shot
                .is_some_and(|(s, t)| s == held && obs.tic - t < delay);
        // The trigger held, the next shot leaves a cycle after the last.
        let again = weapon_in(held, obs).cycle as i64 - delay;
        let due = pressed && since >= again - 1;
        let target = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id).cloned());
        let (Some(t), true) = (target, due) else {
            self.trigger.line_held = None;
            return;
        };
        let pos = obs.pos();
        let reach = delay as f64;
        let h = mon(t.name).height;
        if !self.shot_clear(obs, (t.x, t.y), t.z, h) {
            self.trigger.line_held = None;
            return;
        }
        // The trigger not pulled yet, or held and the gun asking whether to fire again.
        let coast = (
            pos.0 + self.velocity.0 * reach,
            pos.1 + self.velocity.1 * reach,
        );
        if c.attack
            && d.fire
            && (!self.trigger.held || since <= again + 1)
            && !self.shot_clear_from(obs, coast, (t.x, t.y), t.z, h)
        {
            c.attack = false;
            *self
                .tally
                .entry("triggers not pulled: no line where she'd be by the shot".to_string())
                .or_insert(0.0) += 1.0;
            // And she slows for it: with a line to it and a mind to shoot, she stays in the line till
            // she can. Otherwise she is out and back round a corner with never a shot.
            if legs {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                *self
                    .tally
                    .entry("tics she slowed to get a shot off".to_string())
                    .or_insert(0.0) += 1.0;
            }
            self.trigger.line_held = None;
            return;
        }
        if !legs || c.forward.hypot(c.strafe) == 0.0 {
            self.trigger.line_held = None;
            return;
        }
        const STRIDE: f64 = 10.0; // (a tic's run, about, from a walk)
        let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
        let spot = (
            pos.0 + way.cos() * STRIDE * reach,
            pos.1 + way.sin() * STRIDE * reach,
        );
        if self.shot_clear_from(obs, spot, (t.x, t.y), t.z, h) {
            self.trigger.line_held = None;
            return;
        }
        let from = *self.trigger.line_held.get_or_insert(obs.tic);
        if obs.tic - from > GIVEN_UP {
            return;
        }
        (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
        *self
            .tally
            .entry("tics her legs held for a shot on its way out".to_string())
            .or_insert(0.0) += 1.0;
    }

    /// The trigger let go between shots at something far off. A bullet goes dead straight
    /// unless the trigger has been held since the shot before (P_GunShot's `accurate`: the
    /// player's `refire` is 0); held, it's up to 5.6 degrees off either way, most near the
    /// middle ((P_Random - P_Random) << 18). The gun asks once a cycle whether the trigger is
    /// still down (A_ReFire: the pistol 10 tics after its shot, the chaingun 8 after the first
    /// of its two). Let go for that tic, it comes back to ready and the next press is a first
    /// shot again: the pistol's cycle is then 19 tics for 14, the chaingun's 10 for 8. So she
    /// taps when holding would land less than that share of its bullets on a target as wide as
    /// this one is from here: the same damage a second or more, and every bullet on the target.
    pub(super) fn tap(&mut self, obs: &Observation, c: &mut Controls) {
        /// the most a held shot goes off, degrees
        const SPREAD: f64 = 5.6;
        // Tics from the press's first bullet to the gun's asking; a held cycle over a tapped one.
        let (asks, worth) = match obs.weapon {
            Some(2) => (10, 14.0 / 19.0),
            Some(4) => (8, 8.0 / 10.0),
            _ => return,
        };
        if !c.attack {
            return;
        }
        let Some(t) = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id))
        else {
            return;
        };
        let half = (mon(t.name).radius / dist((t.x, t.y), obs.pos()).max(1.0))
            .atan()
            .to_degrees();
        let lands = if half >= SPREAD {
            1.0
        } else {
            1.0 - (1.0 - half / SPREAD).powi(2)
        };
        if lands >= worth {
            return;
        }
        // She sees the bullet gone a tic after the gun has let it go, and what she does now is the
        // game's next tic.
        let since = obs.tic - self.trigger.burst_from;
        if (asks - 2..=asks).contains(&since) {
            c.attack = false;
            *self
                .tally
                .entry("tics the trigger let go for a straight shot".to_string())
                .or_insert(0.0) += 1.0;
        }
    }

    /// The chaingun's way of the same. It fires as long as the trigger is down, two bullets a
    /// press, the second 4 tics after the first whether she has let go or not (A_FireCGun,
    /// twice). So she fires a burst at a time. Where her speed would have her out of the line by
    /// the burst's end, the trigger isn't pulled and she slows for it. Once pulled, her legs
    /// wait the burst out if they'd take her out of the line, and no second burst is begun.
    /// After that they're her own for a while (`REST`), with the trigger let go where they take
    /// her out of the line: without that rest the legs wait for the shot and the shot for the
    /// legs, and she does neither.
    pub(super) fn keep_burst(&mut self, obs: &Observation, c: &mut Controls, legs: bool) {
        const BURST: i64 = 5;
        /// her legs are her own this long after a burst they waited for (tics)
        const REST: i64 = 12;
        let burst = self
            .trigger
            .last_shot
            .is_some_and(|(s, t)| s == 4 && obs.tic - t < BURST);
        if !c.attack && !burst {
            return;
        }
        let Some(t) = self
            .target
            .and_then(|id| self.visible.iter().find(|t| t.id == id).cloned())
        else {
            return;
        };
        let pos = obs.pos();
        let h = mon(t.name).height;
        if !self.shot_clear(obs, (t.x, t.y), t.z, h) {
            return;
        }
        let reach = BURST as f64;
        let coast = (
            pos.0 + self.velocity.0 * reach,
            pos.1 + self.velocity.1 * reach,
        );
        let out_coast = !self.shot_clear_from(obs, coast, (t.x, t.y), t.z, h);
        let out_legs = legs && c.forward.hypot(c.strafe) > 0.0 && {
            const STRIDE: f64 = 10.0;
            let way = (obs.angle - c.turn + (-c.strafe).atan2(c.forward).to_degrees()).to_radians();
            !self.shot_clear_from(
                obs,
                (
                    pos.0 + way.cos() * STRIDE * reach,
                    pos.1 + way.sin() * STRIDE * reach,
                ),
                (t.x, t.y),
                t.z,
                h,
            )
        };
        if !out_coast && !out_legs {
            return;
        }
        if obs.tic < self.trigger.burst_rest {
            c.attack = false;
            return;
        }
        if burst {
            // The burst is begun: no other after it, her legs waiting it out.
            c.attack = false;
            if legs {
                (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
                *self
                    .tally
                    .entry("tics her legs held for a shot on its way out".to_string())
                    .or_insert(0.0) += 1.0;
            }
            if self
                .trigger
                .last_shot
                .is_some_and(|(_, t)| obs.tic - t == BURST - 1)
            {
                self.trigger.burst_rest = obs.tic + 1 + REST;
            }
            return;
        }
        if out_coast {
            c.attack = false;
            *self
                .tally
                .entry("triggers not pulled: no line where she'd be by the shot".to_string())
                .or_insert(0.0) += 1.0;
        }
        if legs {
            (c.forward, c.strafe) = crate::motor::brake(obs, obs.angle - c.turn);
            *self
                .tally
                .entry("tics she slowed to get a shot off".to_string())
                .or_insert(0.0) += 1.0;
        }
    }
}
