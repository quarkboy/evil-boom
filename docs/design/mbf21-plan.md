# MBF21, UDMF and the community WADs built for them (a plan)

> **Status:** proposed, nothing built. Third version, 2026-11-20. It replaces the note of
> 2026-11-13 (in git: `4e4b8b3`) and is now aimed at two WADs, by the maintainer's decision:
> **Sunlust** first (Boom), **Eviternity II** second (MBF21). "Any MBF21 WAD" stays the end.
>
> **Made from:** the bot's source and DSDA-Doom's at the pinned commit (`eb8eb98`), read, not
> run; the MBF21 specification v1.4 (kraflab/mbf21 `docs/spec.md`), the Boom reference v1.3
> (`boomref.txt`, the copy in Woof's `docs/`) and the UMAPINFO specification (kraflab/umapinfo
> `docs/spec.md`), fetched and read; and a census of the two WADs themselves, lump by lump
> ([mbf21-census.md](mbf21-census.md); Memento Mori, a vanilla WAD already on disk, was added to
> it as an earlier rung). **Nothing was measured or played.** Where a claim is
> still inferred or from memory it says so; section 9 lists the runs that would settle each.

MBF21 is the newest step of the conservative line Doom → Boom → MBF → MBF21: "the full spec of
boom & mbf, plus" two sector bits, two line flags, three scroller types and a much larger
DeHackEd (the spec's opening line and contents). Most of what an MBF21 map does that vanilla
doesn't is Boom's. The census bears that out: Sunlust needs none of MBF21 and little of Boom's
numbered types; what it needs is Boom's *generalized* lines and a complevel. Eviternity II
needs everything.

## 1. The goal, and what "fully compatible" means

She plays any WAD made for complevel 21 (and so 9 and 11 under it) as she plays the IWADs: by
the same rules, with no map, monster or texture named in them.

1. **It runs.** Any Doom-format map the engine loads, she loads, at the complevel it was made
   for, and goes on to the right level after.
2. **Nothing the map says is read wrong.** Every special, flag, lock and thing is understood
   as the engine has it, or known to be unknown. A wrong reading (a death pit taken for a
   floor) is worse than none.
3. **What she knows of the game comes from the game as loaded:** monsters, missiles, weapons,
   pickups, switch textures, from the engine after the WAD's patches.
4. **What has no model degrades to trying and learning**, never to a stall.

Not meant:

- **Surviving them.** Section 4 says plainly how far her fighting is from Sunlust's.
  Compatibility buys understanding; survival is the ordinary work on her play, and for most
  of Sunlust far more of it than this plan.
- **UDMF, Hexen-format maps, ZDoom features** (section 8). **ID24:** no `id24` in the pinned
  engine's source. **Demo sync, multiplayer, `-fast`.**
- **Maps that need a trick the planner can't plan:** the same limit as on the IWADs.

## 2. Where things stand

**The complevel.** `engine.rs` (`Engine::new`) passes `-iwad`, `-file`, `-skill`, `-warp`: no
`-complevel`. The engine then takes the WAD's `COMPLVL` lump if there is one
(`dsda/settings.c`, `dsda_WadCompatibilityLevel`), else its setting
`default_compatibility_level`, built in as 21 (`dsda/configuration.c:237`; `g_game.c:2757`).
No `dsda-doom.cfg` is on this machine to say otherwise. So **DOOM and DOOM II are played at
complevel 21 today** (inferred from the source; section 9 has the run that confirms it), and
so would **Sunlust be, which has no `COMPLVL` lump** and was made for 9.

*Crash* is a run that can't start; *wrong* a false belief she acts on; *blind* something
there she doesn't know of; *missed* something she could use and doesn't. The last column is
what the census found: S for Sunlust, E for Eviternity II, lines or sectors in the whole WAD.

### Loading and the game around a level

| | today | needed | gap | S / E |
|---|---|---|---|---|
| Complevel | not passed: `COMPLVL`, else 21 | the WAD's own | no option to set it; no log says it | S: no lump, wants 9. E: `mbf21` |
| PWADs | `--pwad` to `-file` (`config.rs`); each also opened as a WAD (`wad.rs` `load_level`) | `.deh`, `.bex`, zips, which the engine takes in `-file` (`d_main.c:1790`) | **crash** on a `.deh` ("is not a WAD") | neither: both carry `DEHACKED` as a lump |
| Extended nodes | `wad.rs` `load_map` leaves the BSP empty for XNOD, ZNOD, XGLN, ZGLN; `navigator.rs:290` refuses the map. `xNd4` (DeePBSP) isn't in the check: read as vanilla nodes (inferred: garbage) | the engine reads all of them (`p_setup.c:300-342`) | **crash** | S: XNOD on 2 maps of 32. E: XNOD on 30 of 37, DeePBSP on 1, ZNOD on 1 |
| Map sizes | 16-bit indices throughout, as the format | | none: E's MAP36 is at 65,535 lines and sides exactly, the format's last | |
| Map names | `engine.rs` `episode_map`: `MAPnn` any number, `ExMy` one digit each; `wad.rs` `level_names` wants five letters | UMAPINFO keeps the IWAD's shape but "the numbers x and y can exceed their original limits": MAP100, E1M10 (its spec, "MAP mapname") | small | E: MAP33-MAP37, which work |
| Level order | `sweep.rs` `next_map`: vanilla's | UMAPINFO's `next`, `nextsecret`, `endpic` and the rest (`dsda/mapinfo/u.c`) | **wrong** level next | S: vanilla's order (its `MAPINFO` is ZDoom's, which the engine doesn't read for Doom). E: five secret exits from MAP03, 09, 14, 16, 22, 29, and MAP30 to MAP37 |
| Boss deaths | `analysis.rs` `boss_rules`: by map name and vanilla thing number; Keens on any map | MBF21: by the monster's flags (the spec's `MAP07BOSS1` ... `E4M8BOSS`; `p_enemy.c` `A_BossDeath`); UMAPINFO's `bossaction`; and any thing given `A_BrainDie`, `A_KeenDie` or `A_LineEffect` by DeHackEd | **blind** | S: MAP24 and MAP30 have no exit line or floor, only boss brains: they end when one dies (`p_enemy.c:2896`). E: no `bossaction`; `BrainDie` on 2 frames, `KeenDie` on 4, `LineEffect` on 48 |
| Exits | lines 11, 51, 52, 124, 197, 198; sector special 11 (`exit_floors`) | those; MBF21's kill-and-exit floors; a brain's death | as above | S: 2 exit floors (MAP17), handled. E: no kill-and-exit floors |

