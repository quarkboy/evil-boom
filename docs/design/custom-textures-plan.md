# Switches, doors and lifts in a WAD's own textures (a plan)

> **Status:** proposed, nothing built. Written 2026-11-20 from a reading of
> `knowledge/model.rs` (`LOOKS_INTERACTIVE`, `interactables`, `needs_key`),
> `knowledge/model/walls.rs` (`looks_interactive`, `lift_face`, `door_shaped`, `wall_signs`),
> the engine's `p_switch.c`, and read-only tallies over five WADs' lumps: DOOM, DOOM II,
> Memento Mori, Sunlust and Eviternity II. **Nothing was built, run or played:** the numbers
> are counts of lines, not of exits. Belongs with [mbf21-plan.md](mbf21-plan.md) (its stage 8)
> but holds for any WAD with textures of its own.

## 1. The problem

She takes a line for something to try partly by its look: `LOOKS_INTERACTIVE`, eleven
texture-name prefixes from DOOM and DOOM II (`SW1`, `SW2`, `DOOR`, `BIGDOOR`, `SPCDOOR`,
`EXITDOOR`, `ZDOOR`, `ICKDOOR`, `ZELDOOR`, `PLAT`, `SUPPORT`). A WAD with other names gets
nothing from it. Eviternity II's 550 switch lines and 231 use-doors: none recognised.

**What the look is used for today, said plainly.** `interactables` (`model.rs:577`) only
ever offers lines that have a special (`self.info.get(&li)`), and of those, with perception
on, the ones that look the part, are door-shaped, are a lift's face, are in her way or are
known locked. So a wall that merely looks like a switch is never pressed, and a real switch
that doesn't look like one isn't offered. The look is a **gate on real triggers**, not a
belief about walls. That is already more than a player knows, and this plan doesn't change
it: it replaces what the gate goes by.

**The scope, by the maintainer's decision.** Telling a switch from a wall by its picture is
something any player simply knows, as they know what an imp looks like. The bot isn't meant
to work it out or learn it. For DOOM and DOOM II that knowledge is the hand-written list. For
any other WAD it's the same list made the same way: by going over the WAD once.

## 2. The design: a list per WAD, tallied once

On first loading a WAD, go over every map of it once and count, for each texture, the line
faces it's on and how many of those belong to a line that's really used: a switch, a use-door,
a mover worked from its own face, a gun target. A texture used that way often enough is on
that WAD's list. The list is kept in a file and read from then on, as `LOOKS_INTERACTIVE` is
compiled in for the IWADs.

**Realism:** this is knowledge of the texture set, not of any line: the standing of someone
who's played with these textures before. It's **an omniscient shortcut of the same kind and
degree as the hand-written list, and no more**: that list, too, was made by knowing which of
DOOM's textures the mappers put on switches and doors. Section 5 says where the tally would
go past that (a texture used once), and the rubric stops short of it.

### What's tallied, exactly

For each line with a front side, **the textures its front shows**, by the map's heights as it
starts:

- a one-sided line: its middle;
- a two-sided line: its **lower** if the back floor is above the front floor, its **upper**
  if the back ceiling is below the front ceiling, its **middle** if it has one.

Each such texture gets one face counted. If the line is used from that side it gets one real
use too, of a kind, by the line's trigger (`specials.rs`: `Trigger::used()`, `shot()`):

| kind | lines | which slot shows, in DOOM and DOOM II (1,039 doors, 443 switches, 717 own faces) |
|---|---|---|
| switch | S1, SR (generalized: trigger bits 2, 3) acting on other sectors by tag; exits | middle 332, lower 62, upper 39 |
| door | D1, DR (generalized: bits 6, 7, the push kinds, whatever they move) | the upper, 1,033 of 1,039: the door sector's own line seen from outside, shut |
| a mover's own face | S1, SR whose back sector carries the line's tag: a lift called from its side, a door with a switch special on itself | lower 538 (a lift up), upper 227 (a door) |
| gun | G1, GR | middle and upper |

