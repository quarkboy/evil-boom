//! How things stand this tic, worked out once (`Combat::observe`'s last step) and read from
//! `Combat::now` by whatever decides anything after: the fight's tree, the retreats, the errands,
//! the explorer's hooks. One answer a tic to "is she outmatched", "what shoots at her", "is she
//! short of ammo", however many rules ask; and the one place that says what a decision was made
//! on (what a log of decisions records, what a decision model is shown).
//!
//! Only what follows from what she's seen and what she has: nothing a decision made this tic
//! changes. What depends on the fight's own clocks (`armed`: a monster given up on is out of her
//! fist's reach) is asked of the fight, which reads the rest from here. A question about what she
//! doesn't have yet ("with that gun in hand, could she?") is asked of the arsenal with the
//! observation it supposes, not of this.

use super::arsenal::Odds;
use super::{mon, spread, Combat};
use crate::geom::{dist, Point};
use crate::motor::{Observation, Thing};
use crate::tactical::bestiary::Attack;

/// How things stand, this tic.
#[derive(Clone, Debug)]
pub struct Assessment {
    pub tic: i64,
    pub pos: Point,
    pub health: f64,
    /// invulnerable, with time enough left to make something of it
    pub invulnerable: bool,
    /// the monsters in sight that are after her (not each other)
    pub against_her: Vec<Thing>,
    /// of those, what shoots: bullets, missiles, an arch-vile's fire
    pub shooters: Vec<Thing>,
    /// of those in sight, what she's in a fight with (`Combat::engaged`)
    pub engaged: Vec<Thing>,
    /// what's against her would do to her, about (damage a second)
    pub threat: f64,
    /// what her best gun with ammo does (damage a second)
    pub firepower: f64,
    /// a gun with ammo for it (more than the fist)
    pub guns_armed: bool,
    /// next to nothing left to shoot with
    pub short_of_ammo: bool,
    /// her odds against what's in sight
    pub odds: Odds,
    /// those odds too long: more than `guns.outmatched` seconds of killing, more than she'd
    /// last, or more than her ammo
    pub outmatched: bool,
    /// several near her, round her rather than all one side
    pub surrounded: bool,
    /// an arch-vile in sight she can't kill in a moment
    pub vile: bool,
    /// health lost a second, lately
    pub damage_rate: f64,
    /// seconds till she's dead at that rate (a lot: not losing any)
    pub time_to_death: f64,
}

impl Default for Assessment {
    fn default() -> Self {
        Assessment {
            tic: i64::MIN,
            pos: (0.0, 0.0),
            health: 100.0,
            invulnerable: false,
            against_her: Vec::new(),
            shooters: Vec::new(),
            engaged: Vec::new(),
            threat: 0.0,
            firepower: 0.0,
            guns_armed: false,
            short_of_ammo: false,
            odds: Odds { unhurt: false, kill_secs: 0.0, last_secs: f64::INFINITY, fists: false, ammo_short: false },
            outmatched: false,
            surrounded: false,
            vile: false,
            damage_rate: 0.0,
            time_to_death: f64::INFINITY,
        }
    }
}

impl Assessment {
    /// For a decision's record (and a decision model's eyes): the numbers, and what's against
    /// her by name, distance and health, nearest first.
    pub fn summary(&self) -> serde_json::Value {
        let mut them: Vec<&Thing> = self.against_her.iter().collect();
        them.sort_by(|a, b| dist((a.x, a.y), self.pos).total_cmp(&dist((b.x, b.y), self.pos)));
        let finite = |x: f64| if x.is_finite() { serde_json::json!((x * 10.0).round() / 10.0) } else { serde_json::Value::Null };
        serde_json::json!({
            "health": self.health,
            "invulnerable": self.invulnerable,
            "threat": self.threat,
            "firepower": finite(self.firepower),
            "guns_armed": self.guns_armed,
            "short_of_ammo": self.short_of_ammo,
            "kill_secs": finite(self.odds.kill_secs),
            "last_secs": finite(self.odds.last_secs),
            "ammo_short": self.odds.ammo_short,
            "outmatched": self.outmatched,
            "surrounded": self.surrounded,
            "vile": self.vile,
            "damage_rate": finite(self.damage_rate),
            "time_to_death": finite(self.time_to_death),
            "shooters": self.shooters.len(),
            "engaged": self.engaged.len(),
            "against_her": them.iter().map(|t| serde_json::json!({"id": t.id, "name": t.name, "away": dist((t.x, t.y), self.pos).round(), "health": t.health})).collect::<Vec<_>>(),
        })
    }