### Lines (`specials.rs`, `movers.rs`, `navigator/analysis.rs`)

| | today | needed | gap | S / E |
|---|---|---|---|---|
| Vanilla 1-141 | full | | none | S: 4,156 modelled lines. E: 15,648 |
| **Generalized 0x2F80-0x7FFF** | `generalized()`: category, trigger, lock, speed. `movers.rs`: "Boom generalized movers aren't predicted" | targets, direction, change, crush, door kind and delay, lift target, stair step (`p_spec.h:107-180`, `p_genlin.c`). A push trigger works the line's back sector (`p_genlin.c:47`; boomref: "a manual linedef affects the sector on the second sidedef"), as `affected` already has it | **blind, mostly:** triggers with no effects, and what plans filters on `!t.effects.is_empty()` (`objectives.rs:116`, `:186`, `routing.rs`, `view/planning.rs:183`). A use-door she may press by its look and walk through while she sees it open | **S: 1,573 lines, on every map** (5 on MAP01 to 173 on MAP29); 4 in 5 are floors. **E: 3,254** |
| Boom's 142-272 | ten: teleports 174, 195, 207-210, 243, 244; exits 197, 198. The rest "unclassified", `Trigger::None`: not triggers (`find_triggers`) | about 120 (`p_spec.c`, `p_switch.c`) | **blind** | **S: almost none:** elevators 227 and 231 (MAP06), ceiling 200 twice (MAP24), light 157. E: floors 219-222 (187 lines), 153, 154, 159, 239, ceilings 166 and 200, a perpetual lift 181 |
| Line-to-line teleports | 243, 244 named, not in `TELEPORTERS_FOR_PLAYERS`; `find_teleports` wants a type-14 thing | "Line with same tag" (boomref 9.2: 243, 244, 262-267) | **blind:** not routes | S: none. **E: 352 for her** (244 on 286, 263 on 60, 243 on 6), 83 more for monsters only (265, 267) |
| Monsters' silent teleports 268, 269 | unclassified | for `monster_reach` only | small | S: 402. E: 654 |
| MBF21's 1024-1026 | unclassified | wall scrollers (the spec) | none | E: 27 |
| Flags | `map.rs`: vanilla's, pass-use, block-players, the reserved bit as `comp_reservedlineflag` | block-land-monsters (0x1000). The reserved rule is 21's only (`p_setup.c:1912`) | small | S: none past vanilla's. E: 0x1000 on 5,697, 0x2000 on 6,740 |
| **Pass-use** | **not handled.** `use_gates_on` (`analysis.rs:784`) ends the use trace at the first other special line, as vanilla | at any Boom complevel the trace goes on through a line flagged pass-use (`p_map.c:2771`) | missed, and on the safe side: a switch behind such a line isn't taken to be reachable from in front of it | S: none. E: 40 lines |

### Sectors (`specials.rs` `classify_sector`, `navigator.rs:314`)

| | today | needed | gap | S / E |
|---|---|---|---|---|
| Damage | vanilla's by the low five bits and Boom's bits 5-6, the larger | under 32, vanilla's; from 32 **only** the bits (`p_spec.c` `P_PlayerInCompatibleSector`) | small wrong | S: 1 sector read as 20 that's 5. E: none |
| **Instant death (bit 12)** | not read: damage 0 | "Alternate damage mode" (the spec): bits 6-5 = 00 kills without a suit or invulnerability, 01 kills, 10 kills and exits, 11 kills and takes the secret exit | **wrong, deadly.** And never learned: see below | S: none. **E: 2,305 sectors on 6 maps**, modes 0 and 1 only |
| Learning a floor | `perception.rs` `feel`: a floor is learned to hurt when she's hurt on it twice, 32 tics apart, by as much. `felt` isn't cleared by `Knowledge::restart`: **it carries across lives** | | a death floor kills at the first touch: there's no second hit, so **it's never learned, in any number of lives** | |
| Kill monsters (bit 13) | not read | | none worth having | E: 82 |
| Friction (bit 8), push (bit 9) | not read | with lines 223 and 224-226 | see movement | S: 23 friction (MAP07). E: 233 friction, 149 push |

