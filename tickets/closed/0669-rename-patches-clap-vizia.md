---
id: "0669"
title: Rename patches-clap to patches-clap-vizia, consume plugin-common
priority: high
created: 2026-04-24
---

## Summary

Rename the existing `patches-clap` crate to `patches-clap-vizia` and
retarget it to consume `patches-plugin-common` (ticket 0668). Behaviour
unchanged; this is purely a reshuffle to make room for a parallel
webview crate.

## Acceptance criteria

- [ ] Directory `patches-clap/` → `patches-clap-vizia/`.
- [ ] `Cargo.toml` `name = "patches-clap-vizia"`.
- [ ] Workspace members list updated.
- [ ] `package-vsix.sh`, `deploy.sh`, and any CI / docs that name the
      crate or its built `.clap` artifact updated. Grep for
      `patches-clap` and `patches_clap` across the repo.
- [ ] Local `GuiState` etc. replaced with re-imports from
      `patches-plugin-common`.
- [ ] CLAP plugin loads in host, file browse / reload / rescan / halt
      banner all work as before.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

The built `.clap` filename may change. Check any host-side paths or
release automation that referenced the old name. Keep the vizia-side
code intact — no behavioural changes in this ticket.
