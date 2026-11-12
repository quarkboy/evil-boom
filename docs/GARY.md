# Testing with Gary

[Gary](https://github.com/Govorunb/gary) is a stand-in Neuro server for testing integrations:
a scheduler that makes an engine act on non-silent context, on forces and when idle, context
trimmed as it fills, only registered actions sent, a diagnostics panel for protocol mistakes,
and engines from random (Randy) and by hand (like Tony) to real LLMs through any
OpenAI-compatible service. Closer to Neuro than our own stand-ins: the mock
(`neuro-mock`, actions from a file: the protocol) and the GPT stand-in (`watch --gpt`, our
own loop round a model).

It also serves an MCP endpoint, so Claude Code can drive a session: see what the game
registered, read the model's whole context, wait for results, make the engine act, add
context, send actions. Registered for this project (local scope):
`claude mcp add --transport http gary http://127.0.0.1:8000/mcp`.

## Running it

`tools/neuro/gary.sh [--local] [--watch|--stream] [WAD MAP [watch options]]` starts Gary (and with
`--local`, the local model's server), then the game at real speed connected to it: headless, or
in the watch window (`--watch`), or in the stream's window with Doom's own screens (`--stream`).
Each writes its log to `out/watch/live.log`. Gary runs inside WSL:
WSL's networking here is NAT, so Gary on the Windows side couldn't be reached by the game at
localhost, and its MCP endpoint only takes connections from its own machine.

- Gary: the Linux AppImage in `~/opt/gary`, its server on `127.0.0.1:8000` (Neuro's own port).
- The local model: llama.cpp's server (built with CUDA in WSL), `~/models/Qwen3-8B-Q4_K_M.gguf`
  on `127.0.0.1:8080`, all on the 6 GB GPU (8k context). Thinking off (Gary's reasoning effort
  "auto" sends "none"), a tool call takes 0.7-1 s; thinking on, 25 s. Gary's author: models
  under 20-30B check the path, not behaviour; the stronger test is OpenAI (`gpt-6.1-sol`, the GPT
  stand-in's).

## Gary's settings

- Character: id `evil`, display name `Evil Neuro` (its default is `gary`; our startup handling
  reads who's playing from it).
- Engines: OpenAI-compatible at `https://api.openai.com/v1` (the key typed into Gary by the
  user; Gary never shows it to agents), model `gpt-6.1-sol`, reasoning effort low; and
  OpenAI-compatible at `http://127.0.0.1:8080/v1`, model `local`, for the local model. Response
  method: tool calling (structured output the fallback).
- Its system prompt names the model Gary, and says games may call it Evil.

## What it found, and what's left as it is

- **Gary's diagnostics** (`get_diagnostics`): it wants `additionalProperties: false` on every
  schema; the SDK's specification lists `additionalProperties` among the keywords Neuro
  probably doesn't handle well, so ours follow the specification (a field she gives that isn't
  in the schema is mended to its own name if it's a known alias, else dropped:
  `neuro/repair.rs`). Its action-result timeouts found a real gap: the watch window, its game
  over, answered nothing (fixed: answered that the game's over).
- **Auto-act** (`set_auto_act`): on, the engine is prompted after a few idle seconds as well as on
  non-silent context and forces: closest to Neuro, acting at her own pace. Off, it acts only on
  what the game says and forces: a test of whether our own prompts keep her playing.
- The model's context is trimmed to fit (8k for the local model): `read_events` keeps more
  (the newest 1,000 events), `get_context` is what the model has now.
- **OpenAI's newer models through Gary** (gpt-6.1-sol): Gary speaks only the Chat Completions
  API, and on it these models refuse what Gary sends (`max_tokens`; function tools with a
  reasoning effort, which take the Responses API; strict schemas with optional fields). Rather
  than fork and build Gary (a Tauri app: Node, Rust and WebKit's development libraries),
  `tools/neuro/openai-relay.py` (127.0.0.1:8090; Gary's OpenAI engine's server URL
  `http://127.0.0.1:8090/v1`) takes Gary's chat completions and sends them as Responses API
  requests, giving the answers back as chat completions: tool calling and structured output
  both work, at any reasoning effort. The key in the header is passed on as it is, never
  logged. Gary's OpenAI engine also needs the model's context window set by hand (32000 kept it
  trimmed like hers). The three are worth reporting to Gary's author.
