---
id: "0522"
title: Split patches-dsp slot_deck.rs integration tests by category
priority: low
created: 2026-04-17
epic: E087
---

## Summary

[patches-dsp/tests/slot_deck.rs](../../patches-dsp/tests/slot_deck.rs)
is 764 lines exercising the full SlotDeck pipeline across six
categories (round-trip, OLA, WOLA, overload/starvation, pool recovery,
FFT-based low-pass). Split along those axes. Shared helpers
(`run_ola`, `run_wola`, `run_fft_lowpass`, `rms`) move to
`slot_deck/support.rs`.

## Acceptance criteria

- [ ] `patches-dsp/tests/slot_deck.rs` reduced to a stub
      (`mod slot_deck;`).
- [ ] `patches-dsp/tests/slot_deck/mod.rs` declares the category
      submodules listed below.
- [ ] Shared helpers lifted to `slot_deck/support.rs`, imported by
      each category that needs them.
- [ ] Each category submodule contains the tests from its matching
      section, verbatim; no test logic edits.
- [ ] `cargo test -p patches-dsp --test slot_deck` passes with
      the same test count as before.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.

## Target layout

```
patches-dsp/tests/slot_deck.rs                 # stub
patches-dsp/tests/slot_deck/mod.rs             # submodule declarations
patches-dsp/tests/slot_deck/support.rs         # run_ola, run_wola, run_fft_lowpass, rms
patches-dsp/tests/slot_deck/round_trip.rs      # startup silence, identity (inline + threaded), late frames
patches-dsp/tests/slot_deck/ola.rs             # OLA Hann overlap 2 / 4
patches-dsp/tests/slot_deck/wola.rs            # WOLA Hann overlap 2 / 4
patches-dsp/tests/slot_deck/overload.rs        # write overload, pool starvation
patches-dsp/tests/slot_deck/pool.rs            # starvation recovery, slow processor, return-full, recycling
patches-dsp/tests/slot_deck/fft_lowpass.rs     # FFT brick-wall low-pass via WOLA
```

## Notes

Pattern: [patches-dsl/tests/expand_tests.rs](../../patches-dsl/tests/expand_tests.rs)
+ [patches-dsl/tests/expand/](../../patches-dsl/tests/expand/). Part of
epic E087 (tier C follow-on to E085).
