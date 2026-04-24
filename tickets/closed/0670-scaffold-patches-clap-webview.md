---
id: "0670"
title: Scaffold patches-clap-webview crate with wry + baseview
priority: high
created: 2026-04-24
---

## Summary

New sibling crate `patches-clap-webview`. Same CLAP surface as
`patches-clap-vizia` — factory, descriptor, entry, plugin, extensions,
audio thread bridge — but the GUI extension opens a wry webview parented
to the host's window handle instead of a vizia view. For this ticket the
webview just displays a static "hello" HTML page; IPC and UI content
come in 0671 and 0672.

## Acceptance criteria

- [ ] New crate `patches-clap-webview` in workspace.
- [ ] Deps: `patches-plugin-common`, `clack-*` (matching vizia crate
      versions), `wry`, `raw-window-handle`, `baseview` (if required
      for parent window plumbing on macOS/Linux).
- [ ] CLAP factory / entry / descriptor cloned from
      `patches-clap-vizia`. Descriptor id differentiated so both can
      coexist in one host scan.
- [ ] `gui` extension opens a wry webview parented to
      `clap_window.handle` on macOS, Windows, Linux. Static HTML bundled
      in the crate renders.
- [ ] Close / resize / show / hide work without crashing the host.
- [ ] Audio processing identical to vizia crate (same reload flow,
      same halt semantics).
- [ ] `cargo build -p patches-clap-webview` produces a loadable `.clap`
      on at least macOS. Linux/Windows best-effort; document gaps.

## Notes

Parent-window parenting is the main risk:

- macOS: `WKWebView` added as subview of `NSView` handle.
- Windows: `WebView2` bound to `HWND`.
- Linux: `WebKitGTK` inside the GTK window hierarchy; baseview's
  `XcbWindow` may not play nicely — note and defer if blocked.

Precedent: `nih_plug_webview` crate. Read for window-handle plumbing
but do not take a dep on nih-plug itself.

Resize requests flow webview → Rust → CLAP `gui.request_resize`. Stub
with a fixed size for this ticket; proper resize in 0671 or 0672.
