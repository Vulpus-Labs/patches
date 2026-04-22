---
id: "0628"
title: Capture bit-identical audio baseline for vintage modules (pre-migration)
priority: high
created: 2026-04-22
epic: "E109"
---

## Summary

Before `patches-vintage` leaves `default_registry()`, capture a
reference audio rendering of a fixed-input patch using the
in-process vintage modules. This is the parity oracle for Spike 8
Phase E; without it, there is no way to prove the bundle-loaded
version is bit-identical.

## Acceptance criteria

- [ ] Choose a fixed-input patch exercising at least VChorus (BBD
      and VFlanger if cheap to include). Patch source committed to
      `patches-integration-tests/fixtures/`.
- [ ] Render offline for N seconds at a fixed sample rate with a
      deterministic input (silence + impulse, or a committed WAV).
- [ ] Commit reference output WAV + SHA-256 hash alongside the
      patch fixture.
- [ ] Add a `#[test]` that renders the same patch through the
      in-process registry and asserts hash equality. Test must
      currently pass; it will be retargeted to the bundle in 0629.

## Notes

Render must be reproducible across machines: fixed block size,
fixed seed for any PRNG, no wall-clock dependencies. Document the
render invocation in the fixture directory README.
