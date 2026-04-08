# ADR 0026 — Vizia + Baseview for Cross-Platform Plugin GUI

**Date:** 2026-04-07
**Status:** Accepted

---

## Context

The CLAP plugin (`patches-clap`) currently has a macOS-only GUI implemented
with native AppKit via `objc2-app-kit`. Windows and Linux stubs return `false`
from `gui_set_parent`, so the plugin has no UI on those platforms. The GUI is
simple — a path label, Browse and Reload buttons, and a status label — but it
must work everywhere DAW hosts run.

Several cross-platform Rust GUI frameworks were evaluated for suitability as an
embedded audio plugin UI:

| Framework | Parent-window embedding | Audio widget ecosystem | License |
|-----------|------------------------|----------------------|---------|
| **vizia + baseview** | Built-in baseview backend | 25+ views, CYMA visualisers | MIT |
| egui + baseview | Via egui-baseview adapter | No audio-specific widgets | MIT/Apache |
| iced + baseview | Via iced\_baseview adapter | iced\_audio widgets | MIT |
| Slint | No Rust API for host embedding | None | RF/GPL/Commercial |
| Raw baseview | Yes (foundation layer) | None — build everything | MIT/Apache |

Key constraints:

- **Must embed in a host-provided parent window** (HWND / NSView / X11 Window).
  This rules out Slint, which has no supported Rust API for this.
- **No interactive graph editor needed.** The DSL text file is the canonical
  patch representation; the UI only needs to display state and trigger
  browse/reload actions. A future read-only patch visualisation would use a
  layout library (`rust-sugiyama`) and draw with framework primitives.
- **The egui node-editor ecosystem is irrelevant** given the display-only
  requirement, removing egui's main advantage over vizia for this use case.

## Decision

Use **vizia** with its built-in **baseview** backend for the plugin GUI.

- vizia was designed with audio plugin UIs as a primary use case.
- baseview handles parent-window embedding on macOS (Cocoa), Windows (Win32),
  and Linux (X11) — the three APIs the CLAP GUI extension requires.
- The CSS-like styling system and retained-mode architecture suit a UI that
  mostly displays state and responds to occasional button clicks.
- MIT licensed with no commercial restrictions.
- If patch-graph visualisation is added later, vizia's drawing primitives can
  render nodes and edges from coordinates produced by a layout library.

The existing `GuiState` struct, `on_main_thread` callback pattern, and `rfd`
file-dialog integration remain unchanged — only the rendering and windowing
layer is replaced.

## Consequences

**Positive:**

- The plugin GUI works on macOS, Windows, and Linux from a single codebase.
- `gui_show()` and `gui_hide()` (currently stubs) can be properly implemented
  via baseview window visibility.
- DPI scaling (`gui_set_scale`) can be forwarded to vizia's scale context.
- The macOS-only `objc2`, `objc2-app-kit`, and `objc2-foundation` dependencies
  are removed.

**Negative:**

- vizia is pre-1.0 (currently 0.3.x) — API may change between versions.
- vizia and baseview are git dependencies, not published on crates.io.
- Adds a larger dependency tree than the previous hand-rolled AppKit approach.

**Neutral:**

- vizia + baseview replace the platform-specific GUI code; the plugin's audio
  processing, state management, and CLAP extension plumbing are unaffected.

## Alternatives considered

See the evaluation table above. egui + baseview was the runner-up — simpler to
start with but less well-suited to a retained, state-display UI. iced was
rejected due to API churn between versions causing breakage in downstream
adapter crates.
