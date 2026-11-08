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

## Perception: what she learns, and how sure (2026-11-06)

Tracing the maps perception lost, each back to the route that failed:
- **Walls judged as last seen** (not as the map starts): a stair or a raised floor since makes a wall of what was floor, and a lift's side on it a suspect (MAP21, MAP29, MAP23).
- **Presses that add on** (stair builds, "raise 24/512") each count as their own try (`Knowledge.tried_key`): MAP21's four stair switches each build 16 higher.
- **A trap she's in** isn't kept out of (MAP21's nukage pit, its lifts out tried only from inside).
- **A switch on a busy sector** with a lasting effect (SR/GR, not crushers) isn't taken to have worked (MAP12's "open for good" switch pressed while the door was open on its DR timer).
- **Certainty** (the user's idea): an effect put down to a press is *sure* if the sector started moving (by its sound) within 8 tics of it, and only if it moves the way the line sends it; else it's in doubt (`Knowledge.doubts`). The planner doesn't build on a doubted *permanent* effect (as seen instead); a doubted repeatable switch is pressed again, once a life, once nothing else is left, and the effect made sure, kept (it fits) or unlearned. Doubting non-permanent effects too lost MAP27 (they come back anyway: nothing to guard).
- **The way back** ("can she get back?", before a one-way move): doors that close again count as shut, but for ones she can open from where she'd be, by their own lines or by any switch or walk-over line she knows there (MAP19's closet, its door opened from outside and its way out a once-only teleporter). A margin shutting doors near the end of their timer for routes was tried and dropped: MAP12 was the use gates', and it cost MAP19.
- **Once-only teleporters used this life** are out of her view (MAP27's, walked over again from the room it led out of: every way out was "stuck" on it).
- **Use gates between two sectors**: a press across a gap that's shut at the start between a movable sector and another (a raised platform's floor at an alcove's ceiling) is possible once there's a gap (`use_line_gates`, in Rust too): MAP12's switch beside a platform the W1 lines lower. In the relaxed needs, an activator that waits on the very sector being produced doesn't count as a way to produce it (MAP27's lift switch, pressed from below across the lift's edge only once it's down).
- **The box over a ledge from below**: a path segment may not take the box further over a line it's already over whose high side is more than a step above the floor under its centre, even a line the way crosses later (MAP19's fan of stairs meeting in a point beside a floor two steps down: set off from beside the point, every way went along it and she pushed at it 600 times). Trimming portals at such ends instead cost MAP29 and was dropped.
- `tests/test_follower.py` runs on DSDA-Doom (each game in a process of its own).
- **The stranding report** gives, for the omniscient plan, where its way goes through what she doesn't know or by heights she doesn't believe, its teleports/doors/drops, and where her reach ends on it (and if that's a poly she's learned to avoid, or a trap): MAP27's stuck poly on the way out of a room was found that way.

Sweep: 55/68 (MAP12, MAP08, MAP21, MAP23, MAP29 back; MAP19, MAP27 lost). MAP27 is sensitive to the order things are done in: lowering sectors 28/29 early leaves the lift ride from switch 739 too long for the planner's walking speed (`LEG_SPEED` 12; flat-out running is ~15.1, but the follower doesn't make the lift either); MAP28 needs a red-eyed skull (one of three WOOD4 panels, shifted by its y offset) pressed above an opening, and a symbol panel shot (SKINSYMB), both suspected only in work-in-progress changes that cost E3M4.

Perception sweep after these: **56/68** (out: MAP05, MAP14, MAP27, MAP28). The peek sweep was 54/68 at this point (58 before the perception work), traced:
- **A doubt is rechecked only on a press with what it acts on still** (and once a life): E3M7's lift line walked over while the lift came back from another's press was turned down by the game, "unlearned", and the lift lost to the planner.
- **"Pressed, but nothing went off" counts this try only**: MAP08's shoot-switch, shot "again" from where the shot hits the switch on the block's other face first, went on for ever (an old shot of it counted). Pressing "again" also goes by the last press of anything with the same action.
- Peek: 57/68 (out: MAP15, a one-way move into a dead end late on; MAP21, a corridor exactly the box's width between a wall and key bars alongside it, which the planner takes as open, as a door she could open, though nothing opens it on the way; MAP27).
- **Sweeps deterministic again, load or no load**: searches were cut off by the clock (the explorer's route searches at 1.5s, the follower's replans at 10s, the planner's wait for a route without squeezes at 10s), so eight maps at once took other ways than one alone (MAP14, MAP17, MAP23 passed alone and not in the sweep, or the other way round). The expansions decide now; the seconds are a backstop (30s, 120s). A spot to look out from must fit the box with half a unit of play (MAP21's, between a wall and key bars exactly 32 apart).

- **...and the same run alone, again and again**: the Rust core's hash maps and sets were seeded at random in each process (std's), and where a choice fell to the order they went round in, MAP17 alone exited two runs of three and got stuck on the third. `native/src/det.rs`: the same hashing with fixed keys, everywhere. The clock backstops on searches are 600s (30s still bound with eight at once).
- **Slow isn't stuck**: the sweep gives a map an hour of game time (`--max-seconds 3600`; was 1500): MAP15 under peek exits at 2051s (the user: fix failures, not inefficiency).

