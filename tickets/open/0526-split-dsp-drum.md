---
id: "0526"
title: Split patches-dsp drum.rs by primitive
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-dsp/src/drum.rs](../../patches-dsp/src/drum.rs) is 596 lines
collecting five independent primitives used by the drum voices:
`DecayEnvelope`, `PitchSweep`, the `saturate` fn, `MetallicTone`
(six-oscillator inharmonic summer), and `BurstGenerator` (clap noise
bursts). Plus a 270-line test block at the bottom.

Each primitive is independently usable and has no coupling to the
others beyond sharing the crate.

## Acceptance criteria

- [ ] Convert `drum.rs` to `drum/mod.rs` + sibling submodules:
      `envelope.rs` (DecayEnvelope), `sweep.rs` (PitchSweep),
      `saturate.rs` (saturate fn), `metallic.rs` (MetallicTone),
      `burst.rs` (BurstGenerator).
- [ ] Inline `mod tests` either split per primitive into
      `<primitive>.rs`'s own `#[cfg(test)] mod tests` blocks or kept
      together in `drum/tests.rs` — whichever groups cleaner.
- [ ] Public re-exports from `patches-dsp/src/lib.rs` unchanged.
- [ ] `mod.rs` under ~50 lines (module declarations + re-exports).
- [ ] `cargo build -p patches-dsp`, `cargo test -p patches-dsp`,
      `cargo clippy` clean.

## Notes

E090. No behaviour change. No audio-thread allocation concerns — these
are stateless/tiny state primitives.
