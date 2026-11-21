// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Instinct: with no order from her, the bot carrying on by itself, and handing back.

use super::*;

impl Director {
    /// On her own for want of an order: what she's taken up is said (quietly) as it changes;
    /// with nothing left she'd do but leave, she stops and says so: leaving is an order's.
    pub(super) fn auto(&mut self, agent: &mut Agent, obs: &Observation) {
        // (nothing left she'd do but search walls: with an exit known, she stops, leaving being
        // an order's. With none known the way on is still to find, and she keeps at it, last
        // resorts and all, as she would playing by herself)
        if agent.x().out_of_options.is_some() && !agent.x().knowledge.exits().is_empty() {
            self.end_auto(agent);
            self.doing = None;
            self.auto_spent = true;
            let exits = self.objectives_of(agent, obs, &[Kind::Exit]);
            let known: Vec<(String, String)> = exits
                .iter()
                .map(|e| (e.id.clone(), self.describe(e, obs)))
                .collect();
            if !known.is_empty() && self.ask_done_here(obs, &known) {
                return;
            }
            self.report(
                obs,
                false,
                Event::Outcome,
                match exits.first() {
                    Some(e) => format!(
                        "Nothing more to do here that you know of but search walls or leave ({}).",
                        self.describe(e, obs)
                    ),
                    None => "Nothing more to do here that you know of but search walls or leave."
                        .to_string(),
                },
            );
            return;
        }
        // (nothing ordinary left, a last resort held back: asked; meanwhile she waits)
        if agent.x().current.is_none() && self.question().is_none() {
            if let Some(r) = agent.x().resort_wanted {
                let exit = self
                    .objectives_of(agent, obs, &[Kind::Exit])
                    .first()
                    .map(|e| self.describe(e, obs));
                if self.ask_resort(obs, r, exit) {
                    return;
                }
            }
        }
        // (what she's taken up, in plain words, as it changes: not each pickup and barrel on
        // the way, and not more than a line every few seconds)
        let Some((kind, phrase)) = agent
            .x()
            .current
            .as_ref()
            .map(|o| (o.kind, plain(o.kind, &o.label)))
        else {
            return;
        };
        if !matches!(kind, Kind::Item | Kind::Barrel)
            && phrase != self.told_auto
            && self
                .told_auto_tic
                .is_none_or(|t| obs.tic - t >= AUTO_SAY_EVERY)
        {
            self.tell(obs, Event::Instinct, format!("By instinct: {phrase}."));
            self.next_due = Some("step");
            self.told_auto = phrase;
            self.told_auto_tic = Some(obs.tic);
        }
    }

    /// On by instinct from now: the explorer chooses (not leaving the level).
    pub(super) fn go_instinct(&mut self, agent: &mut Agent) {
        self.idle_since = None;
        self.doing = Some(Doing::Auto);
        self.told_auto.clear();
        let x = agent.x_mut();
        x.directed = false;
        // (an order's rush ends with the order: what she takes up by herself isn't pressed on
        // with mid-fight)
        x.press_on = false;
        // (her last resorts only as she's let them: the first held back is asked about)
        x.resorts = Some(self.resorts.clone());
        x.hold_exits(true);
        x.out_of_options = None;
    }

    /// On by instinct, as her answer had it: said.
    pub(super) fn on_instinct(&mut self, agent: &mut Agent, obs: &Observation) {
        self.auto_spent = false;
        self.go_instinct(agent);
        self.report(
            obs,
            true,
            Event::OnInstinct,
            "On by instinct (not leaving the level) till your next action.".into(),
        );
    }

    /// Back to her orders: the explorer chooses nothing of its own.
    pub(super) fn end_auto(&mut self, agent: &mut Agent) {
        let x = agent.x_mut();
        x.directed = true;
        x.hold_exits(false);
        x.current = None;
        x.follower = None;
        x.ended = None;
    }
}
