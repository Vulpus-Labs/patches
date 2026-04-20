---
id: "E098"
title: ADR 0045 spike 2 — per-type ArcTable with fixed-capacity refcount map
created: 2026-04-19
depends_on: ["ADR 0045 spike 1"]
tickets: ["0580", "0581", "0582", "0583", "0584"]
---

## Goal

Land the audio-safe refcounted handle mechanism that ADR 0045
specifies in section 2 and resolved design point 2. After this
epic:

- Payload-specific id newtypes (`FloatBufferId`, `SongDataId`) are
  defined in `patches-ffi-common` as `#[repr(transparent)]` u64s
  encoding `(generation << 32) | slot`.
- A lock-free, fixed-capacity, open-addressed slot array backs
  audio-thread `retain` / `release` as wait-free single-atomic
  operations reached by direct slot indexing.
- `ArcTable<T>` on the control thread owns the `Arc<T>` values,
  mints ids, services a `SegQueue` of pending releases, and drops
  `Arc`s only on the control-thread drain.
- Each runtime owns one `ArcTable` per payload type; capacity is
  supplied at construction (planner formula wiring comes in a
  later spike). Exhaustion is a clean control-thread error, never
  an audio-thread failure.
- No growth (that is spike 6); no wiring into the audio path yet
  (spikes 3+7 consume this); no `HostEnv` vtable yet (spike 7).

Implements ADR 0045, spike 2. Depends on spike 1 (ticket series
TBD — `ParamLayout` as a pure function).

## Tickets

| ID   | Title                                                             | Priority | Depends on |
| ---- | ----------------------------------------------------------------- | -------- | ---------- |
| 0580 | Payload id newtypes with slot+generation encoding                 | high     | —          |
| 0581 | Lock-free refcount slot array with wait-free retain/release       | high     | 0580       |
| 0582 | ArcTable<T>: mint, pending_release SegQueue, control-thread drain | high     | 0580, 0581 |
| 0583 | Per-runtime typed table set with caller-supplied capacity         | medium   | 0582       |
| 0584 | Multi-threaded soak + Miri + exhaustion coverage                  | high     | 0582, 0583 |

## Definition of done

- `FloatBufferId` and `SongDataId` are `#[repr(transparent)]` u64
  newtypes, constructible only inside `patches-ffi-common`, with
  encode/decode helpers for `(generation, slot)`.
- Refcount slot array: fixed capacity, `{ AtomicU64 id_and_gen,
  AtomicU32 refcount }` per slot, retain/release are each a single
  `fetch_add` / `fetch_sub` with no probing on the audio path.
  Linear probing runs only on the control thread at insertion.
- `ArcTable<T>` holds `Mutex<HashMap<u64, Arc<T>>>` on the control
  thread only; audio-thread access goes through the refcount slot
  array and a `crossbeam_queue::SegQueue` `pending_release`. Drain
  on the control thread decrements `Arc` and drops.
- Per-runtime container exposes `FloatBuffer` and `SongData`
  tables; capacity is a construction parameter; mint past capacity
  returns `ArcTableError::Exhausted` on the control thread.
- Single-threaded unit tests cover mint/retain/release balance and
  exhaustion. Multi-threaded soak (one minter, one simulated-audio
  retainer/releaser) settles to zero refcounts and drops every
  `Arc` on shutdown. Miri passes on the atomic-heavy module.
- `cargo build`, `cargo test`, `cargo clippy` clean; no
  `unwrap`/`expect` in library code; no new `Cargo.toml`
  dependency without prior sign-off (`crossbeam-queue` is the
  likely ask).

## Non-goals

- Growth / atomic pointer swap of the slot array (spike 6).
- Planner-computed capacity formula (wired when the planner grows
  a parameter inventory; spike 2 accepts a caller-supplied value).
- Wiring into `ParamFrame` / audio-thread consumption (spike 3).
- `HostEnv` C vtable and plugin-facing `arc_release` callback
  (spike 7).
- Cross-runtime content deduplication (lives above the runtime).
