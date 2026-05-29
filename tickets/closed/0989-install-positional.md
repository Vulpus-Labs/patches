---
id: "0989"
title: Install pipeline — positional install metadata, in-order modules vec
priority: medium
created: 2026-05-29
---

## Summary

The action phase's install pipeline currently uses two `NodeId`-keyed
side-tables:

- `Instantiated::install_meta: HashMap<NodeId, InstallMeta>` — `build_draft`
  does `install_meta.get(&id).ok_or_else(InternalError)` per Install node.
- `Instantiated::modules: HashMap<NodeId, (Box<dyn Module>, ParamState)>` —
  `assemble` does `modules.remove(&id).ok_or_else(InternalError)` per install.

Both side-tables carry a "missing entry → internal error" path that exists
only because the data is keyed by `NodeId` instead of being aligned
positionally with the install decisions. The decisions are already iterated
in execution order; the install metadata and module instances can ride that
order directly.

Replace both with positional, install-order collections:

```rust
struct Instantiated {
    instance_ids: HashMap<NodeId, InstanceId>,   // still keyed (read by survivors too)
    installs: Vec<InstalledNode>,                // install-order; no NodeId lookup
}

struct InstalledNode {
    node_id: NodeId,
    instance_id: InstanceId,
    module: Box<dyn Module>,
    param_state: ParamState,
    meta: InstallMeta,
}
```

`build_draft` walks `installs` in order alongside the install branch of
`decisions`. `assemble` drains `installs` directly into `new_modules` /
`new_module_param_state`. No `HashMap::get` / `remove` on the install path,
no `InternalError` for missing entries.

## Acceptance criteria

- [ ] `Instantiated` carries a positional `Vec<InstalledNode>` (or equivalent
      install-order container) instead of two NodeId-keyed HashMaps.
- [ ] `build_draft` has no `install_meta.get(&id).ok_or_else(...)` path. The
      install branch reads `InstallMeta` directly from the positional entry.
- [ ] `assemble` has no `modules.remove(&id).ok_or_else(...)` path. The
      module + param state flow positionally into `new_modules` /
      `new_module_param_state`.
- [ ] `installs: Vec<(NodeId, usize)>` in `PlanDraft` is updated (or removed
      in favour of carrying the per-install port vectors through, so
      `assemble`'s `node_states.get(id)` for `set_ports` also disappears).
- [ ] `instance_ids` remains a `HashMap<NodeId, InstanceId>` because
      Update-branch lookups are keyed by NodeId during `build_draft`. Verify
      that map stays read-only after `instantiate` returns.
- [ ] Audio goldens bit-identical; `just push` green.

## Notes

Part of epic **E162**. Depends on 0988. The "missing-entry InternalError"
paths this ticket removes are dead in correct executions — they exist
because the data layout is loose. Tightening to positional storage moves the
guarantee into the type and removes three error branches from the action
phase. Open question: whether `set_ports` belongs on `assemble`'s positional
walk or can be hoisted into `instantiate` (cleaner; depends on whether
`build_draft` needs the modules unmodified for any later read — currently it
does not).
