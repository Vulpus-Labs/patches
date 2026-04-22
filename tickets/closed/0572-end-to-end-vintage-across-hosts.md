---
id: "0572"
title: End-to-end — vintage patch runs in player, CLAP, VSCode/LSP via bundle
priority: high
created: 2026-04-19
epic: "E095"
---

## Summary

Final acceptance: a patch using vintage modules runs unchanged in
`patches-player`, `patches-clap`, and is diagnostics-clean in VSCode,
purely by loading the `patches-vintage` bundle at runtime.

## Acceptance criteria

- [ ] Example patch under `examples/` uses at least one vintage module
      (VChorus is the natural pick).
- [ ] `patches-player --module-path <target>/debug examples/<patch>`
      plays the patch.
- [ ] CLAP plugin, with `module_paths` persisted to point at the same
      dir, loads the bundle on activate and plays the same patch via
      its host.
- [ ] VSCode with `patches.modulePaths` set to the dir opens the patch
      file with no unknown-module diagnostics; hover on a vintage
      module shows its descriptor and module version.
- [ ] Rescan verification:
      1. Bump `module_version` on one vintage module in source.
      2. `cargo build -p patches-vintage`.
      3. Trigger CLAP GUI rescan and LSP `patches.rescanModules`.
      4. The new version is reflected in both environments (hover text
         and/or `ScanReport` summary), without restarting either host.
- [ ] Workspace `cargo build && cargo test && cargo clippy` clean.

## Notes

This is the epic's definition-of-done rolled into one ticket —
intentionally covers multiple hosts because its purpose is to prove
they agree on a single bundle.
