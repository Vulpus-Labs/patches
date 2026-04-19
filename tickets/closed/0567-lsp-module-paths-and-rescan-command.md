---
id: "0567"
title: patches-lsp workspace config for module paths + rescan custom command
priority: high
created: 2026-04-19
epic: "E094"
---

## Summary

Teach the LSP to read a `patches.modulePaths` setting via
`workspace/configuration`, scan those paths on init and on config
change, and expose a `patches/rescanModules` custom LSP request.

## Acceptance criteria

- [ ] On initialisation, LSP pulls `patches.modulePaths` (array of
      absolute/workspace-relative strings) via `workspace/configuration`.
- [ ] Paths resolved against workspace root; non-existent paths reported
      as warnings (not fatal).
- [ ] Initial scan populates the LSP's registry before serving the
      first diagnostics batch.
- [ ] `workspace/didChangeConfiguration` with updated `modulePaths`
      re-reads the setting but does *not* auto-rescan (consistent with
      CLAP). User must invoke rescan explicitly.
- [ ] Custom request `patches/rescanModules` (no params) performs a
      full rescan, refreshes registry, and re-publishes diagnostics for
      all open documents. Response is the `ScanReport` (serde).
- [ ] Hover and diagnostics reflect module names and descriptors loaded
      from bundles.

## Notes

ADR 0044 §5. In-process scan is acceptable for now; subprocess
isolation is a future ADR.