Not counted as real: walk-over lines, a door or lift worked from elsewhere, anything she
can't set off by use or a shot. `looks_interactive` today tests all three of a front's
textures whether they show or not; the list should be tested the way it's tallied, on what
shows. A line whose special the table doesn't know yet (Boom's numbered types, before
mbf21-plan's stage 9) isn't a trigger to her and isn't tallied.

### The rubric

A texture is listed when **it's on at least 2 real lines and at least a tenth of its faces
are real**, the kinds pooled; **or the WAD declares it a switch** (below). Below, what each
form of the rule would recognise: real lines with a listed texture showing, of all there are
(lines that show no texture at all are in the totals: Eviternity II has 59 such switches);
and the plain faces that carry a listed texture, with how many of those are walls (one-sided,
or no way through).

**DOOM and DOOM II together** (68 maps; 52,455 plain faces)

| list | textures | switches (443) | doors (1,039) | own faces (717) | plain faces listed | of them walls |
|---|---|---|---|---|---|---|
| the hand-written prefixes (today) | 84 | 386 | 646 | 372 | 6,638 | 6,085 |
| declared switches only | 54 | 382 | 36 | 85 | 236 | 230 |
| 2+ uses, half its faces | 64 | 366 | 531 | 165 | 246 | 229 |
| 2+ uses, a quarter | 75 | 366 | 642 | 194 | 533 | 501 |
| 2+ uses, a quarter, or declared | 87 | 386 | 647 | 196 | 687 | 650 |
| **2+ uses, a tenth, or declared** | 107 | **390** | **749** | **406** | 2,261 | 2,099 |
| 2+ uses, a twentieth, or declared | 129 | 392 | 861 | 600 | 7,825 | 6,603 |
| 1+ use, a tenth, or declared | 111 | 390 | 752 | 406 | 2,282 | 2,120 |
| a tenth, or declared, or the prefixes | 115 | 394 | 779 | 602 | 8,205 | 7,509 |

**Memento Mori** (32 maps; 46,428 plain faces)

| list | textures | switches (467) | doors (818) | own faces (735) | plain faces listed | of them walls |
|---|---|---|---|---|---|---|
| the hand-written prefixes (today) | 70 | 407 | 557 | 357 | 6,013 | 5,313 |
| declared switches only | 45 | 403 | 2 | 78 | 50 | 33 |
| 2+ uses, a quarter, or declared | 77 | 404 | 629 | 271 | 531 | 435 |
| **2+ uses, a tenth, or declared** | 93 | **409** | **671** | **329** | 1,181 | 1,018 |
| 2+ uses, a twentieth, or declared | 108 | 417 | 696 | 387 | 2,433 | 2,116 |
| a tenth, or declared, or the prefixes | 98 | 409 | 685 | 457 | 6,726 | 5,916 |

**Sunlust** (32 maps; 188,379 plain faces)

| list | textures | switches (440) | doors (20) | own faces (371) | plain faces listed | of them walls |
|---|---|---|---|---|---|---|
| the hand-written prefixes (today) | 37 | 364 | 4 | 195 | 1,022 | 956 |
| declared switches only | 25 | 361 | 0 | 110 | 646 | 390 |
| 2+ uses, a quarter, or declared | 32 | 364 | 8 | 132 | 698 | 441 |
| **2+ uses, a tenth, or declared** | 33 | **364** | **8** | **136** | 711 | 450 |
| 2+ uses, a twentieth, or declared | 35 | 374 | 8 | 199 | 1,485 | 1,175 |
| a tenth, or declared, or the prefixes | 40 | 366 | 8 | 200 | 1,426 | 1,108 |

**Eviternity II** (37 maps; 532,917 plain faces)

| list | textures | switches (550) | doors (231) | own faces (468) | plain faces listed | of them walls |
|---|---|---|---|---|---|---|
| the hand-written prefixes (today) | 0 | 0 | 0 | 0 | 0 | 0 |
| declared switches only | 37 | 400 | 1 | 151 | 407 | 372 |
| 2+ uses, half its faces | 36 | 313 | 101 | 110 | 165 | 154 |
| 2+ uses, a quarter, or declared | 55 | 413 | 111 | 242 | 567 | 494 |
| **2+ uses, a tenth, or declared** | 71 | **424** | **207** | **287** | 1,446 | 1,110 |
| 2+ uses, a twentieth, or declared | 78 | 426 | 209 | 309 | 1,877 | 1,463 |
| 1+ use, a tenth, or declared | 111 | 461 | 208 | 289 | 1,453 | 1,115 |

