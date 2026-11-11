//! How to fight with it: the style a weapon's fought with at a monster (up close or from a
//! distance), the distance she'd fight it from (its window), and how far the weapon reaches.

use super::{mon, Combat};
use crate::motor::Thing;
use crate::tactical::bestiary::{melee_at, Attack, Weapon};

/// How she fights a monster with a weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// straight up to it, then standing at it hitting: the fist berserk at what it'd punch, the
    /// chainsaw at what it'd keep flinching (`bestiary::melee_at`)
    Melee,
    /// from the gun's window, the steering keeping her there (and the fist or chainsaw at what's
    /// come right up to her)
    Ranged,
}

impl Style {
    pub fn of(w: &Weapon, t: &Thing) -> Style {
        if melee_at(w, t.name) {
            Style::Melee
        } else {
            Style::Ranged
        }
    }
}

/// How near to t she'd fight it from with w, and how far: the gun's own (its spread, its blast)
/// and what t does up close (claws, bites; a cyberdemon's rockets too quick to dodge). Up close
/// (`Style::Melee`): right up to it, whatever its claws.
pub fn window(w: &Weapon, t: &Thing) -> (f64, f64) {
    if Style::of(w, t) == Style::Melee {
        return w.keep;
    }
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

/// a man's size, as the game has its zombies and imps: (radius, height)
const MAN: (f64, f64) = (20.0, 56.0);

impl Combat {
    /// Close enough to shoot at with w: inside its reach, or (at something that hurts her from
    /// anywhere) as far as enough of a shot still lands.
    pub fn in_reach(&self, w: &Weapon, t: &Thing, d: f64) -> bool {
        let info = mon(t.name);
        if d >= self.tune.engage.range {
            return false;
        }
        if d < w.reach {
            return true;
        }
        info.attack == Attack::Hitscan
            && w.slot != 1
            && w.hits(info.radius, d) >= self.tune.engage.far_hits
    }

    /// Worth a shot at with w from here, whether or not it's near enough to fight: in its
    /// reach, or wide enough that as much of the shot lands as would on a man at its reach.
    /// For the trigger only: how she stands to it (closes in, goes on by, keeps clear) goes by
    /// the gun's reach as before. (Taken for in reach outright, MAP32 on the easiest skill: the
    /// cyberdemon 1400 off across the hall was no longer "out of reach: going on" but closed in
    /// on with a pistol; 5 exits of 16 became 1.)
    pub fn worth_a_shot(&self, w: &Weapon, t: &Thing, d: f64) -> bool {
        if self.in_reach(w, t, d) {
            return true;
        }
        let info = mon(t.name);
        // (not at what's too much for her as she stands: a far shot at it does it nothing and
        // brings it on. MAP32 on the easiest skill, the same 16 runs: the cyberdemon plinked at
        // from across the hall with a pistol, 5 exits became 1 whichever way she stood to it)
        if d >= self.tune.engage.range || self.now.outmatched {
            return false;
        }
        // (a gun's reach is how far its spread's worth a shot at something the size of a man;
        // at something wider it's worth it from farther: as far as as much of the shot lands
        // as lands on a man at its reach. The user's: "doesn't the SSG reach at that distance,
        // just not optimally?" MAP07: an arachnotron 655 off, 128 across, a little under half
        // the pellets on it from there; "out of reach" at the super shotgun's 480, dodged
        // before for 500 s with 76 shells and not a shot)
        w.hitscan
            && w.spread > 0.0
            && w.slot != 1
            && w.lands(info.radius, info.height, d) >= w.lands(MAN.0, MAN.1, w.reach)
    }
}
