//! The game's end of the console's link (`watch --control ADDR`): connected to the supervisor
//! once, at the start; a thread reading its commands, one writing what the game says. The game
//! never waits on either, and plays on if the console goes (the stream isn't the console's).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use super::{Command, FromGame, PROTOCOL};

/// the game's state sent this often
const STATE_EVERY: Duration = Duration::from_millis(250);

pub struct Link {
    out: Sender<FromGame>,
    commands: Receiver<Command>,
    /// lost: told once in the log
    lost: Receiver<String>,
    last_state: Option<Instant>,
}

impl Link {
    /// Connect to the supervisor at `addr` (host:port) and say who this is.
    pub fn connect(addr: &str, wad: &str, map: &str, maps: Vec<String>) -> Result<Link, String> {
        let stream = TcpStream::connect(addr).map_err(|e| format!("the console at {addr}: {e}"))?;
        stream.set_nodelay(true).ok();
        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let (out, out_rx) = channel::<FromGame>();
        let (cmd_tx, commands) = channel::<Command>();
        let (lost_tx, lost) = channel::<String>();
        let lost_w = lost_tx.clone();
        thread::spawn(move || write_all(stream, &out_rx, &lost_w));
        thread::spawn(move || read_all(reader, &cmd_tx, &lost_tx));
        let link = Link {
            out,
            commands,
            lost,
            last_state: None,
        };
        link.send(FromGame::Hello {
            protocol: PROTOCOL,
            pid: std::process::id(),
            commit: crate::config::COMMIT.to_string(),
            wad: wad.to_string(),
            map: map.to_uppercase(),
            maps,
        });
        Ok(link)
    }

    pub fn send(&self, m: FromGame) {
        let _ = self.out.send(m);
    }

    /// Another way in for what's to be sent (a thread of the game's own: the pictures').
    pub fn sender(&self) -> Sender<FromGame> {
        self.out.clone()
    }

    /// The next command come in, if there's one.
    pub fn command(&self) -> Option<Command> {
        self.commands.try_recv().ok()
    }

    /// Why the link's gone, the once it goes.
    pub fn lost(&self) -> Option<String> {
        self.lost.try_recv().ok()
    }

    /// Whether the state's due to be sent again (and, if it is, it's counted as sent).
    pub fn state_due(&mut self) -> bool {
        let now = Instant::now();
        if self.last_state.is_some_and(|t| now - t < STATE_EVERY) {
            return false;
        }
        self.last_state = Some(now);
        true
    }
}

fn write_all(mut stream: TcpStream, out: &Receiver<FromGame>, lost: &Sender<String>) {
    for m in out {
        let Ok(line) = serde_json::to_string(&m) else {
            continue;
        };
        if let Err(e) = writeln!(stream, "{line}").and_then(|()| stream.flush()) {
            let _ = lost.send(e.to_string());
            return;
        }
    }
}

fn read_all(stream: TcpStream, commands: &Sender<Command>, lost: &Sender<String>) {
    for line in BufReader::new(stream).lines() {
        match line {
            // (a line it can't read is dropped: the supervisor checked it, a newer one may know more)
            Ok(l) => {
                if let Ok(c) = serde_json::from_str::<Command>(&l) {
                    if commands.send(c).is_err() {
                        return; // (the game's gone)
                    }
                }
            }
            Err(e) => {
                let _ = lost.send(e.to_string());
                return;
            }
        }
    }
    let _ = lost.send("closed".into());
}
