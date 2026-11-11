# The cleanup before the Neuro SDK work: plan

Asked for 2026-11-11: the bot plays well enough; before it's adapted to the Neuro SDK's control,
the code is to be made fit to work in. Comments that tell the story of a level instead of the
rule; a full survey of the fight's tree, the exploring, the knowledge and the tactical layer for
likely bugs; unclear sections rewritten; data structures made to match the code's structure.

This is the plan. Nothing in it changes what she does except where a step says so, and those
steps are scored on their own.

## What the survey found (commit 2d2f5ef)

| | |
|---|---|
| Rust source | 104 files, 43,768 lines: 34,334 code, 7,046 comment |
| Comment lines naming a level | 612, in 545 comments (the "cases") |
| Lines over 160 columns | 257 |
| Tests | 46 (`native/tests`: the follower, a few levels) |
| Debug switches read from the environment | 16 names, 41 reads, several every tic |
| `unsafe` | 96, nearly all the engine binding (`dsda.rs`) |

Where the weight is:

| area | lines | of which comment | case comments |
|---|---|---|---|
| tactical (the fight, the player) | 8,642 | 1,990 | 171 |
| knowledge (the model, the explorer) | 6,479 | 1,216 | 199 |
| motor (the route follower) | 4,458 | 762 | 111 |
| navigator, nav, plan, jumps | 6,096 | 758 | 76 |
| the rest (engine, viewer, console, neuro, control, tools) | 18,093 | 2,320 | 55 |

The state each layer carries, by fields of its main struct: `RouteFollower` 101, `Combat` 95,
`Explorer` 74, `Knowledge` 52, `Watch` 31, `Player` 27. The longest functions:
`Explorer::list_objectives` 441 lines, `sweep::run_map_in` 405, `Combat::observe` 355,
`RouteFollower::act_at_goal` 348, `RouteFollower::walk` 245, `Explorer::plan_route` 229,
`RouteFollower::check_blocked` 196, `Explorer::next` 183.

## Ground rules

1. **A refactor changes nothing she does.** Runs are the same every time on their seed, so
   this can be checked exactly, not judged. Step 0 builds the check.
2. **A bug fix does change what she does**, so it is never folded into a refactor: its own
   commit, its own score (`score --like out/score/base.tsv`), kept only if it's no worse.
   The three rules tried and dropped on 2026-11-11 are the reminder that a fix that looks
   right can cost.
3. **Small commits, one area at a time**, each leaving the tree building, clippy clean, and
   the parity check passing.
4. **No behaviour is tuned during the cleanup.** A number that looks wrong is written down
   (the list at the end of this file), not changed.

## Step 0: the safety net

- **Comment-only check (done):** `tools/code_only.py --check` strips comments and whitespace
  from every changed `.rs` file and compares with the last commit. A comment pass must pass
  it; then it cannot have changed anything.
- **Parity set (done):** `tools/parity.sh`: 24 fixed-seed `run-map`s chosen to cover the
  fight, the exploring, jumps, crushers, barrels, lifts, teleporters and a carried loadout
  (under half a minute), each run's outcome and counts hashed. `parity.sh save` records them
  from a build; `parity.sh check` says which runs differ. A refactor commit must come out
  identical. Before a risky one, the full `score --like` (17 minutes) as well:
  it must report 1088 runs the same to the tic.
## Step 1: the comments

What a comment is for after this pass: **what the code does and why, by the game's mechanics
and what she can know.** Not where it was found.

- Out: level names, seeds, run counts, health figures from a traced run, "the user's", the
  story of the failure, the history of what the code used to do.
- In: the rule, the mechanic that makes it right (a blast reaches 128; a hitscanner turns to
  her before it fires; a once-only line loses its special), what it interacts with, the
  name of the tuning number or the function that decides it.
- Doc comments (`///`) on every node, rule and struct field that isn't obvious; plain
  sentences; no parenthesised asides as the default form.
- Module headers brought up to date with what's in the module (the tree's diagram was two
  nodes behind).
- A decision stays in `docs/DECISIONS.md`; the story of a failure is in the git history.

**Pilot (done): `tactical/combat/tree.rs`.** All 40 of its comments rewritten
to this, a doc comment that had drifted onto the wrong item put back, six nodes that had
none given one. `code_only.py --check` passes. That file is the style to approve or correct
before the rest is done.

Order for the rest, each area a commit: tactical/combat, tactical/player, knowledge/explorer,
knowledge/model, motor/follower, navigator and nav and plan and jumps, then the rest. About
7,000 comment lines; this is the largest single piece of the work by volume and the safest.

## Step 2: the survey for bugs

Read every function in the four areas asked for, with these questions: does it do what its
comment says; does it agree with the engine's rule it models (checked against the DSDA
source); does another place decide the same thing differently; what state does it leave
behind. Findings go in a list with evidence; each fix is then scored on its own (rule 2).

Suspects already in hand from this week's traces, to start the list:

