---
id: "0596"
title: Flip `Module::update_validated_parameters` signature to `&ParamView<'_>`
priority: high
created: 2026-04-20
depends_on: ["0595"]
---

## Summary

Change the `Module` trait in
[patches-core/src/modules/module.rs](../../patches-core/src/modules/module.rs)
from

```rust
fn update_validated_parameters(&mut self, params: &ParameterMap);
```

to

```rust
fn update_validated_parameters(&mut self, params: &ParamView<'_>);
```

Update the default `update_parameters` method and the engine call
site (wired in 0595) to pass the view built over the plan's
`ParamFrame`.

## Scope

- Trait signature + default method body.
- Engine / pool call site switch: dispatch the `ParamView`
  produced during `adopt_plan` rather than the `ParameterMap`.
- Compile break is expected and wide (~60 module impls plus
  out-of-tree consumers). This ticket only touches the trait +
  one call site; 0597 / 0598 restore the build by migrating
  every impl.

## Acceptance criteria

- [ ] Trait signature flipped; default `update_parameters`
      validates against the descriptor, packs into the frame,
      builds a view, and dispatches.
- [ ] Engine dispatch path uses the view. No `ParameterMap`
      reaches `Module::update_validated_parameters`.
- [ ] Workspace build only fails in downstream module
      implementations (expected — fixed by 0597 / 0598).

## Non-goals

- Migrating module implementations (0597, 0598).
- Removing shadow oracle (0600).
- File-variant rejection (0599).
