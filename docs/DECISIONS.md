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

## Open questions / research TODO
- [ ] Next spike: build the nav graph from subsectors + key/switch gating; plot reachable
      regions per key state for E1M3.
- [x] ~~What is ViZDoom's actual compatibility?~~ See findings above.
- [ ] Verify Boom/MBF feature support in ViZDoom with a real community Boom PWAD.
- [ ] Check input/control: continuous turn/move delta buttons in ASYNC_PLAYER mode at 35 Hz,
      visible window at stream-worthy resolution.
- [ ] Engine choice beyond the spike (user has questions before deciding).
- [ ] Survey existing open-source DOOM bot / pathing code (ZDoom bots, MBF helper dogs,
      multiplayer bot projects, etc.).
- [ ] Can two characters (Neuro + Evil) connect to one game simultaneously?
- [ ] Benchmark classifier options (hosted Jev vs. local CPU models) for latency/quality.