- **The fight has her eyes, the route her legs: her pace collapses.** `Motion::go` slows a
  run that isn't the way she's facing, to 15% facing the other way. With the fight aiming
  behind her the route crawls at 2 units a tic. Fixed only for the barrel run
  (`Move::route_faces`); every other "route goes on" move still has it.
- **Shots traced by where she faced before the tic's turn.** The use trace had this bug
  (fixed 2026-11-11); `detect_fired`'s shot trace is written the same way.
- **Two ways to set monsters on each other.** `RetreatKind::Bait` and the tree's
  `setting_on` node were written apart and overlap.
- **The barrel chain is worked out in six places**, with the blast and chain distances as
  separate constants in three files, and walls counted in some and not others.
- **Whether `Observation::barrels` is every barrel or the ones she's seen** differs by who
  reads it; the fight keeps its own `barrels_seen`.
- **Stale beliefs about heights.** Two cases this week (a stair's unseen steps; an exit step
  believed lower than seen). The model's heights against what she last saw need one rule.
- **The arbiter's own header says its order is "not yet a considered rule".** Three loops
  between the fight and the route were noted in DECISIONS.md step 5 and left.
- **A dip in a floor that hurts is reckoned a fraction of a hit.** It is none or a whole
  one. The wall rules now work round this; the estimate itself is still used elsewhere.
- **`episode` and `watch --episode` and `record` are three loops over the same thing**, and
  only one of them has lives.

## Step 3: structure

The aim in each area: a struct's fields are one concern, a function does one thing, and what
is decided is a type, not a string or a flag left set.

**The fight (`tactical/combat`)**
- `Combat`'s 95 fields into what they are: what she perceives (seen, heard, remembered),
  the tic's assessment (`Now`, already apart), clocks (`Patience`, already apart; the dozen
  loose `*_tic` and `*_until` fields into it), the aim and trigger state, the steering's
  memory, counters for the stats.
- `Situation`: the errand is a string tested for emptiness; `vile` and `duck` are tuples of
  a name and a way. Typed.
- One place for "hold fire" (it is decided in the node's flag, `Combat::hold_fire`,
  `HeldFire`, and `look`).
- `no_target` builds a `Decision` by hand six times with the same logging round each.
- Every number in a node that's a judgement into `Tuning` (the tree still has a 512 and a
  60 of its own).

**The player (`tactical/player.rs`, 2,086 lines)**
- Split by what it does: the per-tic loop and the arbiter's hand-off; retreats (seven
  kinds, each with its start, its way and its end: a trait or a table, not three matches
  to keep in step); errands and supplies; the advisor.

**The explorer (`knowledge/explorer`)**
- `Explorer::next` is a chain of fallbacks that set and clear mode flags (`suspecting`,
  `harm_ok`, `strafe_ok`, `exits_open`, `strong_only`, `from_dead_end`) round calls to the
  same function. A list of tiers, each a value saying what it allows; this week's E2M4
  bug was a tier that silently left the walls out.
- `plan_route` has six silent `return None`s. A typed refusal (too costly for her health,
  a dead end, shuts her in, a jump she'd not come back from): the log, the debug switch
  and, later, what Neuro is told all read the same value.
- `list_objectives` (441 lines) into one function a kind of objective.
- Path steps are strings that later code searches (`contains("one-way drop")`,
  `contains("run-jump")`, a sector number parsed back out of the text). An enum.

**The knowledge (`knowledge/model`)**
- What she has seen, what she believes, and what the map really has are kept in several
  parallel maps (`seen_heights`, `live`, `states`, `learned`, `doubts`, `changed_floor`).
  One record a sector.
- Things are matched by their class name and category as strings throughout
  (`t.category == "Monster"`). Enums at the engine boundary.

**The follower (`motor/follower`)**
- 101 fields: the track and where she is on it; one sub-state for each thing it can be in
  the middle of (a jump, a door, a press, a barrel, a crusher, a detour, an unstick), which
  today are groups of `Option`s that must be cleared together.
- `walk` and `act_at_goal` each do a dozen things in sequence; each sub-state its own step.
- Flags set from outside (`mortal`, `barrel_run`) into what the follower was made with.

**Across layers**
- The debug switches: one `Debug` value read once at start, not 41 `std::env::var` calls.
- Logs are strings pushed into three different vectors and one journal. Typed events with
  one renderer (see step 4).
- One game loop: `sweep::run_map_in`, the viewer's and the recorder's are separate, which
  is why a recorded run can't be promised to match a headless one.

## Step 4: ready for the Neuro SDK

What directed play needs from the bot, and what's missing today:

- **What she can be told to do**, as a typed set of orders with typed outcomes. `control/
  directed.rs` (1,178 lines) does this over the explorer's objectives; the fight, the
  retreats and the barrel run sit outside it, so an order can be overridden with no word of
  why. Every reason she stops, refuses or is taken off an order as a value (step 3's
  refusals and moves are most of them).
- **What she reports**, terse (her context is small): events as data first, text second.
- **One loop** that a headless run, the watcher, the recorder, the console and a directed
  session all drive the same way.
- **The agent's surface** (`play::Agent`) as the one thing the control layer talks to; today
  it reaches into `Explorer` and `Player` fields directly.

`docs/NEURO_INTEGRATION_PLAN.md` is the plan for the integration itself; this step only
makes the bot's side of that boundary clean.

## Step 5: the documents

- `CLAUDE.md`'s "Where things stand" (it says 0.400; the score is 0.464).
- `docs/DECISIONS.md` is 2,400 lines of history in order of writing; a short current-state
  summary at its head, the history kept below.
- `docs/DEBUGGING.md` against the switches as they'll be after step 3.
- `docs/REALISM.md`: the infighting, the barrel run, the walk-between test.

## Order and size

| step | what | changes behaviour | check |
|---|---|---|---|
| 0 | parity set | no | itself |
| 1 | comments, area by area | no | `code_only.py --check` |
| 2 | the survey; the list of findings | no | reading |
| 3 | structure, area by area | no | parity; full score before each merge of an area |
| 2b | the fixes from the survey, one at a time | yes | each its own score |
| 4 | the agent's surface, typed events, one loop | no | parity |
| 5 | documents | no | reading |

Steps 1 and 2 go together (the same reading does both). Step 3 is where the risk is; the
follower and the explorer's tiers are the two I'd do most carefully.

## Decided (the user, 2026-11-11)

1. **The comment style** is the pilot's. And a pass on the names themselves (functions,
   fields, moves, variables), so each says the rule it is: part of step 3, area by area,
   behaviour unchanged.
