# The Neuro SDK integration

Evil BOOM lets an AI streamer play DOOM through the
[Neuro SDK](https://github.com/VedalAI/neuro-sdk) (the Neuro-sama SDK). The model gives
orders at its own pace. The bot carries them out at the game's 35 tics a second, and fights by
reflex unless told otherwise. The game connects to the SDK's WebSocket server as any
integration does:

```sh
evil-boom watch doom E1M1 --fight 2 --mortal --neuro ws://127.0.0.1:8000
```

With `--stream --present` it's the stream's window with Doom's own screens. The
[console](CONSOLE.md) starts the game this way for a stream.

## How it plays

- **Time decides the design.** A model's answer takes seconds; a fight takes tics. So the model
  decides where to go and what matters, and the bot decides how: moving, aiming, dodging,
  shooting.
- **Instinct between her actions.** An action of hers done, she carries on by instinct at once
  (exploring new ground first, as her `explore` setting says), and a decision's forced on her:
  what she's taken up, carry on or something else (a question; the same after a fight, and at
  most every 15 s as instinct takes up new things). An action of hers takes over at once.
  `doom_policy instinct off` makes her stand and wait instead.
- **She's told the game as a player.** Everything she reads is in the second person and short
  ("You picked up a shotgun"; "A cacodemon is down"). It covers moments worth reacting to (big
  kills, barrel chains, close calls, secrets, levels done), at most a few a minute beyond the
  urgent ones, and the bot's suggestions when she asks or is waiting.
- **Questions with defaults.** When the bot faces a choice that's hers (leave with things left,
  take a costly way, a fight turning bad) it asks with a default and a clock. No answer in time,
  and the default is taken. Urgent ones are done at once, with a few seconds to change them.
- **Her plans are hers to hear.** What the bot settles by itself at the level's scale is told
  her as it's taken up: the invulnerability fetched before she does again what she died after
  doing (a question: get it, or go without), where she's off to with it, a gun before any key
  on the pistol. Her status says the one in force, and a list says what's been left alone for
  the deaths it cost ("died there once").
- **One force at a time** (the SDK's rule). A decision's forced when she's waited with nothing
  to do (medium), when a question's gone unanswered (high), when a game's hers to start and she
  hasn't (medium), and, quietly (`silent`), to ask what next while she's on by instinct (low,
  and not while she's mid-sentence: `speech_finished` is read). A question that can't wait is
  forced the tic it's asked: high, or critical when her health's going in a fight. A force is
  out till one of its own actions is accepted, or 30 s have passed with none; no other is sent
  meanwhile, and what would have been forced is told aloud instead.
- **Pausing is the stream's.** She has no pause of her own. While the game's paused from the
  window or the console, what she sends is still answered: a look, her status and her settings
  as ever, anything that would move her told the game's paused.
- **The game is hers too.** She starts the game (level, skill, one level or straight on), moves
  on from the tally screen, and restarts a level that's going nowhere.

## The actions

| action | what it does |
|---|---|
| `doom_look` | what's round you: the room, what it leads to, monsters, items, doors and switches in sight, sounds (and the bot's suggestion, unless she's fighting) |
| `doom_query` | what she knows of: `all` (areas, doors, switches, items, monsters or exits, with ids, the first to take first; monsters nearest first; 15 at most, and a cut list says so), or `ref` (one thing: what it is and where); `include_explored` for areas with nothing left |
| `doom_status` | health, armor, weapons, ammo, keys, the tally, what you're doing; in a fight, what against, what the fight's doing, and that she isn't shooting when she isn't and it'll last; a plan of her own in force; the ultra instinct while it's on, with what's left of it |
| `doom_go` | go to an area, item, door, switch or exit (a thing's picked up, pressed or opened); `explore` (none, sweep, secrets, everything) goes over the area, or a compass direction, as she gets there; `risky`, `rush` (to an area: till she's in it), `queue_up` |
| `doom_exit` | leave the level by the nearest exit; with things left she's asked first; no answer and she stays, or leaves if her goal is `out` or her `explore` setting `speedy`. Going on to another level with health, armor or ammo she's seen and could still take with her, the question names them and its default is to stock up first (the supplies goal) |
| `doom_fight` | your say in the fight for a while, any together: `target`, `weapon`, `fire` (hold, free), `override_stance`, `retreat`, `cancel`; none given, what's holding |
| `doom_goal` | two standing aims, neither playing for her: each puts what it's after first in what's suggested and in what she does on instinct. `finish`: when the level's done: `out`, or `kills`, `secrets`, `completionist` (asked if she's sure) and then out; `none`, she's asked. `prefer`: what comes first meanwhile, till she changes it: `health` (with no say while she's at 100 or more), `ammo` (none while what she holds, the BFG left out, would kill a cyberdemon), `hunt`, `unexplored`. How she explores stays her `explore` setting |
| `doom_policy` | standing settings: stance, save, risk, powerups, explore, instinct |
| `doom_answer` | answer one of the bot's questions |
| `doom_game` | the game itself: start, next level, restart; only the moves hers, and the levels, skills and play on offer, in its schema (registered again when the console changes them) |

