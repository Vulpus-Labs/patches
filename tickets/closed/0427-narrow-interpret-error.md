---
id: "0427"
title: Narrow InterpretError to registry binding only
priority: high
created: 2026-04-15
---

## Summary

`patches-interpreter::build` currently mixes structural and binding
errors into a single `InterpretError` enum. After 0426 lands the
structural cases belong to stage 3a's `StructuralError`. Narrow
`InterpretError` to registry-binding cases only: unknown module type,
invalid shape argument, missing required param, wrong param type,
out-of-range param, unknown port, cable kind mismatch, orphan port_ref.

## Acceptance criteria

- [ ] `InterpretError` variants restricted to registry binding.
- [ ] Structural cases previously surfaced here either removed (if
      already caught in stage 3a) or forwarded from 3a's error type.
- [ ] `build()` / `build_with_base_dir()` signatures unchanged; return
      type may carry both structural and binding diagnostics if the
      consumer passes a non-fail-fast policy.
- [ ] Interpreter tests updated; no test should reach a binding error
      through a structurally invalid input.
- [ ] `cargo test -p patches-interpreter`, `cargo clippy` clean.

## Notes

Depends on 0426. `patches-clap::error::CompileError` already
discriminates Interpret separately — this ticket justifies that
distinction at the type level too.
