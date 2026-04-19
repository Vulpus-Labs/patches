---
id: "0565"
title: patches-player --module-path CLI and pre-compile scan
priority: medium
created: 2026-04-19
epic: "E094"
---

## Summary

Add `--module-path <DIR>` (repeatable) to `patches-player`. Before
compiling the patch, run `PluginScanner` with those paths against
the registry. Log the `ScanReport` summary.

## Acceptance criteria

- [ ] `patches-player --module-path DIR [--module-path DIR …] PATCH`
      parses into a `Vec<PathBuf>`.
- [ ] Scan runs once, before `runtime.compile`, mutating the local
      registry.
- [ ] Scan summary printed: N loaded, M replaced, K skipped, errors
      listed with path + reason.
- [ ] Patches referencing externally-loaded modules compile and play.
- [ ] No rescan capability — exit/restart is the documented refresh
      path.

## Notes

ADR 0044 §5. Single-shot lifecycle means no `Arc<Library>` drop
concerns beyond process exit.
