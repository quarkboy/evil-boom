# A human in the game with Evil: co-op or deathmatch (a plan)

> **Status:** proposed, nothing built. Written 2026-11-13 from a reading of the engine boundary
> (`dsda/evil_api.c`, the patch), DSDA-Doom's source at the pinned commit, the watch loop and
> the bot. Nothing here was measured: what needs measuring says so.

The goal: a person plays Doom in the same game as Evil, beside her (co-op) or against her
(deathmatch), she still directed by her AI or by no one. The person may be at another machine.

This shares its first two stages with [multiplayer-plan.md](multiplayer-plan.md) (Evil and
Neuro, two bots): two players in the engine, and a bot that knows there's another player. What's
different here is the second player's source (a keyboard and mouse, perhaps over a network), and
that a person can't be told a plan, run in a sweep, or made to wait.

## 1. What the reading found

- **The engine takes two players without a network.** `G_Ticker` copies `local_cmds[i]` for
  every `playeringame[i]` (`g_game.c:1571`); there's no consistency check left to fail. A game
  is a netgame when `playeringame[1]` is set. Co-op rebirth and deathmatch respawn are in
  `G_DoReborn` and work with `netgame` on, no level reload.
- **DSDA-Doom has no netcode.** `d_client.c` is a stub (`-solo-net` and `FakeNetUpdate`).
  There's no protocol for a stock source port to join by. Any network is ours to write.
- **The boundary is one player throughout:** 17 uses of `consoleplayer` in `evil_api.c`, one
  ticcmd slot in `evil_tic`. Three of the patch's hooks are worse than that, being *any*
  player or the *listener*, and would go wrong silently with two:
  - hits (`p_inter.c`): recorded for any `target->player`. The human's hits would be hers.
  - pellets (`p_map.c`): counted for any `t1->player`.
  - sounds (`s_sound.c`): `Evil_NoteSound` hears from `GetSoundListener()`, the displayed
    player. If the human's view is the one drawn, she hears with the human's ears.
  - `after_her` in `evil_mobjs`: true when a monster's after any player.
- **The bot can't see another player at all.** `engine.rs:475` drops `DoomPlayer` from the
  things she's given. `control/names.rs` calls `DoomPlayer` "you"; `death.rs` reads a
  `DoomPlayer` source as her own doing.
- **The planner reads the map as single-player:** `map.rs` `in_skill` drops `MTF_NOTSINGLE`
  things, which are there in a netgame (more monsters, weapons, sometimes keys).
- **The window gives presses, not held keys or the mouse** (`evil_event_filter`): enough for
  the watch's keys, not for playing. The engine's own input (`I_StartTic`, `G_BuildTiccmd`) is
  there, with the player's own bindings and mouse settings, and unused.
- **The watch loop can stall a tic:** "don't race to catch up after a slow plan"
  (`viewer.rs`). Alone, a hitch is a hitch. With a person in the game it's their game frozen.
  How long and how often: to be measured (stage 0).
- **Death is a level restart** with lives (`engine.restart`, `agent.restart`), and pause and
  speed belong to the watcher and to her (`doom_pause`). None of these can stand in a shared game.

## 2. The decision: where the human sits

| | how | for | against |
|---|---|---|---|
| A | same process, a second window | least code to a first game | one sound mix for two listeners (the stream needs hers, the person theirs); two views drawn; input for a second window; same machine only |
| **B** | **a second `evil-boom` process, `join`, in step with the host over a socket** | one view, one listener, the engine's own input, per process; same machine and remote are one thing; the stream's process is untouched by the guest's drawing | a small protocol; both sides need the same build and WAD |
| C | a stock port (Chocolate, PrBoom+) joins | nothing for the guest to install but Doom | no netcode in DSDA to answer it; its protocol and exact demo-compatible sync to write. Dropped |

**B.** Doom is built for it: the same game on each machine, only commands exchanged. This
project already depends on the engine replaying exactly (parity, the score). The guest's
process is the engine, a window and the keyboard: no bot, no knowledge, no WAD analysis.

Not the classic peer-to-peer lockstep, where each waits for the other: **the host decides.** The
guest sends its commands as it makes them; the host puts the latest into each tic, never
waiting, and sends back every tic's commands for both players; the guest plays only what the
host sent. A slow guest link costs the guest (their own moves a round trip late, as in 1993),
never the stream. No prediction on the guest's side to begin with: fine on a LAN or a good
link, to be judged by playing (section 6).

A, if ever wanted, is B on one machine (`join 127.0.0.1`).

## 3. The work, by layer

### The engine boundary (`dsda/evil_api.c`, the patch, `dsda.rs`)

