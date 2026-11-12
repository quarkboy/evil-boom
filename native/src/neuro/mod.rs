//! The Neuro integration (docs/NEURO_INTEGRATION_PLAN.md): Evil directs the bot through the Neuro
//! games SDK's WebSocket protocol. `link`: the connection; `session`: the game's side of it;
//! `repair`: her action's data mended where its meaning is plain; `mock`: a stand-in Neuro server for testing (actions sent from a file, everything shown);
//! `gpt`: a stand-in with a language model playing Evil (config/neuro-gpt.json).

pub mod gpt;
pub mod link;
pub mod mock;
pub mod repair;
pub mod session;
