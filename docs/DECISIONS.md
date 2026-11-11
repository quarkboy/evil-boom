# Evil BOOM — Decision Log

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

## Invulnerability and the BFG (2026-11-08)

The user's: better planning for both (seen in an E3 run). From the game's own:
- **Invulnerability** is known from its pickup message ("Invulnerability!": 30 s, the last 3 not
  counted on), as a player knows it. While it lasts nothing outmatches her; no retreats, ducking,
  dodging, holding at edges or keeping out of sight; her own rockets safe; hurting floors fine (drops
  still not); nothing too dangerous to go for.
- **Kept for a high-risk fight** (the user's: super high priority then): a sphere isn't gone for
  unless something she knows of, awake, outmatches her (`needs_invulnerability`); then it's urgent
  (before keys) and an errand mid-fight, so long as the way to it doesn't pass within 320 (and
  its radius) of what she can't take on: kept clear of till it moves off the line. E3M9 (E3
  loadout, 8 seeds): its sphere behind the cyberdemon; spotted and left: killed by it 1/8; run at
  straight past it: 5/8; with a clear way: 0/8, the cyberdemon killed 8/8.
- **BFG**: the launch (30 tics; the rocket's 8) counted in the time to kill; while her ball's in the
  air no ducking (A_BFGSpray: forty lines from her, ±45° along its flight, 1024 out, when it
  lands); its crowd counted in that cone, not all in sight. Tried and dropped: no BFG unless a
  boss or 3 in the spray (E3M4: plasma at close cacodemons spent 186 cells and died; the BFG, 120,
  through).

Results (new set: Doom episodes, Doom II single levels from a pistol start; HNTR, one life):
E1 8.0 (8/8), E2 6.1 -> 5.9 (2 -> 3/8 through), E3 5.0 -> 6.5 (0 -> 1/8), Doom II 23 -> 24/32;
arenas combat 119 -> 118/128, doom2 228.

## The BFG's aim and hold; melee chases given up; dodging across, not along (2026-11-08)

- **BFG aim**: with it up, the target is the monster (one she has a shot at, within 1024) whose line
  puts the most threat in the spray, ±45° of the ball's way (A_BFGSpray: the ball goes to the
  first on her line, the spray along the way it went, from her).
- **BFG hold**: from the trigger (the ball leaves 30 tics on) till it lands, she stands still (a
  survival node after dodging): no ducking, retreats or posts, the route not walked on; the spray
  comes from where she is.