Each action's full schema and description are in `native/src/neuro/session.rs`. What each one
does is in `native/src/control/directed/`.

Only the actions that make sense where she is are registered (each change slows her next answers
a moment, so it changes only when where she is does): in a level, every one but `doom_game`
(that only where she may restart the level), and `doom_exit` once she's seen an exit in this
life; off a level (the title card, the tally, a finale, game over), `doom_game`, `doom_answer`,
`doom_status`, `doom_policy` and `doom_goal`. A decision forced on her names only registered ones. Arguments a model gets slightly wrong (a field's alias,
a stray field) are mended against the schema (`neuro/repair.rs`).

### Settings (`doom_policy`)

Its description is two sentences; what each value does is what it answers with nothing given
(it would otherwise sit in her context the whole game).

- **stance:** `careful` (the default: backs off sooner), `steady` (stands and fights more),
  `bold` (fights more still, finishes fewer levels), `avoid` (keeps clear of fights), `yolo`
  (fights everything head on; asked first).
- **save:** `cells` (plasma and BFG kept for bosses), `none`.
- **risk:** `careful`, `normal`, `bold` (floors that hurt).
- **powerups:** `need` (spheres and invulnerability when they'd help), `now` (as they come).
- **explore:** `normal` (new ground first, back over what's done after), `methodical` (everything
  as it comes, secrets too: slow), `speedy` (new ground only, never back). A goal doesn't change it.
- **instinct** (`on`, `off`: she waits for each action). The bot's suggestions are always given
  (in a look, a list, a decision forced, and when an order can't be done); they were hers to
  turn off once, and aren't. How long
  a question waits for her answer is the stream's (the console), not hers.

### The clocks (`directed` in a tuning file)

How long she's waited for and how often she's put to are the tuning's `directed` group, in
seconds (`evil-boom tuning` prints them): `idle_force` 6, `ask_force` 3, `next_force` 15 and
`next_silent`, `after_look` 3, `force_timeout` 30, `force_settle` 8,
`speech_stale` 20, `answer_wait` 8,
`now_open` 3, `questions_a_minute` 3, `moments_a_minute` 6, `status_every` 20, `tactic` 10,
`stance` 30. A fast model and a slow stand-in each want their own: `--tuning FILE` with just
that group. The console changes the forcing times, the answer wait and the quiet asking as
it runs.

## Testing without Neuro

### By hand: a directed script

```sh
evil-boom watch doom E1M1 --headless --speed 1 --directed me.txt
```

The game reads orders from `me.txt` as lines are appended (`order go a3`, `fight hold_fire 5`,
`goal key blue`, `look`, `why`, ...), and writes what comes back to `me.out`. The full command
list is at the top of `native/src/control/script.rs`.

### The protocol: a mock server

```sh
evil-boom neuro-mock --port 8000 out/neuro/actions.txt
evil-boom watch doom E1M1 --neuro ws://127.0.0.1:8000
```

A stand-in Neuro server: it sends the actions you append to the file, one a line
(`doom_go {"ref": "north"}`, `doom_exit`, or `wait-for TEXT` to hold the rest until the game
says something with TEXT in it; `speaking` and `spoken` send her `speech_finished`, a part and
the end), and logs everything the game sends to `FILE.log`, each force with its priority and
whether it's silent.

### A language model as Evil

```sh
evil-boom watch doom E1M1 --fight 2 --mortal --gpt
```

Our own loop round OpenAI's Responses API, with a persona: `config/neuro-gpt.json` (the
endpoint, model, reasoning effort, history, persona). The key goes in
`config/neuro-gpt.local.json` (`{"api_key": "..."}`, git-ignored) or the environment variable the
config names. `evil-boom neuro-gpt` runs the same as a Neuro server. Its log is
`out/neuro/gpt.log`. `tools/neuro/gpt-evil.sh` runs it headless at real speed.

### Gary

[Gary](https://github.com/Govorunb/gary) is a stand-in Neuro server built for testing
integrations. It schedules a real model's turns as Neuro would, trims context as it fills, has
diagnostics for protocol mistakes, and serves an MCP endpoint, so a coding assistant can watch
and drive a session. It's the closest test short of Neuro.

```sh
tools/neuro/gary.sh [--local] [--watch|--stream] [WAD MAP [watch options]]
```

starts Gary if it isn't running (and with `--local`, llama.cpp's server for a local model), then
the game connected to it: headless, in the watch window, or in the stream's window. Set
`GARY_APPIMAGE`, `LLAMA_SERVER` and `LLAMA_MODEL` to where they are on your machine.

In Gary's settings: the character's id `evil` (display name *Evil Neuro*), and an
OpenAI-compatible engine (tool calling).

OpenAI's newer models refuse some of what Gary's Chat Completions client sends.
`tools/neuro/openai-relay.py` (on `127.0.0.1:8090`; Gary's engine URL `http://127.0.0.1:8090/v1`)
relays Gary's requests to the Responses API and the answers back. The key is passed through,
never logged. Gary's OpenAI engine also needs the model's context window set by hand.

Gary as released (1.0.6) is from before the SDK's `silent` on a force, and drops a force that
carries it (`prot/invalid_message`). For one, add `--tuning config/gary.json`, which asks what
next aloud (`directed.next_silent` false); a force that isn't silent doesn't send the key. A
Gary whose force schema takes `silent` (one line in its `src/lib/api/v1/spec.ts`) needs
nothing.

### Co-op: a Neuro each

```sh
tools/neuro/garys.sh 2
evil-boom watch doom2 MAP01 --players 2 --fight 3 --mortal --neuro ws://127.0.0.1:8000 --tuning config/gary.json
```

`--players N` with `--neuro URL` gives each player her own Neuro: player 1 on URL's port, player
2 on the next, and so on (or a comma-separated URL for each). Each session is the single-player
one, her own link, director, actions and context. On top of that she's told:

- **who she is in the game** with the rules: "Co-op: you're player 1 (green) of 2. The others
  are players like you; their status is in yours. Your bullets hurt them."
- **where the others are** in her status: "team: player 2 (indigo) in area-14 NW 323, health 100,
  fighting". Area ids are the same in every player's names, since they're built from the map.
- **what they do that a player would say aloud**, as moments: a death, a key taken.
- **her own death and rebirth:** her death as in single-player, then "You're back at your start
  with a pistol. The level's as it was." There's no restart of the level and no `doom_game`. The
  game is the watch's: one level, or `--episode`.

Headless, a co-op game directed by Neuro runs at real speed, and each player's log is written as
it goes (every 5 s). `tools/neuro/garys.sh N` starts a Gary for each player. Player K's profile,
`evil-boom-pK` on port 7999+K, is made on first use as a copy of `evil-boom`'s, with only the
port and the name shown changed (*Evil Neuro K*).

### Deathmatch: a Neuro each

```sh
tools/neuro/garys.sh 2
evil-boom watch doom2 MAP01 --players 2 --deathmatch --mortal --neuro ws://127.0.0.1:8000 --tuning config/gary.json
```

As co-op, a Neuro each, with the deathmatch's own rules, actions and settings:

- **No instinct, ever:** it's off and locked. An action done, she stands and waits; every move is
  hers but the bot's reflexes (moving, fighting, dodging, picking up what she walks over).
- **The rules:** "Deathmatch: you're player 1 (green) of 2; the others are your rivals. Most frags
  wins (to 20 frags or 10 minutes). Killed, you're back at once at a random start with a pistol.
  Weapons stay where they lie... No exit. You hear rivals...; you see them only in sight."
- **`doom_hunt`:** `hunt` goes after the rivals by what she's seen and heard (`tactical/rivals.rs`),
  the freshest clue first, clue after clue, looking about when there's no word, till her next
  action; `gear` takes guns, ammo, armor and health till she's geared up ("Geared up."), looking
  about for some when she knows of none.
- **`doom_policy`:** `fight` (berserk, aggressive, steady, careful, evasive) and `hunt` (relentless,
  eager, steady, careful, passive), the styles of `tactical/styles.rs`, in place of the stance;
  `risk` and `powerups` as ever; no `save`, `explore` or `instinct`. `doom_fight`'s
  `override_stance` takes the fighting styles for a while; `target: rival` shoots a rival first.
- **Not offered:** `doom_exit`, `doom_goal`, `doom_game`.
- **Her status:** her kit without the keys (every player has them all), then the score, best
  first ("frags: player 2 (indigo) 7, you 5 (to 20); 4:12 left"), whether she's geared up, and the
  last word of a rival ("a shotgun NE 600, 3s ago"). No positions of the others: Doom's deathmatch
  shows none.
- **Moments:** each frag, as Doom tells every player, with the score ("You fragged player 2
  (indigo). You lead, 5 to 3."); "In sight: 1 rival"; back at a random start; "Match over: you
  won, with 20 frags." 

Gary's diagnostics ask for `additionalProperties: false` on every schema. Ours follow the SDK's
specification instead, which lists `additionalProperties` among the keywords Neuro may not
handle well, and mend or drop unknown fields.

## The design

How the control layer was planned, and the notes from directing levels by hand, are in
[design/neuro-sdk-plan.md](design/neuro-sdk-plan.md).
