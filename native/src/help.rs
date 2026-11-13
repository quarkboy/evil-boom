// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! The program's own help: `evil-boom help [TOPIC]`, `evil-boom COMMAND --help`, `--version`.
//! Kept to what each command takes, as main.rs and cli.rs parse it; docs/USAGE.md says the same
//! at more length.

/// `evil-boom --version`.
pub fn version() -> String {
    format!(
        "evil-boom {} (build {})",
        env!("CARGO_PKG_VERSION"),
        crate::config::COMMIT
    )
}

/// `evil-boom` and `evil-boom help`: every command, in a line each.
pub fn overview() -> String {
    format!(
        "Evil BOOM {}: a DOOM bot for an AI streamer to direct, running DSDA-Doom.

Usage: evil-boom COMMAND [ARGUMENTS] [OPTIONS]

Watching and streaming:
  watch WAD MAP              watch her play, in a window
  console                    the stream console: a web page that starts and steers the game
  record WAD MAP OUT.mp4     a run recorded to a video

Directing her (the Neuro SDK protocol):
  neuro-mock                 a stand-in Neuro server: actions sent from a file you write to
  neuro-gpt                  a language model playing Evil, as a Neuro server

Runs without a window, and testing:
  run-map WAD MAP            one level; its log in out/explore/
  sweep WAD [MAPS]...        many levels side by side, a line each, then a summary
  episode WAD MAP            an episode on one life, several seeds side by side
  score                      the bot's score: every level, HMP and UV, eight seeds
  score-report FILE...       a score's report, or two compared
  score-watch FILE           a score's progress in a small window
  arena FILE [NAMES]...      fights set up in a file (also arena-one, arena-watch)

Looking inside a level:
  dump-nav WAD MAP           the level's analysis
  sector WAD MAP SECTOR...   sectors as the map has them
  follow WAD MAP             plan the level and follow the plan
  drive WAD MAP --script F   drive her by hand from a script
  secret-survey WAD MAP...   how each secret is found
  tuning                     the numbers she judges a fight by, as JSON

WAD: doom, doom2, tnt or plutonia (your own IWADs: see docs/BUILDING.md), or a path.
MAP: E1M1 (DOOM) or MAP01 (DOOM II).

  evil-boom help COMMAND     that command's arguments and options (or COMMAND --help)
  evil-boom help options     the options most commands take: skill, lives, seed, ...
  evil-boom help music       the music: the SC-55 recordings, SoundFonts
  evil-boom --version

Documentation: https://github.com/quarkboy/evil-boom (docs/USAGE.md)
",
        env!("CARGO_PKG_VERSION")
    )
}

/// A command's help, or a topic's (options, music); none if there's no such thing.
pub fn topic(name: &str) -> Option<&'static str> {
    Some(match name {
        "watch" => WATCH,
        "console" => CONSOLE,
        "record" => RECORD,
        "neuro-mock" => NEURO_MOCK,
        "neuro-gpt" => NEURO_GPT,
        "run-map" => RUN_MAP,
        "sweep" => SWEEP,
        "episode" | "episode-one" => EPISODE,
        "score" => SCORE,
        "score-report" => SCORE_REPORT,
        "score-watch" => SCORE_WATCH,
        "arena" | "arena-one" | "arena-watch" => ARENA,
        "dump-nav" => DUMP_NAV,
        "sector" => SECTOR,
        "follow" => FOLLOW,
        "drive" => DRIVE,
        "secret-survey" => SECRET_SURVEY,
        "tuning" => TUNING,
        "options" => OPTIONS,
        "music" => MUSIC,
        _ => return None,
    })
}

const WATCH: &str = "\
Usage: evil-boom watch WAD MAP [OPTIONS]

Watch her play MAP, in a window: the game beside the map of what she knows and her log.

  evil-boom watch doom E1M1 --fight 3 --mortal        E1M1 on Hurt Me Plenty
  evil-boom watch doom2 MAP01 --fight 4 --episode      DOOM II from MAP01 on, Ultra-Violence
  evil-boom watch doom2 MAP01 --stream --title         the stream's window, from the title card

