---
id: "0168"
title: Zero-allocation ParameterMap lookup via two-level storage
epic: E025
priority: medium
created: 2026-03-20
---

## Summary

`ParameterMap::get` currently constructs a throwaway `ParameterKey { name: String, index: usize }` for every lookup, allocating a `String` each time. This matters because `update_validated_parameters` is called on the audio thread during plan adoption (in `AudioCallback::receive_plan`). Before T-0131 introduced `ParameterKey`, the map was `HashMap<String, ParameterValue>` and lookup used `String: Borrow<str>` — zero allocation. The composite key broke that property and there is no way to recover it via `Borrow` in stable Rust without `unsafe`.

The fix is to change the internal storage to `HashMap<String, Vec<(usize, ParameterValue)>>`. The outer lookup uses `Borrow<str>` (zero allocation); the inner linear scan is O(n) on the number of indices for a given name, which is almost always 1.

## Acceptance criteria

- [ ] `ParameterMap` internal storage changed to `HashMap<String, Vec<(usize, ParameterValue)>>`.
- [ ] `get(name, index)` and `get_scalar(name)` perform zero heap allocation.
- [ ] `insert` / `insert_param` upsert correctly into the inner `Vec` (update existing index entry or push).
- [ ] `iter()` return type changed to yield `(&str, usize, &ParameterValue)` — no `ParameterKey` constructed per item.
- [ ] `keys()` return type changed to yield `(&str, usize)` pairs.
- [ ] `entry()` / `entry_param()` (which return `HashMap::Entry`, no longer replicable) replaced with `get_or_insert(name: &str, index: usize, f: impl FnOnce() -> ParameterValue)`.
- [ ] `ParameterDescriptor::matches` signature changed from `matches(&self, key: &ParameterKey)` to `matches(&self, name: &str, index: usize)`.
- [ ] `validate_parameters` in both `modules/module.rs` and `module/module.rs` updated to iterate `(name, index)` pairs and call the new `matches` signature.
- [ ] Module `build()` in both `module.rs` files updated to use `get_or_insert` instead of `entry()/entry_param()`.
- [ ] Planner diff loop in `planner/mod.rs` updated for new `iter()` signature.
- [ ] YAML serialisation in `graph_yaml.rs` updated for new `iter()` signature.
- [ ] `ParameterKey` assessed: remove if no longer needed by any public caller, otherwise keep as a value type with no internal role.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

Root cause is T-0131 (`ParameterKey` newtype), which changed `HashMap<String, ParameterValue>` to `HashMap<ParameterKey, ParameterValue>`. There is no stable-Rust way to implement `Borrow<Q> for ParameterKey` for a borrowed `Q = (&str, usize)` because `Borrow::borrow` must return `&Q` and `Q` is not stored inside `ParameterKey`.

`Arc<str>` for the key name was considered (T-0149) but rejected: it trades a heap allocation for atomic operations, which are also undesirable on the audio thread.

The two-level map approach restores O(1) zero-allocation lookup for the common case (index 0, the vast majority of parameters) and O(n_indices) for the rare indexed case, with a linear scan on what is almost always a one-element Vec.

`entry()` / `entry_param()` cannot be replicated as `HashMap::Entry` handles because the entry now spans two levels. The only callers use the `.or_insert_with()` pattern, which maps directly to `get_or_insert`.