Perception 56/68 (out: MAP05, MAP14, MAP27, MAP28); peek 58/68 (out: MAP14, MAP27). MAP14's doors open by walk-over lines nobody can see, one along the far end of a pit where the box is stopped with its centre on the line: to trace further.
- **Suspicion of faces over openings**: a face above or below a way through counts when unlike the walls round it, as does the same texture shifted up or down from its neighbours between the same sectors (MAP28's red-eyed skull, one of three WOOD4 panels: pressed now). Giving each suspected wall its own identity (so one given up on doesn't hold the rest back) cost E2M9, E3M4, MAP25: reverted.
- **A noise checked from where the box can stand** (`_stand_spot`): MAP05's noise goal was a poly's middle 12 short of reach, pushed at till the poly was taken for one she gets stuck in, and the way on through it lost.
- **Locked lines without the key** don't count as used (the game says so and does nothing): MAP05's blue door, looking like the wall round it, pressed as a suspect wall before she had the key, was taken as a once-only line used up. **A tried wall is tried again** with a key she didn't hold when she tried it.

Perception **58/68** (out: MAP27, MAP28); peek 57/68 (out: MAP14, MAP15, MAP27; MAP15 ends where no route to the exit is left, late on).
- **A doubted line's recheck is its one go a life, whether or not it could tell anything**: MAP27's lift switch, its lift always moving when pressed, was "tried again, unsure" 1800 times. With this and the key rule above, MAP27's red door (a marble face, looking like the wall) is pressed with the key: MAP27 exits.

