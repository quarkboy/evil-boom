# Behavior and code review, 18 November 2026

## Review notes

The baseline is main at `b2ed22a` plus the existing working-tree edits to the route follower and combat tree. Those edits change how Evil waits for perpetual lifts and when she gives up on a group of enemies she cannot hit. Findings below refer to the working tree unless stated otherwise.

I traced the path from an engine observation to a movement decision, the paths for restarting and resuming play, and the tools used to decide whether a change improved a run. I also checked the previous Rust review, the November cleanup notes and the project's run-comparison instructions.

Start with the parity and score failure handling before judging cleanup work by either tool. For the first gameplay correction, the E1M1 replay has a short, deterministic failure and an observable expected result. Short Rust source references below are relative to `native/src/`.

## Findings reproduced on this checkout

### 1. Resuming a run teaches Evil a secret in the wrong sector

On Ultimate Doom E1M1, `run-map doom E1M1 --secrets --seed 1 --max-seconds 600` found all three secrets and exited in 158 game seconds. Its log places the first secret in sector 83 at tic 1666. Branching the resulting recording at 53.6 seconds with the same WAD, mode and seed logged a discovery in **sector 64 at tic 1876**, with the engine tally still at one secret. By then she had walked out of sector 83.

The normal runner checks the tally and passes the sector under the player to `Explorer::found_secret`. The replay loop restores controls and knowledge notes but skips this observation. On handoff, the runner starts its previous tally at zero. The next observation misattributes all the earlier progress to the current sector. On a later life, `secrets_again` builds objectives from those remembered sectors. The error affects the evidence used to debug a run as well as the bot's future search.

Share the tally transition between ordinary play and replay, including its reset on restart, and carry the last observed tally through handoff. Setting the starting count alone would hide the false discovery while losing the real one. A regression check can use this exact recording and compare remembered sectors before and after a restart. See `native/src/sweep/setup.rs:176`, `native/src/sweep.rs:631` and `native/src/knowledge/explorer/objectives.rs:1405`.

### 2. Parity can pass when every run fails

In a temporary directory I used an unchanged copy of `tools/parity.sh` with `/bin/false` as its binary. The save printed “saved 24 runs” and the check printed “all 24 runs the same”. Both exited successfully. The run pipeline hashes whatever reaches the end of the filter, including an empty stream. Each job still emits its scenario label. Two failed runs of the same scenario therefore compare equal, and a failed save can replace a valid baseline.

Require a successful child and one complete result per requested scenario before publishing the baseline or comparing it. Propagate failure through both the per-run pipeline and the parallel collection. Keep the old baseline until all 24 have succeeded. Check all-failed and one-failed batches as well as a real save/check pair. The existing harness deliberately compares summary lines. Use the existing full-recording comparison when a cleanup promises identical controls. See `tools/parity.sh:36` and `tools/dev/compare-runs.py`.

### 3. Score counts a run that never started as a loss

With one requested seed, `score doom2 MISSING --skills 3 --seeds 1 --jobs 1 --no-window` exited successfully. The TSV contained an `ERROR` row with zero seconds and the report classified that level as never beaten with a score of 0.000. This can make a broken build or missing input look like a gameplay regression and contaminate the baseline for later comparisons.

The parent discards stderr, observes that the child has ended without checking its status and synthesizes `RESULT ERROR` if the result is missing. The parser fills absent metrics with zeros. Keep the scenario key and child diagnostic, mark the batch incomplete, and make the command fail. Preserve completed rows for diagnosis. An ordinary Doom death or stall is a valid observation and should still contribute to the score. See `native/src/score.rs:154`, `native/src/score.rs:426` and `native/src/score.rs:65`.

### 4. A sweep silently discards the starting loadout

I ran E1M1 at skill 3, seed 1, mortal, one life and a 120-second cap. The full recording includes controls and knowledge notes.

| Entry point | Requested starting inventory | Exit time | Items | Recording SHA-256 prefix |
| --- | --- | ---: | ---: | --- |
| `run-map` | Pistol start | 73 s | 4/37 | `48f893574a54` |
| `run-map` | `--carry 287,200,50,0,20,1,100,100,1` | 46 s | 2/37 | `a2bed93e5998` |
| `sweep` | Same `--carry` | 73 s | 4/37 | `48f893574a54` |

