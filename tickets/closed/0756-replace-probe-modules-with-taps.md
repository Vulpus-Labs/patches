---
id: "0756"
title: Replace ad-hoc probe modules in tests with taps
priority: medium
created: 2026-04-29
---

## Summary

Several integration tests carry bespoke probe modules whose only purpose is to expose internal state (cable values, connectivity flags, callback counts) to the test via `Arc<Mutex<…>>` / `AtomicU32` / `as_any().downcast_ref`. The tap API (ADR 0043, tickets 0752–0754) now provides a first-class mechanism to read this state, which removes the need for the probe modules entirely.

Candidates:

- `patches-integration-tests/tests/poly_cables.rs` — `PolyProbe` (Arc<Mutex> values + AtomicBool connectivity) → tap on the source's poly output port + connectivity query.
- `patches-integration-tests/tests/interval_scaling.rs` — `PeriodicCounter` (AtomicU32 counting `periodic_update` calls) → tap-based counter or scheduling observation.
- `patches-integration-tests/tests/connectivity_notification.rs` — `Probe` tracking `set_ports` calls → connectivity tap.
- `patches-planner/src/builder/tests/structural.rs::structural_change_rebuilds_instance` — `StructuralProbe` reached via `as_any().downcast_ref` to read `seen_path` → tap read on module-visible state.
- `patches-integration-tests/tests/poly_filters.rs::poly_filters_survive_plan_reload` — currently asserts only that `engine.last_left/right` is non-NaN after plan reload. The real invariant is "planner module-reuse path preserves filter state across reload"; the NaN check is a weak sentinel. Strengthen via a tap on the filter's coefficients / delay state and assert pre/post-reload equality (or sample-equal output to a no-reload control run). Test stays engine-level — this is a planner concern, not a kernel one.

## Acceptance criteria

- [ ] Each listed test rewritten to use taps; corresponding probe module deleted.
- [ ] No `Arc<Mutex<…>>` / `AtomicU32` / `as_any().downcast_ref` plumbing remains in these test files for the purpose of state inspection.
- [ ] Test intent preserved (same invariants checked, same coverage of edge cases).
- [ ] Tap API gaps surfaced during conversion captured as follow-up tickets rather than worked around with new probe modules.

## Notes

If a candidate cannot be converted because the tap API doesn't yet expose the needed signal (e.g. per-callback scheduling counts), file a follow-up ticket against the tap API and leave the probe in place for now — do not stretch the test to fit a partial conversion.

## Resolution (2026-04-30)

Closed as misframed. Per-candidate review against the current tap API (ADR
0043 / 0054 / 0059, `patches-modules/src/tap.rs`):

| Candidate                                            | Verdict                                                                                                                                                                                                          |
| ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `poly_cables.rs::PolyProbe`                          | Not convertible. `Tap` writes only mono/stereo/trigger to the backplane — poly cables are not tappable. Tests also inspect `set_ports` count and connectivity flags, which are not on a cable.                   |
| `interval_scaling.rs::PeriodicCounter`               | Not convertible. Counts `periodic_update()` calls; tap is per-sample raw publication, no surface for call counts.                                                                                                |
| `connectivity_notification.rs::Probe`                | Reframed: tests are already stubs that don't inspect probe state. Replaced `Probe` with `Tuner` (existing 1-mono-in / 1-mono-out module). No tap involvement, but the custom probe and its boilerplate are gone. |
| `patches-planner/.../structural.rs::StructuralProbe` | Not convertible. Planner-level test (`PatchBuilder` only); no engine, no audio thread, no tap backplane. Probe state inspection is the test's whole point.                                                       |
| `poly_filters.rs` (NaN sentinel)                     | Not convertible. Filter coefficients/delay state are module-internal, not on a cable.                                                                                                                            |

Tap API is for cable-sample observation, not module-internal inspection.
The premise that taps remove the need for these probes is wrong: probes
inspect call counts, port objects, and prepared internal state, none of
which are cable signals. Closing rather than splitting into per-gap
tickets, since "make module call counts and internal state observable
via taps" is not a desirable direction — it would re-create the ADR-0021
emit-style coupling that ADR 0043 deliberately rejected.

The only deliverable taken: `connectivity_notification.rs` Probe → `Tuner`
substitution, removing ~60 LOC of boilerplate.
