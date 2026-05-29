# ADR 0079 — Patch-graph JSON surface; presentation off-loaded to consumers

- Status: proposed
- Date: 2026-05-28
- Supersedes: none (narrows E072's `patches/renderSvg` LSP request)
- Related: ADR 0067 (blast-radius cuts within the monorepo), ADR 0073
  (monorepo split), ADR 0075 (source provenance), E072 (patch SVG rendering)

## Context

`patches-svg` renders an expanded patch as an SVG string. The LSP exposes it
as a custom request `patches/renderSvg`; the VS Code extension drops the
returned string into a webview via `innerHTML`. Layout (Sugiyama via
`rust-sugiyama`) and rendering (SVG emission) are fused inside one Rust crate,
and the editor is coupled to a Rust-rendered, non-interactive SVG blob.

Two observations motivate revisiting this:

1. **The dependency cost of `patches-svg` is no longer the headline.** Ticket
   0797 already dropped `patches-modules` from the LSP. `cargo tree` shows the
   only remaining weight `patches-svg` adds to `patches-lsp` is
   `rust-sugiyama → petgraph` (`fixedbitset`/`hashbrown`/`foldhash`) + `log`.
   `patches-core`/`patches-dsl`/`patches-manifest` are in the LSP regardless.
   So a split is a modest dep cleanup, not a major one — it must justify
   itself on other grounds.

2. **The expand→resolve pipeline has no clean external surface.** Today the
   only externally-observable artifact of template expansion + port-kind
   resolution is either internal Rust types (in tests) or an SVG string
   (snapshot-tested by grepping XML substrings). There is no diffable,
   tool-readable representation of "what a `.patches` file expands to."

The same intermediate the SVG renderer already consumes —
`patches_dsl::FlatPatch` (post template-expansion, pre-interpreter) enriched
with per-port `CableKind`/`PolyLayout` from the bundled manifest — is exactly
what a JSON document would carry. Emitting it directly turns an internal
intermediate into a versioned contract.

## Decision

**Introduce a JSON representation of the expanded, port-kind-resolved patch
graph as the contract between the DSL pipeline and all presentation/diagnostic
consumers. Layout and rendering move out of Rust to the consumer.**

### 1. What the JSON contains

The cheap, partial-patch-friendly tier — matching what `patches-svg` consumes
today:

- **Modules:** id, type name, shape args, resolved params, port aliases.
- **Connections:** from/to module+port+index, cable `map` (scale etc.).
- **Port-kind annotations:** each port's `CableKind` / `PolyLayout`, resolved
  via `registry_from_manifest(bundled_manifest())` — the same lookup
  `patches-svg` uses. Unknown module types degrade to unclassified, as today.
- **Full provenance:** expansion chains and source spans, verbatim
  (`patches_dsl::Provenance`). See §4 for why full, not stripped.

This tier needs only `patches-dsl` + `patches-manifest`. **No interpreter, no
module registry build, no `rust-sugiyama`.** Partial/invalid patches still
emit, preserving the graceful-degradation property the live VS Code panel
relies on. Layout (node positions) is explicitly **not** in the JSON — it is a
presentation concern.

A richer second tier (fully-validated `ModuleGraph`: SCCs, fusion decisions,
buffer layout) is deferred — it pulls `patches-interpreter` + `patches-modules`
and serves deep debugging, not diagrams. If built, it is a separate command,
not the default surface (see Open questions).

### 2. Serde without polluting the leaf crate

`patches-dsl` has no `serde` dependency today and `FlatPatch`/`FlatModule`/
`FlatConnection` carry no derives. The DSL crate stays serde-free: the JSON
schema is defined as **mirror structs in a new `patches-graph-json` crate**
(`patches-dsl` + `patches-manifest` deps) that map `FlatPatch` + resolved port
kinds → serializable types. This keeps the schema (a stable external contract)
decoupled from the internal `FlatPatch` representation, so the two can evolve
independently and the schema can be versioned deliberately.

The crate ships a small CLI (emit JSON for a `.patches` file to stdout/disk,
with include-path resolution) so the surface is usable outside the LSP.

### 3. Consumers read JSON; presentation lives outside Rust

- **LSP:** add `patches/graphJson` returning the JSON document. The existing
  `patches/renderSvg` is retired once the JS renderer reaches parity (phased).
- **VS Code:** the webview consumes the JSON and lays it out / renders with a
  JS engine (e.g. `elkjs` or `dagre` for layered layout) into HTML with
  embedded SVG. The diagram becomes interactive (pan/zoom/click-to-source via
  the provenance spans) instead of a static `innerHTML` blob.
- **`patches-svg`:** retired, or demoted to a standalone CLI that consumes the
  JSON for server-side doc rendering. Either way it leaves the LSP dep tree,
  taking `rust-sugiyama`/`petgraph`/`log` with it.

### 4. Testing surface: full provenance in output, redact in golden

The JSON doubles as a golden-file surface for the expand→resolve pipeline,
replacing brittle SVG-substring assertions and many hand-written
slice-assertions in `patches-dsl/tests/expand/*`. Most expander/inliner tests
load one fixture and assert on one slice of the resulting `FlatPatch` (module
set, a connection, a resolved param, a composed scale); a single golden of the
expansion captures all such slices at once and catches regressions the
targeted asserts miss.

The emitter carries **full provenance (expansion chains + raw source spans)** —
spans are load-bearing for the debug/diagram surface (hover, click-to-source).
The **golden harness redacts at compare time**, not the emitter:

- **Redact spans to a stable placeholder** (`"[span]"`), don't drop the field —
  the golden still proves provenance *exists* where expected, just not its
  volatile byte value. Raw offsets shift on every fixture edit and would churn
  goldens catastrophically.
- **Canonicalize ordering:** sort modules by id, connections by
  `(from, from_port, from_index, to, to_port, to_index)` before compare.
- **Tolerant float compare:** composed scales (e.g. 0.5 × 0.8) carry
  representation noise; compare numerically with an epsilon (existing tests use
  `1e-12`) or round before serialize.

`insta` (already a `patches-svg` dev-dependency) provides snapshot storage +
redaction paths; ordering canonicalization + float rounding happen in the
serializer feeding it.

What does **not** collapse into goldens, and stays as targeted tests:

- **AST/parse-level tests** (`ast_port_index_variants`, `ast_param_decl_arity`,
  …) inspect the parsed `File` *before* expansion — wrong layer for a FlatPatch
  golden.
- **Error-path tests** assert expansion *fails* with a message substring;
  substring match is already robust to wording, and tolerant JSON equality
  doesn't apply to the `Err` path.
- **A few negative-intent asserts** (`v1` must *not* appear as a FlatModule):
  goldens capture these implicitly but less legibly; keep an explicit handful
  to document intent.

## Phased delivery

- **Phase 1 (E157):** `patches-graph-json` crate — schema, emitter, CLI; POC
  emits and reviews `voice_template.patches`.
- **Phase 1.1 (E157):** canonicalizing golden harness; migrate the structural
  `patches-dsl/tests/expand/*` slice-asserts to fixture goldens.
- **Phase 2 (E158):** LSP `patches/graphJson`; VS Code JS layout + render
  (HTML + embedded SVG, interactive).
- **Phase 3 (E158):** retire the Rust SVG pipeline — remove
  `patches/renderSvg`, drop `patches-svg` from the LSP dep tree.

Phases are strictly ordered; each leaves the tree green. E157 delivers
standalone value (better tests + a debug/interchange surface) even if E158
never lands.

## Alternatives considered

- **Strip spans from the emitter** (instead of redacting in the golden).
  Rejected: cripples the debug/diagram use case (no hover, no click-to-source).
  Volatility is a *comparison* problem, solved at compare time.
- **Add `serde` derives directly to `patches-dsl` types.** Rejected: couples
  the external schema to the internal representation and adds a dep to the
  leaf DSL crate. Mirror structs keep both clean and let the schema version
  independently.
- **Keep rendering in Rust, just expose JSON alongside SVG.** Possible, but
  leaves `rust-sugiyama` in the LSP and forgoes interactivity. The point is to
  make JSON *the* contract and push presentation out.
- **Emit the fully-validated `ModuleGraph` as the default JSON.** Rejected for
  the default: pulls `patches-interpreter` + `patches-modules`, and loses the
  partial-patch rendering the live panel needs. Kept as a deferred second tier.

## Consequences

**Gains:**

- A versioned, diffable contract for the expand→resolve pipeline: better
  regression coverage, a debug surface, and an interchange format, all from one
  artifact.
- Test simplification: ~20+ hand-written slice-asserts collapse to ~8 fixture
  goldens + a canonicalizing harness (AST + error + a few intent asserts stay).
- Interactive diagrams; Rust sheds the layout/render burden; LSP sheds
  `rust-sugiyama`/`petgraph`/`log`.

**Costs:**

- Layout is reimplemented in JS (`elkjs`/`dagre` provide layered layout, so not
  from scratch, but real work). Until Phase 2 reaches parity, `patches-svg`
  stays.
- Two artifacts (resolver + renderer) to keep in sync — but that *is* the
  contract; the schema is versioned for exactly this reason.
- Goldens can rot via blind `UPDATE=1` accept; mitigated by review discipline
  and retained intent asserts.

## Open questions

1. **Compiled patch artifact for player/CLAP load.** Should the JSON be
   persisted as a "compiled" `.patches` + includes and loaded by player/CLAP
   where present? **Deferred.** The perf case is weak: JSON is pre-interpreter,
   so parse+expand (~100µs/file) is all that's skipped — the interpret/build is
   not. Hot-reload wants source, and a compiled artifact adds staleness +
   schema-skew handling. The one real pro is **CLAP plugin-state
   self-containment** (a DAW project embedding the compiled patch survives the
   source files moving), but that rides on CLAP state serialization and is its
   own concern. Tracked as spike ticket 0969, not scheduled into E157/E158.
2. **Second (validated) tier.** If/when deep-graph debugging (SCCs, fusion,
   buffer layout) wants a surface, define it as a separate command over the
   `ModuleGraph`, not by widening the default cheap tier.
3. **Schema versioning policy.** The JSON is an external contract once any
   third-party diagram tool consumes it. Decide a version field +
   compatibility expectations when the first external consumer appears.
