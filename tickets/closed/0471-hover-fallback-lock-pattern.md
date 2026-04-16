---
id: "0471"
title: Document or restructure hover fallback lock pattern
priority: low
created: 2026-04-15
status: closed-wontfix
---

## Resolution

Closed without action. The original ticket claimed hover acquires
the workspace lock twice. Re-reading `workspace/mod.rs:561-592`,
`state` is locked once at entry and held across both the
expansion-aware path and the tolerant-AST fallback:
`with_expansion_context` takes `&mut state` and does not release,
and the fallback reuses the same borrow. One lock cycle, not two.
No fragility to document or restructure.

## Summary

`patches-lsp/src/workspace/mod.rs:561-592` (hover) tries
expansion-aware path first, then falls back to tolerant AST.
The pattern acquires the workspace lock twice (once for the
fancy path, once for fallback). Behaviour is correct today
because the second lock gets a fresh view, but the pattern is
fragile and undocumented.

A future refactor that holds the lock across both paths could
silently use stale state for the fallback. Or a third path
added in between could break the assumption.

## Acceptance criteria

- [ ] One of:
      - explicit doc comment on the hover method describing the
        lock cycle and why fallback re-acquires; OR
      - restructure so `with_expansion_context` returns
        cleanly (Option/Result) and fallback paths are visibly
        separate locked regions.
- [ ] Pattern is reusable for any future fancy-then-fallback
      handler (don't bake the pattern into hover only).
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Lowest urgency in the epic. Documenting may be enough;
restructuring is a stretch goal.
