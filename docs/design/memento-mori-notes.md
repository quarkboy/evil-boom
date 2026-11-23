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
| the loader and the gate rule (below) | 13 |

By the omniscient planner at 0.9.1, 17 of the 32 had no route: MAP04, 06, 09, 12, 15, 16, 17,
19, 21, 23, 25, 26, 28, 29, 30, 31, 32 (of which she got out of 19, 29 and 32 anyway, by
exploring).

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

**MAP05: open.** Two things seen, neither mended yet:
- The lift that is a whole room's floor (sector 112, tag 9) is called from a wall in a
  recess eight units deep in the nukage moat (line 717), faced in the same metal as the walls
  beside it. Mapped, never pressed. A rule tried (a use line at the back of a niche too
  shallow to stand in is offered) didn't get it pressed: the place to press from is in the
  moat, with no way back that she knows of. Not kept.
- She wedges at (-288, 22) against the corner of an island in the moat, "stuck" six times a
  life, and starts again.

## Still to look at

MAP06, 09, 12, 14 to 18, 20 to 23, 25, 26, 28, 30 (the Icon of Sin), 31; then skills 1, 2 and 4
(the same maps with other things in them); then with monsters able to hurt her.
