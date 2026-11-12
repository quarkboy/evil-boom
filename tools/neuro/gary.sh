#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# gary.sh [--local] [--watch|--stream] [WAD MAP [watch options...]]: Evil's integration tested against Gary
# (github.com/Govorunb/gary), a stand-in Neuro server with real LLM engines and an MCP endpoint
# (docs/NEURO.md).
#
# Starts Gary with its server on (127.0.0.1:8000, MCP at /mcp) if it isn't running; with
# --local, llama.cpp's server for a local model too (127.0.0.1:8080, OpenAI-compatible, for
# Gary's "OpenAI-compatible" engine); then, given a WAD and map, the game at real speed connected
# to Gary (`watch WAD MAP --headless --speed 1 --neuro ws://127.0.0.1:8000`), its log in
# out/watch/live.log; with --watch, in the viewer's window instead of headless (to watch it
# play; close the window to stop); with --stream, in the stream's window with Doom's own screens
# (the tally, the next level, the game offered at the end: as on stream). Gary and the model
# server stay up.
#
# Where things are on this machine (set them; the defaults are under $HOME): GARY_APPIMAGE,
# LLAMA_SERVER, LLAMA_MODEL.
set -u
cd "$(dirname "$0")/../.."
GARY_APPIMAGE=${GARY_APPIMAGE:-$HOME/opt/gary/Gary-1.0.4_linux-amd64.AppImage}
LLAMA_SERVER=${LLAMA_SERVER:-$HOME/src/llama.cpp/build/bin/llama-server}
LLAMA_MODEL=${LLAMA_MODEL:-$HOME/models/Qwen3-8B-Q4_K_M.gguf}

local_model=false
headless=--headless
while [ $# -gt 0 ]; do
  case "$1" in
    --local) local_model=true ;;
    --watch) headless="" ;;
    --stream) headless="--stream --present" ;;
    *) break ;;
  esac
  shift
done

# Wait up to $2 tries for a URL to answer (any HTTP status).
up() {
  for _ in $(seq 1 "$2"); do
    [ "$(curl -s -m 2 -o /dev/null -w '%{http_code}' "$1")" != "000" ] && return 0
    timeout 1 tail -f /dev/null
  done
  return 1
}

if ! up http://127.0.0.1:8000/mcp 1; then
  echo "starting Gary..."
  # (no FUSE under WSL: extract and run; X11, WSLg's Wayland windows having no decorations;
  # WebKit's DMA-BUF renderer fails under WSL)
  APPIMAGE_EXTRACT_AND_RUN=1 GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    # (the last session's log kept, by when it ended: a run read after Gary's closed)
    glog="$(dirname "$GARY_APPIMAGE")/gary.log"
    [ -f "$glog" ] && mv "$glog" "${glog%.log}.$(date -r "$glog" +%Y%m%d-%H%M%S).log"
    nohup "$GARY_APPIMAGE" --start-server > "$glog" 2>&1 &
  up http://127.0.0.1:8000/mcp 40 || { echo "Gary's server didn't come up"; exit 1; }
fi
echo "Gary: ws://127.0.0.1:8000, MCP http://127.0.0.1:8000/mcp"

if $local_model; then
  if ! up http://127.0.0.1:8080/health 1; then
    echo "starting the local model ($(basename "$LLAMA_MODEL"))..."
    # (all layers on a 6 GB GPU: an 8k context, the KV cache at 8 bits; --jinja for tool calls)
    nohup "$LLAMA_SERVER" -m "$LLAMA_MODEL" --host 127.0.0.1 --port 8080 -ngl 99 -c 8192 \
      -fa on --cache-type-k q8_0 --cache-type-v q8_0 --jinja --alias local \
      > "$HOME/opt/llama-server.log" 2>&1 &
    up http://127.0.0.1:8080/health 60 || { echo "the model server didn't come up"; exit 1; }
  fi
  echo "local model: http://127.0.0.1:8080/v1 (model \"local\")"
fi

[ $# -ge 2 ] || exit 0
# shellcheck disable=SC2086
exec out/build/release/evil-boom watch "$1" "$2" $headless --speed 1 --neuro ws://127.0.0.1:8000 "${@:3}"
