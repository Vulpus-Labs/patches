# LSP architecture

`patches-lsp` is the language server backing the [VS Code
extension](install-vscode.md). It runs as a per-platform native
binary bundled inside the VSIX and speaks LSP over stdio. This
chapter is for contributors.

Source: `patches-lsp/`.

## What it provides

- **Diagnostics** — parse errors, validation failures, layering
  warnings.
- **Hover** — module type descriptions, port info, parameter
  details.
- **Go-to-definition** — jump to template definitions and module
  type registrations.
- **Completions** — module types, port names, parameter names.
- **Inlay hints** — stereo desugaring (the synthesised splitter
  / `__l` / `__r` / joiner modules at every `stereo module` decl),
  off by default.
- **Patch graph SVG rendering** — exposed as the custom
  `patches/renderSvg` request; the VS Code extension's *Show
  Patch Graph* command consumes it.
- **Module rescan** — `patches/rescanModules` re-probes the
  configured bundle dirs without restarting the server.

## Dual-parser architecture

The LSP holds **two parsers** for every `.patches` document:

1. **Tree-sitter** (`patches-lsp/src/parser.rs`,
   `tree_nav.rs`, `tree-sitter-patches/`) — error-recovering
   grammar, fast incremental re-parse. Drives editor-facing
   features that need to behave on a half-written document:
   syntax-aware navigation, hover-context classification,
   completion-context determination.
2. **Pest** (`patches-dsl::pipeline`) — the engine's canonical
   parser. Drives every feature that needs the patch's *semantic*
   meaning: diagnostics, the expansion-aware hover layer,
   go-to-definition across templates, the SVG renderer.

Both parsers run on every document change. The tree-sitter parse is
incremental and cheap; the pest parse runs from scratch but
typically completes within ~100 µs for a real patch
(`project_pest_vs_tree_sitter_perf` benchmarks: pest ~100 µs, TS
~143 µs parse + ~204 µs parse+lower per file). Perf is not the
driver for this architecture — *correctness on partial input* is.

### Why both

A single parser can't serve both audiences:

- The engine's pest grammar refuses to produce any AST for a
  patch with a syntax error. That's the right policy for the
  engine — invalid input should fail loudly — but it leaves the
  editor with no tree to query when the user is mid-typing. Hover,
  completions, and signature help would go blank on every
  intermediate keystroke.
- A pure tree-sitter approach gives editor robustness but loses
  the semantic accuracy that the engine's expansion stage
  provides. Template-expanded hover information (showing the
  *concrete* module instantiated from a template call, with all
  parameters substituted) needs the pest pipeline.

The split: tree-sitter for "what is the user looking at right
now?", pest for "what would this patch actually do if it
compiled?".

### Expansion-aware hover (option C)

Hover information for a module call inside a template
instantiation reflects the *expanded* module — pest is run alongside
tree-sitter, gated on a clean parse. If pest succeeds, hover uses
the expanded view; if pest fails, hover falls back to tree-sitter's
local view. The user gets the best available answer at every
keystroke without the editor ever stalling.

Memory note: `project_lsp_expansion_hover` for the design
discussion behind this choice.

## Workspace model

The LSP keeps a `Workspace` (`patches-lsp/src/workspace/`) tracking:

- The open document set.
- The latest tree-sitter tree per document.
- The latest pest parse + expansion + validation result per
  document.
- The module registry, populated from in-tree modules plus any
  FFI bundles found in the configured `patches.moduleBundleDirs`.

When a document changes, the workspace runs incremental
tree-sitter, then re-runs the pest pipeline for the affected
document. Both results are cached and consulted by feature
handlers.

## Diagnostics

Three sources:

1. **Parse errors** from pest. Surface span, expected-rule
   message, location of the failure.
2. **Validation errors** from the interpreter — unknown module
   types, port mismatches, missing parameters, cable-kind
   mismatches.
3. **Layering warnings** from the planner — patterns the engine
   accepts but warns about (e.g. ambiguous fan-in, unconnected
   audio outputs).

Tree-sitter errors are *not* surfaced as LSP diagnostics. They
exist only to keep tree-sitter's tree well-formed for navigation;
the user-facing parse errors come from pest, which has better
diagnostic quality.

## Hover

Hover dispatch uses the tree-sitter cursor context to determine
*what kind of thing* is under the cursor (module type, port name,
parameter, template ref), then routes to a handler that builds the
hover content. Most handlers consult the pest-driven workspace
state for the substantive content (descriptors, expanded
parameter values); a few fall back to a tree-sitter-only path for
syntactic positions (keyword help, literal explanations).

Source: `patches-lsp/src/hover/`.

## Go-to-definition / navigation

Definitions resolved:

- Module type → registry entry (in-tree module's doc location, or
  bundle-discovered descriptor).
- Template call → template definition (same file or include-resolved).
- Pattern / song reference → declaration site.
- Host control reference → host-control declaration.

Source: `patches-lsp/src/navigation.rs`.

## SVG rendering

The `patches/renderSvg` custom request runs the pest pipeline to
produce a `FlatPatch`, then calls `patches_svg::render_svg`. The
returned SVG markup is sent back as JSON; the VS Code extension
renders it in a webview panel.

See [Visualising patches](authoring-visualising.md) for the
user-facing description.

## Module bundle discovery

The LSP loads module descriptors from the same FFI bundles the
runtime hosts use. The `patches.modulePaths` extension setting
threads through to `patches_ffi::PluginScanner`, which walks the
listed directories (and the env-var / default tiers) and registers
each bundle's modules.

`patches/rescanModules` re-walks without restarting the LSP —
useful after installing or rebuilding a bundle while the editor
is open.

## VSIX bundling

The VSIX is platform-specific. At build time
(`scripts/package-vsix.sh`, see the release workflow), the
`patches-lsp` binary for the target platform is built, copied into
`patches-vscode/server/`, and the TypeScript client compiled
against it. The result is one `.vsix` per platform; users install
the one that matches their VS Code host.

Platform target table:

| VSIX target     | LSP binary path inside VSIX                 |
| --------------- | ------------------------------------------- |
| `darwin-arm64`  | `server/patches-lsp` (aarch64-apple-darwin) |
| `darwin-x64`    | `server/patches-lsp` (x86_64-apple-darwin)  |
| `win32-x64`     | `server/patches-lsp.exe`                    |
| `linux-x64`     | `server/patches-lsp` (built from source)    |

The extension's `patches.lspPath` setting overrides the bundled
binary, useful for running a debug build during development.

## Syntax corpus policy

Any grammar change — pest or tree-sitter — requires a corresponding
entry in `patches-lsp/tests/syntax_corpus/` (memory:
`feedback_syntax_corpus_policy`). The corpus captures fixtures
exercising the new syntactic shape so regressions surface in CI
before reaching users. Treat the corpus as part of the grammar's
specification, not as an optional test layer.

## Reading the source

For a feature trace:

1. The handler is registered in `server.rs` (`PatchesLanguageServer::*`
   methods or custom-method bindings).
2. It reads from the `Workspace` to get tree-sitter / pest state
   for the affected document.
3. It produces an LSP response from the cached state plus the
   active position.

Each layer is small. The dual-parser architecture is the central
oddity; once you have that mental model, the rest reads as standard
LSP plumbing.
