// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! An order begun: what it comes to (an objective, a place, a search), whether it can be
//! gone for, and going.

use super::*;

impl Director {
    /// (a deathmatch) The next of a hunt (the freshest, surest clue of a rival, as the fight has
    /// them: `Kind::Hunt`) or of gearing up (a pickup); none, told so once, and a hunt waits for
    /// word of one, listening; gearing up done once she's geared up, or with nothing to take.
    pub(super) fn hunt_next(&mut self, agent: &mut Agent, obs: &Observation, gear: bool) {
        if gear && agent.player().is_some_and(|p| p.geared_up(obs)) {
            self.report(obs, true, Event::Outcome, "Geared up.".into());
            return;
        }
        // (what she took last ended as she took it, gone: the ranking it came from is as old as
        // a second, and has it still; taken again from it every tic, she went round and round)
        if obs.tic - self.hunt_took <= 1 {
            self.fresh_ranking();
        }
        let open = self.open_objectives(agent, obs);
        let wanted: Vec<Objective> = open
            .iter()
            .filter(|o| {
                if gear {
                    o.kind == Kind::Item
                } else {
                    o.kind == Kind::Hunt && o.thing.is_none()
                }
            })
            .cloned()
            .collect();
        for o in wanted {
            if agent.x_mut().take(obs, o) {
                self.doing = Some(Doing::Hunt(gear));
                self.hunt_idle = false;
                self.hunt_took = obs.tic;
                return;
            }
        }
        // (nothing to take that she knows of, or no word of anyone: she looks about, new ground
        // first, the level walked round again once it's all seen; word of a rival turns her)
        let roam: Vec<Objective> = open.into_iter().filter(explores).collect();
        let mut roaming = false;
        for o in roam {
            if agent.x_mut().take(obs, o) {
                roaming = true;
                self.hunt_took = obs.tic;
                break;
            }
        }
        self.doing = Some(Doing::Hunt(gear));
        if !std::mem::replace(&mut self.hunt_idle, true) {
            let text = match (gear, roaming) {
                (true, true) => "No gear you know of: looking about for some.",
                (false, true) => "No word of a rival: looking about.",
                (true, false) => "No gear you know of, and nowhere new to look.",
                (false, false) => "No word of a rival: you wait here, listening.",
            };
            // (how the hunt's going, not news to answer: quiet, or she orders it again and again)
            self.report(obs, true, Event::Begun, text.into());
        }
    }

