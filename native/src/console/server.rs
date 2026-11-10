//! `evil-boom console`: the supervisor. Serves the page (`page.html`, baked in) and its API over
//! HTTP; keeps the settings; starts the game (`evil-boom watch ... --control ADDR`) as a child,
//! stops and restarts it; relays the page's commands to it and its state and log to the page.
//!
//! Every API call carries the token (`?t=`), from this machine too: without it any web page open
//! in the streamer's browser could post to the console. It's made once and kept, with the
//! settings, in config/console.local.json (not committed). The page polls (`/api/poll`) a few
//! times a second: one or two pages, on a LAN, need nothing cleverer.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::process::{Child, Command as Process, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use super::{Command, FromGame, GameState, PROTOCOL};

const PAGE: &str = include_str!("page.html");
/// log lines kept for pages that come late
const LOG_KEEP: usize = 3000;
/// at most this many lines in one poll's answer
const POLL_LINES: usize = 500;
/// a request's body: at most this (settings and commands are small)
const BODY_MAX: u64 = 64 * 1024;
/// a game asked to quit given this long before it's killed
const QUIT_WAIT: Duration = Duration::from_secs(3);
/// a connected game silent this long is taken for hung and killed (generous: a level change
/// works out the new level's analysis in the game's own loop, seconds on a big map)
const HANG: Duration = Duration::from_secs(45);
/// a game not connected this long after it was started: hung too (the first level's analysis,
/// and DSDA's start, come first)
const HELLO_WAIT: Duration = Duration::from_secs(90);
/// a crashed game is started again after this long...
const RESTART_DELAY: Duration = Duration::from_secs(2);
/// ...unless it's crashed this many times in this long: then it's left for the streamer
const CRASH_LIMIT: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_secs(120);

/// How the game's started: what changing needs a new game process.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// an IWAD by its short name: "doom", "doom2", "tnt", "plutonia" (never a path)
    pub wad: String,
    pub map: String,
    /// 1-5 (3 Hurt Me Plenty, 4 Ultra-Violence); 0: no monsters
    pub skill: i32,
    /// damage hurts her (off: god mode, for rehearsing)
    pub mortal: bool,
    pub lives: i32,
    /// a fixed seed (testing); none: a random one each start (real play)
    pub seed: Option<u32>,
    /// on through the episode, level after level (one life)
    pub episode: bool,
    /// who directs her: "bot" (on her own), "neuro" (Neuro at `neuro_url`), "gpt" (the GPT stand-in)
    pub mode: String,
    pub neuro_url: String,
    /// the game's sound muted at the start (and unmutable from the page: the sound's still there)
    pub muted: bool,
    /// the loudness each game's given as it starts: sfx, music (0-15), master (0-100); none: as
    /// the game's own settings have it. Changed from the page as it plays.
    pub sfx_volume: Option<i32>,
    pub music_volume: Option<i32>,
    pub master_volume: Option<i32>,
    /// a game that crashes (or hangs) started again, at the level she was on
    pub auto_restart: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { wad: "doom2".into(), map: "MAP01".into(), skill: 3, mortal: true, lives: 1, seed: None, episode: true, mode: "bot".into(), neuro_url: "ws://127.0.0.1:8000".into(), muted: false, sfx_volume: None, music_volume: None, master_volume: None, auto_restart: true }
    }
}

impl Settings {
    fn check(&self) -> Result<(), String> {
        if !crate::config::iwad_names().any(|n| n == self.wad) {
            // (a name, never a path: the page can't reach for files on the PC)
            return Err(format!("wad: one of {}", crate::config::iwad_names().collect::<Vec<_>>().join(", ")));
        }
        crate::engine::episode_map(&self.map)?;
        if !(0..=5).contains(&self.skill) {
            return Err("skill: 1-5, or 0 for no monsters".into());
        }
        if !(1..=99).contains(&self.lives) {
            return Err("lives: 1-99".into());
        }
        if !["bot", "neuro", "gpt"].contains(&self.mode.as_str()) {
            return Err("mode: bot, neuro or gpt".into());
        }
        if self.mode == "neuro" && !self.neuro_url.starts_with("ws://") {
            return Err("neuro_url: ws://host:port".into());
        }
        Ok(())
    }

