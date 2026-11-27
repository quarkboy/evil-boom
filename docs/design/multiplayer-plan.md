# Two players: Evil and Neuro, co-op or deathmatch (a plan)

> **Status:** stage 1 built (2026-11-25), for up to four bots: see "Stage 1 as built" below.
> Written 2026-11-13 from a reading of the engine boundary, the bot and the Neuro session as
> they are now. Section 4 (whose win a deathmatch is) added 2026-11-14.

## Stage 1 as built (2026-11-25)

`evil-boom watch WAD MAP --players N` (2 to 4; `viewer/coop.rs`): one game, N agents, nothing
shared between them, none aware of the others (their bodies aren't among her things).

- **The boundary** (`EVIL_ABI` 2): `evil_players(n)` for the games begun after it (netgame on
  through `solo_net`, which is what `G_DoNewGame` sets netgame from); `evil_as(i)`, the player
  every call about "the player" is about (0 unless told: single-player is unchanged, parity
  24/24); `evil_cmd(i, ...)` stages another player's command before `evil_tic`, and a player
  given none stands still; `evil_draw_as(i, ...)` draws a player's view with their status bar
  (`ST_EvilView` in the patch: their numbers, arms and keys; the face straight ahead as hurt as
  they are, its moods worked out only for player 0).
- **The patch's hooks, by player:** hits carry the player hit; pellets, messages and the
  sounds' loudness and side are kept for each player (a sound with no origin, her pickups, heard
  only by the listener, as the game plays it). `after_her` is the player asked about.
- **Two traps found:** `G_DoNewGame` calls `G_ReloadDefaults`, which takes every player but
  the first out of the game (a hook, `evil_players_again`, puts them back); and a player added
  with no body is marked to be born, which in a netgame the next tic's reborns do before the new
  game loads the level (a crash: they're held live until `G_InitNew` marks them itself).
- **`Engine`** keeps each player's suits, message and hits (`as_player` swaps them in);
  `step_all` plays a tic with every player's controls; god mode and the carry go to all.
- **Death** is Doom's co-op rebirth: a use press a second after dying, the level going on. Her
  head is reset as for a level begun again (`Agent::restart`): heights back to the map's,
  monsters back where first seen. Not right for a level that went on; a `reborn` that keeps the
  world is to come.
- **First runs** (seed 2, mortal, headless): E1M1 UV x4 exited 104 s; MAP01 HMP x3 121 s; E1M3
  UV x2 684 s; MAP07 UV x4 148 s (17 deaths). Most of the damage taken was each other's (MAP07: 70
  of 90 hits): vanilla co-op has friendly fire and DSDA no setting against it. And at the start
  they block each other, staring at each other, till the stuck rules back them out. Both are
  stage 2's first work.

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

## Stage 2, a first pass (2026-11-26)

The maintainer's brief: just for fun, make them slightly less dumb, no sweeps. Measured with
`tools/coop/run.sh` (8 levels, 4 bots, HMP, mortal; out/coop/).

- **Rebirth keeps the world** (`Agent::reborn`): a death in co-op isn't a level begun again. Her
  knowledge of heights, lines set off and what's dead stays; only what she was doing goes.
