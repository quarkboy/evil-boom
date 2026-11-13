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
- **Instinct when idle.** With no action from her for 3 seconds she carries on by instinct
  (exploring new ground first, as her `explore` setting says). An action of hers takes over at
  once. `doom_policy idle 0` makes her wait instead.
- **She's told the game as a player.** Everything she reads is in the second person and short
  ("You picked up a shotgun"; "A cacodemon is down"). It covers moments worth reacting to (big
  kills, barrel chains, close calls, secrets, levels done), at most a few a minute beyond the
  urgent ones, and the bot's suggestions when she asks or is waiting.
- **Questions with defaults.** When the bot faces a choice that's hers (leave with things left,
  take a costly way, a fight turning bad) it asks with a default and a clock. No answer in time,
  and the default is taken. Urgent ones are done at once, with a few seconds to change them.
- **The game is hers too.** She starts the game (level, skill, one level or straight on), moves
  on from the tally screen, and restarts a level that's going nowhere.

## The actions

| action | what it does |
|---|---|
| `doom_look` | what's round you: the room, what it leads to, monsters, items, doors and switches in sight, sounds, a suggestion |
| `doom_list` | known areas, doors, switches, items, monsters or exits, with ids, likeliest first |
| `doom_inspect` | what something is and where |
| `doom_status` | health, armor, weapons, ammo, keys, the tally, what you're doing and fighting |
| `doom_why` | why you're doing what you're doing: the aim and what it was chosen over, the fight, why you're not shooting, the last action that failed |
| `doom_go` | go to an area, item, door, switch or exit (`risky`, `sure`, `rush`, `then`) |
| `doom_explore` | explore an area or a direction |
| `doom_search` | search an area or a direction for secrets (thorough 2 or 3) |
| `doom_press` | press a switch |
| `doom_exit` | leave the level; with things left she's asked first (`sure: true` doesn't ask). Going on to another level with health, armor or ammo she's seen and could still take with her, the question names them and its default is to stock up first (the supplies goal) |
| `doom_stop` | stop and wait |
| `doom_fight` | your say in the fight for a while: a target, a weapon, hold fire, a stance |
| `doom_goal` | a standing aim worked toward whenever no action is in hand: out, everything, a key, kills, supplies, secrets |
| `doom_policy` | standing settings: stance, save, risk, powerups, explore, idle, hints, answers |
| `doom_answer` | answer one of the bot's questions |
| `doom_game` | the game itself: start, next level, restart |

Each action's full schema and description are in `native/src/neuro/session.rs`. What each one
does is in `native/src/control/directed/`. Arguments a model gets slightly wrong (a field's alias,
a stray field) are mended against the schema (`neuro/repair.rs`).

### Settings (`doom_policy`)

- **stance:** `careful` (the default: backs off sooner), `steady` (stands and fights more),
  `bold` (fights more still, finishes fewer levels), `avoid` (keeps clear of fights), `yolo`
  (fights everything head on; asked first).
- **save:** `cells` (plasma and BFG kept for bosses), `none`.
- **risk:** `careful`, `normal`, `bold` (floors that hurt).
- **powerups:** `need` (spheres and invulnerability when they'd help), `now` (as they come).
- **explore:** `new` (new ground first, back over what's done after), `thorough` (everything as
  it comes, secrets too), `quick` (new ground only, never back).
- **idle** seconds before instinct, **hints** (`off`, `on_stall`, `always`), **answers** (seconds
  a question waits before its default).

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
(`doom_explore {"target": "north"}`, `doom_exit`, or `wait-for TEXT` to hold the rest until the game
says something with TEXT in it), and logs everything the game sends to `FILE.log`.

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

Gary's diagnostics ask for `additionalProperties: false` on every schema. Ours follow the SDK's
specification instead, which lists `additionalProperties` among the keywords Neuro may not
handle well, and mend or drop unknown fields.

## The design

How the control layer was planned, and the notes from directing levels by hand, are in
[design/neuro-sdk-plan.md](design/neuro-sdk-plan.md).
