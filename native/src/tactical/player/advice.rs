// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The advisor (an outside model asked for a stance and a target): set up and consulted.

use super::*;

impl Player {
    /// A decision model to ask (see `crate::advisor`).
    pub fn advise(&mut self, advisor: Advisor) {
        self.advisor = Some(advisor);
    }

    /// The advisor's answers that are due, taken; then what there is to ask it, asked. An
    /// answer holds a while (`hold_tics`) and lapses: the rules', and the tuning she came with.
    pub(super) fn consult(&mut self, obs: &Observation) {
        let Some(advisor) = self.advisor.as_mut() else {
            return;
        };
        let (hold, every) = (
            advisor.config.hold_tics,
            (advisor.config.target_every, advisor.config.stance_every),
        );
        let mut said: Vec<String> = std::mem::take(&mut advisor.errors);
        for a in advisor.due(obs.tic) {
            match a.point {
                "target" => {
                    let Some(id) = a
                        .choice
                        .strip_prefix('m')
                        .and_then(|n| n.parse::<i32>().ok())
                    else {
                        continue;
                    };
                    self.fight.combat.forced_target = Some(id);
                    self.target_until = obs.tic + hold;
                }
                "stance" => {
                    let Some(stance) = advisor.config.stances.iter().find(|s| s.name == a.choice)
                    else {
                        continue;
                    };
                    match self.base_tune.with(&stance.tuning) {
                        Ok(t) => {
                            let t = Rc::new(t);
                            self.fight.combat.tune = Rc::clone(&t);
                            self.fight.tune = t;
                            self.stance = (stance.name.clone(), obs.tic + hold);
                        }
                        Err(e) => {
                            said.push(format!("the {} stance: {e}", stance.name));
                            continue;
                        }
                    }
                }
                _ => continue,
            }
            said.push(format!(
                "advised (asked at tic {}): {} {}, {:.0}% sure",
                a.asked,
                a.point,
                a.choice,
                a.share * 100.0
            ));
            self.explorer.journal.note(|| Record {
                detail: format!("{:.2}", a.share),
                ..Record::new(obs.tic, "advised", format!("{} {}", a.point, a.choice))
            });
        }
        if obs.tic >= self.target_until {
            self.fight.combat.forced_target = None;
        }
        if !self.stance.0.is_empty() && obs.tic >= self.stance.1 {
            self.stance.0.clear();
            self.fight.combat.tune = Rc::clone(&self.base_tune);
            self.fight.tune = Rc::clone(&self.base_tune);
        }
        let now = &self.fight.combat.now;
        let state = || now.words(obs.armor.unwrap_or(0.0));
        if now.against_her.len() >= 2
            && obs.tic - self.asked.0 >= every.0
            && !advisor.waiting("target")
        {
            self.asked.0 = obs.tic;
            let options = now
                .against_her
                .iter()
                .map(|t| {
                    (
                        format!("m{}", t.id),
                        format!("the {} {:.0} away", t.name, dist((t.x, t.y), now.pos)),
                    )
                })
                .collect();
            advisor.ask(Question {
                point: "target",
                tic: obs.tic,
                state: state(),
                instructions: "Which should she shoot first?".into(),
                options,
            });
        }
        if !now.against_her.is_empty()
            && obs.tic - self.asked.1 >= every.1
            && !advisor.waiting("stance")
        {
            self.asked.1 = obs.tic;
            let options = advisor
                .config
                .stances
                .iter()
                .map(|s| (s.name.clone(), s.says.clone()))
                .collect();
            advisor.ask(Question {
                point: "stance",
                tic: obs.tic,
                state: state(),
                instructions: "How should she fight this?".into(),
                options,
            });
        }
        for line in said {
            self.explorer
                .log
                .push(format!("  [fight] tic {}: {line}", obs.tic));
        }
    }
}
