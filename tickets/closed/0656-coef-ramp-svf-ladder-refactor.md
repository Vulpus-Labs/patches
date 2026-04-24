---
id: "0656"
title: Refactor SVF / ladder / ota_ladder onto CoefRamp
priority: medium
created: 2026-04-23
epic: E112
depends_on: ["0655"]
---

## Summary

After the 0655 codegen gate passes, migrate the remaining filter kernels
onto `CoefRamp` / `PolyCoefRamp`. One commit per kernel, mono + poly
separable.

## Acceptance criteria

- [ ] `SvfKernel` + `PolySvfKernel`: K=2 (f, q_damp). `stability_clamp`
      stays in the kernel's `begin_ramp` wrapper that builds the
      `[f32; 2]` target array before delegating. Shared clamp of active
      `f` on snap must still happen — not in `CoefRamp`, in the wrapper.
- [ ] `LadderKernel` + `PolyLadderKernel`: K=3 (g, k, drive). `variant`
      stays as a sibling field on the kernel (not ramped).
- [ ] `OtaLadderKernel` + `PolyOtaLadderKernel`: K per current shape.
- [ ] One commit per kernel; tests pass after each.
- [ ] `cargo clippy` clean after each commit.
- [ ] Dependent module code (`patches-modules/src/svf.rs`, `poly_svf.rs`,
      filter variants, `patches-vintage/src/vladder.rs`,
      `fdn_reverb/processor.rs`) still compiles + tests pass.

## Notes

If any kernel has an extra invariant (SVF's clamp-on-snap) that doesn't
fit `CoefRamp`'s generic begin_ramp, express it in the wrapper — don't
add conditional logic to `CoefRamp`. If the wrapper-pattern fails to
preserve the invariant cleanly for some kernel, mark that kernel as
"keep hand-rolled" in the epic doc and move on.

If 0655 failed and was reverted, this ticket closes as no-op (primitive
stays, no kernel refactors).
