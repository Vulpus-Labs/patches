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

- [ ] Extract `get_float`, `get_int`, `get_bool` (matching on `ParameterValue`)
      into `patches-modules/src/common/` (e.g. `param_access.rs`). Currently
      duplicated verbatim in:
  - `patches-modules/src/delay.rs:53-74`
  - `patches-modules/src/stereo_delay.rs:60-80`
  - `patches-modules/src/mixer.rs` (similar)
- [ ] Callers in `delay.rs` / `stereo_delay.rs` / `mixer.rs` use the shared
      helpers. No local copies remain.
- [ ] Consider whether the parameter-update loop pattern in `delay.rs:~188` and
      `stereo_delay.rs:~195` benefits from a closure or small helper once the
      accessors move out. Only extract if the result is clearly tidier — do not
      introduce a macro for the sake of DRY.

### H3 — MIDI message parsing

- [ ] Add a `MidiMessage` enum (or similar) to `patches-core/src/midi.rs` (or
      `midi_io.rs`) covering NoteOn / NoteOff / CC / PitchBend, with the
      velocity-0-NoteOn-as-NoteOff convention handled in the parser.
- [ ] `patches-modules/src/midi_in.rs:110-143`, `poly_midi_in.rs`, `midi_cc.rs`,
      `midi_drumset.rs` dispatch on the parsed enum instead of raw status bytes.
- [ ] Unit test coverage for the parser in `patches-core` (at minimum:
      NoteOn, NoteOn vel=0, NoteOff, CC, PitchBend centre / extremes).

### M6 — Denormal flush helper

- [ ] Add `pub fn flush_denormal(x: f32) -> f32` (or `fn flush_denormal(x: &mut f32)`)
      to `patches-dsp/src/lib.rs`.
- [ ] `patches-dsp/src/dc_blocker.rs:37-39` and
      `patches-dsp/src/envelope_follower.rs:54-57` use it.
- [ ] Grep for other `!x.is_normal() && x != 0.0` patterns in dsp/modules and
      migrate any found.

### M7 — Attack/release envelope coefficient caching

- [ ] Evaluate extracting a `TimeConstantEnvelope` (or similar) struct to
      `patches-dsp` that owns `attack_coeff` / `release_coeff` and
      `set_attack_ms` / `set_release_ms` using `compute_time_coeff`.
- [ ] If the abstraction is clean, migrate `patches-dsp/src/limiter_core.rs`
      and `patches-dsp/src/envelope_follower.rs` to it. If the state machines
      differ enough that the wrapper is awkward, document the decision in the
      ticket notes and close without extracting.

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

Out of scope (tracked separately): M5 — LSP vs DSL include resolver dedup
(`patches-lsp/src/server.rs:127-245` vs `patches-dsl/src/loader.rs:131-212`).
Different I/O shapes (URI + file vs Path + closure) mean a shared trait /
state machine needs its own design. See the pass-1 remediation plan.

Run `cargo clippy` and `cargo test` after each sub-task.
