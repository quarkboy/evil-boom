# Ultra instinct: a fight searched in the game itself (a plan)

> **Status:** a prototype of stages 1 to 3 on the branch `ultra-instinct` (2026-11-19), tried on
> one level only, E2M9 from a pistol start: section 8. Nothing live, nothing of Evil's.
> Written 2026-11-16 after the TNT and Plutonia sweeps, where she dies on about nine Plutonia
> runs in ten on the easiest skill. Section 4's numbers were estimates; section 8 has the
> measured ones.

The goal: a mode Evil can call on when a level has beaten her too often, in which the fight is
played as well as the game allows. Not a better rule for the fight: a search, tic by tic, over
what she could do next, played forward in the real engine and judged by what happens.

It is the opposite of realism, on purpose. It knows what every shot will roll and where every
monster will be two seconds on. So it is a separate mode, never her play by default, never in
the score, and shown for what it is on stream.

## 1. The idea

The engine is deterministic: the same inputs from the same state give the same game, random
numbers and all. DSDA-Doom can save and restore the whole state in memory (its key frames,
`dsda/key_frame.c`, which its rewind uses; it ships a small brute-force tool for TAS work on the
same footing, `dsda/brute_force.c`).

So nothing about the fight needs a model. To know what a move does, make it and look; to choose
between moves, make each from the same saved state and compare. A shot fired a tic later draws
different random numbers: the search finds the good roll, or the tic a monster's pain chance
comes up, without knowing there is such a thing.

## 2. What it searches

- **Moves, not inputs.** Every turn, step and trigger each tic is too many. A move is 4 to 12
  tics of: one of eight ways to go or standing still; which monster to face (the turn worked
  out, not searched); which gun; fire or hold.
- **A rolling look ahead.** From the saved state, each candidate move is played forward, the
  state it leaves is scored, the best few are kept and carried on from, to two or three
  seconds. The first move of the best line is made for real; then again from there. (A beam
  search with a rolling horizon.)
- **Death is never chosen.** A line she dies on is thrown out. If every line dies, it goes
  back, a second, five, to the room's door, and searches from there with that way ruled out:
  what a person making a TAS does by hand.
- **The score of a state:** her health and armor; the monsters' health left, each weighed by
  how dangerous it is; ammo spent; how many can see her; how soon each thing in the air lands;
  and how far along the way she is. The numbers go in `tactical/tuning.rs` like any other.
- **Where to go** is not its business. The explorer's route gives the way on, as a term in the
  score. What needs the whole level in view (rockets kept for what's three rooms on, which room
  first) is beyond a look ahead of seconds, and stays with the planner.

## 3. How Evil gets it

- **Hers to call, when she's stuck.** Offered after a level has killed her some number of times
  (to be set; three, say), not before: `doom_fight` with a new `ultra` value, or an action of
  its own. What she reads is short: "Ultra instinct: the fight played perfectly, at half speed.
  On till this fight's over."
- **The stream's to allow.** A console setting, as whether instinct is hers to turn off: when
  it's off, the option isn't shown her at all.
- **It shows.** The game runs at half speed while it's on (section 4: that's the budget, and
  it's a look worth having). The stream column says it's on.
- **It ends** when the fight does (nothing in sight or heard for a moment), or when she says.
  Whether it's rationed (so many seconds a level, a wait before the next) is the maintainer's
  call; nothing in the build depends on it.
- **Not in the score, not in a sweep.** The score's runs never have it. Its own check is
  whether it gets through fights she dies in.

## 4. What it can afford (estimates)

Measured: the engine alone is about 0.05 ms a tic (E1M6, 2026-11-15). Allowing for reading the
state and a full room, 0.1 ms: 10,000 tics a second on a core.

At half speed the game moves 17.5 tics a second. A decision every 4 tics has 228 ms.

| | tics it can play | futures of 1 s | futures of 2 s |
|---|---|---|---|
| one core | 2,300 | 65 | 33 |
| 14 cores | 32,000 | 900 | 450 |

