---
id: E146
title: Externalise bundles; publish 3-crate SDK from main repo
status: open
created: 2026-05-11
---

## Summary

Per [ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md):

- **Main repo** (`patches`) stays as-is, minus the bundle
  extractions. Single Cargo workspace covering foundation + host +
  tools.
- **Bundle repo** (`patches-bundles`) holds vintage, drums, and fft
  as a four-crate workspace shipping cdylib artefacts. Originally
  planned as three separate repos; consolidated after the cut
  proved the per-bundle isolation wasn't earning its keep.
- **Three crates** (patches-sdk, patches-core, patches-ffi-common)
  publish to crates.io. Everything else stays workspace-internal or
  git-only.
- **patches-registry merges into patches-core** as a `registry`
  submodule (ADR 0040's split is undone).

Pre-requisite: [E145](E145-external-sdk-abi-cut.md) must close so the
FFI ABI is stable before bundle authors are exposed to it.

Module-author day one (post-publish):

```toml
[dependencies]
patches-sdk = "0.7"

# Optional, for module authors who want DSP kernels or FFT harness:
patches-dsp = { git = "https://github.com/.../patches", tag = "v0.7.0" }
```

## Tickets

### Phase A — main-repo preparation

All Phase A work happens inside the existing workspace. No repos
created yet.

- **0889** — Merge `patches-registry` into `patches-core` as a
  `registry` submodule. Update every `use patches_registry::*` in
  the workspace. Sequencing: do first; downstream tickets benefit
  from the simpler import surface.

- **0873** — Extract drums into `patches-drums` workspace crate
  (`cdylib + rlib`). Move `patches-dsp::drum::*` submodule into the
  new crate's `primitives/`. Add `export_modules!` listing the 7
  drum types.

- **0874** — Extract pitch_shift + convolution_reverb into a
  `patches-fft/` workspace directory with two crates:
  `patches-fft-harness` (rlib: slot_deck, window_buffer,
  partitioned_convolution, spectral_pitch_shift moved from
  patches-dsp) and `patches-fft-bundle` (cdylib + rlib).
  `RealPackedFft` stays in patches-dsp.

- **0875** — Create `patches-sdk` re-export crate. Surface: Module
  trait + descriptors + registry (from patches-core post-0889),
  `export_modules!` macro (from patches-ffi-common). Does NOT
  re-export patches-dsp — module authors who want kernels git-dep
  it directly. Migrate vintage, drums, fft-bundle, test-plugins to
  consume patches-sdk.

- **0876** — Drop drum + pitch_shift + convolution_reverb
  registrations from `patches-modules::default_registry()`. Wire
  stdlib bundles (vintage, drums, fft) into the default
  `PluginScanner` search path.

- **0877** — Audit `pub` surfaces on the 3 publishable crates
  (patches-sdk, patches-core, patches-ffi-common). Demote `pub`
  → `pub(crate)` where unused externally. Other foundation crates:
  optional defensive audit, not blocking.

- **0878** — Fill Cargo.toml publish metadata on the 3 publishables
  (description, license, repository, keywords, categories,
  rust-version, missing_docs). Defensive metadata on other
  foundation crates optional.

- **0879** — Reserve `patches-sdk`, `patches-core`, `patches-ffi-common`
  on crates.io with 0.0.0 placeholders. Plus defensive reservations
  for `patches-dsp`, `patches-dsl`, `patches-fft-harness`. **User
  action.**

### Phase B — publish + bundle cuts

- **0888** — Publish patches-sdk, patches-core, patches-ffi-common to
  crates.io as 0.7.x. patches-core 0.6 → 0.7 marks the registry
  merge.

- **0882 / 0883 / 0884** — Cut patches-vintage, patches-drums, and
  patches-fft out of the main workspace. After an initial split into
  three repos, the bundles were consolidated into a single
  `patches-bundles` repo as a four-crate Cargo workspace
  (patches-vintage, patches-drums, patches-fft-harness,
  patches-fft-bundle). Shared CI / licence / release cadence; the
  three-repo split added overhead without compensating decoupling.
  Bundle repo at `github.com/Vulpus-Labs/patches-bundles`.

## Tickets removed from the original 16-ticket plan

The earlier draft of this epic included tickets 0880, 0881, 0885,
0886, 0887 — cut foundation, sdk, host, tools repos and orchestrate
cross-repo CI. None apply under the coarse-grained 4-repo decision.
They are closed/superseded.

## Sequencing

Phase A linear:

```text
0889 (registry merge) → 0873 (drums) → 0874 (fft) → 0875 (sdk crate)
                                                       ↓
0876 (default_registry + PluginScanner)
0877, 0878, 0879 — parallel with Phase A or after
```

Phase B:

```text
0888 (publish to crates.io) → 0882 + 0883 + 0884 (bundle cuts)
                              → consolidate into patches-bundles
```

0888 can technically happen before bundle cuts; bundle Cargo.toml
becomes simpler if it can `patches-sdk = "0.7"` from crates.io rather
than git tag of the main repo.

The bundle cuts initially landed as three separate repos
(patches-vintage, patches-drums, patches-fft) before being merged
into a single patches-bundles workspace.

## Out of scope

- Splitting other module families. The drum/fft cut establishes the
  template; future families either join `patches-bundles` or carve
  out their own repo if the coupling really is independent.
- C++ SDK (E145 deliverable).
- ABI v13 or beyond (E145 owns).
- mdBook docs sync. **Explicit decision:** docs cut loose during
  the transition; rebuild post-cut.
- Per-platform CI matrix for bundle cdylibs (each bundle repo owns
  its own).

## Risks

- **patches-core 0.7 breaking change.** The registry merge is a
  breaking change to imports across the workspace. Single-PR change
  but touches many files. Mitigated by being entirely mechanical
  (`use patches_registry::*` → `use patches_core::registry::*`).
- **First crates.io publish.** First time names are reserved + first
  time a real 0.7.0 ships. Practise on a placeholder repo if needed.
- **Bundle drift after cut.** Once bundles live in separate repos,
  they iterate on their own; a drift between bundle's pinned
  patches-sdk version and host's expectations is possible. Mitigated
  by `descriptor_hash` ABI check at load time (E145).

## Notes

ADR 0073 is the design; this epic is execution.
