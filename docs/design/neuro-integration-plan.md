# Neuro-controlled Doom: architecture and implementation plan

> **Status:** superseded by [neuro-sdk-plan.md](neuro-sdk-plan.md) (2026-11-11). Kept for the first slice's reasoning.

Date: 2026-11-08 (revised from the 2026-11-07 draft)

Status: agreed direction; implementation not begun. The classifier and style design (section 5)
needs its own design pass before it is built. Timings and defaults are to be measured.

## Purpose

Evil plays Doom through the bot. She decides **where to go and what to do** (exploration is
hers), and she shapes **how the bot plays** (its fighting style, risk, weapons, resources) in her
own words. The Rust bot keeps everything that has to happen faster than a language model can
answer: moving, aiming, dodging, the fight from tic to tic. It executes her decisions and her
style, and it never acts on goals of its own.

| Role | Who | Speed |
|---|---|---|
| Commander | Evil, through the Neuro games SDK (a GPT stand-in through a mock Neuro server for testing) | seconds |
| Style and small-decision classifier | a fast local model (TEV), hosted Jev for testing | tens to hundreds of ms |
| Executor | the Rust bot: navigation, combat, motor control at 35 Hz | every tic |

The game never pauses for a model.

## 1. What Evil does

### Exploration: directed by her

The bot doesn't explore on its own initiative in this mode. When it isn't forced to react (a fight,
a hazard), it **waits for Evil's next decision**, holding a safe position (and, after a short
wait, forcing one: section 3). She decides with:

