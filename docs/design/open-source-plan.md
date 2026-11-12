# Open-sourcing Evil BOOM: survey and cleanup plan

Date: 2026-11-12.

> **Status:** in progress. Decided (section 2): keep the history, its author emails rewritten
> before publishing; copyright "Quarkboy"; Evil Neuro's name stays, an unofficial fan project;
> CLAUDE.md trimmed; the design notes organised and trimmed; binary releases for Windows and
> Linux from GitHub's CI. Done: licensing, privacy, cleanup, the docs, the release layout, the
> CI and release workflows (steps 2-5, 7). Left: the Windows build checked on a real machine or
> CI's first run (step 6), the trimmed CLAUDE.md approved, the author emails rewritten, and the
> final checks (step 8).

The goal: a public GitHub repository under GPL-3.0-or-later that a stranger can read, build on
Linux or Windows, test, and contribute to, with nothing in it we don't want public.

## 1. What the survey found

### The repository

- 177 tracked files: the crate `native/` (127 source files, ~67k lines of Rust, 3 integration
  test files), the engine patch `dsda/` (patch, `evil_api.c`, an old spike crate), 12 docs, 11
  tools, 7 arena files, 2 configs, `README.md`, `LICENSE` (GPL-3.0 text), `CLAUDE.md`.
- History: 540 commits, 43 MB, no large binaries. The biggest blobs are the many versions of
  `docs/design/decisions/` (243 KB now). The earlier Python implementation (`src/evildoom`, ViZDoom
  spikes, Python tests) is deleted from the tree but still in the history.
- `out/` (75 GB of builds, logs, recordings, score sets) is ignored. So are `third_party/`
  (the DSDA-Doom checkout), `wads/`, `.worktrees/`, `.claude/worktrees/` and
  `config/*.local.json`.

### Secrets and personal details

- **No secrets anywhere**, in the tree or in any of the 540 commits: no API keys, tokens or
  `.local.json` files were ever committed. Keys are read from `config/*.local.json` (ignored) or
  environment variables named in the committed configs (`OPENAI_API_KEY`, `JEV_API_KEY`).
- **No personal paths** (`/home/samuelp`, `C:\Users\...`) in tracked files or the history.
- **Names and emails:** `dsda/evil_api.c` carries "Copyright (C) 2026 Samuel Pinansky". The commits
  have two author emails: a personal Gmail address (317 commits) and a J-Novel Club work address
  (223 commits). Both become public with the history.
- **Stream operations:** `docs/design/console-plan.md` mentioned how the stream's other consoles are
  reached from the web. That's about someone else's setup and should go (done).
- **Small personal asides:** "this laptop" (CLAUDE.md); provenance notes in code comments ("the
  maintainer's", 10 places in 9 files); dated session notes throughout the docs.

### Licenses

- **Our code:** GPL-3.0-or-later, stated in `LICENSE`, the README and `Cargo.toml`. Only
  `evil_api.c` has a license header.
- **DSDA-Doom** (not vendored; fetched at a pinned commit, `eb8eb98`, and patched): all 469 GPL
  source files say "version 2 of the License, or (at your option) any later version", so they can
  be used under GPL-3.0. The rest is compatible too: Nuked OPL3 (LGPL-2.1-or-later), Raven's
  Heretic/Hexen code (GPL), `md5.c` and `win_opendir.c` (public domain), `scanner.cpp` and
  `gl_sky.c` (BSD-style), `xs_Float.h` (a published snippet, permissive). Nothing is
  GPL-2.0-only. Our patch and `evil_api.c` are GPL-3.0-or-later; the combined engine library is
  GPL-3.0.
- **Engine's runtime libraries:** SDL2 and SDL2_mixer (zlib), libsndfile (LGPL-2.1+), libzip
  (BSD), zlib. All compatible. They matter only for binary releases (section 4).
- **Rust dependencies** (151 crates): MIT and/or Apache-2.0 (most), BSD-2/3, ISC, Zlib,
  Unicode-3.0, Unlicense, and CDLA-Permissive-2.0 (the root-certificate data). All compatible with
  GPL-3.0. Apache-2.0 is compatible with GPL-3.0 (not with GPL-2.0), so we couldn't move down to
  GPL-2 later.
- **Code derived from other ports:** the movement simulator (`jumps/physics.rs`) says it "follows
  ZDoom 2.8.1 ... function by function"; the line-of-sight (`los.rs`) and view (`sight.rs`) code
  follow ZDoom's P_CheckSight and R_RenderBSPNode. These are our own Rust, not copied text. Their
  logic comes from id Software's Doom source (GPL since 1999, and in DSDA-Doom). ZDoom's
  changes were Randy Heit's, under a BSD-style license, and are under GPL-3.0 in GZDoom today. So
  it's compatible, but the attributions point at ZDoom 2.8.1, which as a whole was not
  GPL-licensed. They should point at the GPL sources (section 4).
- **Game data:** no WADs or game assets are in the repository or the history. The arena files are
  our own text descriptions. The console page embeds no images or fonts. The viewer's font is the
  `font8x8` crate (public domain).

### Building and testing

- **Linux** builds and tests clean (clippy, rustfmt; 49 unit tests pass in under a second).
- **Windows:** the README documents it (MSYS2 MinGW64 for the engine, rustup for the rest). The
  code has only one Windows-specific block, which finds the engine's DLLs. It hasn't been built
  on Windows since the move to Linux (2026-11-08), and I couldn't check it here: one dependency
  (`ring`, through the HTTPS client) needs a Windows C compiler to cross-check.
- **Tests:** 49 unit tests need nothing. The integration tests (`native/tests/`) need the IWADs
  and the built engine, and are skipped without them. The tools we actually judge changes with,
  `step.sh`, `screen.py` and the quarter sets, live untracked in `out/score/`, so nobody else
  could follow the testing procedure CLAUDE.md describes.
- **Out of date:** the README's layout list (it omits `neuro`, `control`, `console`, `score`,
  `advisor`, `arena`, `survey` and more) and its link to the Rust review notes.

