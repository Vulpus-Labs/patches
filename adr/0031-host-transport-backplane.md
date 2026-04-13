# ADR 0031 — Host transport via GLOBAL_TRANSPORT backplane

**Date:** 2026-04-12
**Status:** accepted

---

## Context

Patches runs both standalone (via `patch_player` with CPAL) and as a
CLAP plugin hosted in a DAW. When hosted, the DAW provides transport
state — whether playback is running, the current tempo, beat and bar
positions, and time signature. Modules like `MasterSequencer` need
access to this state to synchronise with the host rather than running
an independent clock.

CLAP communicates transport not via MIDI but through a dedicated
`clap_event_transport` struct on every process call. The existing
`GLOBAL_CLOCK` backplane slot (slot 8) carries only a mono sample
counter and is not read by any module.

## Decision

**Upgrade `GLOBAL_CLOCK` (slot 8) from mono to poly and rename it
`GLOBAL_TRANSPORT`.** The poly slot carries both the existing sample
counter and host transport state in a fixed lane layout:

| Lane   | Name           | Description                                     |
| ------ | -------------- | ----------------------------------------------- |
| 0      | sample_count   | Monotonic sample counter (was GLOBAL_CLOCK)     |
| 1      | playing        | 1.0 while transport is playing, 0.0 stopped     |
| 2      | tempo          | Host tempo in BPM                               |
| 3      | beat           | Fractional beat position                        |
| 4      | bar            | Bar number                                      |
| 5      | beat_trigger   | 1.0 pulse on beat boundary, 0.0 otherwise       |
| 6      | bar_trigger    | 1.0 pulse on bar boundary, 0.0 otherwise        |
| 7      | tsig_num       | Time signature numerator                        |
| 8      | tsig_denom     | Time signature denominator                      |

Lanes 9–15 are reserved for future use.

**The CLAP plugin populates lanes 1–8** by reading
`clap_event_transport` before the sample loop and calling a new
`PatchProcessor::write_transport()` method. Beat and bar triggers
are derived by detecting position crossings between process calls.

**The processor always writes lane 0** (sample counter) inside
`tick()`, regardless of host. In standalone mode lanes 1–8 remain
at 0.0.

**`AudioEnvironment` gains a `hosted: bool` flag.** The CLAP plugin
sets `hosted: true`; standalone sets `false`. This is a static,
pre-resolved signal — no runtime detection needed.

**Modules read the backplane directly.** `MasterSequencer` gains a
`sync` parameter (`auto` | `free` | `host`). In `auto` mode (the
default) it checks `AudioEnvironment::hosted` once in `prepare` to
decide its clock source: host transport if hosted, internal BPM
otherwise. `free` forces the internal clock regardless; `host`
forces host transport regardless. In host-derived modes the
sequencer reads `GLOBAL_TRANSPORT` from the backplane — no wiring
required. In `free` mode it ignores the transport lanes and runs
its own BPM clock as before.

**A separate `HostTransport` module** unpacks the backplane slot
into named mono outputs for patches that want to route transport
signals explicitly — e.g. gating effects on/off with playback,
driving tempo-synced LFOs, or controlling generative sections.
This module is a convenience; sequenced modules do not need it.

## Alternatives considered

### Separate backplane slot for transport

Reserve a new slot (e.g. slot 10) rather than upgrading the
existing clock slot. Rejected because `GLOBAL_CLOCK` is unused by
any module, the sample counter fits naturally as lane 0 of the
transport poly, and using a single slot avoids fragmenting related
temporal state across two locations.

### New ReceivesTransport trait / broadcast mechanism

Similar to `ReceivesMidi`, add a trait for modules that want
transport events. Rejected because transport state is continuous
(valid every sample), not event-driven. A backplane slot fits
the data model — it is always available, zero-cost when unread,
and requires no new dispatch infrastructure.

### MIDI real-time messages (0xFA Start, 0xFC Stop, etc.)

Route transport as MIDI system real-time bytes through the existing
`ReceivesMidi` pipeline. Rejected because CLAP provides transport
via its own struct (not MIDI), the data is richer than MIDI
real-time messages can express (tempo, beat position, time
signature), and conflating host transport with MIDI would
misrepresent the source of the data.

### Explicit wiring from HostTransport to MasterSequencer

Require patches to wire `HostTransport.transport -> seq.transport`.
Rejected because the sequencer's relationship to the host clock is
analogous to its relationship to the sample counter — it is
infrastructure, not a patch-level routing decision. Modules that
need transport for musical purposes (not sequencing) can use
`HostTransport`.

## Consequences

- `GLOBAL_CLOCK` is renamed throughout the codebase. No module
  currently reads it, so the blast radius is limited to the
  processor, cables module, planner, and documentation.
- The backplane slot moves from `CableValue::Mono` to
  `CableValue::Poly`, a slightly larger write per tick. The cost
  is negligible — one 16-float array copy per sample.
- `MasterSequencer` gains dual-mode clock logic. The `free` path
  is unchanged; the `host` path adds a branch per tick to read
  transport lanes.
- Standalone patches are unaffected. Transport lanes default to
  0.0, and the sequencer defaults to `sync: free`.
