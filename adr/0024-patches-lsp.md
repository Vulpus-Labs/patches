# ADR 0024 — Patches LSP

**Date:** 2026-04-07
**Status:** Proposed

---

## Context

Patches is a text-based modular synthesis DSL. Authors currently edit `.patches`
files with no editor assistance — no completions, no inline diagnostics, no
hover documentation. As the DSL grows richer (templates, shape-parameterised
modules, enums, arity expansion, unit literals), the cost of working without
intellisense increases.

The goal is LSP-based editor support: completions, diagnostics, and hover info,
compatible with any LSP-capable editor. A minimal VS Code extension is the
initial client.

### Constraints

1. **The runtime pipeline must remain unchanged.** `patches-dsl` (pest parser +
   template expander) and `patches-interpreter` are production code. The LSP
   must not alter their types, error handling, or control flow.

2. **Error-tolerant parsing.** The runtime parser (pest) hard-fails on invalid
   input. An IDE parser must produce a useful tree from incomplete or malformed
   source — the common state while the author is typing.

3. **Module metadata is shape-dependent.** A `Mixer(channels: 4)` has different
   ports and parameters than a `Mixer(channels: 2)`. The LSP must resolve shape
   arguments to provide accurate completions.

## Decision

### Two-parser architecture

Keep pest for the runtime path. Add tree-sitter for the IDE path. Tree-sitter
provides error-tolerant, incremental parsing — it produces a concrete syntax
tree (CST) with `ERROR`/`MISSING` nodes rather than hard-failing. The two
parsers agree on all valid input; they diverge only in error-handling behaviour.

### Independent analysis pipeline

The LSP does not reuse the runtime's template expander or interpreter. It
implements its own analysis over its own tolerant AST types.

**Rationale:** the runtime expander's job is to produce a `FlatPatch` — a fully
inlined, flattened graph with all templates resolved, parameters substituted,
and arity expanded. The LSP does not need this. It needs:

- **Template signatures** (name, declared params with types/defaults, declared
  in/out ports) — extracted directly from the tolerant AST.
- **Module descriptors** for concrete module instances — obtained by calling
  `Registry::describe(name, shape)`, the same query the runtime uses.
- **Scope-local validation** — checking connections and parameters within each
  body (template or patch) against the descriptors of the modules declared in
  that scope.

None of this requires template inlining or flattening. The expander's
complexity (recursive instantiation, parameter environment threading, arity
product expansion, scale composition across template boundaries) is unnecessary
for IDE analysis and would be difficult to make tolerant without a full rewrite.

A shared tolerant AST (making `patches-dsl` AST fields `Option<T>`) was
considered and rejected: it would add defensive handling throughout the runtime
path for states that can never occur there, violating constraint 1.

### Single crate

The entire LSP implementation lives in one crate, `patches-lsp`, with internal
modules. Nothing in this pipeline is reused by other crates — the tree-sitter
grammar, tolerant AST, semantic analysis, and LSP server are all internal
concerns.

### Registry as the sole shared interface

The only runtime code the LSP calls is `Registry::describe(name: &str, shape:
&ModuleShape) -> Result<ModuleDescriptor, BuildError>`. This is a pure,
stateless query with no side effects. The LSP constructs a `ModuleShape` from
whatever shape arguments are available, falling back to `ModuleShape::default()`
(channels=0, length=0, high_quality=false) for missing or invalid args. The
returned descriptor drives completions and diagnostics.

No new methods or types are added to the registry or module descriptor. The
existing `ModuleDescriptor` fields — `inputs`, `outputs`, `parameters` — 
contain all the metadata the LSP needs.

### Four-phase semantic analysis

1. **Shallow scan** — tolerant full-file pass extracting declaration names and
   kinds (modules, templates, enums). Fast, no descriptor resolution.

2. **Dependency resolution** — build a declaration dependency graph (templates
   referencing other templates); topo-sort; flag cycles as diagnostics.

3. **Descriptor instantiation** — bottom-up, evaluate shape arguments to
   produce a `ModuleDescriptor` per concrete module instance via registry
   query. Template signatures are extracted directly from their declarations.

4. **Body and connection analysis** — validate parameter names/types and
   connection port names/indices against resolved descriptors. Emit
   diagnostics with source spans.

### Stable model + current syntax

The LSP composites two representations: the stable semantic model from the
last fully-analysable file state, and the current CST for cursor position
context. This is the same pattern rust-analyzer uses. Completions degrade
gracefully rather than going dark when the file is transiently invalid.

### VS Code extension as early test harness

A minimal VS Code extension is introduced alongside the LSP server scaffold,
not as a final polishing step. The extension does nothing but spawn the
`patches-lsp` binary and connect via stdio using `vscode-languageclient`. This
gives a live feedback loop from the first ticket: edit a `.patches` file in
VS Code's Extension Development Host (F5), see diagnostics and completions as
they are implemented.

The extension expects `patches-lsp` on `$PATH` (or a configured path). No
bundling, packaging, or marketplace publishing — it is a development tool.

A TextMate grammar for syntax highlighting is included in the initial extension
scaffold. The DSL's keyword set is small and stable, and the grammar is a
standalone JSON file with no LSP dependency — cheap to write and immediately
useful.

### Corpus test strategy

Tree-sitter's built-in corpus test format pairs input source with expected
S-expression CST output. Initial corpus entries are generated from existing
valid `.patches` fixture and example files. Error recovery behaviour is
verified with dedicated malformed-input corpus entries.

## Consequences

- **No impact on runtime pipeline.** `patches-dsl`, `patches-interpreter`, and
  `patches-engine` are unchanged.
- **Grammar maintenance cost.** Two grammars (pest + tree-sitter) must agree on
  valid syntax. Changes to the DSL syntax require updating both. Corpus tests
  generated from shared fixture files mitigate drift.
- **Single-crate simplicity.** All LSP code is co-located and can evolve
  freely without cross-crate API stability concerns.
- **Module registry is the contract.** Adding a new module type automatically
  enriches LSP intellisense — the LSP queries the same registry the runtime
  uses.
