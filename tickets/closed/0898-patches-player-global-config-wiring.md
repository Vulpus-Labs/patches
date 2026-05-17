---
id: "0898"
title: patches-player — load + persist global config, TUI bundle-dir action
priority: medium
created: 2026-05-15
closed: 2026-05-17
status: closed
epic: E148
adr: 0075
---

## Summary

Wire `patches-player` to the new `GlobalConfig` machinery (0896, 0897).
On startup, load global config from the OS-native location; merge its
`bundle_dirs` into the scanner. Add a TUI action that lets the user
add/remove bundle directories at runtime; mutations persist to the
global config via the existing sidecar debounce loop.

## Acceptance criteria

- [x] `RatatuiEnv::global_config_path` returns
      `directories::ProjectDirs::from("", "", "Patches")
      .map(|p| p.config_dir().join("settings.toml"))`. Missing
      `ProjectDirs` → `None` (extremely rare; tolerate gracefully).
- [x] `RatatuiEnv::load_global_config` reads + deserialises the file.
      Missing file → `Ok(None)`. Schema-version mismatch → surface a
      status-log entry and treat as missing (don't crash, don't
      overwrite).
- [x] `RatatuiEnv::save_global_config` writes atomically (`tmp` +
      `rename`). Creates the config directory if missing
      (`patch_player` is the canonical first-run populator).
- [x] `common_setup` (or its replacement) constructs the scanner via
      `PluginScanner::with_global_config(cli_module_paths,
      &global_cfg)`. CLI `--module-path` entries are **not** persisted
      — they remain per-invocation.
- [x] New `Controller::Action::AddBundleDir(PathBuf)` and
      `Action::RemoveBundleDir(PathBuf)` in `patches-plugin-common`:
      - update `controller.module_paths` in-memory
      - emit a delta with `persistable_changed = true` so the
        existing debounce loop flushes to global config
      - trigger a registry rescan and refresh `module_names`
- [x] TUI surface for the action — at minimum, a keybinding +
      prompt; full panel can wait. Status-log entry on success and
      on save failure.
- [x] On exit, any pending debounced save flushes (mirror the
      existing `flush_sidecar` end-of-run hook).
- [x] Integration smoke: launch `patch_player`, add a directory via
      the action, exit, relaunch — the directory is still scanned.
      Captured as a hand-run check in the ticket; no new automated
      test required.
- [x] `just commit -p patches-player` green.

## Notes

The existing `flush_sidecar` lives in
[patches-player/src/main.rs:59]. The global-config flush is a sibling:
same debounce window (`SIDECAR_DEBOUNCE`), same on-exit drain. Consider
a small refactor so both share one "dirty timestamp + flush" struct,
but only if it doesn't bloat the diff.

This ticket inherits ADR 0063's design constraint that the env owns
all I/O — `Controller` never touches the filesystem directly.

## Closure (2026-05-17)

- `RatatuiEnv::{global_config_path,load_global_config,save_global_config}`
  implemented; atomic tmp+rename write, schema mismatch surfaces as
  `io::Error::InvalidData` and lands in the view log.
- `common_setup` rebuilt around `PluginScanner::with_global_dirs` —
  loads `settings.toml` via the env, merges CLI overrides without
  persisting them, and seeds the controller's bundle-dir list from
  global config.
- `Controller::Action::{AddBundleDir, RemoveBundleDir, AddBundleDirPick}`
  added alongside `StateDelta.global_config_changed`. `AddBundleDir`
  hot-scans the new dir via `Env::scan_into`; `RemoveBundleDir` logs a
  "restart to drop loaded modules" hint.
- `EnvSideChannel.scanned_paths` dedupes `scan_into` against startup
  scans and any prior live scans, so patch reloads no longer log
  skip-low-version noise for already-loaded paths.
- TUI `b` / `B` keybindings open an inline prompt; the footer doubles
  as the input line. Commits enqueue an action that the main loop
  drains and dispatches through `Controller::apply`.
- Global-config debounce mirrors the sidecar (`SIDECAR_DEBOUNCE`),
  with a sibling end-of-run flush.
- Sidecar `module_paths` is now scrubbed on save and ignored on load
  (ADR 0075 §"Persistence boundary"); existing files stay forward-
  compatible — the field remains in the schema, just stops carrying
  data.
- `just commit -p patches-player -p patches-plugin-common` green.
