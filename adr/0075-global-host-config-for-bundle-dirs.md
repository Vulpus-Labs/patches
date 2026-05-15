# ADR 0075 — Global host config for `.pxm` bundle directories

## Status

Proposed (2026-05-15).

## Context

Two hosts load `.pxm` FFI bundles today:

- `patch_player` ([patches-player/src/main.rs]) takes `--module-path
  <DIR|FILE>` (repeatable) and honours the `PATCHES_PLUGIN_PATH` env
  var via `patches_ffi::PluginScanner`. The chosen paths are alive only
  for the lifetime of the process.
- The CLAP plugin ([patches-clap/]) has no UI to add bundle dirs at
  all; today the only way to get a non-default scan path into the
  plugin is to launch the host with `PATCHES_PLUGIN_PATH` exported.

Per-patch persistence already exists for both shells (ADR 0063 §5,
ticket 0776):

- Ratatui writes `<patch>.patches.state` next to the `.patches` file,
  falling back to `$XDG_STATE_HOME/patches/<hash>.patches.state` when
  the patch directory isn't writable ([patches-player/src/controller_env.rs:228-258]).
- CLAP delegates persistence to the host via the standard CLAP state
  API (`Env::sidecar_path` returns `None`).

Both currently store only **patch-scoped** state (param overrides,
monitor view toggles, etc.). There is no place to durably record
**host-scoped** settings — the most pressing being "where should I
look for `.pxm` bundles?", but the same slot will eventually need to
hold default device names, oversampling preference, monitor-tab
default, etc.

`stdlib_scanner()` ([patches-ffi/src/scanner.rs:239]) handles the
workspace-developer case — auto-locating bundles built into
`target/<profile>/`. It does **not** address the end-user case where
`.pxm` bundles are installed elsewhere on the filesystem, nor the CLAP
plugin (which has no command line to pass `--module-path` through).
This ADR addresses the user-installed and CLAP cases; the stdlib
auto-scan stays as-is.

## Decision

Introduce a single cross-platform global config file shared by both
hosts. Add a `Env::load_global_config` / `Env::save_global_config`
pair to `patches-plugin-common`, and have `patches_ffi::scanner`
consult global-config bundle dirs as a third resolution tier.

### Locations

Resolve via the `directories` crate (`ProjectDirs::from("", "",
"Patches")`):

| OS      | Config file                                              | Default bundle dir                                  |
| ------- | -------------------------------------------------------- | --------------------------------------------------- |
| Linux   | `~/.config/patches/settings.toml`                        | `~/.local/share/patches/bundles/`                   |
| macOS   | `~/Library/Application Support/Patches/settings.toml`    | `~/Library/Application Support/Patches/bundles/`    |
| Windows | `%APPDATA%\Patches\config\settings.toml`                 | `%APPDATA%\Patches\data\bundles\`                   |

Rationale for `directories`: hand-rolled XDG fallbacks (as in
`xdg_fallback_for` today) get macOS and Windows wrong. The crate is
small (no transitive runtime deps), already battle-tested in the Rust
ecosystem, and gives us native conventions on each OS.

### Schema (v1)

TOML, owned by `patches-plugin-common::GlobalConfig`:

```toml
schema_version = 1

# Directories scanned for .pxm bundles, in priority order.
# Absent / empty => default data dir is used.
bundle_dirs = [
    "/Users/me/audio/patches-bundles",
]
```

Future host-scoped settings (default device, default oversampling,
monitor default) extend this struct with optional fields; v1 stays
minimal.

### Scanner resolution order

`patches_ffi::PluginScanner` paths come from, in priority order:

1. `PATCHES_PLUGIN_PATH` env var (existing).
2. Constructor-supplied paths — CLI `--module-path`, CLAP UI list.
3. `GlobalConfig::bundle_dirs` from `settings.toml`.
4. Default data dir (`ProjectDirs.data_dir()/bundles/`) **iff it
   exists**. Never auto-created by the host.

The hosts pass the constructor-supplied list to the scanner; the
scanner internally appends (3) and (4). This keeps callers ignorant of
the global-config tiers — they just hand over their per-invocation
overrides.

### Persistence boundary

- **Per-patch sidecar** (existing): patch knobs, monitor view state,
  recording mute. Never contains module paths.
- **Global config** (new): module paths and other host-scoped
  preferences. Never contains per-patch state.

### Host wiring

- `patches-player`: on startup, load global config; merge CLI
  `--module-path` entries into the in-memory list **without** writing
  back (CLI is per-invocation). The TUI's "Add bundle directory"
  action (new) writes to global. Use the existing sidecar debounce
  machinery for the save.
- `patches-clap`: on `init()`, load global config. Add UI controls to
  add/remove bundle dirs; each mutation persists immediately and
  triggers a controller-level rescan. Reads must tolerate missing
  config and read-only filesystems (sandboxed hosts) — fall back to
  in-memory list for the session, surface a status-log entry.

### Sandbox compatibility

The CLAP plugin runs in some hosts (Logic, GarageBand) under macOS app
sandboxing. The config file location remains reachable under sandbox
extensions granted to plugins, but **directory creation** can still
fail. Treat the global config as read-mostly from the CLAP plugin:
write only on explicit user action, never auto-create on first load.
`patches-player` (unsandboxed) is the canonical first-run populator.

## Consequences

### Positive

- Single source of truth for module paths across both hosts.
- Cross-platform paths follow OS conventions (XDG on Linux, Apple
  guidelines on macOS, AppData on Windows) without ad-hoc fallbacks.
- Existing `PATCHES_PLUGIN_PATH` flow is preserved as the highest-
  priority override — CI and ad-hoc shells keep working unchanged.
- Schema lives in `patches-plugin-common` alongside `PersistedSettings`,
  so both shells share serde code paths.

### Negative

- One new optional dependency (`directories`) in
  `patches-plugin-common`. Small, no transitive bloat.
- A second persistence surface to keep schema-versioned. Mitigated by
  reusing the `schema_version` pattern already established for the
  sidecar (`SIDECAR_SCHEMA_VERSION`).
- Sandboxed CLAP hosts may surface "save failed" status when the user
  adds a directory. Acceptable: the session-scoped fallback still
  works, and the failure is transparent.

### Out of scope

- Migrating existing sidecar contents — sidecars stay as they are,
  patch-scoped.
- A GUI installer that drops bundles into the default data dir. Users
  copy `.pxm` files there manually or point the host at their own
  directory.
- Per-host overrides (e.g. CLAP wanting a different bundle list than
  patches-player). YAGNI — revisit if the use case appears.
- Roaming sync. The file is local; users wanting sync drop it in
  Dropbox/iCloud manually.