- **Melee chases** (the user's): closing in to get at something in melee has its own clock, as the
  out-of-reach chase has (20 s, then left 30 s); that clock never ran for melee, a melee target
  always counting as reachable. E3M4: a demon she couldn't get at, closed in on and lost sight of
  for an hour (1905/2462 s -> 750/806 s).
- **Dodging** (the user's): among ways that clear, across a missile's way rather than along it
  (24 x |cos| off). Backing straight off outran an imp's ball (16 to its 10) and scored as clear,
  then open room ahead decided: off down the room, the ball after her.

Results (HNTR, one life): E1 8.0 (8/8), E2 5.9 -> 7.1 (3 -> 7/8 through), E3 6.5 (1 -> 2/8),
Doom II pistol-start levels 24/32 (MAP10 0 -> 3/4); arenas combat 118 -> 119/128 (health left
83.7 -> 85.0), doom2 228/240 (85.8 -> 86.5).

## Crushers by kind; spent teleporters; out from under all of her; no stepping for a shot in nukage (2026-11-09)

- **Out from under all of her** (E2M2's crusher corridor): the escape from a ceiling coming down
  aims for a poly whose middle has her whole box clear of moving ceilings, not the first poly not
  moving (her middle out, an edge still under, pinned against the wall beside). 32/32 E2M2 seeds
  exit (30/32 before).
- **Spent teleporters** (E3M9's stuck, 3 of 8 E3 seeds): a W1 teleporter she's gone through this
  life is a line like any other (vanilla clears its special; she's only taken to have gone through
  one when she was teleported, so in every complevel). The navigator never walked a teleport line
  from its front; E3M9's red key lies east past an 8-wide strip with one in it, so after the
  ambush there, out to heal, the way back was a wall. Now her view (`Restriction::spent`) opens
  those portals; the omniscient planner is unchanged.
- **Crushers by kind** (DSDA's p_ceilng.c, p_map.c; `crush.rs`): 10 every 4th tic while the ceiling
  is lower than she's tall; she can't move then. Slow ones (25, 49, 73, 141, Boom's under 3x)
  slow to an eighth on her: held, dead. Fast ones (6, 77) keep on: one stroke, 120 raw at the
  worst, armor taking its share.
  - **The fight** (E2M4: a slow crusher stopped 9 over her head, chased into after an imp across
    its own start line, set off, pinned in 9 tics): a crusher she knows is no-go, stopped or not,
    when its room over her head in tics is less than getting out of it takes at the worst (from
    its farthest point, starting at full speed the wrong way) and its stroke costs more than she'd
    take on (slow: always; fast: more than a tenth of her health, the user's).
  - **Routes** (the user's (c)): crossing a crusher's start line into it while it's stopped like
    that costs a stroke of it in distance (a way round shorter than that is better), and the
    follower, at such an entry, steps 2 over the line at a crawl, out as it starts, waits at the
    edge and crosses on its way up (the timing it had). If backing out wouldn't beat the ceiling,
    the step fails. A crusher stands still a tic as it turns: still longer than 2 tics is stopped.
- **No stepping for a shot on a floor that hurts** (E3M5's nukage pit, 4 deaths in 4 sets): an imp
  on the ledge above 500 off, no shot from the pit; stepping for one goes away from a target
  above (the angle), out past the 512 the node applies within, the route's way off brought her
  back in: 570 tics of it, dead, at 5.5 a second (the fight's back-out is from 6). The step only
  kept her on the floor; she shoots on the way off. E3M5 8/8 seeds (6/8).
  Not done: the same flip at 512 off a hurting floor (a stall, not seen yet).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.4 (7/8 through), E3 6.5 -> 6.8 (1 -> 0/8 through;
no E3M9 stucks, no E3M5 deaths), Doom II 27/32; arenas combat 119/128, doom2 226/240. E2M4 seed 5
(stuck: dropped into a pit for a blue armor, its only way out a teleporter behind a door she
hadn't seen) and E3M9's ambush on arrival are older problems met in a different state.

## Level campaign, round 1: barrels going off, waits that aren't coming, a hurting floor's own cost (2026-11-09)

Pistol-start single levels on all four skills, four helper sessions a round, each with one level
(the user's). Round 1: E1M5, E1M6, E1M9, E2M2.

- **A barrel going off** (E1M6 UV: one set off by a fireball, its blast set off a second and
  pushed it her way, met while dodging the imp's last fireball): the observation has the barrels
  at 0 health still there (hit: 15 tics from blowing); a survive node, before ducking and dodging,
  takes her away from the nearest barrel when one going off, or its chain, would reach her (walls
  not counted). E1M6 UV seeds 0-2 exit (died).
- **A wait for a sector that isn't coming** (E1M5 UV: a platform still rising from the move before
  ignored the lowering line she crossed, as vanilla's EV_DoFloor skips a sector with a mover; she
  waited in the nukage, counted "coming" for 1.5x its travel + 10 s, dead): it's coming only while
  it's getting nearer the heights wanted (within a second). E1M5 UV: died -> stuck (on the
  lowered platform, every way "no route": to trace).
- **A hurting floor at the end of a route costs what the map says** (E1M9: 5 a hit counted as 20,
  every way on from a lip over the nukage refused at 68 health): the worst (20) only for a floor
  the map gives none for, as on the way. E1M9 still stuck (the way off the floor cuts into a wade
  to a timed door).
- E2M2 UV traced, not fixed: out of ammo early, the ammo errand into awake monsters, the fist
  chosen over the pistol against melee-only monsters.

Results (HNTR, one life): E1 8.0 (8/8), E2 7.4 -> 7.8 (7 -> 6/8 through: E2M8's cyberdemon twice),
E3 6.8 -> 7.5 (0 -> 1/8), Doom II 27/32; arenas unchanged.

## Level campaign, round 2: teleporters from the front, ammo errands through monsters, one-hit claws (2026-11-09)

Round 2: E2M3, E2M5, E2M7, E2M8 (pistol start, all four skills).

- **A teleporter goes off only crossed from its front** (E2M7 HNTR: a WR teleporter reached from
  behind on a pad by nukage, the Cross step counted done either way, "didn't take me anywhere",
  the only way out dropped, died in the nukage): EV_Teleport does nothing from side 1; the step is
  crossed only from the front, so from behind she goes back over it from the front. HNTR seeds
  0-2 exit (died).
- **A walk for ammo ends when one of them steps onto its way** (E2M5 UV: out of ammo, a demon
  moved between her and the clip, pushed against, bitten from 96 to 29): the rest of the planned
  path passing a monster after her nearer than their bodies touching ends the retreat. UV still
  dies (out of ammo by 218 s).
- **One hit that could kill her** (E2M3 UV: out of ammo at 35, a shell under a baron, its claw
  10-80): she backs off from a clawer whose biggest hit (p_enemy.c: imp, lost soul 24; demon 40;
  cacodemon, revenant 60; baron, knight 80) is her health or more, armed or not, and the pull
  towards an errand stops when it's within that one's reach (80). UV lives longer on 3 of 4 seeds;
  still dies.
- E2M8 traced, not fixed: an objective chosen before the cyberdemon woke is never vetted again,
  and losing sight of it a moment ends the fight and hands her back to a way into its view.

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.4 (6 -> 7/8 through), E3 7.5 -> 7.1 (1/8),
Doom II 27 -> 28/32; arenas unchanged. The new E3 failures (her own rocket at a lost soul 150
off, E3M7; E3M9's arrival ambush holding the fist) are outside these changes: to triage.

## Level campaign, round 3: rockets held only for what can get in their way, a switch first for a gun out of reach (2026-11-09)

Round 3: E2M9, E3M2, E3M3, E3M4.

- **Rockets held only for a claw that could come into their way** (E3M4 UV: four rockets held for
  every demon 237-344 off, "rocket not safe", the pistol spent on them, punched to death by five):
  a melee monster seen in the last 3 s had its 128 of leeway on the ring round her too, so any
  within 334, behind her included, held the rocket. A rocket bursts only on what's in its way: the
  leeway now widens only the way (beside the line to the target, or its reach beyond); the ring is
  each one's radius plus the blast. E3M4 HMP seed 0 exits (died 194 s); UV lives 53 -> 2939 s.
- **A switch first, with a better gun in sight she can't get to** (E3M3 HMP/UV: the shotgun up on a
  floor a switch lowers, seen at the start, the switch ranked with the walls and left all level,
  out of ammo with the pistol, dead): switches count as a quarter as far while a better gun lies on
  ground she's seen but can't reach. E3M3 HMP seeds 0, 1 exit (died).
- Traced, not fixed: E2M9 (out of ammo among barons, the only ammo behind the one door she never
  opens: no known ammo means visited places, never the unexplored); E3M2 UV (a cover retreat's
  route ran her into shotgun guys just seen, the fight's way the other way overridden).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.4 -> 7.1 (7 -> 6/8 through), E3 7.1 -> 7.2 (1 -> 3/8),
Doom II 28 -> 27/32; arenas unchanged. E3M6's stucks (3) are as before on the level alone.

## Level campaign, round 4: a crusher crossing ends, doors aren't crushers (2026-11-09)

Round 4: E3M5, E3M6, E3M7, E3M8.

- **A crusher crossing ends once she's out from under it with none ahead** (E3M8 HNTR: a door just
  opened taken for a crusher, a crossing timed, never cleared: `crossing_crusher()` kept the fight
  "busy" for the rest of the route while a cacodemon came up and bit her for 60). Cleared out
  from under with none ahead, and while waiting at an edge. HNTR seed 0 46 -> 73 s.
- **A sector only doors move is no crusher to the fight** (E3M5 HMP: a DR door closed onto her
  box's edge, taken for a crusher at work, "out from under a crusher" handed her to the route,
  parked at the door under fireballs): vanilla's T_VerticalDoor never hurts (a closing door goes
  back up, a close-only one rests on her). HMP seed 1 exits, seed 0 124 -> 160 s; UV 588 -> 786 s.
- Traced, not fixed: E3M6 UV (out of ammo, walked past a cacodemon whose bite isn't a one-hit
  kill); E3M7 UV (out of ammo, fleeing across blood at 5.5 a tic: `Motion::go` slows her moving
  off her facing, the route's assumption, while the fight holds her facing; Doom runs her
  backwards at full speed; both versions of a fix moved HMP seed 0 the wrong way).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.1 -> 7.8 (6/8 through; only E2M8 stops it), E3 7.2 ->
6.8 (3 -> 0/8; E3M8 six times, E3M4 twice), Doom II 27 -> 25/32 (MAP12 seeds 0 and 3, MAP05 seed 3:
the runs part by two tics at a door no longer taken for a crusher; latent, for the Doom II pass);
arenas unchanged.

## Level campaign, round 5: a blocked sprint, a melee she can't land, a monster in an errand's way (2026-11-09)

Round 5: E3M9, Doom II MAP03, MAP04, MAP05.

- **A run-jump's sprint stopped short of the takeoff is blocked** (MAP05 HMP: imps at the takeoff,
  the sprint pushed into them for its 4 s with the route keeping the legs, 173 health to dead):
  still short and under 2 a tic a quarter second in, the sprint's off and the route fails; the
  fight has the legs again. Not counted a missed jump.
- **A melee that doesn't hurt it is given up** (MAP05 ITYTD stuck: a demon a floor below, in the
  fist's reach across but too steep to hit, "at it" resetting the clock for 3200 s): the give-up
  clock goes back only when the target's health does.
- **A monster in an errand's way is shot first** (MAP04 HMP: a zombieman in the doorway on the way
  to the shotgun, pushed against for 80 tics while she aimed at the shotgun guys behind, shot down):
  on an errand, one that attacks, touching-near and within 60 degrees of the way, is turned to and
  shot (the route keeps the legs). MAP04 HMP seed 0 exits (died at 12 s).
- Dropped after the regression set: a retreat taken over only by escaping moves, the errand held
  while retreating (MAP03 UV seed 0 exited with it): arenas 119 -> 115/128 and 226 -> 222/240.
- Traced, not fixed: E3M9 UV (out of ammo among imps from a door, shots spread over the crowd);
  MAP03 HMP (backed out of an ambush, the route straight back up the lift into it).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.9 (7/8 through), E3 6.8 -> 6.2 (0 -> 1/8; E3M6's
teleporter stuck, E3M9's arrival), Doom II 25 -> 26/32; arenas unchanged.

## Level campaign, round 6: a jump given up only for a body in the way; switches from a hurting floor at a dead end (2026-11-09)

Round 6: Doom II MAP06-MAP09.

- **A run-jump is given up only for a monster's body in the way** (round 5's rule, "stopped short
  of the takeoff", broke MAP06 ITYTD/HNTR: knocked aside mid-sprint, stood a moment, the plasma
  rifle's jump given up, stuck; and missed MAP05 HMP's imps, against her just past the takeoff):
  stopped a quarter second into the sprint with a living monster's box touching hers ahead (towards
  the takeoff, or past it the landing). MAP05 ITYTD/HMP and MAP06 ITYTD/HNTR seed 0 exit.
- **At a dead end, switches pressed from a floor that hurts** (MAP09: the start an island in a
  nukage pit, the lifts' switches pressed from the nukage, all left out by "no switch from a
  hurting floor with floor that doesn't in reach", the island counting): once nothing at all is
  left (walls, the panic round, the second goes), they're offered. MAP09 with no monsters exits.
- Dropped after the regression set: MAP08's rocket judged at its window's distance (MAP08 seed 1
  lost, none gained) and MAP07's re-plan when a seen gun comes in reach (MAP07 HMP seed 2 and MAP10
  seed 0 lost, MAP07 UV seed 0 gained). With both, E3 was 8.1 (4/8 through) but E2 6.8, Doom II 24.
- Traced, not fixed: MAP06 UV (a better gun urgent skips the danger check: the spider by the
  plasma rifle); MAP10 HNTR (with MAP07's change) killed by her own rocket at full health beside a
  door, the fight 8 tics over: to trace.

Results (HNTR, one life): unchanged: E1 8.0 (8/8), E2 7.9 (7/8), E3 6.2 (1/8), Doom II 26/32;
arenas 119/128, 226/240.

## Level campaign, round 7: health an errand only near, a cheap way not refused for health (2026-11-09)

Round 7: Doom II MAP10-MAP13.

- **Mid-fight, health is an errand only within 512** (MAP10 UV: at 38 health a stimpack 1097 off
  was urgent, and an urgent item was the fight's errand at any distance: every decision "towards
  the Stimpack" across a yard of imps, zombiemen and shotgun guys with the pistol, dead): urgent or
  not, as other health was; urgent things that aren't health unchanged. UV 70 -> 82 s, HMP 350 ->
  449 s (still dying).
- **A way costing a tenth of her health or less isn't refused for being short of health** (MAP12
  HNTR stuck: at 13 health every way over the nukage, 0.5 a way under a suit, refused while health
  less cost was under 25, waiting for healing or a suit she couldn't reach without them; stood on a
  pillar for the hour): the mortal margin applies only to a way costing more than RISK_SHARE of her
  health. MAP12 HNTR seed 0 exits.
- Rejected: MAP13's "no item under a running crusher in a fight" (lost HNTR seed 0: a different
  run, dead to her own rocket).
- Traced, not fixed: MAP11 HMP/UV (the target a pain elemental or cacodemon while hitscanners do
  the damage; keeping clear only of the target); MAP13 (a plasma rifle in a room of two crushers,
  the urgent objective never interrupted, pinned straddling both).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.9 (7/8), E3 6.2 (1/8) unchanged; Doom II 26 -> 27/32;
arenas unchanged.

## Level campaign, round 8: closing in round a corner, every frame of a liquid's animation (2026-11-09)

Round 8: Doom II MAP14-MAP17.

- **Closing in, round a corner** (MAP14 HMP: a door jamb in line with her box's edge, `clear_run`
  0 straight on every tic, `closing_in` gave up and she stood in a chaingunner's fire from 88 to
  dead): straight on not clear, she veers 20 then 40 degrees either side (still closing) before
  giving up; Doom slides her past the corner. MAP14 seeds 0-3, all four skills: no exit lost.
- **A liquid's floor looks like one in every frame of its animation** (MAP15 HNTR: a pit floored
  SLIME12 taken for dry ground, "SLIME0" matching SLIME01-09 only; dropped into it, the only way
  out a teleporter, dead): SLIME10-12 too (SLIME09-12 one animation). HNTR seeds 0-3 live longer
  (none exits yet); ITYTD seed 3 now runs a different way and dies out of ammo to a baron.
- Traced, not fixed: MAP16 ITYTD (on a pillar under imp fire, a dodge with no safe way still takes
  over and drops the retreat's legs; with a way, it flips each tic); MAP17 HNTR (an arch-vile's
  wind-ups with nowhere to hide: fought on from full, cover that doesn't break its sight).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.9 -> 7.8 (6/8), E3 6.2 -> 6.6 (1/8; E3M4 deaths
gone), Doom II 27/32; arenas unchanged.

## Level campaign, round 9: no coasting off a ledge in a fight, a gun urgent only on a way she can take (2026-11-09)

Round 9: Doom II MAP18-MAP21.

- **The fight brakes rather than coast off a ledge** (MAP19 HMP: a run-jump onto a 16-wide strip
  over a demon pit; the fight took the legs with no safe move and gave no thrust, the jump's speed
  slid her 33 units off the lip into the pit, stuck there for the hour): with the legs and no move
  of its own, the fight runs its slide check with no push; if her speed would take her over a drop
  she can't climb back or onto a floor that hurts, she brakes (`coasting_off`).
- **A gun or ammo is urgent only on a way past nothing she couldn't take on with it** (MAP20: a
  rocket launcher by the cyberdemon, urgent, so every danger check skipped, gone for again and again
  under its rockets; a launcher on the floor is 2 rockets): urgent only when the straight way to it
  passes nothing she couldn't take on armed with it (`clear_way_to` on what she'd have); otherwise
  an ordinary objective, danger checks and all. MAP20 ITYTD 62 -> 168 s, HNTR 23 -> 45 s.
- Left out after the regression set: MAP21's "a press carried out of the use ray's reach fails
  the step" (a dodge over a teleporter left her pressing a door 1500 off, the door written off).
  Failing cost the Doom II arena 4 fights (doors in boss arenas' cover dropped mid-fight); walking
  back to the press spot instead cost Doom II three levels (back into the fire). To revisit.
- Traced, not fixed: MAP18 (out of ammo by 116 s on HNTR; an untried teleporter into cacodemons).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.8 -> 7.0 (6/8), E3 6.6 -> 7.9 (1 -> 4/8 through), Doom
II 27 -> 28/32 (only MAP08 fails); arenas combat 119 -> 118/128, doom2 226/240.

## Level campaign, round 10: a barrel chain going off, kept off out of a fight too (2026-11-09)

Round 10: Doom II MAP22-MAP25.

- **A barrel chain going off, out of a fight** (MAP23: a mancubus's fireballs set the start room's
  barrels going; the fight over, the route walked her into the chain burning through the field,
  dead): the follower handled only barrels she shot herself. Now, with any barrel going off, she
  works out its chain (each within a blast of one going off, in turn): a blast reaching her, away
  from its nearest barrel; one reaching where she's moving within 15 tics (a barrel's death
  frames), she holds; on once neither. MAP23 ITYTD seed 0 7 -> 19 s, seed 2 5 -> 20 s.
- Traced, not fixed: MAP22 HMP (health spent on repeated risky trips over nukage, then the lift's
  switch, pressed only from the nukage, refused at 16 health); MAP24 UV (sleeping imps make every
  near objective dangerous; the super shotgun on a ledge spotted, never an objective); MAP25 (a
  stepping stone with five barrels: the next run-up past the middle one, which can't be shot from
  out of the blasts; the planner takes barrels inside a poly as walkable).

Results (HNTR, one life): E1 8.0 (8/8), E2 7.0 -> 7.4 (6 -> 7/8), E3 7.9 -> 8.0 (4/8), Doom II
28/32; arenas 118/128, 226/240.

## Level campaign, round 11: lining up a jump isn't a jump (2026-11-09)

Round 11: Doom II MAP26-MAP29.

- **Only a jump under way keeps the fight off the legs** (MAP27 UV: out of ammo at 43 health, routed
  over a run-jump; at the run-up, the whole jump counted "busy" so the fight couldn't move her, yet
  it turned her to aim at a spectre, so the line-up never squared her to the takeoff: she stood
  there, two spectres biting, dead): the sprint and a run over sinking floors keep the legs; lining
  up is standing with no jump begun, so the fight has them; a line-up it broke off starts its clock
  again. UV 107 -> 154 s; MAP27 ITYTD/HNTR seeds 0-3 unchanged.
- Traced, not fixed: MAP26 HMP/UV (out of ammo with dropped chainguns 110-530 off, all rejected as
  past monsters, "none known" in the log; keeping clear of chaingunners by distance, in their fire);
  MAP28 (UV: three pain elementals; HMP: the target a cacodemon while a shotgunner 83 off shoots);
  MAP29 (a lava alcove left only by a plain-textured lift switch; "try the wall" always "no route",
  even at 78 health: to trace).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.4 (7/8), E3 8.0 (4/8), Doom II 28/32; arenas
118/128, 226/240.

## Level campaign, round 12: unarmed, the route keeps the turn; Commander Keens fought (2026-11-09)

Round 12: Doom II MAP30-MAP32, Doom E4M1.

- **Unarmed, the route keeps the turn** (MAP31 HMP: out of ammo, a retreat for ammo through a door
  that had shut again; the fight's decision merged with the route turned her to aim at an SS, the
  route presses a door only facing it: 450 tics in front of it, shot down): with nothing to shoot
  with (`armed()` false), the route's turn stands in a merged decision. MAP31 HMP seeds 0, 1 live
  longer; no exit lost over seeds 0-3 on all skills.
- **Commander Keens are monsters to the fight** (MAP32: the exit behind tag 666, opened by the last
  Keen's death; the fight left Keens out, so never shot, no exit seen); and **no fist at something
  harmless she's given up getting at** (Keens over a ledge, punched at for a minute with bullets in
  the pistol). For anything that fights back, wider, it cost E3 0.8 levels.
- Traced, not fixed: MAP30 (the brain isn't a target or an exit trigger to her, and is killed only by
  rocket splash fired level through a slot from the lift, unseen: a new behaviour); E4M1 UV (a
  baron through a narrow opening, the rocket's lane blocked, 12 rockets unused: nothing moves her to
  a clear lane).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.4 (7/8), E3 8.0 (4/8), Doom II 28/32; arenas
118/128, 226/240.

## Level campaign, round 13: out of ammo, to the ammo itself; a floor learned harmless only unprotected (2026-11-09)

Round 13: Doom E4M2-E4M5.

- **Out of ammo, to the ammo itself** (E4M5 UV: "away for the Shell" went to the middle of the poly
  it lay in, 78 off it in a big room, "got there" without touching it, ten times; died out of
  ammo): to the item's own spot. E4M5 UV seed 0 exits (died at 117 s); no exit lost seeds 0-3.
- **A floor is learned harmless only from time on it unprotected** (E4M2: lava walked under a suit
  learned "doesn't hurt after all" the moment the suit ran out): the clock from the latest of
  stepping on, the last hurt there, and the suit's end.
- Traced, not fixed: E4M2 (in a one-way lava pit at 32 health, the way out refused by the tenth-of-
  health cap; lifting the cap on hurting floors made HNTR worse); E4M3 UV (a one-way drop into a
  crowd at low health for a medikit: routes' risk counts floors, not monsters); E4M4 HMP/UV ("keeping
  clear" of cacodemons it can't kill while firing at them: every gun emptied).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.4 (7/8), E3 8.0 (4/8), Doom II 28/32; arenas
118/128, 226/240.

## Level campaign, round 14: unseen walls a way out of a pit, a switch before walls in one, no fist at a killer (2026-11-09)

Round 14: Doom E4M6-E4M9. The end of the 10-minute pass.

- **A drop isn't a dead end while there are walls down there she hasn't seen** (E4M6 ITYTD stuck:
  the only way on a drop into a lava pit whose way out is lifts on walls unseen from above; the
  dead-end check counted unknown map, known switches and teleporters, not unseen walls). A pit seen
  all round is still refused. ITYTD stuck -> dies later (sniped on a ledge).
- **On a hurting floor with no floor that doesn't in reach, switches and doors before walls** (E4M8:
  a missed jump into a lava pit, the way out a lift switch 404 off; the risky pass ranked plain walls
  75 off first, three tried, 170 tics of lava, dead): walls are dropped while a switch or door is on
  offer; a seen switch was built as a way out, a wall's a guess. E4M8 seed 0 ITYTD, HNTR, HMP exit
  (all four died).
- **No fist at what one hit of could kill her, with a gun to hand** (E4M7 HMP: berserk at 41 health,
  the fist free and so chosen at a baron, the tree backing off: 600 tics without a shot fired):
  `claw_max` >= health rules the fist out when she has a gun with ammo. E4M7 HMP 1/4 -> 3/4 exits.
- Dropped after the regression set: E4M9's "not moving" judged from her position change rather than
  her momentum (pinned against a demon's box, momentum building, 4 bites): right by the mechanics,
  but E3 8.0 -> 5.4 (E3M6 stuck three times). To revisit.

Results (HNTR, one life): E1 8.0, E2 7.4 (7/8), E3 8.0 -> 7.9 (3/8), Doom II 28/32; arenas 118/128,
226/240.

## Larger changes after the 10-minute pass: a rocket's way held through its launch; no ammo spent on what she can't kill (2026-11-09)

The 10-minute pass's re-survey (pistol start, seed 0; exits ITYTD/HNTR/HMP/UV): Doom E1-E3 25/23/20/11
of 27 (26/22/16/11 before the campaign), E4 7/5/2/1 of 9 (6/5/2/0), Doom II 20/18/4/2 of 32
(20/18/3/3): 132 -> 138 level-skills.

- **A rocket's way held through its launch** (MAP32 UV, killed by her own rocket at 31 health; MAP30
  ITYTD, 112 of 182 damage taken her own rockets): the launcher lets the rocket go 8 tics after the
  trigger, along the way she faces then. Pressed at an SS, the fight took a new target the next
  tic and swung her 86 degrees through those tics; out mid-turn into the wall beside her. Now, from
  the press till it's away, her aim is held at the way she faced at the press (the way it was
  judged safe), unless that way is now unsafe (the turn off a rocket's way out, as before). MAP32
  UV and HMP: no rocket damage to herself. Tracking within 10 degrees instead (to keep leading a
  moving target) did worse over the regression set (E3 8.2 -> 7.2), so held.
- **No ammo spent on what she can't kill, keeping clear of it** (E4M4 HMP/UV: two cacodemons kept
  clear of and shot at till every gun was empty, then bitten to death unarmed): outmatched and
  keeping clear of a target all her ammo couldn't kill (`ammo_short_for`, from `too_much`), she
  holds fire unless it's in her way. Neutral over the regression set; E4M4 still runs dry by other
  ways into the same fight (what's wanted is not walking into it: the route, to do).

Results (HNTR, one life): E1 8.0, E2 7.4 -> 7.0 (4/8), E3 7.9 -> 8.2 (2/8), Doom II 28/32; arenas
118 -> 119/128, 226 -> 225/240.
- **Shut in on floors that hurt, a line to set off isn't held to a tenth of her health** (E4M2: a
  lava pit, no floor that doesn't hurt in reach, at 32 health the door's line out refused, 23
  against a cap of 3.2; walls tried in the lava till dead): staying costs all she has, so the cap
  is lifted for objectives with a trigger (switch, door, walk-over or teleport line) while she's on
  a floor that hurts and all her reach does. Lifted for every way (walls, looking, fighting) was
  worse (E4M2's helper: HNTR 386 -> 174 s). E4M2 ITYTD seed 0 exits (died). Regression set level
  (doom2 arena 225 -> 226).
- Tried and dropped: targets weighed by time to kill (threat over the kind's health, softened):
  Doom II 28 -> 25 (MAP10 twice, MAP02), E3 8.2 -> 7.6. Too broad; a narrower form (hitscanners in
  range over slow missile monsters) is for later.

## 20-minute pass, round A: unarmed to the ammo before cover; a floor hurts the same each time; a purposeful wade looked round (2026-11-09)

The 20-minute pass takes the 54 levels still not beaten on UV (pistol start, seed 0), four agents a
round, 20 minutes each. Round A: E1M5, E1M7, E1M9, E2M3.

- **Unarmed and shot at, the ammo before cover** (E1M5 UV: fists out at 13 health, an imp's
  fireballs and a demon on her; hid from the imp 190 tics, the demon followed, dead as the cover
  ended): low and shot at with no gun that has ammo, cover doesn't end anything (the cover ends
  when they stop shooting, and nothing she does there kills them): the retreat is for ammo, cover
  only when there's no way to any. E1M5 UV seed 0 exits.
- **A floor hurts the same each time** (E1M7 UV: a zombieman's 15 and a shotgunner's 9 on a plain
  floor 96 tics apart taken for the floor, and the room the whole level goes through refused): a
  floor's damage comes every 32 tics and is the same amount each time, so a second hurt only
  confirms the floor when it's a multiple of 32 tics after the first *and by as much*. E1M7 UV
  seed 0 exits.
- **A purposeful wade looked round before wading back; a door that shuts by itself no way out**
  (E1M9: a nukage room behind a door that walk-over lines outside open for a few seconds, the only
  way on a lift switch inside; she waded in for a look, turned straight back at the doorway every
  time, the switch never seen; once the door shut, too far a wade back for her health): stepped
  onto a hurting floor on purpose for a risky objective there, getting off isn't urgent while what
  she'd have left after paying the way back again is over the margin (it's one of the risky
  options, by distance); and safe floor reached only through a door that closes by itself with
  nothing on her side to open it doesn't count as a way out (`shut_in`, `in_pit`). E1M9
  ITYTD/HNTR/HMP seed 1 exit.
- Traced, not fixed: E2M3 UV (out of ammo by 535 s, then 800 s unarmed). `blocking()` ("a monster
  near and in front: walking on would walk into it") judges *in front* by her facing, and she faces
  what she aims at: anything she shoots within 320 is "in her way". The outmatched node's ammo hold
  flipped every tic and ~30 pistol rounds went into cacodemons she couldn't kill. Punching lost
  souls unarmed (a claw that can't kill her) was tried: the mechanic works, the runs went either way.

Results (HNTR, one life): E1 8.0, E2 7.0 (4 -> 5/8), E3 8.2 -> 7.0 (2 -> 0/8), Doom II 28 -> 27/32;
arenas 119/128, 226/240. E3's losses traced to failures the new runs walked into, none of the
rules: E3M6 seed 6 shut in past a WR door-close line (the open E3M6 stucks), E3M4 seed 1 under a
stopped crusher started again by a walk-over line one sector before its edge (`set_off_first`
knows only lines on its own edge), E3M8 seed 7 (the Cyberdemon). Doom II MAP12 seed 0: berserk
fists at a cacodemon, as before, this time killed.
- **What's in her way is the way the route walks her, for keeping ammo** (E2M3 UV, found by round
  A's helper): the outmatched node keeps its ammo for a target it can't kill unless that target is
  in her way. "In her way" was `blocking()`, "near and in front" by her facing; she faces what she
  aims at, so anything she aimed at within 320 was in her way, and the hold flipped tic by tic (~30
  pistol rounds into cacodemons). Now `in_her_way` judges it by the way the route last walked her
  (the route's controls, kept while the fight holds the route). `blocking()` keeps its facing
  sense for `standing` (the route waits while she fights what's close in front): changed there
  too (v41), the route walked on through close fights, E2 7.0 -> 7.9 but E3 7.0 -> 5.6 and Doom II
  27 -> 25 (her own rocket pressed walking on, MAP10/MAP12). Ammo hold only: E3 7.0 -> 7.1 (1/8),
  doom2 arena 226 -> 227, the rest the same.
- **A stopped crusher set off from a line before its edge** (E3M4 seed 1 in round A's regression
  set: a slow crusher stopped low by a crusher-stop line, its start lines a ring 8 units out from
  its edge; walking in, she crossed one and was under it as it came down, pinned, crushed 36
  times): `set_off_first` knew only a start line on the crusher's own edge. Now it takes the last
  line on the way in, up to and over the edge, that sets it off: the timing runs from her middle
  crossing that line, and she's set off from it at a crawl, backing off only as far as all of her
  is out of the crusher (less by how far out the line is). E3 7.1 -> 7.5 (E3M6 seed 6 through), the
  rest the same.

## 20-minute pass, round B: box-aware crusher escape, teleport lines kept out of fight moves, ammo worth being cut off for (2026-11-09)

Round B: E2M4, E2M5, E2M7, E2M8.

- **Out from under a crusher, all of her, round what's in the way** (E2M4 UV: dodging a baronball
  she set off a fast crusher's walk-over ring, 4 units of her box under its edge; the fight's way
  out took the first heading whose step 8 out had her middle outside, east along its edge, into a
  pillar's corner, pinned there till crushed): `way_out_of_crusher` needs room for her whole box at
  each step (any corner in a wall: not that way), and a heading is out only once all of her box is
  clear of a crusher at work (`box_in_crusher`, shared with `in_crusher`). E2M4 UV seeds 0-2 exit.
- **No fight move over a teleport line from its front** (E2M7 HMP: backing off from a demon
  towards a rocket launcher, over a WR teleporter's front, into a closet with two demons in front
  of its only way out, bitten to death): `safe`/`slide_ok` run the slide for walls, drops and
  hurting floors; now also for a player teleport line crossed from its front (P_CrossSpecialLine
  acts from side 0 only). Taking a teleporter on purpose is the route's. HMP seed 0 160 -> 175 s.
- **Short of ammo, the ammo is worth being cut off for** (E2M5 UV: out of ammo, three clips and a
  shell offered, every one past doors she knew no way back through, so cut off by the reach check
  that only lets the exit, a teleporter or a key cost that much; she explored a dead end instead,
  the baron followed, cornered): an urgent ammo or gun item the level placed counts as worth it
  while she's short (`Hooks::worth_cut_off`). UV seed 0 234 -> 378 s.
- Dropped after the regression set: E2M8's "a mover she waits at that won't move for her" (every
  known line that moves it a used one-shot or out of reach on the live heights: replan or fail the
  step): it judged ordinary lifts she was about to call unmovable; E1M7 stuck 7/8, E1 8.0 -> 6.2,
  Doom II MAP05 stuck x3.
- Leads noted: `ammo_short_for` compares her ammo with a monster's full health (the cyberdemon's
  4000: E2M8 UV held 50 rockets "short" all fight); `for_ammo`'s fallback "away" spot can be nearer
  the monsters; the `to_errand` node spends the last rounds on an outmatched target.

Results (HNTR, one life): E1 8.0, E2 7.0 (5 -> 6/8), E3 7.5 -> 6.6 (E3M6 seed 4 stuck: a secret
door's wall try interrupted, fixed next), Doom II 27/32; arenas 119/128, 227/240.
- **A wall's try counts once it's pressed** (E3M6, round B's regression set, twice: shut in by a
  closing door, the way out a secret door she could open from her side; its try began as she
  stepped over a line that lowers a floor, the look round that set off took the press's time, and
  the wall, marked tried on arrival, was never offered again: stuck the hour): a wall is tried when
  pressed, or when its time runs out unpressed; a look round before the press starts the try again
  after (not after a press: a second press shuts some doors again).
- **A rocket's way held through its launch after the fight's over too** (E3M9: the last rocket
  pressed at a baron as it died; the fight over, the route turned her for a door through the 8
  tics before launch, the rocket out into the wall by her, dead at 53): with nothing left to fight
  and a rocket still to go out, the fight keeps the aim (its way out if one's needed, else the way
  she faced at the press), the legs the route's.
- **A homing missile going round her is shaken off round a corner** (MAP10: four revenant tracers
  bunched into one, dodged till the hour ran out): a homing missile (the revenant's tracer) still
  dodged 3 s on, with nothing else in sight after her, steers the dodge (among the ways clear of
  it) towards the nearest place it can't see her: straight off if there's one within 384, else
  round corners by the mesh (`nearest_heading`, from the crusher escape's search). Overriding the
  dodge instead ran her into the bunch (195 at once). In a fight (things in sight after her) it's
  dodged as before: running off round a corner walked her into an arachnotron's plasma.

Results (HNTR, one life), with round B: E1 8.0, E2 7.0 -> 7.2 (6/8), E3 6.6 -> 7.4 (1/8), Doom II 27
-> 25/32 (MAP10 seeds 0 and 2 killed by barons at the exit line; seed 0 was the tracers' hour);
arenas 119/128, 227 -> 226/240.
- **What she's put into a monster counts against keeping her ammo** (E2M8 UV, found by round B's
  helper: fifty rockets held "short" of the cyberdemon's 4000 all the fight, killed with it at
  half): the outmatched node's ammo hold compared all her ammo with the monster's full health.
  Now `ammo_short_against` takes off what she reckons she's put into it: the rounds gone from the
  gun up while it was her target, at what such a shot does landing at that range (never its
  health). E2M8 UV seeds 0-3: 1/4 exits either way (seed 0 now, seed 2 before), each living as
  long or longer (328/162/410/265 -> 449/162/571/329 s). Regression set level: only cyberdemon
  arena fights move, both ways (doom2 arena 226 -> 225).

## 20-minute pass, round C: a sleeping pack not woken on an errand; the way to a gun out of reach weighed (2026-11-09)

Round C: E2M9, E3M3, E3M4, E3M5.

- **A shot that would wake a sleeping pack isn't fired on an errand either** (E3M5 UV: out of ammo
  and off for a clip, through a room of seven sleeping demons she'd sneaked past five times; on
  the errand `to_errand` came before `sneaking`, "shooting on the way to Clip" at a demon, her last
  bullets woke all seven, bitten to death with the fist): `sneaking` now comes before `to_errand`
  in the fight tree: no shot that would wake a pack worth too much, errand or not; the route keeps
  the legs. UV seed 0 422 -> 1040 s.
- **A frontier into a better gun's sector, out of reach, weighed as a switch is** (E3M4 HMP: a
  chaingun and a dropped shotgun seen on ground with no known way on; the frontier that was the
  way to both (and the exit) risky for its monsters, the risky pass by travel alone took two
  cacodemons over a hurting floor with the pistol): with a better gun seen and out of reach, a
  frontier whose unseen polys are in its sector counts a quarter as far (`SWITCH_FOR_GUN`, as for
  switches). E3M4 HMP seeds 1 and 2 exit (1/10 -> 3/10 over HMP/UV seeds 0-3 and the two lower).
- Dropped after the regression set: E3M4's "no look round again for a walk-over line set off
  before" (two crusher-stop lines across a stair crossed back and forth in a fight, a look at each):
  E2 7.2 -> 7.8 and E3 7.4 -> 7.9, but Doom II 25 -> 23 (MAP12 seed 2 stuck: line 511, a W1 floor
  raise whose effect a later crossing would have shown, never learned, never planned with; MAP12
  seed 3, MAP10 seed 1). What's wanted: the look skipped only for lines whose effect can't be seen
  from where she is (to do).
- Traced, not fixed: E3M3 UV (the start crowd eats 50 bullets; the shotgun behind a switch with
  imps near it is `dangerous` before it's weighed; saving the pistol for the switch is the idea);
  E2M9 (four barons, damage spread over three, none killed; focus fire lost ITYTD exits because the
  back-off follows the target, stepping from a far wounded baron into a near one's claws: back off
  from the nearest clawer instead; ammo behind a baron dropped by `clear_way_to` while a Medikit is
  urgent, the explorer and `for_ammo` pulling her back and forth).

Results (HNTR, one life): E1 8.0, E2 7.2 (6/8), E3 7.4 -> 7.6 (1/8), Doom II 25/32; arenas 119/128,
225/240.

## 20-minute pass, round D: a lift's floor is no crusher; a better gun by the boss; a far ammo errand; no standing on a hurting floor (2026-11-09)

Round D: E3M6, E3M7, E3M8, E3M9.

- **Sectors only lifts move aren't crushers when their floor comes up** (E3M9 UV seed 0: a
  thin lift-as-door sector, floor 64 to 0 and back, by the switch she was pressing; her box's
  edge over it as it rose made her "under a crusher", `off_crusher` gave the route the legs
  (waiting at the lift) over the dodge, a cyberdemon rocket due in 21 tics hit, 136 at once): a
  plat that doesn't crush reverses when blocked (vanilla T_PlatRaise), and no lift crushes, so
  like doors' ceilings, a lift-only sector's rising floor isn't `crushing`. E3M9 seed 0: UV
  died 357 s -> exits, HNTR died -> exits, HMP lives 176 -> 405 s, ITYTD exits as before; UV
  seeds 1-3 unchanged.
- **A better gun judged against the boss she knows is alive** (E3M8 ITYTD: the plasma rifle 246 off
  not urgent, the launcher "better" by its blast, 252 against 205 a second; the spider mastermind
  takes no blast (P_RadiusAttack skips it); the rifle then ruled out as dangerous, rockets spent on
  cacodemons, kept clear of the spider with the pistol, shot down): `better_gun` compares guns by
  damage a second on a boss she knows is still alive (`firepower_on`/`dps_on`: a rocket on a boss
  by its hit alone), so the gun better against it is "a better gun", urgent, past the danger
  filter. E3M8 ITYTD seed 2 exits (the first E3M8 exit); seed 0 has the spider down to 120.
- **No standing still on a floor that hurts** (E3M6 HMP: on the exit route over nukage at 28
  health, ten lost souls 1100-1400 off, `standing` (low health) held the route "closing in", dead
  on the floor): on a hurting floor `standing` passes unless the target's in her way; the route
  goes on, shooting (as `stepping_for_shot`).
- **A far ammo item isn't the fight's errand** (E3M7 HMP: four cacodemons, every rocket "not safe";
  the urgent RocketBox 3938 off made the fight's errand, backed off "towards" it into them): mid-fight
  an urgent ammo item is the errand only within `ITEM_RANGE` (as the explorer's own turn-aside for
  ammo; health already 512). Out of ammo, the for-ammo retreat still routes her to it. HMP seed 0
  exits again.
- Dropped after the regression set: E3M6's rocket check with her own run projected over the whole
  flight (E3M6 HMP: the route running her at a cacodemon, a rocket fired 621 off burst 107 from her).
  Over the flight for every shot, the arch-vile arena lost 2 (arch-viles 1063 off "not safe");
  narrowed to the route having her legs, Doom II MAP10 seed 3 died holding 7 rockets "not safe" on
  the way. What's wanted instead: when a rocket's held for her own run, the fight stops her for it.

Results (HNTR, one life): E1 8.0, E2 7.2 -> 6.9 (seeds both ways: E2M4 seed 5's stuck and an E2M8
death gone, E2M2 seed 0 (the crusher corridor) and two E2M8 deaths new), E3 7.6 (1/8), Doom II
25/32; arenas 119/128, 225/240.

## 20-minute pass, round E (Doom II): no saw rush past shooters; cover through doors and in time; an errand's way clear of them all (2026-11-09)

Round E: MAP01, MAP03, MAP04, MAP05.

- **No closing in with the saw or the berserk fist with a shooter about** (MAP01 UV: the chainsaw
  picked against an imp of five, 84 health; the melee node walked her in and stood her at it while
  the other four fireballed and clawed her to death, 0 rounds fired the level): `best_gun` skips the
  up-close weapons when she'd have to close in (the target past the melee node's "at it" distance),
  another engaged monster attacks from range, and she has a gun with ammo. One already at her still
  gets the saw. MAP01 UV seeds 0-7: 7/8 -> 8/8.
- **Cover in time, through doors, not held off by another retreat's failure** (MAP04 UV: through a
  door that shut behind her into a yard of hitscanners on a better-gun errand; every cover spot back
  behind the shut door, routed with no doors, all failed silently; cover only due under 40 health;
  a failed try at another retreat at full health held every retreat off 105 tics): cover is routed
  through ways open now, then through doors she can open (as the ammo retreat); due also when, at
  the rate she's losing health, she'd be under the retreat line by the time she'd run the cover
  range (`cover_due`); a failed different retreat doesn't hold off cover. MAP04 UV still dies
  (later); the killer is the urgent better-gun errand past `dangerous` (to do).
- **An errand for ammo or a gun is clear only of everything by the way together** (MAP05 UV: out of
  ammo, four shells in a yard with three awake imps; each imp alone "killable" with four shells,
  the super shotgun's blasts counted at 200 each against 60-health imps): as if she had the item,
  what she'd have must kill all of them awake by the way, each shot counted no further than the
  toughest one's health (`ammo_short_for_each`, `clear_way_armed`; only this check: in
  `ammo_short_of` everywhere it cost MAP05 HMP its exit). MAP05 UV seed 0 31 -> 52 s.
- Dropped after the regression set: MAP03's "the fight's errand is the retreat's way while backing
  out", "a clawer touching her when she's pinned gets shot" and "no backing off towards an errand
  nearer the clawer" (HMP 4/6 -> 6/6 but HNTR seed 2 stuck in the regression set: a one-way drop
  into a pit whose way out is a floor she'd raised, and UV 3/6 -> 2/6, the errand taking her back
  into the closet trap at low health). What's wanted: no going back on an errand into the room just
  backed out of; no one-way drop where the only way out is a mover not as it needs to be.
- Seen, to trace: E2M4's "nothing left to do" stucks (regression seeds, and HNTR seed 0 on its own)
  with unseen polys past a door, one-way drops and a teleporter she's used.

Results (HNTR, one life): E1 8.0, E2 6.9 -> 6.1 (E2M4 seed 3 stuck, two cyberdemon deaths on E2M8),
E3 7.6 (1 -> 4/8 through), Doom II 25 -> 26/32 (MAP12 seed 0); arenas 119/128, 225/240.

## 20-minute pass, round F: floor damage through armor; out of a hitscanner's sight round a corner; rockets judged from rocket range (2026-11-09)

Round F: E2M4's "nothing left to do" stucks, MAP06, MAP07, MAP08.

- **A hurting floor's damage goes through armor** (E2M4 HNTR, stuck the hour, and the regression
  set's E2M4 stucks: the way on a secret door pressed from a small lava room; with blue armor, her
  last visit there 10 and 10 armor a hit, the route's cost counted 69.5 of health against 80, under
  the 25 margin, every way there refused; "nothing left to do"): vanilla `P_DamageMobj` takes a
  floor's damage through armor like any hit, so the route's floor cost is the raw damage less what
  her armor takes, while it lasts (`Observation::through_armor`, shared with the crusher stroke's
  cost). E2M4 HNTR seeds 0 and 3 stuck -> exit; none lost.
- **Outmatched by a hitscanner she's keeping her ammo from, round a corner out of its sight**
  (MAP06 UV: the spider mastermind 500-700 off; "keeping clear (ammo kept)" steered her back and
  forth in its sight for 20 s, not firing; no straight-off spot to duck to; shot down): distance does
  nothing against hitscan, so with fire held at a hitscanner that outmatches her she goes the mesh's
  way to the nearest place it can't see (`hidden_way`, the tracers' search). MAP06 UV lives longer
  on seed 0, shorter on seed 1; no exit either way.
- **A rocket judged from where she'd fire it, and a pack past its blast no bar** (MAP08 HNTR: the
  launcher's demon pack; `best_gun` dropped the launcher whenever a rocket wasn't safe from where she
  stood, so the fist was her gun and she closed in, 11 rockets unfired; and `rocket_safe` refused a
  rocket for any monster near its path, twelve demons 950 off "not safe"): a rocket judged from
  further out than she stands, past its blast, isn't ruled out by the target's being close now (she
  backs off to it); a monster in its path bars it only within blast range of her (`rocket_clear`).
  MAP08 HNTR seeds 0-3: every one past the ambush, seed 2 exits (died at 52-74 s before).
- Traced, not fixed: MAP07 UV (the super shotgun against nine arachnotrons; the plasma rifle and
  launcher behind platform switches whose routes the fight held a minute at full health; re-choosing
  an "out" retreat's spot and dodging towards its way cost HMP its exit).

Results (HNTR, one life): E1 8.0, E2 6.1 -> 7.9 (3 -> 7/8), E3 7.6 -> 7.9 (3/8), Doom II 26 -> 28/32
(MAP08 seed 2 exits); arenas 119/128, 225 -> 223/240 (two draws: indoor cacodemons, the spider
mastermind's cover).

## 20-minute pass, round G: a target with no shot at it given up; no margin at a dead end; kill first only what she'd kill soon (2026-11-09)

Round G: MAP09, MAP10, MAP11, MAP12.

- **No standing at a target she's had no shot at for 20 s** (MAP09, every skill stuck the hour: an
  island in a nukage pit, two zombiemen on ledges around it, "no shot (too steep)" at either and
  them never hitting her; `standing` and "lost sight: keeping at it" held the route, switching
  between them, every step given up as taking too long, the dead end's fallback never reached):
  with no shot at a target for `CLOSE_GIVE_UP`, it's given up for `GIVEN_UP_TICS` as an
  out-of-reach or melee one is (a shot resets the clock, so does a gap over 20 s; one in her way
  is exempt); `standing` passes to shooting on the way (`no_shot_long`). Narrowed to "not with a
  fist up, being hurt resets it" it lost the MAP09 exit: kept as is.
- **No health margin at a dead end** (MAP09: with the route free, the dead end's lift switches,
  each a wade costing 15 at 37 health, refused by the 25 margin, which waits for healing or a
  radiation suit she can't reach): with `harm_ok` (a dead end) and a line to set off, only a way
  that would take all her health is refused. MAP09 ITYTD seeds 0-2 stuck -> exit; HNTR and HMP
  stuck -> die (at the switch on the floor, 25 health: wall searches in the nukage spent her health
  before the switches; to do).
- **Kill first only what she'd kill soon** (MAP11 UV: the pistol at a pain elemental, 400 health,
  some 16 s of pistol, its 2.5 priority; three shotgun guys by it shot her dead untargeted): the
  arch-vile's and pain elemental's kill-first boost holds only if she'd kill it within 8 s
  (`KILL_FIRST`), and drops only with another hitscanner in sight. MAP11 UV seed 0 8 -> 20 s (the
  shotgun guys killed, the shotgun picked up).
- Dropped after the regression set: MAP12's berserk fist or saw passed over for a target she
  wouldn't close in on (HMP seed 0 exits) cost Doom II MAP10 seed 0 (the barons at the exit).
- Traced, not fixed: MAP10 HMP (a caged cyberdemon behind a window by the only gap in a fence; the
  objective resumed back to the window after it hit her: `dangerous()` is checked only at the
  choice; ending it near an unbeatable monster cost HMP seed 1's exit); MAP12's line 511 (a W1
  floor raise between two slime sectors, the way on, never crossed: walls tried, the slime room not
  walked; regression seeds stuck on it).

Results (HNTR, one life): E1 8.0, E2 7.9 (7/8), E3 7.9 -> 8.0 (3 -> 4/8), Doom II 28 -> 27/32 (MAP12
seed 1 stuck on line 511); arenas 119/128, 223/240.

## 20-minute pass, round H: seams walked over; two crushers side by side; a stalled melee given up; a hurting floor's replan at once (2026-11-09)

Round H: MAP12's walk-over line, MAP13, MAP14, MAP15.

- **Lines between look-alike sectors she's stood on both sides of, never walked over** (MAP12 HNTR
  seed 1 stuck: line 511, a W1 floor raise between two slime sectors, each entered across other
  lines, so no threshold over it was ever offered; "try the wall" stood her 40 units short):
  she keeps the lines her middle has gone over (`crossed`, cleared on a restart) and each
  sector's heights when first seen (`first_heights`); with nothing left before the things given
  up on, she walks over each line between two reachable polys she has stood in whose sectors
  looked the same when first seen (heights, flats, light: a split only a trigger or a mover
  needs), 32 units into the side away from her (`seams`). MAP12 HNTR seed 1 stuck -> exit (937 s);
  seeds 0, 2, 3 and fight 1 seed 0 exit as before, fight 3 seed 0 dies as before (567 s).
- **A crusher the way goes into straight out of another is judged before the first is entered,
  and from under any of her** (MAP13 UV: a fast crusher, a slow one beyond it with the plasma
  rifle in it; under the fast one, the follower's "under one already" branch never looked ahead, so
  she walked on into the slow one on its way down, pinned under its edge; and the escape took only
  the first ceiling coming down over her box, through the fast one in time, the slow one over her
  edge ignored). Now (`hazards.rs`): the walking rule's go for a crusher also needs the one straight
  after it open now (`open_now`); under one (any of her), the next crusher out of where her middle
  is not open now (and no crossing of it begun) is out from under all of them (`escape_crusher`),
  not on into it nor a wait at its edge under one (E4M7's); and every ceiling coming down on her is
  weighed for the escape, not the first found.

Results, MAP13 seeds 0-3 (ITYTD seed 0): ITYTD exits (1547 -> 1209 s); crusher deaths 5 -> 2 of 12
(UV s0, HMP s1, s2 gone; HNTR s2 is the fight falling back to cover from inside the slow crusher
with the rifle in hand, HMP s3 out of ammo, the route's crusher_guard not the walking phase:
traced to there, not fixed). No exit anywhere on HNTR-UV either way.
- (The seams, after the regression set: a seam is tried once its trip is over, gone over or not;
  a spot past a line can be got to round it, and E1M5 walked two of them in turn for the hour, E1
  8.0 -> 7.0.)
- **A melee that gets no nearer is given up, and a ledge stays a ledge** (MAP14 HMP: out of ammo,
  berserk; a chaingunner on a ledge 300 off she can't climb to, walked at with the fist 16 s till
  dead; the melee node's give-up is 20 s without the target's health dropping, and `armed()` counted
  the berserk fist with everything after her punchable, so the ammo retreat never began): closing
  in that gains under 32 units in 3 s gives it up (`MELEE_GAIN`/`MELEE_STALL`) and remembers its
  sector: while it's still there and she isn't, it's out of her fist's reach (`out_of_fist_reach`),
  and the berserk fist counts as arming her only if nothing after her is. MAP14 HMP seed 3 exits;
  seeds 0-2 live longer.
- **Stuck on a floor that hurts, a new path at once** (MAP15 HNTR seeds 2 and 3: a walk-over line
  lowered the ledge she was on into lava; the walk, planned from up there, ran into a step 32 over
  the lava, and the follower backed off twice before a new path, 3 s at 20 a second): on a hurting
  floor no backing off first.
- Dropped after the regression set: MAP15's rocket hold through the launch tic, turned back to the
  press's facing only if still clear (no self-rocket damage on MAP15): the combat arena 119 -> 117
  (arch-vile and revenant fights).

Results (HNTR, one life): E1 8.0, E2 7.9 -> 7.6 (5/8), E3 8.0 -> 7.2 (4 -> 0/8; no one change: each
taken out alone gives 7.2-7.4, the spider mastermind's fight by seed), Doom II 27/32 (MAP12 seed 1
exits, MAP10 seed 0 dies at the exit); arenas 119/128, 223/240.

## 20-minute pass, round I: a retreat keeps its legs over a dodge that clears nothing; the fist not up for what it can't reach; a way past the monster already at her (2026-11-09)

Round I: MAP16, MAP17, MAP18, MAP19.

- **A retreat's legs kept from a dodge that clears nothing; the dodge leans its way** (MAP16 HNTR:
  the blue skull on a small raised platform, its trap's imps; "backing out the way she came", yet
  per-tic positions within 20 of the key from the retreat's start till dead, 197 from fireballs:
  on so small a ledge `safe()` turns down almost every way, the dodge had none or flipped two
  opposite ways tic by tic, standing her still, and its pull was the errand's goal, the key under
  her): retreating, a dodge takes the legs only with a way clear of every missile coming
  (`dodge_clear`: its best score >= 0); falling back, it leans towards the retreat route's next
  point (`RETREAT_AHEAD` on), not the errand. MAP16 ITYTD and HNTR seeds 0-3: 0/8 -> 8/8 exit.
- **The fist not up for what it can't reach while a gun has a shot** (MAP17 HNTR, stuck the hour:
  berserk, a pain elemental on a ledge 80 up; the melee given up, `best_gun` kept the fist, free and
  fastest up close; the tree flipped between stepping for a shot and closing in 16,000 times each,
  149 bullets and 50 shells never fired): `best_gun` passes the fist over for a target out of her
  fist's reach (given up on, or where a stalled melee left it) while any gun has a shot in it.
  MAP17 HNTR seed 2 exits, the others much further; ITYTD seed 2 stuck instead (a revenant 1200
  off closed in on for the hour: a shot but no reach, nothing gives it up; to do).
- **A way off for ammo isn't walled off by the monster already at her** (MAP19 ITYTD: the pistol
  empty, two spectres at 47 and 55; `for_ammo`'s way, passing no nearer than she was, was called off
  the next tic by `walled_off`, which counted any monster within touch of the way, the one beside
  her too; every retry the same, bitten to death on the spot): a monster walls off her way only if
  the way passes within touch of it and nearer it than she is now (less `AMMO_MARGIN`), the test
  `for_ammo` picks by. And (MAP18) a for-ammo way called off as walled off counts as that try: the
  cover branch goes to cover for `RETREAT_AGAIN` (it flipped ammo, over, ammo, tic by tic, under
  fireballs).
- Seen, to do: a lip over a demon pit (MAP19: her box on the ledge, her middle over the pit, bitten
  from below with no shot, `blocking()`'s facing sense exempting it from the no-shot give-up);
  MAP16 HMP/UV's arch-vile at 30-39 s; MAP18's opening imps (the far nearly-dead one targeted while
  two claw her).

Results (HNTR, one life): E1 8.0, E2 7.6 -> 7.8 (6/8), E3 7.2 -> 8.0 (1/8), Doom II 27/32; arenas
119 -> 118/128, 223 -> 222/240 (spider and cyberdemon fights both ways).

## 20-minute pass, round J: a barrel chain judged whole; a dodge clear of the hit; the fist's true reach; no knife-edge squeezes (2026-11-09)

Round J: MAP20, MAP21, MAP22, MAP23.

- **Barrels judged by the whole chain, and the chain outrun** (MAP23, every skill dead at 4-5 s:
  the start hall one chain of barrels, a mancubus behind a row): a barrel by a monster is shot
  only if its whole chain goes off clear of her (`barrel_shot` checked only the barrel's own 224);
  a way away from barrels mustn't bring her nearer any barrel near where it ends (it took her from
  one row into the next), else that node steps aside; a chain going off is fled from the barrel
  that goes off first (fewest blasts from a lit one, then nearest: it burns a barrel a blast); and
  keeping clear of what outmatches her, while the route walks her away from it (within 90
  degrees) the route keeps the legs. MAP23 start hall survived (5 -> 9-29 s); no exit yet.
- **A dodge clear of each missile's hit, not its blast** (MAP20: the cyberdemon's rockets down a
  corridor 128 wide, a blast needing 151; no dodge "clear", so the retreat's route had the legs and
  stood her at a shut door 12 off a rocket's line, 100 at full health): a dodge counts as clear when
  it clears every missile's own hit (`hit_clearance`: the blast and up to 160 more); no duck whose
  way crosses a missile's hit (the duck from the spider ran her across a rocket's line); no cover
  in a sector that's shut now (hidden only behind the shut door, in the doorway once there).
- **The fist's and saw's reach is 64, middle to middle** (MAP22 HNTR: berserk, punching a
  chaingunner from 68-77 off, not one landing, shot 16 times): the punch's trace runs `MELEERANGE`
  (64) from her middle, and a monster's hit where it crosses the diagonal through its middle
  (`PIT_AddThingIntercepts`), so its radius adds nothing; reach 64 middle to middle in `in_reach`,
  the melee node and the arsenal, the stop 12 short (`MELEE_SLACK` 24 -> 12). MAP22 HNTR seed 0 and
  HMP seed 2 exit, HNTR seed 2 dies (net +1).
- **No squeeze past shut sectors** (MAP21: hundreds of "stuck" at a gap exactly the box's 32 wide,
  between a wall and a bar's end or two bars' corners 24 apart both ways; the engine lets her
  through only at the exact unit moving exactly along it, which the follower never hits):
  `fits_truly_near` (a smaller box fitting within 8 of each squeeze point, ad0e9fe) is gone, a
  squeeze past shut sectors she can't open is no way, and an objective's stand spot must fit the
  box with half a unit of play (`with_play`). MAP21 no monsters seed 0 stuck -> exits (61/61
  sectors); with monsters, deaths in the gap become stucks further on (a run-jump way not found).

Results (HNTR, one life): E1 8.0, E2 7.8 -> 7.5 (cyberdemon deaths on E2M8), E3 8.0 (1 -> 3/8),
Doom II 27 -> 29/32 (MAP08 and MAP12 seed 2); arenas 118 -> 119/128, 222 -> 223/240.

## 20-minute pass, round K: a quick hitscanner fought where it stands when she's outmatched (2026-11-09)

Round K: MAP24, MAP25, MAP26, MAP27.

- **Outmatched, a hitscanner that dies quickly is fought, not kept clear of** (MAP27 HMP: an ambush
  room, the door shut behind her, walls dropping on zombiemen, sergeants and chaingunners with
  demons behind; outmatched by the crowd, each former human "kept clear of" in its sight or "got
  past" deeper in, 100 -> 10 in 6 s from 40-110 off): hitscan doesn't fall off with range, and a
  non-sniper hitscanner dies sooner than she'd get out of its sight; outmatched, such a target
  (`quick_hitscan`) is fought where it stands (no keeping clear or getting past it, `standing`
  treats it as a duel). MAP27 HMP seed 2 138 -> 996 s, UV seed 3 107 -> 750 s; no exit either way.
  Regression set level.
- Dropped after the regression set: MAP24's "along her sides only what stops her box blocks a
  way" (`clear_run`'s side lines ignoring drops: the dodge on a walkway's edge had no way at all,
  four fireballs hit; MAP24 HNTR seed 0 exits): E1 8.0 -> 7.2 (E1M3, a dodge mid-run-jump into a
  nukage pit) and E2 7.5 -> 7.0.
- Traced, not fixed: MAP25 (backing out over a hurting floor at a crawl: `Motion::go` cuts the
  route's speed to 15-45% when its way is over 90 degrees from her facing, meant for the route
  turning to it; with the fight holding her aim the turn never comes and `combat.controls` keeps
  the cut, 2.4 a tic over a blood pit; Doom backpedals at full speed. And `Fitter::detour` skips the
  walls her box touches at a segment's end without the crossing check `straighten` has: a detour
  through the wall she's pressed to); MAP26 UV (ammo gone on the start's chaingunners; a dropped
  chaingun ruled dangerous; `for_ammo` routes on the heights now, blind to lifts; `dangerous()`
  counts monsters not known awake).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.5, E3 8.0, Doom II 29/32; arenas 119/128, 223/240.

## The Icon of Sin's brain may be known (the user, 2026-11-09)

The boss brain (MAP30) is hidden behind its wall, but its wake-up sound plays to the whole level:
she may treat it as known (the user's call). The way to it, traced in round L: `A_BrainDie` ends the
level on any map; the brain has 250 health behind a slot 32 tall; a rocket (spawned at her z + 32,
flying level, going off on any floor it meets) gets in only when fired with her z inside the slot's
gap less 32, a window of about 6 tics on the lift's climb; 108 a hit, three rides. To build: the
brain as an exit objective whose activators are floors (a mover's range among them) from which a
rocket spawns inside the opening on the line to it, and a node that fires in the window; rockets
kept for it.

## 20-minute pass, round L: no fist or saw brought up within two claws of death (2026-11-09)

Round L: MAP28, MAP29, MAP30, MAP32.

- **Bringing up the fist or saw in a claw's reach weighs two claws, not one** (MAP28 HMP: a demon 50
  off at 43 health, its bite up to 40; `best_gun` picked the chainsaw, cheaper than the pistol, the
  one-claw rule letting it; the switch takes some 32 tics, more than a demon's 24-tic bite cycle,
  bitten for 28 mid-switch, then the saw ruled out and `SWITCH_WAIT` holding the pistol back,
  bitten dead): if the fist or saw isn't up yet and the target's within claw reach, two claws are
  weighed against her health. MAP28 HMP seed 2 19 -> 53 s. Regression set level.
- Dropped after the regression set: MAP32's aim held where she faced at the press till the shot
  goes out, for every gun (the pistol's bullet 4 tics after the press, the shotguns' 3; a fight
  flapping on and off swung her 10-20 degrees through each; MAP32 exits 6/16 -> 7/16 seeds): E3 8.0
  -> 6.6.
- Traced, not built: MAP30 (the brain: see the entry above, the user's call that she may know of
  it); MAP29 (a run-jump landed 50 degrees off its line into lava with the fight shooting through
  the sprint, unproven; a one-way drop into lava for a wall look; an SR lift line reachable from the
  lava never tried).

Results (HNTR, one life): unchanged: E1 8.0, E2 7.5, E3 8.0, Doom II 29/32; arenas 119/128, 223/240.

## 20-minute pass, round M (episode 4): a floor priced by what one like it has done; no chasing what's out of sight over a hurting floor (2026-11-09)

Round M: E4M1, E4M2, E4M3, E4M4.

- **A floor that looks like one that hurt her is priced at what that one did** (E4M1 HMP, stuck the
  hour: the red door pressed from a nukage trench the map gives no damage, never stood on, so
  priced at the worst a floor does, 20 a hit, 26.7 at 37 health against the 25 margin, refused;
  the pit beside it with the same flat had hurt her 5 a hit): a floor she takes to hurt that the
  map gives no damage for is priced at the most a floor with the same flat has done to her
  (`floor_hit`), the worst only if none like it ever has, along a route and at its end. E4M1 HMP
  seed 0 gets through the red door (still stuck past it: a never-hurt BLOOD3 floor priced at 20).
- **No chasing what's gone out of sight while on a floor that hurts** (E4M2 HNTR: a lava hall under
  a suit, a demon with no shot at it (too steep) seen and lost by turns, "keeping at it" after it
  for 1500 tics till the suit ran out, 125 of lava; E4M3 HMP the same over a cacodemon): on a
  floor that hurts by the map (a suit's time runs out), `lost_sight` doesn't take the controls;
  the route's way off comes first. E4M2 ITYTD seeds 0-7 exits 4 -> 5, HNTR 1 -> 3.
- Dropped after the regression set: E4M3's `standing` on a hurting floor judged by the route's way
  rather than her facing (E3 8.0 -> 7.0 with it, chaotically: taking out either it or the
  `lost_sight` guard gives 8.0); E4M4's re-path at the act phase when a fight has pulled her off
  the spot (pressed from 1100 off, a straight line over lava): the doom2 arena 223 -> 221
  (cyberdemon fights).

Results (HNTR, one life): E1 8.0, E2 7.5, E3 8.0 -> 7.8, Doom II 29/32; arenas 119/128, 223/240.

## 20-minute pass, round N (episode 4): an out-of-reach goal for ammo dropped; no retreat from the route to ammo that keeps clear (2026-11-09)

Round N: E4M6, E4M7, E4M8, E4M9. The last of the 20-minute pass.

- **A goal for ammo she can't reach past them is dropped** (E4M9 UV: out of ammo, the rocket
  launcher behind a door that opened on an imp and a baron; `for_ammo` refused it, no way past
  them, "away from them", then out of sight the fight ended and the explorer took the same goal
  back into them, 17 times in 1200 tics): when `for_ammo` finds no ammo or gun reachable without
  passing what's after her and her goal is one it refused, the goal is dropped
  (`Explorer::give_up`); the next choice is made knowing they're there. E4M9 HNTR seed 2 exits.
- **Unarmed, no retreat from a route to ammo that keeps clear of them** (E4M7 UV: out of ammo, the
  explorer's way to ammo through a door, a drop and a run-jump, which `for_ammo`'s plain way on the
  heights as they are can't see; "away from them" through the door, the route back through it, for
  600 s): with nothing from `for_ammo`, if the objective is ammo or a gun she can use and its path
  keeps `for_ammo`'s clearance, no retreat; the route goes on. Results unchanged (a loop broken).
- Dropped after the regression set: E4M8's backing off on a hurting floor giving the route the legs
  (she outruns any clawer) and `standing` judged by the route's way there (E4M8 HMP 1/4 -> 3/4):
  E3 7.8 -> 6.6 (both parts; the second alone 7.2).
- Traced, not fixed: E4M6 ITYTD (stuck: a route between two closed door "pillars" 24 apart, floor
  at ceiling, a blue door raising them; her knowledge takes never-opened door sectors as passable;
  after the stuck, every objective "no route").

Results (HNTR, one life): unchanged: E1 8.0, E2 7.5 (4/8), E3 7.8 (2/8), Doom II 29/32; arenas
119/128, 223/240.

Over the 20-minute pass (rounds A-N, 54 levels not beaten on UV): from E1 8.0, E2 7.0, E3 8.2, Doom II
28/32, arenas 119/128 and 226/240 to E1 8.0, E2 7.5, E3 7.8, Doom II 29/32, arenas 119/128 and
223/240, with exits gained on many levels' seeds (see each round). Next, as the user set it: larger
changes, then the 40-minute pass; paused here at the user's word.

## The score: every level, HMP and UV, eight random seeds (the user, 2026-11-09)

Progress is measured by a score, not the regression set (the user's call: an episode counts a
late level only when everything before it went right, unfair to later levels; single-seed
surveys swing with one seed; arena fights are too artificial to count, a tool for trying ideas
only). `evil-boom score` plays every level of Doom (E1-E4, 36) and Doom II (32) from a pistol start
on one life (mortal), on HMP and UV, eight random seeds each (1088 runs), a TSV row a run
(`out/score/`). A run scores 1 for getting out, else half the share of the level's monsters killed
(partial credit, the user's measure: how far into the fight she got; the half keeps a run that
kills all and dies below one that gets out); a level's score is its runs' mean; the score is the
mean over levels and skills, each the same weight. Beside it, how consistently levels are beaten
(the user wants that optimized too): the spread (each level's variance of its runs' points,
averaged), and the levels beaten on every run and on none. `score-report A.tsv B.tsv` compares
sets: the score's own noise between measurements, and which levels move most. `run-map`'s RESULT
line now always carries `kills x/y`.

## Stuck round: eight agents on the levels that stall most (2026-11-09)

The first round judged by the score. Eight agents, 30 minutes each, on the levels with most
stalled runs in the baseline (MAP19, E4M1, MAP21, E3M7, MAP06, E4M5, E4M6, E1M9), each measuring
its level with `score` before and after; seven fixes merged.

- **No standing at a target with no shot, in front of her or not; a route waiting to press has
  the turn** (E4M5: on a 16-wide ledge over the exit's pit, a shotgun guy below too steep for
  either to hit, `standing` held 300 s, its no-shot give-up exempted by `blocking()` (facing: she
  faces what she aims at); and falling back to cover behind a shut door, the fight kept her turned
  to an imp 1100 off, the door never pressed for 6000 tics): the give-up applies in front too; while
  a route waits at a door or switch to press it and she hasn't fired this tic, the route turns her
  (`RouteFollower::pressing`, `Player::press_turn`). E4M5 0.52 -> 0.68-0.84, stalls 5 -> 0 of 16.
- **Edges a few units off are the same boundary** (MAP19 HMP: the lift out of the demon pit had no
  mesh portal: a corridor poly's edge, from a seg whose rounded split vertex is extended about
  twice its length, lay 2.1 off the lift's line against a tolerance of 2; the pit a dead end, the
  hour): `shared_segment` within 3 units. 26 of 68 maps gain 1-7 portals, none lose any.
- **A dead end's look into the unseen needs no margin; a wade from one isn't an urgent way back;
  a still mover on a hurting floor replanned at once** (E1M9 HMP: on a dry island in a nukage pit,
  the way on a room past a door the island's edge opens; "look into" it refused by the 25 margin,
  the dead-end pass lifting it only for lines to set off and skipping what was refused earlier the
  same think; waded in, "get off the floor" sent her straight back; a door re-crossed while moving
  (EV_DoDoor ignores it) waited at in the nukage): a frontier look takes margin 0 under `harm_ok`
  and the pass retries same-think refusals; wading in from a dead end (`waded_dead_end`) makes the
  way back a weighed option and counts her shut in (switches from the floor offered); a mover
  standing still with nothing coming, on a hurting floor, is replanned after half a second. The 4
  stuck HMP seeds exit.
- **A floor changed to one that hurts priced by its model; a dead end's other ways may leave her
  one hit** (E3M7 UV: at 18 health the blue key's pickup lowered the floor round its alcove into
  blood (W1 lowerAndChange copies the model sector's special: 10 a hit), priced at the worst 20,
  every way out refused): `changed_floor` keeps the floor's hit (`Navigator::floor_hit_after`); under
  `harm_ok` a way that isn't a line or a look is taken if what she'd have left covers one hit of
  its hardest floor (margin 0 for those killed her in a wade of 40 at 41).
- **Teleported away while at a step's goal: the step walked again** (MAP21: a dodge stepped her onto
  a teleporter beside the yellow bars' press spot; the follower, still in its act phase, pressed
  from 1300 off three times, and the bars were written off): moved unexpectedly while acting or
  fetching, the step starts over as a walk from where she is.
- **Unarmed, no "away" off a route that keeps clear** (MAP06 HMP: unarmed, the blue card's lift
  window missed every 3 s by "away from them" to a far spot, 20 game minutes): with no ammo
  reachable clear of them, no hitscanner after her, not hurt, and the explorer's route keeping 96
  from them all, she stays on it.
- **Walls given up one at a time; dry ones first when she's on dry floor** (E4M6: on a ledge in a
  lava field, the way out a hidden door in the ledge's own wall; the panic round's 12 walls all
  far, across lava; and every wall objective shared one identity, so one dropped "no route" benched
  them all): a wall is identified by its line; in a panic round, mortal, unsuited and on dry floor,
  walls reached without crossing a hurting floor come first.
- Not fixed: E4M1 (past the red door, "no route to the exit" even for the omniscient planner at
  21-29 health; shut in or its pricing, to trace).
- Seen: one MAP22 HMP run thought for 679 s of real time over 758 game seconds (39 tics/s): the
  planner, to trace (the stall cutoff counts game time, so it bounds this only slowly).

Score (all 68 levels, HMP+UV, 8 seeds): 0.428 -> 0.437 (HMP 0.513 -> 0.543, UV 0.343 -> 0.331),
within the score's noise (+-0.006); stalled runs 69 -> 45 of 1088; on the eight levels, stalls 31
-> 17, MAP19 HMP exits 0/8 -> 3/8, E3M7 HMP 1/8 -> 3/8.
- **The dead-end pass takes up again only what had no route** (MAP22 HMP, a run thinking 679 s of
  real time for 758 of game, 39 tics a second: the stuck round's dead-end pass took up again
  everything dropped earlier the same think, so a hunted monster dropped "not there" (she stood on
  its last-seen spot) was taken up again at once, dropped, taken up, every tic, each think routing
  every wall afresh, 140,000 wall routes): only what was given up for want of a route this think
  (the health margin's refusals) comes back (`refused_now`). The seed: 681 s -> 19 s real (it now
  dies at 719 s game instead of stalling). E1M9 HMP's stuck seeds: 2 of 4 exit, 2 stall, the same
  with or without this (an interaction of the round's merged fixes, to trace).
- **No risky way for what little it's worth** (the user's: the health a way costs against what
  it's for): with nothing safe left, a place seen already looked at again, a monster gone from
  where it was last seen, and a sound (already) aren't offered as risky; they keep the tenth-of-
  health cap. Score 0.437 -> 0.448 (HMP 0.543 -> 0.537, UV 0.331 -> 0.358). Traced alongside: E1M9
  HMP seed 1105803694's health went on a revisit estimated at 2.8 that cost 28, the wait for a lift
  standing in the nukage not counted (the estimate counts the time walking on a floor, not
  waiting on it): to do.

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

## Realism: the fight's hurting floors by what she knows; the survey (the user, 2026-11-09)

The fourth merge of step 4 couldn't be made without changing play: the fight took a floor to
hurt by the map's damage values (and a floor seen changed, and one going down into harm), the
rest of her by what she knows (`Knowledge::damaging`: felt, else by its look). Asked, the user
made the call: it's time for realism (CLAUDE.md's rule is now "Combat: realistic now").

- The fight's `hurts` goes by `Knowledge::hazards` (the player hands it over each tic): what she
  takes to hurt, none of it under a radiation suit (the map's values knew nothing of suits: she
  kept off nukage in a fight with one on). A floor going down into harm stays (her own eyes,
  though where it ends is the map's: REALISM.md's M4).
- Score: 0.448 -> 0.445 (HMP 0.537 -> 0.535, UV 0.358 -> 0.355), one measurement each, inside
  the noise (0.008 a set). 6 of the 32 parity runs play differently.
- `docs/REALISM.md`: the survey of every other place she goes by the level's truth (eight by
  the observation, six by the map), what each would be instead, and the order. The user's: after
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
grow into (the user's: it's to be expanded a lot).

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
  its health: she doesn't know it). It's to hold only what she knows: REALISM.md comes first.
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

