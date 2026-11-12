# Fighting: from first pieces to the level campaigns (2026-11-05 to 11-09)

The fight, built up from dodging and gun choice to a decision tree, then hardened level by level: crushers, barrels, rockets and autoaim, bosses, the BFG, and the campaign rounds and 20-minute passes that traced every death and stuck run to a general rule.

## Fighting: first pieces (2026-11-05)
- Native `LineOfSight` (ZDoom's P_CheckSight without REJECT): who can see whom.
- `Navigator(skill=)`: which things are on the map at the skill played.
- Observations carry the things about (monsters, missiles, items, by ZDoom class name),
  weapons held, ammo per slot and armour; controls select any weapon slot.
- `tactical.bestiary` (monster sizes, toughness, attack kind, threat; missiles; weapons),
  `tactical.combat.Combat` (noticing, targets, weapon choice, aim with lead, dodging missiles,
  backing off from clawing monsters, strafing when it's hot, shooting on the move otherwise),
  `tactical.player.Player` (explorer + fight; items she needs and has seen as objectives;
  monsters seen still about hunted once there's nothing else).
- Testing starts at lower skills (HNTR/HMP) and works up to Ultra-Violence (the maintainer's call).

## Fighting: dodging, guns, shots that land (2026-11-05)

From watching her play (`tools/watch.py --fight SKILL --mortal`):
- **Dodging** predicts each missile against where she's going (her velocity from the engine), not where she stands; tries 16 ways, simulating a running player's push and Doom's friction, and takes the one that keeps her furthest from every missile coming; a missile is dodged till it's gone by (before, she stopped half way out of its path and walked back in).
- **Weapon choice** is the time to kill the target: Doom's damage, refire and spread (the share of pellets that land on something that wide that far off), plus the switch (0.8 s) and what the ammo's worth (rockets dear, more so when few). The super shotgun is told apart (WEAPON3 counts both shotguns; slot 3 from another slot brings up the SSG). Before, rough dps with range cut-offs had her flip between shotgun and pistol in mid-range fights.
- **Shots that land**: she fires only if a shot from her gun (36 above her feet) reaches some part of the target within autoaim's slope (100/160) with nothing in the way (the native sight test, the target's span clipped to that slope); else it'd go out level, into the lip of the ledge the imp stands on. With one monster about that she can see but not hit, she steps back from it (above her) or towards it (below).
- **Out of reach**: against hitscanners (they hurt from anywhere) she fires past a gun's reach while a fifth of a shot lands; when standing to fight something still out of reach she closes in, zig-zagging (she used to strafe at a chaingunner far off, never firing). Holding fire is logged with why.
- **Better guns**: one better than her best, seen, breaks off what she's doing, but only if the route there is no longer than the fighting time it'd save against what she knows is still about (and 10 s for what's to come), at 400 units/s; further, it's an ordinary pickup in its turn and the fight comes first.
- **Rockets** aren't fired with a biter seen near her in the last 3 s (E1M9: dead at full health, a demon round a corner stepped into the blast).

HMP batch (18 maps, 3 lives): 10 → 12 exited (MAP04 now gets through). Deaths left: pistol starts against crowds (MAP08's cacodemons, MAP11), out of ammo, lost souls.

## Fighting: distance, crowds, ammo, sound, infighting (2026-11-05)

- **Distance**: each gun has a band it does best in (`Weapon.keep`: pistol 192-640, shotgun 128-448, SSG 96-320, chaingun/plasma 192-768, rockets/BFG 320-1024). Nearer than the band she backs off aslant (strafing on); further, or out of reach, she closes in; between, she strafes.
- **Surrounded** (3+ monsters within 640 spread over 160° round her): to a place she's been (96-1024 off) that's seen by the least threat, has them all on one side, is far from the nearest and near her, by a way not within 96 of any of them; shooting on the way, 4 s at most.
- **The slide**: "is that way safe" also follows where her speed carries her (8 tics pushing, then letting go: ~170 units at a run, sideways speed carrying on). E1M3: strafing by the start, she slid into the nukage and couldn't get out.
- **Out of ammo** (nothing but the fist): the fight doesn't stand; the route goes on, to the ammo she needs (MAP05, MAP11: she walked up to imps to punch them).
- **Errands mid-fight**: with only the pistol, or short of ammo, a gun or ammo she knows where it is breaks off the fight once (the explorer takes it up, urgent, if the route's worth it); on the way the fight only aims and shoots (and dodges, and backs off from biters).
- **Out of reach**: a monster in sight that no gun she has gets to from there doesn't stop the route.
- **Sound** (`tactical/sound.py`, P_NoiseAlert): a shot floods through two-sided lines with an opening, at most one sound-blocking line. A monster standing about (seen, not seen to move or turn) isn't shot if the shot would wake enough others she's seen standing about that she'd be outmatched; she sneaks past. (The flood is the game's physics, known; the monsters counted are only those seen.) The fist is silent.
- **Infighting**: a monster awake, facing another (within 30°) and not her, for half a second, moving or faced back, is fighting it: not a target, not counted as threat. A kind doesn't fight its own unless its attack is hitscan (imps' fireballs don't hurt imps).
- **Baiting**: a heavy (cyberdemon, spider, mancubus, arachnotron, revenant, baron, knight) she'd take over 8 s to kill, with monsters of another kind within 1024 of it: to a place she's been where its line of fire to her passes through one of them, holding fire; over once anything's infighting (or 4 s).
- **Arch-viles**: in sight and not killable in 2 s, into cover whatever her health (its fire does nothing once it can't see her; MAP11: three deaths at 50-70 health).

HMP, 9 maps (E1M1, E1M3, E1M9, MAP01, MAP04, MAP05, MAP08, MAP09, MAP11), 3 lives: 4 exit (E1M1, E1M3, MAP01, MAP04, the last two with no deaths); MAP05 stuck once in an explorer loop on lift 645 (exited the two runs before: to trace). Deaths left: pistol starts against cacodemons (MAP08), pain elementals, arch-viles, demons and imps close up (E1M9).

## The fight as a tree (2026-11-08)

The maintainer: a clear decision for which weapon, a clear one for how to fight with it, and a state
tree for the fight rather than one long chain of cases. Restructured with no change in play
(checked: every seed of the regression set and every arena run the same as before).

- **The tree** (`tactical/combat/tree.rs`): each tic, the first node that applies acts. Nothing
  in sight: out of a vile's sight · lost sight of it · the fight over. A target or a missile:
  *survive* (out of a vile's sight, dodging, out from under a crusher, away from a barrel) ·
  *busy* (in a jump: shooting on the way) · *fight* (backing off from what claws, to the errand,
  out of ammo: getting past, sneaking past, closing in out of reach, out of reach: going on,
  stepping for a shot, outmatched: keeping clear / getting past, keeping off, standing and
  fighting, shooting on the way). The aim and trigger are sized up first (`Combat::size_up`); a
  node only overrides them. Nodes keep the branches' own code: one may look, keep something (a
  clock, the steering's way) and pass.
- **Which weapon** (`combat/arsenal.rs`): slots as the game brings them up, ammo, armed,
  firepower, outmatched, and the gun for a monster, every rule for it in `best_gun`.
- **How to fight with it** (`combat/style.rs`): a weapon's window at a monster and its reach.
- **Retreats** (`player.rs`): an explicit state, `Retreat { kind, route }`, kinds cover / for
  ammo / back / out / bait, started by `start_retreat` (the first rule that holds, in that order)
  and ended by `retreat_over` (each kind its own).

## Up close: berserk and the chainsaw (2026-11-08)

Berserk wasn't observed: the fist was judged at 11 a punch whatever, and with nothing to shoot
she went off for ammo even berserk. The chainsaw was never used at all: Evil sends weapon numbers
straight in the tic command, so slot 1 always brought up the fist.

- **What the game does** (DSDA source): A_Punch is (P_Random%10+1)*2, ten times with pw_strength
  (20-200, 110 on average), a punch every 22 tics held; picking berserk up gives 100 health and
  the fist. A_Saw is the same 2-20 (berserk or not) every 4 tics held. Every weapon but none
  flagged WPF_SILENT alerts monsters when fired (P_FireWeapon): the fist too, so `would_wake`'s
  "the fist makes no noise" was wrong and is gone. Slot 1, as G_BuildTiccmd has it: the chainsaw
  once owned, but the fist while berserk.
- **Engine**: `evil_player.strength`; slot 1 brings up the chainsaw once she has it, unless
  berserk (as slot 3 the super shotgun).
- **Which weapon** (arsenal, bestiary): slot 1 is the berserk fist (110 a punch) while berserk,
  else the chainsaw (11 every 4 tics) once she has it, else the fist. Not the saw at what it won't
  keep flinching, nor the saw or the berserk fist at anything below 35 health, with a gun to hand
  (E3M4: at 14 health, up to a demon with the saw). Firepower counts slot 1 as the plain fist; the
  berserk fist counts against what it'd punch (`too_much`), and makes her armed against nothing
  but such (no retreat for ammo from them).
- **How to fight with it** (`Style::Melee`): the berserk fist at a monster of 400 health or less
  (what she'd take meanwhile is about punches x the chance it isn't flinching x its swipe: a
  revenant or cacodemon ~55-60, a hell knight 160, a baron 330), the chainsaw at 150 or less
  (a demon needs two unflinched saw hits in a row to bite, a pair in eleven). The tree's melee
  node: straight at it (the steering's pull to a gun's window left her circling a revenant
  100-200 off), then standing at it hitting; the weapon brought up at once, not whatever's best
  from where she stands while closing in (E3M4: rockets up at a demon 299 off, the fist at 46).
- **Arena** `arenas/berserk.txt` (the maintainer's): the fist, a berserk pack, no ammo, 1-4 revenants
  in MAP32's hall. Before: 7/8, 3/8, 1/8, 0/8 (11/32); now 8/8 (15 s a win), 3/8, 3/8, 0/8
  (14/32). Two or more still lose: the run in across the open floor into their tracers, and the
  second punching her while she punches the first (cover on the way in, one at a time: later).

## Crushers in every phase (2026-11-08)

The commonest death (9 of 32 regression runs: E3M5 five times, E2M2 three, E3M4 once), traced on
E3 seed 1, E3M5: a switch in a 64-unit alcove under a slow crusher. She went in, it started
down, the fight's "out from under a crusher" pulled her out; it judged by her middle only, so it
let go with 14 units of her still under; the route follower, at its goal (walking up to the
switch), had no crusher check at all (only the walking phase did) and walked her back in; her
momentum took her deeper as the fight turned her round, and at 55 units over the floor she was
pinned.

The mechanics: P_TryMove refuses any move whose box touches a sector with less room than she's
tall (the lowest ceiling over the box): partly under a crusher below floor + 56, she can't move
at all till it rises (a slow crusher's round trip: ~100 tics, a hit every 4).

- **The fight**: under a crusher is any of her under it (her box's corners), not her middle.
- **The follower, every phase** (`crusher_guard`: at a goal, settling, fetching; walking has
  `crusher_ahead`): partly under a ceiling coming down with less than 64 units to spare, out from
  under it first (the escape walking already had); a ceiling coming down a step from her box,
  held where she is till it goes up.

Results (HNTR, one life): E1 3.6 -> 4.1 (1/8 through, the first), E2 3.6 -> 4.2, E3 3.6 -> 5.6
(three to the E3M8 boss), Doom II 7.9 -> 8.6; arenas unchanged.

## A shot barrel isn't gone till it's blown (2026-11-08)

E1 ended at E1M3-E1M5 in 7 of 8 seeds; at E1M5 every hit that mattered was her own barrel
blast. Traced (E1 seed 0, E1M5, tic by tic): a barrel in the way, a post chosen 155 off it (out
of every blast), the barrel shot from there; then "off back the way she came", which started 27
off the barrel (where she stood when she chose the post), and the barrel, hit, was left out of
what she saw at once, so it counted as gone: she ran back past it as it went off, 15 tics after
the hit (its death frames), 66 health and 34 armor, dead.

- **Barrels till they're gone**: the observation keeps a barrel going off (its death frames,
  before and after the blast) as a barrel; it's the most dangerous one there.
- **Shot from out of every blast, she stays**: `flee_safe` says so (no run checked then, none
  needed), the post's run off is dropped, and the settle after holds where she is rather than
  backing off from the nearest barrel (towards another, across the hall).

Results (HNTR, one life, 8 seeds; Doom II 16): E1 4.1 -> 7.5 (4/8 through, from 1); E2 4.2 ->
3.8, E3 5.6 -> 5.2 (1/8 through, the first), Doom II 8.6 -> 8.2 (paths change; E2 seed 0's new
end, E2M4 stuck, is exploration: no routes, 2 barrel lines in the log); 263 levels over the set
against 249. Arenas unchanged.

## Barrels cleared on sight (2026-11-08)

The maintainer: shoot any barrel she sees, since barrels are dangerous from being blown up by enemy
fire too. Before, barrels were shot only when one blocked her way.

- **Seen**: barrels as monsters are (in view or near, nothing in the way: `Combat::barrels_seen`),
  kept while they're there.
- **Offered** (the fight's hooks, `Kind::Barrel`): each seen barrel within 1024, with nothing in
  sight (in a fight, the fight's barrel rules) and the ammo (the pistol with 10, or the shotgun
  with 12 shells: the barrel rule's guns); one objective a barrel, given up on as any other is
  (the explorer's backoff).
- **Done** (`Step::barrel`): the follower takes the barrel on as the step starts, from the
  nearest post out of every blast of its chain (the barrel-in-the-way routine: stay there if out
  of every blast, else the run off checked against the chain).
- **Pushed barrels**: a hit thrusts a barrel by its damage x 100 / its mass 100 (a pistol shot:
  6-20 units), so a barrel is the one there was if it's within 48 of where that was (it was taken
  for gone after each hit and shot at again as another, 16 times in E1M1).

Results (HNTR, one life): E1 7.5 -> 7.4 (4/8 through), E2 3.8 -> 2.8 (2/8; six die on E2M2, five
of them crushers, reached now rather than stuck before), E3 5.2 -> 5.5, Doom II 8.2 -> 8.6; 263
levels over the set either way; runs with barrel damage 5 -> 4. Arenas unchanged.

## E2M2: once-only lines that didn't take, and crushers by time (2026-11-08)

Traced first by a helper session in its own worktree (the maintainer asked for it in parallel), then
applied here.

- **A once-only line pressed while another mover has its sector** (E2 seeds 3, 4: stranded for
  the hour on a lift's pillar with 8 units over her head): the game uses up an S1 switch or W1
  line only if its action starts (p_switch.c, p_spec.c), and a floor action skips a sector that
  has a floor mover already; her knowledge took it as used because the sector moved after the
  press (the lift's doing), never offered it again. Now it took only if a target moved, since
  the press, nearer where that line puts it (`Knowledge::moved_its_way`); not known where: any
  move, as before.
- **Fast crushers** (E2 seeds 5, 6: a 64-wide passage, its fast crusher open some 16 tics a cycle,
  caught under it again and again): "rising with room" let her start from a standstill with too
  little time to cross. Now a crossing is timed (`through_in_time`): the ceiling simulated at its
  pace last seen between floor + 8 and its top, her run from her pace now along the way (portal
  to portal: E3M4's ring round a pillar, a straight line was half its length); in only if there's
  room over her from her front getting under to her back out of it, and only while it's going up
  (or still with room: in on its way down on the sums was a dash to beat it, E3M4's slow one);
  then gone through, not waited out halfway; under one coming down, on through if she'd be out
  with room all the way, else out from under as before. A crusher's pace is by the tics since
  she last looked (the fight had her in between: a slow one taken to move 23 a tic).

Results (HNTR, one life): E1 7.4 -> 7.4 (4/8), E2 2.8 -> 5.8 (2/8 -> 3/8 through; none end on
E2M2), E3 5.5 -> 4.9 (E3M4/E3M5 crushers, by the A/B the timed crossing's: next), Doom II 8.6 ->
8.4; 263 -> 279 levels over the set. Arenas unchanged.

## Crushers stopped low; out from under one between routes (2026-11-08)

- **Stopped low** (E2 seeds 3, 4, 7, E2M4: an hour waiting at the computer room's cage, a
  perpetual slow crusher she'd stopped low herself by walking its crusher-stop lines; traced by a
  helper session in its own worktree, applied here): a perpetual crusher turns at each end at once,
  so still is stopped, and it stays so till a crusher line starts it again. The planner counted on
  its rest height (any trigger that isn't permanent "comes back"), the follower waited for it to
  rise or still with room (never), and the waiting kept the stuck clocks from running out. Now the
  planner doesn't count on a crusher's rest height while it's been heard stopped and still for a
  second (`Knowledge::stopped_for`), and the follower fails the step at a crusher still and too
  low for 3 s ("stopped too low to get under") for the explorer to plan round it.
- **Between routes** (E3 seed 2, E3M5: a switch pressed in a crusher's alcove, the route over,
  stood there till it came down): the fight's tree gets her out from under a crusher at work with
  no fight on, but only with no route under way (the follower's own crusher rules have her
  otherwise: taken over mid-way, a timed crossing under E2M2's fast crusher was turned back in it,
  E2 1.2 levels).
- **Committed crossings** only where the way comes out the far side, and an escape ends one (a
  switch's alcove under a crusher, pulled out of as it came down, walked back into on the replan).

Results (HNTR, one life): E1 7.4 -> 7.4 (4/8), E2 5.8 -> 7.0 (3/8 -> 4/8 through; no E2M4
stucks), E3 4.9 -> 4.9, Doom II 8.4 -> 8.3; 279 -> 287 levels. E3's crusher deaths on routes
(E3M4's ring, E3M5's alcoves) are still to trace: with the fight's escape on mid-route too, E3 was
5.8 but E2 1.2.

## No looking round under a moving ceiling (2026-11-08)

The last crusher deaths (E3 seeds 3, 7, E3M4), traced tic by tic: a route into E3M4's ring of a
slow crusher round its pillar, in as it went up; a line walked over on the way, and the explorer
stopped her to look round at what it had done (`glance`), half a second stood still as the
ceiling came down 160 -> 132; the route's crusher rules don't run while the explorer has her
looking, and the fight leaves crushers to a route under way, so nothing got her out till 59 over
the floor, too late. Rule: no glance while any of her is under a sector she hears moving.

Results (HNTR, one life): E1 7.4 -> 7.2 (4/8 -> 5/8 through), E2 7.0 -> 7.1 (4/8 -> 7/8 through),
E3 4.9 -> 6.9 (4 to the E3M8 boss), Doom II 8.3 -> 8.2; 287 -> 301 levels; no crusher deaths
left in the set. Arenas unchanged.

## Barrels in a fight with biters (2026-11-08)

E1M8's start (traced from pistol starts, 0/8 out): the yard is a grid of some 20 barrels, the
demons among them (the maintainer: it's meant for blowing the barrels to take most of them). She walked
in to fight them and her own shots set the barrels off round her.

- **No shot through a barrel whose chain would reach her** (`barrel_in_line`): a bullet takes the
  first thing on its line; within the gun's spread at that distance (a missile: what its box
  touches), a barrel ahead of the target whose chain (each within a blast of the one before)
  comes within a blast of her holds her fire.
- **Clear of barrels whenever she's fighting**, not only with something shooting in sight: the
  "away from a barrel" move and the steering's barrel penalty; her shots set them off too.
- Tried and dropped: barrels by monsters shot only with their whole chain clear of her (stricter
  than "that barrel well clear"): fewer barrel kills, Doom II 8.0 -> 7.5 in the A/B, and E1M8
  didn't need it.

Results (HNTR, one life): E1M8 from a pistol start 0/8 -> 8/8 out. Sets: E1 7.2 -> 7.5 (5/8 ->
7/8 through), E2 7.1 -> 6.6 (E2M7: her own rockets, launched 8 tics after the trigger into a
wall she'd turned to: next), E3 6.9, Doom II 8.2 -> 8.0; 301 -> 296 levels. Arenas unchanged.

## Rockets: the lane at its own height, the way she faces, and turned off at the last (2026-11-08)

The crate arena (arenas/crates.txt) made her own rockets the loss: 5 of 6 lost fights, one rocket
at full health. Logged per rocket (where it came out, where it went off): it went off at an
obstacle's edge, never on it. Two causes: the lane was judged toward the target, but a rocket
goes where she faces, and she fires within a couple of degrees (a crate's corner clipped 3 off
the target's way); and the lane was judged as seeing, from her eyes, so a step as high as a
rocket's bottom (her feet + 32: the 32-high steps) was "clear" and flown into.

- **The lane at the rocket's height** (`rocket_flies`): rays at its bottom and top (her feet + 32
  and + 40, climbing or dipping to the target's middle), its box's reach each side (radius, 1.4x
  on the diagonal) and 8 more (a miss is better than a near thing, the maintainer's); toward the target
  and the way she faces; only as far as a blast could come back to her (160 + 8): beyond, a miss
  (checked all the way she held fire at a cyberdemon till it had her: Doom II 7.4).
- **The last resort** (the maintainer's: the trigger can't be called back): each tic till the rocket's
  out (8 after the trigger), if the way she faces would put it off within 160, turned to the nearest
  way that won't.

Results (HNTR, one life): crate arena 74/80 -> 75/80 (lost 6 -> 0 or 1, health left 77 -> 85);
deaths her own rockets led 5 -> 1, their damage 897 -> 589 over the set; E1 7.5 (7/8), E2 6.6 ->
6.9 (6/8), E3 6.9 -> 6.6, Doom II 8.0 -> 7.7, Doom II arena 227 -> 228; 296 -> 291 levels.

## Rockets where autoaim sends them (2026-11-08)

The maintainer: autoaim catches human players, a rocket tilted down at what's below into a ledge's lip
(firing down into a pit) or into a railing it would fly over level. The lane was judged toward the
target and the way she faces; the game sends it to the first monster on her way within 1024, or
5.6 degrees to one side then the other (P_SpawnPlayerMissile, P_AimLineAttack), swinging and
tilting it to that one. `autoaim` predicts it (to the monster's middle: the game aims at what's seen
of it, above a railing higher, so the middle is the nearer of the two to what's in between); the
lane and the last resort judge that way too.

Arena: arenas/autoaim.txt (tools/arenas/autoaim_arena.py): ledges over pits found in Doom/Doom II
(E1M6, MAP03, MAP08, MAP09, MAP20) and MAP14's railings, the imps where a shot down reaches them
(autoaim's 100/160 slope); a spawn that doesn't fit is put at the nearest spot round it.

Results (HNTR, one life): autoaim arena own-rocket hits 31 -> 21; crate arena 75 -> 76/80; over
the set runs with own-rocket damage 13 -> 7, damage 589 -> 405; E1 7.5, E2 6.9, E3 6.6 -> 7.0,
Doom II 7.7 -> 7.6, Doom II arena 228 -> 229; 291 -> 293 levels. MAP09's pit: she holds fire at the
lip and never steps up to clear it (next: stepping for a rocket shot).

## Plasma where autoaim sends it, a safer gun for a blocked rocket, ducking hitscan heavies (2026-11-08)

- **Plasma** (the maintainer's: not wasted when autoaim would put it in the lip below, or the floor in
  front of something up on a ledge): held when its middle wouldn't get to the target the way
  autoaim sends it (aimed at the visible part, as P_AimLineAttack does), and a step on toward it
  (never off the ledge) to where it would. Its middle only: with its box's sides, cover edges 14 off
  the line 700 out held her fire at a Spider Mastermind (MAP16's cover, 0/8 -> 6/8 lost).
  Not for rockets: stepping for a rocket shot put her by the crates' columns (own-rocket hits 26 -> 44).
- **A blocked rocket** (the maintainer's: a safer weapon rather than one held): the launcher ruled out for
  2 s after its way at the target was blocked close by; kept (waited with) only when the next gun
  would take 3x as long (the pistol at a Cyberdemon, cells gone: 3/8 drawn). Rocket safety needs
  both slopes autoaim may give (the visible part's, the middle's) clear.
- **Running after her own rocket**: the lane is checked further the faster she's following it
  (20 a tic to her 16: a wall 600 out is a blast 120 from her). MAP06, shooting on the way, the
  route running her after it, dead twice.
- **Crushing floors**: a floor seen coming up lately, with under 72 left beneath the ceiling, is a
  crusher too (E2M4's secret: over a W1 "raise floor, crush" line, stood on it with nothing left to
  do). Lifts end level with a floor beside them, room enough over it.
- **Ducking** a hitscan shooter in a long fight (not over in 4 s with the gun up): it shows its
  wind-up (the spider mastermind turns to her 20 tics before its first burst, info.c; the zombies
  10), and fires on only while it sees her (A_SpidRefire, A_CPosRefire). She shoots through the
  wind-up and leaves for the nearest place out of its sight with just the time to get there, holds
  till its fire would have broken off, and "keeping at it" brings her back out. Leaving at once,
  she got next to no shooting done (spider-cover 1/8). Once it's firing: anywhere within 24 tics.
- **Posts**: in such a fight with nowhere near to duck (E3M8's open star), to the nearest place she
  knows of (seen or been, on a 32 grid: the nav's polys are too coarse in a hall) that none of them
  sees, with somewhere they would within 128 to step out to, across the open if need be (the
  maintainer's: the building in the middle).
- **Boss ammo**: with a boss seen and about, rockets, cells and the BFG's cost 10x against anything
  else (E3M8: 138 cells on two cacodemons before the spider).
- Tried and dropped: steering toward places near a shadow (kept her behind the cover, unable to
  shoot: spider-cover 8/8 -> 2/8); fighting the spider from 768-1280 rather than 640-896 (the maintainer's
  idea, its spread; spider-cover 6/8 against 8/8, E3M8 no better).

Results (HNTR, one life): E1 7.5 (7/8), E2 6.9 -> 7.0 (6/8), E3 7.0 -> 6.9 (0/8 -> 1/8 through: E3M8
won), Doom II 7.6 -> 7.8; arenas combat 120/128, Doom II 229 -> 230; autoaim arena own-rocket hits
21 -> 19, crates 76 -> 77/80 and 26 -> 21; plasma autoaim arena (arenas/autoaim-plasma.txt) 81 ->
83/112. E3M8 with an E3M7 loadout: 0/4 -> 1/6, the spider left at 370-1720 rather than 1300-2250.

## E3M8 on HMP, boss fights, ledges (2026-11-08)

Traced from E3M8 (the Spider Mastermind) from a pistol start on HMP and with the loadout she
brings from E3M7; each rule general, the map only its test.

- **Outmatched** counts a hitscan shooter however far (its bullets get there), but only a
  "sniper": one she couldn't see off in 4 s with what she has (`sniper`), not a zombieman across
  the map. It weighs her too (the maintainer's): longer to kill than she'd last against it, her health
  with her armor's share of each hit (a third green, half blue: `lasts`), is outmatched.
- **No chasing** ("keeping at it") a monster she can't take on.
- **Taking a sniper on** (`takes_on`): once she'd see it off before it saw her off (gun, health,
  armor; not ammo, picked up as she goes), the outmatched moves don't apply to it and she stands
  and fights, whatever it's after; otherwise she keeps clear.
- **Leading walkers**: a monster walks by steps, its momentum nothing; tracked over 12 tics, the
  rocket or plasma ball led by that (90 rockets at the spider, aimed where it was, 200 damage).
- **Weapon churn**: the gun in hand counts as 15% quicker (another must be clearly better: the
  chaingun and plasma rifle swapped 17 times at 700-880); nothing readied for the next fight
  while an awake monster she knows of is about (the shotgun up each time the spider went out of
  sight, 14 times). E3M8 with the E3M7 loadout: HNTR 9 -> 13/16, HMP 2 -> 7/16.
- **A boss in sight**: others further than 384 not shot (left to it: infighting, the maintainer's).
- **Danger**: an objective a sniper (from where she last saw it) could see counts as near it.
  **Ammo** counts as twice as far with something she can't take on about (the building's plasma
  rifle and blue armor first: 8/16 got them, 0/16 without; half as far, 0/16).
- **Posts** look out to 1536, the far side of what's between her and them preferred (the maintainer's:
  the other side of the building). **Exposure**: ways across ground a sniper after her would see
  her on count 3x (she routes round; replanned once when it first appears); one she can see busy
  with another monster doesn't count. **Hold at the edge** (the maintainer's: a player hears when its
  attack's over): hidden, about to step into the sight of a sniper that's firing, she waits for
  it to stop (its wind-up then gives time to cross).
- **Ledges**: the fight's `safe` judges a drop from where she stands, not the floor under her
  middle (on a lip, her middle over the drop, the game keeps her up: MAP06, walked off a crate top
  552 down, then along a nukage channel's lip and in); a route ending within 48 of a drop is
  arrived at slowly (E1M3, run at a pit's lip off a step down, airborne, over); no barrel firing
  spot on a floor that hurts.
- The observation carries her armor type; the death line what's in sight's health.

Tried and dropped: counting known ammo on the map toward outmatched (no effect); fighting a
sniper from cover even outmatched (every pistol start dead at 25-36 s); a landing margin for drops
(the MAP06 falls weren't the landing).

Results (HNTR, one life, 8 seeds, last set): E1 7.5 -> 8.0 (8/8 through), E2 7.0 -> 6.1 (2/8),
E3 6.9 -> 5.0, Doom II 7.8 -> 7.1 (16 seeds; MAP06 10/16 failed); arenas combat 120 -> 119/128,
doom2 230 -> 228/240. The sets swung a lot between builds (E1 6.1 / 8.0, E2 8.0 / 6.1 on small
changes): routes shift and meet other old weaknesses. E3M8: from E3M7's loadout HNTR 13/16, HMP
7/16; from a pistol start on HMP 1/16. Committed with these numbers (the maintainer's call: the rules
are sound, tune later). The regression set changes to Doom E1-E3 episodes plus Doom II single
levels from a pistol start (the maintainer's: MAP06 is a gimmick map, no test).

## Invulnerability and the BFG (2026-11-08)

The maintainer's: better planning for both (seen in an E3 run). From the game's own:
- **Invulnerability** is known from its pickup message ("Invulnerability!": 30 s, the last 3 not
  counted on), as a player knows it. While it lasts nothing outmatches her; no retreats, ducking,
  dodging, holding at edges or keeping out of sight; her own rockets safe; hurting floors fine (drops
  still not); nothing too dangerous to go for.
- **Kept for a high-risk fight** (the maintainer's: super high priority then): a sphere isn't gone for
  unless something she knows of, awake, outmatches her (`needs_invulnerability`); then it's urgent
  (before keys) and an errand mid-fight, so long as the way to it doesn't pass within 320 (and
  its radius) of what she can't take on: kept clear of till it moves off the line. E3M9 (E3
  loadout, 8 seeds): its sphere behind the cyberdemon; spotted and left: killed by it 1/8; run at
  straight past it: 5/8; with a clear way: 0/8, the cyberdemon killed 8/8.
- **BFG**: the launch (30 tics; the rocket's 8) counted in the time to kill; while her ball's in the
  air no ducking (A_BFGSpray: forty lines from her, ±45° along its flight, 1024 out, when it
  lands); its crowd counted in that cone, not all in sight. Tried and dropped: no BFG unless a
  boss or 3 in the spray (E3M4: plasma at close cacodemons spent 186 cells and died; the BFG, 120,
  through).

Results (new set: Doom episodes, Doom II single levels from a pistol start; HNTR, one life):
E1 8.0 (8/8), E2 6.1 -> 5.9 (2 -> 3/8 through), E3 5.0 -> 6.5 (0 -> 1/8), Doom II 23 -> 24/32;
arenas combat 119 -> 118/128, doom2 228.

## The BFG's aim and hold; melee chases given up; dodging across, not along (2026-11-08)

- **BFG aim**: with it up, the target is the monster (one she has a shot at, within 1024) whose line
  puts the most threat in the spray, ±45° of the ball's way (A_BFGSpray: the ball goes to the
  first on her line, the spray along the way it went, from her).
- **BFG hold**: from the trigger (the ball leaves 30 tics on) till it lands, she stands still (a
  survival node after dodging): no ducking, retreats or posts, the route not walked on; the spray
  comes from where she is.
- **Melee chases** (the maintainer's): closing in to get at something in melee has its own clock, as the
  out-of-reach chase has (20 s, then left 30 s); that clock never ran for melee, a melee target
  always counting as reachable. E3M4: a demon she couldn't get at, closed in on and lost sight of
  for an hour (1905/2462 s -> 750/806 s).
- **Dodging** (the maintainer's): among ways that clear, across a missile's way rather than along it
  (24 x |cos| off). Backing straight off outran an imp's ball (16 to its 10) and scored as clear,
  then open room ahead decided: off down the room, the ball after her.

Results (HNTR, one life): E1 8.0 (8/8), E2 5.9 -> 7.1 (3 -> 7/8 through), E3 6.5 (1 -> 2/8),
Doom II pistol-start levels 24/32 (MAP10 0 -> 3/4); arenas combat 118 -> 119/128 (health left
83.7 -> 85.0), doom2 228/240 (85.8 -> 86.5).

## Crushers by kind; spent teleporters; out from under all of her; no stepping for a shot in nukage (2026-11-09)

- **Out from under all of her** (E2M2's crusher corridor): the escape from a ceiling coming down
  aims for a poly whose middle has her whole box clear of moving ceilings, not the first poly not
  moving (her middle out, an edge still under, pinned against the wall beside). 32/32 E2M2 seeds
  exit (30/32 before).
- **Spent teleporters** (E3M9's stuck, 3 of 8 E3 seeds): a W1 teleporter she's gone through this
  life is a line like any other (vanilla clears its special; she's only taken to have gone through
  one when she was teleported, so in every complevel). The navigator never walked a teleport line
  from its front; E3M9's red key lies east past an 8-wide strip with one in it, so after the
  ambush there, out to heal, the way back was a wall. Now her view (`Restriction::spent`) opens
  those portals; the omniscient planner is unchanged.
- **Crushers by kind** (DSDA's p_ceilng.c, p_map.c; `crush.rs`): 10 every 4th tic while the ceiling
  is lower than she's tall; she can't move then. Slow ones (25, 49, 73, 141, Boom's under 3x)
  slow to an eighth on her: held, dead. Fast ones (6, 77) keep on: one stroke, 120 raw at the
  worst, armor taking its share.
  - **The fight** (E2M4: a slow crusher stopped 9 over her head, chased into after an imp across
    its own start line, set off, pinned in 9 tics): a crusher she knows is no-go, stopped or not,
    when its room over her head in tics is less than getting out of it takes at the worst (from
    its farthest point, starting at full speed the wrong way) and its stroke costs more than she'd
    take on (slow: always; fast: more than a tenth of her health, the maintainer's).
  - **Routes** (the maintainer's (c)): crossing a crusher's start line into it while it's stopped like
    that costs a stroke of it in distance (a way round shorter than that is better), and the
    follower, at such an entry, steps 2 over the line at a crawl, out as it starts, waits at the
    edge and crosses on its way up (the timing it had). If backing out wouldn't beat the ceiling,
    the step fails. A crusher stands still a tic as it turns: still longer than 2 tics is stopped.
- **No stepping for a shot on a floor that hurts** (E3M5's nukage pit, 4 deaths in 4 sets): an imp
  on the ledge above 500 off, no shot from the pit; stepping for one goes away from a target
  above (the angle), out past the 512 the node applies within, the route's way off brought her
  back in: 570 tics of it, dead, at 5.5 a second (the fight's back-out is from 6). The step only
  kept her on the floor; she shoots on the way off. E3M5 8/8 seeds (6/8).
  Not done: the same flip at 512 off a hurting floor (a stall, not seen yet).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.4 (7/8 through), E3 6.5 -> 6.8 (1 -> 0/8 through;
no E3M9 stucks, no E3M5 deaths), Doom II 27/32; arenas combat 119/128, doom2 226/240. E2M4 seed 5
(stuck: dropped into a pit for a blue armor, its only way out a teleporter behind a door she
hadn't seen) and E3M9's ambush on arrival are older problems met in a different state.

## Level campaign, round 1: barrels going off, waits that aren't coming, a hurting floor's own cost (2026-11-09)

Pistol-start single levels on all four skills, four helper sessions a round, each with one level
(the maintainer's). Round 1: E1M5, E1M6, E1M9, E2M2.

- **A barrel going off** (E1M6 UV: one set off by a fireball, its blast set off a second and
  pushed it her way, met while dodging the imp's last fireball): the observation has the barrels
  at 0 health still there (hit: 15 tics from blowing); a survive node, before ducking and dodging,
  takes her away from the nearest barrel when one going off, or its chain, would reach her (walls
  not counted). E1M6 UV seeds 0-2 exit (died).
- **A wait for a sector that isn't coming** (E1M5 UV: a platform still rising from the move before
  ignored the lowering line she crossed, as vanilla's EV_DoFloor skips a sector with a mover; she
  waited in the nukage, counted "coming" for 1.5x its travel + 10 s, dead): it's coming only while
  it's getting nearer the heights wanted (within a second). E1M5 UV: died -> stuck (on the
  lowered platform, every way "no route": to trace).
- **A hurting floor at the end of a route costs what the map says** (E1M9: 5 a hit counted as 20,
  every way on from a lip over the nukage refused at 68 health): the worst (20) only for a floor
  the map gives none for, as on the way. E1M9 still stuck (the way off the floor cuts into a wade
  to a timed door).
- E2M2 UV traced, not fixed: out of ammo early, the ammo errand into awake monsters, the fist
  chosen over the pistol against melee-only monsters.

Results (HNTR, one life): E1 8.0 (8/8), E2 7.4 -> 7.8 (7 -> 6/8 through: E2M8's cyberdemon twice),
E3 6.8 -> 7.5 (0 -> 1/8), Doom II 27/32; arenas unchanged.

## Level campaign, round 2: teleporters from the front, ammo errands through monsters, one-hit claws (2026-11-09)

Round 2: E2M3, E2M5, E2M7, E2M8 (pistol start, all four skills).

- **A teleporter goes off only crossed from its front** (E2M7 HNTR: a WR teleporter reached from
  behind on a pad by nukage, the Cross step counted done either way, "didn't take me anywhere",
  the only way out dropped, died in the nukage): EV_Teleport does nothing from side 1; the step is
  crossed only from the front, so from behind she goes back over it from the front. HNTR seeds
  0-2 exit (died).
- **A walk for ammo ends when one of them steps onto its way** (E2M5 UV: out of ammo, a demon
  moved between her and the clip, pushed against, bitten from 96 to 29): the rest of the planned
  path passing a monster after her nearer than their bodies touching ends the retreat. UV still
  dies (out of ammo by 218 s).
- **One hit that could kill her** (E2M3 UV: out of ammo at 35, a shell under a baron, its claw
  10-80): she backs off from a clawer whose biggest hit (p_enemy.c: imp, lost soul 24; demon 40;
  cacodemon, revenant 60; baron, knight 80) is her health or more, armed or not, and the pull
  towards an errand stops when it's within that one's reach (80). UV lives longer on 3 of 4 seeds;
  still dies.
- E2M8 traced, not fixed: an objective chosen before the cyberdemon woke is never vetted again,
  and losing sight of it a moment ends the fight and hands her back to a way into its view.

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.4 (6 -> 7/8 through), E3 7.5 -> 7.1 (1/8),
Doom II 27 -> 28/32; arenas unchanged. The new E3 failures (her own rocket at a lost soul 150
off, E3M7; E3M9's arrival ambush holding the fist) are outside these changes: to triage.

## Level campaign, round 3: rockets held only for what can get in their way, a switch first for a gun out of reach (2026-11-09)

Round 3: E2M9, E3M2, E3M3, E3M4.

- **Rockets held only for a claw that could come into their way** (E3M4 UV: four rockets held for
  every demon 237-344 off, "rocket not safe", the pistol spent on them, punched to death by five):
  a melee monster seen in the last 3 s had its 128 of leeway on the ring round her too, so any
  within 334, behind her included, held the rocket. A rocket bursts only on what's in its way: the
  leeway now widens only the way (beside the line to the target, or its reach beyond); the ring is
  each one's radius plus the blast. E3M4 HMP seed 0 exits (died 194 s); UV lives 53 -> 2939 s.
- **A switch first, with a better gun in sight she can't get to** (E3M3 HMP/UV: the shotgun up on a
  floor a switch lowers, seen at the start, the switch ranked with the walls and left all level,
  out of ammo with the pistol, dead): switches count as a quarter as far while a better gun lies on
  ground she's seen but can't reach. E3M3 HMP seeds 0, 1 exit (died).
- Traced, not fixed: E2M9 (out of ammo among barons, the only ammo behind the one door she never
  opens: no known ammo means visited places, never the unexplored); E3M2 UV (a cover retreat's
  route ran her into shotgun guys just seen, the fight's way the other way overridden).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.4 -> 7.1 (7 -> 6/8 through), E3 7.1 -> 7.2 (1 -> 3/8),
Doom II 28 -> 27/32; arenas unchanged. E3M6's stucks (3) are as before on the level alone.

## Level campaign, round 4: a crusher crossing ends, doors aren't crushers (2026-11-09)

Round 4: E3M5, E3M6, E3M7, E3M8.

- **A crusher crossing ends once she's out from under it with none ahead** (E3M8 HNTR: a door just
  opened taken for a crusher, a crossing timed, never cleared: `crossing_crusher()` kept the fight
  "busy" for the rest of the route while a cacodemon came up and bit her for 60). Cleared out
  from under with none ahead, and while waiting at an edge. HNTR seed 0 46 -> 73 s.
- **A sector only doors move is no crusher to the fight** (E3M5 HMP: a DR door closed onto her
  box's edge, taken for a crusher at work, "out from under a crusher" handed her to the route,
  parked at the door under fireballs): vanilla's T_VerticalDoor never hurts (a closing door goes
  back up, a close-only one rests on her). HMP seed 1 exits, seed 0 124 -> 160 s; UV 588 -> 786 s.
- Traced, not fixed: E3M6 UV (out of ammo, walked past a cacodemon whose bite isn't a one-hit
  kill); E3M7 UV (out of ammo, fleeing across blood at 5.5 a tic: `Motion::go` slows her moving
  off her facing, the route's assumption, while the fight holds her facing; Doom runs her
  backwards at full speed; both versions of a fix moved HMP seed 0 the wrong way).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.1 -> 7.8 (6/8 through; only E2M8 stops it), E3 7.2 ->
6.8 (3 -> 0/8; E3M8 six times, E3M4 twice), Doom II 27 -> 25/32 (MAP12 seeds 0 and 3, MAP05 seed 3:
the runs part by two tics at a door no longer taken for a crusher; latent, for the Doom II pass);
arenas unchanged.

## Level campaign, round 5: a blocked sprint, a melee she can't land, a monster in an errand's way (2026-11-09)

Round 5: E3M9, Doom II MAP03, MAP04, MAP05.

- **A run-jump's sprint stopped short of the takeoff is blocked** (MAP05 HMP: imps at the takeoff,
  the sprint pushed into them for its 4 s with the route keeping the legs, 173 health to dead):
  still short and under 2 a tic a quarter second in, the sprint's off and the route fails; the
  fight has the legs again. Not counted a missed jump.
- **A melee that doesn't hurt it is given up** (MAP05 ITYTD stuck: a demon a floor below, in the
  fist's reach across but too steep to hit, "at it" resetting the clock for 3200 s): the give-up
  clock goes back only when the target's health does.
- **A monster in an errand's way is shot first** (MAP04 HMP: a zombieman in the doorway on the way
  to the shotgun, pushed against for 80 tics while she aimed at the shotgun guys behind, shot down):
  on an errand, one that attacks, touching-near and within 60 degrees of the way, is turned to and
  shot (the route keeps the legs). MAP04 HMP seed 0 exits (died at 12 s).
- Dropped after the regression set: a retreat taken over only by escaping moves, the errand held
  while retreating (MAP03 UV seed 0 exited with it): arenas 119 -> 115/128 and 226 -> 222/240.
- Traced, not fixed: E3M9 UV (out of ammo among imps from a door, shots spread over the crowd);
  MAP03 HMP (backed out of an ambush, the route straight back up the lift into it).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.9 (7/8 through), E3 6.8 -> 6.2 (0 -> 1/8; E3M6's
teleporter stuck, E3M9's arrival), Doom II 25 -> 26/32; arenas unchanged.

## Level campaign, round 6: a jump given up only for a body in the way; switches from a hurting floor at a dead end (2026-11-09)

Round 6: Doom II MAP06-MAP09.

- **A run-jump is given up only for a monster's body in the way** (round 5's rule, "stopped short
  of the takeoff", broke MAP06 ITYTD/HNTR: knocked aside mid-sprint, stood a moment, the plasma
  rifle's jump given up, stuck; and missed MAP05 HMP's imps, against her just past the takeoff):
  stopped a quarter second into the sprint with a living monster's box touching hers ahead (towards
  the takeoff, or past it the landing). MAP05 ITYTD/HMP and MAP06 ITYTD/HNTR seed 0 exit.
- **At a dead end, switches pressed from a floor that hurts** (MAP09: the start an island in a
  nukage pit, the lifts' switches pressed from the nukage, all left out by "no switch from a
  hurting floor with floor that doesn't in reach", the island counting): once nothing at all is
  left (walls, the panic round, the second goes), they're offered. MAP09 with no monsters exits.
- Dropped after the regression set: MAP08's rocket judged at its window's distance (MAP08 seed 1
  lost, none gained) and MAP07's re-plan when a seen gun comes in reach (MAP07 HMP seed 2 and MAP10
  seed 0 lost, MAP07 UV seed 0 gained). With both, E3 was 8.1 (4/8 through) but E2 6.8, Doom II 24.
- Traced, not fixed: MAP06 UV (a better gun urgent skips the danger check: the spider by the
  plasma rifle); MAP10 HNTR (with MAP07's change) killed by her own rocket at full health beside a
  door, the fight 8 tics over: to trace.

Results (HNTR, one life): unchanged: E1 8.0 (8/8), E2 7.9 (7/8), E3 6.2 (1/8), Doom II 26/32;
arenas 119/128, 226/240.

## Level campaign, round 7: health an errand only near, a cheap way not refused for health (2026-11-09)

Round 7: Doom II MAP10-MAP13.

- **Mid-fight, health is an errand only within 512** (MAP10 UV: at 38 health a stimpack 1097 off
  was urgent, and an urgent item was the fight's errand at any distance: every decision "towards
  the Stimpack" across a yard of imps, zombiemen and shotgun guys with the pistol, dead): urgent or
  not, as other health was; urgent things that aren't health unchanged. UV 70 -> 82 s, HMP 350 ->
  449 s (still dying).
- **A way costing a tenth of her health or less isn't refused for being short of health** (MAP12
  HNTR stuck: at 13 health every way over the nukage, 0.5 a way under a suit, refused while health
  less cost was under 25, waiting for healing or a suit she couldn't reach without them; stood on a
  pillar for the hour): the mortal margin applies only to a way costing more than RISK_SHARE of her
  health. MAP12 HNTR seed 0 exits.
- Rejected: MAP13's "no item under a running crusher in a fight" (lost HNTR seed 0: a different
  run, dead to her own rocket).
- Traced, not fixed: MAP11 HMP/UV (the target a pain elemental or cacodemon while hitscanners do
  the damage; keeping clear only of the target); MAP13 (a plasma rifle in a room of two crushers,
  the urgent objective never interrupted, pinned straddling both).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.9 (7/8), E3 6.2 (1/8) unchanged; Doom II 26 -> 27/32;
arenas unchanged.

## Level campaign, round 8: closing in round a corner, every frame of a liquid's animation (2026-11-09)

Round 8: Doom II MAP14-MAP17.

- **Closing in, round a corner** (MAP14 HMP: a door jamb in line with her box's edge, `clear_run`
  0 straight on every tic, `closing_in` gave up and she stood in a chaingunner's fire from 88 to
  dead): straight on not clear, she veers 20 then 40 degrees either side (still closing) before
  giving up; Doom slides her past the corner. MAP14 seeds 0-3, all four skills: no exit lost.
- **A liquid's floor looks like one in every frame of its animation** (MAP15 HNTR: a pit floored
  SLIME12 taken for dry ground, "SLIME0" matching SLIME01-09 only; dropped into it, the only way
  out a teleporter, dead): SLIME10-12 too (SLIME09-12 one animation). HNTR seeds 0-3 live longer
  (none exits yet); ITYTD seed 3 now runs a different way and dies out of ammo to a baron.
- Traced, not fixed: MAP16 ITYTD (on a pillar under imp fire, a dodge with no safe way still takes
  over and drops the retreat's legs; with a way, it flips each tic); MAP17 HNTR (an arch-vile's
  wind-ups with nowhere to hide: fought on from full, cover that doesn't break its sight).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.9 -> 7.8 (6/8), E3 6.2 -> 6.6 (1/8; E3M4 deaths
gone), Doom II 27/32; arenas unchanged.

## Level campaign, round 9: no coasting off a ledge in a fight, a gun urgent only on a way she can take (2026-11-09)

Round 9: Doom II MAP18-MAP21.

- **The fight brakes rather than coast off a ledge** (MAP19 HMP: a run-jump onto a 16-wide strip
  over a demon pit; the fight took the legs with no safe move and gave no thrust, the jump's speed
  slid her 33 units off the lip into the pit, stuck there for the hour): with the legs and no move
  of its own, the fight runs its slide check with no push; if her speed would take her over a drop
  she can't climb back or onto a floor that hurts, she brakes (`coasting_off`).
- **A gun or ammo is urgent only on a way past nothing she couldn't take on with it** (MAP20: a
  rocket launcher by the cyberdemon, urgent, so every danger check skipped, gone for again and again
  under its rockets; a launcher on the floor is 2 rockets): urgent only when the straight way to it
  passes nothing she couldn't take on armed with it (`clear_way_to` on what she'd have); otherwise
  an ordinary objective, danger checks and all. MAP20 ITYTD 62 -> 168 s, HNTR 23 -> 45 s.
- Left out after the regression set: MAP21's "a press carried out of the use ray's reach fails
  the step" (a dodge over a teleporter left her pressing a door 1500 off, the door written off).
  Failing cost the Doom II arena 4 fights (doors in boss arenas' cover dropped mid-fight); walking
  back to the press spot instead cost Doom II three levels (back into the fire). To revisit.
- Traced, not fixed: MAP18 (out of ammo by 116 s on HNTR; an untried teleporter into cacodemons).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.0 (6/8), E3 6.6 -> 7.9 (1 -> 4/8 through), Doom
II 27 -> 28/32 (only MAP08 fails); arenas combat 119 -> 118/128, doom2 226/240.

## Level campaign, round 10: a barrel chain going off, kept off out of a fight too (2026-11-09)

Round 10: Doom II MAP22-MAP25.

- **A barrel chain going off, out of a fight** (MAP23: a mancubus's fireballs set the start room's
  barrels going; the fight over, the route walked her into the chain burning through the field,
  dead): the follower handled only barrels she shot herself. Now, with any barrel going off, she
  works out its chain (each within a blast of one going off, in turn): a blast reaching her, away
  from its nearest barrel; one reaching where she's moving within 15 tics (a barrel's death
  frames), she holds; on once neither. MAP23 ITYTD seed 0 7 -> 19 s, seed 2 5 -> 20 s.
- Traced, not fixed: MAP22 HMP (health spent on repeated risky trips over nukage, then the lift's
  switch, pressed only from the nukage, refused at 16 health); MAP24 UV (sleeping imps make every
  near objective dangerous; the super shotgun on a ledge spotted, never an objective); MAP25 (a
  stepping stone with five barrels: the next run-up past the middle one, which can't be shot from
  out of the blasts; the planner takes barrels inside a poly as walkable).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.0 -> 7.4 (6 -> 7/8), E3 7.9 -> 8.0 (4/8), Doom II
28/32; arenas 118/128, 226/240.

## Level campaign, round 11: lining up a jump isn't a jump (2026-11-09)

Round 11: Doom II MAP26-MAP29.

- **Only a jump under way keeps the fight off the legs** (MAP27 UV: out of ammo at 43 health, routed
  over a run-jump; at the run-up, the whole jump counted "busy" so the fight couldn't move her, yet
  it turned her to aim at a spectre, so the line-up never squared her to the takeoff: she stood
  there, two spectres biting, dead): the sprint and a run over sinking floors keep the legs; lining
  up is standing with no jump begun, so the fight has them; a line-up it broke off starts its clock
  again. UV 107 -> 154 s; MAP27 ITYTD/HNTR seeds 0-3 unchanged.
- Traced, not fixed: MAP26 HMP/UV (out of ammo with dropped chainguns 110-530 off, all rejected as
  past monsters, "none known" in the log; keeping clear of chaingunners by distance, in their fire);
  MAP28 (UV: three pain elementals; HMP: the target a cacodemon while a shotgunner 83 off shoots);
  MAP29 (a lava alcove left only by a plain-textured lift switch; "try the wall" always "no route",
  even at 78 health: to trace).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.4 (7/8), E3 8.0 (4/8), Doom II 28/32; arenas
118/128, 226/240.

## Level campaign, round 12: unarmed, the route keeps the turn; Commander Keens fought (2026-11-09)

Round 12: Doom II MAP30-MAP32, Doom E4M1.

- **Unarmed, the route keeps the turn** (MAP31 HMP: out of ammo, a retreat for ammo through a door
  that had shut again; the fight's decision merged with the route turned her to aim at an SS, the
  route presses a door only facing it: 450 tics in front of it, shot down): with nothing to shoot
  with (`armed()` false), the route's turn stands in a merged decision. MAP31 HMP seeds 0, 1 live
  longer; no exit lost over seeds 0-3 on all skills.
- **Commander Keens are monsters to the fight** (MAP32: the exit behind tag 666, opened by the last
  Keen's death; the fight left Keens out, so never shot, no exit seen); and **no fist at something
  harmless she's given up getting at** (Keens over a ledge, punched at for a minute with bullets in
  the pistol). For anything that fights back, wider, it cost E3 0.8 levels.
- Traced, not fixed: MAP30 (the brain isn't a target or an exit trigger to her, and is killed only by
  rocket splash fired level through a slot from the lift, unseen: a new behaviour); E4M1 UV (a
  baron through a narrow opening, the rocket's lane blocked, 12 rockets unused: nothing moves her to
  a clear lane).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.4 (7/8), E3 8.0 (4/8), Doom II 28/32; arenas
118/128, 226/240.

## Level campaign, round 13: out of ammo, to the ammo itself; a floor learned harmless only unprotected (2026-11-09)

Round 13: Doom E4M2-E4M5.

- **Out of ammo, to the ammo itself** (E4M5 UV: "away for the Shell" went to the middle of the poly
  it lay in, 78 off it in a big room, "got there" without touching it, ten times; died out of
  ammo): to the item's own spot. E4M5 UV seed 0 exits (died at 117 s); no exit lost seeds 0-3.
- **A floor is learned harmless only from time on it unprotected** (E4M2: lava walked under a suit
  learned "doesn't hurt after all" the moment the suit ran out): the clock from the latest of
  stepping on, the last hurt there, and the suit's end.
- Traced, not fixed: E4M2 (in a one-way lava pit at 32 health, the way out refused by the tenth-of-
  health cap; lifting the cap on hurting floors made HNTR worse); E4M3 UV (a one-way drop into a
  crowd at low health for a medikit: routes' risk counts floors, not monsters); E4M4 HMP/UV ("keeping
  clear" of cacodemons it can't kill while firing at them: every gun emptied).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.4 (7/8), E3 8.0 (4/8), Doom II 28/32; arenas
118/128, 226/240.

## Level campaign, round 14: unseen walls a way out of a pit, a switch before walls in one, no fist at a killer (2026-11-09)

Round 14: Doom E4M6-E4M9. The end of the 10-minute pass.

- **A drop isn't a dead end while there are walls down there she hasn't seen** (E4M6 ITYTD stuck:
  the only way on a drop into a lava pit whose way out is lifts on walls unseen from above; the
  dead-end check counted unknown map, known switches and teleporters, not unseen walls). A pit seen
  all round is still refused. ITYTD stuck -> dies later (sniped on a ledge).
- **On a hurting floor with no floor that doesn't in reach, switches and doors before walls** (E4M8:
  a missed jump into a lava pit, the way out a lift switch 404 off; the risky pass ranked plain walls
  75 off first, three tried, 170 tics of lava, dead): walls are dropped while a switch or door is on
  offer; a seen switch was built as a way out, a wall's a guess. E4M8 seed 0 ITYTD, HNTR, HMP exit
  (all four died).
- **No fist at what one hit of could kill her, with a gun to hand** (E4M7 HMP: berserk at 41 health,
  the fist free and so chosen at a baron, the tree backing off: 600 tics without a shot fired):
  `claw_max` >= health rules the fist out when she has a gun with ammo. E4M7 HMP 1/4 -> 3/4 exits.
- Dropped after the regression set: E4M9's "not moving" judged from her position change rather than
  her momentum (pinned against a demon's box, momentum building, 4 bites): right by the mechanics,
  but E3 8.0 -> 5.4 (E3M6 stuck three times). To revisit.

Results (HNTR, one life): E1 8.0, E2 7.4 (7/8), E3 8.0 -> 7.9 (3/8), Doom II 28/32; arenas 118/128,
226/240.

## Larger changes after the 10-minute pass: a rocket's way held through its launch; no ammo spent on what she can't kill (2026-11-09)

The 10-minute pass's re-survey (pistol start, seed 0; exits ITYTD/HNTR/HMP/UV): Doom E1-E3 25/23/20/11
of 27 (26/22/16/11 before the campaign), E4 7/5/2/1 of 9 (6/5/2/0), Doom II 20/18/4/2 of 32
(20/18/3/3): 132 -> 138 level-skills.

- **A rocket's way held through its launch** (MAP32 UV, killed by her own rocket at 31 health; MAP30
  ITYTD, 112 of 182 damage taken her own rockets): the launcher lets the rocket go 8 tics after the
  trigger, along the way she faces then. Pressed at an SS, the fight took a new target the next
  tic and swung her 86 degrees through those tics; out mid-turn into the wall beside her. Now, from
  the press till it's away, her aim is held at the way she faced at the press (the way it was
  judged safe), unless that way is now unsafe (the turn off a rocket's way out, as before). MAP32
  UV and HMP: no rocket damage to herself. Tracking within 10 degrees instead (to keep leading a
  moving target) did worse over the regression set (E3 8.2 -> 7.2), so held.
- **No ammo spent on what she can't kill, keeping clear of it** (E4M4 HMP/UV: two cacodemons kept
  clear of and shot at till every gun was empty, then bitten to death unarmed): outmatched and
  keeping clear of a target all her ammo couldn't kill (`ammo_short_for`, from `too_much`), she
  holds fire unless it's in her way. Neutral over the regression set; E4M4 still runs dry by other
  ways into the same fight (what's wanted is not walking into it: the route, to do).

Results (HNTR, one life): E1 8.0, E2 7.4 -> 7.0 (4/8), E3 7.9 -> 8.2 (2/8), Doom II 28/32; arenas
118 -> 119/128, 226 -> 225/240.
- **Shut in on floors that hurt, a line to set off isn't held to a tenth of her health** (E4M2: a
  lava pit, no floor that doesn't hurt in reach, at 32 health the door's line out refused, 23
  against a cap of 3.2; walls tried in the lava till dead): staying costs all she has, so the cap
  is lifted for objectives with a trigger (switch, door, walk-over or teleport line) while she's on
  a floor that hurts and all her reach does. Lifted for every way (walls, looking, fighting) was
  worse (E4M2's helper: HNTR 386 -> 174 s). E4M2 ITYTD seed 0 exits (died). Regression set level
  (doom2 arena 225 -> 226).
- Tried and dropped: targets weighed by time to kill (threat over the kind's health, softened):
  Doom II 28 -> 25 (MAP10 twice, MAP02), E3 8.2 -> 7.6. Too broad; a narrower form (hitscanners in
  range over slow missile monsters) is for later.

## 20-minute pass, round A: unarmed to the ammo before cover; a floor hurts the same each time; a purposeful wade looked round (2026-11-09)

The 20-minute pass takes the 54 levels still not beaten on UV (pistol start, seed 0), four agents a
round, 20 minutes each. Round A: E1M5, E1M7, E1M9, E2M3.

- **Unarmed and shot at, the ammo before cover** (E1M5 UV: fists out at 13 health, an imp's
  fireballs and a demon on her; hid from the imp 190 tics, the demon followed, dead as the cover
  ended): low and shot at with no gun that has ammo, cover doesn't end anything (the cover ends
  when they stop shooting, and nothing she does there kills them): the retreat is for ammo, cover
  only when there's no way to any. E1M5 UV seed 0 exits.
- **A floor hurts the same each time** (E1M7 UV: a zombieman's 15 and a shotgunner's 9 on a plain
  floor 96 tics apart taken for the floor, and the room the whole level goes through refused): a
  floor's damage comes every 32 tics and is the same amount each time, so a second hurt only
  confirms the floor when it's a multiple of 32 tics after the first *and by as much*. E1M7 UV
  seed 0 exits.
- **A purposeful wade looked round before wading back; a door that shuts by itself no way out**
  (E1M9: a nukage room behind a door that walk-over lines outside open for a few seconds, the only
  way on a lift switch inside; she waded in for a look, turned straight back at the doorway every
  time, the switch never seen; once the door shut, too far a wade back for her health): stepped
  onto a hurting floor on purpose for a risky objective there, getting off isn't urgent while what
  she'd have left after paying the way back again is over the margin (it's one of the risky
  options, by distance); and safe floor reached only through a door that closes by itself with
  nothing on her side to open it doesn't count as a way out (`shut_in`, `in_pit`). E1M9
  ITYTD/HNTR/HMP seed 1 exit.
- Traced, not fixed: E2M3 UV (out of ammo by 535 s, then 800 s unarmed). `blocking()` ("a monster
  near and in front: walking on would walk into it") judges *in front* by her facing, and she faces
  what she aims at: anything she shoots within 320 is "in her way". The outmatched node's ammo hold
  flipped every tic and ~30 pistol rounds went into cacodemons she couldn't kill. Punching lost
  souls unarmed (a claw that can't kill her) was tried: the mechanic works, the runs went either way.

Results (HNTR, one life): E1 8.0, E2 7.0 (4 -> 5/8), E3 8.2 -> 7.0 (2 -> 0/8), Doom II 28 -> 27/32;
arenas 119/128, 226/240. E3's losses traced to failures the new runs walked into, none of the
rules: E3M6 seed 6 shut in past a WR door-close line (the open E3M6 stucks), E3M4 seed 1 under a
stopped crusher started again by a walk-over line one sector before its edge (`set_off_first`
knows only lines on its own edge), E3M8 seed 7 (the Cyberdemon). Doom II MAP12 seed 0: berserk
fists at a cacodemon, as before, this time killed.
- **What's in her way is the way the route walks her, for keeping ammo** (E2M3 UV, found by round
  A's helper): the outmatched node keeps its ammo for a target it can't kill unless that target is
  in her way. "In her way" was `blocking()`, "near and in front" by her facing; she faces what she
  aims at, so anything she aimed at within 320 was in her way, and the hold flipped tic by tic (~30
  pistol rounds into cacodemons). Now `in_her_way` judges it by the way the route last walked her
  (the route's controls, kept while the fight holds the route). `blocking()` keeps its facing
  sense for `standing` (the route waits while she fights what's close in front): changed there
  too (v41), the route walked on through close fights, E2 7.0 -> 7.9 but E3 7.0 -> 5.6 and Doom II
  27 -> 25 (her own rocket pressed walking on, MAP10/MAP12). Ammo hold only: E3 7.0 -> 7.1 (1/8),
  doom2 arena 226 -> 227, the rest the same.
- **A stopped crusher set off from a line before its edge** (E3M4 seed 1 in round A's regression
  set: a slow crusher stopped low by a crusher-stop line, its start lines a ring 8 units out from
  its edge; walking in, she crossed one and was under it as it came down, pinned, crushed 36
  times): `set_off_first` knew only a start line on the crusher's own edge. Now it takes the last
  line on the way in, up to and over the edge, that sets it off: the timing runs from her middle
  crossing that line, and she's set off from it at a crawl, backing off only as far as all of her
  is out of the crusher (less by how far out the line is). E3 7.1 -> 7.5 (E3M6 seed 6 through), the
  rest the same.

## 20-minute pass, round B: box-aware crusher escape, teleport lines kept out of fight moves, ammo worth being cut off for (2026-11-09)

Round B: E2M4, E2M5, E2M7, E2M8.

- **Out from under a crusher, all of her, round what's in the way** (E2M4 UV: dodging a baronball
  she set off a fast crusher's walk-over ring, 4 units of her box under its edge; the fight's way
  out took the first heading whose step 8 out had her middle outside, east along its edge, into a
  pillar's corner, pinned there till crushed): `way_out_of_crusher` needs room for her whole box at
  each step (any corner in a wall: not that way), and a heading is out only once all of her box is
  clear of a crusher at work (`box_in_crusher`, shared with `in_crusher`). E2M4 UV seeds 0-2 exit.
- **No fight move over a teleport line from its front** (E2M7 HMP: backing off from a demon
  towards a rocket launcher, over a WR teleporter's front, into a closet with two demons in front
  of its only way out, bitten to death): `safe`/`slide_ok` run the slide for walls, drops and
  hurting floors; now also for a player teleport line crossed from its front (P_CrossSpecialLine
  acts from side 0 only). Taking a teleporter on purpose is the route's. HMP seed 0 160 -> 175 s.
- **Short of ammo, the ammo is worth being cut off for** (E2M5 UV: out of ammo, three clips and a
  shell offered, every one past doors she knew no way back through, so cut off by the reach check
  that only lets the exit, a teleporter or a key cost that much; she explored a dead end instead,
  the baron followed, cornered): an urgent ammo or gun item the level placed counts as worth it
  while she's short (`Hooks::worth_cut_off`). UV seed 0 234 -> 378 s.
- Dropped after the regression set: E2M8's "a mover she waits at that won't move for her" (every
  known line that moves it a used one-shot or out of reach on the live heights: replan or fail the
  step): it judged ordinary lifts she was about to call unmovable; E1M7 stuck 7/8, E1 8.0 -> 6.2,
  Doom II MAP05 stuck x3.
- Leads noted: `ammo_short_for` compares her ammo with a monster's full health (the cyberdemon's
  4000: E2M8 UV held 50 rockets "short" all fight); `for_ammo`'s fallback "away" spot can be nearer
  the monsters; the `to_errand` node spends the last rounds on an outmatched target.

Results (HNTR, one life): E1 8.0, E2 7.0 (5 -> 6/8), E3 7.5 -> 6.6 (E3M6 seed 4 stuck: a secret
door's wall try interrupted, fixed next), Doom II 27/32; arenas 119/128, 227/240.
- **A wall's try counts once it's pressed** (E3M6, round B's regression set, twice: shut in by a
  closing door, the way out a secret door she could open from her side; its try began as she
  stepped over a line that lowers a floor, the look round that set off took the press's time, and
  the wall, marked tried on arrival, was never offered again: stuck the hour): a wall is tried when
  pressed, or when its time runs out unpressed; a look round before the press starts the try again
  after (not after a press: a second press shuts some doors again).
- **A rocket's way held through its launch after the fight's over too** (E3M9: the last rocket
  pressed at a baron as it died; the fight over, the route turned her for a door through the 8
  tics before launch, the rocket out into the wall by her, dead at 53): with nothing left to fight
  and a rocket still to go out, the fight keeps the aim (its way out if one's needed, else the way
  she faced at the press), the legs the route's.
- **A homing missile going round her is shaken off round a corner** (MAP10: four revenant tracers
  bunched into one, dodged till the hour ran out): a homing missile (the revenant's tracer) still
  dodged 3 s on, with nothing else in sight after her, steers the dodge (among the ways clear of
  it) towards the nearest place it can't see her: straight off if there's one within 384, else
  round corners by the mesh (`nearest_heading`, from the crusher escape's search). Overriding the
  dodge instead ran her into the bunch (195 at once). In a fight (things in sight after her) it's
  dodged as before: running off round a corner walked her into an arachnotron's plasma.

Results (HNTR, one life), with round B: E1 8.0, E2 7.0 -> 7.2 (6/8), E3 6.6 -> 7.4 (1/8), Doom II 27
-> 25/32 (MAP10 seeds 0 and 2 killed by barons at the exit line; seed 0 was the tracers' hour);
arenas 119/128, 227 -> 226/240.
- **What she's put into a monster counts against keeping her ammo** (E2M8 UV, found by round B's
  helper: fifty rockets held "short" of the cyberdemon's 4000 all the fight, killed with it at
  half): the outmatched node's ammo hold compared all her ammo with the monster's full health.
  Now `ammo_short_against` takes off what she reckons she's put into it: the rounds gone from the
  gun up while it was her target, at what such a shot does landing at that range (never its
  health). E2M8 UV seeds 0-3: 1/4 exits either way (seed 0 now, seed 2 before), each living as
  long or longer (328/162/410/265 -> 449/162/571/329 s). Regression set level: only cyberdemon
  arena fights move, both ways (doom2 arena 226 -> 225).

## 20-minute pass, round C: a sleeping pack not woken on an errand; the way to a gun out of reach weighed (2026-11-09)

Round C: E2M9, E3M3, E3M4, E3M5.

- **A shot that would wake a sleeping pack isn't fired on an errand either** (E3M5 UV: out of ammo
  and off for a clip, through a room of seven sleeping demons she'd sneaked past five times; on
  the errand `to_errand` came before `sneaking`, "shooting on the way to Clip" at a demon, her last
  bullets woke all seven, bitten to death with the fist): `sneaking` now comes before `to_errand`
  in the fight tree: no shot that would wake a pack worth too much, errand or not; the route keeps
  the legs. UV seed 0 422 -> 1040 s.
- **A frontier into a better gun's sector, out of reach, weighed as a switch is** (E3M4 HMP: a
  chaingun and a dropped shotgun seen on ground with no known way on; the frontier that was the
  way to both (and the exit) risky for its monsters, the risky pass by travel alone took two
  cacodemons over a hurting floor with the pistol): with a better gun seen and out of reach, a
  frontier whose unseen polys are in its sector counts a quarter as far (`SWITCH_FOR_GUN`, as for
  switches). E3M4 HMP seeds 1 and 2 exit (1/10 -> 3/10 over HMP/UV seeds 0-3 and the two lower).
- Dropped after the regression set: E3M4's "no look round again for a walk-over line set off
  before" (two crusher-stop lines across a stair crossed back and forth in a fight, a look at each):
  E2 7.2 -> 7.8 and E3 7.4 -> 7.9, but Doom II 25 -> 23 (MAP12 seed 2 stuck: line 511, a W1 floor
  raise whose effect a later crossing would have shown, never learned, never planned with; MAP12
  seed 3, MAP10 seed 1). What's wanted: the look skipped only for lines whose effect can't be seen
  from where she is (to do).
- Traced, not fixed: E3M3 UV (the start crowd eats 50 bullets; the shotgun behind a switch with
  imps near it is `dangerous` before it's weighed; saving the pistol for the switch is the idea);
  E2M9 (four barons, damage spread over three, none killed; focus fire lost ITYTD exits because the
  back-off follows the target, stepping from a far wounded baron into a near one's claws: back off
  from the nearest clawer instead; ammo behind a baron dropped by `clear_way_to` while a Medikit is
  urgent, the explorer and `for_ammo` pulling her back and forth).

Results (HNTR, one life): E1 8.0, E2 7.2 (6/8), E3 7.4 -> 7.6 (1/8), Doom II 25/32; arenas 119/128,
225/240.

## 20-minute pass, round D: a lift's floor is no crusher; a better gun by the boss; a far ammo errand; no standing on a hurting floor (2026-11-09)

Round D: E3M6, E3M7, E3M8, E3M9.

- **Sectors only lifts move aren't crushers when their floor comes up** (E3M9 UV seed 0: a
  thin lift-as-door sector, floor 64 to 0 and back, by the switch she was pressing; her box's
  edge over it as it rose made her "under a crusher", `off_crusher` gave the route the legs
  (waiting at the lift) over the dodge, a cyberdemon rocket due in 21 tics hit, 136 at once): a
  plat that doesn't crush reverses when blocked (vanilla T_PlatRaise), and no lift crushes, so
  like doors' ceilings, a lift-only sector's rising floor isn't `crushing`. E3M9 seed 0: UV
  died 357 s -> exits, HNTR died -> exits, HMP lives 176 -> 405 s, ITYTD exits as before; UV
  seeds 1-3 unchanged.
- **A better gun judged against the boss she knows is alive** (E3M8 ITYTD: the plasma rifle 246 off
  not urgent, the launcher "better" by its blast, 252 against 205 a second; the spider mastermind
  takes no blast (P_RadiusAttack skips it); the rifle then ruled out as dangerous, rockets spent on
  cacodemons, kept clear of the spider with the pistol, shot down): `better_gun` compares guns by
  damage a second on a boss she knows is still alive (`firepower_on`/`dps_on`: a rocket on a boss
  by its hit alone), so the gun better against it is "a better gun", urgent, past the danger
  filter. E3M8 ITYTD seed 2 exits (the first E3M8 exit); seed 0 has the spider down to 120.
- **No standing still on a floor that hurts** (E3M6 HMP: on the exit route over nukage at 28
  health, ten lost souls 1100-1400 off, `standing` (low health) held the route "closing in", dead
  on the floor): on a hurting floor `standing` passes unless the target's in her way; the route
  goes on, shooting (as `stepping_for_shot`).
- **A far ammo item isn't the fight's errand** (E3M7 HMP: four cacodemons, every rocket "not safe";
  the urgent RocketBox 3938 off made the fight's errand, backed off "towards" it into them): mid-fight
  an urgent ammo item is the errand only within `ITEM_RANGE` (as the explorer's own turn-aside for
  ammo; health already 512). Out of ammo, the for-ammo retreat still routes her to it. HMP seed 0
  exits again.
- Dropped after the regression set: E3M6's rocket check with her own run projected over the whole
  flight (E3M6 HMP: the route running her at a cacodemon, a rocket fired 621 off burst 107 from her).
  Over the flight for every shot, the arch-vile arena lost 2 (arch-viles 1063 off "not safe");
  narrowed to the route having her legs, Doom II MAP10 seed 3 died holding 7 rockets "not safe" on
  the way. What's wanted instead: when a rocket's held for her own run, the fight stops her for it.

Results (HNTR, one life): E1 8.0, E2 7.2 -> 6.9 (seeds both ways: E2M4 seed 5's stuck and an E2M8
death gone, E2M2 seed 0 (the crusher corridor) and two E2M8 deaths new), E3 7.6 (1/8), Doom II
25/32; arenas 119/128, 225/240.

## 20-minute pass, round E (Doom II): no saw rush past shooters; cover through doors and in time; an errand's way clear of them all (2026-11-09)

Round E: MAP01, MAP03, MAP04, MAP05.

- **No closing in with the saw or the berserk fist with a shooter about** (MAP01 UV: the chainsaw
  picked against an imp of five, 84 health; the melee node walked her in and stood her at it while
  the other four fireballed and clawed her to death, 0 rounds fired the level): `best_gun` skips the
  up-close weapons when she'd have to close in (the target past the melee node's "at it" distance),
  another engaged monster attacks from range, and she has a gun with ammo. One already at her still
  gets the saw. MAP01 UV seeds 0-7: 7/8 -> 8/8.
- **Cover in time, through doors, not held off by another retreat's failure** (MAP04 UV: through a
  door that shut behind her into a yard of hitscanners on a better-gun errand; every cover spot back
  behind the shut door, routed with no doors, all failed silently; cover only due under 40 health;
  a failed try at another retreat at full health held every retreat off 105 tics): cover is routed
  through ways open now, then through doors she can open (as the ammo retreat); due also when, at
  the rate she's losing health, she'd be under the retreat line by the time she'd run the cover
  range (`cover_due`); a failed different retreat doesn't hold off cover. MAP04 UV still dies
  (later); the killer is the urgent better-gun errand past `dangerous` (to do).
- **An errand for ammo or a gun is clear only of everything by the way together** (MAP05 UV: out of
  ammo, four shells in a yard with three awake imps; each imp alone "killable" with four shells,
  the super shotgun's blasts counted at 200 each against 60-health imps): as if she had the item,
  what she'd have must kill all of them awake by the way, each shot counted no further than the
  toughest one's health (`ammo_short_for_each`, `clear_way_armed`; only this check: in
  `ammo_short_of` everywhere it cost MAP05 HMP its exit). MAP05 UV seed 0 31 -> 52 s.
- Dropped after the regression set: MAP03's "the fight's errand is the retreat's way while backing
  out", "a clawer touching her when she's pinned gets shot" and "no backing off towards an errand
  nearer the clawer" (HMP 4/6 -> 6/6 but HNTR seed 2 stuck in the regression set: a one-way drop
  into a pit whose way out is a floor she'd raised, and UV 3/6 -> 2/6, the errand taking her back
  into the closet trap at low health). What's wanted: no going back on an errand into the room just
  backed out of; no one-way drop where the only way out is a mover not as it needs to be.
- Seen, to trace: E2M4's "nothing left to do" stucks (regression seeds, and HNTR seed 0 on its own)
  with unseen polys past a door, one-way drops and a teleporter she's used.

Results (HNTR, one life): E1 8.0, E2 6.9 -> 6.1 (E2M4 seed 3 stuck, two cyberdemon deaths on E2M8),
E3 7.6 (1 -> 4/8 through), Doom II 25 -> 26/32 (MAP12 seed 0); arenas 119/128, 225/240.

## 20-minute pass, round F: floor damage through armor; out of a hitscanner's sight round a corner; rockets judged from rocket range (2026-11-09)

Round F: E2M4's "nothing left to do" stucks, MAP06, MAP07, MAP08.

- **A hurting floor's damage goes through armor** (E2M4 HNTR, stuck the hour, and the regression
  set's E2M4 stucks: the way on a secret door pressed from a small lava room; with blue armor, her
  last visit there 10 and 10 armor a hit, the route's cost counted 69.5 of health against 80, under
  the 25 margin, every way there refused; "nothing left to do"): vanilla `P_DamageMobj` takes a
  floor's damage through armor like any hit, so the route's floor cost is the raw damage less what
  her armor takes, while it lasts (`Observation::through_armor`, shared with the crusher stroke's
  cost). E2M4 HNTR seeds 0 and 3 stuck -> exit; none lost.
- **Outmatched by a hitscanner she's keeping her ammo from, round a corner out of its sight**
  (MAP06 UV: the spider mastermind 500-700 off; "keeping clear (ammo kept)" steered her back and
  forth in its sight for 20 s, not firing; no straight-off spot to duck to; shot down): distance does
  nothing against hitscan, so with fire held at a hitscanner that outmatches her she goes the mesh's
  way to the nearest place it can't see (`hidden_way`, the tracers' search). MAP06 UV lives longer
  on seed 0, shorter on seed 1; no exit either way.
- **A rocket judged from where she'd fire it, and a pack past its blast no bar** (MAP08 HNTR: the
  launcher's demon pack; `best_gun` dropped the launcher whenever a rocket wasn't safe from where she
  stood, so the fist was her gun and she closed in, 11 rockets unfired; and `rocket_safe` refused a
  rocket for any monster near its path, twelve demons 950 off "not safe"): a rocket judged from
  further out than she stands, past its blast, isn't ruled out by the target's being close now (she
  backs off to it); a monster in its path bars it only within blast range of her (`rocket_clear`).
  MAP08 HNTR seeds 0-3: every one past the ambush, seed 2 exits (died at 52-74 s before).
- Traced, not fixed: MAP07 UV (the super shotgun against nine arachnotrons; the plasma rifle and
  launcher behind platform switches whose routes the fight held a minute at full health; re-choosing
  an "out" retreat's spot and dodging towards its way cost HMP its exit).

Results (HNTR, one life): E1 8.0, E2 6.1 -> 7.9 (3 -> 7/8), E3 7.6 -> 7.9 (3/8), Doom II 26 -> 28/32
(MAP08 seed 2 exits); arenas 119/128, 225 -> 223/240 (two draws: indoor cacodemons, the spider
mastermind's cover).

## 20-minute pass, round G: a target with no shot at it given up; no margin at a dead end; kill first only what she'd kill soon (2026-11-09)

Round G: MAP09, MAP10, MAP11, MAP12.

- **No standing at a target she's had no shot at for 20 s** (MAP09, every skill stuck the hour: an
  island in a nukage pit, two zombiemen on ledges around it, "no shot (too steep)" at either and
  them never hitting her; `standing` and "lost sight: keeping at it" held the route, switching
  between them, every step given up as taking too long, the dead end's fallback never reached):
  with no shot at a target for `CLOSE_GIVE_UP`, it's given up for `GIVEN_UP_TICS` as an
  out-of-reach or melee one is (a shot resets the clock, so does a gap over 20 s; one in her way
  is exempt); `standing` passes to shooting on the way (`no_shot_long`). Narrowed to "not with a
  fist up, being hurt resets it" it lost the MAP09 exit: kept as is.
- **No health margin at a dead end** (MAP09: with the route free, the dead end's lift switches,
  each a wade costing 15 at 37 health, refused by the 25 margin, which waits for healing or a
  radiation suit she can't reach): with `harm_ok` (a dead end) and a line to set off, only a way
  that would take all her health is refused. MAP09 ITYTD seeds 0-2 stuck -> exit; HNTR and HMP
  stuck -> die (at the switch on the floor, 25 health: wall searches in the nukage spent her health
  before the switches; to do).
- **Kill first only what she'd kill soon** (MAP11 UV: the pistol at a pain elemental, 400 health,
  some 16 s of pistol, its 2.5 priority; three shotgun guys by it shot her dead untargeted): the
  arch-vile's and pain elemental's kill-first boost holds only if she'd kill it within 8 s
  (`KILL_FIRST`), and drops only with another hitscanner in sight. MAP11 UV seed 0 8 -> 20 s (the
  shotgun guys killed, the shotgun picked up).
- Dropped after the regression set: MAP12's berserk fist or saw passed over for a target she
  wouldn't close in on (HMP seed 0 exits) cost Doom II MAP10 seed 0 (the barons at the exit).
- Traced, not fixed: MAP10 HMP (a caged cyberdemon behind a window by the only gap in a fence; the
  objective resumed back to the window after it hit her: `dangerous()` is checked only at the
  choice; ending it near an unbeatable monster cost HMP seed 1's exit); MAP12's line 511 (a W1
  floor raise between two slime sectors, the way on, never crossed: walls tried, the slime room not
  walked; regression seeds stuck on it).

Results (HNTR, one life): E1 8.0, E2 7.9 (7/8), E3 7.9 -> 8.0 (3 -> 4/8), Doom II 28 -> 27/32 (MAP12
seed 1 stuck on line 511); arenas 119/128, 223/240.

## 20-minute pass, round H: seams walked over; two crushers side by side; a stalled melee given up; a hurting floor's replan at once (2026-11-09)

Round H: MAP12's walk-over line, MAP13, MAP14, MAP15.

- **Lines between look-alike sectors she's stood on both sides of, never walked over** (MAP12 HNTR
  seed 1 stuck: line 511, a W1 floor raise between two slime sectors, each entered across other
  lines, so no threshold over it was ever offered; "try the wall" stood her 40 units short):
  she keeps the lines her middle has gone over (`crossed`, cleared on a restart) and each
  sector's heights when first seen (`first_heights`); with nothing left before the things given
  up on, she walks over each line between two reachable polys she has stood in whose sectors
  looked the same when first seen (heights, flats, light: a split only a trigger or a mover
  needs), 32 units into the side away from her (`seams`). MAP12 HNTR seed 1 stuck -> exit (937 s);
  seeds 0, 2, 3 and fight 1 seed 0 exit as before, fight 3 seed 0 dies as before (567 s).
- **A crusher the way goes into straight out of another is judged before the first is entered,
  and from under any of her** (MAP13 UV: a fast crusher, a slow one beyond it with the plasma
  rifle in it; under the fast one, the follower's "under one already" branch never looked ahead, so
  she walked on into the slow one on its way down, pinned under its edge; and the escape took only
  the first ceiling coming down over her box, through the fast one in time, the slow one over her
  edge ignored). Now (`hazards.rs`): the walking rule's go for a crusher also needs the one straight
  after it open now (`open_now`); under one (any of her), the next crusher out of where her middle
  is not open now (and no crossing of it begun) is out from under all of them (`escape_crusher`),
  not on into it nor a wait at its edge under one (E4M7's); and every ceiling coming down on her is
  weighed for the escape, not the first found.

Results, MAP13 seeds 0-3 (ITYTD seed 0): ITYTD exits (1547 -> 1209 s); crusher deaths 5 -> 2 of 12
(UV s0, HMP s1, s2 gone; HNTR s2 is the fight falling back to cover from inside the slow crusher
with the rifle in hand, HMP s3 out of ammo, the route's crusher_guard not the walking phase:
traced to there, not fixed). No exit anywhere on HNTR-UV either way.
- (The seams, after the regression set: a seam is tried once its trip is over, gone over or not;
  a spot past a line can be got to round it, and E1M5 walked two of them in turn for the hour, E1
  8.0 -> 7.0.)
- **A melee that gets no nearer is given up, and a ledge stays a ledge** (MAP14 HMP: out of ammo,
  berserk; a chaingunner on a ledge 300 off she can't climb to, walked at with the fist 16 s till
  dead; the melee node's give-up is 20 s without the target's health dropping, and `armed()` counted
  the berserk fist with everything after her punchable, so the ammo retreat never began): closing
  in that gains under 32 units in 3 s gives it up (`MELEE_GAIN`/`MELEE_STALL`) and remembers its
  sector: while it's still there and she isn't, it's out of her fist's reach (`out_of_fist_reach`),
  and the berserk fist counts as arming her only if nothing after her is. MAP14 HMP seed 3 exits;
  seeds 0-2 live longer.
- **Stuck on a floor that hurts, a new path at once** (MAP15 HNTR seeds 2 and 3: a walk-over line
  lowered the ledge she was on into lava; the walk, planned from up there, ran into a step 32 over
  the lava, and the follower backed off twice before a new path, 3 s at 20 a second): on a hurting
  floor no backing off first.
- Dropped after the regression set: MAP15's rocket hold through the launch tic, turned back to the
  press's facing only if still clear (no self-rocket damage on MAP15): the combat arena 119 -> 117
  (arch-vile and revenant fights).

Results (HNTR, one life): E1 8.0, E2 7.9 -> 7.6 (5/8), E3 8.0 -> 7.2 (4 -> 0/8; no one change: each
taken out alone gives 7.2-7.4, the spider mastermind's fight by seed), Doom II 27/32 (MAP12 seed 1
exits, MAP10 seed 0 dies at the exit); arenas 119/128, 223/240.

## 20-minute pass, round I: a retreat keeps its legs over a dodge that clears nothing; the fist not up for what it can't reach; a way past the monster already at her (2026-11-09)

Round I: MAP16, MAP17, MAP18, MAP19.

- **A retreat's legs kept from a dodge that clears nothing; the dodge leans its way** (MAP16 HNTR:
  the blue skull on a small raised platform, its trap's imps; "backing out the way she came", yet
  per-tic positions within 20 of the key from the retreat's start till dead, 197 from fireballs:
  on so small a ledge `safe()` turns down almost every way, the dodge had none or flipped two
  opposite ways tic by tic, standing her still, and its pull was the errand's goal, the key under
  her): retreating, a dodge takes the legs only with a way clear of every missile coming
  (`dodge_clear`: its best score >= 0); falling back, it leans towards the retreat route's next
  point (`RETREAT_AHEAD` on), not the errand. MAP16 ITYTD and HNTR seeds 0-3: 0/8 -> 8/8 exit.
- **The fist not up for what it can't reach while a gun has a shot** (MAP17 HNTR, stuck the hour:
  berserk, a pain elemental on a ledge 80 up; the melee given up, `best_gun` kept the fist, free and
  fastest up close; the tree flipped between stepping for a shot and closing in 16,000 times each,
  149 bullets and 50 shells never fired): `best_gun` passes the fist over for a target out of her
  fist's reach (given up on, or where a stalled melee left it) while any gun has a shot in it.
  MAP17 HNTR seed 2 exits, the others much further; ITYTD seed 2 stuck instead (a revenant 1200
  off closed in on for the hour: a shot but no reach, nothing gives it up; to do).
- **A way off for ammo isn't walled off by the monster already at her** (MAP19 ITYTD: the pistol
  empty, two spectres at 47 and 55; `for_ammo`'s way, passing no nearer than she was, was called off
  the next tic by `walled_off`, which counted any monster within touch of the way, the one beside
  her too; every retry the same, bitten to death on the spot): a monster walls off her way only if
  the way passes within touch of it and nearer it than she is now (less `AMMO_MARGIN`), the test
  `for_ammo` picks by. And (MAP18) a for-ammo way called off as walled off counts as that try: the
  cover branch goes to cover for `RETREAT_AGAIN` (it flipped ammo, over, ammo, tic by tic, under
  fireballs).
- Seen, to do: a lip over a demon pit (MAP19: her box on the ledge, her middle over the pit, bitten
  from below with no shot, `blocking()`'s facing sense exempting it from the no-shot give-up);
  MAP16 HMP/UV's arch-vile at 30-39 s; MAP18's opening imps (the far nearly-dead one targeted while
  two claw her).

Results (HNTR, one life): E1 8.0, E2 7.6 -> 7.8 (6/8), E3 7.2 -> 8.0 (1/8), Doom II 27/32; arenas
119 -> 118/128, 223 -> 222/240 (spider and cyberdemon fights both ways).

## 20-minute pass, round J: a barrel chain judged whole; a dodge clear of the hit; the fist's true reach; no knife-edge squeezes (2026-11-09)

Round J: MAP20, MAP21, MAP22, MAP23.

- **Barrels judged by the whole chain, and the chain outrun** (MAP23, every skill dead at 4-5 s:
  the start hall one chain of barrels, a mancubus behind a row): a barrel by a monster is shot
  only if its whole chain goes off clear of her (`barrel_shot` checked only the barrel's own 224);
  a way away from barrels mustn't bring her nearer any barrel near where it ends (it took her from
  one row into the next), else that node steps aside; a chain going off is fled from the barrel
  that goes off first (fewest blasts from a lit one, then nearest: it burns a barrel a blast); and
  keeping clear of what outmatches her, while the route walks her away from it (within 90
  degrees) the route keeps the legs. MAP23 start hall survived (5 -> 9-29 s); no exit yet.
- **A dodge clear of each missile's hit, not its blast** (MAP20: the cyberdemon's rockets down a
  corridor 128 wide, a blast needing 151; no dodge "clear", so the retreat's route had the legs and
  stood her at a shut door 12 off a rocket's line, 100 at full health): a dodge counts as clear when
  it clears every missile's own hit (`hit_clearance`: the blast and up to 160 more); no duck whose
  way crosses a missile's hit (the duck from the spider ran her across a rocket's line); no cover
  in a sector that's shut now (hidden only behind the shut door, in the doorway once there).
- **The fist's and saw's reach is 64, middle to middle** (MAP22 HNTR: berserk, punching a
  chaingunner from 68-77 off, not one landing, shot 16 times): the punch's trace runs `MELEERANGE`
  (64) from her middle, and a monster's hit where it crosses the diagonal through its middle
  (`PIT_AddThingIntercepts`), so its radius adds nothing; reach 64 middle to middle in `in_reach`,
  the melee node and the arsenal, the stop 12 short (`MELEE_SLACK` 24 -> 12). MAP22 HNTR seed 0 and
  HMP seed 2 exit, HNTR seed 2 dies (net +1).
- **No squeeze past shut sectors** (MAP21: hundreds of "stuck" at a gap exactly the box's 32 wide,
  between a wall and a bar's end or two bars' corners 24 apart both ways; the engine lets her
  through only at the exact unit moving exactly along it, which the follower never hits):
  `fits_truly_near` (a smaller box fitting within 8 of each squeeze point, 401d947) is gone, a
  squeeze past shut sectors she can't open is no way, and an objective's stand spot must fit the
  box with half a unit of play (`with_play`). MAP21 no monsters seed 0 stuck -> exits (61/61
  sectors); with monsters, deaths in the gap become stucks further on (a run-jump way not found).

Results (HNTR, one life): E1 8.0, E2 7.8 -> 7.5 (cyberdemon deaths on E2M8), E3 8.0 (1 -> 3/8),
Doom II 27 -> 29/32 (MAP08 and MAP12 seed 2); arenas 118 -> 119/128, 222 -> 223/240.

## 20-minute pass, round K: a quick hitscanner fought where it stands when she's outmatched (2026-11-09)

Round K: MAP24, MAP25, MAP26, MAP27.

- **Outmatched, a hitscanner that dies quickly is fought, not kept clear of** (MAP27 HMP: an ambush
  room, the door shut behind her, walls dropping on zombiemen, sergeants and chaingunners with
  demons behind; outmatched by the crowd, each former human "kept clear of" in its sight or "got
  past" deeper in, 100 -> 10 in 6 s from 40-110 off): hitscan doesn't fall off with range, and a
  non-sniper hitscanner dies sooner than she'd get out of its sight; outmatched, such a target
  (`quick_hitscan`) is fought where it stands (no keeping clear or getting past it, `standing`
  treats it as a duel). MAP27 HMP seed 2 138 -> 996 s, UV seed 3 107 -> 750 s; no exit either way.
  Regression set level.
- Dropped after the regression set: MAP24's "along her sides only what stops her box blocks a
  way" (`clear_run`'s side lines ignoring drops: the dodge on a walkway's edge had no way at all,
  four fireballs hit; MAP24 HNTR seed 0 exits): E1 8.0 -> 7.2 (E1M3, a dodge mid-run-jump into a
  nukage pit) and E2 7.5 -> 7.0.
- Traced, not fixed: MAP25 (backing out over a hurting floor at a crawl: `Motion::go` cuts the
  route's speed to 15-45% when its way is over 90 degrees from her facing, meant for the route
  turning to it; with the fight holding her aim the turn never comes and `combat.controls` keeps
  the cut, 2.4 a tic over a blood pit; Doom backpedals at full speed. And `Fitter::detour` skips the
  walls her box touches at a segment's end without the crossing check `straighten` has: a detour
  through the wall she's pressed to); MAP26 UV (ammo gone on the start's chaingunners; a dropped
  chaingun ruled dangerous; `for_ammo` routes on the heights now, blind to lifts; `dangerous()`
  counts monsters not known awake).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.5, E3 8.0, Doom II 29/32; arenas 119/128, 223/240.

## The Icon of Sin's brain may be known (the maintainer, 2026-11-09)

The boss brain (MAP30) is hidden behind its wall, but its wake-up sound plays to the whole level:
she may treat it as known (the maintainer's call). The way to it, traced in round L: `A_BrainDie` ends the
level on any map; the brain has 250 health behind a slot 32 tall; a rocket (spawned at her z + 32,
flying level, going off on any floor it meets) gets in only when fired with her z inside the slot's
gap less 32, a window of about 6 tics on the lift's climb; 108 a hit, three rides. To build: the
brain as an exit objective whose activators are floors (a mover's range among them) from which a
rocket spawns inside the opening on the line to it, and a node that fires in the window; rockets
kept for it.

## 20-minute pass, round L: no fist or saw brought up within two claws of death (2026-11-09)

Round L: MAP28, MAP29, MAP30, MAP32.

- **Bringing up the fist or saw in a claw's reach weighs two claws, not one** (MAP28 HMP: a demon 50
  off at 43 health, its bite up to 40; `best_gun` picked the chainsaw, cheaper than the pistol, the
  one-claw rule letting it; the switch takes some 32 tics, more than a demon's 24-tic bite cycle,
  bitten for 28 mid-switch, then the saw ruled out and `SWITCH_WAIT` holding the pistol back,
  bitten dead): if the fist or saw isn't up yet and the target's within claw reach, two claws are
  weighed against her health. MAP28 HMP seed 2 19 -> 53 s. Regression set level.
- Dropped after the regression set: MAP32's aim held where she faced at the press till the shot
  goes out, for every gun (the pistol's bullet 4 tics after the press, the shotguns' 3; a fight
  flapping on and off swung her 10-20 degrees through each; MAP32 exits 6/16 -> 7/16 seeds): E3 8.0
  -> 6.6.
- Traced, not built: MAP30 (the brain: see the entry above, the maintainer's call that she may know of
  it); MAP29 (a run-jump landed 50 degrees off its line into lava with the fight shooting through
  the sprint, unproven; a one-way drop into lava for a wall look; an SR lift line reachable from the
  lava never tried).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.5, E3 8.0, Doom II 29/32; arenas 119/128, 223/240.

## 20-minute pass, round M (episode 4): a floor priced by what one like it has done; no chasing what's out of sight over a hurting floor (2026-11-09)

Round M: E4M1, E4M2, E4M3, E4M4.

- **A floor that looks like one that hurt her is priced at what that one did** (E4M1 HMP, stuck the
  hour: the red door pressed from a nukage trench the map gives no damage, never stood on, so
  priced at the worst a floor does, 20 a hit, 26.7 at 37 health against the 25 margin, refused;
  the pit beside it with the same flat had hurt her 5 a hit): a floor she takes to hurt that the
  map gives no damage for is priced at the most a floor with the same flat has done to her
  (`floor_hit`), the worst only if none like it ever has, along a route and at its end. E4M1 HMP
  seed 0 gets through the red door (still stuck past it: a never-hurt BLOOD3 floor priced at 20).
- **No chasing what's gone out of sight while on a floor that hurts** (E4M2 HNTR: a lava hall under
  a suit, a demon with no shot at it (too steep) seen and lost by turns, "keeping at it" after it
  for 1500 tics till the suit ran out, 125 of lava; E4M3 HMP the same over a cacodemon): on a
  floor that hurts by the map (a suit's time runs out), `lost_sight` doesn't take the controls;
  the route's way off comes first. E4M2 ITYTD seeds 0-7 exits 4 -> 5, HNTR 1 -> 3.
- Dropped after the regression set: E4M3's `standing` on a hurting floor judged by the route's way
  rather than her facing (E3 8.0 -> 7.0 with it, chaotically: taking out either it or the
  `lost_sight` guard gives 8.0); E4M4's re-path at the act phase when a fight has pulled her off
  the spot (pressed from 1100 off, a straight line over lava): the doom2 arena 223 -> 221
  (cyberdemon fights).

Results (HNTR, one life): E1 8.0, E2 7.5, E3 8.0 -> 7.8, Doom II 29/32; arenas 119/128, 223/240.

## 20-minute pass, round N (episode 4): an out-of-reach goal for ammo dropped; no retreat from the route to ammo that keeps clear (2026-11-09)

Round N: E4M6, E4M7, E4M8, E4M9. The last of the 20-minute pass.

- **A goal for ammo she can't reach past them is dropped** (E4M9 UV: out of ammo, the rocket
  launcher behind a door that opened on an imp and a baron; `for_ammo` refused it, no way past
  them, "away from them", then out of sight the fight ended and the explorer took the same goal
  back into them, 17 times in 1200 tics): when `for_ammo` finds no ammo or gun reachable without
  passing what's after her and her goal is one it refused, the goal is dropped
  (`Explorer::give_up`); the next choice is made knowing they're there. E4M9 HNTR seed 2 exits.
- **Unarmed, no retreat from a route to ammo that keeps clear of them** (E4M7 UV: out of ammo, the
  explorer's way to ammo through a door, a drop and a run-jump, which `for_ammo`'s plain way on the
  heights as they are can't see; "away from them" through the door, the route back through it, for
  600 s): with nothing from `for_ammo`, if the objective is ammo or a gun she can use and its path
  keeps `for_ammo`'s clearance, no retreat; the route goes on. Results unchanged (a loop broken).
- Dropped after the regression set: E4M8's backing off on a hurting floor giving the route the legs
  (she outruns any clawer) and `standing` judged by the route's way there (E4M8 HMP 1/4 -> 3/4):
  E3 7.8 -> 6.6 (both parts; the second alone 7.2).
- Traced, not fixed: E4M6 ITYTD (stuck: a route between two closed door "pillars" 24 apart, floor
  at ceiling, a blue door raising them; her knowledge takes never-opened door sectors as passable;
  after the stuck, every objective "no route").

Results (HNTR, one life): unchanged: E1 8.0, E2 7.5 (4/8), E3 7.8 (2/8), Doom II 29/32; arenas
119/128, 223/240.

Over the 20-minute pass (rounds A-N, 54 levels not beaten on UV): from E1 8.0, E2 7.0, E3 8.2, Doom II
28/32, arenas 119/128 and 226/240 to E1 8.0, E2 7.5, E3 7.8, Doom II 29/32, arenas 119/128 and
223/240, with exits gained on many levels' seeds (see each round). Next, as the maintainer set it: larger
changes, then the 40-minute pass; paused here at the maintainer's word.

## The score: every level, HMP and UV, eight random seeds (the maintainer, 2026-11-09)

Progress is measured by a score, not the regression set (the maintainer's call: an episode counts a
late level only when everything before it went right, unfair to later levels; single-seed
surveys swing with one seed; arena fights are too artificial to count, a tool for trying ideas
only). `evil-boom score` plays every level of Doom (E1-E4, 36) and Doom II (32) from a pistol start
on one life (mortal), on HMP and UV, eight random seeds each (1088 runs), a TSV row a run
(`out/score/`). A run scores 1 for getting out, else half the share of the level's monsters killed
(partial credit, the maintainer's measure: how far into the fight she got; the half keeps a run that
kills all and dies below one that gets out); a level's score is its runs' mean; the score is the
mean over levels and skills, each the same weight. Beside it, how consistently levels are beaten
(the maintainer wants that optimized too): the spread (each level's variance of its runs' points,
averaged), and the levels beaten on every run and on none. `score-report A.tsv B.tsv` compares
sets: the score's own noise between measurements, and which levels move most. `run-map`'s RESULT
line now always carries `kills x/y`.

## Stuck round: eight agents on the levels that stall most (2026-11-09)

The first round judged by the score. Eight agents, 30 minutes each, on the levels with most
stalled runs in the baseline (MAP19, E4M1, MAP21, E3M7, MAP06, E4M5, E4M6, E1M9), each measuring
its level with `score` before and after; seven fixes merged.

- **No standing at a target with no shot, in front of her or not; a route waiting to press has
  the turn** (E4M5: on a 16-wide ledge over the exit's pit, a shotgun guy below too steep for
  either to hit, `standing` held 300 s, its no-shot give-up exempted by `blocking()` (facing: she
  faces what she aims at); and falling back to cover behind a shut door, the fight kept her turned
  to an imp 1100 off, the door never pressed for 6000 tics): the give-up applies in front too; while
  a route waits at a door or switch to press it and she hasn't fired this tic, the route turns her
  (`RouteFollower::pressing`, `Player::press_turn`). E4M5 0.52 -> 0.68-0.84, stalls 5 -> 0 of 16.
- **Edges a few units off are the same boundary** (MAP19 HMP: the lift out of the demon pit had no
  mesh portal: a corridor poly's edge, from a seg whose rounded split vertex is extended about
  twice its length, lay 2.1 off the lift's line against a tolerance of 2; the pit a dead end, the
  hour): `shared_segment` within 3 units. 26 of 68 maps gain 1-7 portals, none lose any.
- **A dead end's look into the unseen needs no margin; a wade from one isn't an urgent way back;
  a still mover on a hurting floor replanned at once** (E1M9 HMP: on a dry island in a nukage pit,
  the way on a room past a door the island's edge opens; "look into" it refused by the 25 margin,
  the dead-end pass lifting it only for lines to set off and skipping what was refused earlier the
  same think; waded in, "get off the floor" sent her straight back; a door re-crossed while moving
  (EV_DoDoor ignores it) waited at in the nukage): a frontier look takes margin 0 under `harm_ok`
  and the pass retries same-think refusals; wading in from a dead end (`waded_dead_end`) makes the
  way back a weighed option and counts her shut in (switches from the floor offered); a mover
  standing still with nothing coming, on a hurting floor, is replanned after half a second. The 4
  stuck HMP seeds exit.
- **A floor changed to one that hurts priced by its model; a dead end's other ways may leave her
  one hit** (E3M7 UV: at 18 health the blue key's pickup lowered the floor round its alcove into
  blood (W1 lowerAndChange copies the model sector's special: 10 a hit), priced at the worst 20,
  every way out refused): `changed_floor` keeps the floor's hit (`Navigator::floor_hit_after`); under
  `harm_ok` a way that isn't a line or a look is taken if what she'd have left covers one hit of
  its hardest floor (margin 0 for those killed her in a wade of 40 at 41).
- **Teleported away while at a step's goal: the step walked again** (MAP21: a dodge stepped her onto
  a teleporter beside the yellow bars' press spot; the follower, still in its act phase, pressed
  from 1300 off three times, and the bars were written off): moved unexpectedly while acting or
  fetching, the step starts over as a walk from where she is.
- **Unarmed, no "away" off a route that keeps clear** (MAP06 HMP: unarmed, the blue card's lift
  window missed every 3 s by "away from them" to a far spot, 20 game minutes): with no ammo
  reachable clear of them, no hitscanner after her, not hurt, and the explorer's route keeping 96
  from them all, she stays on it.
- **Walls given up one at a time; dry ones first when she's on dry floor** (E4M6: on a ledge in a
  lava field, the way out a hidden door in the ledge's own wall; the panic round's 12 walls all
  far, across lava; and every wall objective shared one identity, so one dropped "no route" benched
  them all): a wall is identified by its line; in a panic round, mortal, unsuited and on dry floor,
  walls reached without crossing a hurting floor come first.
- Not fixed: E4M1 (past the red door, "no route to the exit" even for the omniscient planner at
  21-29 health; shut in or its pricing, to trace).
- Seen: one MAP22 HMP run thought for 679 s of real time over 758 game seconds (39 tics/s): the
  planner, to trace (the stall cutoff counts game time, so it bounds this only slowly).

Score (all 68 levels, HMP+UV, 8 seeds): 0.428 -> 0.437 (HMP 0.513 -> 0.543, UV 0.343 -> 0.331),
within the score's noise (+-0.006); stalled runs 69 -> 45 of 1088; on the eight levels, stalls 31
-> 17, MAP19 HMP exits 0/8 -> 3/8, E3M7 HMP 1/8 -> 3/8.
- **The dead-end pass takes up again only what had no route** (MAP22 HMP, a run thinking 679 s of
  real time for 758 of game, 39 tics a second: the stuck round's dead-end pass took up again
  everything dropped earlier the same think, so a hunted monster dropped "not there" (she stood on
  its last-seen spot) was taken up again at once, dropped, taken up, every tic, each think routing
  every wall afresh, 140,000 wall routes): only what was given up for want of a route this think
  (the health margin's refusals) comes back (`refused_now`). The seed: 681 s -> 19 s real (it now
  dies at 719 s game instead of stalling). E1M9 HMP's stuck seeds: 2 of 4 exit, 2 stall, the same
  with or without this (an interaction of the round's merged fixes, to trace).
- **No risky way for what little it's worth** (the maintainer's: the health a way costs against what
  it's for): with nothing safe left, a place seen already looked at again, a monster gone from
  where it was last seen, and a sound (already) aren't offered as risky; they keep the tenth-of-
  health cap. Score 0.437 -> 0.448 (HMP 0.543 -> 0.537, UV 0.331 -> 0.358). Traced alongside: E1M9
  HMP seed 1105803694's health went on a revisit estimated at 2.8 that cost 28, the wait for a lift
  standing in the nukage not counted (the estimate counts the time walking on a floor, not
  waiting on it): to do.
