# Evil DOOM — Decision Log

Integration for Evil Neuro (and Neuro-sama) to play DOOM on stream via the
[Neuro SDK](https://github.com/VedalAI/neuro-sdk).

## Decided

### Control model: real-time commander (no pausing)
The game never pauses for Evil. Three layers:
- **Strategic (Evil, ~5–20s):** high-level actions + an "intent" string.
- **Tactical (fast classifier, ~2–5 Hz):** Jev-style decision model, conditioned on Evil's
  current intent, picks targets / engage-retreat / weapon / next waypoint.
- **Motor (deterministic code, 35 Hz):** aim, strafe, dodge, path following, use.
  Must keep functioning on heuristic fallback if the classifier is slow/down.

Classifier access goes through a SystemOne-style interface (`state` + `questions`) so hosted
(Jev) and local models (e.g. Julia-1, Laya) are swappable via config.

### Perception: engine data only
Never rely on Evil's vision (Vedal controls whether it's on). State comes from engine data,
narrated to text. Knowledge base / fog of war: items, doors, switches become available to her
context only after she has "seen" them, and context is pruned to what's relevant.

### Map preprocessing
Preprocess WADs into a nav graph (BSP subsectors as nav polygons) with reachability under every
key state. Track all 6 keys individually (vanilla color locks accept either card or skull; Boom
generalized locks can distinguish them → up to 64 states). Best-effort static graph, verified
and corrected at runtime.

### Actions
Dynamic registration (re-register enums as the situation changes). Fuzzy acceptance of her
input where it can be sensibly repaired (`success: true` + correction note); reserve
`success: false` for truly impossible requests, since failures trigger a full force retry.

### Language / topology
Python first, possibly ported later. External bridge process on the same machine as the engine.
All engine access goes through an engine-adapter interface so the engine can be swapped.

### Content & compatibility target
- **Phase 1:** stock IWADs from the user's Steam install, plus whatever map/mod features
  **ViZDoom** supports. Start with ViZDoom for the spike.
- **Expansion target:** full **MBF21** compatibility (e.g. Eviternity II, which targets MBF21 /
  DSDA-Doom). Eviternity (2018) targets MBF.
- **Expected:** we will likely need to switch from ViZDoom to a proper modern port
  (DSDA-Doom, UZDoom/GZDoom, Woof!, etc.) after the spike. The engine-adapter interface
  exists to make that switch cheap.

