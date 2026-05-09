---
id: "0853"
title: Multi-source input port shape (Source + SmallVec)
priority: medium
created: 2026-05-09
epic: E142
---

## Summary

First step of ADR 0071. Replace the per-port single-source fields on
`MonoInput`, `PolyInput`, `StereoInput` with an inline collection of
`Source` records. Reads iterate and sum. The graph builder still emits
at most one edge per input — no user-visible behaviour change. This
ticket exists to take the structural-API ripple in isolation, with
green CI before the multi-edge builder change in 0854.

## Acceptance criteria

- [ ] New `Source` struct in `patches-core/cables` with fields
      `cable_idx: usize`, `scale: f32`, `offset: f32`,
      `clip: Option<(f32, f32)>`, and (stereo-only)
      `broadcast_from_mono: bool`. Compile-time-constant `Source::ZERO`
      pointing at the read-sink slot for the disconnected default.
- [ ] `MonoInput` / `PolyInput` / `StereoInput` carry
      `sources: SmallVec<[Source; 1]>` (or equivalent inline-capacity-1
      vector — `smallvec` is acceptable as a new dependency, scoped to
      `patches-core`). `connected: bool` retained.
- [ ] `MonoInput::scalar(idx, scale)`, `MonoInput::single(source)`, and
      similar one-source constructors keep test/harness call sites
      single-line. `MonoInput::cable_idx` (currently a public field) is
      removed; consumers use `sources[0].cable_idx` or destructure.
- [ ] `pool.read_mono`, `pool.read_poly`, `pool.read_stereo` iterate the
      `sources` slice, applying per-source map and (stereo) broadcast,
      summing. Single-source case must compile to the same instructions
      as today (verify with one micro-bench in `patches-core` if a
      regression seems plausible).
- [ ] Cable builder populates a 1-element `sources` slice per input.
      `ModuleGraph::connect_with_map` still rejects duplicate inputs in
      this ticket — no semantic change yet.
- [ ] `ModuleHarness` (`patches-core/test_support`) updated to construct
      ports through the new constructors. Existing module tests pass
      unchanged.
- [ ] `just inner` green on the touched-crate scope.

## Out of scope

- Multi-edge connections. The builder still rejects them; that lands
  in ticket 0854.
- Retiring `fan_in.rs` / `Sum` / `PolySum` / `StereoSum`. Those land in
  0854 / 0855 once multi-edge actually flows.

## Notes

- Single-source port-write helpers (`pool.write_mono` / `write_poly` /
  `write_stereo`) are unaffected — outputs stay single-cable.
- `StereoInput::broadcast_from_mono` migrates from a port-level field
  to a per-`Source` field. Today only single-source stereo inputs use
  it, so the per-source field is set on `sources[0]` at builder time
  exactly as it is on the port today.
- `SmallVec` keeps `MonoInput`'s size in the same ballpark as today
  (one inline `Source` is `24` bytes; the SmallVec header adds
  capacity/length tracking). If the size growth matters for cache, the
  inline-capacity value is the lever — bumping past `1` is reserved
  for ticket 0854 once we measure.
- Document at the type level (one rustdoc paragraph per port struct)
  that `sources` is fixed at build time and never mutated on the audio
  thread.
