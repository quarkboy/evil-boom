# MBF21, UDMF and the community WADs built for them (a plan)

> **Status:** proposed, nothing built. Written 2026-11-13 from the MBF21 specification (v1.4,
> <https://github.com/dsda-org/mbf21>, now kept at doom-cross-port-collab/mbf21) and a reading of
> our map reader, specials, bestiary and engine boundary.

MBF21 is the newest step of the conservative line Doom → Boom → MBF → MBF21: everything Boom
and MBF have, plus a few map features and a much larger DeHackEd (custom monsters, weapons and
attacks without new code). Many current community WADs target it, often with UMAPINFO beside it
for their level order and names.

## 1. The short answer

**The engine already plays MBF21.** DSDA-Doom is MBF21's reference implementation (kraflab
and Xaser wrote the first one in it): an MBF21 WAD, its DeHackEd patch and its maps load and play
correctly in our build today.

**What doesn't is the bot's understanding.** She'd walk into an instant-death pit thinking it
safe, not know what most of Boom's switches and lines do, treat a custom monster as a vanilla
one with the same number (or as nothing at all if it's a new one), and go on to the wrong level
in a WAD with its own level order. "Full compatibility" means her knowledge of the map, the
monsters, the weapons and the items coming from what the engine actually loaded, not from tables
of vanilla's values.

## 2. What we have and what's missing

### The map

| | now | missing |
|---|---|---|
| Line flags | vanilla, Boom's pass-use, MBF21's block-players; the reserved flag clears the extended ones (`map.rs`: `comp_reservedlineflag`) | block-land-monsters (only for predicting monsters; small) |
| Generalized line types (Boom's 0x2F80-0x7FFF bitfields) | decoded (`specials.rs`: doors, locked doors, lifts, floors, ceilings, crushers, stairs) | |
| Boom's extended types (142-272) | only its teleports and exits; the rest are "Other" | its lifts, doors, floors, ceilings and stairs by number; and the ones that change how she gets about: friction (ice and mud), wind and current pushers and the push/pull points, floor scrollers that carry her (conveyors), deep water and other height transfers (a floor that isn't where it's drawn) |
| Sector specials | vanilla's, Boom's damage, secret and friction bits (`classify_sector`) | **MBF21's alternate damage mode (bit 12):** with it set, the damage bits mean *instant death* (unless a rad suit or invulnerability; or regardless), or *kill and exit* (normal or secret). Read now as 0 damage: a death pit looks safe, and a kill-exit isn't known as an exit at all. Bit 13 (kills grounded monsters) matters little to her |
| MBF21's scrollers (1024-1026) | | cosmetic (wall textures): nothing needed |
| The BSP (`wad.rs`: SEGS, SSECTORS, NODES) | vanilla's node format | the extended and compressed ones nodebuilders write for big maps (XNOD, ZNOD and the GL kinds in NODES; ZNODES): DSDA-Doom reads them, we'd misread them, in Doom-format WADs too |

### Things

- **Monsters** (`tactical/bestiary.rs`): a table of vanilla values by class name (radius,
  height, health, attack kind, melee, threat), and the class from the engine's type number
  (`dsda::type_name`, vanilla's 0-140 or so). DeHackEd can change every one of those values for
  a type, and DEHEXTRA / DSDHacked (with MBF21) add new types past vanilla's: those have no name,
  so she doesn't know them as monsters.
- **Attacks:** MBF21's generic codepointers (`A_MonsterProjectile`, `A_MonsterBulletAttack`,
  `A_MonsterMeleeAttack`, `A_RadiusDamage`, `A_SpawnObject`, ...) make new attacks out of
  arguments. Her fight knows vanilla's attacks by monster.
- **Monster flags she should know:** MBF's `FRIEND` (a helper dog or marine: never to be shot,
  kept out of her line of fire; not read at all now), MBF21's `BOSS`, `RIP` (a ripper missile
  goes through her and on), `NORADIUSDMG`, `LONGMELEE`, melee range, and the infighting /
  projectile / splash groups (who'll fight whom).
- **Items:** the game decides what a pickup gives by its *sprite*, not its type
  (`P_TouchSpecialThing`). Her item knowledge goes by type name, so a re-sprited or new
  pickup is misread.
- **Weapons** (`combat/arsenal.rs`): vanilla's damage, ammo and ammo per shot. DeHackEd changes
  ammo types, MBF21 adds ammo per shot and weapon flags, and new weapon codepointers
  (`A_WeaponProjectile`, `A_WeaponBulletAttack`, ...) make new weapons.

### The game

- **Level order:** `sweep::next_map` is vanilla's order (MAP15's secret exit to MAP31, and so
  on), used by continuous play and by Doom's own screens alike (`viewer.rs`, `present.rs`). A
  WAD with UMAPINFO has its own; she'd go on to the wrong level.