### Testing
LLM stand-in for Evil (Gemini Flash or similar via user's API key) with simulated latency
(3–15s) and a "fake Evil" persona. Also SDK tools: Randy, Tony, Jippity, Gary.

### Deferred
Stream-specific features (overlay, twin mode, Evil-specific flavor) — after a working prototype.
Pacing parameters (context frequency, force cadence, priorities) — tune on the prototype.

## ViZDoom compatibility findings (2026-11-03)
ViZDoom 1.3.2 (pip wheel, Python 3.13, Windows), engine = ZDoom 2.8.1 (2016).
Probe script: `spikes/vizdoom_probe.py`. Steam install: `C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom`.

Tested (headless load + objects/sectors/labels state):

| Content | Result |
|---|---|
| `base/DOOM.WAD` (Ultimate Doom 1.9) E1M1 | OK |
| `base/doom2/DOOM2.WAD` MAP01 | OK |
| `base/plutonia/PLUTONIA.WAD` MAP01 | OK |
| `base/master/wads/VIRGIL.WAD` MAP03 | OK |
| `rerelease/doom.wad`, `rerelease/doom2.wad` | OK (same map data as classic) |
| `rerelease/nerve.wad` (No Rest for the Living) | OK |
| `rerelease/sigil.wad` E5M1, `rerelease/sigil2.wad` E6M1 | OK |
| `rerelease/masterlevels.wad` MAP01 | OK |
| `rerelease/id1.wad` (Legacy of Rust, id24) MAP01 | Loads, but 42 objects categorized "Internal" (likely unknown id24 things) |
| `rerelease/id1.wad` MAP07 | **Crash (segfault)** — treat id24 as unsupported |

From ZDoom 2.8.1's known feature set (not yet tested here): Boom generalized linedefs, most
MBF features, DEHACKED/BEX, Hexen-format maps, UDMF, ACS, DECORATE. **No MBF21, no id24.**

What ViZDoom exposes at runtime:
- Every object in the level (ground truth): id, name, category (Monster/Key/Weapon/Ammo/...),
  position, angle, velocity, sector_id.
- Labels buffer = objects actually rendered on screen → natural "has Evil seen it" signal for
  the fog-of-war knowledge base.
- Sectors with floor/ceiling heights (live → can observe doors/lifts opening) and their lines.
- Game variables (health, armor, ammo, position, angle, weapon, ...), console commands.

What it does **not** expose: line specials/tags, line flags, sector specials (damaging floors,
etc.), thing flags. Lines only have endpoints + `is_blocking`.
→ **The map preprocessor must parse WADs directly** (LINEDEFS/SIDEDEFS/SECTORS/THINGS/SSECTORS/
NODES). That's engine-independent anyway, so it survives the later engine switch. ViZDoom
provides the live dynamic state; our parser provides the static map knowledge.

## WAD parsing spike findings (2026-11-03)
Own parser in `src/evildoom/wad/` (no third-party deps): lump directory, Doom-format map lumps
(things, linedefs, sidedefs, vertexes, sectors, standard segs/subsectors), line special
classification (vanilla 1–141, Boom generalized bitfields incl. 6-key locks, Boom extended
exits/teleports) and sector specials (damage, secret). Hexen/UDMF maps and extended nodes are
detected but not parsed yet. Spike: `spikes/wad_spike.py` (summary + annotated PNG in `out/`).

- Every map in DOOM, DOOM2, TNT, Plutonia, NRFTL, Sigil, Sigil II, Master Levels parses.
- **Stock content uses zero Boom generalized specials** — the vanilla table covers Phase 1.
- Legacy of Rust (`id1.wad`) uses 432 generalized lines plus many Boom extended and id24-only
  specials (1023, 1024, 1080, 2048, 2083, …) → firmly in the modern-engine phase.
- "Affected sector" resolution: D1/DR specials act on the line's back sector; everything else
  on sectors with a matching tag. This gives us switch → door/lift/floor links for the nav graph.

## Navigation design (2026-11-03)
Code: `src/evildoom/nav/` (`navmesh.py`, `movers.py`, `reach.py`), spike `spikes/nav_spike.py`.

**Topology is static, heights are not.** Three separate concerns:
1. **Nav mesh (static):** one convex polygon per subsector, rebuilt by clipping the map bounds
   down the BSP tree, then by the subsector's segs. Portals = shared polygon edges, split per
   linedef. Linedefs are matched to polys geometrically (not via seg ownership), because
   nodebuilders round split vertices to integers (~1 unit off the line) and sometimes put segs
   into zero-area subsectors on partition lines.
2. **Passability (dynamic):** a portal A→B is crossable iff, for some pair of height states,
   step up ≤ 24, opening ≥ 56, and the line isn't impassable/block-players. Any drop is allowed,
   so drops > 24 are one-way edges.
3. **Heights:** `HeightStates` = sector → set of (floor, ceiling).
   - Runtime: one state per sector = live ViZDoom sector heights; re-plan when they change.
   - Planning: initial heights plus every state a mover can produce (`MoverModel`, mirrors
     vanilla target rules: lowest-neighbor-ceiling − 4 for doors, stair chains, lifts, etc.).
     The "ever possible" union over-approximates on purpose.

**Doors vs. remote triggers.** Use-doors (DR/D1) are handled inside A*: a door sector counts as
open only when entered from a poly the door can be used from (and the lock is satisfied).
Remote triggers (switches, walk-over lines, gun lines, boss deaths) are planned as route legs by
a best-first search over (triggers used, current position), scored by area reachable *from
where you'd stand afterwards* (some triggers sit past one-way drops). An upper-bound check
(fire everything reachable) skips hopeless searches.

**Pathing.** A* over polys (cost = distance between portal entry points, ×4 on damaging
floors; heuristic disabled on maps with teleporters), then funnel-algorithm string pulling with
portals shrunk by the player radius.

**Key progression.** Alternate "fire every reachable trigger to a fixed point" with "pick up
every reachable key"; each key set is a stage. Boss-death specials (E1M8, E4M6, E4M8, MAP07,
Keens anywhere) are pseudo-triggers activated from the monsters' positions.

