# ADR 0068 — Untagged cable pool and host-control scratch buffer

**Date:** 2026-05-04
**Status:** Proposed
**Related:**
[ADR 0057 — Host control cables](0057-host-control-cables.md),
[ADR 0045 — Backplane regions](0045-backplane-regions.md),
[ADR 0046 — Patch parameters](0046-patch-parameters.md)

## Context

Two related questions surfaced while implementing ADR 0057's host
control cables (epic E135):

1. **Cable pool tagging.** `CableValue` is currently
   `enum { Mono(f32) | Poly([f32; 16]) }`. Every cable read goes
   through a `match` whose unreachable arm is documented as "graph
   validation should prevent this." Connection bind already determines
   each cable's kind statically; the runtime tag duplicates information
   the planner already owns.

2. **Host control automation event handling.** CLAP delivers all param
   events for an audio block as a single sorted event list at the top
   of `process()`. Today's design (ticket 0809) writes events into a
   backplane region one slot at a time, with the audio thread reading
   the cell every sample. This:
   - gives only block-rate automation (ADR 0057 §4 explicitly parks
     sample-accurate as future work);
   - generates zipper noise on knob / slider sweeps because the
     block-rate step has no smoothing;
   - leaves the per-sample read pattern incoherent — modules read one
     scalar from a backplane cell per sample with no batched access.

The two are linked. If we want sample-accurate automation with cheap
smoothing, we need a per-block scratch buffer the audio thread fills
once and the engine reads with a coherent stride. The most efficient
write from that scratch into the cable pool is a contiguous `memcpy`
across the four reserved control cables. The `CableValue` tag prevents
that.

## Decision

### 1. Drop the `CableValue` tag; keep slot width fixed at `[f32; 16]`

Replace `CableValue` with a fixed-width `[f32; 16]` per cable slot.
Cable kind is known statically by reader and writer from the connection
descriptor (already enforced at bind time). The 16-wide slot stays so
that a slot used as Mono in one plan can be repurposed as Poly in a
later plan without reallocating the cable pool.

- `read_mono` reads `slot[0]`.
- `read_stereo` reads `(slot[0], slot[1])`.
- `read_poly` returns `slot[..16]` directly (no enum unwrap).
- `write_*` symmetric: writers know their kind and write the relevant
  prefix; bytes beyond the prefix are unspecified.
- The unreachable-arm panics in `read_*` go away; no behavioural
  change for well-formed graphs (graph validation already rejects
  kind-mismatched reads at bind time).
- FFI raw-parts API surface adjusts: `(*mut [f32; 16], len, wi)`.

### 2. Pre-render host control events into a SoA scratch, transpose to AoS frame, memcpy to backplane

At the top of each `process()` call:

1. **Step-fill (SoA `[channel][sample]`).** Walk the `clap_input_events`
   list once. For each param event, resolve `id → channel` via a flat
   lookup shipped with the current plan. Carry-forward the previous
   value into samples `[last_offset .. event_offset)` of the channel's
   row, then write the new value at `event_offset`. Tail-fill to the
   block end. Trigger channels are zero-filled with a `1.0` impulse at
   each event sample; toggle channels carry-forward. Unaffected
   channels carry their previous-block tail.

2. **Smoothing (per-row, in-place).** For each channel where
   `kind.smoothed()` is true (knob, slider), apply a one-pole
   `y[n] = y[n-1] + α (step[n] - y[n-1])` in-place over the row, with
   `y[-1]` taken from the per-channel tail state from the previous
   block. α is derived from the host sample rate and a fixed
   ~5 ms time constant. Toggle and trigger rows skip this pass.

3. **Transpose to AoS frame `[sample][64]`.** One linear pass; output
   buffer `[block_size][64]` of `f32`. After this point the per-sample
   layout is contiguous: 64 floats == 4 cables × 16 channels per cable.

4. **Persist tail.** Copy each row's last sample back into the
   per-channel tail state for the next block.

The audio thread per-sample tick of `HostControl` reduces to:

```rust
pool[hc_base..hc_base + 4][wi].copy_from_slice(
    bytemuck::cast_slice(&frame[t]),
);
```

— one 256-byte `memcpy` per sample, four contiguous cable slots
written in one shot. This relies on (1) being in place.

### 3. Fixed channel cap of 64 across four contiguous cables

The host-control backplane reserves four contiguous cable slots,
yielding 64 control channels (4 × 16). The registry's `live` map and
tombstone-table cap are both bounded by 64. Compile-time check in the
planner rejects manifests with > 64 declared host controls.

### 4. Smoothing time-constant is per-kind, not per-declaration

