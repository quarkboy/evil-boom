#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# GPT-Evil playing with no window, at x1: `evil-boom watch --headless --speed 1 --gpt`, the
# language-model stand-in for Evil (config/neuro-gpt.json and its .local.json for the key) run in
# the same program as the game. (Drop --headless for the window: `evil-boom watch doom E1M1 --gpt`.)
#
# Live logs, to follow while she plays:
#   out/neuro/gpt.log   what the game tells her, what she calls, what comes back
#   out/watch/live.log  the game: her orders, the reports, the bot's own log, with game time
#   e.g.  tail -f out/neuro/gpt.log out/watch/live.log
#
# usage: tools/gpt-evil.sh [WAD MAP [watch options...]]
#   default: doom E1M1 --fight 2 --mortal; e.g. tools/gpt-evil.sh doom E1M1 --fight 2 --mortal --episode
set -euo pipefail
cd "$(dirname "$0")/.."
wad=${1:-doom}
map=${2:-E1M1}
shift $(($# < 2 ? $# : 2))
if [ $# -eq 0 ]; then
    set -- --fight 2 --mortal
fi
echo "logs: tail -f out/neuro/gpt.log out/watch/live.log"
exec out/build/release/evil-boom watch "$wad" "$map" --headless --speed 1 --gpt "$@"