## The stream console, Doom's own screens, the stream's window (the user, 2026-11-10)

Built on the `console` branch, merged into main; the whole plan, the protocol and what each
stage was tested by: `docs/CONSOLE_PLAN.md`.

- **A supervisor, the game its child.** `evil-boom console` serves a plain HTML/JS page (no
  build step; tiny_http) and starts the game as `watch ... --control ADDR`, a JSON line a
  message over a local TCP link (`console::Command` down, `FromGame` up, `PROTOCOL`). The
  supervisor outlives a crash or a hang (restarted at the level it was on, three in two minutes
  and it stops trying); the game plays on if the console goes. One game a process (DSDA's), so
  a new WAD is a new child; a level of the same WAD is a goto in the running one.
- **On the LAN, by a token** (`config/console.local.json`, not committed, with the settings):
  every API call needs it, from localhost too. Across the web: none of it in the repo; the
  streamer's own tunnel (Cloudflare Access) in front, the page's paths relative so it serves
  under a prefix, the login it vouches for named in the log.
- **Doom's own screens** (`watch --present`, on from the console by default): the title card
  held (no demos: the user's), the menu walked to the game as a player walks it, the melt, a
  death watched, the tally and the finales pressed on. The engine draws them
  (`evil_api.c`'s presentation; `WI_EvilWaiting`, `D_EvilPageDrawer` in the patch); Rust says
  when, a tic at a time (`viewer/present.rs`). A melt holds the game and keeps its random
  numbers apart, but the title's tics are in the seed (`rngseed` takes `gametic`), so a
  presented run isn't the headless run on its seed: the user's call, streams play on random
  seeds (the console's default). Headless play is untouched: run-maps line for line the same.
- **The stream's window** (`watch --stream [right|left]`, the console's default): 1920x1080, the
  game 4:3 at 1440x1080, a 480 column for viewers (the level, her map as she knows it, what
  she's doing in plain words, what Evil said, the tally, the run, what happened lately). The
  status bar's things (health, ammo, keys) aren't repeated: the game shows them.
- **Volume**: DSDA's sfx and music volumes, and a master the patch adds (`evil_master_volume`,
  a gain in `I_UpdateSound`), each a slider; kept in the settings.

## MAP07's exit step raised twice: left as the game has it (the user, 2026-11-10)

The last two arachnotrons dead within one's death animation of the other (49-51 tics apart in
the runs traced), the game's check runs for each and the step round the exit (tag 667, raised
by its lower texture's 16) goes up twice, 8 to 40: 32 above the yard, no climbing it, no exit.
3 of 16 runs on the easiest skill. The user's: "that's intended game design. We leave her
stuck forever. That would actually be hilarious during a stream." So: no rule to space the
last kills, no giving up when she sees it. Not to be "fixed". (Seen with it and not asked to
be left: she waits for the step at 24, the model's height, when she sees it at 40.)

## Barrels are gone round, and run out of; two bosses are set on each other (the user, 2026-11-11)

MAP20 and MAP23 on the easiest skill: 16 runs of 16 dead in under a minute on each.

**Barrels (MAP23).** The mesh has no barrels in it (they can be blown up), so a route by it
ran into a row of them and stood there. Now the follower plans a lane between the barrels she
sees in her track (`motor/follower/lanes.rs`: a search over a 4-unit grid, her box against
each barrel's as the game tests it), to where the track's clear again, and plans again as more
come into sight; the sidestep (`round_barrel`) only where there's no lane. The user's: "it's
required to plan a route around the barrel obstacles". And in the blasts of six or more
chained barrels (`barrels.field`) she runs for the nearest place she knows with no barrel
within 384, at once, something there to set them off or not, and nothing takes her legs or
eyes till she's there (`RetreatKind::OutOfBarrels`; the user's: "find shortest path to a room
without barrels and RUN", "cannot be interrupted by ANYTHING until she gets to safety", "it
should start immediately"). With nothing to set them off, once only from about the same
place (a way that lies through barrels would never be taken). No such place known and
something that shoots awake: on along her route flat out, facing the way she runs, the
trigger let go (`Move::ThroughBarrels`). A chain going off she isn't running from: the
shortest way out of its blasts, or on along the route, never to and fro between two barrels.
Tried and dropped: a timed model of the chain (which barrel goes when): too noisy to steer by.

Found on the way and left: the route's legs go at a sixth of her pace while the fight has her
turned to aim behind her (`Motion::go` slows a run sideways or backwards). Fixed for the
barrel run only (`Move::route_faces`); everywhere else it's as it was, not measured.

**Two bosses (MAP20).** A spider mastermind and a cyberdemon both awake (the only two the
code calls bosses): she goes to the nearest place in a line with the two, one between her and
the other, so what the far one fires at her hits the near one (`Combat::set_on`,
`Move::SettingOn`); neither turned to her, they're left to it and the route goes on
(`Move::LeavingToIt`). No shot at either while both live; one dead and the other turned to
her, the fight as it ever was (the user's, in place of two rules of mine that held the
trigger on anything she couldn't kill in every move and walked her past what outmatched her
after 30 s: those two lost 0.0175 on the score and are gone). A monster's health is reckoned
by what she's seen it lose to anything, not only by what she's put in.

Easiest skill, 16 seeds: MAP23 0.03 -> 0.45 points (3 exits), MAP20 0.07 -> 0.21 (1 exit).
Score: 0.460 -> 0.459, run for run -0.0009 (standard error 0.0049), exits 408 -> 403.

## Episodes on lives; Doom's second episode's stucks (the user, 2026-11-11)

`episode --lives N`: N lives for the whole episode. Dead, the level's begun again with what
she came into it with (weapons, ammo, armour, health, the weapon in her hands: the loadout
has that now, `evil_ready` in the engine API brings it up) and what she'd learned of the level
kept (the user's: "I would like knowledge kept"; what she saw before she died, not a level
looked round beforehand). Episode 1 goes through on every skill; episode 2 stuck in 12 of 32:

- **E2M4, a secret door at the back of a lava alcove**, the only way off a ledge. Its wall,
  her best suspect there, was refused a few points short of the health margin, and walls with
  less to go on across the lava tried first (a dip reckoned a fraction of a hit; it's none or
  a whole one) till it was out of reach for good. Now: down to trying walls, a wall's weighed
  with the margin of nothing else to do; a wall tried over any floor that hurts is left to the
  risky pass, the best suspect first; and the last pass, the margin lifted, has the walls in it.
- **E2M4, the crusher stopped low** by a stop line walked over further on, its start line its
  own edge. Intended, the user's: left. But stopped with her height under it she walks
  through (the user's: "you are supposed to eventually walk into the crusher"); the room to
  spare she wanted was for one coming down.
- **E2M1, a stair built round the corner**: five steps heard, one seen, two neither, taken for
  still on the floor and the stair for no way up. Now a step of a stair learned for a line
  takes the rest of that stair she's seen with it.

Score, all of it: 0.459 -> 0.464, run for run +0.0049 (standard error 0.0033).

The recorder: the game library is never unloaded (its sound thread went on calling into it
after a recorded episode ended: a segfault, the sound never put with the video).

## Directed play: the bot's pick, lists in its order, `doom_look` (the user, 2026-11-11)

The SDK plan's 5.5 and 4.1 (`docs/NEURO_SDK_PLAN.md`), in `control/directed/look.rs`.

- **One opinion, the explorer's.** The pick is what she'd do next on her own: the first of
  the explorer's ranked objectives she has a route to. Nothing new is weighed for it, so it
  can't disagree with what the idle fallback then does, and it's built only on what she
  knows. Considered and left: a ranking of its own for the commander (two opinions to keep in
  step); the exit as a pick whenever it's reachable (the explorer's own first choice, but
  then the pick says "leave" from the moment the exit's seen: leaving stays an order's, as
  on her own, and the exit is the pick only with nothing else left she can get to).
- **Under the id it's ordered by**: something to explore by its area (`doom_explore`), a key
  by the key lying there, a door, switch, item by its own. A spare pickup isn't a pick (she
  takes those by herself).
- **Lists in that order too**, not by distance alone; what the explorer has no objective for
  comes after, nearest first.
- **Where it's said**: marked in `doom_list`, last in `doom_look` and a forced decision; a
  failed order says what's untried nearest what it couldn't reach, or the pick; with no order
  for 30 s it's suggested quietly, at 60 s to be heard. `hints` (`off`, `on_stall`, `always`)
  sets how much; `doom_policy` carries it and `idle`.
- **`doom_look`**: in sight is the engine-side line of sight from her eyes over floor she
  knows, whichever way she faces: a look is her looking round, not her 110 degrees this tic.
  The fight's line is the assessment's (`Combat::now`): safe, dangerous (engaged), deadly
  (outmatched), with surrounded, short of ammo, an arch-vile; and the target and gun the
  fight has chosen, in place of the explorer's pick.

Checked: E1M2 with every suggestion taken as an order and nothing else: 68 of 70 carried
out, out by the exit at 738 s, 41/41 kills. Parity: 24 runs the same (none of it is in a
headless run).

## What Evil's told is told to her as the player (the user, 2026-11-11)

Everything the API tells her is in the second person, as hers to have done: "You found a
secret", "You died fighting 2 Demon", "You're waiting. What next?"; the actions' descriptions
and the briefing too ("You're playing Doom. Moving, fighting ... you do by reflex; you choose
where you go."). The bot isn't a second character in her context: going on without an order is
"by instinct", the fight's own choices "by reflex", its pick "Suggested:". Considered:
"autopilot" for those (a machine beside her again, the thing the change removes). The words
the viewers and the streamer read stay in the third person. For the same reason what she
does is an action, never an order ("No action for 8 s", "Dropped for the new action"); the
code's `Order` and the script's `order go` keep their name, she never reads them.

Who's playing comes with the startup's acknowledgement (`characterId`): kept as
`neuro::session::Character` (Neuro, Evil, another, not said). Nothing differs by it yet.

## Kills and blasts told as moments (the user, 2026-11-11)

`control/directed/kills.rs`. She was told of kills only by a fight's summary and the tally,
and nothing of barrels. Now: a big monster's death (a boss, or 500 health and up; first
tried by threat, as sightings are, and five chaingunners each got a line; then from 400,
and six cacodemons did); several dead
within 3 tics as one line with how many gibbed; barrels going off gathered till the chain's
over (none lit, none gone for 30 tics), with what died within a blast of them and what it
cost her; her own blast; the level's last monster. One barrel that takes nothing with it is
her clearing barrels by instinct and isn't told (the first try told each: thirty lines a
level). Gibbed is the game's own rule (`P_KillMobj`: health below minus the spawn health,
for the kinds with an extreme death), read off the corpses by the game's runner and counted
only for a death in her sight. The hits on her (the engine's, with their cause) reach the
director the same way, as the tally does. Headless runs have none of it: parity the same.

## Things by the names a player knows (the user, 2026-11-11)

What she read had the class names the code keys on ("Fatso", "DoomImp", "ShellBox",
"BlurSphere": ZDoom's, by way of `dsda::type_name`). `control/names.rs` is the one table from
those to what they're called, one and several ("mancubus", "mancubi"; "box of shells"), used
wherever a thing is named to her; the code's tables keep the class names. A test holds every
monster and pickup the game has to a name. Her status by instinct read the explorer's label
("try door (line 136)", "look into sector 21"): the plain phrase now, as the viewers get.

## Why a route failed, told plainly (the user, 2026-11-11)

An action that couldn't be done told her the follower's log line: "failed: FAILED at (-151,
428): unexpected drop into sector 108: the only way on drops where the plan didn't (step 1:
pick up GreenArmor)". Coordinates, sectors and steps are the trace's, not hers.

The follower's failures are a type now (`motor::follower::Trouble`, nineteen kinds) and the
explorer's other ends too (`knowledge::explorer::Why`: seen on the way, gone, took too long,
no route, the route's trouble, the fight's give-up...). Each prints exactly the old log text
(`Display`: the logs, the sweeps' and every tool reading them unchanged; an E1M2 log compared
byte for byte with the build before differs only in its timing line) and has a plain phrase
with no numbers in it, which `Ended::plain` carries to the director: "Couldn't get to item-t212
(green armor): the only way on is a drop you didn't mean to take." Considered: rewording the
log line at the director by matching its text (a second copy of every message, broken by the
first rewording of one).

