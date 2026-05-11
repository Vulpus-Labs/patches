# ADR 0073 — Externalise bundles; publish a 3-crate SDK from the main repo

**Date:** 2026-05-11
**Status:** Proposed
**Related:**
[ADR 0039 — FFI plugin bundles](0039-non-rust-ffi-plugins.md),
[ADR 0040 — Kernel carve: registry/planner/cpal/host](0040-kernel-carve.md),
[ADR 0045 — Spike 8: vintage migration](0045-spike-8-vintage-as-ffi.md),
[ADR 0067 — Blast-radius cuts within the monorepo](0067-blast-radius-cuts-within-monorepo.md),
[E145 — External SDK ABI cut preparation](../epics/open/E145-external-sdk-abi-cut.md)

## Context

ADR 0067 deferred a repo split. Some conditions have since changed:

- E145 stabilises the FFI ABI surface.
- Vintage already ships as a `cdylib + rlib` bundle
  ([patches-vintage/Cargo.toml](../patches-vintage/Cargo.toml)) loaded
  via `PluginScanner` (ticket 0570, ADR 0045 Spike 8 Phase C).
  `default_registry()` no longer calls `patches_vintage::register()`
  ([patches-modules/src/lib.rs:222](../patches-modules/src/lib.rs#L222)).
- The `export_modules!` macro
  ([patches-ffi-common](../patches-ffi-common/)) makes a Rust module
  crate into an FFI bundle near-mechanically.
- Two more module families isolate cleanly: drums (the
  `patches_dsp::drum::*` primitives have zero non-drum consumers) and
  FFT (pitch_shift + convolution_reverb consume slot_deck,
  WindowBuffer, partitioned_convolution, spectral_pitch_shift —
  nothing else does).

We considered a fine-grained split (seven successor repos:
foundation, sdk, vintage, drums, fft, host, tools). It collapses on
inspection: the foundation/host/tools split adds no value to module
authors (crates.io is a flat namespace; `cargo add patches-sdk` works
regardless of where the source repo lives) and costs cross-repo
coordination on every foundation change.

The actual goals:

1. **Easy onboarding for module authors.** `cargo new my-module` +
   `cargo add patches-sdk` is the day-one experience.
2. **Limit dependency and runtime footprint for module dev.** SDK
   pulls only the contract surface (Module trait, ABI types,
   registration). DSP kernels, FFT harness, manifest, dsl, etc. are
   optional git deps for authors who want them.
3. **External bundles as a real distribution channel.** Vintage,
   drums, fft are `cdylib` artefacts shipped as tarballs, not Rust
   library crates. Living in their own repos serves both as a
   distribution unit and as the template a third-party author copies.

Goals 1+2 are about **crate publication strategy**, not repo layout.
Goal 3 is about **repo layout** — bundle distribution channels are
naturally per-repo.

## Decision

**Coarse-grained repo split. Fine-grained crate publication.**

### Repo strategy: four repos

| Repo                | Contents                                                                                                                                                                                                                                                       | Distribution                                                                    |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| **patches**         | Existing monorepo minus the three bundles. Contains: foundation crates (incl. publishables), engine, planner, modules (minus drum/fft modules), host runtime, player, clap, lsp, svg-cli, tools, vscode, integration-tests, profiling. Single Cargo workspace. | 3 crates → crates.io; binaries (player, clap, lsp, etc.) → GitHub Releases.     |
| **patches-vintage** | Existing vintage cdylib bundle (BBD effects).                                                                                                                                                                                                                  | cdylib + descriptor JSON → GitHub Release tarball.                              |
| **patches-drums**   | Drum modules + drum DSP primitives extracted from main repo.                                                                                                                                                                                                   | cdylib + descriptor JSON → GitHub Release tarball.                              |
| **patches-fft**     | FFT harness (rlib) + pitch_shift + convolution_reverb modules (cdylib).                                                                                                                                                                                        | cdylib + descriptor JSON → GitHub Release tarball; harness as git-only library. |

Foundation + host + tools stay together in the main repo. ADR 0067's
validation tiers (inner/commit/push/smoke) already address CI noise
within a workspace; they continue to apply.

### Publication strategy: three crates to crates.io

| Crate                | Purpose                                                                        | Why on crates.io                                                                                      |
| -------------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| `patches-sdk`        | Module-author entry point. Thin re-export crate.                               | `cargo add patches-sdk` is the day-one experience.                                                    |
| `patches-core`       | Module trait, descriptors, ports, cables, params, registry.                    | Re-exported by patches-sdk; Cargo requires direct deps to be on crates.io if the parent is.           |
| `patches-ffi-common` | Wire format types (ABI v12+), JSON descriptor schema, `export_modules!` macro. | Re-exported by patches-sdk for the macro; also imported directly by the host's engine/planner/loader. |

**Everything else stays git-only.** Module authors who want DSP
kernels, FFT harness, DSL types, manifest, etc. add them as git deps:

```toml
patches-sdk = "0.7"
patches-dsp = { git = "https://github.com/.../patches", tag = "v0.7.0" }
patches-fft-harness = {
    git = "https://github.com/.../patches-fft",
    tag = "v0.7.0",
}
```

Acceptable: writing a module past day one is no longer day-one, and
`cargo add --git` is one line.

### Splits to undo before publish

The current workspace has internal splits made for hygiene that no
consumer realises. Most of them are fine to keep (the cost is just an
extra Cargo.toml). One blocks the 3-crate publish goal:

**patches-registry → merges into patches-core** as a `registry`
submodule. ADR 0040 split it out so the kernel could be
"registry-agnostic"; in practice every host-side consumer (planner,
interpreter, engine, modules, host) pulls both. The hygiene exists on
paper and costs a published crate. Merging saves the publish slot.

Other splits **kept**:

- `patches-ffi-common` stays separate from core. Independent ABI
  stability contract: ABI v11→v12 cycles should not couple to core's
  Rust API cadence. `descriptor_hash` derives from ffi-common types.
- `patches-io-ring`, `patches-tracker-core`, `patches-alloc-trap`,
  `patches-dsp`, `patches-dsl`, `patches-interpreter`,
  `patches-diagnostics`, `patches-manifest`, `patches-svg`,
  `patches-io`, `patches-ffi` (loader): host-internal, never publish,
  no reason to merge. Future audit can revisit specific pairs if dev
  ergonomics demand.

### What moves out of patches-dsp

Drum extraction:

- `patches-dsp/src/drum/` (envelope, sweep, metallic, burst, saturate)
  → `patches-drums/src/primitives/`.
- Internal `crate::fast_sine` / `crate::fast_tanh` refs rewrite to
  `patches_dsp::fast_sine` / `patches_dsp::fast_tanh` (general
  utilities stay in dsp).
- Re-exports at [patches-dsp/src/lib.rs:86](../patches-dsp/src/lib.rs#L86)
  (`pub use drum::{...}`) delete.

FFT extraction:

- `patches-dsp/src/slot_deck/` → `patches-fft-harness/src/slot_deck/`.
- `patches-dsp/src/window_buffer.rs` → `patches-fft-harness/src/window_buffer.rs`.
- `patches-dsp/src/partitioned_convolution/` → `patches-fft-harness/src/partitioned_convolution/`.
- `patches-dsp/src/spectral_pitch_shift/` → `patches-fft-harness/src/spectral_pitch_shift/`.
- `patches-dsp/src/fft.rs` (`RealPackedFft`) **stays** in dsp —
  cross-crate users include observation (spectrum analyser), vintage
  vdco tests, integration-tests, patches-modules test_support.
- `patches-dsp/tests/slot_deck/*` and
  `patches-dsp/tests/fft_lowpass.rs` → `patches-fft-harness/tests/`.

After both extractions, patches-dsp is a tighter set of reusable
kernels (svf, ladder, biquad, halfband, oscillator, noise, delay,
envelope, fft transform, atomics) with no module-family-specific
submodules.

### Registry discovery

Drum and fft modules drop their inline `r.register::<T>()` calls from
`patches-modules::default_registry()`. Host discovers them at startup
via `PluginScanner` reading a default search path — same mechanism
vintage uses. Host release artefacts (CLAP bundle, player tarball)
include the three stdlib `.dylib`s (vintage, drums, fft) in the
search path.

### Crate names to reserve on crates.io

Three must-publish names:

```text
patches-sdk
patches-core
patches-ffi-common
```

Defensive (cheap squat insurance for foundation-adjacent names):

```text
patches-dsp
patches-dsl
patches-fft-harness
```

Skip entirely: cdylib bundle names (`patches-vintage`,
`patches-drums`, `patches-fft-bundle`) — they ship as artefact
tarballs, not library crates. Binary crate names (`patches-player`,
`patches-clap`, `patches-lsp`, etc.) — not library crates either.
patches-vscode is TypeScript.

### Phase A: monorepo-internal preparation

1. Merge `patches-registry` into `patches-core::registry`.
2. Extract drums into a workspace `patches-drums` crate (cdylib + rlib).
3. Extract pitch_shift + convolution_reverb into `patches-fft-harness`
   (rlib) + `patches-fft-bundle` (cdylib + rlib) workspace crates.
4. Create the `patches-sdk` re-export crate. Migrate vintage, drums,
   fft-bundle, test-plugins to depend on patches-sdk instead of
   core/dsp/registry/ffi-common directly.
5. Drop extracted-module registrations from `default_registry()`;
   wire stdlib bundles into PluginScanner default search path.
6. Audit `pub` surfaces on the 3 publishable crates; demote to
   `pub(crate)` where unused externally.
7. Fill Cargo.toml publish metadata (description, license, repository,
   keywords, categories, rust-version, missing_docs lint) on the 3
   publishables. Defensive on others.
8. Reserve crates.io names per the list above.

### Phase B: publish + bundle cuts

1. Publish patches-sdk, patches-core, patches-ffi-common to crates.io
   as 0.7.x. patches-core's increment from 0.6 → 0.7 reflects the
   registry merge.
2. Cut patches-vintage repo. Bundle Cargo.toml uses
   `patches-sdk = "0.7"` from crates.io.
3. Cut patches-drums repo. Same.
4. Cut patches-fft repo (harness + bundle crates). Bundle uses
   crates.io; harness uses git path to patches-dsp from main repo
   (for RealPackedFft).

## Consequences

**Gains:**

- Module authors get a one-line install: `cargo add patches-sdk`.
- Three published crates total — manageable surface, manageable
  release tooling.
- Main repo stays as one Cargo workspace; existing CI, ticket flow,
  ADR home, validation tiers all continue working.
- Bundles distribute as artefact tarballs from their own repos. They
  also serve as reference templates for third-party module authors.

**Costs:**

- patches-registry merger requires updating every `use
  patches_registry::*` import workspace-wide (~mechanical).
- Foundation API stability discipline must come from review process,
  not repo separation. A breaking change to patches-core is a
  publish event; treat it accordingly.
- Bundle source code lives outside main-repo `just push`. CI for the
  three bundle repos runs independently. Bundle-level regressions
  surface via host-repo integration tests run against built bundle
  artefacts (already the model for vintage today).

**Migration path open:**

- If host or tools later wants its own repo (e.g. CLAP plugin
  development gets heavy enough to want isolation), this ADR doesn't
  prevent it. The 4-repo decision is the floor, not the ceiling.

## Alternatives considered

- **Seven repos (foundation, sdk, vintage, drums, fft, host, tools).**
  Initially proposed; collapsed on cost/benefit. Cross-repo
  coordination overhead exceeds the isolation benefit at single-laptop
  scale. Module-author UX is identical to the 4-repo option.
- **One repo (no bundle split).** Bundles stay as workspace cdylib
  crates inside main repo. Loses: bundles can't iterate independently;
  the "this is what an external module repo looks like" template
  vanishes. Acceptable but undersells the achievement of vintage's
  spike 8 migration.
- **Publish only patches-sdk.** Impossible: Cargo forbids crates.io
  crates depending on git sources. SDK's deps (core, ffi-common) must
  be on crates.io too.
- **Merge ffi-common into core.** Would reduce publish set to 2.
  Rejected: couples ABI stability cadence to Rust API cadence; an
  ergonomic `patches-core` PR could silently change the wire format.

## Out of scope

- Splitting other module families (oscillators, filters, fx) into
  bundles. The drum/fft cut establishes the template; future families
  follow it without a new ADR.
- C++ SDK (E145 deliverable).
- ABI v13 or beyond (E145 owns the ABI surface).
- Restructuring host or tools crate boundaries inside main repo.
- mdBook docs sync. **Explicit decision:** docs are cut loose during
  the transition and rebuilt once everything has landed. Do not gate
  any ticket on doc updates.
