#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# garys.sh N: N copies of Gary (2 to 4), one for each player of a co-op game directed by Neuro
# (`watch WAD MAP --players N --neuro ws://127.0.0.1:8000`: player 1 on 8000, 2 on 8001, ...).
#
# Each is the user's Gary build with a profile of its own (gary-instance.sh): player 1's is
# `evil-boom` (port 8000), player K's `evil-boom-pK` (port 7999+K), made on first use as a copy
# of `evil-boom`'s settings (its engines, its model, its key) with only the port changed. Those
# not up are started, detached; their logs in out/neuro/gary-pK.log. They stay up.
#
# GARY_DIR: the Gary build's checkout (default ~/projects/neuro-scumm/gary).
set -u
cd "$(dirname "$0")/../.."
n=${1:?usage: garys.sh N}
GARY_DIR=${GARY_DIR:-$HOME/projects/neuro-scumm/gary}
profiles="$HOME/.local/share/gary-instances"
store=data/dev.govorunb.gary/localstorage/tauri_localhost_0.localstorage
mkdir -p out/neuro

up() {
  for _ in $(seq 1 "$2"); do
    [ "$(curl -s -m 2 -o /dev/null -w '%{http_code}' "$1")" != "000" ] && return 0
    timeout 1 tail -f /dev/null
  done
  return 1
}

[ -f "$profiles/evil-boom/$store" ] || { echo "no evil-boom Gary profile in $profiles"; exit 1; }
for k in $(seq 1 "$n"); do
  port=$((7999 + k))
  name=evil-boom
  [ "$k" -gt 1 ] && name=evil-boom-p$k
  if [ ! -f "$profiles/$name/$store" ]; then
    echo "making Gary profile $name (port $port) from evil-boom's"
    mkdir -p "$(dirname "$profiles/$name/$store")"
    # (a copy as sqlite makes it, the write-ahead log in; then only the port and the name shown
    # changed: localStorage keeps its values as UTF-16)
    python3 -I - "$profiles/evil-boom/$store" "$profiles/$name/$store" "$port" "$k" <<'EOF'
import json, sqlite3, sys
src, dst, port, k = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
a = sqlite3.connect(f"file:{src}?mode=ro", uri=True)
b = sqlite3.connect(dst)
a.backup(b)
v = b.execute("select value from ItemTable where key='userPrefs'").fetchone()[0]
prefs = json.loads(v.decode("utf-16-le"))
prefs["api"]["server"]["port"] = port
prefs["app"]["character"]["displayName"] = f"Evil Neuro {k}"
b.execute("update ItemTable set value=? where key='userPrefs'",
          (json.dumps(prefs).encode("utf-16-le"),))
b.commit()
EOF
  fi
  if up "http://127.0.0.1:$port/mcp" 1; then
    echo "player $k: Gary up on ws://127.0.0.1:$port"
    continue
  fi
  echo "player $k: starting Gary ($name) on $port..."
  GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    nohup "$GARY_DIR/gary-instance.sh" "$name" --start-server > "out/neuro/gary-p$k.log" 2>&1 &
  up "http://127.0.0.1:$port/mcp" 40 || { echo "Gary $name didn't come up on $port"; exit 1; }
  echo "player $k: Gary up on ws://127.0.0.1:$port"
done
