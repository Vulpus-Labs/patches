---
id: "0762"
title: Two-pass rescan (probe + apply) and scan-before-compile on reload
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061", "0044"]
---

## Summary

Implement the cheap probe / restart-when-needed split for `Rescan`,
and fix the reload race where a patch referencing an FFI module fails
because the registry hasn't scanned yet.

## Acceptance criteria

- [ ] `Env::probe_paths(&[PathBuf]) -> RescanProbe { added, replaced,
      removed, unchanged, errors }`. Reads bundle manifests; does not
      keep libraries loaded.
- [ ] `Action::Rescan` runs probe, updates `module_names` /
      status log / diagnostic view from the result, and only sets
      `requires_restart = true` if added/replaced/removed is non-empty.
- [ ] `Action::AddModulePath` runs probe automatically as a preview;
      does not restart.
- [ ] ABI-mismatch / dlopen errors in the probe surface to the GUI
      *before* any restart, with the per-path detail currently emitted
      by `push_scan_details`.
- [ ] `Action::Reload` and `Action::LoadPath` run scan-then-compile in
      that order; a patch that references a module in `module_paths`
      compiles on first try.
- [ ] Tests: probe diff against a known-versioned set; reload of a
      patch that imports an FFI module succeeds without an explicit
      Rescan.

## Notes

Per ADR 0044 §3, no in-place hot-swap. When the probe finds anything
actionable, the apply pass goes through the existing
`request_restart` path. The probe is purely an optimisation: it lets
us skip restarts when there is genuinely nothing to do, and it gives
the user immediate error feedback.
