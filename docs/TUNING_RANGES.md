# How far her judgement numbers can move: a range sweep

The stage 7 screen (`POLICY_SURVEY.md`) moved a few numbers at once and found every setting but
`avoid` within a few hundredths of the defaults. This sweep asks the next question: one number
at a time, pushed well past what any stance or setting uses, where does she start playing
clearly worse? That says how much room each setting has, which numbers matter at all, and
whether any default sits on a slope.

Run with `tools/tuning-ranges.sh [A B C D]` (the main build, `out/build`: nothing else built
under it while it runs). Each value is the quarter set (272 runs, two seeds a level-skill, about
4.5 minutes at 12 jobs), compared run for run with the defaults on the same runs; the results
gather in `out/score/ranges/summary.tsv`, and a stopped sweep picks up where it was.

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

59 values: A 26 (about 2 hours), B 11 (50 min), C 20 (1.5 hours), D 7 (30 min), plus the
defaults once; about 4.5 hours in all, any phase alone.

## Reading it

- The quarter set's own noise: about 0.01 for a change that touches few runs, up to 0.02 for one
  that touches most (each value's standard error is in the summary).
- A **fall-off** is a value whose change is beyond twice its standard error and more than 0.02,
  or whose exits drop by more than five. The range a setting can use is the span with none.
- A value beyond the defaults that's clearly better is a candidate default: the full set
  decides, run for run, as before.
- Whatever it shows, no option of hers is dropped for its score (the user's); what it shows is
  how far each can go, and what's written into her descriptions.

## Results

(Filled in as the phases finish.)