Perception **59/68**: only MAP28 out (besides the 8 boss maps). Peek 57/68 (out: MAP14, MAP15, MAP27).
- **The places she got stuck get another go** once nothing else is left (the poly-avoidance after two failures had cost the only way on in E3M4, MAP05, MAP27). **A panel unlike the same panel either side** is a stronger suspect (+1.5: a mapper's mark for a switch, MAP28's symbol). Skipping walls given up on lately before taking the best few (so MAP28's symbol, 25th, came up) let the wall tier run on for ever and cost MAP14, MAP23, E3M4: not kept.

Still perception 59/68 (MAP28 out), peek 57/68.

## The 100% secrets sweep (2026-11-07)

`tools/explore_sweep.py --secrets` (the user's third step): every secret there is, then the exit. She knows only the tally's count (as the automap shows a player: how many, not where); while it's short, the exit is last (`exit_last`): everything else first, the suspect walls and the panic tier included. Levels with secrets nobody can get pass that many short: E4M3 two (torch sectors no one can stand in), E4M7 one, MAP15 one (#9), MAP27 one (#5: a teleporter's sector, the teleport line in the way); the user, and Paul Falstad's Doom II secrets list (gamers.org).

First sweep (perception, 3 lives): **21/68 with every secret**, 56/68 exited. Most of the rest leave with some missing once there's nothing left to try (E2M4 4/10, MAP10 1/18, MAP19 3/9): the work list for finding secrets.

## What the game tells her (2026-11-07)

The user: pressing a locked door without its key, the game says so ("You need a blue key to open this door"), and a player learns from that. DSDA-Doom's messenger is patched (`dsda/dsda-doom.patch`) to keep the last message given to the player and its tic, shown or not; `evil_player_state` hands both over and `Observation.message` has one when it's new. A press whose line the game answers with a need-a-key message puts the line in `Knowledge.locked`: a door or switch from then on however it looks (offered once she holds the key, first among doors), where before it was only a suspect wall to be tried again with a new key.

Also found: `ADDS_ON` (the per-line key for "raise N" floors) had a backspace character where its `\b` should have been since it was written, so it never matched; fixed.

**The game's random numbers came from the clock**: DSDA-Doom adds `time(NULL)` to its seed at every new game (`I_GetRandomTimeSeed`), so a barrel took a different number of shots to burst each run and everything after went its own way (MAP23, alone, four runs: 376 to 1299 game seconds). Sweep results before this carry that noise. The patch adds `evil_seed(fixed, value)`: a seed of our choosing, the same run every time given the same commands, for testing. A real run keeps the clock (the user: we want random seeds for the real deal), and that's `DsdaEngine`'s default (`seed=None`); the sweep and the watcher take `--seed` (0 unless told; `random` for the clock), so a sweep's run can be watched again as it was, and a map can be tried over several seeds.
- **Which wall next counts each wall's own failures** (`_wall_drops`): the walls still share one wait once given up on (that keeps the wall tier paced; giving each its own cost other maps before), but among those with a route, one given up on before counts as twice as far for each time. MAP14 (with the lock message, she opened the red door sooner and went another way): the nearest wall, line 864, behind a chain of timed switches, timed out every time and still came first every time; the blue door like the wall (811), at the top of the suspects with the key held, never got a go. Now it does: MAP14 exits in 1958 game seconds (3107 before), the same on seeds 0, 1, 2.

Sweeps on seed 0 with all of the above: perception **59/68** (only MAP28 out), peek 57/68 (MAP14, MAP15, MAP27 out): as before, now repeatable.

## Evil in Rust (2026-11-07)

With the game in-process, nearly all the time went into Evil's own thinking, in Python; the user: port all of it. Evil is now one Rust crate (`native/`, package `evildoom`): a library and the `evil` command (`sweep`, `run-map`, `watch`, `dump-nav`, `follow`), with DSDA-Doom in-process as before. The Python (`src/evildoom`, `tools/`, `spikes/`, the ViZDoom adapter, the PyO3 bindings) is gone; it's in git history.

- **How it was checked**: module by module, the Rust's logs keep the Python's line formats, compared on six short levels (E1M1, E1M2, E2M4, MAP01, MAP02, MAP23; seed 0) line for line, the analysis by `evil dump-nav` beside the Python's dump, and the route following by `evil follow`. They match but for timings; the one difference seen (MAP17 peeking) is a tie among equally good spots, broken by set order.
- **Shape**: the `Navigator` (a level's analysis, its caches in `RefCell`s; one per process, so `Rc`), `View`s of it restricted to what she knows, the `Explorer` with a `Hooks` trait for what the 100%-items run and the fight add (wanted, last resort, gone, dangerous, interrupt, before exit), and `play::Agent` choosing between them.
- **Speed**: Evil's thinking ~5-7x faster on the parity levels (E1M2: 711 tics/s to 3577; MAP01 1658 to 10934). A full perception sweep, 68 maps at 8 jobs, takes under 3 minutes, most of it the boss maps running out their hour.
- **Results** (seed 0, 3 lives): perception **59/68** (only MAP28 out besides the 8 boss maps), as in Python. Expert (every item, then out): 53/68 exited, 24/68 with every item: the first expert sweep written down, the work list for pathfinding.

## Expert runs on one life; tools for tracing (2026-11-07)

The user: the goal is 100% items on one life; lives are a testing device, never something the bot plans by. Runs now have unlimited lives (`--lives N` to limit) under the one game-time cap over all of them (3600 s), and the 100% runs end STALLED after 300 game seconds (`--stall`) with nothing more collected and nothing new learned of the level (sectors seen, what lines do), counting the best so far over the whole run. They leave as soon as the count they're after (on the automap, as a player sees it) is complete.

Rules, each traced from a concrete failure and kept narrow (the user: reason a fix through before trying it; the first broad versions cost other levels):
- **Spent once-only lines** are spent everywhere: what she can get to and the follower's replans now leave them out, as the route planner did (E2M1's door switch 288, pressed once from the alcove).
- **Through it while it's open**: right after she presses a door or lift open that comes back by itself, what's beyond, if she's never walked there, is urgent while there's time to make it (E2M1's room with the teleporter to the secret bonus corridor). Not for a lift walked onto (E2M4's lift 66: back to where she'd been can wait).
- **Back at rest**: a door or lift that comes back, set off after one that stays, shuts again (vanilla: a DR door pressed when open for good cycles); only movers that cycle back (not crushers, not perpetual lifts). Seen-long-ago alone isn't enough: what moved it may be unknown to her (E2M4's door 48, opened for good by walk-over line 668).
- **What would cut her off from other objectives** goes last (a player finishes a room before dropping out of it one-way); the exit excepted; not what can only be had now.
- **Lifts in "could she get back"**: at rest unless she can call them from there, as doors were (MAP27's lift 156, whose only switch is on it).
- **Never knowingly away from every exit**: an objective that would cut her off from every exit she can get to isn't taken, safe or risky (MAP24's wall worth a drop into the nukage pit; its secret ledge over it).
- **Turning is standing still**: when she only turns (a glance, a look round, facing a wall), her momentum is braked away (MAP24: a glance just after landing a run-jump slid her off a small ledge).
- **Ground that changed unforeseen**: a crossing ahead that's become a drop through heights outside anything she believed gives the objective back to be judged again, if the only way on drops where the plan didn't (E2M4: walk-over line 909, unknown, raised a floor on the way and left the goal down in the blood pit). A lift down when she counted on it up is foreseen: on as before (E1M2).
- **The last resort, with the exit there** (100% runs): walls with more to go on than one weak sign are all pressed; of the rest, a round of 12, then out (E2M5 went round every wall of the level; E4M7's and E4M9's ways on scored 5.0 and 5.5).
- **Walls between suspects**, a fix: the "can she get back" loop ended only when no opener applied, not when nothing changed (it spun on a spent switch).

Tools (`evil`): `run-map --snapshot S [--focus X,Y,ZOOM]` (the sweep's own run, nothing drawn till S, then the viewer's window to out/watch.png); every run-map records its inputs (out/runs/<IWAD>_<MAP>[_expert...].run: controls, restarts, and the journal of what the explorer itself put into her knowledge: tried walls, stuck places, missed jumps, traps, failed presses), and `--from FILE@S` plays one back to S (the same state exactly; E2M4 to 1000 s in 0.9 s) and hands over to the bot as it is now; `drive` takes her over from a script (goto by her knowledge or the whole map's, warp, face, use, move, wait, bot N, where, exit?, sector, line, objectives, snapshot); `sector` shows a sector's lines and things; `EVIL_DEBUG_OBJECTIVES=a-b` logs every option and why an objective ended. The watcher's own side panel calls `objectives()`, which changes the explorer's state: a watched run can differ from a sweep's (snapshots don't).

Found with them: E2M4's computer map sits in a cage of pillars with 32-unit gaps (in DSDA a 32-unit box doesn't pass, aligned or not), lowered by walk-over lines on the edge of a nook the game won't let her into; earlier runs got it somehow, still unexplained.