    /// `watch`'s arguments for these settings, its link to the console at `control`.
    fn args(&self, control: &str) -> Vec<String> {
        let mut a: Vec<String> = vec!["watch".into(), self.wad.clone(), self.map.to_uppercase()];
        if self.skill > 0 {
            a.extend(["--fight".into(), self.skill.to_string()]);
        }
        if self.mortal {
            a.push("--mortal".into());
        }
        a.extend(["--lives".into(), self.lives.to_string()]);
        a.extend(["--seed".into(), self.seed.map_or_else(|| "random".into(), |s| s.to_string())]);
        if self.episode {
            a.push("--episode".into());
        }
        match self.mode.as_str() {
            "neuro" => a.extend(["--neuro".into(), self.neuro_url.clone()]),
            "gpt" => a.push("--gpt".into()),
            _ => {}
        }
        a.extend(["--control".into(), control.to_string()]);
        a
    }
}

/// What's kept between runs of the console (config/console.local.json).
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    token: String,
    settings: Settings,
    /// the Neuro addresses the page may choose among: edited in the file only, on the PC (from the
    /// page, any address would let whoever has it point the PC's connection at any host)
    neuro_urls: Vec<String>,
}

impl Default for Saved {
    fn default() -> Saved {
        Saved { token: String::new(), settings: Settings::default(), neuro_urls: vec!["ws://127.0.0.1:8000".into()] }
    }
}

/// The game process as the page sees it.
#[derive(Clone, Debug, Default, Serialize)]
struct GameProc {
    running: bool,
    pid: Option<u32>,
    /// how it last ended ("stopped", "exit status: 0", ...)
    last_exit: Option<String>,
    /// the console asked it to stop: its ending isn't a crash
    #[serde(skip)]
    stopping: bool,
    #[serde(skip)]
    started: Option<Instant>,
    /// to be muted as soon as it's connected
    #[serde(skip)]
    start_muted: bool,
    /// killed for hanging (once)
    #[serde(skip)]
    hung: bool,
}

struct Shared {
    settings: Settings,
    token: String,
    neuro_urls: Vec<String>,
    saved_path: PathBuf,
    /// where the game connects back to (127.0.0.1, a port of the system's choosing)
    control_addr: String,
    game: GameProc,
    child: Option<Child>,
    /// the link to the running game, once it's said hello
    to_game: Option<TcpStream>,
    hello: Option<Value>,
    state: Option<GameState>,
    log: VecDeque<(u64, String)>,
    seq: u64,
    /// the game's last word (hello or state): silence too long is a hang
    heard: Option<Instant>,
    /// when the game last crashed, the last few
    crashes: VecDeque<Instant>,
    /// the session's log, everything the page shows (out/console/)
    file: Option<std::fs::File>,
    began: Instant,
}

impl Shared {
    fn push(&mut self, line: String) {
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "[{:8.1}s] {line}", self.began.elapsed().as_secs_f64());
        }
        self.seq += 1;
        self.log.push_back((self.seq, line));
        while self.log.len() > LOG_KEEP {
            self.log.pop_front();
        }
    }

    fn save(&self) -> Result<(), String> {
        let saved = Saved { token: self.token.clone(), settings: self.settings.clone(), neuro_urls: self.neuro_urls.clone() };
        let text = serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())?;
        if let Some(dir) = self.saved_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.saved_path, text).map_err(|e| format!("{}: {e}", self.saved_path.display()))
    }

    fn send(&mut self, c: &Command) -> Result<(), String> {
        let line = serde_json::to_string(c).map_err(|e| e.to_string())?;
        let stream = self.to_game.as_mut().ok_or("no game connected")?;
        writeln!(stream, "{line}").and_then(|()| stream.flush()).map_err(|e| format!("the game didn't take it: {e}"))
    }
}

type Hub = Arc<Mutex<Shared>>;

fn lock(hub: &Hub) -> MutexGuard<'_, Shared> {
    // (a thread that panicked holding it left nothing half-done that matters more than going on)
    hub.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Serve the console on `bind` (host:port) till the program's stopped; `new_token`: the old token
