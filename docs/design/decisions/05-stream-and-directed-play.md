# The stream and directed play (2026-11-10 onward)

The stream console and Doom's own screens, then Evil's control through the Neuro SDK: directed play, what she's told, questions with defaults, her settings, goals, and the fixes found by watching models play. The plan behind it is ../neuro-sdk-plan.md.

## The stream console, Doom's own screens, the stream's window (the maintainer, 2026-11-10)

Built on the `console` branch, merged into main; the whole plan, the protocol and what each
stage was tested by: `docs/design/console-plan.md`.

- **A supervisor, the game its child.** `evil-boom console` serves a plain HTML/JS page (no
  build step; tiny_http) and starts the game as `watch ... --control ADDR`, a JSON line a
  message over a local TCP link (`console::Command` down, `FromGame` up, `PROTOCOL`). The
  supervisor outlives a crash or a hang (restarted at the level it was on, three in two minutes
  and it stops trying); the game plays on if the console goes. One game a process (DSDA's), so
  a new WAD is a new child; a level of the same WAD is a goto in the running one.
- **On the LAN, by a token** (`config/console.local.json`, not committed, with the settings):
  every API call needs it, from localhost too. Across the web: none of it in the repo; the
  streamer's own tunnel (Cloudflare Access) in front, the page's paths relative so it serves
  under a prefix, the login it vouches for named in the log.
- **Doom's own screens** (`watch --present`, on from the console by default): the title card
  held (no demos: the maintainer's), the menu walked to the game as a player walks it, the melt, a
  death watched, the tally and the finales pressed on. The engine draws them
  (`evil_api.c`'s presentation; `WI_EvilWaiting`, `D_EvilPageDrawer` in the patch); Rust says
  when, a tic at a time (`viewer/present.rs`). A melt holds the game and keeps its random
  numbers apart, but the title's tics are in the seed (`rngseed` takes `gametic`), so a
  presented run isn't the headless run on its seed: the maintainer's call, streams play on random
  seeds (the console's default). Headless play is untouched: run-maps line for line the same.
- **The stream's window** (`watch --stream [right|left]`, the console's default): 1920x1080, the
  game 4:3 at 1440x1080, a 480 column for viewers (the level, her map as she knows it, what
  she's doing in plain words, what Evil said, the tally, the run, what happened lately). The
  status bar's things (health, ammo, keys) aren't repeated: the game shows them.
- **Volume**: DSDA's sfx and music volumes, and a master the patch adds (`evil_master_volume`,
  a gain in `I_UpdateSound`), each a slider; kept in the settings.

## MAP07's exit step raised twice: left as the game has it (the maintainer, 2026-11-10)

The last two arachnotrons dead within one's death animation of the other (49-51 tics apart in
the runs traced), the game's check runs for each and the step round the exit (tag 667, raised
by its lower texture's 16) goes up twice, 8 to 40: 32 above the yard, no climbing it, no exit.
3 of 16 runs on the easiest skill. The maintainer's: "that's intended game design. We leave her
stuck forever. That would actually be hilarious during a stream." So: no rule to space the
last kills, no giving up when she sees it. Not to be "fixed". (Seen with it and not asked to
be left: she waits for the step at 24, the model's height, when she sees it at 40.)

## Barrels are gone round, and run out of; two bosses are set on each other (the maintainer, 2026-11-11)

MAP20 and MAP23 on the easiest skill: 16 runs of 16 dead in under a minute on each.

**Barrels (MAP23).** The mesh has no barrels in it (they can be blown up), so a route by it
ran into a row of them and stood there. Now the follower plans a lane between the barrels she
sees in her track (`motor/follower/lanes.rs`: a search over a 4-unit grid, her box against
each barrel's as the game tests it), to where the track's clear again, and plans again as more
come into sight; the sidestep (`round_barrel`) only where there's no lane. The maintainer's: "it's
required to plan a route around the barrel obstacles". And in the blasts of six or more
chained barrels (`barrels.field`) she runs for the nearest place she knows with no barrel
within 384, at once, something there to set them off or not, and nothing takes her legs or
eyes till she's there (`RetreatKind::OutOfBarrels`; the maintainer's: "find shortest path to a room
without barrels and RUN", "cannot be interrupted by ANYTHING until she gets to safety", "it
should start immediately"). With nothing to set them off, once only from about the same
place (a way that lies through barrels would never be taken). No such place known and
something that shoots awake: on along her route flat out, facing the way she runs, the
trigger let go (`Move::ThroughBarrels`). A chain going off she isn't running from: the
shortest way out of its blasts, or on along the route, never to and fro between two barrels.
Tried and dropped: a timed model of the chain (which barrel goes when): too noisy to steer by.

Found on the way and left: the route's legs go at a sixth of her pace while the fight has her
turned to aim behind her (`Motion::go` slows a run sideways or backwards). Fixed for the
barrel run only (`Move::route_faces`); everywhere else it's as it was, not measured.

**Two bosses (MAP20).** A spider mastermind and a cyberdemon both awake (the only two the
code calls bosses): she goes to the nearest place in a line with the two, one between her and
the other, so what the far one fires at her hits the near one (`Combat::set_on`,
`Move::SettingOn`); neither turned to her, they're left to it and the route goes on
(`Move::LeavingToIt`). No shot at either while both live; one dead and the other turned to
her, the fight as it ever was (the maintainer's, in place of two rules of mine that held the
trigger on anything she couldn't kill in every move and walked her past what outmatched her
after 30 s: those two lost 0.0175 on the score and are gone). A monster's health is reckoned
by what she's seen it lose to anything, not only by what she's put in.

Easiest skill, 16 seeds: MAP23 0.03 -> 0.45 points (3 exits), MAP20 0.07 -> 0.21 (1 exit).
Score: 0.460 -> 0.459, run for run -0.0009 (standard error 0.0049), exits 408 -> 403.

## Episodes on lives; Doom's second episode's stucks (the maintainer, 2026-11-11)

`episode --lives N`: N lives for the whole episode. Dead, the level's begun again with what
she came into it with (weapons, ammo, armour, health, the weapon in her hands: the loadout
has that now, `evil_ready` in the engine API brings it up) and what she'd learned of the level
kept (the maintainer's: "I would like knowledge kept"; what she saw before she died, not a level
looked round beforehand). Episode 1 goes through on every skill; episode 2 stuck in 12 of 32:

- **E2M4, a secret door at the back of a lava alcove**, the only way off a ledge. Its wall,
  her best suspect there, was refused a few points short of the health margin, and walls with
  less to go on across the lava tried first (a dip reckoned a fraction of a hit; it's none or
  a whole one) till it was out of reach for good. Now: down to trying walls, a wall's weighed
  with the margin of nothing else to do; a wall tried over any floor that hurts is left to the
  risky pass, the best suspect first; and the last pass, the margin lifted, has the walls in it.
- **E2M4, the crusher stopped low** by a stop line walked over further on, its start line its
  own edge. Intended, the maintainer's: left. But stopped with her height under it she walks
  through (the maintainer's: "you are supposed to eventually walk into the crusher"); the room to
  spare she wanted was for one coming down.
- **E2M1, a stair built round the corner**: five steps heard, one seen, two neither, taken for
  still on the floor and the stair for no way up. Now a step of a stair learned for a line
  takes the rest of that stair she's seen with it.

Score, all of it: 0.459 -> 0.464, run for run +0.0049 (standard error 0.0033).

The recorder: the game library is never unloaded (its sound thread went on calling into it
after a recorded episode ended: a segfault, the sound never put with the video).

## Directed play: the bot's pick, lists in its order, `doom_look` (the maintainer, 2026-11-11)

The SDK plan's 5.5 and 4.1 (`docs/design/neuro-sdk-plan.md`), in `control/directed/look.rs`.

- **One opinion, the explorer's.** The pick is what she'd do next on her own: the first of
  the explorer's ranked objectives she has a route to. Nothing new is weighed for it, so it
  can't disagree with what the idle fallback then does, and it's built only on what she
  knows. Considered and left: a ranking of its own for the commander (two opinions to keep in
  step); the exit as a pick whenever it's reachable (the explorer's own first choice, but
  then the pick says "leave" from the moment the exit's seen: leaving stays an order's, as
  on her own, and the exit is the pick only with nothing else left she can get to).
- **Under the id it's ordered by**: something to explore by its area (`doom_explore`), a key
  by the key lying there, a door, switch, item by its own. A spare pickup isn't a pick (she
  takes those by herself).
- **Lists in that order too**, not by distance alone; what the explorer has no objective for
  comes after, nearest first.
- **Where it's said**: marked in `doom_list`, last in `doom_look` and a forced decision; a
  failed order says what's untried nearest what it couldn't reach, or the pick; with no order
  for 30 s it's suggested quietly, at 60 s to be heard. `hints` (`off`, `on_stall`, `always`)
  sets how much; `doom_policy` carries it and `idle`.
- **`doom_look`**: in sight is the engine-side line of sight from her eyes over floor she
  knows, whichever way she faces: a look is her looking round, not her 110 degrees this tic.
  The fight's line is the assessment's (`Combat::now`): safe, dangerous (engaged), deadly
  (outmatched), with surrounded, short of ammo, an arch-vile; and the target and gun the
  fight has chosen, in place of the explorer's pick.

Checked: E1M2 with every suggestion taken as an order and nothing else: 68 of 70 carried
out, out by the exit at 738 s, 41/41 kills. Parity: 24 runs the same (none of it is in a
headless run).

## What Evil's told is told to her as the player (the maintainer, 2026-11-11)

Everything the API tells her is in the second person, as hers to have done: "You found a
secret", "You died fighting 2 Demon", "You're waiting. What next?"; the actions' descriptions
and the briefing too ("You're playing Doom. Moving, fighting ... you do by reflex; you choose
where you go."). The bot isn't a second character in her context: going on without an order is
"by instinct", the fight's own choices "by reflex", its pick "Suggested:". Considered:
"autopilot" for those (a machine beside her again, the thing the change removes). The words
the viewers and the streamer read stay in the third person. For the same reason what she
does is an action, never an order ("No action for 8 s", "Dropped for the new action"); the
code's `Order` and the script's `order go` keep their name, she never reads them.

Who's playing comes with the startup's acknowledgement (`characterId`): kept as
`neuro::session::Character` (Neuro, Evil, another, not said). Nothing differs by it yet.

## Kills and blasts told as moments (the maintainer, 2026-11-11)

`control/directed/kills.rs`. She was told of kills only by a fight's summary and the tally,
and nothing of barrels. Now: a big monster's death (a boss, or 500 health and up; first
tried by threat, as sightings are, and five chaingunners each got a line; then from 400,
and six cacodemons did); several dead
within 3 tics as one line with how many gibbed; barrels going off gathered till the chain's
over (none lit, none gone for 30 tics), with what died within a blast of them and what it
cost her; her own blast; the level's last monster. One barrel that takes nothing with it is
her clearing barrels by instinct and isn't told (the first try told each: thirty lines a
level). Gibbed is the game's own rule (`P_KillMobj`: health below minus the spawn health,
for the kinds with an extreme death), read off the corpses by the game's runner and counted
only for a death in her sight. The hits on her (the engine's, with their cause) reach the
director the same way, as the tally does. Headless runs have none of it: parity the same.

## Things by the names a player knows (the maintainer, 2026-11-11)

What she read had the class names the code keys on ("Fatso", "DoomImp", "ShellBox",
"BlurSphere": ZDoom's, by way of `dsda::type_name`). `control/names.rs` is the one table from
those to what they're called, one and several ("mancubus", "mancubi"; "box of shells"), used
wherever a thing is named to her; the code's tables keep the class names. A test holds every
monster and pickup the game has to a name. Her status by instinct read the explorer's label
("try door (line 136)", "look into sector 21"): the plain phrase now, as the viewers get.

## Why a route failed, told plainly (the maintainer, 2026-11-11)

An action that couldn't be done told her the follower's log line: "failed: FAILED at (-151,
428): unexpected drop into sector 108: the only way on drops where the plan didn't (step 1:
pick up GreenArmor)". Coordinates, sectors and steps are the trace's, not hers.

The follower's failures are a type now (`motor::follower::Trouble`, nineteen kinds) and the
explorer's other ends too (`knowledge::explorer::Why`: seen on the way, gone, took too long,
no route, the route's trouble, the fight's give-up...). Each prints exactly the old log text
(`Display`: the logs, the sweeps' and every tool reading them unchanged; an E1M2 log compared
byte for byte with the build before differs only in its timing line) and has a plain phrase
with no numbers in it, which `Ended::plain` carries to the director: "Couldn't get to item-t212
(green armor): the only way on is a drop you didn't mean to take." Considered: rewording the
log line at the director by matching its text (a second copy of every message, broken by the
first rewording of one).

## Stage 1's moments finished: levels, jumps, teleports, crushers, the budget (the maintainer, 2026-11-11)

`control/directed/moments.rs` and `kills.rs`; the plan's 5.3 has the lines. What decided:

- **A big jump** is told before she runs at it, from the jump the follower lines up for
  (`JumpNews`, carried by the explorer, the last eight only: in a headless run nobody takes
  them): 128 or more across, 64 or more to fall, or a hurting floor below. A hop isn't. How it
  came down: after one told, and a miss always. A miss is the follower's (another sector than
  planned) only when it's another area: off the spot in the same room is made (the first try
  told "Missed the jump: came down in area-111", area-111 being where it was going).
- **A level** is told begun once its title's to be had (the game's automap title), with the
  skill and what she brings; done on its exit, before the next level's begun (the runner
  calls `level_done` on leaving), with the time, the tally and what she leaves with.
- **A teleport into a crowd**: moved more than 64 in a tic, then 3 or more fighting her within
  a second. **Out of ammo** in a fight: the assessment's `guns_armed` gone with something
  engaged. **A crusher**: the hits the engine gives with its cause, gathered as her own
  blast's are. **Raised**: one she knew dead in her sight again.
- **The budget**: 6 moments a minute, the plan's number; over it, what isn't urgent is told
  silently. Urgent (`Report::urgent`): her death, health marks (now 50, 25, 10), losing health
  fast, a crusher, a teleport into a crowd, out of ammo, a level's end, an action's answer.
- Left for stage 4: the one `Situation` snapshot and typed events (the questions are what
  need them; the status, the look and the lists already read the same helpers).

Checked on MAP06 (jumps, a teleport), E1M1 to E1M2 (the level lines). Parity: 24 runs the
same.

## The SDK plan's open questions answered (the maintainer, 2026-11-11)

Section 9 of `docs/design/neuro-sdk-plan.md` has them. What they change now: with no action from Evil
she **waits** (not on by herself by default): stays put, looks round (a quarter turn every 4
s, the other way the next time), takes pickups only within 256 (was 1024: waiting she
doesn't really go anywhere), fights what comes. Directed play only: parity the same.
For later stages: question deadlines 3 s (class T) and 8 s (class D), from her answering in
250 ms on average and rarely 5 s; restart hers once she's made no progress for a while (90 s
proposed); next level hers to give from the tally screen; no level choice; skill behind the
console's switch; cheats held off; free-text style a plan of its own (a Jev-like model).

## The screens between levels told to her (the maintainer, 2026-11-11)

In stream mode (`--present`, Doom's own screens) she heard nothing between levels, and the
episode's last level got no end line (it was told only on the way into the next). Now: the
level's end is told as the tally comes up (once: `level_done` is idempotent, and the next
level's start doesn't tell it again); a finale's text is told as it comes up, as it reads
("On screen: \"Once you beat the big badasses ...\""), Doom's episode endings and Doom II's
stories between levels alike, from a new engine call (`evil_finale_text`); game over is
told. Her link is passed what's told at once (`Session::between`): no level's tics run on
those screens to pass it on. Her actions still wait for the next level (next level hers is
stage 8). Headless runs have no screens: there, as before, a level's end is told going into
the next.

The text is typed out on screen (a character every 3 tics), so it's told as it is (the maintainer's):
the engine gives as much as is shown so far, and each sentence goes to her as it's finished
typing, the rest once the text stops growing (`present::next_chunk`). On E1M8's ending: nine
lines, 1 to 8 s apart.

Checked: E1M8 in stream mode to Episode 1's end (its end line and its text). Doom II's
stories go through the same finale screen; not seen in a test (MAP06 and MAP11 weren't left
in the time given).

## Evil starts the game: level, skill, one level or on through (the maintainer, 2026-11-11)

The maintainer changed answer 5: she chooses the level to start at, the skill, and whether to play
just that level or on through its episode (Doom II: through the game), from what the console
offers (`control/game.rs` `Choices`; `doom_game start`). By default the console offers its own
map and skill, one level (`evil_maps`, `evil_skills`, `evil_play` empty), so often her one
choice is to start: what's on offer is the streamer's to widen.

When: off a level with nothing under way (the title card waiting, a game over, a level played
by itself once its tally's gone, an episode's end once its story's told), never mid-level
("a new game's offered when it's over"). The offer is told her when it's first there and again
whenever the console changes it. Her link is served on those screens (`Session::off_level`:
any other action answered that there's no level to do it in). Her choice sets the skill for the
next level begun (`Engine::set_skill`; the menu walk shows it), and the window's own copy of the
options (skill, episode) the levels are loaded with; a chosen game is always a level loaded
afresh. An episode's last level played by itself now ends in its finale too (it's the game's).

Checked in a window: at the title, E1M9 refused (not offered), "E1M1 4" refused (single or
continuous not said, both offered), "E1M1 4 single" started at Ultra-Violence (28 monsters
where Hurt Me Plenty has 6); out by its exit, offered again at the title; "E1M2 3 continuous"
started. Parity: 24 runs the same.

## Evil BOOM throughout (the maintainer, 2026-11-11)

The binary became `evil-boom` on 2026-11-08 and the rest kept the old name; now all of it is
Evil BOOM: the package and library `evilboom`; the console page, the windows' titles ("Evil
BOOM" for the stream's, the one OBS finds: its window source to be chosen again), the README,
the docs' titles; the environment variables `EVILBOOM_DOOM_DIR`, `EVILBOOM_ROOT`,
`EVILBOOM_NATIVE_STATS` (`config::env`, the `EVILDOOM_` names still read); the Linux WAD
folder `~/.local/share/evilboom`, the old `evildoom` one used while there's no new one (the
maintainer's files aren't moved); the engine patch's comments. Left: the names of the Python code
this once was, in the record and in two comments that cite it.

## Stage 1 closed: its gate run, and what reading it cold found (2026-11-11)

The gate: the GPT stand-in as Evil on MAP07 (Doom II, skill 2, pistol start, seed 3, 8
minutes at x1, as the run before the SDK work), everything she was told read cold and counted
by channel (`words.py`: messages, words, words a minute).

Found, and fixed:
- **An explore order paused for good.** It paused while anything was fighting her; a mancubus
  out of reach kept shooting from 900, the fight's move was "out of reach: going on", and the
  order never went on: six of the eight minutes with nothing told her worth acting on, eight
  actions in all. Now it pauses only while the fight has her legs (its `Move` doesn't let the
  route go on), as she plays on her own. With run 1's orders replayed (a loadout to live
  through it): area-0 explored at 197 s.
- **Her status was 72% of the words** (24 copies in 8 minutes, 113 words a minute): sent on
  any change at all, each shell, the mancubus's bearing. Now when what she'd act on changes
  (`status_gist`: health and armor by tens, weapons, keys, what she's doing, fighting or not,
  the tally): 6 words a minute.
- **The budget kept the wrong things quiet**: answers to her actions counted against it, then
  her first sight of two mancubi went silent. Urgent lines no longer count, and a heavy
  monster's first sighting is urgent.
- **"10 of backpack"** in the look (several of a kind in one place now "10 backpacks"), and a
  "she" left in a refused retreat.