    pub(super) fn start(&mut self, agent: &mut Agent, obs: &Observation, o: Order) {
        // (a new order replaces the one in hand, and says so: two given together, the first
        // would be gone without a word. An errand in the middle of exploring is an errand: the
        // exploring goes on after it)
        let errand = matches!(o, Order::Go(..));
        // (told to stop, she stops: not on by instinct the moment after)
        self.auto_spent = matches!(o, Order::Stop);
        self.had_order = true;
        self.nudged = 0;
        if matches!(self.doing, Some(Doing::Auto)) {
            self.end_auto(agent);
        }
        match self.doing.take() {
            Some(Doing::Area(id, what, secrets)) if errand => {
                self.resume = Some(Order::ExploreArea(id, secrets));
                self.report(
                    obs,
                    true,
                    Event::Begun,
                    format!("Exploring {what} goes on after this."),
                );
            }
            Some(Doing::Way(b, secrets)) if errand => {
                self.resume = Some(Order::ExploreWay(b, secrets));
                self.report(
                    obs,
                    true,
                    Event::Begun,
                    format!("Exploring {} goes on after this.", compass(b)),
                );
            }
            Some(Doing::Once(_, what)) if !matches!(o, Order::Stop) => {
                self.report(
                    obs,
                    false,
                    Event::Replaced,
                    format!("Dropped for the new action: {what}."),
                );
            }
            Some(Doing::Area(_, what, _)) if !matches!(o, Order::Stop) => {
                self.report(
                    obs,
                    false,
                    Event::Replaced,
                    format!("Dropped for the new action: exploring {what}."),
                );
            }
            Some(Doing::Way(b, _)) if !matches!(o, Order::Stop) => {
                self.report(
                    obs,
                    false,
                    Event::Replaced,
                    format!("Dropped for the new action: exploring {}.", compass(b)),
                );
            }
            _ => {}
        }
        if !errand {
            self.resume = None;
        }
        let x = agent.x_mut();
        x.current = None;
        x.follower = None;
        x.ended = None;
        self.taken.clear();
        self.told_paused = false;
        // (a deathmatch: hunting hers only while she's ordered it)
        if let Some(p) = agent.player_mut() {
            p.set_hunt_ordered(matches!(o, Order::Hunt(false)));
        }
        match o {
            Order::Hunt(gear) => {
                self.hunt_idle = false;
                self.hunt_next(agent, obs, gear);
            }
            Order::Stop => self.report(obs, true, Event::Outcome, "Stopped.".into()),
            Order::Exit => match self
                .objectives_of(agent, obs, &[Kind::Exit])
                .into_iter()
                .next()
            {
                Some(e) => self.go(agent, obs, &e.id, false),
                None => self.report(obs, false, Event::Outcome, "No exit known yet.".into()),
            },
            Order::Go(r, risky) => {
                let id = self.resolve(&r);
                self.go(agent, obs, &id, risky);
            }
            Order::ExploreArea(r, secrets) => {
                let id = self.resolve(&r);
                let Some(a) = self.places(agent).by_id(&id).cloned() else {
                    self.report(obs, false, Event::Outcome, format!("No area \"{r}\"."));
                    return;
                };
                let what = self.tag(&a.id, obs);
                let fits = |d: &Self, a2: &Agent, o: &Objective| d.in_area(a2, &id, o);
                if self.next_in(agent, obs, secrets, fits) {
                    self.report(
                        obs,
                        true,
                        Event::Begun,
                        format!("Exploring {what}{}.", with_secrets(secrets)),
                    );
                    self.doing = Some(Doing::Area(id, what, secrets));
                } else {
                    let found = self.found(agent, obs, fits);
                    let instead = self.instead(agent, obs);
                    self.report(
                        obs,
                        true,
                        Event::Outcome,
                        format!(
                            "Nothing to {} in {what}.{found}{instead}",
                            if secrets > 0 {
                                "explore or search"
                            } else {
                                "explore"
                            }
                        ),
                    );
                }
            }
            Order::ExploreWay(b, secrets) => {
                let pos = obs.pos();
                let fits = move |_: &Self, _: &Agent, o: &Objective| {
                    dist(pos, o.goal) > 64.0 && norm(heading(pos, o.goal) - b).abs() < EXPLORE_CONE
                };
                if self.next_in(agent, obs, secrets, fits) {
                    self.report(
                        obs,
                        true,
                        Event::Begun,
                        format!("Exploring {}{}.", compass(b), with_secrets(secrets)),
                    );
                    self.doing = Some(Doing::Way(b, secrets));
                } else {
                    let found = self.found(agent, obs, fits);
                    let instead = self.instead(agent, obs);
                    self.report(
                        obs,
                        false,
                        Event::Outcome,
                        format!(
                            "Nothing to {} {}.{found}{instead}",
                            if secrets > 0 {
                                "explore or search"
                            } else {
                                "explore"
                            },
                            compass(b)
                        ),
                    );
                }
            }
        }
    }

