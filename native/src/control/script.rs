//! Directed play's commands as text, one a line: what a scripted commander (a `drive` script, or
//! someone typing into the file a watched game reads) says, and what comes back.
//!
//!   list TOPIC [all] [N] what she knows of: areas (those explored only with all), doors, items,
//!                       monsters, exits
//!   name ID NAME...     a name for something listed (refused when too long or taken)
//!   order go REF [risky] | order explore REF|DIR | order search REF|DIR [2|3] | order exit |
//!   order stop
//!                       an order (REF an id or a name; DIR n, ne, east, ...); `then` last: after
//!                       the order in hand, not instead of it
//!   check REF [risky]   could she go there now (as doom_go asks), and if not, why
//!   fight target REF|KIND [SECONDS] | fight weapon NAME|auto [SECONDS] | fight hold_fire
//!   [SECONDS] | fight fire | fight bold|careful [SECONDS] | fight steady | fight auto
//!                       her say in the fight, each for a while (10 s, a stance 30)
//!   idle SECONDS|wait   with no order that long she carries on by herself (not leaving the
//!                       level); wait: she waits, as long as it takes
//!   policy [SETTING VALUE]
//!                       her standing settings (stance, save, risk, powerups, explore); none: as they are
//!   look                what's round her: the room, what it leads to, what's in sight and heard,
//!                       the bot's pick
//!   hints off|on_stall|always
//!                       the bot's own pick: never; where asked and after a while with no order
//!                       (as it starts); or as soon as she's waiting
//!   answer ID CHOICE    a question's answer (answer q3 a)
//!   answers SECONDS|wait
//!                       how long a question waits before its default
//!   game start|next|restart ...
//!                       the game itself, when it's hers (see control::game)
//!   status              her health, armor, weapons, ammo, keys, what she's doing
//!   why                 why she's doing it: the aim and what it was chosen over, the fight,
//!                       why she's not shooting, the last action that failed
//!   where               where she is, her health, what she's doing
//!   log [N]             her log's last N lines (40), as the watcher shows it

use super::directed::{bearing, Director, Hints, Order, Topic};
use crate::motor::Observation;
use crate::play::Agent;