Lines kept by the beam share their beginnings, so what's looked at is somewhat more than the
straight count. A few hundred futures of two seconds covers the sensible moves at a decision;
what's chosen to try, and how a state is scored, will matter more than the count.

Not known, and first to find out:

- **What a save and a restore cost.** Every future starts with a restore. At 1 ms it's a
  seventh off the two-second column; at 10 ms on a big level it halves it.
- **A full Plutonia room's tic.** Forty monsters cost more than E1M6's.

**More than one core** can't be threads: the engine's state is the process's. It's worker
processes, each with the game in it. They need no states sent between them: each stays in step
by making the same move the rest have settled on (the game being deterministic), and looks
ahead over its own share of the candidates.

Offline there's no cap at all: as many futures as there's time for, and the result is a list
of inputs, which DSDA writes out as a demo that plays in the game at full speed.

## 5. Stages

Each stage is something that runs, with its check.

1. **Save and restore at the engine boundary** (`dsda/evil_api.c`, `engine.rs`): two calls over
   the key frames. *Check:* a level played 500 tics, saved, played 500 more, restored, played
   the same 500 again: the same state to the last number. And the cost of each, on E1M1 and on
   the largest map.
2. **One fight, offline.** A state with monsters in sight; the search clears the room or says
   it can't. No speed cap, one core. *Check:* the arena's fights (`evil-boom arena`), then a
   Plutonia start she dies at within a minute. What it got through, in how many tics played.
3. **Going back on death,** and the route from the explorer, over a whole level offline.
   *Check:* Plutonia MAP01 to MAP05 from a pistol start on Ultra-Violence: exits, and how often
   it had to go back.
4. **Live, at half speed,** in the watch window, with the workers. *Check:* the same fights as
   stage 2 within the budget; the frame steady (`EVIL_DEBUG_FRAMES`).
5. **Hers to call:** the action, the console setting, what she's told, the stream column.
   *Check:* the Gary test bed: offered only after the deaths, not offered when the stream has
   it off, ends with the fight.
6. **A demo written out** of an offline run (optional).

## 6. What could go wrong

- **A look ahead of seconds, and a death decided earlier.** Walking into a room that can't be
  won from inside isn't seen till it's too late. Going back covers it at a cost in time;
  whether that's bearable live is what stage 4 finds.