    /// Waiting on a floor that hurts: to the nearest known floor that doesn't, the way she was
    /// going rather than back (through what she came for, out the far side: an item in nukage
    /// picked up isn't stood on after, waiting for the next order). True if she's off to it.
    pub(super) fn off_hurting_floor(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        let nav = std::rc::Rc::clone(&agent.x().nav);
        let pos = obs.pos();
        let Some(here) = nav.poly_at(pos) else {
            return false;
        };
        if !agent.x().knowledge.damaging(nav.sector_of_poly(here)) {
            self.told_trapped = false;
            return false;
        }
        let v = obs.velocity.unwrap_or((0.0, 0.0));
        let way = if v.0.hypot(v.1) > 1.0 {
            v.1.atan2(v.0).to_degrees()
        } else {
            obs.angle
        };
        self.fresh_ranking();
        let _ = self.open_objectives(agent, obs); // (what she can get to, up to date)
        let x = agent.x();
        let mut spots: Vec<(f64, Point)> = x
            .reach
            .iter()
            .filter(|&&p| !x.knowledge.damaging(nav.sector_of_poly(p)))
            .map(|&p| {
                let c = nav.poly_center(p);
                // (behind her counts as up to twice as far)
                let back = norm(heading(pos, c) - way).abs() / 180.0;
                (dist(pos, c) * (1.0 + back), c)
            })
            .collect();
        spots.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, c) in spots.into_iter().take(OFF_TRIES) {
            let x = agent.x();
            let at = x.stand_spot_for(c, &x.reach);
            let mut o = Objective::new(Kind::Visit, at, "off the floor that hurts");
            o.risky = true; // (in it already: the way out may wade)
            if agent.x_mut().take(obs, o) {
                return true;
            }
        }
        // (no way off she knows of: hers to hear, once, not to die waiting in silence)
        if !self.told_trapped {
            self.told_trapped = true;
            self.report(
                obs,
                false,
                Event::Trapped,
                format!(
                    "You're on a hurting floor, no known way off (health {:.0}).",
                    obs.health.unwrap_or(0.0)
                ),
            );
        }
        false
    }

    /// Waiting: the nearest pickup she takes by herself (`spare`), if it's safe: one the explorer
    /// offers (past its checks for danger and for being cut off), by a way with nothing that hurts,
    /// not far. True if she's off for it.
    pub(super) fn pick_up_nearby(&mut self, agent: &mut Agent, obs: &Observation) -> bool {
        let spares = agent.x().spares.clone();
        let mut options: Vec<Objective> = self
            .open_objectives(agent, obs)
            .into_iter()
            .filter(|o| {
                o.kind == Kind::Item && !o.risky && o.thing.is_some_and(|t| spares.contains(&t))
            })
            .collect();
        let pos = obs.pos();
        options.sort_by(|a, b| dist(pos, a.goal).total_cmp(&dist(pos, b.goal)));
        for o in options.into_iter().take(PICKUP_TRIES) {
            let near = agent
                .x_mut()
                .route_to(&o, obs)
                .is_some_and(|st| crate::knowledge::explorer::route_length(&st) <= PICKUP_RANGE);
            if near && agent.x_mut().take(obs, o) {
                return true;
            }
        }
        false
    }

    /// The objective going to (trying, picking up) `id` would be; why not, if there's none.
    pub(super) fn objective_for(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        id: &str,
    ) -> Result<Objective, String> {
        // (the explorer's options, which also brings what she can get to up to date)
        let open = self.open_objectives(agent, obs);
        if let Some(a) = self.places(agent).by_id(id).cloned() {
            // (its middle; else somewhere in it she has a way to: what's unexplored in it first
            // (what it was suggested for), then the rest of it she knows. The middle can be a
            // ledge, a pillar's top or a pit's floor with the area's ways in all round it.)
            let what = format!("go to {}", Places::sketch(&a));
            let x = agent.x();
            let nav = std::rc::Rc::clone(&x.nav);
            let in_area = |p: Point| {
                nav.poly_at(p)
                    .is_some_and(|q| a.sectors.contains(&nav.sector_of_poly(q)))
            };
            let mut spots = vec![x.stand_spot_for(a.center, &x.reach)];
            spots.extend(
                open.iter()
                    .filter(|o| explores(o) && in_area(o.goal))
                    .take(AREA_TRIES)
                    .map(|o| o.goal),
            );
            let mut polys: Vec<Point> = x
                .knowledge
                .known_polys()
                .into_iter()
                .filter(|&q| a.sectors.contains(&nav.sector_of_poly(q)))
                .map(|q| nav.poly_center(q))
                .collect();
            polys.sort_by(|p, q| dist(*p, a.center).total_cmp(&dist(*q, a.center)));
            spots.extend(
                polys
                    .into_iter()
                    .take(AREA_TRIES)
                    .map(|p| x.stand_spot_for(p, &x.reach)),
            );
            let first = Objective::new(Kind::Visit, spots[0], what.clone());
            return Ok(spots
                .into_iter()
                .map(|at| Objective::new(Kind::Visit, at, what.clone()))
                .find(|o| self.advised_way(agent, obs, o))
                .unwrap_or(first));
        }
        if let Some(t) = id
            .split_once("-t")
            .and_then(|(_, n)| n.parse::<i32>().ok())
            .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n))
        {
            let x = agent.x();
            let Some(at) = x.touch_spot((t.x, t.y), t.z, &x.reach) else {
                let (name, at) = (called(t.name), (t.x, t.y));
                let toward = self.toward(agent, obs, at);
                return Err(format!("No known way to the {name} yet.{toward}"));
            };
            let mut o = Objective::new(Kind::Item, at, format!("pick up {}", t.name));
            o.thing = Some(t.id);
            return Ok(o);
        }
        // (or what she's doing this moment: listed a moment ago, then taken up by the order in hand
        // as she got to it; GPT-Evil's door-l148, "here" in the list, "nothing to go to" a tic later)
        let current = agent.x().current.clone().filter(|o| id_of(o) == id);
        open.into_iter()
            .find(|o| id_of(o) == id)
            .or(current)
            .ok_or_else(|| {
                self.no_longer(agent, obs, id)
                    .or_else(|| place_not_id(id))
                    .unwrap_or_else(|| {
                        format!("Nothing \"{id}\" to go to (done, gone or unknown).")
                    })
            })
    }

    /// On her way to an exit by an order (or one waiting its turn)?
    pub fn leaving(&self) -> bool {
        matches!(&self.doing, Some(Doing::Once(id, _)) if id.starts_with("exit-"))
            || self.queue.iter().any(|q| {
                matches!(&q.order, Order::Exit)
                    || matches!(&q.order, Order::Go(r, _) if self.resolve(r).starts_with("exit-"))
            })
    }

    /// What became of a door, switch or exit she was told of that's no longer listed: worked
    /// already (the bot opened the door on its way, as often as not), or out of her reach from
    /// here. None if she was never told of it (the caller says it's unknown).
    pub fn no_longer(&self, agent: &Agent, obs: &Observation, id: &str) -> Option<String> {
        // (a thing gone from the map was picked up: nothing else takes one away)
        if let Some(n) = id
            .strip_prefix("item-t")
            .or_else(|| id.strip_prefix("key-t"))
            .and_then(|n| n.parse::<i32>().ok())
        {
            let there = obs.things.iter().flatten().any(|t| t.id == n);
            return (!there).then(|| format!("{id}: taken already."));
        }
        // (one of the explorer's spots, suggested once: done since, or out of reach now)
        if id.starts_with("spot-") {
            return Some(format!(
                "{id}: nothing there to do now (done, or out of reach)."
            ));
        }
        if !self.told_exits.contains(id) {
            return None;
        }
        let line = id
            .split_once("-l")
            .and_then(|(_, l)| l.parse::<usize>().ok());
        let worked = line.is_some_and(|l| agent.x().knowledge.tried(l));
        Some(if worked && id.starts_with("door-") {
            format!("{id}: opened already.")
        } else if worked {
            format!("{id}: pressed already.")
        } else {
            format!("{id}: no way to it from where you are now.")
        })
    }

    /// A place as lists give it, a way and a distance from her ("SW 305"), read as what's
    /// listed nearest there (within a quarter of the distance, or 96): its id. None if it isn't
    /// one, or nothing's near it.
    pub fn at_position(&mut self, agent: &mut Agent, obs: &Observation, r: &str) -> Option<String> {
        let (way, d) = r.trim().rsplit_once(' ')?;
        let d: f64 = d.parse().ok()?;
        let a = bearing(way)?.to_radians();
        let pos = obs.pos();
        let spot = (pos.0 + d * a.cos(), pos.1 + d * a.sin());
        let near = (d / 4.0).max(96.0);
        [
            Topic::Doors,
            Topic::Switches,
            Topic::Items,
            Topic::Exits,
            Topic::Monsters,
            Topic::Areas,
        ]
        .into_iter()
        .flat_map(|t| self.list(agent, obs, t))
        .map(|e| (dist(e.at, spot), e.id))
        .filter(|(off, _)| *off <= near)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, id)| id)
    }

    /// Her reference found nothing listed: what became of it if she was told of it, else that
    /// it's not known.
    pub fn not_found(&self, agent: &Agent, obs: &Observation, r: &str) -> String {
        self.no_longer(agent, obs, &self.resolve(r))
            .or_else(|| place_not_id(r))
            .unwrap_or_else(|| format!("Nothing known as \"{r}\"."))
    }

    /// Could she go to `r` now (risky: past what she'd rather not, if it won't kill her)? What
    /// it is, or why not (and what would do it, if risky would).
    pub fn check_go(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        r: &str,
        risky: bool,
    ) -> Result<String, String> {
        let id = self.resolve(r);
        let mut obj = self.objective_for(agent, obs, &id)?;
        obj.risky = risky;
        let what = self.tag(&id, obs);
        // (a pickup the game won't give her now: said, not walked to. End to end, a dozen
        // trips to shells, shotguns and green armor, each ending "couldn't pick it up")
        let thing = id
            .strip_prefix("item-t")
            .and_then(|n| n.parse::<i32>().ok())
            .and_then(|n| obs.things.iter().flatten().find(|t| t.id == n));
        // (co-op: a gun she has stays where the level put it, and gives her nothing)
        if thing.is_some_and(|t| agent.x().knowledge.left_lying(obs, t)) {
            return Err(format!(
                "{what}: you have it. In this game it stays where it lies and gives you nothing."
            ));
        }
        if let Some(why) = thing.and_then(|t| Self::cant_take(t.name, obs)) {
            return Err(format!("{what}: you can't pick it up now: {why}."));
        }
        // (a barrel stopped her on the way there, and nothing's changed: said, not walked again)
        if agent.x().barrel_blocked(&obj, obs) {
            return Err(format!(
                "{what}: a barrel's in the way and there's nowhere safe to shoot it from; nothing's changed since you tried."
            ));
        }
        if agent.x_mut().route_to(&obj, obs).is_some() {
            return Ok(what);
        }
        if !risky {
            obj.risky = true;
            if agent.x_mut().route_to(&obj, obs).is_some() {
                return Err(format!("The only known way to {what} crosses a hurting floor (or worse); resend with risky: true."));
            }
        }
        let toward = self.toward(agent, obs, obj.goal);
        Err(format!("No survivable way to {what} known.{toward}"))
    }

    /// Go to (try, pick up) something by id.
    pub(super) fn go(&mut self, agent: &mut Agent, obs: &Observation, id: &str, risky: bool) {
        let mut obj = match self.objective_for(agent, obs, id) {
            Ok(o) => o,
            Err(why) => {
                self.report(obs, false, Event::Outcome, why);
                return;
            }
        };
        obj.risky = risky;
        let what = self.tag(id, obs);
        let at = obj.goal;
        if agent.x_mut().take(obs, obj) {
            self.report(obs, true, Event::Begun, format!("Going to {what}."));
            self.doing = Some(Doing::Once(id.to_string(), what));
        } else {
            let toward = self.toward(agent, obs, at);
            self.report(
                obs,
                false,
                Event::Outcome,
                format!("No way to {what}.{toward}"),
            );
        }
    }
}
