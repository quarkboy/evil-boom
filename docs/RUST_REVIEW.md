# Rust structure and performance review — November 2026

The review baseline is `dd07421`, which includes the pre-existing uncommitted gameplay
improvements. The refactor keeps decision ordering, thresholds, floating-point operations,
and recorded controls intact. It does not retune the bot.

## Where to change things

Paths below are relative to `native/src`. Public entry points remain in their parent
modules; the implementation children are private. State stays owned by `Explorer`,
`Knowledge`, `RouteFollower`, and `Combat`, so a single tic still has one explicit
sequence of state changes. The children provide focused implementations on those owners,
rather than introducing separate objects with overlapping state.

| Module | Responsibility |
| --- | --- |
| `knowledge/explorer.rs` | Objective types, explorer state, lifecycle, and the main control loop |
| `knowledge/explorer/world.rs` | Believed world states, restricted views, spent triggers, and jump exclusions |
| `knowledge/explorer/objectives.rs` | Enumerating and ranking goals: exits, keys, frontiers, healing, secrets, sounds |
| `knowledge/explorer/positions.rs` | Standability, pickup sampling, nearby standable positions |
| `knowledge/explorer/routing.rs` | Route caches, search budgets, alternatives, and damage estimates |
| `knowledge/explorer/selection.rs` | Selecting the next goal, suspected walls, and last-resort choices |
| `knowledge/explorer/execution.rs` | Taking and completing goals, glances, and failure tracking |
| `knowledge/explorer/viewpoints.rs` | Deterministic parallel wall viewpoint searches |
| `knowledge/model.rs` | Knowledge state, initialization, queries, and memory across lives |
| `knowledge/model/perception.rs` | Updating knowledge from sight, movement, damage, and sound |
| `knowledge/model/learning.rs` | Associating actions with observed sector changes and learned effects |
| `knowledge/model/walls.rs` | Interactive textures, suspicious walls, and missing map information |
| `motor/follower.rs` | Route state, step bookkeeping, control dispatch, and recovery |
| `motor/follower/walking.rs` | Walking, steering, and speed selection |
| `motor/follower/jumping.rs` | Run-up, takeoff, airborne control, and landing |
| `motor/follower/interactions.rs` | Doors, lifts, triggers, and end-of-step actions |
| `motor/follower/hazards.rs` | Crusher waits and moving-floor hazards |
| `motor/follower/barrels.rs` | Barrel obstruction, shooting positions, and blast retreat |
| `tactical/combat.rs` | Combat state, observations, and decisions |
| `tactical/combat/aiming.rs` | Target/weapon selection, aiming, and shot safety |
| `tactical/combat/movement.rs` | Dodging, strafing, and combat movement |
| `navigator/view.rs` | Restricted navigation, reachability, and progression |
| `navigator/view/planning.rs` | Planner facts, queries, relaxed dependencies, and result conversion |
| `sweep.rs` | Running maps and coordinating sweep processes |
| `sweep/setup.rs` | Engine/agent setup and replay restoration |
| `sweep/status.rs` | Optional live timing and search diagnostics |
| `cli.rs` | Shared run-option parsing and child-process option serialization |

Constants live beside the policy they control. In particular:

| To adjust | Start here |
| --- | --- |
| Objective priority and relative distance weights | `Explorer`'s `Kind::weight` and `Objective::priority` |
| Retry delays, look duration, objective timeout | `knowledge/explorer.rs` |
| Viewpoint candidate count, range, worker threshold/cap | `knowledge/explorer/viewpoints.rs` |
| Pickup ring radii and sampling interval | `knowledge/explorer/positions.rs` |
| Adaptive search budget, alternate activators, damage sampling | `knowledge/explorer/routing.rs` |
| Barrel retreat and shooting candidate limits | `motor/follower/barrels.rs` |
| Jump and walking tolerances | Their corresponding follower children |
| Estimated activation delay | `plan/world.rs::ACTIVATION_TICS`, shared with `plan/timing.rs` |
| Core player dimensions and step clearance | `geom.rs`, re-exported by `navigator.rs` |
| Monster/weapon characteristics and pickup values | `tactical/bestiary.rs` |

