// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! What she hears and what she does about it: a shot, a monster waking, a hit from where
//! she can't see; the look round that answers it.

use super::*;

impl Combat {
    /// What she makes of this tic's sounds (the game's, as she'd hear them: `Observation::sounds`).
    /// A death cry: if it's the cry of a death she just saw, that; else one of the kinds that
    /// cry so has died, about that far off on that side: the one she knows of, out of sight,
    /// last seen where the cry fits is taken to be it (none fits: she can't tell whose). An
    /// attack: kept a moment, for whether something out of sight is firing.
    pub(super) fn hear(&mut self, obs: &Observation) {
        let (tune, pos) = (Rc::clone(&self.tune), obs.pos());
        self.senses
            .cries_due
            .retain(|&(tic, _)| obs.tic - tic <= tune.senses.cry_due);
        self.senses
            .attacks_heard
            .retain(|&(tic, _)| obs.tic - tic <= tune.senses.heard_firing);
        for s in &obs.sounds {
            match s.what {
                Heard::Attack => self.senses.attacks_heard.push((obs.tic, s.kinds)),
                // A monster's own steps. The few kinds that make any are heard all the while they walk, a
                // step at a time, and say about where it is: as far off as the step is loud, on the side it
                // came from. The one she knows of, of that kind, out of her sight, is put there: of the two
                // places the sound could be (in front and behind sound the same), the one nearer where she
                // had it.
                Heard::Walking if !s.own && self.tune.senses.track_steps => {
                    let far = if s.volume >= 127 {
                        160.0
                    } else {
                        1200.0 - f64::from(s.volume) * 1040.0 / 127.0
                    };
                    let off = ((128.0 - f64::from(s.separation)) / 96.0)
                        .clamp(-1.0, 1.0)
                        .asin()
                        .to_degrees();
                    let at = |way: f64| {
                        (
                            pos.0 + far * way.to_radians().cos(),
                            pos.1 + far * way.to_radians().sin(),
                        )
                    };
                    let places = [at(obs.angle + off), at(obs.angle + 180.0 - off)];
                    let whose = self
                        .known
                        .values()
                        .filter(|k| {
                            s.kinds.contains(&k.name)
                                && !self.dead.contains(&k.id)
                                && !self.visible.iter().any(|t| t.id == k.id)
                        })
                        .flat_map(|k| places.iter().map(move |&p| (dist(p, k.pos()), k.id, p)))
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    if let Some((_, id, p)) = whose {
                        if let Some(k) = self.known.get_mut(&id) {
                            (k.x, k.y, k.tic, k.lost, k.awake, k.going) =
                                (p.0, p.1, obs.tic, false, true, (0.0, 0.0));
                        }
                        *self.tally.entry("steps heard".to_string()).or_insert(0.0) += 1.0;
                    }
                }
                Heard::Death => {
                    if let Some(i) = self
                        .senses
                        .cries_due
                        .iter()
                        .position(|(_, kind)| s.kinds.contains(kind))
                    {
                        self.senses.cries_due.remove(i);
                        continue;
                    }
                    // How far off by how loud: full within 160, nothing at 1200. A boss's cry is as loud
                    // anywhere and says nothing of where.
                    let far = if s.volume >= 127 {
                        160.0
                    } else {
                        1200.0 - f64::from(s.volume) * 1040.0 / 127.0
                    };
                    let fits = |k: &Seen| {
                        if s.own {
                            return Some(0.0);
                        }
                        let d = dist(k.pos(), pos);
                        let off_by = if s.volume >= 127 {
                            (d - far).max(0.0)
                        } else {
                            (d - far).abs()
                        };
                        let side = 128.0
                            - 96.0 * norm(heading(pos, k.pos()) - obs.angle).to_radians().sin();
                        let side_off = (side - f64::from(s.separation)).abs();
                        (off_by <= tune.senses.heard_fit && side_off <= tune.senses.heard_side)
                            .then_some(
                                off_by / tune.senses.heard_fit + side_off / tune.senses.heard_side,
                            )
                    };
                    let whose = self
                        .known
                        .values()
                        .filter(|k| {
                            s.kinds.contains(&k.name)
                                && !self.dead.contains(&k.id)
                                && !self.visible.iter().any(|t| t.id == k.id)
                        })
                        .filter_map(|k| fits(k).map(|f| (f, k.id, k.name)))
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    if let Some((_, id, name)) = whose {
                        self.dead.insert(id);
                        self.kills += 1;
                        self.kind_died.insert(name, obs.tic);
                        self.log
                            .push(format!("tic {}: {name} {id} dead (heard)", obs.tic));
                        self.count("deaths heard", 1.0);
                    }
                }
                _ => {}
            }
        }
    }

