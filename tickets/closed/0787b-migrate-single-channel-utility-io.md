---
id: "0787b"
title: Migrate single-channel utility and IO modules to TEMPLATE
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0786"]
---

## Summary

Migrate the remaining single-channel modules (utility, IO,
math/signal helpers) in `patches-modules` to declare `const TEMPLATE`
and delete their `describe()` override.

In scope (representative): `audio_out`, `audio_in`, `mixer/mono` (if
single-channel), `gain`, `attenuverter`, `crossfade`,
`stereo_splitter`, `stereo_joiner`, `cv_*`, math ops. Confirm final
list against audit.

## Acceptance criteria

- [ ] Every in-scope module declares `const TEMPLATE`.
- [ ] Each module's `describe()` override removed.
- [ ] Descriptor output byte-identical pre/post migration.
- [ ] `cargo test -p patches-modules` passes.
- [ ] After this ticket, no remaining `describe()` overrides on
      single-channel modules — verify with grep.

## Notes

- Mechanical; the residual ~10-12 modules.
- Final ticket of the single-channel batch; on close, only
  channel-aware (0788) and poly (0789) batches remain.
