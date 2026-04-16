---
id: "0473"
title: Style SVG cables by port kind and poly layout
priority: medium
created: 2026-04-16
---

## Summary

Every `PortDescriptor` carries `kind: CableKind`
(`patches-core/src/cables.rs:84`) and `poly_layout: PolyLayout`
(`patches-core/src/cables.rs:100`). Connection resolution already
validates kind match (`patches-interpreter/src/descriptor_bind.rs:595`
and `patches-core/src/graphs/graph.rs:205`). `render_svg`
currently renders every cable with the same `.cable` class.
Thread the kind + layout through and emit a distinguishing CSS
class so mono / poly-audio / poly-transport / poly-midi cables
are visually distinct.

## Acceptance criteria

- [ ] `flat_to_layout_input` (`patches-svg/src/lib.rs:80`)
      resolves each connection's output-port descriptor to
      `(CableKind, PolyLayout)` and attaches it to the layout
      edge.
- [ ] Emitted `<path>` carries one of: `.cable-mono`,
      `.cable-poly-audio`, `.cable-poly-transport`,
      `.cable-poly-midi`.
- [ ] Default theme gives each class a distinct stroke
      (colour and/or width). Document the palette in the
      `Theme` doc comment.
- [ ] Tests cover each of the four classes appearing in SVG
      output for a patch that exercises all layouts.
- [ ] `cargo build`, `cargo test -p patches-svg`,
      `cargo clippy` clean.

## Notes

No first-class "clock" kind exists — clock is mono audio by
convention, indistinguishable from CV from the descriptor alone.
Four classes is the full resolution available.

MIDI layout is only reachable through poly ports; no mono-MIDI
variant needed.

Pairs with 0472 (source hints). Independent — either can land
first.
