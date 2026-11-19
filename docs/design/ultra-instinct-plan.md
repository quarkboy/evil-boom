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
