---
id: E148
title: Global host config for `.pxm` bundle directories
status: closed
created: 2026-05-15
closed: 2026-05-17
adr: 0075
---

## Goal

Give both hosts (`patch_player` and the CLAP plugin) a durable,
cross-platform place to record host-scoped settings — most pressingly
the list of directories to scan for `.pxm` bundles. Today the CLAP
plugin has no way at all to remember a scan directory; `patch_player`
forgets the `--module-path` list at exit. Both shells learn to read
and write the same `settings.toml` via a shared `Env::load_global_config`
/ `Env::save_global_config` pair in `patches-plugin-common`. See
[ADR 0075](../../adr/0075-global-host-config-for-bundle-dirs.md).

## Scope

- New `GlobalConfig` struct + serde TOML round-trip in
  `patches-plugin-common`, schema-versioned alongside
  `PersistedSettings`. v1 fields: `schema_version: u32`, `bundle_dirs:
  Vec<PathBuf>`. Future host-scoped settings extend this struct.
- New `Env::load_global_config` / `Env::save_global_config` trait
  methods. Both hosts implement them against
  `directories::ProjectDirs::from("", "", "Patches")`.
- Single new dependency: `directories` crate in
  `patches-plugin-common`. Approve before adding (per CLAUDE.md
  "ask before adding new dependencies").
- `patches_ffi::PluginScanner` gains a resolution tier: after
  caller-supplied paths and `PATCHES_PLUGIN_PATH`, it consults the
  global config's `bundle_dirs`, then the default data dir
  (`ProjectDirs.data_dir()/bundles/`) if it exists. Order documented
  in ADR 0075.
- `patch_player`: load global config on startup, merge with CLI
  `--module-path` for the in-memory list, add a TUI action to
  add/remove bundle dirs that writes back to global. Uses existing
  sidecar debounce machinery.
- `patches-clap`: load global config on `init()`; add UI controls to
  add/remove bundle dirs and trigger a controller-level rescan;
  tolerate read-only / sandboxed file system.
- `Controller::Action::AddBundleDir(PathBuf)` /
  `Action::RemoveBundleDir(PathBuf)`: live in
  `patches-plugin-common`, persist via the env, rescan via existing
  controller registry-rebuild path.

## Out of scope

- GUI installer or bundle manager. Users drop `.pxm` files into the
  default data dir themselves (or point the host at their own dir).
- Per-host overrides — both hosts read the same `bundle_dirs` list.
- Migrating sidecar contents — sidecars remain patch-scoped.
- Roaming/sync of the config file across machines.
- Default device / oversampling / monitor preferences. Schema is
  designed to grow into these but they are not delivered here.
- Workspace `stdlib_scanner` behaviour
  ([patches-ffi/src/scanner.rs:239]). Untouched.

## Tickets

- [0896 — `GlobalConfig` schema and env trait methods in patches-plugin-common](../../tickets/closed/0896-global-config-schema-and-env-trait.md)
- [0897 — `PluginScanner` resolution tier for global bundle dirs](../../tickets/closed/0897-scanner-global-config-resolution.md)
- [0898 — patches-player: load + persist global config, TUI bundle-dir action](../../tickets/closed/0898-patches-player-global-config-wiring.md)
- [0899 — patches-clap: load + persist global config, UI bundle-dir controls](../../tickets/closed/0899-patches-clap-global-config-wiring.md)

## Acceptance

- Both `patch_player` and the CLAP plugin read `settings.toml` from
  the OS-native config location on startup and pick up `bundle_dirs`
  without any environment variable.
- Adding a directory through the CLAP UI persists across host
  restarts and is also visible to a subsequent `patch_player` launch
  (the file is shared).
- Adding a directory through the `patch_player` TUI persists across
  restarts and is visible to the CLAP plugin.
- `PATCHES_PLUGIN_PATH` and `--module-path` continue to work as
  before (highest-priority overrides).
- Missing `settings.toml` is not an error on either host — both fall
  back to in-memory defaults and start cleanly.
- Sandboxed CLAP host (Logic on macOS) loads without crashing or
  refusing modules; if a save fails, a status-log line surfaces.
- All four tiers (`inner` / `commit` / `push` / `smoke`) green.

## Outcome (2026-05-17)

All four child tickets closed. Both shells share
`patches_plugin_common::default_global_config_path` so the file
location matches across hosts. `Controller::Action::AddBundleDir` /
`RemoveBundleDir` + `StateDelta.global_config_changed` carry the
mutation; each shell owns its own flush cadence — `patch_player`
debounces (mirrors the existing sidecar flush), the CLAP plugin
writes immediately on each mutation. Sidecar `module_paths` is
scrubbed on save and ignored on load per ADR 0075 §"Persistence
boundary"; existing files remain forward-compatible. CLI
`--module-path` and `PATCHES_PLUGIN_PATH` keep working as
highest-priority per-invocation overrides via
`PluginScanner::with_global_dirs`.

`just commit` green for `patches-player`, `patches-plugin-common`,
and `patches-clap`. Full `push` / `smoke` tiers not run here —
defer to CI; the work is scoped to two shells + the shared
plugin-common surface.
