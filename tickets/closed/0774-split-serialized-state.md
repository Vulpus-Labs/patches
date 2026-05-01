---
id: "0774"
title: Split SerializedState into PatchIdentity + PersistedSettings
priority: medium
created: 2026-04-30
---

## Summary

Today `SerializedState` mixes patch identity (`file_path`,
`dsl_source`) with user settings (`tap_opts`, `window_size`,
`module_paths`). Presets need only the settings half, and the
persistence transports differ between the two halves (path is
local-machine-only; settings are portable).

Split into:

```rust
pub struct PatchIdentity {
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
}

pub struct PersistedSettings {
    pub host_controls: HashMap<String, f32>,  // empty until 0057
    pub tap_opts: HashMap<String, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,
    pub module_paths: Vec<PathBuf>,
}
```

`Action::StateLoad` carries both. CLAP `state_save`/`state_load`
serialises both in one envelope. Presets (0777) serialise only
`PersistedSettings`.

## Acceptance criteria

- [ ] `PatchIdentity` and `PersistedSettings` types in
      `patches-plugin-common`.
- [ ] `Action::StateLoad(PatchIdentity, PersistedSettings)` (or
      equivalent struct) replaces current `StateLoad(SerializedState)`.
- [ ] CLAP shell envelope serialisation updated; round-trip test.
- [ ] `host_controls` field present but empty/unused until 0057.
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

ADR 0063 §5, §6. Depends on 0773 (tap_opts key shape). Blocks 0775,
0776, 0777.
