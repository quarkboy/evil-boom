#!/usr/bin/env bash
# GPT-Evil playing with no window, at x1: the language-model stand-in for Evil
# (evil-boom neuro-gpt, config/neuro-gpt.json and its .local.json for the key) and the game
# directed by it over the Neuro protocol (evil-boom watch --headless --speed 1 --neuro).
#
# Live logs, to follow while she plays:
#   out/neuro/gpt.log   what the game tells her, what she calls, what comes back
#   out/watch/live.log  the game: her orders, the reports, the bot's own log, with game time
#   e.g.  tail -f out/neuro/gpt.log out/watch/live.log
#
# usage: tools/gpt-evil.sh [WAD MAP [watch options...]]
#   default: doom E1M1 --fight 2 --mortal; e.g. tools/gpt-evil.sh doom E1M1 --fight 2 --mortal --episode
#   PORT=8001 for another port. Ctrl-C stops both.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=out/build/release/evil-boom
port=${PORT:-8000}
wad=${1:-doom}
map=${2:-E1M1}
shift $(($# < 2 ? $# : 2))
if [ $# -eq 0 ]; then
    set -- --fight 2 --mortal
fi
mkdir -p out/neuro out/watch
"$bin" neuro-gpt --port "$port" > out/neuro/gpt.out 2>&1 &
gpt=$!
trap 'kill $gpt 2>/dev/null || true' EXIT
sleep 1
if ! kill -0 "$gpt" 2>/dev/null; then
    echo "neuro-gpt didn't start:" >&2
    cat out/neuro/gpt.out >&2
    exit 1
fi
echo "GPT-Evil on port $port; logs: tail -f out/neuro/gpt.log out/watch/live.log"
"$bin" watch "$wad" "$map" --headless --speed 1 --neuro "ws://127.0.0.1:$port" "$@"
