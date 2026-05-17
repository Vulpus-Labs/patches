# Installing the VS Code extension

The VS Code extension provides syntax highlighting and language-server
features (diagnostics, hover, go-to-definition, completions) for
`.patches` files. The language server itself is `patches-lsp`, a
native binary bundled inside the VSIX for the current platform.

The extension is **platform-specific**: each VSIX contains the
`patches-lsp` binary for one OS / architecture. Install the one that
matches the machine VS Code is running on.

| VSIX name                                    | Platform                |
| -------------------------------------------- | ----------------------- |
| `patches-vscode-darwin-arm64-<ver>.vsix`     | Apple Silicon Macs      |
| `patches-vscode-darwin-x64-<ver>.vsix`       | Intel Macs              |
| `patches-vscode-win32-x64-<ver>.vsix`        | 64-bit Windows          |
| `patches-vscode-linux-x64-<ver>.vsix`        | 64-bit Linux (built     |
|                                              | from source — not yet   |
|                                              | shipped in releases)    |

The release archives contain the matching VSIX for that OS already;
the file is also attached as a separate asset on each release for
people who only want the extension.

## Install from VSIX

From a terminal:

```bash
code --install-extension patches-vscode-<platform>-<ver>.vsix
```

Or from inside VS Code:

1. *Extensions* (Cmd+Shift+X / Ctrl+Shift+X).
2. Three-dot menu → *Install from VSIX…*
3. Pick the VSIX.

Reload the window. Open any `.patches` file — the *Patches DSL*
extension activates, the editor gains syntax colouring, and the LSP
starts in the background.

### macOS: bundled LSP quarantine

If `patches-lsp` itself was downloaded as part of the release archive
it will likely be quarantined. The extension auto-starts the bundled
binary on first activation; if it fails (the *Patches DSL: Output*
panel shows "operation not permitted" or a Gatekeeper dialog appears),
strip quarantine from the binary inside the installed extension:

```bash
xattr -dr com.apple.quarantine \
  ~/.vscode/extensions/vulpus-labs.patches-vscode-*/server/
```

Then *Developer: Reload Window*. Running `macos-unquarantine.sh` on
the release archive avoids this — the script clears the flag on the
`.vsix` and, if the `code` CLI is on PATH, installs the extension
for you. `code --install-extension` preserves the cleared attribute,
so the bundled LSP starts cleanly.

### Windows: bundled LSP unblock

If the LSP fails to start on Windows, locate the installed extension
under `%USERPROFILE%\.vscode\extensions\vulpus-labs.patches-vscode-*`,
open `server\patches-lsp.exe` → *Properties* → *Unblock*, and reload
the window.

## Build from source

From a repo checkout, the canonical recipe is the per-platform build
script:

```bash
./scripts/package-vsix.sh darwin-arm64
# or: darwin-x64 | win32-x64 | linux-x64
# omit the arg to build all four
```

This release-builds `patches-lsp` for the requested target, copies
the binary into `patches-vscode/server/`, compiles the TypeScript
client, and produces a VSIX under `dist/`. Then:

```bash
code --install-extension dist/patches-vscode-darwin-arm64-*.vsix --force
```

`./deploy.sh` runs this for the current host architecture and reloads
the extension into VS Code in one step — the recommended path during
local development on macOS.

### Linux dependencies for building

Building `patches-lsp` needs ALSA dev headers
(`sudo apt install libasound2-dev`); building the VSIX itself needs
Node.js 20+ and `npx`.

## Configuration

The extension contributes two settings of note (both under *Patches
DSL* in the settings UI):

- **`patches.lsp.path`** — override the bundled binary with an
  alternative `patches-lsp` (e.g. a debug build under `target/debug/`).
  Leave empty to use the bundled binary, or to fall back to PATH
  lookup if no bundle is present.
- **`patches.modulePaths`** — directories or specific dylib
  files to scan for FFI module bundles. Required if you reference
  modules from external bundles (e.g. `patches-vintage`) in a
  `.patches` file — without it the LSP can't resolve their
  descriptors and hover/diagnostics will be incomplete.

## Verification

1. Open a `.patches` file (any from `examples/` will do).
2. Syntax should be coloured: keywords, module type names, ports,
   parameters distinct.
3. Hover over a module type name (e.g. `Osc`) — a tooltip describes
   the module and lists its ports.
4. Introduce a typo (`Osce`); a red squiggle appears with a "Unknown
   module type" diagnostic.

If steps 3–4 don't work the LSP isn't running. Open *Output* →
*Patches DSL* in the bottom panel for the server log; the most
common cause on macOS is the quarantine flag covered above.
