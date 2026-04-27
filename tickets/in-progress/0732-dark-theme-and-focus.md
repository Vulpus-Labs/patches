---
id: "0732"
title: Dark theme CSS pass + keyboard focus rings
priority: low
created: 2026-04-26
epic: "E124"
---

## Summary

Final styling pass on the webview shell. Dark theme that doesn't
clash with common DAW chrome; visible keyboard focus on every
interactive element.

## Acceptance criteria

- [ ] Background / foreground / accent palette defined in CSS
      variables.
- [ ] Buttons, list rows, and tab triggers all have visible
      `:focus-visible` styles.
- [ ] Tab navigation works without a mouse.
- [ ] Spot-check visual contrast meets WCAG AA for body text.
- [ ] `cargo clippy` and `cargo test` clean.
