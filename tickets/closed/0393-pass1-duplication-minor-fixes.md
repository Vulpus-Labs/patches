---
id: "0393"
title: Pass 1 duplication cleanup — parameter helpers, MIDI dispatch, denormal, envelope coeffs
priority: medium
created: 2026-04-13
---

## Summary

Code review pass 1 (since v0.5.1) surfaced several duplicated patterns that
should be extracted into shared helpers. Bundled here as minor cleanups; the
larger LSP/DSL include-resolver dedup is tracked separately.

## Acceptance criteria

### H1 / H2 — Parameter accessor helpers

- [x] Extracted to `patches-modules/src/common/param_access.rs`.
- [x] `delay.rs`, `stereo_delay.rs`, `mixer.rs` use the shared helpers; no
      local copies remain.
- [x] Parameter-update loops left as-is — they already read cleanly once the
      accessors move out, and a closure would add noise without saving lines.

### H3 — MIDI message parsing

- [x] `MidiMessage` enum added to `patches-core/src/midi.rs` (NoteOn, NoteOff,
      ControlChange, PitchBend, Other). Zero-velocity NoteOn normalised to
      NoteOff in `parse`.
- [x] `midi_in.rs`, `poly_midi_in.rs`, `midi_cc.rs`, `midi_drumset.rs` dispatch
      on `MidiMessage::parse`.
- [x] Parser unit tests in `patches-core/src/midi.rs` cover NoteOn, NoteOn
      vel=0, NoteOff, CC, PitchBend centre / min / max, and an Other
      passthrough.

### M6 — Denormal flush helper

- [x] `pub fn flush_denormal(x: f32) -> f32` added to `patches-dsp/src/lib.rs`.
- [x] `dc_blocker.rs` and `envelope_follower.rs` use it.
- [x] Grep found no other occurrences.

### M7 — Attack/release envelope coefficient caching

- [x] Evaluated. **Not extracting.** Rationale in Notes.

## Notes

Findings come from the pass-1 duplication review of commits since `v0.5.1`.
Severities assigned at review time:

- H1/H2: high — 20+ lines of identical boilerplate across 3 modules.
- H3: high — hand-rolled status-byte matching is error-prone (MIDI velocity-0
  NoteOn semantics) and the `midi_io` backplane refactor (ticket 0379) stopped
  short of parsing.
- M6: medium — three-line pattern, but audio code benefits from a single
  canonical denormal guard.
- M7: medium — judgement call; do not force the abstraction if it does not fit.

M7 decision: `EnvelopeFollower` and `LimiterCore` share only the two coeff
fields and their `set_*_ms` setters. Beyond that the state machines diverge:

- `EnvelopeFollower` tracks rising amplitude — attack applies when `|input| >
  envelope`; the target is the input itself.
- `LimiterCore` tracks smoothed gain reduction — attack applies when the
  target gain is *below* current; the target is computed externally from a
  peak window and threshold.
- `LimiterCore::set_attack_ms` also resizes the peak window and returns a
  change flag used by callers; a uniform setter would either leak those
  concerns or force the limiter to wrap its own update path.

A shared "two-coeff exponential smoother" wrapper would save roughly ten
lines of storage/setters while adding a direction-selection parameter and
obscuring the per-caller semantics. Left independent.

Out of scope (tracked separately): M5 — LSP vs DSL include resolver dedup
(`patches-lsp/src/server.rs:127-245` vs `patches-dsl/src/loader.rs:131-212`).
Different I/O shapes (URI + file vs Path + closure) mean a shared trait /
state machine needs its own design. See the pass-1 remediation plan.

Run `cargo clippy` and `cargo test` after each sub-task.