/// no longer works (one that's got out).
pub fn serve(bind: &str, new_token_wanted: bool) -> Result<(), String> {
    let saved_path = crate::config::root().join("config").join("console.local.json");
    let saved: Saved = match std::fs::read_to_string(&saved_path) {
        // (a file that's there but can't be read isn't written over: it's the streamer's)
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", saved_path.display()))?,
        Err(_) => Saved::default(),
    };
    let token = if saved.token.len() >= 16 && !new_token_wanted { saved.token } else { new_token() };
    let control = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("the game's link: {e}"))?;
    let control_addr = control.local_addr().map_err(|e| e.to_string())?.to_string();
    let server = Server::http(bind).map_err(|e| format!("{bind}: {e}"))?;
    let port = server.server_addr().to_ip().map_or(0, |a| a.port());
    let hub: Hub = Arc::new(Mutex::new(Shared {
        settings: saved.settings,
        token: token.clone(),
        neuro_urls: saved.neuro_urls,
        saved_path,
        control_addr,
        game: GameProc::default(),
        child: None,
        to_game: None,
        hello: None,
        state: None,
        log: VecDeque::new(),
        seq: 0,
        heard: None,
        crashes: VecDeque::new(),
        file: session_log(),
        began: Instant::now(),
    }));
    lock(&hub).save()?;
    {
        let hub = Arc::clone(&hub);
        thread::spawn(move || accept_games(&control, &hub));
    }
    {
        let hub = Arc::clone(&hub);
        thread::spawn(move || watch_child(&hub));
    }
    println!("console: http://127.0.0.1:{port}/?t={token}");
    if !bind.starts_with("127.") && !bind.starts_with("localhost") {
        if let Some(ip) = lan_ip() {
            println!("  on the LAN: http://{ip}:{port}/?t={token}");
        }
        println!("  (under WSL2 the LAN reaches it only with mirrored networking, or a port proxy on Windows)");
    }
    lock(&hub).push(format!("[console] serving on {bind}"));
    for req in server.incoming_requests() {
        handle(&hub, req);
    }
    Ok(())
}

/// The session's log file, out/console/<commit>-<time>.log (none if it can't be made: the page
/// has it all the same).
fn session_log() -> Option<std::fs::File> {
    let dir = crate::config::out_dir().join("console");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.log", crate::config::run_stamp()));
    println!("  log: {}", path.display());
    std::fs::File::create(path).ok()
}

/// A token: 128 bits from the standard library's randomly keyed hasher (keys from the system's
/// randomness), as hex.
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(now);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// This machine's address on the LAN (the one its default route goes out by; nothing is sent).
fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// The game processes connecting back: each one's lines read; the one that says hello with the
/// running child's pid is the game.
fn accept_games(control: &TcpListener, hub: &Hub) {
    for stream in control.incoming().map_while(Result::ok) {
        let hub = Arc::clone(hub);
        thread::spawn(move || read_game(stream, &hub));
    }
}

fn read_game(stream: TcpStream, hub: &Hub) {
    let Ok(writer) = stream.try_clone() else { return };
    writer.set_write_timeout(Some(Duration::from_secs(1))).ok();
    let mut writer = Some(writer);
    let mut ours = false;
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let Ok(m) = serde_json::from_str::<FromGame>(&line) else { continue };
        let mut s = lock(hub);
        match m {
            FromGame::Hello { protocol, pid, commit, wad, map, maps } => {
                if s.game.pid != Some(pid) {
                    s.push(format!("[console] a game not started here tried to connect (pid {pid}): refused"));
                    return;
                }
                if protocol != PROTOCOL {
                    s.push(format!("[console] the game speaks protocol {protocol}, this console {PROTOCOL}: rebuild one of them"));
                }
                s.push(format!("[console] game connected: {wad} {map}, build {commit}"));
                s.hello = Some(json!({ "pid": pid, "commit": commit, "wad": wad, "map": map, "maps": maps }));
                s.heard = Some(Instant::now());
                s.to_game = writer.take();
                ours = true;
                if s.game.start_muted {
                    let _ = s.send(&Command::Mute { on: true });
                }
                let (sfx, music, master) = (s.settings.sfx_volume, s.settings.music_volume, s.settings.master_volume);
                if sfx.is_some() || music.is_some() || master.is_some() {
                    let _ = s.send(&Command::Volume { sfx, music, master });
                }
            }
            _ if !ours => return,
            FromGame::State(st) => {
                s.state = Some(st);
                s.heard = Some(Instant::now());
            }
            FromGame::Log { lines } => lines.into_iter().for_each(|l| s.push(l)),
            FromGame::Event { what } => s.push(format!("[event] {what}")),
        }
    }
    if ours {
        let mut s = lock(hub);
        s.to_game = None;
        s.push("[console] the game's link closed".into());
    }
}

