//! Directed play's commands as text, one a line: what a scripted commander (a `drive` script, or
//! someone typing into the file a watched game reads) says, and what comes back.
//!
//!   list TOPIC [N]      what she knows of: areas, doors, items, monsters, exits
//!   name ID NAME...     a name for something listed (refused when too long or taken)
//!   order go REF | order explore REF|DIR | order exit | order stop
//!                       an order (REF an id or a name; DIR n, ne, east, ...)
//!   status              her health, armor, weapons, ammo, keys, what she's doing
//!   where               where she is, her health, what she's doing
//!   log [N]             her log's last N lines (40), as the watcher shows it

use super::directed::{bearing, Director, Order, Topic};
use crate::motor::Observation;
use crate::play::Agent;

/// Carry out one command; what it says back (an order's own outcome comes later, as reports).
pub fn command(d: &mut Director, agent: &mut Agent, obs: &Observation, line: &str) -> Result<Vec<String>, String> {
    let w: Vec<&str> = line.split_whitespace().collect();
    match w.first().copied() {
        Some("list") => {
            let topic = w.get(1).and_then(|t| Topic::parse(t)).ok_or("list areas|doors|items|monsters|exits [N]")?;
            let n = w.get(2).and_then(|v| v.parse().ok()).unwrap_or(20);
            let entries = d.list(agent, obs, topic);
            let mut out = vec![format!("{} known", entries.len())];
            out.extend(entries.iter().take(n).map(|e| format!("  {}", d.describe(e, obs))));
            Ok(out)
        }
        Some("name") => {
            let id = w.get(1).ok_or("name ID NAME")?;
            d.name(id, &w[2..].join(" ")).map(|()| vec![format!("named {id} \"{}\"", w[2..].join(" "))])
        }
        Some("order") => {
            let o = match w.get(1).copied() {
                Some("go") => Order::Go(w[2..].join(" ")),
                Some("explore") => {
                    let r = w[2..].join(" ");
                    bearing(&r).map_or(Order::ExploreArea(r), Order::ExploreWay)
                }
                Some("exit") => Order::Exit,
                Some("stop") => Order::Stop,
                _ => return Err("order go REF | explore REF|DIR | exit | stop".into()),
            };
            d.order(o);
            Ok(Vec::new())
        }
        Some("status") => Ok(vec![d.status(agent, obs)]),
        Some("where") => {
            let x = agent.x();
            let doing = x.current.as_ref().map_or("waiting for orders", |o| o.label.as_str());
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
            let own: Vec<&String> = log.iter().filter(|l| !l.starts_with(">>") && !l.starts_with("<<")).collect();
            Ok(own[own.len().saturating_sub(n)..].iter().map(|l| format!("| {l}")).collect())
        }
        _ => Err(format!("unknown command \"{line}\": list, name, order, status, where, log")),
    }
}
