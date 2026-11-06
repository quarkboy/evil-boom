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
- No walking a ledge narrower than the box as a "jump": a jump's box may stay over its
  takeoff ledge only 4 tics with a wall within 3 units of it, 30 at most otherwise (MAP27:
  an 8-unit strip between a pit and a block, a 660-unit walk with the centre over a pit).
  Tried and dropped: requiring every jump to work 3 units to either side (lost jumps E4M2
  and E4M6 need).
- Relaxed plan: a height-dependent trigger's result with everything round it at its lowest
  is part of the fully-fired world, and what that relies on round it is needed too, by
  effect: any line making the same change counts (MAP27: the platforms under the red key's
  pedestal; only for these, wider it slowed MAP15). Lines with the same effect: one per
  node again (more doubled MAP17's planning). Only used stair switches split the search's
  worlds.

## MAP09 without the dash: 67 of 68 (2026-11-04)
- The rising bridge isn't a race: it's jumped onto, raised, from the top of the lift beside
  it. Jumps are now also tried with the sectors met in flight at their highest possible
  floor, when the plain try finds nothing (a raised platform to land on).
- A timed line's heights are temporary when it fires even if another line makes them for
  good (door 101: the shut-again switch was taken as opening it for good, as the other
  switch does).
- Timed legs: a jump off a lift that's still down waits for it to come back up (about 60
  tics with lining up), so later windows are judged against that.
- Jumps have to work half a unit to either side of their line (MAP09's first choice through
  a doorway glanced off the jamb from 0.4 units over; 1 unit removed jumps E4M6 and MAP24
  need). A jump's flight wants 8 units of headroom over the player.
- Smoothing never starts a shortcut at a jump's takeoff (what follows is the flight: MAP29
  cut across the pit after landing). Pinning the landing as well forced paths through the
  exact spot (E4M2, MAP17, MAP25).
- Follower: before pressing a height-dependent switch, wait for the lifts round it to be
  where the plan has them; a lift line set off on the way is crossed only once the lift is
  back (and the press before it waits too); a sector still on its way to the plan's height
  is waited for, not replanned round; a jump goes without settling when what it needs is
  about to run out, and takes a ceiling a little lower than planned; after a permanent
  move the old height is forgotten.
- Every Doom and Doom II map exits except MAP30 (the Icon of Sin).

## Python copies of the native core removed (2026-11-04)
- The navmesh build, flood fill, path-finding (A*, funnel, box fitting, smoothing) and the
  plan search now exist only in Rust (`native/`); the native module is required. Python
  keeps what hasn't been ported: map loading, triggers and movers, the one-off analysis
  (progression, the bound, the relaxed plan), the jump finder and movement sim, the
  follower and the engine adapter. `NavMesh` takes its polys and portals from the native
  build and keeps only lookups (BSP point lookup, line grid, sector neighbours).
- From here on the ported parts are changed in Rust only; no parity checks.

## Native core reorganised (2026-11-04)
- `native/src` split by job: `geom` (points, segments, the funnel, the line grid), `map`
  (the map parts, read straight from Python's `MapData`), `mesh`, `nav` (passability and
  flood fill) with `nav/path` (A*) and `nav/fit` (smoothing, box fitting), `plan` (the
  search) with `plan/triggers`, `plan/spots`, `plan/timing`, `plan/world`, `python` (the
  classes and conversions) and `stats`. The WAD reader the Rust side no longer used is gone.
- What crosses from Python is named: `Nav(map, obstacles, facts, fits, landable)` and
  `set_plan_data(facts)` and `plan(start, goals, states, query)` take dicts whose keys are
  the Rust structs' fields (`NavFacts`, `PlanFacts`, `Query`); absent lines and sectors are
  None rather than -1. Errors from Python callbacks propagate instead of being swallowed.
- Per-call state is passed, not kept on the core: path smoothing has its own `Fitter`, the
  plan search a `Planner` borrowing the core, the plan facts and their caches separately (no
  locks or atomics). Builds clean under clippy with no lint allowances; `cargo test` covers
  the geometry (funnel included) and the plan's world operations. Plans for all 68 maps came
  out identical to before.