### Movement and movers she models herself

| | today | Boom/MBF21 | how bad | S / E |
|---|---|---|---|---|
| Friction | `jumps/physics.rs`: one `FRICTION`, one `MOVE_FACTOR` | per sector, by line 223's length (boomref: "Sludge<100, Ice>100") | jumps wrong on ice; stopping distance off | S: 8 lines, MAP07. E: 9 lines, 5 maps |
| Floors that carry (252, 253, 247, 248, 216, 217), wind and pushers (224-226) | none | a steady shove | **Walking, by the code:** the follower steers from where she is each tic; pushed 96 off her path for 10 tics she replans (`follower.rs:138`, `walking.rs:109`); no progress for 35 tics, she backs off (`walking.rs:62`); after 6 replans of a step or 30 of a route it's "replanned too often" and the route is dropped (`follower.rs:142`, `:151`). So a push weaker than her walk is absorbed; one stronger ends the route, and nothing tells the explorer why: it may choose it again. A stall risk, not measured | S: 811 carried sectors, most under dolls and in monster closets; 65 large empty ones may be walked (not told apart). **E: she starts on one in MAP14, MAP31, MAP33**; 8 wind lines |
| Voodoo-doll machinery | the last player-1 start is hers (`map.rs`); a doll is dropped as `DoomPlayer` (`engine.rs` `observe`) | a doll carried over lines: timed and chained events | the world changes from causes she can't see. What she sees move she uses (`seen_heights`); `saw_move` only credits a line whose table effect fits. Waits can be long | **S: 31 of 32 maps** (2 to 36 dolls). E: all 37 |
| Teleports she doesn't plan | a jump of more than 64 units in a tic is a teleport (`model.rs:44`, `perception.rs:134`; `follower.rs:1034`). Knowledge credits the nearest `Teleport` line within 48 and learns where it went; the follower, off its plan, logs "moved unexpectedly" and replans | silent ones are noticed the same way: nothing depends on the fog or the sound | **works, with two holes:** a line-to-line teleport that moves her less than 64 units isn't noticed at all; and one of 262-267 isn't a `Teleport` line to her, so it's "moved all at once" and nothing learned. For 243 and 244 the one learned point stands for a whole line of exits | S: 208 on 65 lines, planned already. E: as above |
| Deep water, fake floors (242) | none | drawing only: "Things will stand on the actual floor or hang from the actual ceiling, while this function provides another rendered floor and ceiling" (boomref, 242) | movement right. What she *sees* is wrong: `sight.rs` and a floor's look go by the real sector | S: 354 lines, 26 maps. E: 1,622 |
| Elevators (227-232) | none | | blind | S: 2 lines (MAP06) |
| Sky, light, translucency | none | drawing only | none | |
| Crushers | `crusher_kind` has the generalized ones | | none | |

### Things

| | today | a WAD can | gap | S / E |
|---|---|---|---|---|
| What a thing is | the engine's type number through `dsda::type_name` (vanilla's); `observe` drops a type without a name | change any type; add types (DEHEXTRA, DSDHacked) | **blind:** a new monster isn't in her world | S: none new. **E: 230 new types, 27 of them monsters**: a tenth to a half of most maps' monsters |
| A type remade | fought as the vanilla one of that number; flags unread but `SHOOTABLE` and `CORPSE` | | **wrong** | **S: the Commander Keen** is `Bits = 0`, 10,000 health, running `A_VileChase`: an unshootable thing that raises the dead. She'd take it for a Keen and a target (inferred from `observe` and `bestiary.rs`; 56 of them, MAP30 only). E: lost soul 45 health, cyberdemon's groups |
| Monsters, attacks, missiles | `bestiary.rs` by name; names compared as strings in about a hundred places | MBF21's fields and codepointers (the spec: thing flags, groups, fast speed, melee range; `A_MonsterProjectile` and the rest) | wrong or blind | S: nothing. E: `MonsterProjectile` on 1,956 frames, `SpawnObject` 2,756, `SeekTracer` 53, `HealChase` 16, `RadiusDamage` 14; `MBF21 Bits` on 32 things |
| Friends (MBF) | flags come over, unread | thing flag 0x80, `MF_FRIEND` | **wrong:** she shoots them | E: 5 things |
| Weapons | `bestiary.rs` `weapon`, by slot | remake them; MBF21's ammo per shot, flags, weapon codepointers | **wrong** | S: none. E: the pistol remade; the chainsaw now a "Perforator", bullets, 25 a shot |
| Pickups | by type name and editor number | the game goes by sprite (`p_inter.c:495`) | wrong, blind | neither adds a pickup |
| Solid decorations | `things.rs` `solid_radius`, vanilla's | new ones | bumps and replans, as today | E: 83 new solid kinds |
| The Icon of Sin | the mode's map analysis starts from the first thing 88 (`icon/lanes.rs:298`); **the mode itself only starts on hearing `dsbossit`** (`player/boss.rs:249`), the sound a boss shooter makes on waking (`p_enemy.c:2857`), and only with rockets | brains as scripted exits | **can't misfire without a shooter on the map.** With one, and a decoy brain earlier in the things, its lanes aim at the decoy | S: 9 brains on 6 maps, **no shooter (87, 89) anywhere**: the mode never starts. E: no brain things |
| The game's messages | `knowledge/model.rs` `needs_key` looks for "need" with a key word, or "will open" | BEX strings | missed: a locked door not learned as locked | **E: "Blue keycard required."**: no "need" in any of its lock messages |

