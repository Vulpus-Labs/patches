---
id: "0668"
title: Create patches-plugin-common, extract GUI state and portable helpers
priority: high
created: 2026-04-24
---

## Summary

New crate `patches-plugin-common` holding the pieces of `patches-clap`
that are independent of the GUI toolkit and (as far as practical) of
CLAP itself. Prepares the ground for a second plugin crate using a
webview GUI (E115) without designing a premature abstraction.

## Acceptance criteria

- [ ] New workspace crate `patches-plugin-common`. Deps limited to
      `patches-core`, `patches-engine`, `patches-diagnostics`,
      `patches-interpreter`, `patches-dsl`, `serde` (for snapshot
      serialisation later). No `clack_*`, no `vizia`, no `wry`.
- [ ] `GuiState`, `DiagnosticView`, `STATUS_LOG_CAPACITY`, and their
      impls moved out of `patches-clap::gui` into this crate.
- [ ] `GuiState` derives `serde::Serialize` (paths as strings; skip
      fields that don't serialise cleanly — the webview shell will
      project what it needs).
- [ ] Audit `patches-clap/src/plugin.rs` and
      `patches-clap/src/extensions.rs` for backend-agnostic logic:
      module-path persistence, reload orchestration helpers, status
      message formatting, diagnostic projection. Lift what moves
      cleanly; leave the rest in the plugin crate.
- [ ] `patches-clap` updated to re-export / consume the moved types;
      behaviour unchanged.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

Goal is not to design a `PluginGui` trait or a fully abstract API —
that would be premature with only one implementation. Pull out what
is obviously common; accept that plugin.rs / extensions.rs will be
partially duplicated in the webview crate (ticket 0670), and let
real duplication drive later abstraction.

Hard-stop reload (ADR 0044) and halt handling (ADR 0051) orchestration
are strong candidates for common code but may be entangled with CLAP
host callbacks — err on the side of leaving them in the plugin crate
if extraction gets messy.
