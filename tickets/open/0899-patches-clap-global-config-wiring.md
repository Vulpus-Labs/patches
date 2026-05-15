---
id: "0899"
title: patches-clap — load + persist global config, UI bundle-dir controls
priority: medium
created: 2026-05-15
epic: E148
adr: 0075
---

## Summary

Wire the CLAP plugin to the new `GlobalConfig` machinery (0896, 0897).
On `init()`, load global config and feed `bundle_dirs` into the
scanner so the plugin sees user-installed `.pxm` bundles without any
environment variable. Add wry-webview UI controls to list, add, and
remove bundle directories; persist mutations immediately and rescan.

## Acceptance criteria

- [ ] CLAP env impl gains `global_config_path`, `load_global_config`,
      `save_global_config`. Path resolution shares
      `directories::ProjectDirs::from("", "", "Patches")` with
      patches-player so both shells hit the same `settings.toml`.
- [ ] `init()` loads global config; failure is non-fatal (in-memory
      fallback). Missing file is silent (the CLAP plugin is **not**
      first-run populator — patches-player is).
- [ ] Webview UI exposes:
      - current `bundle_dirs` list (read-only display)
      - "Add directory…" button → native open-dir dialog (use the
        existing file-dialog plumbing if present; otherwise a plain
        text-entry fallback is acceptable for v1)
      - per-row "Remove" control
- [ ] Each mutation dispatches `Action::AddBundleDir` /
      `Action::RemoveBundleDir`; the env's `save_global_config` writes
      immediately (no debounce — CLAP UI actions are explicit).
- [ ] Save failure surfaces a status-log line; the in-memory list
      still updates so the session works.
- [ ] Sandbox tolerance: under macOS app sandbox (Logic), the plugin
      must load without crashing even if the config directory is
      unreachable. Document the failure mode in the status log.
- [ ] **Never auto-create** the config directory from the CLAP plugin.
      If `save_global_config` finds the parent absent, it errors —
      patches-player owns first-run population.
- [ ] Hand-run smoke under at least one CLAP host (Bitwig Linux is
      the easiest; macOS Logic if available). Captured in the ticket
      as a note; no new automated test.
- [ ] `just commit -p patches-clap` green.

## Notes

The CLAP plugin currently has no UI surface for the module-path list
at all. Even a minimal one (plain text list + add/remove buttons) is a
strict improvement. A polished design can follow once the persistence
plumbing is proven.

The "no auto-create from CLAP" rule is the key sandbox-safety bit
(ADR 0075 §"Sandbox compatibility"). Hosts that block directory
creation will still let the plugin read an existing config and operate
on an in-memory list for the session.
