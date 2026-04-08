---
id: "0250"
title: Structural edge cases (duplicates, diamonds, empty, zero arity)
priority: medium
created: 2026-04-02
---

## Summary

Several structural edge cases in the expander have no test coverage: unused
templates, empty template bodies, duplicate module IDs in the same scope,
diamond wiring (two sources into one port), and zero-arity `[*n]` expansion.

## Acceptance criteria

- [ ] **Unused template:** A template defined but never instantiated. Verify
      expansion succeeds (the template is silently ignored) or, if a warning
      is expected, verify the warning is emitted.
- [ ] **Empty template body:** A template with `in`/`out` ports but no modules
      or connections. Verify expansion succeeds and produces no modules or
      connections from it.
- [ ] **Duplicate module IDs:** Two `module osc : Osc` declarations in the
      same patch scope. Verify the expander returns `ExpandError` (if this is
      illegal) or document the intended behaviour with a passing test.
- [ ] **Diamond wiring:** Two template instances whose outputs both connect to
      the same target port. Verify both connections survive in the FlatPatch
      (they should — the audio engine sums them).
- [ ] **Zero-arity `[*n]` with n=0:** Verify this produces zero connections
      (valid degenerate case) or returns a clear error. Either outcome is
      acceptable as long as it's tested and intentional.

## Notes

- These are all quick tests — each one is a small inline DSL snippet with a
  targeted assertion.
- The duplicate-module-ID case may reveal that the expander silently overwrites
  one module, which would be a latent bug worth catching.
- Epic: E046
