#!/usr/bin/env bash
# macOS post-extract install script for a Patches release bundle.
#
# Run from the extracted release directory:
#   ./macos-unquarantine.sh
#
# What it does:
#   1. Strips com.apple.quarantine from bundled binaries (player,
#      CLAP plugin, bundled .dylib modules, .vsix).
#   2. Installs Patches.clap into ~/Library/Audio/Plug-Ins/CLAP/.
#   3. If the `code` CLI is on PATH, installs the bundled .vsix into
#      VS Code (replacing any previous install).
#
# Steps 2 and 3 are skipped silently if their inputs aren't present.
# The script is idempotent — safe to re-run.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Removing com.apple.quarantine from items in: $DIR"

quarantine_targets=(
  "$DIR/patch_player"
  "$DIR/Patches.clap"
  "$DIR/libpatches_vintage.dylib"
)

for t in "${quarantine_targets[@]}"; do
  if [ -e "$t" ]; then
    xattr -dr com.apple.quarantine "$t" 2>/dev/null || true
    echo "  cleared: $t"
  fi
done

for v in "$DIR"/*.vsix; do
  [ -e "$v" ] || continue
  xattr -dr com.apple.quarantine "$v" 2>/dev/null || true
  echo "  cleared: $v"
done

CLAP_SRC="$DIR/Patches.clap"
CLAP_DEST_DIR="$HOME/Library/Audio/Plug-Ins/CLAP"
CLAP_DEST="$CLAP_DEST_DIR/Patches.clap"

if [ -e "$CLAP_SRC" ]; then
  mkdir -p "$CLAP_DEST_DIR"
  # Drop any quarantine flag on the existing copy before overwriting,
  # so a leftover xattr from an earlier download doesn't survive.
  [ -e "$CLAP_DEST" ] && xattr -dr com.apple.quarantine "$CLAP_DEST" 2>/dev/null || true
  cp -R "$CLAP_SRC" "$CLAP_DEST"
  echo "Installed CLAP plugin -> $CLAP_DEST"
else
  echo "No Patches.clap in $DIR — skipping CLAP install."
fi

VSIX="$(ls -t "$DIR"/*.vsix 2>/dev/null | head -n1 || true)"
if [ -n "$VSIX" ]; then
  if command -v code >/dev/null 2>&1; then
    code --uninstall-extension vulpus-labs.patches-vscode >/dev/null 2>&1 || true
    code --install-extension "$VSIX" --force
    echo "Installed VS Code extension from $VSIX"
  else
    echo "No 'code' CLI on PATH — skipping VS Code extension install."
    echo "  To enable: open VS Code → Command Palette →"
    echo "  'Shell Command: Install code command in PATH', then re-run."
  fi
else
  echo "No .vsix in $DIR — skipping VS Code extension install."
fi

echo ""
echo "Done. You may still need to right-click → Open the player on first launch."
