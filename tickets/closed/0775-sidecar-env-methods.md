---
id: "0775"
title: Add sidecar load/save Env methods with JSON transport
priority: medium
created: 2026-04-30
---

## Summary

Add to the `Env` trait:

```rust
fn sidecar_path(&self, patch_path: &Path) -> Option<PathBuf>;
fn load_sidecar(&mut self, path: &Path)
    -> std::io::Result<Option<PersistedSettings>>;
fn save_sidecar(&mut self, path: &Path, settings: &PersistedSettings)
    -> std::io::Result<()>;
```

Ratatui `Env` impl: JSON to `<patch>.patches.state` adjacent to the
`.patches` file. If parent directory is read-only, fall back to XDG
state dir keyed by a hash of the absolute patch path; status-log the
fallback.

CLAP `Env` impl: no-ops returning `Ok(None)` / `Ok(())`. CLAP uses
host `state_load`/`state_save`, not sidecars.

## Acceptance criteria

- [ ] `Env` trait extended; both impls compile.
- [ ] Ratatui sidecar JSON round-trips `PersistedSettings`.
- [ ] XDG fallback exercised by a unit test (read-only parent dir).
- [ ] Missing sidecar returns `Ok(None)`, not an error.
- [ ] Schema version field embedded in the JSON for future migrations.
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

ADR 0063 §5. Depends on 0774. Lifecycle wiring is 0776.
