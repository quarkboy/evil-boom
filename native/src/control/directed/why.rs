//! `doom_why`: why she's doing what she's doing, from what the bot has worked out this tic (the
//! SDK plan's 4.1): her action in hand, or on instinct the objective and what it was chosen over;
//! the fight, what it's doing and what pushed it; why she isn't shooting; the last action that
//! didn't work. In plain words, as everything she reads.

use super::{engaged, explores, fighting, id_of, plain, Director, Doing, Entry, Event};
use crate::knowledge::explorer::{Kind, Objective};
use crate::motor::Observation;
use crate::play::Agent;

/// a failed action this recent is told as the last that didn't work (tics)
const FAILED_LATELY: i64 = 60 * 35;

impl Director {
    /// Why, in a few lines.
    pub fn why(&mut self, agent: &mut Agent, obs: &Observation) -> String {
        let mut out = Vec::new();
        // (what she ordered she knows: only what the bot chose by itself is said, the objective
        // it's on, on instinct or inside an explore of hers; and that she's waiting, if she is)
        if let Some(g) = self.goal_label() {
            out.push(format!("Your goal: {g}."));
        }
        match &self.doing {
            None if self.auto_spent => {
                out.push("Nothing more to do here by yourself: it's yours to say.".to_string());
            }
            None if agent.x().current.is_none() => {
                out.push("Waiting for your next action.".to_string());
            }
            Some(Doing::Off) => out.push("Getting off a floor that hurts.".to_string()),
            _ => {}
        }
        // (the bot's own choice: the objective's reason, and the next it was chosen over)
        if matches!(
            self.doing,
            Some(Doing::Auto | Doing::Area(..) | Doing::Way(..))
        ) {
            if let Some(o) = agent.x().current.clone() {
                let open = self.open_objectives(agent, obs);
                // (by the id she'd act on, what's to explore by its area: each once, and not the
                // one in hand's)
                let mut seen = vec![self.pick_of(agent, obs, &o, String::new()).id];
                let mut over: Vec<String> = Vec::new();
                for p in open.iter().filter(|p| explores(p) && id_of(p) != id_of(&o)) {
                    let pick = self.pick_of(agent, obs, p, String::new());
                    if seen.contains(&pick.id) {
                        continue;
                    }
                    seen.push(pick.id.clone());
                    let e = Entry {
                        id: pick.id,
                        what: String::new(),
                        at: pick.at,
                    };
                    over.push(self.describe(&e, obs));
                    if over.len() == 2 {
                        break;
                    }
                }
                // (what the bot calls it, without its own reason in brackets: the "because" says it)
                let what = plain(o.kind, &o.label);
                let what = what.split(" (").next().unwrap_or(&what);
                let mut line = format!("{what} {}", because(&o, obs));
                if !over.is_empty() {
                    line.push_str(&format!(", rather than {}", over.join(" or ")));
                }
                out.push(format!("{}{}.", line[..1].to_uppercase(), &line[1..]));
            }
        }
        // (the fight: how it stands, what it's doing, what with)
        if self.now.fighting() {
            let mut how = Vec::new();
            for (is, what) in [
                (self.now.outmatched, "outmatched"),
                (self.now.surrounded, "surrounded"),
                (self.now.short_of_ammo, "short of ammo"),
                (self.now.vile, "up against an arch-vile"),
            ] {
                if is {
                    how.push(what);
                }
            }
            if self.now.health < 50.0 {
                how.push("hurt");
            }
            let against = fighting(&engaged(agent, obs), obs).unwrap_or_default();
            let doing = match self.now.doing() {
                "fighting it out" => "standing and fighting",
                "pushing on, shooting on the way" => "pushing on and shooting on the way",
                "getting out of the way" => "getting out of the way of a shot",
                other => other,
            };
            let mut line = format!("You're fighting {against}: {doing}");
            if !how.is_empty() {
                line.push_str(&format!(", because you're {}", how.join(" and ")));
            }
            out.push(format!("{line}."));
            if let Some(why) = agent.player().and_then(|p| p.combat().holding_fire()) {
                // (the fight's own words, said to her)
                let why = why.replace("by her", "near you");
                out.push(format!("You're not shooting: {why}."));
            }
        }
        // (the last action that didn't work, if lately)
        if let Some(r) = self
            .reports
            .iter()
            .rev()
            .find(|r| !r.ok && r.event != Event::Replaced && obs.tic - r.tic <= FAILED_LATELY)
        {
            out.push(format!(
                "The last that didn't work ({} s ago): {}",
                (obs.tic - r.tic) / 35,
                r.text
            ));
        }
        out.join("\n")
    }
}

/// Why the bot's after an objective, as she'd say it to herself ("because you need ammo").
fn because(o: &Objective, obs: &Observation) -> String {
    let thing = o
        .thing
        .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n));
    let needed = |what: &str| format!("because you need {what}");
    match o.kind {
        Kind::Item if o.needed => match thing.map(|t| t.category) {
            Some("Ammo") => needed("ammo"),
            Some("Health") => needed("health"),
            Some("Armor") => needed("armor"),
            Some("Weapon") => "because it's a better gun".to_string(),
            _ => needed("it"),
        },
        Kind::Item => "because it's worth the trip".to_string(),
        Kind::Key => "because you don't have it yet".to_string(),
        Kind::Door => "because you haven't tried it".to_string(),
        Kind::Switch => "because you haven't pressed it".to_string(),
        Kind::Frontier | Kind::Room | Kind::Visit => {
            "because it's the nearest place you haven't explored".to_string()
        }
        Kind::Recheck => "to take another look".to_string(),
        Kind::Noise => "because you heard something there".to_string(),
        Kind::Teleporter => "because you haven't taken it".to_string(),
        Kind::Hunt => "because there are monsters left".to_string(),
        Kind::Wall | Kind::Secret => "to search for a secret".to_string(),
        Kind::Barrel => "so it can't go off by you".to_string(),
        Kind::Exit => "because it's the way out".to_string(),
    }
}