What the tables say:

- **The share is what matters, the count hardly.** Between 2 and 5 uses little moves; between
  a half and a tenth of its faces, doors and lifts' faces double. A switch texture is nearly
  always a switch (in DOOM II `SW1GSTON` 12 of 12, `SW1COMM` 10 of 10); a door texture is
  also door frames and trim (`EXITDOOR` 63 real of 163 faces); a lift's face is mostly wall
  (`SUPPORT3` 188 of 3,508).
- **A tenth is the knee.** At a quarter Eviternity II's doors stand at 111 of 231, at a tenth
  207; below a tenth the gain is small and the plain faces listed treble (in the IWADs, 2,261
  to 7,825).
- **Pooled beats per kind** at every setting (a texture that's a switch here and a lift's
  face there counts once): e.g. the IWADs at 2 uses and a quarter, 366/642/194 pooled against
  364/618/177 by kind.
- **Counting from one use** adds 37 of Eviternity II's switches for seven more plain faces.
  Those are textures on a single line in the whole WAD: listing one is knowing that line, not
  a texture set. Left out, by the maintainer's rubric ("used as a switch more than X times").
- **Lifts' faces are the weak kind everywhere**, about three in five at best: most lifts are
  faced like the walls beside them. That's shape's work (`lift_face`), not a list's.

### The check against the IWADs

Does the tally find the hand-written list again? Over DOOM and DOOM II at 2 uses and a
quarter it lists 75 textures, **61 of them ones the prefixes match**. At the recommended
tenth, with the declared switches, it recognises 390 switches, 749 doors and 406 own faces
where the prefixes recognise 386, 646 and 372, **and lists a third as many plain faces**
(2,261 against 6,638).

- *What it adds* (not matched by any prefix): door and lift faces under other names
  (`WOODMET1`, `WOODMET2`, `WOODMET4`, `TEKBRON2`, `MODWALL2`, `MODWALL4`, `BFALL1`,
  `STONE5`, `SILVER3`, `PANBLACK`), gun targets (`SKINSYMB`, `MARBFACE`, the `GST` faces),
  and at a tenth `MARBLE2` (139 real of 815), `ROCK5` (39 of 188), `DOORRED`, `DOORYEL`.
- *What it leaves out* that the prefixes have: `DOORTRAK` (4 real of 1,141 faces),
  `DOORSTOP` (0 of 499), `SUPPORT3` (188 of 3,508), `SUPPORT2` (21 of 515), the key-coloured
  frames at a quarter (`DOORBLU` 11 of 117), and switch textures on one line only (`SW1MARB`,
  `SW1SKULL`, the pressed `SW2...` halves), which the declared list brings back.
- *So could it replace the hand-written list?* By these counts yes, and it would be the
  better list: more recognised, fewer plain faces. But the two don't recognise the same lines
  (together they reach 602 own faces, where each alone reaches about 400: `SUPPORT3` and
  `DOORTRAK` are on lifts the tally misses), and swapping the list changes her play on the 68
  levels everything is tuned on. **Not as part of this:** keep the prefixes for the IWADs
  as they are, and try the swap later as a change of its own, on the three-lives test.

### What the WAD declares

The game has its own list of switch textures: the pairs in the `SWITCHES` lump, or the
engine's built-in ones (its own `SWITCHES` in `dsda-doom.wad`). They're the textures the
engine swaps and clicks for when pressed (`p_switch.c`, `P_ChangeSwitchTexture`: a press on
anything else changes nothing to see or hear). The lump is 20 bytes a pair, read in Rust with
the rest of the WAD: no engine call needed. It serves two ways:

- **an input:** declared textures are listed whatever the tally says, which restores the
  switch textures used once;
- **a check:** on every WAD here the tally's switch textures and the declared ones nearly
  coincide (Eviternity II: 400 switch lines by the declared list alone, 424 with the tally).
  A WAD where they don't is worth a look.

It covers switches only: 1 of Eviternity II's 231 doors, 36 of the IWADs' 1,039.