### Docs

| file | size | what it is |
|---|---|---|
| `docs/design/decisions/` | 243 KB | the full dated decision log since the start |
| `docs/design/neuro-sdk-plan.md` | 57 KB | the Neuro control layer: plan, stages, notes |
| `docs/design/realism.md` | 47 KB | survey of omniscient shortcuts, the realism work list |
| `docs/design/cleanup-2026-11.md` | 21 KB | the previous cleanup, with its scores |
| `docs/design/console-plan.md` | 19 KB | the stream console plan |
| `CLAUDE.md` | 15 KB | working rules plus "where things stand" session status |
| `docs/design/policy-survey.md` | 15 KB | stage 7 settings survey |
| `docs/design/neuro-integration-plan.md` | 15 KB | the first integration plan (superseded) |
| `docs/design/rust-review.md` + `rust-review-results.json` | 24 KB | a code review and its raw output |
| `docs/DEBUGGING.md` | 12 KB | how to trace a failed run (useful to anyone) |
| `docs/design/tuning-ranges.md` | 8 KB | a tuning sweep's results |
| `docs/NEURO.md` | 4 KB | testing against Gary |

They're valuable but written as a working diary: dated, in session order, with plans and status
mixed. A newcomer has no entry point besides the README.

### Odds and ends

- `dsda/spike/`: the first in-process engine experiment (its own crate), superseded by `native/`.
- `docs/rust-review-results.json`: raw tool output.
- `tools/`: `build-engine.sh`, `parity.sh`, `code_only.py`, `rename.py` and the Gary/GPT
  scripts are in use. `compare-runs.py` predates the score. `autoaim_arena.py` and
  `crate_arena.py` generated two arena files and aren't needed to use them.
- `Cargo.toml` has no `repository`, `readme`, `homepage` or `rust-version`.

## 2. Decisions for you

1. **History: keep it, or start fresh?**
   - *Keep it* (recommended). It's clean of secrets and shows how the project grew. Before
     publishing, rewrite the author emails with `git filter-repo --mailmap` if you don't want
     the J-Novel Club address (or either address) public.
   - *Fresh start:* a single first commit, with the old history kept privately. Simpler, but the
     decision log loses its commits.
2. **Your name in copyright notices.** "Samuel Pinansky" or a handle?
3. **Naming and Neuro references.** The project is named and written for Evil Neuro and talks
   about Vedal's stream. Using the names to describe what it's for is normal, but it reads as
   affiliated. Is publishing it under these names fine with Vedal? Either way the README should
   say it's an unofficial fan project unless it's official. The GPT stand-in's persona
   (`config/neuro-gpt.json`) plays the character; keep it, or make it a neutral example?
