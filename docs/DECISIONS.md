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

## Open questions / research TODO
- [x] ~~What is ViZDoom's actual compatibility?~~ See findings above.
- [ ] Verify Boom/MBF feature support in ViZDoom with a real community Boom PWAD.
- [ ] Check input/control: continuous turn/move delta buttons in ASYNC_PLAYER mode at 35 Hz,
      visible window at stream-worthy resolution.
- [ ] Engine choice beyond the spike (user has questions before deciding).
- [ ] Survey existing open-source DOOM bot / pathing code (ZDoom bots, MBF helper dogs,
      multiplayer bot projects, etc.).
- [ ] Can two characters (Neuro + Evil) connect to one game simultaneously?
- [ ] Benchmark classifier options (hosted Jev vs. local CPU models) for latency/quality.