The parser stores the loadout. The sweep rebuilds its child arguments with `option_args`, which omits it. The episode launcher uses the same function for its first level. This directly affects the project's recommended technique of giving the bot a loadout to isolate a movement problem. Round-trip the loadout with the existing `carry_arg`, including the optional ready weapon. Check the actual child invocation and recording against a direct run. See `native/src/cli.rs:147`, `native/src/cli.rs:164` and `native/src/main.rs:581`.

### 5. A normal watch accepts a replay argument and starts from the beginning

`watch doom E1M1 --headless --mute --seed 1 --from absent.run@30 --max-seconds 2` completed two seconds of play successfully with a nonexistent recording. The shared parser consumes `--from`, but the conversion to `WatchOptions` drops it. Normal watch goes through `first_watch`, while watch with a snapshot takes a different path via `run_map`. Someone watching a failed run can therefore inspect a different starting state from the one they requested.

Make each entry point either honor an accepted option or give a specific error before starting the engine. Include normal watch and snapshot watch in the option matrix. Correct the combat replay example in `docs/DEBUGGING.md:149` as well. It combines `--fight 2` and `--from`, which setup explicitly rejects. See `native/src/main.rs:655`, `native/src/main.rs:760`, `native/src/viewer.rs:723` and `native/src/sweep/setup.rs:145`.

### 6. Changing a map's slot changes what counts as finding all its secrets

I copied E1M1's ten map lumps into a temporary local PWAD under the name E4M3, leaving the geometry and things intact. With `--secrets --seed 1`, the original reached **3/3 in 158 s**. The replacement took the exit at **1/3 in 56 s** and also reported `SECRETS`. The other two were demonstrably obtainable on the same geometry.

`broken_secrets` keys the historical allowances solely by the map name. Both the exit policy and the result classifier use the allowance, including after a PWAD replaces the map. Keep the known exceptions for their original map data. Resolve them against the effective map's identity, after overlays, and default to the actual tally on an unfamiliar map. A music-only overlay on an unchanged original map should keep its allowance. See `native/src/sweep/setup.rs:26`, `native/src/sweep.rs:263`, `native/src/sweep.rs:1138` and `native/src/viewer.rs:1256`.

### 7. The score report overstates run equality and accepts incomplete pairs

I passed `score-report` two one-row sets with matching seed, outcome, kill count and elapsed seconds. The second row had different items, lines mapped and damage taken. It reported “1 runs the same to the tic”. The producer writes rounded **whole seconds**, and the comparison uses just outcome, seconds and kills. It cannot establish agreement even on the end tic.

Separately, malformed TSV rows are silently skipped, the baseline index overwrites duplicate keys, and `paired` accepts partial overlap. A one-row fragment of a full candidate can still produce a paired delta. Report the actual matching fields and distinguish a partial exploratory comparison from the complete, one-to-one pairing required by `step.sh`. Preserve the supported older TSVs with their optional trailing columns. For control equality, compare a recording digest. See `native/src/sweep.rs:1159`, `native/src/score.rs:340`, `native/src/score.rs:472` and `native/src/score.rs:676`.

## Gameplay trace on Doom II MAP13

The current combat edits address a real situation: several visible ledge enemies can take turns as targets, resetting the practical cost of trying a different one. On the current tree I played skill 4, seed 2, with the carried loadout from finding 4. I used the default invulnerability to observe movement without ending on a death. Two 480-second runs agreed on their reported gameplay counts. I extended the scenario to 2,700 game seconds. It ended at the time cap with 95/110 kills, 36/61 items, 229/247 sectors seen and zero damage taken. The program labels a capped run `STUCK`. That label alone says little about the end position.

Three observations locate the repeated behavior.