Keys: space pause, N (or right arrow) a tic at a time while paused, + and - the speed, A the
whole map, F follow her, M mute, L Evil's log, T text size, the mouse wheel zooms, Esc quits.

The game:
  --episode               on to the next level when she exits, what she has carried over
  --speed X               play speed (1: real time; the sound plays only at 1)
  --mute                  no sound
  --all                   the whole map shown, not only what she's seen

The window:
  --stream [right|left]   the stream's 16:9 window: the game on one side, a column for the
                          viewers on the other (right unless told); titled \"Evil BOOM\" for OBS
  --present               Doom's own screens: the title card, the menu, the melt, the tally,
                          the finales, death as the game shows it
  --title                 those, waiting at the title card until the console begins the game
  --window-size WxH       the window's size; the picture is scaled to fit it
  --render WxH            the game drawn at that size (640x480 unless told; 320x200 as in 1994;
                          1440x1080 draws the stream's game pixel for pixel)
  --fullscreen            the whole screen, at the desktop's resolution, borderless
  --no-vsync              frames shown when ready, not at the screen's refresh
  --headless              no window: played as fast as it goes, its log in out/watch/live.log

The music (see evil-boom help music):
  --music sc55|idkfa      the songs as the Steam re-release recorded them
  --soundfont NAME|opl    the MIDI synth: a SoundFont in soundfonts/, or the OPL synth

Directed play:
  --neuro URL             directed by Neuro (or neuro-mock) over its WebSocket, ws://HOST:PORT
  --gpt [CONFIG]          directed by a language model, run in this program
                          (config/neuro-gpt.json unless named)
  --directed FILE         orders read from FILE as lines are added to it; answers in FILE.out
  --choices SPEC          the games hers to start (\"maps=E1M1,E1M2|all;skills=3,4;play=single\")

Run options (evil-boom help options) work here too: --fight, --mortal, --lives, --seed, ...
";

const CONSOLE: &str = "\
Usage: evil-boom console [--bind HOST:PORT] [--new-token]

The stream console: a web page to start, stop and steer the game, and set how it's started
(the WAD and level, skill, who directs her, the window, the music). It runs the game as a
`watch` of its own, restarting it if it crashes. It prints the page's address, with its token
(every request needs it).

  --bind HOST:PORT    where it serves the page (0.0.0.0:7878: this machine and the network;
                      127.0.0.1:7878 for this machine only)
  --new-token         a new token for the page; the old one stops working

Its settings and token are kept in config/console.local.json. See docs/CONSOLE.md.
";

const RECORD: &str = "\
Usage: evil-boom record WAD MAP OUT.mp4 [OPTIONS]

A run recorded to an MP4 (H.264, through ffmpeg), played as fast as it goes. Directed runs
(--neuro, --gpt, --directed) record in real time, with the game's sound.

  --view game|full|hud    the game's view; the whole watch window; or the game with what she's
                          doing over it and a map in the corner (hud unless told)
  --size WxH              the video's size
  --speed N               N times as fast (whole numbers)
  --episode               on through the episode
  --crf N                 quality (20 unless told; lower is better and bigger)
  --preset P              ffmpeg's preset (veryfast unless told)
  --ffmpeg PATH           the ffmpeg to use (the one on the path unless told)

With --neuro URL, --gpt [CONFIG] or --directed FILE as for watch, and the run options
(evil-boom help options).
";

const NEURO_MOCK: &str = "\
Usage: evil-boom neuro-mock [--port N] [FILE]

A stand-in Neuro server, for trying directed play by hand: run it, then a watch with
--neuro ws://127.0.0.1:PORT. Every line added to FILE (out/neuro/actions.txt unless named) is
sent to the game as an action; everything the game sends is shown, and kept in FILE.log.

  --port N    the port to listen on (8000)

See docs/NEURO.md for the actions.
";

const NEURO_GPT: &str = "\
Usage: evil-boom neuro-gpt [--port N] [--config FILE]

A language model playing Evil, served as a Neuro server for a watch to connect to (watch --gpt
runs it inside the watch instead). It speaks to an OpenAI-compatible chat API.

  --port N          the port to listen on (8000)
  --config FILE     its settings (config/neuro-gpt.json); the API key goes in
                    config/neuro-gpt.local.json or OPENAI_API_KEY

Its log is out/neuro/gpt.log. See docs/NEURO.md.
";

const RUN_MAP: &str = "\
Usage: evil-boom run-map WAD MAP [RUN OPTIONS]

One level, no window, as fast as it goes. It prints a RESULT line (EXITED, DIED, STUCK,
STALLED, ...) and
writes everything about the run to out/explore/<IWAD>_<MAP>.txt, with a stamped copy. The run
itself is recorded in out/runs/ (for --from).

  evil-boom run-map doom2 MAP14 --fight 3 --mortal

The run options: evil-boom help options. See docs/DEBUGGING.md for reading the log.
";

const SWEEP: &str = "\
Usage: evil-boom sweep WAD [MAPS]... [WAD [MAPS]...]... [--jobs N] [RUN OPTIONS]

Many levels at once, a process each: a line for each as it finishes, then a summary. A WAD with
no maps after it means all of its levels. WAD here: doom, doom2, or a path to a .wad.

  evil-boom sweep doom doom2 --jobs 8
  evil-boom sweep doom E1M1 E1M2 doom2 MAP01 --fight 3 --mortal

  --jobs N    how many at a time (8)

The run options: evil-boom help options.
";

const EPISODE: &str = "\
Usage: evil-boom episode WAD MAP [--seeds K] [--jobs N] [RUN OPTIONS]

An episode from MAP on, on one life, what she has carried from level to level, as K runs with
different seeds side by side. (episode-one: one of them, as `episode` runs it.)

  evil-boom episode doom E1M1 --fight 3 --mortal --seeds 8

  --seeds K    how many runs (8)
  --jobs N     how many at a time (8)
  --lives N    N lives for the episode: dead, the level again with what she came into it with,
               and what she'd learned of it kept

The run options: evil-boom help options.
";

const SCORE: &str = "\
Usage: evil-boom score [WAD [MAPS]...]... [OPTIONS]

The bot's score: every level of DOOM and DOOM II (or those named) from a pistol start, one life,
on each skill, with random seeds. A row a run goes to out/score/, then the report: 1 for an exit,
else half the share of monsters killed. See docs/TESTING.md.

  --skills 3,4    the skills (Hurt Me Plenty and Ultra-Violence)
  --seeds K       seeds a skill (8)
  --jobs N        how many at a time (12)
  --out FILE      where the rows go
  --like FILE     the same runs as an earlier score's (its levels, skills and seeds)
  --tuning FILE   given to every run; --bar NAMES too
  --no-window     no progress window
";

const SCORE_REPORT: &str = "\
Usage: evil-boom score-report FILE...

A score's report from its rows (out/score/*.tsv). Two or more: how far apart they are, and run
for run when they're the same runs (--like).
";

const SCORE_WATCH: &str = "\
Usage: evil-boom score-watch FILE [--total N]

A score's progress in a small window, read from its rows as they come in (score opens one
itself unless --no-window). --total N: how many runs it will have.
";

const ARENA: &str = "\
Usage: evil-boom arena FILE [NAMES]... [--jobs N] [--seeds K]
       evil-boom arena-one FILE NAME [--seed N]
       evil-boom arena-watch FILE NAME [--seed N] [--speed X] [--mute]

Fights set up in FILE (arenas/combat.txt has some; see native/src/arena.rs for the format):
arena plays each (or those named) K times (3), a line each, then how each went; arena-one
plays one once, its log in out/arena/NAME.txt; arena-watch plays one in a window.

  --jobs N    how many at a time (8)
";

const DUMP_NAV: &str = "\
Usage: evil-boom dump-nav WAD MAP

The level's analysis as the bot makes it (the navigation mesh, triggers, jumps), printed: for
comparing builds.
";

const SECTOR: &str = "\
Usage: evil-boom sector WAD MAP SECTOR...

Sectors as the map has them: heights, specials and tags, their lines and what they do, the
things in them, and the jumps landing there. For answering \"what lowers this floor\".
";

const FOLLOW: &str = "\
Usage: evil-boom follow WAD MAP

Plan the level from its start and follow the plan in the game (seed 0, no monsters): the
follower's log.
";

const DRIVE: &str = "\
Usage: evil-boom drive WAD MAP --script FILE [RUN OPTIONS]

Drive her by hand from a script, a command a line: goto X Y, warp X Y [ANGLE], face X Y, use,
move F S T N, wait N, fire N [SLOT], bot N (the bot plays N tics), where, items, and more (see
native/src/drive.rs). With --from FILE@S, from a moment of a recorded run.
";

const SECRET_SURVEY: &str = "\
Usage: evil-boom secret-survey WAD MAP... [--jobs N]

For each level, its secrets: the lines pressed or shot to reveal each, and how the bot's search
rates them and every other wall once she's explored all she can. Written to
out/survey/<WAD>.tsv and .txt. --jobs N: how many at a time (12).
";

const TUNING: &str = "\
Usage: evil-boom tuning

The numbers she judges a fight by, as JSON: save it, change what you like, and give it to a run
with --tuning FILE.
";

const OPTIONS: &str = "\
Run options: they go with watch, run-map, sweep, episode, record, drive and score.

  --fight SKILL        monsters in, at that skill: 1 I'm Too Young To Die ... 3 Hurt Me Plenty,
                       4 Ultra-Violence, 5 Nightmare (without it, no monsters)
  --mortal             she can die (without it, nothing hurts her)
  --lives N|unlimited  how many tries at a level (unlimited unless told)
  --seed N|random      the game's random numbers (0 unless told; random for real play)
  --max-seconds S      give up after S game seconds, all lives together (3600)
  --pwad FILE          a PWAD over the IWAD, repeatable (any command): maps from the last that
                       has them

What she goes for:
  --expert             every item, then out
  --secrets            out only once every secret's found
  --complete           every kill, item and secret, then out
  --stall S            those runs end after S game seconds with nothing new (300)

What she knows:
  --known              the level known beforehand: she looks round it first with no monsters,
                       then plays it knowing the map and where every monster starts
  --peek               every switch and door known from the start

Testing:
  --carry W,C,S,L,R,B,H,A,T   the loadout she starts with: weapons (a bit each), clip, shells,
                       cells, rockets, backpack (0/1), health, armor, armor type
  --tuning FILE        the fight's numbers from FILE (evil-boom tuning prints them)
  --bar NAMES          moves and retreats she isn't to make, by name
  --decisions          keep the journal of her decisions beside the log
  --advisor FILE       ask a decision model as she plays
  --from FILE@S        replay a recorded run (out/runs/) to S game seconds, then the bot
  --snapshot S         stop after S game seconds and draw the window to out/watch.png
  --focus X,Y,ZOOM     that picture's map centred there
";

const MUSIC: &str = "\
The music.

Doom's music is MIDI. Out of the box it plays through the OPL synth, as a Sound Blaster played
it. Two things, from your own copies, make it sound like the original:

  The DOOM + DOOM II re-release on Steam: watch --music sc55 plays its recordings of the
  original soundtrack on a Roland SC-55 (--music idkfa: Andrew Hulshult's). Evil finds
  rerelease/extras.wad in Steam's folder, or a rerelease/ folder beside the WADs' base/, or
  where EVILBOOM_RERELEASE_DIR says; the first game makes a music WAD of it in out/music/.

  Patch93's SC-55 SoundFont (search the web for it), saved as soundfonts/SC-55.sf2: the MIDI
  synth from then on, for every song the recordings don't have (a PWAD's own). Any other .sf2
  in soundfonts/ can be chosen with --soundfont NAME; --soundfont opl for the OPL synth.

The console has both as settings (Music, MIDI synth). See docs/BUILDING.md.
";
