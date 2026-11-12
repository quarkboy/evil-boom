//! Questions from the bot (the plan's 4.6): where the bot wants her say, one question with two or
//! three options by letter, which is the default, and when the default's taken. Class D (it can
//! wait): she holds, the default taken at the deadline. Class T (it can't): the default done at
//! once, her answer in 3 s switching it, never forced. At most one open, at most a few a minute; answered with `doom_answer`, or
//! overtaken by any action of hers, or closed by a death, a restart or a new level. An answer
//! to a question that's closed is told it's too late and what came of it, never applied.

use super::Event;
use super::{Director, Doing, Order, Report};
use crate::motor::Observation;
use crate::play::Agent;

/// at most this many questions a minute (over it, the caller does as it would unasked)
const QUESTIONS_A_MINUTE: usize = 3;
const MINUTE: i64 = 60 * 35;
/// the default taken after this long (tics) with no answer, unless her policy says otherwise
pub const ANSWER_WAIT: i64 = 8 * 35;
/// no progress this long (tics): asked whether she's stuck
const STUCK: i64 = 180 * 35;
/// a question that can't wait: open this long (tics) for her to switch from its default
const NOW_OPEN: i64 = 3 * 35;
/// a fight's turned bad below this health
const LOW_HEALTH: f64 = 25.0;

/// What an answer does.
#[derive(Clone, Debug)]
pub enum Then {
    /// an order, as if she'd given it
    Order(Order),
    /// on by instinct (the idle fallback, now)
    Instinct,
    /// the level again from its start
    Restart,
    /// on instinct, this last resort hers to take from now on (this level)
    Allow(crate::knowledge::explorer::Resort),
    /// off instinct: she waits for her next action
    Stay,
    /// her say in the fight (`Director::fight`'s: "retreat", "bold", ...)
    Tactic(&'static str),
    /// one of her settings taken (`policy.rs`)
    Policy(&'static str, &'static str),
    /// the run-jump lined up for not taken, nor that leap again
    DontJump(Box<crate::navigator::Jump>),
    /// nothing: as she was
    Nothing,
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub label: String,
    pub then: Then,
}

/// What a question's about, for whether it still applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum About {
    /// for her to settle (it holds till then)
    Settle,
    /// the fight she's in: over with it
    Fight,
    /// the run-jump lined up for (its key): over once she's jumped
    Jump(String),
}

#[derive(Clone, Debug)]
pub struct Question {
    pub id: u32,
    /// it can't wait: the default's done already, her answer switches it
    pub now: bool,
    pub about: About,
    pub text: String,
    pub choices: Vec<Choice>,
    pub default: usize,
    pub opened: i64,
    /// the default taken at this tic (None: she's waited for)
    pub deadline: Option<i64>,
}

/// A question about to be asked.
struct Draft {
    text: String,
    choices: Vec<Choice>,
    default: usize,
    /// told her already (it's the answer to her action)
    told: bool,
    deadline: Option<i64>,
    about: About,
    now: bool,
}

/// A question as it ended, for an answer that comes too late.
#[derive(Clone, Debug)]
pub struct Closed {
    pub id: u32,
    pub how: String,
}

#[derive(Default)]
pub(super) struct Asking {
    pub(super) open: Option<Question>,
    pub(super) closed: Vec<Closed>,
    next_id: u32,
    asked: Vec<i64>,
    /// stuck asked for this stretch with nothing new
    asked_stuck: bool,
}

fn key(i: usize) -> char {
    char::from(b'a' + u8::try_from(i).unwrap_or(25))
}

