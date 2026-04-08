---
id: "0151"
title: Emit diagnostics for implicit DSL scale/index defaults
epic: E025
priority: medium
created: 2026-03-20
---

## Summary

The DSL expander in `patches-dsl/src/expand.rs` (lines 250-256) silently defaults missing arrow scales to `1.0` and missing port indices to `0`:

```rust
conn.arrow.scale.unwrap_or(1.0)
from_ref.index.unwrap_or(0)
```

A typo in a DSL source file (e.g. `scale: 0.` instead of `scale: 0.5`) that produces `None` at parse time is silently accepted as `1.0`. The author gets no feedback that their intent wasn't captured.

## Acceptance criteria

- [ ] Define a `Warning` type (or reuse an existing diagnostic type) in the DSL crate.
- [ ] `expand()` (or its caller) collects warnings and returns them alongside the expanded patch.
- [ ] When `scale` is absent on a connection, emit a warning if the connection is non-trivial (i.e. if context suggests a scale was intended — or simply always note it in verbose mode).
- [ ] When `index` is absent on a multi-port module reference, emit a warning that index 0 was assumed.
- [ ] `patches-player` prints collected warnings to stderr after successful patch load.
- [ ] Existing DSL tests pass; add a test that verifies warnings are emitted for the implicit-default paths.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The warnings should not be errors: a DSL file with no explicit scales is valid and intentional for simple patches. The goal is visibility, not strictness.
