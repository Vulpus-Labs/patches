---
id: "0673"
title: Canvas meter prototype with stub tap data
priority: high
created: 2026-04-24
---

## Summary

Prove that a `<canvas>`-based meter fed from Rust can sustain 60 Hz
visual updates with negligible CPU cost. This is the single biggest
question mark on the webview approach: if meters are unusable, the
spike's conclusion is likely "stick with vizia". Use a stub data
source — we don't have the tap-attach API yet.

## Acceptance criteria

- [ ] Stub producer on the main thread generates synthetic peak/RMS
      values for N channels (N configurable, default 4).
- [ ] Values pushed to JS at 60 Hz via a dedicated channel (not the
      GuiState snapshot path). Consider batched binary payload
      (`Float32Array` over base64 or `postMessage` transferable if
      reachable via wry) before settling on plain JSON.
- [ ] JS renders peak+RMS bars per channel on a single `<canvas>`
      using 2D context. `requestAnimationFrame` pull of the most
      recent buffered frame.
- [ ] CPU cost measured on macOS: idle plugin window with meters
      running vs hidden. Record numbers in ticket notes or evaluation
      ticket.
- [ ] No visible frame drops at 60 Hz with 4 channels on macOS.
- [ ] Document observed per-frame overhead of `evaluate_script`
      versus alternative paths tried.

## Notes

Real tap-attach API is tracked separately (observation UI plan in
memory). For this ticket fake the data. What matters is the transport
and rendering cost.

If macOS WKWebView is fine but Linux WebKitGTK chokes, record that
— it's a real consideration for the evaluation, not a blocker for
the spike completing.