- **Which player is hers:** `evil_player` (0 by default), used wherever `consoleplayer` is now
  and in the four hooks above (hits on her only, her pellets, sounds from her body whoever's
  displayed, `after_her` her in particular with `after_other` beside it). Single-player
  callers unchanged: `tools/parity.sh` must hold.
- **A tic with every player's command:** `evil_tic_all(cmds, n)` writing each
  `local_cmds[i]`; `evil_tic` stays as the one-player case of it.
- **A game with players and a mode:** `evil_new_game` gains the count and co-op, deathmatch
  or altdeath; sets `playeringame[]`, `netgame`, `deathmatch` before `G_DeferedInitNew`.
- **The human's command, on the guest:** `evil_build_cmd(out)`: `I_StartTic` and
  `G_BuildTiccmd` into a buffer, the game's window shown and grabbing the mouse (what
  `evil_init` turns off for the host). The guest is `consoleplayer` there, so the status bar,
  messages, weapon choice and sound are the engine's own, for free.
- **A check that both are the same game:** `evil_sync_hash()`: the random index, and each
  player's position and health. Cheap enough for every tic.
- **Players' tallies:** frags, and kills, items and secrets per player (`evil_get_status`
  reads one player's now).
- **Off in a shared game:** god, powers, warp, gifts, the carried loadout set from outside.
  They change the game on one side only.

### The session (new: `native/src/net/`, `evil-boom join`, `watch --host`)

- **Hello:** build id, the WADs' hashes, compatibility level, skill, map, mode, seed, the
  guest's name. Refused with the reason if anything differs.
- **Each tic, host to guest:** the tic, both commands, the hash, and any event (below). Guest
  to host: its command, numbered. TCP with no delay to begin with (a few bytes, 35 a second);
  UDP only if playing shows it's needed.
- **Events that both must do at the same tic:** new game, the move to the next level (the host
  holds `ga_worlddone` when presenting: the guest must hold and release with it), restart,
  pause. All through one list, so nothing reaches the engine on one side only.
- **A hash that differs:** both stop, and each writes its last tics' commands to `out/`, so a
  desync is traced from a recording, not guessed at.
