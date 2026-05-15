# Patches — Claude context

## Project overview

Patches is a Rust system for defining modular audio patches using a DSL. Patches can be reloaded at runtime to modify the patch setup, enabling live-coding performance. The system also includes an efficient audio engine for running these patches.

The two key concerns are:

1. **DSL and patch definition** — a format for describing signal graphs of audio modules
2. **Audio engine** — real-time audio processing with hot-reload capability

## Workspace layout

```text
patches-core/              Core types, traits, and execution plan runtime
patches-dsp/               Pure DSP kernels (filters, delay, noise, ADSR)
patches-dsl/               PEG parser and template expander for the .patches DSL format
patches-interpreter/       Validates FlatPatch against module registry; builds ModuleGraph
patches-modules/           Module implementations (oscillators, filters, effects, etc.)
patches-engine/            Patch builder, sound engine, CPAL integration
patches-player/            patch_player binary: load a patch, play, hot-reload on change
patches-io/                I/O integration (audio capture, WAV recording)
patches-clap/              CLAP audio plugin (wry webview GUI)
patches-plugin-common/     GUI-toolkit-agnostic plugin state/helpers
patches-lsp/               Language Server Protocol for .patches files (used by VS Code extension)
patches-ffi/               FFI bindings for loading native module plugins
patches-ffi-common/        Shared types for FFI plugin interface
patches-profiling/         Profiling utilities
patches-integration-tests/ Cross-crate integration tests (publish = false)
test-plugins/              Example native plugins (gain, conv-reverb) for FFI testing
patches-vscode/            VS Code extension: syntax highlighting + LSP client (TypeScript)
docs/                      mdBook manual (source in docs/src/)
tickets/                   Work tracking (see Ticket workflow below)
epics/                     Epics grouping related tickets
adr/                       Architecture decision records
```

`patches-dsl` has no audio or module dependencies (only `pest`). `patches-dsp` is the workspace's leaf DSP crate — depends only on `rtrb` (lock-free ring buffers), with no `patches-core`, CPAL, or serde footprint — and is the home for reusable DSP building blocks (biquad and SVF filter kernels, halfband interpolator/decimator, delay buffer, peak window, phase accumulator, ADSR core, noise PRNG and spectral shaping filters). `patches-modules` depends on `patches-core` and `patches-dsp`. `patches-interpreter` depends on `patches-core`, `patches-dsl`, and `patches-modules`. `patches-engine` depends on `patches-core` and `patches-dsp`. `patches-player` (the binary) depends on all crates and is where the DSL pipeline meets the engine. `patches-lsp` provides diagnostics, hover, and go-to-definition for `.patches` files; it is bundled into the VS Code extension as a platform-specific binary. `patches-integration-tests` depends on `patches-core`, `patches-engine`, and `patches-modules`; it is never published. New audio modules should live in `patches-modules`; pure DSP algorithms with no module protocol concerns belong in `patches-dsp`.

## Commands

```bash
cargo build               # build all crates
cargo test                # run all tests
cargo clippy              # lint (fix all warnings before considering work done)
cargo test -p patches-core    # test a single crate
```

### Tiered validation profiles (ADR 0067)

Four Justfile recipes; each tier is a strict superset of the one above.

| Tier     | When                             | Recipe                           |
| -------- | -------------------------------- | -------------------------------- |
| `inner`  | every agent iteration            | `just inner [-p touched-crate]`  |
| `commit` | before a commit closing a ticket | `just commit [-p touched-crate]` |
| `push`   | before `git push` (and CI)       | `just push`                      |
| `smoke`  | manual / epic close / scheduled  | `just smoke`                     |

Agent runs `inner` per iteration, `commit` before closing a ticket,
`push` before pushing. CI runs `just push` on PRs and pushes to main.
`smoke` is manual dispatch.

## Ticket workflow

Work is tracked in `tickets/` using markdown files organised by status:

- `tickets/open/` — not yet started
- `tickets/in-progress/` — currently being worked on
- `tickets/closed/` — done

Filename convention: `NNNN-short-description.md` (e.g. `0001-dsl-parser.md`).

Use `tickets/TEMPLATE.md` as the starting point for new tickets.

When starting a ticket: move it to `in-progress/`. When done: move it to `closed/`.

## Architecture decision records

Design decisions with trade-offs are recorded in `adr/` as numbered markdown files (`NNNN-short-description.md`). Reference the relevant ADR from tickets and code comments where a decision might otherwise seem arbitrary.

## Audio engine conventions

