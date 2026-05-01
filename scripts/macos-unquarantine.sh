#!/usr/bin/env bash
# Strip macOS quarantine attribute from bundled Patches binaries.
# Run after extracting the release archive on macOS:
#   ./macos-unquarantine.sh
#
# This removes the "downloaded from the internet" Gatekeeper flag so
# the player, CLAP plugin, and native module dylibs will load.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Removing com.apple.quarantine from items in: $DIR"

targets=(
  "$DIR/patch_player"
  "$DIR/Patches.clap"
  "$DIR/libpatches_vintage.dylib"
)

for t in "${targets[@]}"; do
  if [ -e "$t" ]; then
    xattr -dr com.apple.quarantine "$t" 2>/dev/null || true
    echo "  cleared: $t"
  fi
done

# Clear quarantine on any .vsix as well (harmless if absent).
for v in "$DIR"/*.vsix; do
  [ -e "$v" ] || continue
  xattr -dr com.apple.quarantine "$v" 2>/dev/null || true
  echo "  cleared: $v"
done

echo "Done. You may still need to right-click → Open the first time."
