---
id: "0367"
title: PolyLayout enum and descriptor integration
priority: medium
created: 2026-04-12
---

## Summary

Add a `PolyLayout` enum to `patches-core` and extend
`ModuleDescriptor`'s poly port builder methods to accept an
optional layout tag. This enables the interpreter to validate
that connected poly ports carry compatible structured data.

## Acceptance criteria

- [ ] `PolyLayout` enum in `patches-core` with variants:
      `Audio` (default/untyped), `Transport`, `Midi`
- [ ] `PolyLayout` derives `Debug, Clone, Copy, PartialEq, Eq`
- [ ] Descriptor builder methods accept optional layout:
      `.poly_input("name", Some(PolyLayout::Midi))` or
      `.poly_input_with_layout("name", PolyLayout::Midi)`
- [ ] Default layout is `Audio` when not specified (backward
      compatible)
- [ ] `PolyMidiIn` input tagged `PolyLayout::Midi`
- [ ] `HostTransport` input tagged `PolyLayout::Transport`
- [ ] Any module with MIDI poly outputs tagged `PolyLayout::Midi`
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- See ADR 0033 for design rationale.
- This ticket adds the data model only — interpreter validation
  comes in 0368.
- `Audio` is compatible with any layout to preserve backward
  compatibility (untyped poly ports can connect to anything).
