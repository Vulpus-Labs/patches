---
id: "0368"
title: Interpreter validates poly layout compatibility
priority: medium
created: 2026-04-12
---

## Summary

Update the interpreter to check that connected poly ports have
compatible `PolyLayout` tags. Mismatched layouts (e.g. a `Midi`
output wired to a `Transport` input) produce a diagnostic error
at patch load time rather than silent corruption at runtime.

## Acceptance criteria

- [ ] During connection resolution, the interpreter reads
      `PolyLayout` from both source and destination port
      descriptors
- [ ] Connection is allowed if layouts match, or if either side
      is `Audio` (untyped)
- [ ] Connection is rejected with a clear error message if
      layouts are both non-`Audio` and differ (e.g. "cannot
      connect Midi output to Transport input")
- [ ] Error message includes module names, port names, and both
      layout types
- [ ] Existing patches with untyped poly connections continue to
      load without errors
- [ ] Unit test: `Midi` → `Midi` connection succeeds
- [ ] Unit test: `Audio` → `Midi` connection succeeds
- [ ] Unit test: `Midi` → `Transport` connection is rejected
- [ ] Unit test: `Audio` → `Audio` connection succeeds
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- `Audio` acts as a wildcard — it represents traditional untyped
  poly (16-channel audio/CV). This ensures zero breakage for
  existing patches.
- The LSP should also surface these errors as diagnostics, but
  that can be a follow-up ticket.
