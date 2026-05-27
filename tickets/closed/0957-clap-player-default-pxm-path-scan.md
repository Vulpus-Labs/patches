---
id: "0957"
title: CLAP and player rescan use only caller paths, skip env + default .pxm dirs
priority: medium
created: 2026-05-27
---

## Summary

The CLAP plugin never consulted the OS-default `.pxm` bundle directory
(`ProjectDirs::from("","","Patches").data_dir()/bundles`, e.g.
`~/Library/Application Support/Patches/bundles/` on macOS) nor the
`PATCHES_PLUGIN_PATH` env var. Its scan path built a `PluginScanner::new(paths)`
from only the caller-supplied list, and an `if paths.is_empty()` early-return
meant the default dir was never scanned when no paths were configured.

Per ADR 0075 there are four scanner tiers: (1) `PATCHES_PLUGIN_PATH`,
(2) caller paths, (3) `GlobalConfig::bundle_dirs` from `settings.toml`,
(4) OS-default bundle dir if present. Only `PluginScanner::with_global_dirs`
folds in all four; `PluginScanner::new` covers tier 2 only. The player resolved
all four at startup (`common_setup`) but its hot-reload rescan path
(`controller_env::scan_into_registry`) had the same `new()` + empty-guard gap,
so a runtime rescan silently dropped tiers 1 and 4.

## Acceptance criteria

- [x] CLAP scan resolves tiers 1 + 4 in addition to caller paths.
- [x] Default `.pxm` bundle dir is scanned even when no other paths configured.
- [x] Player hot-reload rescan resolves all four tiers (parity with startup).
- [x] Env/default tiers are resolved transiently at scan time, not persisted
      back into `module_paths` (which is saved as `bundle_dirs`).
- [x] `cargo build` + `cargo clippy` clean for both crates.

## Notes

Fix: swap `PluginScanner::new(paths)` → `PluginScanner::with_global_dirs(paths, &[])`
in both scan sites and gate the summary on the resolved `scanner.paths` rather
than the raw input list.

- `patches-clap/src/plugin.rs` — `scan_into_registry`
- `patches-player/src/controller_env.rs` — `scan_into_registry`

Tier 3 is passed empty (`&[]`) at both sites on purpose: the settings.toml
`bundle_dirs` are already merged into the controller's `module_paths` (CLAP does
this in `plugin_init`; player seeds it from `global_cfg.bundle_dirs`). Passing
them again as tier 3 — and, worse, letting tier 1/4 entries leak into
`module_paths` — would pollute the persisted `bundle_dirs` on the next save.
Resolving env + default transiently per scan keeps the saved config clean.

See ADR 0075 (global host config for bundle dirs).
