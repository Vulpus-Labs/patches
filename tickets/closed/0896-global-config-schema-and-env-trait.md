---
id: "0896"
title: GlobalConfig schema and Env trait methods in patches-plugin-common
priority: high
created: 2026-05-15
epic: E148
adr: 0075
---

## Summary

Introduce the on-disk schema for host-scoped settings (`GlobalConfig`)
in `patches-plugin-common` and add the `Env::load_global_config` /
`Env::save_global_config` trait methods that both hosts will
implement. v1 carries only `bundle_dirs: Vec<PathBuf>` plus a
`schema_version` — designed to grow into default device, oversampling,
monitor preferences without breaking back-compat.

Adds `directories` as a new dependency on `patches-plugin-common`.
Confirm with project owner before merging (per CLAUDE.md).

## Acceptance criteria

- [x] `patches-plugin-common::GlobalConfig` struct with serde
      `Serialize`/`Deserialize`, fields:
      - `schema_version: u32` (constant `GLOBAL_CONFIG_SCHEMA_VERSION = 1`)
      - `bundle_dirs: Vec<PathBuf>` (default empty)
- [x] `GLOBAL_CONFIG_SCHEMA_VERSION` constant exported alongside
      `SIDECAR_SCHEMA_VERSION`. Mismatches surface a clear error rather
      than panicking.
- [x] `Env` trait gains:
      - `fn global_config_path(&self) -> Option<PathBuf>` (returns the
        OS-native `settings.toml` location, or `None` if the env
        cannot resolve one — analogous to `sidecar_path`)
      - `fn load_global_config(&mut self) -> std::io::Result<Option<GlobalConfig>>`
        (`Ok(None)` for missing-but-not-error)
      - `fn save_global_config(&mut self, cfg: &GlobalConfig) -> std::io::Result<()>`
- [x] Default trait impls return `None` / `Ok(None)` / a not-supported
      error, mirroring the sidecar pattern so existing test envs
      compile unchanged.
- [x] TOML round-trip test in `patches-plugin-common` covers an empty
      config, a config with one path, and schema-version mismatch.
- [x] `directories` (and `toml`) added to `patches-plugin-common/Cargo.toml`.
- [x] `just inner -p patches-plugin-common` green.

## Notes

The CLAP plugin and the Ratatui TUI both depend on
`patches-plugin-common`, so this is the natural home. Concrete env
impls (path resolution, atomic write) land in the per-host wiring
tickets (0898, 0899).

Schema format choice: TOML, matching no existing precedent in the
workspace but appropriate for a human-edited preferences file. The
sidecar uses JSON ([patches-plugin-common/src/controller.rs:324]) but
is machine-written; preferring TOML here is deliberate so users can
edit by hand if needed.

Atomic write contract: implementers should write to `settings.toml.tmp`
in the same directory and `rename` over the target, to prevent
corruption on concurrent edits or crashes. Document this in the
trait method's doc comment.
