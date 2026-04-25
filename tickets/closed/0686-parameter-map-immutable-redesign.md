---
id: "0686"
title: ParameterMap — immutable redesign (defaults + with_overrides)
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

The 0681 mutation run flagged ParameterMap as under-tested. Closer
reading shows the bigger problem is API shape: ParameterMap exposes a
HashMap-flavoured surface (`set`/`take`/`contains_key`/`len`/
`get_or_insert`/two `FromIterator` impls/scalar aliases) but is
actually a **partial → complete assignment** abstraction with three
real consumers: validation, default-fill, frame packing.

Redesign as immutable, two constructors only.

## Design

```rust
impl ParameterMap {
    /// Construct a fully-populated map from descriptor defaults.
    /// Every (name, index) declared by the descriptor is present.
    pub fn defaults(descriptor: &ModuleDescriptor) -> Self;

    /// Construct a fully-populated map by layering sparse overrides
    /// on top of an existing (typically defaults-filled) map.
    pub fn with_overrides(
        base: &Self,
        overrides: impl IntoIterator<Item = (String, usize, ParameterValue)>,
    ) -> Self;

    pub fn get(&self, name: &str, index: usize) -> Option<&ParameterValue>;
    pub fn iter(&self) -> impl Iterator<Item = (&str, usize, &ParameterValue)>;
}
```

No mutating methods. No `set` / `insert_param` / `get_or_insert` /
`take` / `take_scalar` / `len` / `contains_key` / scalar aliases /
2-tuple `FromIterator` / `Display` for `ParameterKey` (unless an
external use surfaces).

Sparse override transport (planner state diff, interpreter→Module)
becomes `Vec<(String, usize, ParameterValue)>` — not a ParameterMap.
This keeps the type honest: a ParameterMap is always complete.

## Migration sites

- `patches-interpreter/src/lib.rs` — currently builds a ParameterMap
  with `insert_param`. Switch to building `Vec<(String, usize,
  ParameterValue)>`; pass to `Module::update_parameters` /
  construction as overrides.
- `patches-core/src/modules/module.rs` (`Module::update_parameters`
  default impl) — replace `get_or_insert` loop with
  `ParameterMap::with_overrides(&defaults, overrides_iter)`.
- `patches-ffi/src/loader.rs` and `patches-wasm/src/loader.rs` — same
  loop, same replacement.
- `patches-core/src/param_layout/mod.rs` (`fill_descriptor_defaults`)
  — collapses into `ParameterMap::defaults`.
- `patches-planner/src/builder/mod.rs` (`resolve_file_params`) —
  iterate input, transform File→FloatBuffer entries, collect into
  Vec<(String, usize, ParameterValue)>, build new ParameterMap via
  `with_overrides(&base, ...)` or a dedicated transform constructor.
- `patches-planner/src/state/mod.rs` (parameter diff) — produces
  `Vec<(String, usize, ParameterValue)>` not ParameterMap.
  `parameter_updates: Vec<(usize, Vec<(...)>)>` flows into engine.
- `patches-core/src/param_frame/pack.rs` — currently merges
  defaults+overrides on read. With pre-merged maps, pack can take a
  single complete `&ParameterMap`. Simplifies signature.
- All test/bench callers using `insert`/`insert_param` directly:
  rebuild via the new constructors or via collected Vec.

## Acceptance criteria

- [ ] New ParameterMap with only `defaults` + `with_overrides` +
      `get` + `iter`.
- [ ] All migration sites updated.
- [ ] `validate_parameters` adapted to take an override iterator (or
      stays with ParameterMap if convenient).
- [ ] All tests pass (`cargo test`).
- [ ] `cargo clippy` clean.
- [ ] Re-run `cargo mutants -p patches-core --file
      'patches-core/src/modules/parameter_map.rs'` — survivors should
      drop to a small set covering real `defaults` / `with_overrides`
      semantics. Record before/after.

## Notes

- Allocates one ParameterMap per default-fill / override-merge.
  Control-thread only; not on the audio path.
- ADR-worthy? Probably yes — this changes the contract of a
  cross-crate type. Add an ADR sketch to the PR.
- **Execute on its own branch off a clean tree.** Current `main`
  working copy has substantial unrelated in-flight changes
  (clap-webview crate, deleted `patches-clap/`, `dist/`); mixing the
  refactor in would tangle reviewable scope.