- **Lists** of what's known: unexplored openings (frontiers), doors and switches not yet tried,
  items seen, keys and locked doors, the exit if seen, monsters remembered. These come from what
  the explorer already generates internally (its objectives and their reasons), filtered by what
  she has seen (the automap rule: nothing she hasn't seen).
- **Inspection** of one thing: where it is relative to her and to named places, the way there
  (its length, known hazards, monsters remembered near it), what it's likely for.
- **Orders**: go to / try / pick up / explore toward a listed thing; explore a direction or an
  area; go back to a named place; stop; leave the level by the exit.

Orders run to completion, failure or replacement; the bot reports which, and waits again.

### Naming places

Doom maps have no room names. The bot gives each known place and thing a stable ID and a short
generated description (from keys, doors, items, lifts, damaging floors, size, direction). **Naming
is left to Evil** ("call that the slime pit"); the bot doesn't suggest names. Her names are used
from then on in both directions: her orders and the bot's reports. Names are map-local and persist
across deaths on that map. A name too long (over about 32 characters) or already used for
something else on the map is refused with a failing result saying why, so she picks another; it
isn't truncated or changed (a name she didn't choose is one she won't use).

### Style: how the bot plays, set by her

She describes how to play in free form ("play aggressive", "save the rockets", "don't waste the
BFG on small stuff", "keep your distance from that cyberdemon", "snipe from cover"). The
classifier turns that into **style parameters** (section 5) the fight and the explorer read. The
values we've been tuning by hand become her settings, with our tuned values as the default style.

## 2. Context: what she's told

- **Status**, short and regular (rate-limited, only when something changed): health, armor,
  weapons, ammo, keys, current order and its progress.
- **Events**: order completed or failed (and why), something new seen (key, exit, weapon,
  powerup, big monster), damage taken heavily, low ammo or health, death, level complete.
  One line each, coalesced, never repeating what she already has.
- **On request**: the lists and inspections above.

Context is the stream experience: too much floods her, too little leaves her blind. Budgets and
wording are tuned by watching real runs.

## 3. Action interface (structured)

Neuro actions take a JSON schema; Evil fills parameters herself. Stable registrations; availability
changes only at lifecycle transitions; references validated at execution time, not by
re-registering enums as things change.

| Action | Arguments | Purpose |
|---|---|---|
| `doom_list` | `what`: frontiers / doors / items / keys / monsters / places | What she can act on |
| `doom_inspect` | `ref` (ID or her name) | One thing in detail |
| `doom_go` | `ref`, optional `then` | Go to, try, pick up, explore at a listed thing |
| `doom_explore` | `direction` or `ref` | Explore toward a direction or around a place |
| `doom_name` | `ref`, `name` | Name a place or thing |
| `doom_style` | `instruction` (free text) | Change how the bot plays (classifier) |
| `doom_stop` | none | Cancel the current order; hold |
| `doom_exit` | none | Leave by the known exit |
| `doom_start` / `doom_restart` / `doom_next_level` | as available | Lifecycle |

| State | Available |
|---|---|
| Ready | start, list, inspect |
| Playing | all but lifecycle |
| Dead | restart, list, inspect |
| Level complete | next level, restart, list, inspect |

### Fitting the SDK (checked against the spec, best practices and changelog, 2026-11-08)

- **Waiting for her is a force.** The bot waiting for her next decision is the game "blocked on
  her decision", which the SDK handles with `actions/force`: without one she gets distracted and
  forgets the game. Timeout-then-force: when the bot goes idle, give her a few seconds to act on
  her own, then force with `state` (status and the current options, Markdown), a short `query`,
  `ephemeral_context: true` (the option lists are bulky and change), and `priority: "medium"`
  (real time, but not cutting her off; `"high"` only if something is attacking while she talks).
- **One force at a time**, waited out; a new one can cancel the one in progress. Our actions are
  non-disposable, so she may act just before or after a force resolves: the handler accepts any
  valid action at any time, independent of the force state (the SDK's ActionWindow pattern).
- **Results within seconds, after validation, before execution.** The server gives up after about
  20 s and Neuro is frozen until it arrives. A successful result's `message` says what changed for
  her ("Heading for the blue door, about 600 units, past the slime pit."), per the latest
  guidance. A failure during a force is retried automatically, so failure messages are actionable
  ("No 'east door'; known doors: ..."). A request that's valid but can't be done now (a door
  already open) is `success: true` with an explanation, to avoid a retry loop.
- **Completion is context**, not a second result: "Reached the blue door; it needs the blue key."
- **Context**: Markdown without `#` headings, occasional and meaningful, never a stream of
  position updates. Status and routine updates `silent: true`; events worth a reaction on stream
  (a boss sighted, a key found, nearly dead, the level done) `silent: false`. Rules and controls
  once at startup and at each new level.
- **Schemas**: `type: "object"`, simple keywords only (no `oneOf`, `additionalProperties`,
  `$ref`, ...). Enums for fixed sets (`doom_list`'s `what`); a free string for references whose
  options change (validated by us, failing with the valid options). One action with a few
  parameters rather than chained forces (each link is a round trip).
- **Stable registration.** Register everything at startup; changes only at real lifecycle
  transitions, which are rare (start, death, level end). Names prefixed `doom_` (names are shared
  with any other integration connected to her).
- **Unknown or stale actions** get a failing result, never silence.
- **Reconnect**: re-send `startup`, re-register at once. Her context survives a disconnect, so
  the resync is a short status, not a replay.
- **Startup acknowledgement** carries `characterId` (`evil` for Evil Neuro): log it.
- **Information**: what a human player would have (the SDK's "don't let her cheat"; our
  automap rule). Be painfully explicit; lists of ~20 are fine.
- **Testing**: Randy (the SDK's mock: random actions, only when forced, HTTP to inject actions)
  for protocol basics; neuro-api-tony (the mock the published integrations name for local testing) for
  manual play; our GPT stand-in as a third mock for whole-level runs. Malformed data has to be tested
  by us: Randy never sends it.
- **Implementation**: the protocol is small (eight message types); a module of our own over
  `tungstenite` is simpler to own than the community `neuro-sama` crate (unendorsed, last
  changes unknown), though its schema types are a useful reference. Vendor-prefix any custom
  commands; unknown commands are silently ignored by the server.

## 4. The executor (the bot)

- **Directed mode.** The explorer's objective generation stays; its own selection becomes
  optional. In directed mode objectives come only from Evil's orders; when none is active the
  bot holds a safe spot and waits. The autonomous mode stays as a baseline and for tests.
- **Reflexes stay in Rust.** Dodging, ducking, crushers, falling, barrel blasts, arch-vile fire,
  the BFG's hold: immediate, every tic, never waiting on a model.
- **Fights.** Self-defence is automatic; whether to stand, pursue, break off or avoid is part of
  the style (and of her orders: "avoid fights" changes engagement within its documented
  self-defence floor).
- **Perceived world.** Everything model-facing comes from a `PerceivedWorld` snapshot built from
  the bot's knowledge (what she's seen and learned), never raw engine state. Facts, inferences
  and unknowns are marked as such.
- **Arbitration.** One owner of movement, aim, fire and weapon at a time; commitments (a rocket or
  BFG shot under way) are explicit and can't be stolen.

## 5. Classifier and style (needs its own design pass)

Two jobs for the fast classifier:

1. **Style interpretation.** Free text from `doom_style` (and style parts of other instructions)
   -> changes to a set of style parameters. The classifier chooses among supplied values; it
   doesn't invent parameters. Ambiguous or unsupported requests come back to Evil as such.
2. **Small fight decisions**, where speed matters and her style should show: weapon choice, target
   choice, engage / hold / break off, when to spend a powerup. Rust supplies the feasible options;
   the classifier picks one in the style's terms; Rust revalidates before acting. At most one
   request in flight; stale answers dropped (correlated by map, order, option set and tic);
   deterministic fallback when it's slow or down.

**Style parameters** to be designed from the bot's existing tuned constants, for example:
aggression (stand / pursue / keep clear thresholds), preferred range, risk tolerance (outmatched
test, retreat health), ammo policy (conserve per kind, boss reserve), weapon preferences and
prohibitions, powerup policy (invulnerability, BFG), infighting use, cover use. Each has a range, a
default (today's tuned value), and documented effect.

This means reorganising much of the fight code so its decisions read from one style object and
expose their options, rather than from constants scattered through the tree. The design pass
decides the parameter set, the decision points handed to the classifier, and how much of the
combat tree becomes classifier-selectable.

## 6. First slice (thin, to prove the idea; not to be optimised for long)

1. Directed mode in the bot: orders from a queue, waiting when idle, completion and failure
   reports. Lists and inspection from the explorer's own objectives. Generated descriptions and
   naming.
2. The Neuro adapter in the existing binary (its own thread), the structured actions above minus
   `doom_style`, status and event context.
3. A mock Neuro server: scripted first, then a GPT stand-in through the same adapter.
4. The current viewer window and log as the console.

Gate: a GPT stand-in completes a level by listing, inspecting and ordering, with the bot waiting
between orders and reporting truthfully.

## 7. Later stages

| Stage | Deliverable |
|---|---|
| Style | Style parameters, `doom_style`, the classifier (interpretation first, then small fight decisions) |
| Console | A web console: messages, orders, classifier decisions, bot state, troubleshooting controls |
| Robustness | A worker process for the engine (crash isolation), reconnects, restart, live settings |
| Replay | Recorded controls and decisions, decision replay against other models |
| Stream rehearsal | Game-only capture, long runs through connection and inference failures |

Carried over from the earlier draft, to apply in these stages: correlation IDs and staleness on
every model answer; settings applied atomically at a tic boundary with revisions; the console never
mutates the engine directly; localhost binding and server-side credentials; a developer
ground-truth view never leaks into model-facing snapshots; restarts clear map-local orders unless
carried explicitly.

## 8. Measures

Instruction fidelity (did the bot do what she said, within her style), order completion and
failure honesty, waiting time between orders, survival and progress, context volume, classifier
latency (p50/p95/p99), stale and fallback rates, and whether her style changes are visible in play.
Winning alone isn't the measure.

## References

Check and pin versions when implementing.

- [Neuro API specification](https://github.com/VedalAI/neuro-sdk/blob/main/API/SPECIFICATION.md)
- [Neuro integration best practices](https://github.com/VedalAI/neuro-sdk/blob/main/API/BEST_PRACTICES.md)
- [Neuro SDK changelog](https://github.com/VedalAI/neuro-sdk/blob/main/CHANGELOG.md) (read 2026-11-08: latest entry 21 August 2026)
- [Randy](https://github.com/VedalAI/neuro-sdk/tree/main/Randy) and [neuro-api-tony](https://github.com/Pasu4/neuro-api-tony) (mock servers)
- Published integrations: in [Hollow Knight](https://github.com/VedalAI/neuro-hollow-knight) and
  Cyberpunk, Vedal plays and Neuro is a companion (tactics, shooting at him, directions): not a
  precedent for her controlling the game. Where she plays (Slay the Spire 2, Inscryption, Pokémon
  Platinum) the games are turn-based or menu-driven. None has her directing a real-time game, so
  the waiting and forcing design here is ours to work out and measure.
- [TEV in Ollama and the SystemOne interface](https://ollama.com/library/tev1)
- [Together TEV repository](https://github.com/togethercomputer/tev1)
- [TypeSafe Choice documentation](https://docs.typesafe.ai/primitives/choice)
- [TypeSafe confidence documentation](https://docs.typesafe.ai/confidence)
