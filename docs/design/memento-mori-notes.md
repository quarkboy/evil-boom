# Memento Mori, level by level (working notes)

> **Status:** in progress, on the branch `memento-mori` (2026-11-23 on). The maintainer's
> brief: custom-WAD compatibility, starting with Memento Mori (`wads/mm_allup/MM.WAD`), every
> level from a pistol start (the WAD is made for that), all four skills, to find what stops her
> finishing: switches she doesn't take for switches, and the way levels are built. The
> three-lives test is left for when these changes are taken to `main`; DOOM and DOOM II are
> watched meanwhile by `tools/parity.sh` (what each change does to the 24 runs is in its
> commit).

## How it's measured

- **Charmed, from a pistol start** (`run-map doom2 MAPnn --pwad wads/mm_allup/MM.WAD --fight S
  --charmed --seed 1`): she plays as one who can die and nothing hurts her, so what's left is
  whether she can get through the level at all. A "death" in a charmed run is her starting the
  level again because she's stranded or in a trap.
- **The omniscient planner** (`follow`, `dump-nav`): whether a route exists by the planner's
  own model of the map. `dump-nav` now prints the stages of what can be reached with every
  trigger fired and key taken, the edges where that reach ends and the triggers never fired:
  on a map with "no route" the far side of one of those edges is what the model lacks.
- The survey: `out/mm-survey/sweep.sh NAME SKILL` (all 32, a copy of the binary, so the build
  can go on); a small WAD reader for looking at lines and tags, `out/mm-survey/tools/mm.py`.

## Where it stood, and stands (Hurt Me Plenty, charmed, seed 1, an hour of game time)

| | exits of 32 |
|---|---|
| 0.9.1 | 12 (MAP16 and MAP17 wouldn't load) |
| the loader, the gate rule, MAP04's four, the mesh (7420edb) | 15 |
| MAP26's door and exit ring (0aedce7) | 16 |
| MAP14's door, MAP05's switch (f895af5) | 18 |
| MAP20's ledge and ceiling (575ceee) | 19 |

**All four skills at d298546** (charmed, seed 1, the hour; `out/mm-survey/1124a-*`): skill 1:
20 of 32 (not out: 09 12 16 17 18 21 22 23 25 26 30 31); skill 2: 21 (04 09 12 16 17 18 21 22
25 30 31); skill 3: 20 (09 12 15 16 17 18 22 25 26 28 30 31); skill 4: 18 (04 09 12 15 16 17 21
22 23 25 27 28 30 31). MAP06 out on all four. (MAP26 at skill 3 was out on seed 1 before and
isn't now: it's out on 1 seed of 5 on the old build and 0 of 4 on this: never sound.)

**All four skills at 575ceee** (charmed, seed 1, the hour; `out/mm-survey/1123g-*`):

| skill | exits of 32 | not out |
|---|---|---|
| 1 | 19 | 06 08 09 12 17 18 21 22 23 25 26 30 31 |
| 2 | 20 | 06 09 12 16 17 18 21 22 23 25 30 31 |
| 3 | 19 | 06 09 12 15 16 17 18 22 23 25 28 30 31 |
| 4 | 18 | 04 06 09 12 15 16 17 21 22 23 25 28 30 31 |

Not out on any: 06, 09, 12, 17, 22, 23, 25, 30, 31. Out on some and not others, so worth a
seed or two more before a trace: 04, 08, 15, 16, 18, 20, 21, 26, 28 (MAP20 is out on all four
here, without endless bullets).

Not out at f895af5: MAP06, 09, 12, 15, 16, 17, 18, 20, 22, 23, 25, 28, 30 (the Icon of Sin), 31.

By the omniscient planner at 0.9.1, 17 of the 32 had no route: MAP04, 06, 09, 12, 15, 16, 17,
19, 21, 23, 25, 26, 28, 29, 30, 31, 32 (of which she got out of 19, 29 and 32 anyway, by
exploring). At 575ceee, of those she isn't out of: no route even in principle on MAP06 (107 of
281 sectors reached), 09 (325 of 513), 16 (218 of 444), 31 (30 of 508): the model lacks
something there; a route in principle but none planned on MAP12, 15, 17, 23, 25, 28; a whole
plan on MAP18, 20, 22, where what stops her is in the playing.

