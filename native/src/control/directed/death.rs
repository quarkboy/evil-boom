//! Her death, told as it came: the blow that killed her, and what had hurt her in the half
//! minute before it, each by what it was (a monster, its missile, her own rocket, a barrel and
//! who set it off, the floor, a crusher), as the game has every hit on her.

use std::collections::{BTreeMap, VecDeque};

use super::{called, Director, Event, Report};
use crate::dsda::{type_name, Hit, HitCause};

/// what hurt her this long (tics) before her death is part of its story
const STORY: i64 = 30 * 35;
/// the things before the last blow told, at most
const STORY_MOST: usize = 3;

/// "an imp", "a mancubus": a thing with its article, lower case.
fn one(name: &str) -> String {
    let called = called(name);
    let an = called.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {called}", if an { "an" } else { "a" })
}

/// What a hit came from, in words: "an imp's fireball", "a shotgun guy", "your own rocket",
/// "a barrel an imp set off", "the floor".
pub fn hurt_by(h: &Hit) -> String {
    match h.cause {
        HitCause::Crusher => return "a crusher".into(),
        HitCause::Floor => return "the floor".into(),
        HitCause::Other => return "the level".into(),
        HitCause::Thing => {}
    }
    let name = |k: Option<i32>| k.and_then(type_name);
    let (thing, source) = (name(h.inflictor), name(h.source));
    match (thing, source) {
        (Some("ExplosiveBarrel"), _) if h.own => "a barrel you set off".into(),
        (Some("ExplosiveBarrel"), Some(s)) if s != "ExplosiveBarrel" && s != "DoomPlayer" => {
            format!("a barrel {} set off", one(s))
        }
        (Some("ExplosiveBarrel"), _) => "a barrel".into(),
        (Some(t), _) if h.own => format!("your own {}", called(t)),
        (Some(t), Some(s)) if t != s => format!("{}'s {}", one(s), called(t)),
        (Some(t), _) => one(t),
        (None, Some(s)) => one(s),
        (None, None) => "something".into(),
    }
}

impl Director {
    /// The hits on her this tic, kept for her death's story (the last half minute).
    pub(super) fn remember_hits(&mut self, tic: i64, hits: &[Hit]) {
        for h in hits.iter().filter(|h| h.health > 0) {
            self.hurt.push_back((tic, hurt_by(h), h.health));
        }
        while self.hurt.front().is_some_and(|(t, _, _)| tic - t > STORY) {
            self.hurt.pop_front();
        }
    }

    /// Her death as it came: the last blow, then what had hurt her before it, most first.
    pub(super) fn death_story(&self) -> String {
        let mut hurt: VecDeque<(i64, String, i32)> = self.hurt.clone();
        let Some((_, last, took)) = hurt.pop_back() else {
            return "You died.".into();
        };
        let mut before: BTreeMap<String, (usize, i32)> = BTreeMap::new();
        for (_, by, health) in hurt {
            let e = before.entry(by).or_insert((0, 0));
            *e = (e.0 + 1, e.1 + health);
        }
        let mut before: Vec<(String, (usize, i32))> = before.into_iter().collect();
        before.sort_by_key(|b| std::cmp::Reverse(b.1 .1));
        let earlier: Vec<String> = before
            .iter()
            .take(STORY_MOST)
            .map(|(by, (n, health))| {
                if *n > 1 {
                    format!("{by} x{n} (-{health})")
                } else {
                    format!("{by} (-{health})")
                }
            })
            .collect();
        if earlier.is_empty() {
            format!("You died: {last} (-{took}).")
        } else {
            format!(
                "You died: {last} (-{took}). Before it, in {} s: {}.",
                STORY / 35,
                earlier.join(", ")
            )
        }
    }

    /// She's died (`hits`: those the game hasn't passed on yet, the last blow among them):
    /// her death told, once, as it came; `again`, the level begins again, her orders and her
    /// say in the fight the life before's. (No lives in what she's told: on stream the game goes
    /// on as the game itself does, again and again; lives are a test's.)
    pub fn died(&mut self, tic: i64, again: bool, hits: &[Hit]) {
        self.remember_hits(tic, hits);
        if !self.told_dead {
            self.told_dead = true;
            let story = self.death_story();
            let more = if again {
                " The level again from its start, with what you came in with."
            } else {
                ""
            };
            self.reports.push(Report {
                tic,
                ok: false,
                text: format!("{story}{more}"),
                event: if again { Event::DiedAgain } else { Event::Died },
            });
        }
        if again {
            self.new_life();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hurt_by;
    use crate::dsda::{type_of, Hit, HitCause};

    fn hit(inflictor: &str, source: &str, own: bool) -> Hit {
        Hit {
            health: 10,
            armor: 0,
            inflictor: type_of(inflictor),
            source: type_of(source),
            own,
            cause: HitCause::Thing,
            side: None,
            source_id: None,
        }
    }

    /// What hurt her, in words.
    #[test]
    fn hurt_in_words() {
        assert_eq!(
            hurt_by(&hit("Rocket", "DoomPlayer", true)),
            "your own rocket"
        );
        assert_eq!(
            hurt_by(&hit("DoomImpBall", "DoomImp", false)),
            "an imp's fireball"
        );
        assert_eq!(
            hurt_by(&hit("ShotgunGuy", "ShotgunGuy", false)),
            "a shotgun guy"
        );
        assert_eq!(
            hurt_by(&hit("ExplosiveBarrel", "DoomImp", false)),
            "a barrel an imp set off"
        );
        assert_eq!(
            hurt_by(&hit("ExplosiveBarrel", "DoomPlayer", true)),
            "a barrel you set off"
        );
    }
}
