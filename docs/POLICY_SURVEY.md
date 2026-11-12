# Stage 7: her standing settings (`doom_policy`), a survey before measuring

The SDK plan's stage 7 (`NEURO_SDK_PLAN.md` 4.4): settings Evil sets once and that hold
(across levels and deaths, till she changes them), as opposed to `doom_fight`'s say for a
while and an order's for one thing. This is the survey before any scored run: what each
setting would mean, what it would read on, educated guesses at its cost, and a plan to
measure. Nothing here is built yet beyond the three that exist (`idle`, `hints`, `answers`).

## What a setting is for

Two reasons, and every setting should serve at least one:

- **Play**: a real choice with a real trade (survive vs. kill; time vs. thoroughness; ammo
  now vs. ammo for the boss) she can make well or badly.
- **Stream**: something to declare and own ("I'm saving every rocket for the cyberdemon",
  "we're going in bold"), and to be blamed or praised for when it goes wrong or right.

A setting that serves neither (a number she can't judge, a choice with no visible effect) is
context she pays for every message and shouldn't have.

## Two kinds, measured differently

1. **Settings that change how the bot plays** (the fight, the routes, the pickups). They can
   be measured on the score: the bot alone, with the setting's numbers as a `--tuning` file,
   on the quarter set (272 runs, ~4.5 min), then the full set for any that look more than
   about 0.02 off. That's the plan's gate: "each setting's effect measured; none outside its
   documented range".
2. **Settings that change what she's asked or told** (questions, hints, idling). The bot alone
   never asks or idles, so the score can't see them; they're checked with scripted directed
   games, as stages 4 and 6 were.

## The proposals

Values in **bold** are the proposed defaults (as now).

### 1. `stance`: **steady** | bold | careful | avoid  (kind 1)

How readily she fights. `bold` and `careful` exist already as `doom_fight` stances for 30 s;
this makes them standing.

| value | numbers (over the tuning) | guess |
|---|---|---|
| bold | `engage.stand_threat` 3, `engage.low_health` 25, `retreat.health` 30, `guns.outmatched` 45 | kills faster, dies more: -0.02 to -0.05 |
| **steady** | the defaults | 0 |
| careful | `stand_threat` 9, `low_health` 50, `retreat.health` 55, `guns.outmatched` 20 | slower, fewer deaths: -0.01 to +0.01 |
| avoid (new) | careful's, plus `engage.keep_clear` 1024 -> 1536, `errands.danger_near` 640 -> 1024, `engage.leave_be` 15 -> 8 | gets past rather than fights: kill share down, exits maybe up; -0.03 to +0.01 (most unsure) |

Stream: the most legible setting there is. Strong yes.

### 2. `retreat_at`: never | low | **normal** | early  (kind 1)

`retreat.health`: never 0, low 25, normal 40, early 60. **Proposal: fold it into `stance`**
rather than keep it. Every stance already sets `retreat.health`, so two settings would fight
over one number, and "retreat early" is what `careful` means. One knob fewer in her context.

### 3. `save`: none | **bosses** | heavies  (kind 1, a small code change)

What the big guns' ammo (rockets, cells, BFG) is kept for. Now: with a boss seen and still
about, using them on anything else costs ten times (`guns.boss_save`).

| value | rule | guess |
|---|---|---|
| none | `guns.boss_save` 1: spend freely | rockets on imps: -0.01 to +0.01 |
| **bosses** | as now | 0 |
| heavies (new) | the same weighting whenever the target isn't a heavy (500+ health: hell knight and up), boss or not | kept for barons and mancubi; fodder with the shotgun: -0.02 to 0 |

Stream: strong ("not wasting a rocket on you, imp"). Yes.

### 3b. Guns: her choice of weapon, done properly (the user's, 2026-11-11)

Now: `doom_fight weapon NAME` for 10 s (at most 60), used while it has a shot; otherwise the
bot's pick that tic. Built with the gun settings, as one piece:

- **The ordered launcher kept, not flipped away from.** When a rocket isn't safe she keeps the
  launcher in hand and backs off to where it is (as the bot does with its own launcher),
  switching away only with something right on her (in claw reach) or while she's taking hits;
  and once switched away, the other gun kept a couple of seconds before going back. Now a
  monster at the edge of the blast could flip her between guns, a second lost each time (seen
  in the code, not yet in a game). Judged by tracing fights with the launcher ordered: the
  score never orders guns.
- **The plain shotgun hers to choose**, apart from the super shotgun: it's the better gun at
  range (the SSG's pellets spread up and down: an imp on a ledge). Guns are chosen as a player
  presses a number; in Doom II, 3 brings up the super shotgun, and 3 again with it up brings
  the shotgun. So "shotgun" presses 3 till the shotgun's up, and the names stop being one slot.
  (The bot's own choice never takes the plain shotgun once it has the SSG: whether it should,
  at range, is a change to the bot to be scored on its own.)
- **The fist hers only with a berserk pack taken this level**: without, it's no weapon worth
  choosing (refused: "Not without a berserk pack this level"). The chainsaw is its own name.
  (The game's own toggle on 1: the chainsaw first, the fist with berserk and the saw up.)
- **A standing choice**, if wanted: `favour GUN` (used whenever it has a shot and is safe) and
  `save` (above) as settings, so it isn't only for a moment.

### 4. `risk` (the plan's `hurt_floors` and `one_way`, merged): careful | **normal** | bold  (both kinds)

How readily she takes a floor that hurts or a drop she can't come back from.

- **Directed**: what's asked already does this (stage 4's "go anyway / don't"). `careful` would
  ask even for cheap harm; `bold` would not ask for a floor that costs under, say, 20 health.
- **On instinct and the bot**: two parts. `errands.floor_cost` (20: the health a pickup lying
  on a floor that hurts is reckoned to cost) times 2 (careful) or 0.5 (bold), a tuning number.
  And when the explorer's risky pass (one-way drops, floors that hurt) is let in: now only
  with nothing safe left; `bold` would let it in once the safe options are only far ones,
  `careful` only as the last resort after walls. That part is a little new code in `next`.

Guess: a small effect either way, under 0.01; bold may lose a few runs to nukage and gain a
few that were stuck. Stream: modest. Merging two settings into one saves context. Yes,
merged; the tuning part first, the explorer part if it's worth it.

### 5. `barrels`: **clear** | leave  (kind 1)

Now (CLAUDE.md's rule) every barrel she's seen is shot from out of its blasts, with no monster
in sight. `leave` stops going out of her way to clear them (still keeping clear of their
blasts in a fight).

Guess: leave is faster and saves ammo, and costs some deaths to a barrel a monster sets off by
her: -0.01 to -0.02. Stream: weak; barrels are better as moments than as a setting. **Proposal:
drop it**, unless you want it for the bit.

### 6. `infighting`: **use** | ignore  (kind 1)

Setting bosses on each other and baiting heavies (`SettingOn`, `retreat.bait_heavy` 8).
`ignore` turns both off.

Guess: ignore -0.005 to -0.02 (the maps with two bosses: MAP20, E2M8's area). Stream: good
when it happens ("let them fight"), but she can already say it in the moment with `doom_fight`.
**Proposal: drop it**; rare, and better as a moment.

### 7. `pickups`: **all** | needed  (kind 1)

Now she takes spare pickups when it's safe (bonuses, armor, health that won't be wasted, ammo
with room). `needed` takes only what she needs (health when hurt, ammo when short, armor
under 50).

Bot alone: `errands.item_range` (1536) down to a few hundred for spares, needed ones as now.
Guess: needed -0.01 to -0.03 (less armor going into fights), faster levels. Stream: modest
("we don't need it, keep moving"). Keep, low priority.

### 8. `secrets`: **0** | 1 | 2 | 3  (kind 2)

The default for her `doom_explore` orders' `secrets` (now 0 unless she says), and whether
instinct searches walls. Directed only, no score effect. Stream: good ("we're a completionist
now"). Cheap. Yes.

### 9. `ask`: everything | **irreversible** | nothing  (kind 2)

Which of the bot's questions she's asked. `irreversible` is what's built (stages 4 and 6).
`nothing` takes every default without asking: for when she'd rather talk to chat. `everything`
adds questions on close calls (a costly jump, a fight turning bad: already in; plus a route
with a monster on it, a powerup far off).

Stream: real, it's how much she wants to be bothered. Yes, but `everything` only once there's
more to ask.

### 10. `switches`: ask | press  (kind 2)

**Proposal: drop.** She presses switches herself (`doom_press`), and on instinct the bot does.
A question per switch would be noise.

### Existing: `idle`, `hints`, `answers`

Already in `doom_policy`. `idle` is the plan's mode in all but name.

## What it adds up to

From the plan's 13 to 9: `stance`, `save`, `risk`, `pickups`, `secrets`, `ask`, plus the
existing `idle`, `hints`, `answers`. Dropped: `retreat_at` (into stance), `barrels`,
`infighting`, `switches`; `one_way` merged into `risk`.

How she sees them: `doom_policy` with nothing given answers with all of them in one line
("stance steady, save bosses, risk normal, pickups all, secrets 0, ask irreversible, idle
wait, hints on_stall, answers 8 s"); the briefing names any not at its default; a change is
told back and noted in the journal, so "why did I do that" can say "your policy: bold".

## Measuring (kind 1), in order

1. A tuning file per non-default value: stance bold, careful, avoid; save none, heavies (after
   its code); risk careful, bold; pickups needed. Eight files.
2. Each on the quarter set (`out/score/screen.py`), ~4.5 min each, ~36 min in all: run for
   run against the defaults' quarter set.
3. Any more than 0.02 off, or surprising: the full set (17 min each), asked first.
4. What each costs is written beside it, in the schema's words if it's short ("bold: kills
   faster, dies more"), so she chooses knowing.

Kind 2 (`secrets`, `ask`): scripted directed games, a level each, as for stages 4 and 6.

## Decided (the user, 2026-11-11)

- **`retreat_at`**: dropped, folded into `stance`.
- **`save`**: cells matter more than rockets; rockets can be used freely. So the default
  changes: **rockets spent freely, cells and the BFG kept for bosses** (now both are kept once
  a boss is seen). Values: `cells` (the new default), `none` (spend everything). This changes
  the bot alone too, so it's scored on the full set against the baseline, as any change to how
  she plays.
- **`barrels`, `infighting`, `switches`**: dropped.
- **`pickups`**: everything that isn't wasted is always picked up (as now: a stimpack at 99%
  isn't). The setting is only for the big ones, renamed **`powerups`: `need` (default) |
  `now`**, over soulspheres, megaspheres and invulnerability. Invulnerability is already kept
  for a fight that needs it; soulspheres and megaspheres are taken now whenever they'd add
  anything, so `need` is new for them (the user's rule): a **megasphere** when it adds at least
  100 effective health, (200 - health) + (200 - armor), the blue armor's points counted as what
  they'll take for her; a **soulsphere** when it adds at least 50, under 150 health. Both tuning
  numbers (`errands.mega_gain` 100, `errands.soul_gain` 50). Short of that they're left where
  they lie, to come back for. Also a change to the bot alone: scored on the full set.
- **`secrets`**: not a setting. `doom_explore` searches at 1 by default (the likeliest few
  walls; it was 0), and a command of its own, `doom_search area|direction [2|3]` (2 by
  default), for the thorough searches.
- **`ask`**: not hers. Every question is asked, the frequency watched (at most 3 a minute as
  now); if some kind comes too often, that kind's made rarer, not her switch.
- **`stance`** and **`risk`**: not answered; kept as proposed.

So her settings are **`stance`, `save`, `risk`, `powerups`**, with `idle`, `hints`, `answers`
as they are, and the gun work of 3b. New command: `doom_search`.

## Order of work

1. The gun work (3b): the ordered launcher kept, the plain shotgun, the fist with berserk; a
   standing `favour` if wanted. Traced in fights, not scored.
2. `doom_search`, and explore's default of 1. Scripted directed games.
3. The two default changes to the bot (`save cells`, `powerups need`): each on the full set,
   run for run against `out/score/base.tsv`, kept if not worse.
4. `stance` (bold, careful, avoid) and `risk` (careful, bold) as tuning files on the quarter
   set; what each costs written into the schema's words.
5. `doom_policy` taking them, telling them back, the briefing naming any not at its default.

## What was asked of the user (answered above)

1. Fold `retreat_at` into `stance`? (Proposed: yes.)
2. Drop `barrels`, `infighting` and `switches`? (Proposed: yes; `doom_fight` covers the
   moments.)
3. Build `stance avoid` and `save heavies` (each a little new code), or start with what's
   there?
4. Any setting you want for the stream that isn't here.

## Measured (2026-11-11): each value on the quarter set, the bot alone

272 runs (two seeds a level-skill), the defaults 0.464 (102 levels finished). The quarter set's
own noise is about 0.01-0.02 on a broad change; a value that moves few runs reads more exactly.

| value | score | finished | runs moved | told her as |
|---|---|---|---|---|
| stance careful | 0.472 (+0.008, s.e. 0.018) | 108 | 238 | backs off sooner, as good or better |
| powerups now | 0.465 (+0.001, s.e. 0.006) | 104 | 35 | as they come |
| risk careful | 0.464 (0) | 102 | 1 | |
| save none | 0.464 (0) | 102 | 2 | |
| risk bold | 0.458 (-0.007, s.e. 0.004) | 99 | 35 | |
| stance yolo | 0.458 (-0.006, s.e. 0.018) | 98 | 249 | fights everything head on; asked first |
| stance bold | 0.444 (-0.020) | 95 | | fights more, finishes fewer levels |
| stance avoid | 0.421 (-0.043, s.e. 0.019) | 90 | 255 | to get past a fight, not to leave on |

What it says: the stances are real choices with real costs; `avoid` left on all the time costs
most (the user's: it's for a hard room, not a whole level; so it's a `doom_fight` stance for a
while too); `careful` may be as good as the default or better, worth a full set as a default
on its own. `risk` hardly touches the bot alone (only a pickup on a floor that hurts goes by
it); in directed play it's what she's asked about a costly way that matters, so `risk` is
mostly for the explorer part not yet built (when the risky pass is let in). `save none` moved
two runs: bosses are rare in the quarter set. Every value stays hers (the user's: options aren't
kept or dropped by the score).

**Careful as the default, the full set** (the user asked): -0.0059 (s.e. 0.0095), 0.453 against
0.459, 399 levels finished against 402, but more consistent: 26 level-skills beaten on every
run against 20, 55 never against 58; 929 of 1088 runs moved (264 better, 300 worse). No
difference in score beyond noise; the quarter set's +0.008 was noise too. Left to the user.
