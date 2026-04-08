#!/usr/bin/env bash
#
# Smoke-test a .vsix package for the Patches VS Code extension.
#
# Usage:
#   ./scripts/smoke-test-vsix.sh path/to/patches-vscode-darwin-arm64-0.0.1.vsix
#
# What it does:
#   1. Installs the .vsix into VS Code
#   2. Checks that the extension is listed
#   3. Verifies the bundled patches-lsp binary exists and is executable
#   4. Runs the binary with --version (or checks it starts without error)
#   5. Opens a .patches file in VS Code and waits briefly for the LSP to activate
#
# Prerequisites:
#   - `code` CLI available in PATH
#   - On macOS: you may need to remove quarantine first (see RELEASE_NOTES.md)

set -euo pipefail

VSIX="${1:?Usage: $0 <path-to-vsix>}"

if ! command -v code &>/dev/null; then
  echo "ERROR: 'code' CLI not found in PATH."
  echo "Install it from VS Code: Cmd+Shift+P > 'Shell Command: Install code command in PATH'"
  exit 1
fi

echo "==> Installing $VSIX"
code --install-extension "$VSIX" --force

echo ""
echo "==> Checking extension is installed"
if code --list-extensions | grep -q "vulpus-labs.patches-vscode"; then
  echo "    OK: vulpus-labs.patches-vscode is installed"
else
  echo "    FAIL: extension not found in installed list"
  exit 1
fi

# Find the extension install directory
EXT_DIR=$(find ~/.vscode/extensions -maxdepth 1 -name 'vulpus-labs.patches-vscode-*' -type d | sort -V | tail -1)
if [[ -z "$EXT_DIR" ]]; then
  echo "    FAIL: could not find extension directory under ~/.vscode/extensions/"
  exit 1
fi
echo "    Extension dir: $EXT_DIR"

echo ""
echo "==> Checking bundled LSP binary"
BINARY="$EXT_DIR/server/patches-lsp"
if [[ "$(uname -s)" == *MINGW* || "$(uname -s)" == *MSYS* || "$(uname -s)" == *CYGWIN* ]]; then
  BINARY="$EXT_DIR/server/patches-lsp.exe"
fi

if [[ -f "$BINARY" ]]; then
  echo "    OK: binary exists at $BINARY"
else
  echo "    FAIL: bundled binary not found at $BINARY"
  echo "    (This means the .vsix was packaged without the server/ directory)"
  exit 1
fi

if [[ -x "$BINARY" ]]; then
  echo "    OK: binary is executable"
else
  echo "    WARN: binary is not executable (may need chmod +x or quarantine removal)"
fi

# On macOS, check for quarantine attribute
if [[ "$(uname -s)" == "Darwin" ]]; then
  if xattr -l "$BINARY" 2>/dev/null | grep -q "com.apple.quarantine"; then
    echo "    WARN: quarantine attribute present — run:"
    echo "      xattr -d com.apple.quarantine $BINARY"
  else
    echo "    OK: no quarantine attribute"
  fi
fi

echo ""
echo "==> Quick LSP launch test (checking binary runs without immediate crash)"
if timeout 3 "$BINARY" --stdio </dev/null >/dev/null 2>&1; then
  echo "    OK: binary ran without immediate error"
else
  EXIT_CODE=$?
  if [[ $EXIT_CODE -eq 124 ]]; then
    # timeout exit code — the process was still running, which is good
    echo "    OK: binary stayed alive (killed by timeout, as expected)"
  else
    echo "    WARN: binary exited with code $EXIT_CODE"
    echo "    (This may be fine if it expects stdin input from an LSP client)"
  fi
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SAMPLE_FILE="$REPO_ROOT/examples/radigue_drone.patches"

if [[ -f "$SAMPLE_FILE" ]]; then
  echo ""
  echo "==> Opening sample .patches file in VS Code"
  echo "    File: $SAMPLE_FILE"
  code "$SAMPLE_FILE"
  echo "    VS Code opened. Manually verify:"
  echo "      - Syntax highlighting is active (keywords coloured)"
  echo "      - LSP status shows in the status bar or Output > Patches Language Server"
  echo "      - Hover over a module name shows type info"
  echo "      - Diagnostics appear for any syntax errors"
else
  echo ""
  echo "==> No sample .patches file found at $SAMPLE_FILE"
  echo "    Open any .patches file in VS Code to verify the extension works."
fi

echo ""
echo "==> Smoke test complete."
echo "    If all checks passed, the .vsix is good."
