# ADR 0023 — Windowed cross-thread transfer buffer (`SlotDeck`)

**Date:** 2026-03-31
**Status:** Proposed

---

## Context

Certain audio processing algorithms are too expensive to run inline on the audio
thread (e.g. convolution reverb, spectral effects via FFT, neural post-processing).
These algorithms share a common shape:

1. Collect a window of N input samples on the audio thread.
2. Transfer the window to a processing thread.
3. The processing thread transforms the window (windowing → FFT → spectral
   transform → IFFT → overlap-add normalisation, or equivalent).
4. Transfer the result back to the audio thread.
5. The audio thread reconstructs the output stream by summing overlapping result
   windows (overlap-add).

This pattern requires:
- **No allocation on the audio thread.** All buffers must be pre-allocated.
- **No blocking on the audio thread.** If a required resource is unavailable,
  the audio thread must degrade gracefully (drop writes, output silence) rather
  than wait.
- **Correct ownership** of each buffer at all times so no unsafe shared-mutable
  access is needed.
- **Support for arbitrary overlap factors** (>2) and window sizes to support
  COLA-compliant configurations.

The structure should be **general-purpose** — it does not encode any knowledge
of FFT, windowing functions, or spectral transforms. Those are the concern of the
processing thread. The transfer mechanism simply moves owned buffer slots between
threads in a controlled way.

---

## Decision

### Abstraction: `SlotDeck`

The fundamental structure is a **`SlotDeck`**: a fixed pool of `Box<[f32]>` buffer
slots, initially owned entirely by one side, that flow in a directed cycle:

```
     ┌──────────────────────────────────────┐
     │           audio thread               │
     │  free_slots ──► active_slots ──► filled_tx
     │       ▲                              │
     │  recycled_rx ◄───────────────────────┘
     │                            (processor thread)
     └───────────────────────────────────────┘
```

A `SlotDeck` is instantiated in pairs:

```
audio writes → processor  :  WritesDeck { sender: SlotDeckSender, receiver: SlotDeckReceiver }
processor writes → audio  :  ReadsDeck  { sender: SlotDeckSender, receiver: SlotDeckReceiver }
```

Both decks use the same type with different ownership origins:
- `WritesDeck`: initial slots given to the audio thread (it is the sender).
- `ReadsDeck`: initial slots given to the processing thread (it is the sender).

This symmetry means the same type and channel structure handles both directions.

### Slot lifecycle

Each slot is a `Box<[f32]>` of fixed length (`window_size` floats). Its lifecycle
within one deck is:

```
  free (held by sender)
      │
      │  sender opens a new window, claims a free slot
      ▼
  active (sender is writing into it)
      │
      │  sender's write head passes the end of the window;
      │  slot pushed to filled_tx channel
      ▼
  in-flight (inside rtrb channel)
      │
      │  receiver pops from filled_rx channel
      ▼
  processing (receiver owns it, does work)
      │
      │  receiver finishes; pushes to recycled_tx channel
      ▼
  in-flight (inside rtrb recycle channel)
      │
      │  sender pops from recycled_rx and returns to free list
      ▼
  free (held by sender)  ── cycle repeats
```

### Channels (four per pair of decks; eight total for a write+read pair)

All channels are `rtrb` SPSC ring buffers (wait-free, no allocation on push/pop).

| Channel | Direction | Payload | Purpose |
| --- | --- | --- | --- |
| `filled_tx/rx` | sender → receiver | `FilledSlot { start: u64, data: Box<[f32]> }` | Transfer a completed input window |
| `recycled_tx/rx` | receiver → sender | `Box<[f32]>` | Return an empty slot for reuse |
| `result_tx/rx` | processor → audio | `FilledSlot { start: u64, data: Box<[f32]> }` | Transfer a completed output window |
| `result_recycle_tx/rx` | audio → processor | `Box<[f32]>` | Return an empty output slot for reuse |