Known approximations: "raise by shortest lower texture" uses +64 (needs TEXTURE1/2); player
width ignored for narrow portals; gun triggers treated as "adjacent"; Boom generalized movers
have no height prediction; teleport destination uses the first type-14 thing in the tagged
sector.

## Route execution + demos (2026-11-03)
`nav/walkthrough.py` turns a level plan into steps (walk path, then use / shoot / cross /
pickup / exit). `motor/follower.py` executes them tic by tic, engine-independently
(Observation in, Controls out); `engine/vizdoom_engine.py` adapts ViZDoom and records/replays
demos (`new_episode(path)` / `replay_episode(path)`). Spike: `spikes/play_route.py`.

Lessons from driving real routes:
- **Pressing use on a moving door reverses it.** Only press when the blocking sector's heights
  have been still for a moment.
- **Doors hide behind thin door-track sectors**, so live passability is checked for every
  crossing within reach along the path, not just the next one.
- **DOOM collides with a square box**, so paths keep 24 units from portal ends, convex wall
  corners get detour waypoints, and waypoints are nudged ≥ 18 units off walls. Pushing off a
  *concave* corner slides the point onto the other wall, so pushes must pass a clearance check.
- Use traces 64 units straight ahead: walk up and square up to the door before pressing.
- ViZDoom ignores turn input for the first few tics of an episode (warm-up).
- Turning: `TURN_LEFT_RIGHT_DELTA` is degrees/tic, positive = clockwise. Sector ids match the
  WAD's sector indices.

## Survey of maps that don't plan to the exit (2026-11-03)
39/66 stock DOOM + DOOM II maps with an exit line plan start-to-exit. Of the other 27,
diagnosed by relaxing one constraint at a time in the progression analysis:

- **Planner search (11 maps):** progression reaches the exit but `plan_level` can't sequence
  it — E2M2, E2M7, E3M2, E3M6, E3M7, E4M8, MAP12, MAP18, MAP23, MAP28, MAP32. Biggest bucket;
  likely the "ever possible" union hiding one-shot/ordering constraints, plus search budget.
- **Run-off jumps (11 of the remaining 16 are fixed by adding them):** sole fix for MAP15 and
  MAP29; also fixes E4M1, E4M2, E4M6, E4M9, MAP06, MAP14, MAP17, MAP20, MAP22.
- **Texture-height floor raises:** now implemented (TEXTURE1/TEXTURE2 heights; "-" skipped
  as in ZDoom). E4M7's raise-to-texture lines go +128, not the old +64 guess — but E4M7 still
  doesn't plan: its real blocker is upstream (switch 915's activators are never reached) and
  was misattributed to texture heights at first. Needs a hands-on look.
- **Possible width false positive:** MAP06's lift (sector 122) to pit portal is rejected as too
  narrow — to verify.
- **Unexplained so far:** MAP11 (only fix: allow 32-unit steps), MAP13 (96-unit steps, no
  mover), MAP04, MAP09. Worth a hands-on look in `tools/pathfind_lab.py`.
- Width and headroom relaxations fix nothing: the width check isn't over-blocking.

## Jumps, verification and planner guidance (2026-11-03)
- **Run-off jumps** (`Navigator._find_jumps`): vanilla thrust/friction for run-up speed, flight
  per tic like P_XYMovement/P_ZMovement, with the whole 32x32 box: you keep ledge height until
  the box leaves the ledge and land as soon as its front reaches something within a step. Angles
  0/±30/±45/±60 off each edge, takeoffs every 16 units. Moved heights a jump needs (a lowered
  fence) become `Jump.requires`; the planner only uses the jump once the world has them.
