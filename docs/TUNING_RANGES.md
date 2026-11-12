# How far her judgement numbers can move: a range sweep

The stage 7 screen (`POLICY_SURVEY.md`) moved a few numbers at once and found every setting but
`avoid` within a few hundredths of the defaults. This sweep asks the next question: one number
at a time, pushed well past what any stance or setting uses, where does she start playing
clearly worse? That says how much room each setting has, which numbers matter at all, and
whether any default sits on a slope.

Run with `tools/tuning-ranges.sh [A B C D]` (the main build, `out/build`: nothing else built
under it while it runs). Each value is played on two quarter sets (`out/score/quarter.tsv` and
`quarter-b.tsv`: 272 runs each, two seeds a level-skill, the second two others; about 4.5
minutes each at 12 jobs), compared run for run with the defaults on the same runs; the results
gather in `out/score/ranges/summary.tsv` and `summary.b.tsv`, and a stopped sweep picks up
where it was. (Phase A was played on the first set alone, and its borderline values on the
second after: below.)

## The numbers and the values (defaults in bold)

| phase | number | what it is | values |
|---|---|---|---|
| A | `engage.stand_threat` | threat in sight at which she stands and fights rather than walks on | 0, 3, 6, **9**, 12, 18, 30 |
| A | `engage.low_health` | below this health, anything in sight is fought where she is | 0, 20, 35, **50**, 70, 90 |
| A | `retreat.health` | below this, with something shooting at her, she backs off out of its sight | 0, 25, 40, **55**, 70, 85 |
| A | `guns.outmatched` | seconds of killing past which she counts herself outmatched | 5, 10, **20**, 30, 45, 100 |
| B | `engage.keep_clear` | outmatched, how near an awake monster she keeps clear of | 512, 768, **1024**, 1536, 2048 |
| B | `engage.leave_be` | monsters she'd take longer than this (s) to kill are left be | 4, 8, **15**, 30, 60 |
| B | `errands.danger_near` | monsters this near a goal count against going there | 320, **640**, 1024, 1536 |
| C | `errands.item_range` | items further than this only when needed badly | 512, 1024, **1536**, 2048, 3072 |
| C | `errands.floor_cost` | health a pickup on a floor that hurts is reckoned to cost | 0, 10, **20**, 40, 80 |
| C | `errands.mega_gain` | a megasphere when it adds this much | 1, 50, **100**, 150, 200 |
| C | `errands.soul_gain` | a soulsphere when it adds this much health | 1, 25, **50**, 75, 100 |
| C | `errands.hurt` | below this health, health items come first | 25, **50**, 75 |
| C | `errands.survive_health` | below this, losing it fast: health first | 50, **80**, 110 |
| D | `guns.boss_save` | how much dearer the big guns' ammo is with a boss about | 1, 3, **10**, 30 |
| D | `guns.short_of_ammo` | rounds under which she's short of ammo | 5, **15**, 30 |
| D | `retreat.cover_range` | how far she'll look for cover to fall back to | 384, **768**, 1536 |

59 values. On both sets: B 11 (1.7 hours), C 20 (3 hours), D 7 (1 hour).

## Reading it

- The quarter set's own noise: about 0.01 for a change that touches few runs, up to 0.02 for one
  that touches most (each value's standard error is in the summary).
- **The defaults' own runs can be lucky.** Careful was chosen partly on the first quarter set,
  and on its seeds it scores 0.472, against 0.447 on the full set's other 816 runs. A value that
  changes most runs is a fresh draw, and on that set alone looks 0.02-0.03 worse for nothing:
  every one of phase A's 22 values came out below the defaults there. Hence the second set.
- A **fall-off** is a drop on both sets (their mean change beyond 0.02 and twice its standard
  error), or exits down by more than five on both. The range a setting can use is the span with
  none.
- A value beyond the defaults that's clearly better is a candidate default: the full set
  decides, run for run, as before.
- Whatever it shows, no option of hers is dropped for its score (the user's); what it shows is
  how far each can go, and what's written into her descriptions.

## Results

### Phase A: the stances' numbers (2026-11-11)

Change from the defaults, run for run; the first set (all values), the second (the values that
were borderline on the first), exits out of 272 (the defaults: 108 on the first set, 103 on the
second).

| number | value | first set | second set | exits | reading |
|---|---|---|---|---|---|
| `engage.stand_threat` (9) | 0 | -0.043 (0.019) | -0.024 (0.019) | 89 / 96 | **fall-off** |
| | 3 | -0.034 (0.019) | -0.030 (0.019) | 93 / 95 | **fall-off** |
| | 6 | -0.003 (0.016) | | 103 | fine |
| | 12 | -0.030 (0.016) | | 96 | lucky defaults (score 0.442) |
| | 18 | -0.010 (0.016) | | 101 | fine |
| | 30 | -0.032 (0.017) | | 95 | lucky defaults (0.441) |
| `engage.low_health` (50) | 0 | -0.025 (0.009) | | 97 | fine (0.448) |
| | 20 | -0.028 (0.009) | | 96 | fine (0.445) |
| | 35 | -0.013 (0.008) | | 102 | fine |
| | 70 | -0.012 (0.010) | | 103 | fine |
| | 90 | -0.021 (0.015) | | 97 | fine (0.451) |
| `retreat.health` (55) | 0 | -0.026 (0.015) | | 96 | fine (0.447) |
| | 25 | -0.023 (0.014) | | 98 | fine (0.450) |
| | 40 | -0.044 (0.013) | +0.004 (0.015) | 92 / 104 | the first set's luck |
| | 70 | -0.041 (0.015) | +0.004 (0.016) | 93 / 105 | the first set's luck |
| | 85 | -0.043 (0.017) | -0.031 (0.017) | 93 / 92 | **fall-off** |
| `guns.outmatched` (20) | 5 | -0.034 (0.019) | -0.001 (0.017) | 91 / 104 | the first set's luck |
| | 10 | -0.012 (0.013) | | 102 | fine |
| | 30 | -0.011 (0.007) | | 104 | fine (13 runs changed) |
| | 45 | -0.011 (0.007) | | 104 | fine (14 runs changed) |
| | 100 | -0.014 (0.007) | | 103 | fine (15 runs changed) |

"Fine" on the first set alone: its score (in brackets where the change looks large) is within
the quarter's noise of what the defaults score off their lucky seeds (about 0.45), and its exits
within five of the full set's rate (about 100 a quarter). What it says:

- **Nothing beats the defaults**: careful sits on a broad plateau, not a slope.
- **`engage.stand_threat`**: 6 to 30 holds; 3 and below, standing to fight almost whatever's
  in sight, costs about 0.03 on both sets. Bold uses 3 and yolo 0: their measured costs (bold
  -0.020, yolo -0.006 on the first set) are about what this says.
- **`engage.low_health`**: no fall-off anywhere from 0 to 90; the number hardly matters.
- **`retreat.health`**: 0 to 70 holds; 85 (backing off whenever she's below 85 with something
  shooting) costs about 0.035 on both. Yolo's 0 is fine by it.
- **`guns.outmatched`**: 5 to 100 holds; above 20 it barely changes a run (13 to 15 of 272),
  so outmatched is mostly decided by what's against her, not by the seconds.

For her settings: the stances' spans are all inside what holds (careful 9/50/55/20, steady
6/35/40/30, bold 3/25/30/45) but for bold's and yolo's `stand_threat`, which is where their cost
comes from: as their descriptions say.
