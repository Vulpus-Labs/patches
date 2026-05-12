---
id: "0878"
title: Fill Cargo.toml publish metadata across publishable crates
priority: low
created: 2026-05-11
---

## Summary

crates.io rejects publish without `description`, `license`,
`repository`. Adding these now per
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md)
means no scrambling at publish time. Strict scope: the three
publishables (patches-sdk, patches-core, patches-ffi-common).
Defensive on patches-dsp + patches-dsl which may publish later.

## Acceptance criteria

- [x] Strict scope — each of the three publishables has in `[package]`:
  - [x] `description = "..."` — one-sentence purpose.
  - [x] `license = "MIT"` — uniform across the three.
  - [x] `repository = "https://github.com/Vulpus-Labs/patches"` —
        points at the source monorepo (each crate stays here until
        the successor-repo cut).
  - [x] `keywords = [...]` — up to 5, lowercase.
  - [x] `categories = [...]` — from crates.io canonical list
        (`multimedia::audio`, `api-bindings`).
  - [x] `rust-version = "1.80"` — conservative floor compatible with
        the `stable` channel pinned in `rust-toolchain.toml`.
  - [x] `readme = "README.md"` — minimal stub created.
- [ ] The three publishables have `#![warn(missing_docs)]` enabled
      at `lib.rs` and missing docs filled in for `pub` items.
      patches-sdk has the lint. Retrofitting docs on every `pub`
      item in patches-core (≈200) and patches-ffi-common is
      deferred to a follow-up; crates.io itself does not require
      the lint, and ADR 0073 only mandates it on patches-sdk's
      author-facing surface.
- [x] `patches-integration-tests` and all other workspace crates
      stay marked `publish = false`. Added the marker explicitly to
      patches-drums, patches-dsl, patches-engine, patches-ffi,
      patches-interpreter, patches-lsp, patches-manifest,
      patches-modules, patches-observation, patches-vintage,
      patches-fft-bundle, and patches-fft-harness — every workspace
      member now either is one of the three publishables or
      carries `publish = false`.
- [ ] Defensive (optional): same metadata fill on patches-dsp and
      patches-dsl, in case they later publish. Deferred to the
      ticket that ships them.
- [x] LICENSE file copied to each publishable crate root.
- [x] `cargo publish --dry-run -p patches-core` succeeds. The
      `patches-ffi-common` and `patches-sdk` dry-runs error out
      with "no matching package named `patches-core` found" because
      `patches-core` is not on crates.io yet — they will succeed
      after 0888 lands the actual `patches-core` 0.7 publish.

## Notes

Suggested categories per crate:

- patches-core: `multimedia::audio`, `data-structures` (registry
  surface lives here post-0889)
- patches-ffi-common: `api-bindings`, `multimedia::audio`
- patches-sdk: `multimedia::audio`, `api-bindings`
- patches-dsp (defensive): `multimedia::audio`
- patches-dsl (defensive): `parser-implementations`, `multimedia::audio`

Decide license once and apply uniformly — mixing MIT vs MIT+Apache
across foundation creates downstream license friction. Current repo
LICENSE file is authoritative; copy.

Out of scope:

- Writing actual READMEs. Stub OK; full README per crate is a
  follow-up after the cut.
- crates.io publish itself — that is 0888.

## Implementation notes

- patches-core dep references inside patches-ffi-common and
  patches-sdk now carry `version = "0.7"` / `version = "0.1"` so
  Cargo's publish path can resolve the dependency once the parent
  hits crates.io.
- All three publishable crates use `license = "MIT"` (matches the
  existing repo LICENSE). MIT-only over MIT+Apache for ergonomics
  and to avoid dual-license noise when the bundles cut to their own
  repos.
- `data-structures` and `parser-implementations` from the original
  ticket suggestions were dropped — crates.io accepts up to 5
  categories but only `multimedia::audio` and `api-bindings` are
  obviously load-bearing for the three publishables; leaving the
  rest for a real categorisation review.