- **Teammates are known** (`Observation::mates`, as Doom's co-op automap shows every player).
- **Friendly fire** (`Combat::spare_mates`): the trigger let go with a teammate inside the gun's
  spread short of the target, or near where a rocket or the BFG goes off. Vanilla co-op's
  friendly fire stays on (the maintainer: funnier).
- **Giving way** (`Follower::give_way`): stopped half a second with a teammate at her elbow isn't
  stuck (stuck there often, she gave the way up for the level). The higher-numbered steps off
  the other's line of travel, the other goes on; four tries, then stuck as usual.
- **What they tell each other** (`Knowledge::mates_used`): a once-only line a teammate set off is
  used for her too. Told it was used but not what it did, so with nothing else to do she goes
  and sees to those switches herself.
- **Numbers**: before, 8/8 exits, 2600 s, 17 deaths, friendly-fire damage 2857, stuck 1554. After,
  7/8, deaths 13, friendly fire 1238, stuck 1042 (without the switch sharing, fixes2: 8/8, 2625
  s, 8 deaths, 744, 786). The miss is MAP07 seed 7: sector 1, lowered first by switch 162 and then
  raised by the arachnotrons' death, ends at 40, and the mover model waits for 24. Order of
  events; the sharing only changed which run met it. MAP07 on five other seeds exits in 81-97 s.

- **Shared knowledge** (2026-11-26, the maintainer's call, settling section 6's open question):
  what one comes to know of the level all know, every tic (`Knowledge::share_from`): lines,
  sectors, where's been walked, heights (the freshest sighting wins), what lines do, teleports,
  walls tried, locked doors, floors that hurt or end the level, traps. What's of her own doing
  stays hers (what she pressed, doubts, sounds, where she got stuck). `tools/coop/run.sh`: 7/8
  exits, 5906 s -> 8/8, 1934 s; stuck 1042 -> 450; deaths 13 both; friendly-fire damage 1238 ->
  1476. MAP07 seed 7 exits (75 s): what switch 162 did is known now.

## Deathmatch, a first version (2026-11-26)

`watch WAD MAP --players N --deathmatch` (`--altdeath` for Doom's second kind), monsters in with
`--fight N` or none. Over at `--frags N` (20) or `--minutes M` (10), either 0 for none: the level
is ended there, which brings up Doom's tally. Same window as co-op, each card with the frags; the
status bars show Doom's own FRAG box (`ST_EvilView` counts it for the player drawn).

- **The boundary** (`EVIL_ABI` 3): `evil_mode` (0 co-op, 1 deathmatch, 2 altdeath, restored after
  `G_ReloadDefaults` with the players), `evil_frags` (the game's frag table), and `mine` on each
  thing: her own body, or a missile she fired.
- **The others are opponents:** another player alive is a thing named `Opponent` (in the
  bestiary: a player's size and health, as dangerous as a chaingunner), seen and fought as a
  monster is, by what she sees and hears. Their missiles aren't hers (`engine::OTHER_PLAYERS` as
  the owner): dodged. In co-op a teammate's missile stays taken as hers, as before: counted as
  hostile there, the fight turned on where it came from (co-op test: friendly fire 1226 -> 1888).
- **Nothing shared,** no claims, no teammates; the exit kept back (it would end the match). With
  nothing to do for 3 s she walks the level round again (what she's walked forgotten).
- **First matches** (headless): MAP01 4 players no monsters, 10-9-7-3 in 121 s; with UV monsters,
  10-8-8-5 in 109 s; E1M1 2 players, 10-8 in 420 s; MAP07 3 players altdeath, 10-9-7 in 345 s.
  The co-op test is unchanged by it (main-a796-1126 = dm-coop2-1126, run for run).

## Neuro in co-op, a first pass (2026-11-26)

Each player is directed by her own Neuro (`--players N --neuro URL`: ports counted up from
URL's, or a URL each), tested against a Gary each (`tools/neuro/garys.sh N`, GLM 5.3 in all).
As section 3 planned, `Session` is self-contained, so the co-op watch holds one per seat and
keeps it from level to level. What was new:

- **The director's `team`** (`control/directed/team.rs`): the others as she'd know them,
  given by the watch each tic. That's a line in her status (where each is, area id, bearing and
  distance, health, fighting), moments for a death or a key taken, and her own rebirth told.
  The brief names her ("player 1 (green) of 2").
- **Not yet:** the co-op actions (`doom_follow`, `doom_regroup`, `doom_cover`, an item left
  for another), questions that are everyone's, and a switch another hit, told her. The bot
  already shares what each learns; the model doesn't hear of it. Deathmatch with Neuro is
  refused for now.
- **Pacing:** headless with Neuro runs at real speed (it's on the model's clock); the window
  always has.
- **Small lifts, one at a time (0e57162):** a lift under 128 on its longer side with another
  player on it is waited for off its edge (28 clear, 20 s at most). A box over a lift's edge
  under a lower ceiling stops the lift rising at all (Plutonia MAP06's sector 146: all four
  stuck behind two on it). Doors aren't lifts here: a wait in a doorway bought nothing.
- **The console (2026-11-26):** a Multiplayer tab sets players, co-op or deathmatch and its
  limits; co-op takes `--control` now (`viewer/coop/console.rs`): pause, speed, sound, the
  picture, god mode, the exits, and what's said and set to every Neuro. Restart level and Go to
  level aren't there yet (each needs every seat started afresh, as `next_level` does). The Evil
  panel stays empty: there are four of her; the players table is what's shown instead.
- **A person playing (`--human`, 2026-11-26):** player 1, at the co-op window's keyboard and
  mouse (`viewer/coop/human.rs`; `evil_window_input` in `evil_api.c` reads SDL's key state and
  relative mouse). Player 1 because the game's sound and its own view are the console player's;
  the Neuros direct players 2 on (`neuro_from`). Their view is the window's left 1200x900, the
  others' down the side. Each Neuro is told "Player 1 (green) is a person, playing with you."
  Their own window, for OBS to take the co-op window apart, is next (SDL can have two; the
  `evil_window_*` functions keep one).
- **Deathmatch hunting (2026-11-27, `tactical/rivals.rs`):** gear up (a gun past the pistol with
  20 shells / 100 bullets / 5 rockets / 40 cells, health 60 or 40 with 50 armor), then hunt by
  what's seen and heard: sounds only a player makes (super shotgun, chainsaw, fist, BFG, a
  player's grunts and death), and with no monsters a pistol, shotgun, rocket, plasma, teleport,
  or a door or lift she didn't set going; placed by loudness and ear, the mirror one dropped if
  in view. Back to gearing up under 40 health or with only the pistol. No automap of the others
  (Doom's deathmatch shows none). `--hunt N` sets one bot against the old one. Measured on
  five deathmatch maps, 8 seeds: frags a minute both hunting 2.32 -> 2.86; head to head the
  hunter +0.47 a match (s.e. 0.68). Ambushing ruled out (the maintainer: cheap, no fun on
  stream). Open: hunts route through hurting floors (MAP11's slime), costing the health that
  ends the hunt.
- **Deathmatch 2.0's returns (2026-11-27):** what the level put down comes back 30 s after it's
  taken (not the invulnerability or invisibility spheres, not what's dropped). She knew of a
  thing come back the moment it did, wherever she was (her memory of things is by name and
  spot, and the new one matched): now one she's seen gone is gone to her till she sees it back,
  and once its 30 s are up she goes to see whether it's there when she needs it (health hurt,
  armor short, the big ones whenever). Against the old knowledge, 80 matches seat for seat:
  -0.68 frags a match (s.e. 0.88): the knowledge she shouldn't have had bought nothing.
- **Spraying and styles (2026-11-27):** a rival dodges, so rockets and plasma at one are spread
  across where they could be by the shot's landing (`deathmatch.spray`, 0.5 of a runner's
  ground in the flight): rockets the middle, then either side, then half way; plasma swept;
  fired as each comes round, rounds not given up on for missing. Seat for seat against aimed
  shots: +0.38 a match over five levels (s.e. 0.56), but on MAP07, where rockets and plasma are
  most of it, +3.59 (s.e. 1.01, 32 matches; at 1.0 wide +2.62). Styles (`tactical/styles.rs`,
  numbers over the tuning as stances are), most aggressive first: hunting relentless, eager,
  steady (the tuning's own), careful, passive; fighting berserk, aggressive, steady, careful,
  evasive (the stances' standing and falling back, and the spray wide to none). Against
  steady, 40 matches each, none clearly stronger (hunting +0.70, +1.35, -0.65, -1.43; fighting
  -1.35, -0.42, -0.72, -0.33, s.e. about 0.9): they're flavours, for a deathmatch policy and
  Evil's stances later. `--hunt-style`, `--fight-style`.
- **Neuro in deathmatch (2026-11-27):** a Neuro each, instinct off and locked (the maintainer:
  every move hers). `doom_hunt` (hunt | gear) carries the bots' hunting and gearing up as her
  order, clue after clue, looking about with no word (its progress quiet: told aloud, Gary
  ordered the hunt again at each, 34 times in 4 minutes); `doom_policy` fight/hunt styles in
  place of the stance; exit, goal and game not offered; status the score, the clock, the last
  word of a rival; frags and the match's end told as Doom's messages tell every player. NEURO.md
  has the whole of it.
- **Your own window (2026-11-27, `--human-window`):** the person playing has a second SDL window
  (`evil_play_open`/`_show` in evil_api.c, no vsync: the main one keeps the time); its keys and
  wheel come apart (events 4-6) and the keyboard and mouse are read from it. The main window is
  the stream's again, every view or the camera, for OBS; the sound stays the person's whoever's
  on camera (one sound out of the machine: theirs to play by).
  Its size (`--human-window-size`) and full screen (`--human-fullscreen`, `evil_play_fullscreen`,
  the desktop's) are its own; the picture in it is the game's one render size for every player
  (`--render`): a size of its own for one view would mean the engine drawing at two.
