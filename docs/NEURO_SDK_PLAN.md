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
| **Cheats** | S | `doom_cheat` (4.7), only when the streamer's switched them on | off | below |
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
  a) take it   b) finish up here first (default, in 15 s)   c) stop
```

- An **id**, the question in a line, two to four **options with keys**, which is the
  **default**, and **when** it's taken.
- **Class D** (the bot can wait): she holds somewhere safe; the question goes as context (not
  silent); if she hasn't acted in a few seconds it's an `actions/force` with `doom_answer` and
  the orders that would also resolve it, state ephemeral. At the deadline the default's taken
  and said ("No answer: finishing up here first."). Deadline from policy: 15 s on stream by
  default, `wait` to hold for her.
- **Class T** (it can't wait): the bot **does its default at once** and says what, with the
  alternative and how long it's open: "[ask q18] Archvile, 600 E. Going for cover (default).
  a) cover b) rush it c) keep on to the exit. Open 6 s." An answer in time switches; after,
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
- **`doom_cheat code`**: the codes of 2.4's table, by their Doom names (she'll know them; so
  will chat). **Registered only while the streamer has cheats switched on** in the console,
  and unregistered when they're off: with registration the SDK's own way of saying what's
  possible, a cheat she isn't offered is one she can't be talked into by chat.
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
| 1 | `Situation`, typed `Event`, the grammar, the budgeter, the fight summary, `doom_look` | a recorded level's whole context reads well cold; under the word budget; GPT stand-in's choices no worse |
| 2 | The journal always on; `doom_why`; the trigger's one reason | every "why" asked at 50 random moments of a run has a true, short answer |
| 3 | `doom_mode` / `idle` fallback; `doom_goal` | a level finished with three goals and no orders; no stretch over 25 s with her idle and the bot still |
| 4 | Proposals: the type, class D confirmations from `Refusal` and the tiers, deadlines, `doom_answer` | scripted answers at 2, 8 and 30 s late: each either applied or refused as stale, never applied wrong |
| 5 | Tactics: `doom_fight` and its seams, expiries | arenas: each tactic visibly changes the fight and never lowers survival below autopilot's by more than noise when left to expire |
| 6 | Class T proposals (act now, override in time); the "close or high-stake" filter tuned | a hard level with the stand-in: questions per minute within budget, share answered in time, outcomes |
| 7 | `doom_policy`; then `doom_style` through the classifier | each setting's effect measured on the score's quarter set; none outside its documented range |
| 8 | The game itself: `doom_game`, `doom_cheat`, the console's switches for what she's allowed and its view of questions and policy | an episode on stream settings, unattended, through a death, a pistol restart, a skill change and each cheat; a disallowed action refused; a cheated level never in a measure |
| 9 | Latency rehearsal: the mock delaying and dropping her answers; Tony and Randy for malformed and random input | no hang, no stale action, context volume steady over an hour |

Measures throughout (the earlier plan's, plus): seconds she's idle; questions asked, answered
in time, defaulted; overrides refused as stale; words of context a minute by channel; notable
events a minute in and out of fights; and the score with the layer off, unchanged.

## 8. Notes from driving it by hand (stage 0)

To be filled in as I play.

## 9. For the user to decide

1. **The default mode on stream**: `auto` after 20 s with only irreversible things asked (my
   proposal), or waiting for her as now?
2. **How long she takes to answer** in practice (think plus speech): the deadlines (6 s for
   class T, 15 s for D) are guesses until measured with her.
3. **Sixteen actions**: is that too much standing context for her? The cut, if so: fold
   `doom_look` into `doom_status`, and `doom_goal` into `doom_explore`.
4. **`doom_style` free text**: wanted for the stream's sake ("play like a coward" is a bit),
   or are the fixed settings enough to begin with?
5. **Meta control**: which of restart, level, skill and cheats is hers by default, and which
   only when you switch it on? My proposal: restart and next hers always; level, skill and
   cheats behind the console's switches, off to begin with.
