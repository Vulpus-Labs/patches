---
id: "0661"
title: patches-player halt diagnostic and reload prompt
priority: medium
created: 2026-04-24
epic: E113
adr: 0051
depends_on: ["0660"]
---

## Summary

When the engine halts, `patches-player` should print a clear diagnostic
naming the offending module and wait for the user to reload the patch
(the existing file-watch reload path clears the halt).

## Acceptance criteria

- [ ] Main loop polls `processor.halt_info()` on each iteration (it
      already polls for hot-reload events; add alongside).
- [ ] On first observation of `Some(info)`, render a one-shot diagnostic
      via the existing `diagnostic_render` path: module name, slot
      index, first line of the panic payload.
- [ ] Do not spam: track a "last reported halt" flag; re-report only
      after a rebuild has cleared halt and a new halt has occurred.
- [ ] File-watch reload and manual reload both trigger a plan rebuild
      that clears halt; verify no extra work needed beyond the existing
      reload path.
- [ ] Manual test with an example patch containing a deliberately-
      panicking module (hidden behind a cfg flag or a test-only module).

## Notes

No need for a separate "press R to reload" prompt — file watching
already covers the common case. Just make the diagnostic clear enough
that the user knows what to do.
