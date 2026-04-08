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

# Map: vscode platform target -> cargo target triple -> binary name
declare -A CARGO_TARGETS=(
  ["darwin-arm64"]="aarch64-apple-darwin"
  ["darwin-x64"]="x86_64-apple-darwin"
  ["linux-x64"]="x86_64-unknown-linux-gnu"
  ["win32-x64"]="x86_64-pc-windows-msvc"
)

declare -A BINARY_NAMES=(
  ["win32-x64"]="patches-lsp.exe"
)

DEFAULT_BINARY="patches-lsp"

build_target() {
  local vscode_target="$1"
  local cargo_triple="${CARGO_TARGETS[$vscode_target]}"
  local binary="${BINARY_NAMES[$vscode_target]:-$DEFAULT_BINARY}"

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
  npx @vscode/vsce package --target "$vscode_target" -o "$DIST_DIR/"

  # Clean up server dir
  rm -rf "$SERVER_DIR"

  echo "==> Done: $DIST_DIR/patches-vscode-$vscode_target-*.vsix"
}

# If a specific target is given, build only that one
if [[ $# -gt 0 ]]; then
  for target in "$@"; do
    if [[ -z "${CARGO_TARGETS[$target]+x}" ]]; then
      echo "Unknown target: $target"
      echo "Valid targets: ${!CARGO_TARGETS[*]}"
      exit 1
    fi
    build_target "$target"
  done
else
  for target in "${!CARGO_TARGETS[@]}"; do
    build_target "$target"
  done
fi

echo ""
echo "All .vsix packages:"
ls -1 "$DIST_DIR"/*.vsix 2>/dev/null || echo "(none found)"