## Stage 1's moments finished: levels, jumps, teleports, crushers, the budget (the user, 2026-11-11)

`control/directed/moments.rs` and `kills.rs`; the plan's 5.3 has the lines. What decided:

- **A big jump** is told before she runs at it, from the jump the follower lines up for
  (`JumpNews`, carried by the explorer, the last eight only: in a headless run nobody takes
  them): 128 or more across, 64 or more to fall, or a hurting floor below. A hop isn't. How it
  came down: after one told, and a miss always. A miss is the follower's (another sector than
  planned) only when it's another area: off the spot in the same room is made (the first try
  told "Missed the jump: came down in area-111", area-111 being where it was going).
- **A level** is told begun once its title's to be had (the game's automap title), with the
  skill and what she brings; done on its exit, before the next level's begun (the runner
  calls `level_done` on leaving), with the time, the tally and what she leaves with.
- **A teleport into a crowd**: moved more than 64 in a tic, then 3 or more fighting her within
  a second. **Out of ammo** in a fight: the assessment's `guns_armed` gone with something
  engaged. **A crusher**: the hits the engine gives with its cause, gathered as her own
  blast's are. **Raised**: one she knew dead in her sight again.
- **The budget**: 6 moments a minute, the plan's number; over it, what isn't urgent is told
  silently. Urgent (`Report::urgent`): her death, health marks (now 50, 25, 10), losing health
  fast, a crusher, a teleport into a crowd, out of ammo, a level's end, an action's answer.
