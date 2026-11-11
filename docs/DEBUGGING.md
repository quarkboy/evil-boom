# Debugging Evil

How to find out why a run went wrong, from the evidence the runs leave, before changing anything.
(The rule: trace a concrete failure back to how it was decided; no guessed fixes.)

## Narrow it down first

- **One level, not an episode.** An episode failure on MAPnn is reproduced with `run-map` on that
  map alone, with the same seed and skill.
- **Take combat out of a movement question.** Give her a good loadout, so ammo and weak guns
  aren't what kills her, and see whether the non-combat failure still happens:

  ```
  evil-boom run-map doom2 MAP08 --fight 2 --mortal --lives 1 --seed 1 --carry 287,200,50,0,20,1,100,100,1
  ```

  `--carry` is nine numbers: `weapons,clip,shell,cell,rocket,backpack,health,armor,armortype`.
  `weapons` is a bitmask by weapon type: fist 1, pistol 2, shotgun 4, chaingun 8,
  rocket launcher 16, plasma 32, BFG 64, chainsaw 128, super shotgun 256 (so 287 = fist, pistol,
  shotgun, chaingun, launcher, SSG). Armortype is 1 (green) or 2 (blue).
- **Several seeds, one at a time** (see "the log" below for why not in parallel). Once the fix
  holds with a loadout, check that level from a pistol start or in an episode.
- No full sweeps without the user's go-ahead; up to 8 chosen levels or seeds per test is fine.

## The log: out/explore/\<IWAD\>_\<MAP\>.txt

Every `run-map` writes its whole log here, overwriting it (the latest), and keeps a copy named by
the seed, the commit and the time it started (UTC):
`out/explore/DOOM2_MAP08.s3.e1fed91-20261109-051726.txt` (`-dirty` after the commit: built with
changes not committed). So runs in parallel, or one after another, each leave their own; the
same for the arenas' logs (out/arena), the headless watcher's live log (out/watch/live.log) and
the GPT stand-in's (out/neuro/gpt.log).

What's in it, by line shape:

