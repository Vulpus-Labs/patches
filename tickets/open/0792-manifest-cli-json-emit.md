---
id: "0792"
title: patches-manifest --json emits ModuleManifest
priority: medium
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0790", "0791"]
---

## Summary

Extend `patches-tools/src/bin/patches-manifest.rs` with a `--json`
flag that walks `default_registry()` and emits a `ModuleManifest`
JSON document to stdout (or a `--output <path>` file). The text
human-readable form is preserved as the default.

Wire CI to regenerate a checked-in
`patches-lsp/data/module-manifest.json` whenever module descriptors
change, and fail CI if the checked-in file is stale.

## Acceptance criteria

- [ ] `--json` flag emits valid JSON deserializing back to
      `ModuleManifest`.
- [ ] `--output <path>` writes to file.
- [ ] Snapshot test in `patches-tools` confirms manifest matches the
      checked-in file (regenerate via `cargo run -p patches-tools
      --bin patches-manifest -- --json --output ...`).
- [ ] CI step (or pre-commit hook entry) detects staleness and
      surfaces a clear regenerate-with-this-command message.

## Notes

- Default registry is the source of truth. Plugin-supplied templates
  are merged at LSP startup, not at manifest generation time
  (deferred — see ADR 0066 §4 for the FFI plugin path).