- **It knows what she hasn't seen.** A monster about to teleport in is in the futures it plays.
  That's the mode, and it's said so; but it must not leak into her ordinary play (what she
  "knows" after it's off is what she saw while it was on, no more).
- **The score of a state is the whole design.** A poor one and it dithers, or wins the next two
  seconds and loses the room. Expect most of the work to be here.
- **Saving on a big level may be slow.** If it is, fewer and longer futures, or saving less
  often and playing forward from the last save.
- **One of her rules is "don't rebuild a binary in use".** The workers are the same program
  started again: a build under a live session mixes two.

## 7. Not in this plan

Using what it finds to teach her ordinary fight (where its line and hers part is a list of what
she lacks) is the most valuable thing it could do, and a separate piece of work once there are
lines to compare.

## 8. The prototype (2026-11-19, branch `ultra-instinct`)

`evil-boom ultra WAD MAP [--depth N --width N --move N] [run options]` plays a level offline
with it on; `evil-boom ultra-check WAD MAP` is stage 1's check. The search is
`native/src/ultra.rs`; the save and restore are `evil_save`, `evil_restore`, `evil_forget` in
`dsda/evil_api.c` (DSDA's own archive, with what the boundary keeps beside it), `Engine::save`
and `restore` (`Mark`: what the Rust side keeps).

**Stage 1, measured.** Saved after 500 tics, played 500 more, put back, the same 500 again: the
same state to the last number on E1M1, E2M9 and MAP15. Parity with the engine library rebuilt:
all 24 the same.

| | saved game | a save | a restore | a tic, the engine alone |
|---|---|---|---|---|
| E2M9 | 35 KB | 0.006 ms | 0.05 ms | 0.002 ms |
| E1M1 | 100 KB | 0.025 ms | 0.2 ms | 0.005 ms |
| MAP15 | 321 KB | 0.08 ms | 0.7 ms | 0.018 ms |

Far cheaper than section 4 allowed for: a restore is some tens of tics' worth, not hundreds.
(The engine's tic there is with nothing drawn and her not thinking; with her, 0.1 to 0.7 ms.)

**What's built.** A move is 6 tics: still, one of eight compass ways, or along the way on;
shooting (turned to the nearest of them, the turn worked out) or not. 21 moves from each line
kept, 8 lines kept, 8 moves deep (1.4 seconds), the first move of the best made for real, then
again. A line she dies on is dropped. The game is kept every second; with no line she lives
on, or dead for real, it goes back to the last one kept (three goes, then the one before),
the lines' worth shaken so it isn't the same line again. What a state is worth: her health and
armor, the monsters' health left by how dangerous each is, the way on still to go, a missile
about to land, something clawing in reach, keys, sectors moved, ammo, the exit. The numbers
are in `ultra::Tune` for now.

Where to go is still hers: she's asked every real tic, and with nothing after her within 1,500
and nothing of theirs in the air she plays as she does. Searched, the way she'd go is the term
"the way on"; and not shooting, the turn, the press and the change of gun are her own (which
way she faces changes nothing a compass move counted on, and it's facing a door and pressing it
that opens it: without that she never got through E2M9's door).

**E2M9, Ultra-Violence, pistol start.** As she plays, 0 of 16 in the score (dead at about 100
seconds). With it, 8 seeds:

| seed | out at | health, armor | lowest health | kills | gone back | real time |
|---|---|---|---|---|---|---|
| 1 | 170 s | 119, 51 | 6 | 14/14 | 10 | 98 s |
| 2 | 164 s | 147, 97 | 2 | 14/14 | 21 | 98 s |
| 3 | 240 s | 200, 136 | 53 | 14/14 | 0 | 140 s |
| 4 | 211 s | 128, 103 | 3 | 14/14 | 54 | 121 s |
| 5 | 325 s | 167, 67 | 1 | 14/14 | 52 | 183 s |
| 6 | 173 s | 102, 80 | 1 | 14/14 | 31 | 98 s |
| 7 | 178 s | 200, 166 | 76 | 14/14 | 0 | 91 s |
| 8 | 323 s | 74, 0 | 1 | 14/14 | 74 | 183 s |

One core, about 1,000 to 2,000 looks a run, each about 7,000 tics played ahead and 90 ms.

**What it shows, and doesn't.**

- It isn't yet the fight "played as well as the game allows": in five of the eight she was
  down to 6 health or less, most of it lost in the first twenty seconds, and lived by going
  back. A look of 1.4 seconds weighs a hit taken, not a room walked into. Going back on a
  hit that needn't have been taken (not only on a death) is the next thing to try.
- The first version, 6 deep and 6 wide and with her turn and press not passed through, ended
  at the time cap at 2 health having gone back 229 times: the depth and the door both mattered.
- After going back she still knows what she saw in the tics undone, and her clock is made to
  go only forward (the tics gone back added to what she's told). Her knowledge isn't put back.
- One level. Not the arenas, not Plutonia, not live at half speed, no workers, nothing Evil
  can call.

### After a day of watching it (2026-11-19, the maintainer's notes acted on)

In the order they went in, each tried on the same eight seeds of E2M9:

- **A tape.** A run's controls as it came out in the end are written beside its log, and
  `watch WAD MAP --tape FILE` plays them in the window.
- **Aiming only at what she can see** (the game's own sight check, `evil_nearest_seen`): she'd
  been firing rockets into the wall a monster stood behind. **Not pressing on walls:** a move
  told to go somewhere that goes under 8 units costs a little, so standing or a way that goes
  is taken. **A minute of games kept** to go back to, not twelve seconds.
- **The search has her only in a fight:** one of them in her sight, something of theirs in the
  air near, or one after her within 256. With the last monster a room away it had held her
  still for 21 seconds, her way on a straight line through a wall.
- **Her hand's limits on the turn** (18 degrees a tic, changing by 10 at most). It costs: five
  of eight down to 1 health and back 20 to 86 times, where it was three with the free turn.
- **Going back for a hurt, not only a death:** 8 points a second of health and armor together
  over 3 seconds, and the game goes back to before it and tries another way, three goes, the
  last let stand.

With all of it, E2M9 Ultra-Violence from a pistol start, 8 seeds: 8 out in 151 to 314 s of
game; lowest health 37 to 100 (five of them 72 or more); never gone back for a death; gone back
for a hurt 2 to 16 times, 3 hurts let stand in all; 69 to 148 s real on one core. Still one
level.

### E4M6 (2026-11-19)

Ultra-Violence and the easiest skill, pistol start, 8 seeds each, before the changes it led
to: the easiest 3 out of 7 finished (in 596 to 1,945 s, one of them never back for a death),
Ultra-Violence 0 of 6. What it showed, and what was done:

- **The lava river read as a way to take:** it costs nothing the tic she steps in and the look
  is 1.4 s. A hurting floor her way on doesn't cross now costs.
- **The games kept ran out** 100 s in. Now thinned the further back, about two a doubling of
  how long ago, reaching over the whole run (the maintainer's scheme).
- **Stood still at the start** with monsters in sight across the lava: the way on followed by
  its nearer points, her own legs when the search has nothing better than standing and
  nothing presses her, and the way on worth more the longer she stays in one place.
- **A run with nothing more collected, learned or tried for `--stall` seconds ends** (STALLED).
- **What's left is the level's, not the fight's.** With those in, the two easiest-skill runs
  that finished stood at 1 to 3 health at the time cap: about seventy times onto a hurting
  floor for her own objectives (a wall to try, a medikit marked risky), at a drain under the
  hurt gate's 8 a second, then nothing left she'd try. The search kept her alive in the
  fights; the order of things on this level is a plan, and that's Evil's.
- **"Orphan mobj in msecnode list"** is DSDA's own warning as it saves, for a thing just
  removed and still in a sector's list; it skips it. 38 times in one E4M6 run, whose tape
  played back with no save or restore came out the same (1,519 s, 43/51 kills, 3 health).

### A restore that wasn't the game it saved (2026-11-19, Plutonia MAP01)

The first tape off Plutonia (MAP01, Ultra-Violence, an exit at 617 s with 88 health the lowest)
played out in the window as a death at 200 s. Not the load on the machine: the same headless.

- **Traced** with `ultra-check WAD MAP --tape FILE`, which plays a tape three ways in one
  process (as it is; saved and put back before every tic; saved, played 30 tics on and put
  back before every tic) and says where they part. The third parted from the first at tic
  3977: a revenant's missile turned in one and not the other.
- **The mechanics.** A_Tracer turns the missile only on tics where `gametic - basetic` is a
  multiple of four. DSDA's load works `basetic` out from the `gametic` of the moment
  (`dsda_UnArchiveContext`), and `evil_restore` set `gametic` back after the load. So a game
  put back from N tics on had its missiles homing N tics out of step: a different game from
  the one saved wherever N wasn't a multiple of four, and a move is 6 tics. Nothing else
  reads `basetic` in play. E2M9 and E4M6 have no revenants; stage 1's check put back a game
  with none in the air.
- **The rule:** `evil_restore` sets `gametic` before the load. All three ways are then one
  game on that tape.
- **Every run checks itself now:** the tape is played over from the start at the end of a run
  with nothing saved or put back, and the result line says whether it came out the same.
- The Plutonia runs made before this (21 finished) aren't games Doom would play, and are
  thrown out.

### Stage 4, first step: live in the window, one process (2026-11-19)

The maintainer's brief for the live mode: not for playing a level through, but switched on for
a while to get her out of a pickle. So the search, the games kept and the going back are now a
`Session` (`ultra.rs`) begun at any tic and dropped at any other, which goes back no further
than where it began. The offline run plays through the same thing (E2M9 seeds 1 to 3: the same
runs to the tic as before).

- **In the window:** `U` turns it on from where she stands; it's dropped when the search hasn't
  had her for 5 seconds (the fight's over), or by `U` again. `watch --ultra` has it on from
  the start and keeps it on (for trying it; works with `--headless`). While the search has
  her the game runs at half the window's speed. Going back shows as the jump it is, and is
  logged ("back 2.4s: hurt too fast"). When it's dropped the log has what it cost.
- **Not yet:** the workers. The look runs between two frames.

**Measured, E2M9, Ultra-Violence, seed 1, one core.** Headless: out at 154 s as offline; 821
looks, 71 ms a look, the longest 117 ms. A move of 6 tics at half speed is 343 ms, so on this
level one core is enough for the search. The window, 30 s of each (`EVIL_DEBUG_FRAMES`):

| | frames a second | gap between frames, median / p95 / longest | late frames |
|---|---|---|---|
| as she plays | 59.9 | 16.7 / 16.7 / 16.9 ms | 0 |
| ultra instinct on | 49.1 | 16.7 / 81.7 / 103.6 ms | 6.0% |

The budget holds and the picture doesn't: every look stops the window for five frames, about
three times a second. That's what the workers are for (the look in other processes, the window's
own only playing the move settled on), and on Plutonia MAP01 a look was 800 ms, twice the
budget, where the restore is most of it (DSDA's load sets the level up again: `G_InitNew`).

### Stage 4, the workers (2026-11-19)

`--workers N` (on `watch` and on `ultra`): the looks are done by N more processes of this
program, each with the game in it (`ultra::Pool`, `evil-boom ultra-worker`).

- **They keep nothing between looks.** Not kept in step by playing the same moves, as section
  4 had it: for each look every worker is handed the game as it stands, as bytes
  (`evil_frame_get` and `evil_frame_put` at the boundary: DSDA's archive with what the boundary
  keeps beside it; `Engine::hand`, `take`), over a local port. 27 KB on E2M9, 110 KB on
  Plutonia MAP01. So a game gone back to, a session begun mid-level, the next level: all just
  the next game handed over, and nothing to fall out of step.
- **Each has a share of the first moves** (every n-th of the 21) and looks ahead from those
  alone, keeping fewer lines (between them about twice what one process keeps). It answers
  with the best of its share and how deep that got; the deepest, then the best, is taken. So
  it isn't the same search as one process's: wider, and split by its first move.
- **The window doesn't wait.** A tic that needs a look asks for it and isn't played; the frames
  go on; the tic is played when the answers are in (she's asked what she'd do once, kept with
  what she saw). A run and the headless watcher wait.
- **Checked** (`EVIL_ULTRA_VERIFY=1`: every worker's share looked at in the asking process as
  well, from the same game): E2M9 8,856 answers and Plutonia MAP01 (120 s, revenants about)
  2,456, none different. Every run's tape still plays out the same.

**Measured, Ultra-Violence, a look's time (the asking to the last answer):**

| | its own | 8 workers | 14 workers |
|---|---|---|---|
| E2M9, mean | 71 ms | 24 ms | |
| Plutonia MAP01's first 120 s, mean (longest) | 351 ms (421) | 116 ms (173) | 142 ms (233) |
| tics played ahead in those 120 s | 2.5 million | 4.4 million | 7.4 million |

Eight is plenty: fourteen each keep two lines whatever, so do more and take longer. Against
the 343 ms a move has at half speed, Plutonia MAP01 now fits with room.

**The window, E2M9, 30 s with it on** (`EVIL_DEBUG_FRAMES`):

| | frames a second | gap, median / p95 / longest | late frames |
|---|---|---|---|
| its own looks | 49.2 | 16.7 / 80.6 / 100.8 ms | 5.9% |
| 8 workers | 59.9 | 16.7 / 16.7 / 17.0 ms | 0 |

As steady as with it off. Still to do for stage 4: a cheaper restore (DSDA's load sets the
level up again each time, and that's most of a look on a big level).
