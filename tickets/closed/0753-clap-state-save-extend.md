---
id: "0753"
title: CLAP state_save drops tap display opts and window size
priority: medium
created: 2026-04-28
---

## Summary

`state_save` / `state_load` in `patches-clap/src/extensions.rs` only
persist `file_path`, `dsl_source`, and `module_paths`. The following
GUI-relevant fields are lost across host save/load:

- `GuiState::tap_opts` — per-slot scope decimation, scope window samples,
  spectrum FFT size. Reopening a project resets every meter/scope to
  defaults.
- `gui_width` / `gui_height` on the plugin (the comment at
  `plugin.rs:94` claims they persist between `gui_destroy` / `gui_create`,
  which is true within a session, but they are never written to the
  state stream).

Diagnostic history (`status_log`, `diagnostic_view`) is intentionally
ephemeral and out of scope.

## Acceptance criteria

- [ ] State stream gains a versioned section carrying `tap_opts` and
      window size.
- [ ] Loading legacy state (no trailing section) succeeds with sensible
      defaults — same EOF tolerance pattern as the existing
      `module_paths` section (ticket 0566).
- [ ] Round-trip test: populate `tap_opts` and resize the window, save,
      reload into a fresh plugin instance, observe the same values.

## Notes

Current format (see doc comment at `extensions.rs:120-135`):

```
[len][file_path]
[len][dsl_source]
[u32 count][len][module_path]...   // optional, EOF-tolerant
```

Extend with a further optional trailing section. Bump format version
or rely on the same EOF-tolerant approach if a single new section is
sufficient. `TapDisplayOpts` is `Copy` and small — encode as
`[u32 slot][u32 scope_decimation][u32 scope_window_samples][u32 spectrum_fft_size]`
per entry, count-prefixed. Window size: `[u32 width][u32 height]`.
