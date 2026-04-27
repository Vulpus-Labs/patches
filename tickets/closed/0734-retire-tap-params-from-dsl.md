---
id: "0734"
title: Retire tap params from DSL grammar (move all config client-side)
priority: medium
created: 2026-04-27
---

## Summary

Tap parameters in the DSL (`~meter+osc(name, osc.window_ms: 100,
meter.decay: 200, ...)`) are obsolete now that the observer holds
fixed-size raw buffers and clients pick decimation / FFT size / RMS
window / peak decay at read time via `SubscribersHandle` typed
options. The server already ignores all tap params; this ticket
removes the syntax surface and the plumbing that supports it.

## Acceptance criteria

- [ ] `tap_param`, `tap_param_key`, `tap_params` rules removed from
      `patches-dsl/src/grammar.pest`.
- [ ] Parser (`patches-dsl/src/parser/expressions.rs`) and AST
      (`patches-dsl/src/ast.rs`) drop tap-param nodes.
- [ ] `TapDescriptor::params` field removed; `TapParamMap` type
      deleted; `tap_schema.rs` deleted or shrunk to just the closed
      set of component names.
- [ ] `validate.rs` no longer checks tap-param keys / values / types.
- [ ] Tree-sitter grammar updated (`patches-lsp/tree-sitter-patches/
      grammar.js`); `parser.c` regenerated; LSP completions
      (`completions/mod.rs`) and hover (`hover/tap.rs`,
      `tree_nav.rs`) drop tap-param entries.
- [ ] All `.patches` fixtures updated to drop tap-param syntax;
      example patches (`examples/*.patches`) updated to match.
- [ ] DSL tests pass (`cargo test -p patches-dsl`); LSP tests pass.
- [ ] Observer side: `build_pipeline` signature drops `params`
      argument or simplifies to ignore the now-empty map.

## Notes

Phase A (ticket 0735, immediate) already moves runtime configurability
to client-driven read opts. This ticket is the syntax cleanup that
follows — purely subtractive, no behaviour change. Done first because
removing the syntax breaks every fixture and several tests, but the
runtime no longer depends on any of it.

Coordinate with anyone who has open `.patches` files using the old
syntax — a one-shot migration script can strip `, key: value` entries
from `~tap(name, ...)` calls.
