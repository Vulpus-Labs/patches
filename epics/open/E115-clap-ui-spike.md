---
id: "E115"
title: CLAP UI backend spike — vizia vs webview
created: 2026-04-24
tickets: ["0668", "0669", "0670", "0671", "0672", "0673", "0674"]
adrs: []
---

## Goal

Evaluate a webview-based GUI (wry + HTML/CSS/JS) as an alternative to
the current vizia implementation for the CLAP plugin. Ship two working
`.clap` artifacts built from shared core logic, compare on iteration
speed, memory footprint, runtime perf (especially canvas-drawn meters
fed by tap data), cross-platform portability, and code maintainability.

Outcome feeds a later ADR deciding the long-term GUI stack. No ADR
written up-front — decision is contingent on spike results.

## Approach

Refactor before fork: lift backend-agnostic pieces out of
`patches-clap` into `patches-plugin-common`. Rename existing crate to
`patches-clap-vizia`. Add a parallel `patches-clap-webview` that
consumes the same common crate. Accept some duplication in CLAP glue
(plugin.rs, extensions.rs) — the second implementation is what will
reveal the real abstraction boundary.

## Scope

1. Extract `GuiState`, diagnostic view, status log, and any portable
   orchestration helpers into `patches-plugin-common`.
2. Rename `patches-clap` → `patches-clap-vizia`; retarget to consume
   common crate. Behaviour unchanged.
3. Scaffold `patches-clap-webview` with wry, baseview parenting, blank
   window.
4. IPC layer: bidirectional `GuiState` sync between Rust main thread
   and JS. JSON for control state; reduced binary payload for meter
   data.
5. HTML/CSS/JS shell reproducing current vizia UI: file path display,
   browse/reload/rescan, module path list, status log, diagnostics
   panel, halt banner.
6. Canvas meter prototype: fake tap producer → `<canvas>` peak/RMS
   bars. Validate 60Hz update with negligible CPU.
7. Evaluation report comparing both backends across the agreed axes.

## Non-goals

- ADR / final decision.
- Real tap-attach API (E... observation UI epic blocks on this; the
  spike uses a stub producer).
- Web asset bundling pipeline beyond what's needed to ship a working
  `.clap`.
- Parameter automation UI (neither backend supports params yet).

## Tickets

- 0668 — Create `patches-plugin-common`, extract GUI state and helpers
- 0669 — Rename `patches-clap` → `patches-clap-vizia`, consume common
- 0670 — Scaffold `patches-clap-webview` crate with wry + baseview
- 0671 — Webview ↔ Rust IPC for `GuiState` sync
- 0672 — HTML/CSS/JS shell matching current vizia UI
- 0673 — Canvas meter prototype with stub tap data
- 0674 — Spike writeup and backend comparison
