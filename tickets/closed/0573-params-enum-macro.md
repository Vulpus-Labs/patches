---
id: "0573"
title: params_enum! macro producing typed enums matched to descriptors
priority: high
created: 2026-04-19
---

## Summary

Introduce a `params_enum!` macro that generates a Rust enum whose
discriminants match the order of variant names in a
`ParameterKind::Enum` declaration. Modules will use this enum to
consume parameter values by variant index once the payload type
changes (ticket 0574). This ticket is additive: no existing module
is migrated here.

## Acceptance criteria

- [ ] New macro `params_enum!` lives in `patches-core` (likely
      `patches-core/src/modules/` alongside existing macros).
- [ ] Given input:
      ```rust
      params_enum! {
          pub enum OscFmType {
              Linear,
              Logarithmic,
          }
      }
      ```
      the macro produces:
      - a `#[repr(u32)]` enum with explicit discriminants `0`, `1`, …;
      - a `pub const VARIANTS: &[&'static str]` exposing variant names
        as snake_case (or the name spelling used in descriptors;
        decision noted in ticket);
      - a `TryFrom<u32>` impl returning `Result<Self, u32>` (error
        carries the out-of-range value);
      - a `pub const DEFAULT: Self` if the macro syntax includes a
        default annotation (optional — decide in ticket).
- [ ] Unit tests cover round-tripping index → enum, name list
      matches declaration order, out-of-range `TryFrom` failure.
- [ ] Macro-generated enums are usable in `match` expressions with
      exhaustiveness checking.
- [ ] No existing module is migrated in this ticket; the only
      consumer is the new unit tests.

## Notes

Naming casing: the DSL uses snake_case variant names (e.g.
`unipolar_positive`). Macro variants are Rust-style (`CamelCase`);
`VARIANTS` const produces snake_case strings for descriptor parity.
The macro converts variant identifiers to snake_case at expansion.

A `default: Variant` annotation in the macro body is convenient
but not strictly required; the descriptor already carries a default
string. Sketch:

```rust
params_enum! {
    pub enum LfoMode {
        Bipolar,
        UnipolarPositive,
        UnipolarNegative,
    }
    default = Bipolar;
}
```

Alternative: skip `default` in the macro and require the module to
wire the default through the descriptor, as it does today. Decide
during implementation; lean toward the simpler (no-default) form
initially.

Out of scope: no descriptor auto-generation. The module still
writes `.enum_param("name", LfoMode::VARIANTS, "bipolar")` or
similar explicitly; the macro just provides the typed enum and
name list.
