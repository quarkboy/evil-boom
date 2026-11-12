# How Evil BOOM works

Evil is a Doom bot built to be directed by an AI streamer. On its own it explores a level,
fights, and finds the exit, knowing only what a player at the screen could know. Directed, it
takes orders (go there, fight that, search here, set a goal) and carries them out at the
game's 35 tics a second, while the streamer's model decides at the speed of thought.

Everything is one Rust crate, `native/` (the `evilboom` library and the `evil-boom` program).
It runs the real game in-process: DSDA-Doom, built as a library with a small C boundary
(`dsda/evil_api.c`), stepped one tic at a time.

## The layers

```
             Neuro / a model / a script / nobody
                          |  actions, in the SDK's protocol (neuro/, control/)
                          v
   directed play: orders, goals, questions, what she's told   (control/directed*)
                          |
   the explorer: what to do next, from what she knows          (knowledge/explorer*)
   the fight:    what to do about monsters, this tic           (tactical/combat*)
   the player:   which of them has her legs, retreats          (tactical/player.rs)
                          |
   the follower: walking a route, jumps, lifts, doors          (motor/follower*)
                          |  controls: move, turn, fire, use
                          v
   DSDA-Doom, a tic at a time                                   (dsda.rs, engine.rs)
```

Each tic: the game's state is read (`engine`, into an `Observation`). The knowledge model updates
what she has seen, heard and learned (`knowledge/model`). The fight sizes up the situation once
(`Combat::now`) and decides through its tree. The player picks who has her legs: the fight, a
retreat, or the explorer's current route. The follower turns the route into controls, and they're
sent to the game.

## The level, before she plays

Planning starts from the WAD itself, not from the engine:

- `wad`, `map`, `things`, `specials`: the map's lumps and what every line and sector special does.
- `mesh`: a navigation mesh, one convex polygon per subsector, joined by portals.
- `movers`: where doors, lifts, floors and crushers go when set off, and how long they take.
- `nav`: which heights let the player across where, path-finding, fitting her box along a path.
- `jumps`: run-off jumps across gaps and up ledges, found geometrically and then checked in a
  fixed-point player-movement simulator (`jumps/physics.rs`). Results are cached in
  `out/navcache`, keyed by the map's contents.
- `navigator`: the level's analysis (triggers, teleporters, jumps, what fits where) and the
  questions asked of it: where can she get to, and by which route.
- `plan`, `walkthrough`: the search for a route to a goal through triggers, keys, lifts and
  timed doors. The planner knows the whole map. What keeps her honest is that she only plans with
  what she knows.

## What she knows

`knowledge/` is the line between the game's truth and her view of it:

- `sight` decides which lines she sees from where, as Doom's renderer would, so the automap she
  plans on is the one a player would have.
- `knowledge/model` keeps what she's seen, heard, set off and learned: which switch moved which
  sector, which floor hurts, which walls looked suspicious. It also keeps what she's sure of and
  what she isn't.
- `knowledge/explorer` chooses objectives from that: unexplored ground, items, keys, doors and
  switches to try, walls that may hide a secret, the exit. They're ranked by kind and distance,
  safe ones before risky ones. The omniscient planner answers "how do I get there", but only
  through what she knows.

## The fight

`tactical/` handles monsters, by what she has seen and heard, not by the game's state:

- `tactical/combat/tree.rs`: the fight as a decision tree, evaluated every tic. The first node
  that applies wins: survive (dodge, duck, get out from under a crusher), then the fight itself
  (close in, keep clear, stand and fight, make a run for it, ...).
- `tactical/combat/assess.rs`: how things stand this tic (outmatched, what's against her, short
  of ammo), worked out once and read by every rule.
- `combat/arsenal.rs` and `combat/style.rs`: which gun, and how to fight with it.
- `tactical/tuning.rs`: every judgement number (how near, how long, what's worth what) in one
  place. `evil-boom tuning` prints them; `--tuning FILE` changes them for a run.
- `tactical/player.rs`: the fight and the explorer together. It decides who has her legs and
  runs retreats (to cover, back the way she came, out of a crowd).
- `tactical/bestiary.rs`: monsters, missiles, weapons and items by their numbers.

## Getting there

`motor/follower*` walks a route at 35 Hz: steering along the path, run-up and takeoff for jumps,
waiting for lifts and doors, pressing switches, timing crushers, shooting barrels out of the way,
and replanning when the world doesn't match the plan.

## Directed play and the Neuro SDK

- `neuro/`: the Neuro SDK's WebSocket protocol (`link`, `session`), a mock server (`mock`), a
  language-model stand-in (`gpt`), and argument repair for actions a model gets slightly wrong.
- `control/directed*`: what an action means to the bot. That covers orders, standing goals,
  policies, fight tactics, questions with defaults and deadlines, and everything she's told
  (moments, kills, deaths, powerups, why she's doing what she's doing), all in the second
  person and kept short.
- `control/game.rs`: starting the game, the next level, restarting.
- `control/places.rs`, `control/names.rs`: the level's areas, named the way a player would.

[NEURO.md](NEURO.md) describes the actions and how to test with them.

## Around it

- `sweep`, `score`, `play`, `replay`: headless runs, the score, recording and replaying runs.
- `viewer/`: the watch window (`minifb`), the stream layout, Doom's own screens.
- `console/`: the stream console, a web page that supervises a `watch` child process
  ([CONSOLE.md](CONSOLE.md)).
- `arena`, `drive`, `survey`: fights set up for development, driving by hand, the secret survey.
- `advisor`: an optional decision model asked during play (TypeSafe's System One kind).

## Determinism

Every run is the same on its seed: the engine's random numbers are seeded, and the bot's own
collections iterate in a fixed order (`det`). That's what makes parity checks and run-for-run
score comparisons possible ([TESTING.md](TESTING.md)). A change that should change nothing must
produce exactly the same runs.

## Where to change things

| to change | start in |
|---|---|
| what she does in a fight | a node in `tactical/combat/tree.rs` (or a style, or a rule in `best_gun`) |
| a fight's numbers | `tactical/tuning.rs` |
| what she explores next | `knowledge/explorer/objectives.rs`, `selection.rs` |
| how she follows a route | `motor/follower/walking.rs`, `jumping.rs`, `interactions.rs`, `hazards.rs` |
| what she knows and learns | `knowledge/model/perception.rs`, `learning.rs` |
| how a line special behaves | `specials.rs`, `movers.rs` |
| a Neuro action | `neuro/session.rs` (its schema), `control/directed/` (what it does) |
| what she's told | `control/directed/moments.rs`, `kills.rs`, `events.rs`, `why.rs` |
| the engine boundary | `dsda/evil_api.c` and `native/src/dsda.rs` together |

A number that's a judgement goes in `Tuning`; a fact about the engine stays a `const` where it's
used. What the rest of her does about a fight decision goes by its `Move`
(`combat/moves.rs`), never by its log label.

The design history (why things are the way they are, what was tried and dropped, and with what
results) is in [design/](design/README.md).
