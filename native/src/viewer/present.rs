//! Doom's own screens in the watcher, for the stream (`watch --present`, docs/CONSOLE_PLAN.md
//! stage 4): the title card (held there, no demos: the user's), the menu walked to a new game,
//! the melt between screens, a death as the game shows it, the tally and the finales with a
//! press on each as a player makes it. The engine draws and runs them (evil_api.c's
//! presentation); this says when, a tic at a time.
//!
//! The levels play as they do without it: each is started as any is (a new game: the seed, the
//! loadout), a melt holds the game rather than playing under it, and the melt's random numbers
//! are kept from the play's.

use super::{enter_level, load_level, next_level, Watch, WatchOptions};
use crate::motor::Controls;
use crate::wad::Wad;

/// on the title card this long before the menu opens (tics)
const TITLE_HOLD: i64 = 70;
/// between the skull's moves
const MENU_STEP: i64 = 9;
/// after a choice, before the next
const MENU_CHOOSE: i64 = 20;
/// her death watched this long before she's back (or it's over)
const DEATH_HOLD: i64 = 105;
/// the tally, counted, looked at this long before a press goes on
const TALLY_READ: i64 = 88;
/// a Doom II finale's text, all shown, read this long before a press
const TEXT_READ: i64 = 210;
/// still waiting after a press (a finale's first press finishes its text): pressed again after this
const PRESS_AGAIN: i64 = 45;

/// A step of the menu walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Open,
    /// the skull to this item, one move at a time
    To(i32),
    Choose,
    /// the menu shut and the level started
    Start,
}

/// The menu walked to a new game: (tics before, step), and how far it's got.
pub(super) struct Walk {
    steps: Vec<(i64, Step)>,
    at: usize,
    wait: i64,
}

impl Walk {
    /// To `map` at `skill` (1-5) as a player gets there: the main menu, New Game, the episode
    /// (Doom), the skill. A level no menu starts (not an episode's first): from the title
    /// straight in.
    fn to(map: &str, skill: i32) -> Walk {
        let m = map.to_uppercase();
        let doom1_episode = m.strip_prefix('E').filter(|r| r.ends_with("M1")).and_then(|r| r[..1].parse::<i32>().ok());
        let steps = if m == "MAP01" || doom1_episode.is_some() {
            let mut s = vec![(TITLE_HOLD, Step::Open), (MENU_STEP, Step::To(0)), (MENU_CHOOSE, Step::Choose)];
            if let Some(e) = doom1_episode {
                s.extend([(MENU_CHOOSE, Step::To(e - 1)), (MENU_CHOOSE, Step::Choose)]);
            }
            s.extend([(MENU_CHOOSE, Step::To(skill - 1)), (MENU_CHOOSE, Step::Choose), (MENU_CHOOSE, Step::Start)]);
            s
        } else {
            vec![(TITLE_HOLD, Step::Start)]
        };
        Walk { wait: steps[0].0, steps, at: 0 }
    }
}

pub(super) enum Stage {
    /// the title card; on the way to a game through the menu, or (None) waiting to be begun
    Title(Option<Walk>),
    Level,
    /// dead, the view sinking
    Dead { since: i64 },
    /// out of the level: the tally, a finale (the clock when it began waiting for a press, and
    /// when it was last pressed); `last`: the episode's last level (its end, the screens from
    /// here on the game's own for good)
    Between { waiting_since: Option<i64>, pressed: Option<i64>, last: bool },
    /// the run's over (game over: the title card); the screen as it is till it's begun again
    Over,
}

pub(super) struct Present {
    pub(super) stage: Stage,
    /// the screens' own clock (tics, the game's or a melt's)
    clock: i64,
    /// the run's first level, where a begin starts
    first_map: String,
    /// the skill the menu shows (1-5)
    skill: i32,
    /// the level loaded with the watcher hasn't been played: a begin starts it as it is
    fresh: bool,
}

impl Present {
    /// Doom's own screens on, the title card up; `wait`: there till it's begun (the console's),
    /// else on through the menu at once.
    pub(super) fn start(watch: &Watch, o: &WatchOptions, wait: bool) -> Option<Present> {
        if !watch.engine.present(true) {
            return None; // (an engine without them)
        }
        watch.engine.title();
        let skill = if o.fight > 0 { o.fight } else { 3 };
        let first_map = watch.map_name.clone();
        let stage = Stage::Title((!wait).then(|| Walk::to(&first_map, skill)));
        Some(Present { stage, clock: 0, first_map, skill, fresh: true })
    }

    /// From the title card (again) through the menu to the run's first level, whatever's on now.
    pub(super) fn begin(&mut self, watch: &Watch) {
        if !matches!(self.stage, Stage::Title(None)) {
            watch.engine.title();
        }
        self.stage = Stage::Title(Some(Walk::to(&self.first_map, self.skill)));
    }

    /// What's on, in a word (the console's).
    pub(super) fn label(&self, menu: bool) -> &'static str {
        match &self.stage {
            Stage::Title(_) if menu => "menu",
            Stage::Title(None) => "title",
            Stage::Title(Some(_)) => "title (starting)",
            Stage::Level => "level",
            Stage::Dead { .. } => "dead",
            Stage::Between { last: true, .. } => "the end",
            Stage::Between { .. } => "between levels",
            Stage::Over => "over",
        }
    }
}

/// The tics owed, as the screens have them; the watcher (a new one on a new level).
pub(super) fn run(mut watch: Watch, game: Option<(&Wad, &WatchOptions)>, owed: &mut f64) -> Result<Watch, String> {
    let mut ticked = false;
    while *owed >= 1.0 && !watch.paused {
        *owed -= 1.0;
        let Some(mut p) = watch.present.take() else { break };
        p.clock += 1;
        watch = tic(watch, &mut p, game)?;
        if watch.present.is_none() {
            watch.present = Some(p);
        }
        ticked = true;
    }
    if ticked {
        watch.frame = watch.engine.frame();
    }
    Ok(watch)
}