4. **CLAUDE.md.** My proposal: keep it, trimmed to the working rules (no session status), with the
   shared rules moved to `CONTRIBUTING.md` and CLAUDE.md pointing there. AI-assisted
   contributors get the same rules as everyone. The alternative is to drop it.
5. **Planning docs.** Keep them as a "design notes" archive (proposed), or condense them into the
   architecture docs and drop the rest?
6. **Binary releases on GitHub,** or source only for now? Releases add obligations (section 4) and
   a packaging job.

## 3. The proposed layout

```
README.md                what it is, what it does, a quick start, links; the disclaimers
LICENSE                  GPL-3.0 (as now)
CONTRIBUTING.md          how changes are judged (trace then fix, general rules, the score,
                         parity), code style (rustfmt, clippy, no #[allow]), commit messages
CLAUDE.md                (if kept) AI-assistant notes: points to CONTRIBUTING and the docs
THIRD_PARTY.md           DSDA-Doom and its pinned commit, the engine's libraries, the Rust
                         dependencies' licenses (generated), the derived-code attributions
docs/
  BUILDING.md            Linux (Fedora, Debian/Ubuntu), Windows (MSYS2 + rustup), the engine
                         build by hand, WAD folders, environment variables, troubleshooting
  TESTING.md             unit tests, integration tests with IWADs, parity, the score and
                         run-for-run comparisons, screening with the quarter sets, arenas
  ARCHITECTURE.md        the module map (from README and RUST_REVIEW), the 35 Hz loop, how
                         planning/knowledge/fight/follower fit, the engine boundary
  USAGE.md               the commands (sweep, run-map, watch, episode, score, record, drive,
                         --pwad ...) and their options, moved out of main.rs's header
  NEURO.md               the Neuro SDK integration: actions, what Evil is told, policies,
                         testing with Gary / neuro-mock / the GPT stand-in (../NEURO.md folds in)
  CONSOLE.md             running the stream console (how to use it; the plan's design part
                         goes to design/)
  DEBUGGING.md           as now, tidied (paths, current flags)
  design/
    README.md            an index: what each note is, when it was written, if superseded
    decisions.md         the decision log (the decision log), as is or condensed
    realism.md, neuro-sdk-plan.md, neuro-integration-plan.md (superseded), console-plan.md,
    cleanup-2026-11.md, policy-survey.md, tuning-ranges.md, rust-review.md
tools/
  build-engine.sh, parity.sh                 (as now)
  score/step.sh, score/screen.py             (moved from out/score, paths made relative)
  score/quarter.tsv, score/quarter-b.tsv     (the screening sets: small, needed to repeat them)
  gary.sh, openai-relay.py, gpt-evil.sh      (Neuro testing)
  dev/code_only.py, dev/rename.py            (code tools)
  arenas/autoaim_arena.py, arenas/crate_arena.py   (or removed: the arena files are kept)
.github/workflows/ci.yml  Linux and Windows: engine build, cargo build, clippy, fmt, unit tests
```

Removed: `dsda/spike/`, `docs/rust-review-results.json`, `tools/compare-runs.py` (after a
check that nothing uses it).

## 4. The work, in order

Each step is its own commit. A step that touches code is checked with `tools/parity.sh` (same
runs, same results) and `tools/code_only.py --check` where it's comments only.

**Step 1. Decisions** (section 2). The rest waits on 1, 3, 4 and 5.

**Step 2. Licensing.**
- SPDX headers: `// SPDX-License-Identifier: GPL-3.0-or-later` and a copyright line at the top
  of every source file (Rust, C, scripts). Comments only; checked with `code_only.py --check`.
- Re-point the derived-code attributions. `physics.rs`, `los.rs`, `sight.rs`, `wad.rs` and
  `movers.rs` should name what they follow as it's available under the GPL: id's code as in
  DSDA-Doom, and GZDoom (GPL-3.0) for the ZDoom-only behaviours. I'll check each ZDoom-only
  behaviour named exists there (aircontrol, FSlide, the sight start height, FWallCoords) and that
  no code text (comments, tables) was copied.