## Jump finder and simulator move to Rust, traced from ZDoom (2026-11-05)
- **The jump finder and the movement simulator are native** (`native/src/jumps`:
  `finder`, `physics`, `check`; Python's `nav/physics.py` and the finder in `reach.py` are
  gone). Python asks `Navigator.jump_lab()` (`evildoom_native.JumpLab`): `find` for the
  jumps (cached in out/navcache, keyed by the finder's version), `check` for their verdicts,
  `fits_at` (P_CheckPosition, for `_landable`), `run_landings` (the follower's runs over
  sinking floors). Checking a map's jumps takes ~0.1-0.3 s (Python: ~5 s).
- **The simulator follows the ZDoom 2.8.1 source function by function** (the ViZDoom tree):
  ViZDoom's floored delta buttons and G_AddViewAngle; R_InitTables' finesine (truncated, its
  second quarter mirrored off by one); P_MovePlayer; P_XYMovement's sub-steps and re-basing;
  P_CheckPosition over every line in the blockmap's blocks (the map's BLOCKMAP lump, now
  read by `load_map`), with R_PointInSubsector's sector; P_TryMove (no z raise on a step:
  P_ZMovement lands you); FSlide::SlideMove (three corner traces through FPathTraverse,
  short ones meeting lines' infinite extensions as ZDoom's do, HitSlideLine's divline
  projection); P_ZMovement. Traces now match the engine exactly, tic for tic, on every jump
  compared (MAP24, E4M6: 80 tics, 0.000 apart; the Python port drifted up to 100 units).
  Left out: monsters, items, specials, slopes, 3D floors, water; solid things are
  infinitely tall boxes (ZDoom lets a player pass over a low one).
- **Exact physics showed jumps that only work pushed just so.** MAP24's run along a
  channel narrower than the box (the rims hold you up): coasting from the edge stops short,
  pushing all the way overshoots the 20-unit landing. The check now also tries letting go of
  forward 16, 32, ... units past the takeoff (`soft+N`, `Jump.release`, the follower does
  the same), and a jump that won't stick is tried from shorter run-ups, 16 at a time
  (`ok~x,y`: that run-up). Both are what a player would do; robustness to being a little
  off still applies. Neither is tried onto a floor that sinks once stepped on
  (`Navigator.sinking_sectors`): that's not for stopping on, only running on (MAP08's exit
  run went wrong landing softly).
- **After landing a jump the follower paths on from where it landed** (`_path_on`): the
  plan's path past a jump was smoothed across it and can wander (MAP22: back off the ledge
  it lands on, onto a floor with no way up).

