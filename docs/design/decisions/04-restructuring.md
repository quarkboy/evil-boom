# Restructuring and realism (2026-11-09)

Seven steps that made the fight testable and tunable (typed decisions, one Tuning, one assessment a tic, the arbiter, the advisor, the score on fixed seeds), and the start of realism: the fight going by what she knows rather than the game's truth. The realism survey and its work list are in ../realism.md.

## Restructuring, step 1: the fight's decision as a type (2026-11-09)

The first step of the restructuring towards a decision model (a classifier answering small
questions: which target, what stance, whether to take a risk) over rules whose numbers can be
varied. What the rest of her did about a fight decision went by the first words of its log label
(`why.starts_with("dodging")`, a list of prefixes for "the route keeps her legs", another for
"getting out of the way"), so rewording a log line could change her play, and nothing outside
could name a decision.

- `tactical/combat/moves.rs`: `Move`, one variant a decision (the tree's nodes, the three made
  with nothing in sight, the rocket's two after the tree). `Act` and `Decision` carry it as
  `kind`; `why` stays, for the log and the viewer only.
- The prefix lists are two properties of a `Move`: `route_goes_on` (was "shooting on the way",
  "getting past", "out of", "sneaking": the last node, an errand, getting past, an arch-vile's
  sight, out of reach, out of ammo, sneaking) and `escaping` (was "dodging", "ducking", "ducked",
  "out of", "out from", "away from"). "out of" had caught three unrelated labels by accident
  ("out of the X's sight", "out of reach: going on", "out of ammo: getting past"): the same three
  are named in `route_goes_on`; of them only the arch-vile's can be a takeover, so only it is in
  `escaping`.
- Why the trigger's held (`HeldFire`) and why she died (`Cause`) are enums too: the plasma step
  and the death count compared their strings.
- Left as they are (not the fight's): the explorer's drop reasons ("no route", "stuck" read out
  of the follower's log line), `directed.rs`'s "pressed", the sweep's "stranded". For step 5 (the
  arbiter), when those layers get typed outcomes.

Nothing changes: the parity set (16 maps, HMP seed 1 and UV seed 2, 32 runs) is line for line the
same as before.

## Restructuring, step 2: the tactical layer's judgements in one `Tuning` (2026-11-09)

The fight and the player judged by some 200 `const`s spread over eight files: fixed at compile
time, so neither a tuning run nor a decision model could vary one.

- `tactical/tuning.rs`: `Tuning`, 152 numbers in eleven groups by what they're about (`senses`,
  `engage`, `dodge`, `barrels`, `steer`, `hide`, `aim`, `guns`, `errands`, `retreat`, `deaths`),
  each with the comment its constant had. The defaults are the constants' values. The player owns
  it (an `Rc`, the fight's `Combat` sharing it) and every rule reads `self.tune.<group>.<field>`;
  the three free functions that needed one (`miss_by`, `clearance`, `hit_clearance`) take the
  group; `in_reach` became a method.
- What's a judgement and what's a fact: a number she could reasonably hold otherwise (a distance
  kept, a give-up clock, a weight, a threshold of health or time-to-kill) moved; a number the
  engine fixes (a rocket's speed and box, a blast's reach, autoaim's range, the BFG's delay, an
  arch-vile's 66 tics), her own speed (`RUN`, `RUN_SPEED`, `GET_THERE`), and a search's
  resolution or budget (`WAYS`, `PUSH_TICS`, `POST_LOOKS`, the trail's length) stayed `const`:
  53 left in the tactical layer.
- From outside: `evil-boom tuning` prints the defaults as JSON; `--tuning FILE` (any run option's
  place; passed on to a sweep's processes) reads a file naming any groups and fields, a
  misspelt name an error; `Player::set_tuning` swaps one in mid-level (for a decision model's
  stance; nothing calls it yet).
- Not moved yet: the explorer's and the follower's numbers (`knowledge/` 103, `motor/` 85), the
  bestiary's two (`PUNCH_HEALTH`, `SAW_HEALTH`), the score's own run options. `score` doesn't
  take `--tuning` yet (step 7's harness).

Nothing changes: the parity set (32 runs) is line for line the same; a file changing three
numbers does change the runs it's given to.

## Restructuring, step 3: one assessment a tic (2026-11-09)

"Is she outmatched", "what's shooting at her", "is she short of ammo", "is she surrounded" were
each worked out afresh by whichever rule asked, some seventy times a tic between the fight's
tree, the retreats, the errands and the explorer's hooks, a few of them (surrounded, an
arch-vile she can't kill in a moment, what shoots) by the player's own versions. Nothing said, in
one place, what a decision had been made on.

- `tactical/combat/assess.rs`: `Assessment`, worked out once as she looks (`Combat::observe`'s
  last step) and kept as `Combat::now`: her place and health, invulnerable or not, what's
  against her (and of it what shoots, what she's in a fight with), the threat, her firepower,
  guns with ammo, short of ammo, outmatched, surrounded, an arch-vile to hide from, the rate
  she's losing health and the seconds that leaves. Every rule reads `now`; the functions that
  work the answers out are private to the fight (the builder's), so there's one way to ask.
- The tree's `Situation` lost what `now` has (health, threat, outmatched): it's the fight's
  options this tic (target, aim, missiles, the ways out); `now` is how things stand.
- What isn't in it, and why. `armed`: a monster given up on is out of her fist's reach, and the
  tree gives up mid-tic, so it stays a question to the fight (which reads `now` for the rest).
  `bfg_flying`: changed by this tic's trigger. The hurting floor under her: the explorer's
  knowledge, the player's `Plight` still. And the supposing questions ("with that gun in hand,
  could she take what's by it?"): the arsenal's functions take the observation supposed, so
  they keep working from one (`too_much`, `sniper`, `takes_on`, `ammo_short_*`, `firepower`).
- Next on it (not done): a line logged at each decision with the assessment it was made on, and
  a way to force a choice, before any model is attached.

Nothing changes: the parity set (32 runs) and a second set (12 more maps, HMP and UV on other
seeds, three lives for the start-again path: 24 runs, 28 deaths) are line for line the same as
the build before.

## Restructuring: a journal of decisions, and choices forced (2026-11-09)

Before a decision model is attached: a record of what the rules decide and on what, and a way to
make her decide otherwise.

- `decisions.rs`: with `--decisions`, a line of JSON a decision beside the run's log
  (`out/explore/<IWAD>_<MAP>.decisions.jsonl`): the target (when it changes; every candidate and
  its score), the fight's move (when it changes; its `Move` and label), a retreat begun (its
  kind), an objective taken up (every one routed, with its weighed way). Each with `state`, the
  tic's assessment (`Assessment::summary`). The four are the four questions planned for a model
  (which target, what stance, whether to back off, what next); the records are what it'd be
  shown, and what the rules answered.
- Forced: `--bar NAMES` passes over the named moves (a node giving one is skipped, the next
  tried) and retreat kinds; `Player::force_target(id)` makes a monster in sight her target
  whatever the scores. The stance is `Player::set_tuning` (step 2); objectives can be ordered
  already (`control/directed.rs`).
- Seen in the first journal read (MAP06 UV, one run): 1509 changes of move in 385 s, most of
  them between out of reach, hiding from what outmatches her, closing in and stepping for a
  shot. The tree is asked afresh every tic and nothing holds a move once made: for step 5.

Nothing changes with neither asked for: the parity set is line for line the same, and a run's
log is the same with the journal on.

## Restructuring, step 4: one of each skill (2026-11-09)

The same skill written out more than once, each copy a little different. Three merged with
nothing changed in play (both parity sets line for line the same after each):

- **Who can see her where** (`combat/sight.rs`): `Combat::sees(eye, p, z, heights)`, with `eye`
  and `eye_of` for where a monster sees from. It was written out ten times: the arch-vile's
  shadow, the duck, the mesh's way round a corner, the steering's exposed ways, a sniper's shot
  from where it was last seen, the way back out, cover's spots, the ground a sniper watches, a
  post and its peek. The searches still differ in where they look (straight runs from where she
  stands, the mesh, the places she's been, a grid over what she knows): those are different
  skills (a second's duck against a walk to a post), not copies; what they ask of a place is now
  the one question.
- **Keeping at a monster she's getting nowhere with** (`combat/patience.rs`): `Patience::spent`
  for the four clocks that ended in the same give-up (closing in out of reach, no shot,
  hitting without hurting, walking at it and getting no nearer): a limit, progress by a measure
  where there is one, afresh after a break where the effort comes and goes. The explorer's and
  the follower's clocks (an objective's time, a step's, a press's tries, a wait for a lift) are
  left: each is part of its own state machine, and they come up again with the arbiter (step 5).
- **Too much for her** (`Odds`, `combat/arsenal.rs`): how long to kill them, how long she'd
  last, whether her ammo would do; `too_much(secs)` is the asker's line on those (30 s:
  outmatched; 15 s: left standing about). The assessment carries the odds against what's in
  sight. `sniper` and `takes_on` keep their own lines (one monster's kill time against a few
  seconds, and against how long she'd last against it alone).

## Realism: the fight's hurting floors by what she knows; the survey (the maintainer, 2026-11-09)

The fourth merge of step 4 couldn't be made without changing play: the fight took a floor to
hurt by the map's damage values (and a floor seen changed, and one going down into harm), the
rest of her by what she knows (`Knowledge::damaging`: felt, else by its look). Asked, the maintainer
made the call: it's time for realism (CLAUDE.md's rule is now "Combat: realistic now").

- The fight's `hurts` goes by `Knowledge::hazards` (the player hands it over each tic): what she
  takes to hurt, none of it under a radiation suit (the map's values knew nothing of suits: she
  kept off nukage in a fight with one on). A floor going down into harm stays (her own eyes,
  though where it ends is the map's: realism.md's M4).
- Score: 0.448 -> 0.445 (HMP 0.537 -> 0.535, UV 0.358 -> 0.355), one measurement each, inside
  the noise (0.008 a set). 6 of the 32 parity runs play differently.
- `docs/design/realism.md`: the survey of every other place she goes by the level's truth (eight by
  the observation, six by the map), what each would be instead, and the order. The maintainer's: after
  the restructuring, one at a time, each scored; a very large drop means a bug in the change.

## Restructuring, step 5: the arbiter (2026-11-09)

Who walks her (the fight's move, a retreat's route, the explorer's route) was decided in three
places: a `takeover` flag worked out inside the fight's `decide`, overwritten by the player for a
dodge mid-retreat, and read again inside the fight's `controls`.

- `tactical/arbiter.rs`: `arbitrate` gives `Legs::{Fight, Retreat, Route}` from what the fight's
  decision wants (`Decision::wants_legs`), whether a retreat's under way, and whether a dodge
  has a clear way; `controls` is told. Today's order exactly, written down for the first time:
  a dodge mid-retreat only with a clear way; any other move of the fight's that wants the legs,
  retreat or not (the retreat's way then only where the fight's steering leans); a retreat; the
  route. The second line wasn't known as a rule before it was written: a retreat to cover with
  something in sight is mostly the fight's legs.
- The outcomes the explorer and the follower passed about as words are typed: a drop for want
  of a route (`drop_no_route`), a follower's failure by being stuck (`failed_stuck`), a press
  with nothing seen to go off (`Ended::pressed`).
- The journal notes each change of legs. Both parity sets line for line the same.

What the journal shows now it can (eight UV runs, seed 7): the legs change hands 1379 times in
1460 s of play, half of the holds 10 tics or less. Three loops found, each a decision undoing
its own condition, none fixed here (each is a traced fix of its own, to be scored):

- **Standing and walking on** (E1M3, one imp, threat 3): "standing and fighting" and "shooting on
  the way" by turns every 1-7 tics for seconds. Standing isn't by threat there (3 against 6), so
  by the imp blocking her way or a duel: a condition that goes by the route's way, which stops
  being walked the tic the fight has the legs.
- **Closing in and going on** (MAP12, an arachnotron out of reach): `outmatched` on and off
  every 2-4 tics. Outmatched counts what's within 1200; closing in brings it inside, outmatched
  hands the legs to the route, which walks her back out.
- **Out of ammo** (E2M2): "out of ammo: getting past" (the route's legs) and the fight's legs
  with nothing in sight, every 2 tics for seconds: the monsters in sight one tic and not the
  next as the route and the fight turn her by turns; unarmed, the fight still "keeps at" what
  it's lost sight of.

A hold on the legs at the arbiter (no change of hands inside some reaction time) would slow all
three without curing any; considered and not done, as a guess. Each loop's condition is to be
traced and given its own slack.

## Restructuring, step 7: the same runs, compared run for run (2026-11-09)

Two score sets on seeds of their own differ by about 0.008 with nothing changed, so a change
had to move the score 0.02 to be seen, and a smaller one needed several sets.

- `score --like FILE` plays the very runs of an earlier set: its levels, skills and seeds.
  `--tuning FILE` and `--bar NAMES` are passed to every run (a tuning or a barred move scored).
- `score-report A B`, the two being the same runs, adds the comparison run for run: the mean
  difference and its standard error, how many runs are the same to the tic, how many better and
  worse, by skill, and the levels that moved most.
- Checked: the 1088 runs of the last measurement played again on the build ten commits on (all
  of them meant to change nothing) came out the same to the tic, every one: +0.0000. So runs
  are the same on their seeds across a whole measurement, twelve at a time; the restructuring
  since the floors change has altered nothing on any of the 68 levels; and a change's effect can
  be read exactly, on the runs it touches. `out/score/base.tsv` is that set (0.445): the
  baseline the realism changes are measured against.

## Restructuring, step 6: the advisor, a decision model's say (2026-11-09)

The seam for a classifier of the System One kind (hosted Jev; tev1 behind llama.cpp's server
speaks the same), with two questions through it to prove it end to end. Not the advisor it's to
grow into (the maintainer's: it's to be expanded a lot).

- `advisor.rs`: a `Question` (how things stand in words, the instructions, the choices each
  with what it means) goes to a backend on its own thread; its `Answer` (the choice with the
  most belief, if `min_share` or more) is due `latency_tics` after the asking, however fast the
  run goes: flat out, the run waits for the reply and holds it till its tic, so it plays as a
  watched game would with a model that slow, and repeats when the model says the same. Under
  `min_share`, or an error, or no reply: nothing, the rules'.
- Asked now (`Player::consult`): **which target**, with two or more against her, at most every
  `target_every` tics, the answer forced as her target (`Combat::forced_target`) for
  `hold_tics`; **what stance**, in a fight, every `stance_every`, the stance's numbers put over
  her tuning for `hold_tics` (`Tuning::with`). The stances are in the settings file, names and
  numbers both: three to start with (bold, steady, careful), their numbers a first guess, never
  tuned.
- The state is `Assessment::words`: her health and armor, her firepower, ammo, the rate she's
  losing health, outmatched and surrounded, and what's against her by kind and distance (not
  its health: she doesn't know it). It's to hold only what she knows: realism.md comes first.
- Settings: `config/jev.json` (committed), overlaid by `config/jev.local.json` (the key; not
  committed). `--advisor FILE` on a run; off without it. Backend "first" answers the first
  choice at once, for trying the plumbing with no model.
- Tried: E1M3 UV seed 2 with Jev: 132 answers taken in 440 s of game (99 targets, 21 bold, 12
  careful), 37 s real for a run that takes 4 without (each question some 180 ms); she got out,
  as she does without. That's one run: it says the thing works, nothing about whether it helps.
  Both parity sets are line for line the same with it off.
- Next for it, in order of what I'd do: every open question in one request (Jev answers several
  for the price of one); the fight's move, a retreat and the next objective as questions (the
  journal has their options already); the journal with how each run ended, as data to train
  tev1 on. Not the tic-by-tic things (a dodge, the aim): at 180 ms those stay the rules'.
