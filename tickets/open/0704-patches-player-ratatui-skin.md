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

This ticket lands the skin and input handling. Code against the
real `SubscribersHandle` API from `patches-observation` (already
shipped in 0701) — no separate stub trait. Bars are driven by a
fake publisher thread feeding a locally-constructed `LatestValues`
with a deterministic walk, so the layout can be iterated without
the live engine→observer wire-up. 0705 replaces the fake publisher
with the real plumbing and feeds the live manifest in.

## Decisions locked in

- **Data source**: `SubscribersHandle` (peak + RMS read as
  `read(slot, ProcessorId::Peak/Rms)`, drops via `dropped(slot)`).
  In 0704 the backing `LatestValues` + `DropCounters` are owned by
  a test harness in `patches-player`, not by a real `Subscribers`.
- **Tap list**: hardcoded fixture in 0704 (e.g. two named slots
  `"a"`, `"b"`). Manifest-driven tap discovery is 0705.
- **Recording toggle**: `r` requires `--record <path>` at launch.
  Toggling without a path logs "no record path; pass --record" to
  the event log. Mid-run start/stop of `wav_recorder` is out of
  scope for this ticket — toggle just mutes/unmutes writes via a
  flag the recorder already supports, or, if it doesn't, suppresses
  via a wrapper. Do not extend `wav_recorder` itself here.
- **Dependencies**: `ratatui` + `crossterm`. Add to
  `patches-player/Cargo.toml` only after confirming with user.
- **dB bands**: green `≤ −18 dBFS`, amber `−18..−6 dBFS`, red
  `≥ −6 dBFS`. Threshold constants in code.
- **Diagnostics → event log**: engine halt info and reload outcome
  (success/failure) move from stderr into the event log pane.

## Acceptance criteria

- [ ] `patches-player` boots into a ratatui TUI; `q` quits cleanly,
  restoring the terminal.
- [ ] `r` toggles WAV recording per the recording-toggle decision
  above.
- [ ] Header shows: patch path, sample rate, oversampling factor,
  engine state (running / stopped / error). Patch reload state
  surfaces in the event log only — no live-reload UI in this ticket.
- [ ] Meter pane: one labelled bar pair per fixture tap (peak + RMS),
  driven by a fake-publisher thread. Layout adapts to terminal
  width; gracefully truncates labels.
- [ ] dB colour bands per locked-in thresholds, with named constants.
- [ ] Event log pane scrolls newest-at-bottom; capacity bounded.
  Engine halt + reload outcomes route here instead of stderr.
- [ ] Footer lists active keybindings; updates if more are added.
- [ ] No allocation-per-frame in the redraw path beyond what ratatui
  itself does; redraw at ~30 Hz.
- [ ] Existing player non-TUI smoke tests either retire or move to
  a `--no-tui` mode if any consumer relies on them.

## Notes

A `View` struct holds the `SubscribersHandle`, the fixture tap list,
event log ring, and recording flag; the input loop and draw loop
both reference it. 0705 replaces the fake-publisher harness with
manifest-driven tap discovery and the live observer plumbing —
the `View` shape should not need to change.

## Cross-references

- ADR 0055 §5 — TUI layout and bringup keybindings.
- E119 — parent epic.
