---
id: "0566"
title: patches-clap persisted module paths, activate-scan, hard-stop rescan
priority: high
created: 2026-04-19
epic: "E094"
---

## Summary

The CLAP plugin must persist user-configured module search paths, scan
them when the plugin activates, and expose a GUI "Rescan" action that
performs a full hard-stop reload.

## Acceptance criteria

- [ ] Plugin state (CLAP state extension) serialises `module_paths:
      Vec<PathBuf>` alongside existing state fields; round-trips on
      save/load.
- [ ] On `activate` (or equivalent entry point after state is
      deserialised), run `PluginScanner` with the persisted paths
      before the first compile.
- [ ] GUI exposes:
      - editable list of module paths (add/remove);
      - "Rescan" button that triggers the hard-stop reload flow.
- [ ] Hard-stop rescan flow:
      1. Stop processing / deactivate audio;
      2. Drop current `ExecutionPlan` (releases instance `Arc<Library>`);
      3. Scan, update registry;
      4. Recompile the active patch source;
      5. Reactivate / resume processing.
- [ ] If recompilation fails post-rescan, surface the error in the GUI
      and leave the previous (last-good) state untouched — do not
      strand the user with a silent audio dropout.
- [ ] Changing the path list at runtime does not auto-rescan; the user
      presses Rescan. (Avoids accidental audio interruption.)

## Notes

ADR 0044 §3, §5. Softer double-buffered hot-swap is explicitly out
of scope.