### By its look

| | today | a WAD can | gap | S / E |
|---|---|---|---|---|
| Switches, doors, lifts | `LOOKS_INTERACTIVE`: eleven name prefixes. With perception on, a use line is tried only if it looks the part, is door-shaped, a lift's face, in the way or known locked (`model.rs` `interactables`) | its own textures, and a `SWITCHES` lump saying which are switches (`p_switch.c:112`) | **missed** | S: its `SWITCHES` are all `SW1`/`SW2` but one; still 206 of 811 switch lines are on a face with no known prefix (lift sides, plain walls). **E: 0 of 1,018 switch lines and 0 of 231 use-doors** have a prefix she knows; 30 of its 70 switch pairs aren't `SW...` |
| Floors that hurt | `HAZARD_FLATS`: seven names; then what she's felt | own flats | learned by standing on it | S: 1,721 of its 2,313 hurting sectors are `OSLIME01`, unknown to her. E: none of its liquids is known |

### The engine boundary (`dsda/evil_api.c`, `dsda.rs`)

Given now: the player, every mobj (type, place, speed, radius, height, health, `flags`, state,
owner, attack tics), sectors' heights, the tally, sounds and each type's sounds, hits, the
screens. Wanted by the stages below: the complevel; `mobjinfo`, states with codepointers and
arguments, `weaponinfo`, ammo maxima and Misc; a mobj's `flags2`; the switch texture list; the
next map; what ends the level on a death; the loaded level's counts, for checking ours.

## 3. The two targets

Full tables: [mbf21-census.md](mbf21-census.md).

| | Sunlust | Eviternity II |
|---|---|---|
| Complevel | 9, by its text file ("Boom-compatible"); **no `COMPLVL` lump** | 21: `COMPLVL` = `mbf21` |
| Nodes | vanilla on 30 maps; XNOD on MAP28, MAP30 | vanilla on 5; XNOD on 30; DeePBSP on MAP24; ZNOD on MAP36 |
| Largest map | 34,605 lines, 64,914 sides | 65,535 lines, 65,535 sides |
| Lines she models | 4,156 | 15,648 |
| Generalized (named, no heights) | 1,573, every map | 3,254, every map |
| Other triggers not modelled | 4 lines on 2 maps | 236 Boom floors, ceilings and a lift; 352 line-to-line teleports |
| MBF21 sector bits | none | 2,305 death floors; 82 kill-monster |
| New line flags | none | block land monsters 5,697; block players 6,740; pass-use 40 |
| Things | vanilla's; the Keen remade (MAP30) | 230 new types, 27 of them monsters; 2 weapons remade |
| Level order, exits | vanilla's; MAP24, MAP30 end on a brain's death | UMAPINFO, five secret exits; bosses end levels by codepointer |
| Switch faces she'd know | 605 of 811 | 0 of 1,018 |
| Dolls | 31 of 32 maps | all 37 |
| Friction, wind | MAP07 | 5 maps; she starts on a conveyor in 3 |

**What Sunlust needs of this plan:** the complevel (stage 1); generalized movers (stage 2),
without which no map of it is understood; open learning (stage 3), because 31 maps script
with dolls; XNOD for two maps (stage 4); and for the last few maps, things by their flags and
a brain's death as an exit (stage 5). It needs **nothing** of: MBF21's sector bits, the Boom
table beyond two elevators and a ceiling, line-to-line teleports, UMAPINFO, monsters and
weapons from the engine, MBF21's codepointers, the switch list. Marking ice (MAP07) and
floors that carry is wanted, not urgent.

**The smallest work that gets her through:**

- *Sunlust MAP01 to MAP05, as maps:* `--complevel 9` and generalized floors, ceilings, lifts
  and doors (5, 38, 24, 97 and 29 lines on those five). Nothing else on them is unmodelled but
  drawing (242, sky, light, scrolling walls) and the dolls of MAP02 to MAP05 (9, 2, 22, 13),
  whose effects she can only see happen. Two stages. Whether that's *enough* isn't known till
  she's watched there: the scripted doors are the likely trouble, and stage 3 is the answer.
- *All of Sunlust, as maps:* add XNOD (MAP28, MAP30), the elevators (MAP06) and ceiling 200
  (MAP24), a brain's death as the exit (MAP24, MAP30), the remade Keen (MAP30), ice (MAP07).
- *Eviternity II:* all of the above, and DeePBSP and ZNOD; death floors; the Boom table and
  line-to-line teleports; the switch list and the lock messages (without which she'd press
  almost nothing there); UMAPINFO's order; unknown monsters seen at all, then the bestiary
  from the engine and MBF21's attacks; floors that carry her. That is the whole plan.

## 4. Difficulty, honestly

Understanding a map and surviving it are separate, and for Sunlust the second is the wall.
Its text file: "UV is designed primarily for ubermensch doom-gods, thus we encourage most
players to start off on HMP or lower."

