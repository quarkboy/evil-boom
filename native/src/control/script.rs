//! Directed play's commands as text, one a line: what a scripted commander (a `drive` script, or
//! someone typing into the file a watched game reads) says, and what comes back.
//!
//!   list TOPIC [all] [N] what she knows of: areas (those explored only with all), doors, items,
//!                       monsters, exits
//!   name ID NAME...     a name for something listed (refused when too long or taken)
//!   order go REF [risky] | order explore REF|DIR | order exit | order stop
//!                       an order (REF an id or a name; DIR n, ne, east, ...); `then` last: after
//!                       the order in hand, not instead of it
//!   check REF [risky]   could she go there now (as doom_go asks), and if not, why
//!   idle SECONDS|wait   with no order that long she carries on by herself (not leaving the
//!                       level); wait: she waits, as long as it takes
//!   status              her health, armor, weapons, ammo, keys, what she's doing
//!   where               where she is, her health, what she's doing
//!   log [N]             her log's last N lines (40), as the watcher shows it

use super::directed::{bearing, Director, Order, Topic};
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
            let (entries, hidden) = d.brief(agent, obs, topic, all);
            let mut out = vec![format!("{} known", entries.len())];
            out.extend(
                entries
                    .iter()
                    .take(n)
                    .map(|e| format!("  {}", d.describe(e, obs))),
            );
            if hidden > 0 {
                out.push(format!(
                    "  and {hidden} explored areas (list areas all to see them)"
                ));
            }
            Ok(out)
        }
        Some("name") => {
            let id = w.get(1).ok_or("name ID NAME")?;
            d.name(id, &w[2..].join(" "))
                .map(|()| vec![format!("named {id} \"{}\"", w[2..].join(" "))])
        }
        Some("order") => {
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
                if let Some(q) = d.exit_check(agent, obs) {
                    return Ok(vec![format!("{q} (order ... sure)")]);
                }
            }
            let o = match w.get(1).copied() {
                Some("go") => match w.last() {
                    Some(&"risky") => Order::Go(w[2..w.len() - 1].join(" "), true),
                    _ => Order::Go(w[2..].join(" "), false),
                },
                Some("explore") => {
                    // (secrets N: the walls searched that thoroughly, 1-3; secrets alone, 2)
                    let at = w.iter().rposition(|x| *x == "secrets").filter(|&i| i > 2);
                    let secrets = at.map_or(0, |i| w.get(i + 1).and_then(|n| n.parse::<u8>().ok()).unwrap_or(2).min(3));
                    let r = w[2..at.unwrap_or(w.len())].join(" ");
                    bearing(&r).map_or(Order::ExploreArea(r, secrets), |b| Order::ExploreWay(b, secrets))
                }
                Some("exit") => Order::Exit,
                Some("stop") => Order::Stop,
                _ => return Err("order go REF [risky] [sure] [rush] | explore REF|DIR [secrets 1-3] | exit [sure] | stop".into()),
            };
            d.order_with(o, rush, then);
            Ok(Vec::new())
        }
        Some("idle") => {
            // (with no order this many seconds she carries on by herself; `idle wait`: she waits)
            d.idle_auto = w
                .get(1)
                .and_then(|v| v.parse::<f64>().ok())
                .map(|secs| (secs * 35.0) as i64);
            Ok(vec![match d.idle_auto {
                Some(t) => format!("with no order for {} s she carries on by herself", t / 35),
                None => "with no order she waits".to_string(),
            }])
        }
        Some("status") => Ok(vec![d.status(agent, obs)]),
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
                .map_or("waiting for orders", |o| o.label.as_str());
            Ok(vec![format!(
                "at ({:.0}, {:.0}), facing {:.0}, health {:.0}, armor {:.0}, {:.1}s; doing: {doing}",
                obs.x,
                obs.y,
                obs.angle,
                obs.health.unwrap_or(0.0),
                obs.armor.unwrap_or(0.0),
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
            "unknown command \"{line}\": list, name, order, status, where, log"
        )),
    }
}