(`result_*` channels are the `filled_*` and `recycled_*` of the `ReadsDeck`.)

### Audio-thread state

The audio thread maintains two fixed-capacity arrays (no heap allocation after
init), sized by `const MAX_OVERLAP: usize`:

```rust
active_inputs:  [Option<(u64, Box<[f32]>)>; MAX_OVERLAP]  // being written
active_outputs: [Option<(u64, Box<[f32]>)>; MAX_OVERLAP]  // being summed
```

and two scalar cursors:

```rust
write_head: u64   // sample position of the next input sample
read_head:  u64   // sample position of the next output sample
```

### Write-side logic (audio thread, per sample)

```
every hop_size samples (write_head % hop_size == 0):
    drain recycled_rx to replenish free_slots
    if free_slots is non-empty:
        claim a slot; record start = write_head; place in active_inputs

for each slot in active_inputs:
    if write_head falls within [slot.start, slot.start + window_size):
        slot.data[write_head - slot.start] = input_sample
    if write_head == slot.start + window_size - 1:
        push FilledSlot { start, data } to filled_tx (non-blocking)
        if push fails (channel full): return data to free_slots  ← writes dropped
        remove slot from active_inputs
```

Failure modes are silent degradation, never blocking.

### Read-side logic (audio thread, per sample)

```
drain result_rx: move newly-completed output windows into active_outputs
    if active_outputs is full: push displaced slot to result_recycle_tx (or drop)

output_sample = 0.0
for each slot in active_outputs:
    if read_head falls within [slot.start, slot.start + window_size):
        output_sample += slot.data[read_head - slot.start]
    if read_head >= slot.start + window_size - 1:
        push data to result_recycle_tx (non-blocking)
        remove slot from active_outputs

read_head += 1
```

### Processing-thread logic

```
loop:
    while let Some(FilledSlot { start, data }) = filled_rx.pop():
        // data holds raw samples from audio thread
        let mut out = get_free_output_slot()   // from result_recycle_rx or pre-alloc
        process(&data, &mut out)               // window fn, FFT, transform, IFFT, etc.
        result_tx.push(FilledSlot { start: start + latency_offset, data: out })
        recycled_tx.push(data)                 // return input slot
    sleep_or_yield()
```

The processing thread applies windowing functions, normalisation, and any
transforms. The transfer mechanism has no knowledge of these.

### Configuration parameters

All three parameters must be powers of 2:

| Parameter | Example | Meaning |
| --- | --- | --- |
| `window_size` | 2048 | Length of each analysis/synthesis window in samples |
| `overlap_factor` | 4 | Number of overlapping windows; `hop_size = window_size / overlap_factor` |
| `processing_budget` | 128 | Audio-clock samples the processor is allowed before a result is considered late |

### Latency

Total latency has two additive components:

```
total_latency = window_size + processing_budget
```

**Collection latency (`window_size` samples):** a full window must be filled
before it can be dispatched to the processor. This is unavoidable.

**Processing budget (`processing_budget` samples):** the amount of audio-clock
time the processor is permitted to take. This is a configuration choice that
trades off latency against the risk of late frames. A larger budget reduces
dropped results under load at the cost of increased end-to-end latency.

At steady state the `read_head` lags the `write_head` by exactly
`window_size + processing_budget` samples. The output stream is silent for
the first `window_size + processing_budget` samples while the pipeline fills.

**Example:** `window_size = 2048`, `processing_budget = 128` → total latency
= 2176 samples ≈ 45 ms at 48 kHz.

**Late-frame discard:** a result frame is late — and discarded — when the audio
thread's `read_head` has already advanced past `frame.start + window_size`.
Because `read_head` lags `write_head` by exactly `total_latency`, this is
equivalent to the processor having taken more than `processing_budget`
audio-clock samples for that frame.

### Pool sizing

Given overlap factor `F`, window size `W`, and processing budget `B`
(hop = `W/F`, `pipeline_slots = B / hop`):