Her fighting as it stands, from the baseline's three-lives rows (`out/lives/base.rows`, read
as they lay: 4 seeds, three lives): on DOOM II she exits 188 of 272 runs on HMP and 110 of 272
on UV. On UV she exits no run of MAP06 to MAP13 or MAP16 to MAP24. On HMP the maps she never
exits are MAP08, 13, 16, 17, 20, 23, 24, 27, 29: roughly, anything past 15,000 points of
monster health, or with several arch-viles or a cyberdemon.

Against that, monsters and their summed health by skill (easy / HMP / UV):

| | monsters | health | arch-viles | revenants | cyberdemons |
|---|---|---|---|---|---|
| DOOM II, its hardest by health (MAP08) | 66 / 111 / 144 | 27,610 / 33,170 / 40,550 | 0 | 0 | 1 |
| DOOM II, a typical late map (MAP24) | 51 / 108 / 167 | 7,270 / 15,920 / 23,110 | 0 | 0 | 0 |
| Sunlust MAP01 | 69 / 78 / 80 | 5,870 / 7,410 / 8,430 | 0 / 0 / 1 | 0 / 2 / 2 | 0 |
| Sunlust MAP02 | 71 / 81 / 95 | 6,980 / 10,220 / 14,110 | 2 / 3 / 4 | 2 / 1 / 3 | 0 |
| Sunlust MAP03 | 108 / 122 / 152 | 13,370 / 17,070 / 22,030 | 0 / 0 / 2 | 5 / 6 / 15 | 0 |
| Sunlust MAP04 | 84 / 97 / 108 | 16,250 / 20,350 / 22,530 | 0 / 1 / 3 | 15 / 19 / 27 | 0 |
| Sunlust MAP05 | 130 / 138 / 149 | 10,640 / 14,990 / 21,940 | 2 / 3 / 3 | 4 / 4 / 12 | 0 |
| Sunlust MAP08 | 125 / 145 / 162 | 29,110 / 40,340 / 55,180 | 2 / 4 / 5 | 12 / 17 / 14 | 1 / 2 / 3 |
| Sunlust MAP15 | 224 / 272 / 319 | 52,420 / 72,920 / 105,320 | 1 / 2 / 5 | 33 / 44 / 79 | 2 / 3 / 5 |
| Sunlust MAP20 | 438 / 461 / 544 | 158,470 / 168,070 / 213,940 | 12 / 12 / 26 | 77 / 85 / 91 | 5 / 5 / 11 |
| Sunlust MAP30 | 1,829 / 1,981 / 2,187 | 655,520 / 758,380 / 856,960 | 40 / 52 / 96 | 546 / 561 / 606 | 24 / 26 / 26 |

Three things the numbers don't show and that go against her: Sunlust's skills differ little
(MAP01 has 69 monsters on easy, 80 on UV: easy thins the worst, not the crowd); its fights
are locked rooms and ambushes with no way back, where her retreats have nowhere to go; and
it's balanced to the ammo, pistol start.

My read, to be tested, not trusted:

- **Plausible on easy, perhaps HMP:** MAP01. By the numbers it's a DOOM II mid-map.
- **A reach on easy:** MAP02, MAP03, MAP05, MAP06, MAP31, MAP23. Health in DOOM II's late
  range, but with two to five arch-viles even on easy, in ambushes.
- **Not as she fights now, on any skill:** MAP04 (15 revenants on easy), MAP07 to MAP19, 21,
  22, 25 to 27, 32: past her DOOM II ceiling in health, with cyberdemons and arch-viles in
  number. MAP32 is 67 monsters on easy, 12 of them arch-viles.
- **Out of the question:** MAP20, 24, 28, 29, 30: hundreds to thousands of monsters. It
  happens that the maps needing the oddest work (the brain exits, the remade Keen, XNOD) are
  these, so that work buys understanding only.

So **compatibility alone buys this on Sunlust:** every map loaded and explored as a map (with
`--fight 1 --charmed`, or directed), switches and lifts understood, MAP01 very likely
finished on the easier skills, and a true picture of where she dies on the rest. It does not
buy a Sunlust playthrough. Eviternity II is the gentler of the two by a long way: MAP02 to
MAP10 run 5,700 to 46,000 health on HMP with three arch-viles at most, nearer DOOM II's
second half, and once she understands it (its own monsters included: she can't see them
before stage 5), it's the better place to watch her play.

## 5. The decisions

### A. Where game data comes from

