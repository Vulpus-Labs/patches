---
id: "0611"
title: PortFrame wire format + encode/decode helpers
priority: high
created: 2026-04-21
---

## Summary

Implement ADR 0045 §5: `PortFrame` as a single pre-allocated
`Vec<u8>` whose layout is derived from the module's descriptor
port counts at `prepare`. Encode on the control thread; decode
to a borrowed `PortView<'_>` on the audio thread.

## Acceptance criteria

- [ ] `#[repr(C)] struct PortFrameHeader { idx, input_count, output_count }`.
- [ ] `PortFrame` owns `Vec<u8>` sized at construction from a
      `PortLayout` derived from the descriptor.
- [ ] `pack_ports_into(layout, &[InputPort], &[OutputPort], &mut frame)`
      control-thread encoder.
- [ ] `PortView<'a>` borrows `(&layout, &[u8])`; getters for
      `input(i)` and `output(i)`.
- [ ] Round-trip unit test across several port-count shapes.
- [ ] Size computation panics on overflow (not UB).

## Notes

Epic E103. Mirrors the `ParamFrame` / `ParamView` pair from
Spike 3 (E099). Dispatch plumbing lives in Phase B.
