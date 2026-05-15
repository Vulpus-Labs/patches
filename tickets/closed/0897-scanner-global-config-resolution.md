---
id: "0897"
title: PluginScanner resolution tier for global bundle dirs
priority: high
created: 2026-05-15
epic: E148
adr: 0075
---

## Summary

Extend `patches_ffi::PluginScanner` to consult `GlobalConfig::bundle_dirs`
and the default data dir as additional resolution tiers, after caller-
supplied paths and `PATCHES_PLUGIN_PATH`. Resolution order is fixed by
ADR 0075 §"Scanner resolution order".

## Acceptance criteria

- [x] New helper `PluginScanner::with_global_dirs(paths,
      global_bundle_dirs: &[PathBuf])` that produces a scanner whose
      `paths` list is the concatenation of:
      1. `PATCHES_PLUGIN_PATH` env entries (deduplicated, existing
         behaviour preserved)
      2. caller-supplied `paths`
      3. `global_bundle_dirs` (from `GlobalConfig::bundle_dirs` at
         the host)
      4. `default_bundle_dir()` =
         `ProjectDirs::from("", "", "Patches").data_dir().join("bundles")`,
         **only if the directory exists** (never created by the
         scanner).
- [x] De-duplicate paths so an entry that appears in two tiers is
      scanned once. Canonicalise where possible; fall back to
      lexical equality if `canonicalize` fails.
- [x] Existing constructor `PluginScanner::new(paths)` continues to
      work and does **not** read global config — the new tiering is
      opt-in, so unit tests and other callers stay deterministic.
- [x] `stdlib_scanner()` ([patches-ffi/src/scanner.rs]) is
      untouched. Its workspace `target/<profile>/` discovery remains
      orthogonal to user-installed bundles.
- [x] Unit tests cover:
      - empty config, empty env, empty `paths` → scanner has zero
        non-default paths
      - env paths take priority and survive dedup
      - global-config paths appear after caller paths
      - non-existent default data dir is silently skipped (not an
        error)
- [x] `just inner -p patches-ffi` green.

## Notes

Signature deviates from the original ticket draft
(`with_global_config(paths, &GlobalConfig)`) to keep `patches-ffi`
free of `patches-plugin-common`. Adding the latter as a dep would
have pulled `patches-engine` (and its `midir` MIDI runtime),
`patches-dsl`, and `patches-diagnostics` into the plugin loader
crate — substantial surface for a one-field projection. The
host-side wiring tickets (0898, 0899) hold `GlobalConfig` and pass
`cfg.bundle_dirs.as_slice()` into the scanner.

`PluginScanner::paths` is a public field today
([patches-ffi/src/scanner.rs]) — keep it that way to avoid breaking
existing callers. The new helper just produces a scanner with a
pre-merged list.
