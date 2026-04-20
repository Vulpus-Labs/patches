---
id: "0593"
title: Tag engine audio threads at entry (CPAL callback, headless ticks)
priority: high
created: 2026-04-20
---

## Summary

Wire `mark_audio_thread()` (ticket 0592) into every real
audio-thread entry point and install the `TrappingAllocator`
(ticket 0591) in binaries that opt into the trap. When the
`audio-thread-allocator-trap` feature is off this ticket is a
no-op at runtime — the tagging calls compile to nothing and no
`#[global_allocator]` is installed.

## Acceptance criteria

- [ ] `patches-cpal`: depend on `patches-alloc-trap`
      (feature-agnostic). The first time `AudioCallback::data_callback`
      runs on a given thread, call `mark_audio_thread()`. Use
      `std::sync::Once` or a `Cell` on the callback struct to
      keep it to one call.
- [ ] `patches-player`: add an opt-in
      `audio-thread-allocator-trap` feature that turns on the
      matching feature in `patches-alloc-trap`, and install
      `#[global_allocator] static A: TrappingAllocator = …`
      when the feature is on (behind `#[cfg(feature = "…")]`).
- [ ] `patches-clap`: same pattern as `patches-player` — an
      opt-in feature that wires the global allocator. Tag at
      plugin callback entry analogously to `patches-cpal`.
- [ ] `patches-integration-tests`: install the global allocator
      (feature-gated) so `HeadlessEngine`-driven tests run
      under the trap. Do not tag the test thread
      unconditionally — ticket 0594 migrates each sweep test to
      use `NoAllocGuard::enter()` or a per-test
      `mark_audio_thread()` call.
- [ ] Both feature states build clean: default (off) and
      `--features audio-thread-allocator-trap` where
      applicable. No change to the default cargo test run.
- [ ] Sanity check: with the feature on, run
      `patches-player` against `examples/simple.patches` for
      10 s of real playback and confirm no abort. (Doc this as
      a manual check in the ticket's closed notes.)
- [ ] Clippy clean across the touched crates.

## Notes

Why `Once`-guarded rather than unconditional: CPAL may invoke
the callback closure from a different thread than the one that
built the stream, and we cannot rely on any specific
thread-creation hook. The first-callback tag is the simplest
place that is guaranteed to run exactly on the audio thread.

The CLAP callback path is symmetric to CPAL here; if the CLAP
entry point is currently structured through a shared
`PatchProcessor::process` call, one `mark_audio_thread()` per
process-entry per thread is enough.

Depends on 0591, 0592. Parallel-safe with 0594.