impl Question {
    /// As she reads it: "[ask q3] Leave with 3 to explore? a) leave anyway b) stay (default, in
    /// 8 s). doom_answer."
    pub fn line(&self, tic: i64) -> String {
        if self.now {
            let opts: Vec<String> = self
                .choices
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let doing = if i == self.default { " (doing it)" } else { "" };
                    format!("{}) {}{doing}", key(i), c.label)
                })
                .collect();
            return format!(
                "[ask q{}] {} {}. Open {} s: doom_answer to switch.",
                self.id,
                self.text,
                opts.join("  "),
                NOW_OPEN / 35
            );
        }
        let opts: Vec<String> = self
            .choices
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let when = match self.deadline {
                    Some(d) if i == self.default => {
                        format!(" (default, in {} s)", ((d - tic).max(0) + 34) / 35)
                    }
                    None if i == self.default => " (default, when you say)".to_string(),
                    _ => String::new(),
                };
                format!("{}) {}{when}", key(i), c.label)
            })
            .collect();
        format!(
            "[ask q{}] {} {}. Answer with doom_answer.",
            self.id,
            self.text,
            opts.join("  ")
        )
    }
}

impl Director {
    /// The question open now, if any.
    pub fn question(&self) -> Option<&Question> {
        self.asking.open.as_ref()
    }

    /// Ask her (`text`, the choices, which is the default): opened, its line given back, and
    /// told her as news unless it's the answer to an action of hers (`told`: she has it there);
    /// None if one's open already or the minute's questions are spent (the caller does as it
    /// would have unasked).
    pub(super) fn ask(
        &mut self,
        obs: &Observation,
        text: String,
        choices: Vec<Choice>,
        default: usize,
        told: bool,
    ) -> Option<String> {
        let deadline = self.answer_wait.map(|w| obs.tic + w);
        self.open(
            obs,
            Draft {
                text,
                choices,
                default,
                told,
                deadline,
                about: About::Settle,
                now: false,
            },
        )
    }

    /// Ask her about something that can't wait: its default done at once and said, her answer
    /// in `NOW_OPEN` switching it. False if not asked (one open, the minute's spent).
    pub(super) fn ask_now(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        text: String,
        choices: Vec<Choice>,
        default: usize,
        about: About,
    ) -> bool {
        let then = choices[default].then.clone();
        let opened = self.open(
            obs,
            Draft {
                text,
                choices,
                default,
                told: false,
                deadline: Some(obs.tic + NOW_OPEN),
                about,
                now: true,
            },
        );
        if opened.is_some() {
            self.carry_out(agent, obs, then);
        }
        opened.is_some()
    }

    /// A question opened (`d`), its line given back and told her unless it's told already;
    /// None if one's open or the minute's spent.
    fn open(&mut self, obs: &Observation, d: Draft) -> Option<String> {
        let a = &mut self.asking;
        a.asked.retain(|&t| obs.tic - t < MINUTE);
        if a.open.is_some() || a.asked.len() >= QUESTIONS_A_MINUTE {
            return None;
        }
        a.asked.push(obs.tic);
        a.next_id += 1;
        let q = Question {
            id: a.next_id,
            now: d.now,
            about: d.about,
            text: d.text,
            choices: d.choices,
            default: d.default,
            opened: obs.tic,
            deadline: d.deadline,
        };
        let line = q.line(obs.tic);
        self.asking.open = Some(q);
        if !d.told {
            self.reports.push(Report {
                tic: obs.tic,
                ok: true,
                text: line.clone(),
                event: super::Event::Asked,
            });
        }
        Some(line)
    }

    /// Her answer to question `id` (by its letter, or the start of an option's words): done, and
    /// what it was; or why not (no such question; closed already, and how).
    pub fn answer(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        id: &str,
        choice: &str,
    ) -> Result<String, String> {
        let n: u32 = id
            .trim()
            .trim_start_matches(['q', 'Q'])
            .parse()
            .map_err(|_| format!("\"{id}\" isn't a question's id (q3, say)."))?;
        let Some(q) = self.asking.open.clone().filter(|q| q.id == n) else {
            return Err(match self.asking.closed.iter().rev().find(|c| c.id == n) {
                Some(c) => format!("Too late: q{n} is closed ({}).", c.how),
                None => format!("No question q{n}."),
            });
        };
        let c = choice.trim().to_lowercase();
        let i = q
            .choices
            .iter()
            .enumerate()
            .position(|(i, opt)| {
                c == key(i).to_string()
                    || c.trim_end_matches(')') == key(i).to_string()
                    || (!c.is_empty() && opt.label.to_lowercase().starts_with(&c))
            })
            .ok_or_else(|| {
                let keys: Vec<String> = (0..q.choices.len()).map(|i| key(i).to_string()).collect();
                format!("q{n}: answer {}.", keys.join(", "))
            })?;
        let label = q.choices[i].label.clone();
        self.close(format!("you answered: {label}"));
        if q.now && i == q.default {
            return Ok(format!("q{n}: {label}, as it was."));
        }
        self.carry_out(agent, obs, q.choices[i].then.clone());
        Ok(format!("q{n}: {label}."))
    }