/// The child's life: its ending noticed and told; a crash restarted (unless it keeps crashing);
/// a hang killed (and so restarted).
fn watch_child(hub: &Hub) {
    loop {
        thread::sleep(Duration::from_millis(200));
        let mut s = lock(hub);
        // (Some((how, crashed)): it's ended; a crash is any end not asked for and not clean)
        let ended = match s.child.as_mut().map(Child::try_wait) {
            Some(Ok(Some(status))) => Some((status.to_string(), !status.success())),
            Some(Err(e)) => Some((e.to_string(), true)),
            _ => None,
        };
        let Some((how, crashed)) = ended else {
            check_hang(&mut s);
            continue;
        };
        let crashed = crashed && !s.game.stopping;
        let how = if s.game.stopping { "stopped".to_string() } else if s.game.hung { format!("{how} (hung)") } else { how };
        s.push(format!("[console] the game ended: {how}"));
        // (where she was, to go on from: the same WAD's level, as the game last told it)
        let same_wad = s.hello.as_ref().and_then(|h| h.get("wad")).and_then(Value::as_str) == Some(s.settings.wad.as_str());
        let at = s.state.as_ref().filter(|_| same_wad).map(|st| st.map.clone());
        s.child = None;
        s.to_game = None;
        s.game = GameProc { last_exit: Some(how), ..GameProc::default() };
        if crashed && s.settings.auto_restart {
            let now = Instant::now();
            s.crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
            s.crashes.push_back(now);
            if s.crashes.len() >= CRASH_LIMIT {
                let n = s.crashes.len();
                s.push(format!("[console] {n} crashes in {}s: not started again (start it from the page)", CRASH_WINDOW.as_secs()));
                s.crashes.clear();
            } else {
                s.push(format!("[console] starting it again in {}s{}", RESTART_DELAY.as_secs(), at.as_ref().map_or(String::new(), |m| format!(", at {m}"))));
                let hub = Arc::clone(hub);
                thread::spawn(move || {
                    thread::sleep(RESTART_DELAY);
                    if lock(&hub).child.is_none() {
                        if let Err(e) = start_at(&hub, at) {
                            lock(&hub).push(format!("[console] not started again: {e}"));
                        }
                    }
                });
            }
        }
    }
}

/// A game that's gone silent killed (its end then a crash's, restarted as one is).
fn check_hang(s: &mut Shared) {
    if s.child.is_none() || s.game.stopping || s.game.hung {
        return;
    }
    let silent = match (s.to_game.is_some(), s.heard, s.game.started) {
        (true, Some(t), _) => t.elapsed() > HANG,
        (false, _, Some(t)) => t.elapsed() > HELLO_WAIT,
        _ => false,
    };
    if silent {
        let waited = if s.to_game.is_some() { HANG } else { HELLO_WAIT };
        s.push(format!("[console] no word from the game in {}s: killed as hung", waited.as_secs()));
        s.game.hung = true;
        if let Some(c) = s.child.as_mut() {
            let _ = c.kill();
        }
    }
}

/// Start the game with the settings as they are now.
fn start(hub: &Hub) -> Result<(), String> {
    start_at(hub, None)
}

/// Start the game with the settings as they are now, at `map` if one's given (going on after a
/// crash) rather than the settings' own.
fn start_at(hub: &Hub, map: Option<String>) -> Result<(), String> {
    let mut s = lock(hub);
    if s.child.is_some() {
        return Err("the game's running".into());
    }
    let mut settings = s.settings.clone();
    if let Some(m) = map {
        settings.map = m;
    }
    settings.check()?;
    let args = settings.args(&s.control_addr);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Process::new(&exe).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
    for (out, tag) in [(child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>), "out"), (child.stderr.take().map(|o| Box::new(o) as Box<dyn Read + Send>), "err")] {
        if let Some(out) = out {
            let hub = Arc::clone(hub);
            thread::spawn(move || {
                for l in BufReader::new(out).lines().map_while(Result::ok) {
                    lock(&hub).push(format!("[{tag}] {l}"));
                }
            });
        }
    }
    s.push(format!("[console] started: evil-boom {}", args.join(" ")));
    s.game = GameProc { running: true, pid: Some(child.id()), last_exit: s.game.last_exit.take(), stopping: false, started: Some(Instant::now()), start_muted: s.settings.muted, hung: false };
    s.heard = None;
    s.child = Some(child);
    s.hello = None;
    s.state = None;
    Ok(())
}

