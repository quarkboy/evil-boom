# Two players: Evil and Neuro, co-op or deathmatch (a plan)

> **Status:** proposed, nothing built. Written 2026-11-13 from a reading of the engine boundary,
> the bot and the Neuro session as they are now.

The goal: one game with two players in it, each played by the bot and directed by her own AI
(Evil on one, Neuro on the other), working together through a level (co-op) or against each
other (deathmatch), on one stream. Two bots with no one directing them should work too: that's
how it's tested, and it's a fallback when one AI isn't there.

## 1. What's single-player now

Everything is built for one player, but most of it is per player already in all but name.

- **The engine boundary** (`dsda/evil_api.c`) uses `consoleplayer` 17 times: one ticcmd slot
  (`evil_tic`), one player's state (`evil_player_state`, `evil_get_status`), god mode, powers,
  warps, gifts, and the view drawn (`displayplayer`). Every one is "the player", never a
  choice.
- **The bot** (`Agent`: knowledge, explorer, navigation, the fight) takes an `Observation` and
  gives a command. Nothing in it is global: two `Agent`s could run side by side already. What
  it doesn't know about is another player: the other one's body comes through `evil_mobjs` as
  a `DoomPlayer` thing, which it has no rule for (not a monster to fight, not an obstacle that
  moves).
- **The Neuro session** (`neuro/session.rs`) is one link to one server, with one director. The
  Neuro SDK has no notion of two characters on one connection: Neuro and Evil each run their
  own server, so a game with both connects twice.
- **The watch, the stream window and the console** draw one view and steer one player.

## 2. Doom's own rules for two players (what the engine already does)

Turned on with `netgame` and `playeringame[1]` in one process: no network needed. DSDA-Doom
already runs a netgame alone (`-solo-net`, `g_game.c`); two players in one process is the same
game with two ticcmds a tic.