    /// Each tic: an open question's deadline (its default taken, and said); whether she's stuck.
    pub(super) fn questions(&mut self, agent: &mut Agent, obs: &Observation) {
        // (one that couldn't wait: its default's done; closed quietly at its time, or when
        // what it was about is over)
        if let Some(q) = self.asking.open.clone().filter(|q| q.now) {
            // (the fight as the summary has it: over after a moment with nothing after her,
            // not each tic one's out of sight)
            if q.about == About::Fight && self.fight.is_none() {
                self.close("the fight's over".into());
            } else if q.deadline.is_some_and(|d| obs.tic >= d) {
                self.close(format!("no answer: {}", q.choices[q.default].label));
            }
        }
        if let Some(q) = self
            .asking
            .open
            .clone()
            .filter(|q| q.deadline.is_some_and(|d| obs.tic >= d))
        {
            let c = q.choices[q.default].clone();
            self.close(format!("no answer: {}", c.label));
            self.reports.push(Report {
                tic: obs.tic,
                ok: true,
                text: format!("No answer to q{}: {}.", q.id, c.label),
                event: super::Event::Defaulted,
            });
            self.carry_out(agent, obs, c.then);
        }
        self.ask_if_stuck(agent, obs);
        self.ask_fight(agent, obs);
    }

    /// A run-jump come down (its key): a question about it closed.
    pub(super) fn jumped(&mut self, key: &str) {
        if self
            .asking
            .open
            .as_ref()
            .is_some_and(|q| q.about == About::Jump(key.to_string()))
        {
            self.close("jumped".into());
        }
    }

    /// A fight turned bad (outmatched, an arch-vile or a boss after her, her health low), once a
    /// fight and not when she's had her say in it: what the fight's doing told and kept, falling
    /// back or going bold hers for 3 s.
    fn ask_fight(&mut self, agent: &mut Agent, obs: &Observation) {
        let sit = self.now.clone();
        if !sit.fighting() || self.asking.open.is_some() || self.tactics.any(obs.tic) {
            return;
        }
        let Some(f) = self.fight.as_mut().filter(|f| !f.asked) else {
            return;
        };
        let them: Vec<String> = sit
            .against
            .iter()
            .map(|(n, c)| super::some(*c, n))
            .collect();
        let why = if sit.vile {
            "An arch-vile"
        } else if sit.boss {
            "A boss"
        } else if sit.outmatched {
            "Outmatched"
        } else if sit.health < LOW_HEALTH {
            "Health low"
        } else {
            return;
        };
        f.asked = true;
        let doing = sit.doing();
        let mut choices = vec![Choice {
            label: format!("as it is ({doing})"),
            then: Then::Nothing,
        }];
        // (falling back offered only with cover she knows a way to: else it'd be refused)
        let cover = !sit.retreating && agent.player_mut().is_some_and(|p| p.has_cover(obs));
        if cover {
            choices.push(Choice {
                label: "fall back to cover".into(),
                then: Then::Tactic("retreat"),
            });
        }
        choices.push(Choice {
            label: "go bold".into(),
            then: Then::Tactic("bold"),
        });
        self.ask_now(
            agent,
            obs,
            format!("{why}: {} (health {:.0}).", them.join(", "), sit.health),
            choices,
            0,
            About::Fight,
        );
    }

    /// Closed (`how`): kept for an answer that comes after.
    fn close(&mut self, how: String) {
        if let Some(q) = self.asking.open.take() {
            self.asking.closed.push(Closed { id: q.id, how });
            let over = self.asking.closed.len().saturating_sub(8);
            self.asking.closed.drain(..over);
        }
    }