New baseline, expert (seed 0, unlimited lives, 3600 s, stall 300 s): **17/68 with every item**, 38 exited, 30 stalled (8 of those the boss/kill maps and MAP28). Restart loops to look at: E1M4 (26 levels again, 0 items), MAP30.

## Linux, and the name evil-boom (2026-11-08)

The user moved to a new machine (Fedora 45 under WSL) and wanted both platforms kept. The binary
is now `evil-boom` (the user: for Boom, and Evil's first original song); the package and library
stay `evildoom`.

- **WADs**: Linux defaults to `~/.local/share/evildoom` in Steam's layout (`base/...`), copied
  there rather than read through `/mnt/c` (slow); Windows keeps the Steam folder.
- **Engine build**: `tools/build-engine.sh` for both (Linux, MSYS2 MINGW64). The patch now sets
  the library's LIBRARY_OUTPUT_DIRECTORY too (a `.so` is a library output, a `.dll` a runtime
  one) and depends on `dsda-doom-wad`, so both land in `prboom2/build`. On Fedora,
  `CMAKE_FIND_PACKAGE_PREFER_CONFIG=OFF`: DSDA prefers package configs, and zlib-ng's names a
  `libz.a` that isn't installed.
- **Viewer**: minifb 0.27 -> 0.28. Under WSLg 0.27 segfaulted on X11 with AspectRatioStretch
  and corrupted the heap on Wayland (found with a minimal program, option by option); 0.28 is
  fine on both.
- **Checked**: the same results as on Windows, seed for seed (E1 one life 3.6 levels; Doom II
  7.9, MAP12 stuck 7/16). Expert sweep, 68 maps, seed 0, 8 jobs: **48/68 with every item**,
  56 exited, 10 stuck (the 8 boss maps, E1M4 44/45, MAP21 17/18, MAP26 45/58), 2 stalled
  (E4M6, MAP14), in **207 s** wall-clock.

## The fight as a tree (2026-11-08)

The user: a clear decision for which weapon, a clear one for how to fight with it, and a state
tree for the fight rather than one long chain of cases. Restructured with no change in play
(checked: every seed of the regression set and every arena run the same as before).

