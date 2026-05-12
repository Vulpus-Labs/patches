---
id: "0888"
title: Publish patches-sdk + patches-core + patches-ffi-common to crates.io
priority: low
created: 2026-05-11
---

## Summary

Publish the three crates that
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md)
identifies as the published surface: patches-sdk, patches-core,
patches-ffi-common. Initial release 0.7.0.

All three live in the main `patches` Cargo workspace. crates.io
sees them as separate crates that happen to share a repo —
standard pattern (tokio, serde, bevy).

Phase B of the cut. Sequencing: this ticket must complete before
bundle repo cuts (0882-0884) so bundle Cargo.toml can reference
`patches-sdk = "0.7"` from crates.io rather than git-tag.

## Acceptance criteria

- [x] Pre-publish checks complete (tickets 0877 + 0878 closed):
  - [x] `cargo publish --dry-run -p patches-core` succeeds.
  - [x] `cargo publish --dry-run -p patches-ffi-common` succeeds.
  - [x] `cargo publish --dry-run -p patches-sdk` succeeds.
  - [x] All `pub` items documented per `#![warn(missing_docs)]`.
  - [x] LICENSE present; Cargo.toml metadata complete.
- [x] Publish in dep order:
  1. `patches-core` 0.7.0 (post-0889 registry merge).
  2. `patches-ffi-common` 0.1.0 (kept on its own version line; no
     registry-merge breakage to signal, so the 0.7.0 jump from the
     original plan was dropped).
  3. `patches-sdk` 0.7.0.
- [x] docs.rs builds successfully for all three.
- [x] `cargo install --dry-run` test from a fresh directory:
      `cargo new test-mod --lib && cargo add patches-sdk` succeeds
      and a minimal module compiles.

## Resolution

All three crates uploaded from commit 95c4f29 ("Close 0878"):

- `patches-core@0.7.0` — registry-merged content.
- `patches-ffi-common@0.1.0` — published on the existing 0.1 line.
  `patches-sdk@0.7.0` pins `patches-ffi-common = "0.1"`.
- `patches-sdk@0.7.0` — re-exports patches-core + ffi-common.

docs.rs renders all three (HTML for `Module`, `patches_sdk`, and
`patches_ffi_common` confirmed). Smoke test in `/tmp/sdk-smoke`:
`cargo new sdk-smoke --lib && cargo add patches-sdk` then a
passthrough `Gain` module (the seven SDK imports the gain
test-plugin uses) compiles clean against crates.io versions with
no path overrides.

## Notes

Sequencing dependency: tickets 0877 (pub audit), 0878 (Cargo.toml
metadata), 0879 (reserved names), 0889 (registry merge) all must
close first. 0873-0876 (workspace prep) likewise.

patches-core 0.6.x → 0.7.0 is a major version bump signalling the
registry merger (ADR 0040 reversal). Document in CHANGELOG.

**Not published to crates.io:**

- Bundle cdylibs (vintage, drums, fft-bundle) — ship as GitHub
  Release tarballs containing cdylib + descriptor JSON.
- Host binaries (player, clap, lsp, etc.) — ship as GitHub Releases.
- Internal foundation crates (dsp, dsl, manifest, interpreter,
  diagnostics, svg, io, io-ring, ffi loader, alloc-trap,
  tracker-core, engine, planner, observation, modules, host, cpal,
  plugin-common, profiling, integration-tests) — workspace-internal,
  git-only.

Optional later: publish patches-dsp and patches-dsl if external
demand emerges. Reserved names exist (ticket 0879).

Out of scope:

- 1.0 stabilisation. 0.x for the foreseeable future.
- Yanking policy beyond the crates.io defaults.
- crates.io org / team setup (prerequisite).
- Publishing further foundation crates (dsp, dsl etc.) — names are
  reserved, real publishes deferred until demand.
