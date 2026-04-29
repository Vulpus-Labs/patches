---
id: "0733"
title: Manual DAW test pass (Bitwig + Reaper, macOS + Windows)
priority: medium
created: 2026-04-26
epic: "E124"
---

## Summary

End-to-end manual test pass on the rebuilt webview across the
supported DAW / OS matrix. File notes against this ticket.

## Acceptance criteria

- [ ] Bitwig macOS: load patch, reload, browse, scan-path add /
      remove / rescan, halt-recover, resize, close / reopen.
- [ ] Reaper macOS: same matrix.
- [ ] Reaper Windows: same matrix.
- [ ] No leaks on close / reopen (verified via process memory delta
      across 10 cycles).
- [ ] Issues found logged as follow-up tickets, not fixed in this
      one (unless trivial).