- Left for stage 4: the one `Situation` snapshot and typed events (the questions are what
  need them; the status, the look and the lists already read the same helpers).

Checked on MAP06 (jumps, a teleport), E1M1 to E1M2 (the level lines). Parity: 24 runs the
same.

## The SDK plan's open questions answered (the user, 2026-11-11)

Section 9 of `docs/NEURO_SDK_PLAN.md` has them. What they change now: with no action from Evil
she **waits** (not on by herself by default): stays put, looks round (a quarter turn every 4
s, the other way the next time), takes pickups only within 256 (was 1024: waiting she
doesn't really go anywhere), fights what comes. Directed play only: parity the same.
For later stages: question deadlines 3 s (class T) and 8 s (class D), from her answering in
250 ms on average and rarely 5 s; restart hers once she's made no progress for a while (90 s
proposed); next level hers to give from the tally screen; no level choice; skill behind the
console's switch; cheats held off; free-text style a plan of its own (a Jev-like model).

## The screens between levels told to her (the user, 2026-11-11)

In stream mode (`--present`, Doom's own screens) she heard nothing between levels, and the
episode's last level got no end line (it was told only on the way into the next). Now: the
level's end is told as the tally comes up (once: `level_done` is idempotent, and the next
level's start doesn't tell it again); a finale's text is told as it comes up, as it reads
("On screen: \"Once you beat the big badasses ...\""), Doom's episode endings and Doom II's
stories between levels alike, from a new engine call (`evil_finale_text`); game over is
told. Her link is passed what's told at once (`Session::between`): no level's tics run on
those screens to pass it on. Her actions still wait for the next level (next level hers is
stage 8). Headless runs have no screens: there, as before, a level's end is told going into
the next.

