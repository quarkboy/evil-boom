# Evil DOOM

A DOOM integration for Evil Neuro (and Neuro-sama) using the
[Neuro SDK](https://github.com/VedalAI/neuro-sdk). Evil gives the orders; a fast classifier and
a deterministic bot carry them out in real time.

See [docs/DECISIONS.md](docs/DECISIONS.md) for the architecture and decision log.

## Layout

```
src/evildoom/
  wad/        WAD + map lump parsing, line/sector special classification (engine-independent)
  nav/        navigation graph and key-gated reachability built from parsed maps
  knowledge/  Evil's knowledge base: what she has seen, pruned into relevant context
  engine/     engine adapters (ViZDoom first; a modern MBF21 port later)
  bridge/     Neuro SDK websocket client, action registration and handling
  tactical/   fast classifier layer (Jev-style decisions conditioned on Evil's intent)
  motor/      35 Hz deterministic control: aiming, movement, path following
spikes/       throwaway experiments
tests/
docs/
```

## Setup

```sh
python -m venv .venv
.venv/Scripts/python -m pip install -e ".[dev,vizdoom]"
```

WADs are not included. By default the tools look in the Steam install
(`C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom`); override with the
`EVILDOOM_DOOM_DIR` environment variable.

## Spikes

```sh
.venv/Scripts/python spikes/wad_spike.py doom E1M1      # summary + out/E1M1.png
.venv/Scripts/python spikes/nav_spike.py doom E1M3      # key stages + walkthrough + out/*_nav.png
.venv/Scripts/python spikes/vizdoom_probe.py <iwad> <map> [pwad]
```