/// A tic of the game with nobody playing: on a screen, or dead (`press`: use, as a player
/// presses on).
fn idle(watch: &mut Watch, press: bool) {
    watch.engine.step(&Controls { use_: press, ..Controls::default() });
    let _ = watch.engine.take_hits();
}

fn tic(mut watch: Watch, p: &mut Present, game: Option<(&Wad, &WatchOptions)>) -> Result<Watch, String> {
    let screen = watch.engine.screen().unwrap_or_default();
    if screen.wiping {
        watch.engine.wipe_tic();
        return Ok(watch);
    }
    match &mut p.stage {
        Stage::Title(None) | Stage::Over => idle(&mut watch, false),
        Stage::Title(Some(walk)) => {
            if walk.wait > 0 {
                walk.wait -= 1;
                idle(&mut watch, false);
                return Ok(watch);
            }
            let step = walk.steps.get(walk.at).map_or(Step::Start, |s| s.1);
            let done = match step {
                Step::Open => {
                    watch.engine.menu(0, 0);
                    true
                }
                Step::To(t) if screen.item != t => {
                    watch.engine.menu(1, screen.item + (t - screen.item).signum());
                    walk.wait = MENU_STEP;
                    false
                }
                Step::To(_) => true,
                Step::Choose => {
                    watch.engine.menu(2, 0);
                    true
                }
                Step::Start => {
                    watch.engine.menu(3, 0);
                    watch = start_level(watch, p, game)?;
                    p.stage = Stage::Level;
                    return Ok(watch);
                }
            };
            if done {
                walk.at += 1;
                walk.wait = walk.steps.get(walk.at).map_or(0, |s| s.0);
            }
            idle(&mut watch, false);
        }
        Stage::Level => {
            if watch.engine.dead() {
                p.stage = Stage::Dead { since: p.clock };
                idle(&mut watch, false);
            } else if watch.engine.exited() {
                let last = game.is_some_and(|(_, o)| o.episode) && crate::sweep::next_map(&watch.map_name, watch.engine.secret_exit()).is_none();
                p.stage = Stage::Between { waiting_since: None, pressed: None, last };
                idle(&mut watch, false);
            } else {
                // (a level again, asked for by a 100% run, as the window's loop gives it)
                if watch.agent.x().restart_wanted {
                    watch.agent.x_mut().restart_wanted = false;
                    if watch.lives_left > 0 {
                        watch.lives_left -= 1;
                        watch.engine.restart(true);
                        watch.agent.restart();
                        watch.controls = None;
                    }
                }
                watch.tic();
            }
        }
        Stage::Dead { since } => {
            if p.clock - *since < DEATH_HOLD {
                idle(&mut watch, false);
            } else if watch.lives_left > 0 {
                // (back as a death brings her back: the level again, a life spent)
                watch.lives_left -= 1;
                watch.engine.restart(true);
                watch.agent.restart();
                watch.controls = None;
                p.stage = Stage::Level;
            } else {
                // (game over: the title card, till it's begun again)
                watch.engine.title();
                p.stage = Stage::Over;
            }
        }
        Stage::Between { waiting_since, pressed, .. } => {
            if screen.held {
                // (the game's moved on: the next level, started as an episode's is)
                match game.filter(|(_, o)| o.episode) {
                    Some((w, o)) => match next_level(watch, w, o)? {
                        Some(next) => {
                            watch = next;
                            p.stage = Stage::Level;
                        }
                        None => return Err("the episode went on past its last level".into()),
                    },
                    None => {
                        watch.engine.title();
                        p.stage = Stage::Over;
                    }
                }
                return Ok(watch);
            }
            let press = if screen.waiting {
                let since = *waiting_since.get_or_insert_with(|| {
                    watch.agent.x_mut().log.push(format!("present: the {} waits for a press", if screen.state == 1 { "tally" } else { "finale" }));
                    p.clock
                });
                let read = if screen.state == 1 { TALLY_READ } else { TEXT_READ };
                p.clock - since >= read && pressed.is_none_or(|t| p.clock - t >= PRESS_AGAIN)
            } else {
                *waiting_since = None;
                false
            };
            if press {
                *pressed = Some(p.clock);
                watch.agent.x_mut().log.push(format!("present: pressed on ({})", if screen.state == 1 { "tally" } else { "finale" }));
            }
            idle(&mut watch, press);
        }
    }
    Ok(watch)
}

/// The run's first level begun, from the title: the one loaded with the watcher as it is if it
/// hasn't been played, else afresh (a new agent, the lives again).
fn start_level(mut watch: Watch, p: &mut Present, game: Option<(&Wad, &WatchOptions)>) -> Result<Watch, String> {
    let lives = game.map_or(watch.lives_left + 1, |(_, o)| o.lives);
    if p.fresh && watch.map_name == p.first_map {
        // (the engine's level begun again, as the run begins it; her agent as it was made: it
        // hasn't played, and a restart would count a life she never lost)
        watch.engine.restart(false);
        watch.controls = None;
        watch.lives_left = lives - 1;
    } else if let Some((w, o)) = game {
        let loaded = load_level(w, &p.first_map, o)?;
        watch = enter_level(watch, w, o, &p.first_map.clone(), loaded, None)?;
    } else {
        watch.engine.restart(false);
        watch.agent.restart();
        watch.controls = None;
    }
    p.fresh = false;
    Ok(watch)
}
