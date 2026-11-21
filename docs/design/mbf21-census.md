# A census of Sunlust and Eviternity II (for the MBF21 plan)

> **Status:** findings, 2026-11-20 (Memento Mori added the same day, at the end). The tables behind [mbf21-plan.md](mbf21-plan.md), section 3.
> Read from the WADs themselves by a read-only script over their lumps; nothing was run in the
> game. The script was a scratch one and isn't in the repository: the plan's stage 0 makes it a
> command (`evil-boom census`).

**The files.** Sunlust: `sunlust.zip` from the idgames archive
(`levels/doom2/Ports/megawads/sunlust.zip`), `sunlust.wad` of 65,966,252 bytes, sha256
`6a90becf56040896...`. Eviternity II: `eviternityii.zip` from the authors' site
(eviternity.dfdoom.com, its download link), `Eviternity II.wad` of 293,081,691 bytes, dated
2024-11-23 ("Final Version"), sha256 `3127fbc96579960c...`. Both are under `wads/`, which git
ignores.

**How the lines are sorted.** By the bot's own tables, parsed from its source as of today:

- *modelled*: `movers.rs` gives the special heights, or it's an exit or a teleport in
  `TELEPORTERS_FOR_PLAYERS`.
- *known-na*: named in `specials.rs`, nothing for her to model (lights, wall scrolling, monsters'
  teleporters, a stop).
- *named-no-heights*: generalized (0x2F80-0x7FFF): named, a trigger, no effect predicted.
- *named-no-model*: named a teleport but not a route (243, 244).
- *unclassified*: "unclassified special N": not a trigger at all. Descriptions are the Boom
  reference's (boomref v1.3) where it lists the type in a table, mine for the scrollers; `[--]`
  is a passive line, `[W1]` and the like how it's set off.

