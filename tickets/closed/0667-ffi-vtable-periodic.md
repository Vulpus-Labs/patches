---
id: "0667"
title: FFI vtable — add wants_periodic and periodic_update; ABI bump
priority: high
created: 2026-04-24
epic: E114
adr: 0052
depends_on: ["0663"]
---

## Summary

Add a `wants_periodic: bool` field and a `periodic_update` fn pointer to
`FfiPluginVTable`. Bump `ABI_VERSION` from 4 to 5. Update
`export_plugin!` and the plugin SDK to populate both from the module's
`Module::WANTS_PERIODIC` and `Module::periodic_update`. Update the host
loader to read `wants_periodic` once at plugin load and wire the fn
pointer into the plan's periodic dispatch.

## Acceptance criteria

- [ ] `FfiPluginVTable` (`patches-ffi-common/src/types.rs`) gains
      `wants_periodic: bool` and
      `periodic_update: unsafe extern "C" fn(handle, *const CablePoolFfi)`
      (exact signature matches existing conventions in the file).
- [ ] `ABI_VERSION = 5` in `patches-ffi-common/src/types.rs`.
- [ ] `HostEnv` pinning test updated if layout changes; vtable pinning
      test added if not already present.
- [ ] `export_plugin!` macro populates both new slots from the module
      type.
- [ ] Host loader (`patches-ffi`) records `wants_periodic` per loaded
      plugin and includes those slots in the plan's `periodic_indices`.
- [ ] `test-plugins/` rebuilt; `patches-player` loads them and an
      integration test using a periodic test-plugin passes.
- [ ] Version-mismatch path (plugin built at ABI 4, host at ABI 5)
      produces the existing rejection diagnostic, not silent decode
      truncation.

## Notes

ABI bump is additive (new slots at the end), but descriptor-hash inputs
are unchanged — see ADR 0052.  External plugins built against v0.7.0
pre-release will need a rebuild; acceptable pre-1.0.
