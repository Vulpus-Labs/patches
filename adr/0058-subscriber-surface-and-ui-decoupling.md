# ADR 0058 — Subscriber surface: state vs events, mutex-based vectors, UI decoupling

**Date:** 2026-04-26
**Status:** Accepted
**Related:**
[ADR 0053 — Observation three-thread split](0053-observation-three-thread-split.md),
[ADR 0056 — Observer pipeline and frame layout](0056-observer-pipeline-and-frame-layout.md)

## Context

ADR 0053 §8 sketched the observer→UI handoff as "speculative" and offered
two candidate transports (`ArcSwap` triple-buffer or `Arc<Mutex<Frame>>`)
without picking one. ADR 0056 §6 said scope/spectrum vector outputs
"need their own publish mechanism (per-slot double buffer or seqlock)
parallel to the scalar surface" and deferred the choice as "out of scope
for the meter-only bringup."

Tickets 0701, 0705, 0709, 0710 have now built that surface. Implementation
choices were forced by the work but not documented. This ADR records
what was built and why, so future readers don't have to reconstruct the
rationale from code, and so future additions (event-shaped observations,
trigger LEDs, MIDI flashes) land against an explicit contract.

The decisions cover:

1. State (pull) vs events (push) — when to use each.
2. Vector storage: `Mutex<Option<Vec<f32>>>` rather than `ArcSwap` /
   seqlock / triple-buffer.
3. Per-tap variable buffer lengths (scope, post-0710).
4. Coalescing semantics: where work is and isn't saved.
5. Sample-time vs wall-clock split.

## Decisions

### 1. Two transports, chosen by question shape

The subscriber surface is split by the *question* the consumer is asking:

- **"What is X right now?"** → pull, latest-wins. Stored in
  `LatestValues`. Includes meter peak/RMS scalars, spectrum frames,
  scope buffers. UI polls at paint rate (30 Hz TUI, 60 Hz GUI).
  Falling behind silently coalesces — no overflow, no backpressure,
  no ordering guarantee across publishes.
- **"Tell me each time X happens"** → push, ordered, lossy on full.
  Stored in an SPSC `rtrb` ring. Includes diagnostics
  (`NotYetImplemented`, `InvalidSlot`), and is the right home for
  future event-shaped observations: trigger fires, gate edges,
  one-shot MIDI flashes, replan completion notices, manifest errors.
  UI drains on each frame; missed events would be lost on full, but
  the ring is sized so that any realistic UI cadence keeps up.

Drop counters bypass `LatestValues` and are read directly from the
audio-side ring's shared atomic state via `SubscribersHandle::dropped`.

The rule for adding a new observation kind: if coalescing it would
destroy meaning, it goes on the event ring. Otherwise it goes on
`LatestValues`.

### 2. Vector storage is `Mutex<Option<Vec<f32>>>`

Per-slot spectrum and scope buffers are stored as
`[Mutex<Option<Vec<f32>>>; MAX_TAPS]`:

- `Mutex` is acceptable because **both sides are off the audio
  thread.** Observer thread writes; UI thread reads. Contention is
  observer-vs-UI on a small memcpy (~2 KB spectrum, ~128 B–32 KB
  scope), which is bounded and non-real-time.
- `Option` distinguishes "never published" from "published zeros" so
  `read_*_into` can return a meaningful `bool` and the UI can render
  a "no data yet" state instead of a flat line at zero.
- `Vec<f32>` (rather than `Box<[f32; N]>`) accommodates per-tap
  variable lengths — see §3.

Alternatives rejected:

- **`ArcSwap` / triple-buffer.** Wait-free read, lock-free write, and
  an obvious match for "swap a frame." Rejected: the read-side
  consumer has to allocate or hold an `Arc` reference long enough to
  copy out, which complicates the UI's "copy into a reusable scratch
  buffer" pattern. The lock cost we're avoiding is microseconds; the
  per-publish allocation it introduces is a worse trade.
- **Seqlock.** Versioned read-retry-on-write. Sound but more code for
  a problem that doesn't exist (no audio-thread reader, contention
  is rare).

The rule of thumb: any state held by the observer that's bigger than
one machine word lives behind a mutex. Anything that fits in one word
goes in an atomic cell.

### 3. Per-tap variable buffer lengths