Engine-format values, special numbers, weapon slots, and fixed-point arithmetic are not
interchangeable tuning knobs. The WAD records and game tables remain together in their
existing modules. This avoids hiding their layout behind hundreds of one-use constants.

## Work removed from repeated paths

- Reuse the DSDA sector-query staging buffer and the follower's two height snapshots.
- Build observed things and barrels in one pass, without an intermediate object list or
  repeated category lookup.
- Convert sector heights to the simulator's fixed-point form once per batch of position
  checks. Avoid constructing default heights only to overwrite them immediately.
- Cache the immutable pickup ring offsets, preserving their original order and duplicates
  across rings.
- Select only the six nearest viewpoint candidates before sorting them. Original input
  indices break distance ties, matching the former stable full sort exactly.
- Inspect fired-action history in reverse without cloning the whole history.
- Normalize a wall texture once before testing its interactive prefixes.
- Compute the crusher set once per barrel-position search.
- Retain search diagnostic history only while a live status writer consumes it. Adaptive
  budget signals remain active with diagnostics disabled; scalar signals use `Cell`.

Named `GoalKey`, `ViewKey`, and `StandableCache` structures replace opaque tuples. The
planner signature still hashes the original tuple in its original field order.

The separate `dsda/spike` executable now imports the adapter's canonical C structures.
Its old copies had drifted from `evil_api.c`, including the player structure's size.
Sharing these definitions removes that ABI mismatch and future duplicate maintenance.

## Other modules reviewed

The following units remain in their existing locations because their responsibilities
are already cohesive, or a proposed optimization would risk changing tie-breaking,
collision rules, or observable behavior:

| Files | Review focus |
| --- | --- |
| `wad.rs`, `map.rs`, `things.rs`, `specials.rs` | Binary layouts, flags, static game tables, and lookup behavior |
| `geom.rs`, `mesh.rs` | Geometry tolerances, polygon construction, portal numbering, BSP lookup |
| `nav.rs`, `nav/path.rs`, `nav/fit.rs` | Reachability, A* ordering, collision fitting, detours, and smoothing |
| `movers.rs` | Neighbor-dependent targets, stairs, travel times, and temporary heights |
| `plan.rs`, `plan/spots.rs`, `plan/triggers.rs`, `plan/world.rs`, `plan/timing.rs` | Search ordering, world identity, caches, activation points, timers |
| `navigator.rs`, `navigator/analysis.rs`, `navigator/widths.rs`, `navigator/jumps.rs` | Static analysis, gate geometry, trigger reach, jump cache and verification |
| `jumps.rs`, `jumps/physics.rs`, `jumps/finder.rs`, `jumps/check.rs`, `jumps/climbs.rs` | Simulator construction, fixed-point arithmetic, candidate ordering and landing checks |
| `sight.rs`, `los.rs` | Occlusion ordering, projection, slope windows and spatial traversal |
| `motor.rs`, `walkthrough.rs` | Control values, momentum/track geometry, route-to-action conversion |
| `knowledge/completion.rs` | Item eligibility, restart limits and cached failed searches |
| `tactical/player.rs`, `tactical/bestiary.rs`, `tactical/sound.rs` | Combat/exploration integration, cover selection, game tables, hearing |
| `play.rs`, `replay.rs`, `drive.rs`, `viewer.rs` | Agent lifecycle, recording/replay, scripted control, rendering and display pacing |
| `config.rs`, `det.rs`, `lib.rs`, module roots | Configuration, deterministic collections and exports |

In particular, replacing deterministic hash collections, sorting by squared distance,
changing physics rounding, or parallelizing mutable planner state would need separate
evidence. Those changes are not part of this refactor.

## Validation and reproduction

Both default and `--no-default-features` release suites pass: 21 unit tests and nine
integration tests. The IWADs and DSDA library were present, so the real-engine tests ran.
Clippy passes with `--all-targets -- -D warnings` for both feature configurations and
for `dsda/spike`. The spike completed 20,000 engine tics.

