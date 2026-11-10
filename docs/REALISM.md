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
| O8 only the things lying about that she's seen | 0.448 | -0.0034 ± 0.0048 | 807 / 96 / 99 | E2M7 HMP -0.34, MAP06 HMP +0.21, E3M2 UV -0.20 |
| O1 only the missiles she's noticed | 0.435 | -0.0130 ± 0.0085 | 259 / 243 / 269 | E4M5 UV -0.37, E3M5 UV -0.34, MAP14 HMP -0.27, E3M9 HMP +0.25 |
| O2, O3 monsters out of sight as she last saw them; deaths seen or heard; one not where she left it is lost | 0.439 | +0.0047 ± 0.0071 | 439 / 196 / 198 | MAP05 HMP +0.32, E2M7 UV +0.30, MAP32 HMP -0.23 |
| O4 a monster's health her reckoning | 0.439 | +0.0000 | 1088 / 0 / 0 | none: no rule went by the number itself, only by its going down in her sight |
| O5 whom a monster's after by which way it's turned | 0.436 | -0.0028 ± 0.0071 | 459 / 198 / 201 | E3M5 UV +0.28, MAP18 HMP -0.22, MAP25 HMP +0.21 |
| The engine reporting its sounds (nothing reading them) | 0.436 | +0.0000 | 1088 / 0 / 0 | none: the patch changes nothing in the game |
| Hearing 1: a death by its cry, firing by an attack heard (the game's sounds) | 0.432 | -0.0049 ± 0.007 | see the commit | E3M4 HMP +0.24, E4M8 HMP -0.22, MAP10 HMP +0.22 |
| Hearing 2: turning to look at a sound, a hit, a bump (the shortcuts still in) | 0.426 | -0.0051 ± 0.0096 | 161 runs the same to the tic, 288 better, 288 worse | E2M8 UV +0.50, E1M5 UV -0.34, E2M1 UV -0.31 |
| The reveal when hurt taken out (a hit gives a side to look) | 0.429 | +0.0029 ± 0.0066 | 491 / 198 / 180 | E2M8 UV -0.25, MAP10 HMP +0.23, E3M6 UV +0.22 |
| Near taken out for missiles (noticed only in her view) | 0.431 | +0.0019 ± 0.0084 | 345 / 237 / 241 | E4M3 HMP -0.49, E1M5 UV -0.44, MAP11 HMP +0.25 |
| Near taken out for monsters (seen only in her view) | 0.375 | -0.0566 ± 0.0104 | 50 / 292 / 414 | very large: traced, see below |
| ...with the looking's two faults fixed | 0.382 | -0.0497 ± 0.0106 against before near came out | 32 / 299 / 411 | E1M7 UV -0.44, E3M2 HMP -0.43, E2M2 UV -0.38 |
| Memory: monsters seen lately kept in mind, counted 1.5 times | 0.370 | -0.0112 ± 0.0099 (-0.0609 ± 0.0102 against before near came out) | 27 / 374 / 377 | E2M8 HMP -0.62, E3M6 HMP -0.51, E1M1 UV -0.42 |
| A hit from the side turned to with missiles in the air and before a monster lost from sight (the user's watching of E1M6) | 0.390 | +0.0201 ± 0.0066 | 558 / 192 / 130 | exits 290 to 318; health lost to close attackers out of view 31.3 a run to 28.1 |
| No stopping to look at a noise on a floor that hurts (the user's watching of E1M5) | 0.386 | -0.0037 ± 0.0021 | 1006 / 16 / 23 | 82 runs touched; kept as the sounder rule, though it scores a shade under |
| A monster's footsteps not taken for attacks (work list A1) | 0.383 | -0.0032 ± 0.0028 | 935 / 35 / 48 | a correction; nothing's done with a footstep yet |
| One hand turns her: no faster than 18 degrees a tic, its rate changed by at most 10 a tic (the user's: a jitter tracking an enemy near cover) | 0.399 | +0.0164 ± 0.0101 | 19 / 381 / 347 | jitters 0.29 a game minute to 0.16; at 6 a tic: 0.08, score -0.0105 ± 0.0101 |
| Instinct: a monster known where it is for half a second after something comes between (the user's allowance; work list C1) | 0.398 | -0.0019 ± 0.0100 | 27 / 332 / 368 | nothing either way; a full second: +0.0021 ± 0.0100. Kept at half a second |

O8's first score was -0.0157 ± 0.0077 with 664 runs changed and MAP03 down 0.4 on both skills.
Traced: not the rule. Things were noticed from her view as it was the tic before (the explorer's
eyes took the tic in after the percept was made), so everything about items came a tic late, and
a tic's shift early in a run plays the rest of it differently. An instrumented build found the
rule itself differing from the old one 15 times in 48 runs, each a drop from a monster dying out
of view. Her eyes now take the tic in first (`Explorer::sense`, on the game as it is), and the
traced MAP03 run is line for line as it was before the change. The lesson for the rest: a
change that moves most runs when its rule should touch few has a timing shift in it; look at
the count of runs the same before the score.

O1 changed 829 runs, which looked like O8's timing shift again; it isn't. Traced on E4M5 UV
(an exit before, a death now): the first difference is at tic 118, a cacodemon's ball "due in 34
tics, 10 off" dodged before, from a cacodemon she had never seen and with no line of sight to the
ball; now it isn't noticed. Missiles from what she can't see are common, so the rule touches
most fights: this cost is the rule's own.

## Hearing and feeling: bearings, then the shortcuts out (the plan, 2026-11-09)

Found after the survey (the user's questions): three shortcuts stand in for senses she didn't
have. Hurt, she noticed everything in line of sight all round for two seconds. A monster or a
missile within 384 units with nothing between counted as seen whichever way she faced ("near":
written as "heard", but Doom has no sound of a missile in flight, and a monster standing still
makes none). And that near rule was what kept her from backing into a monster: with it gone, one
behind her isn't known, and she'd push against it till the stuck rule turned her another way,
never knowing what it was.

What a player has instead, each a direction to look and no more:

1. A sound from something not in view (a launch, a cry on seeing her, an attack): its side by
   the balance between her ears, front or back unknown, so the way she isn't already looking.
2. A hit: the status bar's face looks left, right or straight at what hit her (the engine's hit
   record gets that: the push of a small hit is no bigger than her own stride).
3. A bump: stopped while pushing where the map she knows has no wall: something's there, the
   way she was pushing.

Not a wandering monster's growls (146 of an imp's in three runs: she'd never stop turning).

One behaviour for all three: turn to it, and see what her eyes then show. Then the shortcuts
out, one at a time, each scored: the reveal when hurt; near for missiles; near for monsters
(the dearest, likely: it's how she knows of anything close behind her).

Near taken out for monsters was the very large drop (0.431 to 0.375). Traced on MAP07 HMP: two
faults in the looking (a look started again by each new sound, its ways reckoned from mid-turn,
so never finished; a hit's side looked to 90 degrees and not on round behind), fixed, for 0.007
of it back. The rest is what near had been besides: her memory. Everything she reckons in a
fight came from what's in her view now, and a second after; a monster she saw two seconds ago
and turned from was gone from the threat, from outmatched and surrounded, from the ways her
steering keeps clear of. Next: monsters seen lately kept in mind where she last saw them (the
user's: and counted for more than one in sight; one you know is round you and can't place is
the bigger threat).

Memory didn't bring it back: -0.011 ± 0.010 on its own (about nothing), -0.061 in all against
before near came out. So the guess that near's cost was her memory isn't borne out, as built.
E1M1 UV (-0.42) traced: not memory. A shotgun guy 1600 off through a gap, in her line of sight
on one tic and out of it the next as her legs moved her a unit either way: "shooting on the
way" (the route's legs) and "lost sight: keeping at it" (the fight's) by turns for a thousand
tics, 44 rounds spent for 5 damage, then out of ammo for the rest of the level. One of the
loops of DECISIONS.md's step 5 (a decision undoing its own condition), there before; with
nearly every run played differently now (27 of 1088 the same), which runs meet it is chance.
What near's 0.05 is made of isn't known yet.

## What near's 0.05 is made of (measured, 2026-11-09)

Two sets on the same build and seeds, each run counting what hurt her and what she knew of it
(`STATS`, `score`'s FILE.stats.jsonl): as she is, and with `senses.see_near` set back to 384.
Score 0.370 against 0.421 (-0.0505 ± 0.0099); exits 290 against 346 of 1088.

Health lost a run, by what she knew of what hit her:

| | near on | now |
|---|---|---|
| in view | 101.6 | 83.0 |
| not in view, within 384, to a side (by the face) | 0.3 | 31.3 |
| not in view, anything else | 7.6 | 8.3 |
| the floor, barrels, her own, crushers | 29.4 | 26.2 |
| all | 139.0 | 148.8 |

The whole of it is one line: close attackers out of her view, 31 health a run that was none.
Of the 31: 17 from monsters she'd never seen, 8.5 from ones seen but out of mind, 6 from ones
remembered; bullets 12.8, claws 10.3, missiles 8.2; the same on both skills. She's alerted by a
hit from the side 2.1 times a run. Nothing else moved much: ammo and damage dealt fall with the
shorter runs.

So: monsters come up beside and behind her that she never looks at. A player looks about, and
hears one that's close: a wandering monster's noise was left out of what turns her (there's
always one somewhere), but a loud one is a close one. `senses.heed_active_volume` and
`senses.heed_volume` are the loudness a sound needs to turn her, to be tried by tuning file.

## Watched runs: the user's verdicts (2026-11-10)

The user watching the runs that die from close attackers out of view (the list above), one at a
time. To act on once the watching's done, not before.

- **MAP04 HMP, seed 180954446** (dies at 48 s). The first corridor: a room full of shotgun guys
  behind a "window" that goes up and down, so they come in and out of sight over and over (and
  it makes noise). Dangerous in itself. The user's idea: a little **instinct**: she goes on
  knowing where an enemy is, as it is, till it's been truly out of sight for about half a second
  (17 tics or so). "It's cheating a little bit but our goal is good streaming content."
- **E1M1 UV, seed 1861030831** (dies at 50 s). Ran into a room and let herself be flanked on
  two sides by two shotgun guys. She shouldn't have gone forward to where she was more exposed:
  she should have fought from the corridor behind the door, which she knew was safe. And she
  made it worse by running on into ground she hadn't explored, waking more shotgun guys.
- **MAP11 HMP, seed 1461101707** (dies at 37 s). She starts shooting at lost souls far off that
  are harmless, behind bars she can see, and lets a cacodemon and a chaingunner come up on her.
  A level with a very tricky start ("it's right there in the name"): the best play is to get out
  of the wide open exposed place with enemies all round, and fight her way into one of the side
  rooms, where she at least has a wall at her back.
- **E2M3 UV, seed 574408264** (dies at 55 s). She chose to fight a baron with a pistol rather
  than try the risky but necessary run past it. A demon helped trap her too, but it was mostly in
  view (the user's, pretty sure).
- **E3M3 HMP, seed 2075529707** (dies at 62 s). She kept backing up to dodge or keep off
  enemies, waking more of them in the rooms behind her, which then came up behind and hit her.
  She needs a strong pull to stand her ground and fight closer to the enemies, and to dodge
  missiles sideways, so as not to go into ground she hasn't explored while fighting.
- **MAP07 HMP, seed 1033029263** (dies at 80 s). A map of its own kind, but it shows weaknesses.
  First: picking up the blur sphere makes the map harder: it makes the missiles aimed at her
  more random. A rule not to pick one up with mancubi or arachnotrons about might be good.
  Second: pressing a switch while fighting needs work. Her dodging takes her further from the
  switch she's going for (and should press: it gives her the better weapons); after dodging
  there are switches nearer that might be easier, but the objective isn't changed from the one
  she first chose. So the mancubi get killed, which lets all the arachnotrons out, with her
  still on the super shotgun: no megasphere, rocket launcher, plasma rifle or chaingun.
- **E3M6 HMP, seed 1858596759** (dies at 104 s). Just unfortunate. A trap with an
  invulnerability sphere you all but land on: she jumped over it, the monsters swarmed her, and
  she died before getting to it. The monsters were all low-level ones, just spawned all round
  her.
- **Seen across the runs, on how she fights** (the user's, with E3M6): breaking line of sight
  behind a doorway, stepping left and right, she goes very far to each side, much further than
  it takes. Tighten it to only as far as needed: again so as not to wake more enemies.
- **E1M6 UV, seed 499438419** (dies at 124 s). Strange: it looks as if something goes wrong.
  At about 90 s she waits in front of a lift, trying to pick up a medikit and failing; then,
  giving up, she backs into a shotgun guy and never turns round, even hit point blank several
  times. The user's: deserves a careful trace.
  Traced. Two things. (1) The medikit is on a lift she'd sent off herself, crossing its line as
  she backed away from a spectre; she then waited 20 s "at the mover" for a lift that doesn't
  come back by itself, and only afterwards tried its switch. A wait for a lift that has no
  reason to come: to fix with the follower's waits. (2) The shotgun guy: 43 units off at her
  side, three blasts, never looked at. A hit from the side was only turned to with a target in
  sight and no missile in the air: here imps' fireballs were flying (so not then), and with
  the imp gone from sight "lost sight: keeping at it" came before the look (so not then
  either). Fixed: a hit's turned to with missiles in the air (a dodge is her legs', her eyes
  are free) and before a monster she's lost sight of.
- **MAP02 HMP, seed 756527427** (dies at 126 s). There was a megaarmor she saw and didn't pick
  up: she went into a teleporter instead. It would have helped a lot.
- **E3M2 UV, seed 82960982** (dies at 151 s). Did well till she set off a trap; then, rather
  than run back to where it was fairly safe, she ran forwards, opening up still more monsters,
  and that was that.
- **E1M5 HMP, seed 595929723** (died at 185 s; with the hit-from-the-side fix she gets out).
  Fine now. Some needless damage on the nukage in the middle: her movement interrupted by a
  noise, it seemed.
- **E2M8 HMP, seed 1243369520** (dies at 191 s). "A comedy of errors." She fails to pick up the
  rocket launcher for a long while, and by the time she has it she's no bullets left for the
  lost souls. Then she fixes on looking at a wall in one of the rooms with rockets, with the
  cyberdemon stuck in that same room; the objective's never changed (as with MAP07's switch),
  so she ends up squeezing into the room with it, "and that goes about as well as you'd
  expect".
- **The user's, on monsters heard walking**: the cyberdemon's steps are a sound of their own,
  constant, heard all the time. Worth having in the logic for any enemy with a walking sound
  like that (see the engine): she should have a much better sense of where it is, and hunt it
  more proactively than she does.

## The work list from the watching (2026-11-10)

Twelve runs watched by the user; their verdicts above. Grouped, in the order to do them. Each
is its own change, scored against the build before on the same runs.

**A. Things that are broken** (concrete, cheap)
1. A monster's walking sounds (the cyberdemon's hoof and metal, the spider's metal, the
   arachnotron's steps: played by the chase itself) are taken for attacks: she turns to them as
   to an attack and takes the monster to be firing.
2. An objective isn't thought again after the fight has moved her (MAP07's switch with nearer
   ones by her; E2M8's wall in the cyberdemon's room).
3. Waiting for a lift that has no reason to come back (E1M6: 20 s at one she'd sent off).
4. A monster on the edge of a line of sight, in and out of it each tic: her legs passed to and
   fro, a pistol fired at 1600 units (E1M1: 44 rounds for 5 damage).
5. Whether "in view" is too strict at the edge (E2M3's demon, "mostly in view", counted unseen).

**B. Where she fights** (four of the twelve: E1M1, MAP11, E3M3, E3M2)
1. The fight's own movement keeps to ground she's been on or seen: a dodge sideways, not back
   through a door into rooms she's never been in.
2. When it turns against her (a trap sprung, flanked, outmatched): back the way she came, to
   ground she knows, and make them come to her through the door.
3. In the open with enemies round her: to a wall at her back, a side room.
4. Stepping out of sight at a doorway: only as far as it takes.

**C. What she knows**
1. Instinct (the user's allowance): an enemy is known where it is till it's been truly out of
   sight about half a second.
2. A monster heard walking is kept track of by its steps (side and loudness each step), and
   hunted on her terms.

**D. What to fight and what to take**
1. Not shooting what can't get at her (lost souls behind bars) with real threats coming up.
2. Outmatched with nothing but a pistol against a baron: the run past, not the fight (to trace:
   was she judged outmatched?).
3. An invulnerability sphere when the crowd as a whole is too much, not only when one monster
   is.
4. No blur sphere with missile-throwers about (it spoils their aim, and so her dodging).
5. Armor in reach before a one-way step (MAP02's megaarmor left for a teleporter: to trace).
6. The rocket launcher left a long while (E2M8: to trace).
7. Small pickups across a floor that hurts, and fighting on one (E1M5).
