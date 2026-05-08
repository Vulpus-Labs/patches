---
id: "0839"
title: Bundle FFI parameter-frame setup behind `ValidatedParamFrame`
priority: low
created: 2026-05-08
epic: E139
depends_on: "0838"
---

## Summary

After plugin prepare, the FFI loader runs a six-step setup to construct a
`ParamView` for the plugin:

```rust
let filled = ParameterMap::with_overrides(...);
let frame  = validate_and_pack(&module.descriptor, &filled)?;
let layout = compute_layout(&module.descriptor);
let index  = ParamViewIndex::from_layout(&layout);
let view   = ParamView::new(&index, &frame);
module.update_validated_parameters(&view);
```

Each step depends on the previous; the lifetimes of `index` and `frame`
are tied to `view`'s borrow; the order is fragile if one step is moved.
A `ValidatedParamFrame` newtype that owns `frame + layout + index` and
hands out `ParamView<'_>` on demand makes the dependency graph
structural and shrinks the call-site to two lines.

The same setup recurs on hot reload paths and would be reused by future
plugin-rebind tickets.

## Sites

- [patches-ffi/src/loader.rs:245-249](../../patches-ffi/src/loader.rs#L245)
  — six-step setup.
- Any other call sites that construct `ParamView` after `validate_and_pack`
  (search before implementing).

## Proposed shape

```rust
struct ValidatedParamFrame {
    frame: ParamFrame,
    layout: ParamLayout,
    index: ParamViewIndex,
}

impl ValidatedParamFrame {
    fn new(descriptor: &Descriptor, params: &ParameterMap)
        -> Result<Self, BuildError>
    {
        let frame  = validate_and_pack(descriptor, params)?;
        let layout = compute_layout(descriptor);
        let index  = ParamViewIndex::from_layout(&layout);
        Ok(Self { frame, layout, index })
    }

    fn view(&self) -> ParamView<'_> {
        ParamView::new(&self.index, &self.frame)
    }
}
```

Call site:

```rust
let validated = ValidatedParamFrame::new(&module.descriptor, &filled)?;
module.update_validated_parameters(&validated.view());
```

## Acceptance criteria

- [ ] `ValidatedParamFrame` lives in either `patches-core` (alongside
      `validate_and_pack`) or `patches-ffi` if no other crate needs it
- [ ] FFI loader prepare and reload paths use it; per-call setup is one
      `Self::new()` plus one `.view()`
- [ ] No public-API regression in `ParamFrame`, `ParamLayout`,
      `ParamViewIndex`, or `ParamView` — they remain accessible for any
      caller not on the bundled path
- [ ] `just commit -p patches-ffi -p patches-core` clean

## Notes

Depends on 0838 only for ordering — both touch `loader.rs` and serial
landing avoids merge conflicts. If 0838 slips, this can be done first
against the existing prepare path with no logic change.

Naming: `ValidatedParamFrame` matches the existing `validate_and_pack`
function. If a better name surfaces during implementation, change it.