2. **The cases aren't kept** as a document: they're in the git history.
3. **rustfmt is adopted**, its defaults (the Rust style guide's), as one mechanical commit
   before anything else; `cargo fmt` before every commit from then on.
4. **Step 3 does the fight and the explorer first**, then stock is taken.

## Done so far

| step | what | commit |
|---|---|---|
| 0 | the comment-only check and the parity set | 50e5e1d |
| - | rustfmt over the crate | 930ecb2 |
| 1 | comments: all of tactical, all of knowledge | 4d0d3b3 to 5a57a7d |
| 3 | the debug switches read once (`debug.rs`) | b784191 |
| 3 | a route refused is refused for a typed reason (`Refusal`) | fbea79c |
| 3 | a path's notes are a type (`PathNote`), not strings to search | 4d54088 |
| 3 | the explorer's last resorts are a list of tiers, a function each | d44a1dd |
| 3 | `Combat::observe` is the list of what she takes in each tic; the counts are three functions | ba0d76a, 44b7982 |
| 3 | a decision with nothing in her sights is built and logged in one place | f62467b |
| 3 | five kinds of objective have a function each | ae51a13 |
| 3 | names, first pass (fight and explorer): a flag says what it means (`baiting`, `urgent`, `route_keeps_legs`, `exit_held_back`, `harm_allowed`, ...); `tools/rename.py` does a rename by the compiler's errors | f50c90f |
| 3 | the fight's state by what it's for: 44 of `Combat`'s 97 fields are seven parts of it (`combat/state.rs`: movers, trigger, dodge, steer, cover, off_barrels, senses) | 5bb0266 |
| 3 | sifting the list of things to do is its own module (`explorer/cut_off.rs`): the way back from a place a type (`WayBack`), what a list is sifted by another (`Sift`), out of 300 lines of closures in `list_objectives` (467 lines to 190) | this commit |

Still to do of step 1: the follower (about 110 comments naming a level) and the navigation,
planning and jump code (about 90), after stock is taken.

## Findings (step 2), as they come

Each needs its own scored fix (rule 2); none is fixed yet. The suspects listed under step 2
above stand; these are from reading the code since:

- **A clock runs for a monster she isn't standing for.** `standing` asks `no_shot_long`
  (which runs the no-shot clock, and at its end gives the monster up) before it has checked
  whether she'd stand at all (threat, health, a duel, in her way). A monster she has no shot
  at is so given up on a while whether or not it was ever holding her, and "given up" is read
  elsewhere (lost sight, the fist's choice).
- **`size_up` isn't only sizing up.** It rests the closing clock, starts and ends the
  barrel-avoiding way (`away_from_barrels`), and begins ducks and flights from an arch-vile:
  state changed before any node has decided. The order the nodes are tried in then matters
  in ways the tree's listing doesn't show.
- **`Combat::observe` is half measurement.** About 200 of its 580 lines keep the run's
  counts (shots, misses, what was on her line), threaded through what she perceives. The
  counts read the game's own health figures, which is right for a count and wrong to have
  beside the code that must not.
- **Three sources decide the trigger**: the node's `hold_fire`, `Combat::hold_fire` (set by
  the player for a retreat), and `aim_and_fire`'s own reasons (`HeldFire`); then `look`
  holds it too. Only the last is logged.

## Numbers that look wrong (not to be changed in the cleanup)

To be filled in as the survey goes.
