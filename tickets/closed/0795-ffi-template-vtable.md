---
id: "0795"
title: FFI ABI 8 — module_template vtable replaces describe
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0790"]
---

## Summary

Replace the FFI `describe(shape) -> FfiBytes` vtable entry with
`module_template() -> *const u8` (returning a pointer + length to a
serialized `ModuleDescriptorTemplate` blob owned by the plugin).
Bump `ABI_VERSION` from 7 to 8.

## Acceptance criteria

- [ ] `FfiPluginVTable` updated in `patches-ffi-common/src/types.rs`.
- [ ] `ABI_VERSION = 8`; loader rejects v7 plugins with clear error.
- [ ] Host decodes the template blob once at plugin load; per-instance
      descriptor build uses `template.build_channels(channels)`
      locally on the host side.
- [ ] `patches-ffi/src/loader.rs` updated; no per-instance describe
      call.
- [ ] Loader integration tests pass.

## Notes

- Decide the blob serialization (bincode? postcard? JSON for
  diagnosability?). Smaller is better but the blob is loaded once per
  plugin at host startup — JSON is acceptable.
