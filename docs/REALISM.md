# Realism: where she still goes by the level's truth

A survey (2026-11-09) of every place Evil decides by something a player at the screen couldn't
know. The rule (the user's, 2026-11-09): the fight goes by what she has seen, heard and learned,
as the exploring does. Each item below is to be made realistic one at a time, after the
restructuring, and scored; a very large drop means look for a bug in the change first.

## How far the survey went

- **Read in full:** every direct read of the game's live state and of the map in the fight and
  the player (`tactical/`).
- **Read at each use:** the same reads in the follower (`motor/follower*`), the explorer and
  the knowledge model (`knowledge/`), by search for each source of truth.
- **Not audited:** the knowledge filter itself (`navigator/view*`: what a `View` lets through),
  the navigator's analysis, the secret-wall rubric, and the directed mode (`control/`), which
  lists things for whoever is ordering her about.
- **By design, left alone:** the planner underneath is omniscient and knowledge filters it
  (CLAUDE.md); what monsters and guns do (`bestiary`) is a player's knowledge of the game; the
  Icon of Sin's brain (the user's: she may know of it); the kill, item and secret totals (the
  automap's).

## The two ways truth gets in

1. **The observation** (`motor::Observation`), handed whole to every layer each tic: every thing
   in the level (`things`), every sector's floor and ceiling as they are now (`heights`), every
   barrel (`barrels`). Whoever reads it sees through walls unless it filters for itself. The
   fight filters monsters into `visible` and barrels into `barrels_seen`, and then reads the
   unfiltered lists as well.
2. **The map** (`Navigator`): damage values, what every line does, the geometry of places she
   hasn't been. The explorer reads it through `View` and `Knowledge`; the fight and the follower
   read some of it straight.

The first is best closed in one place: a **percept** made once a tic from the observation and
what she knows (the things in sight, and where she last saw the rest; the heights of sectors in
view, and as last seen for the rest; what she heard), given to the fight and the follower instead
of the observation. Then only the map is left to leak, use by use.

## What she can't hear yet

The realistic stand-in for several items is hearing, and she has little: she hears floors and
ceilings start and stop (worked out from heights changing within range,
`knowledge/model/perception.rs`), and reckons who hears her own shots (`tactical/sound.rs`).
She doesn't hear monsters: waking, attacking, in pain, dying, or a missile fired. Doom plays all
of those within 1200 units with no walls in the way of sound. Two ways to give her them: work
them out from things' states changing within 1200 (no engine change; the same trick as the
floors), or have the engine patch report the sounds it starts (exactly what a player hears; an
engine rebuild). To decide when the first item needing it comes up; the second is the truer.

## The observation's leaks

