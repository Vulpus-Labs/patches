#!/usr/bin/env bash
#
# Local deploy: release-build patches-clap + patches-lsp, install the
# CLAP plugin into the user's Audio Plug-Ins directory (as Patches.clap),
# and (re)install the patches-vscode extension into VS Code.
#
# macOS only. Run from the repo root (or anywhere — the script cd's to its
# own dir).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$REPO_ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "deploy.sh: macOS only." >&2
  exit 1
fi

case "$(uname -m)" in
  arm64)   VSCODE_TARGET="darwin-arm64" ;;
  x86_64)  VSCODE_TARGET="darwin-x64" ;;
  *)       echo "deploy.sh: unsupported arch $(uname -m)" >&2; exit 1 ;;
esac

CLAP_DEST_DIR="$HOME/Library/Audio/Plug-Ins/CLAP"

CLAP_SRC="$REPO_ROOT/target/release/libpatches_clap.dylib"
CLAP_DEST="$CLAP_DEST_DIR/Patches.clap"

echo "==> cargo build --release -p patches-clap"
cargo build --release -p patches-clap

if [[ ! -f "$CLAP_SRC" ]]; then
  echo "deploy.sh: expected $CLAP_SRC after build, not found" >&2
  exit 1
fi

mkdir -p "$CLAP_DEST_DIR"
install_clap() {
  local src="$1" dest="$2"
  echo "==> Installing CLAP plugin -> $dest"
  # Strip quarantine from any previous copy that might still be present,
  # then overwrite.
  [[ -f "$dest" ]] && xattr -d com.apple.quarantine "$dest" 2>/dev/null || true
  cp "$src" "$dest"
}
install_clap "$CLAP_SRC" "$CLAP_DEST"

echo "==> Building .vsix for $VSCODE_TARGET"
"$REPO_ROOT/scripts/package-vsix.sh" "$VSCODE_TARGET"

VSIX="$(ls -t "$REPO_ROOT/dist/patches-vscode-$VSCODE_TARGET-"*.vsix 2>/dev/null | head -n1)"
if [[ -z "$VSIX" ]]; then
  echo "deploy.sh: no .vsix produced in dist/" >&2
  exit 1
fi

if ! command -v code >/dev/null 2>&1; then
  echo "deploy.sh: 'code' CLI not on PATH — open VS Code and run" \
       "'Shell Command: Install code command in PATH', then rerun." >&2
  exit 1
fi

echo "==> Reinstalling $VSIX into VS Code"
code --uninstall-extension vulpus-labs.patches-vscode >/dev/null 2>&1 || true
code --install-extension "$VSIX" --force

echo ""
echo "Done."
echo "  CLAP: $CLAP_DEST"
echo "  VSIX: $VSIX"