- **No allocations on the audio thread.** All buffers and module state must be pre-allocated.
- **No blocking on the audio thread.** No mutexes, no I/O, no syscalls in the processing path.
- **Real-time/non-real-time boundary.** Use lock-free data structures (e.g. ring buffers, atomics) to communicate between the audio thread and the hot-reload/control thread.

## Design desiderata

These are qualities the system should preserve as it evolves. They inform design decisions but are not hard rules — trade-offs are recorded in `adr/`.

- **Parallelism-ready execution.** Module execution order is constrained only by ADR 0072 fusion: across SCCs (the common case for cycle-free patches), modules can run in any order — but *within* an acyclic fused subgraph the planner emits modules in topological order and consumers read producers' current-tick output (`InputPort.fused = true`). Calling fused-subgraph modules out of topo order causes silent reads of stale data. Cycle-bearing edges retain the 1-sample delay and stay order-independent. The execution plan should remain structured so that splitting independent SCCs across threads is a contained change to `ExecutionPlan::tick()` and the builder's buffer layout, with no impact on the Module trait, ModuleGraph, or module implementations.
- **Cache-friendly buffer layout.** Cable buffers should be packed densely in memory. When parallelism arrives, the builder should partition buffers by thread affinity (buffers accessed by the same thread are contiguous) and pad partition boundaries to cache lines to avoid false sharing.
- **Zero-cost descriptors.** Module descriptors (port names, counts) are compile-time constants defined by module implementations, not by the DSL. Port names are `&'static str`; accessing descriptors should not allocate. The DSL specifies *which* modules to instantiate and how to wire them, but port layouts are fixed per module type.
- **Backend-agnostic core.** `patches-core` defines traits and data structures with no knowledge of audio backends, file formats, or UI. Concrete backends live in `patches-engine` or dedicated crates.

## Module documentation standard

Every module in `patches-modules/src/` has a doc comment (either `///` on
the struct or `//!` at file level) in a standard form. This comment is the
**source of truth** for the manual's module reference (`docs/src/modules/`).

When adding or changing a module, keep the comment in this form:

```rust
/// Brief one-line description.
///
/// Extended description (optional — algorithm notes, CV behaviour, etc.).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `name` | mono/poly | What it does |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `name` | mono/poly | What it does |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `name` | float/int/bool/enum | range | `default` | What it does |
```

- Port names must match the strings in the module's `ModuleDescriptor`.
- For indexed ports use `port[i]` notation with a note on the range
  (e.g. "i in 0..N−1, N = `channels`").
- Omit sections that don't apply (e.g. no Parameters table if none exist).
- Preserve valuable technical notes (algorithms, real-time safety remarks)
  after the tables.

## Port naming conventions

ADR 0059 retired the paired `_left`/`_right` convention for symmetric
stereo modules. The current rules:

- Mono modules use simple names: `"in"`, `"out"`, `"cv"`.
- Stereo modules use a single stereo port: `"in"`, `"out"`. The
  underlying cable carries `(L, R)` as a `CableKind::Stereo` value;
  module code reads `(f32, f32)` via `pool.read_stereo(&self.in)`.
- Compound stereo ports likewise collapse to one name per role:
  `"send_a"`, `"return_b"`, etc.
- A mono Audio source feeding a stereo input is silently broadcast
  (`L = R = source`) at cable-builder time, with no synthetic node and
  no warning. Stereo→mono is rejected with `CableKindMismatch`; users
  who want the two halves apart wire through `StereoSplitter` /
  `StereoJoiner`.
- Control/modulation inputs use descriptive names:
  `"mix"`, `"feedback"`, `"voct"`, `"gate"`.

`_left`/`_right` survives in exactly two places:

1. `StereoSplitter` / `StereoJoiner`, whose port asymmetry is the
   point of the module.
2. Mid/side or other genuinely-asymmetric pair processors (none in
   tree today). For those, the descriptor declares paired mono ports.

## Panic policy

ADR 0051 / E113 requires `panic = "unwind"` for the CLAP host and any crate
loaded as an FFI plugin. `PatchProcessor::tick` catches module panics at the
tick boundary and halts the engine cleanly; that only works with table-based
unwinding. Do not add `panic = "abort"` to workspace or plugin profiles.

## General conventions

- No `unwrap()` or `expect()` in library code — use proper error propagation.
- Keep `patches-core` free of audio-backend dependencies so it can be tested without hardware.
- Run `cargo clippy` and `cargo test` before considering any implementation ticket done.
- Ask before adding new dependencies to `Cargo.toml`.
