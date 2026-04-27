---
id: "0717"
title: Plugin-side tap frame pump from SubscribersHandle
priority: high
created: 2026-04-26
epic: "E121"
---

## Summary

Hand the engine's `SubscribersHandle` to the webview shell. On each
`on_main_thread` tick, read every declared tap, build a `TapFrame`,
and push it via `evaluate_script("window.__patches.applyTaps(...)")`.
Throttle to ~30 Hz and skip pushes when the serialised frame is
unchanged from the last push.

## Acceptance criteria

- [ ] `SubscribersHandle` reachable from the webview shell handle.
- [ ] `WebviewGuiHandle` exposes `push_taps(...)` (or equivalent)
      taking a `TapFrame`; throttle window mirrors the existing
      `PUSH_INTERVAL` for snapshots but is independent (frames don't
      block snapshots and vice versa).
- [ ] Cheap dedupe — comparing serialised JSON is acceptable.
- [ ] No allocation on the audio thread; reads are from the
      observer's atomic-scalar surface (ADR 0053 §7).
- [ ] Mirrors the data path used by `patches-player/src/tui.rs`.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

The `applyMeter` channel removed in 0712 is the precedent; this
ticket replaces it with the manifest-aware variant.