    /// How things stand in words, for a decision model (short: its time goes by the words).
    /// What's against her is named as the questions name it (`m` and its id).
    pub fn words(&self, armor: f64) -> String {
        let mut them: Vec<&Thing> = self.against_her.iter().collect();
        them.sort_by(|a, b| dist((a.x, a.y), self.pos).total_cmp(&dist((b.x, b.y), self.pos)));
        let mut out = format!("Health {:.0}, armor {armor:.0}. Her best gun does {:.0} damage a second; {}.", self.health, self.firepower.max(0.0), if self.short_of_ammo { "nearly out of ammo" } else { "ammo enough" });
        if self.damage_rate > 0.0 {
            out += &format!(" Losing {:.0} health a second.", self.damage_rate);
        }
        if self.outmatched {
            out += " What's in sight is more than she can kill as she is.";
        }
        if self.surrounded {
            out += " They're round her on several sides.";
        }
        out += "\nAgainst her:";
        for t in them {
            out += &format!(" m{} {}, {:.0} away{};", t.id, t.name, dist((t.x, t.y), self.pos), if t.attack >= 0 { ", attacking" } else { "" });
        }
        out
    }

    /// A level started again: nothing in sight any more (the rest stands till she next looks).
    pub(super) fn nothing_in_sight(&mut self) {
        self.against_her.clear();
        self.shooters.clear();
        self.engaged.clear();
        self.threat = 0.0;
        self.invulnerable = false;
        self.outmatched = false;
        self.surrounded = false;
        self.vile = false;
    }
}

impl Combat {
    /// How things stand, from what she's just seen (`observe`) and what she has.
    pub(super) fn assess(&mut self, obs: &Observation) -> Assessment {
        let pos = obs.pos();
        let against_her = self.seen_against_her();
        let shooters = against_her.iter().filter(|t| matches!(mon(t.name).attack, Attack::Hitscan | Attack::Missile | Attack::Fire)).cloned().collect();
        let firepower = self.firepower(obs);
        let damage_rate = self.damage_rate(obs.tic);
        let health = obs.health.unwrap_or(100.0);
        let odds = self.odds_in_sight(obs);
        Assessment {
            tic: obs.tic,
            pos,
            health,
            invulnerable: self.invulnerable(obs.tic),
            shooters,
            engaged: self.engaged(obs),
            threat: against_her.iter().map(|t| mon(t.name).threat).sum(),
            firepower,
            guns_armed: self.guns_armed(obs),
            short_of_ammo: self.short_of_ammo(obs),
            odds,
            outmatched: odds.too_much(self.tune.guns.outmatched),
            surrounded: self.surrounded(pos, &against_her),
            vile: against_her.iter().any(|t| t.name == "Archvile" && f64::from(mon(t.name).health) / firepower > self.tune.retreat.vile_kill),
            damage_rate,
            time_to_death: if damage_rate <= 0.0 { f64::INFINITY } else { health / damage_rate },
            against_her,
        }
    }

    /// Several monsters near her, and round her rather than all one side.
    fn surrounded(&self, pos: Point, against_her: &[Thing]) -> bool {
        let t = &self.tune.retreat;
        let near: Vec<Point> = against_her.iter().filter(|m| dist((m.x, m.y), pos) < t.surrounded_near && mon(m.name).attack != Attack::None).map(|m| (m.x, m.y)).collect();
        near.len() >= t.surrounded_count && spread(pos, &near) > t.surrounded_spread
    }
}
