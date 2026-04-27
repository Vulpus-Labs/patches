---
id: "0709"
title: Spectrum processor + EQ-curve tab in patches-player TUI
priority: medium
created: 2026-04-26
---

## Summary

Implement the spectrum tap pipeline (parsed but stubbed in E118 /
E119). Observer-side: windowed FFT, log-frequency magnitude bins,
ballistic decay. TUI-side: a second tab rendering an EQ curve via
ratatui's `Canvas` widget, one curve per declared spectrum tap.

## Acceptance criteria

- [ ] `patches-observation::processor::Spectrum` processor:
  - Buffers samples per slot until FFT_SIZE accumulated (initial:
    1024 samples; document trade-off).
  - Applies a Hann window (or similar) before transform.
  - Uses `patches_dsp::fft` (real-FFT, packed CMSIS layout).
  - Computes magnitude per bin, optionally smoothed (frame-to-frame
    decay or peak-hold).
  - Publishes log-frequency-mapped bins (e.g. 64 or 128 display
    bins) via the subscriber surface — extend the surface beyond
    "scalar per (slot, processor)" to "vector per (slot, processor)"
    in a way that keeps the read path lock-free (e.g. seqlock or
    double-buffered `Vec<f32>` per slot).
- [ ] Observer wiring: `Spectrum` selected when a tap component is
  `TapType::Spectrum`. Replaces the current
  `Diagnostic::NotYetImplemented` stub.
- [ ] TUI tabs: `1` shows meters, `2` shows spectrum. `Tab`
  cycles. Active tab visible in header or footer.
- [ ] Spectrum tab: ratatui `Canvas` with log-frequency x-axis
  (e.g. 20 Hz – 20 kHz), dB y-axis (e.g. -60 .. 0). One coloured
  curve per declared spectrum tap, label legend at top.
- [ ] No allocation in the observer's per-block hot path beyond
  what's already baked in at construction (FFT scratch, window,
  display-bin lookup pre-computed).
- [ ] Tests:
  - White-noise input → roughly flat spectrum within tolerance
    (statistical, low gates to avoid flake).
  - Pure tone at known frequency → peak near the expected display
    bin.
  - Subscriber surface: vector publication round-trips correctly
    under contention (single producer, single consumer).
- [ ] `cargo clippy` clean; `cargo test` green.

## Notes

- FFT_SIZE choice: 1024 at 48 kHz = ~21 ms window, ~47 Hz
  resolution. Fine for visual EQ; not for low-end. Could raise to
  2048/4096 later (parameter on the tap declaration).
- Display bin count: 64 is enough for canvas curves at typical
  terminal widths. Each display bin is a max or average over the
  underlying FFT bins falling in its log-frequency range.
- This ticket extends the subscriber surface from
  scalar-per-processor to vector-per-processor. Design the API so
  the meter pipeline (scalar) and spectrum (vector) coexist
  without forcing meter consumers to allocate.
- Cross-references: ADR 0054 (DSL), ADR 0056 (pipeline), 0701
  (observer crate), E119.