- The bot logged 108 individual give-ups and 11 group give-ups. At tic 54,922 it gave up on five visible enemies together. The group rule really did fire in the intended setting.
- At tics 91,431, 93,020 and 94,430, a timed-out cell pickup was followed by a new cell pickup **on the same tic**. The first two replacement destinations were different, at (1296, −3296) and (1296, −2848). Blacklisting a single item cannot resolve a shared access problem. Nor does a repeated label prove the bot retried the same item.
- The knowledge log records repeated crossings of the lift's stop line 952 and start line 918 during this interval. That ties the pickup churn to a place and a mechanism. It does not establish which movement controller took her across each line. The current follower edits remove the explicit wait on a perpetual lift, while threats, route choice and the lift's actual motion can all influence the next crossing.

The next discriminating experiment is to trace the item ID and goal coordinates, the route step, who owns movement, and the lift height over tics 91,400–94,500. Check whether both cell routes fail at the same transition, and whether the follower was actually allowed to walk it. If they do, expire the **failed transition** when its observed conditions change. If combat repeatedly interrupts an otherwise viable crossing, examine that handoff instead. Check a reachable pickup on an ordinary lift and a case requiring a dodge before changing either rule. Keep MAP13 as a fixture for general behavior.

### The new group and individual give-up clocks can disagree

In the working-tree edits, `leave_the_lot` adds an enemy to both `lot.ids` and `Patience::given_up`. When the shot clears, it removes the ID from the first collection. The second still has the group's deadline. `no_shot_long` consults that deadline **before** checking the clear shot, so the standing node can yield a viable firing position while the spacing rules already consider the enemy available again. Aiming on the move can still fire. This is a decision about who gets to move. It is a source-traced consequence, and the MAP13 trace does not establish how many times it occurred.

Give the group deferral its own reason and expiry. A clear shot should remove just that reason while preserving any independently earned per-enemy deferral. Test a group that starts behind cover, one enemy becoming hittable, and another staying behind cover. Also check a second group coming into view during the cooldown. The default group window starts at 30 seconds and can reach 16 minutes after repeated failures, so stale state matters. See `native/src/tactical/combat.rs:2173`, `native/src/tactical/combat.rs:2277`, `native/src/tactical/combat/tree.rs:943` and `native/src/tactical/combat/patience.rs:100`.

## Other findings traced through the source

These cases have a concrete trigger and a proposed check. Their frequency in ordinary play was not measured.