/// Stop the game: asked to quit, killed if it hasn't after a while. Waits for it.
fn stop(hub: &Hub) {
    {
        let mut s = lock(hub);
        if s.child.is_none() {
            return;
        }
        s.game.stopping = true;
        if s.send(&Command::Quit).is_err() {
            if let Some(c) = s.child.as_mut() {
                let _ = c.kill();
            }
        }
    }
    let asked = Instant::now();
    let mut killed = false;
    loop {
        thread::sleep(Duration::from_millis(100));
        let mut s = lock(hub);
        // (watch_child notices the end and clears it)
        if s.child.is_none() {
            return;
        }
        if !killed && asked.elapsed() > QUIT_WAIT {
            killed = true;
            s.push("[console] the game didn't quit: killed".into());
            if let Some(c) = s.child.as_mut() {
                let _ = c.kill();
            }
        }
    }
}

fn handle(hub: &Hub, mut req: Request) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let param = |k: &str| query.split('&').filter_map(|kv| kv.split_once('=')).find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    let method = req.method().clone();
    // (behind someone's proxy or tunnel it may be under a path of theirs, /doom/: what's from
    // /api/ on is the call, and the page asks by paths relative to itself)
    let Some(path) = path.find("/api/").map(|i| &path[i..]) else {
        let file = path.rsplit('/').next().is_some_and(|last| last.contains('.') && last != "index.html");
        let _ = req.respond(if method != Method::Get || file {
            Response::from_string("not found").with_status_code(404)
        } else if path.ends_with('/') || path.ends_with("/index.html") {
            // (the page is the same for anyone: nothing in it is secret, the API is)
            with_header(Response::from_string(PAGE), "Content-Type", "text/html; charset=utf-8")
        } else {
            // (/doom to /doom/, so the page's relative calls go under it)
            let to = if query.is_empty() { format!("{path}/") } else { format!("{path}/?{query}") };
            with_header(Response::from_string("").with_status_code(308), "Location", &to)
        });
        return;
    };
    let token = lock(hub).token.clone();
    if !param("t").is_some_and(|t| same(&t, &token)) {
        let _ = req.respond(json_response(401, &json!({ "error": "the console's token is needed (?t=, as printed when it started)" })));
        return;
    }
    // (who's asking, for the log: the login a tunnel in front vouches for, else where it's from)
    let who = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Cf-Access-Authenticated-User-Email"))
        .map(|h| h.value.as_str().to_string())
        .or_else(|| req.remote_addr().map(|a| a.ip().to_string()))
        .unwrap_or_else(|| "?".into());
    let mut body = String::new();
    if method == Method::Post && req.as_reader().take(BODY_MAX).read_to_string(&mut body).is_err() {
        let _ = req.respond(json_response(400, &json!({ "error": "the body wasn't text" })));
        return;
    }
    if method == Method::Post && !body.starts_with(r#"{"cmd":"volume""#) {
        // (what each did, and who; not the volume, a slider sends many)
        let what = path.trim_start_matches("/api/");
        lock(hub).push(format!("[console] {who}: {what} {}", body.chars().take(300).collect::<String>()));
    }
    let (code, answer) = match (method, path) {
        (Method::Get, "/api/poll") => (200, poll(hub, param("since").and_then(|v| v.parse().ok()).unwrap_or(0))),
        (Method::Post, "/api/game/start") => done(start(hub)),
        (Method::Post, "/api/game/stop") => {
            let hub = Arc::clone(hub);
            thread::spawn(move || stop(&hub));
            done(Ok(()))
        }
        (Method::Post, "/api/game/restart") => {
            let hub = Arc::clone(hub);
            thread::spawn(move || {
                stop(&hub);
                if let Err(e) = start(&hub) {
                    lock(&hub).push(format!("[console] not started: {e}"));
                }
            });
            done(Ok(()))
        }
        (Method::Post, "/api/cmd") => match serde_json::from_str::<Command>(&body) {
            Ok(c) => {
                let mut s = lock(hub);
                if let Command::Volume { sfx, music, master } = c {
                    // (kept for the next game as well; not logged, a slider sends many)
                    let st = &mut s.settings;
                    st.sfx_volume = sfx.map(|v| v.clamp(0, 15)).or(st.sfx_volume);
                    st.music_volume = music.map(|v| v.clamp(0, 15)).or(st.music_volume);
                    st.master_volume = master.map(|v| v.clamp(0, 100)).or(st.master_volume);
                    let saved = s.save();
                    let sent = if s.to_game.is_some() { s.send(&c) } else { Ok(()) };
                    done(saved.and(sent))
                } else {
                    done(s.send(&c))
                }
            }
            Err(e) => (400, json!({ "error": format!("not a command: {e}") })),
        },
        (Method::Post, "/api/settings") => match serde_json::from_str::<Settings>(&body) {
            Ok(new) => match new.check().and_then(|()| {
                let urls = lock(hub).neuro_urls.clone();
                if new.mode == "neuro" && !urls.contains(&new.neuro_url) {
                    return Err(format!("neuro_url: one of {} (the console's list: neuro_urls in config/console.local.json)", urls.join(", ")));
                }
                Ok(())
            }) {
                Ok(()) => {
                    let mut s = lock(hub);
                    let changed = s.settings != new;
                    s.settings = new;
                    let running = s.child.is_some();
                    match s.save() {
                        Ok(()) => (200, json!({ "ok": true, "restart_needed": changed && running })),
                        Err(e) => (500, json!({ "error": e })),
                    }
                }
                Err(e) => (400, json!({ "error": e })),
            },
            Err(e) => (400, json!({ "error": format!("not settings: {e}") })),
        },
        _ => (404, json!({ "error": "no such call" })),
    };
    let _ = req.respond(json_response(code, &answer));
}