- **Engine verification** (`engine/jump_check.py`): every jump is tried in ViZDoom (warp, line
  up, sprint, must touch down on the predicted sector); disproved jumps are dropped, results
  cached in `out/jumpcache`. Typically 10–20% of simulated jumps fail.
- **Relaxed-plan guidance**: the search tries the triggers/keys a relaxed plan (ordering
  ignored) depends on first. Fixed every "search gave up" map (E2M2, E4M8, MAP12, MAP15, MAP18).
- **Where to stand matters**: trigger and key actions try several spots (one per sector) and
  keep the one leaving the most reachable (E4M1's red door pressed from the nukage pit).
- **Pickups across railings**: box overlap per axis, standing 17 back from impassable lines
  (E4M7's red skull on a balcony).
- Shootable lines usable from line of sight; boss deaths from where the boss can walk/teleport.

## Motor execution: human-looking movement + replanning (2026-11-03)
The follower (`motor/follower.py`) was rewritten so routes look like a person playing (it'll be
watched on stream) and recover when things go wrong.
- **Turning** is rate-limited like a keyboard player (≤ 8°/tic, vanilla's fast keyboard turn is
  7) with acceleration limits, and aims at a point 128 units ahead on the path, so the view
  swings into corners early instead of snapping at waypoints. The old follower turned at 25°/tic
  and reversed direction 50–100 times a minute; now 95th-percentile turn is 5–7°/tic and a few
  reversals a minute.
- **Movement is decoupled from facing.** A velocity controller works out the thrust needed
  against DOOM's momentum (next move = 0.90625 × last move + thrust, thrust 1.5625/tic at
  input 50) and splits it into forward + strafe for the current view angle. Input magnitude is
  capped at 50 (no straferun boost); moving more than 40° off the view costs speed, so it
  strafes a little round corners but never runs sideways. Strafe input: positive = right.
- **Speed planning**: slows before sharp corners (combined turn over 48 units), stops at
  goals, slows to a stop in front of closed doors.
- **Steering look-ahead is clearance-checked**: steer for the farthest point (up to 72) whose
  straight line keeps the square box clear of walls, solid things and uncrossed ledges; else
  the next waypoint exactly. Cutting corners blindly snags the box; turning only at waypoints
  lets momentum swing wide and catch the next corner on S-bends.
- **Getting stuck**: a short smooth back-off away from what we're snagged on (no wiggling),
  twice, then replan.
- **Replanning**: off the path (> 96 units, > 40 outside its polys, or somewhere lower we can't
  step back up from, for 10 tics), stuck, or a door/lift that never opens → re-path the step
  from where we are (`find_path`, ~2 ms); if the goal isn't walkable, `Navigator.plan` a way
  there with triggers (lifts/switches out of a pit) and splice those steps in; failing that,
  replan the rest of the level (`walkthrough.plan_from`). The planner-model heights (initial +
  effects of triggers used so far) plus live heights are the world state; keys are tracked from
  pickup steps.
- Lessons: a rising lift goes back down if the player's box overlaps a neighbouring sector
  whose ceiling is too low, so wait squarely at the edge being crossed; measure door distance
  to the nearest part of the doorway (doors can be 128 wide); "cross" steps must actually cross
  their line; a narrow strip below a ledge can't be dropped into if the box reaches a
  low-ceiling sector beyond it first (E4M1).
- **Planner fixes found by driving**: path smoothing now checks every segment with the exact
  swept square box (separating axes) against static walls and solid things, and detours locally
  via spots diagonally off nearby corners (E1M2's barrel by a wall corner, E1M5's barrel next to
  an octagonal pillar, E3M1's lamp field); waypoints are kept off diagonal walls by the box's
  reach (22.6), not 18; walk-over teleports need their line to be steppable (MAP08's sits in
  front of a closed niche).
- **Moving-sector gates**: a portal can be too narrow only while some nearby moving sector
  blocks (E2M7's raised block beside a 24-wide strip, MAP19/MAP21/MAP26's key bars). Portals
  store the mover lines across them that would narrow them; `passable` treats such a line as a
  wall when its far sector can't be entered from where we are at those heights (a door we can
  open from here counts as open). Lines *alongside* a portal are ignored — BSP leaves thin
  strips next to doors that the player's centre never needs to stop in. The follower opens a
  gate door like any door (walk up, press its use line).
- More planner fixes found by driving: lines the path crosses (doors it opens) aren't walls
  for waypoint nudging (it was zig-zagging waypoints off both faces of a door); use range is
  measured to a poly's edges, not its corners (big polys run past short switch lines); standing
  spots for switches are the point of each candidate poly nearest the line, not its centre.
- More follower fixes: press use aimed at the nearest part of the line within 4° (a 64-unit ray
  from off to one side misses the middle); progress along the path is searched only a little
  behind/ahead of where we were (paths double back: walking off a teleport pad to re-enter it
  from its front side); arriving at a segment's end only counts once we've come along it.
- Lab (`tools/pathfind_lab.py`): shows the bot's current objective, recent log, replans, and
  the live planned path (minimap + zoomed overlay on the game view, Tab toggles); "Play level"
  plans and drives the rest of the level from wherever you are.

## Granular, timed plans (2026-11-03)
- **Outcomes are checked, not assumed.** The engine reports keys held (key items gone from the
  object list); pickups finish when the key is held. Switch presses are checked against the
  sectors they should move and retried closer (3 tries); walk-over lines are checked too.
- **Mover timing** (`MoverModel.timing`): vanilla speeds and waits — doors 2/8 u/tic with a
  150-tic wait, lifts 4/8 with 105, floors 1/4, stairs 1/4 or 4. Predictions match the engine
  within a few tics.
- **Steps carry expectations** (`walkthrough.Expect`: sector, heights, travel, hold, needed by
  the next path) and spell themselves out as sub-steps (`Step.substeps()`: walk to → press /
  cross / pick up → wait for sector X / get to sector Y within N s). The follower logs arrivals
  vs prediction, lateness, and windows closing; waits ("settle") for quick permanent moves the
  next path needs; replans at once when the window it relied on has closed.
- **Timing-aware planning** (`Navigator._timed_path`): after a trigger with a temporary effect,
  the next leg may only use that state if, at ~12 units/tic, it gets to the sector before the
  mover returns; otherwise it's planned without it (or the action is unusable there).
- **No jumps off things that move**: jumps needing a lift/timed door at a temporary height
  aren't planned; before any jump the follower waits for the takeoff floor to stop.
- **Solid decorations are cut into the nav mesh**, grown by the player's half-width, so the
  mesh is where the player's centre can be (A* routes round rows of pillars, and gaps the box
  can't fit vanish). They never move; moving sectors stay height-dependent (portal gates).
  Barrels are not cut — they can be shot away (MAP23 has a row sealing a corridor) — and stay
  soft obstacles handled by box-fit detours.

**Status (execution sweep, god mode, no monsters):** 29 of 64 planned stock maps exit, up from
13 with the old follower. Known remaining failure kinds:
- Boss/Keen maps (E2M8, E3M8, MAP07, MAP32 …) can't finish without monsters, as expected.
- Pillar fields and chains of solid things with gaps under 33 units (E4M9, MAP19): the mesh
  doesn't model them, and local detours/A* retries can't get round long chains. A real fix
  is to cut solid things (and narrow strips) into the nav mesh.
- Timed/sequenced movers (MAP16, E4M8 sector 75): waiting/replanning isn't timing-aware.
- Assorted single-map snags (see `exec_sweep.py` + `triage.py` in the session scratchpad
  approach: run each failing map, look at the first trouble).

## Jumps checked in a movement simulator, not the engine (2026-11-04)
- **Jumps must stick the landing.** A jump only counts if the player can brake to a stop on
  the landing sector (MAP14's strip 110 is 24 units deep: every angled jump onto it "touched"
  it and slid off). The check tries a *soft* sprint first (full speed to the edge, then
  coast), then pushing all the way; one that slides off is retried running *straight across*
  onto the next floor (MAP14: over 110 onto 98, as a human does it). The follower sprints
  exactly as the check did (`Jump.soft`, `Jump.through`).
- **The check runs in `nav/physics.py`**, a port of ZDoom 2.8.1's player movement (thrust and
  1/256 air control, sub-stepped P_TryMove with step-ups from the current z, SlideMove's
  velocity rule, ground-only friction, P_ZMovement gravity) on our geometry. Reference: the
  ZDoom sources in the ViZDoom repo (p_user.cpp, p_mobj.cpp, p_map.cpp). Tic-by-tic traces
  match the engine to ~0.1 unit; jump results agree on ~98% (MAP13 121/121, MAP14 126/135,
  MAP15 358/364). It needs no cache. One map's jumps take about a second, against minutes
  in ViZDoom (~3 s a jump, engine-bound: Python was ~4%).
  `keep_verified(engine=True)` still uses ViZDoom.