/// Carry out one command; what it says back (an order's own outcome comes later, as reports).
pub fn command(
    d: &mut Director,
    agent: &mut Agent,
    obs: &Observation,
    line: &str,
) -> Result<Vec<String>, String> {
    let w: Vec<&str> = line.split_whitespace().collect();
    match w.first().copied() {
        Some("list") => {
            let topic = w
                .get(1)
                .and_then(|t| Topic::parse(t))
                .ok_or("list areas|doors|items|monsters|exits [all] [N]")?;
            let all = w.contains(&"all");
            let n = w.iter().skip(2).find_map(|v| v.parse().ok()).unwrap_or(20);
            let l = d.listing(agent, obs, topic, all, n);
            let mut out = vec![format!("{} known", l.known)];
            out.extend(l.lines.iter().map(|e| format!("  {e}")));
            if l.hidden > 0 {
                out.push(format!(
                    "  and {} explored areas (list areas all to see them)",
                    l.hidden
                ));
            }
            out.extend(l.pick.map(|p| format!("  suggested: {p}")));
            Ok(out)
        }
        Some("name") => {
            let id = w.get(1).ok_or("name ID NAME")?;
            d.name(id, &w[2..].join(" "))
                .map(|()| vec![format!("named {id} \"{}\"", w[2..].join(" "))])
        }
        Some("order") => {
            // (a go that costs is asked first, as Evil's is)
            if w.get(1) == Some(&"go") && !w.contains(&"risky") && !w.contains(&"sure") {
                let r = w[2..]
                    .iter()
                    .filter(|x| !matches!(**x, "rush" | "then"))
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !d.resolve(&r).starts_with("exit-") {
                    let (rush, then) = (w.contains(&"rush"), w.contains(&"then"));
                    return match d.ask_go(agent, obs, &r, false, rush, then) {
                        Ok(None) => Ok(Vec::new()),
                        Ok(Some(q)) => Ok(vec![q]),
                        Err(why) => Ok(vec![format!("couldn't: {why}")]),
                    };
                }
            }
            // (sure: leave the level even with something left, as Evil's sure; rush: press on
            // mid-fight, as Evil's rush; either last, in either order)
            let mut w: Vec<&str> = w.to_vec();
            let (mut sure, mut rush, mut then) = (false, false, false);
            while w.len() > 2 && matches!(w.last(), Some(&"sure") | Some(&"rush") | Some(&"then")) {
                match w.pop() {
                    Some("sure") => sure = true,
                    Some("then") => then = true,
                    _ => rush = true,
                }
            }
            let w = &w[..];
            let leaving = match w.get(1).copied() {
                Some("exit") => true,
                Some("go") => {
                    d.resolve(&w[2..].join(" ")).starts_with("exit-")
                        || w.len() > 3
                            && d.resolve(&w[2..w.len() - 1].join(" ")).starts_with("exit-")
                }
                _ => false,
            };
            if leaving && !sure {
                if let Some(q) = d.ask_leave(agent, obs) {
                    return Ok(vec![q]);
                }
            }
            let o = match w.get(1).copied() {
                Some("go") => match w.last() {
                    Some(&"risky") => Order::Go(w[2..w.len() - 1].join(" "), true),
                    _ => Order::Go(w[2..].join(" "), false),
                },
                Some("explore" | "search") => {
                    // (exploring searches the likeliest walls as it goes, 1; a search 2, or 3 if
                    // said; `secrets N` still says how hard, either way)
                    let at = w.iter().rposition(|x| *x == "secrets").filter(|&i| i > 2);
                    let searching = w.get(1) == Some(&"search");
                    let said = w.last().and_then(|n| n.parse::<u8>().ok()).filter(|_| searching && w.len() > 3);
                    let secrets = at.map_or(said.unwrap_or(if searching { 2 } else { d.policy.explore_secrets() }), |i| w.get(i + 1).and_then(|n| n.parse::<u8>().ok()).unwrap_or(2).min(3));
                    let end = at.unwrap_or(if said.is_some() { w.len() - 1 } else { w.len() });
                    let r = w[2..end].join(" ");
                    bearing(&r).map_or(Order::ExploreArea(r, secrets), |b| Order::ExploreWay(b, secrets))
                }
                Some("exit") => Order::Exit,
                Some("stop") => Order::Stop,
                _ => return Err("order go REF [risky] [sure] [rush] | explore REF|DIR | search REF|DIR [2|3] | exit [sure] | stop".into()),
            };
            d.order_with(o, rush, then);
            Ok(Vec::new())
        }
        Some("fight") => {
            // (the last word a number: for that many seconds)
            let secs = w
                .last()
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|_| w.len() > 2);
            let end = w.len() - usize::from(secs.is_some());
            let r = (end > 2).then(|| w[2..end].join(" "));
            d.fight(
                agent,
                obs,
                w.get(1).copied().unwrap_or(""),
                r.as_deref(),
                secs,
            )
            .map(|said| vec![said])
        }
        Some("idle") => {
            // (with no order this many seconds she carries on by herself; `idle wait`: she waits)
            d.idle_auto = w
                .get(1)
                .and_then(|v| v.parse::<f64>().ok())
                .map(|secs| (secs * 35.0) as i64);
            Ok(vec![match d.idle_auto {
                Some(t) => format!("with no action for {} s you carry on by instinct", t / 35),
                None => "with no action you wait".to_string(),
            }])
        }
        Some("look") => Ok(d.look(agent, obs)),
        Some("policy") => match (w.get(1), w.get(2)) {
            (Some(setting), Some(v)) => d.set_policy(obs, setting, v).map(|s| vec![s]),
            _ => Ok(vec![format!("Your policy: {}.", d.policy.line())]),
        },
        Some("answer") => {
            let id = w
                .get(1)
                .filter(|_| w.len() > 2)
                .ok_or("answer ID CHOICE (answer q3 a)")?;
            d.answer(agent, obs, id, &w[2..].join(" "))
                .map(|said| vec![said])
        }
        Some("answers") => {
            // (seconds a question waits before its default; `wait`: till answered)
            d.answer_wait = w
                .get(1)
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|s| *s > 0.0)
                .map(|s| (s * 35.0) as i64);
            Ok(vec![match d.answer_wait {
                Some(t) => format!("questions wait {} s for an answer", t / 35),
                None => "questions wait for an answer".to_string(),
            }])
        }
        Some("hints") => {
            d.hints = w
                .get(1)
                .and_then(|h| Hints::parse(h))
                .ok_or("hints off|on_stall|always")?;
            Ok(vec![format!("hints {}", d.hints.name())])
        }
        Some("game") => match crate::control::game::GameAct::of_words(&w[1..])? {
            crate::control::game::GameAct::Restart => d.ask_restart(obs.tic).map(|s| vec![s]),
            _ => Err("in a level: a new game, or the next level, when it's over".into()),
        },
        Some("status") => Ok(vec![d.status(agent, obs)]),
        Some("why") => Ok(d.why(agent, obs).lines().map(str::to_string).collect()),
        Some("check") => {
            let risky = w.last() == Some(&"risky");
            let r = w[1..w.len() - usize::from(risky)].join(" ");
            Ok(vec![match d.check_go(agent, obs, &r, risky) {
                Ok(what) => format!("could go: {what}"),
                Err(why) => format!("couldn't: {why}"),
            }])
        }
        Some("where") => {
            let x = agent.x();
            let doing = x
                .current
                .as_ref()
                .map_or("waiting", |o| o.label.as_str());
            Ok(vec![format!(
                "at ({:.0}, {:.0}), facing {:.0}, health {:.0}, armor {:.0}, holding {}, {:.1}s; doing: {doing}",
                obs.x,
                obs.y,
                obs.angle,
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0),
                d.holding(obs),
                obs.tic as f64 / 35.0
            )])
        }
        Some("log") => {
            let n = w.get(1).and_then(|v| v.parse().ok()).unwrap_or(40);
            let log = &agent.x().log;
            let own: Vec<&String> = log
                .iter()
                .filter(|l| !l.starts_with(">>") && !l.starts_with("<<"))
                .collect();
            Ok(own[own.len().saturating_sub(n)..]
                .iter()
                .map(|l| format!("| {l}"))
                .collect())
        }
        _ => Err(format!(
            "unknown command \"{line}\": list, look, name, order, check, fight, idle, hints, status, where, log"
        )),
    }
}