/// How things stand, and the log after line `since`.
fn poll(hub: &Hub, since: u64) -> Value {
    let s = lock(hub);
    // (a page from before the console was restarted: everything kept, from the start)
    let since = if since > s.seq { 0 } else { since };
    let log: Vec<&(u64, String)> = s.log.iter().filter(|(n, _)| *n > since).take(POLL_LINES).collect();
    let seq = log.last().map_or(s.seq, |(n, _)| *n);
    json!({
        "settings": s.settings,
        "neuro_urls": s.neuro_urls,
        "game": s.game,
        "uptime": s.game.started.map(|t| t.elapsed().as_secs()),
        "connected": s.to_game.is_some(),
        "hello": s.hello,
        "state": s.state,
        "log": log,
        "seq": seq,
    })
}

fn done(r: Result<(), String>) -> (u16, Value) {
    match r {
        Ok(()) => (200, json!({ "ok": true })),
        Err(e) => (409, json!({ "error": e })),
    }
}

fn json_response(code: u16, v: &Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let r = with_header(Response::from_string(v.to_string()).with_status_code(code), "Content-Type", "application/json");
    with_header(r, "Cache-Control", "no-store")
}

fn with_header<R: Read>(r: Response<R>, name: &str, value: &str) -> Response<R> {
    // (the names and values here are fixed ASCII: never refused)
    match Header::from_bytes(name.as_bytes(), value.as_bytes()) {
        Ok(h) => r.with_header(h),
        Err(()) => r,
    }
}

/// The same token, compared without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_checked() {
        assert!(Settings::default().check().is_ok());
        assert!(Settings { map: "MAP99X".into(), ..Settings::default() }.check().is_err());
        assert!(Settings { skill: 6, ..Settings::default() }.check().is_err());
        assert!(Settings { wad: "/etc/passwd".into(), ..Settings::default() }.check().is_err());
        assert!(Settings { wad: "doom".into(), map: "E1M1".into(), ..Settings::default() }.check().is_ok());
        assert!(Settings { mode: "chaos".into(), ..Settings::default() }.check().is_err());
        assert!(Settings { mode: "neuro".into(), neuro_url: "http://x".into(), ..Settings::default() }.check().is_err());
    }

    #[test]
    fn settings_make_watch_args() {
        let a = (Settings { skill: 4, seed: Some(7), mode: "neuro".into(), ..Settings::default() }).args("127.0.0.1:1");
        let line = a.join(" ");
        assert!(line.starts_with("watch doom2 MAP01 --fight 4 --mortal --lives 1 --seed 7 --episode --neuro ws://127.0.0.1:8000"), "{line}");
        assert!(line.ends_with("--control 127.0.0.1:1"), "{line}");
        let none = (Settings { skill: 0, mortal: false, muted: true, ..Settings::default() }).args("127.0.0.1:1").join(" ");
        assert!(!none.contains("--fight") && !none.contains("--mortal") && none.contains("--seed random"), "{none}");
        // (muted by a command once it's up, not --mute: that takes the sound away for the run)
        assert!(!none.contains("--mute"), "{none}");
    }

    #[test]
    fn tokens_compared() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
        assert_eq!(new_token().len(), 32);
        assert_ne!(new_token(), new_token());
    }

    #[test]
    fn partial_settings_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"map":"E1M1","wad":"doom"}"#).unwrap_or_default();
        assert_eq!((s.wad.as_str(), s.map.as_str(), s.skill), ("doom", "E1M1", 3));
    }
}