| Area and trigger | Consequence | Suggested change and check |
| --- | --- | --- |
| **Restart accounting in replay.** A live run with a spare stranded retry increments `strandings` and writes a “stranded” restart. Replay increments `deaths` for every restart and the resumed runner initializes `strandings` to zero. The live runner can also charge a stranded restart to a life after the spares run out. `sweep.rs:584, 735`, `sweep/setup.rs:177`. | A branch can acquire a death it never suffered and regain an already spent stranded retry. Its later retry eligibility and reported totals can diverge from the recorded run. | Record what budget paid for a restart, and use the same accounting when playing it back. Exercise a recording with one spare followed by a restart paid from lives. Retain a defined interpretation for older files. |
| **Headless watch and explorer-requested restarts.** A wedged explorer or completionist can set `restart_wanted`. The window and presentation loops consume it. The headless loop checks engine death, then calls `Watch::tic`, which handles the director's separate restart request. `viewer.rs:1127, 1531, 1846`, `viewer/present.rs:307`, `knowledge/explorer/execution.rs:488`. | With a life available, an alive, wedged bot can keep running to the headless time cap at a point where the window would restart. | Share the decision to grant or decline an explorer restart. Let each presentation retain its pacing and death screens. Compare the same wedged state with one retry left and with none. |
| **Directed or console restart during the Icon fight.** A real death calls `Player::restart`, which resets the boss stage, route and engagement. A requested restart calls `Player::start_over`, which clears the normal combat and retreat state but skips `boss_restart`. `tactical/player.rs:760, 776`, `tactical/player/boss.rs:210`, `viewer/console.rs:282`. | A fresh level can retain an in-progress action from the previous attempt, such as riding a platform. The boss logic can continue using a route or stage established before the restart. | Share the per-attempt reset, including the boss, and apply death learning only for the appropriate cause. Start again while riding and verify the bot chooses a fresh post when it has rockets. Preserve the intended memory that the brain was awake. |
| **A stuck prompt can be spent before it opens.** `ask_if_stuck` sets `asked_stuck` before calling `ask`. With the three-per-minute quota used, `open` returns `None`. `control/directed/ask.rs:286, 863, 869, 892`. | The quota can expire while the bot remains stalled, but the operator never receives the question for that stretch. The same early flag suppresses reconsideration when there were initially no choices. | Set the flag on a successful open. Check a full quota at the three-minute threshold, then advance until a slot opens with the bot still stuck. Require exactly one question. |
| **Stop can lose to a pending crash restart.** The supervisor schedules a delayed start. During the delay, `stop` sees no child and returns. The timer later sees no child and starts one, even if automatic restart was disabled during the delay. `console/server.rs:741, 759, 879`. | The game can come back after the operator chose to keep it stopped. Overlapping stop/restart workers also wait on whichever child is current. | Track the pending restart as part of supervisor state. Cancel it on an explicit stop or settings change. Bind a running stop operation to its original child. Check a stop during the delay and two overlapping restart requests using a disposable child. |
| **Batch workers can wait on a full pipe.** Score, sweep and arena poll for exit before reading their children's piped output. Some also drop errors from `try_wait`. `score.rs:428`, `main.rs:582, 1065`. | A child blocked writing diagnostics cannot exit, and its parent waits indefinitely. The risk depends on output and pipe capacity. The short MAP13 run emitted 7,583 bytes on this host, whose measured pipe capacity was 65,536 bytes. That run completed. | Drain pipes while each child is alive, or redirect each to a per-run file. Keep concurrency bounded and surface process errors. Check with a successful worker whose ordinary diagnostics exceed the pipe size. |
| **A planner budget miss is cached as a failed route.** The planner distinguishes exhausted search and budget termination for statistics, but its route result is optional. The explorer can cache a missing result for 20 game seconds and reduce the expansion budget for subsequent searches. `plan.rs:347, 550`, `knowledge/explorer/routing.rs:39, 264, 574`. | A difficult but reachable objective can disappear temporarily after a time-limited search. With a wall-clock deadline, host load can also change when this happens, even with the same seed. | Carry the termination reason with the route result. Keep the present backoff for exhausted searches. Measure retries and queued work for budget-limited searches before changing their policy. Test a reachable objective with a deliberately short expansion budget. |

## Structure worth changing

1. **Keep one meaning for a CLI option across runners.** The four steps are parsing, validating for a command, projecting into watch settings and forwarding to a subprocess. Findings 4 and 5 show different losses between those steps. A small table of supported options per entry point, exercised through the real CLI, would make omissions visible. Use the existing loadout formatter and keep old CLI spellings. This is a smaller first change than replacing the parser.
2. **Give restart reasons and per-life state one owner.** The engine owns warmup and time, the player owns the route and combat memory, and the runner owns budgets. A restart event can carry the reason and budget used through all three without folding presentation into gameplay. Keep death learning, level knowledge and per-life observations explicit. The same tally observer used by replay, run, survey and watch is a good first extraction. See `engine.rs:253`, `play.rs:90`, `survey.rs:273` and `viewer.rs:1884`.
3. **Make lift behavior a property of the special.** The two edited follower sites repeat the same repeatable, temporary, non-perpetual crossing predicate. Navigation and walkthrough also recognize perpetual lifts by searching display text. A wording edit can change behavior. Put the property with the existing special/trigger definitions, then use it in waiting, lookahead and planning. Keep repeatable ordinary lifts, perpetual start and perpetual stop as separate examples. Preserve route order and compare recordings before tuning waits. See `specials.rs:364`, `navigator.rs:972`, `walkthrough.rs:172`, `motor/follower/walking.rs:427` and `motor/follower/interactions.rs:1171`.
4. **Split machine results from human descriptions at the producer.** `run_map_in` already has the counts, precise tic and outcome. Score reparses the printed prose, and `step.sh` repeatedly re-runs the same reports to extract lines. Emit a versioned machine-readable result alongside the existing human output, decode it once in the batch coordinator, and render TSV and summaries from that record. Keep old TSV readers for historical scores. Include the exact tic and optional recording digest for the two different kinds of comparison. This also permits honest labels for time-capped and genuinely wedged runs.
5. **Use the existing movement arbitration point.** `tactical/arbiter.rs` already decides between fight, retreat and route. The player then overrides that decision for the Icon fight immediately after the call. Pass that existing exception into the arbiter so a decision trace can report the final owner and reason in one place. Preserve the special dodge/ride precedence. Check ordinary retreat, a dodge with nowhere to go, and a boss lift ride. See `tactical/player.rs:953` and `tactical/arbiter.rs:38`.
6. **Avoid rebuilding level history on every directed tic.** `known_progress` builds a new ordered set of every fired line and sums every learned effect. Directed play asks every tic, even when neither collection changed. A repeated crossing lengthens the history without adding a distinct line. Maintain or invalidate counts at the mutation sites, including removal when a trigger is found not to have fired. Compare the old/new counters on every tic of a recording before relying on them for restarts. Profile a long directed run first. The MAP13 trace exercised repeated crossings, but the review measured no CPU saving. See `sweep.rs:315`, `control/directed/restart.rs:20` and `knowledge/model/learning.rs:370, 491`.