- `THIRD_PARTY.md`: DSDA-Doom (commit, license, the patch's role), the engine's libraries, the
  Rust crates' licenses (generated with `cargo about` or from `cargo metadata`), the
  derived-code notes.
- `Cargo.toml`: `repository`, `readme`, `rust-version`, `homepage`.
- If binary releases (decision 6): each release ships the GPL text, `THIRD_PARTY.md`, and a link to
  the exact source (this repo's tag, plus DSDA-Doom's commit and our patch). It includes
  `dsda-doom.wad` (GPL, built from DSDA-Doom's data). On Windows it bundles the MSYS2 DLLs: their
  licenses (SDL2 zlib, libsndfile LGPL) go with them, with source pointers for the LGPL ones.

**Step 3. Privacy.**
- `docs/design/console-plan.md`: remove the stream-infrastructure details.
- Code comments: "the maintainer's" becomes the reason it was chosen ("watched: ...", "chosen for the
  stream: ..."). Comments only, checked as above.
- "this laptop" and similar go with the CLAUDE.md trim.
- If chosen: the history's author emails rewritten (`git filter-repo --mailmap`), done last
  before publishing, on a fresh clone.

**Step 4. Repository cleanup.**
- Remove `dsda/spike/` and `docs/rust-review-results.json`; check and remove
  `tools/compare-runs.py`.
- Move the score scripts and quarter sets into `tools/score/`, made to run from any checkout (no
  absolute paths; the binary given or found). The baseline set for run-for-run comparisons stays
  out of git: it's per machine and per build, and TESTING.md says how to make one.
- `.gitignore`: tidy the leftovers (ViZDoom, Python packaging) once the Python tools that remain
  are known.

**Step 5. Docs.**
- New: `README.md` rewritten as the front page (what it is, a short demo description, quick
  start for Linux and Windows, the commands that matter, links, license, disclaimers). Also
  `BUILDING.md`, `TESTING.md`, `ARCHITECTURE.md`, `USAGE.md`, `NEURO.md`, `CONSOLE.md`,
  `CONTRIBUTING.md`, `THIRD_PARTY.md`.
- Moved to `docs/design/` with an index: the plans, surveys and the decision log. Each gets a
  line at the top saying when it was written and whether it's current or superseded.
- `CLAUDE.md` (if kept): the working rules that are this project's, not session status. The
  "where things stand" sections move into the design notes or are dropped.
- `docs/DEBUGGING.md`: check every flag and path it names still exists.
- The main.rs header's command help moves to `USAGE.md`, leaving a short summary in `--help`.

**Step 6. Windows.**
- A build on a real Windows machine following the new `BUILDING.md` word for word: engine in
  MSYS2 MINGW64, Rust with rustup (MSVC toolchain, the default). Fix what breaks and note it.
- Check the paths that differ: the WAD folder default, `EVIL_MINGW_BIN`, the `.exe` re-launch in
  `score`, `episode` and `survey`, the console's child process, ffmpeg for `record`, the viewer's
  window and sound.
- The rename-aside note for a running `evil-boom.exe` goes in BUILDING.md.

**Step 7. CI** (`.github/workflows/ci.yml`).
- Linux (Ubuntu runner): engine build, `cargo build`, `cargo clippy --all-targets -- -D
  warnings`, `cargo fmt --check`, `cargo test --lib`.
- Windows: MSYS2 for the engine (`msys2/setup-msys2`), rustup for the rest, the same checks.
- No IWADs in CI: the integration tests skip themselves, and TESTING.md says how to run them
  locally.

**Step 8. Before going public.**
- A fresh clone on each OS, following README → BUILDING → TESTING exactly.
- A last scan of the tree and history for keys, emails and paths (the survey's commands, kept in
  the plan's appendix).
- GitHub settings: description, topics, the license detected as GPL-3.0, issue templates
  (optional), Discussions (optional).

## 5. Appendix: the survey's checks

Run again before publishing:

```sh
# secrets in the tree and the history
git grep -nIiE 'sk-[a-z0-9_-]{16,}|ghp_[a-z0-9]{20,}|api[_-]?key"?\s*[:=]\s*"[^"]{8,}'
git log --all -p | grep -iE '^\+.*(sk-[A-Za-z0-9_-]{20,}|ghp_|xox[bp]-)'
git log --all --name-only --format= | grep -iE 'local\.json|\.env$|secret|token|\.key$|\.pem$'
# personal paths and emails
git log --all -p | grep -E '^\+' | grep -oiE '/home/[a-z]+|C:\\\\Users\\\\[^\\\\]+' | sort -u
git log --all --format='%an <%ae>' | sort | uniq -c
# the Rust dependencies' licenses
cd native && cargo metadata --format-version 1 --locked | python3 -c \
  "import json,sys,collections; c=collections.Counter(p.get('license') for p in json.load(sys.stdin)['packages']); [print(n,l) for l,n in c.most_common()]"
```