- At most `F` input slots are active on the audio thread at any moment.
- At most `F` output slots are active on the audio thread.
- `pipeline_slots` additional slots per direction cover frames in-flight or
  being processed during the budget window.

Recommended: `total_slots = 2 * F + pipeline_slots` per direction.
With the example values: `2 * 4 + (128 / 512) = 8 + 1` → 9 slots, round up
to a power of 2 → 16 per direction.

### COLA compliance

The transfer mechanism is COLA-agnostic. The processing thread is responsible
for choosing a window function and normalisation factor consistent with
Constant Overlap-Add reconstruction. For a Hann window with overlap factor 2,
the standard COLA condition is satisfied. The audio thread's overlap-add read
(simple summation) is correct for any COLA-compliant window/hop configuration.

### Crate placement

`SlotDeck` lives in `patches-dsp`. It has no dependencies on `patches-core`,
`patches-modules`, or any audio-backend crate — only `rtrb`. It is a
general-purpose real-time transfer primitive.

---

## Considered alternatives

### Shared ring buffer with atomic write/read heads

A single large ring buffer, with the audio thread writing at the write head and
the processing thread reading overlapping windows by polling. Rejected because:

- Detecting "this window is complete" requires the processing thread to compare
  the write head against each window's end — extra synchronisation overhead.
- The processing thread cannot know when a window has been fully written without
  a separate atomic or notification signal.
- No natural mechanism for the processor to "return" output windows to the audio
  thread; a second shared ring would still be needed for results.

The `SlotDeck` approach uses ownership transfer (move semantics via `rtrb`) as the
synchronisation primitive, which is idiomatic in Rust and avoids manual atomic
bookkeeping.

### Single shared buffer with `Arc<Mutex<_>>`

Rejected immediately: any mutex on the audio thread violates the no-blocking
requirement.

### Atomic-state-machine per slot

Each slot tagged with an `AtomicU8` state (`Free`, `Writing`, `Processing`,
`Ready`, `Reading`). Both threads poll slot states directly.

Rejected because:
- Both threads need access to the same array, requiring `Arc` or `unsafe`.
- The audio thread must scan all slots on every sample — O(N) per sample vs
  the proposed O(overlap_factor) per sample.
- Reasoning about correctness of relaxed/acquire/release orderings across
  multiple state machines is fragile.

### `Box<[f32]>` moved through channels vs index into shared slab

Alternative: pre-allocate a `Box<[[f32; W]; N]>` slab, pass `usize` indices
through channels. Avoids moving fat pointers.

Rejected for the ADR design phase because:
- Moving a `Box<[f32]>` through `rtrb` copies only a pointer (8 bytes) — same
  cost as moving an index.
- Index-based schemes require `unsafe` to access the slab from two threads
  even with rtrb-as-synchronisation. `Box` moves are sound by construction.
- Implementation complexity is higher with no measurable performance benefit.

This may be revisited if profiling shows channel contention.

---

## Consequences

- **Positive:** No allocation on audio thread after initialisation. No locks.
  Correct ownership by construction (Rust move semantics, no `unsafe` in the
  transfer layer). General-purpose: usable for any windowed algorithm.
- **Positive:** Symmetric design — the same `SlotDeck` type handles both
  input→processor and processor→output directions.
- **Positive:** Silent degradation under overload: writes are dropped, reads
  output silence. The audio thread never stalls.
- **Negative:** Inherent window-size latency is unavoidable and must be
  communicated to users of any module built on this mechanism.
- **Negative:** Pool sizing requires tuning. Under-sizing the pipeline slack
  causes dropped windows under heavy load. A monitoring counter (dropped input
  windows, dropped output windows) should be exposed for diagnostics.
- **Negative:** The processing thread must be managed externally (spawned,
  joined, given handles). `SlotDeck` does not own or manage the processing thread.

---

## Implemented in

*Not yet implemented — design phase.*