- **Boss deaths:** vanilla's (E1M8's barons, MAP07's 666 and 667) by map name; MBF21 sets them
  by flags, UMAPINFO by its `bossaction`.
- **The compatibility level:** DSDA-Doom takes it from a WAD's `COMPLVL` lump; a WAD without one
  needs `-complevel 21` passed. We have no option for it.
- **Map formats:** MBF21 is Doom-format maps, which we read. UDMF isn't part of it.

## 3. UDMF (and Doom-in-Hexen)

UDMF isn't part of MBF21: it's the text map format from the ZDoom family. DSDA-Doom plays it in
the `doom`, `heretic`, `hexen`, `zdoom` and its own `dsda` namespaces (`zdoom` read as the
subset `dsda` shares with it), and Doom-in-Hexen maps (the binary Hexen layout with Doom's
things), with extended nodes (XGLN, ZGLN and the rest). Its own documentation is in our checkout:
`third_party/dsda-doom/docs/udmf.md` and `things_and_specials.md`.

What that brings to a map:
- **Specials the Hexen way:** about 215 action specials with up to five arguments
  (`Door_Open(tag, speed, delay)`, `Plat_DownWaitUpStay`, `Teleport`, `Floor_RaiseByValue`, ...),
  a different numbering from Doom's line types, and how a line is triggered set apart in flags
  (`playercross`, `playeruse`, `monsteractivate`, `repeatspecial`, `impact`) with its lock in
  `locknumber`.
- **Sectors by properties:** damage as `damageamount`, `damageinterval` and `leakiness`
  instead of special bits; friction, thrust, scrolling and gravity as numbers.
- **Line flags past Doom's:** `blockplayers`, `blockeverything`, `blockuse`, `blocksight`,
  `blockhitscan`, and `midtex3d`: a walkable middle texture, a floor she could stand on that
  isn't a sector's.
- **Things with more to them:** per-skill and per-class flags, `friend`, and a special with
  arguments on the thing itself (run when it dies: the boss-death trick, generalised).
- **Polyobjects:** walls that slide and swing (doors, crushers), which nothing in our
  navigation knows of.

What DSDA-Doom doesn't have (slopes, 3D floors, portals, ZDoom's scripting and actor
languages, as far as its documentation lists) a UDMF map for GZDoom may need; those maps won't
play in our engine at all, and aren't the target. The target is UDMF made for DSDA-Doom, a small
but growing set.