- Windows: ViZDoom tics were bound to the 15.6 ms default timer resolution; the adapter sets
  1 ms (`timeBeginPeriod`), ~3x faster runs.

- **Box fit is a cost, not a filter (2026-11-04).** Planning used to search the whole level
  twice: strict (refusing paths where the swept 32x32 box may clip a wall) and, if that
  failed, lenient. Strict usually "failed" by timing out (MAP13: 120 s wasted). Now one
  search: `find_path` returns its least-squeezed path with `Path.squeezes`, and the plan
  search ranks nodes by (can't reach a goal, squeezes so far, ...), so clean routes win and
  a squeezed route to the exit is taken only when nothing cleaner is ahead in the queue.
  MAP13 plans in 2 s instead of 124 s.

## One planning mode: lifts and timed doors last across legs (2026-11-04)
- Temporary heights (a lift down for 3 s, a door open for 4 s) carry the time they have
  left, and any later leg may use them until it runs out (MAP22: through a door to a switch
  and back through it; MAP24: onto a lift for a key and on across it). They used to last only
  for the leg right after their trigger. A floor's old height while it moves for good still
  only lasts one leg.
- To keep the search small: the "world" for duplicate checks is what lasts (heights added
  that aren't temporary, keys, the area reachable without temporary heights); timers are
  compared by dominance within a world, keeping at most TIMER_SETS per world by total time
  left; a carried timer that doesn't change what's reachable is dropped; a switch isn't
  pressed again while its lift or door is still running; nodes are ranked by the area they
  reach without carried timers.
- MAP17 (many repeatable lifts) is the slow case: ~300 s to plan; the planning limit is
  60 s for now.

## Navigation core moves to Rust (2026-11-04)
- A first compiled piece (`native/`, PyO3 + maturin, crate `evildoom_native`) does the
  planner's flood fill. It matches Python on every call checked, but is only 2-3x faster:
  crossing the Python/Rust boundary thousands of times per plan dominates. So the whole
  navigation core (map data, navmesh, planner, path-finding, movers, jump finder, movement
  sim) is to be ported in stages, with Python calling in about once per plan; the ViZDoom
  adapter, follower and Neuro integration stay in Python. The Python version is the
  reference until each stage plans every map the same. Keep it portable (final deployment
  is likely Linux).
- Stage 3: the plan search itself (`native/src/plan.rs`) is native; Python does the one-off
  analysis (upper bound, relaxed plan) and rebuilds the Route. Identical plans on 35 maps
  checked. Most of the time was box fitting (cheap geometric test now goes before the height
  rules) and repeated flood fills (now cached on start poly, keys and a hash of the heights).
  MAP17 does 5000 expansions in 8 s instead of 62 s, and plans within the budget, now raised
  to 20000 expansions (the 60 s limit still applies).

## Map fixing round: 52 → 64 of 68 (2026-11-04)
Each change below came from one map and was checked with a full Doom + Doom II sweep for
regressions. Planner changes are made in both Python and Rust, and checked to plan
identically.
- Trenches: a sector narrower than the box everywhere and lower than all its neighbours
  can't be stood in; it's planned at its lowest neighbour's floor, following them as they
  move (E4M1, MAP08). The mover model keeps the real heights.
- The follower finds itself by the floor the box actually stands on, not the polygon under
  its centre (MAP24's 24-unit walkways, E4M9).
- Unexpected one-way drops (a walk-over line on the way sank something) stop the follower
  and trigger a replan with the real heights (MAP08's bridge). Floors that sink when stepped
  on are run across, re-aiming with the movement sim every few tics (MAP08's exit).
- Search: nodes holding more of the relaxed plan's keys rank first (MAP09). Walk-over lines
  count only from a side they can be crossed from (MAP09). A rising floor keeps its old
  height only until it's a step above it.
- Box fit: a squeeze against shut sectors we can't open is a real block (MAP21). Waypoints
  aren't dropped where going straight on would walk off a ledge (MAP29).
- Gun lines: shot only from where they span at least 6° and only with a hitscan weapon;
  the bot takes out the pistol if it's holding a fist or chainsaw (MAP19).
- Barrels in the way are shot from beyond their blast radius (MAP23).
- A lift being ridden is waited on from its middle (MAP06). Lift lines are pressed at their
  nearest point (MAP29). Doors aren't pressed through another switch line (E3M9). Backing
  off when stuck keeps away from ledges too (MAP29).
- Exit floors (sector special 11) are pseudo-triggers: stand there till they end the level
  (E1M8). The sweep keeps stepping briefly after a walk-over exit (E4M8).
- Still failing: MAP09 (press a switch, then reach a rising bridge within about 24 tics).
  MAP30 is out of reach in principle (boss brain). Tried and reverted: keeping a lowered
  floor's old height briefly (broke MAP08, didn't fix MAP27).

## Effects that depend on the heights when fired (2026-11-04): 66 of 68
- Stairs build on from where their first step is now (MAP21: four switches, each one step
  higher). Specials whose target comes from the heights round them ("lower to highest
  neighbour + 8" and the like) are worked out again from the heights at the time, once
  something round them has changed (MAP27: the red key's pedestal only comes down to 8
  once the four platforms round it are down). Natively these call back into Python
  (Navigator.effects_from_reps), cached.
- So pressing them again can do something new: the one-shot ones are tracked as used, in
  the search's nodes and signature and in the follower (replans start from them).
- Several lines with the same effect: the closest, plus any that leave somewhere else
  reachable (MAP27: the lines in the platform room trap you there).
- Relaxed plan: a needed switch that can only be pressed through a door names that door's
  key; a producer behind its own effect (a switch opening a door from the inside) is passed
  over for the locked door's key (MAP27: yellow, then blue, then red).
- Follower: stairs aren't pressed while their first step is away (also a lift); a press is
  made from where it's stuck if the switch is in reach (through a door the press opens);
  run-ups strafe to hold the jump's line; a sector still rising to the plan's height is
  waited for at the edge rather than treated as an unexpected drop (MAP29).

## Open questions / research TODO
- [ ] Solid things and sub-33 gaps in the nav mesh itself (see status above).
- [ ] Timing-aware handling of lifts/doors that close on a timer (be on them in time).
- [ ] Jumps that need lift/timed-door heights can't be set up by the verifier ("unverified").
- [ ] Straferunning (SR50, ~141% speed) as a second jump speed.
- [x] ~~Jump verification is slow on first run~~: movement simulator (see above).
- [x] ~~What is ViZDoom's actual compatibility?~~ See findings above.
- [ ] Verify Boom/MBF feature support in ViZDoom with a real community Boom PWAD.
- [ ] Check input/control: continuous turn/move delta buttons in ASYNC_PLAYER mode at 35 Hz,
      visible window at stream-worthy resolution.
- [ ] Engine choice beyond the spike (user has questions before deciding).
- [ ] Survey existing open-source DOOM bot / pathing code (ZDoom bots, MBF helper dogs,
      multiplayer bot projects, etc.).
- [ ] Can two characters (Neuro + Evil) connect to one game simultaneously?
- [ ] Benchmark classifier options (hosted Jev vs. local CPU models) for latency/quality.
