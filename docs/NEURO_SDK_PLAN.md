# Evil's control of the game: the full Neuro SDK layer

Date: 2026-11-11. Status: a plan, for the user to cut down or redirect; stage 0 (driving the
existing layer by hand to find what's missing) is under way.

This builds on `docs/NEURO_INTEGRATION_PLAN.md` (2026-11-08), whose first slice exists:
`control/directed.rs` (the `Director`: lists, orders, reports), `neuro/session.rs` (nine
actions, status, events, a forced decision when she's idle), `neuro/gpt.rs` (a GPT stand-in),
`advisor.rs` (a fast classifier's say in target and stance). What that slice gives her is
**where to go**. This plan is the rest: every decision the bot makes, which of them she can
have, at what speed, at what grain, and what she has to be told to decide well and to have
something to say on stream.

## 1. What decides the design: time

A language model that also talks answers in seconds. The game runs at 35 tics a second and
never pauses for her (the earlier plan's rule, kept). So every decision is sorted by **how long
its answer stays worth having**, and that class fixes how she can touch it:

| Class | Answer worth having for | Examples | How she has a say |
|---|---|---|---|
| **R** reflex | under 0.3 s | a dodge's direction, the aim, the trigger, out from under a crusher | never asked. A **policy** set beforehand shapes it ("don't bother dodging imps", "never the BFG"). |
| **T** tactical | 1 to 8 s | which monster first, which gun, stand or back off, fall back to cover, press on through | the bot acts on its rule **at once**; she can **override** (an order that holds a while) or set a **stance**. Told only when it's worth a line. |
| **D** deliberate | 10 s to minutes | where next, press this switch, take the drop with no way back, leave with things left | the bot **proposes or waits**; she chooses. A default after a wait she can set. |
| **S** standing | the level or the session | how thorough, how careful, how she fights, restart from a pistol | set whenever; holds till changed. |

Three rules follow, and everything below keeps to them:

1. **Every decision has the bot's own answer.** She's never needed for the game to go on; at
   every grain the rules' answer is the default. What changes with her settings is only how
   long the bot waits for her before using it.
2. **Nothing she says is carried out stale.** An answer names what it answers (an id); the
   bot checks it still applies (the monster's alive and about, the door's still shut, she's
   still on that ledge) and says so if not.
3. **One vocabulary.** The same ids, the same words for directions, distances, danger and
   outcomes in every list, event, question and answer (section 5).

## 2. The decision map

Every decision the bot makes today, where it's made, its class, and what she gets. "Level"
is the grain of control from section 3 that reaches it. Decisions marked **new seam** need a
way in that the code doesn't have yet (section 6).

### 2.1 Where to go and what to do (the explorer)

| Decision | Made in | Class | Hers through | Default when she's silent |
|---|---|---|---|---|
| The next objective (of 15 kinds: exit, key, switch, door, frontier, room, item, barrel, wall, teleporter, noise, hunt, ...) | `explorer/selection.rs` `next`, `objectives.rs` | D | orders (go, explore, press, exit); goals; autopilot | autopilot: the rules' choice. Directed: waits |
| Which pickup is worth a detour (needed health, armor, a gun, ammo) | `hooks.wanted`, `objectives.rs` `healing`, `spare_pickups` | D / S | policy `pickups` (all spare / needed only / ask); `doom_go item` | spare pickups taken when safe (as now) |
| Press a switch nobody knows the effect of | `Kind::Switch`; never pressed by an explore order | D | `doom_press`; policy `switches: ask / press / leave` | ask (as now: exploring finds, she presses) |
| Open a door, a keyed door | `Kind::Door` | D | orders; part of exploring | part of exploring |
| Take a teleporter (where it leads unknown) | `Kind::Teleporter` | D | `doom_go`; policy `teleporters: ask / take` | ask the first time, then take |
| A way that costs health (a floor that hurts) | `Refusal::ShortOfHealth`, `HarmWhileSafeLeft`; `harm_allowed` | D | `risky` on an order; a **confirmation**; policy `hurt_floors` | refused while anything safe is left |
| A drop or jump with no known way back | `Refusal::JumpOverNoWayBack`, `DropOntoHarm`, `ShutsHerIn`; `cut_off.rs` | D | a **confirmation** ("the only way on is a one-way drop into the yard") | taken only when nothing else is left |
| Search walls for secrets, and how hard | `Suspecting`, the tiers of `next` | S / D | `secrets` 0 to 3 on explore; policy `secrets` | off |
| Clear barrels before they're a danger | `Kind::Barrel` | S | policy `barrels: clear / leave / ask` | clear when no monster's in sight |
| Go and see what made a noise; hunt a monster heard | `Kind::Noise`, `Kind::Hunt` | D | `doom_go monster-N`; goal `hunt` | autopilot does; directed tells her |
| Leave by the exit, with things left | `Order::Exit`, `exit_check` | D | `doom_exit`, `sure` | asks if things are left |
| Give an objective up (took too long, failed) | `selection.rs` drop rules | D | told; she can order it again | given up, told why |
| The last resorts (strafing runs, stuck places again, every wall, the held-back exit, jumps missed before, harm allowed) | the ten tiers of `next` | D | each tier she hasn't allowed is a **confirmation** ("nothing left but walking through the lava: 40 health") | autopilot: as now. Directed: asked |

### 2.2 The fight

| Decision | Made in | Class | Hers through | Default |
|---|---|---|---|---|
| Fight this at all: stand, close in, shoot on the way, get past, keep clear, sneak by | `combat/tree.rs` (one node a move), `Move` | T | **stance** (bold / steady / careful / avoid); `doom_fight push / hold / disengage`; `rush` on an order | the tree |
| Which monster first | `pick_target` | T | `doom_fight target REF` (holds while it's about: `forced_target`, exists) | the rule |
| Which gun | `arsenal.rs` `best_gun` | T | `doom_fight weapon NAME` (new seam); policy `save` (rockets, cells, BFG for bosses) | the rule |
| Hold fire (not to wake a pack; to leave two monsters at each other) | `Sneaking`, `LeavingToIt`, `HeldFire` | T | `doom_fight hold_fire` / `fire`; policy `stealth` | the rule |
| Retreat, and which kind: to cover, back the way she came, out of the middle, to ammo, to a corner to fight from | `RetreatKind`, `player.rs` | T | `doom_fight retreat [to REF]`; policy `retreat_health` | the rules |
| Set monsters on each other; bait | `SettingOn`, `RetreatKind::Bait` | T | `doom_fight infight`; policy `infighting: use / ignore` | used where seen |
| Use a powerup now (it's taken on pickup; so: go for it) | pickups | T / D | `doom_go item`; told when one's near in a hard fight | spare ones taken when safe |
| Run a field of barrels, shoot a barrel by a monster | `OutOfBarrels`, barrel nodes | R / T | policy only | the rules |
| Dodge, duck, shake off a homing missile, out from an arch-vile's sight, hold still for the BFG | survive nodes | R | policy `dodging` at most | always |
| Aim, lead, trigger timing, rocket safety | `aiming.rs` | R | never; `HeldFire` is told as a reason | always |
| Turn to look at a sound, a hit from behind | `Looking` | R | never | always |

### 2.3 Getting there (the follower)

| Decision | Made in | Class | Hers | Default |
|---|---|---|---|---|
| Wait for a lift, a door, a crusher's stroke | `follower/hazards.rs`, `interactions.rs` | R / T | never; told if a wait runs long ("waiting on a crusher, 8 s") | always |
| Commit to a run-jump | `jumping.rs` | T | a **confirmation** only when a miss is costly (lands in harm, no way back) | taken |
| Press again, closer; give a press up | `press` | T | told on giving up | as now |
| Back off when stuck, plan another way, give the step up | `walking.rs` `stuck` | T | told on giving up | as now |
| Shoot a barrel in the way, from where | `barrels.rs` | T | policy `barrels` | as now |

### 2.4 The game itself (meta control)

Control over the game, not the player in it: hers too, within what the streamer allows
(4.7). All class S or D: none has a clock, and those that throw a run away ask first.

| Decision | Class | Hers through | Default | Engine |
|---|---|---|---|---|
| Start a game: the WAD, the episode, the level, the skill | S | `doom_game new` | the console's | `evil_new_game` (exists) |
| The level again, mid-level or dead: what she learned kept, what she came in with restored | D | `doom_game restart` | dead: again, once the death's been shown | lives (`episode --lives`, exists) |
| The level again from a pistol start (when she arrived with nothing: E2M5's lesson) | D | `doom_game restart pistol` | not unless asked | `evil_new_game` |
| The level again as new: what she learned forgotten too | D | `doom_game restart fresh` | not unless asked | a new `Knowledge` |
| Next level; skip this level; go to a level by name | D | `doom_game next` / `level NAME` | next at the tally, carrying | `Engine::goto` (exists) |
| Change the skill (takes effect on a restart) | S | `doom_game skill N` | unchanged | `evil_new_game` |
| Back to the title; end the session | S | `doom_game title` | the console's | `watch --present --title` (exists) |
| Pause for the stream (a break, an ad) | S | the streamer's console only | | the console's (exists) |
| **Cheats** | S | `doom_cheat` (4.7), only when the streamer's switched them on; the codes are hers to know | off | below |
| Who's playing: autopilot, directed, between | S | `doom_policy idle`, `ask` (section 3) | directed with a fallback | |

Cheats, each mapped to what the bot can truly do with it (a cheat she can't use is refused
with the reason, not faked):

| Cheat | Does | The bot's side |
|---|---|---|
| `iddqd` god mode | nothing hurts her | `evil_god` and `Combat::immune` (exist: the scouting pass plays so): the fight stops dodging and retreating, floors that hurt are floors |
| `idkfa` / `idfa` everything / no keys | all guns, full ammo, keys | `evil_give` (exists); she sees it in what she carries; keyed doors open to her |
| `idbehold` a powerup | berserk, invulnerability, the suit, the map, light, invisibility | `evil_give` extended; each is something she already understands as a pickup |
| `iddt` / the computer map | the whole map known | her knowledge filled in, as `--peek` runs have it (exists): the one place the automap rule is hers to break |
| `idclev` warp | another level | `Engine::goto` (the same as `doom_game level`) |
| `idmus` music | changes the track | the engine's; nothing to the bot. A stream toy |
| `idclip` no clipping | walks through walls | **refused**: her planning is the walls. Said so ("I can't walk through walls even when I'm allowed to") |
| `idchoppers` | a chainsaw | `evil_give` |

A cheated level is marked so, in her status, in the tally she's told at the end and on the
stream's column ("cheated: god mode from 2:14"), and never counts in anything measured.

## 3. Grain: five levels, hers to mix

She shouldn't have to micromanage to play, nor be shut out of the fight because she chose to
direct. Each level is a set of actions; all are registered at once (the SDK wants registration
stable); what differs is how much the bot does unasked.

**Level 0, autopilot.** The bot plays as it does in a score run. She watches, comments, and
steps in with anything from the levels below; what she orders is done, then the bot carries on
by itself. For a level she's bored of, a part she doesn't care about, or a streamer's "just
finish it".

**Level 1, goals.** One line of intent the bot works toward by itself, reporting as it goes:
`get out`, `everything` (100%), `find KEY`, `kill everything`, `get healthy` (health, armor,
ammo before going on), `find secrets`. A goal is a filter and an order over the explorer's own
objectives (the 100% runs' `--complete` and the key-first rules already exist; this exposes
them). The bot asks only for confirmations.

**Level 2, orders** (exists). Go, explore, press, exit, stop: one thing done, reported, and she
waits. Finer than a goal: she picks the door.

**Level 3, tactics.** In a fight: target, gun, push, hold, retreat, hold fire. Each holds for
a short stated time or till its subject's gone, then the rules have it back, so a forgotten
order can't kill her. This is where her play shows on stream ("kill the archvile FIRST").

**Level 4, policy.** Standing settings in fixed words (section 4.4): stance, what to save,
how careful with floors and drops, pickups, secrets, barrels, what to ask her about. And free
text ("play like a coward") through the classifier, which may only choose among those same
settings. The tuned numbers are the default policy.

**Confirmations** cut across the levels: the decisions in section 2 marked so are put to her
as a question with the bot's own answer attached, when her policy says ask.

The mode (`doom_mode`) is only two settings: **who picks the next objective when she's idle**
(`wait` for her, `auto` after N seconds, `auto` at once) and **what's asked** (`everything
marked`, `only what can't be undone`, `nothing`). Autopilot is `auto` at once and `nothing`;
the current directed play is `wait` and `everything`; the default proposed for stream is
`auto` after 20 s (announced: "No order: carrying on by myself, exploring north") and `only
what can't be undone`, so dead air never lasts and she's never shut out.

## 4. The actions

Sixteen at most, registered for the whole game but for `doom_cheat` (descriptions as terse as today's: every word is in
her context throughout). The nine that exist keep their shape.

### 4.1 Looking (exist; two added)

- `doom_list what [all]`, `doom_inspect ref`, `doom_status`: as now.
- **`doom_look`** (new): what's round her now, as a player's eyes would give it: the room
  (size, exits by direction, floor that hurts), what's in sight (monsters by kind and
  distance, items, switches, barrels), what she hears. The one call for "what's going on".
  **Built** (`control/directed/look.rs`; `look` in a script): a line a kind of thing, none
  for a kind there's nothing of:

      In area-18: small room, explored.
      Danger: deadly (outmatched). On monster-90 (Fatso) with the super shotgun.
      Leads to: area-0 (SW 35, 10 to explore); not seen yet (SE 233)
      Monsters: monster-90 Fatso (SW 428); monster-88 Fatso (NW 543)
      Out of sight, seen lately: 2 (nearest W 600).
      Items: item-t98: ShellBox (S 181); item-t78: PlasmaRifle (NW 550); +7 more
      Untried: switch-l131 (N 814)
      Barrels: 2, nearest NE 226.
      Heard, not seen: about E 600.
      My pick: door-l40: door not tried (N 210).

  In sight is a line of sight from her eyes at the heights as they are, whichever way she
  faces (a look is a look round), of what's on floor she knows; "leads to" is the areas across
  the openings she's seen of the one she's in (a door, a step or a lift between counted
  through; not a window's sill or a ledge above), one she's seen nothing of said as "not seen
  yet". Out of a fight the last line is the explorer's pick; in one it's left out, and the
  danger line names the monster and gun the fight has chosen. What's not in it yet: the
  room's shape, and sounds other than the explorer's noises.
- **`doom_why`** (new): why she's doing what she's doing, from the decision journal: the
  objective and what it was chosen over; in a fight, the move and its reason ("backing off: 2
  demons within reach, shotgun reloading"); why she isn't shooting (`HeldFire`, made one
  answer: section 6); why the last order failed.

### 4.2 Orders (exist; one added)

- `doom_go ref [risky] [sure] [rush]`, `doom_press ref [rush]`, `doom_explore target
  [secrets]`, `doom_exit [sure]`, `doom_stop`, `doom_name ref name`: as now.
- **`doom_goal goal [ref]`** (new): `out | everything | key | kills | supplies | secrets |
  none`. Replaces the order queue with a standing aim; `none` back to waiting.

### 4.3 Tactics (new)

One action, so the fight costs one description:

- **`doom_fight do [ref] [seconds]`**, `do` one of:
  - `target` (ref a monster or a kind: "the archvile", `monster-12`): shot first while it's
    about and she can get a shot.
  - `weapon` (ref a gun; `auto` to give it back).
  - `push`: fight on forward: stands and closes where the rules would keep clear (still out of
    blasts and from under crushers).
  - `hold`: fights from where she is; no closing in, no chasing.
  - `retreat` (ref optional: a place, an item, "cover"): breaks off to it, shooting on the way.
  - `hold_fire` / `fire`.
  - `ignore` (ref): leaves a monster be unless it's at her (a turret on a ledge, a boss to run
    past).
  - `auto`: the rules again.
- Each lasts `seconds` (default 10, at most 60) or till its subject's dead or lost, and the
  result says so ("Targeting the archvile, 10 s."). The survive moves (class R) are never
  switched off by it.

### 4.4 Policy (new)

- **`doom_policy set value`**: fixed settings, each an enum (the schema lists them, so she
  can't invent one):

  | set | values | reads on |
  |---|---|---|
  | `stance` | `bold`, `steady`, `careful`, `avoid` | `Tuning` (the advisor's stances, tuned) |
  | `save` | any of `rockets`, `cells`, `bfg`, `none` | `best_gun`'s reserve rules |
  | `retreat_at` | `never`, `low` (25), `normal` (40), `early` (60) | `retreat.health` |
  | `hurt_floors` | `never`, `ask`, `if_cheap`, `freely` | `harm_allowed`, `Refusal` |
  | `one_way` | `ask`, `last_resort`, `freely` | `cut_off.rs` |
  | `switches` | `ask`, `press` | the explorer's switch objectives |
  | `pickups` | `all`, `needed`, `ask_big` | spare pickups, powerups |
  | `secrets` | `0`..`3` | wall search |
  | `barrels` | `clear`, `leave` | `Kind::Barrel` |
  | `infighting` | `use`, `ignore` | bait, setting on |
  | `ask` | `everything`, `irreversible`, `nothing` | confirmations |
  | `hints` | `off`, `on_stall`, `always` | rankings' picks and unprompted suggestions (5.5) |
  | `idle` | `wait`, `auto_20`, `auto` | the mode |
- **`doom_style text`** (later stage): free text to the classifier, which answers with
  `doom_policy` changes it made, in those words ("stance careful; save rockets"), or says it
  didn't understand.

### 4.5 Answers and the game (new)

- **`doom_answer id choice`**: the reply to a confirmation or a proposal (4.6). `choice` is
  one of the options given, by its key.
- The game itself is 4.7.

### 4.6 Questions from the bot: proposals with a default and a clock

One shape for every time the bot wants her say:

```
[ask q17] One way on: a drop into the yard, no way back known. 3 things left up here.
  a) take it   b) finish up here first (default, in 8 s)   c) stop
```

- An **id**, the question in a line, two to four **options with keys**, which is the
  **default**, and **when** it's taken.
- **Class D** (the bot can wait): she holds somewhere safe; the question goes as context (not
  silent); if she hasn't acted in a few seconds it's an `actions/force` with `doom_answer` and
  the orders that would also resolve it, state ephemeral. At the deadline the default's taken
  and said ("No answer: finishing up here first."). Deadline from policy: 8 s on stream by
  default (section 9: she answers in 250 ms on average, rarely 5 s), `wait` to hold for her.
- **Class T** (it can't wait): the bot **does its default at once** and says what, with the
  alternative and how long it's open: "[ask q18] Archvile, 600 E. Going for cover (default).
  a) cover b) rush it c) keep on to the exit. Open 3 s." An answer in time switches; after,
  it gets "too late: q18 closed (she's in cover)". Never forced: a force that arrives after the
  moment is worse than none.
- **At most one open question**, at most a few a minute (a budget; over it, the default is
  just taken and logged). A question is asked only where the options are **close or the stake
  is high**: the journal already has each rule's options and scores, so "close" is the margin
  between the best two, and "high stake" is a list (can't be undone, costs over N health, a
  boss, the last of an ammo). Everything else is done and, if notable, told.
- Asked by the fast classifier instead when one's configured and the class is T: same
  question, same options, same journal (the advisor seam). Evil's answer outranks it.

### 4.7 The game itself (new)

- **`doom_game do [value]`**, `do` one of `restart` (value `pistol` or `fresh`, else as she
  came in), `next`, `level` (value a level's name), `skill` (value 1 to 5), `new` (value an
  episode or a level), `title`. Anything that throws away a run in progress answers with a
  question first (a class D proposal: "Restart E2M5 from a pistol start? You're 6 minutes in
  with the red key."), as leaving with things left does now.
- **`doom_cheat code`**: a code typed, as at the keyboard. **She isn't told what the codes
  are** (the user's): the description says only "Type a cheat code.", the parameter is a free
  string, not an enum, and nothing lists them. Whether she knows `iddqd` and `idkfa` is hers
  to show, and chat's to tell her. What comes back is what the game would give: the code's own
  message on one that works ("Degreelessness Mode On", "Very Happy Ammo Added"), and on
  anything else just "Nothing happens.": no hint, no list, no "did you mean". A near miss
  (`idfk`) is nothing, as in the game. Case and spaces don't matter; `idclev` and `idmus` take
  their two digits, `idbehold` its letter, as typed.
  - No-clip is the one that works in the game and not for her: "Nothing happens. (I can't walk
    through walls.)"
  - The streamer can drop a hint from the console (a line of `say`), if she's flailing.
  - **Registered only while the streamer has cheats switched on** in the console, and
    unregistered when they're off: registration is the SDK's own way of saying what's possible,
    so a cheat she isn't offered is one she can't be talked into by chat.
- **What the streamer allows** is a short list in the console, each a switch: change level,
  change skill, restart, cheats (all or named ones). Off, the action answers "not allowed
  right now" and nothing else changes. The streamer's own controls (start, stop, pause, goto,
  say, act for her) stay as they are and outrank hers.
- Told, not silent, when any of it happens, hers or the streamer's: "Restarting E2M5 from a
  pistol start." "God mode on (cheat)." A cheat's a moment; so is refusing one.

## 5. What she's told

Two readers: **her deciding** (needs the state, the options, the stakes, tersely) and **her
on stream** (needs moments: something to react to, with enough colour to say something
about). Both are served by five channels, each with a budget, in one grammar.

### 5.1 The grammar (one vocabulary everywhere)

- **Voice** (the user's, 2026-11-11): she's told everything as the one playing, in the second
  person: "You found a secret (2/5).", "You're losing health fast", "You died", never "she
  found". There's no bot beside her in what she reads: what she does without an order she
  does "by instinct" (exploring on) or "by reflex" (the fight); the bot's pick is
  "Suggested: ...". What the viewers and the streamer read (the stream's panel, the console,
  the logs) keeps "she".
  And she takes **actions**, she doesn't give **orders** (there's nobody to order): "No
  action for 8 s: you carry on by instinct", "Dropped for the new action". (In the code and
  in this plan an "order" is still the thing an action queues: `Order`, `order go` in a
  script.)
- **Names**: things go by what a Doom player calls them (`control/names.rs`): "mancubus",
  "2 mancubi", "baron of Hell", "chaingunner", "box of shells", "megaarmor", "partial
  invisibility"; never the class names the code keys on ("Fatso", "ShellBox", "BlurSphere").
  What she's doing by instinct is in plain words too ("picking up the shotgun (a better
  gun)"), not the explorer's label.
- **Who's playing**: the startup's acknowledgement gives `characterId` (`neuro`, `evil`);
  `Session::character()` has it (`Character`), for telling one twin differently from the
  other. Nothing is tailored by it yet. The stand-ins (`neuro-mock`, `neuro-gpt`) acknowledge
  as Evil.

- **Ids**: `kind-N` (`door-3`, `switch-7`, `area-2`, `monster-12`, `item-t41`, `exit-1`),
  stable for the map; her names replace them in everything said back.
- **Where**: compass and distance from her, rounded: `NE 400`. Height when it matters:
  `NE 400, up on a ledge`. Never coordinates.
- **Danger**, three words from the fight's own assessment: `safe`, `dangerous`, `deadly`
  (outmatched), with what makes it so: "deadly: 2 barons, 14 shells".
- **Certainty**: what she's seen is stated; what's inferred says so ("probably opens the red
  door"; "heard, not seen"); what's unknown is `unknown`, never guessed silently.
- **Numbers** she'd read off the status bar are exact (health, ammo); everything else is
  rounded and few.
- **Outcome first**: "Got the red key." "Failed: no way to switch-7 (the bridge is down)."
- **One line an item**, Markdown lists for several, no headings below `##` (the SDK's rule),
  nothing repeated that she's been told and hasn't changed.

### 5.2 The channels

| Channel | When | Silent? | Budget |
|---|---|---|---|
| **Briefing** | once on connecting; a short one each level (the map's name, skill, what she carries in) | silent | ~60 words; ~20 a level |
| **Status** | when it's changed, at most every 20 s (as now); always inside a question | silent | one line |
| **Events** (5.3) | as they happen, coalesced a tic; several in a second become one line | by kind | ~6 notable a minute; over that, the least important are dropped to the status |
| **Answers** | to her looks and orders: the result's message, within seconds, before the doing (SDK) | (results) | lists of 15 |
| **Questions** (4.6) | proposals and confirmations | not silent | one open; a few a minute |

Plus the **fight summary**, a kind of event: when a fight ends (nothing after her for 2 s),
one line: "Fight over (14 s): killed 3 imps, a baron. Lost 45 health, 18 shells, 4 rockets.
Health 31." That one line is what she decides the next minute on, and what she brags or
complains about.

### 5.3 The events, and which are moments

Told **not silent** (worth a reaction), each with a cooldown so none floods:

- **Found**: a key; the exit; a weapon she hasn't; a powerup; a secret ("Secret found! 2/5").
- **Threat**: a boss or heavy first seen ("A cyberdemon. E 900."); an ambush (several at once:
  the fight's own trap test); outmatched; an arch-vile.
- **Close calls**: health under 25 and under 10; a death dodged (a rocket's blast missed by
  little; out from a crusher with it low): from the journal's survive moves.
- **Hurt badly at once**: 40+ in a second, with what did it ("Shotgunner point blank: -52").
- **Kills that matter**: a boss or heavy down; the last monster of the level; a multi-kill
  (a barrel chain, a rocket): "Barrel chain: 4 dead."
- **Mishaps** (the funny ones): fell into nukage; crushed; teleported into monsters; blew
  herself up; out of ammo mid-fight; stuck and gave something up.
- **Progress**: an order done or failed (with why); a door opened by a switch she pressed
  ("switch-7 opened the bars N of here"); half and all of kills / items / secrets; level done
  with time and tally against par.
- **Death**: what killed her, where, how the fight had gone ("Killed by a revenant's missile
  at the red door, 3 s after two more teleported in behind").

**Built of the kills and mishaps** (2026-11-11, `control/directed/kills.rs`; what happens
together is one line, told when it's over):

    A baron of Hell is dead.                              (a boss, or 500 health and up)
    2 killed at once: 2 zombiemen, all gibbed.             (deaths within 3 tics: one shot, one blast)
    3 barrels went off in a chain (N 486): killed 1 imp.
    46 barrels went off in a chain (N 1579); it cost you 112 health.
    Your own blast caught you: -26 health.                 (silent under 20)
    Every monster on the level is dead (74/74).
    Gibbed: 1 zombieman.                                   (one alone: silent)

A death is one she knows of (seen, or heard and placed); gibbed (the game's rule: killed by
more than its health again, for the five kinds with the frames) counts only for one in her
sight as it died; a barrel is one near enough to hear (1200). One barrel that kills nothing
and doesn't hurt her isn't told: that's her clearing barrels. Not built: a death dodged,
telefrags, crushers, out of ammo mid-fight, a monster raised again.

**Built of the rest** (2026-11-11, `control/directed/moments.rs`, `kills.rs`):

    MAP06: The Crusher. Skill: Hurt me plenty. You come in with: Health 100, armor 0; ...
    E1M1 done in 5:23: kills 6/6, items 37/37, secrets 3/3. You leave with: Health 95, ...
    Jumping for it: 170 across to area-111 (NE), a 128 drop below.     (then:)
    Made the jump.  /  Missed the jump: came down in area-9, the floor hurts.
    Teleported right into 4 shotgun guys!
    Out of ammo mid-fight: down to your fists.
    A crusher caught you: -42 health. Health 31.
    Raised from the dead: 2 imps.
    Your health is down to 9.                                  (marks at 50, 25, 10)

A jump is told before she runs at it when it's big: 128 or more across, a drop of 64 or more
below, or a floor that hurts below ("a long shot" when the jump's checked to come off less
than 80% of ways it could be made); how it came down is told after a told one, and a miss
always (one off the planned spot but in the same area is made). A level's end is told on its
exit, before the next level's start. Not built: a death dodged (a blast missed by little),
par times.

**The budget**: at most 6 moments a minute reach her as moments; past that, what's not urgent
(her life in danger, a level's end, an action's answer) goes silent (`Report::urgent`).

Told **silent** (to know, not to react to): doors and switches seen; ordinary items; an order
begun; a pause for a fight and its end; routine waits.

Never told: anything she hasn't seen, heard or learned (the automap rule; the realism work
is what makes this safe to promise).

### 5.4 Making the moments good

- **Cause with effect.** An event names what did it: the journal and the damage tally have
  it ("what hurt her and what she knew of it" is already per run).
- **Before and after.** Close calls and deaths carry the second before ("she'd just picked up
  the blue key"), from the last few events kept in a ring.
- **Her part in it.** When what happened followed from her order or policy, it says so ("as
  ordered: rushing through", "policy: saving rockets"), so she can own it or blame the bot.
- **The bot's part.** When the bot overrode or refused her, it always says, with the reason,
  at once ("Not retreating: nowhere out of the baron's sight. Still fighting.").
- **Budgets by quiet.** In a long quiet stretch the budget loosens to small things (a wait, a
  view: "a big room, lava below"), so there's always something; in a fight it tightens to
  threats, close calls and the summary.

### 5.5 The bot's own opinion: rankings and suggestions

The bot always has an answer of its own (rule 1): the explorer weighs every objective, the
fight scores every target and gun. She should be able to lean on that without being led by
it. Three strengths, set by policy `hints` (`off`, `on_stall`, `always`; default `on_stall`):

- **Ranked, always.** Every list comes in the bot's own order, best first, not by distance
  alone, and each line can carry its reason in a few words: "door-3 (N 210): untried, nearest",
  "area-7: 12 to explore, the way the red door's switch probably is". This costs no extra
  words and no extra message; an unranked list makes her guess (stage 0: I had six areas and
  nothing to choose between them by).
- **A pick, when she asks or is asked.** `doom_list`, `doom_look` and every forced decision
  mark one line `(my pick)`, with why. A failed order says what would work instead ("No way
  to the chaingun yet. Likeliest way: area-107, S 600.") rather than only that it failed.
- **A nudge, when she's stalled.** Unprompted suggestions are tied to no progress, and
  escalate, so a good run of her own choices is never talked over:

  | No progress for | The bot |
  |---|---|
  | (never, while her orders are getting somewhere) | says nothing unasked |
  | two orders failed in a row, or 30 s idle | "Suggestion: door-3, N 210 (untried)." Silent: for her to use or not |
  | 60 s, or the same thing tried three times | the suggestion again, not silent, with what's been tried: "Tried switch-2 three times. The bars it opens are W, not here: try area-4." |
  | 3 minutes with nothing new mapped, killed or picked up | a question (4.6): "Stuck? a) I take over till something new turns up (default, 20 s) b) show everything known c) keep going" |
  | the idle policy's limit | takes over as the mode says, and says so |

  "Progress" is the score's own stall measure (something newly mapped, set off, walked into,
  killed, collected), so it means the same here as in testing.
- **Never** a suggestion built on what she hasn't seen: the pick is the explorer's, which
  plans only from what she knows. And when the bot's pick is a guess it says so ("no idea
  what switch-9 does: only thing left untried").
- In a fight the same, lighter: the status line's "fighting" part names the bot's target and
  gun ("on the revenant with the SSG"), which is its ranking in a phrase; a tactic of hers
  that the rules rate badly gets one honest line when it's given ("Rushing a baron with 9
  shells: I'd keep clear."), then is done as ordered.

For the stream this also gives her something to push against: taking the bot's pick, or
pointedly not, is a bit either way.

**Built** (2026-11-11, `control/directed/look.rs`):

- Lists come in the explorer's own order (a door, a switch, a wanted item by its objective's
  place; an area by the best thing left to explore in it), the rest after, nearest first.
- The pick is what she'd do next on her own: the first of the explorer's objectives she has a
  way to, a pickup she takes by herself anyway left out, and never the exit while anything
  else is left (as on her own: leaving is an order's). It's given under the id it's ordered
  by (something to explore by its area, a key by the key) with why in a few words. `doom_list`
  marks its line `(my pick: why)`, and under the areas gives it apart when it's none of them;
  `doom_look` and a forced decision end with it.
- A failed order says what would do: "No known way to the Soulsphere yet. Nearest untried to
  it: area-98 (SW 1731)."; "Nothing to explore in area-65. My pick: door-l467: door not
  tried, blue key in hand (SW 2118)."
- The nudge: with no order for 30 s the pick is suggested, silent; at 60 s once more, not
  silent. `hints always`: as soon as she waits. `hints off`: no pick anywhere (the lists'
  order stays). Set by `doom_policy` (`idle`, `hints`: its first two keys), `hints` in a
  script.
- The test: E1M2 with every suggestion taken as an order and nothing else given (a script,
  `hints always`): 70 picks, 68 carried out (one route failed at a drop, one wall was gone
  by the time it was ordered), out by the exit at 738 s with every monster dead. So the pick
  is always something she can order as it's written, and following it alone finishes a level.

Not built of this section: "the same thing tried three times" and the 3-minute question (they
want proposals, 4.6); the fight's one honest line on a tactic it rates badly; target and gun
in the status line (they're in `doom_look`).

## 6. What the code needs (the boundary)

The cleanup left typed pieces for this; the layer is built on them, in one module tree
(`control/`), with nothing in the fight or the explorer knowing who's asking.

| Piece | Exists | To build |
|---|---|---|
| What she perceives | `Combat::now` (`Assessment`), `known`, `visible`, the explorer's `Knowledge`, `Places` | `Situation`: one snapshot type the status, `doom_look`, questions and events all read, so they can't disagree |
| What the bot decided, and over what | `decisions::Journal` (target, move, retreat, objective, legs, with options and scores) | on always in directed play (a ring, not a file); `doom_why`; the "close call" and "close decision" tests read it |
| Objectives as data | `Objective`, `Kind`, `Ident`; `Refusal`; `PathNote` | `Refusal` and the tiers of `next` surfaced as confirmations (a `Proposal` type: id, options, default, deadline, class, validity check) |
| Her orders | `Order` (go, explore area/way, exit, stop), `rush`, `risky`, `sure` | `Goal`; idle fallback (`auto` after N s) |
| Her say in the fight | `Combat::forced_target`, `Combat::barred` (moves the tree isn't to make), `Tuning::with` (stances), `press_on` | **new seams**: a forced gun (a rule at the top of `best_gun`); a requested retreat (a `RetreatKind` begun from outside, to a place); `hold` and `push` as stances over `Tuning` plus `barred`; `ignore` (a set like `wanted_dead`, the other way); each with an expiry |
| Why the trigger's held | three sources (the node, `baiting`, `HeldFire`), one logged | one `HeldFire` with all the reasons, hers included (`ByOrder`): the open finding in CLEANUP_PLAN.md, done here |
| Policy | `Tuning` (9 groups, ~200 numbers), `--tuning FILE`, the advisor's stances | `Policy`: the fixed settings of 4.4, each mapped onto `Tuning` fields and explorer flags in one table; applied at a tic boundary |
| Events | `Director::event`/`report` (strings, `notable`) | typed `Event` (kind, importance, cooldown key, cause) rendered by one function; the budgeter; the fight summary; the ring for before-and-after |
| The link | `neuro/link.rs`, `session.rs` (context, force, result, reconnect) | deadlines; one open question; answers checked for staleness |
| The game itself | the console's commands (`watch --control`: start, stop, goto, pause), `episode --lives`, `evil_new_game`, `evil_god`, `evil_give`, `Engine::goto`, `Combat::immune`, `--peek` | `doom_game`, `doom_cheat`; the console's switches for what she's allowed; `evil_give` for powerups; a level marked cheated; `restart fresh` |

Ground rules carried over: headless runs and the score never touch any of this (parity with
the layer off is line for line); nothing omniscient crosses the boundary; everything she
reads is short.

## 7. Stages, each with its test

| # | Stage | Test (gate) |
|---|---|---|
| 0 | **Drive what exists by hand**: a level through `watch --headless --directed FILE` in real time, me as Evil, noting every moment I wanted to know or do something I couldn't | notes in this file (section 8); they reorder the stages below |
| 1a | **First, from stage 0's notes**: the idle fallback (`idle` policy, announced); orders queue or say what they replaced, exploring goes on after an errand; short ids with a phrase; "seen, no way yet" as a state; an order's outcome says what came of it; a failure says what would work; lists ranked with a pick | E1M2 driven by hand again at the same pace: out in under 7 minutes, no order lost unsaid, idle under a fifth of the time |
| 1 | **Done** (2026-11-11): the grammar, the budgeter, the fight summary, `doom_look`, the moments, locks; its gate run (the GPT stand-in on MAP07, its whole context read cold, words counted: DECISIONS.md). `Situation` and typed `Event` moved to stage 4 | a recorded level's whole context reads well cold; under the word budget; GPT stand-in's choices no worse |
| 2 | **`doom_why` built** (2026-11-12, from what the bot has worked out that tic, not a journal: `control/directed/why.rs`): the bot's own choice in the second person with its reason and what it was chosen over, the fight and what pushed it, why she isn't shooting, the last action that failed. The journal always on: not needed for it | every "why" asked at 50 random moments of a run has a true, short answer |
| 3 | `doom_mode` / `idle` fallback; `doom_goal` | a level finished with three goals and no orders; no stretch over 25 s with her idle and the bot still |
| 4 | **Built** (2026-11-11, class D): questions (`control/directed/ask.rs`), `doom_answer`, deadlines (8 s, `doom_policy answers`), the force at 3 s; asked: leaving with things left, a way that costs (a hurting floor, a one-way drop), nothing but walls left on instinct, stuck 3 minutes, and on instinct each last resort before it's taken (strafing runs, stuck places, every wall, missed jumps, a hurting floor, the held-back exit). Gate passed (DECISIONS.md). `Situation` and typed events: stage 6 | scripted answers at 2, 8 and 30 s late: each either applied or refused as stale, never applied wrong |
| 1b (was 5) | Tactics: `doom_fight` and its seams, expiries; hurt told by rate, death told with its cause | arenas: each tactic visibly changes the fight and never lowers survival below autopilot's by more than noise when left to expire; MAP07 with the loadout, directed by hand: out alive at least once in six where the bot alone is 0 of 6 |
| 6 | **Built** (2026-11-11): class T questions (the default done at once, 3 s to switch, never forced): a fight turned bad (outmatched, an arch-vile, a boss, health under 25; once a fight), a run-jump whose miss costs. `Situation`, one snapshot a tic (status, look, questions). Gate run with the stand-in (DECISIONS.md). Typed events built after it (`events.rs`) |
| 7 | `doom_policy`; then `doom_style` through the classifier | each setting's effect measured on the score's quarter set; none outside its documented range |
| 8 | The game itself: `doom_game`, `doom_cheat`, the console's switches for what she's allowed and its view of questions and policy | an episode on stream settings, unattended, through a death, a pistol restart, a skill change and each cheat; a disallowed action refused; a cheated level never in a measure |
| 9 | Latency rehearsal: the mock delaying and dropping her answers; Tony and Randy for malformed and random input. **Done** (2026-11-12): her data mended where plain (`neuro/repair.rs`), the rules told again every 10 min or 30,000 characters, an open question asked again on reconnecting, no force while the link's down; every action answered on every screen (Gary's timeouts found the gaps); Randy through Gary, 4 minutes on E1M1: 103 random actions, all answered, no crash, 58 refused each with its reason. Delayed and dropped answers not rehearsed apart: a late answer is refused as stale (stage 4's gate), and she's on instinct after 3 s | no hang, no stale action, context volume steady over an hour |

Measures throughout (the earlier plan's, plus): seconds she's idle; questions asked, answered
in time, defaulted; overrides refused as stale; words of context a minute by channel; notable
events a minute in and out of fights; and the score with the layer off, unchanged.

## 8. Notes from driving it by hand (stage 0)

**E1M2, HMP, pistol start, one life, 2026-11-11.** Me as Evil through `watch --headless
--speed 1 --directed FILE`: an order appended to the file, the reports read back, each round
trip of mine 5 to 30 s (about what she'd take, thinking and talking). **Out in 13:19 of game
time, 23/41 kills, 8/42 items, 2/6 secrets, never under 94 health.** A score run's exit of
this level takes about 4 minutes: most of the rest was her standing waiting on me. What I
ran into, in the order it cost me:

1. **She stood idle 486 of the 799 seconds** (from the reports: an order's end to the next one's start). Every order done left her waiting till I'd read
   it and thought. Nothing told me she was idle but the absence of news. The idle fallback
   (section 3) and the force are the first thing to have; nothing else matters as much.
2. **A second order silently threw away the first.** "go shotgun" then "go switch": she
   pressed the switch and never took the shotgun; "go armor" during an explore dropped the
   explore. Nothing said so. Orders want to queue (do this, then that) or to say "replaced
   exploring area-24", and exploring wants to go on after an errand.
3. **Nothing to choose by.** Six areas, each "N to explore"; three doors, each an id and a
   bearing. I picked by guessing. And the counts misled: "area-34: 1 to explore", then
   "Nothing to explore in area-34". A ranking and a pick (5.5) would have halved my turns.
4. **"Exit found: exit-l873", then `list exits`: none, and `order exit`: "No exit known
   yet", for a minute.** She'd seen it through an opening with no way to it yet. Seen but not
   reachable has to be a state of its own, said the same everywhere ("exit seen E 460, no way
   to it yet").
5. **Failures didn't say what would work.** "No known way to the Chaingun yet; explore near
   it": near it is which area? I never got it. "Nothing to explore in area-0" for an id that
   didn't exist yet (it did later) should have been "no area-0".
6. **Done said nothing of what came of it.** "Done: door-l527" (the red door): did it open,
   what's behind it? "Pressed switch-l313: moves area-89": moves how, and is that a way on?
   Then "Nothing to explore in area-89". The outcome she needs is "opened: a room N with 3
   imps" or "raised the bridge to the exit".
7. **I never knew where she was.** Status has no place in it; an area is "here" in the list
   only while it has something left to explore. `doom_look` (4.1) is the fix.
8. **The moments weren't told.** Two secrets found: nothing. 23 kills: nothing but
   "Monsters: 1 Zombieman (W 58)" as each came into sight, a dozen of them, and "Exploring
   paused while she's fighting" without saying what she was fighting or how it went. No fight
   summary, no health lost. For the stream there was nothing to react to in the whole level
   except "Got the shotgun" and "Got the red keycard".
9. **Ids are a mouthful.** `door-l389`, `item-t212`, `switch-l313`: she'll read these aloud.
   Short ones counted as she meets them (`door-3`), and a phrase with each ("the red door",
   "the switch by the pillar"), cost nothing.
10. **"Walls to search: 3/9/47"** means nothing unread; it's on every explore's end.
11. **A new gun 186 away wasn't taken** till ordered (spare ammo and health are): a gun she
    hasn't got should be a pickup she takes by herself, by default.
12. **The fight never needed me** on this level and skill: see MAP07 below.

**MAP07, HMP, with a full loadout (super shotgun, chaingun, rocket launcher), one life.**
Dead in 112 seconds, 6 of 12 killed. This is the level where the fight is everything, and I
had no say in it at all:

(The bot by itself, same loadout, six seeds: dead all six, in 51 to 124 s with 4 to 10 killed. So
this isn't my idling: the level's past what her rules can do, which makes it the test for
tactics: can a few orders a player would give (fight from the start room's door, rockets on
the mancubi, the megasphere when it's low) get her through where the rules don't?)

13. **No order means anything in a fight.** Go, explore, press, exit: none of them is "fight
    these", "fall back to the start room", "use the rockets". With no order she was "waiting",
    which in an arena is standing in the open defending herself. Tactics (4.3) aren't a
    refinement; on a level like this they're the whole of her play.
14. **I learned she was dying too late to matter.** 100 to 57 in the first twenty seconds:
    nothing told. The first word was "Health down to 33", then "13" four seconds on. By the
    time a model has read "33" and answered, she's dead. The hurt has to be told on the way
    down, by rate ("losing 20 a second to 3 mancubi"), and the bot's own retreat has to be
    the default with her override, not the other way.
15. **What I'd have ordered, in order**: back into the start room and fight from the door
    (`retreat`); rockets on the mancubi (`weapon`); the megasphere when the arachnotrons came
    (`doom_go item rush`, which failed: "no known way yet", though it was the one thing that
    would have saved her and I couldn't tell why not).
16. **The level's own event went untold**: the last mancubus dying lowered the walls and let
    the arachnotrons out. What I got was four lines of "Monsters: 1 Arachnotron" at four
    bearings. The moment is "the walls are coming down: arachnotrons, 4 of them, all round".
17. **Her death wasn't reported to the orders' side at all** (the last line is "Health down
    to 18"), and with one life the game ended there. Death needs its line (what, where, how
    the fight had gone) and the restart question.
18. **"picking up nearby"** was her status through the whole fight. And ten backpacks were
    ten lines of the items list.

**MAP07 again, with the first tactics built** (`fight weapon`, `fight target`, `fight
hold_fire`, stances; each with its expiry). "fight weapon rocket 60" before pressing the
switch: **the four mancubi dead in 19 s for 27 health** (without it, on the same level: 40 s,
and into the arachnotrons at 57). So one order of the kind a player gives did change the
fight. Then I lost her to the arachnotrons at 64 s all the same, and how is the lesson:

19. **My own order killed her.** "fight weapon ssg 60" against arachnotrons took her in to
    the super shotgun's range of plasma spiders, in the open. The bot did as told. It needs the
    one honest line of 5.5 when a tactic's rated badly ("SSG on arachnotrons means closing to
    100 in the open: I'd keep back"), and then to do it.
20. **There's still no way to say "get behind something"**. The megasphere and the start room
    were both "no known way" or not what I meant; `go AREA rush` isn't a retreat. A retreat
    she can order (to cover, to a place) is the next seam; it's the one that would have
    mattered here.
21. **A turn of mine lost to a tooling slip was 25 s of her fighting unwatched**, and that's
    how she died. For Evil the same will happen whenever she's talking to chat: the fight's
    defaults have to carry her through a turn she misses. A tactic running out mid-fight goes
    back to the rules (it did); a stance she set stays its time.

**Episodes 1 and 2, HMP, pistol start, directed by hand (2026-11-11).** Both finished, every
level out by its exit, with the idle fallback at 6 s doing most of the playing and me giving
the orders a commander would: leave when the exit's known, a stance or a gun for a hard
fight, a nudge when she'd stalled.

| | Levels | Game time | Deaths | My orders |
|---|---|---|---|---|
| Episode 1 | E1M1 to E1M8 | about 63 minutes | 1 (E1M6: four spectres and two imps at arm's length) | an exit each level; `fight careful` for E1M6's end the second time; `fight weapon rocket` on E1M8, which was a mistake (below) |
| Episode 2 | E2M1 to E2M8 | about 110 minutes | 1 (E2M8, the Cyberdemon, in 35 s) | an exit each level; areas to explore when E2M4 stalled; `fight weapon rocket` on the Cyberdemon the second time: dead in 157 s, her health never under 200 |

Not one unbroken run each: three times a fault of the layer's ended the game and I started
it again at that level with what she'd carried in (given with `--carry`): the two deaths
(`watch --episode` was one life whatever `--lives` said), and E2M4, where on her own she
stopped for good with no exit known. All three are fixed below. What it showed:

22. **The idle fallback is most of the game.** With it she's never standing; my part was a
    dozen orders an episode. That's the right default for a stream: Evil can talk to chat for
    two minutes and the level goes on.
23. **On her own she won't leave, and the exits were hidden with that**: I had to stop her to
    learn an exit was known. An exit seen should be told and listed at once, held or not; "may
    I leave" is the question, not "is there an exit".
24. **On her own she stopped when only walls were left, even with no exit known** (E2M4, 20
    minutes in): now she keeps at it, last resorts and all, unless an exit's known.
25. **Stopping her to ask something costs her what she was doing** (a timed lift, a route
    half walked). Looks (`list`, `status`) must never need a stop.
26. **My two worst orders were tactics.** "Rockets" on E1M8 with demons at arm's length took
    her from 122 to 18 in four seconds (and the level then ended on its own floor, luckily);
    the same order on the Cyberdemon at range won the fight untouched. The gun order needs the
    bot's own safety on it (it has the launcher's "not into something near" rule; the blast
    of a rocket at a demon in her face got past it), and the one honest line of 5.5.
27. **The orders file was read from its start again on each level** (so "order exit sure" was
    given again at the top of the next level) and Neuro's link was made afresh: fixed, the
    director goes on to the next level with her.
28. **What I was told was right for deciding and thin for a stream**: keys, guns, secrets,
    fast losses, deaths. A level's end said nothing (time, tally against par, what she
    carries out), nor a level's start (its name).

29. **The pick followed blindly finishes a level** (E1M2, 738 s, section 5.5), and reads well
    as a to-do: the door with the key in hand, the armour wanted, then the next room. Where
    it's dull is the tail: wall after wall "to search" once the rooms are done. That's when
    leaving should be offered (the exit check's question, as a proposal), not a sixth wall.

What this changes in the stages: the idle fallback and order queueing move to the front
(with the grammar: ids, "seen, no way yet", outcomes); rankings and the pick come with them
rather than later; the events for moments (secrets, kills, the fight summary) are stage 1 as
planned. After MAP07: tactics move up to straight after 1a, with the telling of hurt by rate and of death; a level like that is unplayable for her without them, and class T questions (the bot retreats by default, she overrides) come with them rather than two stages later.

## 9. Decided by the user (2026-11-11)

1. **With no action from her she waits**: stays where she is, looks round now and then, fights
   what comes at her, and doesn't go anywhere. Not `auto` by default (`doom_policy idle` is
   still hers to set). Built: a quarter turn one way every 4 s, the next the other way; the
   pickups she takes waiting only within 256 along the way (was 1024).
   **Changed (the user's, 2026-11-11, after watching a model play through Gary):** with no
   action for **6 s** she carries on by instinct (`idle 6` the default, announced; any action
   of hers takes over; `idle 0` has her wait as above). The bot stood still 10-20 s at a time
   while a model looked things up between short orders: dead air on stream.
2. **She's fast**: 100-150 ms at best, 250 ms on average; 2-3 s when busy or thinking, rarely
   5. So the deadlines (4.6) are: **class T, 3 s** (the bot's default acts first and an answer
   in 3 s overrides it); **class D, 8 s** (covers all but the rare slow answer; the default
   then, said). The 15 s and 6 s guesses are gone.
3. **Sixteen actions is fine.** No folding.
4. **`doom_style` free text needs a Jev-like model**: a plan of its own after this one is
   mostly done, maybe an experimental mode. Out of this plan's stages.
5. **Meta control**:
   - **restart** is hers, but only unlocked after no progress for a while (the score's stall
     measure: nothing new mapped, killed, set off or picked up); till then `doom_game
     restart` answers why not and how long. Proposed: 90 s; and at once after a death that
     costs a life, as now. **Built** (`control/directed/restart.rs`): 90 s, told her once
     when it's hers; no life spent; what she came in with, what she knows kept.
   - **next level** is hers to give: after an exit the game holds at the tally screen till
     she says `doom_game next`, as a player presses a key there (she's told the tally there).
     **Built** (stream mode, playing on through): held for her word, on by itself after 30 s
     so the stream's never left there.
   - **choose a level** (changed, the user's): yes. She starts the game: the level, the skill,
     and single (just that level) or continuous (on through its episode; Doom II: through
     the game). Offered at the title card and again once what she chose is over (the level,
     or the episode's end, its story told). What's on offer is the console's: by default only
     its own map and skill, single, so often her one choice is to start. **Built**
     (`control/game.rs`, `doom_game start`; the console's "Evil starts the game", "Levels she
     may start", "Skills she may choose", "Play she may choose"; `--choices` for a watch).
   - **skill**: not asked; stays behind the console's switch, off.
   - **cheats**: held off. `doom_cheat` stays planned, not built, till you say.
