---
id: "E111"
title: ADR 0045 Spike 9 — fuzzing, property tests, observability
created: 2026-04-23
adrs: ["0045", "0043"]
tickets: ["0649", "0650", "0651", "0652"]
---

## Goal

Close out ADR 0045 Spike 9: verify the parameter/port data plane
against adversarial input and make it observable in production. This
is the deeper testing wave that E094 and E095 shipped past without
covering.

ADR 0045 body:
`Spike 9 is not yet closed` (adr/0045-ffi-parameter-port-data-plane.md:592).

## Scope

1. **Frame fuzzing** — malformed `ParamFrame` inputs (wrong size,
   wrong layout hash, corrupted tail) are rejected, never decoded.
2. **`ArcTable` fuzz** — randomised retain/release sequences hold
   invariants (no double free, refcount monotone-to-zero, no leak).
3. **Soak test** — 10 000+ cycle integration run with randomised
   parameter updates, asserting no audio-thread allocation (via
   Spike 4 trap) and clean `Arc` cleanup on shutdown.
4. **Observability counters** — per-runtime: table capacity, high
   watermark, growth events, frame-dispatch rate, pending-release
   queue depth. Surfaced via tap infrastructure (ADR 0043).

## Tickets

| ID   | Title                                                  | Priority | Depends on |
| ---- | ------------------------------------------------------ | -------- | ---------- |
| 0649 | ParamFrame malformed-input fuzz (size/hash/tail)       | high     | —          |
| 0650 | ArcTable retain/release randomised fuzz               | high     | —          |
| 0651 | 10k-cycle soak under allocator trap, randomised params | high     | 0649, 0650 |
| 0652 | Runtime counters + tap exposure (ADR 0043)             | medium   | —          |

## Definition of done

- Fuzz targets committed and runnable in CI (time-boxed) plus a
  longer nightly budget.
- Soak test green under allocator trap; clean Arc refcounts at
  shutdown.
- Counters readable via tap; documented in the manual alongside
  existing observability.
- ADR 0045 status updated: Spike 9 closed.
- `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.

## Out of scope

- New module migrations or ABI changes (Spikes 7/8 done).
- Fuzzing of DSL parser or interpreter — different surface.
