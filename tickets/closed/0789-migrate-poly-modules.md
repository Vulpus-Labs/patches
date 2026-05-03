---
id: "0789"
title: Migrate Poly* modules to TEMPLATE const
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0786"]
---

## Summary

Migrate the 12 fixed-polyphonic modules (`poly_adsr`, `poly_lfo`,
`poly_osc`, `poly_vca`, `poly_midi_to_cv`, etc.) to declare
`const TEMPLATE`. These modules carry `CableKind::Poly` ports but
their descriptor does not vary with channel count.

## Acceptance criteria

- [ ] Each `Poly*` module declares `const TEMPLATE`.
- [ ] Existing `describe()` override removed.
- [ ] Descriptor output byte-identical pre/post migration.
- [ ] `poly_midi_to_cv` (asymmetric: 1 mono midi in → mixed mono/poly
      outputs) round-trips correctly.
- [ ] `cargo test -p patches-modules` passes.

## Notes

- Channels axis is irrelevant for these but the template still
  declares it (defaults to 1, ignored by build).
- `poly_midi_to_cv` should fit `global + per_axis` cleanly per the
  audit; verify on close.
