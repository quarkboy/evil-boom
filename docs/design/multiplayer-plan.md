# Two players: Evil and Neuro, co-op or deathmatch (a plan)

> **Status:** proposed, nothing built. Written 2026-11-13 from a reading of the engine boundary,
> the bot and the Neuro session as they are now. Section 4 (whose win a deathmatch is) added
> 2026-11-14.

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
  deathmatch). Start apart; share only what they'd tell each other (section 6).
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
- **Deathmatch actions:** what to hunt (her, or items), a stance, and the rest of section 4's
  list: these have to decide the match, or it's two bots and a coin. Frags and taunt-worthy
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

## 4. Deathmatch: whose win is it?

A deathmatch is fighting from start to end, and the fight is the bot's (a model's answer takes
1.5 to 6 s; an exchange between two players is over in under one). Left as it is, a match is
two copies of one bot and a coin: neither twin did anything to win. The other way, no instinct
at all and every step and shot hers, is fair, and no good at the game's speed: each order is
for a fight seconds gone. Funny for two minutes, then two players standing in rocket fire.

**The plan: the same reflexes for both, and choices that decide it.**

- **One instinct, the same for each, and modest.** Aiming, dodging and firing are the bot's,
  with the same numbers on both sides and not too sharp: where she stands and what she holds
  must beat raw aim. Reflexes too good, and whoever sees the other first wins, whatever was
  chosen.
- **What takes seconds is theirs, and there's more of it than in a level:**
  - *Items.* In Doom's deathmatch the gun counts for more than the aim (a super shotgun or a
    rocket launcher against a pistol is seldom close). Which gun, armor or sphere to go for,
    when to keep the other from one, when it's back (altdeath).
  - *Where to fight.* Wait at a doorway, hold the high ground, keep out of an open room,
    break off hurt and come back armed.
  - *Hunting and hiding.* They hear each other, and are told what they hear: "shots at the
    plasma rifle: go round".
  - *Reading the other.* Evil always runs for the rocket launcher; Neuro can be waiting
    there. This is what makes a win hers, and worth watching.
- **Actions to match** (beside what she has: go, a stance, a gun, hold fire): go for an item
  and hold it; wait in ambush at a place, facing a way; hunt (to where she last saw or heard
  her); break off to a place; keep a distance or close in. Told to her: what she hears, where
  the other was last seen and with what, what's respawned that she knows of, the frags.
- **The gate that it works:** two bots under different standing orders must not split even.
  If orders don't move the result, they're not doing enough (or the reflexes do too much):
  measured before any AI plays it (section 5, stage 5).

**The other way, as a mode of its own: a duel by hand.** No instinct: every step, turn and
shot is hers, and the game waits for them. Either slowed far down, or stepped a moment at a
time with an order from each a step (correspondence chess with shotguns). Fair, all theirs,
and its clumsiness is the joke where at full speed it'd be the trouble. Cheap once two
players work: a speed and a small list of actions (step a way, turn to, fire, switch gun).
A segment of a stream, not the match.

## 5. Stages, each with a gate

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
   the actions and moments (section 4). Gate: two bots on a few deathmatch maps, both scoring
   frags, neither stuck; the same two under different standing orders (one for the guns, one
   straight at her; one in ambush, one hunting) not splitting even; then two stand-ins.
6. **The duel by hand** (section 4): the game slowed or stepped, instinct off, the small
   action list. Gate: two stand-ins finish a duel, and it's watchable.

Co-op first: it reuses nearly everything she does now. Deathmatch reuses stages 1, 3 and 4,
and needs a fight she doesn't have yet.

## 6. Open questions

- **How sharp the deathmatch reflexes are:** dull enough that a better gun and a better place
  win, sharp enough to look like play. A number to find with two bots (section 4's gate).
- **The duel by hand: slowed or stepped?** Slowed keeps it moving; stepped is fairer to a slow
  answer. And how much each is told a step without filling her context.

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