**Ammo, not the way.** From a pistol start she can run dry on these maps, and charmed she
still won't close on a Hell Knight with her fists: on MAP18 one stood on a stair 64 wide and
that was the level. `--charmed --endless-bullets` takes that out (MAP18: out in 3,488 s with
it): use both when the question is the way through, and charmed alone when it's whether she
gets through as she'd play it.

## What was found, by level

**MAP16, MAP17: wouldn't load.** A sidedef of sector 65535 in each. The engine takes an
out-of-range sector as sector 0 (`P_LoadSideDefs`); the loader refused the map. Now as the
engine has it.

**MAP02: the gate that opens from a line in front of it.** The grate between the start and
the rest is opened by use lines across the way a few units before its bars, faced like the
bricks round them. She tries a use line only if it looks the part, is door-shaped, is a lift's
face, is in her way or is locked: these were none. A press goes to the first special line it
crosses (the use trace), so a player pressing the gate presses the line. Rule: a use line
with, a press's reach past it, a thin sector too low to walk through, counts as that door
(`before_a_door`). Stuck (37 restarts) -> out in 328 s.

**MAP04: four things.**
1. *The blue key is got by running down a stair.* It's in an alcove 32 above the floor at the
   stair's foot, no other way in. A step down is fallen off, not stepped, and at a run she's
   still within a step of the alcove's floor 80 units on. The jump finder only took off from
   drops of more than a step: now also from a step down, where a floor within 160 past it is
   more than a step above the floor below and no more than a step above the one left.
2. *Two timed lifts in a row.* Out of the keep by a lift that rests up (called from inside),
   and on by another lift called from deep inside the keep and down for three seconds. Blocked
   by the first she waited three seconds before pressing the switch beside her: all of the
   second's window. A mover standing at rest isn't coming: pressed for at once.
3. *From the first lift's pit* the replan goes by the heights as they are, by which a lift
   that's down has no way off it. Her own lift, down on its timer, is now also taken at rest.
4. *The drop into that pit was "unexpected"*, and the new way thrown back for dropping where
   the plan didn't: but she'd pressed for that lift herself. Counted as on its way.

   Stuck -> out in 2,271 s, 106/106 kills.

**MAP05: the switch in the moat.** The lift that is a whole room's floor (sector 112) is
called from a switch in a recess of the nukage moat (line 717). She saw it in the first
minutes and pressed it in the 58th: no switch is pressed from a floor that hurts while
there's other floor, bar with nothing else at all to do, and that came after every suspect
wall of the level (46 minutes of them). Now a pass for such switches comes before the walls
(`switches_waded_for`), the health margin and the cut-off rules standing. Stuck at the hour
-> out in 1,628 s. (A rule tried earlier, a use line at the back of a shallow niche, did
nothing for it and wasn't kept. She also wedges at (-288, 22) on an island's corner in the
moat now and then: not looked into.) This one changes play on any level with a switch in a
hurting floor: DOOM II MAP09 with a loadout, eight seeds on one life, 0 out before and 2 after.

**MAP14: a door on a timer, 900 units from its switch.** She learned the switch (heard the
door) and never went: the planner had her at the door 220 tics after the press and the door
"lasting" 213. A door coming down is walked under till the gap is less than her height: 34
tics more here. Counted (`MoverModel::timing_from`). Stuck (9 restarts) -> out in 2,128 s.

**MAP18: ammo** (above). Out with endless bullets; nothing wrong with the way found.

