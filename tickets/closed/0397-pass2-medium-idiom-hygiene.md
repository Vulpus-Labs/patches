---
id: "0397"
title: Pass 2 medium — builder param-diff move, SongId newtype, mutex poison, misc
priority: medium
created: 2026-04-13
---

## Summary

Pass-2 medium findings, bundled. Each is a small, local change. The
`SongId` item is the largest and touches the audio-thread-visible
`TrackerData` shape; the rest are localised.

## Acceptance criteria

### M1 — Mark `BuildError` format! sites as non-RT

- [x] Doc comment added to `BuildError` enum stating the constructor path
      is planner-thread only and `format!` allocation is acceptable. Did
      not migrate formatting into `Display`: changing the
      `InternalError(String)` / `ModuleCreationError(String)` shape would
      require threading structured data through every call site. The
      documentation boundary is the cheaper win.

### M2 — Avoid cloning `ParameterMap` in the planner decision loop

- [x] `builder.rs:351` now matches `decision` by value. `param_diff` moves
      directly into `parameter_updates`. `input_ports` / `output_ports` are
      still cloned into `port_updates` because they are also consumed later
      by the `NodeState::insert` site; the `ParameterMap` clone — the
      primary cost — is eliminated.

### M4 — `MidiFrame::read_event` / `write_event` should not panic on hot path

- [x] Both `assert!` calls converted to `debug_assert!`; doc comments
      updated to describe the debug-only panic behaviour.

### M6 — Do not silently accept poisoned GUI mutex

- [x] All 11 call sites across `plugin.rs`, `extensions.rs`, and
      `gui_vizia.rs` replaced with `.expect("gui_state mutex poisoned")`.
      Poisoned state now surfaces instead of being silently reused.

### M7 — Strip `name_to_index` from audio-visible `SongBank`

- [x] `SongBank::name_to_index` removed. The audio-thread-shared
      `Arc<TrackerData>` no longer carries any `String` keys.
- [x] Interpreter keeps its own `song_name_to_index: HashMap<String, usize>`
      as a local; `validate_sequencer_songs` now accepts it as an argument.
- [x] Test fixtures in `master_sequencer.rs` and `pattern_player.rs`
      updated; unused `HashMap` imports removed.
- [~] `SongId(u32)` newtype deferred. Current indexing is `usize`
      end-to-end (`MasterSequencer::song_index: Option<usize>`,
      `data.songs.songs.get(idx)`) and song references flow as
      `ParameterValue::Int`. A newtype would add clarity but requires
      either a new `ParameterValue` variant or a wrapper at each index
      site — punt until debug or type pressure justifies it. Debug-trace
      name reconstruction is likewise deferred: dump the interpreter's
      name table separately when needed.

### M8 — Replace one-shot `AtomicBool` with `OnceLock`

- [x] `PROCESS_LOGGED` converted to `std::sync::OnceLock<()>`; first-call
      semantics are now explicit (`set(()).is_ok()`). `PROCESS_COUNT`
      kept as `AtomicU32` — it is a genuine counter, not a one-shot —
      with a comment documenting the `Relaxed` ordering choice.

## Notes

Pass-2 review, findings M1, M2, M4, M6, M7, M8.

Dropped from scope:

- **M3** (repeated `id.clone()` in builder): low win; defer until the clone
  is demonstrated hot in a profile.
- **M5** (`test_support` public re-exports): already gated behind
  `#[cfg(any(test, feature = "test-support"))]` in
  `patches-core/src/lib.rs:13`. False alarm during review.
