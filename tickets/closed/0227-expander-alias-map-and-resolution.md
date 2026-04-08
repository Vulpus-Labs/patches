---
id: "0227"
title: Expander: build and resolve per-instance alias maps
priority: medium
created: 2026-03-30
---

## Summary

Teach Stage 2 expansion to:

1. When processing a `ModuleDecl` whose shape args contain an `AliasList`,
   compute the integer count and build a per-instance alias map
   (`HashMap<String, u32>`) stored under the instance name.
2. When resolving `PortIndex::Alias(name)` in a connection, look up `name`
   in the alias map for the target module instance. If found, emit the
   corresponding integer index. If not found, return an `ExpandError`.
3. When resolving `ParamIndex::Alias(name)` in a param entry, look up `name`
   in the alias map for the enclosing module instance. Same error behaviour.

After this ticket, the full alias-list + alias-indexed port/param round trip
works end-to-end.

## Acceptance criteria

- [ ] Expander maintains `alias_maps: HashMap<String, HashMap<String, u32>>`
      keyed by module instance name, populated during `ModuleDecl` expansion.
- [ ] `ShapeArgValue::AliasList(names)` in a shape arg: emit `Scalar::Int(names.len())`
      as the shape arg value and populate `alias_maps[instance_name]`.
- [ ] `PortIndex::Alias(name)` resolution: look up `alias_maps[target_module][name]`
      → `u32` index, or `ExpandError` if missing.
- [ ] `ParamIndex::Alias(name)` resolution: look up `alias_maps[enclosing_module][name]`
      → `u32` index, or `ExpandError` if missing.
- [ ] Remove the TODO left in T-0223: `PortIndex::Alias` is now resolved via
      the alias map first, falling back to template env only if the alias map
      has no entry (so existing template-param uses of `<k>` syntax are
      unaffected, since they use `PortIndex::Alias` only after the syntax
      migration).
- [ ] Tests:
  - Full round-trip: `Sum(channels: [drums, bass, guitar])` module, connections
    using `mix.in[drums]` etc., param entries `gain[bass]: 0.6` — flat output
    has integer indices.
  - Alias not found → `ExpandError` with helpful span.
  - Two modules with independent alias maps (`left`/`right` on one, `high`/`low`
    on another) do not conflict.
  - Template instantiation with alias list: `MixBus(n: [drums, bass, guitar])`
    → `n: 3` forwarded, alias map built on the instance.
- [ ] `cargo test` passes, `cargo clippy -- -D warnings` clean.

## Notes

Depends on T-0226.

Alias maps exist only during Stage 2 expansion and are not part of the flat IR.
The flat output contains only integer indices, as specified in ADR 0020.