- **The tree** (`tactical/combat/tree.rs`): each tic, the first node that applies acts. Nothing
  in sight: out of a vile's sight · lost sight of it · the fight over. A target or a missile:
  *survive* (out of a vile's sight, dodging, out from under a crusher, away from a barrel) ·
  *busy* (in a jump: shooting on the way) · *fight* (backing off from what claws, to the errand,
  out of ammo: getting past, sneaking past, closing in out of reach, out of reach: going on,
  stepping for a shot, outmatched: keeping clear / getting past, keeping off, standing and
  fighting, shooting on the way). The aim and trigger are sized up first (`Combat::size_up`); a
  node only overrides them. Nodes keep the branches' own code: one may look, keep something (a
  clock, the steering's way) and pass.
- **Which weapon** (`combat/arsenal.rs`): slots as the game brings them up, ammo, armed,
  firepower, outmatched, and the gun for a monster, every rule for it in `best_gun`.
- **How to fight with it** (`combat/style.rs`): a weapon's window at a monster and its reach.
- **Retreats** (`player.rs`): an explicit state, `Retreat { kind, route }`, kinds cover / for
  ammo / back / out / bait, started by `start_retreat` (the first rule that holds, in that order)
  and ended by `retreat_over` (each kind its own).

## Up close: berserk and the chainsaw (2026-11-08)

Berserk wasn't observed: the fist was judged at 11 a punch whatever, and with nothing to shoot
she went off for ammo even berserk. The chainsaw was never used at all: Evil sends weapon numbers
straight in the tic command, so slot 1 always brought up the fist.

- **What the game does** (DSDA source): A_Punch is (P_Random%10+1)*2, ten times with pw_strength
  (20-200, 110 on average), a punch every 22 tics held; picking berserk up gives 100 health and
  the fist. A_Saw is the same 2-20 (berserk or not) every 4 tics held. Every weapon but none
  flagged WPF_SILENT alerts monsters when fired (P_FireWeapon): the fist too, so `would_wake`'s
  "the fist makes no noise" was wrong and is gone. Slot 1, as G_BuildTiccmd has it: the chainsaw
  once owned, but the fist while berserk.
- **Engine**: `evil_player.strength`; slot 1 brings up the chainsaw once she has it, unless
  berserk (as slot 3 the super shotgun).
- **Which weapon** (arsenal, bestiary): slot 1 is the berserk fist (110 a punch) while berserk,
  else the chainsaw (11 every 4 tics) once she has it, else the fist. Not the saw at what it won't
  keep flinching, nor the saw or the berserk fist at anything below 35 health, with a gun to hand
  (E3M4: at 14 health, up to a demon with the saw). Firepower counts slot 1 as the plain fist; the
  berserk fist counts against what it'd punch (`too_much`), and makes her armed against nothing
  but such (no retreat for ammo from them).
- **How to fight with it** (`Style::Melee`): the berserk fist at a monster of 400 health or less
  (what she'd take meanwhile is about punches x the chance it isn't flinching x its swipe: a
  revenant or cacodemon ~55-60, a hell knight 160, a baron 330), the chainsaw at 150 or less
  (a demon needs two unflinched saw hits in a row to bite, a pair in eleven). The tree's melee
  node: straight at it (the steering's pull to a gun's window left her circling a revenant
  100-200 off), then standing at it hitting; the weapon brought up at once, not whatever's best
  from where she stands while closing in (E3M4: rockets up at a demon 299 off, the fist at 46).
- **Arena** `arenas/berserk.txt` (the user's): the fist, a berserk pack, no ammo, 1-4 revenants
  in MAP32's hall. Before: 7/8, 3/8, 1/8, 0/8 (11/32); now 8/8 (15 s a win), 3/8, 3/8, 0/8
  (14/32). Two or more still lose: the run in across the open floor into their tracers, and the
  second punching her while she punches the first (cover on the way in, one at a time: later).

## Crushers in every phase (2026-11-08)

The commonest death (9 of 32 regression runs: E3M5 five times, E2M2 three, E3M4 once), traced on
E3 seed 1, E3M5: a switch in a 64-unit alcove under a slow crusher. She went in, it started
down, the fight's "out from under a crusher" pulled her out; it judged by her middle only, so it
let go with 14 units of her still under; the route follower, at its goal (walking up to the
switch), had no crusher check at all (only the walking phase did) and walked her back in; her
momentum took her deeper as the fight turned her round, and at 55 units over the floor she was
pinned.

The mechanics: P_TryMove refuses any move whose box touches a sector with less room than she's
tall (the lowest ceiling over the box): partly under a crusher below floor + 56, she can't move
at all till it rises (a slow crusher's round trip: ~100 tics, a hit every 4).

- **The fight**: under a crusher is any of her under it (her box's corners), not her middle.
- **The follower, every phase** (`crusher_guard`: at a goal, settling, fetching; walking has
  `crusher_ahead`): partly under a ceiling coming down with less than 64 units to spare, out from
  under it first (the escape walking already had); a ceiling coming down a step from her box,
  held where she is till it goes up.

Results (HNTR, one life): E1 3.6 -> 4.1 (1/8 through, the first), E2 3.6 -> 4.2, E3 3.6 -> 5.6
(three to the E3M8 boss), Doom II 7.9 -> 8.6; arenas unchanged.

## A shot barrel isn't gone till it's blown (2026-11-08)

E1 ended at E1M3-E1M5 in 7 of 8 seeds; at E1M5 every hit that mattered was her own barrel
blast. Traced (E1 seed 0, E1M5, tic by tic): a barrel in the way, a post chosen 155 off it (out
of every blast), the barrel shot from there; then "off back the way she came", which started 27
off the barrel (where she stood when she chose the post), and the barrel, hit, was left out of
what she saw at once, so it counted as gone: she ran back past it as it went off, 15 tics after
the hit (its death frames), 66 health and 34 armor, dead.

- **Barrels till they're gone**: the observation keeps a barrel going off (its death frames,
  before and after the blast) as a barrel; it's the most dangerous one there.
- **Shot from out of every blast, she stays**: `flee_safe` says so (no run checked then, none
  needed), the post's run off is dropped, and the settle after holds where she is rather than
  backing off from the nearest barrel (towards another, across the hall).

Results (HNTR, one life, 8 seeds; Doom II 16): E1 4.1 -> 7.5 (4/8 through, from 1); E2 4.2 ->
3.8, E3 5.6 -> 5.2 (1/8 through, the first), Doom II 8.6 -> 8.2 (paths change; E2 seed 0's new
end, E2M4 stuck, is exploration: no routes, 2 barrel lines in the log); 263 levels over the set
against 249. Arenas unchanged.

## Barrels cleared on sight (2026-11-08)

The user: shoot any barrel she sees, since barrels are dangerous from being blown up by enemy
fire too. Before, barrels were shot only when one blocked her way.

- **Seen**: barrels as monsters are (in view or near, nothing in the way: `Combat::barrels_seen`),
  kept while they're there.
- **Offered** (the fight's hooks, `Kind::Barrel`): each seen barrel within 1024, with nothing in
  sight (in a fight, the fight's barrel rules) and the ammo (the pistol with 10, or the shotgun
  with 12 shells: the barrel rule's guns); one objective a barrel, given up on as any other is
  (the explorer's backoff).
- **Done** (`Step::barrel`): the follower takes the barrel on as the step starts, from the
  nearest post out of every blast of its chain (the barrel-in-the-way routine: stay there if out
  of every blast, else the run off checked against the chain).
- **Pushed barrels**: a hit thrusts a barrel by its damage x 100 / its mass 100 (a pistol shot:
  6-20 units), so a barrel is the one there was if it's within 48 of where that was (it was taken
  for gone after each hit and shot at again as another, 16 times in E1M1).

Results (HNTR, one life): E1 7.5 -> 7.4 (4/8 through), E2 3.8 -> 2.8 (2/8; six die on E2M2, five
of them crushers, reached now rather than stuck before), E3 5.2 -> 5.5, Doom II 8.2 -> 8.6; 263
levels over the set either way; runs with barrel damage 5 -> 4. Arenas unchanged.

## E2M2: once-only lines that didn't take, and crushers by time (2026-11-08)

Traced first by a helper session in its own worktree (the user asked for it in parallel), then
applied here.

- **A once-only line pressed while another mover has its sector** (E2 seeds 3, 4: stranded for
  the hour on a lift's pillar with 8 units over her head): the game uses up an S1 switch or W1
  line only if its action starts (p_switch.c, p_spec.c), and a floor action skips a sector that
  has a floor mover already; her knowledge took it as used because the sector moved after the
  press (the lift's doing), never offered it again. Now it took only if a target moved, since
  the press, nearer where that line puts it (`Knowledge::moved_its_way`); not known where: any
  move, as before.
- **Fast crushers** (E2 seeds 5, 6: a 64-wide passage, its fast crusher open some 16 tics a cycle,
  caught under it again and again): "rising with room" let her start from a standstill with too
  little time to cross. Now a crossing is timed (`through_in_time`): the ceiling simulated at its
  pace last seen between floor + 8 and its top, her run from her pace now along the way (portal
  to portal: E3M4's ring round a pillar, a straight line was half its length); in only if there's
  room over her from her front getting under to her back out of it, and only while it's going up
  (or still with room: in on its way down on the sums was a dash to beat it, E3M4's slow one);
  then gone through, not waited out halfway; under one coming down, on through if she'd be out
  with room all the way, else out from under as before. A crusher's pace is by the tics since
  she last looked (the fight had her in between: a slow one taken to move 23 a tic).

Results (HNTR, one life): E1 7.4 -> 7.4 (4/8), E2 2.8 -> 5.8 (2/8 -> 3/8 through; none end on
E2M2), E3 5.5 -> 4.9 (E3M4/E3M5 crushers, by the A/B the timed crossing's: next), Doom II 8.6 ->
8.4; 263 -> 279 levels over the set. Arenas unchanged.

## Crushers stopped low; out from under one between routes (2026-11-08)

- **Stopped low** (E2 seeds 3, 4, 7, E2M4: an hour waiting at the computer room's cage, a
  perpetual slow crusher she'd stopped low herself by walking its crusher-stop lines; traced by a
  helper session in its own worktree, applied here): a perpetual crusher turns at each end at once,
  so still is stopped, and it stays so till a crusher line starts it again. The planner counted on
  its rest height (any trigger that isn't permanent "comes back"), the follower waited for it to
  rise or still with room (never), and the waiting kept the stuck clocks from running out. Now the
  planner doesn't count on a crusher's rest height while it's been heard stopped and still for a
  second (`Knowledge::stopped_for`), and the follower fails the step at a crusher still and too
  low for 3 s ("stopped too low to get under") for the explorer to plan round it.
- **Between routes** (E3 seed 2, E3M5: a switch pressed in a crusher's alcove, the route over,
  stood there till it came down): the fight's tree gets her out from under a crusher at work with
  no fight on, but only with no route under way (the follower's own crusher rules have her
  otherwise: taken over mid-way, a timed crossing under E2M2's fast crusher was turned back in it,
  E2 1.2 levels).
- **Committed crossings** only where the way comes out the far side, and an escape ends one (a
  switch's alcove under a crusher, pulled out of as it came down, walked back into on the replan).

Results (HNTR, one life): E1 7.4 -> 7.4 (4/8), E2 5.8 -> 7.0 (3/8 -> 4/8 through; no E2M4
stucks), E3 4.9 -> 4.9, Doom II 8.4 -> 8.3; 279 -> 287 levels. E3's crusher deaths on routes
(E3M4's ring, E3M5's alcoves) are still to trace: with the fight's escape on mid-route too, E3 was
5.8 but E2 1.2.

## No looking round under a moving ceiling (2026-11-08)

The last crusher deaths (E3 seeds 3, 7, E3M4), traced tic by tic: a route into E3M4's ring of a
slow crusher round its pillar, in as it went up; a line walked over on the way, and the explorer
stopped her to look round at what it had done (`glance`), half a second stood still as the
ceiling came down 160 -> 132; the route's crusher rules don't run while the explorer has her
looking, and the fight leaves crushers to a route under way, so nothing got her out till 59 over
the floor, too late. Rule: no glance while any of her is under a sector she hears moving.

Results (HNTR, one life): E1 7.4 -> 7.2 (4/8 -> 5/8 through), E2 7.0 -> 7.1 (4/8 -> 7/8 through),
E3 4.9 -> 6.9 (4 to the E3M8 boss), Doom II 8.3 -> 8.2; 287 -> 301 levels; no crusher deaths
left in the set. Arenas unchanged.

## Barrels in a fight with biters (2026-11-08)

E1M8's start (traced from pistol starts, 0/8 out): the yard is a grid of some 20 barrels, the
demons among them (the user: it's meant for blowing the barrels to take most of them). She walked
in to fight them and her own shots set the barrels off round her.

- **No shot through a barrel whose chain would reach her** (`barrel_in_line`): a bullet takes the
  first thing on its line; within the gun's spread at that distance (a missile: what its box
  touches), a barrel ahead of the target whose chain (each within a blast of the one before)
  comes within a blast of her holds her fire.
- **Clear of barrels whenever she's fighting**, not only with something shooting in sight: the
  "away from a barrel" move and the steering's barrel penalty; her shots set them off too.
- Tried and dropped: barrels by monsters shot only with their whole chain clear of her (stricter
  than "that barrel well clear"): fewer barrel kills, Doom II 8.0 -> 7.5 in the A/B, and E1M8
  didn't need it.

Results (HNTR, one life): E1M8 from a pistol start 0/8 -> 8/8 out. Sets: E1 7.2 -> 7.5 (5/8 ->
7/8 through), E2 7.1 -> 6.6 (E2M7: her own rockets, launched 8 tics after the trigger into a
wall she'd turned to: next), E3 6.9, Doom II 8.2 -> 8.0; 301 -> 296 levels. Arenas unchanged.

## Rockets: the lane at its own height, the way she faces, and turned off at the last (2026-11-08)

The crate arena (arenas/crates.txt) made her own rockets the loss: 5 of 6 lost fights, one rocket
at full health. Logged per rocket (where it came out, where it went off): it went off at an
obstacle's edge, never on it. Two causes: the lane was judged toward the target, but a rocket
goes where she faces, and she fires within a couple of degrees (a crate's corner clipped 3 off
the target's way); and the lane was judged as seeing, from her eyes, so a step as high as a
rocket's bottom (her feet + 32: the 32-high steps) was "clear" and flown into.

- **The lane at the rocket's height** (`rocket_flies`): rays at its bottom and top (her feet + 32
  and + 40, climbing or dipping to the target's middle), its box's reach each side (radius, 1.4x
  on the diagonal) and 8 more (a miss is better than a near thing, the user's); toward the target
  and the way she faces; only as far as a blast could come back to her (160 + 8): beyond, a miss
  (checked all the way she held fire at a cyberdemon till it had her: Doom II 7.4).
- **The last resort** (the user's: the trigger can't be called back): each tic till the rocket's
  out (8 after the trigger), if the way she faces would put it off within 160, turned to the nearest
  way that won't.

Results (HNTR, one life): crate arena 74/80 -> 75/80 (lost 6 -> 0 or 1, health left 77 -> 85);
deaths her own rockets led 5 -> 1, their damage 897 -> 589 over the set; E1 7.5 (7/8), E2 6.6 ->
6.9 (6/8), E3 6.9 -> 6.6, Doom II 8.0 -> 7.7, Doom II arena 227 -> 228; 296 -> 291 levels.

## Rockets where autoaim sends them (2026-11-08)

The user: autoaim catches human players, a rocket tilted down at what's below into a ledge's lip
(firing down into a pit) or into a railing it would fly over level. The lane was judged toward the
target and the way she faces; the game sends it to the first monster on her way within 1024, or
5.6 degrees to one side then the other (P_SpawnPlayerMissile, P_AimLineAttack), swinging and
tilting it to that one. `autoaim` predicts it (to the monster's middle: the game aims at what's seen
of it, above a railing higher, so the middle is the nearer of the two to what's in between); the
lane and the last resort judge that way too.

Arena: arenas/autoaim.txt (tools/autoaim_arena.py): ledges over pits found in Doom/Doom II
(E1M6, MAP03, MAP08, MAP09, MAP20) and MAP14's railings, the imps where a shot down reaches them
(autoaim's 100/160 slope); a spawn that doesn't fit is put at the nearest spot round it.

Results (HNTR, one life): autoaim arena own-rocket hits 31 -> 21; crate arena 75 -> 76/80; over
the set runs with own-rocket damage 13 -> 7, damage 589 -> 405; E1 7.5, E2 6.9, E3 6.6 -> 7.0,
Doom II 7.7 -> 7.6, Doom II arena 228 -> 229; 291 -> 293 levels. MAP09's pit: she holds fire at the
lip and never steps up to clear it (next: stepping for a rocket shot).

## Plasma where autoaim sends it, a safer gun for a blocked rocket, ducking hitscan heavies (2026-11-08)

- **Plasma** (the user's: not wasted when autoaim would put it in the lip below, or the floor in
  front of something up on a ledge): held when its middle wouldn't get to the target the way
  autoaim sends it (aimed at the visible part, as P_AimLineAttack does), and a step on toward it
  (never off the ledge) to where it would. Its middle only: with its box's sides, cover edges 14 off
  the line 700 out held her fire at a Spider Mastermind (MAP16's cover, 0/8 -> 6/8 lost).
  Not for rockets: stepping for a rocket shot put her by the crates' columns (own-rocket hits 26 -> 44).
- **A blocked rocket** (the user's: a safer weapon rather than one held): the launcher ruled out for
  2 s after its way at the target was blocked close by; kept (waited with) only when the next gun
  would take 3x as long (the pistol at a Cyberdemon, cells gone: 3/8 drawn). Rocket safety needs
  both slopes autoaim may give (the visible part's, the middle's) clear.
- **Running after her own rocket**: the lane is checked further the faster she's following it
  (20 a tic to her 16: a wall 600 out is a blast 120 from her). MAP06, shooting on the way, the
  route running her after it, dead twice.
- **Crushing floors**: a floor seen coming up lately, with under 72 left beneath the ceiling, is a
  crusher too (E2M4's secret: over a W1 "raise floor, crush" line, stood on it with nothing left to
  do). Lifts end level with a floor beside them, room enough over it.
- **Ducking** a hitscan shooter in a long fight (not over in 4 s with the gun up): it shows its
  wind-up (the spider mastermind turns to her 20 tics before its first burst, info.c; the zombies
  10), and fires on only while it sees her (A_SpidRefire, A_CPosRefire). She shoots through the
  wind-up and leaves for the nearest place out of its sight with just the time to get there, holds
  till its fire would have broken off, and "keeping at it" brings her back out. Leaving at once,
  she got next to no shooting done (spider-cover 1/8). Once it's firing: anywhere within 24 tics.
- **Posts**: in such a fight with nowhere near to duck (E3M8's open star), to the nearest place she
  knows of (seen or been, on a 32 grid: the nav's polys are too coarse in a hall) that none of them
  sees, with somewhere they would within 128 to step out to, across the open if need be (the
  user's: the building in the middle).
- **Boss ammo**: with a boss seen and about, rockets, cells and the BFG's cost 10x against anything
  else (E3M8: 138 cells on two cacodemons before the spider).
- Tried and dropped: steering toward places near a shadow (kept her behind the cover, unable to
  shoot: spider-cover 8/8 -> 2/8); fighting the spider from 768-1280 rather than 640-896 (the user's
  idea, its spread; spider-cover 6/8 against 8/8, E3M8 no better).

Results (HNTR, one life): E1 7.5 (7/8), E2 6.9 -> 7.0 (6/8), E3 7.0 -> 6.9 (0/8 -> 1/8 through: E3M8
won), Doom II 7.6 -> 7.8; arenas combat 120/128, Doom II 229 -> 230; autoaim arena own-rocket hits
21 -> 19, crates 76 -> 77/80 and 26 -> 21; plasma autoaim arena (arenas/autoaim-plasma.txt) 81 ->
83/112. E3M8 with an E3M7 loadout: 0/4 -> 1/6, the spider left at 370-1720 rather than 1300-2250.

## E3M8 on HMP, boss fights, ledges (2026-11-08)

Traced from E3M8 (the Spider Mastermind) from a pistol start on HMP and with the loadout she
brings from E3M7; each rule general, the map only its test.

- **Outmatched** counts a hitscan shooter however far (its bullets get there), but only a
  "sniper": one she couldn't see off in 4 s with what she has (`sniper`), not a zombieman across
  the map. It weighs her too (the user's): longer to kill than she'd last against it, her health
  with her armor's share of each hit (a third green, half blue: `lasts`), is outmatched.
- **No chasing** ("keeping at it") a monster she can't take on.
- **Taking a sniper on** (`takes_on`): once she'd see it off before it saw her off (gun, health,
  armor; not ammo, picked up as she goes), the outmatched moves don't apply to it and she stands
  and fights, whatever it's after; otherwise she keeps clear.
- **Leading walkers**: a monster walks by steps, its momentum nothing; tracked over 12 tics, the
  rocket or plasma ball led by that (90 rockets at the spider, aimed where it was, 200 damage).
- **Weapon churn**: the gun in hand counts as 15% quicker (another must be clearly better: the
  chaingun and plasma rifle swapped 17 times at 700-880); nothing readied for the next fight
  while an awake monster she knows of is about (the shotgun up each time the spider went out of
  sight, 14 times). E3M8 with the E3M7 loadout: HNTR 9 -> 13/16, HMP 2 -> 7/16.
- **A boss in sight**: others further than 384 not shot (left to it: infighting, the user's).
- **Danger**: an objective a sniper (from where she last saw it) could see counts as near it.
  **Ammo** counts as twice as far with something she can't take on about (the building's plasma
  rifle and blue armor first: 8/16 got them, 0/16 without; half as far, 0/16).
- **Posts** look out to 1536, the far side of what's between her and them preferred (the user's:
  the other side of the building). **Exposure**: ways across ground a sniper after her would see
  her on count 3x (she routes round; replanned once when it first appears); one she can see busy
  with another monster doesn't count. **Hold at the edge** (the user's: a player hears when its
  attack's over): hidden, about to step into the sight of a sniper that's firing, she waits for
  it to stop (its wind-up then gives time to cross).
- **Ledges**: the fight's `safe` judges a drop from where she stands, not the floor under her
  middle (on a lip, her middle over the drop, the game keeps her up: MAP06, walked off a crate top
  552 down, then along a nukage channel's lip and in); a route ending within 48 of a drop is
  arrived at slowly (E1M3, run at a pit's lip off a step down, airborne, over); no barrel firing
  spot on a floor that hurts.
- The observation carries her armor type; the death line what's in sight's health.

Tried and dropped: counting known ammo on the map toward outmatched (no effect); fighting a
sniper from cover even outmatched (every pistol start dead at 25-36 s); a landing margin for drops
(the MAP06 falls weren't the landing).

Results (HNTR, one life, 8 seeds, last set): E1 7.5 -> 8.0 (8/8 through), E2 7.0 -> 6.1 (2/8),
E3 6.9 -> 5.0, Doom II 7.8 -> 7.1 (16 seeds; MAP06 10/16 failed); arenas combat 120 -> 119/128,
doom2 230 -> 228/240. The sets swung a lot between builds (E1 6.1 / 8.0, E2 8.0 / 6.1 on small
changes): routes shift and meet other old weaknesses. E3M8: from E3M7's loadout HNTR 13/16, HMP
7/16; from a pistol start on HMP 1/16. Committed with these numbers (the user's call: the rules
are sound, tune later). The regression set changes to Doom E1-E3 episodes plus Doom II single
levels from a pistol start (the user's: MAP06 is a gimmick map, no test).