    /// An open question overtaken (an action of hers, a death, a restart, a new level).
    pub(super) fn overtaken(&mut self, by: &str) {
        self.close(format!("overtaken by {by}"));
    }

    /// An order of hers in place of what she's doing: it settles what's asked about where she
    /// goes (leaving, a way that costs, a jump), not how she fights.
    pub(super) fn ordered(&mut self) {
        if self
            .asking
            .open
            .as_ref()
            .is_some_and(|q| q.about != About::Fight)
        {
            self.overtaken("your action");
        }
    }

    /// Her say in the fight: it settles what's asked about the fight.
    pub(super) fn said_in_fight(&mut self) {
        if self
            .asking
            .open
            .as_ref()
            .is_some_and(|q| q.about == About::Fight)
        {
            self.overtaken("your say in the fight");
        }
    }

    fn carry_out(&mut self, agent: &mut Agent, obs: &Observation, then: Then) {
        match then {
            Then::Order(o) => self.queue.push_back(super::Queued {
                order: o,
                rush: false,
                then: false,
            }),
            Then::Instinct => self.on_instinct(agent, obs),
            Then::Restart => self.restart_wanted = self.restart_allowed,
            Then::Allow(r) => {
                self.resorts.insert(r);
                let x = agent.x_mut();
                x.resorts = Some(self.resorts.clone());
                x.resort_wanted = None; // (stale till she chooses again: not asked twice)
            }
            Then::Policy(setting, v) => {
                let said = self.policy_to(setting, v);
                self.tell(obs, Event::Tactic, said);
            }
            Then::Tactic(t) => {
                let said = match self.fight(agent, obs, t, None, None) {
                    Ok(said) | Err(said) => said,
                };
                self.tell(obs, Event::Tactic, said);
            }
            Then::DontJump(j) => {
                agent.x_mut().decline(obs, &j);
                self.tell(obs, Event::NotJumping, "Not jumping.".into());
            }
            Then::Stay => {
                if matches!(self.doing, Some(Doing::Auto)) {
                    self.end_auto(agent);
                    self.doing = None;
                    self.auto_spent = true;
                }
            }
            Then::Nothing => {}
        }
    }

    /// Leaving with things left: asked (leave anyway, or stay), the question's line given back;
    /// None when there's nothing left to ask about (or no asking now: then the old answer).
    pub fn ask_leave(&mut self, agent: &mut Agent, obs: &Observation) -> Option<String> {
        let left = self.exit_check(agent, obs)?;
        Some(
            self.ask(
                obs,
                format!("{left} Leave anyway?"),
                vec![
                    Choice {
                        label: "leave anyway".into(),
                        then: Then::Order(Order::Exit),
                    },
                    Choice {
                        label: "stay".into(),
                        then: Then::Nothing,
                    },
                ],
                1,
                true,
            )
            .unwrap_or_else(|| format!("{left} Leave anyway? Resend with sure: true.")),
        )
    }

    /// A go her way's only by something that costs (a hurting floor, a one-way drop): asked
    /// (go anyway, or don't), the line given back; Err as `check_go` has it otherwise. Ok(None):
    /// she can go, ordered.
    pub fn ask_go(
        &mut self,
        agent: &mut Agent,
        obs: &Observation,
        r: &str,
        risky: bool,
        rush: bool,
        then: bool,
    ) -> Result<Option<String>, String> {
        match self.check_go(agent, obs, r, risky) {
            Ok(_) => {
                self.order_with(Order::Go(r.to_string(), risky), rush, then);
                Ok(None)
            }
            Err(why) if !risky && self.check_go(agent, obs, r, true).is_ok() => {
                let what = self.tag(&self.resolve(r), obs);
                Ok(Some(
                    self.ask(
                        obs,
                        format!(
                            "The only way to {what} crosses a floor that hurts, or drops where you can't get back."
                        ),
                        vec![
                            Choice {
                                label: "go anyway".into(),
                                then: Then::Order(Order::Go(r.to_string(), true)),
                            },
                            Choice {
                                label: "don't".into(),
                                then: Then::Nothing,
                            },
                        ],
                        1,
                        true,
                    )
                    .unwrap_or(why),
                ))
            }
            Err(why) => Err(why),
        }
    }