## Review scope and verification

The Rust source inventory contains 136 files under `native/src`. I followed behavior across module boundaries and used targeted source inspection in each area below. This was a Linux review of the first-party project. The graphical stream console, Windows packaging, live Neuro/model service and third-party engine internals did not receive end-to-end testing.

| Path through the project | Evidence and extent |
| --- | --- |
| WAD → movers → navigation → route → motor | Reviewed effective map selection, texture/height inputs, jump caching, search termination, trigger classification and follower waiting. Existing real-data tests verified E1M1 planning, knock-off recovery and jump simulation against DSDA. The replacement-map experiment exercised a real PWAD overlay. |
| Engine → observation → knowledge → objective | Traced engine warmup, secret discovery, knowledge journal, objective IDs, drop timers and selection. The E1M1 branch and MAP13 pickup trace cover two ways the bot's interpretation can diverge from a player's. |
| Combat → player → controls | Traced target/shot checks, individual and group patience, movement arbitration, retreat and boss reset. The MAP13 result is one diagnostic scenario with a loadout and invulnerability. It gives no estimate of survival or score changes across seeds. |
| Directed play → viewer → console | Read question quota and restart handling, pause/connection dispatch, headless and presentation loops, and child supervision. Exercised the headless CLI locally. The prompt and console findings are source traces. |
| Recording → replay → sweeps → score | Ran direct and child-process entry points, compared recording hashes, branched an actual recording, injected a missing map, exercised two failed parity batches in isolation, and compared deliberately different TSV rows. |
| Build, tests and tools | Offline release tests passed: 54 unit and nine integration tests. The DSDA library and Doom and Doom II data loaded. Formatting and release Clippy with all targets and warnings denied passed. Compared current wiring against the architecture, debugging and testing notes and the earlier cleanup/cycle reviews. |

The isolated fixtures and outputs are in the WSL temporary directory `/tmp/evil-boom-review-uy4_ol8y`. Its `out/explore/DOOM2_MAP13.txt` contains the 2,700-second trace. These temporary files can be regenerated from the settings above. The build used a separate temporary target, and the existing integration suite used its normal ignored engine data directory. Only this review document was added to the checkout. There was no candidate gameplay change to score, and no full sweep or held-out map run.

## Suggested implementation order

1. **Restore confidence in comparisons.** Make failed parity and score jobs visible, validate the paired run keys, and label the equality actually measured. Use the failed-worker and differing-TSV cases above as acceptance checks.
2. **Make requested runs reproducible.** Fix option forwarding and replayed secret observations. Re-run the E1M1 checks above, including byte-for-byte comparison of direct and sweep recordings with the same loadout. Then correct the map-identity exception and verify both E1M1 fixtures.
3. **Consolidate restarts and lift classification in small changes.** Check a granted and a declined restart in each runner. For changes intended to preserve play, compare fixed-seed recordings and the repaired parity set. Keep a state-reset correction separate from a mechanical extraction.
4. **Tune only after identifying which rule prevents progress.** Use the MAP13 interval above to distinguish route failure, combat ownership and a valid response to a threat. Evaluate any change on ordinary lifts, other enemies and multiple seeds, then on the same baseline score set. A shorter MAP13 trace alone would be insufficient grounds to keep it.
