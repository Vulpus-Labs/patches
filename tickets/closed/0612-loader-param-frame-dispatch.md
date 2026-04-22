---
id: "0612"
title: Loader — dispatch ParamFrame bytes via new ABI; drop JSON
priority: high
created: 2026-04-21
---

## Summary

Rewrite `patches-ffi/src/loader.rs` `update_validated_parameters`
path. Take the `ParamFrame` already present in the plan
(post-Spike 5), pass `(bytes.as_ptr(), bytes.len(), &HOST_ENV)`
to the plugin's extern entry. Delete the
`json::serialize_parameter_map` call and the surrounding allocator
churn.

## Acceptance criteria

- [ ] `DylibModule::update_validated_parameters` calls the new
      extern fn via the typedef from 0609.
- [ ] No `json::` reference in this code path.
- [ ] No `Vec`/`Box`/`String` construction on the call path —
      bytes come straight from the plan's `ParamFrame`.
- [ ] `HostEnv` stored as `&'static HostEnv` via `OnceLock` at
      loader init.
- [ ] `cargo build -p patches-ffi` clean (plugin side broken
      until E105/E106 — expected).

## Notes

Epic E104.
