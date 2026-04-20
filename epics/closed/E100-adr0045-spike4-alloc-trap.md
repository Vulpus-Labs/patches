---
id: "E100"
title: ADR 0045 spike 4 — audio-thread allocator trap
created: 2026-04-20
depends_on: []
tickets: ["0591", "0592", "0593", "0594"]
---

## Goal

Spike 4 of ADR 0045. Provide a process-wide enforcement mechanism
for the no-allocation-on-the-audio-thread rule: a
`#[global_allocator]` shim, gated by a
`audio-thread-allocator-trap` cargo feature, that aborts on any
`alloc` / `dealloc` / `realloc` performed on a thread tagged as
audio-thread. Tag the real audio threads at startup so the trap
covers every entry path (CPAL callback, integration-test headless
tick), not just one test file.

Prior art: `patches-integration-tests/tests/alloc_trap.rs` already
carries a `TrappingAllocator` with a per-test scope guard and a
suite of per-patch sweeps. This spike generalises that mechanism
into a shared crate, switches from scope-guard activation to
thread-tag activation, and wires it through the real engine entry
points so all subsequent spikes (5, 6, 7, 8) run under the trap
without extra ceremony.

After this epic:

- A new `patches-alloc-trap` crate (or a module inside an
  existing common crate — see ticket 0591) exposes
  `TrappingAllocator`, `mark_audio_thread()`, and a no-op
  build-out when the feature is off.
- The feature defaults off; enabling it installs the allocator
  and arms the trap. When off, the crate compiles to nothing on
  the hot path and the workspace builds exactly as today.
- Every thread that calls into `PatchProcessor::process` /
  `HeadlessEngine::tick` in production or integration tests is
  tagged at its entry point.
- The existing per-scope guard in `alloc_trap.rs` is retired in
  favour of the thread-tag mechanism. Its test coverage (simple,
  poly_synth, fm_synth, fdn_reverb_synth, pad, pentatonic_sah,
  drum_machine, tracker_three_voices) stays green, now running
  against the thread-tagged allocator.
- A deliberate-alloc negative test confirms the trap actually
  aborts when an audio-tagged thread allocates.
- CI has a debug job that builds and tests the workspace with
  `--features audio-thread-allocator-trap`.

## Tickets

| ID   | Title                                                                 | Priority | Depends on |
| ---- | --------------------------------------------------------------------- | -------- | ---------- |
| 0591 | TrappingAllocator in shared crate behind feature flag                 | high     | —          |
| 0592 | Audio-thread tagging API (`mark_audio_thread`, per-thread TLS)        | high     | 0591       |
| 0593 | Tag engine audio threads at entry (CPAL callback, headless ticks)     | high     | 0592       |
| 0594 | Migrate alloc_trap integration tests + deliberate-alloc negative test + CI job | high | 0593 |

## Affected surface

- New crate `patches-alloc-trap` (or equivalent module) —
  allocator shim, tagging API, feature-gated.
- `patches-cpal::callback`: tag the CPAL callback thread on first
  entry.
- `patches-integration-tests`: rewrite `alloc_trap.rs` to use the
  shared mechanism; delete the local `TrappingAllocator` and
  `NoAllocGuard`.
- `patches-player`, `patches-clap`: no code change; they already
  drive audio through the tagged thread.
- CI config: one debug job added with the feature enabled.

## Design notes

- **Global allocator, per-thread flag.** One `GlobalAlloc` impl
  process-wide. A `thread_local! { static AUDIO_THREAD: Cell<bool> }`
  flag controls whether alloc calls abort. This matches the
  shape already working in `alloc_trap.rs`; the change is
  lifetime (thread-wide, set once at audio-thread startup)
  rather than scope (guard per tick).
- **Feature off = zero cost.** When
  `audio-thread-allocator-trap` is off, `TrappingAllocator`
  compiles to a transparent forward to `System`, and
  `mark_audio_thread()` is an empty inline function. No
  `#[global_allocator]` statement is emitted; downstream crates
  inherit the system allocator unchanged.
- **Feature on = shim installed.** The crate emits
  `#[global_allocator] static A: TrappingAllocator = …`. Any
  crate that links `patches-alloc-trap` inherits the shim.
  Binaries that want the trap depend on
  `patches-alloc-trap` with the feature on; libraries stay
  feature-agnostic.
- **Abort, not panic.** `panic!` allocates. The shim uses
  `std::process::abort()` so the debugger catches the exact
  call stack.
- **`TRAP_ARMED` latch.** The thread-local check is guarded by a
  process-wide `AtomicBool TRAP_ARMED` that is set true the
  first time `mark_audio_thread()` runs. Until then, the trap
  is inert — this protects static initialisers and any
  pre-tag allocation in the CPAL stack.
- **Scope guards retained as a test utility.** The existing
  `NoAllocGuard` pattern is still useful in tests that want to
  arm the trap on a thread that isn't the real audio thread
  (e.g. `HeadlessEngine::tick` called from a unit test). Export
  both: `mark_audio_thread()` for production entry points,
  `NoAllocGuard::enter()` for test scopes.

## Definition of done

- `cargo build --workspace` and
  `cargo build --workspace --features
  patches-alloc-trap/audio-thread-allocator-trap` both clean.
- `cargo test --workspace` green with the feature off (default).
- `cargo test --workspace --features
  patches-alloc-trap/audio-thread-allocator-trap` green in
  debug. Same tests, same assertions.
- The deliberate-alloc negative test confirms the trap fires
  (runs as a `#[should_panic]`-equivalent via a subprocess that
  is expected to abort with a non-zero status).
- `patches-player` runs end-to-end with the feature on for at
  least 10 s of real playback without aborting.
- Clippy clean on the new crate.
- No new third-party dependency beyond `libc` (for `abort` — or
  just `std::process::abort`, no new dep).

## Non-goals

- Release-build coverage. The trap is a debug tool; release
  builds never install it. The feature guard ensures this.
- Per-allocation attribution (which call site allocated). The
  abort-on-first-hit gives a debugger stack — good enough for
  this spike. A "count but don't abort" soft mode is a possible
  future extension noted inline in `alloc_trap.rs`.
- Instrumenting the FFI path. Spike 4 covers in-process modules
  only. Spike 7 re-uses the same trap to validate external
  plugins.
- `ParamView` migration. Spike 5 depends on the trap landing
  here but does not change until that spike.

## Relationship to other spikes

- Independent of spikes 1, 2, 3 — may land in any order. ADR
  0045 explicitly calls it out as "land as early as convenient".
- Spike 5 (`ParamView` migration) will run its full-suite
  validation under the trap; spike 5 assumes spike 4 is in
  place.
- Spikes 6, 7, 8 all retro-validate against the trap.
