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
3. *Still open:* charmed with endless bullets she's stranded two to six times in the hour
   and not out, on all four skills: at (1055, -856), a hall at floor 0 under ledges 64 up
   whose only doors are two secret closets (the reach from there is 22 sectors: the model
   has no way out of it, and the map's lines show none either: wants the walkthrough);
   at (632, 441) and (-364, 178). And most of her hour goes on big monsters with a pistol.

**The omniscient planner on these maps** (`follow`, the "omniscient plan from there" line of
a stranding): "no route to the exit" on MAP06, 09, 12, 15, 17, 25, 28 though the exit is in
the staged reach. On MAP06 the relaxed pass can't name the maker of a height that two lines
make between them (the exit's slab, opened by one and its sill lowered by another), so the
lines on the way aren't among those searched with. It doesn't stop her: a stranding is her
own explorer with nothing left to do, and the planner's line there is only for the reader.
Not mended.

**MAP22: a cave with no way out, by the model.** Twice in the hour she took a teleporter into
a water cave (sectors 315, 316, 327, 328, 333, 339, 341, 548, 549) and was stranded there:
the reach from the pad is those nine and no trigger. The way on is past a pit 128 across
and 32 deep that the cave's floor runs round in strips too thin for her; out of the pit on
the far side is a rise of 32. A run across it would just clear (32 of fall is eight tics;
128 units in eight tics wants full pace), if there's the run-up. Needs the level's
walkthrough before anything is called broken.

**MAP31: not begun.** By the model 30 of 508 sectors can be reached; she sees 122. To trace.

## Still to look at

MAP09 (the strandings), MAP12 (stranded at (5024, -2083), (4645, -4190), (4784, -3564), each
with the exit in the reach from there: her explorer, not the model), MAP17, 23, 25, then the
ones out on some skills only; MAP22 and MAP31 want their walkthroughs; MAP16, MAP31: no
route in principle, the model first. The switch-texture list (`custom-textures-plan.md`):
still not needed by anything met. Then with monsters able to hurt her.