Parse DEHACKED, BEX, DEHEXTRA, DSDHacked and MBF21's fields in Rust (a second reader to keep
in step with `d_deh.c`'s 3,500 lines), or **ask the engine after it has loaded everything**.
The second, as the first version had it, and Eviternity II settles any doubt: 9,587 frames
and things numbered to 2,005 are DSDHacked's ranges. `evil_mobjinfo(type)`, `evil_state(n)`
(sprite, tics, next, the codepointer's name, its arguments), `evil_weaponinfo(n)`,
`evil_game_info()` (complevel, ammo maxima, Misc), read once a process, as `SoundBook`
already reads sounds. The planner's editor-number tables (`things.rs`) are filled from the
same when a game is up.

**Identity:** a thing is its type number; what she knows of it is that type's entry. The
ZDoom class names stay as the names of vanilla's types *while the entry is vanilla's* (on an
unpatched IWAD every entry read back equals the table's: the parity gate). Sunlust's Keen is
the case in point: type 24, no longer a Keen.

### B. Where the map comes from

Rust readers for the node formats, or the level handed over by the engine. The first version
recommended the engine. **For these targets, Rust readers:** XNOD is vanilla's layout with
wider fields and fractional vertices; DeePBSP the same idea; ZNOD is XNOD deflated (an
inflate: a dependency or sixty lines). The census found no GL nodes. The engine route stays
for UDMF (section 8). Add one cheap call now, `evil_level_counts()` (the loaded level's
counts and a checksum of line flags and specials), checked every run.

### C. A special or a thing with no model

**The knowledge layer can't learn an effect it has no table entry for.** `saw_move` and
`heard_move` (`knowledge/model/learning.rs`) require `t.effect(s).is_some()`; `learned` holds
sector numbers, not heights; and a line the table doesn't know isn't a trigger
(`find_triggers`). What exists is that she plans on heights she's *seen* (`explorer/world.rs`).

Three layers, each the fallback of the one before:

1. **The table is the game's rules.** Generalized lines and Boom's types go in as vanilla's
   are. What a *particular* switch does is still learned by pressing it.
2. **Open learning, new.** Any line she can set off is a trigger. With no table effect, what
   she sees or hears start moving right after is put down to it, with the heights it came to
   rest at. And **the same for what moves with no line of hers behind it:** a sector seen to
   move by a doll's doing is remembered as one that moves, with where it went, so a door that
   opened on a timer in one life is expected in the next. With dolls on 31 of Sunlust's maps
   this is no longer the fallback for exotica: it's how those maps work.
3. **A thing with no entry she can read is never dropped:** an unknown shootable,
   kill-counted thing is a monster of unknown kind; an unshootable one is never a target; an
   unknown missile is dodged as the widest and fastest she knows.

For monsters and weapons: work the entry out of the engine's tables (knowing the WAD as
someone who's played it: an omniscient shortcut, said so), or start every unvanilla type as
"unknown, dangerous" and learn it by what it's seen to do. **The first, then the second
measured against it.** Sunlust doesn't need either; Eviternity II needs the first to be
survivable.

A death floor is the sharpest case of learning: decision C applied to floors. A death with no
hit from a monster, on a floor never stood on before, is that floor's doing, and every floor
of its look with it. That makes the realistic route work across lives; reading bit 12 from the
map is the planner's omniscient one. Both: the planner never routes over one, and she can say
how she knows.

### D. Complevel policy

Let the engine choose, and **say it:** the log's first lines carry the level in force, and
`--complevel N` sets it for a WAD with no lump (Sunlust: 9). The bot's own reading keys on it
where the rules differ: the reserved line flag and sector bits 12-13 (21 only), pass-use,
generalized lines and sectors (9 up). Whether the IWADs should move to 2 and 3 is section 10's
first question.

### E. The game around a level: ask, don't table

`sweep::next_map` and `boss_rules` become boundary calls: `evil_next_map(secret)` and
`evil_level_enders()`: the things whose death ends or changes the level (UMAPINFO's actions;
MBF21's boss flags; and, read from the states, any type whose death frames run `A_BrainDie`,
`A_KeenDie`, `A_BossDeath` or `A_LineEffect`). Knowledge of the level before it's played, as
`boss_rules` is now.

### F. Ice, wind and conveyors: model or mark

Carry Boom's friction, pushers and scroll carrying exactly in the simulator and the follower;
or **mark the sectors** at load (no planned jump takes off from, crosses or lands on one;
routes cost more there; a carry faster than her walk is one-way; and a route dropped for
"replanned too often" on a marked floor is remembered as against the current, not tried
again). **Mark first.** Sunlust has ice on one map. Eviternity II starts her on a conveyor
three times: watch those before deciding on the exact model.

## 6. Stages

Ordered for Sunlust, then Eviternity II. IWAD gates are `tools/parity.sh` where nothing should
change, the three-lives test where play may. Small is a day or two, medium a week, large more.

| # | stage | size | serves | gate |
|---|---|---|---|---|
| 0 | **The census as a command** (`evil-boom census --pwad ...`): what section 3 was made with, kept, so every later stage is counted down on real WADs. No play changes | small | both | its tables match mbf21-census.md |
| 1 | **The complevel:** `--complevel N`; `evil_game_info()`; the level in force in every log; `evil_level_counts()` checked each run | small | **Sunlust, all of it** | Sunlust MAP01 starts at 9 and says so; IWAD parity |
| 2 | **Generalized movers:** floors, ceilings, lifts, doors, locked doors, stairs and crushers get their targets, timing and change in `movers.rs`, from `p_genlin.c` | medium | **Sunlust: every map** (1,573 lines). E: every map (3,254) | a test map per family; a unit test over every generalized field value against a table written from `p_genlin.c`; IWAD parity (the IWADs have none); Sunlust MAP01-05 charmed (`run-map --fight 1 --charmed`): exits |
| 3 | **Open learning** (decision C2): effect-less lines as triggers; learned heights; sectors that move unprompted remembered across lives | medium | **Sunlust: 31 maps** with dolls. E: all 37 | a doll-timer test map: she waits or returns, and the next life goes straight there; the three-lives test on the IWADs no worse |
| 4 | **XNOD** in `wad.rs` | small | Sunlust MAP28, MAP30. **E: 30 maps** | the same test map built both ways gives the same mesh; those maps load |
| 5 | **Things by what they are now:** flags read (`SHOOTABLE`, `COUNTKILL`, `MISSILE`, `FRIEND`, `flags2`): an unshootable thing is no target, a friend never; a type with no name comes through as unknown monster, missile or obstacle; a vanilla name only while the type's entry is vanilla's (`evil_mobjinfo`, first use). **A death that ends the level** (`evil_level_enders`) as an exit objective | medium | Sunlust MAP24, MAP30 (brains, Keens). **E: every map** (its monsters seen at all) | IWAD parity; an arena with a remade Keen and a brain to kill |
| 6 | **Marking ice and floors that carry** (decision F); elevators 227-232 and ceilings 199-206 in the table | small | Sunlust MAP06, 07, 24. E: 5 maps | test maps; parity |
| | *Sunlust is understood here. What's left of it is fighting (section 4).* | | | |
| 7 | **MBF21's sector bits:** death floors walls to walking, jumping, retreating, dodging (mode 0 open under a suit or invulnerability); learned from a death (decision C); `classify_sector` by complevel; block-land-monsters | small | **E: 6 maps**, 2,305 sectors | test maps; IWAD parity (no IWAD sector has the bits) |
| 8 | **By its look:** the switch list from the engine in place of `SW1`/`SW2` prefixes; locked doors from the engine, not the message's English; DeePBSP and ZNOD | small to medium | **E: every switch it has**; MAP24, MAP36. S: little (its switches are `SW...`) | test maps; the three-lives test (switch recognition changes play) |
| 9 | **Boom's numbered types** (142-272) in the table and `movers.rs`; line-to-line and reversed teleports as routes, and noticed when short; pass-use in the use reach; monsters' silent teleports in `monster_reach` | medium | E: about 670 lines. S: none | a test map per family; a unit test over every number `p_spec.c` and `p_switch.c` handle; parity |
| 10 | **The level order** from the engine (`evil_next_map`) | small | E | an episode with a secret level played through in order; parity |
| 11 | **The bestiary from the engine:** `Bestiary` built from `mobjinfo` and states; ammo, health and weapon numbers from the game; pickups by sprite | large | E | on the IWADs every derived value equals today's table; parity; an arena of a plain-DeHackEd WAD's monsters |
| 12 | **Attacks and weapons from states:** vanilla codepointers by name, MBF21's by their arguments; threat from damage over time against health; rippers, seekers, groups; remade weapons | large | E (`MonsterProjectile` on 1,956 frames) | arenas of Eviternity II's own monsters by type (`evil_spawn`), her deaths against vanilla ones of like strength |
| 13 | **Floors that carry her, wind:** as decided after watching E's MAP14, MAP31, MAP33 | medium | E | those maps' starts |
| 14 | **The standing test** (section 7); the census at zero for both | small | all | |

Before any of them, Memento Mori (section 7) needs none and is the place to start watching.
Stages 1 and 2 are the first week and put her on Sunlust MAP01. Stage 3 is the one whose size
I trust least: it changes what the knowledge model holds. 11 and 12 are independent of 7 to
10. What moved against the second version: the Boom table fell from the largest early stage
to a middling late one (Sunlust uses four of its lines); generalized movers and open learning
came to the front; the switch list rose (Eviternity II can't be played without it). TNT and
Plutonia stay held out throughout.

**What each stage touches that it shouldn't break:** `specials.rs` is used by 36 files (keep
to the existing `Category`s where the meaning holds); monster and missile names are compared
as strings in about a hundred places (stage 5 and 11 don't rewrite them: a vanilla type still
vanilla keeps its name; a rule moves to a property when a trace asks for it); the nav cache's
key takes a version with stages 2 and 4; an unknown monster needs a name she can be told,
short ("something new": a new type's name isn't in DeHackEd).

## 7. The test plan

**Per feature: small maps, ours, in the repository** (`tests/wads/`, a few sectors each, a
line in a README saying what must happen), run as integration tests that skip without the
engine: each generalized family; a doll-timer door; an elevator; ice with a ledge; a conveyor
with and against; the same map with vanilla, XNOD, ZNOD and DeePBSP nodes; each death-floor
mode; a pass-use switch; each teleport kind, a short line-to-line one among them; a switch on
an unusual texture with a `SWITCHES` lump; a UMAPINFO order with a secret level. And a
DeHackEd test patch: a remade Keen, a brain that ends the level, a changed monster, a new
one, an MBF21 projectile and bullet attacker, a ripper, a friend, a remade weapon.

**Per WAD: the census**, before and after each stage.

**Memento Mori first: the rung before Sunlust.** A 1996 megawad already in `wads/mm_allup/`,
and by its lumps a vanilla one (the census, last section): every line, sector and thing is
one she models; vanilla nodes; no DEHACKED; ordinary exits but MAP30, a real Icon of Sin.
**It needs no stage of this plan** and, in my judgement from the files, runs today (USAGE.md
already uses it as the `--pwad` example). So it's the control for everything above: a
community WAD where a failure is her play or an old trick, not a format. What it does have:
no `COMPLVL`, so it runs at 21 where it wants 2, as DOOM II does; 7 sectors that shut at 30
seconds (special 10, MAP17 and MAP25), which nothing of hers models; one doll (MAP13); deaf
monsters and teleport ambushes as the rule; co-op-sized fights ("Most levels were made for
co-op play", its text file). By section 4's measure, on HMP: MAP01 to MAP06, MAP08, MAP10 and
MAP11 are within what she exits on DOOM II (1,700 to 13,500 health, no arch-vile); MAP07,
12, 13, 17, 19, 20, 22, 25, 27 and 32 are a reach; MAP09, 14 to 16, 18, 21, 23, 24, 26 and
28 to 31 are past her (MAP23: 302 monsters, 28 arch-viles, on every skill). Its rungs: charmed
on every map; then three lives on HMP, MAP01 to MAP08.

**Sunlust, in three rungs,** each its own report, never mixed into the IWADs' numbers:

1. *As a map:* `run-map --fight 1 --charmed` on every map: does she get out? This is the
   compatibility measure, and by the maintainer's own test for whether a level can be got
   through. Refusals, stalls and what she didn't understand are the findings.
2. *Three lives on the easiest skill,* MAP01 to MAP06 and MAP31: exits, and what killed her.
3. *Three lives on HMP,* the same maps, once rung 2 has exits.

**Eviternity II** the same way, MAP02 to MAP09 first. Each rung is 8 levels or fewer a test.

Other WADs, for "any MBF21 WAD", each checked on download (its `COMPLVL`, its text file); the
names are from memory: Judgment, Ad Mortem, Going Down Turbo (MBF21, fairly sure); Eviternity,
Valiant (MBF, heavy DeHackEd); Ancient Aliens, Scythe 2, Going Down (Boom).

## 8. After MBF21: UDMF and Doom-in-Hexen

(From the first version.) UDMF isn't part of MBF21. DSDA-Doom plays it in the `doom`,
`heretic`, `hexen`, `zdoom` and `dsda` namespaces, and Doom-in-Hexen maps; its documentation
is in the checkout (`third_party/dsda-doom/docs/udmf.md`, `things_and_specials.md`,
`doom_in_hexen.md`). It brings Hexen-style specials with arguments and triggers as flags;
sector damage, friction and gravity as properties; walkable middle textures; specials on
things; polyobjects. `wad.rs` refuses both formats. Supporting them is decision B's other
option (the level from the engine), then the specials mapped onto the categories the explorer
knows. Stage 3's open learning is what makes an unmapped special there survivable.

## 9. Runs that would settle what reading couldn't

Nothing here was run. Each of these is one command and what to look for.

1. **The complevel in force.** The stock engine with a scratch settings file (so nothing is
   written where the bot reads): `third_party/dsda-doom/prboom2/build/dsda-doom -iwad
   <doom2.wad> -config <a new file outside the repository> -warp 1`, then type `tntcomp` in
   the level. Expect "Complevel: 21 - MBF21" (`m_cheat.c:560`). Then again with `-file
   wads/sunlust/sunlust/sunlust.wad`: expect 21 too, which is the fault stage 1 fixes. (Don't
   give it the bot's own data folder, `out/dsda`: on quitting it would save a settings file
   there, and the bot's engine would read it from then on.)
2. **Memento Mori, which should simply work.** `evil-boom run-map doom2 MAP01 --pwad
   wads/mm_allup/MM.WAD --fight 1 --charmed`, and the same with `MAP17` (a door that shuts at
   30 seconds). Expect a start and an exit; in the log, "unclassified special" nowhere. Then
   mortal: `--fight 3 --mortal --lives 3`.
2a. **Does she load Sunlust today.** `evil-boom run-map doom2 MAP01 --pwad
   wads/sunlust/sunlust/sunlust.wad --fight 1 --charmed`. Expect it to start. In the log:
   the generalized lines among untried or effect-less triggers, and where she stops. And
   `MAP28`: expect "no usable BSP nodes".
3. **A teleport she didn't plan.** The same on Eviternity II's MAP03 (244 on 2 lines) or
   MAP35 (117): look for "moved unexpectedly" and "moved to (x, y) all at once" in the log,
   and whether she recovers. Needs stage 4 first: both maps are XNOD.
4. **Against a conveyor.** Eviternity II MAP14 from its start (she starts on one): look for
   "off course" and "replanned too often". Also needs XNOD.
5. **The remade Keen.** Sunlust MAP30 charmed, once XNOD is in: does she stand firing at
   Keens.
6. **The Icon of Sin mode** needs no run: with no boss shooter in either WAD it can't start.

## 10. Open questions for the maintainer

1. **The IWADs at complevel 21.** Meant, or an accident of the default? Staying keeps every
   baseline; 2 and 3 are truer and change physics and monster AI under every tuned number.
2. **What counts as "plays Sunlust".** Charmed exits on every map (reachable with this plan),
   or mortal exits (MAP01, perhaps a handful more on easy)? It sets where this plan stops and
   the fighting work starts.
3. **Unknown monsters: read or learned** (decision C). Not needed for Sunlust.
4. **Open learning's reach** (stage 3): remembering that a sector moved by itself, across
   lives, is new knowledge of a kind she doesn't keep now. Within the realism rule as I read
   it (she saw it), but it's the maintainer's rule.
5. **Test maps in the repository:** small binary WADs of ours under `tests/wads/`, with the
   editor's sources? Or generated, which needs a node builder?
6. **The engine pin:** ID24 and later fixes need a newer DSDA-Doom; before or after this?
