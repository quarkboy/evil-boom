# Notes for Claude sessions on this repo

Evil BOOM: a bot that plays DOOM / DOOM II for Evil Neuro, live on stream (Neuro SDK
integration later). Evil is all Rust (`native/`, the `evil-boom` binary) running DSDA-Doom
in-process (`dsda/`: the patch and `evil_api.c`; the DSDA source itself is fetched into
`third_party/`, not committed). Decisions and their reasons: `docs/DECISIONS.md` (keep it
updated). How to trace a failed run: `docs/DEBUGGING.md` (read it before debugging).

## How the user wants the work done

- **Human, not TAS.** Recoveries, missed lift windows, wasted time are fine if she finishes.
  Fix failures, not inefficiency.
- **Trace, then fix.** Take a concrete failure (a death, a stuck) back through the logs to the
  decision that caused it. Work out the game mechanics involved (check vanilla/DSDA source, the
  map's lines with `evil-boom sector`), the narrowest rule that covers it, and everything else that
  rule touches. No first-idea heuristics; no guessed fixes. Say what was considered.
- **General, never level-specific.** Every behaviour rule comes from the game's mechanics and what
  she can observe, so it holds on any map or WAD: no map names, sector or line numbers, or
  coordinates in a rule. The goal is a robust player bot an AI can later control, not one tuned
  to Doom/Doom II's maps; the failing map is only the test case for the rule.
- **Look it up** (DoomWiki, secret lists) before calling anything unreachable or broken. DoomWiki
  and Fandom refuse automated fetches: ask the user to paste the page, they're happy to.
- **No rules that depend on lives left.** Lives are a test setting; the goal is everything on one
  life. Risk rules must hold on any life.
- **Exploration paradigm:** she plans only from what she has seen (the automap rule), learns
  effects by using/seeing them. Keep the omniscient planner underneath; knowledge filters it.
- **Combat: realistic now** (the user's, 2026-11-09; it was "survivable first, realistic later").
  The fight goes by what she has seen, heard and learned, as the exploring does, not by the
  map's truth: an omniscient shortcut met in the fight is replaced with her knowledge, even at
  some cost in score (measure it and say what it cost). No new omniscient shortcuts. What's left to make
  realistic, and the order: `docs/REALISM.md` (after the restructuring; one at a time, each
  scored; a very large drop: look for a bug in the change first).
- **Barrels:** any barrel she's seen is cleared (no monster in sight), from out of every blast of
  its chain; shotgun within ~320 with 12+ shells, else single pistol taps (first shot is
  accurate). Walls/switches shot with pistol taps.
- **Rust only**, idiomatic, no `#[allow]`s, no warnings: `cargo clippy --release --all-targets`
  clean. The code is rustfmt-formatted (its defaults): `cargo fmt` before every commit,
  `cargo fmt --check` clean.
- **A refactor changes nothing she does**, and that's checked: `tools/parity.sh save` with the
  build before, `tools/parity.sh check [BINARY]` with the build after (24 fixed-seed runs, half
  a minute); a comment pass, `tools/code_only.py --check`. The cleanup under way:
  `docs/CLEANUP_PLAN.md`.
- **Commit to `main`** when a change is tested and no worse. Commit messages say what failed,
  why, what the rule is, and the before/after numbers.
- **Builds:** Linux (Fedora 45 under WSL, VS Code over WSL) is the main machine now; Windows
  still builds. The binary is `out/build/release/evil-boom` (`evil-boom.exe` on Windows); the
  user often runs it with the viewer open. After a change: `cargo build --release --target-dir
  ../out/build` from `native/`. Engine: `tools/build-engine.sh` (Linux, or MSYS2 MINGW64). On
  Windows the running exe can't be overwritten: rename it aside first
  (`evil-boom.old.<time>.exe`). On Linux don't rebuild while a sweep or episode runs: they
  start the binary again for each map. WADs: `~/.local/share/evilboom/base/...` (Steam layout; the old
  `evildoom` folder is still found if there's no `evilboom` one).
- **Seeds:** real play uses random seeds; a fixed seed (`--seed N`, default 0) is for testing.
  Try several seeds wherever RNG matters (any fight; shot barrels). No-monster, no-barrel runs are
  seed-independent: one run per code version is enough.
- Don't test on TNT or Plutonia: they're held out for real-world testing. Doom and Doom II only.

## Testing

- **The score is the measure of progress** (the user's, 2026-11-09; it replaces the old
  regression set of episodes, a Doom II HNTR sweep and arenas): `evil-boom score` plays all 68
  levels (Doom E1-E4, Doom II) from a pistol start, one life, mortal, on HMP and UV, 8 random seeds
  each (1088 runs, ~17 min at `--jobs 12`; a progress window pops up). A run scores 1 for an exit,
  else half its kill share; the score is the mean over level-skills. Consistency counts too: the
  levels beaten on every run, and each level's spread. `score-report A.tsv B.tsv` compares sets
  (TSVs in `out/score/`). Its own noise: about +-0.008 a measurement (seven sets of near-identical
  builds: 0.428-0.448), so one set against another needs 0.02-0.025 to be real; for smaller
  effects, two sets a build; a single level's points move about +-0.07 by seed alone. Judge a change by it; commit
  messages give the before/after score.
- **Comparing two builds: the same runs** (2026-11-09). `evil-boom score --like out/score/base.tsv
  --out out/score/NAME.tsv` plays the very runs of the baseline (its 1088 levels, skills and
  seeds); `score-report out/score/base.tsv out/score/NAME.tsv` then compares run for run. A run
  is the same every time on its seed (all 1088 came out the same to the tic on a second go), so
  a change that touches nothing scores exactly 0.0000 different, and one that touches a few runs
  shows only those: use this for every change now, not sets on seeds of their own. After a change
  that's kept, its set is the next baseline (copy it to `out/score/base.tsv`).
- **Arenas aren't part of the score** (the user's: too artificial). Use them while developing a
  fight idea, not to judge it.
- **No other full sweeps without asking.** Up to 8 chosen levels (or seeds) per test is fine.
- For a movement/exploration fix: that level alone, a good loadout so combat doesn't confound it
  (`--carry 287,200,50,0,20,1,100,100,1`; format in docs/DEBUGGING.md), several seeds, one at a
  time (each run-map overwrites `out/explore/<IWAD>_<MAP>.txt`), then the score.
- **Parity checks** (a refactor meant to change nothing): a handful of `run-map`s on fixed seeds
  compared line for line with the old build's; runs are deterministic, so that's enough.
- 12 parallel jobs (`--jobs 12` on this laptop).

## Where the fight's code is

`tactical/combat/tree.rs` is the fight, a node a decision (see its header); `combat/arsenal.rs`
which weapon; `combat/style.rs` how to fight with it; retreats are `Retreat`/`RetreatKind` in
`tactical/player.rs`. New behaviour goes in as a node (or a style, a rule in `best_gun`), not as
another case in a chain. What the rest of her does about a fight decision goes by its `Move`
(`combat/moves.rs`), never by its log label. A number that's a judgement (how near, how long,
what it's worth) goes in `tactical/tuning.rs` (`Tuning`, read as `self.tune.<group>.<field>`;
`evil-boom tuning` prints the defaults, `--tuning FILE` changes any of them for a run); a fact
about the engine stays a `const` where it's used.
How things stand this tic (outmatched, what's against her, short of ammo, surrounded) is
`Combat::now` (`combat/assess.rs`), worked out once as she looks: a rule reads it, it doesn't work
its own answer out.

## Where things stand (2026-11-10)

The restructuring (seven steps: typed decisions, `Tuning`, one assessment a tic, one of each
skill, the arbiter, the advisor, the score's fixed seeds) is done: DECISIONS.md. Then realism
(the user's: "it's time for realism"; with a limit: "our goal is good streaming content", a
little cheating allowed where it plays better): `docs/REALISM.md` has the survey, the table of
every change's score, what the user saw watching twelve runs, and the work list from it.

How changes are measured now: `score --like out/score/base.tsv` (the same 1088 runs) and
`score-report base new` run for run; each run's counts beside it (FILE.stats.jsonl: what hurt
her and what she knew of it, turns, alerts); `out/score/screen.py` and `out/score/quarter.tsv`
(two seeds a level-skill, 4.5 min) for screening tuning files. A change that moves most runs
when its rule should touch few has a timing shift in it.

Score 0.400 (it was 0.445 before realism; 0.370 at its lowest, with monsters seen only in her
110-degree view). She senses by sight (in view, a second after, half a second of instinct), by
the game's own sounds (the engine patch reports them: a death by its cry, an attack, a missile
fired, a monster's steps), by a hit's side and by bumping into things, each a way to turn and
look. What the measuring found: she's hurt more slowly than she was (the fight keeps off ground
she's never been on) and dies as often, later: what's short now is what she kills with the
health and ammo she has (work list D: what to fight and what to take), not what she knows.

Done from the work list: A1 (footsteps), C1 (instinct), B1 (known ground), D5 (armor; a
needed pickup before a far keyed door). Open: A2-A5, B2-B4 (the fall back is built, off), C2
(tracking a walker by its steps), D1-D4, D6, D7; the survey's map-side items (M2-M6) and live
heights (O6); the three loops of DECISIONS.md's step 5; the advisor to be grown.

## Since (2026-11-11)

Score 0.460, 401 exits (`out/score/base.tsv`; run for run against it, as above). The cleanup
(`docs/CLEANUP_PLAN.md`): comments, formatting, the fight's, explorer's and follower's
structure done; its findings each scored (a table there: three kept, the pace one tried twice
and lost, two checked and no bug); what's left is structure with no failing run.

Next is Evil's control through the Neuro SDK: `docs/NEURO_SDK_PLAN.md` (every decision mapped
by how long its answer's worth having; five grains of control; the bot's questions; what
she's told; control of the game itself, cheats typed and never listed; notes from directing
levels by hand, which reorder its stages). Built of it so far, in `control/directed.rs`: a
replaced order says so and an errand resumes exploring; `then`; the idle fallback (`idle N`);
fights told (fast loss, a summary, secrets); `fight` / `doom_fight` (a gun, a target, hold
fire, a stance, each for a while); the bot's pick and lists in its order, `look` / `doom_look`,
`hints` and `idle` as `doom_policy` (`control/directed/look.rs`: the pick is the explorer's
next objective under the id it's ordered by; E1M2 finishes on the picks alone). What Evil
reads is in the second person, hers ("You found a secret"; "by instinct", "Suggested:": no
"she", no bot beside her); who's playing (`Character`) comes with the startup's acknowledgement. Kills and blasts
are told as moments (`control/directed/kills.rs`: a big monster dead, several at once and
how many gibbed, barrel chains and what they took, her own blast, the last monster); and levels begun and done, big jumps and how they came down, a teleport
into a crowd, out of ammo, crushers, monsters raised, with at most 6 moments a minute past
the urgent ones (`moments.rs`). Stage 1 of the SDK plan is done (its gate run: the GPT
stand-in on MAP07, 56 words a minute told her); `Situation` and typed events moved to stage 4
(questions). A directed explore pauses only while the fight has her legs. The plan's five questions are answered (its section 9):
with no action she waits and looks round; deadlines 3 s / 8 s; restart unlocked by no
progress; next level hers from the tally screen; cheats held off. And (changed) she starts the game:
level, skill, one level or on through, from what the console offers (`control/game.rs`,
`doom_game start`; offered at the title and when what she chose is over); `doom_game next` from
the tally screen (30 s, else on by itself) and `doom_game restart` after 90 s with nothing new. To direct a game by hand: `watch <wad> <map> --headless
--speed 1 --directed FILE`, orders appended to FILE (`control/script.rs`), reports in FILE's
`.out` twin (`me.txt` gives `me.out`).

Episodes 1 and 2 (HMP) have been finished by directed play with the idle fallback (`idle 6`)
and a dozen orders each; the notes are section 8 of the SDK plan. `watch --episode --lives N`
keeps its lives; a headless directed game restarts a level on a death and says so. Jitter:
measured as "quick darts" (STATS; `EVIL_DEBUG_DARTS`), two holds tried and left as tuning,
off (`steer.about_tics`, `steer.dart_tics`): each cost about 0.02 for a fifth to a third fewer;
standing still for a monster out of sight cost 0.028. The table is in CLEANUP_PLAN.md.

## The stream console (2026-11-10)

`evil-boom console` (a web page for the streamer; the game its child, `watch --control`, Doom's
own screens with `--present`, the stream's 1920x1080 window with `--stream`): `docs/CONSOLE_PLAN.md`,
and its DECISIONS.md entry. A console's game runs `out/build/release/evil-boom`: as with a
sweep, don't rebuild under it. Headless runs (run-map, sweep, score) don't touch any of it.

## The Linux port (done 2026-11-08)

Same results as on Windows (E1 3.6 levels, Doom II 7.9, the same per-seed endings). What it took:
the default WAD folder per platform (`config.rs`), the patch putting the `.so` beside
`dsda-doom.wad` (and building the WAD with it), `CMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF` for
Fedora's zlib-ng, minifb 0.27 -> 0.28 (0.27 segfaulted on X11 with AspectRatioStretch and
corrupted the heap on Wayland). Setup for both platforms: README. minifb is built X11-only (its
Wayland window has no decorations under WSLg); SDL catches SIGTERM, so close the watcher from its
window. The watchers (`watch`, `arena-watch`) play the game's sound by default (`--mute`, M key;
off while paused or not at x1); everything else is silent. Tests use `--jobs 12` (the user's call
for this laptop). Linux baselines (HNTR, one life): E1 3.6, E2 2.8 (2/8 through), E3 3.9, Doom II
7.9; arenas combat 120/128, doom2 227/240.