| # | What she knows that she shouldn't | Where | Realistic instead | Cost (a guess) |
|---|---|---|---|---|
| O1 | **Missiles anywhere**: the dodge looks at every missile in the level heading her way, behind her or through a wall | `combat/movement.rs` `incoming` | those in sight (in view, or near: a rocket's hiss is heard); one from behind is a hit | small to medium |
| O2 | **Monsters out of sight, as they are now** (`alive`: every monster, each tic): a seen monster's death out of sight (the kill counted, its hunt dropped); a sniper out of sight firing or not (`hold_at_edge`: the comment says heard, the code reads its state); the monster just lost sight of (its live state behind the last-seen place) | `combat.rs` `observe`, `remaining`, `lost_sight`; `player.rs` `hold_at_edge`, `gone` | last seen only; a death heard (or the corpse seen); firing heard | medium: hunts for the dead, steps into a burst |
| O3 | **Any monster close by, seen or not**: a rocket's lane ("seen yet or not"), a jump's "body ahead", who was close when she was hit (the death's cause) | `combat/aiming.rs` rocket lane; `motor/follower/jumping.rs`; `player.rs` `plight_now` | what's in sight (and what's touching her: felt) | small |
| O4 | **Exact health of monsters**: the melee give-up's "has it been hurt", alive-or-not checks, damage dealt | `combat/tree.rs` `melee`; `combat.rs` `observe` | her own reckoning (`put_in` already keeps what she's put into each), flinches and deaths seen | small |
| O5 | **Whom a monster is after** (`after_her`, the game's own target): what's fighting her and what's fighting each other, a sniper's ground she can cross | `combat.rs` `engaged`, `watch_infighting`; `combat/tree.rs`; `player.rs` | which way it faces, moves and shoots (the infighting watch already goes half by that) | small to medium |
| O6 | **Every sector's floor and ceiling, now**: crushers found anywhere in the level as they move; the fight's sight lines and footing past what she can see; a retreat's route (doors and lifts as they are, in view or not); the follower's timing of lifts, doors and jumps; the reach of her own shots' noise | `combat.rs` `observe` (crushers), `heights`; `player.rs` cover and post routes; `motor/follower*` (some 45 reads); `tactical/sound.rs` | in view: as they are; else as last seen (`Knowledge::seen_heights`, what the explorer plans on), with a mover heard | unknown, likely the largest: a lift or door out of view misjudged |
| O7 | **Barrels anywhere**: chains reckoned, lanes and ways kept clear with every barrel in the level, though `barrels_seen` is kept | `combat/aiming.rs` (5), `combat/steering.rs`, `combat.rs` `size_up`; `motor/follower/walking.rs`, `barrels.rs` | `barrels_seen` | small |
| O8 | **Items in places she's seen, as they are now**: an item counts as known when its spot is known, so what a monster drops dying out of her sight is known at once, wherever she is | `player.rs` `supplies`, `errands`; `knowledge/explorer/objectives.rs`, `selection.rs`; `knowledge/completion.rs` | items she's seen lying there (a list kept as she looks, as monsters are) | small |

## The map's leaks

| # | What she knows that she shouldn't | Where | Realistic instead | Cost (a guess) |
|---|---|---|---|---|
| M1 | **Which floors hurt, in the fight**: the map's damage values | `combat/movement.rs` `hurts` | **done 2026-11-09**: what she takes to hurt (felt, else by its look; none under a suit) | being scored |
| M2 | **Which floors hurt, elsewhere**: a jump's landing and footing, "burning" while stuck or waiting at a door, where to shoot a barrel from, the wading clock; with no deaths (not mortal) she learns a floor from the map | `motor/follower/jumping.rs` (5), `walking.rs`, `interactions.rs`, `barrels.rs`; `knowledge/explorer.rs`; `knowledge/model/perception.rs` | `Knowledge::damaging` | small |
| M3 | **How much a floor hurts** before she's felt it: the map's 5, 10 or 20 a hit, in a way's cost | `knowledge/model/perception.rs` (`said`) | what a floor of that look has done to her, the worst till she's felt one (the rule already used where the map says nothing) | small to medium: ways refused or taken wrongly at first |
| M4 | **What a line will do, unlearned**: floors a walked-over line lowers into something that hurts; which sectors only doors move, which are lifts | `combat.rs` `new` (`sinks_into_harm`, `doors`, `lifts`); `motor/follower/walking.rs` | what she's learned lines do (`Knowledge::learned`), a door by its look | small |
| M5 | **Ground she hasn't seen, in the fight's moves**: the way round a corner by the mesh, room to run, a dodge's landing, are over the whole map; only cover and posts keep to what she knows | `combat/sight.rs` `hidden_way`; `combat/movement.rs` `clear_run`, `fits`; `combat/steering.rs` | the polys she's seen (`Knowledge::known_polys`) | small: a fight is mostly where she can see |
| M6 | **Where her shots are heard**: the noise spread through the whole map, sound-blocking lines and all; only the sleepers she's seen are counted | `tactical/sound.rs` | spread over the sectors she knows | small |

## Suggested order

Smallest and safest first, the one with the unknown cost split up and last:

1. M1, the fight's hurting floors (done; its score is the first number).
2. The percept for monsters and missiles: O2, O3, O1. This needs the hearing decided.
3. O7 and O8: barrels and items she's seen.
4. O4 and O5: health and targets by what she sees them do.
5. M2, M3, M4: the map's damage and lines' effects outside the fight.
6. M5, M6: unseen ground and her own noise.
7. O6, a piece at a time, each scored: crushers seen or heard only; the fight's footing and
   sight lines; a retreat's route; the follower.

Each is one commit with its score before and after (two measurements a build where the change
is small: the score's noise is about 0.008 a set).

## Results

Each change scored on the same 1088 runs as the build before it (`score --like`), run for run:
the mean difference in score, its standard error, and how many runs changed at all. The score
before any of them: 0.445.

| Change | Score after | Run for run | Runs the same / better / worse | Moved most |
|---|---|---|---|---|
| M1 the fight's hurting floors by what she knows | 0.445 | (not paired: 0.448 before, seeds of its own) | | |
| O7 only the barrels she's seen | 0.451 | +0.0057 ± 0.0044 | 837 / 66 / 42 | E1M8 UV -0.28, E1M4 UV -0.22, E1M5 UV +0.20 |
