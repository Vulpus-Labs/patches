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

- [ ] Pre-publish checks complete (tickets 0877 + 0878 closed):
  - [ ] `cargo publish --dry-run -p patches-core` succeeds.
  - [ ] `cargo publish --dry-run -p patches-ffi-common` succeeds.
  - [ ] `cargo publish --dry-run -p patches-sdk` succeeds.
  - [ ] All `pub` items documented per `#![warn(missing_docs)]`.
  - [ ] LICENSE present; Cargo.toml metadata complete.
- [ ] Publish in dep order:
  1. `patches-core` 0.7.0 (post-0889 registry merge).
  2. `patches-ffi-common` 0.7.0.
  3. `patches-sdk` 0.7.0.
- [ ] docs.rs builds successfully for all three.
- [ ] `cargo install --dry-run` test from a fresh directory:
      `cargo new test-mod --lib && cargo add patches-sdk` succeeds
      and a minimal module compiles.

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
