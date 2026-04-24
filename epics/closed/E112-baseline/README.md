---
epic: E112
purpose: Baseline disasm of filter kernels before CoefRamp refactor
captured: 2026-04-23
---

# E112 baseline disasm

Reference disassembly of hot filter kernel functions before the
`CoefRamp` refactor (epic E112, tickets 0653–0656). Ticket 0655 diffs
against these files as its go/no-go gate.

## Environment

- `rustc 1.94.0 (4a4ef493e 2026-03-02)`
- Target: `aarch64-apple-darwin` (Apple Silicon, NEON)
- Profile: `release` (`cargo build -p patches-dsp --release`)
- Toolchain: system `objdump` (llvm)

## Files

| File | Symbol | Source |
|------|--------|--------|
| [monobiquad_tick.s](monobiquad_tick.s) | `patches_dsp::biquad::MonoBiquad::tick` | [patches-dsp/src/biquad/mod.rs:120](../../../patches-dsp/src/biquad/mod.rs#L120) |
| [polybiquad_tick_all.s](polybiquad_tick_all.s) | `patches_dsp::biquad::PolyBiquad::tick_all` | [patches-dsp/src/biquad/mod.rs:279](../../../patches-dsp/src/biquad/mod.rs#L279) |

## Capture method

```bash
cargo build -p patches-dsp --release
cd target/release/deps
ar x libpatches_dsp-*.rlib
objdump -d patches_dsp-*.cgu.2.rcgu.o > full.s
# Symbols located via `nm -g` on the extracted object files;
# per-symbol sed extracted from full.s.
```

## What to look for in the diff

- **PolyBiquad::tick_all**: every per-step loop compiles to `ldp q*, q*` /
  `fmul.4s` / `fmla.4s` / `stp q*, q*` sequences — two Q-register passes
  cover all 16 voices per step. The refactored version must produce the
  same fmul/fmla/ldp/stp pattern with the same number of SIMD ops.
  Register allocation may differ; instruction count and shape may not.
- **MonoBiquad::tick**: scalar; reads coefs + state, does TDFII recurrence,
  advances 5 deltas. Post-refactor should be equivalent.
- The `saturate` branch cbz at the top of each function: both branches
  should remain after the refactor (tanh and linear paths).
- The `ramp` branch cbz (PolyBiquad only): the delta-advance loop should
  remain gated on `has_cv` in the same position.

## Pass bar

Ticket 0655 passes the gate iff:

- Same count of SIMD ops (`fmul.4s`, `fmla.4s`, `fadd.4s`) in the hot loop
  region, ±register allocation differences.
- Same branching structure (saturate cbz, ramp cbz, early returns).
- No new function calls introduced in the hot path.
- If any of the above fails, revert ticket 0654 and close E112 at 0655.