The text is typed out on screen (a character every 3 tics), so it's told as it is (the user's):
the engine gives as much as is shown so far, and each sentence goes to her as it's finished
typing, the rest once the text stops growing (`present::next_chunk`). On E1M8's ending: nine
lines, 1 to 8 s apart.

Checked: E1M8 in stream mode to Episode 1's end (its end line and its text). Doom II's
stories go through the same finale screen; not seen in a test (MAP06 and MAP11 weren't left
in the time given).

## Evil starts the game: level, skill, one level or on through (the user, 2026-11-11)

The user changed answer 5: she chooses the level to start at, the skill, and whether to play
just that level or on through its episode (Doom II: through the game), from what the console
offers (`control/game.rs` `Choices`; `doom_game start`). By default the console offers its own
map and skill, one level (`evil_maps`, `evil_skills`, `evil_play` empty), so often her one
choice is to start: what's on offer is the streamer's to widen.

When: off a level with nothing under way (the title card waiting, a game over, a level played
by itself once its tally's gone, an episode's end once its story's told), never mid-level
("a new game's offered when it's over"). The offer is told her when it's first there and again
whenever the console changes it. Her link is served on those screens (`Session::off_level`:
any other action answered that there's no level to do it in). Her choice sets the skill for the
next level begun (`Engine::set_skill`; the menu walk shows it), and the window's own copy of the
options (skill, episode) the levels are loaded with; a chosen game is always a level loaded
afresh. An episode's last level played by itself now ends in its finale too (it's the game's).

Checked in a window: at the title, E1M9 refused (not offered), "E1M1 4" refused (single or
continuous not said, both offered), "E1M1 4 single" started at Ultra-Violence (28 monsters
where Hurt Me Plenty has 6); out by its exit, offered again at the title; "E1M2 3 continuous"
started. Parity: 24 runs the same.

## Evil BOOM throughout (the user, 2026-11-11)

The binary became `evil-boom` on 2026-11-08 and the rest kept the old name; now all of it is
Evil BOOM: the package and library `evilboom`; the console page, the windows' titles ("Evil
BOOM" for the stream's, the one OBS finds: its window source to be chosen again), the README,
the docs' titles; the environment variables `EVILBOOM_DOOM_DIR`, `EVILBOOM_ROOT`,
`EVILBOOM_NATIVE_STATS` (`config::env`, the `EVILDOOM_` names still read); the Linux WAD
folder `~/.local/share/evilboom`, the old `evildoom` one used while there's no new one (the
user's files aren't moved); the engine patch's comments. Left: the names of the Python code
this once was, in the record and in two comments that cite it.

## Stage 1 closed: its gate run, and what reading it cold found (2026-11-11)

The gate: the GPT stand-in as Evil on MAP07 (Doom II, skill 2, pistol start, seed 3, 8
minutes at x1, as the run before the SDK work), everything she was told read cold and counted
by channel (`words.py`: messages, words, words a minute).

Found, and fixed:
- **An explore order paused for good.** It paused while anything was fighting her; a mancubus
  out of reach kept shooting from 900, the fight's move was "out of reach: going on", and the
  order never went on: six of the eight minutes with nothing told her worth acting on, eight
  actions in all. Now it pauses only while the fight has her legs (its `Move` doesn't let the
  route go on), as she plays on her own. With run 1's orders replayed (a loadout to live
  through it): area-0 explored at 197 s.
