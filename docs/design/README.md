# Design notes

How Evil BOOM came to be the way it is: the decisions and why, the plans, the surveys, and what
was tried and dropped, with the numbers. These are working notes, written as the work went, so
they're dated and in the order things happened. For how things are *now*, start with
[../ARCHITECTURE.md](../ARCHITECTURE.md). Come here for the *why*, or before proposing something
that may already have been tried.

The notes call the project's maintainer "the maintainer", and Evil "she". Scores are on the
score described in [../TESTING.md](../TESTING.md), compared run for run unless they say otherwise.

## The decision log

The decision log, in five chapters:

| chapter | dates | what it covers |
|---|---|---|
| [1. Foundations](decisions/01-foundations.md) | Nov 3-5 | the first decisions; the navigation mesh, the planner over moving sectors, jumps checked in a movement simulator, timed plans; the explorer finishing every map that needs no kill |
| [2. The engine](decisions/02-engine.md) | Nov 5-8 | DSDA-Doom in-process; everything in Rust; what she perceives and learns (the automap rule, hidden secrets); Linux |
| [3. Fighting](decisions/03-fighting.md) | Nov 5-9 | the fight from first pieces to a decision tree; crushers, barrels, rockets and autoaim, bosses, the BFG; the level campaigns and 20-minute passes that traced every death and stuck run to a general rule |
| [4. Restructuring and realism](decisions/04-restructuring.md) | Nov 9 | typed decisions, one `Tuning`, one assessment a tic, the arbiter, the advisor, the score on fixed seeds; the start of realism |
| [5. The stream and directed play](decisions/05-stream-and-directed-play.md) | Nov 10 on | the stream console and Doom's own screens; Evil's control through the Neuro SDK; what she's told; questions, settings, goals; fixes found by watching models play |

New decisions go at the end of the latest chapter.

## Plans and surveys

| note | status | |
|---|---|---|
| [realism.md](realism.md) | in progress | every place she still decides by the level's truth, the work list, and what each change scored |
| [neuro-sdk-plan.md](neuro-sdk-plan.md) | built through stage 7 | the full control layer: decisions by how long an answer's worth having, five grains of control, the actions, what she's told, the bot's questions; notes from directing levels by hand |
| [neuro-integration-plan.md](neuro-integration-plan.md) | superseded | the first integration plan, and its first slice |
| [console-plan.md](console-plan.md) | built | the stream console's design, its protocol and stages |
| [policy-survey.md](policy-survey.md) | done | her standing settings, surveyed before measuring |
| [tuning-ranges.md](tuning-ranges.md) | done | how far each judgement number can move before she plays worse |
| [cleanup-2026-11.md](cleanup-2026-11.md) | done | the code cleanup before the Neuro work: findings and their scores |
| [rust-review.md](rust-review.md) | done | a structure and performance review of the Rust code |
| [open-source-plan.md](open-source-plan.md) | in progress | preparing the repository for publication |
| [multiplayer-plan.md](multiplayer-plan.md) | proposed | two players, Evil and Neuro, co-op or deathmatch: what each layer needs, in stages |
| [human-player-plan.md](human-player-plan.md) | proposed | a person in the game with Evil, co-op or deathmatch: a second process in step with the host; shares its first stages with the two-players plan |
| [mbf21-plan.md](mbf21-plan.md) | proposed | MBF21, UDMF and community WADs: the engine plays them already; what the bot must learn to understand them |

## Where things stand (2026-11-12)

- **The score:** about 0.44 (1 for an exit, else half the kill share, over DOOM and DOOM II on
  HMP and UV, 8 seeds each), with about 380 of 1,088 runs reaching the exit. Realism cost
  some score: the fight now goes by what she has seen and heard, and she's hurt more slowly
  than before but still dies about as often. What's short now is what she does with the health
  and ammo she has (the realism work list's part D), more than what she knows.
- **What she senses:** sight (her field of view, a moment's memory, half a second of instinct),
  the game's own sounds (a death cry, an attack, a missile, footsteps), which side a hit came
  from, and bumping into things. Each is a reason to turn and look.
- **Directed play:** the Neuro SDK layer is built through stage 7 of its plan: orders, the
  fight's tactics, standing settings, goals, the bot's questions with defaults, `doom_why`,
  moments told as they happen, control of the game itself, and instinct after 3 idle seconds.
  DOOM's first two episodes have been finished by directed play with a dozen orders each.
- **Open:** the rest of the realism work list; the advisor (a decision model's say) to be grown;
  structure left from the cleanup that has no failing run behind it.
