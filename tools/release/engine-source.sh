#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
# engine-source.sh VERSION [OUT]: the engine's complete source as built into a release,
# out/release/evil-boom-VERSION-engine-source.tar.gz: DSDA-Doom at the pinned commit with
# dsda/dsda-doom.patch applied (third_party/dsda-doom, without its git data or build folder).
# The GPL asks a binary release to offer it; the release attaches it.
set -euo pipefail
cd "$(dirname "$0")/../.."
version=${1:?usage: tools/release/engine-source.sh VERSION [OUT]}
out=${2:-out/release}
mkdir -p "$out"
tar -C third_party --exclude=.git --exclude=prboom2/build \
  -czf "$out/evil-boom-$version-engine-source.tar.gz" dsda-doom
echo "$out/evil-boom-$version-engine-source.tar.gz"
