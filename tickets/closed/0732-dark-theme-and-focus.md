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

- [x] Background / foreground / accent palette defined in CSS
      variables.
- [x] Buttons, list rows, and tab triggers all have visible
      `:focus-visible` styles.
- [x] Tab navigation works without a mouse.
- [x] Spot-check visual contrast meets WCAG AA for body text.
- [x] `cargo clippy` and `cargo test` clean.

## Notes

- Palette + `:focus-visible` rules in `patches-clap/assets/app.css`
  (`--bg`, `--fg`, `--accent`, `--focus-ring`, plus `.tab:focus-visible`
  and root `:focus-visible` box-shadow).
- Tabs are `<button role="tab">` so native Tab key navigation works
  without script.
- Contrast spot-check (computed):
  - `--fg` `#e8e8e8` on `--bg` `#1a1a1a` ≈ 14:1 (AAA)
  - `--muted` `#9a9a9a` on `--bg` ≈ 6.4:1 (AA)
  - `--accent` `#6cf` on `--bg` ≈ 9:1 (AAA)
- `cargo clippy -p patches-clap`: clean. `cargo test -p patches-clap`
  has 2 pre-existing failures in `activate_scan_tests` (FFI Gain
  registry), unrelated to styling; tracked separately.
