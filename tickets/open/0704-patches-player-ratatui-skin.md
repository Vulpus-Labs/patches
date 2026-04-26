---
id: "0704"
title: patches-player ratatui skin (header / meter pane / event log / footer)
priority: high
created: 2026-04-26
---

## Summary

Replace `patches-player`'s current CLI status output with a ratatui
TUI per ADR 0055 §5. Layout: header (patch path, sample rate,
oversampling, engine state), meter pane (one peak+RMS bar pair per
declared meter tap, labelled by tap name, dB-coloured), event log
pane (scrolling), footer (keybindings).

This ticket lands the skin and input handling but does **not** wire
in the observer subscription — that's ticket 0705. Meter bars render
from a stub data source until 0705 lands, so the layout can be
iterated independently.

## Acceptance criteria

- [ ] `patches-player` boots into a ratatui TUI; `q` quits cleanly,
  restoring the terminal.
- [ ] `r` toggles WAV recording via the existing `wav_recorder`. No
  new recording machinery; reuse what's there.
- [ ] Header shows: patch path, sample rate, oversampling factor,
  engine state (running / stopped / error). Patch reload state
  surfaces in the event log only — no live-reload UI in this ticket.
- [ ] Meter pane: one labelled bar pair per tap (peak + RMS), driven
  by a stub data source for now (e.g. zeros, sine, or a deterministic
  walk). Layout adapts to terminal width; gracefully truncates
  labels.
- [ ] dB colour bands: green / amber / red thresholds at conventional
  levels (document in the code).
- [ ] Event log pane scrolls newest-at-bottom; capacity bounded.
- [ ] Footer lists active keybindings; updates if more are added.
- [ ] No allocation-per-frame in the redraw path beyond what ratatui
  itself does; redraw at ~30 Hz.
- [ ] Existing player non-TUI smoke tests either retire or move to
  a `--no-tui` mode if any consumer relies on them.

## Notes

ratatui-side state machine: a `View` struct holds the latest meter
values, event log ring, and recording flag; the input loop and
draw loop both reference it. Keep the redraw pure-ish so 0705 only
has to swap the stub data source for an observer subscriber.

## Cross-references

- ADR 0055 §5 — TUI layout and bringup keybindings.
- E119 — parent epic.