### Leakage: the whole WAD, this map, or the maps before

The tally over the whole WAD uses maps she hasn't played to inform the one she's on. So does
the IWAD list. The alternatives, at the recommended rule:

| tallied over | IWADs: switch / door / own face | Memento Mori | Sunlust | Eviternity II |
|---|---|---|---|---|
| the whole WAD | 390 / 749 / 406 | 409 / 671 / 329 | 364 / 8 / 136 | 424 / 207 / 287 |
| this map only | 396 / 813 / 569 | 421 / 689 / 440 | 417 / 20 / 211 | 432 / 208 / 328 |
| the maps before it only | 382 / 559 / 149 | 408 / 542 / 224 | 365 / 0 / 160 | 400 / 72 / 160 |

"This map only" recognises most, and is the least defensible: within one map a texture's
share is close to pointing at its lines. "The maps before it" is the nearest to honest and
much the weakest for doors and lifts (Eviternity II's doors: 72 of 231), and a level started
alone would have nothing. **The whole WAD**, as the IWAD list is.

### Who reads the list, and who mustn't

| reader | today | with the list |
|---|---|---|
| `interactables` (`model.rs:592`): which real use and gun lines she'll try | `looks_interactive` | **reads it.** This is what it's for. A plain face with a listed texture costs nothing here: only lines with a special are offered |
| the wall search (`walls.rs:293`, `wall_signs`: "looks interactive" is skipped) | `looks_interactive` | **must not read it.** Whatever the list holds is dropped from the walls she'd try: with the prefixes that's 6,085 wall faces in the IWADs today, `SUPPORT3`'s among them. The maintainer found the cost of widening it this week (the lift look put there, every lookalike gone from the walls, exits lost). It keeps the prefixes, unchanged |
| `drive`, `survey`, the viewer | `looks_interactive`, for showing | read it, and say which list a line is on |

So `looks_interactive` becomes two questions: *is its texture on the WAD's list* (for
`interactables`), and *does it match the old prefixes* (for the wall search, exactly as now).
On the IWADs both are the prefixes and nothing changes: `tools/parity.sh` must hold. A later
question, not this plan's: whether the wall search should skip a line because of its look at
all, or only because `interactables` has it.

For a PWAD the list is **the tally, the declared switches, and the prefixes too**: then no
WAD is recognised worse than today (Memento Mori's lift faces: 329 by the tally, 457 with the
prefixes, on IWAD textures it reuses).

### Where it's computed and kept

- **Computed** in the Rust WAD reader, as a pass of its own: for each map of the game as
  loaded (the IWAD's maps, replaced by the PWADs' where they have them, as `wad::load_level`
  resolves them), read `LINEDEFS`, `SIDEDEFS` and `SECTORS` only (no nodes), classify each
  special with `specials::classify_line`, tally. The scratch script took 36 seconds on
  Eviternity II in Python, 530,000 faces; in Rust it should be well under a second (not
  measured).
- **Kept** as a small text file under `out/`, beside the nav cache (`out/navcache`,
  `navigator/jumps.rs`): one texture a line with its counts, so it can be read and, if the
  maintainer wants, edited. Sweeps and the lives test start a process a map, so the file
  matters.
- **Keyed** by a hash of what it was made from: those three lumps of every map, the
  `SWITCHES` lump, and a version number for the rubric and the specials table (the tally
  changes when the table learns Boom's types).
- **A command to see it:** `evil-boom looks --pwad ...` printing the list with each texture's
  real uses and faces, by kind, and what the prefixes would have said. The census of
  mbf21-plan's stage 0 is the natural home.

## 3. Building it

| # | step | size | gate |
|---|---|---|---|
| 1 | The tally, the file and the `looks` command. No play changes | small | its counts match this note's tables for the five WADs |
| 2 | `interactables` reads the WAD's list (tally, declared, prefixes) for a PWAD; the wall search keeps the prefixes | small | IWAD parity; on Memento Mori and Sunlust the count of untried switches and doors at the end of a charmed run falls (each log's last line has it); the three-lives test on Memento Mori MAP01-08 no worse |
| 3 | Locks from the engine, not the message's English (section 4) | small | IWAD parity; a locked door in a WAD with reworded messages is learned as locked |
| 4 | Later, by itself: the tally's list in place of the prefixes on the IWADs | small to write | the three-lives test, all 68 levels |