Ticket 0710 made the scope buffer length a per-tap parameter
(`osc.window_ms`). Storage in `LatestValues::scopes` is therefore
`Vec<f32>` rather than `Box<[f32; SCOPE_BUFFER_LEN]>`. `publish_scope`
resizes the vec on length change; the per-tap processor allocates its
ring once at construction (per ADR 0056 §5: param change → identity
change → fresh processor) so the published length is stable for the
life of a tap.

Spectrum buffers remain fixed-length (`SPECTRUM_BIN_COUNT`) because
FFT size is a compile-time constant; if/when it becomes a per-tap
param the same `Vec`-backed pattern applies.

### 4. Coalescing is on transport, not on work

`LatestValues` saves *bandwidth and lock time*, not *CPU*. The observer
thread still:

- Drains every block from the tap ring.
- Runs every processor on every block.
- Publishes on every emit boundary (per-block for meters, every
  `SCOPE_EMIT_BLOCKS` for scope, every `SPECTRUM_HOP_BLOCKS` for
  spectrum).

UI just memcpys whichever happens to be latest at poll time. So at
48 kHz / TAP_BLOCK = 64 the observer computes ~187 scope frames/sec
and ~187 spectrum frames/sec per tap regardless of UI rate.

If observer CPU becomes a problem the lever is **emit cadence**, not
the bus. Options, in increasing complexity:

- Bump `SCOPE_EMIT_BLOCKS` / `SPECTRUM_HOP_BLOCKS` (also affects
  spectrum time resolution, not just CPU).
- Skip processors whose subscriber surface hasn't been read since
  last publish. Cheap: one atomic "consumed" flag per slot. Adds a
  coupling `LatestValues` currently doesn't have.
- UI-driven pacing: observer only computes on `request_frame` token
  via SPSC. Cleanest CPU-wise, worst latency-wise.

None are warranted at current scale (1024-pt FFT every 5 ms is
trivial). Documented so the ceiling is known and the levers are
explicit.

### 5. Sample-time vs wall-clock split

Per ADR 0056 §7 the audio→observer leg carries `sample_time`. This
ADR formalises what each clock is *for*:

- **Sample-time** governs cross-tap coherence. Scope decimation is
  anchored on `t % SCOPE_DECIMATION == 0`, so every scope tap picks
  out the same source samples and overlaid waveforms share an x-axis
  origin. Any future trigger / sweep / cross-tap alignment lives in
  sample-time.
- **Wall-clock** governs paint scheduling and bus poll timing. UI
  repaints on display vsync; observer wakes when the OS schedules it.
  Jitter on this clock is bounded by display frame time and is
  sub-perceptual for visualisation.

Visible jitter sources, all bounded:

- Observer wake latency (sub-ms typical, tens of ms under load).
- Mutex contention on vector publishes (microseconds).
- UI poll phase vs observer publish phase (≤ one UI frame).

The contract the surface offers is **"eventually, soon, roughly in
order"** — exactly what atomics + per-slot mutexes deliver with no
orchestration. If audio were being generated from this path the
contract would have to be sample-accurate end-to-end; for visualisation
it deliberately is not.

## Consequences

- ADR 0053 §8 ("Observer → UI handoff (speculative)") is superseded by
  this ADR. The choice between `ArcSwap` and `Mutex` is closed in
  favour of `Mutex<Option<Vec<f32>>>`.
- ADR 0056 §6's deferred "per-slot double buffer or seqlock" choice is
  closed the same way.
- Adding a new scalar observation = add a `ProcessorId` variant, bump
  `ProcessorId::COUNT`, allocate one more `AtomicU32` per slot.
- Adding a new vector observation = add a sibling `[Mutex<Option<Vec<...>>>; MAX_TAPS]`
  to `LatestValues` with matching publish/read methods.
- Adding a new event observation = add a variant to `Diagnostic` (or a
  parallel enum if the event is non-diagnostic).
- The "consumed" flag optimisation is unblocked but unbuilt. If the
  observer becomes CPU-bound on idle UI, the change is local: an
  `AtomicBool` per slot, set on read, cleared on publish, with the
  observer skipping process() when set.
- `SCOPE_BUFFER_LEN` is no longer a global invariant — it's the
  default ring length when `osc.window_ms` is omitted. Code reading
  scope buffers must use the actual published length, not the
  constant.

## Cross-references

- ADR 0053 §8 — speculative section now closed by this ADR.
- ADR 0056 §6 — vector publish mechanism now specified here.
- Ticket 0710 — introduced per-tap variable scope length, forcing the
  `Vec`-backed storage.