Our side: `wad.rs` refuses Hexen-format and UDMF maps outright. Supporting them means a TEXTMAP
reader (or Hexen's binary layout), the extended nodes, Hexen-style specials mapped to what the
explorer knows (a door, a lift, a teleport, an exit, with the trigger from the flags), the
sector properties, and walkable midtextures and polyobjects in navigation.

**Or read the map from the engine.** The engine has already read every format, nodes and all,
into the same structures (`line_t`, `sector_t`, `side_t`, the subsectors). One boundary call
that hands over the loaded level would replace our WAD reader for every format at once: Doom,
Hexen, UDMF, and the extended nodes. It's a bigger change to the base everything stands on
(parity on the IWADs is the gate), but it's done once, where a reader per format is done three
times and kept in step with the engine forever. That's the recommendation.

## 4. The approach: ask the engine, not tables

Every one of the "things" gaps has the same fix: the engine has the loaded truth, after every
patch. New calls on the boundary:

- `evil_mobjinfo(type)`: health, radius, height, speed and fast speed, mass, flags (vanilla,
  MBF, MBF21), melee range, the three groups, and its spawn / see / melee / missile / death
  states.
- `evil_state(n)`: a state's sprite, tics, next state, codepointer (by name) and its args.
- `evil_weaponinfo(n)`: ammo type, ammo per shot, flags, its fire states.

From those, the bot works out what the tables say now:
- a monster: what it is (counts as a kill, shootable, floats, flies), how it attacks (walk its
  missile and melee states for codepointers: vanilla's by name, MBF21's generic ones by their
  args: a projectile's type, a bullet attack's damage and spread), how dangerous (damage over
  time against health, in place of the hand-set threat for anything not vanilla);
- an item: what it gives, by its sprite;
- a weapon: its ammo, cost per shot, and what it fires.

Vanilla's own types keep their names and the hand-tuned numbers (threat, the punch and saw
limits), checked against the engine's values: in an unpatched IWAD the two must agree, so the
score is unchanged (parity).

**Realism:** reading a type's stats is knowing the WAD as someone who's played it would; she
doesn't learn a custom monster's attacks by watching them. That's an omniscient shortcut by
CLAUDE.md's rule, and says so. The realistic version (an unknown monster treated as dangerous
till she's seen what it does) can come after, measured against it.

## 5. Stages, each with a gate

1. **Boom's map specials, all of them:** the extended types by number, friction, pushers,
   conveyors, deep water. Not MBF21 yet, but the base of every Boom-format WAD. Gate: the IWAD
   score unchanged; a sweep of a few Boom-format WADs finished no worse.
2. **MBF21's map bits and the game around them:** alternate damage (death pits avoided, kill
   exits known as exits), `--complevel`, the level order from the engine (UMAPINFO when there is
   one), boss deaths by flag. Gate: a small test map per feature (one per sector mode, made with
   a map editor, kept in `arenas/`); the IWADs unchanged.
3. **Monsters, weapons and items from the engine:** the three calls above, the bestiary and
   the arsenal built from them, friends never targeted. Gate: IWAD parity (every vanilla value
   read back equal to the table's); a heavily DeHackEd'd MBF WAD swept.
4. **Custom attacks:** MBF21's codepointers read for attack kind, damage and range; ripper
   missiles dodged as such. Gate: an arena of custom monsters (a DeHackEd test patch), her
   deaths and kills against vanilla ones of the same strength.
5. **The map from the engine:** the loaded level handed over by the engine in place of our WAD
   reader; then the extended nodes come free, and UDMF and Doom-in-Hexen maps load. Gate: IWAD
   parity (the same geometry and nodes as read from the WAD).
6. **UDMF's map features:** Hexen-style specials and their trigger flags, sector properties,
   walkable midtextures, thing specials; polyobjects last (rare in Doom maps). Gate: a few
   UDMF maps made for DSDA-Doom, swept.
7. **The test set:** a handful of popular community WADs per format (Boom, MBF, MBF21, UDMF), checked
   for which they are (their `COMPLVL`), swept with their own report. Not mixed into the score,
   which stays the IWADs'.

Stages 1 and 2 are the smaller part and help every community WAD; stage 3 is the largest and
the one that makes custom monsters fair game. Stage 5 could come first: it's the base the rest
reads from, and it fixes big maps' nodes in every format. TNT and Plutonia stay held out
throughout.

## 6. Open questions

- **Which WADs:** a test set chosen with the community (popular, a range of formats, a few with
  custom monsters), each checked for its format before it's counted as MBF21.
- **DSDHacked:** DSDA-Doom's unlimited types, states, sprites and sounds come with many MBF21
  WADs. The engine calls above handle it if they take any number, not vanilla's limits.
- **Unknown monsters, realistically:** how careful she is with a monster she's never seen
  attack, and whether what she learns carries across levels of a WAD.