Monsters are counted per skill from the thing flags (easy is skills 1-2, HMP 3, UV 4-5;
multiplayer-only things left out), lost souls included; health is each kind's spawn health
summed (Eviternity II's own kinds from its DEHACKED). "Dolls" is the player-1 starts beyond
the one she starts at. Sector damage is read as the engine reads it (`p_spec.c`
`P_PlayerInCompatibleSector`): under 32 the vanilla kinds, from 32 the bits only.

## Sunlust

Boom format, complevel 9: its text file says "Advanced engine needed: Boom-compatible"; it has
**no `COMPLVL` lump**, so our engine would run it at its default, 21. Lumps of note: `DEHACKED`
(2,687 bytes), `MAPINFO` (ZDoom's old format, which DSDA-Doom doesn't read for Doom; it gives
vanilla's order, MAP15 to MAP31 to MAP32 to MAP16, and MAP07's special), `SWITCHES`, `ANIMATED`.
No UMAPINFO, no OPTIONS.

**DEHACKED, all of it:** level names and intermission texts; and Thing 25, the Commander Keen,
remade: `Bits = 0` (not solid, not shootable, not counted), 10,000 hit points, speed 12, its
frames 763-773 all running `A_VileChase`. An unseen thing that walks about raising the dead. 56
of them, all on MAP30.

**SWITCHES:** 45 pairs, every one named `SW1...`/`SW2...` but `LITGTHW1`/`LITGTHB1`.
**ANIMATED:** adds the flats `SLUDGE01-04`, `OSLIME01-04`, `GRAYSLM1-4` and a dozen wall textures.

### Sizes and nodes

| map | nodes | lines | sides | sectors | things | segs | subsectors |
|---|---|---|---|---|---|---|---|
| MAP01 | vanilla | 5476 | 9699 | 632 | 301 | 12281 | 4413 |
| MAP02 | vanilla | 8172 | 13768 | 1671 | 389 | 17468 | 5903 |
| MAP03 | vanilla | 6324 | 10685 | 643 | 450 | 13366 | 5202 |
| MAP04 | vanilla | 9073 | 16135 | 1061 | 395 | 22143 | 8495 |
| MAP05 | vanilla | 7985 | 14749 | 1304 | 514 | 19255 | 6763 |
| MAP06 | vanilla | 8153 | 14622 | 674 | 287 | 19692 | 7095 |
| MAP07 | vanilla | 12655 | 21695 | 1813 | 888 | 28549 | 10539 |
| MAP08 | vanilla | 3422 | 5868 | 444 | 394 | 7995 | 2913 |
| MAP09 | vanilla | 7911 | 14121 | 1158 | 814 | 18374 | 6574 |
| MAP10 | vanilla | 10226 | 17969 | 887 | 599 | 23540 | 8716 |
| MAP11 | vanilla | 8429 | 14258 | 1068 | 657 | 19361 | 7324 |
| MAP12 | vanilla | 3354 | 5859 | 378 | 463 | 7451 | 2941 |
| MAP13 | vanilla | 10790 | 18502 | 1511 | 846 | 23684 | 9103 |
| MAP14 | vanilla | 8259 | 13734 | 630 | 366 | 17718 | 7223 |
| MAP15 | vanilla | 11001 | 19568 | 1077 | 876 | 26337 | 10781 |
| MAP16 | vanilla | 17684 | 31603 | 2378 | 1132 | 37527 | 15148 |
| MAP17 | vanilla | 5519 | 9152 | 435 | 302 | 11648 | 5240 |
| MAP18 | vanilla | 13396 | 25450 | 1623 | 865 | 34460 | 13929 |
| MAP19 | vanilla | 7562 | 13711 | 910 | 458 | 18279 | 6655 |
| MAP20 | vanilla | 15036 | 26581 | 1458 | 1136 | 33617 | 13678 |
| MAP21 | vanilla | 8769 | 15739 | 1036 | 608 | 20795 | 7811 |
| MAP22 | vanilla | 7140 | 12696 | 685 | 405 | 17502 | 7354 |
| MAP23 | vanilla | 5564 | 9836 | 568 | 335 | 12550 | 5023 |
| MAP24 | vanilla | 26492 | 50221 | 3701 | 2562 | 65396 | 25775 |
| MAP25 | vanilla | 10663 | 20437 | 2131 | 1073 | 26752 | 8382 |
| MAP26 | vanilla | 4667 | 7832 | 558 | 478 | 10054 | 3417 |
| MAP27 | vanilla | 15792 | 28962 | 2226 | 637 | 38394 | 13143 |
| MAP28 | XNOD | 34376 | 64914 | 3573 | 1913 | 0 | 0 |
| MAP29 | vanilla | 9392 | 16744 | 1009 | 980 | 21956 | 7733 |
| MAP30 | XNOD | 34605 | 63566 | 4304 | 6039 | 0 | 0 |
| MAP31 | vanilla | 14201 | 27610 | 846 | 360 | 38235 | 13448 |
| MAP32 | vanilla | 8267 | 14919 | 1051 | 330 | 19550 | 8045 |

Two maps (MAP28, MAP30) have XNOD nodes. MAP28's 64,914 sidedefs are near the format's 65,535;
nothing is past a limit of our reader.

### What each map uses that she doesn't model, and what's against her

| map | easy/HMP/UV monsters | health easy/HMP/UV | gen lines | of them use/gun | other unmodelled triggers | carry scrollers | friction lines | 242 | keens | brains | exits (line) | death/exit sectors | keys | dolls |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MAP01 | 69/78/80 | 5870/7410/8430 | 5 | 3 | - | 0 | 0 | 4 | 0 | 0 | 2 | 0 | Y | 0 |
| MAP02 | 71/81/95 | 6980/10220/14110 | 38 | 3 | - | 5 | 0 | 6 | 0 | 0 | 6 | 0 | BY | 9 |
| MAP03 | 108/122/152 | 13370/17070/22030 | 24 | 11 | - | 8 | 0 | 0 | 0 | 0 | 4 | 0 | BY | 2 |
| MAP04 | 84/97/108 | 16250/20350/22530 | 97 | 11 | - | 19 | 0 | 11 | 0 | 0 | 8 | 0 | BYR | 22 |
| MAP05 | 130/138/149 | 10640/14990/21940 | 29 | 2 | - | 2 | 0 | 7 | 0 | 0 | 8 | 0 | - | 13 |
| MAP06 | 92/97/101 | 19040/21230/23070 | 32 | 17 | 227x1 231x1 | 5 | 0 | 25 | 0 | 0 | 14 | 0 | Y | 5 |
| MAP07 | 201/203/228 | 22640/32610/44660 | 80 | 9 | - | 4 | 8 | 5 | 0 | 0 | 8 | 0 | - | 10 |
| MAP08 | 125/145/162 | 29110/40340/55180 | 23 | 6 | - | 7 | 0 | 5 | 0 | 0 | 36 | 0 | BY | 7 |
| MAP09 | 136/151/182 | 23140/40300/57510 | 76 | 9 | - | 3 | 0 | 15 | 0 | 0 | 36 | 0 | BYR | 16 |
| MAP10 | 192/215/235 | 37420/49520/64480 | 26 | 12 | - | 11 | 0 | 8 | 0 | 1 | 4 | 0 | BR | 9 |
| MAP11 | 159/196/248 | 17810/28130/40220 | 26 | 1 | - | 3 | 0 | 22 | 0 | 0 | 1 | 0 | Y | 13 |
| MAP12 | 236/240/247 | 34930/39950/43020 | 35 | 17 | - | 2 | 0 | 0 | 0 | 0 | 1 | 0 | BR | 2 |
| MAP13 | 241/267/307 | 31010/42340/55440 | 26 | 6 | - | 2 | 0 | 13 | 0 | 0 | 4 | 0 | BYR | 8 |
| MAP14 | 116/120/126 | 39760/42440/45840 | 14 | 5 | - | 15 | 0 | 22 | 0 | 0 | 4 | 0 | Y | 8 |
| MAP15 | 224/272/319 | 52420/72920/105320 | 45 | 10 | - | 1 | 0 | 0 | 0 | 0 | 16 | 0 | BYR | 8 |
| MAP16 | 214/278/345 | 55860/84860/121120 | 27 | 9 | - | 3 | 0 | 7 | 0 | 1 | 1 | 0 | BYR | 5 |
| MAP17 | 94/112/126 | 29600/40720/48640 | 28 | 3 | - | 2 | 0 | 2 | 0 | 0 | 0 | 2 | YR | 2 |
| MAP18 | 235/320/415 | 46130/81150/125060 | 43 | 5 | - | 1 | 0 | 8 | 0 | 0 | 8 | 0 | BYR | 13 |
| MAP19 | 162/193/220 | 55760/69990/88790 | 18 | 9 | - | 2 | 0 | 6 | 0 | 0 | 4 | 0 | YR | 2 |
| MAP20 | 438/461/544 | 158470/168070/213940 | 52 | 16 | - | 9 | 0 | 2 | 0 | 1 | 1 | 0 | BYR | 6 |
| MAP21 | 192/183/263 | 25110/29770/42450 | 47 | 6 | - | 17 | 0 | 6 | 0 | 0 | 8 | 0 | BYR | 15 |
| MAP22 | 130/142/158 | 35520/41240/58040 | 15 | 8 | - | 4 | 0 | 0 | 0 | 0 | 1 | 0 | BY | 4 |
| MAP23 | 90/86/84 | 14640/18110/20470 | 99 | 10 | - | 14 | 0 | 4 | 0 | 0 | 2 | 0 | YR | 12 |
| MAP24 | 1250/1571/1808 | 192020/300540/407970 | 125 | 4 | 200x2 | 8 | 0 | 9 | 0 | 4 | 0 | 0 | YR | 29 |
| MAP25 | 343/414/544 | 68020/86400/133090 | 32 | 4 | 157x34 | 4 | 0 | 6 | 0 | 0 | 4 | 0 | YR | 18 |
| MAP26 | 138/183/256 | 55880/78670/100650 | 19 | 7 | - | 7 | 0 | 5 | 0 | 0 | 4 | 0 | Y | 5 |
| MAP27 | 192/206/231 | 55590/68870/93630 | 46 | 5 | - | 14 | 0 | 37 | 0 | 0 | 1 | 0 | Y | 10 |
| MAP28 | 783/851/958 | 303930/381190/466290 | 51 | 12 | - | 8 | 0 | 28 | 0 | 1 | 5 | 0 | BYR | 13 |
| MAP29 | 372/467/587 | 122660/170120/230800 | 173 | 11 | - | 17 | 0 | 9 | 0 | 0 | 1 | 0 | YR | 13 |
| MAP30 | 1829/1981/2187 | 655520/758380/856960 | 158 | 46 | - | 64 | 0 | 73 | 56 | 1 | 0 | 0 | Y | 36 |
| MAP31 | 56/63/57 | 9900/10370/11400 | 49 | 4 | - | 93 | 0 | 9 | 0 | 0 | 2 | 0 | YR | 10 |
| MAP32 | 67/76/105 | 21640/26920/57600 | 15 | 3 | - | 1 | 0 | 0 | 0 | 0 | 1 | 0 | Y | 4 |

"Other unmodelled triggers" leaves out monsters' own teleporters (268, 269) and lights. Maps
with no exit line and no exit floor (MAP24, MAP30) have boss brains: the level ends when one
dies (`A_BrainDie`). No map has a boss shooter (things 87, 89): none is an Icon of Sin fight.

| map | monsters e/h/u | health e/h/u | arch-viles | cyberdemons | masterminds | revenants | pain elementals | chaingunners | custom (not vanilla) |
|---|---|---|---|---|---|---|---|---|---|
| MAP01 | 69/78/80 | 5870/7410/8430 | 0/0/1 | 0/0/0 | 0/0/0 | 0/2/2 | 0/1/1 | 4/8/12 | 0/0/0 |
| MAP02 | 71/81/95 | 6980/10220/14110 | 2/3/4 | 0/0/0 | 0/0/0 | 2/1/3 | 1/1/3 | 2/5/10 | 0/0/0 |
| MAP03 | 108/122/152 | 13370/17070/22030 | 0/0/2 | 0/0/0 | 0/0/0 | 5/6/15 | 2/2/3 | 3/4/7 | 0/0/0 |
| MAP04 | 84/97/108 | 16250/20350/22530 | 0/1/3 | 0/0/0 | 0/0/0 | 15/19/27 | 0/1/1 | 4/3/28 | 0/0/0 |
| MAP05 | 130/138/149 | 10640/14990/21940 | 2/3/3 | 0/0/0 | 0/0/0 | 4/4/12 | 0/0/0 | 0/19/38 | 0/0/0 |
| MAP06 | 92/97/101 | 19040/21230/23070 | 2/4/5 | 0/0/0 | 0/0/0 | 4/4/5 | 1/3/4 | 12/14/18 | 0/0/0 |
| MAP07 | 201/203/228 | 22640/32610/44660 | 0/0/2 | 0/0/0 | 0/0/0 | 1/29/49 | 0/0/0 | 3/6/8 | 0/0/0 |
| MAP08 | 125/145/162 | 29110/40340/55180 | 2/4/5 | 1/2/3 | 0/0/0 | 12/17/14 | 0/3/3 | 11/16/20 | 0/0/0 |
| MAP09 | 136/151/182 | 23140/40300/57510 | 1/3/3 | 0/2/2 | 0/0/0 | 4/15/28 | 0/4/13 | 1/5/25 | 0/0/0 |
| MAP10 | 192/215/235 | 37420/49520/64480 | 2/2/8 | 2/3/4 | 0/0/0 | 8/11/17 | 4/5/5 | 2/2/8 | 0/0/0 |
| MAP11 | 159/196/248 | 17810/28130/40220 | 0/1/6 | 0/0/0 | 0/0/0 | 15/22/30 | 0/1/6 | 5/10/35 | 0/0/0 |
| MAP12 | 236/240/247 | 34930/39950/43020 | 0/2/4 | 1/1/1 | 0/1/1 | 8/10/10 | 4/13/17 | 11/18/19 | 0/0/0 |
| MAP13 | 241/267/307 | 31010/42340/55440 | 2/3/7 | 0/1/1 | 1/1/1 | 9/13/27 | 2/3/10 | 3/10/20 | 0/0/0 |
| MAP14 | 116/120/126 | 39760/42440/45840 | 0/1/4 | 2/2/2 | 0/0/0 | 9/13/14 | 0/1/1 | 0/16/16 | 0/0/0 |
| MAP15 | 224/272/319 | 52420/72920/105320 | 1/2/5 | 2/3/5 | 0/0/0 | 33/44/79 | 0/0/1 | 6/14/18 | 0/0/0 |
| MAP16 | 214/278/345 | 55860/84860/121120 | 4/7/25 | 0/1/1 | 0/0/3 | 53/77/110 | 2/2/5 | 2/2/4 | 0/0/0 |
| MAP17 | 94/112/126 | 29600/40720/48640 | 5/7/9 | 0/0/2 | 0/0/0 | 12/16/22 | 0/0/4 | 6/10/6 | 0/0/0 |
| MAP18 | 235/320/415 | 46130/81150/125060 | 5/8/14 | 0/1/3 | 1/3/4 | 31/59/84 | 0/4/16 | 0/0/0 | 0/0/0 |
| MAP19 | 162/193/220 | 55760/69990/88790 | 0/8/16 | 2/2/2 | 0/0/0 | 38/34/43 | 2/2/5 | 18/16/16 | 0/0/0 |
| MAP20 | 438/461/544 | 158470/168070/213940 | 12/12/26 | 5/5/11 | 1/1/1 | 77/85/91 | 14/17/20 | 67/67/88 | 0/0/0 |
| MAP21 | 192/183/263 | 25110/29770/42450 | 0/2/4 | 0/1/1 | 1/1/1 | 12/15/29 | 2/4/4 | 17/17/29 | 0/0/0 |
| MAP22 | 130/142/158 | 35520/41240/58040 | 3/3/8 | 0/0/1 | 0/0/0 | 28/32/30 | 0/0/3 | 6/6/10 | 0/0/0 |
| MAP23 | 90/86/84 | 14640/18110/20470 | 2/5/7 | 0/0/0 | 0/0/0 | 12/19/22 | 4/4/4 | 24/26/20 | 0/0/0 |
| MAP24 | 1250/1571/1808 | 192020/300540/407970 | 18/27/47 | 1/3/5 | 0/0/0 | 147/243/314 | 2/4/14 | 11/13/24 | 0/0/0 |
| MAP25 | 343/414/544 | 68020/86400/133090 | 1/3/8 | 2/2/2 | 0/0/0 | 29/40/83 | 0/1/7 | 0/0/6 | 0/0/0 |
| MAP26 | 138/183/256 | 55880/78670/100650 | 6/9/13 | 1/2/3 | 0/0/0 | 21/30/39 | 0/4/1 | 12/19/21 | 0/0/0 |
| MAP27 | 192/206/231 | 55590/68870/93630 | 4/5/10 | 0/2/3 | 2/2/2 | 42/41/55 | 6/0/1 | 19/19/9 | 0/0/0 |
| MAP28 | 783/851/958 | 303930/381190/466290 | 14/23/47 | 10/17/18 | 4/4/4 | 108/135/141 | 0/23/23 | 0/24/32 | 0/0/0 |
| MAP29 | 372/467/587 | 122660/170120/230800 | 14/23/45 | 7/13/17 | 0/0/0 | 83/89/124 | 0/1/1 | 12/12/14 | 0/0/0 |
| MAP30 | 1829/1981/2187 | 655520/758380/856960 | 40/52/96 | 24/26/26 | 0/0/0 | 546/561/606 | 18/58/78 | 49/49/55 | 0/0/0 |
| MAP31 | 56/63/57 | 9900/10370/11400 | 3/3/6 | 0/0/0 | 0/0/0 | 8/8/5 | 0/0/0 | 6/11/26 | 0/0/0 |
| MAP32 | 67/76/105 | 21640/26920/57600 | 12/18/36 | 0/1/3 | 0/0/0 | 20/22/64 | 0/0/0 | 0/0/0 | 0/0/0 |

### Totals

**lines: modelled: 4156 lines, 41 kinds**

```
    1448  109 blazing door open stay [W1]
    1292  97 teleport [WR]
     219  19 floor lower to highest neighbor [W1]
     202  38 floor lower to lowest [W1]
     182  52 exit level [W1]
     179  62 lift [SR]
      83  137 yellow blazing door open stay [S1]
      81  112 blazing door open stay [S1]
      65  208 silent teleport [WR]
      54  2 door open stay [W1]
      51  123 blazing lift [SR]
      41  23 floor lower to lowest [S1]
      31  103 door open stay [S1]
      31  133 blue blazing door open stay [S1]
      22  135 red blazing door open stay [S1]
      19  110 blazing door close [W1]
      14  36 floor turbo lower to highest neighbor + 8 [W1]
      14  25 slow crusher [W1]
      14  88 lift [WR]
      12  6 fast crusher [W1]
      10  105 blazing door open-wait-close [WR]
      10  195 teleport [SR]
      10  141 silent crusher [W1]
      10  124 secret exit [W1]
       9  3 door close [W1]
       8  61 door open stay [SR]
       6  53 perpetual lift start [W1]
       5  114 blazing door open-wait-close [SR]
       4  115 blazing door open stay [SR]
       4  1 door open-wait-close [DR]
       4  71 floor turbo lower to highest neighbor + 8 [S1]
       4  39 teleport [W1]
       4  27 yellow door open-wait-close [DR]
       3  11 exit level [S1]
       2  120 blazing lift [WR]
       2  86 door open stay [WR]
       2  63 door open-wait-close [SR]
       2  134 red blazing door open stay [SR]
       1  29 door open-wait-close [S1]
       1  40 ceiling raise to highest [W1]
       1  98 floor turbo lower to highest neighbor + 8 [WR]
```

**lines: known-na: 3984 lines, 5 kinds**

```
    2898  126 teleport (monsters only) [WR]
     676  48 scroll wall left [None]
     228  125 teleport (monsters only) [W1]
     181  81 light to 255 [WR]
       1  57 crusher stop [W1]
```

**lines: named-no-heights: 1573 lines, 19 kinds**

```
     987  gen floor W1
     178  gen floor S1
     156  gen floor WR
      58  gen door WR
      52  gen ceiling W1
      32  gen lift WR
      27  gen lift SR
      22  gen floor SR
      12  gen locked door S1
      12  gen door DR
      11  gen floor G1
      10  gen ceiling S1
       7  gen door G1
       3  gen door W1
       2  gen door SR
       1  gen locked door SR
       1  gen crusher W1
       1  gen locked door GR
       1  gen door S1
```

**lines: named-no-model: 0 lines, 0 kinds**

```

```

**lines: unclassified: 6301 lines, 16 kinds**

```
    1912  271 Property Transfer: Transfer sky texture to tagged sectors [--]
    1660  255 Scrollers: scroll wall by sidedef offsets [--]
    1165  85 Scrollers: wall scrolls right [--]
     402  269 Teleport Linedef types: Yes Yes No Set TP thing in tagged sector [WR]
     354  242 Property Transfer: Set Tagged Lighting, Flats Heights to 1st Sidedef's Sector, [--]
     346  253 Scrollers: scroll tagged floor and CARRY objects [--]
     292  260 Property Transfer: Make Tagged Lines (or this line if tag==0) Translucent [--]
      80  213 Property Transfer: Set Tagged Floor Lighting to Lighting on 1st Sidedef's Sector [--]
      35  261 Property Transfer: Set Tagged Ceiling Lighting to Lighting on 1st Sidedef's Sector [--]
      34  157 Lighting Linedef Types: Minimum Neighbor [WR]
       8  223 Property Transfer: Length Sets Friction in tagged Sector,Sludge<100, Ice>100 [--]
       5  247 Scrollers: displacement: CARRY objects [--]
       4  248 Scrollers: displacement: floor and CARRY [--]
       2  200 Ceiling linedef types: Dn Slow None -- No No Highest Neighbor Floor [W1]
       1  227 Elevator Linedef Types: Fast Next Highest Floor [W1]
       1  231 Elevator Linedef Types: Fast Next Lowest Floor [W1]
```

**line flags past the vanilla ones**

```

```

**sector specials**

```
    1700  vanilla 5
     769  vanilla 8
     473  vanilla 16
     287  vanilla 17
     136  vanilla 7
      86  vanilla 9
      42  vanilla 2
      23  friction
      17  vanilla 3
       4  vanilla 1
       2  vanilla 11
       1  damage 10 + secret
       1  light 8 + secret
       1  light 16 + damage 5 + BOT-MISREADS damage 20 vs 5
```

**use lines by look (kind, looks interactive by prefix)**

```
     605  ('switch', True)
     206  ('switch', False)
      16  ('door', False)
       4  ('door', True)
```

**floor flats of damaging sectors (flat, bot takes it for a liquid)**

```
    1721  ('OSLIME01', False)
     332  ('LAVA1', True)
     167  ('NUKAGE1', True)
      24  ('SLIME09', True)
      14  ('FLOOR33D', False)
      12  ('DIAMOND6', False)
       9  ('FLAT5_3', False)
       7  ('GOTHMT1F', False)
       5  ('SKINFLT2', False)
       4  ('GROUND25', False)
       3  ('F_SKY1', False)
       3  ('AQUAFLAT', False)
       2  ('MFLR8_3', False)
       2  ('GROUND02', False)
       2  ('BLOOD1', True)
       1  ('HEXAMIN3', False)
       1  ('GRASS2', False)
       1  ('SLIME01', True)
       1  ('GOTHLE2F', False)
       1  ('ALLBLAKF', False)
       1  ('DIAMOND4', False)
```

**floor flats of MBF21 death sectors**

```

```

**Generalized lines, decoded** (`p_spec.h`'s fields):

```
     654  floor down to lowest neighbour floor
     168  floor down to lowest neighbour ceiling
     143  floor up to lowest neighbour ceiling
      75  floor up to highest neighbour floor
      59  door open-wait-close
      53  floor down to highest neighbour floor
      46  floor up to lowest neighbour floor
      40  floor down to lowest neighbour floor, change: texture
      35  floor down to next neighbour floor
      29  lift to lowest neighbour floor
      29  lift to next lowest floor
      27  ceiling up to highest neighbour ceiling
      24  floor down by 32
      21  door open stay
      20  floor down by 24
      18  floor up to next neighbour floor
      14  floor up to ceiling
      14  floor down to lowest neighbour ceiling, change: texture
      13  floor down to lowest neighbour floor, change: texture+type
      10  floor up by 32
       9  ceiling up to lowest neighbour ceiling
       6  ceiling up to next neighbour ceiling
       6  ceiling up to highest neighbour floor
       5  floor down to lowest neighbour ceiling, change: texture+type
       4  ceiling down to floor
       3  floor up by 24, change: texture
       3  ceiling down to lowest neighbour ceiling
       3  ceiling up to highest neighbour ceiling, change: texture
       3  floor up to lowest neighbour ceiling, change: texture
       3  floor up by 32, change: zero type
       2  floor up by 32, change: texture
       2  locked door yellow card (card or skull) open stay
       2  door close
       2  floor up to highest neighbour floor, change: texture
       2  floor up by 24
       2  ceiling up by 32
       2  locked door yellow skull open stay
       2  locked door all open stay
       2  locked door yellow card open stay
       2  floor up to lowest neighbour ceiling, change: zero type
       1  door close-wait-open
       1  locked door any open stay
       1  floor up to lowest neighbour ceiling, change: texture+type
       1  floor up by shortest lower texture
       1  crusher
       1  floor up to ceiling, crush
       1  ceiling up to highest neighbour floor, change: texture
       1  locked door red card open stay
       1  lift to lowest neighbour ceiling
       1  locked door all (card or skull) open stay
       1  locked door blue skull (card or skull) open stay
       1  locked door yellow skull (card or skull) open stay
       1  locked door red skull (card or skull) open stay
       1  floor up to lowest neighbour floor, change: texture
       1  floor up to next neighbour floor, change: texture
       1  ceiling down to highest neighbour floor
```

**Thing types:** every one is vanilla's but type 32000 (5 things, on a few maps; not in the
engine's table, which skips it. From memory it's a map editor's own marker).

**Floors that carry** (sectors tagged by a carrying scroller: 252, 253, 247, 248, 216, 217), by
what stands on them at the start:

```
MAP02: 9 doll, 7 empty, small, 6 monster, 1 items only
MAP03: 6 items only, 2 doll
MAP04: 22 doll, 16 monster, 1 empty, large
MAP05: 13 doll, 4 monster
MAP06: 4 doll, 1 empty, large
MAP07: 19 monster, 10 doll, 2 items only, 1 empty, small, 1 empty, large
MAP08: 7 empty, small
MAP09: 16 doll, 7 monster
MAP10: 54 empty, small, 8 empty, large, 2 items only, 1 barrel, 1 doll
MAP11: 11 doll, 3 monster, 1 empty, small
MAP12: 1 empty, small, 1 doll
MAP13: 8 doll
MAP14: 7 empty, large, 5 doll, 3 empty, small
MAP15: 8 doll, 3 monster
MAP16: 54 empty, small, 5 doll, 2 items only, 2 empty, large, 1 barrel, 1 monster
MAP17: 2 doll
MAP18: 13 doll, 1 empty, small
MAP19: 1 empty, small, 1 doll
MAP20: 56 empty, small, 4 doll, 2 items only, 1 empty, large, 1 barrel
MAP21: 15 doll, 15 items only, 7 monster, 1 empty, large
MAP22: 3 empty, small, 1 doll
MAP23: 6 empty, large, 6 doll, 1 empty, small, 1 items only
MAP24: 29 doll, 6 items only, 4 empty, small, 4 monster, 2 empty, large
MAP25: 14 doll
MAP26: 3 empty, small, 2 doll, 2 items only
MAP27: 8 items only, 7 doll, 3 empty, large
MAP28: 54 empty, small, 13 doll, 2 items only, 1 barrel
MAP29: 8 empty, small, 5 doll, 4 monster
MAP30: 29 empty, large, 27 doll, 12 monster
MAP31: 84 empty, small, 4 doll, 3 empty, large, 2 items only
MAP32: 4 doll
TOTAL {'empty, small': 343, 'monster': 86, 'doll': 262, 'items only': 51, 'empty, large': 65, 'barrel': 4}
```

Dolls and monsters' closets account for most. The 65 large empty ones may be floors she
walks: not told apart by this census.

## Eviternity II

MBF21: `COMPLVL` lump reading `mbf21`. Lumps of note: `DEHACKED` (1,439,098 bytes), `UMAPINFO`,
`SWITCHES`, `ANIMATED`, `GAMEINFO` (`IWAD = "DOOM2.WAD"`), and for other engines `MAPINFO`,
`ZMAPINFO`, `DECORATE`, `ZSCRIPT`, `SNDINFO`, `LANGUAGE`. 37 maps, MAP01-MAP37.

**UMAPINFO:** an entry a map. Order MAP01-MAP30 then MAP37 (`endpic`); five secret exits
(MAP03 to MAP31 back to MAP04; MAP09 to MAP32 to MAP10; MAP14 to MAP33 to MAP15; MAP16 to MAP34
to MAP17; MAP22 to MAP35 to MAP23; MAP29 to MAP36 to MAP30); seven episodes starting at MAP01,
06, 11, 16, 21, 26 and 37. **No `bossaction`:** its bosses end levels through codepointers.

**DEHACKED:** 383 things, 368 of them new (numbers 151 to 2005: past DEHEXTRA's range, so
DSDHacked); 9,587 frames, all but 8 new (to 10,667); 2 weapons; `[CODEPTR]`, `[STRINGS]`,
`[SPRITES]`.
- *Monsters:* 27 new kill-counted kinds (a former corporal and captain, astral cacodemons,
  arachnotrons and mancubi, a nightmare demon, an annihilator, a veilimp, dukes of hell,
  necromenaces, the Origin in three phases, and others), from 30 to 11,000 hit points, and one of
  99,999 (MAP24's cyberdemon variant). Fields used: `MBF21 Bits` on 32 things, `Infighting group`
  and `Projectile group` on 5, `Melee range` on 2. The lost soul has 45 hit points; the
  cyberdemon is in groups of its own.
- *Other new placeable things:* 202, of which 83 solid and 9 shootable but not counted
  (a babycaco, minions, a timed barrel, effect triggers, breaking glass). **No new pickups.**
- *Codepointers set* (how often): `SpawnObject` 2,756, `MonsterProjectile` 1,956, `FaceTarget`
  1,039, `Chase` 339, `RandomJump` 206, `PlaySound` 101, `Spawn` 89, `SeekTracer` 53,
  `LineEffect` 48, `HealChase` 16, `RadiusDamage` 14, `JumpIfTargetCloser` 10,
  `MonsterBulletAttack` 6, `Mushroom` 6, `AddFlags`/`RemoveFlags` 7, `KeenDie` 4, `BrainDie` 2,
  and `MonsterMeleeAttack`, `NoiseAlert`; for weapons `WeaponBulletAttack` 4,
  `WeaponProjectile` 3, `WeaponSound` 4, `GunFlashTo` 3, `ConsumeAmmo` 2, `CheckAmmo`,
  `WeaponMeleeAttack`.
- *Weapons:* the pistol remade (ammo per shot 1, new frames); the chainsaw's slot become the
  "Perforator" (bullets, 25 a shot).
- *Strings:* the locked-door messages are reworded ("Blue keycard required.", "Any key item
  required.", "All three keys required."): none has the word "need" that
  `knowledge/model.rs` `needs_key` looks for.

**SWITCHES:** 70 pairs; 30 of them aren't named `SW1`/`SW2` (OTEX's `OSWTCH...` and others).
**ANIMATED:** 30 new animated flats (OTEX's liquids: `ONUKE`, `OLAVA`, `OSLUDG`, `OICYWA`,
`OWATER`, `ZWATER`, `OTAR`, `OBLOD`, `OGOOPY`, and teleporter pads).

**Thing flags:** friendly (0x80) on 5 things.

### Sizes and nodes

| map | nodes | lines | sides | sectors | things | segs | subsectors |
|---|---|---|---|---|---|---|---|
| MAP01 | XNOD | 35270 | 65425 | 5597 | 3041 | 0 | 0 |
| MAP02 | vanilla | 23239 | 43310 | 4503 | 3882 | 54359 | 21426 |
| MAP03 | XNOD | 13830 | 24481 | 1861 | 2826 | 0 | 0 |
| MAP04 | XNOD | 19646 | 34941 | 3911 | 2974 | 0 | 0 |
| MAP05 | XNOD | 18051 | 31998 | 2960 | 3928 | 0 | 0 |
| MAP06 | XNOD | 8982 | 14647 | 1681 | 815 | 0 | 0 |
| MAP07 | vanilla | 19241 | 33612 | 4163 | 1632 | 41876 | 13759 |
| MAP08 | XNOD | 17967 | 32309 | 4475 | 3631 | 0 | 0 |
| MAP09 | XNOD | 19192 | 34974 | 3973 | 3022 | 0 | 0 |
| MAP10 | XNOD | 5624 | 9329 | 996 | 721 | 0 | 0 |
| MAP11 | XNOD | 23697 | 43459 | 3862 | 6744 | 0 | 0 |
| MAP12 | XNOD | 9154 | 17605 | 1316 | 2128 | 0 | 0 |
| MAP13 | XNOD | 24500 | 46239 | 5026 | 4353 | 0 | 0 |
| MAP14 | XNOD | 44497 | 48591 | 7881 | 4624 | 0 | 0 |
| MAP15 | vanilla | 14888 | 24201 | 2313 | 2212 | 31929 | 10599 |
| MAP16 | XNOD | 12629 | 22197 | 3253 | 2122 | 0 | 0 |
| MAP17 | XNOD | 24704 | 44799 | 5764 | 6592 | 0 | 0 |
| MAP18 | XNOD | 46468 | 53446 | 9221 | 5554 | 0 | 0 |
| MAP19 | XNOD | 14098 | 25791 | 2736 | 3730 | 0 | 0 |
| MAP20 | XNOD | 19928 | 36974 | 3965 | 4642 | 0 | 0 |
| MAP21 | XNOD | 17767 | 31634 | 3962 | 1942 | 0 | 0 |
| MAP22 | vanilla | 18661 | 33042 | 4864 | 1631 | 38624 | 12855 |
| MAP23 | XNOD | 13196 | 21127 | 2667 | 1638 | 0 | 0 |
| MAP24 | DeePBSP | 43716 | 28740 | 3294 | 3322 | 121290 | 47965 |
| MAP25 | XNOD | 5782 | 9819 | 989 | 1075 | 0 | 0 |
| MAP26 | XNOD | 64782 | 65521 | 10861 | 3622 | 0 | 0 |
| MAP27 | XNOD | 28220 | 52728 | 4647 | 1616 | 0 | 0 |
| MAP28 | XNOD | 28191 | 53310 | 4059 | 1247 | 0 | 0 |
| MAP29 | XNOD | 17090 | 32231 | 1954 | 1448 | 0 | 0 |
| MAP30 | vanilla | 18669 | 36634 | 1933 | 518 | 44416 | 17550 |
| MAP31 | XNOD | 33822 | 59596 | 5590 | 1169 | 0 | 0 |
| MAP32 | XNOD | 49773 | 43631 | 9461 | 10753 | 0 | 0 |
| MAP33 | XNOD | 62307 | 63179 | 20131 | 2449 | 0 | 0 |
| MAP34 | XNOD | 15429 | 28099 | 2776 | 5356 | 0 | 0 |
| MAP35 | XNOD | 7790 | 12576 | 1258 | 1715 | 0 | 0 |
| MAP36 | ZNOD | 65535 | 65535 | 10479 | 8416 | 0 | 0 |
| MAP37 | XNOD | 9110 | 16024 | 1746 | 1871 | 0 | 0 |

32 of 37 maps have extended nodes: XNOD on 30, DeePBSP on MAP24, ZNOD (compressed) on MAP36.
MAP36 has exactly 65,535 lines and 65,535 sidedefs, the format's last.

### What each map uses that she doesn't model, and what's against her

| map | easy/HMP/UV monsters | health easy/HMP/UV | gen lines | of them use/gun | other unmodelled triggers | carry scrollers | friction lines | 242 | keens | brains | exits (line) | death/exit sectors | keys | dolls |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MAP01 | 1/1/1 | 400/400/400 | 118 | 1 | 159x1 265x1 | 401 | 0 | 556 | 0 | 0 | 19 | 0 | R | 23 |
| MAP02 | 76/94/120 | 4270/5710/6890 | 50 | 7 | 159x2 219x1 | 6 | 0 | 6 | 0 | 0 | 8 | 0 | BY | 8 |
| MAP03 | 86/112/154 | 9460/10870/14140 | 29 | 1 | 159x2 219x18 244x2 | 2 | 0 | 0 | 0 | 0 | 5 | 0 | Y | 8 |
| MAP04 | 120/149/193 | 19850/23300/27550 | 141 | 12 | 219x1 221x2 | 4 | 0 | 0 | 0 | 0 | 5 | 0 | B | 7 |
| MAP05 | 199/264/343 | 27310/41280/49990 | 24 | 11 | 219x1 | 2 | 0 | 1 | 0 | 0 | 1 | 0 | BR | 5 |
| MAP06 | 65/84/109 | 7120/9150/12990 | 2 | 1 | 265x24 | 3 | 0 | 0 | 0 | 0 | 12 | 0 | R | 4 |
| MAP07 | 366/383/401 | 42520/45400/50720 | 77 | 17 | 166x1 221x2 244x4 | 1 | 0 | 0 | 0 | 0 | 5 | 0 | BYR | 14 |
| MAP08 | 136/195/230 | 25810/45750/58910 | 107 | 22 | 267x2 | 14 | 0 | 14 | 0 | 0 | 1 | 0 | BYR | 10 |
| MAP09 | 222/296/394 | 33640/44430/64780 | 14 | 4 | 219x3 221x5 | 5 | 0 | 6 | 0 | 0 | 2 | 0 | BYR | 6 |
| MAP10 | 195/237/288 | 22910/28970/39650 | 5 | 1 | 159x1 219x5 221x2 | 3 | 0 | 0 | 0 | 0 | 1 | 0 | R | 4 |
| MAP11 | 354/364/370 | 53490/60410/67210 | 79 | 37 | 244x20 | 12 | 0 | 2 | 0 | 0 | 1 | 0 | R | 15 |
| MAP12 | 189/241/362 | 22850/32930/56610 | 29 | 2 | 219x3 221x1 267x13 | 8 | 0 | 0 | 0 | 0 | 1 | 0 | - | 7 |
| MAP13 | 138/182/240 | 35270/45720/60260 | 17 | 13 | 219x1 221x4 | 57 | 0 | 16 | 0 | 0 | 2 | 0 | B | 3 |
| MAP14 | 265/342/414 | 59310/76660/95980 | 509 | 30 | 153x4 244x10 | 706 | 0 | 102 | 0 | 0 | 2 | 0 | BYR | 66 |
| MAP15 | 325/367/383 | 86790/116300/125920 | 132 | 9 | - | 2 | 0 | 12 | 0 | 0 | 1 | 0 | BYR | 19 |
| MAP16 | 181/238/292 | 28140/36670/43590 | 19 | 3 | 159x1 219x3 221x7 | 3 | 0 | 0 | 0 | 0 | 2 | 0 | BYR | 7 |
| MAP17 | 184/252/356 | 29100/42660/70250 | 67 | 12 | 153x10 166x1 181x1 | 11 | 0 | 0 | 0 | 0 | 12 | 0 | BYR | 12 |
| MAP18 | 305/385/471 | 30520/42210/55440 | 179 | 22 | 244x3 | 8 | 0 | 26 | 0 | 0 | 7 | 0 | BYR | 16 |
| MAP19 | 173/208/260 | 27240/34540/43040 | 16 | 1 | 221x2 244x1 | 2 | 0 | 1 | 0 | 0 | 5 | 0 | BYR | 6 |
| MAP20 | 180/244/331 | 50460/66080/84980 | 24 | 12 | 219x8 221x2 | 2 | 0 | 27 | 0 | 0 | 1 | 0 | BYR | 7 |
| MAP21 | 136/172/220 | 19300/23810/31020 | 58 | 1 | 154x9 219x8 221x1 244x2 | 2 | 4 | 11 | 0 | 0 | 1 | 0 | YR | 10 |
| MAP22 | 152/213/290 | 30050/43430/60400 | 118 | 47 | 219x8 221x1 244x8 | 6 | 2 | 6 | 0 | 0 | 2 | 3 | BYR | 15 |
| MAP23 | 357/411/460 | 50650/61370/73340 | 91 | 20 | - | 1 | 1 | 11 | 0 | 0 | 1 | 0 | BYR | 13 |
| MAP24 | 368/479/585 | 5989571/6009271/6029161 | 83 | 29 | 244x3 | 7 | 0 | 5 | 0 | 0 | 1 | 5 | BYR | 11 |
| MAP25 | 44/67/88 | 10830/13510/16010 | 83 | 0 | 219x4 | 3 | 0 | 3 | 0 | 0 | 1 | 0 | - | 3 |
| MAP26 | 1666/1731/1825 | 429680/445390/474790 | 268 | 20 | 219x9 221x2 244x6 | 23 | 0 | 16 | 0 | 0 | 3 | 103 | BR | 27 |
| MAP27 | 195/244/313 | 51570/68140/93290 | 57 | 25 | 219x1 | 9 | 0 | 6 | 0 | 0 | 1 | 0 | BYR | 12 |
| MAP28 | 130/178/250 | 22930/33760/50050 | 81 | 39 | 221x3 | 2 | 0 | 19 | 0 | 0 | 25 | 0 | BY | 6 |
| MAP29 | 153/211/293 | 57660/75010/106800 | 15 | 5 | 219x2 221x3 244x2 | 3 | 0 | 11 | 0 | 0 | 2 | 0 | BYR | 12 |
| MAP30 | 28/38/50 | 14800/20000/26000 | 56 | 1 | 157x3 220x2 | 5 | 0 | 22 | 0 | 0 | 0 | 0 | - | 11 |
| MAP31 | 280/345/439 | 28800/38130/51080 | 75 | 21 | 153x8 200x2 219x4 244x6 | 24 | 0 | 13 | 0 | 0 | 3 | 23 | BYR | 7 |
| MAP32 | 1483/1785/2047 | 207840/263590/312290 | 214 | 31 | 153x1 157x14 239x1 244x8 | 179 | 1 | 667 | 0 | 0 | 1 | 40 | BR | 42 |
| MAP33 | 2/2/2 | 300/300/300 | 92 | 1 | 243x6 244x46 263x59 | 41 | 0 | 3 | 0 | 0 | 2 | 2131 | - | 8 |
| MAP34 | 148/204/247 | 27080/46860/55700 | 69 | 21 | 265x29 | 9 | 0 | 25 | 0 | 0 | 17 | 0 | BYR | 9 |
| MAP35 | 121/175/229 | 27130/38190/47030 | 9 | 3 | 219x2 221x9 244x117 | 13 | 0 | 0 | 0 | 0 | 2 | 0 | Y | 10 |
| MAP36 | 3441/3664/3899 | 300630/347190/395540 | 167 | 18 | 159x3 166x1 219x27 220x24 221x3 222x3 244x34 263x1 267x14 | 108 | 0 | 30 | 0 | 0 | 1 | 0 | BYR | 46 |
| MAP37 | 0/0/0 | 0/0/0 | 80 | 46 | 244x14 | 1 | 1 | 5 | 0 | 0 | 1 | 0 | - | 18 |

"Death/exit sectors" here are all MBF21 instant-death floors (alternate damage modes 0 and
1): no map uses the kill-and-exit modes. MAP01, MAP30, MAP33 and MAP37 are story or boss maps.

| map | monsters e/h/u | health e/h/u | arch-viles | cyberdemons | masterminds | revenants | pain elementals | chaingunners | custom (not vanilla) |
|---|---|---|---|---|---|---|---|---|---|
| MAP01 | 1/1/1 | 400/400/400 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 1/1/1 |
| MAP02 | 76/94/120 | 4270/5710/6890 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 2/3/3 |
| MAP03 | 86/112/154 | 9460/10870/14140 | 0/0/0 | 0/0/0 | 0/0/0 | 0/1/2 | 1/1/1 | 4/5/10 | 9/15/23 |
| MAP04 | 120/149/193 | 19850/23300/27550 | 2/2/2 | 0/0/0 | 0/0/0 | 8/9/10 | 3/4/4 | 6/7/8 | 13/15/20 |
| MAP05 | 199/264/343 | 27310/41280/49990 | 2/3/4 | 0/2/2 | 0/0/0 | 13/19/26 | 0/0/0 | 5/7/8 | 37/51/56 |
| MAP06 | 65/84/109 | 7120/9150/12990 | 1/1/2 | 0/0/0 | 0/0/0 | 4/6/7 | 0/0/1 | 3/4/5 | 5/9/15 |
| MAP07 | 366/383/401 | 42520/45400/50720 | 1/1/1 | 0/0/0 | 0/0/0 | 16/16/20 | 0/1/2 | 9/11/12 | 29/34/40 |
| MAP08 | 136/195/230 | 25810/45750/58910 | 0/2/5 | 0/0/0 | 0/0/0 | 5/20/25 | 0/1/1 | 2/7/11 | 11/23/38 |
| MAP09 | 222/296/394 | 33640/44430/64780 | 3/3/4 | 0/0/0 | 0/0/0 | 18/27/36 | 0/1/1 | 7/9/11 | 8/14/22 |
| MAP10 | 195/237/288 | 22910/28970/39650 | 0/0/0 | 0/0/1 | 0/0/0 | 1/2/6 | 0/0/0 | 1/4/4 | 13/16/20 |
| MAP11 | 354/364/370 | 53490/60410/67210 | 1/2/4 | 0/0/0 | 0/0/0 | 22/23/21 | 0/0/1 | 0/0/0 | 46/60/69 |
| MAP12 | 189/241/362 | 22850/32930/56610 | 1/1/1 | 0/1/3 | 0/0/0 | 6/8/14 | 0/0/0 | 4/7/10 | 9/12/18 |
| MAP13 | 138/182/240 | 35270/45720/60260 | 2/3/5 | 0/0/0 | 0/0/0 | 30/39/47 | 0/0/0 | 9/13/19 | 29/41/52 |
| MAP14 | 265/342/414 | 59310/76660/95980 | 4/7/10 | 1/1/1 | 0/0/0 | 22/34/41 | 0/4/7 | 5/6/7 | 50/58/73 |
| MAP15 | 325/367/383 | 86790/116300/125920 | 4/4/5 | 1/3/4 | 2/2/2 | 7/24/30 | 5/9/11 | 2/1/1 | 123/133/139 |
| MAP16 | 181/238/292 | 28140/36670/43590 | 2/2/3 | 1/1/1 | 0/0/0 | 4/7/9 | 0/0/1 | 4/10/16 | 25/29/37 |
| MAP17 | 184/252/356 | 29100/42660/70250 | 1/1/1 | 0/0/1 | 0/0/0 | 13/21/35 | 0/1/2 | 5/8/14 | 28/45/78 |
| MAP18 | 305/385/471 | 30520/42210/55440 | 0/0/1 | 0/0/0 | 0/0/0 | 4/14/21 | 2/3/3 | 5/6/7 | 89/125/166 |
| MAP19 | 173/208/260 | 27240/34540/43040 | 2/3/5 | 0/0/0 | 0/0/0 | 9/12/14 | 1/1/1 | 21/23/25 | 37/34/42 |
| MAP20 | 180/244/331 | 50460/66080/84980 | 0/1/2 | 1/1/1 | 0/0/0 | 23/38/54 | 0/0/0 | 14/18/25 | 65/81/105 |
| MAP21 | 136/172/220 | 19300/23810/31020 | 1/2/2 | 0/0/0 | 0/0/0 | 5/8/14 | 0/0/0 | 8/13/16 | 10/14/18 |
| MAP22 | 152/213/290 | 30050/43430/60400 | 0/2/4 | 1/1/1 | 0/0/0 | 6/8/11 | 1/1/1 | 4/6/7 | 25/52/86 |
| MAP23 | 357/411/460 | 50650/61370/73340 | 3/3/4 | 0/0/0 | 0/0/0 | 16/21/36 | 1/2/3 | 13/18/29 | 62/95/145 |
| MAP24 | 368/479/585 | 5989571/6009271/6029161 | 4/6/10 | 3/3/3 | 0/0/0 | 37/47/56 | 7/11/13 | 4/12/13 | 147/192/226 |
| MAP25 | 44/67/88 | 10830/13510/16010 | 1/1/1 | 0/0/0 | 0/0/0 | 3/5/7 | 0/0/0 | 2/2/3 | 3/3/4 |
| MAP26 | 1666/1731/1825 | 429680/445390/474790 | 11/14/18 | 6/6/6 | 0/0/0 | 79/78/72 | 0/0/0 | 0/1/1 | 329/365/473 |
| MAP27 | 195/244/313 | 51570/68140/93290 | 4/4/5 | 0/0/0 | 0/0/0 | 9/12/13 | 1/3/6 | 10/13/16 | 82/105/142 |
| MAP28 | 130/178/250 | 22930/33760/50050 | 2/3/4 | 0/0/0 | 0/0/0 | 13/20/30 | 0/0/0 | 4/4/5 | 50/73/104 |
| MAP29 | 153/211/293 | 57660/75010/106800 | 0/1/3 | 2/2/3 | 0/0/0 | 22/28/35 | 0/0/0 | 1/1/1 | 24/42/68 |
| MAP30 | 28/38/50 | 14800/20000/26000 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 28/38/50 |
| MAP31 | 280/345/439 | 28800/38130/51080 | 0/1/2 | 0/0/0 | 0/0/0 | 12/15/23 | 0/0/1 | 11/12/18 | 35/44/64 |
| MAP32 | 1483/1785/2047 | 207840/263590/312290 | 26/26/26 | 4/4/4 | 1/1/1 | 57/95/116 | 1/1/1 | 12/12/17 | 70/109/126 |
| MAP33 | 2/2/2 | 300/300/300 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 |
| MAP34 | 148/204/247 | 27080/46860/55700 | 3/4/5 | 0/0/0 | 0/0/0 | 3/11/14 | 0/0/1 | 0/0/1 | 76/102/130 |
| MAP35 | 121/175/229 | 27130/38190/47030 | 5/7/9 | 1/1/1 | 0/0/0 | 10/12/15 | 0/0/0 | 12/16/26 | 18/28/43 |
| MAP36 | 3441/3664/3899 | 300630/347190/395540 | 41/55/70 | 6/6/6 | 2/2/2 | 85/103/128 | 2/6/9 | 16/16/16 | 107/149/190 |
| MAP37 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 |

MAP24's health is its 99,999-point cyberdemon variant: not meant to be killed.

### Totals

**lines: modelled: 15648 lines, 74 kinds**

```
   11732  97 teleport [WR]
     906  207 silent teleport [W1]
     612  38 floor lower to lowest [W1]
     546  39 teleport [W1]
     201  109 blazing door open stay [W1]
     171  23 floor lower to lowest [S1]
     152  19 floor lower to highest neighbor [W1]
     145  52 exit level [W1]
     139  208 silent teleport [WR]
     127  130 floor turbo raise to next higher [W1]
      91  123 blazing lift [SR]
      48  8 build stairs 8 [W1]
      47  62 lift [SR]
      44  1 door open-wait-close [DR]
      42  133 blue blazing door open stay [S1]
      41  33 red door open stay [D1]
      39  2 door open stay [W1]
      39  129 floor turbo raise to next higher [WR]
      36  6 fast crusher [W1]
      29  34 yellow door open stay [D1]
      28  137 yellow blazing door open stay [S1]
      27  135 red blazing door open stay [S1]
      27  112 blazing door open stay [S1]
      27  36 floor turbo lower to highest neighbor + 8 [W1]
      24  120 blazing lift [WR]
      24  31 door open stay [D1]
      24  128 floor raise to next higher [WR]
      22  83 floor lower to highest neighbor [WR]
      21  106 blazing door open stay [WR]
      20  32 blue door open stay [D1]
      19  82 floor lower to lowest [WR]
      17  119 floor raise to next higher [W1]
      16  99 blue blazing door open stay [SR]
      14  103 door open stay [S1]
      12  91 floor raise to lowest ceiling [WR]
      10  124 secret exit [W1]
       9  5 floor raise to lowest ceiling [W1]
       9  40 ceiling raise to highest [W1]
       7  37 floor lower to lowest, change texture [W1]
       7  88 lift [WR]
       7  101 floor raise to lowest ceiling [S1]
       6  110 blazing door close [W1]
       6  3 door close [W1]
       5  195 teleport [SR]
       5  118 blazing door open stay [D1]
       5  115 blazing door open stay [SR]
       4  61 door open stay [SR]
       4  22 floor raise to next higher, change texture [W1]
       4  25 slow crusher [W1]
       4  117 blazing door open-wait-close [DR]
       4  53 perpetual lift start [W1]
       3  73 slow crusher [WR]
       3  71 floor turbo lower to highest neighbor + 8 [S1]
       3  102 floor lower to highest neighbor [S1]
       3  18 floor raise to next higher [S1]
       3  98 floor turbo lower to highest neighbor + 8 [WR]
       3  121 blazing lift [W1]
       3  69 floor raise to next higher [SR]
       2  11 exit level [S1]
       2  122 blazing lift [S1]
       2  127 turbo build stairs 16 [S1]
       2  107 blazing door close [WR]
       2  105 blazing door open-wait-close [WR]
       2  47 floor raise to next higher, change texture [G1]
       1  60 floor lower to lowest [SR]
       1  59 floor raise 24, change texture [W1]
       1  4 door open-wait-close [W1]
       1  141 silent crusher [W1]
       1  134 red blazing door open stay [SR]
       1  41 ceiling lower to floor [S1]
       1  58 floor raise 24 [W1]
       1  100 turbo build stairs 16 [W1]
       1  63 door open-wait-close [SR]
       1  86 door open stay [WR]
```

**lines: known-na: 13995 lines, 9 kinds**

```
   11830  126 teleport (monsters only) [WR]
    1337  48 scroll wall left [None]
     515  81 light to 255 [WR]
     180  12 light to brightest neighbor [W1]
      58  13 light to 255 [W1]
      28  104 light to darkest neighbor [W1]
      24  125 teleport (monsters only) [W1]
      12  35 light to 35 [W1]
      11  80 light to brightest neighbor [WR]
```

**lines: named-no-heights: 3254 lines, 26 kinds**

```
    1749  gen floor W1
     484  gen floor WR
     213  gen lift SR
     191  gen ceiling W1
     112  gen floor S1
      86  gen lift WR
      82  gen door W1
      55  gen ceiling WR
      49  gen locked door S1
      44  gen door WR
      34  gen door DR
      33  gen ceiling S1
      25  gen floor G1
      20  gen locked door DR
      17  gen door S1
      16  gen floor SR
      16  gen lift W1
      10  gen locked door D1
       5  gen door G1
       3  gen lift GR
       3  gen locked door SR
       2  gen door SR
       2  gen door GR
       1  gen stairs W1
       1  gen ceiling SR
       1  gen stairs G1
```

**lines: named-no-model: 292 lines, 2 kinds**

```
     286  244 silent line-to-line teleport [WR]
       6  243 silent line-to-line teleport [W1]
```

**lines: unclassified: 9906 lines, 42 kinds**

```
    2307  271 Property Transfer: Transfer sky texture to tagged sectors [--]
    1622  242 Property Transfer: Set Tagged Lighting, Flats Heights to 1st Sidedef's Sector, [--]
    1404  255 Scrollers: scroll wall by sidedef offsets [--]
    1365  252 Scrollers: CARRY objects on tagged floor [--]
     614  269 Teleport Linedef types: Yes Yes No Set TP thing in tagged sector [WR]
     483  260 Property Transfer: Make Tagged Lines (or this line if tag==0) Translucent [--]
     443  85 Scrollers: wall scrolls right [--]
     423  213 Property Transfer: Set Tagged Floor Lighting to Lighting on 1st Sidedef's Sector [--]
     364  261 Property Transfer: Set Tagged Ceiling Lighting to Lighting on 1st Sidedef's Sector [--]
     145  253 Scrollers: scroll tagged floor and CARRY objects [--]
     139  216 Scrollers: accel: CARRY objects [--]
     109  219 Floor linedef types: Dn Slow None -- No No Next Neighbor Floor [W1]
      60  263 Teleport Linedef types: Yes Yes Yes Preserve Line with same tag (reversed) [WR]
      54  265 Teleport Linedef types: Yes Yes No Preserve Line with same tag (reversed) [WR]
      49  221 Floor linedef types: Dn Slow None -- No No Next Neighbor Floor [S1]
      40  268 Teleport Linedef types: Yes Yes No Set TP thing in tagged sector [W1]
      30  217 Scrollers: accel: floor and CARRY [--]
      29  267 Teleport Linedef types: Yes Yes No Preserve Line with same tag [WR]
      27  1024 Scrollers: MBF21 wall scroll by tag [--]
      26  220 Floor linedef types: Dn Slow None -- No No Next Neighbor Floor [WR]
      23  153 Floor linedef types: -- ---- TxTy Trg No No None [W1]
      17  157 Lighting Linedef Types: Minimum Neighbor [WR]
      15  254 Scrollers: scroll tagged wall [--]
      15  215 Scrollers: accel: floor [--]
      12  272  [passive]
      12  218 Scrollers: accel: wall [--]
      10  159 Floor linedef types: Dn Slow TxTy Num No No Lowest Neighbor Floor [S1]
       9  154 Floor linedef types: -- ---- TxTy Trg No No None [WR]
       9  223 Property Transfer: Length Sets Friction in tagged Sector,Sludge<100, Ice>100 [--]
       9  246 Scrollers: displacement: floor [--]
       8  247 Scrollers: displacement: CARRY objects [--]
       8  224 Property Transfer: Length/Direction Sets Wind Force in tagged Sectors [--]
       6  249 Scrollers: displacement: wall [--]
       5  245 Scrollers: displacement: ceiling [--]
       3  166 Ceiling linedef types: Up Slow None -- No No Highest Neighbor Ceiling [S1]
       3  251 Scrollers: scroll tagged floor [--]
       3  222 Floor linedef types: Dn Slow None -- No No Next Neighbor Floor [SR]
       2  200 Ceiling linedef types: Dn Slow None -- No No Highest Neighbor Floor [W1]
       1  226 Property Transfer: Length Sets Point Source Wind/Current Force in Tagged Sectors [--]
       1  248 Scrollers: displacement: floor and CARRY [--]
       1  181 Platform Linedef types: 3s Slow None -- No Lowest and Highest Floor (perpetual) [SR]
       1  239 Floor linedef types: -- ---- TxTy Num No No None [W1]
```

**line flags past the vanilla ones**

```
    6740  0x2000
    5697  0x1000
      40  0x200
```

**sector specials**

```
   12353  vanilla 8
    2159  MBF21 death mode 1
     981  vanilla 7
     686  vanilla 1
     586  vanilla 17
     572  vanilla 5
     404  vanilla 16
     399  damage 10
     296  light 8 + damage 10
     233  friction
     122  vanilla 9
     105  light 8 + damage 5 + push
     102  light 8 + damage 5
      78  light 7 + MBF21 death mode 1
      67  MBF21 kill monsters
      28  push
      27  light 8 + damage 20
      24  light 8 + MBF21 death mode 1
      17  light 8 + MBF21 death mode 0
      16  light 8 + push
      12  vanilla 12
      12  MBF21 death mode 0
       9  MBF21 death mode 0 + MBF21 kill monsters
       7  vanilla 13
       6  MBF21 death mode 1 + MBF21 kill monsters
       5  vanilla 2
       4  secret
       4  damage 5
       2  light 8 + secret
       1  light 30 + damage 20
       1  vanilla 3
       1  light 17 + secret
       1  damage 10 + secret
```

**use lines by look (kind, looks interactive by prefix)**

```
    1018  ('switch', False)
     231  ('door', False)
```

**floor flats of damaging sectors (flat, bot takes it for a liquid)**

```
     898  ('OSLUDG01', False)
     606  ('ONUKEA01', False)
     412  ('ONUKEC01', False)
     387  ('OICYWA01', False)
     257  ('OLAVAE01', False)
     118  ('OLAVAC01', False)
      65  ('OLAVAA02', False)
      30  ('OBRCKB31', False)
      29  ('OROCKO01', False)
      24  ('OCONCS00', False)
      22  ('OBRCKK04', False)
       6  ('OTL08A06', False)
       5  ('OBKMTD11', False)
       3  ('OCONCL01', False)
       3  ('OWOODD35', False)
       3  ('OSNOWB01', False)
       3  ('OTL08A14', False)
       3  ('OSOLID01', False)
       2  ('OTAR__01', False)
       2  ('OTLPTA31', False)
       2  ('OMRBLM00', False)
       1  ('OHELLA01', False)
       1  ('OTL16B12', False)
       1  ('OBKMTD00', False)
       1  ('OTLMXB02', False)
       1  ('OBLODA01', False)
       1  ('OCONCD01', False)
       1  ('OROCKD02', False)
       1  ('OGRSSA01', False)
       1  ('OHERRA04', False)
       1  ('OMRBLA00', False)
       1  ('OBRCKB32', False)
       1  ('OLAVAD01', False)
```

**floor flats of MBF21 death sectors**

```
    1870  OBKMTB02
     261  OLAVAD01
     102  ONUKEA01
      25  ZWATER01
      12  OTL32B04
      12  ONDSTF44
       7  OTL16B06
       5  OWATER01
       4  OWRNGC03
       3  OATRBBMX
       2  OSNOWB01
       1  ESNOWA01
       1  OSOLID01
```

Not one of its 1,018 switch lines or 231 use-doors has a texture `LOOKS_INTERACTIVE` knows.
The switch faces most used: `OSWTCHA1` 65, `OSWTCHI2` 58, `OSWTCHF0` 55, `OSWTCHM1` 53,
`ESWTCHM0` 52, `OSWTCHK0` 50, `OKEYSB01` 48, `OSWTCHN0` 46.

**Generalized lines, decoded:**

```
     663  floor down to lowest neighbour floor
     303  lift to lowest neighbour floor
     213  floor up to lowest neighbour floor
     140  floor down to highest neighbour floor
     138  floor up to highest neighbour floor
     138  ceiling up to highest neighbour ceiling
     134  floor up by 32
     117  floor down to lowest neighbour floor, change: texture
     115  floor down to lowest neighbour ceiling
     107  door open stay
     106  floor down to next neighbour floor
      87  floor up to next neighbour floor, change: texture+type, crush
      85  floor down to lowest neighbour floor, change: texture+type
      75  door open-wait-close
      68  floor up by shortest lower texture
      67  floor up to lowest neighbour ceiling, change: texture
      63  floor up to lowest neighbour ceiling
      46  floor up to next neighbour floor
      43  ceiling down to highest neighbour ceiling
      33  floor up to lowest neighbour floor, change: texture
      31  floor down to lowest neighbour ceiling, change: zero type
      30  floor down to highest neighbour floor, change: texture
      27  floor down to next neighbour floor, change: texture
      24  floor up to next neighbour floor, change: texture
      23  floor down by 32
      22  floor down to lowest neighbour ceiling, change: texture
      19  ceiling up to lowest neighbour ceiling
      18  floor up to next neighbour floor, change: texture+type
      17  floor down to lowest neighbour ceiling, change: texture+type
      16  floor down to lowest neighbour floor, change: zero type
      15  ceiling down to lowest neighbour ceiling
      13  floor up to highest neighbour floor, change: texture
      13  ceiling up to next neighbour ceiling
      13  floor up to highest neighbour floor, change: zero type
      12  locked door red skull open stay
      12  ceiling down to floor
      12  ceiling up by 32
      11  locked door blue skull open stay
       9  floor up to ceiling
       9  lift to next lowest floor
       9  locked door any open stay
       8  locked door yellow skull open stay
       8  floor up to ceiling, crush
       8  locked door red card open stay
       8  ceiling down to next neighbour ceiling
       7  floor down by shortest lower texture
       6  locked door red card open-wait-close
       6  floor down to next neighbour floor, change: texture+type
       6  floor down to lowest neighbour floor, crush
       5  floor up to next neighbour floor, change: zero type
       5  floor up to lowest neighbour ceiling, change: texture+type
       5  locked door yellow card open stay
       4  locked door all (card or skull) open stay
       4  door close
       4  floor up to lowest neighbour floor, change: texture+type
       4  locked door blue card open stay
       4  floor up by 32, change: zero type
       4  floor up to lowest neighbour ceiling, crush
       3  floor up to highest neighbour floor, change: texture, crush
       3  lift to lowest neighbour ceiling
       3  lift perpetual lowest-highest
       2  ceiling down to lowest neighbour ceiling, change: texture
       2  ceiling down to next neighbour ceiling, change: texture
       2  floor up by 32, change: texture
       2  stairs up step 16
       2  locked door any (card or skull) open stay
       2  locked door red skull (card or skull) open stay
       2  ceiling up to highest neighbour ceiling, change: zero type
       2  locked door blue skull (card or skull) open stay
       2  locked door yellow skull (card or skull) open stay
       2  ceiling up to floor
       2  locked door all open stay
       2  ceiling up to highest neighbour ceiling, change: texture
       2  floor down by 32, change: zero type
       2  ceiling up by 24
       2  ceiling down by 24
       2  locked door blue card open-wait-close
       2  floor up to lowest neighbour ceiling, change: zero type
       2  ceiling down by 32
       2  locked door yellow skull open-wait-close
       1  ceiling up to next neighbour ceiling, change: texture
       1  floor down by 24
       1  floor up by 24, change: texture
       1  floor up by 24
       1  floor down to next neighbour floor, crush
       1  floor up to next neighbour floor, crush
       1  floor up to lowest neighbour floor, crush
       1  floor down to highest neighbour floor, change: zero type
       1  floor down by 24, change: zero type
       1  floor down to highest neighbour floor, change: texture+type
       1  locked door blue card (card or skull) open stay
       1  ceiling down to floor, crush
       1  floor up to highest neighbour floor, change: texture+type
       1  ceiling down to lowest neighbour ceiling, change: texture+type, crush
       1  ceiling up to highest neighbour floor
```

**Thing types:** vanilla's, Boom's pull point (5002, once), and 230 new: 8001-8235 (its
monsters, decorations and effects) and 14101-14137.

**Floors that carry**, by what stands on them at the start:

```
MAP01: 270 empty, small, 118 empty, large, 15 doll, 13 items only
MAP02: 82 empty, small, 8 doll, 1 empty, large
MAP03: 7 doll, 1 items only
MAP04: 7 doll
MAP05: 5 doll, 1 items only
MAP06: 21 monster, 3 items only, 1 doll
MAP07: 14 doll, 7 monster, 4 empty, small
MAP08: 10 doll, 2 monster, 1 empty, large
MAP09: 13 empty, large, 7 empty, small, 6 doll, 2 items only, 1 monster
MAP10: 4 doll, 3 empty, small, 2 items only
MAP11: 21 empty, small, 15 doll, 1 monster, 1 empty, large
MAP12: 47 empty, small, 19 monster, 10 empty, large, 7 doll, 6 items only
MAP13: 51 empty, small, 5 monster, 3 doll
MAP14: 111 empty, small, 58 empty, large, 35 doll, 26 items only, 6 monster, 2 barrel, 1 REAL START
MAP15: 19 doll, 1 empty, large, 1 empty, small, 1 items only
MAP16: 4 doll, 1 items only, 1 empty, small
MAP17: 54 empty, small, 12 doll, 1 monster
MAP18: 17 empty, small, 16 doll, 4 items only, 1 empty, large
MAP19: 6 doll, 1 items only
MAP20: 6 doll, 1 items only
MAP21: 10 doll, 9 empty, small, 1 items only
MAP22: 15 doll, 8 empty, small, 8 monster, 3 items only, 2 empty, large
MAP23: 13 doll, 2 empty, small
MAP24: 10 doll, 2 monster, 1 items only, 1 empty, small
MAP25: 3 doll, 2 items only
MAP26: 28 empty, small, 27 doll, 13 empty, large, 3 items only
MAP27: 11 doll, 7 monster, 1 items only
MAP28: 6 doll, 1 items only, 1 monster
MAP29: 12 doll, 2 items only
MAP30: 10 doll, 2 empty, large, 2 monster
MAP31: 161 empty, small, 21 empty, large, 18 barrel, 6 doll, 4 monster, 1 REAL START, 1 items only
MAP32: 143 empty, small, 108 empty, large, 78 monster, 37 doll, 12 items only
MAP33: 2355 empty, small, 603 empty, large, 22 monster, 8 doll, 5 items only, 1 REAL START
MAP34: 29 monster, 9 doll
MAP35: 208 empty, small, 10 doll, 4 empty, large, 4 monster
MAP36: 46 doll, 39 items only, 23 monster, 10 empty, small, 8 empty, large
MAP37: 8 doll, 2 empty, large
TOTAL {'doll': 441, 'empty, large': 967, 'empty, small': 3594, 'items only': 133, 'monster': 243, 'barrel': 20, 'REAL START': 3}
```

She starts *on* a carrying floor in MAP14, MAP31 and MAP33.


## Memento Mori

Added 2026-11-20, the same census: an earlier, easier rung than Sunlust. The files were on
disk already (`wads/mm_allup/`): `MM.WAD` (7,825,282 bytes, dated 1996-02-10, sha256
`3e34522f61e40e50...`), `MMMUS.WAD` (35 lumps, all music), and text files (`MM.TXT`, `MMHELP.TXT`,
`AUTHORS.TXT`, `CHANGES.TXT`, `FILE_ID.DIZ`). No `.deh` or `.bex` in the folder.

**A vanilla WAD, by its lumps:** 450 lumps: 32 maps (MAP01-MAP32), `TEXTURE1` and `PNAMES`
(its own textures), a demo, graphics. No `COMPLVL`, `DEHACKED`, `UMAPINFO`, `MAPINFO`,
`SWITCHES`, `ANIMATED` or `OPTIONS`. Every line special is one of vanilla's 1-141; no
generalized line, no Boom type; no line flag past vanilla's; every sector special is vanilla's
(under 32); every thing type is vanilla's. It wants complevel 2; with no lump our engine runs
it at 21, as it runs DOOM II.

### Sizes and nodes

| map | nodes | lines | sides | sectors | things | segs | subsectors |
|---|---|---|---|---|---|---|---|
| MAP01 | vanilla | 646 | 998 | 135 | 118 | 1124 | 425 |
| MAP02 | vanilla | 1003 | 1436 | 171 | 222 | 1506 | 445 |
| MAP03 | vanilla | 521 | 785 | 114 | 105 | 852 | 329 |
| MAP04 | vanilla | 1531 | 2085 | 322 | 329 | 2192 | 696 |
| MAP05 | vanilla | 1484 | 2038 | 250 | 323 | 2216 | 807 |
| MAP06 | vanilla | 1646 | 2460 | 295 | 376 | 2530 | 938 |
| MAP07 | vanilla | 1678 | 2470 | 377 | 410 | 2641 | 874 |
| MAP08 | vanilla | 1235 | 1733 | 188 | 188 | 1988 | 804 |
| MAP09 | vanilla | 2225 | 1780 | 546 | 522 | 3536 | 1319 |
| MAP10 | vanilla | 1595 | 2162 | 289 | 393 | 2549 | 997 |
| MAP11 | vanilla | 1491 | 2006 | 267 | 306 | 2166 | 864 |
| MAP12 | vanilla | 1247 | 1910 | 226 | 229 | 2038 | 779 |
| MAP13 | vanilla | 2444 | 2001 | 473 | 503 | 4178 | 1585 |
| MAP14 | vanilla | 1772 | 2472 | 305 | 198 | 2754 | 1087 |
| MAP15 | vanilla | 2864 | 4315 | 449 | 701 | 5024 | 1899 |
| MAP16 | vanilla | 2215 | 3350 | 450 | 725 | 3629 | 1333 |
| MAP17 | vanilla | 1521 | 2406 | 368 | 373 | 2684 | 925 |
| MAP18 | vanilla | 2098 | 2978 | 441 | 443 | 3189 | 1103 |
| MAP19 | vanilla | 1418 | 1905 | 295 | 437 | 1992 | 732 |
| MAP20 | vanilla | 1802 | 2666 | 291 | 466 | 2983 | 1157 |
| MAP21 | vanilla | 2813 | 3788 | 444 | 773 | 4104 | 1321 |
| MAP22 | vanilla | 2499 | 2091 | 560 | 466 | 4349 | 1712 |
| MAP23 | vanilla | 770 | 1085 | 99 | 569 | 1176 | 429 |
| MAP24 | vanilla | 700 | 1056 | 119 | 464 | 1135 | 445 |
| MAP25 | vanilla | 1263 | 882 | 272 | 228 | 2183 | 1045 |
| MAP26 | vanilla | 1295 | 1958 | 281 | 474 | 2133 | 741 |
| MAP27 | vanilla | 1726 | 2499 | 310 | 409 | 2731 | 983 |
| MAP28 | vanilla | 2592 | 3481 | 530 | 666 | 3753 | 1271 |
| MAP29 | vanilla | 2661 | 2311 | 550 | 699 | 4217 | 1717 |
| MAP30 | vanilla | 1168 | 1756 | 112 | 475 | 1923 | 684 |
| MAP31 | vanilla | 2732 | 3874 | 510 | 704 | 4228 | 1618 |
| MAP32 | vanilla | 2445 | 3684 | 397 | 686 | 4036 | 1383 |

All vanilla nodes. The largest map is 2,864 lines (MAP15). Five maps (MAP09, 13, 22, 25, 29)
have fewer sidedefs than lines: packed sidedefs, shared between lines, which our reader takes
as they are (lines hold indices).

### Totals

**lines: modelled: 3506 lines, 96 kinds**

```
     460  97 teleport [WR]
     429  1 door open-wait-close [DR]
     307  62 lift [SR]
     241  123 blazing lift [SR]
     160  109 blazing door open stay [W1]
     114  38 floor lower to lowest [W1]
     109  23 floor lower to lowest [S1]
      94  120 blazing lift [WR]
      86  114 blazing door open-wait-close [SR]
      85  88 lift [WR]
      78  112 blazing door open stay [S1]
      68  117 blazing door open-wait-close [DR]
      68  52 exit level [W1]
      66  31 door open stay [D1]
      57  28 red door open-wait-close [DR]
      47  118 blazing door open stay [D1]
      46  135 red blazing door open stay [S1]
      45  26 blue door open-wait-close [DR]
      40  63 door open-wait-close [SR]
      39  119 floor raise to next higher [W1]
      38  83 floor lower to highest neighbor [WR]
      36  27 yellow door open-wait-close [DR]
      35  103 door open stay [S1]
      34  2 door open stay [W1]
      32  34 yellow door open stay [D1]
      31  19 floor lower to highest neighbor [W1]
      31  36 floor turbo lower to highest neighbor + 8 [W1]
      29  82 floor lower to lowest [WR]
      26  40 ceiling raise to highest [W1]
      26  37 floor lower to lowest, change texture [W1]
      23  32 blue door open stay [D1]
      22  6 fast crusher [W1]
      22  115 blazing door open stay [SR]
      21  102 floor lower to highest neighbor [S1]
      21  71 floor turbo lower to highest neighbor + 8 [S1]
      21  136 yellow blazing door open stay [SR]
      20  110 blazing door close [W1]
      20  99 blue blazing door open stay [SR]
      17  134 red blazing door open stay [SR]
      16  61 door open stay [SR]
      15  127 turbo build stairs 16 [S1]
      15  60 floor lower to lowest [SR]
      15  33 red door open stay [D1]
      15  11 exit level [S1]
      13  77 fast crusher [WR]
      13  20 floor raise to next higher, change texture [S1]
      13  87 perpetual lift start [WR]
      12  46 door open stay [GR]
      12  94 floor raise to lowest ceiling - 8, crush [WR]
      11  130 floor turbo raise to next higher [W1]
      11  106 blazing door open stay [WR]
      11  22 floor raise to next higher, change texture [W1]
      11  24 floor raise to lowest ceiling [G1]
      10  39 teleport [W1]
      10  45 floor lower to highest neighbor [SR]
      10  107 blazing door close [WR]
       9  5 floor raise to lowest ceiling [W1]
       9  18 floor raise to next higher [S1]
       8  105 blazing door open-wait-close [WR]
       8  124 secret exit [W1]
       8  30 floor raise by shortest lower texture [W1]
       8  98 floor turbo lower to highest neighbor + 8 [WR]
       7  16 door close-wait-open [W1]
       7  59 floor raise 24, change texture [W1]
       7  7 build stairs 8 [S1]
       6  75 door close [WR]
       6  133 blue blazing door open stay [S1]
       5  131 floor turbo raise to next higher [S1]
       5  141 silent crusher [W1]
       5  8 build stairs 8 [W1]
       4  53 perpetual lift start [W1]
       4  100 turbo build stairs 16 [W1]
       4  69 floor raise to next higher [SR]
       4  76 door close-wait-open [WR]
       3  67 floor raise 32, change texture [SR]
       3  73 slow crusher [WR]
       3  15 floor raise 24, change texture [S1]
       2  25 slow crusher [W1]
       2  66 floor raise 24, change texture [SR]
       2  42 door close [SR]
       2  111 blazing door open-wait-close [S1]
       2  116 blazing door close [SR]
       2  137 yellow blazing door open stay [S1]
       2  96 floor raise by shortest lower texture [WR]
       1  9 donut [S1]
       1  64 floor raise to lowest ceiling [SR]
       1  44 ceiling lower to 8 above floor [W1]
       1  47 floor raise to next higher, change texture [G1]
       1  95 floor raise to next higher, change texture [WR]
       1  3 door close [W1]
       1  91 floor raise to lowest ceiling [WR]
       1  84 floor lower to lowest, change texture [WR]
       1  128 floor raise to next higher [WR]
       1  49 slow crusher [S1]
       1  58 floor raise 24 [W1]
       1  41 ceiling lower to floor [S1]
```

**lines: known-na: 326 lines, 14 kinds**

```
     133  126 teleport (monsters only) [WR]
     106  48 scroll wall left [None]
      22  125 teleport (monsters only) [W1]
      12  74 crusher stop [WR]
      10  13 light to 255 [W1]
      10  35 light to 35 [W1]
       8  81 light to 255 [WR]
       7  57 crusher stop [W1]
       7  80 light to brightest neighbor [WR]
       4  12 light to brightest neighbor [W1]
       3  89 perpetual lift stop [WR]
       2  138 light to 255 [SR]
       1  104 light to darkest neighbor [W1]
       1  139 light to 35 [SR]
```

Lines named only, not triggers, or unclassified: **none.**

**sector specials**

```
     234  vanilla 8
     221  vanilla 7
     206  vanilla 17
     154  vanilla 9
      92  vanilla 1
      39  vanilla 12
      36  vanilla 5
      36  vanilla 16
      27  vanilla 3
      17  vanilla 2
      13  vanilla 4
       7  vanilla 10
       4  vanilla 13
```

Special 10 (a door that shuts 30 seconds into the level) is on 7 sectors: 1 on MAP17, 6 on
MAP25. Nothing in the bot models sector specials 10 or 14 (`movers.rs` has only the *line*
kinds 16 and 76): she plans those doors open till she sees them shut.

**use lines by look (kind, looks interactive by prefix)**

```
     804  ('switch', True)
     562  ('door', True)
     398  ('switch', False)
     256  ('door', False)
```

"Switch" here is any S1/SR line, 548 of them lifts worked from their own sides (62, 123),
which she tells by shape (`lift_face`), not texture; "door" is the D1/DR kind, told by shape
too (`door_shaped`). With no `SWITCHES` lump every real switch face is one of vanilla's
`SW1`/`SW2` names. The WAD's own door textures (`AWDOOR`, `AWDOOR2`, `AWDORRED`) don't start
with a prefix `LOOKS_INTERACTIVE` has.

**floor flats of damaging sectors (flat, bot takes it for a liquid)**

```
     206  ('NUKAGE1', True)
      35  ('LAVA1', True)
      16  ('SLIME01', True)
      11  ('BLOOD1', True)
       6  ('TLITE6_5', False)
       4  ('SLIME05', True)
       3  ('RROCK06', False)
       2  ('NUKAGE3', True)
       2  ('BLOOD2', True)
       2  ('DEM1_1', False)
       2  ('DEM1_2', False)
       2  ('DEM1_4', False)
       2  ('DEM1_3', False)
       2  ('DEM1_6', False)
       1  ('GRASS2', False)
       1  ('CEIL3_5', False)
       1  ('FLOOR6_1', False)
       1  ('LAVA3', True)
       1  ('FLAT20', False)
       1  ('NUKAGE2', True)
       1  ('SLIME09', True)
       1  ('GRASS1', False)
       1  ('CEIL5_2', False)
       1  ('FLAT10', False)
       1  ('FLOOR1_6', False)
```

### Monsters by skill

| map | monsters e/h/u | health e/h/u | arch-viles | cyberdemons | masterminds | revenants | pain elementals | chaingunners | custom (not vanilla) |
|---|---|---|---|---|---|---|---|---|---|
| MAP01 | 33/47/57 | 1440/2030/2440 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/1/2 | 0/0/0 |
| MAP02 | 42/63/75 | 1370/4620/6240 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 |
| MAP03 | 25/31/46 | 1260/1710/2810 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 1/1/4 | 0/0/0 |
| MAP04 | 63/106/123 | 6020/10840/19780 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 1/12/15 | 0/0/0 |
| MAP05 | 70/83/98 | 7490/10900/14570 | 0/0/0 | 0/0/0 | 0/0/0 | 1/2/2 | 0/0/1 | 3/5/8 | 0/0/0 |
| MAP06 | 104/147/165 | 5390/12540/20090 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 8/19/34 | 0/0/0 |
| MAP07 | 65/103/130 | 7810/17950/24500 | 0/2/2 | 0/0/0 | 0/0/0 | 2/5/6 | 0/0/0 | 3/10/11 | 0/0/0 |
| MAP08 | 36/48/54 | 4720/10250/11400 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 |
| MAP09 | 177/193/241 | 30520/32610/38860 | 0/0/0 | 0/0/0 | 0/0/0 | 9/9/10 | 0/0/1 | 9/9/11 | 0/0/0 |
| MAP10 | 49/78/86 | 6660/13520/14780 | 0/0/0 | 0/0/0 | 0/0/0 | 0/2/2 | 0/1/1 | 4/13/15 | 0/0/0 |
| MAP11 | 78/106/130 | 7820/11030/12790 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 2/4/5 | 0/0/0 |
| MAP12 | 58/81/86 | 6450/12340/17780 | 1/1/1 | 0/0/0 | 0/0/0 | 3/7/7 | 0/0/0 | 5/8/7 | 0/0/0 |
| MAP13 | 97/133/167 | 14040/21540/26510 | 1/1/2 | 0/0/0 | 0/0/0 | 4/7/7 | 0/0/0 | 12/21/26 | 0/0/0 |
| MAP14 | 89/89/89 | 22190/22190/22190 | 1/1/1 | 1/1/1 | 0/0/0 | 10/10/10 | 1/1/1 | 7/7/7 | 0/0/0 |
| MAP15 | 198/297/365 | 31170/50360/66540 | 0/1/1 | 0/0/1 | 0/0/0 | 10/14/17 | 0/0/1 | 10/22/29 | 0/0/0 |
| MAP16 | 136/169/197 | 27550/35990/42720 | 2/2/2 | 0/0/0 | 0/0/0 | 4/7/7 | 1/1/2 | 18/22/29 | 0/0/0 |
| MAP17 | 63/81/96 | 7380/14220/24700 | 0/0/3 | 0/0/0 | 0/0/0 | 3/5/7 | 1/1/1 | 9/11/13 | 0/0/0 |
| MAP18 | 132/175/200 | 29310/40640/53540 | 2/4/8 | 0/0/0 | 0/0/0 | 8/8/11 | 0/0/1 | 33/49/53 | 0/0/0 |
| MAP19 | 126/139/173 | 10440/12900/15800 | 0/0/0 | 0/0/0 | 0/0/0 | 2/1/2 | 0/0/0 | 3/5/10 | 0/0/0 |
| MAP20 | 164/169/187 | 13480/18710/21780 | 1/2/2 | 0/1/1 | 0/0/0 | 2/5/6 | 1/1/1 | 7/7/9 | 0/0/0 |
| MAP21 | 178/276/340 | 19670/32390/42330 | 0/0/0 | 0/0/0 | 0/0/0 | 0/1/6 | 0/2/6 | 18/35/43 | 0/0/0 |
| MAP22 | 113/153/192 | 17200/21150/23980 | 0/0/0 | 0/0/0 | 2/2/2 | 1/2/3 | 0/0/0 | 17/25/32 | 0/0/0 |
| MAP23 | 302/302/303 | 123970/123970/126970 | 28/28/28 | 2/2/2 | 0/0/1 | 34/34/34 | 8/8/8 | 4/4/4 | 0/0/0 |
| MAP24 | 102/115/132 | 30810/34600/39910 | 1/2/2 | 1/1/1 | 0/0/0 | 14/14/15 | 4/6/6 | 13/15/20 | 0/0/0 |
| MAP25 | 48/89/110 | 8840/14830/18970 | 0/0/1 | 0/0/0 | 1/1/1 | 1/2/5 | 0/0/1 | 3/10/19 | 0/0/0 |
| MAP26 | 94/135/184 | 40790/50690/63220 | 2/2/2 | 1/1/1 | 5/5/5 | 2/2/2 | 0/0/0 | 0/0/0 | 0/0/0 |
| MAP27 | 95/142/162 | 9640/18430/22500 | 0/1/2 | 0/1/1 | 0/0/0 | 1/4/5 | 1/1/2 | 5/7/7 | 0/0/0 |
| MAP28 | 175/250/279 | 31500/62390/83620 | 4/8/14 | 0/0/3 | 0/0/0 | 1/11/12 | 2/2/2 | 11/36/45 | 0/0/0 |
| MAP29 | 250/273/347 | 38760/42980/56650 | 1/1/1 | 0/0/0 | 0/0/0 | 3/3/4 | 0/1/3 | 42/44/50 | 0/0/0 |
| MAP30 | 125/125/125 | 26120/26120/26120 | 4/4/4 | 1/1/1 | 2/2/2 | 9/9/9 | 0/0/0 | 0/0/0 | 0/0/0 |
| MAP31 | 179/261/351 | 17120/24760/34860 | 0/0/0 | 0/0/0 | 0/0/0 | 2/2/2 | 0/0/0 | 25/36/49 | 0/0/0 |
| MAP32 | 106/151/205 | 5580/17870/31190 | 0/0/1 | 0/0/0 | 0/0/0 | 0/2/9 | 0/0/2 | 4/16/22 | 0/0/0 |

### Old tricks, as far as lumps show

| map | keys | P1 starts | exit lines | secret exit lines | sectors tagged 666 | deaf monsters | of monsters | monsters' teleport lines | teleport lines | crusher lines | crushing floors | shut-30s-open lines | sectors shut at 30 s | gun lines | stair lines | perpetual lifts | lines within one sector | lower texture missing | co-op-only monsters | co-op-only items |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MAP01 | - | 1 | 1 | 0 | 0 | 18 | 64 | 4 | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| MAP02 | BsRsYs | 1 | 1 | 0 | 0 | 9 | 107 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 9 | 0 | 30 | 12 |
| MAP03 | BcRcYc | 1 | 1 | 0 | 0 | 51 | 51 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 | 0 | 0 | 12 |
| MAP04 | BcRcYc | 1 | 1 | 0 | 0 | 37 | 145 | 0 | 16 | 0 | 0 | 0 | 0 | 0 | 0 | 6 | 0 | 0 | 0 | 14 |
| MAP05 | BcRc | 1 | 8 | 0 | 0 | 103 | 118 | 0 | 10 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 24 |
| MAP06 | BcRcYc | 1 | 1 | 0 | 0 | 141 | 208 | 2 | 8 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 4 | 0 | 0 | 1 |
| MAP07 | BcRcYc | 1 | 1 | 0 | 1 | 22 | 138 | 0 | 10 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 15 | 0 | 1 | 14 |
| MAP08 | BsRs | 1 | 1 | 0 | 1 | 39 | 62 | 13 | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| MAP09 | BcRcYc | 1 | 4 | 0 | 0 | 56 | 241 | 25 | 19 | 6 | 0 | 0 | 0 | 2 | 2 | 4 | 41 | 3 | 0 | 3 |
| MAP10 | Bc | 1 | 4 | 0 | 0 | 18 | 88 | 0 | 19 | 0 | 0 | 0 | 0 | 0 | 1 | 1 | 8 | 0 | 48 | 44 |
| MAP11 | BcYc | 1 | 1 | 0 | 0 | 15 | 131 | 0 | 6 | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 5 | 0 | 0 | 0 |
| MAP12 | BcRcYc | 1 | 4 | 0 | 0 | 45 | 120 | 0 | 9 | 3 | 0 | 0 | 0 | 0 | 2 | 1 | 19 | 0 | 19 | 8 |
| MAP13 | BsRsYs | 2 | 1 | 0 | 0 | 89 | 177 | 1 | 0 | 0 | 0 | 1 | 0 | 10 | 2 | 0 | 7 | 3 | 0 | 0 |
| MAP14 | BsRs | 1 | 1 | 0 | 0 | 63 | 89 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 5 | 0 | 0 | 8 |
| MAP15 | BcRcYc | 1 | 4 | 4 | 0 | 168 | 379 | 0 | 62 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 | 0 | 17 |
| MAP16 | BcRcYc | 1 | 4 | 0 | 0 | 164 | 197 | 0 | 38 | 0 | 0 | 0 | 0 | 0 | 3 | 0 | 0 | 0 | 8 | 34 |
| MAP17 | BcRcYc | 1 | 1 | 0 | 0 | 93 | 139 | 12 | 6 | 0 | 0 | 0 | 1 | 1 | 2 | 0 | 0 | 0 | 0 | 6 |
| MAP18 | BsRsYc | 1 | 1 | 0 | 0 | 91 | 212 | 2 | 9 | 0 | 0 | 4 | 0 | 0 | 2 | 0 | 0 | 0 | 0 | 1 |
| MAP19 | BcYc | 1 | 1 | 0 | 0 | 112 | 193 | 0 | 10 | 1 | 0 | 0 | 0 | 0 | 3 | 0 | 0 | 0 | 0 | 43 |
| MAP20 | BcRcYc | 1 | 4 | 0 | 0 | 87 | 192 | 0 | 15 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | 0 | 0 | 15 |
| MAP21 | BcRcYc | 1 | 1 | 0 | 0 | 194 | 340 | 0 | 1 | 2 | 0 | 0 | 0 | 0 | 1 | 3 | 7 | 0 | 45 | 26 |
| MAP22 | BsRsYs | 1 | 4 | 0 | 0 | 108 | 194 | 9 | 37 | 0 | 12 | 0 | 0 | 2 | 1 | 0 | 85 | 0 | 0 | 0 |
| MAP23 | BcRcYc | 1 | 4 | 0 | 0 | 0 | 303 | 0 | 16 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 4 | 1 | 20 |
| MAP24 | Bc | 1 | 2 | 0 | 0 | 12 | 137 | 10 | 25 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 2 | 0 | 62 | 33 |
| MAP25 | BsRsYs | 1 | 4 | 0 | 0 | 108 | 110 | 2 | 6 | 1 | 0 | 0 | 6 | 4 | 0 | 0 | 89 | 0 | 0 | 8 |
| MAP26 | BsRsYs | 1 | 8 | 0 | 0 | 36 | 185 | 44 | 14 | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 3 | 3 | 0 | 0 |
| MAP27 | BcRcYc | 1 | 4 | 0 | 0 | 65 | 178 | 3 | 41 | 0 | 0 | 2 | 0 | 1 | 1 | 0 | 12 | 0 | 2 | 17 |
| MAP28 | BsRsYs | 1 | 4 | 0 | 0 | 116 | 306 | 11 | 16 | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 0 | 0 | 2 |
| MAP29 | BsRsYs | 1 | 1 | 0 | 0 | 102 | 347 | 10 | 7 | 7 | 0 | 1 | 0 | 1 | 3 | 0 | 27 | 8 | 0 | 16 |
| MAP30 | Bc | 1 | 0 | 0 | 0 | 9 | 125 | 0 | 11 | 2 | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 31 | 27 |
| MAP31 | RcYc | 1 | 3 | 4 | 0 | 83 | 356 | 2 | 36 | 20 | 0 | 1 | 0 | 0 | 1 | 0 | 8 | 3 | 0 | 4 |
| MAP32 | BcRcYc | 1 | 3 | 0 | 0 | 272 | 277 | 2 | 9 | 2 | 0 | 0 | 0 | 1 | 2 | 0 | 23 | 18 | 29 | 51 |

- **Exits:** every map has an ordinary exit line but MAP30, which is an Icon of Sin proper: a
  brain (88), a shooter (89), 6 spawn targets, and a cyberdemon and two masterminds on the
  floor. Secret exits: 4 lines (type 124) on MAP15 and 4 on MAP31: vanilla's wiring, MAP15 to
  MAP31 to MAP32 to MAP16, by the engine's own order.
- **Tag 666:** MAP07 has one sector tagged 666, 4 mancubi and no arachnotron: vanilla's
  mancubus rule only. MAP08 has a sector tagged 666 with 2 ordinary lines tagged 666: just a
  tag. No Keens anywhere.
- **Voodoo doll:** MAP13 has two player-1 starts. One doll, or a slip.
- **A line with a tag-needing special and tag 0:** one, on MAP29 (type 64, floor raise to
  lowest ceiling, SR). In vanilla it would act on every untagged sector; from Boom on it does
  nothing unless `comp_zerotags` is on (the engine's list, `doomstat.h`); our
  `tagged_sectors(0)` is empty too, agreeing with the engine as we run it.
- **Ambushes:** deaf monsters are the rule (272 of 277 on MAP32, 168 of 379 on MAP15);
  monsters' own teleport lines (125, 126) on 16 maps, most on MAP26 (44) and MAP09 (25).
- **Crushers** on 11 maps (20 lines on MAP31); crushing floors on MAP22.
- **Timed doors:** the 7 sectors above; shut-wait-30-open lines (16, 76) on 6 maps.
- **Co-op:** "Most levels were made for co-op play" (`MM.TXT`). Multiplayer-only monsters on 10
  maps (62 on MAP24, 48 on MAP10, 45 on MAP21) and items on 25; no key is multiplayer-only.
  She's given none of them (`map.rs` `in_skill`), nor is a single player by the engine.
- **Not shown by a census:** self-referencing-sector illusions (lines with one sector both
  sides are counted, but most are ordinary), instant floors, doors that need a run, places
  reached only by an arch-vile's blast or a rocket's. "Lower texture missing" counts
  two-sided lines with a step and no lower texture on the low side, neither sector tagged:
  where illusions would be, not proof of any.