- **Locks**: the game's own message for a lock tried without its key ("You need a blue card to
  open this door", the switch and skull ones, Boom's any and all) told her, once a try; one
  check (`knowledge::model::needs_key`, now also Boom's "Any key will open this door") for the
  director and the knowledge both.

Numbers: before, 1262 words in 8 minutes (158 a minute, status 906 of them), 1 moment a
minute, 8 actions; after, 451 (56 a minute, status 52), 1.2 moments a minute, 11 actions before
she died to two mancubi at once (the fight's: MAP07 kills her playing alone too). The run
before the SDK work: 42 actions, its choices the same kind (items, the switch, areas); the
stand-in's choices aren't worse, the bot under them was what stalled. Parity: 24 runs the
same.

## Next level and restart hers (the maintainer, 2026-11-11)

The plan's answer 5, built (`doom_game next`, `doom_game restart`; the console's two switches,
on by default; `--choices` `next=on`, `restart=on`, and `start=off` for those without a game to
start).

- **Next**: in stream mode, playing on through, the tally screen waits for her word (it was
  pressed on after 2.5 s), and goes on by itself after 30 s so the stream's never stuck there
  (the maintainer didn't say; my default). She's told on the tally screen.
- **Restart**: hers after 90 s with nothing new, progress counted as the score counts it
  (`sweep::known_progress`, now shared: what's collected or killed, sectors seen, effects
  learned, walls tried, lines mapped and set off, places walked), told her once when it's hers;
  before, "not yet" with the seconds to go. The level from its start with what she came in
  with, what she knows of it kept (`Agent::start_over`, as for a 100% run's restart), no life
  spent (my default: lives are a test setting, and a restart she asks for isn't a death).
- `doom_game` takes `do` start, next or restart (`control::game::GameAct`), in a level and out
  of one; what isn't for now says so.

Checked: E1M1 headless, restart refused at once ("90 s to go"), told hers at 94.9 s, granted,
back at the start; E1M1 in stream mode on through, at the tally "On the tally screen: doom_game
next ...", next given, E1M2 begun. Parity: 24 runs the same.

## Stage 4: questions from the bot (2026-11-11)

`control/directed/ask.rs`. One shape for the bot's wanting her say (the plan's 4.6, class D,
the kind that can wait): `[ask q3] <the question> a) ... b) ... (default, in 8 s). Answer with
doom_answer.` At most one open, 3 a minute (past that the caller does as it did unasked); the
default taken at the deadline (8 s, the maintainer's latency; `doom_policy answers`, 0 waits) and
said; Neuro forced a decision (`doom_answer` and the actions that would also settle it) 3 s in.
Any action of hers closes it as overtaken, and so do a death, a restart, a new level. An answer
to a closed question is told it's too late and what came of it, never applied.

Asked now, where the bot asked her to "resend" before or didn't ask at all:
- leaving with things left (`doom_exit`, a go to an exit, without `sure`): leave anyway, or
  stay (the default: leaving can't be undone);
- a go whose only way crosses a floor that hurts or drops where she can't get back: go anyway,
  or don't (default);
- on instinct, nothing left but walls to search with the exit known: leave, or stay (default);
- nothing new for 3 minutes: on by instinct (the default when she isn't), restart (when it's
  hers), keep on as she is.

Found on the way: **leaving was never asked while she was on instinct.** The exit check looked
at the explorer's options, from which the exits are held back on instinct, saw no exit, and
let `order exit` take her out unasked. It looks past the hold now, as the lists do.

The gate (scripted answers 2, 8 and 30 s late): E1M1 at x1, the exit question: answered at 2 s,
applied ("q1: stay"); at 9 s and 30 s, the default taken at 8 s and the answer refused ("Too
late: q2 is closed (no answer: stay)"); after another action, refused as overtaken. Through the
mock Neuro: the question as `doom_exit`'s answer, forced 3 s in ("default, in 5 s"), answered,
a second answer refused. The route question on E1M3 (a medikit past nukage), answered at once:
gone for. The stuck question on E1M1 after 3 minutes waiting: its default, on by instinct.
Not seen in a test: "nothing but walls" on instinct (a whole level's exploring first).

Left: the instinct's other last resorts (strafing runs, stuck places, every wall, jumps
missed, harm allowed) aren't asked: on instinct they're the bot's, and she can stop it any
time. `Situation` and typed events weren't needed for these (a question closes on any action
of hers instead of being checked against a snapshot); they're for class T (stage 6), where a
question about a fight has to be judged against the fight as it stands.

## Stage 4 finished: her instinct's last resorts asked first (2026-11-11)

On instinct the bot went down `next`'s last resorts by itself. Now whoever directs her holds
them back (`Explorer::resorts`: None, all of them, as playing alone and in every score run; on
instinct, only those she's let it take this level) and the first held back with something to
offer is noted (`resort_wanted`) and asked about: "Nothing else to do here that you know of.
Next would be <it>. a) <take it> b) leave by <exit> (when she knows one) c) stay (default)".
Taking it lets it for the rest of the level; staying ends instinct till her next action. The
resorts: strafing run-jumps, the places she got stuck, every wall, the held-back exit, the jumps
she missed, a floor that hurts (the plan's list; the suspect walls and what was given up lately
are routine, and stay hers).

Found testing, both fixed: allowing one asked it again at once (the explorer's note of it stale
till it chose again: cleared on allowing); and "the way out" was held back with no exit known,
so "leave" failed and she was asked again every 20 s for 15 minutes (held back only with an
exit she knows, when directed; alone, the bot's as it was). The "nothing but walls, leave?"
question now offers each exit she knows by name (E1M3 knew a secret exit and the normal one,
and "leave" had taken the first ranked: the secret one).

Checked: MAP09 (seed 19) on instinct: strafing runs asked at 108 s; answered each time, then
every wall, then the hurting floor, each once; left unanswered, its default ended instinct.
The walls question seen on E1M3 (1385 s), E2M4 (1381 s), MAP06 (1225 s). Parity: 24 runs
the same.

## Stage 6: questions that can't wait (2026-11-11)

The plan's class T (`control/directed/ask.rs`): the bot does its default at once and says so,
`[ask q4] Outmatched: 1 demon, 3 imps, 2 hell knights (health 200). a) as it is (pushing on,
shooting on the way) (doing it)  b) go bold. Open 3 s: doom_answer to switch.`; an answer in 3 s
switches it; after, too late. Never forced (a force answered after the moment is worse than
none). Asked:
- **A fight turned bad**, once a fight, not when she's had her say in it: outmatched, an
  arch-vile or a boss after her, or health under 25. Keep as it is (what the fight's doing, in
  words: falling back, keeping clear, pushing on...), fall back to cover (offered only with
  cover she knows a way to: offered without, it was refused), or go bold. Closed when the fight
  ends as its summary has it (not when one goes out of sight a tic: the first try closed
  within a tic).
- **A run-jump whose miss costs** (a floor that hurts below, or a long drop): jump, or don't.
  "Don't" leaves that leap out for good: every run-jump from that sector to that one (the first
  try declined one run-up and the next was lined up at once, three questions in four seconds).
  Closed once she's jumped.

What settles a question: her say in the fight settles a fight question; an order of hers in
place of what she's doing settles the others (where she goes, leaving, a jump). Not the bot's
own going on (resuming an explore after an errand closed one: fixed), nor an order queued
after (`then`), nor a go order for a fight question (the stand-in's go closed its fight
question before its answer came: fixed).

`Situation` (`situation.rs`): how things stand, worked out once a tic from the fight's
assessment and move and her health; her status's gist, the look's danger line (with what the
fight's doing) and the fight's questions read it.

The gate, the GPT stand-in on MAP06 (skill 3, a loadout, 6 minutes at x1): 7 questions (1.2 a
minute, under the 3); the class D one answered in time and applied; none of the 6 class T in
3 s (the stand-in, reasoning, takes several seconds: Evil's 250 ms would; the default stood
each time, as it's meant to). Checked by script: answered at once switches (go bold, fall back,
don't jump), after 3 s refused; a go order then the answer still switches. Parity: 24 runs the
same.

## What she's told, typed (the maintainer, 2026-11-11)

Every piece of news carries what it is (`control/directed/events.rs`, `Event`: her actions
begun and done, questions, her life, what she finds, monsters, her moves, the bot, the game),
and how much each kind matters is one table (`Event::weight`: quiet, a moment, urgent), not a
flag chosen at each of the 70 places that tell her something. `Report` carries its event;
`notable` and `urgent` come from it. The words stay where they're made (they need what only
that place knows). Each kind's weight is what its telling had before: a directed MAP06 game's
whole report file the same, line for line (111 lines), on the build before and after. What it
opens: a budget that drops the least of the moments first, a stream panel or console that
wants only some kinds.

## Her death told as it came (the maintainer, 2026-11-11)

She was told "You died." (and, with lives left, what she'd been fighting), never what killed
her: a rocket of her own in her face read the same as anything else. Now
(`control/directed/death.rs`) the last blow and what hurt her in the half minute before it,
each by what it was, from the engine's own record of every hit (its inflictor, its source, her
own or not, a barrel and who set it off, the floor, a crusher): "You died: a mancubus's fireball
(-36). Before it, in 30 s: a mancubus's fireball x3 (-64)." "Your own rocket", "a barrel an imp
set off", "a shotgun guy" (a hitscanner or a claw: the monster itself). Missiles have plain
names now (fireball, lightning ball, homing rocket, plasma, fire).

And a death reached her from every runner, through one path (`Watch::tell_death`): before,
only a headless game told one (stream mode and the window told none but game over, and the
last life's in a headless game wasn't told either). The killing blow's hit, which the game
passes on a tic after she's seen dead, goes with it; what's told is passed on at once. Whether
the level begins again is said, not guessed from the lives left (with 2 lives the first death
left "0" and was taken for the end).

Checked: MAP07 from a pistol start at UV with 2 and 3 lives: each death told, the lives
counted ("1 life left after this one", "Your last life"), the last told. Not seen in a game:
a death by her own rocket (the wording's in a test). Parity: 24 runs the same.

## Powerups told as she takes them, and as they run out (the maintainer, 2026-11-11)

She was told of a powerup when she saw it ("Spotted: megasphere"), never when she took it
(keys and guns had their line; the game's own "Invulnerability!", "Supercharge!" went
nowhere but the fight's own clock). Now (`control/directed/powers.rs`), by the game's word for
each: what she got and what it does ("You got invulnerability: nothing can hurt you for 30 s.",
"You got a soulsphere: health 200.", "You got a berserk pack: your fist hits ten times as hard
till the level's end."); and for the ones with a time (Doom's: invulnerability 30 s,
invisibility and the radiation suit 60 s, the visor 120 s; another of the same starts it again)
"Your radiation suit runs out in 5 s." (urgent for invulnerability and the suit, what's keeping
her alive) and "Your invisibility's worn off."

Checked on MAP08 by instinct: the soulsphere, invisibility, two suits (the second's time from
its own pickup), each running out and worn off on time. Parity: 24 runs the same.

And the other pickups (the maintainer's: everything but health and armor bonuses, stimpacks, medikits,
ammo, and a gun she has): green armor and megaarmor ("You got megaarmor: armor 200."), the
backpack, and ammo only of a kind she had none of ("You picked up 4 shotgun shells: 4 shells,
where you had none."). On E1M1 from a pistol start: the green armor, the first shells, the
megaarmor, each told; the clips and the rest, not. Berserk: the bot knows it heals (to 100, as
Doom's `P_GiveBody` does: `health_item`, read by the fight's and the explorer's pickups).

## No lives on stream (the maintainer, 2026-11-11)

Lives were for benchmarking; on stream the game is the original's: again and again till the
streamer turns it off. So: nothing she's told speaks of lives (a death: "... The level again
from its start, with what you came in with."; a test's game over: "Game over."); the console has
no Lives setting, no Set-lives control and no lives on its page, and starts its game with
`--lives unlimited` (new: `--lives` given at all, `unlimited` too, keeps a watched episode
from its one-life default). `--lives N` stays for tests and the score.

Checked: MAP07 at UV with `--lives unlimited`: four deaths, each told without a count, the game
going on. Parity: 24 runs the same.

## Stage 7, step 1: her guns (the maintainer, 2026-11-11)

- **The gun up, by the game's number.** She saw a slot (the plain shotgun and the super were
  both "3", the fist and the chainsaw both "1"); now `Observation::ready` and `owned` give the
  game's own numbers. And the bot's model of slot 3 follows what's in her hands: it took the
  super shotgun to be up whenever she had one, but the game brings the plain one up itself
  (shells down to one; Doom 1 with a carried super shotgun). A fix to how she plays: scored on
  the full set, -0.0018 (s.e. 0.0017), 1038 of 1088 runs the same to the tic, kept; the new
  baseline is that set (0.457, 398 exits).
- **A gun she orders is that very gun.** The engine API turned a slot-3 press into the super
  shotgun once she had it, and a slot-1 press into the chainsaw (the slot's best gun, as the
  game's key binding does); a press can now carry the gun's own number (`Controls::exact`,
  `evil_tic`'s 0x100), which the game takes as it is. So "shotgun" brings the plain one (better
  at range: the super's pellets spread up and down), "ssg" the super, "fist" the fist (only
  with a berserk pack this level: "Not the fist without a berserk pack this level"), "chainsaw"
  the saw. Brought up between fights too, not only in one. Her status lists every gun she has,
  the two shotguns apart. Every press the bot makes by itself is as before.
- **An ordered launcher kept, not flipped.** A rocket that'd go off by her: the launcher kept
  in hand (she keeps to its distance, holding fire, as with one of her own choosing) unless
  something's on her or she's been hit in the last second; then another gun, kept at least 2 s
  and till a rocket would fire again. The first version went back after 2 s whatever: on MAP02
  with shotgun guys close it switched every few seconds; now three switches in 45 s, each
  with one on her and hitting.

Checked by hand in directed games (MAP06: shotgun, ssg, rocket, shotgun, chaingun, ssg,
shotgun, each up as ordered; fist and chainsaw refused as they should be; MAP02 with the
launcher ordered). Parity re-saved with the new build (four runs moved by the slot-3 fix,
those of the scored set).

## Stage 7, step 2: searching, a command of its own (the maintainer, 2026-11-11)

Exploring searches the likeliest few walls as it goes (thoroughness 1; it was 0 unless said),
and `doom_explore` has no `secrets` any more; `doom_search area|direction [thorough 2|3]` (2 by
default) is the thorough search, most suspicious walls or every one. In a script, `order search
REF|DIR [2|3]`. What she's told says the thoroughness only when it's a search ("Exploring N
+secrets 3."), 1 being every exploring's. Checked on E1M1 by orders file. Parity: 24 the same
(directed play only).

## Stage 7, step 3: two defaults changed, and her settings (the maintainer, 2026-11-11)

- **Cells kept for bosses, rockets spent freely** (the maintainer's: cells matter more). With a boss
  seen and about, only the plasma rifle's and the BFG's ammo is kept dear (`guns.save_from` 6;
  it was 5, the launcher too). Scored: +0.0017 (s.e. 0.0017), 1065 of 1088 runs the same; kept.
- **The big spheres when they'd add enough** (the maintainer's rule): a megasphere when it adds 100
  to what she can take (`Combat::pickup_gain`, armor by what it'll take for her; `errands.
  mega_gain`), a soulsphere when it adds 50 health (`errands.soul_gain`); short of it, left
  to come back for. Scored: +0.0000, 15 runs moved in time and none in points; kept. The
  baseline is that set (0.459, 402 exits).
- **Her settings** (`control/directed/policy.rs`, `doom_policy`, a script's `policy SETTING
  VALUE`): `stance` steady | bold | careful | avoid | yolo, `save` cells | none, `risk` careful
  | normal | bold, `powerups` need | now; numbers over the run's tuning (`Player::set_policy`,
  applied on a change and on each level's new player; a `doom_fight` stance for a while goes on
  top and ends with a change). Yolo (the maintainer's: all in, for fun) is asked first: "Yolo: you
  fight everything head on, never fall back, never keep clear of anything. Sure?", no by
  default. `powerups now` takes invulnerability as it comes (`errands.invulnerability_now`).
  Which settings exist isn't up to the score (the maintainer's): each value's cost is measured and
  written into her description, not used to keep or drop it.

Her settings measured on the quarter set (the bot alone, each value's tuning file; the table in
`policy-survey.md`): careful 0.472 and powerups now 0.465 against the defaults' 0.464; risk
careful and save none no change; risk bold 0.458, yolo 0.458, bold 0.444, avoid 0.421. Each
value's cost is in her description of it; `avoid` is also a `doom_fight` stance for a while
(the maintainer's: avoid left on all the time won't make progress; it's for getting past a fight).
The level's start names any setting not at its default.

Careful as the default, on the full set: -0.0059 (s.e. 0.0095), 399 exits against 402, 26
level-skills beaten every run against 20, 55 never against 58. Within noise on the score,
more consistent; the default left as it is pending the maintainer's call.

**Careful is the default** (the maintainer's, 2026-11-11, on the full set's result above): its numbers
are the tuning's own now (`engage.stand_threat` 9, `engage.low_health` 50, `retreat.health` 55,
`guns.outmatched` 20); the policy's default stance is `careful`, and `steady` names the old
middle (6, 35, 40, 30). Careful's numbers are still given out in full for it as a stance, so
that over another standing stance, for a while, it's careful. The new defaults play the old
build-with-careful's runs exactly (three checked to the hash); the baseline is that full set
(0.453, 399 exits, 26 level-skills beaten every run). Parity re-saved.

## Three things from the Skyrim integration (the maintainer, 2026-11-11)

Read for comparison: vituha230/Neuro-Skyrim-Integration (an SKSE plugin, C++, Neuro the player
and the code her legs: menus as forced choices, the game paused for a fight's choice, actions
registered as they apply, much written per quest). Its per-situation registration was the SDK's
older advice; the best practices (20 August 2026) say keep the set stable, so ours stays as it
is. Three of its habits taken, the maintainer's choice:

- **Her data mended where its meaning is plain** (`neuro/repair.rs`, read against each action's
  own registered schema): a field under a common other name (`id` for `ref`, `area` for
  `target`, `answer` for `choice`; never over a field she gave under its own name), a number or
  a yes as a string, a float for an integer, an enum's value in other case or with spaces, the
  whole object sent as a JSON string. A field taken under another name is noted in the result
  ("(id read as ref)"), so she gives it right next time. What isn't plain is left for the
  action's check to refuse, as before (the SDK: validate, never assume the schema).
- **The rules told again** (silently) every 10 minutes, or after 30,000 characters told her
  (context and results), whichever first: her context rolls. (Skyrim's: 10 minutes or 50,000.)
  A self-played E1M3 under the mock: told again once at 10 minutes, 11,400 characters in 11.5.
- **An open question asked again on reconnecting**: what was waiting to go out when the link
  dropped is gone, so its line is told again (not silent) and it's forced again when due. And
  no force while the link's down (one went out stale on reconnecting, ahead of everything).

The mock: a dropped connection with a question open told it again and forced it; parity: all
24 runs the same (the session isn't in a scored run).

## The idle fallback on by default (the maintainer, 2026-11-11)

Watching a local model play E1M2 through Gary: her orders are often short (an area explored in
half a second), then she looks things up (status, inspect), and the bot stood still 10-20 s at a
time till Gary's idle prompt and our forced choice got her to act again. Pokes were considered
(the Skyrim integration's "[Do something]") and not taken: our forced choice at 6 s is already
the stronger nudge, and no prompt keeps the bot moving while she's thinking or talking. Now
`Director::new` sets `idle_auto` to 6 s (`IDLE_AUTO`): with no action for 6 s she carries on by
instinct, announced, exits held back; any action takes over; `doom_policy idle 0` has her wait.
The briefing says so. Section 9's first answer is changed with it.

## Her calls on the stream, `explore` as her setting, idle 3 s (the maintainer, 2026-11-11)

- **Instinct for her: new ground first** (built first, then made a setting): the second Gary run
  on E1M2 had the bot on instinct spend 31% of its time on pickups, 11% walking across floor it
  had seen, 10% pressing walls: her wandering finished rooms. On instinct for whoever directs her
  (the explorer's `resorts` given: never when playing alone, so the scored bot is untouched),
  going back over what's done (a walk across, a wall to press, another look, a hunt, a spare
  pickup more than 512 along the way) waits till there's no new ground.
- **`explore`, her fifth setting** (`doom_policy explore`): `new` (that, the default),
  `thorough` (everything as it comes, the run's own order; her explore orders search the most
  suspicious walls too), `quick` (new ground only, never back; no wall searching). Not
  measured: the scored bot never plays on instinct for anyone.
- **Idle 3 s** (was 6): the bot still stood a while after her short orders. The notice ("No
  action for 3 s: on by instinct till your next.") is urgent, so it prompts her as a poke would;
  separate pokes weren't added. Gary's own "act every" timer is the tester's to set.
- **Her calls in the stream's column** (EVIL'S CALLS): her stance, or one taken for a while with
  what's left of it; a target, gun or holding fire that still holds; any setting not at its
  default.
- A wrong setting value is answered with that setting's help alone (`policy::help_for`), not
  all five.

## At 100%, told and asked to leave (2026-11-11)

The third Gary run on E1M2 reached 100% (kills 41/41, items 42/42, secrets 6/6) at 906 s and
left at 1188 s. Nothing told her the level was done (her status showed the numbers; the model
didn't draw the line), the suggestions went on offering walls to search and another look with
every secret found, and on instinct the bot pressed walls and re-looked across the map, its
exit held back for her. Now, for directed play: at 100% with an exit known she's told once,
urgently ("Level 100%: every kill, item and secret. Exit: ..."), and if she's on instinct the bot
stops and asks "Everything's done here (100%). Leave?", leaving the default (nothing left
counts; staying is one answer). The suggestion at 100% is the exit; with every secret found,
walls to search aren't suggested, and on instinct no wall is pressed for one
(`Explorer::secrets_done`, set from the tally). The scored bot is unchanged (parity).

## Barrels in the way: not shot from beside them with a shooter about; a spot with no shot not chosen again (2026-11-11)

Two failures watched on E1M2 (through Gary). (1) 48 health lost: the follower's barrel in the
way, shot from 33 units off to run after (its plan "then off back the way she came", the shorter
walk than a spot out of every blast); a zombieman shooting at her set it off while she stood
there. With a monster about that shoots, as far as she knows (`Combat::now.shooters`, passed to
the follower each tic as `shooters_about`), only spots out of every blast; shoot-and-run only
with none about. (2) Stood 8 s, twice: at the spot chosen to shoot from, the check from where
she stood (within 1.5 of it) found no shot, a line grazing a corner; the spot was dropped, the
same one chosen again, 282 times. A spot found to have no shot is kept (`failed_posts`, 16 near)
and not chosen again for that barrel.

Scored, run for run: -0.0006 (s.e. 0.0007), 1072 of 1088 the same, 1 better 1 worse, exits 398
against 399 (E1M5 UV, one seed: died in a fight after clearing three barrels, within a level's
seed noise). Kept; the baseline is that set. Parity re-saved.

## Too near: a monster's own reach, not the gun's (the maintainer's, 2026-11-12)

Watched: against imps and cacodemons she backed away the whole fight, round corners, the route
walking her back after: a yo-yo. Each gun has a nearest to fight from (`keep.0`: the pistol,
chaingun and plasma rifle 192, the shotgun 128, the launcher and BFG 320), and the fight's
steering counted any spot nearer a monster than that (or than its own standoff, an imp's 128)
against it; an imp walking at her is inside 192 the whole fight. The gun's nearest is for a
blast that reaches her (the launcher, the BFG); for the rest, too near is the monster's own
reach (`standoff`). Now so by default (`steer.near_by_gun` false; true is the old way, parity
with it on: all 24 the same as before).

Scored, run for run: -0.0058 (s.e. 0.0093), 125 of 1088 the same, 294 better, 294 worse; exits
387 against 398; levels beaten every run 23 against 26. Within noise; the maintainer watched both (an
arena of 50 imps, E2M1 on UV) and liked it better. The baseline is that set. (A first try,
clawing monsters circled near where she met them, `melee_span`: -0.0042, 9 exits fewer, and it
missed imps and cacodemons; dropped.)

Cacodemons with only a pistol: she keeps clear at 1024 (`engage.keep_clear`), outmatched (over
20 s to kill one, `guns.outmatched`): the long retreat the maintainer saw, by design.

Stances (the maintainer's, 2026-11-12): `avoid` keeps each gun's own nearest too (`steer.near_by_gun`
on), backing away from what comes close; careful, steady, bold and yolo stand at a monster's
standoff (each states it, so a stance taken for a while over avoid stands its ground). "A
monster's reach" is `standoff` (style.rs): an imp's 128, a cacodemon's 160, about twice a claw's
reach (a melee hit lands under about 60 between their middles), more for what fires too fast
to dodge up close (a mancubus 400, a cyberdemon 640); none for hitscan zombies.

## Tried and dropped: a key seen with no way to it pulls exploring toward it (2026-11-12)

E2M2 (GPT through Gary, and the bot alone): the blue key seen early, its way found 13 minutes
later. Tried: ground to explore, doors, switches and teleporters counted up to half as near by
such a key (fading by 1024). Alone on E2M2 it barely moved (the way there opens from a room
1,400 off, by walk-over lines), and scored run for run -0.0047 (s.e. 0.0025), 16 better, 31
worse, exits 382 against 387: dropped. For her, the hint does it instead ("Explore toward it:
nearest unseen", e6fc4fe).

## On instinct for her: new ground the risky way before going back over (2026-11-12)

E2M2 (GPT through Gary): the far corner and the blue key past it were reachable only through a
corridor of crushers she'd stopped low by walking over their stop line (1518, on a path she
crossed eight times); a stopped-low crusher is the risky pass's (started from its edge, a stroke
timed), and the risky pass came only with nothing else left, which walls to search kept from
happening for minutes, till she set risk bold. Now, on instinct for whoever directs her (exploring
`new` or `quick`), the explorer tries new ground the safe way, then new ground the risky way,
before going back over what's done (`new_ground_first`). Never when she plays alone (parity: all
24 the same). Replayed alone on instinct the corridor never stopped low, and both builds went the
same to the tic.

## doom_goal: a standing aim (2026-11-12)

The SDK plan's stage 3: `doom_goal out | everything | key [ref] | kills | supplies | secrets |
none`. With no action of hers in hand the bot works toward it at once (no idle wait), on
instinct for her with what the goal needs: what it's after first (`Explorer::focus`: keys, hunts,
walls, pickups), everything as it comes and last resorts let go of for the thorough ones
(everything, kills, secrets), the exit given back for out (everything turns into out at 100%). Her
actions come first; the goal goes on after; setting one clears her queue. Met, or with nothing
more she knows of to do for it, it's said ("Goal done: you have the blue key.") and ended. A key
goal ends with its level; the rest stand across levels. Shown in her status ("for your goal"),
doom_why and the stream's column. Not in scored play (parity).

## The watch window: the engine's SDL, frames between tics (2026-11-12)

For the stream: steady frames and sound in step. Measured (`EVIL_DEBUG_FRAMES`, the stream
layout, WSLg, E1M1 HMP and MAP07 UV, 40 s each), the `minifb` window kept to its own sleep timer:
59.4 frames a second against a 60 Hz screen, which slips a frame about every 1.7 s, and Doom's
35 tics on 60 frames showed each tic for one frame or two (41% of frames showed nothing new):
judder. Her thinking wasn't the trouble: the slowest frame's stepping took 7 ms.

Now the window is the engine's own SDL (`evil_window_*`, `viewer/window.rs`): a texture scaled on
the graphics card, shown at the refresh with vsync (paced by the clock where there's none),
native on X11, Wayland and Windows. Its events are taken out by an event filter as they come in:
drawing polls SDL's events (NetUpdate), and the game took keys for its menu and Alt-F4 for
quitting. And each frame is drawn between the tic before and the last (`evil_draw_at`, DSDA's
own interpolation, `movement_smooth`, given how far between by our clock, not its). What that
changes is only the drawing: the previous positions and view the game keeps a tic back, and
moving floors set between for the drawing and put back before anything reads them; the weapon's
own smoothing needs `I_StartDisplay` (its "first frame since a tic"), without which the gun
jumped about. After: 59.9 frames a second, the gap 16.7 ms at the median and 16.8 at p95, no
frame late in 43 s; of the frames that played no tic, 88% showed a picture moved on.

Play unchanged: headless watches (drawn a tic at a time) on five levels gave the same logs as
before; window runs at x4 (drawn between tics throughout) gave the same logs as the headless
ones; parity: all 24 the same. The score's small window stays on `minifb`.

## Music: SoundFonts through FluidSynth (2026-11-12)

The engine was built without FluidSynth, so Doom's MIDI played only through the OPL synth (a
Sound Blaster's FM), which the maintainer found unlike the Roland sound the music was written
on. Now FluidSynth is built in, and `--soundfont NAME` (the console's Music) plays it with a
SoundFont from `soundfonts/`; without one it's OPL as before (the engine's default player would
have been FluidSynth, so the player is set each start). FluidSynth's reverb and chorus are on: the
songs ask for them. The player is set before the game starts without its change callback, which
restarts the music and crashed with the sound not yet up.

Two SoundFonts free to redistribute, fetched by `tools/soundfonts.sh` (pinned, hashes checked)
and in a release, never in git: GeneralUser GS 2.0.3 (its own permissive license) and MuseScore
General 0.2 (MIT, from Fluid R3). Listened to on E1M1, the maintainer found GeneralUser GS the
most like the original. Others (an SC-55 one, say) are anyone's own to add. GeneralUser GS 2
needs FluidSynth 2.3; the Linux release builds 2.6.2 without its own audio drivers (the distro
one brings JACK, PipeWire and readline).

Found while choosing them: the Steam re-release's `rerelease/extras.wad` has the original
soundtrack recorded on a Roland SC-55 Mk2 (`O_` lumps, Ogg Vorbis, 87 tracks) and Andrew
Hulshult's IDKFA (`H_`): see the next entry.

## Music: the re-release's recordings (2026-11-12)

`--music sc55|idkfa` (the console's Music) plays the songs as the Steam re-release recorded them.
`crate::music` finds `rerelease/extras.wad` (beside `base/`, Steam's folders, or
`EVILBOOM_RERELEASE_DIR`) and makes a music WAD in out/music/ for the IWAD: each song lump
(`D_E1M1`) the recording's Ogg data, the WAD loaded before any PWAD so a PWAD's own songs stay
its own, and the engine built with Vorbis. Songs not recorded play through the MIDI synth.
Nothing of the re-release is copied elsewhere.

Which recording is which song is a table (`music_songs.rs`), derived once from the four IWADs
and checked against extras.wad by a test: the same name; the same score under another name
(E2M5 is E1M7's); the same score but for a few timings (DOOM's intermission is E2M3's, the
score 99.98% alike; E3M5, E3M6, E3M7, E3M9 are E1M7, E1M6, E2M7, E1M9's, 99.93% or more); DOOM
II's MAP14 and MAP22 Dave D. Taylor Blues, its MAP08's (re-arranged, taken as the same song on
the maintainer's word); TNT's own songs by the map that first plays them (O_TNT01...). Of the
songs: DOOM 31 of 32 (not INTROA, unused), DOOM II 35 of 35, Plutonia 35 of 35, TNT 24 of 35
(eleven of TNT's own have no recording).

## Music: no bundled SoundFonts; the SC-55 one the default (2026-11-12)

Listened to side by side, GeneralUser GS and MuseScore General fell short of the SC-55 sound, and
the maintainer had them dropped: no SoundFont is fetched or shipped, `tools/soundfonts.sh` is gone,
and `soundfonts/` is for the player's own. The one to have is Patch93's SC-55 SoundFont (made from
Roland's samples, so not ours to ship): the docs say to search for it and save it as
`soundfonts/SC-55.sf2`, and when it's there it's the default MIDI synth (`--soundfont opl` for
OPL; the console's MIDI synth "Default"). With the re-release's recordings (`--music sc55`) it
plays only what they don't have.

## Stocking up before going on (2026-11-13)

Played through (an episode, the console's "Go on through the episode", Evil's continuous play),
she left a level with the exit as soon as she knew a way to it, and what she'd seen lying about
and not needed this moment stayed there: the next level began short of it. Now, going on
(`Hooks::stocking`, the run's `going_on`): once she's seen the exit in this life (the level's
start or since she last died: `Explorer::exit_seen_this_life`, a sector by a known exit line in
her view since) and it's her choice, what she'd take with her is had first, the nearest by her way
each time, until nothing she knows of would add anything: health to 100 (more from the spheres),
armor better than hers, ammo for her guns short of full (a backpack's room counted), a backpack.
Not the bonuses; not the powers. Only by a way clear of what she knows is awake and too much
for her. Those items are set aside from her other choices till then (`stock_only`, kept out
before `list_objectives`' cascade: kept in, they masked its fallbacks, and she chose differently
long before the exit; one run ran its planning out of time chasing far ones, 96 s of thinking on
E2M4, and stuck). Single levels never go on: the score is unchanged (parity).

Evil, going on: `doom_exit` with those left asks first, naming them and what she has, the
default to stock up (the supplies goal, which may choose them), `sure` as before to leave as
she is. Tested through neuro-mock: from 74 health, 106 armor, 41 bullets, 7 shells to 101, 200,
91, 50 before the goal was done.

Measured, `episode` on one life, the four DOOM episodes and DOOM II from MAP01, 8 seeds each
and E3 on 8 more (`EVIL_DEBUG_STOCK=off` for the other side): 5.04 levels an episode against
5.08, 16 of 48 through against 15. On the levels she left, her health rose after the exit first
came first in 68 of 226 (20 of 229 without), and she left with on average 2.7 more health, 2
more armor, 40 more bullets and 13 more shells, 42 s later. No death came while stocking up (the
deaths that differ are earlier in later levels, the run gone another way). E3M4 kills her about
half the time either way.

## Out of reach, out of mind (2026-11-13)

Seen on stream, MAP07: the stream's "now" going between fighting the mancubi and pressing a
switch for a minute, the mancubi not firing. Her log: every 1.4 s "fight over", "lost sight of
Fatso: keeping at it", "out of reach: going on", and round again. A mancubus on a far ledge came
in and out of view as she walked to the switch: in sight, `out_of_reach` judged it beyond every
gun she'd bring up and sent her on; out of sight, `lost_sight` (meant for one that went behind
a pillar) took it up again, never asking about reach. Now `lost_sight` keeps at one only if a gun
she'd bring up reaches where it's reckoned to be, the same test as in sight. On MAP07 HMP, 8 seeds,
the cycles went from 63 to 0. Score, run for run on the baseline's runs: +0.0093 (s.e. 0.0083),
249 better, 192 worse; 0.450 against 0.440, exits 389 against 382.

## doom_name dropped (2026-11-13)

`doom_name` (name a place or thing, the name then usable as its id) came in with the first slice
and was never judged by use. In every GPT stand-in log (12 runs: 132 `doom_go`, 106
`doom_explore`) it was called not once; the only call on record was a hand-written mock line.
It cost an action in her context all game and a choice in every "What next?" force, where a weak
model could waste a turn on it. Removed from her actions and the console's raw actions; the
naming itself stays, for directed scripts (`name`). Back in if Neuro on stream would name rooms
for the fun of it.

## doom_stop is doom_pause (2026-11-13)

`doom_stop` dropped what she was doing, and 3 s later instinct took over again: "stop and wait"
in name only, and never called in any log. Now `doom_pause` pauses the game, as a player
stepping away: Doom's menu up (on its own screens), the game held, no instinct, nothing told
her and no decision forced, till she plays on (`on: false`), or the watcher or the console
does. No limit to it: nothing happens in a paused game. Meanwhile only what doesn't move her is
registered (look, list, inspect, status, why, answer, policy, goal, pause), and anything else is
refused, the console's as well. The directed scripts' `stop` stays as it was.

## A far shot goes out level; a barrel's tap is the route's (2026-11-13)

Seen recording a demo, E1M1 on Ultra-Violence, Evil's goal "everything": she walked to a spot to
shoot a barrel from, a barrel went off, and she stood there firing till her pistol was empty (42
rounds for 25 damage). Two things, traced in her log and replayed through `neuro-mock`:

- The barrel that went off wasn't hers. A shotgun guy came into sight 1,098 off as she got to
  her post, and the fight took the trigger ("shooting on the way"); the first shots at him hit a
  second barrel on that line. The rest went into the side of the pedestal he stands behind (48
  high, all but his top 8 hidden): within 1,024 autoaim tips a bullet up at what shows of him,
  further off it doesn't look (`dsda_PlayerAimBad`, `16*64`) and the bullet goes out level. Her
  shot check had autoaim at any distance. Now, past autoaim's range, a shot is clear only level.
- She stood there because "shooting on the way" leaves the legs to the route, and the route's
  next thing was the tap at its barrel, which wants the trigger and the facing the fight had.
  Neither got done for the 20 s a barrel is given. Now, the route stood to shoot a barrel and the
  fight's move no more than shooting on the way, the gun and the facing are the route's, till
  the tap has landed (the pistol lets go some tics after the trigger). Any other move of the
  fight's still has her.

Not done: autoaim takes a barrel for a target as it does a monster, and her model of it counts
monsters only.

The same start after: the barrel gone in 3 taps, no shot at the shotgun guy from the post, four
shotgun guys dead for 14 rounds by 13 s. Score, run for run on the baseline's runs: +0.0115
(s.e. 0.0060), 196 better, 143 worse, 544 the same; HMP +0.0129, UV +0.0100; 0.461 against 0.450,
exits 399 against 389.

## The 100% goal and its last secret (2026-11-13)

Seen on stream, E1M2 on Ultra-Violence, Evil's goal "everything": 5 of 6 secrets and then a
quarter of an hour of searching that never found the sixth, a secret door (line 897, sector 106)
in the big computer room. Her log: she had the wall as a suspect, 5.5 by her rounds' score and 6.0
by the search's (the level's highest), and was never sent to press it. The model searched four
other areas, of weak walls; nothing she was told said where the strong ones were (`doom_list
areas` gave the door's area as "1 to explore"; the stall notice offered the restart). On
instinct, walls come when there's nothing else, and after each switch whose effect she hadn't
seen there was something else: another look at every sector round her reach (the same seven
twice), lines stepped over, the switches pressed again. Six walls in 280 s. Three times
"look at sector 106 again" walked her to the door to look at it, shut.

- **On instinct for a goal after the secrets (everything, secrets), one left and a way out
  known,** the walls a search would press (its score, thoroughness 2) are in with her ordinary
  objectives: after new ground, pickups and hunts, the likeliest (thoroughness 1's) before the
  rest, nearest first; before a walk over floor she's seen or another look. No way out known,
  the order's as it was: walls are then for the way on, in their turn.
- **Where to search is said.** `doom_list areas` gives an area's likely secret walls (the
  lightest search's); with a secret left and nothing new for 90 s, once: "Secrets 5/6. Likeliest
  walls: area-0 (3), ... doom_search them." (not under a goal that searches them itself).
- **Her goal everything, what the items tally counts is hers to pick up, need it or not.**
  A bonus was a spare pickup only under 200; at 200 with one left off her way, the tally
  couldn't be finished. (No run was found that failed for it; she mostly walks over them.)
  Only once she's seen a way out this life (the maintainer's rule; `exit_seen_this_life`, as
  stocking up goes by): till then a sphere or a berserk she's no need of is left for when she
  might, and one known from a life before isn't seen yet. E1M1 from 200 health and armor: 3
  bonuses gone for before the exit came in sight (hurt by then), 31 after; 37/37.
- **Her goal everything and no kill, item or secret more for 3 minutes, she's asked** "You
  haven't found the last secret in a while. Keep searching?" (what she has most left to find
  named), a) leave the level, the default, in 20 s or her answers' wait if longer; b) keep
  searching. Asked again after 3 minutes more with nothing found. The question about being
  stuck isn't asked on top of it.

Measured through `drive` (`directed`, `goal everything`, 1500 s, every question answered keep
searching), E1M2 UV on 4 seeds: 6/6 secrets and out in 660-825 s on all four, against 5/6 on
all four and not out. Elsewhere it costs time where the old order found everything anyway:
DOOM II MAP01, 100% in 226 s against 160 (48 walls pressed against 13); MAP02 the same to the
tic; MAP05 2 of 3 secrets either way, the second at 852 s against 747. Undirected play is
untouched: parity, all 24 the same.

## Stuck in one place: three causes (2026-11-13)

Seen on stream, E1M1's exit room: stuck between a barrel and a corner till every objective
timed out. The follower's "stuck" counted over eight levels on Ultra-Violence (seed 1) then
showed it isn't corners in general: nearly all of a level's were at one spot, each spot its own
cause.

- **A lane round a barrel went through bars** (E1M1). The lane search asked only for floor
  under her box's corners; a line she can't cross with the same floor both sides passed.
  Now a cell is free where the game's own position check has her box clear
  (`jumps::physics`). Six seeds from inside the door: never there before, there in 3 to 9 s
  after. Score +0.0005 (s.e. 0.0022).
- **The sixth hanging body is solid** (MAP05, an alcove one fills): the table had 73-77, the
  game has 78 too; checked whole against `info.c`, the only one missing. 31 stuck reports to 1.
  Score +0.0006 (s.e. 0.0006), 1 run better, none worse.
- **Kept at out of sight time and again, given up on** (E1M7: imps 1,100 off pulling her to
  and fro, "shooting on the way" in sight with no shot, "keeping at it" out of sight; 133 times
  against 29 before the far-shot rule of this morning). Two rules on the logic lost first:
  nothing past autoaim's range in reach unless it's level with her gun (-0.0061, s.e. 0.0059,
  exits 394 against 400; on E4M9 HMP, 9 seeds, no exit against 3), and the same for
  `lost_sight` alone (E4M9: 1 exit of 9, dead two and a half minutes sooner). Traced on one of
  those seeds, the once is worth having: kept at where the fight was, what's coming comes to
  her a few at a time; sent on, she opens a new sight line hurt. So the logic stays and the
  repetition goes (the maintainer's): kept at the same one, on and off, for `lost_again` (8 s,
  some three times round) with no shot at it in between, it's given up on a while, by the
  ledger the other efforts use. E1M7 UV seed 1: 133 to 21, 63 stuck reports to 1, out in 706 s
  against 1007. E4M9: 7 of the 9 seeds the same to the tic, 3 exits either way. Score +0.0015
  (s.e. 0.0044), 66 better, 66 worse, exits 402 against 400.

## A run-jump under a crusher, and lined up under fire (2026-11-14)

The catalogue's longest loop, MAP06 UV out of ammo for most of an hour, traced back: she never
got onto the crusher's platform in the middle of the room (the plasma rifle on it at every
skill, the blue key on the easy ones). 5 of the baseline's 8 UV runs and 2 of the HMP ones stall
in that room. Getting on is a run-jump across an 8-unit gap, and two things stopped it:

- **She waited for the crusher at its top.** A jump wants the heights it was found with, a
  ceiling 16 lower at most. A crusher that never stops is there some 32 tics in 500, slower
  still while it's crushing something (an eighth the pace). Now a ceiling lower than that will
  do if it leaves her room from when she'd be on (20 tics) for 3 s more, going on as a crusher
  does at the pace it has: `room_through`, what a crusher walked into is timed by.
- **She was never square on to the takeoff.** With imps in view the fight had her facing
  ("shooting on the way to the plasma rifle") till a dodge took her off the spot. Now, from
  what the jump needs being there till the sprint begins, the facing is the route's and the
  fight holds its fire, whichever of its moves leaves her legs to the route; what's urgent
  still takes her.

Tried with it and dropped: this morning's barrel rule (the route's gun while it's stood to
shoot one) widened from "shooting on the way" to all those moves. With it the score was
-0.0032 (s.e. 0.0036), exits 397 against 402; without, as kept, -0.0012 (s.e. 0.0034), 40
better, 41 worse, 963 the same, exits 401 against 402. MAP06 HMP: 6 exits of 8 against 3. UV:
none either way; on the traced seed she now has the plasma rifle at 357 s, but the key there
is on the west platform (up the lift, a jump to the ledge south of it, its switch, a run
across the room: the maintainer's recollection, and the map's), which nothing she knows of
sends her to; and out of ammo the fight has her legs most of the time. Both still to do.

## A wall pressed from where it's pressed from (2026-11-14)

MAP06 UV again, after the jump: she had the switch on the ledge south of the lift (line 244)
as an untried switch all along, and no way to it, having lost the lift. The lift is called
back down by its face (line 359), a wall to her till it's pressed. She set out to press it
eight times, the fight taking her each time (ducking out of the mastermind's sight); then the
press stage, begun again wherever she'd been taken to, turned her to the wall's bearing and
pressed at nothing from 330 off, and the wall was down as tried for good.

- Kept: taken more than 24 from where a wall's pressed from before its press, she doesn't
  press: it's left as anything interrupted is, not tried. And a wall never in front of her
  while its press was still to make isn't tried when its time is up. The log says each wall
  pressed, and each left with it in front of her. Score +0.0003 (s.e. 0.0052), 77 better, 92
  worse, 737 the same, exits 405 against 401.
- Tried with it and dropped: back to the wall up to three times when taken off it, and her
  facing the press's while she's at it (as a switch's is). -0.0090 (s.e. 0.0063), exits 392
  against 401, nearly half the runs changed: more of her time at walls with things about.
  The patch is out/score/wall-press-full.patch.

With a loadout (shotgun, super shotgun, chaingun, a backpack's ammo, megaarmor) MAP06 UV is
then played through, 4 seeds: the mastermind crushed, the plasma rifle off the platform, the
lift's face pressed, up and across to the ledge, its switch, the run to the west platform on
the third press (the first two before she knew what it did, or had seen the key), out in some
1,100 s. From a pistol start she's out of ammo in five or six minutes with 1 of 17 items had,
and that's what's left there: what she spends it on, what she picks up, and what she does
with none.

## A stair of crates smaller than she is (2026-11-14)

The maintainer's, watching E4M9: a box of shells on a crate by the start with a stair of smaller
crates up to it, and a megaarmor a short jump on from there, never gone for. The stair is 16,
then 32 and 40, then the 64 of the crate itself; the two lowest are 16 square, half her width.
In the game she goes up it: she stands on the highest floor under her box, so walked west
along its south side, her middle over the floor beside, she's up 16, 40, 64 (by hand in
`drive`: anywhere in a band 12 wide, at a walk or a run). The mesh has her centre in a sector
and her box through its faces, and with the next crate up in the way it fits through none:
out of reach, to the planner.

That's what a climb is for (`jumps/climbs.rs`), and the finder found this one, and threw it
out: its check that she can't walk there already went by each portal's width alone, where the
planner's own test has the ledges by a portal as walls. Now the check is the planner's
(`Navigator::fits_at_start`, the flood's `fits_now` at the starting heights). The finder's
version is up, 30: every level's jumps found afresh.

Only E4M9 has such a climb, of the 68 levels: 1078 of the 1088 runs the same to the tic, 6
better, 2 worse, +0.0016 (s.e. 0.0015), exits 407 against 405; E4M9 HMP 5 of 8 out against 3.
She goes for the megaarmor 11 s in.

## Her rounds and the spider mastermind (2026-11-14)

MAP06 UV from a pistol start again: out of ammo in five or six minutes. Not from poor shooting
(238 of 312 pellets on a monster, 10 shots of 96 on nothing): the level has 32 shells and a
clip lying about and the rest is cells and rockets; and a third of what she fired went into the
spider mastermind (32 of 96 on the seed traced; 18, 28, 44 on others), 3,000 health that all
she carries couldn't kill and the crusher is there for.

Tried, on the rule that keeps her ammo from what she can't kill (it held only in "keeping
clear"):

- **In every move** (ducking from it, dodging another's shot, waiting out the crusher): lost,
  -0.0146 (s.e. 0.0057), exits 386 against 407, 50 levels changed, the boss levels most. Not
  traced; the rounds she "wastes" on a baron from a pistol start seem to earn their keep.
- **"In her way" only on a level with her** (the mastermind on its platform, 128 over the
  floor she went by on, had counted as in her way, the rule's exception): level, -0.0005 (s.e.
  0.0018). Right as far as it goes, and it took few of the shots away by itself. Not in; the
  patch is out/score/in-her-way-level.patch.
- **Kept, the maintainer's: no shot at what outmatches her that stands in a sector a crusher
  she knows of moves, at work or not, whatever the move.** By what she's learned, not the map:
  in one life it saves little (30 of the 32: she shoots before she's pressed the switch and
  seen what it does), and that's the point of it. Given five lives she shoots the mastermind
  30 to 44 times in the first and 0 to 5 in each after, where without the rule it was 21, 59,
  29, 26 in later lives. Score +0.0008 (s.e. 0.0014), 4 better, 2 worse, 1082 the same (MAP06's
  only), exits 408 against 407. By the map's own crusher sectors the shots go at once (0 to
  2), and that's knowledge she hasn't earned: not done.

Seen in the same runs and not done: in a later life, knowing the switch's crusher kills what
outmatches her, she doesn't go and press it till late.

A blind pistol start here on Ultra-Violence may be more than to ask of her (the maintainer's).

## A way round a jump over a fall for good, and a door pressed shut

MAP06's secret (rocket launcher, backpack, megasphere, megaarmor, behind a door on the ledge
the spiral stairs' top jumps to) was never gone for, "no route": the plan to the stairs' top
took a run-jump across the lava the stairs go round, a shortcut, and a way over somewhere
there's no getting back from is refused whole (`refusal`). The jump onto the ledge itself is
over the stairs, and was fine.

- **Kept: plan again without the jump (and its twins, between the same two floors) before the
  way is refused; taken if no more than 1.5 times as long and 256.** MAP06 UV with a loadout,
  goal everything: the secret found and the launcher taken on every seed that lived to get there
  (before: once in two, by the risky twin, at 1253 s). Score +0.0031 (s.e. 0.0037), 58 better,
  41 worse, exits 409 against 408.
- **Without the cap on length, lost** (-0.0008 with the change below it, exits 403): the way
  round was often several times the length, through fights and hurting floors (MAP11: dead in
  a minute or two; E1M5: 2450 along for 380), where the old way drops the thing and takes the
  jump later as a risk.
- **The test's own gap, left.** `falls_for_good` takes a takeoff on the very lip of a pit for
  the pit's own poly, the pit then an end of the jump and not looked under (a 283 jump over
  MAP06's last lava passed so, missed, and killed her). Told right (the ends by where it's run
  from), it refuses lip jumps she makes and lands all the time: -0.0065 (s.e. 0.0045), exits
  395, with the capped rule; +0.0017 without. So the twins go with a refused jump whether told
  to be over a fall or not, which shuts that hole where the new rule opened it, and no more.
- **Dropped: a thing given up for want of a route offered again once she's moved and is near
  it** (the first try at the same secret). -0.0002 (s.e. 0.0045), MAP06 HMP -0.29 (three exits
  gone, the crusher room gone at in another order). The patch is
  out/score/no-route-near-full.patch.

Found by it, on E1M5: a zombieman opened the exit room's door from inside, she pressed it as
it stood open, and it shut. Nothing learned, the door "tried", and never pressed again: 1,000
seconds of everything else, then dead in the nukage. **A door she can open again and again
whose press taught her nothing is pressed again when there's nothing else, as a repeatable
switch is** (`pressed_again`). That run: out at 605 s. The score's runs: none changed (1086 the
same to the tic, 0 better, 0 worse): it's rare.

## E2M9 from a pistol start: armor before a gun

E2M9 (four barons round the start, ten cacodemons next door, the keys and the exit past
them): 0 exits of 16. Driven by hand, straight for the exit by the megaarmor at her feet and
the soulsphere on the way, shooting nothing: out in 21 to 25 s on 5 seeds of 8, either skill.
A demo the maintainer watched takes every gun in the first room without a shot, facing the
barons to see their fireballs, then lets the barons and the cacodemons at each other next
door. (A monster hurt keeps to who hurt it for 100 of its steps whatever else hits it, P_
DamageMobj's threshold: every shot of hers at a baron keeps a cacodemon's stray one from
setting it on the cacodemon.)

- **Kept: armor within `errands.heal_near` that adds 50 to what she can take is urgent,
  outmatched** (`errands.armor_outmatched`). She walked off from the megaarmor, 64 away, for
  a chaingun at a baron's feet: a better gun was urgent and armor never. Score -0.0011 (s.e.
  0.0017), 21 better, 15 worse, 1039 the same, exits 407 against 409: level.
- Tried with it and not enough alone (0 of 8 each): no shot at what all her ammo couldn't
  kill; that and on along the route (walks into the barons' claws for the guns); that and no
  guns at all (dies at the door, the barons on her by the time she tries it).

## Pockets: no retreat into a small room with one way in

The maintainer's, from watching E2M9: she dies shut in somewhere small, the way out full of
what's after her (an alcove of the first room, the closets with the keys). Every retreat chose
among the places she'd been, the nearest out of the shooters' sight, and nothing looked at
what was behind the place.

`pockets.rs`: the ground round a spot with one narrow way in. Grown out from the spot over
the mesh, each time by the neighbour that leaves the least edge open, the stage with the
narrowest open edge kept: the mouth, one portal or several side by side. A pocket has a mouth
of 320 at most, is the lesser part of her ground, at least 64 deep for its mouth's width and
half as deep as that is wide (a room's corner the mesh cut off isn't one). On E2M9: the eight
alcoves, the key rooms as one behind their 128 mouth, the closets in them. `drive`'s
`pockets` lists a map's. (First by the graph's bridges: one portal's mouths only, and the
alcoves, joined by two or three, weren't found.)

- **Kept: outmatched, a retreat's places (`visited_spots`: cover, out of a ring, off for
  ammo, baiting) leave out those in a pocket of up to 300,000 square (`retreat.pocket_area`)
  while there's another.** Not outmatched, a doorway is still held. Score +0.0017 (s.e.
  0.0046), 109 better, 101 worse, 772 the same, exits 409 against 407.

To come, by the same test: a gun or armor she needs in a pocket fetched and straight out
again, the rest of what's in one left while she's chased, out early when its mouth is being
closed on.

## A fight score beside the score

The score is mostly exits, and most runs end in a death (641 of 1,088): a change that has her
last longer or fight better without tipping a run into an exit doesn't show, and one flipped
exit counts for as much as a real gain elsewhere. A pile of fight changes that lost a hundred
exits read -7.5 standard errors on exits and -11 on time alive alone, from the same runs.

The maintainer's measure, kept beside the score (`Row::fight_points`, printed by score-report
and step.sh): a run's first twenty minutes. Out within them, 1. Else the health the monsters
lost less what she and her armor took, over all the health the level's monsters had; three
quarters of it for dying in that time (a flat quarter off at first: two deaths in three had
dealt less than that and all read nothing); nothing below nothing. A run's RESULT line and the score's file
carry the three numbers (dealt, the monsters' health, taken), as they stood at twenty minutes
for a run that goes on past it.

The monsters' loss is to whatever, not only to her: the game's hits aren't told apart by who
dealt them without a change to the engine's patch, and what she has them do to each other is
hers by the way she plays (the infighting E2M9 is won by). Fifteen minutes was the first
thought; 57 of 409 exits take longer, 18 longer than twenty.

Not what a change is kept by: that stays the score. For reading changes to the fight.

## Off a hit she didn't see coming; out by a gap

Two of the maintainer's, from watching E2M9, each scored alone by a tuning number that turns it
off (the control with both off: every run the same as the baseline's).

- **Kept: hit from a side with nothing in sight there, she's off the other way for half a
  second** (`senses.flinch_tics`), as near straight away as the floor allows, then on with what
  she was at. Clawed from behind she had only turned to look, and was clawed again. Score
  +0.0053 (s.e. 0.0075), exits 411 against 409 (58 gained, 56 lost); the fight score +0.0096
  (s.e. 0.0071). It changes 677 runs for that: a smaller step, just out of a claw's reach, to
  be tried.
- **Kept: surrounded, with a body across her way and others close (`steer.hemmed_near`), out
  by the widest gap between them she fits, the nearest to her way** (`through_gap`). She had
  pushed on at a cacodemon while the rest closed up. Score +0.0017 (s.e. 0.0054), exits 414
  against 409 (32 gained, 27 lost); the fight score +0.0011 (s.e. 0.0048).
- **Not kept: outmatched, what's in a pocket waits unless it's a gun or armor she needs or the
  way on.** Right on E2M9; -0.0130 (s.e. 0.0062), exits 398, the fight score -0.0112 (s.e.
  0.0053).
- **Not kept: no fight to be had** (holding fire and gathering till the ammo in hand would kill
  four fifths of what's awake, the way out weighed first, her eyes on the middle of the missile
  throwers near her, full pace whichever way she faces). With the three above: -0.110 (s.e.
  0.010), exits 310 against 409. From a pistol start that's the opening of most levels, and
  the thing to do there is shoot. On E2M9 itself, 2 exits of 40 against 1. The patch is
  out/score/pile-4.patch; it wants a far narrower sense of when a fight can't be had.
- An experiment, not realism (`out/score/known-throws-experiment.patch`): knowing of every
  missile thrown by a kind of monster she's tracking. E2M9, forty seeds: 6 exits against 1,
  141 kills against 75. Knowing where the monsters are for five seconds after losing sight
  of them, without that: nothing (0 against 1).

## Leaving with things left: the default goes by what she's said

`doom_exit` with things left asks "Leave anyway?" whatever her goal or settings, and no answer
stayed. With her goal `out`, or her `explore` setting `quick`, she has said already that
leaving is what she wants: there no answer leaves. Still asked (the maintainer's choice over
not asking at all), so she can change her mind on seeing what's left. The stock-up question
(going on to another level) is as it was. Directed play only: the score's runs don't ask.

## Exploring's names, a goal that leaves exploring alone, and completionist asked first

The GPT stand-ins chose 100% and thorough nearly every time: dull to watch (the maintainer's).

- **The `explore` values renamed:** `normal` (was `new`), `methodical` (was `thorough`),
  `speedy` (was `quick`). What each does is as it was. The old names are refused, with the
  new ones listed.
- **A goal no longer changes how she explores.** Completionist, kills, secrets and supplies
  made her go back over ground as it came, whatever `explore` said. Now going back is her
  `explore` setting's alone; a goal says what she's after (what's put first, when it's met,
  whether the exit's held). What a thorough goal still takes unasked: the last resorts
  (completionist, kills, secrets) and the wall search (completionist, secrets). With `speedy`
  a goal that needs old ground runs out of things to do and ends, told as that.
- **`everything` is `completionist`, and asked first** ("Slow, and dull to watch. Sure?", the
  default no), as yolo is. The question's own yes sets it without asking again.

Directed play only: no score to measure.

## A goal doesn't play for her

A goal set, the bot went on instinct for it at once, with no idle wait, till it was met. A
stand-in set completionist and did nothing else all level: the bot played, and she was never
waiting, so never asked what next (the maintainer's, from watching). Now a goal starts
nothing. It puts what it's after first in two places: the pick suggested to her (lists, a
look, a decision forced, the nudges; "your goal: ..." its reason; the way out first under
`out`), and what instinct does once the ordinary idle wait has put her on it (from its first
choice). Setting one starts her wait afresh. A key goal with no key seen says so. With
`idle 0` instinct never runs, so every action of hers that ends leaves her waiting, and
she's asked what next after `force_idle` (6 s), her goal's pick marked in what she's shown.

## Asked what next while she's on with something

On instinct she was never waiting, so never asked anything: a model that sets a goal or
nothing at all let the bot play the level. Now a decision's forced on her ("Going to open a
door. Carry on, or something else?") when instinct takes up something new
(the steps she's told of, not each objective: those last a second or seven, less than her
answer takes) and when a fight worth telling of is over ("Fight over. Before it: ..."),
whether she was on instinct or an order. At most every 15 s (`force_next`, the console's; 0
never), not mid-fight, not over an open question, and she goes on meanwhile: her answer
overrides, it isn't waited for. A force can't be declined under the SDK, so carrying on has to
be an answer: it's a question like the others, answered with `doom_answer` ("a) carry on
(default)  b) stop here", the second on instinct only: off it, waiting for her), not a new
action in her context (first built as `doom_carry_on`; the maintainer's point). A light one:
not one of the minute's three, closed by any other question, silent when its time's up. Worded
by what she's doing alone ("Going to open a door. Carry on, or something else?"). Never asked
while an order of hers is in hand, except after a fight that broke into it. Any action of hers
starts the 15 s afresh. The 6 s force is as it
was, for when she's standing with nothing to do. To be judged by watching with a model:
whether a carry-on every so often is a better watch than the bot left to it.

## No standing about between her actions; instinct on or off

From a run with a GPT model through Gary (E1M3): she stood for seconds after each action of
hers, and sent `doom_fight auto` nineteen times.

- The model's first `doom_policy` filled in every field, `idle: 0` among them, which turned
  instinct off: after each action she stood till the 6 s force. A number whose zero means off
  is a trap for a model that fills in every field. Now it's `instinct: on|off`, and on means
  at once: an action of hers done, instinct takes up that tic (it was after 3 s), and she's
  asked what next then, whatever the 15 s (the action's end is the moment to ask). Off, she
  stands and waits, the 6 s force as before.
- Actions that move nothing (a look, a list, her status, a setting, her say in a fight) put
  off the waiting force by 3 s at most and the what-next ask not at all: sent one after
  another they had kept either from ever coming.
- `doom_fight auto` with nothing of hers holding says so ("Nothing of yours to end"). Most of
  the nineteen were Gary's auto-act, left on by accident, wanting some action from her.
- A stop order stops: not on by instinct the moment after (the scripts'; she has no stop).

The what-next question carries her status, the suggested pick and "Unsure? doom_look, or
doom_list." only: it had the lists a waiting force has (up to 27 lines), every time, and
carrying on needs none of them. The waiting force keeps them: there she has to choose.

## A switch looks like one only from its front

MAP06's megasphere is in a shut compartment in the ceiling (a lift, sector 98). Its east side
(line 458) has a switch's texture on the inside, to bring the lift down again from up there.
`looks_interactive` went by the front side's textures whether or not she'd seen that side: she
"found" the switch at 0.5 s from the room, and went to press at it from below (the use line
stops at the compartment's shut side), three presses a visit, again each time the drop's wait
ran out: four visits in a directed run, six (18 presses) on seed 1. Now a two-sided line looks
like a switch or a door only once she's seen it from its front (`Knowledge::seen_front`, kept
as lines are drawn in her view). Seed 1, no monsters: no press at it, the level in 395 s for
470. Score +0.0038 (s.e. 0.0044), exits 413 for 410; fight score +0.0014 (s.e. 0.0040).

An open line with no face is no wall to press: MAP06's lines 468 and 471 are ways through
between two floors at one height, with no texture and nothing above or below them, and were
taken as "unlike the walls round them" (no face against METAL2), then given the megasphere
behind and a gap to see it through: 9.0 and 8.5, pressed before the wall that works (7.0).
Score +0.0008 (s.e. 0.0019), exits 414 for 413; 1,036 runs the same.

## On a floor that hurts, the route at full pace whichever way she faces

MAP06, a directed run: backing off an imp across the nukage of sector 101, on a route to get
off it, she went at 15% of her pace (`Motion::go`'s slowing for legs against her facing) and
lost 70 health in 13 s, 5 at a time. The slowing is a caution kept on purpose everywhere
else (without it: -0.017). On a floor she knows hurts, with no suit on, it's now off
(`Motion::any_way`, set by the explorer each tic). Score +0.0061 (s.e. 0.0055), exits 422
for 414; fight score +0.0064 (s.e. 0.0050). Skill 4 gains (+0.0173), skill 3 doesn't
(-0.0050). It changes 454 runs.

What a floor takes isn't a reason to fall back to cover: the same run, an imp in sight and 5
health a second going to the nukage, she fell back "to cover" from the imp, the floor's doing.
Health lost with nothing having hit her (the status bar's face), on a floor she knows hurts,
is the floor's (`now.hit_rate` is the rest), and cover goes by that. The other rules that go
by the rate she's losing health keep all of it. And the fight's log says "the floor" or
"nothing near" where it said "a shot" (a label only: nothing reads it). Score -0.0004 (s.e.
0.0003), exits 422 for 422; 1,057 runs the same. Kept as the right rule, not for the number.

## Through a jump's sprint she faces the jump

MAP06 at skill 2, directed: three jumps failed in a run (two off the big lift), each with the
fight "shooting on the way" at an imp far off; the twelve that landed had no fight aiming. The
sprint presses forward the way she faces, lined up along the jump, turning 3 degrees a tic at
most; the fight's aim laid over a route's legs keeps their way for the one tic only. Turned
to aim, she ran at the imp: no speed along the jump ("too slow: -0.0 a tic for 12.4"), or off
its line and down. Now the route has her facing and her trigger from the sprint's start till
she's down (under a second; what can't wait has her legs anyway, the arbiter's). MAP06,
skill 2, 8 seeds, one life: jumps missed 12 to 1, sprints called off too slow 15 to 3, every
run out both ways. Score -0.0021 (s.e. 0.0043), exits 420 for 422; fight score -0.0016 (s.e.
0.0037): no worse, and no gain shown. MAP06 itself at skills 3 and 4 had 1 exit of 16 for 5,
where the five sets before had 3, 2, 3, 5, 5: taken as that level's spread, not looked into
further. Seen there and not the sprint's: a dodge mid-sprint carries her 200 units off the
run-up and the sprint is then called too slow; and out of ammo on nukage she goes to and fro
between "outmatched: keeping clear" and "out of reach: going on" till the floor has her.

## Loops on MAP06: keeping clear on a hurting floor; away to nowhere further

- **On a floor that hurts, keeping clear leaves her legs to the route** unless the monster's in
  the route's way (the rule `standing` and `stepping_for_shot` had; `outmatched_keep_clear`
  hadn't). Out of reach of her gun the route went on; in reach, keeping clear backed her off,
  out of reach again: to and fro at that edge on nukage for half a minute, 3 health a second,
  till the floor had her (MAP06 skill 3, seed 729663407: 917 "keeping clear" for 66, floor
  hits 65 for 30, no death). Score +0.0008 (s.e. 0.0015), exits 421 for 420; 1,038 the same.
- **Out of ammo, none known: away only to somewhere further from them** than where she stands
  by `retreat.ammo_gain` (128). The spots to flee to are a way off (128 at least), so where
  she stood was never one, and from each of two spots the best was the other: there and back
  every 1.2 s for five minutes, till the run was called stalled (the same seed, once the
  floor no longer killed her; the cycles catalogue's largest row is this, 2,675 s in one MAP06
  run). It stops the running; what she should do there, with no ammo, none known and them
  between her and the rest, isn't settled by it. Score +0.0029 (s.e. 0.0043), exits 423 for
  421; fight score +0.0026 (s.e. 0.0035).
- **Not kept: a missile a wall will stop isn't dodged.** MAP06's lift stands among seven
  pillars that stop most of what the imps throw, and `incoming` takes any missile whose line
  passes near her, with no look at what's between: she dodged fireballs about to burst on a
  pillar, off the lift's run-up each time. The rule (the missile's middle line at its own
  height, by `LineOfSight`, to where it's nearest her; not for one that steers) made that
  seed a full clear (90 of 90, out in 805 s, from stalled), and lost on the score: -0.0118
  (s.e. 0.0083), exits 407 for 423, 798 runs changed. Checked only to where it could first
  touch her (a wall just past her stops nothing): -0.0096 (s.e. 0.0079), exits 410. E3M6 and
  E4M3 at skill 3 lost a third each both times. Not traced why: dodging more than she need
  may be keeping her moving, as the slow legs did. The patch is out/score/stopped-missiles.patch.
  What was traced and still stands: a dodge mid run-up takes her off it, and the jump's lost.
- **Kept, the same rule for a jump's run-up only:** lining up a run-jump, sprinting for it or
  in the air off it, a missile a wall will stop before it could touch her isn't dodged
  (`Combat::jump_under_way`). All her other dodging is as it was. MAP06 skill 3, 8 seeds, one
  life: sprints called off too slow 10 of 89 to 6 of 126. Score +0.0006 (s.e. 0.0019), exits
  424 for 423; 1,066 runs the same.

## Instinct hers to turn off, or the stream's

Watched through Episode 1 (2026-11-15), she turned instinct off and stood waiting between her
actions, which the console could turn back on but not keep on.

- **The console says whether it's hers** (`evil_instinct`, on the Evil card with the push
  timings; hers by default). Not hers: instinct's on at once and stays on, `doom_policy` is
  registered again without the setting or its line of description, the briefing doesn't name
  it, and `instinct: off` sent anyway is refused ("not yours to turn off").
- **Taken out, not just refused:** a setting she can see and can't use costs her context and a
  wasted action each time she tries it.

## A late answer isn't a failure

The Episode 1 session (2026-11-15, the GPT stand-in): 113 what-next questions, 200 answers to
them. 30 first answers came after the question's default was taken (she had set `answers: 3`
herself; her answers took 2.2 s at the median), each was refused as "Too late", and 28 of
those were then sent three more times: a forced action that fails is sent again at once.

- **An answer to a question already closed succeeds,** saying it's closed and how, with
  nothing done. There's nothing for her to try again. One to a question that never was
  ("No question q7") still fails.
- **How long a question waits is the stream's, not hers** (the console's `answer_wait`, 8 s
  unless set; `doom_policy answers` is gone). She has no way to know how long her answers
  take, and nothing to gain by setting it: the stand-in set 3 s only because the field was
  there to fill.

## A barrel's shot goes where she faces when it leaves the gun

Seen on stream: clearing a barrel with the shotgun, her view swung well off it as she fired.
Traced on E1M2 and E1M7 (`EVIL_DEBUG_BARRELS`, which now prints where she faces each tic after
a tap): the pellets leave 3 tics after the trigger and a pistol's bullet 4 (S_SGUN1, S_PISTOL1),
going the way she faces then. Two faults: with a run off planned she turned to it the tic after
the trigger (a shotgun tap left 32 degrees off); and she pulled the trigger within 2 degrees
while still swinging, and went 2 to 4 past (a barrel 500 off is 1 degree to each side).

- **She faces the barrel till the shot's out** (`TAP_OUT`), her legs off at once all the same.
- **The trigger's pulled only with the turn all but done, and square on the barrel for how far
  it is** (`TAP_STILL`, `TAP_ON`).
- Shots 2 degrees or more off as they left: E1M2 16 of 48 to 0 of 44, E1M7 18 of 31 to 1 of 23
  (that one turned by something else, not traced). Score -0.0007 (s.e. 0.0037), exits 423 for
  424; 859 runs the same.

## An order's rush ends with the order; a new level's clock starts from nothing

Both from the Episode 1 session's log (2026-11-15).

- **Rush stuck on.** After a `doom_go ... rush: true`, everything she then did by instinct
  was "rushing" too: pressed on with mid-fight. The rush was cleared in the tic she stood
  waiting after an order, and since instinct takes up at once there's no such tic. Now going
  on by instinct clears it.
- **Last level's clock.** A level's tics start from nothing, and times kept from the level
  before were compared with them. The questions a minute stayed spent: after three asked
  late on E1M4 (to 704 s), nothing could be asked on E1M5 (508 s long) or for the first 12
  minutes of E1M6, and `doom_goal completionist` was refused 7 times of 8 with "answer
  what's asked first", no question open. The same held her being pushed when idle, asked
  what next at a step, and told moments aloud. Now a question from a later tic than this
  isn't counted, and the session's clocks start again when the clock goes back.

- **The check of every time kept across a level** (asked for after the above). Only a new
  level sets the clock back: a death or a restart carries the tic count on. The director's
  `new_map` now clears the rest of what it timed by the old level (getting off a hurting
  floor last tried, the calm since, a lock or a fast loss last told, what's due to be asked),
  and puts the questions asked lately back by the old level's length, so the three a minute
  count across the change. The session does the same for all its times in one place
  (`Session::clock`): each as long ago as it was. The stream column and the screens between
  levels keep their own clocks and were right. Played E1M1 to E1M3 with the mock: asked what
  next 14 s into E1M2 and 6 s into E1M3, each level's start told aloud.

## Walls not worth a press: a stair's risers, the rest of a run all alike

Watched (2026-11-15): a thorough search pressed at every step of a stair, and at each side of
things that look the same all round.

- **A step like the next isn't pressed.** An open two-sided line with one floor above the
  other, where another such line on to or off either floor shows the same face: a flight's
  riser. A press at it goes on through the opening over it (PTR_UseTraverse: no special, an
  opening, the use carries on; not even a grunt). A door standing open is no step (its floors
  are level) and is weighed as before.
- **One of a run.** One-sided walls she's mapped, joined end to end on the one floor, showing
  the same face at the same height (`run_of`): the best of them stands for the run
  (`Behind::Run`), and once one's been tried the rest are skipped. Two-sided walls were
  already one for each sector behind.
- **What it could miss, looked up in the maps:** a use special on only some lines of such a
  run. In Doom and Doom II there are four: two are switches (seen as switches, not searched
  for), and two are E4M5's start pit, six wall lines with a lift special for tag 3, which no
  sector has: dead, an editing leftover (the pit's lift is tag 2, worked by lines 329 and 335).
- **Measured** by `secret-survey` over 66 maps (E3M3 and E4M7 left out: the survey's exploring
  differed between the runs): walls pressed a map at thoroughness 1, 2, 3: 8, 36, 140 to 7,
  30, 100. Every line that reveals a secret offered at the same thoroughness as before.
  Score -0.0019 (s.e. 0.0029), exits 420 for 423; 912 runs the same, 32 better, 32 worse.

## The picture stuttered more the longer a level went: the director's listings

Watched (2026-11-15): the frame rate fell off as a level went on, plain by 15 minutes into
E1M6.

- **Traced** by timing each part of a watched tic over 20 game minutes of E1M6 on UV, directed
  through the mock. The engine was 0.05 ms a tic throughout and her own play 0.4 ms. The
  director was 2 ms a tic at the first minute and 5 to 8 ms from the fifth on, with single tics
  of 50 to 90 ms: all of it `Director::events` asking the explorer for every open objective
  (`open_objectives`: a safe listing and a risky one) every five tics, to see whether an exit,
  a switch or a door had newly become hers to go to. A listing is the explorer's whole
  reckoning of what she can do (most of it the sift of what would cut her off, `cut_off.rs`),
  and it costs more the more of the level she knows: 15 to 30 ms each on E1M6 well explored.
  Fourteen a second, in pairs, was a quarter of all real time and three to five frames lost
  seven times a second. No memory grew; the cost is the level known.
- **The rule:** that news is looked for once a second, the safe listing on the second and the
  risky one 15 tics into it, so no tic has two. What's newly in reach is told with the safe
  one (it goes by that listing's reach). The level-complete check, which listed twice every tic
  while everything was done with no exit to tell of, looks once a second too. And the watcher's
  own once-a-second listing (the marks on the map) isn't made for the stream's window, which
  draws no map, unless a console page is showing one.
- **Measured** on the same 20 minutes: tics over 16 ms (a frame at 60 Hz) from about 430 a
  minute to between 15 and 150; the director from 5 ms a tic to 1. She played the same run tic
  for tic, told the same doors and switches up to 0.7 s later. Undirected play doesn't go
  through any of this: parity, all 24 runs the same.
- **Left:** a single listing is still a late frame on a big level, two or three times a second,
  and her own choice of what to do next is one more when it comes. Making the listing itself
  cheaper (or taking it off the tic) is the next step if it still shows.

## TNT and Plutonia: the levels the planner couldn't finish (from 2026-11-15)

The sweep of TNT and Plutonia (2026-11-14; 512 runs a difficulty) left levels that stall on
every seed whatever the skill. Asked for a plan knowing the whole map, her planner had none for
five: TNT MAP02, MAP10, MAP18, MAP31 and Plutonia MAP17 (a mechanic her model lacks); it had
one for TNT MAP22, MAP26, MAP29 and Plutonia MAP04 (she fails to find or take the way). Not
yet asked: TNT MAP05 to MAP09, MAP19 to MAP21, Plutonia MAP12. The two WADs stay held out of
tuning: what's traced there goes in as a rule about the game, looked up in the engine, and is
measured on Doom and Doom II.

### TNT MAP18

- **The planner's start was a voodoo doll.** The map has four player 1 starts; the game sets a
  player down at each and the one played is the last (P_SpawnMapThing). The offline tools, and
  her own plan from the level's start (`completion.rs`), took the first: a doll in a closet,
  three polys of reach. `MapData::player_start` is the last. Maps with more than one: TNT
  MAP18 and MAP30, Plutonia MAP06 and MAP28; none in Doom or Doom II (parity the same).
- **Doors sunk in the floor.** The four doors out of the start yard are 32 units thick with
  their floor 30 below the ground on both sides. In the game she walks straight over: a box
  32 wide is always on one side's line or the other's, and stands at the higher floor
  (PIT_CheckLine takes a box only touching a line as clear of it, so she's in the slot only
  dead centre to the last fraction of a unit). Her model had a 30-unit step up out of each
  door. The trench rule (`fill_trenches`) covered this but for two things: it left out every
  sector a special line moves, doors among them, and it took an exact fit for room to stand.
  Now a door's floor is a slot like any other (a door moves its ceiling only; its effect is
  its ceiling over its sides' floor, `with_trenches`), and a place to stand needs half a unit
  to spare on some side (`SLOT_SPARE`).
- **What else it touches.** New slots in Doom and Doom II: doors at E1M5 (sector 122), E3M5
  (161, floor 48 below its sides: thought impassable before) and MAP19 (229); a 32-deep
  sunken switch niche at MAP12 (42); a 24-wide lava slot at MAP20 (39), left out before as the
  back of a tagged door's switch line. About twenty in TNT and Plutonia.
- **Measured:** TNT MAP18 on the easiest skill, 4 seeds: 4 exits (0 of 8 before, all stalled).
  The score: all 1,088 runs the same to the tic (0.482, 420 exits): none of the five is on a
  way she takes there.

### The planner asked again, every level (2026-11-15, after MAP18's two fixes)

`dump-nav` on all 64, set beside the sweep of 2026-11-14 (exits of 32 runs, four skills):

- **No way even in principle** (the first check, `progression`): TNT MAP08, MAP09, MAP10, MAP20
  (0 exits, 7 or 8 of 8 stalled on the easiest skill), MAP27 (2 exits), MAP30; Plutonia MAP07,
  MAP15, MAP17, MAP20, MAP29, MAP30, MAP31 (0 exits), MAP13 (6 exits). The two MAP30s end on
  the Icon of Sin, which is no exit line. MAP13 and TNT MAP27 she has got out of: the check
  says no where there is a way, so it's a pointer to a mechanic, not proof of one.
- **In reach in principle, the search finds no plan:** TNT MAP02, MAP05, MAP06, MAP16, MAP18,
  MAP21, MAP31, MAP32 (0 exits; most stall), MAP28 (10 exits, 8 of 8 on the easiest);
  Plutonia MAP09, MAP10, MAP12, MAP21, MAP22, MAP27 (0 exits); Plutonia MAP03 ran out its
  60 s. MAP18 and MAP28 she gets out of: the search's no is weaker still.
- **A plan found, and she stalls all the same** (easiest skill, 8 of 8): TNT MAP07, MAP19,
  MAP26, MAP29; half the runs on TNT MAP22.

### TNT MAP20

- **The donut (special 9) did nothing in her model.** The courtyard's pillar, with a plasma gun
  and a crowd on top, comes down by a donut switch on its sides, and the way on is over it.
  Modelled as EV_DoDonut has it (`MoverModel::donut`): each tagged sector is a pillar, the
  pool is what's across the pillar's lowest-numbered line, the model is the back sector of the
  pool's first line whose back isn't the pillar; pillar down and pool up to the model's floor
  (here both to 280, a ledge's height: the courtyard rises with her on it). Nothing where the
  game has nothing to go by (a one-sided first line: no donut, or the vanilla overrun). Donuts
  in the IWADs: E1M2, E2M2, TNT MAP20 (11 lines) and MAP21. Score: +0.0000, 1,074 of 1,088
  runs the same to the tic, none better or worse.
- **Open floor with no portal over it.** The outdoor floor before the exit (sector 200) was two
  islands in the mesh. A poly's edge on a BSP partition and its neighbours' on the same
  partition leaned 4 units apart after 760: the node builder runs a partition through its
  seg's rounded end, a little off the linedef's own line, and the poly that has that seg is
  cut again by the linedef's line, which takes a wedge off it beyond the linedef's end.
  `shared_segment` allows 3 units. Now two polys of one sector not joined otherwise are joined
  where edges with no line of the map along either overlap within 8 units and lean apart by no
  more than 1 in 32 (`leaning_segment`). 483 portals more over the four IWADs' 132 maps (117 on
  30 maps of Doom and Doom II), none fewer.
- **A door with no portal on its line.** The door out of the blue key's pit (line 1352) had
  none: the first stretch two polys were found to share was taken for their portal, and here
  that was 8 units of the door's sliver running on past its corner along the next wall, with no
  line on it. Two polys now join along the longest stretch any two of their edges share.
- **Measured:** MAP20 with no monsters exits (3,507 s exploring; 630 s with the level known
  beforehand); before, the blue key was got and the pit never left. With monsters on the
  easiest skill the four runs before the last fix ran out their hour; not run again yet. The
  score with both mesh rules: -0.0013 (s.e. 0.0032), exits 418 for 420, 959 of 1,088 runs the
  same to the tic, 23 better and 25 worse (the leaning join alone: +0.0003, s.e. 0.0027, 421).

### TNT MAP09 (traced, not fixed)

The blue key is on a crate 128 high whose only way on is a run through a window from the room
above (sector 361, over a sill 16 deep). The finder has that jump eleven ways; the check
throws every one out. Tried in the game she stops dead at the window: five barrels stand in a
row on the floor under it, and a thing blocks at any height here. So, most likely: the barrels
shot first, then the jump. Not confirmed (the barrels weren't got to blow in the drive
script). A rule for it: a jump only barrels block is kept, to be made once they're cleared.

### TNT MAP18 again: the plan search, and where a switch is pressed from

MAP18 was the one level of these the planner reached "in principle" and found no plan for,
with ten minutes and a hundred times its budget. Looked into as a question of the search
(orderings, depth, pruning), it came down to the level's model.

- **What the search was doing.** Its nodes went deep (most at 18 or 19 steps, 13 of them
  lasting changes), a fifth of them worlds not seen before, six duplicates thrown away for
  each one kept. Three in four of its seconds were paths for legs, most of those failing:
  legs to press the switches of a pillar from outside the ring of posts round it.
- **Where a switch can be pressed from** (`polys_in_use_range`) asked only for somewhere in
  the poly, in reach, clear of one-sided walls. Now the face of anything nothing moves that
  she can't get onto or off is a wall to it too (a fixed post). And where every such place
  has her box over the line of something that may move and is a wall to the box as the map
  starts (a post that comes down, a door shut beside the one pressed), the press from that
  poly waits on one of the places coming clear (`stand_gates`, read by both `activators_now`).
  A wall to the box: no opening over the line she fits, or a face over a step high with her
  more than a step below its top (`nav::box_over`).
- **The relaxed plan didn't name a fence.** Its way to a trigger went through a portal too
  narrow with the posts up, and asked for nothing, the two sides being the one sector. Now
  the gate lines in the box's way at such a portal at the start and not in the end have what
  moves their sectors asked for. And a press that waits on a place to stand isn't taken for
  one with nothing in its way.
- **Measured.** MAP18's relaxed plan from 12 lines to 27 (the four switches that lower the
  posts, the red key); a plan of 28 steps at the usual budget in 7 s, where there was none
  in 45,000 steps. Over the four IWADs' 132 maps the plans found go from 96 to 99 (TNT MAP18,
  Plutonia MAP09 and MAP22; none lost, 16 others' plans different). Its press of the pillar's
  west switch from by the fence, once the south posts are down, tried in the game: it works
  (from 44 units off, aimed just inside the line's end). Score -0.0005 (s.e. 0.0011), exits
  417 for 418; 1,057 of 1,088 runs the same to the tic, 6 better, 7 worse.
- **Tried and not kept.** A step that loses nothing taken at once, alone: twice in 10,000
  steps on MAP18, every walk to a switch there going over a one-way drop. Lifts and timed
  doors as ways to walk, crudely (as use-doors are): fewer steps to choose from, each twice
  as slow, no plan. Ranking by the relaxed plan's lasting changes still to make: a plan in
  4,515 steps, but one that pressed a switch through the fence with the posts up, which the
  game doesn't allow (tried: 68 units off, the reach 64). Refusing a press whose chosen spot
  has the box on a fixed wall: level on the score (+0.0010, s.e. 0.0008) but it refuses the
  press above that works; the places-to-stand rule replaces it. A depth cap: the sound bound
  is about 40 lasting steps and the deepest node had 19.
- **Not done.** The flood fill by which portals of a poly the box gets between (built for
  the plain fill, saved aside): right in the one case tried in the game, two more plans and
  two levels newly closed in principle not yet looked at, twice the fill's time. The legs
  kept by the heights their search asked for (the same 188 legs asked for 11,522 times on
  MAP18): exact by parity, next.
- **Legs kept by the heights their search asked for** (`Planner::leg`). A plan's worlds
  differ mostly somewhere else in the level, and the cache of legs went by the whole world: on
  MAP18 11,522 legs were asked for, 188 different ones, 723 found kept. A leg is now kept with
  the sectors its path search asked the heights of and what it was told; in a world that has
  those the same the search would go the same way step for step. Parity, all 24 runs the
  same; the same searches step for step and three times as fast where legs are most of the
  time (TNT MAP05's 2,000 steps 17.7 s to 5.0 s, Plutonia MAP03's 20,000 45 s to 13 s).

### TNT MAP05 and MAP32, Plutonia MAP03: plans the search had no way to

Each "in reach in principle, no plan". `EVIL_DEBUG_RELAXED` now says, when a search is over,
which of the relaxed plan's lines were never set off, and for each the first crossing on the
way to it (the way before it has done what it does) that the search's furthest node doesn't
make, with the heights on both sides (`relaxed_stops`). That named each cause.

- **TNT MAP05: a line walked over onto a floor she can't be on yet.** The blue key's lift is
  set off over the lines round a pedestal with 32 units of headroom, lowered by another line.
  The relaxed plan asked for nothing: a walk-over line's polys were its target whatever was
  beyond. Now, where from nowhere can she walk onto its far side as the level starts, it asks
  for what makes the room there. Score +0.0006 (s.e. 0.0006), exits 418 for 417; 1,082 of
  1,088 runs the same to the tic, 2 better, none worse.
- **The way to a switch, before the switch.** The relaxed plan's way to a trigger was the
  shortest with everything set off, which can run through the very wall it lowers. It's
  looked for first without the height the trigger's wanted for. (In the same change.)
- **TNT MAP32: a lift beside a walkway that's lowered first.** The way to the blue skull's
  switch: lower a walkway, step onto it, call the lift beside it, ride up. A lift's low stop
  was worked out once, from the floors round it as the map starts, where this one is already
  at its lowest. It now follows the floors round it when it's called (`LIFT` among
  `depends_on_heights`), as EV_DoPlat does. Score with the above +0.0016 (s.e. 0.0010), exits
  419 for 417; 1,053 the same to the tic, 16 better, 10 worse.
- **Plutonia MAP03: a route found and thrown away.** The search with the relaxed plan's
  lines found a route, a squeezed one; the search from all the lines, run after it for a
  better, ran out its 20,000 steps, and "no route" was the answer. The first one's kept when
  the second has none. Score with the two above +0.0010 (s.e. 0.0012), exits 418 for 417;
  1,052 the same to the tic, 16 better, 11 worse.
- **Together** the plans found over the 132 maps go from 99 to 103 (TNT MAP05 and MAP32,
  Plutonia MAP03 and MAP10), none lost.

### A squeeze, walked (2026-11-16)

A leg is "squeezed" where the path's fit, sweeping the box along straight lines with a quarter
unit to spare and room kept from corners, finds it over a wall and no way round, and the walls
aren't shut sectors only (that's no way at all). Squeezed legs wait behind clean ones, a plan
with one is a fallback, and the search goes on for a clean one. Whether she'd really get
through was a guess.

- **The stretch is walked** in the player simulator the jumps are checked with
  (`JumpLab::walks`): run, facing the far end, sliding along what's in the way, five tries
  begun up to 6 units to either side; given up when she stops getting nearer or the floor goes
  from under her by more than a step (off a ledge; stairs are walked). Four of five there:
  no squeeze. The sectors by the stretch at the roomiest heights the leg's world has them.
- **Against the game:** seven stretches of Plutonia MAP03 and Doom II MAP29 walked in the
  engine: six as the simulator had them; the seventh a stair the first rule failed for its
  floor changing at all, mended, and now one try in five (a squeeze still: cautious).
- **A clean route from the narrow search is the route.** The search from all the lines after
  it was for "the route as ever", and found the same or one no better by anything measured,
  in up to a minute. Now only where the narrow search has none, or a squeezed one.
- **Keys of bars beside a doorway** asked for by the relaxed plan, as the posts' switches are.
- **Measured.** Planning all 132 maps from 227 s to 156 s (Plutonia MAP03 12 s to 0.06 s, TNT
  MAP05 12 s to 0.1 s); plans found 103 either way (Doom II MAP06 gained, Plutonia MAP09
  lost till the change after this one). Score +0.0020 (s.e. 0.0035), exits 423 for 418; 872 of
  1,088 runs the same to the tic, 52 better, 40 worse.
- **Plutonia MAP09: down a lift for a key, and no line to call it back.** The relaxed plan
  names one line for each thing it needs, and the narrow search may set off only its lines:
  the lift called from its top was named, the line at its foot not, and the search was stuck
  in the pit with the key. The narrow search may now set off every line whose effect is the
  same as one of the relaxed plan's (the ranking's list as it was: with these in it Doom II
  MAP15's plan was lost). The stop report says where the way to each key stops, too.
- **Measured.** Plans over the 132 maps 103 to 105, none lost (Doom II MAP06, and MAP17 by a
  fallback route with a stretch that can't be walked: 104 honestly). Score, this and the
  walked squeezes together against the set before both: +0.0055 (s.e. 0.0020), exits 428 for
  418; 1,017 of 1,088 runs the same to the tic, 26 better, 13 worse.
- **A score run wants the machine to itself.** The walked squeezes' own run (+0.0020, s.e.
  0.0035, 52 better and 40 worse) was made with the planner's 132-map comparisons running
  beside it, and some seventy of its runs came out different for that alone: the next run,
  on a quieter machine, had them back as they were (the searches' clock backstops). The
  figure above is the one to go by.
- **A try begun on a ledge above.** Doom II MAP17's fallback route has a stretch from a floor
  at 96 into the face of rock steps 104 above it: no squeeze but a wall (walked in the game,
  she slides along its foot). The simulator had it two tries in five: the tries begun to one
  side had the box over the ledge's edge, and were set down on the ledge. Every try now
  begins on the floor the stretch does. That stretch none in five; the stretches checked
  against the game as they were; the score's 1,088 runs all the same to the tic.

### Plutonia MAP03: dead in 16 seconds, of the floor (2026-11-16)

Every seed, no kill: she starts on a platform in a slime moat, the way off a run-jump to a
ledge. A jump over where there's no getting back from isn't taken while anything safe is left
(`JumpOverNoWayBack`), so it was the risky pass's; and there the first choice with a way to
it was a wall to try down in the moat, 180 off, nearer than the jump's. The risky pass refused
a way only for costing more health than she had to spare, and the walk to that wall costs a
few hits. That there's no way off the floor wasn't counted.

- **The rule:** mortal, a drop onto a floor that hurts with no way off it she knows isn't
  taken, risky or not (`DropOntoHarm`, as the safe pass has it for any such drop). A jump
  over the same floor still is. The drop is left for when there's nothing else at all
  (`with_harm_allowed`).
- **Measured:** MAP03 on the easiest skill, 4 seeds: from dead at 16 s with no kill to 159,
  164, 187 and 1,329 s with 10, 11, 11 and 42 of 52 killed (no exits: the monsters now). Score
  +0.0031 (s.e. 0.0024), exits 433 for 428; 1,008 of 1,088 runs the same to the tic, 25 better,
  23 worse.

### E1M7: to and fro at a barrel for an hour (2026-11-16)

In the ring corridor a barrel stands in the walkway, the route past it, and three imps are in
sight 600 to 900 off that she has no shot at and that never hit her. With a target she steps
away from a barrel within 160 till she's 224 from it; the route walks her back; 28 tics a
turn, till the hour's up. In the baseline too: one Hurt Me Plenty run stalled at it, 2,003
steps away in 2,148 s.

The guard against it (two seconds of stepping away at most, then three of rest) counted from
when she last got clear, and she got clear in half a second each time. It was written for
never getting clear at all (pressed against a wall).

- **The rule:** the two seconds are a bout's, the coming back between counted: the clock
  starts afresh only after a rest's length with no stepping away. The rest is the route's
  time to take her past the barrel. No new number.
- **Not done:** giving up on what she can't hit doesn't end it (the barrel step and "shooting
  on the way" are decided before the nodes that count patience); and she holds the shotgun's
  fire at those imps with 200 bullets for the chaingun.
- **Measured:** the stalled run exits in 420 s (17 steps away), another on the level 586 to
  511 s. Score +0.0016 (s.e. 0.0031), exits 435 for 433; 928 of 1,088 runs the same to the
  tic, 24 better, 19 worse.

### TNT MAP32: a door she shut behind herself (2026-11-16)

Door 122 starts open, a trap line shuts it, a switch (line 872) opens it for good; and its
own line (824, pressed again and again, on the outer face only) she then pressed as a door to
try. A press on such a door standing open starts its open-wait-close afresh: it shuts after
the wait, with nothing to press on the far side. 97 of 256 sectors seen in the hour.

- **The rule:** a door opened by pressing it again and again, standing open as she last saw
  it, that she's learned another line opens to stay, isn't a door to try (`stands_open`); nor
  pressed if she finds it so on coming up to it (the switch pressed the moment before).
- **Tried wider, and dropped:** any pressed door standing open. A door pressed once that
  opens and stays can't be shut by a press, and the try is what takes her to it (E3M7's line
  835, over a floor that hurts: 4 exits lost there). Any repeatable door standing open, the
  other line learned or not (a monster come through it, mostly): 151 runs moved, -0.0035
  (s.e. 0.0036).
- **A timed door believed to rest shut, not as the map starts** (tried with it): not needed
  for MAP32, -0.0011 (s.e. 0.0012) alone. Not in.
- **Measured:** MAP32, no monsters: 184 of 256 sectors (no exit yet: stranded where the
  planner has no way either). Score -0.0007 (s.e. 0.0014), exits 434 for 435; 1,070 of 1,088
  runs the same to the tic, and all the rest E2M4's (line 516, the same make of door) but two.
  E2M4 on 8 fresh seeds a skill: 8 exits of 16 with it, 7 without.

### E3M7: keeping clear, for the hour, of what can't see her (2026-11-16)

Two cacodemons sit in alcoves behind windows that can't be walked through. Out of ammo to
kill one, she's outmatched, holds her fire (the ammo kept) and keeps clear of them: for fifty
minutes in one run, her health never moving. They throw nothing because the map's REJECT
table has their alcoves unable to see the two sectors she was in, though the window gives a
clear line (they're on the screen). Her own sight (`los.rs`) leaves the table out, so she has
them awake, in sight and outmatching her.

- **Not given the table.** A person sees them too, and learns by watching that they do
  nothing. So the rule goes by what she can watch.
- **The rule:** outmatched and holding her fire for the ammo, one that shoots, in her sight
  `engage.clear_give_up` (20 s) with nothing thrown at her and nothing hurting her, is left a
  while, as one she can't get at (`Effort::KeptClear`). Not one that only claws: kept clear
  of, it never gets its attack in.
- **Tried wider, and dropped:** the same whenever she keeps clear, shooting or not. 226 runs
  moved, 67 better and 85 worse: shooting, the fight gets somewhere by itself.
- **Measured:** score -0.0006 (s.e. 0.0042), exits 437 for 434; 893 of 1,088 runs the same,
  59 better, 70 worse; stalls 35 for 34. E3M7's scored runs: two stalls to exits, four exits
  to deaths (not traced). E3M7 on 8 fresh seeds a skill: 8 exits of 16 for 5, both runs stuck
  for the hour gone, none worse, one exit 2,079 s to 850 s.
- **A stall is worse than a death** (the maintainer, of this): on stream a death is a quick
  start again, a loop is painful to watch. Even on the score, taken for the stalls.

### E1M4: five seconds stood at a big door (2026-11-16)

Seen on stream: she runs up to a door, stands at it some five seconds, then goes round and
opens it. E1M4's middle room: two big doors with no line to press, opened by walk-over lines
(a ring that opens them for a while, four by the blue key that open them to stay). She
crosses the ring, the doors open and start to shut; she crosses the lines by the key while
they're still moving, which the game ignores (a line does nothing to a sector with a mover);
she takes them for open for good, and comes to one shut.

The five seconds are the follower's patience with a lift, which comes back by itself. A shut
door doesn't.

- **The rule:** a mover shut tight (no room in it) and standing still, nothing of hers on its
  way to it and no switch by her, is waited at half a second before the way's made again, not
  five.
- **Tried with it, and left out:** the step ended at once when that finds no way (the route's
  map is older than what she learned on the way, so it often doesn't: 17 s at the door in one
  run, half a second with this). About -0.002 on the score, within noise; the maintainer
  chose the plain half second.
- **Not fixed:** that she takes the line for having worked. What a crossing did is learned
  from what moved after it.
- **Measured:** score -0.0001 (s.e. 0.0014), exits 437 and stalls 35 as before; 1,051 of
  1,088 runs the same to the tic, 10 better, 6 worse.

### A jump's flight isn't judged as a walk (2026-11-16)

The path's fit took the stretch from a jump's takeoff to its landing for a walk: waypoints
added round the corners by it (the pillar jumped off), and what the box passes over (the far
wall of the gap, the lip landed on) counted as a squeeze. Doom II MAP17's plan took a minute
for it, the search trying ways round squeezes that weren't; and any rule harder on squeezes
closed real jumps (MAP22's hop to the red skull).

- **The rule:** from a takeoff the stretch is the flight: no corners rounded, nothing fitted,
  the landing kept as a waypoint. The jump was tried as it is (`JumpLab`).
- **Measured:** MAP17 planned in 0.2 s for 61 s, MAP22 in 0.7 s for 1.4 s; six other maps'
  plans as they were. Score -0.0006 (s.e. 0.0014), exits 436 for 437, stalls 38 for 35 (MAP14
  to and fro between dead and stalled, as in every run of the day; E4M1 one exit to a stall,
  its route parting from the old one at tic 790 by five units of path); 1,037 of 1,088 runs
  the same, 15 better, 10 worse.

### Glides aren't ways (TNT MAP32, 2026-11-16)

TNT MAP32's ring walkway had her walking at things for minutes till she'd nowhere left to
go: a fence of skull posts risen across it (16 wide, the gaps 16 and 24; line 1279 lowers
them), and the moat outside it where it narrows to exactly 32 between the walkway's edge
and the outer wall. That last a player gets through only on the exact unit: a speedrunner's
glide (the maintainer's word for it), never asked of anyone, and not hers; walked at in the
game she stops at its mouth.

All three are pinches inside one long piece of the mesh, where no crossing's width is
checked: only the whole path's fit sees them, and it had a pinch for a dead end only when
what pinched was a shut door. A block or a ledge's face, more than a step above her with
headroom over it, was "a tight spot that may do".

- **The rule:** a stretch pinched between faces more than a step above the floor she's on
  (and walls), with nowhere up to two boxes' width to either side that the box clears them,
  is no way (`pinched`, `shut_in` with the floor). Not by the path's end (a press at the foot
  of a lift), nor a path along a ledge's foot (room on its other side). The first try, any
  such face in the box's way, closed both of those, and the planner's way to MAP32's first
  lift with them.
- **What it shows up:** MAP32's plan was by a glide, and now there's none (followed in the
  game, the old one stuck at the red skull's barrier anyway). Seven other maps' plans as
  they were, MAP17's and MAP22's with their jumps (the entry above).
- **Measured:** on top of the flight change, score -0.0003 (s.e. 0.0016), exits 435 for 436,
  stalls 34 for 38; 1,064 of 1,088 runs the same, 9 better, 11 worse. Against the baseline
  before both: -0.0008 (s.e. 0.0022), exits 435 for 437, stalls 34 for 35.
- **MAP32 still:** no exit. The walkway's east tip strands her (her middle over the moat,
  stood on the walkway); and the way from the red skull back up to the walkway isn't found.

- **Judged from the floor she'd be on there** (the same day). From the walkway's north side
  her way to the lift's far switch ran to the walkway's east tip, off its corner and on up
  the 32-wide moat: one stretch, begun on the walkway, and judged from the walkway's floor,
  where the moat's sides aren't faces above her. She stood at the tip till stranded, twice a
  run. A pinch is now judged from the floor under her middle at the pinched spot, when
  that's below where the stretch began. MAP32, monsters in and no dying: no stranding at the
  tip, 254 of 256 sectors for 233, at the exit chamber by 1,730 s (the lift behind the posts
  was at 2,867 s). The score's 1,088 runs all the same to the tic.

### TNT MAP32's exit: the lift she never saw move (2026-11-16)

The exit is behind two things in a row: a door opened for four seconds by a switch on a shelf
a teleporter away, and right behind it a lift lowered by a switch by another teleporter. She
learned the door. The lift's switch she pressed again and again and never learned, the lift
being behind the shut door whenever she went to look: 1,300 off from its switch, out of
earshot as it starts down; and by the teleporter she's beside it in under a second, as it
waits at the bottom.

- **The rule:** a lift's line set off within `LIFT_BACK_TICS` (six seconds), and a mover of
  its heard to start: the lift on its way back up, and the line's doing, as a clank within
  8 tics of a press already was (`heard_move`). She's told which sector a sound is from, if
  she knows the sector, as before; nothing new of the map is given her.
- **Measured:** score +0.0000 (s.e. 0.0015), exits 435 as before, stalls 32 for 34; 1,053 of
  1,088 runs the same, 4 better, 8 worse.
- **Back and forth between the two** (the same day). Knowing both, she still didn't get in:
  come to the door and found it shut, the way back on track was made by the heights as they
  were, the lift she'd just lowered down in them. Off to open the door, back, and the lift
  had gone up; off to lower the lift, back, and the door had shut; and round again.
  - **The rule:** in the follower's way back on track (`replan`), where the new way comes to
    something of hers that's timed after its time is up, it's planned again with that at
    rest: the line that works it is then a step too, in its place.
  - **Tried in the explorer's own planning, and dropped:** any route that comes late to a
    timed door open just now, planned with the door at rest. It fires all the time on a level
    of doors: with every narrowing tried (lifts left out, her own doors left out, taken only
    with a line more to set off) -0.003 to -0.005, Doom II MAP19 three exits to stalls.
  - **Measured:** TNT MAP32 on the easiest skill with no dying: out in 2,055 s, 71 of 71
    killed, never stranded (it ran out the hour). The score's 1,088 runs all the same to the
    tic.

### TNT MAP05: the pedestal there's room on only for three seconds (2026-11-16)

The blue card is on a lift that comes down when the edge of a pedestal in the middle of the
room is walked over. The pedestal has 32 of headroom, and can be stepped onto only while a
switch has it down, three seconds. She found the switch and learned what it lowers (heard);
pressed the pedestal's edges as walls; and ran out of things to do, stranded three times in
the hour.

Lines to step over and see (`thresholds`) are for places in her reach she hasn't stood in:
the pedestal was in her reach, by the switch, but a place to step to had to be one the box
stands in as she last saw things, and she last saw no room on it.

- **The rule:** a place to step to is one the box stands in as last seen, or as a line she
  knows the effect of leaves that sector (`standable_moved`). The planner has the rest: the
  switch, onto the pedestal in its window, the lift down, the card.
- **Measured:** TNT MAP05 on the easiest skill with no dying: out in 1,402 s (it ran out the
  hour). Score +0.0007 (s.e. 0.0018), exits 436 for 435, stalls 32 as before; 1,046 of 1,088
  runs the same, 7 better, 3 worse.

### Testing whether a level can be got through at all: `--charmed` (2026-11-16)

On TNT and Plutonia she mostly dies of the monsters before the way through is ever tried, so
"can she find the way" was being tested with no monsters, or with them and nothing able to
hurt her. Both mislead. With no monsters the fight's side is off, and no barrel is ever
cleared (TNT MAP32: stood against one by the red skull). With nothing able to hurt her she
isn't mortal, and her cautions are off: she walks into pits there's no way out of, and takes
a floor she's stood on unhurt for one that doesn't hurt (Plutonia MAP09's slime).

- **`--charmed`:** she plays as one who can die (every caution on, what a floor does taken
  from the map since standing on it tells her nothing), and nothing hurts her. With `--fight
  1` it's the test for the way through, the fight aside. Not for the score; no play of any
  other run is changed by it.

### Plutonia MAP09: the hop onto the switch's pillar (2026-11-16)

The last switch stands on a pedestal on a pillar, a 64 gap of slime from the rim and 48
below it. Run straight at the pedestal she comes down on the pillar and is brought up
against it, at the switch: in the game, at every speed tried. The jump check had no such
jump. The candidate along that line was a jump to the pedestal's top; flown, it stopped on
the pillar short of it, so it failed; and the candidates meant for the pillar came in aslant
and slid off beside the pedestal. All she had was a strafing run onto the pedestal, three
in ten, taken once there was nothing else: into the slime.

- **The rule:** a jump that comes to rest short of where it was meant to land (not past it,
  along the flight; within her width of it), on a floor of its own more than a step above
  the lowest it crossed, is a jump to there (`Mode::Short`, verdict "short@x,y"; tried from
  shorter run-ups too). A fall into the gap itself isn't: that floor is the lowest crossed.
- **Tried first without "not past it":** a Doom II MAP19 jump that ended 20 beyond its
  landing came in, and two exits there went (-0.0014).
- **Measured:** score -0.0000, exits 436 as before, stalls 30 for 32; 1,083 of 1,088 runs
  the same, 1 better, 2 worse. Plutonia MAP09 needs two more things to get out (below).

### A floor still on its way to where the route has it (2026-11-16)

Plutonia MAP09's last switch raises the bridge to the exit 200 at the slow speed, nearly six
seconds. Her heights have what a line she knows does as done, once it's set off: the next
route, made a moment after the press, has the bridge up, and she walked off the rim onto it
half raised. Every way to the exit failed at once ("unexpected drop"), and she went on to
other things below.

The follower waited at an edge for a floor only when the route's own step had set it going,
or for a lift on its way round.

- **The rule:** at a drop the route didn't have, where one of the two floors is moving this
  tic and the route's heights have the crossing level, she waits at the edge while it moves,
  whoever set it going; `MOVING_WAIT` (15 s) at most, for what goes up and down for good.
- **Measured:** score +0.0030 (s.e. 0.0025), exits 440 for 436, deaths 614 for 622, stalls
  34 for 30; 976 of 1,088 runs the same, 27 better, 13 worse. The stalls: five deaths and
  an exit to stalls, two the other way. One traced (Doom II MAP29): not held by the wait.
  She lives past where she died at 169 s, and ends on a safe patch in a floor that hurts
  with nothing she'll risk, saying "nothing left to do" till the hour's up: with a life to
  spare that's the level begun again in a minute, not a loop. How many of the score's stalls
  are that kind isn't counted yet.

### Not marooned above a floor that hurts (Plutonia MAP09, 2026-11-16)

Before ever trying the last switch she had jumped down onto the low spans and islands of the
slime pit ("walk across sector 133", risky): ground that doesn't hurt, with no way back up
and no way off but into the slime, which has none either. A drop into the pit itself was
refused (`DropOntoHarm`); a jump to a perch over it wasn't, the perch not being the floor
that hurts.

- **The rule:** mortal, a way that ends by a jump or a drop on ground she can't come back
  from, off which everything she could get to is that ground or floor that hurts, is refused
  as the drop into the pit is (`marooned`). What she could get to is reckoned as `dead_end`
  reckons it: with the doors she can open, the lines she knows, and the key in hand if it's
  a key she's going for. Not when there's a switch, a door or a teleporter to try there, nor
  with ground there she hasn't seen into, nor a teleporter to be got to.
- **How it got there.** First with a second half, the way planned again keeping out of a
  dead-end floor that hurts: -0.0050, dropped. Alone, reckoned by walking only: -0.0042 (E4M4's
  chaingun on its island with the level going on beyond; a teleporter pad in the slime).
  With unseen ground and teleporters let through: -0.0012, four more stalls (Doom II MAP21's
  red skull on its island, the door it opens the way on).
- **Measured:** score -0.0001 (s.e. 0.0001), exits 440 as before, stalls 35 for 34 (an E4M6
  death to a stall); 1,085 of 1,088 runs the same. Plutonia MAP09 with `--charmed` on the
  easiest skill: out on two seeds of three (941 s and 901 s, all but one killed); it ran out
  the hour on all three. The third isn't traced.

- **A teleporter's hop counted as a walk** (TNT MAP32's exit again, with `--charmed`). At the
  exit with both switches learned, she had no plan in: `lapses` reckons whether a timed door
  will still be open when a later leg gets to it, by the walk between the middles of the
  mesh's pieces on the way; and a teleporter's two ends can be the level apart. A way by two
  teleporters never made its four seconds. Pieces that don't join now count for no walk.
  MAP32 with `--charmed`, easiest skill: out in 1,714 s and 1,610 s on two seeds (it ran out
  the hour). The score's 1,088 runs all the same to the tic.

### Plutonia MAP22: wedged for forty minutes and not knowing it (2026-11-16)

A slime fall: three steps down, 32 wide each, the roof stepping down with them. She jumped
up onto the lowest from below (a line to step over, risky), ran to the top, and couldn't
come back down: on the middle step, her own width, she's held up by the lip of the one
above, and at that height doesn't fit under the next roof. The exact middle unit clears
both (stood there in the game she walks out): a glide again.

She was "stuck" there 1,159 times in forty minutes. Being wedged ("stuck here whatever she
sets out for: stranded", the level begun again) was counted only from objectives that
failed stuck. Hers were each given up after their twenty seconds as taking too long, the
follower stuck all the while and never failing.

- **The rule:** an objective timed out with the follower stuck counts towards being wedged
  as one failed stuck does (`stuck_again`); and wedged, the spot and the mesh's pieces next
  to it are a trap for the go after.
- **Measured:** the score's 1,088 runs all the same to the tic. MAP22 with `--charmed`:
  stranded and begun again within a minute or so of wedging, 1,478 of 1,709 lines seen for
  1,305; no exit yet (wedged there again the life after in spite of the mark, and at a
  second spot, (-1720, -136): next).

- **A switch 0.6 out of her reach, and well inside the game's** (MAP22, the same day). The
  plan's next switch (line 1052) is behind a grid of bars a foot off the floor. They stopped
  her 56.6 from it; the press works from there, over the bars (in the game: its sector moved).
  Stuck on the way to a press she presses from where she is, if within `PRESS_RANGE`, 56, how
  near she'd choose to stand: she walked at the bars 54 times. Stuck, it's now `PRESS_STUCK`,
  62: the game's own reach, 64, less a little. Pressed at the first try, 498 s. Score -0.0002
  (s.e. 0.0001), exits and stalls as before; 1,086 of 1,088 runs the same.
  - **Tried and dropped:** the spot to press from having to be one she can stand at (presses
    that work from within reach were lost, and she was at the walls 447 times); a spot chosen
    among the bars (the same).
- **Wedged again the life after.** The trap's mark keeps risky options whose goal is in it
  out; a wall to try at the fall's foot, not risky and its goal outside, went by the same
  jump onto the fall. The jumps that land where she's been wedged are barred for the level
  (`wedge_jumps`). Wedged there once in the hour for three times. The score's 1,088 runs
  all the same to the tic.
- **MAP22 traced** (2026-11-16, with the maintainer's video and wiki checks). The level's
  order is blue card, yellow card, the yellow switch (line 238, in a hall behind blue
  doors) that lifts the yellow bars, switch 1052 behind them, its teleporter to the red
  card. Line 1052 can be pressed through the bars (posts with gaps, 24 clear of the floor: a
  press isn't stopped by a line with any opening), a skip the wiki knows; taken without the
  blue card it leaves her in the hall for good. Both cards lie in pits floored with nukage
  that doesn't hurt, which she takes by its look till she's stood in it.
- **A floor that sank once and was raised again stays up.** The yellow card is over a
  bridge that a once-only line on its edge lowers (lines 1509, 1521) and a switch raises
  again (1638). Raised, she ran off it into the pit every time: the follower took any floor
  with such a line for sinking still (`Navigator::sinking_sectors`, from the map alone) and
  ran for "the nearest floor that stays put".
  - **The rule:** a floor sinks under her only while a walk-over line that lowers it is
    still to go off this life (`View::sinking`, from the once-only walk-over lines she has
    set off, `spent_walks`).
  - **Measured:** the score's 1,088 runs all the same to the tic. MAP22 with `--charmed`:
    the yellow card picked up (never before).
- **A trap is a trap for every option, and for the keys she held.** Started again from the
  hall with nothing left to do, she had it marked a trap and went back each life: the mark
  kept only risky options out, and the red card (not risky, held back as "after what it
  cuts off") was taken as before.
  - **The rule:** nothing whose goal is in a trap is offered, risky or not (`ranked`); and
    the trap is kept with the keys she held (`Note::Trap`), holding only while she has no
    key she lacked then: the door it opens may be the way out. Wedged is wedged whatever
    she holds.
  - **Measured:** the score's 1,088 runs all the same to the tic (one life: no trap is ever
    come back to). MAP22 with `--charmed`, two hours allowed: exits, at 6,796 s on the sixth
    life, never before.
  - **Left:** three lives go on learning the trap (its mark is where she could get to when
    she gave up, which leaves out the red card's own ledge a drop above); the blue card
    isn't offered till 6,100 s; and line 1052 pressed through the bars is the skip that
    makes the trap: next.
- **MAP22's skip left to the trap's mark** (the maintainer's call, 2026-11-16). Three rules to
  keep her from pressing line 1052 past the bars were tried and dropped, each scored on the
  1,088 runs (baseline 0.493, 440 exits, 35 stalls):
  - *a press waits for what moves that's between her and the line* (the walls of moving
    sectors her box would meet on the straight way up to it): 0.488, 432 exits. It took a
    door at the foot of a lift's shaft for barred (Doom II MAP26's line 279).
  - *a press is stood for before the line's face, not off its end* (the maintainer's
    idea: the skip is pressed slantwise from the corner outside the bars, 40 past the
    switch's end): 0.488, 433 exits, 39 stalls; 111 runs moved, Doom II MAP19 four exits to
    stalls, E3M6 three lost. Not traced. It trims the press places of 297 lines on the 68
    maps and leaves one with none (a corner facet of a round lift, MAP14's line 537).
  - *an untried switch needs a place to stand for it* (`stand_gates`, which the planner
    keeps for the switches she knows and `untried` doesn't): a further -0.005, four exits
    each on Doom II MAP05 and MAP26. Not traced.
  With `--charmed` either of the last two has MAP22 done in the level's order with no life
  lost to the hall; as committed, she takes the skip once, is stranded, and starts again.
- **Lines that work one thing and join end to end are one thing to try** (the maintainer's
  model, 2026-11-16). The facets of a round lift's face and a wide door drawn in pieces were
  each a switch or door to try till one worked (a press that works settles all that work
  the same: `tried_key`). On the 68 maps, 2,193 lines to press are 1,779 such things; 295 of
  the 414 fewer are lifts' facets, E4M8 113 of them, Doom II MAP14 58.
  - **The rule** (`untried`): one to try for each lot (the same `tried_key`, joined by their
    ends through any line of the map that works it too), pressed from the nearest place any
    of its lines can be pressed from; a line of it tried lately and given up is passed over
    for the next nearest.
  - **Measured:** 0.492 for 0.493 (-0.0005, s.e. 0.0024), exits 439 for 440, stalls 38 for 35;
    58 runs moved, 17 better, 19 worse. Kept as the right model of a switch, not for the
    score. MAP14, which it was hoped to mend, is 2 exits and 8 stalls of 16 for 3 and 7: its
    stalls are something else, to be traced.
- **Doom II MAP14's stalls traced: out of ammo and run back for ever** (2026-11-16). Eight of
  its sixteen runs stalled. One is the level's own trap: a run-jump to the ledge by the
  courtyard and a drop behind the red bars without the red skull, nothing to do there (with
  lives she starts again, keeps out till she has the key, and exits: the trap's mark). The
  other seven are one loop. Out of rounds by 260 to 520 s, what's left to see is east past two
  chaingunners; she sets out, the fight has her "out of ammo, none known: away from them",
  back west, and the same errand is chosen again: every 17 s, 29 to 176 flights a run.
  - **The rule** (`for_ammo`, `retreat.away_give_up`, 60 s): fled so that long with no round
    gained and nothing new seen, no more of it till there is: on with her way, past them.
  - **Measured:** 0.495 for 0.492 (+0.0031, s.e. 0.0015); exits 443 for 439, stalls 30 for 38,
    deaths 615 for 611; 13 runs better, 5 worse. MAP14: six stalls gone, three to exits.
  - **Tried and not kept:** only a round gained setting the clock back, and given up for
    good till then (the maintainer's worry, and a fair one: new ground is seen by going on,
    and with it setting the clock back she goes on a little and is run back again, 159
    flights for 129 on one run). The flights are a tenth as many and she's through the
    same ground in half the time, but 0.495 (+0.0029, s.e. 0.0028), exits 441, stalls 32: two
    runs of E3M7 died-to-stalled, and one of MAP14's left stuck for the hour at other
    things (a shotgun with no way to it, walls). Not traced.
  - **Not built:** the flight weighed against what's fled from (the fight's own threat
    number for them where they stand, so a far chaingunner is gone past sooner than a near
    one). If the deaths after giving up turn out to be at the moment she does.
- **The score: stranded isn't out** (the maintainer's change, 2026-11-16). A run on its one
  life that ended with her stranded (nothing left to do and no way on: a trap she couldn't
  know of, which she marks and keeps out of the next go) was counted with the loops as
  stalled, the worst there is; on stream it's the level again within the minute. Now such
  a run begins the level again, up to three times (`--stranded N`, run-map's and the
  score's; 0 as before), knowing what she learned; dying still ends it, and 300 s with
  nothing new is still a stall.
  - **The same play scored both ways** (commit c5baf30, the same 1,088 runs): 0.495 either
    way (-0.0005, s.e. 0.0014); exits 446 for 443, deaths 628 for 615, stalls 13 for 30, and
    one run out of its hour (E2M4, stranded three times). Of the 30 stalls 17 were her
    stranded: 3 of those now exit, 13 die on a later go, 1 runs out the hour. The 13 left
    are loops.
  - Sets from before this aren't comparable on stalls or deaths; the baseline was made
    again.
- **E2M4 traced: too hurt for the one way on** (2026-11-16; the run stranded three times). None
  of the level's known traps. Both keys by 229 s and the yellow door opened from its front;
  then 1,030 of 1,117 lines seen and no exit, the way on being a teleporter pad in the
  blood before the exit room. That was turned down from the first as a drop onto a floor
  that hurts with no way off it she knows. Her health: 100 to 3 in nine seconds at 734 s (the
  closets by the red lift opening round her, two cacodemons and imps); back to 73; then 73
  to 12 over twelve minutes on floors that hurt, trying walls and stepping over lines with
  nothing safe left; at 12 nothing was to be afforded, the pad least of all.
  - **A teleporter she has seen is the way off the floor it's on** (the maintainer's
    point): a drop onto a floor that hurts isn't turned down as a dead end when it's a
    teleporter there she's going for (`refusal`, both passes). What it costs is weighed as
    any way's is. That run: the pad taken at 1,280 s, out at 1,369 s, never stranded.
    Score 0.496 for 0.495 (+0.0014, s.e. 0.0022); exits 448 for 446; stalled or out of the
    hour 14 for 14; 62 runs moved, 24 better, 22 worse. Doom II MAP24 (far pads in the
    nukage, the maintainer's worry): its 16 runs the same to the tic; MAP29: 4 of 16 moved,
    kills 314 for 250, all still deaths. MAP25: two more stalls, which are three more runs
    living through its first half minute (11 of 16 died in it before, 8 now): every run
    that does stalls, before and after. To be traced.
  - **Too hurt is no trap.** Started again from there, where she could get to (444 pieces
    of the mesh, most of the level) was marked a trap, and with the mark now holding for
    every option she was stranded again within minutes, three times. No mark with a way on
    turned down for want of health in the minute before (not only on the tic she gives up:
    by then what's turned down has been given up a while and isn't asked about). The level
    again is the health for it. Without the teleporter's rule, that run: one stranding for
    three, 55 kills for 27, still out of its hour. With it the score's runs are as the
    teleporter rule's but one (-0.0001).
- **Doom II MAP25 traced** (2026-11-16): dead in its first half minute in 11 runs of 16 (out
  of the start room into the open, shotgun guys, chaingunners and a revenant on her at
  once, on blood that hurts: the fight's, not traced further), and every run that lived
  stalled. The megasphere is at the end of a row of pillars in the blood room, a run-jump
  from each to the next, and the middle pillar has five barrels on it. Hurt, she jumped for
  it; the middle barrel stopped her short of where the jump ends, in the slot between two
  of them, where every way on is a straight line into a barrel and none can be shot from
  so near: "a barrel in the way, nowhere to shoot it from", once a second till the clock
  ran out.
  - **No jump onto barrels** (`barrels_on_landing`): a way is turned down when a run-jump of
    it comes over its landing's floor through a barrel she has seen and that still stands,
    whatever it's for, risky or not.
  - **A shot at a barrel within the gun's own aim up and down** (`shot_reaches`): 100 in
    160 of level, P_AimLineAttack's slopes; steeper, the game aims nothing for her. (A
    barrel on a pillar, from its foot.)
  - **Measured:** 0.497 for 0.496 (+0.0005, s.e. 0.0008); exits 449 for 448, stalls 10 for
    14; 1,080 runs the same to the tic. MAP25: one stall left of five, one exit, its first.
  - TNT MAP09, which the rule was first thought of for, is no different with it (charmed:
    wedged on barrels 76 times either way): another mechanism, not traced.
- **Barrels cleared from where she stands, and only when it's safe** (the maintainer's two
  points, 2026-11-16). The errand went to the place she could get to nearest the barrel,
  within 256, the follower finding where to shoot from once it began. By MAP25's pillar
  that place is down in the blood, a drop she won't take, so they were never shot, a ledge
  in plain view of them across the room. And it was offered whenever nothing was in sight:
  begun on the tic a fight's monsters went round a corner, given up seconds later with them
  back, begun again for the next barrel of the same pillar.
  - **From where she stands** (`barrels`): the follower looks over everywhere within the
    gun's aim of the barrel (1,024, as far as the game aims a shot) for the nearest place
    to her the shot reaches from, out of the blasts; a standing shot is as good from far
    as near. `barrels.spot` is gone. Not begun from a floor that hurts.
  - **Only with nothing about her**, in sight or seen awake a while ago
    (`Assessment::about`). Keeping clear of barrels in a fight, and shooting one by a
    monster, are the fight's as before.
  - **Measured**, on top of the rule above: 0.500 for 0.497; exits 452 for 449, stalls 11
    for 10, deaths 625 for 629. Against the baseline before both: +0.0032 (s.e. 0.0044),
    288 runs moved, 49 better, 38 worse: nearly every level with barrels plays otherwise.
    MAP25: 4 exits and 1 stall of 16 for none and 5; its kills 546 for 330. Lost: three
    exits each on MAP26 (HMP) and E1M6 (UV), two deaths to stalls on E4M5; MAP23 (the
    maintainer's worry) still 16 deaths, its kills 285 for 348. Not traced.
  - **The first go at "from where she stands", without the second rule,** was worse on
    MAP25 than the jump rule alone (three seeds dead at 260 to 520 s for 550 to 840 s): nine
    to eighteen errands a run in the blood room, each given up as "no getting to
    somewhere to shoot it from".
  - **Left:** the follower choosing a place to shoot from that she can't then walk to; and
    a chain of barrels as one errand, not one for each.
- **Stepping for a shot isn't for ever** (2026-11-17; Doom II MAP14's east yard, found once the
  lift's rule below had her that far: eight stalls of sixteen). A shotgun guy or an
  arachnotron in sight and no shot at it: "stepping for a shot"; the step brings a shot,
  the move is then "shooting on the way", which gives the route her legs, and the route
  walks her back out of the shot before it's taken. Some 750 times each way in one run, a
  plasma rifle and 52 cells in hand, "0 rounds spent". Her patience for no shot didn't run:
  its clock is set back whenever she could hit it, and each step got her that.
  - **The rule** (`stepping_for_shot`, `Effort::SteppedFor`): stepped for without a break
    (no more than two seconds between steps) for `engage.close_give_up` with it none the
    worse (its health the measure), it's left a while as one she can't get at, and not
    stepped for meanwhile.
  - **Measured:** 0.500 either way (+0.0000, s.e. 0.0008); exits 452, stalls 10 for 11; 1,077
    runs the same to the tic.
  - **Three goes before it**, each scored: (1) the clock from her first step, any gap
    under twenty seconds: 0.491, 14 exits fewer; (2) without a break: 0.494, 9 fewer. Both
    had stepping pass over every monster given up on, for whatever reason, and that was
    the harm, traced on a run of E4M8: a demon left earlier as out of reach wasn't stepped
    for when it came for her. 281 runs moved by it. (3) a mark of stepping's own that no
    other move reads: MAP14 back to five stalls, the other moves going on with the same
    dance. Kept: both, the general mark set and stepping reading only its own.
- **A lot of joined lines is pressed at any of its lines** (2026-11-17; the union the
  maintainer asked for, which the first go only made over the lines she'd made out). On
  MAP14 she made out one facet of the moat's round lift, the one at the end of its arc by
  the wall (line 375), and pressed from before it nothing goes off (by hand: nothing; the
  facet beside it lowers the lift at once). Eighteen presses in one run, the lift never
  learned; and with no lift known the moat is a fall for good, so every jump across it was
  turned down, the red skull's and the plasma rifle's among them (89 times in twenty
  minutes), and she ran dry on what's this side of it.
  - **The rule** (`untried`): one facet of a face seen, the face is seen: the lot is pressed
    at whichever of its lines (joined to one she's made out, made out or not) has been
    given up the fewest times, the nearest of those. Not "given up lately": that half
    minute was over each time she came back, and the dead facet is the nearest.
  - **Measured**, with the rule above: 0.500 (+0.0003, s.e. 0.0023); exits 451 for 452, stalls
    10 for 11; 57 runs moved, 19 better, 20 worse. MAP14: 7 exits, 8 deaths, 1 stall of 16
    for 6, 9, 1. Without the rule above: MAP14 1 exit and 8 stalls, 0.492.
  - **Tried and dropped:** a lift she knows and can call from below counted as a way back
    from a fall short of a jump (`falls_for_good` by `saturate`): 0.491, ten exits fewer,
    four of them Doom II MAP05's; only over floor that doesn't hurt: the same. Not traced.
  - The maintainer's other notes from watching MAP14, not acted on: shells spent on lost
    souls and on far-off monsters; and every wall tried, the level's textures being out
    of line everywhere, which is what she takes for a clue.
- **A run of walls that match is tried once** (the maintainer's rule, 2026-11-17). She pressed
  every wall round a room, every side of a platform, every riser of a stair seen from
  below: each a wall with signs of its own. His first thought was the faces out of line,
  everywhere on Doom II MAP14; measured, a face shifted beside another shifted never
  occurs as a sign on the 68 maps (a plain wall's shift is hardly gone by as it is), and a
  rule for it changed no wall's weighing. What MAP14's thirty best walls show is "not a
  wall on the automap, a platform's side, somewhere seen behind": a third of them the
  sides and risers of small stairs, a quarter rows of like blocks.
  - **The rule** (`walls_weighed`): walls joined end to end on one floor's side, each showing
    the very signs of the next, are a run; one of it pressed, the rest are tried. (`run_of`
    was that already for one-sided walls of one face in line.) A wide secret door drawn as
    two lines is a run and still pressed once.
  - **The survey** (she explores each map till there's nothing left): walls pressed 513 for
    584 on Doom, 453 for 644 on Doom II (MAP14 56 for 77); secrets found 178 for 180 of 276
    on the 63 maps in both, four maps one up, three one down, MAP17 three down behind
    doors and switches the rule doesn't touch; out of things to do 9% sooner.
  - **The score, on two sets of seeds.** The first: 0.494 for 0.500 (-0.0057, s.e. 0.0058);
    exits 441 for 451, stalls 6 for 10; 427 runs moved, 94 better, 91 worse. One lost exit
    traced (E3M5): five walls pressed round the start room for two, six seconds sooner
    into the first fight, dead at 42 s; no wall passed over that led anywhere. A second
    set, fresh: 0.467 for 0.468 (-0.0013, s.e. 0.0050); exits 411 for 413, stalls 8 for 9;
    93 better, 93 worse. Kept.
- **The baseline's seeds flatter it** (the maintainer's question, the same day). The same
  build on the first set of seeds, every change since 2026-11-09 kept or dropped by it:
  0.500, 451 exits. On a fresh set: 0.468, 413 exits. Two sets differ by about 0.013 from
  luck alone; this is two and a half times that. What was kept was what did no worse on
  those 1,088 runs, and some of it was those runs' luck.
- **A run of walls is the sides of one thing** (2026-11-17: the rule above, put right). The
  maintainer's pass of every level `--charmed` on Hurt Me Plenty (67 levels, E1M8 left out
  for its ending) had Doom II MAP28 and MAP16 with no exit. MAP28's way on is a floor
  raised late into a platform whose sides are a lift, looking like any wall: she finds it by
  trying it as a wall. The sides of another raised platform beside it showed the very
  signs, one of those had been pressed, and the lift's sides were "one of its run tried".
  The build before the rule: out at 898 s.
  - **The rule:** a run is also of one floor behind (`walls_weighed`): the sides of one
    platform. The sides of a stair seen from below, each step a floor of its own, are no
    longer one run; that much of the saving is given back.
  - **Measured.** `--charmed`, the 67 levels: 65 out for 64 (MAP28 at 961 s, MAP16 at
    1,357 s; MAP13 at 1,830 s for 3,460 s; MAP17, out at 3,357 s before, ran out its hour: not
    traced; MAP30 is the Icon of Sin). The score against the rule as it was: first seeds
    +0.0024 (s.e. 0.0031), exits 445 for 441, stalls 8 for 6; second seeds +0.0011 (s.e.
    0.0029), exits 412 for 411, stalls 7 for 8.
  - **What the score can't see.** She dies on MAP28 in every scored run, so its way on
    being taken away cost nothing on either set of seeds. The charmed pass showed it in
    minutes. Worth running beside the score for anything that touches exploring.
- **The fight keeps off a teleporter's front** (2026-11-17; the maintainer's sighting, Doom II
  MAP13 at 600 s). Fighting a mancubus by a pad she was sent across the level seven times,
  a few shots at it each time she'd walked back. Tic by tic: "keeping off" pushes her west
  past the end of the pad's line, "stepping for a shot" pulls her back south-east, and her
  speed carries her round the end and onto it. `slide_ok` already turned down a slide whose
  middle crosses the line; hers was 4 short of it when the game took her.
  - **The rule:** a slide is turned down that ends, on the pad's front, within 4 of its line
    taken as her half-width longer at each end (`PAD_MARGIN`).
  - **Measured.** That fight (`--charmed --endless-bullets`): one crossing for seven, the
    level out at 1,909 s where it ran out its hour. The line lengthened and no margin: six
    crossings, no better. The score, first seeds: 0.490 for 0.496, exits 438 for 445,
    stalls 9 for 8 (a margin of 8: 437 exits; and on the second seeds +0.0003, exits 415 for
    412). Kept at the maintainer's word: the first seeds are the ones the baseline has been
    chosen on, and anything that moves two hundred runs' fights loses there.
- **A walked line looked round for three times is looked round for no more** (2026-11-17; why
  Doom II MAP13's yellow card took fifty minutes, the maintainer's question). Its way is
  the red door, a line beyond it that starts a tall lift, and a jump off the lift. The red
  door was open at 1,794 s and the lift's line first walked over at 3,286 s. Across the
  doorway is the line that stops the lift, which does nothing she can see with the lift
  not started; a walked line whose work she doesn't know has her stop and look round, and
  she did so 378 times in those 25 minutes, each look breaking off the errand in hand
  (49 looks at the room's walls begun, 32 walls). The walk into the far part of the room,
  over the starting line, was offered all the while, not turned down, and never got to.
  - **The rule** (`glance`): looked round for three times this life with nothing learned
    of it, a walked line isn't looked round for again.
  - **Measured.** That run (`--charmed`): three looks for 378; red door to the lift's
    line 7 minutes for 25; out at 2,085 s for 3,519 s. Score, first seeds: +0.0004 (s.e.
    0.0027), exits 438 either way, stalls 10 for 9; second seeds, with the teleporter's
    margin too: -0.0012 (s.e. 0.0047), exits 411 for 412, stalls 6 for 7.
  - **Left:** seven minutes in that room still. A walk over floor she's seen counts as
    twice as far as it is (`Kind::Visit`), and the nearer-looking chores go first.
- **The charmed pass with endless bullets** (the maintainer's, 2026-11-17): every level but E1M8
  `--charmed --endless-bullets` on Hurt Me Plenty, four seeds (268 runs), ranked by mean time
  to get out. Nothing can hurt her and she never wants for rounds, so what is slow is her
  exploring or a loop in the fight. His bar: no Doom II level over thirty minutes. First
  pass: 253 out; none out on MAP30 (the Icon of Sin) and E3M9; over thirty minutes MAP13,
  MAP19, MAP17, MAP29, MAP14, MAP20. After the four rules below: 264 out, all but MAP30's;
  over thirty minutes MAP17 (3,229 s), MAP14 (2,208 s), MAP29 (2,146 s).
- **A lasting move takes its time from where the thing was** (E3M9, the maintainer's sighting).
  Left of the first door a switch opens it, right of it a switch shuts it, both outside, the
  exit out there too. She pressed the one, then the other as a switch not tried, and walked
  in under the closing door: shut in for the level, five strandings a run. She does plan to
  open again a door she has shut; but how long the shutting takes was reckoned from where
  the map starts the door, shut, so it was done as it began, and seen half way down it was
  taken as it stood.
  - **The rule** (`states`, `timing_from`): from where it was when she set it off.
  - **Measured.** The pass: E3M9 out on four seeds of four, 480 s, for none; nothing else
    moved. Score: first seeds -0.0013 (s.e. 0.0015), exits 436 for 438; second -0.0026
    (s.e. 0.0016), 408 for 411. The runs moved are E1M5, E1M7, E1M8, E2M8, E3M9, MAP05;
    the exits lost are E1M5's, to the pocket the fourth rule below ends.
- **Backing off isn't for ever, and one left alone is left longer each time** (MAP19, the
  maintainer watching). On a narrow ledge over a pit of demons she "backed off" from the
  nearest for as long as he watched: it can't get at her, the gun's aim won't go so steep.
  Standing with no shot has a clock and stepping for one has; backing off had none.
  - **The rule:** `Effort::BackedFrom`, as `SteppedFor`: backed off from without a break for
    `engage.close_give_up`, it none the worse, it's left a while and not backed off from
    meanwhile. And each time one is left so, by this move or by stepping, twice as long
    (`left_till`).
  - **Two goes before that held.** The mark kept for the last one only: she went from demon
    to demon, 24 leavings in ten minutes at the ledge. Kept for each, thirty seconds: with
    twenty at each of a pitful the first was hers again before the last was done, 65
    leavings.
  - **Measured.** MAP19 in the pass: out on four seeds of four at 29 to 33 minutes, for one
    of four. Score on top of the rule above: first seeds +0.0014 (s.e. 0.0017), exits 438
    for 436, stalls 9 for 10; second +0.0003 (s.e. 0.0014), exits 408, stalls 4 for 6.
- **Unseen ground behind a door that shuts is hers to go to** (MAP19, the maintainer watching:
  "she tries almost everything else many, many times"). A switch opens two doors that shut
  again; through the lower one a line underfoot opens the way to the yellow skull. She knew
  the switch opened both from 394 s and first went through the lower at 1,430 s. What's
  beyond a door that shuts with its switch outside cuts her off from the rest, and such
  places were left for when nothing safe is; but there's always another wall, another look.
  - **The rule** (`usable`): somewhere she hasn't seen into, with more unseen or untried
    beyond where she'd be left, is kept among the ordinary things to do, held back as
    what cuts her off is (after the rest of what's new).
  - **Measured.** The pass: MAP19 22 to 25 minutes on four seeds; MAP13 28 minutes, four of
    four out, for 53 and two; MAP20 and E4M5 out on all four; 264 of 268 out for 257.
    Score on top of the two rules above: first seeds +0.0058 (s.e. 0.0049), exits 446 for
    438, stalls 7 for 9; second seeds +0.0166 (s.e. 0.0050), exits 427 for 408.
  - **Tried with it and dropped:** new ground before all going back over old (a walk
    across floor seen, a wall, another look, a hunt ranked after anything unseen). MAP19
    in ten minutes, but six runs of the pass stuck that weren't: E1M5 (a walk onto a rising
    platform, later than before, into the pocket of the rule below), MAP18 (a loop, not
    traced), E4M2, and a seed each of MAP13, MAP14, MAP17. Walking floor she's seen is how
    walk-over lines are found, and when it's done matters.
- **A switch isn't pressed from where its face is out of sight** (E1M5, the maintainer's rule).
  In the nukage are two platforms that rise when walked onto, each with a switch at the top
  of its shaft that brings both down: ride up, jump off for the ledge, or press the switch
  if the jump's missed. The game's press has no height to it, and she pressed one switch
  from its platform down at the bottom, its face 270 over her head, where it does nothing;
  then rode the other up, was shut in at the top beside its switch, and didn't press it:
  the two work the same thing and that was tried. A pocket one piece of the mesh wide.
  - **The rule** (`SwitchFace`): where a switch's texture is the upper or lower part of a
    line with a way through, and that part is showing, it's pressed only from a floor
    the part is within sight of (not more than 96 over it, nor under her feet), the heights
    as they are when she would. The planner's press places and a switch not tried both.
  - **Two goes before it.** The face reckoned from the map's starting heights: the
    maintainer's objection, a switch on something that moves is elsewhere by then. Then
    "not showing, not pressed": E4M3 not out on any seed, its lifts carrying a switch's
    texture on an upper part that doesn't exist (the ceilings level). The map can't say
    whether a texture out of sight is a platform yet to rise or a leftover, so the rule is
    for a face showing only.
  - **Surveyed:** 149 such switches on the 68 maps. Of the plans' 257 presses, four are of
    a face not showing: MAP15's (shown only by the stairs its own press builds), two at
    E2M1's start, one on E4M7 that never shows.
  - **Measured.** The pass: 264 of 268 out, as before it; 258 runs the same to the second.
    Score on top of the three rules above: first seeds -0.0006 (s.e. 0.0018), exits 445 for
    446, stalls 6 for 7; second -0.0017 (s.e. 0.0010), exits 424 for 427. E1M5: a stall and
    a death to exits.
  - **All four against the baseline:** first seeds +0.0052 (s.e. 0.0053), exits 445 for 438,
    stalls 6 for 10; second seeds +0.0125 (s.e. 0.0051), exits 424 for 411, stalls 5 for 6.

- **A jump from a shortened run-up carries that run's speed** (2026-11-17). E4M5: every monster
  dead, the exit in sight, stalled. The exit's way was a run-jump off (864, 1344) through a
  window into the exit room, called off nine times as "too slow for the jump (9.0 a tic for
  13.7)"; following the plan with the whole map known failed the same way every time.
  - **Why.** The checker finds the jump comes in too fast from its full run-up and keeps it
    from a shorter one (64 units), but the jump kept the speed of the long run (15.2). The
    follower's check for a sprint knocked short fits its push to that speed, so it wanted
    13.7 where a 64-unit run from standing gives the 9.0 she had.
  - **The rule** (`keep_verified`): a run-up moved by the checker takes the speed that run
    gives (`run_speed`, the game's thrust and friction, as the finder reckons it).
  - **Measured.** First seeds +0.0005 (s.e. 0.0006), exits 446 for 445, stalls 5 for 6, 1,067
    runs the same to the tic; second +0.0011 (s.e. 0.0007), exits 426 for 424, stalls 4 for 5,
    1,076 the same. E4M5 three exits more.
  - **Found on the way, not in this:** the window is a skip (the exit room is behind a yellow
    door); she had the key and had walked through the door, and still planned the jump each
    time, a jump given up for want of speed not being counted a miss.

- **A jump given up for want of speed is a miss; one she was hit out of isn't** (2026-11-17).
  The same E4M5 run: nine plans for the exit, each through the window jump that had just
  failed, with the yellow door's way known and walked.
  - **Why.** A jump she comes down short from is counted and left out of the next plan; one
    called off three sprints running before takeoff was counted for nothing.
  - **The rules.** Given up for want of speed: a miss, as one fallen short from (another way
    first; another go with nothing else to do; twice, out for good). Hit by something since
    the run began (the maintainer's: a hit pushes her, and she shouldn't go back at once to
    be hit again): not counted, left out 30 s (`JUMP_KNOCKED_TICS`). The hit's tic is in the
    observation (`hit_tic`), so it counts while the fight has her legs. A landing looked at
    more than 4 s after the jump isn't judged: 22 of 155 misses timed in a score's logs were
    judged later than that, the fight having moved her since (MAP06: landed at 304.6 s,
    "missed" at 316.1 s).
  - **A go before it:** the hit noticed by the follower on its own tics: 319 misses in two
    scores, none recognised as a knock.
  - **Measured.** First seeds +0.0006 (s.e. 0.0010), exits 447 for 446, stalls 5 for 5;
    second +0.0024 (s.e. 0.0012), exits 430 for 426, stalls 4 for 4. Eleven knocks
    recognised. Barrels and other solid things: nothing added (barrels are cleared; the
    planner knows the rest).

- **A press place is looked for square in from a poly's edge too** (2026-11-17). MAP30: she
  could not plan the second tier's lift (line 47), with the whole map known or not: "try the
  wall (line 47)" over and over, the skull switch behind it never reached.
  - **Why.** Where a line can be pressed from, in a poly, is tried at spots along the poly's
    edges moved a box's half towards its middle. A tier 2,000 across has its middle far along
    its edges: a spot on the edge by the lift, moved towards it, is still on that edge, her box
    over the lift's own line, and the press was held to need the lift down already.
  - **The rule** (`press_point_in`): each spot is tried moved square in from its edge as well.
  - **What it touches.** The analysis of 57 of the 68 maps: mostly presses no longer held to
    need something moved first, and a poly or two more to press some lines from. The 24 parity
    runs the same.
  - **Measured.** First seeds +0.0004 (s.e. 0.0013), exits 447 for 447, stalls 5 for 5;
    second +0.0014 (s.e. 0.0013), exits 433 for 430, stalls 3 for 4. All of it on MAP14.

## The Icon of Sin (2026-11-17)

A level that ends when a boss brain dies had no exit to her: MAP30 was the one level never got
out of, charmed or not. The maintainer's brief: custom logic that takes over on any map with an
awake brain, finds a platform (a moving one too) to angle rockets into the gap from, goes
there and fires till it's dead, killing only what's in the way.

- **An omniscient shortcut, said plainly.** The mode plans with the whole map known
  (`Navigator::full_view`) and takes the brain's place from the map. It's the one place she
  does. The maintainer's earlier call was that the brain may be known (its cry wakes the
  level); this goes further, to the switch and the lifts, because the level is a race against
  what the brain spawns. What it buys: MAP30 from never to out (below).
- **Where to fire from** (`tactical/icon/lanes.rs`). The game's rules for a player's missile,
  worked on the map's lines: it leaves 32 over her feet and flies level, a box 22 across and 8
  tall; it goes off where the box stops fitting; the blast hurts within 128 by the further map
  distance, in its sight. Spots along lines through the brain, each with the heights her feet
  may be at. MAP30: feet 353 to 376, 105 a rocket. `evil-boom brain-posts` prints them.
- **How the floor comes right** (`tactical/player/boss.rs`): as it stands; a lift that passes
  through those heights (called, boarded at the bottom, one rocket as it carries her through:
  the window is 6 tics and the launcher refires in 20); either once a switch has moved the
  floor for good. MAP30 is the last two.
- **Found by tracing its runs.**
  - A player's missile goes at anything in her aim (three lines, 5.6 degrees apart, 1,024
    out): every miss had a monster there, every clean shot hurt. No rocket with one near
    enough for the blast to reach her.
  - Use has no height: she backs off the lift over the line that calls it and presses it in
    the air. A ride every 11 s, for 12.8.
  - The slime was 48% of the damage taken (16 runs): she waits for the pillar on a floor that
    doesn't hurt, and is on the lift the moment it's down.
  - Her legs on its routes are the fight's only for a dodge; with none she died sooner (HMP 2
    of 8 for 7), with every move allowed the route never got there. Rockets are kept from the
    fight (the launcher's left out of what it sees she has).
  - What's in sight and shooting as the brain wakes gets a few seconds first (UV's revenant
    in front of the face: its tracers had her dodging the whole way round).
- **Measured.** Charmed: out on every seed tried, 72 to 111 s. Mortal, 8 seeds: HMP 7 of 8,
  UV 2 of 8. Score, first seeds +0.0032 (s.e. 0.0017), exits 451 for 447 (MAP30 4 of 16 for
  none); second +0.0018 (s.e. 0.0013), exits 435 for 433 (MAP30 2 of 16). Only MAP30's runs
  differ, on both.
- **After the first rocket, what mends her; and the BFG for what's waiting** (the
  maintainer's two, the same day).
  - Hurt (health or armour under 150) with a rocket just away, and a megasphere or
    soulsphere she's seen within 700 of the post: off the lift for it by the planner's way
    from where the lift's going (MAP30: a run-jump from the pillar to the ledge before the
    face), and a radiation suit by it if the floor the lift's called from hurts; then back to
    the rides. Each thing once.
  - What's in sight and shooting as the brain wakes: the BFG ordered for it, till it's dead
    or 12 s (UV's revenant: dead 8 s after the cry).
  - Found with them: a dodge had her legs while she waited on the lift at the bottom, and
    stepped her off it, the ride lost (`Turn::Stay`: not a step); a lift she'd ridden, seen
    at the bottom, was no lift to ride (its rest is remembered); the place to wait for the
    pillar could be the pillar.
  - **Measured**, 24 fresh seeds a skill, mortal: HMP 17 out for 12; UV 7 for 8. Parity's 24
    runs the same.
- **Not done:** other brains than MAP30's are untested (TNT's and Plutonia's are held out); a
  switch that opens the rocket's way (not the floor she stands on) is left to her ordinary
  exploring, the mode trying again as the heights change; no way to it, she explores.

## Stuck runs, round one (2026-11-17)

The maintainer's brief: fix what stalls on the scoring sets, then a fresh seed set, and again.
Eight stalls on the two sets to begin with: E4M6 three, MAP21 two, MAP14, MAP06, MAP20.

- **A ledge is dropped off where a ceiling beyond lets her** (E4M6's three). A lift takes
  her up to a soulsphere's ledge whose one way off is a drop into a sliver of floor, 64 wide
  at its middle and nothing at its ends, the yard beyond it under a ceiling 16 over the
  ledge's floor. The game holds her at the ledge's height till her whole box is past the
  line; crossing at the narrow end her box met the low ceiling first and she stood at the
  edge "waiting at mover" for a floor that doesn't move, every objective, till the run
  stalled.
  - **The rule** (`drop_stretch`, `landing_span`): a path crosses a ledge only along the
    stretch where no ceiling is too low for her up there with the box wholly past.
  - **A go before it** asked for the box to fit there altogether and never tried a line's two
    ends: every ledge lost 8 units at each end, a drop onto a teleporter's pad missed the pad
    (MAP25's start: dead in the blood in 30 s, 17 exits lost on the two sets). Now only
    ceilings are asked, and a ledge open all along is left as it was.
- **Wedged is stranded** (MAP20). "Stuck here 9 times over, whatever for: stranded", and no
  restart: with one life the wedge's verdict was called off as a wish for the level again
  for what's left, which the same flag carries. It takes one of the goes kept for being
  stranded (`Explorer::wedged`).
- **Measured, the two together.** First seeds: +0.0002 (s.e. 0.0038), exits 451 for 451
  (MAP30 three more from the commit before, the rest three fewer), stalls 2 for 5. Second:
  +0.0056 (s.e. 0.0035), exits 442 for 435 (MAP30 five more, the rest two more), stalls 2 for
  3. Stranded restarts 43 for 26.
- **The pieces of a ledge under a low ceiling are no way down** (E4M6, a fourth seed at the
  same ledge). The ledge is several pieces of the mesh: she crossed by one lying wholly
  where the ceiling beyond is too low, and the path's stretch had nothing of it to keep to.
  A drop by a portal is one only if some of the portal is within the stretch she can drop off
  (`drop_reaches`).
- **A line isn't pressed from behind it** (MAP21). A lift built up as stairs, called from
  beside it: 488 presses of its line from the back, at the line's end, where a piece of the
  mesh lies on both sides. The game takes a press from a line's front only; the follower,
  waiting at a floor, passes over a line she's behind.
- **Measured, the two together.** First seeds: 1,081 of 1,088 runs the same, exits 451 for
  451, stalls 2 for 2. Second: 1,078 the same, exits 442 for 442, stalls none for 2.
- **Plasma going short of it is no shot** (MAP14). At 2 health, the plasma rifle her only
  gun with rounds, a shotgun guy 500 off across a pit's lip: fire held ("plasma wouldn't get
  there") and stood facing him five minutes, between steps for a shot and falls back to
  cover, given up twice in the run. Her patience with a monster she has no shot at asked
  only whether the line to it is clear. With the plasma in hand and its fire held for that,
  it's no shot (`no_shot_long`), and he's left a while like any other she can't get at.
  Measured: first seeds 1,083 of 1,088 runs the same, exits 450 for 451, stalls 1 for 2;
  second 1,085 the same, exits 442 for 442, no stalls.
- **Traced and left:** MAP06 on Ultra-Violence, the one stall remaining on the two sets: out
  of ammo by 158 s with the spider mastermind about, a third of the map seen, no route to the
  exit from where she ends. A want of ammo, not a loop.

## Stuck runs, round two: a third seed set (2026-11-17)

The build after round one on eight fresh seeds a skill (`out/score/base3.tsv`): 448 out, 637
died, 3 stalled (MAP07, E2M4 on Ultra-Violence, MAP11 on Ultra-Violence).

- **The next of a boss's kind isn't shot while the one before is dying** (MAP07). All twelve
  dead, the exit's step at 40 where the plan has 24, and no way onto it: the level can't be
  left. The game's own fault: as each arachnotron finishes dying it looks for another alive
  and, finding none, raises the step; the last two killed 51 tics apart (dying takes 55) each
  found none, and it rose twice.
  - **The rule** (`boss_dying`, `HeldFire::BossDying`): on a map where a kind's last death
    moves something (`boss_death_kinds`), fire's held at that kind for 70 tics after one of
    it dies.
  - **Measured** on the four levels with such kinds (MAP07, E1M8, E4M6, E4M8), all three
    seed sets, 192 runs: out 61 for 57, the stall gone; MAP07 on 24 more seeds a skill, 3 out
    for 4 on Hurt Me Plenty, none either way on Ultra-Violence. Parity's 24 runs the same.
- **A way on she knows of comes before the walls** (E2M4 on Ultra-Violence, 53 minutes). At
  18 health, the one way on (through the blood to the exit's door) turned down for want of
  health, and wall rounds from then on: the pass that lifts the margin comes after every
  round of walls and never came. Ways on (a line to set off, a look into somewhere not
  seen) turned down only for health are tried before the walls, refused only if they'd take
  all she has (`ways_on_hurt`). The run: out at 2,800 s.
- **Cover that gets her nothing isn't gone back to** (MAP11 on Ultra-Violence). Out of ammo
  at 20 health: off for a shotgun, two imps in sight, back to cover, off again, 76 times in
  five minutes. Back to cover three times running with nothing killed and nothing learned
  between, cover's off a minute, twice as long each time (`cover_spent`,
  `retreat.cover_runs`, `retreat.cover_rest`), and she goes on with what she was about.
  The run: every monster dead, 837 of 850 lines, dead at 1,258 s on the way to the exit.
- **Measured, the two together, three seed sets.** Exits 445, 446, 454 for 450, 442, 448
  (1,345 for 1,340); run for run -0.0035 (s.e. 0.0038), +0.0030 (0.0043), +0.0049 (0.0037).
  Stalls 2, 2, 0 for 1, 0, 3: the three on the third set gone, four new on the first two
  (E4M9, MAP15, E4M4, MAP14), each further on than she got before (cover rested two or
  three times in each): exploring loops at 692 s, 3,063 s and 2,651 s, and on E4M4 out of
  ammo backing off from an imp. Round three's.

## Stuck runs, round three (2026-11-17)

The four stalls round two left on the first two seed sets, each further on than she got before.

- **Held short of a lift's line while its floor's door is open** (MAP15). A block that's a
  lift by one walk-over line and a door by another. She opened the door and walked over the
  lift's line 52 tics later; the door came down to the lowered floor, the floor came back up
  into it, and the two held each other there for good, every line that moves either doing
  nothing after: the way to the exit shut. The game's own fault. A lift's walk-over line
  just ahead of her, the floor it lowers under a door that's open and shuts by itself: she
  waits short of the line till it has, 8 s at most (`jam_wait`). Alone: 4 or 5 runs changed
  a set, exits 447 for 446 and 454 for 454.
- **Out of ammo, one clock for backing off** (E4M4 on Ultra-Violence). Low and unarmed among
  three imps: "backing off" and "getting past" by turns every few tics for five minutes.
  Backing off is given up a monster at a time, and with them taking turns to be nearest no
  clock ran out. Out of ammo she can hurt none of them: one clock for the lot. That run:
  dead at 244 s. Alone: exits 444 for 446 and 454 for 454, about 15 runs changed a set.
- **A floor she's stood waiting at is as she found it** (E4M9). A fence that's a lift came
  back up beside her while she faced the other way; her plans went across it as down (as
  last seen), and she waited at it, again and again. What the follower has stood a second
  at, or waited for as a lift to come back, is noted at the height it's at (`felt`).
  - **A go before it:** everything within 40 units of her seen at its height, whichever way
    she faced. About 150 runs changed a set; alone, exits 444 for 446 and 446 for 454. Dropped.
- **The three together, three seed sets:** exits 447, 444, 454 for 445, 446, 454 (1,345
  for 1,345); stalls 1, 0, 0 for 2, 2, 0; run for run +0.0013 (s.e. 0.0015), -0.0015
  (0.0016), -0.0001 (0.0002).
- **Tried and held back:** ammo a retreat made for and never had, left alone a while (MAP31
  on Ultra-Violence: out for a stimpack, demons, away for a clip never got to, out for the
  stimpack again). It ends that loop, and with the three above cost exits 439 for 447 and
  451 for 454: on unarmed after one trip that failed, where the loop would have ended by
  itself. The maintainer's idea in its place, next: the walls near her searched hard before
  a long walk for supplies.
- **Out of ammo, the walls near her before a long walk for supplies** (the maintainer's,
  watching the MAP31 loop: walls with things behind them walked past, round after round).
  With nothing to shoot with, and the nearest thing she needs (health, ammo) over 1,024 off
  as the crow flies: the walls within 768 of her (and no further than half way to it) are
  searched as hard as a search for secrets goes (thoroughness 3), ranked as needed, before
  the walk. Each wall once.
  - **A go before it** did the same whenever anything she needs was far off, armed or not:
    over half the runs changed, exits 1,305 for 1,345 on the three sets, stalls 5 for 1
    (E4M7 down 5, 6 and 7; MAP14 1, 6 and 3), items picked up 2% more. "Needs" takes in a
    better gun seen and a medikit when she's a little hurt.
  - **Measured**, out of ammo only: exits 454, 442, 456 for 447, 444, 454 (1,352 for
    1,345); stalls none for 1 (no stall on any of the three sets); run for run +0.0045
    (s.e. 0.0031), -0.0014 (0.0037), +0.0011 (0.0032); about 100 runs changed a set. MAP31
    itself 3 and 1 fewer out on the second and third.

## Her rockets, measured (2026-11-18)

The maintainer's, from watching: rockets into walls, perhaps for her moving while one's
coming out. Each rocket of hers is followed for the run's statistics
(`tactical/combat/rockets.rs`): what it hurt, and what she and its target did between the
trigger and the launch 8 tics on. Two seed sets, 7,806 rockets in 688 runs.

- **54% hurt what she aimed at, 5% something else, 41% nothing.**
- **Not her feet.** 88% are fired with her feet moving (over 16 between trigger and launch),
  and those hurt their target more often, 56% for 45% stood still, at every range. "She
  stepped" was in three quarters of the misses only for being in nearly every rocket.
- **Range.** Under 256, 74%; 256 to 640, 68%; over 640, 41%, and half of all her rockets are
  fired from there. A rocket's 32 tics to 640, and what it was fired at is elsewhere by then.
- **Her own blast.** 6.2% go off within the blast's reach of her; in 298 of those 487 she was
  out of it at the trigger and walked in. Not yet acted on.
- **The launcher's reach: 768** (`bestiary::weapon`), past which it isn't fired, wide
  targets excepted (a cyberdemon's 40 across and up: `style::WIDE`). Alone, three seed sets:
  exits 1,361 for 1,352; a tenth fewer rockets, 60% hurting their target for 54%. Four
  stalls came with it, none its doing: three at E4M5's exit window, below.
- **Retreats to cover are one run of them within three minutes** (`cover_spent`), for one:
  a retreat, the wait and the walk back is a minute by itself, and at E4M5's exit window,
  a shotgun guy under it too steep to shoot, they came 60.1 s apart and the count began
  again each time.
- **The two together, three seed sets:** exits 452, 452, 459 for 454, 442, 456 (1,363 for
  1,352); stalls 0, 0, 1 for none (E1M5, not traced); run for run -0.0029 (s.e. 0.0054),
  +0.0059 (0.0052), +0.0032 (0.0050); rockets hurting their target 61%, nothing 35%.
- **From here, by the maintainer's word:** an experiment is screened on one seed set; three
  only with their leave.

- **A press from an earlier go counts as tried only if she learned what it does**
  (2026-11-18; E1M5, stranded three times and stalled). A once-only switch pressed on her
  first go, what it opened not seen; the level begun again, the door's shut and the switch
  works, and her note of the press had it tried for good: the closet with the way to the
  blue key shut every go after. Correctness, not judgement (`Knowledge::counts`). That run:
  out, 91 of 91. Second seeds: 3 runs changed, no outcome.

- **Five more from the third set's screen** (2026-11-18), scored together on the first
  seed set: exits 452 for 452, stalls 1 for 0, run for run -0.0012 (s.e. 0.0065), 584 of
  1,088 runs changed.
  - **A switch's twin** (`pressed_again`): another line of what a press worked, mapped and
    never pressed itself, is tried from where it is, with no wait, where no line of it has
    taught her what the press does (E1M5: two pillars up and down together, a switch on
    each, the only way off). Without that last condition it was every learned door's
    other face: MAP14, a hundred and fifty "try door again", an hour stuck.
  - **Idle, she looks about** (`IdleLook`): a turn one way of 25 to 120 degrees at 1.5 to
    6 a tic, a pause of 12 to 90 tics, a turn the other way; a full look round first when
    she's out of things to do. It replaces the endless spin, and the fixed left-pause-right
    of waiting to be told.
  - **Cover that gets her nothing, by what she knows** (`Knowledge::known`): the count of
    lines fired was in the comparison, and a teleporter walked is a line fired, so a round
    of eight teleporters back to cover was news every time (MAP21: ten minutes of it, two
    seeds).
  - **A ledge left over its landing's middle, square on** (`smooth`): where only part of
    a ledge can be dropped off, the path is pinned a body's width before and past that
    part's middle; the smoothing after took the crossing back along the line to wherever
    was straightest (E4M6).
  - **Held up on a ledge, the track is kept to** (`overhanging`): her middle over the
    floor below and her box's edge still on the ledge, no corner is rounded. She turned
    along the ledge up there and stopped where a ceiling past it was too low (E4M6, the
    narrow step under the lift: the pin's own stall on the first screen).
  - **The stall left:** MAP06, skill 3 (died at 438 s before). A blazing lift's window of
    4.4 s to make by a drop of 320 that she creeps up to (`creep_to_drop`), 23 times
    missed. Neither the pin nor the ledge rule acts there. Next.

- **MAP06's east lift switch** (2026-11-18): it lowers the west lift for 4.4 s, and her
  way there was a drop of 320 onto 45 of floor with nukage past it. Two faults, traced tic
  by tic, fixed together; that run out in 734 s, 85 of 90.
  - **The creep is kept till she falls** (`RouteFollower::creep`): her middle over the
    edge, the cap went, and with her box still on the ledge four tics she was at 6.5 a tic
    for the 25 of the fall: 140 out, in the nukage.
  - **The plan counts the creep and the fall** (`arrival_tics`): a drop onto a landing
    narrow the way she's going costs what the follower takes over it (`motor::creep_pace`,
    shared). Planned at 3.4 s, walked in 5.4: pressed, missed and gone round again 23
    times. Now that window isn't planned by that drop.
  - First seed set: exits 451 for 452, stalls 0 for 1, one run at the hour's cap for none
    (MAP14 skill 4, died at 2,888 s before: at the blue door only at 3,111 s, then "try
    door (line 1077): no route", seen on MAP14 before these two; not traced); run for run
    -0.0006 (s.e. 0.0041), 209 of 1,088 runs changed. MAP06: 2 more out.

- **A fall under a jump is for good only with no way back she'd plan** (2026-11-18;
  MAP14, an hour's cap hit). The gap before the exit's door: a window, 88 of moat, the
  ledge with the door. `falls_for_good` asked whether she could walk from under the flight
  back to either end, and from the moat she can't; but it's the moat she has been round
  all level, a lift out of it, and the way back to the window is the lift, the blue door
  and the jumps that brought her. The door past the gap was "no route" till everything
  safe in the level had been gone over again, five minutes and more. Now, where walking
  back fails, the planner is asked for a way from there to the takeoff (switches, doors
  she has the key to, jumps), once for what she knows (`back_up`).
  - That run: out at 1,998 s, 78 of 78. First seed set: exits 450 for 451, at the cap 0
    for 1, no stalls; run for run +0.0017 (s.e. 0.0028), 109 of 1,088 runs changed.
  - Left: the two jumps on the way to the blue door are over pits with no way out she'd
    plan, and stay the risky pass's. And past the blue door a shotgun down a one-way drop
    can take her off into the moat before the door's tried (one seed, four minutes).

- **Shut in with a switch at hand, she presses it again** (2026-11-18; E1M5's pillars, a
  run stranded facing the switch). The two pillars are one tag: every raise line and both
  lowering switches work both. Hers was held half up by her own head under the ledge and
  rising again as she pressed; the game skips a floor on its way, the other pillar went
  down, she heard it, and that was the switch learned, truly, and never offered again.
  - **The rule** (`known_switches_again`, the last resort of all): with nothing at all to
    do for three seconds, a repeatable switch within 256, not a door, is pressed again
    though she knows its working, once something has moved since she last pressed it and
    has come to rest.
  - **Tried and lost, second seed set, each -0.005 (s.e. 0.0024), 7 exits:** every
    switch and door in reach; and the switch at hand pressed the moment there was nothing
    to do. "Nothing to do" is often a second's wait, and pressing into the same stillness
    spent the one press before the pillar had carried her up, and blocked the minute's
    re-press that had been getting her out 21 tics ahead of the stranded call.
  - **As kept, second seed set:** exits 433 for 434, runs stranded 19 for 21, 17 of 1,088
    runs changed, run for run -0.0003 (s.e. 0.0012). E1M5: the stranded run out, the other
    stranded one out in 553 s for 1,040, four more out sooner.

## Continuous play, and where the bot stops (2026-11-18)

Episodes on one run of lives: 8 seeds, four lives an episode, three stranded restarts, the two
easiest skills, on `ebf71b6`.

| | skill 1 | skill 2 |
|---|---|---|
| E1 | 8/8 | 8/8 |
| E2 | 8/8 | 8/8 |
| E3 | 8/8 | 5/8 (three out of lives at E3M8) |
| E4 | 1/8 | 0/8 (thirteen of fifteen ended at E4M6, two at E4M2) |

- **E4M6, traced.** The cyberdemon guards the exit from a raised room over the lava; the level
  is nearly all of itself done under its rockets for what kills it (the BFG, cells, an
  invulnerability: one in its room on the two easy skills, one at lava level on every skill),
  then the kill point blank. From the episode's loadouts, second skill, 32 deaths: 17 the
  lava, 15 its rockets; about 19 in the lava pool round the central teleporter (the four
  lifts, the four sides that go four ways); 0 to 3 cells at 22 of them (she comes in with 1 to
  82); on walls, teleporter tries, medikits. Each decision right by itself, the order wrong.
- **Tried and set aside** (`sphere-rush.patch`, not committed): the sphere run for past what
  it's for with the BFG, 120 cells and 80 health; and the sphere kept as the fight's errand
  however long the way (the fight's "keeping clear" held her in the lava at the lift to it).
  E4M6 alone, exits of 16: first skill 6, 11, 11; second skill 0, 2, 1.
- **The maintainer's, and it's a rule from here:** rules of that size, made for one level's
  shape, will cost elsewhere; and the plan of a level (what to get first, what to keep, when
  to spend a suit or a sphere) is past what the bot is for. That's whoever directs her: Evil.
  The bot's part is to do well what it's told and what's in front of it. A mode that planned
  E4M6 for her was drafted and dropped on that word.
- **Left:** E3M8's three on the second skill, not looked at. Whether Evil has the orders such
  a level needs (keep the cells, stay out of there, that item next, a suit and then across)
  is the question that follows.

## Charmed continuous play, and MAP13's key room (2026-11-18)

Continuous play with the charm and endless chaingun bullets (nothing hurts her: what's left
is the exploring), 8 seeds, on `ebf71b6`. Ultimate Doom, all four skills: Episodes 2 and 3
through on all 64; Episode 1 to E1M8 on all 32 (E1M8 can't end charmed: it ends by her
health); Episode 4 short by five runs at the time cap (E4M6 three, E4M2 two: not traced).
Doom II: 8/8 on the two easy skills, 7/8 on Hurt Me Plenty (MAP18), 5/8 on Ultra-Violence
(MAP13 three), the Icon dead on all 28 that reached it. MAP17 takes 54 to 64 minutes on
every skill (not traced: the maintainer suspects the jumps over lava that aren't needed).

- **MAP13 traced.** The yellow key is on a stepped pyramid (tiers at 112, 208, 304 over a
  floor at 16), thirteen imps on the tiers and the lift on Ultra-Violence, seven on Hurt Me
  Plenty, none below. The way up is a perpetual lift (line 918 starts it, the doorway's 952
  stops it), then a run-jump. All but five sectors found by 22 minutes; then one to two and
  a half hours at that room, the key not an objective till she'd learned the lift. Endless
  cells and the BFG made no difference: it isn't firepower.
- **Kept, five rules** (`f6d4c3b` to `4a3d1a4`): no shot at anything in sight is one effort
  and the lot is left together; a perpetual lift isn't waited for before its start line; one
  on another floor with no way to walk to her isn't backed off from; backing out of an ambush
  is rested as cover is; a circling tracer is shaken off with only what she can't fight
  about. MAP13 charmed on Ultra-Violence: all eight out in 26 to 46 minutes. Scored together,
  second seed set: 0.486 to 0.490 (+0.0031, s.e. 0.0047), exits 433 to 440, stranded runs 19
  to 15, one run at the time cap (MAP14 UV, neither new retreat nor fight rule fired in it).
- **Measured and narrowed.** "No way to walk to her" without "on another floor" cost E1M9 10
  exits of 48: the pen of imps by its start, fenced on her own floor, is worth backing off
  from for the room to dodge.
- **Dropped:** monsters that can't come at her not counted in an ambush (the count is right
  where there's elsewhere to go).
- **Left in the room:** about ten minutes from entering it to crossing the lift's line; the
  shot test against the engine's aim (imps on the 208 tier should be hittable from 240 back,
  and she holds fire at 274: not checked); with plain invulnerability (the review's trace of
  18 November) the lift's lines are still crossed over and over, by what isn't known.
- **Revenant tracers** (the maintainer's): on open ground with no corner the player's answer
  is her back to a wall and a step aside at the last moment; and a flock of them kept
  circling can be led onto a cyberdemon. The first is advanced and unbuilt; the second is a
  plan across a fight, Evil's to order if ever.
- **The review of 18 November** (`behavior-review-2026-11-18.md`) checked against the code on
  eight points, all sound: parity passing on failed runs, score counting a run that never
  started, `--carry` dropped by sweep and episode, secret allowances by map name, "the same
  to the tic" being outcome, whole seconds and kills, the Icon's restart, the stuck prompt's
  flag, and the lot's two marks (fixed in `f6d4c3b`).
- **Done from it on the 19th:** parity and the score fail on runs that failed, and the report
  says what it compared; `--carry` reaches a sweep's and an episode's runs; "stuck?" is asked
  once it can be; an ordered restart sets the Icon's fight back; a stop takes back a start due
  after a crash. Not done: the secret allowances by map identity (before community WADs), the
  secret taught in the wrong sector on a replay, `watch --from`, perpetual lifts by a property
  of the special, the restarts' accounting, and the structure.
- **The planner's budget misses: looked at, nothing to do.** A throwaway build searched again,
  with the whole budget and with five times it, for every route search that ran out. All 68
  levels, one seed: Hurt Me Plenty and Ultra-Violence as she plays (66,745 searches) and
  Ultra-Violence charmed to each level's end (58,078): none ran out, no search was made on a
  halved budget, and all 294 with no route had looked everywhere. The clock limit on a route
  search is 600 seconds, a backstop: the count of steps decides, so load doesn't. The fight's
  own searches weren't counted.

## MAP14's alcove behind the red bars (2026-11-19)

MAP14 on Ultra-Violence from a pistol start, 64 seeds, on `d6eeda4`: 17 out (24 to 57
minutes), 47 dead (29 by chaingunners and shotgun guys; 21 with nothing left to shoot), no
run at the time cap; 21 strandings in 15 runs, 15 of them in one place.

- **The place.** The alcove behind the red bars (384 to 576 by -160 to -344): the bars to the
  north, a wall to the west, a ledge 32 up to the east that she drops off and can't step
  back onto, a door to the south. The door is opened by a line beyond it and by one 16
  inside the bars, for whoever walks in once they're down: from inside she comes up to that
  line and can't get her centre over it. With the bars up there's no way out. The level's
  flaw (the maintainer's).
- **Why she goes in.** The door opens and shuts by something else's doing (not found what);
  she saw it open, and the planner takes a sector as last seen: 16 seconds on she planned
  over the ledge, down and out by it, and found it shut.
- **Tried and dropped** (`out/score/counts-on-moved.patch`): a route with a drop she can't
  climb back from refused when, from where it lands her, the way on and the way back both
  hang on a sector seen moved from its rest by nothing she's learned. The 64 seeds: exits 17
  and 17, strandings 21 to 10 (the alcove's 15 to 7). The score, second seed set against
  `lot5.tsv`: -0.0005 (s.e. 0.0008), 1 better, 3 worse, 1,082 the same; exits 440 and 440;
  the one run at the time cap out at 55 minutes; nothing but MAP14 moved in 1,088 runs. A
  rule that in practice is one level's, for no gain on the score: dropped on the
  maintainer's word given beforehand.
- **Also tried, no effect:** a walk-over line counted as set off only where her box has room
  both sides of it. True of the engine; the plan never rested on that line.
- **Left:** the other seven alcove strandings are her dropping in from the ledge to press a
  wall she suspects (the rule that a well-marked wall is worth a pit); three strandings at
  (1170, 422), "try switch (line 375)", not traced.

## Two faults seen on an ultra-instinct tape, both her own (2026-11-19)

Watching E4M6 played by the ultra instinct (a branch: `ultra-instinct-plan.md`), the
maintainer saw her push at bars she can't fit through and fire plasma into a wall. Both
were her ordinary play, and are fixed on main (`git log` for the two commits' numbers).

- **Plasma into a wall.** `projectile_reaches` goes by the shot's middle; at a corner it
  wavers, and she fired when it said clear: 92 cells for nothing on E4M9. Kept: plasma she's
  just reckoned blocked, at a far target standing still that isn't a boss, its rounds landed
  and it none the worse, is held for half a minute.
- **The bars.** Stood in a notch between two shut pillars she touches both, and what she
  touches was let off the next stretch. Kept: not where the stretch goes on into shut faces
  she's touching on both sides.
- **What it cost to get there:** the first versions of both together scored -0.027 (s.e.
  0.0065), exits 455 to 426; the second -0.0065 (s.e. 0.0032); the third +0.0005 (s.e.
  0.0005), 3 runs of 1,088 changed. A rule that stops her shooting, or refuses her a way,
  wants to be as narrow as the failure it came from; and E2M8 is where a shooting rule that
  isn't shows first.

## Two faults of her own found by ultra instinct on Plutonia (2026-11-19)

The ultra-instinct runs on Plutonia MAP01 and MAP02 (the plan's stage 3 check) kept her alive
through the fights and then stalled on her own exploring. Two tapes watched by the maintainer,
two general faults, both hers and both fixed on `main`. Plutonia stays held out of tuning: each
is a rule about the game, looked up in the engine and measured on Doom and Doom II.

- **A teleporter's hop read as a walk** (`22c6fba`). The follower took two polys of a path in a
  row for a walk when a portal joined them. A pad in a pit under the walkway it sends her to
  has just that: she waited for the walkway as for a lift. A path now keeps its teleporter
  hops and all six looks ahead ask one place (`RouteFollower::walk_portal`). Score: all 1,088
  runs the same to the tic.
- **An item resting above the floor under its middle** (`b1bb24b`). A thing rests on the
  highest floor under its box, so one dropped at a ledge's edge stays up when the floor beside
  sinks. She stood under a chaingun 177 units up. Where to stand for an item now goes by where
  she sees it lie, and the out-of-ammo retreat leaves such a one out. Score +0.0007 (s.e.
  0.0034), exits 456 to 457, runs stranded 18 to 16.

Not done: an item she's stood on and couldn't pick up is still offered again by the next
list of options. With the cause above gone no case of it is known.

## Stood for good with a way on she knew: E2M6, lives, and a route weighed whole (2026-11-19)

Found by the ultra-instinct sweep (all 68 levels, Ultra-Violence, pistol start, ten lives, two
seeds; on the branch `ultra-instinct`, its plan has the sweep). Levels where a run reached the
hour alive with few deaths were run on eight more random seeds each to tell a rare stuck from a
fault: MAP11 8 out of 8 and E3M6 7 (rare); E3M7 6; E2M4 4; E2M6 3; MAP14 2; E4M1 and E4M2
none. E2M6 first.

- **Not the ultra instinct's doing:** the same seed with it off is the same run.
- **The cause:** a way on of several legs (four switches, each a trip over the courtyard's
  hurting floor) weighed as one route: 107 of her 100. Refused whole, at full health, for good.
  In the lives before she had found the switches one at a time. Knowing more made her stand.
- **A leg at a time** (`347725f`, `Explorer::way_on_by_legs`): the furthest switch of such a
  way she can get to with her margin left, by itself, once a life each. Two tries at the rule
  went wrong first: the first switch is the lift out of the start, which comes back up behind
  her; and the leg was refused by the other health rule (harm while something safe is left)
  till it was marked a risk taken knowingly.
- **Whatever it costs** (`3e7220f`, `Explorer::way_on_whatever`): the last resort of all. A
  stall is worse than a death, the maintainer's weighing, and the cost is a guess.

Four of the stuck seeds, ten lives: all stood; with the first rule two get out and two stand;
with both, two get out and none stands (one out of lives, one at the hour after eight deaths).
Score: -0.0001 (s.e. 0.0001) and -0.0003 (s.e. 0.0005); exits 457, 457, 456; runs stranded 16,
17, 15. The exit lost is an E3M7 run that had got out by way of a stranding and the level
begun again, and now dies on the way she takes instead.

Still to trace from the same sweep: "the jumps missed once get another go" once a second for
the rest of the hour (E4M2, E4M1, E3M7); E4M1 pressing one known switch 108 times in ten
minutes; MAP14 and E2M4 (mixed).

### E4M1: a switch pressed for half an hour, and a key she wouldn't risk (2026-11-20)

The same sweep's E4M1, none out of eight seeds. One seed traced, two faults:

- **"The jumps missed once get another go", once a second, was nothing happening.** The line
  is logged before the retry's tried. Under it she had nothing left to do. (The line's still
  to mend.)
- **A lift's switch pressed 337 times** (`known_switches_again`): its own lift moving was
  "something moved since her last press" every time. What counts now is what the press
  didn't set going (`Knowledge::last_moved_but`). Score: all 1,088 runs the same.
- **The blue skull 7 health off with 5 left:** refused for good, because a key wasn't a "way
  on", which is what the rules for having nothing else to do go by. It is one now. Score
  +0.0007 (s.e. 0.0007), exits 456 to 457, runs stranded 15 to 14.

The seed, ten lives, no ultra instinct: stood at the hour before; out at 1,638 s with both.
`way_on_whatever` is what got her through the red door on 5 health.

Not yet known: whether E4M2's stuck is one of these. Rerun with the ultra instinct off she
is out of lives in twenty minutes, before the place she stood; it wants the branch brought
up and the same seeds with it on.

## MAP14 watched: plasma's width, the front door, and what didn't work (2026-11-20)

The maintainer watched one of the sweep's stuck MAP14 runs (Ultra-Violence, ten lives, the
ultra instinct on) and it was painful: cells into window edges, shells through a grate, an
hour round the moat, and never in by the courtyard's north door, where the blue skull's door
is three seconds on. What came of it, each scored on the third seed set against the commit
before:

- **Plasma by the ball's whole width** (`80e6b55`), at one further than 384 that isn't a
  boss. +0.0058 (s.e. 0.0034), exits 457 to 468. The whole width everywhere was -0.0005 with
  E4M8 down four exits of sixteen and E2M9 three: close among pillars a ball that clips one
  is a ball, and a stream held is the fight.
- **A door a line underfoot opens every time opens from its far side too** (`036994d`), in
  the reckoning of what would cut her off. The courtyard had been "after what it cuts off",
  behind every wall she only suspects. -0.0006 (s.e. 0.0019), exits 468 to 467, runs
  stranded 15 to 13. The three MAP14 seeds that stood at the hour by the blue skull: two
  get out now (2,911 and 2,991 s), by that door.

Tried and not kept (patches in `out/score/`):

- **An arch-vile she can't hurt not kept clear of** (`vile-not-kept-clear.patch`): she kept
  clear of it 6 times for 100 and still stood at the hour. The level starves her of ammo
  and the arch-vile refills it with monsters; no one rule of the fight mends that.
- **A shotgun's spread reckoned** (`shotgun-fan-v1.patch`, `-v2`): four tries, -0.0512 the
  first (exits 468 to 400), 60 to 64 exits for 85 on eight levels the rest. A shell that
  loses half its pellets is still worth more than a pistol, and holding fire and changing
  guns is paid for in every fight. One run had 378 changes to the pistol and 327 back.
- **Every suspected wall ranked after new ground that's put off**
  (`walls-after-new-ground.patch`): -0.0397, exits 468 to 408. Far too wide: it moved walls
  behind everything on every level.

And looked up, since it was nearly "fixed": **the yard past the red bars is a real soft
lock.** The door at its south end opens for line 1366, sixteen units inside the bars; with
the bars shut her middle can't get past the line (the box stops against the posts with its
middle on it, and on the line counts as this side, P_PointOnLineSide). Jumped into without
the key there's no way out, as the maintainer said. A last resort of walking every bit of
floor she hasn't stood on was written for it and taken out again: it changes nothing there.

Still open on MAP14: from far off the planner has no plan to the courtyard (its north gate
she knows only by a line across the yard); the shells through the grate; a sidestep for a
shot that presses on a wall.

### MAP14 watched again: the cover spot, the lift faced in rock (2026-11-20)

The maintainer went through a whole run (seed 145201490) with times. Traced from them:

- **Retreats only to spots she can stand on** (`9c9c527`). The "pressing on a wall" at
  1,212 s was a retreat to cover at the middle of a sliver poly, 12 nearer the wall than her
  half-width: ten seconds against it. -0.0035 (s.e. 0.0039), exits 467 to 461, stalls 1 to
  0; inside the noise, committed on the maintainer's word as a fix of what she does.
- **A block that looks like a lift is one to try, and in a pit doesn't cut her off.** The
  fifteen minutes from the blue key to its door: the door was first in line the tic she had
  the key, and refused, rightly by what she knew. Its one way was a jump over a pool whose
  only way out is a lift faced in rock (every other lift of the level is in metal), so a
  fall short was a pit for good; and each refusal put the door off twice as long. The look
  that tells a player: a face too high to step up, of a stone unlike the walls either side,
  under a small floor level with one beside it. 62 such faces in the 68 levels, 13 lifts, 6
  worked from the face; only a line that does something is ever offered (as with marked
  faces), so the rest cost nothing. -0.0004 (s.e. 0.0031), exits 461 to 461. MAP14 on ten
  lives: 2,282, 2,534 s to 1,818, 1,569 s.

Tried and not kept:

- **What's put off for want of a route, offered again from other ground**
  (`shelved-offered-again.patch`): -0.0065 for everything (exits 467 to 457), -0.0046 for
  the exit and keyed doors only (459, and a MAP14 stall). The lift was the fault, not the
  putting off.
- **The lift's look in `looks_interactive` itself**: -0.0025, E4M7 five exits down. The
  wall search skips what looks interactive, so every lookalike step and crate was dropped
  from the walls she'd try. The look goes in `interactables` only.

Found and left: the "sidestep that presses on a wall" is a tug of war, not a wall (a step
for a bullet's line with plasma up that the ball won't follow, the route taking her back;
or a step worked out afresh each tic at something walking behind posts), each bout ended
by the 20 s patience. And the red-bar yard ended a MAP14 run in four of the day's batches:
she tries its door's line, notes that nothing came of it, and jumps in again a life later.

### What she takes up next: by the walk, what she needs counted nearer (2026-11-20)

The maintainer's rule: going anywhere is risk, so what to go for next is what she needs
together with how long the walk is, never need alone. Traced from MAP14 (seed 145201490,
679 s): the red skull 55 off beside the plasma rifle she'd just picked up, passed over for
a cell pack 519 off that she "needed", because what's needed came before everything else
whatever the walk; that walk took her across the level and she had the key six and a half
minutes later.

- **One list** (`Objective::tier`, `nearer`): the exit first and what would cut her off
  last, as before; between them everything by the walk, with what she needs (what can't
  wait, what she's short of, a keyed door she has the key to, a key) counted as a quarter
  of its walk (`NEEDED_NEARER`).
- **Ways are still looked for only as far as the first needed thing that has one.** A way
  looked for to each of the four compared, those with none are put off, and that alone
  cost 16 exits of 461 on the old score with the order untouched.
- Known and left: something to look at from where she stands is a walk of nothing and
  comes first, a second of standing each time.

Measured a new way, the maintainer's (the old score, one death a run, is given up: three
knife-edge levels, E3M7, E4M7 and E4M8, decided it whatever was changed): all 68 levels,
HMP and UV, 4 seeds, **three lives**, 544 runs a build (`out/eval3/`). Against the tiers:
exits 293 to 298; progress (1 for an exit, else the share of sectors seen) 0.805 to 0.803;
damage dealt over monster health 0.769 to 0.773; deaths a run 1.08 to 1.10; game time 555
to 530 s (-25, s.e. 14). At a tenth: exits 291, the rest the same, and two runs stood out
the hour, both stranded on the last life (E3M9 past the rocket launcher, MAP29), which
the old score would have begun again: the kind it had 12 of in 1,088 on every build, not a
loop of the ordering's.

Tried on the way and not kept: any key she's seen ranked with what she needs (-0.0106 on
the old score, exits 461 to 445); a key within 256 of her only (`key-at-hand.patch`:
level, and by the crow's flight, which the maintainer rightly wouldn't have).

### The measure changed: three lives, not one death (2026-11-20)

The maintainer's decision. A change to how she plays is judged by `tools/lives/run.sh`: all
68 levels, HMP and UV, four fixed seeds, three lives; exits, progress through the level,
damage dealt over monster health, deaths, time, and the runs that stood out the hour
(docs/TESTING.md). The one-death score is retired, its program and scripts kept. Why: with
one death a run, E3M7, E4M7 and E4M8 (a Baron at arm's length, a Cyberdemon's rocket)
turned on any reshuffle, the control with the order untouched as much as the change, and
carried the verdict; three lives is also nearer how she plays on stream. Still to decide: a
level she's stranded in on her last life stands out its hour here, where the old score began
it again.

### A walked line goes off as the game has it, not as her middle crosses it (2026-11-20)

From the maintainer's watching of a run stranded on E3M9 (the pad past the rocket launcher
that drops the eight walls of the last yard; he suspected the pad hadn't gone off).

- **The mechanic** (P_TryMove, PIT_CheckLine, P_PointOnLineSide): a walk-over line goes off
  when her box, where she ends up, is on the line and her middle has changed sides of the
  line carried on for ever. So passing a line's end within her half-width sets it off, her
  middle never over it. She noted a crossing only when her middle went over the segment.
- **What it cost.** Come at by its corner, the pad's east side went off a tic before her
  middle went over its south side. Put down to the south side, the walls had begun "before"
  it: busy, then, and nothing moved after, so "it didn't take", the crossing struck out,
  and what the pad does never learned in three lives, the walls coming down in front of her.
  Third life: the pad's floor walked already, the walls up again, nothing left to do.
- **Now** `walked_over` (knowledge/model/learning.rs) is the game's rule.

Three-lives test against 5b68061: exits 298 to 294 (-0.007, s.e. 0.009), progress 0.803 to
0.802, damage 0.773 to 0.771; one run stood out the hour (E3M2 UV: out of ammo, a Spectre
she can't get at, "keeping off" and "stepping for a shot" by turns from 546 s on: the
stepping loop, an old fault the reshuffle walked into, to be mended next). On E3M9 itself
the four seeds went 6 exits of 8 to 2, and a story was built on it (a pause to look round
that had drawn the Cyberdemon off); the maintainer asked for a real sample, and on 16 fresh
seeds a skill with five lives it is 22 of 32 to 23, the stranding gone. **Eight runs of one
level are no evidence.**

Tried on the way and not kept:

- **Walking over such a line again in the next life** (`walked-again.patch`): level, and not
  needed once she learns the line.
- **A look round when more moved than she saw move** (`look-round-partly.patch`): exits 298
  to 291, four runs stood out the hour, E3M9 18 of 32 against 22.

Written and not yet measured: **anything untried, whatever it costs, when there is nothing
left but to stand** (`anything-whatever.patch`), from MAP29: at 24 health on a ledge in the
lava, every wall left 2 to 12 of her health against a margin of 20, 49 minutes stood.

### Keeping off has its patience, as backing off and stepping have (2026-11-20)

The run the crossing fix shuffled into an hour stood (E3M2 UV, seed 1008128369): nothing
left but the launcher and ten rockets, a Spectre 150 to 270 off that wasn't coming for her,
and a room too small to get a rocket's distance from it. "Keeping off from Spectre" 590
times, the route's legs taken every few tics, each errand given up as taking too long, from
546 s on. `backing_off` and `stepping_for_shot` each give a monster up after
`engage.close_give_up` at it with it none the worse; `keeping_off` had no end. It has the
same now (`Effort::KeptOff`, `steer.no_keep`, left twice as long each time).

Three-lives test against c8173fe: three runs of 544 moved. Exits 294 to 293 (MAP17 one
fewer), stood out the hour 1 to 0, the rest the same to the third place.

The other stepping loops (MAP14: a step for a bullet's line with plasma up that the ball
won't follow; a step worked out afresh each tic at something walking behind posts) are
bounded by the patience they have, 20 s a bout, and are still as they were.

### Off a floor that's going up, onto a ledge it passes (2026-11-20)

The other contributor's rising-platform patch, from its MAP20 and E1M5 experiments, ported
as it stood (the patch and its handoff note weren't kept in the repository). The
finder looks again at each floor that goes up under her, at the heights of the fixed ledges
near it; such a jump is planned only while she's riding and feels the floor at or past
where there's time to line up, and taken when the floor will be at the ledge's height as
she reaches the edge, else given up ("takeoff too low") and planned again.

- One change: which floors "go up" was the word "raise" in a line's description, at the
  place it was asked. It's `LineSpecial::raises_floor` now, by the specials table, Boom's
  generalized "floor move up" with it. (A first try, "a line that leaves the floor higher
  than it began", cut E1M5 from 37 such jumps to 3 and MAP20 from 12 to 2: those floors
  begin up, are lowered, and come back. The table's wording is the only place a floor's way
  is written down.)
- 834 such jumps in 22 of the 68 levels (MAP09 212, MAP17 97, E4M6 77, MAP14 73, E4M8 69).
  First load of all 68: 75 s of processor to 112, kept after.
- Three-lives test against 20805fd: 9 runs of 544 moved, E1M5's eight (all exits still) and
  one MAP09 run that now exits. Exits 293 to 294, nothing else past the third place.
- On E1M5 UV she goes for the blue armor by it on three seeds of four and lands on two; on
  the third the floor was past and it was given up. On MAP20 from a pistol start she is
  dead in the first room before the cell pack is any use to her (3 to 12 kills): the patch
  reaches it, and she doesn't live to want it.
- Left as it came: it reckons when to line up by a floor rising one unit a tic, and lifts go
  four and eight; on a fast one she has less time and may miss. And she takes such a jump
  only when already riding: no trip to a lift is planned for the sake of one.

### The other contributor's pilots, each on the three-lives test (2026-11-20)

Its archive (not kept in the repository) has one finished patch, the rising platform (above), and
small pilot diffs behind its experiments. Each was applied to main and tested alone,
against 8992745:

| Pilot | Exits (294) | Damage | Stood | Runs moved | Kept |
|---|---|---|---|---|---|
| Arch-vile, no shadow and no shelter: to the nearest floor out of its sight | 296 (MAP27 +2) | +0.0017 | 0 | ~30 | yes |
| Arch-vile, a hiding spot chosen before this attack began: chosen again | 294 | +0.0004 | 0 | ~10 | no: nothing by itself, nothing added to the first |
| Lost souls left for the route when nothing else is against her | 293 | -0.0039 | 0 | ~220 | no: UV two exits down, no gain anywhere |

Its own verdict on the fight experiments stands: making monsters fight each other can be
done, turning that into exits "remains unsolved" (E3M8 1 of 160 to 2 with its patient
policy). Still to try from them: the Spider Mastermind set on any durable monster as it is
on a Cyberdemon (`boss-set-on-heavy.patch`), judged on E3M8, E2M8 and MAP20 with 16 seeds.

### Nothing left but to stand: what was refused for her health, whatever it costs (2026-11-20)

The maintainer's rule, from MAP29 watched (down into the lava and out onto a ledge at 24
health; every wall left to try 2 to 12 of her health against a margin of 20; 56 refusals,
then 49 minutes stood on her last life): better she tries what she doesn't know and dies of
it, knowing then what it was, than stands.

- `anything_whatever`, the last resort of all: stood three seconds with nothing, what was
  turned down for want of health this life (`health_refused`) is let back in, and only
  that, the walls she suspects and then any wall, the health reckoning lifted.
- It took four goes, and the lesson is in them. It first let back in everything she'd ever
  given up, every tic (a run that took half an hour of the machine); then once for what she
  knew (no better: the slowness wasn't in finding nothing); then only what was refused that
  tic (it never fired: by three seconds on, nothing had been refused "this tic"); then
  everything again, once a go. Each mended the slowness seen and none asked why it was
  firing on MAP22 at all, where she stood for another reason: its only test was that she
  had ever been refused anything for her health. Traced, the mend was plain: keep the list
  of what was refused for health, and act on that list alone.
- On the MAP29 run (on the build it happened on): "nothing left to do" at 693 s, two walls
  tried from 696 s, dead of the lava at 702 s. 49 minutes of standing gone.
- Three-lives test against c01b58c: six runs of 544 moved, exits 296 to 294 (MAP12 and
  MAP22 one each), nothing stood out the hour either way. In both lost exits the life went
  either way: without the resort she stood till a monster had her (MAP22, 85 s) or till
  the run began the level again as stranded (MAP12, 60 s); with it she died trying a wall.
  What differed was the life after. So with lives in hand it buys nothing but the chance of
  what a wall turns out to be; on a last life, or on stream where she isn't begun again, it
  is a death in place of an hour.

### E3M8: what she needs to come in with (2026-11-20)

The maintainer's experiment. Ultra-Violence, 100 health and 100 green armor, 16 seeds a
loadout, three lives (each begun with the loadout again); `out/lives/e3m8.sh`. A backpack's
worth is what one gives: 10 bullets, 4 shells, 1 rocket, 20 cells.

| Loadout | Exits | Without dying | Lives lost a run |
|---|---|---|---|
| Pistol start, no armor | 0 of 16 | 0 | 2.00 |
| Every gun but the BFG, 2 backpacks | 0 | 0 | 2.00 |
| ... 4 backpacks | 0 | 0 | 2.00 |
| ... 6 backpacks | 6 | 2 | 1.56 |
| ... 8 backpacks | 13 | 5 | 1.00 |
| The BFG alone, 40 cells | 1 | 0 | 1.94 |
| ... 80 cells | 1 | 0 | 2.00 |
| ... 160 cells | 14 | 10 | 0.56 |
| ... 320 cells | 16 | 16 | 0.00 (16 s of game) |

The level is lost on what she brings, not on how she fights it: no win below six backpacks'
worth with ordinary guns, nor below four BFG shots. And on the boss levels with 16 seeds a
skill and three lives she stands at E3M8 0 of 32, E2M8 25 of 32, MAP20 0 of 32 from a
pistol start and 1 of 32 with what she'd carry from MAP19. The Spider Mastermind set on any
durable monster as it is on a Cyberdemon (`boss-set-on-heavy.patch`) won E3M8 twice in 32
and lost E4M8 twice on the standard test, with less damage dealt on the boss levels over
all (-0.020, s.e. 0.007): not kept.

### A line she can't walk over from where she stands is no way from there (2026-11-20)

MAP14's yard past the red bars again, this time as an hour stood on main on her first life
(UV, seed 145201490): the door out opens for a walk-over line sixteen inside the bars, and
with the bars shut her body stops short of it. Every errand she had went by that line; each
go was 20 seconds and "took too long", and the next went the same way: 471 goes, never once
"nothing left to do", so never begun again as stranded. (That the yard is a soft lock without
the red key was settled on the 19th; what was new is that she didn't know it.)

- The follower says which line it gave up crossing; three times and the explorer keeps the
  ground she could walk to from there, and the line's "walked over from" loses that ground
  in every plan (`build_view`), this life and after, till she does walk over it.
- On the run: 3 goes for 471; the first life stranded and begun again inside two minutes;
  the next two don't jump into the yard at all.
- MAP14 on 16 seeds a skill, three lives: exits 21 of 32 to 26 (UV 9 to 14), stood out the
  hour 4 to 0, progress 0.943 to 0.978. Standard test against 074d809: three runs moved,
  exits 294 to 293 (one of MAP14's eight), nothing stood out the hour.

Tried beside it and not kept: **plasma put down where its ball is stopped short and a bullet
isn't** (`plasma-put-down.patch`), for the stepping loops left on MAP14. On four seeds she
changed guns half as often again or more (72 for 46, 35 for 15) and was held from firing as
much. The fourth rule about changing guns in a fight to lose this fortnight.

### Nothing left but to stand, and something she knows of alive: after it (2026-11-20)

E2M8, HMP, her third life (seed 1418424326; two of the boss test's 32 E2M8 runs): the
Cyberdemon the far side of the arena and not coming for her, given up as out of reach, the
walls pressed, "nothing left to do" at 794 s and stood to the hour. Going to where she last
saw a monster is a last resort she has, but an errand by something she'd not take on is
passed over as dangerous, and on a level that ends with its death that was the one errand
left. `hunt_whatever`, after every other resort: stood three seconds with nothing, the hunt
is taken dangerous or not (`dare_hunts`). On the run: after it eight times, dead at 887 s
with 87% of its health off it.

Boss levels (128 runs): stood out the hour 2 to 0, nothing else moved. Standard test
against e2a242e: no run's outcome moved.

With this, the uncrossable line and what her health refused, a run stands out the hour in
none of the standard test's 544, none of MAP14's 32 and none of the boss levels' 128.

### A door by its shape alone, tried and left (2026-11-20)

From the textures plan's side finding: `door_shaped` wants a thin shut sector *and* a face
unlike the walls beside it, and on Eviternity II the second half halves the doors it finds
(102 of 231 for 197). Without it, on the standard test against eb1bea3: she sees more of
the levels (+0.0046, s.e. 0.0021) and takes longer over them (+17 s, s.e. 6); exits 293 to
291 with E4M7 five down and five other levels up. More doors is more ground and more
walking, and on the IWADs that buys nothing. Left as it is for them
(`door-shape-no-odd.patch`); for a custom WAD, where the list of door textures is the gap,
it belongs with the per-WAD list in custom-textures-plan.md.

### The ultra instinct merged, and what it's worth from the start (2026-11-21)

The three-lives test through the watcher with no window, the ultra instinct off and hers
from a level's start (30 s of it, got back as she plays; `--ultra-after 0`): exits 294 to
401 of 544, progress 0.801 to 0.905, kills 0.584 to 0.745, lives lost a run 1.09 to 0.64;
it came on 12 times a run and a run took 97 s of real time for 37. Levels never exited 12
to 5. Two faults found by the test and mended before the merge: the quick way of putting a
game back left the game's lists of moving lifts and ceilings pointing at freed thinkers
(a crash on E2M3: `evil_level_again` empties them); and a game put back took back the game
but not what she believed she'd done, so a once-only switch pressed in the seconds undone
was used up as she knew it and unpressed in the game (MAP21's ring, stood the hour:
`Knowledge::undone`). Five runs still stand the hour with it on, one of them with it off
too. Tried and not kept: on past the moment she died (no evidence in eight runs); MAP23's
barrel hall, where a death undone five seconds isn't undone far enough, is left as too
hard.

### The code review of 2026-11-21, carried out (2026-11-21)

[code-review-2026-11-21.md](../code-review-2026-11-21.md) has what was found, what was
done and what's left, finding by finding. In short: the bridge checks it matches the
program; no two modules depend on each other; the four largest files are each split by
their jobs; the 2024 edition; and two faults that the new each-way check found on its
first run, both in what a test measures. A level's first run in a fresh folder played on
jumps a last bit off from every later run's (the jumps are kept as text, and text read
back isn't always the number written). And the watcher listed her options afresh once a
second for its map, which changed what she chose next: a run watched was not the run
measured. `tools/eachway.sh`, `tools/demos.sh` and `tools/dev/cycles.py` are the checks
that came of it.

### Her movement by keys, and a run as a demo (2026-11-21)

The maintainer's: a mode in which she's moved only by what the game takes from a keyboard,
within what a hand does, so that a run of hers is something a person could have played and
can be a Doom demo, played back by DSDA-Doom itself. Expected to cost something; worth it
in the long run.

**A demo.** `run-map --demo FILE`: the game's own recording, begun as the level begins
(`evil_record`). An MBF21 demo, which is what the game plays as. Nine runs recorded and
played back by the `dsda-doom` program come out the same to the second, the kill and the
item (`tools/demos.sh`). It needs nothing of the keys: any run of hers on one life is a
demo. What a demo may hold that a hand couldn't make is the push: any size, any direction.

**The keys.** `--keys` (`motor/keys.rs`, `tuning.hand`): each tic's push made the nearest
the five keys give, forward or back at 25 or 50 and sideways at 24 or 40, run choosing
which for both. A key is down two tics at least and up three before it's pressed again;
what's asked and not given is owed and asked for again, a share a tic over a press's
length. Her turning is left: that's the mouse, already kept to what a hand does. Fire, use
and the weapon keys were keys already (a ticcmd names the gun, as the game's own key
handling resolves it).

The three-lives test (544 runs, this build without it 293 exits): the first of it 241; a
key timed by itself, so forward to back is one motion, 251; debts spread over a press 271,
which is what's kept. 22 exits of 293, two runs standing the hour, her killing as it was.

Her hand's speed, each on the same 544: a press three tics at least 259; one tic, and up two,
275 (four more, inside the noise, for a hand quicker than most); a key's change costing
nothing 261. Left at two down, three up.

**Tried and dropped.** By keys, going only where she faces and steering by the turn: 209.
Her turn is eased over many tics, far slower than a hand on a mouse, and she walked the
route wide. If it's tried again it's with a turn as quick as a hand's.

**What the keys cost her, where it shows.** Fine positioning: a tap is a dozen units and
more, so a spot to stand on is hunted back and forth, and a narrow ledge over a floor that
hurts is where it kills (MAP25's first walk). The Icon of Sin's lift (MAP30: 5 of 8 exits
lost): riding it to the height and firing on time. Nothing of hers has been rewritten for
keys yet; the follower still asks for pushes as if any could be had.

**Not yet.** A demo of more than one life (a player dead presses use, and the level begins
again inside the demo; hers begins it another way). Recording from the watcher. The
follower's reaching and stopping written for taps.

## A floor seen to change is costed by what it changed into (2026-11-21)

Doom II MAP24, the maintainer watching: the pit round the rocket platform risen level with
the ledges, and she still walked the eight-wide ledge round it and fell off its corner,
time after time. She knew its height and had it as "safe now" (`Knowledge::changed_floor`);
the path search took "does this floor hurt" from the map as it starts, and a way over a
floor that hurts counts four times its length. Now the search is given the floors she's
seen change and whether each hurts (`View::set_changed_floors`, part of the view's key):
either way, risen into plain floor or lowered into slime. Only what a way costs; what she
can reach is as it was.

The three-lives test, 544 runs: exits 293 -> 295, lives lost a run 1.110 -> 1.105, damage
0.771 -> 0.772, 527 s -> 529 s, none stood the hour in either.

## Ammo for a gun she hasn't yet (2026-11-21)

MAP24, the maintainer watching: two rockets a couple of steps off left lying, she with
only the pistol. Ammo was wanted only for a gun she holds. "There's no reason not to go for
ammo for a gun you don't have yet": it's now needed at half what the same ammo is for a gun
she has (`errands.spare_ammo`), which keeps it under `needed` (it waits its turn behind
keys, doors and guns) and inside the usual range for things lying about. Mid-fight she
still turns aside only for what she can fire, and stocking up before an exit is as it was.

The three-lives test, 544 runs: exits 295 -> 292 (34 runs more, 37 less: it changes most
runs a little), lives lost a run 1.105 -> 1.096, damage 0.772 -> 0.771, 529 s -> 542 s.
Two runs stood the hour that didn't (MAP14, UV, seeds 854202743 and 205498993), not yet
traced. Kept on the maintainer's word, for how she plays; the stands are owed a trace.

## A gun before a key, with only the pistol (2026-11-21)

MAP24 on HMP, the maintainer watching: life after life straight to the blue key in a room
of imps, the shotgun known and left. A key and anything she needs each count a quarter of
their way (`Objective::nearer`), and the nearer is taken: that rule was made for ammo
against a key (MAP14), never for her only real gun against one. Now, while her firepower is
no better than the pistol's (`errands.pistol_only`) and there's a better gun to go for
(lying where she can get to, or carried by something she can), a key counts its whole way
(`errands.key_unarmed`). Any gun that does more than what she has; not the chainsaw.

The three-lives test, 544 runs: exits 292 -> 289 (3 runs more, 6 less), lives lost a run
1.096 -> 1.097, damage as it was, 542 s -> 537 s, stood the hour 2 -> 1. No evidence either
way; kept on the maintainer's word ("pistol alone is almost never enough").

## A run-jump costs a little more than its length (2026-11-21)

The maintainer watching: she leaps gaps the floor beside them crosses. MAP24's shotgun,
on a platform at the end of a spur she can walk, jumped for from the spur's foot (and
landed: the jump wasn't failing, only needless); MAP27's steps to the rocket launcher
hopped corner to corner over the nukage. A jump she's sure of was costed as its straight
length and nothing more, so it won any near-tie with the walk. "Humans don't randomly jump
out a window just to go somewhere."

Every run-jump now costs `JUMP_RISK`, 16 units, over its length: a tie-breaker. It changes
only which way is taken; an objective's distance for ranking is the walked length as before.

Measured on the three-lives test (544 runs), each on the build with the two changes above
and no jump cost (289 exits), with her run-jumps counted over the same runs:

    cost    exits   run-jumps (a run)   not landed
    0       289     2250 (4.1)          129
    16      284     1911 (3.5)          117
    32      281     1733 (3.2)          116
    64      278
    128     278
    256     276
    32 and half the leap's length       277
    256 over a pit, none for a fall     277

The more it costs the fewer exits, with no size at which it's free, and neither blending
by length nor sparing falls helped. At 16: 3 runs more, 8 less, each on a different level.
Half of all her jumps are on six levels; MAP14 alone has 279 of them and no cost of this
size touches those (to be traced).

What the lost exits are, where traced: not worse movement. MAP27 (HMP, 854202743): without
the cost her hops in the first room take her early into the room with the invulnerability,
whose only way back she knows is the nukage and the lift's switch, and that keeps her from
the yellow skull's library for four minutes; with it she's at the library in two and dies
there to what guards it. The jumps were putting off a fight she loses, and the test counts
the putting off as progress. MAP25 (UV): the walk taken for a hop was a wade through the
blood. So the cost is kept small and taken for how she plays, on the maintainer's word,
not for the score.

Not done: a cost on plain drops off a ledge (none now), and a way back that needs a floor
that hurts still counts as a way back.

The cost of 16 by itself, without the two changes before it (jump16 against fix1-lava,
the same 544): exits 295 -> 295 (9 runs more, 9 less; MAP27 -2), lives lost a run 1.105 ->
1.101, 529 s -> 542 s, and two runs stood the hour that didn't (E4M6 HMP 1008128369, MAP06
HMP 854202743). So its five exits on the other build are within what one set of seeds
tells; the stands are what to look at.

## Invulnerability kept for the fight that killed her (2026-11-21)

The maintainer's idea, from MAP27: on a later life she knows where the sphere is and where
the arch-vile is; get the one, then go to the other. She used to take a sphere whenever
something dangerous was awake anywhere, and then walk off with it to a box of shells.

What she does now (`Fight::sphere_use`, `sphere_death`, `Player::revealed_by`):

- **A death remembers what brought it on.** Of what she had in sight in her last ten
  seconds, the one that matters most (`Monster::threat`), and the last line she set off
  (a switch, a door, a line walked over) no more than five seconds before she first saw
  it (`deaths.reveal_secs`). MAP27: "use line 149", the bookcase door, the arch-vile seen
  four tics after.
- **The sphere is for that.** Of her deaths this level that a sphere would have mended
  (not want of a gun, nor a floor), the one behind whose line the worst thing stood; she
  doesn't take a sphere for anything else meanwhile, unless outmatched or surrounded with
  them in sight.
- **Fetched as her way turns to it.** When the route she's on will set that line off
  again (a step's line, or a door it opens that the line moves), and she knows a sphere
  within fifteen seconds' run of it (`errands.sphere_secs`), the sphere comes first; once
  a life, and given a minute.
- **Then straight there.** With it on she goes to the line, the route keeping her legs
  through whatever's between, shooting only what's ahead: its thirty seconds aren't for
  them. There, it's done with for this life.
- **A sphere a dash away** (`errands.sphere_dash`) is run for through what's by it, with
  no falling back on the way, and is worth being shut in for: for the sphere a level puts
  in the room with what it's for.

Tried on the way and dropped: fetching it only when near the line (MAP27's library door
doesn't open from inside: she stood at it till the errand timed out); going by the errand
she had when she died (the door is a step inside "pick up YELLOW_SKULL", never an errand);
taking the latest death (a chaingunner's door, and the sphere spent on that).

MAP27, the test's eight runs: with three lives none out either way, but half the level
seen in four runs where it was none, the arch-vile dead in three within 5 to 22 s of the
door; with six lives, out 2 of 8 for 1, and over half seen in 7 for 2. Not yet good: the
sphere inside the arch-vile's room has a hell knight between it and the door, and the one
dash for it seen ended in 1.7 s.

The three-lives test, 544 runs: exits 284 -> 286 (6 runs more, 4 less), lives lost a run
1.107 -> 1.103, damage 0.768 -> 0.767, 536 s -> 530 s, stood the hour 2 -> 1. 73 runs
differ, on the levels with spheres: MAP27's lines seen 0.28 -> 0.43, MAP21 and MAP10 an
exit more each, MAP28 one less.

**A sphere isn't carried off across the level (the same day).** The levels the rule changed,
played with ten lives (17 levels, 136 runs), showed it at fault on Doom E3M9: a death tied
to a baron's line at the far end; every life after, a sphere taken where she stood for the
plight she was in, and then run 8,300 units of doors, lifts and a teleporter for that line,
out before she was half way, and killed at the same place seven lives running (exits 5 of 8
without the rule, 3 with). The run's reach was checked from the sphere when choosing to
fetch one, never from her when she had it. Now, with it on, she goes for the old fight only
if it's within `sphere_secs` of where she stands, else it's used where she is; and a death
she's twice set off for with a sphere is one a sphere doesn't mend (`CHARGES`).

Ten lives, those 136 runs, without the rule, as first written, as mended: exits 58, 55, 57;
level seen 0.777, 0.784, 0.785; stood the hour 2, 0, 0. E3M9 5, 3, 6 exits; MAP27 1, 2, 2
(seen 0.53, 0.73, 0.73). Still the worse for it with ten lives: MAP18 (3 -> 2 exits, seen
0.78 -> 0.71), E3M4, MAP21 and MAP28 an exit each; not traced. The three-lives test, 544
runs: exits 284 -> 287 (6 runs more, 3 less), stood the hour 2 -> 1.

## The berserk fist: kept at what she's gone at, and for what only claws (2026-11-21)

Doom E3M4, the maintainer watching: a baron in a pit, and she by turns on the fist and the
chaingun, four switches in seven seconds and hardly a blow or a shot between, dead from 200.
The fist berserk is for what she'd go up to (`bestiary::melee_at`: 400 health and under; a
baron costs some 330 to punch down), but it also comes up for anything that has come right
up to her, and from what claws she backs off: in reach the fist, out of it a gun, each
switch a second with neither.

Two rules in `best_gun`, each a setting (`tuning.guns`), both on:

- `fist_commit`: the fist chosen for something in reach is kept, and she at it (the melee
  node), till it's dead or `fist_let_go` (192) off.
- `fist_for_melee` (the maintainer's, "common for good human players"): with nothing on her
  but what only claws or bites, the fist, and the rounds kept.

Both under the guards the fist already had: not with her health under `engage.low_health`,
nor at what one claw of could kill her. A third, `fist_punchable_only` (no berserk fist at
all for what she wouldn't go up to, a gun to hand), is the other way to make her legs and
her hand agree; it changes next to nothing and is off.

The three-lives test, 544 runs, each against the build before (287 exits). The same build
with all three off differed from it in 5 runs (288 exits): that's what the machine's load
does, and one exit either way is nothing.

    rule                 runs changed   exits   more/less   lives lost a run   secs
    none                 5              288     2/1         1.103              532
    fist_commit          76             288     10/9        1.088              524
    fist_punchable_only  10             287     2/2         1.103              532
    fist_for_melee       95             287     8/8         1.116              530
    commit and melee     110            287     10/10       1.110              524

Neither shown better. The commit is in for the fault it mends; the other on the maintainer's
word, for what it saves: rounds, which this test doesn't count and three lives from a pistol
seldom run short of for demons. Berserk ends with the level, the rounds don't: an episode,
or a custom WAD with more of both, is where it would tell.

## Plasma that can't get there: left in two seconds, not twenty (2026-11-21)

Doom II MAP14, the maintainer watching (HMP, 205498993, 645 s and again 677 s): to and fro
across 150 units for twenty seconds, twice. A shotgunner behind the red bars, the plasma
in her hand and her own check saying it wouldn't get there (the gaps are narrower than
the ball); with no shot she went stepping for one, back to her route, standing to fight,
some ninety times, till `engage.close_give_up` left it "a while"; then the next of the
row behind the same bars. Where there's no step to anywhere it would get there
(`plasma_spot`), there's nothing to wait for: `engage.blocked_give_up`, two seconds.

Tried first and dropped: another gun for it while the plasma's blocked. Her target changing
among them, some in reach of it and some not, it was plasma and shotgun by turns every
second or two.

That run: the stepping and standing between 640 and 700 s, 324 lines to 13. The three-lives
test, 544 runs: exits 287 -> 288 (2 runs more, 1 less), 37 runs differ, and the one run
that has stood the hour all day (MAP14, UV, 205498993) is out in 1,323 s, no life lost.

## A walk-over line her centre can't get across (2026-11-22)

Doom II MAP14: an hour stood, "nothing left to do". Two run-jumps along the ledges and a
drop of 32 into a pocket of the courtyard shut in by ledges, the red bars and a door, on the
strength of a way out: walk over line 1366, which opens the door. It can't be walked over
from in there. A walk-over line goes off when her centre crosses it, the bars begin 16
units past it, and her box meets them with her centre on the line and not over. It's
there to be crossed the other way, the bars lowered. ("could not cross line 1366" three
times, the line given up from that ground, the walls pressed, and nothing.)

The planner knew this of a line between two sectors (`View::cross_sides`: the far one
must be one she can step into) and passed over a line with the one sector both sides. Now,
as the level's analysed, each such line is tried just across on either side all along its
length (`analysis::walk_far_sides`): where something's under her box wherever she tries,
the lowest such sector is what the far side is, and `cross_sides` holds her to it like any
other. One line in MAP14 of the six levels looked at, none in the other five.

The three-lives test, 544 runs: only MAP14's eight differ; exits 288 -> 289.

Left: stood with nothing to do, a watched run still stands (the test begins the level
again after a minute, at a life's cost).

(The first test of this was no test: its name, `cross1`, was a folder of two days before,
and `run.sh` plays no run whose row is on file. Names carry the date since, and `run.sh`
keeps the build's hash by its rows and refuses another's.)

## Where a switch is pressed from (2026-11-22)

Doom II MAP14, the maintainer watching: a lift's face pressed three times to no effect
(152 s), then forty seconds pushed against a parapet 12 short of the spot she'd been sent
to press its other faces from (180 to 220 s).

Two things wrong, both in how a use line's ground is judged:

- **From where a press gets there** was tried from the poly's middle or its nearest corner
  (`use_gates_on` from `near` and `c`), which can be 170 units from a line her press reaches
  64 to. A yard counted as ground to press the lift from whose only spot within reach has
  a block of wall between. Now a poly counts only if it has a spot her box can stand at
  from which the trace reaches the part of the line she'd aim at (`press_point_in`: within
  62, nothing between that never opens), and the spots are kept (`Navigator::press_spots`).
- **Where she stands** was 20 units out from the line's middle wherever that fell (here the
  8-wide sill of a door beside the lift), or the poly's nearest point. Now the spot in
  front is taken only if her box fits there; else one of the kept spots.

Got wrong twice on the way, each caught by the three-lives test and the counts beside it:

- *The kept spot nearest the line, always.* Nearest is beside the line's end, in line with
  its face: MAP19 and E4M1, switches that had worked given up after three presses. The
  spot in front stays first; a kept spot must be square to the line (30 degrees off its
  face) and within 54, for the few units she stops short by.
- *Those two held to for "can it be pressed from here" as well.* E3M6's secret exit is
  pressed from the floor below it, 60 off and at an angle, and it works: three exits lost
  for not knowing it. The two questions have their own tests: 62 and nothing between for
  whether, 54 and square for where; and where no spot is square and near, one that
  reaches.

Each of the 544 runs replayed with its presses and its "stuck" lines counted:

                     exits   given up after 3   stuck   timed out
    before           289     21                 2564    214
    nearest spot     287     16                 2220    197
    held to both     288     15                 2303    228
    as kept          291     26                 2270    194

As kept, 60 runs differ; exits 289 -> 291 (5 more, 3 less), lives lost a run 1.107 -> 1.096.
The stuck lines are MAP14's (366 -> 57; the rest 2198 -> 2213). MAP14's presses given up
go 6 -> 9: what she was stuck at for forty seconds she now presses from where it doesn't
quite reach, and leaves in two. Left: a spot that reaches only if she gets right to it,
and a follower that stops short.

**The last units to a switch, and no pressing from under a ledge (the same day).** Two more
of the same family, from MAP14's presses still given up:

- *She stops short.* "There" is within a dozen units of the end of her way, and her way
  keeps clear of walls; a lift's face 59 from the spot she was sent to was pressed from 66.
  `RouteFollower::close_for_press`: with the part of the line she aims at more than 52 off,
  she walks straight at it, 2 units a tic at most, till it's 52 off, or she's got no nearer
  for ten tics (a wall: as near as there is), or the next step would be off her floor.
- *A spot under a ledge isn't one.* `press_point_in` took spots where her box overlaps
  ground more than a step above the poly's floor (`box_on_ledge`): from below she can't be
  there, and from as near as she can it's out of reach. A lift's face "pressed" from the
  yard beside its ledge; a door's switch from a floor 72 under it.

The first alone did little (MAP14's eight runs: 9 given up -> 8): the rest weren't for
want of the last units. Both: 346 presses on those eight runs, none given up, none tried
twice.

The 544 runs: exits 291 -> 288 (4 more, 7 less), lives lost a run 1.096 -> 1.108, 521 s ->
526 s; presses given up 26 -> 13 (MAP14 9 -> 0, E4M3 3 -> 1, MAP29 2 -> 0; E4M1's 7 not
touched, untraced), stuck lines 2270 -> 2332, errands timed out 194 -> 213. Right in what
it does and not shown better on the score; taken on the maintainer's word.

## A switch untried is held to what a switch known is (2026-11-22)

Doom E4M1: a lift's face pressed three times to nothing, in seven runs of eight, from dead
in line with it. Beside the lift stands a block 72 high that a switch brings down later;
between the block and the wall are 8 units, and the furthest she gets towards the spot she
was sent to is in the face's own plane.

The planner knew better (`activators_now`: that ground only once the block's down). But
what lists switches she hasn't tried (`Explorer::untried`) asked only whether the face
shows from that floor (`floors_now`), and offered it; once it's her errand she walks at the
spot and presses from where she fetches up. Now an untried line is held to the same as a
known one: nothing shut between that she can't open, room for her box where she'd stand,
a far side she can step onto; with the heights as she could bring them about with what
she's learned (`states_all`), not only as they stand.

Two seed sets showed what that alone would have cost (MAP05, both sets: 8 runs an exit
less for 2 more). A lift's face at the back of an alcove 16 deep, whose mouth is another
line for the same lift: a press from the room is taken by the mouth's line, so the room
"doesn't reach" the face; that leaves the alcove, where her box is over the lift's own
face, so "not till the lift's down". The line she's there to press isn't a wall to
standing by it (`moving_walls_at`).

The first use of a second seed set (`out/lives/1122-seeds2.txt`), on the maintainer's
word, the switch changes having each come out a few exits down on the one. Exits, first
set and second:

    before any of it (a67d56d)                 289   287 (one stood the hour)
    spots, closing, ledges (8818775)           288   288
    and this, as first written                 284   286
    and this, as kept                          287   287

So the three steps down on the first set were its seeds; and the same two levels losing
on both was not.

E4M1's presses given up: 7 -> 0. Left: E3M4 lost a run on the second set for the list of
doors being grouped otherwise and the other door taking her to the baron's pit, which is
the fight's to mend.

## Plasma not fired through a slot narrower than itself (2026-11-22)

Doom II MAP14, the maintainer watching (570 s): thirteen cells into the red bars, at
shotgunners 260 to 400 off behind them. The bars leave gaps of 24; the ball is 26 wide.
`projectile_reaches` goes by the ball's whole width only past `aim.width_far` (384): nearer
it's the middle alone, on purpose (the whole width everywhere lost E4M8 four exits of
sixteen and E2M9 three, pillars clipped in close fights), and the middle goes through a gap.

The maintainer's: the ball's leftmost and rightmost ways each tried, and held only when
both meet something. One side on a pillar is still a shot; a slot is none, at any range
(`lane_sides`). Bars so thin both edges find gaps would fool it, and are left to.

The three-lives test, 544 runs: exits 287 -> 293 (8 runs more, 2 less), lives lost a run
1.110 -> 1.103, none stood the hour; 63 runs differ. E4M8 +2, MAP25 -2, six others +1.

## A shotgun that isn't landing is put down (2026-11-22)

Doom II MAP14, the maintainer watching (1,020 s): shell after shell at an arachnotron behind
bars, a pellet or two of seven getting through. Trying the fan's ways before firing was
tried earlier and made things a good deal worse (a clipped corner is common, and a fine
shot). The maintainer's instead, after the fact: at something wider than an imp, not a boss,
standing where it stood, `aim.spread_shots` (3) shots that did under `aim.spread_share`
(0.35) of what they should have are a slot; she holds fire at it from there (`no_effect`)
and the fight gives it up. A corner takes one side of the fan and leaves over the share.

MAP14's eight runs: 49 such holds. The three-lives test, 544 runs, against the plasma rule
alone: exits 293 -> 292 (MAP21, one run), 13 runs differ, none stood: level.

## The Neuro layer against the SDK of 11 November, and a model that answers fast (2026-11-22)

A survey of `neuro/` and `control/directed/` against the SDK's specification and best
practices (its copy of 11 November: the dialogue messages and `silent` on a force are new
since the plan was checked), and of the tuning against what `doom_policy` reaches. The layer
was built for a model whose answer takes seconds. The maintainer's word: Neuro can answer in
under half a second, one to two at worst, so the design stands and the fast paths are to be
tried, with what's there now as the fallback. What follows is each change made of it.

### Directed play's clocks are the tuning's (`directed`)

How long she's waited for and how often she's put to were constants in three files
(`session.rs`, `ask.rs`, `directed.rs`), set for a slow stand-in. They're a tuning group now,
`directed`, in seconds: `idle_force` 6, `ask_force` 3, `next_force` 15, `after_look` 3,
`answer_wait` 8, `now_open` 3, `questions_a_minute` 3, `moments_a_minute` 6, `status_every`
20, `tactic` 10, `stance` 30. A `--tuning` file for a fast model and one for a slow stand-in
differ there and nowhere else. The stream console still changes the forcing times and the
answer wait as it runs. The values are as they were: nothing she does changes (parity: 24 of
24 the same).

### One force at a time

The SDK: "Neuro can only handle one action force at a time. Sending an action force while
another one is in progress will cause problems." Ours kept a flag, and three things were
wrong with it. The force for a question left unanswered didn't look at it (an ignored "what
next" and then a real question: two out, 3 s apart). Any action of hers cleared it, one
refused too, which the server puts to her again by itself (ours could be sent on top of its
retry). And nothing ever cleared it but an action: a force the server had dropped (its
retries spent, or a result it gave up on after 20 s; it doesn't tell the game) left her
never forced again.

`Forcing` (`session.rs`): a force is out till one of its own actions is accepted; refused,
it's still out, waited for afresh; unanswered `directed.force_timeout` (30 s), it's taken
as dropped. None is sent while one's out: a question's force waits its turn. Against the
mock server with nobody answering, E1M1: forces at 15.0, 45.4 and 75.4 s, one out at a time
(they'd been every 15 s, on top of each other).

### A game hers to start is forced

The SDK: "Use `actions/force` whenever the game is blocked waiting on her decision. Without
a force she will eventually get distracted and forget about the game." At the title with a
game hers to start (`--choices`), nothing happens till she starts one, and she was only told
so, once. Now, `idle_force` after the offer, a force (medium, `doom_game` alone, the offer
its state), and again whenever the last has gone unanswered its timeout. Timed by the wall's
clock: off a level the game's own doesn't run. The tally screen isn't forced: it goes on by
itself. Against the mock server: one force 6 s after the title's offer, the game started by
its answer.

### "What next" is asked quietly

The SDK's `silent` on a force (documented 10 November): "for frequent or routine decisions
that aren't worth commenting on, so she acts without talking about every one of them".
"Carry on, or something else?" comes up to every 15 s. It goes out `silent: true` now
(`directed.next_silent`; the stream console's "what next asked quietly" turns it off as it
runs, and keeps the choice). The other forces are as they were: a wait, a question and a
game to start are worth her word.

### A question that can't wait is forced, by how much it matters

The plan's class T (a fight turned bad, a costly jump lined up): the default done at once,
her answer switching it within `now_open` (3 s), and "never forced, its moment gone by the
time a force is answered". That was for a model that takes seconds. Neuro answers in under
half a second, one to two at worst (the maintainer): inside the three. So such a question is
forced the tic it's asked, at a priority the asker gives it (`Question::priority`): `high`
(she cuts short what she's saying) for an arch-vile, a boss, outmatched, a jump that costs;
`critical` (she's cut off) only for her health going in a fight. Its actions: `doom_answer`,
and for a fight `doom_fight`, her own say settling it too. Its line goes as silent context
then (the force says it; one prompt, not two).

The fallback is what was there: the default's done whatever she does, a late answer is told
it's too late, and with another force out (one at a time) it's told aloud as before and not
forced. Against the mock server, MAP07 on UV: "Outmatched: 1 mancubus" forced (high) at
3.5 s and 98.8 s; the ones between, with a force out unanswered, told aloud.

### She's heard to be talking

The SDK's `speech_finished` (from her, several a response; `isFinal` true on the last, and on
one cut off) was dropped unread. It's read now: she's talking from a part that isn't final
till the final one, or `directed.speech_stale` (20 s) after the last heard with no end. One
use so far: "what next" isn't asked mid-sentence. At low priority it would wait for her to
finish anyway, and its few seconds for an answer would have run out by then, the ask spent on
nothing; it's asked as she finishes. The urgent forces don't wait: their priority is what
cuts her short. Against the mock server (which sends the message now: `speaking`, `spoken`
in its file): the ask due at 15 s held till she was done at 25.8 s.

### Her own level-scale choices are told her (`plans.rs`)

Since the layer was last worked on the bot has taken to deciding things of the level's scale
by itself on instinct: the invulnerability fetched before she does again what she died
after doing (`errands.sphere_*`), a gun before any key on the pistol
(`errands.key_unarmed`), places left alone for the deaths they've cost (`deaths.*`). The
layer had no words for any of it, and plans of that scale are hers (the maintainer's rule:
level-scale planning belongs to whoever directs her). `Player::plans` gives them as they stand (`Plan`); the
Director tells each as it's taken up:

- **The sphere first:** a question with its default done, "You died to an arch-vile after
  opening door-l149: getting the invulnerability first. a) get it (doing it) b) go
  without", forced at medium; "go without" ends it for this life (`no_sphere_first`). With
  a question open or the minute's spent it's told as a moment.
- **Invulnerable and off with it:** "Invulnerable: straight to what killed you (an
  arch-vile)", or "straight back to where you died".
- **A gun before any key:** "Only the pistol: a gun first, keys after", once a level, and
  only once she's been told of a key.
- **`doom_why`** says each while it stands, and on instinct what's left alone: "Left for
  now: item-t99 (N 1453), died there once".
- **An order of hers** to go to or press what she died after setting off is carried out (it's
  hers), its answer adding "You died to an arch-vile after this last time."

Under an order of hers the sphere rule doesn't run (as before: `interrupt` is instinct's),
which is why the note. MAP27 on UV against the mock server, seed 205498993: the question at
her second go at the library door, the charge told, the lines in `doom_why`. The note on an
order wasn't reached in that run (the door was open by then). The bot alone is as it was:
parity, 24 of 24.

### Gary and `silent`

The first session with a model (Gary's custom build, GLM 5.3 by OpenRouter, E1M1): every
"what next" force came back `prot/invalid_message`, "Unrecognized key: silent", the whole
force thrown away. Gary 1.0.6 is from before the field. Two things: a force that isn't silent
no longer carries the key at all (it was sent as `false` on every force, which would have
lost Gary the idle and question forces too); and `tools/neuro/gary.sh` plays with
`config/gary.json` (`directed.next_silent` false). With it, 75 s of E1M1: four "what next"
forces, each answered by the model in 0.6 to 3 s (a look first, some of them), none on top
of another, no diagnostics.

### `doom_fight` by fields; `doom_policy`'s help out of her context

The SDK: "Keep descriptions to a sentence or two. Longer rules belong in context messages",
and one action with a slightly bigger schema over a chain of them. Two of ours were neither.

**`doom_fight`** was one `do` of ten values with a `ref` that meant a monster for one and a
gun for another: cryptic (the maintainer's word), and a target, a gun and a stance three
actions. It's fields now, any given together (the maintainer's shape): `target` (an id or a
kind, shot first; `none`), `weapon` (a gun she holds; `auto`), `fire` (`hold`, `free`),
`override_stance` (the maintainer's name: it's over her policy's, for a while), `retreat`,
`cancel` (was `auto`: everything of hers dropped), `seconds`. None given answers what's
holding. Each is taken in turn and one that can't be doesn't stop the rest: "Holding fire,
10 s. But: No cyberdemon known of right now. Known now: 2 zombiemen (ids: doom_list
monsters)." A word that won't do is answered with what would (the SDK's rule for a free
string): the kinds she knows of, the guns she holds. The old form still reads (`ref` as
`target`, `gun` as `weapon`, `stance` as `override_stance`: `repair.rs`). GLM 5.3, first
fight with it: `{"fire":"free","target":"zombieman"}`.

**`doom_policy`**'s description carried every value's meaning, 895 characters in her context
the whole game. It's two sentences (116); the meanings are what it answers with nothing
given. Its schema's enums were always there.

### `hints` is gone: suggestions are always given

`doom_policy`'s `hints` (`off`, `on_stall`, `always`) set whether the bot's pick was offered.
`off` took it out of answers she'd asked for (a look, a list, a forced decision's state);
`on_stall` and `always` differed only in when she's nudged while waiting, which with instinct
on she hardly ever is. The maintainer's: pointless, they should always be on; they're named
as suggestions, and she doesn't have to take them. The setting, its script command and the
`Hints` type are removed; the nudge keeps the times it had by default (30 s quietly, 60 s
aloud). A `hints` a model still sends is dropped as any field she doesn't have. `doom_look`'s
description no longer names the suggestion either (it isn't there in a fight).

### One name for what an action's about; `queue_up` said once, shortly

Going, pressing and inspecting took `ref`; exploring and searching took `target` for the
same thing (what the action's about), and `doom_fight`'s `target` is a monster. Exploring
and searching take `ref` now, so `target` means one thing, in the fight. `target`, `area` and
`direction` are still read as `ref` there (`repair.rs`). A place given as lists give it ("SW
305") is mended to an id for going and pressing only: for exploring it may be a direction, and
is taken as it is. And "queue_up: queue this action up next, don't override current action",
in four descriptions, is "queue_up: after what you're doing, not instead."

### `doom_query`: lists and one thing by one action

`doom_list` (a kind of thing) and `doom_inspect` (one thing) were one verb, "what do I know
of this", as two actions: the SDK warns of near-identical actions. They're `doom_query` (the
maintainer's shape and names): `all` a kind (areas, doors, switches, items, monsters, exits),
or `ref` one thing; with both, the one thing; with neither, told which. Fifteen actions for
sixteen.

The old `all: true` said "every one" and wasn't: it only stopped hiding areas with nothing
left, and the list was cut at 15 either way, without a word. It's `include_explored` now,
which says what it does. The cut stays at 15 (the maintainer's: a level can have a hundred
areas) and is said: "(15 of 40 shown: the first to take)". The description says the order,
and that monsters come nearest first (kept so: the maintainer's). `what`, `type`, `kind` and
`category` are read as `all` (`repair.rs`).

### `doom_why` is gone; what it knew that mattered is told her

An action she has to think to call, for why she's doing what she's doing. In the sessions
with a model it was never called, and the maintainer's point settles it: she has no picture
of the game, so she can't know she isn't shooting to ask why. What changes a decision is
pushed; the rest was commentary. It's out of her actions (fourteen now; the stream console's
"Why" still asks the same code), and three things it knew go where she looks:

- **Her status in a fight** says what the fight's doing and, when she isn't shooting for a
  reason that lasts (`HeldFire::lasting`: out of reach, plasma blocked, shots not getting
  there, no shot, left to the boss; not a moment's, a rocket unsafe or a barrel in line),
  that: "fighting 1 shotgun guy (E 1136): falling back; not shooting: no shot: too steep, or
  something in the way". And a plan of her own in force: "; the invulnerability first".
- **News**, once a fight for each such reason held three seconds: "Not shooting: plasma
  wouldn't get there." It's hers to mend: another gun, another place, or leaving it.
- **A list's entry** says what instinct leaves alone for its deaths: ", died there once".

Dropped with the action: each objective's "because", the "rather than" alternatives, the
fight's "because you're outmatched and hurt", the last order that failed (told when it
did). The console's panel has them still. Parity: 24 of 24.

### `doom_go` goes, presses and explores; `sure` is gone

Four actions for one idea, the maintainer's to merge: `doom_go` (to a thing), `doom_press`
(the same order, with a check that it's a switch), `doom_explore` (an area or a direction gone
over) and `doom_search` (the same, more walls). `doom_go` takes `explore` now: `none` (just
go: a thing's picked up, pressed, opened), `sweep` (as `doom_explore` was, the walls her
policy's `explore` says), `secrets` (search 2), `everything` (search 3). A compass direction
is nowhere to go to, so it's swept; `explore` on one thing is said to be for areas, and she
goes. Eleven actions in a level, from fourteen.

**Rush** (the maintainer's rule): to an area, through whatever's in the way, till she's in it;
the going over is as any other and pauses for a fight. A rushed sweep is a run round a room
under fire. It's two orders: the go, rushed, and the sweep queued after it. For a direction
there's no "there", and it isn't rushed.

**`sure`** skipped the question about leaving with things left, on `doom_go` (for an exit
only) and `doom_exit`. The question does it for one round trip, once a level, and she answers
in a second. Gone from both. `doom_exit` has no parameters ("Leave the level by the nearest
exit"); an exit as `doom_go`'s `ref` stays, the only way to choose between two (a secret
exit). `thorough`, `sweep` and `search` are read as `explore` (`repair.rs`).

### `doom_goal` is two things: when the level's done, and what comes first

The maintainer's reading: `key` and `supplies` were errands among aims of another kind, the
conditions for leaving a level. So the action does double duty, said as such.

**`finish`** (standing, across levels): `none` (instinct never leaves; she's asked when
nothing's left), `out`, `kills`, `secrets`, `completionist`. `kills` and `secrets` end in
leaving now, as `completionist` did: "Everything here is dead: leaving." The `key` goal is
gone (keys rank high as it is: the maintainer's), and `supplies` is no longer hers to set (it's
still what "stock up first?" answers with).

**`prefer`** (the maintainer's list): what instinct and the suggestions put first, a boost to
their ranking and nothing more. (First built to end when had, or with the level; the
maintainer's second thought, the same day, is how it stands: it never ends, and health and
ammo simply have no say while she has enough. "Over" below reads "no say".)

- `health`: health that would add anything (stimpacks, medikits, bonuses; the spheres stay
  her `powerups`), as she goes for it when hurt. Over at 100.
- `ammo`: ammo she has room for, as when she's short. Over when what she holds, the BFG left
  out, would kill a cyberdemon by the fight's own count of what her ammo's worth
  (`Combat::ammo_short_for`: each kind with the best gun she has for it).
- `hunt`: the monsters she knows of. Over when none is left alive.
- `unexplored`: new ground, before pickups, doors and switches. Over when none she has a way to
  is left. (Her `explore: speedy` is about never going back; this only reorders.)

One already had isn't set, and says why. The explorer's `Focus` carries it (`Things`, with
the pickups meant, and `Unexplored` are new; `Keys` is gone), sorted first where a goal's was.
Nothing's set by default, so the bot alone is as it was: parity 24 of 24. How well each plays
isn't measured: they're hers to try.

### No pause of hers

`doom_pause` let her pause the game "as stepping away". The maintainer's: whoever's at the
stream can pause from the console, and she shouldn't need to. It's gone: ten actions in a
level. What her pause had that's worth keeping is kept for theirs: while the game's paused in
a level, by the window's space bar or the console, what she sends is answered (it wasn't:
her action waited on a game that wasn't running, and Neuro waits 20 s at most), what doesn't
move her as ever, the rest told "The game's paused by the stream". Her actions stay
registered through it. Untried against a real pause: there's no pausing a headless game, and
the window's wasn't opened for it.
