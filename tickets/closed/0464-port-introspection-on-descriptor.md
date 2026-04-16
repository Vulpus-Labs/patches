---
id: "0464"
title: Move port introspection onto ResolvedDescriptor
priority: medium
created: 2026-04-15
status: closed-wontfix
---

## Resolution

Closed without action. On re-reading, the work this ticket called
for is already done — 0461 pushed the shared lookup onto the
descriptor, and `ResolvedDescriptor` (`analysis/descriptor.rs:47-138`)
already exposes `find_port`, `has_input`, `has_output`,
`has_parameter`, `input_names` (with dedup for indexed ports),
and `output_names`.

The only remaining inline field access is `hover.rs:574-598`
where the tooltip table iterates **all** inputs/outputs to render
them. That is iteration for rendering, not name-based lookup, and
wrapping it in a method would be premature abstraction.

## Summary

Port-by-name lookup, direction filtering, and indexed-port
collapse are reimplemented across:

- `patches-lsp/src/analysis/descriptor.rs` lines 55–92
  (`find_port`)
- `patches-lsp/src/hover.rs` lines 87–107
  (`hover_for_port_ref`)
- `patches-lsp/src/analysis/validate.rs` lines 118–134 (port
  name dedup for typo suggestions)

Each handler must understand both `ResolvedDescriptor` variants
(Module vs Template) and replicate iteration order. 0461 took a
first cut by sharing `port_lookup`; this ticket finishes the
job by pushing the rest of the introspection onto the type.

## Acceptance criteria

- [ ] `ResolvedDescriptor` exposes methods covering the call
      sites: `ports_by_direction`, `dedup_output_names`,
      indexed-port iteration, etc. (exact names TBD).
- [ ] `hover.rs`, `validate.rs`, `descriptor.rs::find_port`
      call methods; no inline walking of `Module`/`Template`
      variants for port introspection.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Builds on 0461. The principle: descriptor variants are
implementation detail; introspection is interface.
