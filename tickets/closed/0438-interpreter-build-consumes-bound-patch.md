---
id: "0438"
title: Consume BoundPatch inside patches-interpreter::build
priority: medium
created: 2026-04-15
---

## Summary

`patches-interpreter::build` currently performs its own descriptor
resolution inline: `shape_from_args` → `Registry::describe` →
`convert_params` → `validate_parameters`, followed by port lookups
inside `add_connection` and `validate_port_ref`. Ticket 0432 split
this work into `descriptor_bind` as a standalone pass but deliberately
scoped consumption to LSP only (decision Q1c).

Refactor `build` to run `descriptor_bind::bind` first, then build the
runtime `ModuleGraph` from the `BoundPatch`'s resolved modules and
connections. Fail-fast consumers (player, CLAP) see the first
`BindError` as today — wrapped into the existing fail-fast story at
the consumer level. Runtime-only concerns (graph topology,
song/pattern shape, `MasterSequencer` song bank, file-path resolution)
stay inside `build`.

## Acceptance criteria

- [ ] `patches-interpreter::build` calls `descriptor_bind::bind`
      internally and consumes `BoundPatch.modules` /
      `BoundPatch.connections`, rather than re-resolving descriptors.
- [ ] `InterpretError` narrows to runtime-only codes (connect failure,
      orphan port_ref against the built graph, tracker shape,
      sequencer/song mismatch, file-path resolution); descriptor-level
      failures surface as `BindError` via `BoundPatch.errors` and the
      caller decides how to render them.
- [ ] Fail-fast consumers (`patches-player`, `patches-clap`) updated
      to inspect `BoundPatch.errors` and short-circuit on the first
      one before `build` proceeds, rendering `BindError` through the
      existing diagnostics path.
- [ ] Duplicated helpers (`shape_from_args`, `convert_params`,
      `format_port_label`, `format_available_ports`) move out of
      `patches-interpreter::lib` into a shared location
      (`descriptor_bind` module or a sibling helper module) so
      `build` and `bind` don't drift.
- [ ] `BindError` gets an `#[non_exhaustive]` variant list if
      extensibility is desired; decide and document.
- [ ] All existing `patches-interpreter` tests pass without behaviour
      regression; new tests assert that descriptor-level failures now
      produce `BindError`, not `InterpretError`.
- [ ] `cargo test --workspace`, `cargo clippy` clean.

## Notes

Independent of tickets 0433, 0435, 0436. Touches player and CLAP call
sites — schedule when those crates are otherwise quiet.

Risk: descriptor-bind runs the registry lookup *twice* for every
module (once in bind, once historically inside `build`) until this
ticket lands. The cost is low and bounded, but a before/after perf
check on `patches-player` load time on a representative large patch
will flag any regression.

Part of E081.