**MAP20: three things, and not out yet.**
1. *The nodes.* Both lifts' doorways are leaves of the map's tree that reach 80 units into
   the lift's room with no line between: the game itself takes a point there for the
   doorway's sector (floor 128, ceiling 192), so with the lift up it's a wall nobody can
   see, and whoever stands there when the lift goes up stays down inside it. The mesh has
   it as the game does. Her own check for a ceiling come down on her went by the sector
   under each corner of her box and fled a ceiling that wasn't there, three tics in six,
   silently, till every errand from that room timed out. Now checked by the game's rule
   (the sector under her middle, the lines her box touches) first.
2. *The red card's ledge.* Its face, a fall of slime over a nukage pit, is a use line that
   raises the pit's floor. Offered now as the face of a ledge with a key on it
   (`under_a_key`), and gone for as a key is. Pressed at 674 s; before, never.
3. *Still open:* she walks into the piece of the doorway that's inside the lift's room, the
   lift goes up round her, and she's stranded (a restart). And from the top of the other
   lift a path is planned across that piece as if the lift were down, waited at ("waiting
   at mover (sector 211)") every 0.8 s. The way out of either is the switch on the room's
   pillar, in reach from there.

**MAP26: the exit's shaft, and its ring.**
1. The way to the exit is a drop of 128 into a shaft where a blue door's slab stands shut
   at the ledge's height. No room past the ledge as the map starts, so no drop, so no exit
   "even in principle". The drop's now judged with doors as open as any line takes them, and
   at such a drop she presses a door of hers that's hung in it.
2. The exit is a ring of walk-over lines round a pit that a line on the way in shuts. She
   went at one line, found it uncrossable three times, and was offered it again for ever
   (the finding only reached the triggers she'd learned, not the exit's objective). Not
   offered from that ground now. She tries each of the eight lines three times before
   looking elsewhere and finding the switch that opens the pit: clumsy, and out in 1,989 s.

**MAP06: three things; out on all four skills** (charmed, seeds 1-3: 12 of 12, 787-2,932 s).
The maintainer pasted the Doom Wiki's walkthrough, and each step was looked for in the model.
1. *Crate to crate, one sector.* In the second warehouse every crate 64 high is sector 192.
   The way on is from the head of a little stair in the corner, across the corner, onto the
   next crate: her box over one or the other all the way and no floor under her middle
   between, so nothing the mesh joins. A jump, to the finder; but it took a landing only on
   another sector than the one left. Now on another stretch of floor: polys of a sector that
   portals join are one stretch (`floor_piece`). The reach went from 107 of 281 sectors to
   250. (What the notes had before, "crates as stairs shallower than her box", was wrong:
   those the climbs had already. It was the two crates being one sector.)
2. *A door opened long before its sill is lowered.* The slab before the exit (sector 235) is
   opened for good by a line walked over early, and its sill lowered by a switch at the end.
   Effects were added to a sector's heights and the next worked out from the starting one
   while that was still among them: the sill came down under a slab still shut. A ceiling
   gone up for good with the floor as it was is now instead of the old heights
   (`nav::opened_for_good`), as a floor lowered for good already was.
3. *A lift narrower than she is.* The way back into the first building is a bar across a
   window (sector 68, 16 wide) that goes down to the yard and up again under a ceiling too
   low to ride to the top: she's carried by the part of her box on it and walks off at the
   room's floor on the way. The plan had it. The follower judged the crossing onto the bar
   by the floor under her middle (the yard's), took the rising bar for a mover out of reach,
   and stepped back off it to wait: 5,383 presses of its switch in 55 minutes. Now, stood on
   the far floor already, she's on it.

**MAP09: two things in the model; not out yet.**
1. *A walk-over line nobody could set off.* The line that lowers a hall's east wall (1006)
   lies on a boundary of the mesh's grid, and the edge along it was a hair off it: the line
   was looked for in the wrong row of cells, the portal had no line, the trigger no ground.
   An edge's lines are now looked for a little round it.
2. *Switches to shoot, from behind a wall.* The blue card's hall has two switches high on
   its west wall that are shot, not pressed (lines 1021, 1022), from the ledge across the
   hall, in front of which that east wall stands till 1006 is crossed. A gun line's ground
   was whatever had a view of it as the map starts. Now a place whose view only sectors
   shut-that-may-open stop is ground too, those sectors its gates (`sight_gates`), as a
   press has had. The exits came into the reach: 330 -> 427 of 513 sectors.
3. *The top of a lift that's passed under.* The lift in the first hallway (sector 10) is
   also a door, and its shaft opens to the north only below the lift's height: it's gone
   through, not ridden (and, the Doom Wiki says, can't be called from the north: go through
   before the backpack's line is crossed and the level can't be finished). From on top of
   it her way north was "a drop" through the wall above the opening, room for her box
   being found 32 past the edge in the hall beyond: 41 minutes of "sector 13 not coming:
   new path". A drop now needs headroom all the way from the ledge to where she lands.
4. *What the walkthrough says of the rest* (pasted by the maintainer): the hall at floor 0
   round (1055, -856) isn't on the single-player way at all (deathmatch starts; no way out
   of it in the map's lines); its middle platform's switch (440) is reached by a running
   jump from the rim, 64 to 64 across 128, which the finder has. The two key routes are by
   teleporters that shut behind her, the yellow one for good. And the stairs in the
   south-west are lowered by the backpack's line (2224) *or* by a switch meant for the
   other players (140): by both, the top step ends 32 under the landing and the way up is
   a sector one unit wide by the wall. She crosses the one and presses the other.
5. *Not out, and why now:* with the level all known she has nothing left to do within two
   minutes of each new start (18 restarts in the hour): no whole route by her planner (below).
   (A rule tried and not kept: no barrel shot from a place with no way back. It kept her
   out of the hall once and she went down by choice later anyway; on E4M5 it cost 218 s.)

**The planner on these maps** (`follow`, the "omniscient plan from there" of a stranding).
"No route to the exit" on MAP06, 09, 12, 15, 17, 25, 28 though the exit is in the staged
reach. It read the lines a goal needs off one way through the level with *everything*
fired, which can be a way only the level's end opens (MAP06: into the last hall by the
exit's teleporter), so the lines of the real way in were never among those searched with,
and the search with every line ran out. Now, when that finds nothing, the needs are read
off the level as it opens (054f552: `View::stages`; each target from what the stage
before had, with the heights of its own stage; the same with no jump made; the ways back
from each line to what it was for; a maker for each part of a height two lines make).
Whole plans from the start on the 100 levels of DOOM, DOOM II and Memento Mori: 78 -> 82
(MM MAP06 and MAP28, DOOM II MAP17 and MAP27). Still none on MM MAP09, 12, 15, 17, 25.

**And it was not what stops her.** The guess was that a level all seen with no plan is one
with "nothing left to do". Measured, no run changed: MAP06, 09, 12, 15, 28 charmed, and
MAP06 and MAP28 with the level known beforehand, came out as they had. What her explorer
goes by is her own reach (everything she *knows* fired), and on MAP09 in the late lives
that reach ends at the two teleporters' pads: she has never learned the ways on from them.
What's left to her there, and on MAP12 the same, is a list of things marked "(risky)" and
dropped at once with "(no route)": MAP09's platform switch (440: a jump onto a platform
with no way back *that she knows of*; pressing it is what makes the way back), MAP12's
"step over line 597" into a sector she believes shut. That is the next thing to trace:
what "(risky)" refuses, and why what's left when nothing else is isn't tried
(`EVIL_DEBUG_NO_ROUTE=1` prints each refusal).

**MAP12: one thing mended, one found and not; not out.**
1. *A lift stowed in a ceiling.* The way down from a room is a lift (sector 125) whose floor
   and ceiling are one height: wall on every side. A line across the room's door calls it
   as she comes in, and so do its own faces (597-599). It came down, went back up while she
   looked where it led, and the door had shut behind her. Its faces were plain walls to
   her. Now the side of something she knows moves, standing as a wall, is a thing to press
   (`on_a_mover`): 144 -> 208 of 226 sectors seen.
2. *The pit by the blue card, and how the game sets off a walk-over line.* The switch that
   opens the blue card's chamber (1068) is down a pit whose rim lines raise posts round the
   pit, for whoever has dropped in (inside, more lines raise the pit's floor again). She
   sets the rim off from outside, every time: going from one 16-wide strip beside the pit
   to another she rounds the pit's corner six units out, never over the pit. Traced tic by
   tic: the game fires a walk-over line when her *box* is on it and her *middle* changes
   sides of the line carried on past its ends (P_TryMove: each line the box met is asked
   which side she's on now and was before). So a line is set off by passing within 16 of
   its end. With the posts up there's no way into the pit, and the level can't be finished:
   "nothing left to do" on the platform, then the platform kept as a trap and refused in
   every life after, and the lives after stranded sooner.
   Tried, none kept:
   - not walking to a spot where her box is out over a drop: out in 2,659 s on one seed
     (skill 3, endless bullets), stuck on the other eleven of twelve, and 11 of parity's
     24 runs went another way;
   - only where the whole poly is rim: no better, and as many runs changed;
   - no portals under 6 wide (the slivers along the rim): she rounds the corner by the
     next way;
   - a guard in the follower that goes wide round the end of a drop's rim: wedged her at
     rims everywhere (15-22 restarts a run).
   What it wants is the way planned clear of a rim's ends by her box, as it is of a wall's,
   where there's room: in the path's funnel, not in the steering. Not done.
   And her model of walk-over lines has them crossed by her middle, on the line itself:
   which ground a line can be set off from, and "couldn't cross it", go by that. The game
   is more generous. Not looked into further.

**MAP27: three things; out on UV, 8 seeds of 8** (charmed; 1,479-2,655 s, one restart in
the eight; it was 4 of 8 at 2e9daec and 1 of 8 in between, by my own change, the first here).
1. *My gun-line rule, too wide.* f585751 offered a door's gun line (893) from outside
   the yellow door of the room before it: shot as that door opened, nothing came of it,
   "tried". Now from behind what's shut only where that's opened *for good*.
2. *A way out that only walking finds.* She's shut in a nukage hall (207) whose way out
   is a door opened by a line across the hall's far end. A floor that hurts is never walked
   to see what walking it sets off. Now it is, as a last resort (`floors_waded_across`).
3. *Held up on a sill.* A switch in an alcove 16 deep at the top of a lift's shaft: pressed
   from the lift, which goes down and leaves her box on the alcove's sill, 400 up. Planned
   from "the floor she's on", nothing was in reach. Now a lip counts for that only if her
   box gets out of its piece of the map by some side; else she's planned from under her
   middle.
How it was found is worth keeping: the survey's one seed said "stuck"; eight seeds on each
build kept from each commit said which commit, and that the level had never been sound
(strandings on every build). **One seed a level says little here: eight, and the count of
restarts, before and after.**

**MAP22: a cave with no way out, by the model.** Twice in the hour she took a teleporter into
a water cave (sectors 315, 316, 327, 328, 333, 339, 341, 548, 549) and was stranded there:
the reach from the pad is those nine and no trigger. The way on is past a pit 128 across
and 32 deep that the cave's floor runs round in strips too thin for her; out of the pit on
the far side is a rise of 32. A run across it would just clear (32 of fall is eight tics;
128 units in eight tics wants full pace), if there's the run-up. Needs the level's
walkthrough before anything is called broken.

**MAP31: not begun.** By the model 30 of 508 sectors can be reached; she sees 122. To trace.

## Still to look at

MAP09 and MAP12 (the "(risky) ... (no route)" drops, above; MAP12 stranded at (5024, -2083), (4645, -4190), (4784, -3564), each
with the exit in the reach from there: her explorer, not the model), MAP17, 23, 25, then the
ones out on some skills only; MAP22 and MAP31 want their walkthroughs; MAP16, MAP31: no
route in principle, the model first. The switch-texture list (`custom-textures-plan.md`):
still not needed by anything met. Then with monsters able to hurt her.
