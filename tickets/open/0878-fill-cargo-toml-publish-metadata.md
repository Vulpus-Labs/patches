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

- [ ] Strict scope — each of the three publishables has in `[package]`:
  - [ ] `description = "..."` — one-sentence purpose.
  - [ ] `license = "MIT OR Apache-2.0"` (or chosen policy, applied
        uniformly).
  - [ ] `repository = "https://github.com/.../<repo>"` — points to
        the future successor repo for that crate (ADR 0073).
  - [ ] `keywords = [...]` — up to 5, lowercase.
  - [ ] `categories = [...]` — from crates.io canonical list (e.g.
        `multimedia::audio`, `api-bindings`).
  - [ ] `rust-version = "1.XX"` — match `rust-toolchain.toml`.
  - [ ] `readme = "README.md"` — even if minimal stub.
- [ ] The three publishables have `#![warn(missing_docs)]` enabled
      at `lib.rs` and missing docs filled in for `pub` items.
- [ ] `patches-integration-tests` and all other workspace crates
      stay marked `publish = false` (default for new crates in this
      workspace; verify explicitly).
- [ ] Defensive (optional): same metadata fill on patches-dsp and
      patches-dsl, in case they later publish.
- [ ] LICENSE file copied to each crate root (or `license-file` set
      to workspace root LICENSE).
- [ ] `cargo publish --dry-run -p <crate>` succeeds for at least
      patches-core, patches-dsp, patches-registry, patches-ffi-common
      as smoke check.

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