Knob and slider smooth at the same fixed time constant; toggle and
trigger pass through unsmoothed. Per-control override (e.g.
`knob foo { smooth: 2ms }`) is deferred — the manifest already carries
arbitrary k/v fields, so overrides can be added without grammar work
when a need arises.

## Consequences

- The `HostControl` module's audio-thread inner loop becomes the
  cleanest in the engine — a single `memcpy` per sample. The engine's
  hottest path (Poly DSP) loses one branch and one match per cable
  read.
- The cable-pool refactor is engine-wide: every `CablePool::read_*` /
  `write_*` call site, every consumer module, the FFI raw-parts API,
  and the MIDI cable-as-sentinel trick in `patches-core/src/midi_io.rs`
  all touch. Each change is mechanical; the test suite catches the
  vast majority of mistakes.
- Sample-accurate automation lands as a side effect of (2), retiring
  ADR 0057 §4's parked item. ADR 0057 §4 should be amended to point at
  this ADR as the implementation.
- Memory budget for the scratch + frame buffers (per audio thread):
  `MAX_BLOCK * 64 * 4` bytes for SoA scratch +
  `MAX_BLOCK * 64 * 4` for AoS frame +
  `64 * 4` for tail state.
  At `MAX_BLOCK = 2048`, scratch + frame = 1 MiB total. Allocated once
  at engine activation; never reallocated. Acceptable.
- Smoothing time constant is hard-coded; if it turns out to need
  per-control override or per-kind tuning, the manifest already
  carries the field map and the registry is already structured to
  forward it to the audio thread via the plan adoption ring.

## Order of execution

1. Untagged cable pool refactor (ticket 0815). Lands first; isolated
   from host control. Engine-wide test sweep validates correctness.
2. Host control scratch buffer + smoothing pipeline (ticket 0816).
   Builds on (1); replaces the placeholder backplane writes from
   ticket 0809. Adds the tail-state and frame buffers, the per-block
   step-fill / smooth / transpose passes, and the per-sample memcpy.
3. Resume E135 at ticket 0811 (CLAP parameter publish + registry
   integration), now writing into the scratch buffer instead of a
   backplane cell.

E136 carries (1) and (2); it must close before E135 resumes.

## Amendment 2026-05-05 — scratch + frame live on the processor, not the module

**Context.** Ticket 0816 placed the SoA scratch, AoS frame, tail, and
`prepare_block` pipeline on the `HostControl` module. While shipping
ticket 0811 (CLAP integration) it became clear this is the wrong
home. `prepare_block` is driven by host events arriving once per
audio buffer, not by the cable graph. Putting it on a module forces
the audio-thread caller to reach into `module_pool` to find a singleton
instance, with no name lookup, and conflates two unrelated
responsibilities (cross-thread event ingest + smoothing pipeline vs.
per-channel port demux).

**Revision.** The split below replaces §2's `HostControl module owns
the pipeline` framing:

- The SoA scratch, AoS frame, per-lane tail, smoothing α, and lane-kind
  table live on `PatchProcessor` alongside `midi_overflow` /
  `transport_poly`. Add:
  - `processor.write_host_control_event(channel, sample_offset, value)`
    — analogue of `write_midi`. Audio-thread caller (CLAP `process`)
    pushes events as it walks the `clap_input_events` queue.
  - `processor.prepare_host_control_block(frames)` — runs
    step-fill / smooth / transpose. Called once per `process()` after
    events are pushed and before the per-sample loop.
  - In `tick()`, before module dispatch, memcpy the AoS row at
    `sample_idx` into `HOST_CONTROL_BASE..HOST_CONTROL_BASE +
    HOST_CONTROL_SLOTS` (four contiguous poly slots, one 256-byte run);
    advance `sample_idx`. Same shape as the existing transport / MIDI
    flush.
- The `HostControl` module shrinks to "read the backplane lane, emit
  on `audio_out[i]` or `trigger_out[i]` per `kind[i]`". No scratch,
  no `prepare_block`, no per-block state — the module's runtime
  contract is exactly `read backplane slot, write output port`.
- Plan-time data shipped in `PlanMeta`:
  - `(ParamId, channel)` map so the audio thread can resolve
    incoming `clap_event_param_value::param_id` to a scratch row.
  - Per-channel `lane_kind` table so the scratch knows where to
    skip smoothing.

**Consequences.** The "find me the right module instance" problem
disappears; CLAP's audio thread interacts with the processor surface
the way it already does for MIDI and transport. Module testing
collapses to a trivial backplane-read assertion. The pipeline tests
(step-fill, smoothing, transpose) move to the engine crate next to
`HostControlScratch`.

**Ticket impact.** Ticket 0816 (host-control scratch on module) is
superseded by ticket 0817 (host-control scratch on processor). 0816
is kept in `closed/` for history; its implementation is removed by
0817's refactor.
