---
id: "0672"
title: HTML/CSS/JS shell matching current vizia UI
priority: medium
created: 2026-04-24
---

## Summary

Build the HTML/CSS/JS UI inside `patches-clap-webview` to reproduce
the functionality of the vizia GUI: file path display, browse / reload
/ rescan buttons, module path list with add/remove, rolling status log,
diagnostics panel, engine halt banner.

## Acceptance criteria

- [ ] HTML/CSS/JS bundled as static assets in the crate; embedded at
      compile time (e.g. `include_str!`) or loaded from a
      per-platform resource dir. Pick one approach and document.
- [ ] Renders: current file path, Browse / Reload / Rescan buttons,
      editable module path list, status log (scrollable, newest at
      bottom), diagnostics list with severity styling, halt banner
      pinned at top when `halt` is `Some`.
- [ ] All controls use `window.__patches.send(...)` (from 0671) to
      emit intents.
- [ ] Layout reasonable at default plugin window size; resizable
      within sane bounds.
- [ ] Visual fidelity to vizia version is *not* required — functional
      parity is. Evaluation ticket (0674) compares look and feel
      separately.
- [ ] No external CDN dependencies; plugin works offline. Vanilla JS
      or a single small bundled dep, no build pipeline beyond what
      `cargo build` already runs.

## Notes

Keep the JS dependency-free where possible. If a framework helps
iteration speed (lit, preact, alpine), note it in the evaluation and
record bundle-size cost. Avoid anything requiring node/npm at build
time — the whole point of the spike is to see whether plain web tech
speeds up iteration, not to introduce a frontend toolchain.