## Explorer finishes every map that doesn't need a kill (2026-11-05)
With knowledge limited to what she's seen (no monsters, god mode, 3 lives), the explorer
exits every DOOM and DOOM2 map except those that open only when monsters die (E1M8, E2M8,
E3M8, E4M6, E4M8, MAP07, MAP32; MAP30 excluded). What it took, beyond earlier fixes:
- **Sounds right after setting something off are looked into at once** ("check a noise
  right away", priority over everything but the exit): MAP19's walk-over line opens a timed
  door out of sight, the way on; looked at later, it was shut and nothing was learned. Not
  in the middle of a run of jumps (one broken off on a landing walked off it).
- **No sounds in the risky tier**: a far noise took MAP09's explorer down a one-way drop.
- **Once-only lines are once per life** (`Fired.life`): after a restart, MAP08's trap went
  off unrecorded and the planner took it for spent. Beliefs from fired effects are this
  life's only.
- **Routes that walk over a trap are refused** (`Explorer._shuts_in`): the planner counts
  what it fires on purpose, not walk-over lines a path merely crosses. Their effects are
  applied to the route's end state; refused if she'd be cut off from most of what she can get
  to (with what she knows how to work, doors gone through counted open), unless what's left
  has unseen parts or untried switches (the trap room's switch that undoes it).
- **Floors change type** (`Navigator.floor_after`, `Knowledge.changed_floor`): a floor seen
  lowered by "lower to lowest, change texture" takes its neighbour's type (MAP08's walkway
  becomes nukage: a pit to avoid dropping into); "raise to next higher, change texture"
  clears it. (The user's diagnosis.)
- **Trenches move with their sides in the known view** too (they never visibly move, so the
  learned effects left them out), and take both their sides' lowest and highest floors.
- **Another press spot when the nearest has no route** (MAP08's exit door: nearest from a
  nukage pit through a gap too tight; pressed from the ledge above).
- **No glancing round mid-jump** (a glance at the run-up overshot onto a sinking floor).
- Tried and dropped: counting a box pinched against a tall ledge as a squeeze (and as a hard
  one). Right in itself, but it changed plan searches elsewhere (MAP17, MAP29 omniscient;
  many explorer maps with hard squeezes) and nothing needs it now.

## Fighting: first pieces (2026-11-05)
- Native `LineOfSight` (ZDoom's P_CheckSight without REJECT): who can see whom.
- `Navigator(skill=)`: which things are on the map at the skill played.
- Observations carry the things about (monsters, missiles, items, by ZDoom class name),
  weapons held, ammo per slot and armour; controls select any weapon slot.
- `tactical.bestiary` (monster sizes, toughness, attack kind, threat; missiles; weapons),
  `tactical.combat.Combat` (noticing, targets, weapon choice, aim with lead, dodging missiles,
  backing off from clawing monsters, strafing when it's hot, shooting on the move otherwise),
  `tactical.player.Player` (explorer + fight; items she needs and has seen as objectives;
  monsters seen still about hunted once there's nothing else).
- Testing starts at lower skills (HNTR/HMP) and works up to Ultra-Violence (the user's call).

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

## Fighting: dodging, guns, shots that land (2026-11-05)

From watching her play (`tools/watch.py --fight SKILL --mortal`):
- **Dodging** predicts each missile against where she's going (her velocity from the engine), not where she stands; tries 16 ways, simulating a running player's push and Doom's friction, and takes the one that keeps her furthest from every missile coming; a missile is dodged till it's gone by (before, she stopped half way out of its path and walked back in).
- **Weapon choice** is the time to kill the target: Doom's damage, refire and spread (the share of pellets that land on something that wide that far off), plus the switch (0.8 s) and what the ammo's worth (rockets dear, more so when few). The super shotgun is told apart (WEAPON3 counts both shotguns; slot 3 from another slot brings up the SSG). Before, rough dps with range cut-offs had her flip between shotgun and pistol in mid-range fights.
- **Shots that land**: she fires only if a shot from her gun (36 above her feet) reaches some part of the target within autoaim's slope (100/160) with nothing in the way (the native sight test, the target's span clipped to that slope); else it'd go out level, into the lip of the ledge the imp stands on. With one monster about that she can see but not hit, she steps back from it (above her) or towards it (below).
- **Out of reach**: against hitscanners (they hurt from anywhere) she fires past a gun's reach while a fifth of a shot lands; when standing to fight something still out of reach she closes in, zig-zagging (she used to strafe at a chaingunner far off, never firing). Holding fire is logged with why.
- **Better guns**: one better than her best, seen, breaks off what she's doing, but only if the route there is no longer than the fighting time it'd save against what she knows is still about (and 10 s for what's to come), at 400 units/s; further, it's an ordinary pickup in its turn and the fight comes first.
- **Rockets** aren't fired with a biter seen near her in the last 3 s (E1M9: dead at full health, a demon round a corner stepped into the blast).

HMP batch (18 maps, 3 lives): 10 → 12 exited (MAP04 now gets through). Deaths left: pistol starts against crowds (MAP08's cacodemons, MAP11), out of ammo, lost souls.

## Fighting: distance, crowds, ammo, sound, infighting (2026-11-05)

- **Distance**: each gun has a band it does best in (`Weapon.keep`: pistol 192-640, shotgun 128-448, SSG 96-320, chaingun/plasma 192-768, rockets/BFG 320-1024). Nearer than the band she backs off aslant (strafing on); further, or out of reach, she closes in; between, she strafes.
- **Surrounded** (3+ monsters within 640 spread over 160° round her): to a place she's been (96-1024 off) that's seen by the least threat, has them all on one side, is far from the nearest and near her, by a way not within 96 of any of them; shooting on the way, 4 s at most.
- **The slide**: "is that way safe" also follows where her speed carries her (8 tics pushing, then letting go: ~170 units at a run, sideways speed carrying on). E1M3: strafing by the start, she slid into the nukage and couldn't get out.
- **Out of ammo** (nothing but the fist): the fight doesn't stand; the route goes on, to the ammo she needs (MAP05, MAP11: she walked up to imps to punch them).
- **Errands mid-fight**: with only the pistol, or short of ammo, a gun or ammo she knows where it is breaks off the fight once (the explorer takes it up, urgent, if the route's worth it); on the way the fight only aims and shoots (and dodges, and backs off from biters).
- **Out of reach**: a monster in sight that no gun she has gets to from there doesn't stop the route.
- **Sound** (`tactical/sound.py`, P_NoiseAlert): a shot floods through two-sided lines with an opening, at most one sound-blocking line. A monster standing about (seen, not seen to move or turn) isn't shot if the shot would wake enough others she's seen standing about that she'd be outmatched; she sneaks past. (The flood is the game's physics, known; the monsters counted are only those seen.) The fist is silent.
- **Infighting**: a monster awake, facing another (within 30°) and not her, for half a second, moving or faced back, is fighting it: not a target, not counted as threat. A kind doesn't fight its own unless its attack is hitscan (imps' fireballs don't hurt imps).
- **Baiting**: a heavy (cyberdemon, spider, mancubus, arachnotron, revenant, baron, knight) she'd take over 8 s to kill, with monsters of another kind within 1024 of it: to a place she's been where its line of fire to her passes through one of them, holding fire; over once anything's infighting (or 4 s).
- **Arch-viles**: in sight and not killable in 2 s, into cover whatever her health (its fire does nothing once it can't see her; MAP11: three deaths at 50-70 health).

HMP, 9 maps (E1M1, E1M3, E1M9, MAP01, MAP04, MAP05, MAP08, MAP09, MAP11), 3 lives: 4 exit (E1M1, E1M3, MAP01, MAP04, the last two with no deaths); MAP05 stuck once in an explorer loop on lift 645 (exited the two runs before: to trace). Deaths left: pistol starts against cacodemons (MAP08), pain elementals, arch-viles, demons and imps close up (E1M9).

## Final engine: DSDA-Doom; Evil to Rust (2026-11-05)

The user's plan: move to **DSDA-Doom** (full MBF21 support, good Linux support). That settles the "Expected" switch above.

Consequences, as discussed:
- **One engine for play and for sweeps.** Evil runs in-process with DSDA-Doom's game simulation (rendered for the stream; built headless for sweeps). Same physics in both; no ViZDoom.
- **Evil moves to Rust entirely** (today ~8.6k lines of Python beside 6.3k of Rust: nav core, sight, line of sight, jump fitting). The Doom engine itself isn't ported: Rust calls DSDA-Doom's C code (FFI). A rewrite of the engine would cost weeks and gain little speed; the win is in-process, no rendering, no conversion.
- **Why the speed:** profiled 2026-11-05, E1M3 with monsters under ViZDoom: engine 53%, observe (ViZDoom's get_state: Python objects for every object and every sector's lines, each tic) 31%, Evil's thinking 16%; ~150 tics/s a process, ~800 at 8 processes. Both the engine and Evil in Rust should reach thousands a process.
- **What has to be redone on the new engine:** jump/run-up checks and anything tuned to ZDoom 2.8's physics; the explorer sweep (60/68) and the fight batches re-run. Boom-family physics differ (actor blocking heights, some timings, autoaim details).
- **To check:** whether two players (Neuro and Evil) can share a DSDA-Doom game.
- **Order:** a spike first (DSDA-Doom built headless as a library, stepped from Rust, monsters and sectors read directly, tics/s on E1M3, multiplayer); then port Evil bottom-up behind the engine interface (engine adapter, motor, knowledge, then tactics last, since they're changing fastest), each module checked against recorded traces and the Python deleted after. Fighting keeps improving in Python on ViZDoom meanwhile; the behaviour carries over.

## DSDA-Doom spike (2026-11-05)

DSDA-Doom v0.30.1 (kraflab/dsda-doom, eb8eb98), built on Windows with MSYS2 mingw64 (SDL2, SDL2_mixer, libsndfile, libzip, zlib; optional music libraries off). Kept out of git (`third_party/`); ours is `dsda/`: `evil_api.c`, `dsda-doom.patch`, `README.md` (build steps), `spike/` (Rust).

- **Ceiling**: `dsda-doom -timedemo -nodraw -nosound` on a 20,000- and a 200,000-tic random-walk demo of E1M3 (HMP, monsters): ~205,000-233,000 tics/s, linear in length.
- **In-process**: DSDA-Doom built as a shared library (`dsda-evil`: the game's own sources plus `evil_api.c`; the patch adds the target and one exported wrapper, `D_DoomMainSetupForEvil`, the setup without the main loop). `evil_init(argv)`, `evil_tic(forward, side, turn, buttons)` (the singletics path: the command written into `local_cmds`, `G_Ticker`), and readers into plain arrays: `evil_sectors` (floor, ceiling), `evil_mobjs` (type, position, momentum, angle, health, flags), `evil_player_state`. A C ABI DLL from mingw loads fine in MSVC Rust (`libloading`). SDL's dummy video/audio drivers: no window.
- **Result**: E1M3, HMP, monsters, a random walk, reading every sector (177), every thing (~310) and the player each tic: **~125,000 tics/s** (stepping 80%, reading 17%: ~1.4 µs a tic for all of it); set up in 0.02 s. ViZDoom: ~1,650 tics/s for the engine alone, ~150 with Evil's Python. Doom is now free; Evil's thinking will be the whole cost.
- **Things to settle in the real adapter**: a thing's identity (the spike uses its address; `mobj_t.index` is the GL renderer's, not an identity): a counter set at spawn. Map thing types to names (mobjtype_t → our bestiary). Keys, items and suits read directly (no "gone since the start" inference). Level exit and death state from the game, not guessed.
- **Two players**: DSDA-Doom has no network play (its "net" is local: `-solo-net`). In-process it isn't needed: each tic the commands of every player in the game are ours to give (`local_cmds[i]`), as multiplayer demos play back. Untested: starting a co-op game with two players in it and feeding Neuro's commands beside Evil's.
- **License**: DSDA-Doom is GPL-2.0+; Evil linked with it in one process makes the combined program GPL. To keep in mind for distribution.

## The explorer on DSDA-Doom (2026-11-06)

Python Evil on DSDA-Doom in-process (`--engine dsda`): the whole explorer sweep (68 maps, god mode, 3 lives) in ~2.5 minutes at 8 jobs. 54/68 at first; the differences, one at a time:
- **Vanilla door rules** (ZDoom differed). A switch on a sector that's busy (a door open on its timer) does nothing, and a once-only one stays to be pressed; a door's own switch pressed on a door open for good restarts it as one that closes. So: all once-only lines used this life are spent to the planner (not just stairs: E3M4's door, opened for good, shut by its own door switch, seemed to open again for the spent one); a once-only press that moved nothing, on sectors moved in the last 200 tics, "didn't take" and isn't used up (E3M4's switch behind a door opened by its door switch), unless what last moved them left them for good and the line has a tag (E3M7's walk-over line opening a door just opened for good: it went off; MAP28's own door line on such a door: it didn't); once-only door lines used aren't doors she can open (MAP19's, the door shut by a walk-over line); and before pressing a once-only switch (not stairs: MAP21), she waits for what it works to come to rest.
- **Jumps**: the movement simulator matches DSDA-Doom to 0.2 units given the same inputs (checked tic by tic, run and jump, with `evil_warp`); it now pushes not at all in the air (vanilla; ZDoom's aircontrol is 1/256) and rounds inputs as the adapter does. Misses come from the follower's run-ups, which aren't the check's to a unit: a jump that comes down off its landing is left out once missed (unless nothing else is left) and for good after two; each jump is scored by how sure it is (made shifted 2 and 4, turned 1.5° and 3°, from a run-up 8 longer and shorter): under a quarter, dropped; the rest dearer to the planner the less sure (up to 1024 units).
- **Path smoothing**: a shortcut may ignore a wall the box already touches, but not go through it; a corner right on the path is rounded on the side of it that's floor (E3M7's doorway: pushed 30 into the wall, she pushed into it for good).
- **Floors hurt on the level's clock** (every 32 tics): only hurt twice on a floor that far apart is it the floor (a monster's shot made floors "hurt after all").
- **Sweeps are deterministic** now (PYTHONHASHSEED fixed for the workers): a change's effect can be told from luck.

- **Use traces, vanilla**: any line with a special stops one (a walk-over line too: E2M4's door was pressed from beyond one, by the planner and in her model); a once-only line used has no special any more, so the trace goes past it (E2M7: a switch behind a used door switch went off, and she never knew).

60/68: parity with ViZDoom (all but the 7 kill maps and MAP30).

## Secrets hidden: she knows what a player sees (2026-11-06)

`Knowledge.perceive` (default; `--peek` gives the old way): a line is a switch, door or lift to her only if it looks like one (a texture on its front: SW1/SW2, DOOR/BIGDOOR/SPCDOOR/EXITDOOR/ZDOOR/ICKDOOR/ZELDOOR, PLAT/SUPPORT). Surveyed over Doom and Doom II's 2,199 usable lines: that catches 1,394 of the 2,012 not flagged secret and 32 of the 187 flagged (the rest look like the walls round them, as meant); the other 618 are mostly lifts and doors in plain textures.

What doesn't look it is found by trying walls, as a player does:
- **Suspicion** (`Knowledge.suspicious`), from what she can see only, of seen walls faced from floor she can get to whose far side she can't: the automap drawing it as a two-sided line (+3; a mapper hides a secret door by flagging it), a raised platform's side (+1), an item she can see but not get to just behind (+2), a texture unlike the walls either side (+1.5; short pieces +0.5). 2 or more: worth a press, one wall for each place behind.
- **Trying** (`wall` objectives): once nothing else is left, the most suspicious few: walk up, face it, press use, watch. What it does (if anything) the knowledge sees as any line set off; tried walls are remembered.
- **Panic** (the user's idea): nothing at all to do, not even a suspicious wall: every wall round the place she's in, nearest first (a lift out of a pit that looks like the pit's wall).

Explorer sweep with it: 52/68 (peeking: 60). Lost: MAP05, MAP08, MAP14, MAP19, MAP21, MAP27, MAP28, MAP29: the suspicion scoring's work list.