Eviternity II can't be loaded till mbf21-plan's node readers exist; steps 1 and 2 can be
built and checked on Memento Mori and Sunlust before that.

**What to measure to know it worked.** Per WAD, charmed runs before and after: the "untried
switches/doors" count at the end; exits; and the time to the first press of each switch she
does press (the log has both). The numbers in this note are how many lines the gate lets
through, which is necessary and proves nothing about play.

## 4. Locks: its own item

`needs_key` (`model.rs:73`) decides the game refused her for want of a key by the message's
English: the word "need" with "key", "card" or "skull", or "will open". Eviternity II rewords
all of them ("Blue keycard required.", "Any key item required."): none matches, and a locked
line is never learned as locked (`learning.rs:298`), nor told to whoever directs her
(`control/directed.rs:863`).

The game's lock messages are eighteen strings the engine holds by name (`s_PD_BLUEO`,
`s_PD_BLUEK`, `s_PD_ANY`, `s_PD_ALL3` and the rest: `p_doors.c:469-659`, `p_spec.c:963-1079`),
and a BEX patch replaces their text, not which one is used. The boundary already passes the
last message by pointer (`evil_message`, the patch to `dsda/messenger.c`). So: `evil_api.c`
says *which* message it was, by comparing that pointer with the eighteen, and
`evil_player` carries a small number (none, or the lock it names) beside the text. `needs_key`
reads the number. The text still goes to Evi as the game wrote it. As honest as now: a player
reads the same message.

## 5. What this doesn't solve, and where that stays

- **A texture used once:** a one-off secret switch, a unique door. Left out on purpose.
- **Switches and lifts that look like walls on purpose,** and every lift faced like its
  surroundings: after the list, Memento Mori has some 280 own faces unrecognised of 735
  (prefixes included), Eviternity II 180 of 468. No list can have them without listing the walls.
- **Lines that show no texture:** 59 of Eviternity II's switches.

These stay where they are today: with `door_shaped` and `lift_face` (shape, not picture), with
lines found in her way or locked, and with the wall search as the late resort. They aren't
this plan's problem. One measured note for whoever next touches `door_shaped`: a shut, thin
sector alone finds 573 of DOOM's 593 use-doors, 776 of Memento Mori's 801 and 197 of
Eviternity II's 231; with "its face unlike the walls beside it" added, as the code has it,
457, 682 and 102. Since only real lines are ever offered, that last condition may be costing
doors for nothing.

**For the stream:** what she still can't tell, whoever directs her can see. The hand-off
exists already (an order to use a line or a wall); what would help is her saying so, short:
"Nothing left I'd take for a switch here."

## 6. Considered and set aside

Working out for herself what a texture looks like: comparing its pixels with known switches, a
classifier over rendered textures, reading its patches (`TEXTURE1`, `PNAMES`), asking the
directing model, weighing rarity or light, or building the list from her own presses. Set
aside by the maintainer's decision: knowing a switch by sight is a player's given, not
something the bot is to discover. Two of them were measured before that decision and would
have lost anyway: patches recognise 3 of Eviternity II's 550 switches (its art is all new)
and flag 1,865 plain faces in DOOM; "a rare texture unlike its neighbours" finds 133 of those
550 and 1,575 plain faces.

## 7. Not determined

- Whether more lines through the gate means more exits, or only more presses. Nothing was run.
- Why widening the look in the wall search cost exits (the lift look, this week). By these
  counts a plain face has no special to lose; the likely cause is a real line skipped by the
  wall search and not offered by `interactables` either (which wants its front seen), but
  that's a guess from reading, not a trace.
- The gate as a whole (list, `door_shaped`, `lift_face`, in her way, locked) wasn't scored
  together: the last two depend on play.
- The tally's time in Rust.
- Counts here take a line's trigger from `specials.rs` and, for Boom's numbered types, from
  the Boom reference: the bot's own tally would leave those out until its table has them
  (Eviternity II: about 65 use lines).