    /// By instinct, nothing left but walls to search or the way out: asked (leave by each exit
    /// she knows, or stay); false if not asked (the caller tells it as before).
    pub(super) fn ask_done_here(&mut self, obs: &Observation, exits: &[(String, String)]) -> bool {
        let mut choices: Vec<Choice> = exits
            .iter()
            .take(2)
            .map(|(id, line)| Choice {
                label: format!("leave by {line}"),
                then: Then::Order(Order::Go(id.clone(), false)),
            })
            .collect();
        choices.push(Choice {
            label: "stay (doom_explore with secrets searches walls)".into(),
            then: Then::Nothing,
        });
        let default = choices.len() - 1;
        self.ask(
            obs,
            "Nothing more to do here that you know of but search walls. Leave?".into(),
            choices,
            default,
            false,
        )
        .is_some()
    }

    /// On instinct, nothing ordinary left and a last resort held back (`r`): asked whether to
    /// take it, leave (with an exit known), or stay (the default: off instinct, waiting for
    /// her). False if not asked.
    pub(super) fn ask_resort(
        &mut self,
        obs: &Observation,
        r: crate::knowledge::explorer::Resort,
        exit: Option<String>,
    ) -> bool {
        use crate::knowledge::explorer::Resort;
        let (what, go) = match r {
            Resort::Strafing => ("strafing run-jumps, the hard kind", "try them"),
            Resort::StuckAgain => ("another go at the places you got stuck", "go back"),
            Resort::EveryWall => ("pressing every wall round you for secrets", "press them"),
            Resort::MissedJumps => ("the jumps you missed before, again", "try them"),
            Resort::Harm => (
                "switches and walls you'd reach over a floor that hurts",
                "go, hurt and all",
            ),
            Resort::Exit => ("the way out", "leave"),
        };
        let mut choices = vec![Choice {
            label: go.into(),
            then: if r == Resort::Exit {
                Then::Order(Order::Exit)
            } else {
                Then::Allow(r)
            },
        }];
        if let Some(e) = exit.filter(|_| r != Resort::Exit) {
            choices.push(Choice {
                label: format!("leave ({e})"),
                then: Then::Order(Order::Exit),
            });
        }
        choices.push(Choice {
            label: "stay".into(),
            then: Then::Stay,
        });
        let default = choices.len() - 1;
        self.ask(
            obs,
            format!("Nothing else to do here that you know of. Next would be {what}."),
            choices,
            default,
            false,
        )
        .is_some()
    }

    /// Nothing new for a long while: asked whether to go on by instinct, restart (if that's
    /// hers), or keep on as she is. Once a stretch.
    fn ask_if_stuck(&mut self, _agent: &Agent, obs: &Observation) {
        let stalled = self.stalled_for(obs.tic);
        if stalled < STUCK {
            self.asking.asked_stuck = false;
            return;
        }
        if self.asking.asked_stuck || self.asking.open.is_some() || self.fight.is_some() {
            return;
        }
        self.asking.asked_stuck = true;
        let mut choices = Vec::new();
        let instinct = matches!(self.doing, Some(Doing::Auto));
        if !instinct {
            choices.push(Choice {
                label: "carry on by instinct".into(),
                then: Then::Instinct,
            });
        }
        if self.restart_allowed {
            choices.push(Choice {
                label: "restart the level".into(),
                then: Then::Restart,
            });
        }
        if choices.is_empty() {
            return; // (on by instinct already, nothing else hers: nothing to ask)
        }
        choices.push(Choice {
            label: "keep on as you are".into(),
            then: Then::Nothing,
        });
        let default = if instinct { choices.len() - 1 } else { 0 };
        self.ask(
            obs,
            format!("Nothing new for {} minutes. Stuck?", STUCK / MINUTE),
            choices,
            default,
            false,
        );
    }
}
