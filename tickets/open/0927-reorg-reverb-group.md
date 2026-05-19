---
id: "0927"
title: Reorg — reverb/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Rename `patches-modules/src/fdn_reverb/` to
`patches-modules/src/reverb/` so future reverb variants share the
directory. The existing `fdn_reverb` module type becomes
`reverb/fdn.rs` (or stays `reverb/fdn_reverb.rs`; pick the cleaner
internal naming and document the choice).

## Acceptance criteria

- [ ] `patches-modules/src/reverb/` exists with FDN reverb sources
      under it; old `fdn_reverb/` removed.
- [ ] Public re-export preserves `patches_modules::FdnReverb`.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only. The internal `line.rs` / `matrix.rs` / kernel
split stays as-is.
