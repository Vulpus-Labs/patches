---
id: "0879"
title: Reserve patches-* crate names on crates.io
priority: low
created: 2026-05-11
---

## Summary

**User action** (requires crates.io login). Publish 0.0.0 placeholder
versions to prevent name-squatting before the real publish (ticket
0888).

Strict scope per
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md):
the three crates that will publish. Defensive scope: a small handful
of foundation-adjacent names that may publish later.

Skip entirely: cdylib bundle names (vintage, drums, fft-bundle —
ship as artefact tarballs, not crates.io library crates), binary
crate names (player, clap, lsp, etc. — GitHub Releases), and
patches-vscode (TypeScript).

## Acceptance criteria

- [ ] Strict reservation — must-publish names:

```text
patches-sdk
patches-core
patches-ffi-common
```

- [ ] Defensive reservation — may publish later:

```text
patches-dsp
patches-dsl
patches-fft-harness
```

- [ ] Each placeholder has:
  - [ ] `description = "Reserved for the Patches project — see <repo URL>"`
  - [ ] `license` matching the chosen policy from ticket 0878.
  - [ ] `repository` URL.

## Notes

A placeholder lib.rs of `//! Reserved.` is sufficient. Avoid `pub`
items so 0.x consumers don't accidentally start using a stub.

After reservation, real publishes (ticket 0888) bump to 0.7.0.
crates.io allows successive versions of an owned crate freely.

Out of scope:

- Setting up a crates.io org / team account if not yet done. That is
  a prerequisite to running this ticket.
- Reserving cdylib bundle / binary / TypeScript names. Per ADR 0073
  these never publish to crates.io.
