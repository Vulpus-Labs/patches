# E044 — Windowed cross-thread transfer buffer (`SlotDeck`)

## Goal

Implement the `SlotDeck` design from ADR 0023: a general-purpose, allocation-free,
lock-free mechanism for transferring overlapping audio windows between the audio
thread and a processing thread, with overlap-add reconstruction on the audio thread.

After this epic, `patches-dsp` contains a fully tested `SlotDeck` primitive
that any module can use to offload expensive windowed processing (FFT
convolution, spectral effects, neural post-processing, etc.) to a background
thread without violating the audio-thread constraints.

## Background

See [ADR 0023](../../adr/0023-windowed-crossthread-transfer-buffer.md).

Key constraints:

- No allocation on the audio thread after initialisation.
- No blocking on the audio thread — silent degradation if resources unavailable.
- Correct ownership by construction (`Box<[f32]>` moved via `rtrb` channels).
- Three configuration parameters, all powers of 2: `window_size`,
  `overlap_factor`, `processing_budget`.
- Total latency = `window_size + processing_budget` samples.

## Tickets

| #      | Title                                                             | Priority |
| ------ | ----------------------------------------------------------------- | -------- |
| T-0233 | `SlotDeck` stubs and ignored test suite                           | high     |
| T-0229 | `FilledSlot`, `SlotDeckConfig`, and buffer pool allocation        | high     |
| T-0230 | `SlotDeckSender` — write-side state machine                       | high     |
| T-0231 | `SlotDeckReceiver` — processing-thread handle                     | high     |
| T-0232 | `OverlapBuffer` — audio-thread composite with overlap-add         | high     |