**Co-op** (`deathmatch = 0`):
- each player at their own start (the IWADs' maps have co-op starts);
- things flagged multiplayer-only appear (`MTF_NOTSINGLE`: more monsters, more items);
- keys and weapons stay on the floor when picked up, so each can take them (`p_inter.c`);
- a dead player comes back at their start with a pistol, the level carrying on; the level
  isn't restarted;
- one player through the exit ends the level for both;
- players can hurt each other (as in vanilla).

**Deathmatch** (`deathmatch = 1`, or `2` for "altdeath": items respawn, weapons are taken):
- random deathmatch starts; respawn on death; frags; a time or frag limit to end it;
- no monsters usually (`-nomonsters`), or with them for chaos.

## 3. The work, by layer

### The engine boundary

- A player argument on everything that's "the player" now: `evil_tic(player, ...)`,
  `evil_player_state(player, ...)`, `evil_get_status(player, ...)`, god, powers, warp, give,
  ready. The single-player callers pass 0; parity must hold (`tools/parity.sh`).
- `evil_new_game` with the number of players and the mode (co-op, deathmatch 1 or 2): sets
  `netgame`, `playeringame[]`, `deathmatch`, and spawns them.
- Drawing a given player's view: `evil_draw_at(player, ...)`, setting `displayplayer` for the
  frame (the status bar and the weapon follow it). Twice a frame for a split screen: about
  twice the drawing cost, small next to the bot's.
- Sound: Doom places sounds for one listener (`displayplayer`). Pick one (the player the
  stream's audio follows), or mix two listeners' panning. The first is enough to start.
- Who did what: hits and deaths already carry their source; a kill by a player needs which.

### The bot

The bulk of the work, and where the score's lessons still apply: trace before fixing.

- **Two agents** in the watch loop, each with its own observation and command. Knowledge
  could be shared (co-op: what one has seen, both know) or kept apart (realism, and
  deathmatch). Start apart; share only what they'd tell each other (section 4).
- **The other player as a thing:** never a target in co-op; a target in deathmatch; never in
  a line of fire in co-op (the fight's shot already checks what's between; a teammate must
  count as a wall, splash damage included: rockets near her are out).
- **Not in each other's way:** two players in a corridor, a lift taken by one, a door one
  holds open for the other. Movement treats the other as a moving obstacle, as monsters are.
- **Sharing:** in co-op, health and armor go to whoever needs them more (the stock-up rule
  already values what a pickup adds: give each item to the one it adds most to, by what they'd
  tell each other); keys and weapons stay, so both can take them; the exit only when both are
  ready, or one waits by it.
- **Splitting up:** two explorers on one map can cover different ground. The explorer's
  choice weighs what the other is heading for. A "together" setting keeps them close instead.
- **Deathmatch fighting:** another player strafes, picks up the best guns and hunts back:
  unlike any monster. The fight's tree needs a player-opponent style (keep moving, take the
  better weapons first, ambush where items are, rocket splash on the ground). New nodes and
  styles, not cases in a chain (CLAUDE.md).
- **Respawn:** co-op rebirth is a new life mid-level (not the restart she knows): the
  explorer's state carries on, her loadout doesn't.

### The Neuro side

- **Two sessions**, one per character, each with its own link, URL, director, registered
  actions and context. `Session` is already self-contained; the watch holds a list.
- **Who's who:** each told "you" for herself and the other by name ("Neuro is at the blue
  door, health 40"). `control/names.rs` names `DoomPlayer` "you" now: per session, the other
  player by name.
- **New actions** (co-op): `doom_follow` (stay with her), `doom_regroup` (meet at a place),
  `doom_cover` (fight from behind her), `doom_leave` an item for her ("the medkit's
  Neuro's"), and her moves told to the other ("Neuro is going to the red key"). They'll talk
  to each other out loud anyway; the game only needs to carry what voice can't (ids, places).
- **Deathmatch actions:** what to hunt (her, or items), a stance; frags and taunt-worthy
  moments told as they happen ("you fragged Neuro with the BFG").
- **Questions that are both's:** leaving with things left, restarting a level: asked of
  both, the default taken unless either says otherwise.
- **Neither may see the other's context:** no peeking at what the other's been told.

### The stream window and console

- **The picture:** two views side by side (two 960x720 in the 1920x1080 picture, a strip
  below for the column's items), or one big with the other inset. A console setting, live.
- **The column:** both players' health, now, calls; the tally shared; in deathmatch, the
  frags and the time left.
- **The console:** a player count and mode; per player, who directs her (bot, Neuro at a URL,
  GPT, a file) and her name; act or talk to either.

### Testing

- Single-player parity after the boundary change (every caller passing player 0).
- Two bots, no AIs: a co-op sweep (both through the exit? how long? any team kills?) and a
  deathmatch arena (frags each, deaths by the other, stuck time). Its own report, not mixed
  into the score.
- Two GPT stand-ins, then Gary with two characters, before Neuro and Evil themselves.

## 4. Stages, each with a gate

1. **Two players in the engine**, both bots, no Neuro: the boundary's player argument, a
   co-op game started, two agents, two views drawn. Gate: single-player parity; E1M1 co-op,
   both reach the exit without shooting each other.
2. **Co-op play:** teammate in lines of fire and movement, sharing pickups, splitting the
   exploring, the exit together, rebirth. Gate: a co-op sweep over DOOM's E1 and DOOM II's
   first ten maps finishes no worse than one bot alone, with no team kills.
3. **Two directors:** two sessions, names, the teammate told, the co-op actions. Gate: two GPT
   stand-ins finish a level together; then two characters in Gary.
4. **The stream:** the split picture, the column for two, the console's per-player settings.
   Gate: the maintainer's watch of a co-op run on the stream layout.
5. **Deathmatch:** the player-opponent fighting style, deathmatch starts and respawns, frags,
   the actions and moments. Gate: two bots on a few deathmatch maps, both scoring frags,
   neither stuck; then two stand-ins.

Co-op first: it reuses nearly everything she does now. Deathmatch reuses stages 1, 3 and 4,
and needs a fight she doesn't have yet.

## 5. Open questions

- **Shared knowledge in co-op:** what one has seen, does the other know? Realism says no, unless
  told; the stream might be more fun if they share a map. A setting, measured both ways.
- **Who's the camera and the sound?** On a split screen, whose sound plays?
- **One process or two?** One engine with two players is simplest and stays in sync; two games
  networked (DSDA's own netcode) would let each AI's machine run its own, but adds the network.
  One process is the plan.
- **Doom II's Icon of Sin, and other levels she can't finish alone:** two players might, with
  one at the switch and one shooting.
- **The score:** co-op and deathmatch get their own reports; the single-player score stays the
  judge of single-player changes.