The baseline's jump test failed because it required an obsolete jump-finder identity and
a historical ViZDoom trace. Its replacement runs two fixed ledge probes against the
installed DSDA engine for 80 tics each, checking every position and requiring airborne
motion. The unchanged simulator's ZDoom trigonometric tables differ slightly from DSDA:
the test allows less than one map unit horizontally and half a unit vertically. This is
distinct from the exact baseline/candidate gameplay comparison below.

Build and save the baseline executable before applying changes, then build the candidate:

```sh
cargo test --release --offline --manifest-path native/Cargo.toml
cargo test --release --offline --no-default-features --manifest-path native/Cargo.toml
cargo clippy --offline --all-targets --manifest-path native/Cargo.toml -- -D warnings
cargo clippy --offline --all-targets --no-default-features --manifest-path native/Cargo.toml -- -D warnings
python tools/dev/compare-runs.py BASELINE_EXE CANDIDATE_EXE --output out/comparison --repeats 3
```

The comparison uses seed 0, at most two lives and 600 game seconds per scenario. It
alternates executable order, retains each recording/log, and fails if any complete `.run`
file differs, including controls, restart markers and journal entries. Use a fresh output
directory; normal map logs and recordings in `out/` are overwritten by each run.
Run without concurrent sweeps or compilation. Wall times include startup, cached map
analysis and reporting; these are end-to-end measurements, not isolated microbenchmarks.

Exact recording equality establishes tic-for-tic control equality in these scenarios
with the same deterministic engine and seed. It is not an exhaustive proof for every
map, seed, or machine; planner wall-clock deadlines still exist.

## Measured results

Windows release builds, Rust 1.99.1, default features, the repository's unchanged release
profile, installed classic IWADs, and the same DSDA DLL and warm navigation cache.
Each cell is the median of three sequential runs. Positive percentages mean less wall
time. [All samples, recording hashes, and control counts](rust-review-results.json) are
retained alongside this report.

| Scenario | Baseline seconds | Refactor seconds | Time reduction |
| --- | ---: | ---: | ---: |
| Doom II MAP13 | 11.044 | 10.410 | 5.7% |
| Doom II MAP17 | 14.698 | 13.203 | 10.2% |
| Doom II MAP24 | 3.699 | 3.486 | 5.8% |
| Doom II MAP29 | 13.468 | 12.933 | 4.0% |
| Doom II MAP26, expert | 5.515 | 5.219 | 5.4% |
| Doom II MAP23, mortal | 3.371 | 3.425 | -1.6% |
| Doom E2M4, expert/mortal | 4.518 | 3.953 | 12.5% |
| Doom E1M3, secrets | 6.382 | 5.457 | 14.5% |
| Doom II MAP01, fight 4/mortal | 0.401 | 0.371 | 7.5% |

All 54 recordings match their scenario's baseline byte for byte: 155,971 control tics
per nine-scenario pass, repeated three times per build. The sum of scenario medians
falls from 63.096 to 58.455 seconds (7.4%). Timings vary, notably MAP29's third pair;
MAP23 is effectively flat, and the sub-second MAP01 timing is particularly noisy.
These results support keeping the reduced repeated work, without claiming a uniform
speedup or attributing the entire gain to one individual optimization.

Five additional baseline/candidate pairs also produced identical recordings: E1M1 with
combat at skill 4, E4M7 mortal, MAP17 with `--peek`, MAP13 resumed from the baseline
recording at 200 seconds, and an E1M1 snapshot at 10 seconds. The snapshot PNGs were
byte-identical too. A further MAP17 run with `EVIL_STATUS` enabled matched its baseline
recording, checking that diagnostic collection does not influence decisions.

That separate instrumented MAP17 run reported, by game second 599, 360 objective lists
taking 3.9 seconds (0.2 seconds in saturation), 1,021 successful routes taking 1.7 seconds,
and 186 failed routes taking 0.5 seconds. Objective enumeration remains the largest of
these measured categories. These counters cover selected phases rather than a complete
CPU sample profile; their timing is separate from the benchmark table.
