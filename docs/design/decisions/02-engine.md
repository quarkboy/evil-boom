# The engine: DSDA-Doom in-process, Evil in Rust (2026-11-05 to 11-08)

Why the engine became DSDA-Doom run as a library a tic at a time, the move of everything to Rust, what she perceives and learns (secrets hidden, the automap rule), and the move to Linux.

## Final engine: DSDA-Doom; Evil to Rust (2026-11-05)

The maintainer's plan: move to **DSDA-Doom** (full MBF21 support, good Linux support). That settles the "Expected" switch above.

Consequences, as discussed:
- **One engine for play and for sweeps.** Evil runs in-process with DSDA-Doom's game simulation (rendered for the stream; built headless for sweeps). Same physics in both; no ViZDoom.
- **Evil moves to Rust entirely** (today ~8.6k lines of Python beside 6.3k of Rust: nav core, sight, line of sight, jump fitting). The Doom engine itself isn't ported: Rust calls DSDA-Doom's C code (FFI). A rewrite of the engine would cost weeks and gain little speed; the win is in-process, no rendering, no conversion.
- **Why the speed:** profiled 2026-11-05, E1M3 with monsters under ViZDoom: engine 53%, observe (ViZDoom's get_state: Python objects for every object and every sector's lines, each tic) 31%, Evil's thinking 16%; ~150 tics/s a process, ~800 at 8 processes. Both the engine and Evil in Rust should reach thousands a process.
- **What has to be redone on the new engine:** jump/run-up checks and anything tuned to ZDoom 2.8's physics; the explorer sweep (60/68) and the fight batches re-run. Boom-family physics differ (actor blocking heights, some timings, autoaim details).
- **To check:** whether two players (Neuro and Evil) can share a DSDA-Doom game.
- **Order:** a spike first (DSDA-Doom built headless as a library, stepped from Rust, monsters and sectors read directly, tics/s on E1M3, multiplayer); then port Evil bottom-up behind the engine interface (engine adapter, motor, knowledge, then tactics last, since they're changing fastest), each module checked against recorded traces and the Python deleted after. Fighting keeps improving in Python on ViZDoom meanwhile; the behaviour carries over.

## DSDA-Doom spike (2026-11-05)

DSDA-Doom v0.30.1 (kraflab/dsda-doom, eb8eb98), built on Windows with MSYS2 mingw64 (SDL2, SDL2_mixer, libsndfile, libzip, zlib; optional music libraries off). Kept out of git (`third_party/`); ours is `dsda/`: `evil_api.c`, `dsda-doom.patch`, `README.md` (build steps), `spike/` (Rust).

- **Ceiling**: `dsda-doom -timedemo -nodraw -nosound` on a 20,000- and a 200,000-tic random-walk demo of E1M3 (HMP, monsters): ~205,000-233,000 tics/s, linear in length.
- **In-process**: DSDA-Doom built as a shared library (`dsda-evil`: the game's own sources plus `evil_api.c`; the patch adds the target and one exported wrapper, `D_DoomMainSetupForEvil`, the setup without the main loop). `evil_init(argv)`, `evil_tic(forward, side, turn, buttons)` (the singletics path: the command written into `local_cmds`, `G_Ticker`), and readers into plain arrays: `evil_sectors` (floor, ceiling), `evil_mobjs` (type, position, momentum, angle, health, flags), `evil_player_state`. A C ABI DLL from mingw loads fine in MSVC Rust (`libloading`). SDL's dummy video/audio drivers: no window.
- **Result**: E1M3, HMP, monsters, a random walk, reading every sector (177), every thing (~310) and the player each tic: **~125,000 tics/s** (stepping 80%, reading 17%: ~1.4 µs a tic for all of it); set up in 0.02 s. ViZDoom: ~1,650 tics/s for the engine alone, ~150 with Evil's Python. Doom is now free; Evil's thinking will be the whole cost.
- **Things to settle in the real adapter**: a thing's identity (the spike uses its address; `mobj_t.index` is the GL renderer's, not an identity): a counter set at spawn. Map thing types to names (mobjtype_t → our bestiary). Keys, items and suits read directly (no "gone since the start" inference). Level exit and death state from the game, not guessed.
- **Two players**: DSDA-Doom has no network play (its "net" is local: `-solo-net`). In-process it isn't needed: each tic the commands of every player in the game are ours to give (`local_cmds[i]`), as multiplayer demos play back. Untested: starting a co-op game with two players in it and feeding Neuro's commands beside Evil's.
- **License**: DSDA-Doom is GPL-2.0+; Evil linked with it in one process makes the combined program GPL. To keep in mind for distribution.

## The explorer on DSDA-Doom (2026-11-06)

Python Evil on DSDA-Doom in-process (`--engine dsda`): the whole explorer sweep (68 maps, god mode, 3 lives) in ~2.5 minutes at 8 jobs. 54/68 at first; the differences, one at a time:
- **Vanilla door rules** (ZDoom differed). A switch on a sector that's busy (a door open on its timer) does nothing, and a once-only one stays to be pressed; a door's own switch pressed on a door open for good restarts it as one that closes. So: all once-only lines used this life are spent to the planner (not just stairs: E3M4's door, opened for good, shut by its own door switch, seemed to open again for the spent one); a once-only press that moved nothing, on sectors moved in the last 200 tics, "didn't take" and isn't used up (E3M4's switch behind a door opened by its door switch), unless what last moved them left them for good and the line has a tag (E3M7's walk-over line opening a door just opened for good: it went off; MAP28's own door line on such a door: it didn't); once-only door lines used aren't doors she can open (MAP19's, the door shut by a walk-over line); and before pressing a once-only switch (not stairs: MAP21), she waits for what it works to come to rest.
- **Jumps**: the movement simulator matches DSDA-Doom to 0.2 units given the same inputs (checked tic by tic, run and jump, with `evil_warp`); it now pushes not at all in the air (vanilla; ZDoom's aircontrol is 1/256) and rounds inputs as the adapter does. Misses come from the follower's run-ups, which aren't the check's to a unit: a jump that comes down off its landing is left out once missed (unless nothing else is left) and for good after two; each jump is scored by how sure it is (made shifted 2 and 4, turned 1.5° and 3°, from a run-up 8 longer and shorter): under a quarter, dropped; the rest dearer to the planner the less sure (up to 1024 units).
- **Path smoothing**: a shortcut may ignore a wall the box already touches, but not go through it; a corner right on the path is rounded on the side of it that's floor (E3M7's doorway: pushed 30 into the wall, she pushed into it for good).
- **Floors hurt on the level's clock** (every 32 tics): only hurt twice on a floor that far apart is it the floor (a monster's shot made floors "hurt after all").
- **Sweeps are deterministic** now (PYTHONHASHSEED fixed for the workers): a change's effect can be told from luck.

- **Use traces, vanilla**: any line with a special stops one (a walk-over line too: E2M4's door was pressed from beyond one, by the planner and in her model); a once-only line used has no special any more, so the trace goes past it (E2M7: a switch behind a used door switch went off, and she never knew).

60/68: parity with ViZDoom (all but the 7 kill maps and MAP30).

## Secrets hidden: she knows what a player sees (2026-11-06)

`Knowledge.perceive` (default; `--peek` gives the old way): a line is a switch, door or lift to her only if it looks like one (a texture on its front: SW1/SW2, DOOR/BIGDOOR/SPCDOOR/EXITDOOR/ZDOOR/ICKDOOR/ZELDOOR, PLAT/SUPPORT). Surveyed over Doom and Doom II's 2,199 usable lines: that catches 1,394 of the 2,012 not flagged secret and 32 of the 187 flagged (the rest look like the walls round them, as meant); the other 618 are mostly lifts and doors in plain textures.

What doesn't look it is found by trying walls, as a player does:
- **Suspicion** (`Knowledge.suspicious`), from what she can see only, of seen walls faced from floor she can get to whose far side she can't: the automap drawing it as a two-sided line (+3; a mapper hides a secret door by flagging it), a raised platform's side (+1), an item she can see but not get to just behind (+2), a texture unlike the walls either side (+1.5; short pieces +0.5). 2 or more: worth a press, one wall for each place behind.
- **Trying** (`wall` objectives): once nothing else is left, the most suspicious few: walk up, face it, press use, watch. What it does (if anything) the knowledge sees as any line set off; tried walls are remembered.
- **Panic** (the maintainer's idea): nothing at all to do, not even a suspicious wall: every wall round the place she's in, nearest first (a lift out of a pit that looks like the pit's wall).

Explorer sweep with it: 52/68 (peeking: 60). Lost: MAP05, MAP08, MAP14, MAP19, MAP21, MAP27, MAP28, MAP29: the suspicion scoring's work list.

## Perception: what she learns, and how sure (2026-11-06)

Tracing the maps perception lost, each back to the route that failed:
- **Walls judged as last seen** (not as the map starts): a stair or a raised floor since makes a wall of what was floor, and a lift's side on it a suspect (MAP21, MAP29, MAP23).
- **Presses that add on** (stair builds, "raise 24/512") each count as their own try (`Knowledge.tried_key`): MAP21's four stair switches each build 16 higher.
- **A trap she's in** isn't kept out of (MAP21's nukage pit, its lifts out tried only from inside).
- **A switch on a busy sector** with a lasting effect (SR/GR, not crushers) isn't taken to have worked (MAP12's "open for good" switch pressed while the door was open on its DR timer).
- **Certainty** (the maintainer's idea): an effect put down to a press is *sure* if the sector started moving (by its sound) within 8 tics of it, and only if it moves the way the line sends it; else it's in doubt (`Knowledge.doubts`). The planner doesn't build on a doubted *permanent* effect (as seen instead); a doubted repeatable switch is pressed again, once a life, once nothing else is left, and the effect made sure, kept (it fits) or unlearned. Doubting non-permanent effects too lost MAP27 (they come back anyway: nothing to guard).
- **The way back** ("can she get back?", before a one-way move): doors that close again count as shut, but for ones she can open from where she'd be, by their own lines or by any switch or walk-over line she knows there (MAP19's closet, its door opened from outside and its way out a once-only teleporter). A margin shutting doors near the end of their timer for routes was tried and dropped: MAP12 was the use gates', and it cost MAP19.
- **Once-only teleporters used this life** are out of her view (MAP27's, walked over again from the room it led out of: every way out was "stuck" on it).
- **Use gates between two sectors**: a press across a gap that's shut at the start between a movable sector and another (a raised platform's floor at an alcove's ceiling) is possible once there's a gap (`use_line_gates`, in Rust too): MAP12's switch beside a platform the W1 lines lower. In the relaxed needs, an activator that waits on the very sector being produced doesn't count as a way to produce it (MAP27's lift switch, pressed from below across the lift's edge only once it's down).
- **The box over a ledge from below**: a path segment may not take the box further over a line it's already over whose high side is more than a step above the floor under its centre, even a line the way crosses later (MAP19's fan of stairs meeting in a point beside a floor two steps down: set off from beside the point, every way went along it and she pushed at it 600 times). Trimming portals at such ends instead cost MAP29 and was dropped.
- `tests/test_follower.py` runs on DSDA-Doom (each game in a process of its own).
- **The stranding report** gives, for the omniscient plan, where its way goes through what she doesn't know or by heights she doesn't believe, its teleports/doors/drops, and where her reach ends on it (and if that's a poly she's learned to avoid, or a trap): MAP27's stuck poly on the way out of a room was found that way.

Sweep: 55/68 (MAP12, MAP08, MAP21, MAP23, MAP29 back; MAP19, MAP27 lost). MAP27 is sensitive to the order things are done in: lowering sectors 28/29 early leaves the lift ride from switch 739 too long for the planner's walking speed (`LEG_SPEED` 12; flat-out running is ~15.1, but the follower doesn't make the lift either); MAP28 needs a red-eyed skull (one of three WOOD4 panels, shifted by its y offset) pressed above an opening, and a symbol panel shot (SKINSYMB), both suspected only in work-in-progress changes that cost E3M4.

Perception sweep after these: **56/68** (out: MAP05, MAP14, MAP27, MAP28). The peek sweep was 54/68 at this point (58 before the perception work), traced:
- **A doubt is rechecked only on a press with what it acts on still** (and once a life): E3M7's lift line walked over while the lift came back from another's press was turned down by the game, "unlearned", and the lift lost to the planner.
- **"Pressed, but nothing went off" counts this try only**: MAP08's shoot-switch, shot "again" from where the shot hits the switch on the block's other face first, went on for ever (an old shot of it counted). Pressing "again" also goes by the last press of anything with the same action.
- Peek: 57/68 (out: MAP15, a one-way move into a dead end late on; MAP21, a corridor exactly the box's width between a wall and key bars alongside it, which the planner takes as open, as a door she could open, though nothing opens it on the way; MAP27).
- **Sweeps deterministic again, load or no load**: searches were cut off by the clock (the explorer's route searches at 1.5s, the follower's replans at 10s, the planner's wait for a route without squeezes at 10s), so eight maps at once took other ways than one alone (MAP14, MAP17, MAP23 passed alone and not in the sweep, or the other way round). The expansions decide now; the seconds are a backstop (30s, 120s). A spot to look out from must fit the box with half a unit of play (MAP21's, between a wall and key bars exactly 32 apart).

- **...and the same run alone, again and again**: the Rust core's hash maps and sets were seeded at random in each process (std's), and where a choice fell to the order they went round in, MAP17 alone exited two runs of three and got stuck on the third. `native/src/det.rs`: the same hashing with fixed keys, everywhere. The clock backstops on searches are 600s (30s still bound with eight at once).
- **Slow isn't stuck**: the sweep gives a map an hour of game time (`--max-seconds 3600`; was 1500): MAP15 under peek exits at 2051s (the maintainer: fix failures, not inefficiency).

Perception 56/68 (out: MAP05, MAP14, MAP27, MAP28); peek 58/68 (out: MAP14, MAP27). MAP14's doors open by walk-over lines nobody can see, one along the far end of a pit where the box is stopped with its centre on the line: to trace further.
- **Suspicion of faces over openings**: a face above or below a way through counts when unlike the walls round it, as does the same texture shifted up or down from its neighbours between the same sectors (MAP28's red-eyed skull, one of three WOOD4 panels: pressed now). Giving each suspected wall its own identity (so one given up on doesn't hold the rest back) cost E2M9, E3M4, MAP25: reverted.
- **A noise checked from where the box can stand** (`_stand_spot`): MAP05's noise goal was a poly's middle 12 short of reach, pushed at till the poly was taken for one she gets stuck in, and the way on through it lost.
- **Locked lines without the key** don't count as used (the game says so and does nothing): MAP05's blue door, looking like the wall round it, pressed as a suspect wall before she had the key, was taken as a once-only line used up. **A tried wall is tried again** with a key she didn't hold when she tried it.

Perception **58/68** (out: MAP27, MAP28); peek 57/68 (out: MAP14, MAP15, MAP27; MAP15 ends where no route to the exit is left, late on).
- **A doubted line's recheck is its one go a life, whether or not it could tell anything**: MAP27's lift switch, its lift always moving when pressed, was "tried again, unsure" 1800 times. With this and the key rule above, MAP27's red door (a marble face, looking like the wall) is pressed with the key: MAP27 exits.

Perception **59/68**: only MAP28 out (besides the 8 boss maps). Peek 57/68 (out: MAP14, MAP15, MAP27).
- **The places she got stuck get another go** once nothing else is left (the poly-avoidance after two failures had cost the only way on in E3M4, MAP05, MAP27). **A panel unlike the same panel either side** is a stronger suspect (+1.5: a mapper's mark for a switch, MAP28's symbol). Skipping walls given up on lately before taking the best few (so MAP28's symbol, 25th, came up) let the wall tier run on for ever and cost MAP14, MAP23, E3M4: not kept.

Still perception 59/68 (MAP28 out), peek 57/68.

## The 100% secrets sweep (2026-11-07)

`tools/explore_sweep.py --secrets` (the maintainer's third step): every secret there is, then the exit. She knows only the tally's count (as the automap shows a player: how many, not where); while it's short, the exit is last (`exit_last`): everything else first, the suspect walls and the panic tier included. Levels with secrets nobody can get pass that many short: E4M3 two (torch sectors no one can stand in), E4M7 one, MAP15 one (#9), MAP27 one (#5: a teleporter's sector, the teleport line in the way); the maintainer, and Paul Falstad's Doom II secrets list (gamers.org).

First sweep (perception, 3 lives): **21/68 with every secret**, 56/68 exited. Most of the rest leave with some missing once there's nothing left to try (E2M4 4/10, MAP10 1/18, MAP19 3/9): the work list for finding secrets.

## What the game tells her (2026-11-07)

The maintainer: pressing a locked door without its key, the game says so ("You need a blue key to open this door"), and a player learns from that. DSDA-Doom's messenger is patched (`dsda/dsda-doom.patch`) to keep the last message given to the player and its tic, shown or not; `evil_player_state` hands both over and `Observation.message` has one when it's new. A press whose line the game answers with a need-a-key message puts the line in `Knowledge.locked`: a door or switch from then on however it looks (offered once she holds the key, first among doors), where before it was only a suspect wall to be tried again with a new key.

Also found: `ADDS_ON` (the per-line key for "raise N" floors) had a backspace character where its `\b` should have been since it was written, so it never matched; fixed.

**The game's random numbers came from the clock**: DSDA-Doom adds `time(NULL)` to its seed at every new game (`I_GetRandomTimeSeed`), so a barrel took a different number of shots to burst each run and everything after went its own way (MAP23, alone, four runs: 376 to 1299 game seconds). Sweep results before this carry that noise. The patch adds `evil_seed(fixed, value)`: a seed of our choosing, the same run every time given the same commands, for testing. A real run keeps the clock (the maintainer: we want random seeds for the real deal), and that's `DsdaEngine`'s default (`seed=None`); the sweep and the watcher take `--seed` (0 unless told; `random` for the clock), so a sweep's run can be watched again as it was, and a map can be tried over several seeds.
- **Which wall next counts each wall's own failures** (`_wall_drops`): the walls still share one wait once given up on (that keeps the wall tier paced; giving each its own cost other maps before), but among those with a route, one given up on before counts as twice as far for each time. MAP14 (with the lock message, she opened the red door sooner and went another way): the nearest wall, line 864, behind a chain of timed switches, timed out every time and still came first every time; the blue door like the wall (811), at the top of the suspects with the key held, never got a go. Now it does: MAP14 exits in 1958 game seconds (3107 before), the same on seeds 0, 1, 2.

Sweeps on seed 0 with all of the above: perception **59/68** (only MAP28 out), peek 57/68 (MAP14, MAP15, MAP27 out): as before, now repeatable.

## Evil in Rust (2026-11-07)

With the game in-process, nearly all the time went into Evil's own thinking, in Python; the maintainer: port all of it. Evil is now one Rust crate (`native/`, package `evildoom`): a library and the `evil` command (`sweep`, `run-map`, `watch`, `dump-nav`, `follow`), with DSDA-Doom in-process as before. The Python (`src/evildoom`, `tools/`, `spikes/`, the ViZDoom adapter, the PyO3 bindings) is gone; it's in git history.

- **How it was checked**: module by module, the Rust's logs keep the Python's line formats, compared on six short levels (E1M1, E1M2, E2M4, MAP01, MAP02, MAP23; seed 0) line for line, the analysis by `evil dump-nav` beside the Python's dump, and the route following by `evil follow`. They match but for timings; the one difference seen (MAP17 peeking) is a tie among equally good spots, broken by set order.
- **Shape**: the `Navigator` (a level's analysis, its caches in `RefCell`s; one per process, so `Rc`), `View`s of it restricted to what she knows, the `Explorer` with a `Hooks` trait for what the 100%-items run and the fight add (wanted, last resort, gone, dangerous, interrupt, before exit), and `play::Agent` choosing between them.
- **Speed**: Evil's thinking ~5-7x faster on the parity levels (E1M2: 711 tics/s to 3577; MAP01 1658 to 10934). A full perception sweep, 68 maps at 8 jobs, takes under 3 minutes, most of it the boss maps running out their hour.
- **Results** (seed 0, 3 lives): perception **59/68** (only MAP28 out besides the 8 boss maps), as in Python. Expert (every item, then out): 53/68 exited, 24/68 with every item: the first expert sweep written down, the work list for pathfinding.

## Expert runs on one life; tools for tracing (2026-11-07)

The maintainer: the goal is 100% items on one life; lives are a testing device, never something the bot plans by. Runs now have unlimited lives (`--lives N` to limit) under the one game-time cap over all of them (3600 s), and the 100% runs end STALLED after 300 game seconds (`--stall`) with nothing more collected and nothing new learned of the level (sectors seen, what lines do), counting the best so far over the whole run. They leave as soon as the count they're after (on the automap, as a player sees it) is complete.

Rules, each traced from a concrete failure and kept narrow (the maintainer: reason a fix through before trying it; the first broad versions cost other levels):
- **Spent once-only lines** are spent everywhere: what she can get to and the follower's replans now leave them out, as the route planner did (E2M1's door switch 288, pressed once from the alcove).
- **Through it while it's open**: right after she presses a door or lift open that comes back by itself, what's beyond, if she's never walked there, is urgent while there's time to make it (E2M1's room with the teleporter to the secret bonus corridor). Not for a lift walked onto (E2M4's lift 66: back to where she'd been can wait).
- **Back at rest**: a door or lift that comes back, set off after one that stays, shuts again (vanilla: a DR door pressed when open for good cycles); only movers that cycle back (not crushers, not perpetual lifts). Seen-long-ago alone isn't enough: what moved it may be unknown to her (E2M4's door 48, opened for good by walk-over line 668).
- **What would cut her off from other objectives** goes last (a player finishes a room before dropping out of it one-way); the exit excepted; not what can only be had now.
- **Lifts in "could she get back"**: at rest unless she can call them from there, as doors were (MAP27's lift 156, whose only switch is on it).
- **Never knowingly away from every exit**: an objective that would cut her off from every exit she can get to isn't taken, safe or risky (MAP24's wall worth a drop into the nukage pit; its secret ledge over it).
- **Turning is standing still**: when she only turns (a glance, a look round, facing a wall), her momentum is braked away (MAP24: a glance just after landing a run-jump slid her off a small ledge).
- **Ground that changed unforeseen**: a crossing ahead that's become a drop through heights outside anything she believed gives the objective back to be judged again, if the only way on drops where the plan didn't (E2M4: walk-over line 909, unknown, raised a floor on the way and left the goal down in the blood pit). A lift down when she counted on it up is foreseen: on as before (E1M2).
- **The last resort, with the exit there** (100% runs): walls with more to go on than one weak sign are all pressed; of the rest, a round of 12, then out (E2M5 went round every wall of the level; E4M7's and E4M9's ways on scored 5.0 and 5.5).
- **Walls between suspects**, a fix: the "can she get back" loop ended only when no opener applied, not when nothing changed (it spun on a spent switch).

Tools (`evil`): `run-map --snapshot S [--focus X,Y,ZOOM]` (the sweep's own run, nothing drawn till S, then the viewer's window to out/watch.png); every run-map records its inputs (out/runs/<IWAD>_<MAP>[_expert...].run: controls, restarts, and the journal of what the explorer itself put into her knowledge: tried walls, stuck places, missed jumps, traps, failed presses), and `--from FILE@S` plays one back to S (the same state exactly; E2M4 to 1000 s in 0.9 s) and hands over to the bot as it is now; `drive` takes her over from a script (goto by her knowledge or the whole map's, warp, face, use, move, wait, bot N, where, exit?, sector, line, objectives, snapshot); `sector` shows a sector's lines and things; `EVIL_DEBUG_OBJECTIVES=a-b` logs every option and why an objective ended. The watcher's own side panel calls `objectives()`, which changes the explorer's state: a watched run can differ from a sweep's (snapshots don't).

Found with them: E2M4's computer map sits in a cage of pillars with 32-unit gaps (in DSDA a 32-unit box doesn't pass, aligned or not), lowered by walk-over lines on the edge of a nook the game won't let her into; earlier runs got it somehow, still unexplained.

New baseline, expert (seed 0, unlimited lives, 3600 s, stall 300 s): **17/68 with every item**, 38 exited, 30 stalled (8 of those the boss/kill maps and MAP28). Restart loops to look at: E1M4 (26 levels again, 0 items), MAP30.

## Linux, and the name evil-boom (2026-11-08)

The maintainer moved to a new machine (Fedora 45 under WSL) and wanted both platforms kept. The binary
is now `evil-boom` (the maintainer: for Boom, and Evil's first original song); the package and library
stay `evildoom`.

- **WADs**: Linux defaults to `~/.local/share/evildoom` in Steam's layout (`base/...`), copied
  there rather than read through `/mnt/c` (slow); Windows keeps the Steam folder.
- **Engine build**: `tools/build-engine.sh` for both (Linux, MSYS2 MINGW64). The patch now sets
  the library's LIBRARY_OUTPUT_DIRECTORY too (a `.so` is a library output, a `.dll` a runtime
  one) and depends on `dsda-doom-wad`, so both land in `prboom2/build`. On Fedora,
  `CMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF`: DSDA prefers package configs, and zlib-ng's names a
  `libz.a` that isn't installed.
- **Viewer**: minifb 0.27 -> 0.28. Under WSLg 0.27 segfaulted on X11 with AspectRatioStretch
  and corrupted the heap on Wayland (found with a minimal program, option by option); 0.28 is
  fine on both.
- **Checked**: the same results as on Windows, seed for seed (E1 one life 3.6 levels; Doom II
  7.9, MAP12 stuck 7/16). Expert sweep, 68 maps, seed 0, 8 jobs: **48/68 with every item**,
  56 exited, 10 stuck (the 8 boss maps, E1M4 44/45, MAP21 17/18, MAP26 45/58), 2 stalled
  (E4M6, MAP14), in **207 s** wall-clock.