- **The guest leaves or drops:** their player stands still (empty commands); the host carries
  on; rejoining is a later matter (it needs the game's state sent: a savegame).
- **Game rules of the session:** speed fixed at 1; pause (the console's or hers) pauses both
  and says who; no level restart on death (the engine's rebirth instead); `lives` not used.

### The bot

Where the work is, and the same rules as any change to her play: trace, then a general rule.
Each item below is tested with two bots (section 5), not by a person.

- **She sees the other player** as a thing of its own kind (not a monster, not an item): where
  they are, facing, moving, when in view; heard when not (their gun, their pain, doors).
  Realism holds: no position she hasn't seen or heard.
- **Co-op: never shoot them.** A teammate in the line of fire blocks the shot; rockets and the
  BFG not at all with them near the target; barrels near them left alone. In
  `combat/tree.rs` as a node ahead of firing, the distances in `tuning.rs`.
- **Co-op: not in the way.** Players block each other. A moving obstacle for the follower, as
  monsters are; give way in a corridor or doorway, don't stand in one, don't take a lift from
  under them.
- **The world changes without her.** Doors opened, switches thrown, items gone, monsters
  dead or woken by someone else. `knowledge/model/learning.rs` puts a sector's movement down
  to what she pressed: with another presser, that needs a trace first (does she learn false
  causes?). Items gone before she arrives must end the objective, not start a search.
- **Co-op's own item rules:** weapons stay on the floor and are refused when owned: an
  objective for one she has would never finish. Multiplayer-only things are in the level:
  `in_skill` takes the mode. Co-op starts, and the rebirth: a new life mid-level with a
  pistol, her knowledge kept, her loadout not.
- **Sharing and the exit:** health and armor left for the worse-off player when they're near
  and she's fine (a tuning group). The exit ends the level for both: by default she asks
  first ("leave? Sam's elsewhere, 40% of the kills left"), through the questions she has
  (`control/directed/ask.rs`).
- **Standing orders about the person:** on her own she explores as now. Options, offered
  whatever they score: stay with them, lead, split up, wait here.
- **Deathmatch is a different game:** no exit and nothing to explore for. Objectives become
  weapons, armor and the places they come back to (altdeath); the fight is against something
  that strafes, hides and has her guns. A player-opponent style in `combat/style.rs` and
  nodes in the tree: keep moving, splash at the feet, don't duel a better gun, break off to
  arm up. What gun they hold she knows only by its sound and its shots.
- **A fair fight:** her aim is the game's truth turned at once. Against a person that needs a
  reaction time and an aiming error (a `versus` group in `tuning.rs`, a few named levels of
  it), or she's no fun to play. An option of the session, not hidden.
- **Fast enough, always:** a tic must fit in 28 ms with a person waiting on it. Whatever
  stage 0 finds slow (a plan, a route search) gets a budget a tic and carries on next tic, or
  she stands and fights meanwhile. Not a thread: the bot must stay the same on its seed.

### What Evil is told, and her actions (`control/directed/`, `neuro/session.rs`)

- The person has a name, given at `join`; `names.rs` says it, never "you".
- Told, tersely: "Sam joined", "Sam's at the red door, health 40" (when seen), "Sam took the
  plasma rifle", "Sam shot you", "you shot Sam", "Sam died", "Sam left the level";
  deathmatch: "you fragged Sam, 3-2", the time left.
- Actions, co-op: `doom_follow`, `doom_wait`, `doom_split`, `doom_leave` (an item, for them);
  deathmatch: hunt them or arm up. In `doom_game`'s schema only when there's a guest.
- Whether to start a shared game, and which, is the console's and the maintainer's, not hers.

### The stream and the console

- The stream's picture stays hers. Optional: the guest's view as an inset (the host can draw
  any player: it runs the whole game), or a second column item with their health and frags.
- The tally in a netgame is Doom's co-op or deathmatch one: `present` and the `wi_stuff.c`
  patch (`WI_EvilWaiting`) need it checked.
- Console: host a game (mode, the port), who's joined, kick; pause for both.

## 4. Stages, each with a gate

0. **Measure.** The worst and 99th-percentile tic time over the baseline's runs; a list of what
   makes a slow tic. Gate: numbers, and a plan for anything over 28 ms. (No code changes to play.)
1. **Two players in the engine** (shared with the two-bots plan): `evil_player`, the hooks,
   `evil_tic_all`, the new-game mode; the second player standing still, or a second `Agent`.
   Gate: parity alone; in co-op E1M1 the second player's hits, sounds and pellets are not hers.
2. **Host and join, same machine:** the session, the guest's window and input, events, the
   hash. Gate: a person plays E1M1 beside her on loopback; a recorded session replays to the
   same hashes on Linux and Windows builds (that the two builds stay in step is assumed from
   Doom's fixed-point game and demos; this gate is where it's proved).
3. **Co-op play** (shared): the bullets above, by trace, on two bots. Gate: a co-op sweep of
   DOOM's E1 and DOOM II's first ten maps: no team damage, both out, no worse than her alone;
   the single-player score unchanged (`tools/score/step.sh`).
4. **Told and directed:** the name, the lines, the actions. Gate: the GPT stand-in in a co-op
   game with a scripted second player; then Gary.
5. **Over a real network:** the guest remote, lag and loss put on with `tc netem`. Gate: the
   host's tic rate unmoved by a bad link; a judgement of how it plays at 30, 80 and 150 ms.
6. **Deathmatch:** starts, respawn, frags, the versus style and its fairness levels. Gate: two
   bots on a few deathmatch maps, both scoring, neither stuck; then people.
7. **The stream:** inset, tally, console. Gate: the maintainer's watch.

After 2 there's something to play; after 3, something worth playing.

## 5. Testing without a person

A person isn't in a sweep, and a recording of one stops meaning anything the tic the bot does
something new. So:

- **Two bots** are the test bed for everything she does about another player.
- **A scripted second player** (`control/script.rs` kinds of thing: stand in the doorway, walk
  into her line of fire, take the item she's heading for, press that switch) for the traces
  two bots won't produce: a person is ruder than a bot.
- **Recorded sessions** for the session itself: replay both command streams, compare hashes.
- Co-op and deathmatch get their own reports. The single-player score stays the judge of
  single-player play, and every stage must leave it where it was.

## 6. Open questions

- **Who's the guest, and where?** The maintainer on the same network needs nothing more than
  stage 2. A streamer across an ocean may need the guest to predict its own moves, which is a
  larger thing (the guest's game run ahead and rolled back). Decide after stage 5's judgement.
- **Friendly fire in co-op:** vanilla has it. Off would need a patch to `P_DamageMobj` on both
  sides. Vanilla first.
- **How good should she be in deathmatch?** The default level of `versus` is a matter of
  taste and of the stream, not of a score.
- **More than one guest:** the engine takes eight players and nothing above is two-only except
  the bot's "the other player". Not planned for, not ruled out.
- **Joining mid-level, and rejoining:** needs the state sent. Later.
- **Evil, Neuro and a person together:** the two plans meet at stage 1; the session's command
  list doesn't care who made a command.
