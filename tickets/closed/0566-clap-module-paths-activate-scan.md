---
id: "0566"
title: patches-clap persisted module paths + activate-time scan
priority: high
created: 2026-04-19
epic: "E094"
---

## Summary

The CLAP plugin must persist user-configured module search paths in
its CLAP state and scan them at activation, before the first compile.
This is the minimum plumbing required for E109 Phase E to load the
vintage bundle through the CLAP host.

GUI path editing and hard-stop rescan are out of scope here — see
0631 for that work.

## Acceptance criteria

- [ ] Plugin state serialises `module_paths: Vec<PathBuf>` alongside
      existing fields; round-trips on save/load.
- [ ] State format change is either versioned or backward-compatible
      with existing (path + source) saves; a legacy save loads with
      `module_paths = vec![]`.
- [ ] On `activate` (after `state_load` has run), rebuild the
      registry as `default_registry()` plus a `PluginScanner` pass
      over `module_paths`. This happens before the first
      `compile_and_push_plan`.
- [ ] Reactivation (sample-rate change, host cycle) rescans — the
      registry is owned by the activated runtime, not the create-time
      one.
- [ ] A headless integration test builds a bundle, crafts a saved
      state pointing at it, loads the state into a freshly-created
      plugin instance, activates, and asserts the bundle's modules
      appear in the active registry.

## Notes

ADR 0044 §3, §5. This is the sub-ticket split out of the original
0566 scope; the GUI-editable path list, "Rescan" button, hard-stop
reload flow, and post-rescan error surfacing moved to 0631.
