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
