---
id: "0655"
title: Verify PolyBiquad codegen after CoefRamp refactor — go/no-go gate
priority: medium
created: 2026-04-23
epic: E112
depends_on: ["0654"]
---

## Summary

Confirm the `CoefRamp` refactor produces equivalent SIMD code for
`PolyBiquad::tick_all`. This is the gate that decides whether to proceed
with SVF / ladder / ota_ladder migrations (ticket 0656).

## Acceptance criteria

- [ ] Disassemble `PolyBiquad::tick_all` before and after the 0654
      refactor (release build, `cargo asm` or `objdump`). Compare the
      per-step loops — they should be byte-identical or trivially
      different (register allocation only).
- [ ] If disassembly is unclear, run a micro-bench: generate CV-modulated
      input over N blocks, time `tick_all`. Pass bar: within ±2% of
      baseline.
- [ ] Test on both architectures the team builds for:
      - x86_64 with AVX2 (check `vaddps` / `vmulps` over ymm)
      - aarch64 with NEON (check `fmla` over v-regs)
      If access to one is a blocker, document which was verified.
- [ ] Decision recorded: proceed to 0656, or revert 0654 and close the
      epic as "primitive built, refactor regressed — kept primitive as
      reference, kernels stay hand-rolled".

## Notes

If `[[f32; 16]; 5]` access via `self.coefs.active[k][i]` produces worse
code than named `b0[i]` fields, try `#[repr(C)]` on the ramp structs or
add `#[inline(always)]` to accessors. Do not hand-roll the step loops
back into the kernel without first checking whether LLVM needs a hint.
