---
id: "0794"
title: LSP drops patches-modules dependency
priority: medium
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0793"]
---

## Summary

Remove `patches-modules` from `patches-lsp/Cargo.toml`. Remove the
feature-flagged registry fallback added in 0793. Verify the LSP
binary builds, tests, and runs end-to-end against the manifest only.

## Acceptance criteria

- [ ] `patches-lsp/Cargo.toml` no longer lists `patches-modules`.
- [ ] `cargo build -p patches-lsp` succeeds.
- [ ] LSP test suite green.
- [ ] Confirm reduced binary size / compile time (record before/after
      numbers in ticket close-out).
- [ ] No remaining references to `default_registry` or
      `patches_modules::` in `patches-lsp/src/`.

## Notes

- Verify by `cargo tree -p patches-lsp` — module-crate transitive
  deps (FFT, file decoders, MIDI) should drop out.
- If any test depends on the registry, port it to manifest fixtures
  or move the test to a crate that legitimately needs the registry.
