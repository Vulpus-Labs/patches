#!/usr/bin/env bash
#
# Build platform-specific .vsix packages for patches-vscode.
#
# Usage:
#   ./scripts/package-vsix.sh              # build all targets
#   ./scripts/package-vsix.sh darwin-arm64  # build one target
#
# Prerequisites:
#   - Rust toolchain with cross-compilation targets installed
#   - Node.js + npm
#   - npx (ships with npm)
#   - @vscode/vsce: installed via `npm install -g @vscode/vsce` or used via npx
#
# For cross-compiling Linux from macOS you'll need an appropriate linker
# (e.g. via `brew install filosottile/musl-cross/musl-cross` for musl targets,
# or use `cross` / build in a container).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VSCODE_DIR="$REPO_ROOT/patches-vscode"
DIST_DIR="$REPO_ROOT/dist"
SERVER_DIR="$VSCODE_DIR/server"

# Known vscode platform targets (space-separated so we avoid bash 4+
# associative arrays — macOS still ships bash 3.2).
ALL_TARGETS="darwin-arm64 darwin-x64 linux-x64 win32-x64"

cargo_triple_for() {
  case "$1" in
    darwin-arm64) echo "aarch64-apple-darwin" ;;
    darwin-x64)   echo "x86_64-apple-darwin" ;;
    linux-x64)    echo "x86_64-unknown-linux-gnu" ;;
    win32-x64)    echo "x86_64-pc-windows-msvc" ;;
    *)            return 1 ;;
  esac
}

binary_name_for() {
  case "$1" in
    win32-x64) echo "patches-lsp.exe" ;;
    *)         echo "patches-lsp" ;;
  esac
}

build_target() {
  local vscode_target="$1"
  local cargo_triple binary
  cargo_triple="$(cargo_triple_for "$vscode_target")"
  binary="$(binary_name_for "$vscode_target")"

  echo "==> Building patches-lsp for $vscode_target ($cargo_triple)"
  cargo build --release -p patches-lsp --target "$cargo_triple"

  echo "==> Packaging .vsix for $vscode_target"
  rm -rf "$SERVER_DIR"
  mkdir -p "$SERVER_DIR"
  cp "$REPO_ROOT/target/$cargo_triple/release/$binary" "$SERVER_DIR/"

  # Compile the extension TypeScript
  cd "$VSCODE_DIR"
  npm ci --ignore-scripts
  npx tsc -p ./

  # Package
  mkdir -p "$DIST_DIR"
  npx @vscode/vsce package --target "$vscode_target" --allow-missing-repository -o "$DIST_DIR/"

  # Clean up server dir
  rm -rf "$SERVER_DIR"

  echo "==> Done: $DIST_DIR/patches-vscode-$vscode_target-*.vsix"
}

# If a specific target is given, build only that one
if [[ $# -gt 0 ]]; then
  for target in "$@"; do
    if ! cargo_triple_for "$target" >/dev/null 2>&1; then
      echo "Unknown target: $target"
      echo "Valid targets: $ALL_TARGETS"
      exit 1
    fi
    build_target "$target"
  done
else
  for target in $ALL_TARGETS; do
    build_target "$target"
  done
fi

echo ""
echo "All .vsix packages:"
ls -1 "$DIST_DIR"/*.vsix 2>/dev/null || echo "(none found)"