- **Her status was 72% of the words** (24 copies in 8 minutes, 113 words a minute): sent on
  any change at all, each shell, the mancubus's bearing. Now when what she'd act on changes
  (`status_gist`: health and armor by tens, weapons, keys, what she's doing, fighting or not,
  the tally): 6 words a minute.
- **The budget kept the wrong things quiet**: answers to her actions counted against it, then
  her first sight of two mancubi went silent. Urgent lines no longer count, and a heavy
  monster's first sighting is urgent.
- **"10 of backpack"** in the look (several of a kind in one place now "10 backpacks"), and a
  "she" left in a refused retreat.
- **Locks**: the game's own message for a lock tried without its key ("You need a blue card to
  open this door", the switch and skull ones, Boom's any and all) told her, once a try; one
  check (`knowledge::model::needs_key`, now also Boom's "Any key will open this door") for the
  director and the knowledge both.

Numbers: before, 1262 words in 8 minutes (158 a minute, status 906 of them), 1 moment a
minute, 8 actions; after, 451 (56 a minute, status 52), 1.2 moments a minute, 11 actions before
she died to two mancubi at once (the fight's: MAP07 kills her playing alone too). The run
before the SDK work: 42 actions, its choices the same kind (items, the switch, areas); the
stand-in's choices aren't worse, the bot under them was what stalled. Parity: 24 runs the
same.

## Next level and restart hers (the user, 2026-11-11)

The plan's answer 5, built (`doom_game next`, `doom_game restart`; the console's two switches,
on by default; `--choices` `next=on`, `restart=on`, and `start=off` for those without a game to
start).

- **Next**: in stream mode, playing on through, the tally screen waits for her word (it was
  pressed on after 2.5 s), and goes on by itself after 30 s so the stream's never stuck there
  (the user didn't say; my default). She's told on the tally screen.
- **Restart**: hers after 90 s with nothing new, progress counted as the score counts it
  (`sweep::known_progress`, now shared: what's collected or killed, sectors seen, effects
  learned, walls tried, lines mapped and set off, places walked), told her once when it's hers;
  before, "not yet" with the seconds to go. The level from its start with what she came in
  with, what she knows of it kept (`Agent::start_over`, as for a 100% run's restart), no life
  spent (my default: lives are a test setting, and a restart she asks for isn't a death).
- `doom_game` takes `do` start, next or restart (`control::game::GameAct`), in a level and out
  of one; what isn't for now says so.

Checked: E1M1 headless, restart refused at once ("90 s to go"), told hers at 94.9 s, granted,
back at the start; E1M1 in stream mode on through, at the tally "On the tally screen: doom_game
next ...", next given, E1M2 begun. Parity: 24 runs the same.

## Stage 4: questions from the bot (2026-11-11)

`control/directed/ask.rs`. One shape for the bot's wanting her say (the plan's 4.6, class D,
the kind that can wait): `[ask q3] <the question> a) ... b) ... (default, in 8 s). Answer with
doom_answer.` At most one open, 3 a minute (past that the caller does as it did unasked); the
default taken at the deadline (8 s, the user's latency; `doom_policy answers`, 0 waits) and
said; Neuro forced a decision (`doom_answer` and the actions that would also settle it) 3 s in.
Any action of hers closes it as overtaken, and so do a death, a restart, a new level. An answer
to a closed question is told it's too late and what came of it, never applied.

Asked now, where the bot asked her to "resend" before or didn't ask at all:
- leaving with things left (`doom_exit`, a go to an exit, without `sure`): leave anyway, or
  stay (the default: leaving can't be undone);
- a go whose only way crosses a floor that hurts or drops where she can't get back: go anyway,
  or don't (default);
- on instinct, nothing left but walls to search with the exit known: leave, or stay (default);
- nothing new for 3 minutes: on by instinct (the default when she isn't), restart (when it's
  hers), keep on as she is.

Found on the way: **leaving was never asked while she was on instinct.** The exit check looked
at the explorer's options, from which the exits are held back on instinct, saw no exit, and
let `order exit` take her out unasked. It looks past the hold now, as the lists do.

The gate (scripted answers 2, 8 and 30 s late): E1M1 at x1, the exit question: answered at 2 s,
applied ("q1: stay"); at 9 s and 30 s, the default taken at 8 s and the answer refused ("Too
late: q2 is closed (no answer: stay)"); after another action, refused as overtaken. Through the
mock Neuro: the question as `doom_exit`'s answer, forced 3 s in ("default, in 5 s"), answered,
a second answer refused. The route question on E1M3 (a medikit past nukage), answered at once:
gone for. The stuck question on E1M1 after 3 minutes waiting: its default, on by instinct.
Not seen in a test: "nothing but walls" on instinct (a whole level's exploring first).

Left: the instinct's other last resorts (strafing runs, stuck places, every wall, jumps
missed, harm allowed) aren't asked: on instinct they're the bot's, and she can stop it any
time. `Situation` and typed events weren't needed for these (a question closes on any action
of hers instead of being checked against a snapshot); they're for class T (stage 6), where a
question about a fight has to be judged against the fight as it stands.

## Stage 4 finished: her instinct's last resorts asked first (2026-11-11)

On instinct the bot went down `next`'s last resorts by itself. Now whoever directs her holds
them back (`Explorer::resorts`: None, all of them, as playing alone and in every score run; on
instinct, only those she's let it take this level) and the first held back with something to
offer is noted (`resort_wanted`) and asked about: "Nothing else to do here that you know of.
Next would be <it>. a) <take it> b) leave by <exit> (when she knows one) c) stay (default)".
Taking it lets it for the rest of the level; staying ends instinct till her next action. The
resorts: strafing run-jumps, the places she got stuck, every wall, the held-back exit, the jumps
she missed, a floor that hurts (the plan's list; the suspect walls and what was given up lately
are routine, and stay hers).

Found testing, both fixed: allowing one asked it again at once (the explorer's note of it stale
till it chose again: cleared on allowing); and "the way out" was held back with no exit known,
so "leave" failed and she was asked again every 20 s for 15 minutes (held back only with an
exit she knows, when directed; alone, the bot's as it was). The "nothing but walls, leave?"
question now offers each exit she knows by name (E1M3 knew a secret exit and the normal one,
and "leave" had taken the first ranked: the secret one).

Checked: MAP09 (seed 19) on instinct: strafing runs asked at 108 s; answered each time, then
every wall, then the hurting floor, each once; left unanswered, its default ended instinct.
The walls question seen on E1M3 (1385 s), E2M4 (1381 s), MAP06 (1225 s). Parity: 24 runs
the same.

## Stage 6: questions that can't wait (2026-11-11)

The plan's class T (`control/directed/ask.rs`): the bot does its default at once and says so,
`[ask q4] Outmatched: 1 demon, 3 imps, 2 hell knights (health 200). a) as it is (pushing on,
shooting on the way) (doing it)  b) go bold. Open 3 s: doom_answer to switch.`; an answer in 3 s
switches it; after, too late. Never forced (a force answered after the moment is worse than
none). Asked:
- **A fight turned bad**, once a fight, not when she's had her say in it: outmatched, an
  arch-vile or a boss after her, or health under 25. Keep as it is (what the fight's doing, in
  words: falling back, keeping clear, pushing on...), fall back to cover (offered only with
  cover she knows a way to: offered without, it was refused), or go bold. Closed when the fight
  ends as its summary has it (not when one goes out of sight a tic: the first try closed
  within a tic).
- **A run-jump whose miss costs** (a floor that hurts below, or a long drop): jump, or don't.
  "Don't" leaves that leap out for good: every run-jump from that sector to that one (the first
  try declined one run-up and the next was lined up at once, three questions in four seconds).
  Closed once she's jumped.

What settles a question: her say in the fight settles a fight question; an order of hers in
place of what she's doing settles the others (where she goes, leaving, a jump). Not the bot's
own going on (resuming an explore after an errand closed one: fixed), nor an order queued
after (`then`), nor a go order for a fight question (the stand-in's go closed its fight
question before its answer came: fixed).

`Situation` (`situation.rs`): how things stand, worked out once a tic from the fight's
assessment and move and her health; her status's gist, the look's danger line (with what the
fight's doing) and the fight's questions read it.

The gate, the GPT stand-in on MAP06 (skill 3, a loadout, 6 minutes at x1): 7 questions (1.2 a
minute, under the 3); the class D one answered in time and applied; none of the 6 class T in
3 s (the stand-in, reasoning, takes several seconds: Evil's 250 ms would; the default stood
each time, as it's meant to). Checked by script: answered at once switches (go bold, fall back,
don't jump), after 3 s refused; a go order then the answer still switches. Parity: 24 runs the
same.

## What she's told, typed (the user, 2026-11-11)

Every piece of news carries what it is (`control/directed/events.rs`, `Event`: her actions
begun and done, questions, her life, what she finds, monsters, her moves, the bot, the game),
and how much each kind matters is one table (`Event::weight`: quiet, a moment, urgent), not a
flag chosen at each of the 70 places that tell her something. `Report` carries its event;
`notable` and `urgent` come from it. The words stay where they're made (they need what only
that place knows). Each kind's weight is what its telling had before: a directed MAP06 game's
whole report file the same, line for line (111 lines), on the build before and after. What it
opens: a budget that drops the least of the moments first, a stream panel or console that
wants only some kinds.

## Her death told as it came (the user, 2026-11-11)

She was told "You died." (and, with lives left, what she'd been fighting), never what killed
her: a rocket of her own in her face read the same as anything else. Now
(`control/directed/death.rs`) the last blow and what hurt her in the half minute before it,
each by what it was, from the engine's own record of every hit (its inflictor, its source, her
own or not, a barrel and who set it off, the floor, a crusher): "You died: a mancubus's fireball
(-36). Before it, in 30 s: a mancubus's fireball x3 (-64)." "Your own rocket", "a barrel an imp
set off", "a shotgun guy" (a hitscanner or a claw: the monster itself). Missiles have plain
names now (fireball, lightning ball, homing rocket, plasma, fire).

And a death reached her from every runner, through one path (`Watch::tell_death`): before,
only a headless game told one (stream mode and the window told none but game over, and the
last life's in a headless game wasn't told either). The killing blow's hit, which the game
passes on a tic after she's seen dead, goes with it; what's told is passed on at once. Whether
the level begins again is said, not guessed from the lives left (with 2 lives the first death
left "0" and was taken for the end).

Checked: MAP07 from a pistol start at UV with 2 and 3 lives: each death told, the lives
counted ("1 life left after this one", "Your last life"), the last told. Not seen in a game:
a death by her own rocket (the wording's in a test). Parity: 24 runs the same.

## Powerups told as she takes them, and as they run out (the user, 2026-11-11)

She was told of a powerup when she saw it ("Spotted: megasphere"), never when she took it
(keys and guns had their line; the game's own "Invulnerability!", "Supercharge!" went
nowhere but the fight's own clock). Now (`control/directed/powers.rs`), by the game's word for
each: what she got and what it does ("You got invulnerability: nothing can hurt you for 30 s.",
"You got a soulsphere: health 200.", "You got a berserk pack: your fist hits ten times as hard
till the level's end."); and for the ones with a time (Doom's: invulnerability 30 s,
invisibility and the radiation suit 60 s, the visor 120 s; another of the same starts it again)
"Your radiation suit runs out in 5 s." (urgent for invulnerability and the suit, what's keeping
her alive) and "Your invisibility's worn off."

Checked on MAP08 by instinct: the soulsphere, invisibility, two suits (the second's time from
its own pickup), each running out and worn off on time. Parity: 24 runs the same.

And the other pickups (the user's: everything but health and armor bonuses, stimpacks, medikits,
ammo, and a gun she has): green armor and megaarmor ("You got megaarmor: armor 200."), the
backpack, and ammo only of a kind she had none of ("You picked up 4 shotgun shells: 4 shells,
where you had none."). On E1M1 from a pistol start: the green armor, the first shells, the
megaarmor, each told; the clips and the rest, not. Berserk: the bot knows it heals (to 100, as
Doom's `P_GiveBody` does: `health_item`, read by the fight's and the explorer's pickups).
