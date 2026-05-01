---
id: "E130"
title: Unified shell MVC — CLAP + Ratatui peer shells over patches-plugin-common
created: 2026-04-30
tickets: ["0771", "0772", "0773", "0774", "0775", "0776", "0777"]
adrs: ["0061", "0063"]
---

## Goal

Land ADR 0063 minus host-control integration: both shells (CLAP webview
and Ratatui terminal) consume one `Controller`, one action vocabulary,
one `GuiSnapshot`, and one persistence schema. Sidecar persistence and
internal preset library fall out of the unified `SerializedState`.

Host-control plumbing (ADR 0057) and range expressions (ADR 0062) are
tracked separately. A small follow-up integration ticket glues 0057 to
the unified controller after both halves exist.

## Scope

Phase A — controller migration:

1. Migrate `patches-clap` JSON-intent and host-callback handlers to
   `Controller::apply` + a CLAP `Env` impl.
2. Migrate `patches-player` Ratatui TUI to `Controller` + Ratatui `Env`
   impl.

Phase B — schema realignment:

3. Re-key `tap_opts` from `usize` slot to `String` tap name.
4. Split `SerializedState` into `PatchIdentity` (path, source) and
   `PersistedSettings` (settings only). Presets consume the latter.

Phase E — persistence:

5. Add `Env::load_sidecar` / `save_sidecar` with JSON transport for
   Ratatui; CLAP impl no-ops.
6. Wire sidecar lifecycle: load on `LoadPath` post-compile, debounced
   save on `persistable_changed` (Ratatui); `mark_state_dirty` on CLAP.

Phase F — presets:

7. Internal preset library: save/load `PersistedSettings` plus patch
   identity to a library directory; load via `Action::StateLoad`.

## Out of scope

- ADR 0057 host-control DSL syntax, manifest type, planner emission,
  backplane region. Separate epic.
- ADR 0062 range expressions (in flight, separate ticket chain).
- CLAP host preset browser integration (`clap_plugin_preset_load`).
- Cross-shell live state sharing.

## Acceptance

- Both shells route every state mutation through `Controller::apply`.
  No bespoke state in shell code.
- `patches-plugin-common` has no CLAP-, wry-, or ratatui-specific
  surface.
- Ratatui shell loads `<patch>.patches.state` if present, saves
  debounced on dirty.
- CLAP `state_save` / `state_load` round-trips `PersistedSettings`.
- A preset saved from one shell loads in the other against the same
  patch with identical effect.
- `cargo clippy` and `cargo test` pass.

## Open questions resolved during impl

- Sidecar debounce window (suggest 500ms, owned by Ratatui shell loop).
- Sidecar fallback location when patch dir is read-only (suggest XDG
  state dir keyed by patch absolute path hash; surface in status log).
- Migration story for unnamed taps under name-keyed `tap_opts`
  (suggest: drop opts for unnamed taps, status-log a warning).

## Deferred to 0057 integration ticket (post-epic)

- `Action::SetHostControl`, `Action::HostControlManifestUpdated`.
- `HostControlSnapshot` extension on `GuiSnapshot`.
- Backplane writer ownership decision (Controller vs `Env`).