    /// Something to look at, `ways` round (world degrees, the likeliest first).
    pub(super) fn alerted(&mut self, obs: &Observation, ways: Vec<f64>, hit: bool, why: &str) {
        // One thing at a time. What she's looking round for is looked round for before the next,
        // its ways reckoned from where she faced when it came: a second sound reckoned from mid-turn
        // would have her turning to and fro and never round. Only a hit takes her from a sound.
        if ways.is_empty() || self.senses.alert.as_ref().is_some_and(|a| a.hit || !hit) {
            return;
        }
        let side = |w: f64| match norm(w - obs.angle) {
            d if d.abs() > 135.0 => "behind her",
            d if d > 0.0 => "to her left",
            _ => "to her right",
        };
        self.log
            .push(format!("tic {}: {why}: looking {}", obs.tic, side(ways[0])));
        self.count(&format!("alert: {why}"), 1.0);
        self.journal.note(|| Record {
            detail: why.to_string(),
            ..Record::new(obs.tic, "alert", side(ways[0]).to_string())
        });
        self.senses.alert = Some(Alert {
            until: obs.tic + self.tune.senses.alert_tics,
            ways,
            hit,
            in_sight: self.visible.iter().map(|t| t.id).collect(),
        });
    }

    /// What tells her to look round, this tic. A sound from nothing she can see (a missile
    /// fired, a monster's cry on seeing her, an attack): its side by the balance between her
    /// ears, which is the same for in front and behind, so in front only if that's out of her
    /// view. A hit: the way the status bar's face looked, to one side (straight at it, she's
    /// looking there). Not a wandering monster's noise: there's always one.
    pub(super) fn heed(&mut self, obs: &Observation) {
        let fov = self.tune.senses.fov;
        let (heed, heed_active) = (
            self.tune.senses.heed_volume,
            self.tune.senses.heed_active_volume,
        );
        let loud = |s: &crate::domain::Sound| match s.what {
            Heard::Launch | Heard::Sight | Heard::Attack => s.volume >= heed,
            Heard::Active => s.volume >= heed_active,
            _ => false,
        };
        for s in obs.sounds.iter().filter(|s| !s.own && loud(s)) {
            let seen = if s.what == Heard::Launch {
                obs.things
                    .iter()
                    .flatten()
                    .any(|t| self.missiles_seen.contains(&t.id) && s.kinds.contains(&t.name))
            } else {
                self.visible.iter().any(|t| s.kinds.contains(&t.name))
            };
            if seen {
                continue;
            }
            // The balance: 128 less 96 times the sine of its bearing, left of her counted up.
            let off = ((128.0 - f64::from(s.separation)) / 96.0)
                .clamp(-1.0, 1.0)
                .asin()
                .to_degrees();
            let (front, back) = (obs.angle + off, obs.angle + 180.0 - off);
            let ways = if off.abs() < fov {
                vec![back]
            } else {
                vec![front, back]
            };
            let what = match s.what {
                Heard::Launch => "a shot fired",
                Heard::Sight => "something's seen her",
                Heard::Active => "something close",
                _ => "an attack",
            };
            self.alerted(obs, ways, false, &format!("heard {what}"));
        }
        for &side in obs.hit_sides.iter().filter(|&&side| side != 0) {
            // Her right is the lesser angle. The face's side is anywhere from 45 degrees off to right
            // behind: to that side first, then on round behind.
            let way = obs.angle - 90.0 * f64::from(side);
            let known = self.visible.iter().any(|t| {
                let rel = norm(heading(obs.pos(), (t.x, t.y)) - obs.angle);
                rel.abs() > 45.0 && (rel < 0.0) == (side > 0)
            });
            if !known {
                self.alerted(obs, vec![way, obs.angle + 180.0], true, "hit from the side");
                // And off the other way a moment, before anything else: what clawed her from
                // out of her sight claws again while she walks on beside it.
                let away = (way + 180.0).to_radians();
                self.senses.flinch = Some((
                    obs.tic + self.tune.senses.flinch_tics,
                    (away.cos(), away.sin()),
                ));
            }
        }
    }