- `tic N: <objective> (k steps, A along, S straight) | ...`: the explorer choosing an objective (its route's length and its straight line from her: far longer along than straight is a long way round).
- `tic N: dropped '<objective>' (<why>)`: an objective given up.
- `    [  12.3s] ...`: the route follower (steps, jumps, "landed at", "sinking: running on",
  "running at H degrees off sector S, for sector T (why)", stuck, replans).
- `  [fight] tic N: ...`: combat (target, mode changes, weapon switches, "holding fire: why",
  "fight over (k kills; r rounds spent, d damage done)").
- `tic N: hit for H (and A armor) by <source>`: **every hit on her**, as the engine applied it.
- `tic N: killed at (x, y) by <source>: health, armor, weapons, ammo; in sight: ...; doing: ...`
- `damage taken: <source> H (+A armor) at tic T (Ns), T2, ...; ...`: the run's damage by
  source, most first, near the end of the log. Run-map prints the same as its `DAMAGE` line, and
  `evil-boom episode` prints it under the level a seed ended on.

Damage sources (from the engine's own damage call, dsda `P_DamageMobj`, patched):
- a monster's name (`ShotgunGuy`, `LostSoul`): its bullet, bite or charge;
- `Cyberdemon's Rocket`, `DoomImp's DoomImpBall`: a missile, with whose it was;
- `her own Rocket`: her own splash;
- `a barrel set off by her` / `a barrel set off by Cyberdemon` / `a barrel`;
- `a crusher` (she didn't fit between floor and ceiling), `the floor` (a hurting floor),
  `the level` (death lines and the like).

The tics in the summary are where to look in the log: `grep -n "tic 1583" ...`.

## The decisions: out/explore/\<IWAD\>_\<MAP\>.decisions.jsonl

`--decisions` on a run keeps a journal beside the log, a line of JSON a decision: `point`
("target", "move", "retreat", "objective"), `chosen`, the `options` weighed with their scores
where the rule compares any (a target's: higher better; an objective's weighed way there:
shorter better), and `state`, the fight's assessment that tic (health, threat, outmatched, what's
against her by name, distance and health). A "move" is logged when the fight's move changes, a
"target" when it does. The log is the same with it on.

To see what a decision is worth, take it away: `--bar Standing,Cover` (any `Move`s and retreat
kinds, by the names the journal gives; a wrong name lists them) plays the run with those passed
over. `Player::force_target` does the same for a target, from code.

## Seeing the spot

- `--snapshot S` stops after S game seconds and draws the viewer to `out/watch.png` (the game
  view, the map of what she knows, the log). Add `--focus X,Y,ZOOM` to centre the map. Read the
  PNG to look at it.
- `evil-boom watch <wad> <map> [run options]` to watch it live; `--headless` checks that the watcher
  plays as the run did.
- `evil-boom sector <wad> <map> <sector>...`: a sector's heights, special, tag, lines (with their
  specials and tags), things, and the run-jumps landing there. Use it to answer "what lowers this
  floor", "what's at that corner", and so on. Line specials are Doom's numbers (37 = W1 floor
  lower to lowest neighbour and change type; 16 = sector special: 20% damage).
- `evil-boom dump-nav <wad> <map>`: the level's analysis.
- `EVIL_DEBUG_SHOT=1`: in the fight's log, where she and the target are (and their sectors)
  when fire's held for no shot, every bullet or shell out with the gap since the last, and
  each one that left with nothing on her line (what was in sight, how far off her aim).
- `EVIL_DEBUG_SCOUT=1` (with `--known`): keeps the log of the pass beforehand.
- `EVIL_DEBUG_REACH=<tic>`: her reach at that tic (polys, portals out of it); with
  `EVIL_DEBUG_HEIGHTS=58,59` those sectors' heights too: real, last seen, believed.
- `EVIL_DEBUG_JUMPS_INTO=<sector>`: the jumps the planner has into a sector.
- `EVIL_DEBUG_USE=1`: every use press in the knowledge events: where, facing (after the tic's
  turn, as the game traces it), the line the model has it hit.
- `EVIL_DEBUG_DEAD_END=1`: for a drop refused as a dead end: the sectors she'd have after it
  and what she has left to try, by where it's pressed from.
  `EVIL_KNOWN_ONLY=map|monsters`: only that much known.
- What her shots hit is in every run's STATS, by the game's own count (the engine patch):
  `shots, by the game's count`, `shots with no pellet on anything`, `pellets fired`, on a
  monster, on a barrel, after the kill in the same shot, on nothing, and fired with the
  trigger held.

## Going back over a run

- Every `run-map` records its run to `out/runs/<IWAD>_<MAP>.run`. `--from FILE@S` plays that
  recording to S game seconds, then hands over to the bot as it is now: a fix can be tested from
  just before the failure.
## Driving her by hand: `evil-boom drive`

`evil-boom drive <wad> <map> --script FILE [run options]` plays a script of commands, one a line
(`#` comments), printing what comes of each. With `--from out/runs/<IWAD>_<MAP>.run@S` it starts
from the moment S of a recorded run, so you can stand her where things went wrong and try things.
Her knowledge keeps taking in what she sees the whole time. The full list is at the top of
src/drive.rs; the most useful:

- **Where things stand:** `where` (position, sector, keys, tally); `sector S...` (heights now and
  as she believes them); `line L...` (what she knows of lines); `items`; `exit?` (any way out
  from here, knowing the whole map).
- **What she'd do and why:** `objectives` (her options, numbered, each with its route); `next`
  (let her pick and do it); `pick N` (make her do option N instead); `force visit X Y`,
  `force wall LINE`, `force item X Y` (give her an objective she wasn't offering).
- **Why she can or can't get somewhere:** `reach X Y [plan]` (how the flood fill her options
  come from gets there: the polys on the way and how each was entered; `plan` for the
  planner's first check); `fits X Y` (does her box stand there at the heights she last saw);
  `path`; `budget N` (route search depth); `stats`.
- **Moving her:** `goto X Y` (her own planner and follower, on what she knows); `goto-full X Y`
  (routed knowing the whole map); `warp X Y [ANGLE]`; `face X Y`; `move F S T N` (raw controls for
  N tics: try a jump exactly); `use`; `fire N [SLOT]`; `wait N`; `bot N` (the bot plays N tics);
  `power N [TICS]`.
- **Looking:** `snapshot NAME [X,Y,ZOOM]` writes the viewer to out/drive/NAME.png.

Example: from just before the MAP08 platforms, is there a way across from the ledge?

```
# evil-boom drive doom2 MAP08 --fight 2 --from out/runs/DOOM2_MAP08.run@376 --script s.txt
where
sector 39 40 41 54 89 88 85
objectives
goto 1440 2930
where
snapshot after 1440,2800,4
```

## Temporary instrumentation

When the log doesn't say enough, add a temporary log line at the decision point, limited to the
tics in question, e.g. `if (13195..=13260).contains(&ob.tic) { log.push(format!("DBG ...")) }`.
Print the inputs to the decision (positions, heights, velocities, what each candidate scored),
not just the outcome. Prefix it `DBG` so it greps out, and **remove it before committing**:
copy the file aside first (`cp src/x.rs /tmp/x.bak`) and copy it back.

Useful spots:
- combat fire decisions: `aim_and_fire` in tactical/combat/aiming.rs (what she fired at, what was
  near, the missiles in flight);
- the sweep loop around `e.step(&ctl)` in sweep.rs (her position, z, sector heights, the controls
  sent, tic by tic);
- the run over sinking floors: `run_heading` in motor/follower/jumping.rs (each candidate
  heading's landing and score).

## Before calling something unreachable or broken

Look it up (DoomWiki, secrets lists). DoomWiki and Fandom refuse automated fetches; ask the user
to paste the page text, they're happy to. Then confirm with `drive` or a snapshot.

## Worked example: MAP08 deaths with a good loadout (2026-11-07)

1. 8 seeds of `run-map ... --carry ...`: all died. `killed at ... by <source>` and the damage
   summaries split them up: the Cyberdemon's rockets, the floor (the pit), and `her own Rocket`
   (70-140 damage in 7 of 8 runs).
2. Her own rocket, tic 1583: temporary logs of each shot and the rockets in flight showed a
   rocket that moved 9.5 units in its last tic and stopped with nothing there, 38 units out.
   `evil-boom sector` and the lines around it put the corner of her alcove's jamb 14.5 units off the
   rocket's path. Doom moves missiles as axis-aligned boxes, so an 11-radius rocket at 115 degrees
   reaches 14.6 units to the side. The lane check used a fixed 13. Fix: the lane's half-width from
   the box's real reach at that heading.
3. The pit: per-tic positions showed her crossing the sinking platforms at full speed as meant.
   The route then ended on a platform (a look-at-wall goal), she stopped there, and it sank. The
   sinking-floor run judged every way off by the path back to that goal, which was going down
   too. Fix (the user's rule): once the floor's going down, make for the nearest solid ground she
   can reach (the pit's banks, not rooms behind its walls), keeping her momentum.