    /// Walking a route with nothing to shoot and nothing to look at: a look to either side every
    /// so often, so that what comes up beside her as she travels is seen before it's on her.
    pub(super) fn glance(&mut self, obs: &Observation) {
        let every = self.tune.senses.glance_every;
        if every <= 0
            || !self.route_under_way
            || self.senses.alert.is_some()
            || obs.tic - self.senses.glanced < every
        {
            return;
        }
        self.senses.glanced = obs.tic;
        let by = self.tune.senses.glance_angle;
        self.count("glances", 1.0);
        self.senses.alert = Some(Alert {
            until: obs.tic + self.tune.senses.alert_tics,
            ways: vec![obs.angle + by, obs.angle - by],
            hit: false,
            in_sight: self.visible.iter().map(|t| t.id).collect(),
        });
    }

    /// One she remembers, close, not looked at for a while: round to where she last saw it.
    pub(super) fn check(&mut self, obs: &Observation) {
        let (every, within) = (self.tune.senses.check_every, self.tune.senses.check_within);
        if every <= 0 || self.senses.alert.is_some() || obs.tic - self.senses.checked < every {
            return;
        }
        let pos = obs.pos();
        let Some(way) = self
            .now
            .remembered
            .iter()
            .map(|t| (dist((t.x, t.y), pos), t))
            .filter(|&(d, _)| d < within)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, t)| heading(pos, (t.x, t.y)))
        else {
            return;
        };
        self.senses.checked = obs.tic;
        self.count("checks", 1.0);
        self.senses.alert = Some(Alert {
            until: obs.tic + self.tune.senses.alert_tics,
            ways: vec![way],
            hit: true,
            in_sight: self.visible.iter().map(|t| t.id).collect(),
        });
    }

    /// For measuring a run: was she going towards the monster with that id, away from it, or
    /// about still, by her own speed along the line to it.
    pub fn measure_approach(&self, pos: Point, id: i32) -> &'static str {
        let Some(t) = self.senses.game_monsters.get(&id) else {
            return "gone";
        };
        let d = dist((t.x, t.y), pos).max(1.0);
        let along = (self.velocity.0 * (t.x - pos.0) + self.velocity.1 * (t.y - pos.1)) / d;
        match along {
            a if a > 2.0 => "going towards it",
            a if a < -2.0 => "going away from it",
            _ => "about still",
        }
    }

    /// Stopped while pushing `way` (world degrees) where the map has room: something's there.
    pub fn bumped(&mut self, obs: &Observation, way: f64) {
        let d = (way.to_radians().cos(), way.to_radians().sin());
        let reach = 3.0 * PLAYER_RADIUS;
        if norm(way - obs.angle).abs() > self.tune.senses.fov
            && self.clear_run(obs, d, reach) >= reach - 1.0
        {
            self.alerted(obs, vec![way], false, "stopped against something");
        }
    }

    /// The way to turn and look, if there's something to look at (`hit_only`: only for a hit:
    /// she has something to shoot). Looked each way, or something new in sight, or time up:
    /// over.
    pub(super) fn look(&mut self, obs: &Observation, hit_only: bool) -> Option<f64> {
        let looked = self.tune.senses.looked;
        let a = self.senses.alert.as_mut()?;
        while a
            .ways
            .first()
            .is_some_and(|&w| norm(w - obs.angle).abs() < looked)
        {
            a.ways.remove(0);
        }
        if obs.tic >= a.until
            || a.ways.is_empty()
            || self.visible.iter().any(|t| !a.in_sight.contains(&t.id))
        {
            self.senses.alert = None;
            return None;
        }
        (a.hit || !hit_only).then(|| a.ways[0])
    }
}
