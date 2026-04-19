# ADR 0045 — FFI parameter and port data plane

**Date:** 2026-04-19
**Status:** Proposed

---

## Context

ADR 0044 gives the *control plane* for external modules: discovery,
versioning, dylib lifetime. What it does not address is the *data
plane* — how parameter values and port bindings cross the ABI on the
audio thread without violating the project's core invariants:

- **No allocation on the audio thread.**
- **No deallocation on the audio thread.**
- **No locking on the audio thread.**
- **No blocking on the audio thread.**

The current FFI plugin path ([patches-ffi/src/loader.rs:71](../patches-ffi/src/loader.rs#L71)) serialises `ParameterMap`
to JSON via `json::serialize_parameter_map(params)` on every parameter
update and allocates two `Vec`s of FFI port structs on every
`set_ports`. Both calls happen on the audio thread. This is a direct
violation of the no-allocation rule — and the rule exists for good
reason: allocation can block for milliseconds on a contended allocator,
which translates to an audible glitch.

The in-process path is clean: `Module::update_validated_parameters`
takes `&mut ParameterMap` and modules destructively drain it, with the
emptied map shipped to a cleanup thread for drop. The FFI path must
match this standard before external modules become a routine deployment
target.

## Goals

The data plane must:

1. Cross the ABI with zero allocation and zero deallocation on the
   audio thread, on both sides of the boundary.
2. Preserve the ergonomic programming interface modules enjoy today —
   `update_validated_parameters(&mut ParameterMap)`-style access with
   name-based lookup.
3. Route all buffer and song data (file samples, preprocessed spectra,
   pattern data) through shared refcounted handles, never copies.
4. Handle descriptor/layout drift between host and plugin deterministically,
   rejecting mismatched plugins at load rather than corrupting data at
   runtime.
5. Be statically verifiable: ownership and lifetime rules encoded in
   types where Rust permits; runtime asserts where it does not.

## Decision

Six rules, binding on every `Module` implementation and every FFI
plugin:

1. **Every parameter value passed across the audio-thread boundary is
   of a type that requires no allocation to copy.**
2. **Buffer data and song data are moved via `Arc`-based handles
   (ids), shared between modules. Arcs are owned exclusively by a
   host-side `ArcTable`; frames and modules hold only ids.**
3. **Parameter updates are transmitted via fixed-layout binary
   scratch buffers sized from the module's descriptor at `prepare`
   time. Wire format is offset-indexed, not tag-value.**
4. **The module implementer sees a read-only `ParamView` that
   preserves name-based lookup semantics, backed by the packed
   buffer.**
5. **Port bindings are delivered via the same fixed-layout scratch
   mechanism.**
6. **No JSON, no serde, no runtime allocation or deallocation on the
   audio thread.**

The sections below make each rule concrete.

### 1. The parameter value type

`ParameterValue` on the audio-thread path is restricted to:

```rust
pub enum ParameterValue {
    Float(f32),
    Int(i64),
    Bool(bool),
    Enum(u32),              // index into descriptor.variants
    FloatBuffer(FloatBufferId),  // u64 newtype; Arc lives in host ArcTable
}
```

`String` and `File` are removed from the runtime-update path. They
remain in `ParameterKind` (a module may declare them in its
descriptor) but are resolved off-thread by the planner — `File`
becomes `FloatBuffer`, `String` is consumed at build time or lifted
into descriptor metadata. Any attempt to send a `String` or `File`
value through an update frame is a planner bug and is rejected with
an assertion at frame-build time.

Rationale: every remaining variant is `Copy` or a refcounted handle.
No variant requires allocation to clone, so the "destructive take"
discipline used by the in-process path becomes unnecessary — clone
and take cost the same.

### 2. Arc handles and the ArcTable

Large blobs — file samples, preprocessed IRs, spectra, song/pattern
data — live as `Arc<[T]>` (or `Arc<SongData>`) in a host-side
`ArcTable`, keyed by a freshly minted `u64` id.

```rust
pub struct ArcTable<T> {
    entries: Mutex<HashMap<u64, Arc<T>>>,  // control-thread only
    pending_release: SegQueue<u64>,        // audio-thread → control
}
```

The `Mutex` is held only on the control thread. The audio thread
never locks it; it communicates exclusively through the lock-free
`pending_release` queue.

**Lifecycle:**

1. *Mint:* planner produces an `Arc<[f32]>`, host inserts it into the
   `ArcTable` at a fresh id, refcount = 1. The id, together with a
   ptr+len snapshot captured now, is written into the parameter
   frame.
2. *Deliver:* frame crosses the SPSC to the audio thread. Plugin
   reads the id, uses the ptr+len to process samples.
3. *Retain by default on delivery:* at frame dispatch the host
   performs one `arc_retain` per id present in the frame, on behalf
   of the plugin. The plugin therefore observes every id already
   held by an outstanding reference — no action required to keep
   the buffer alive across the call. Rationale: the alternative
   (plugin calls `arc_retain` to keep the buffer) converts a
   forgotten call into a silent use-after-free. Retain-by-default
   converts the same mistake into a leak, which is observable (the
   `ArcTable` grows unboundedly) and caught in testing. Trade a
   bounded cost (one atomic bump per id per frame) for the
   elimination of a catastrophic failure mode.
4. *Release from audio thread:* when the plugin no longer needs an
   id, it calls `env.arc_release(id)`. This pushes the id onto
   `pending_release` and returns. The actual `Arc` decrement (and,
   if it hits zero, the deallocation) happens on the control-thread
   drain. Rationale: `Arc::drop` may call the global allocator,
   which is forbidden on the audio thread. Atomic retain/release
   lookups go through a **parallel lock-free refcount map** (a
   fixed-capacity open-addressed table sized at startup) that sits
   alongside the `ArcTable`; insertions and removals happen only on
   the control thread when ids are minted or finally released.
5. *Frame cleanup:* when a consumed frame returns via the cleanup
   SPSC, the host issues one `arc_release` per id carried in the
   frame's tail slots. This cancels the mint-time refcount. The
   retain bumped at dispatch (point 3) is still held, either still
   by the plugin or released by it when the plugin finished with
   the id.
6. *Replacement:* when the plugin receives a new frame carrying a
   new id for the same logical slot, the plugin stores the new id
   and calls `arc_release` on the old one. Refcount eventually
   reaches zero and the `Arc` drops on the control thread.
7. *Shutdown:* when a plugin is destroyed, the host enumerates any
   ids still attributed to it in the refcount map and releases them
   on the plugin's behalf. A non-empty set at this point is a plugin
   leak and is logged.

**Sharing:** multiple modules can hold the same `FloatBufferId`.
Each holder retains independently. Content deduplication (same file
loaded twice → one `Arc`) is handled at mint time via a content or
path cache, outside the hot path.

**Invariant:** at any instant, the control thread knows the set of
all live ids (the `ArcTable` keys). The audio thread knows only ids
it has been handed; it never invents them and never observes the
refcount.

### 3. Fixed-layout scratch buffers

At `prepare` time, both host and plugin independently compute a
`ParamLayout` from the module's descriptor:

```rust
pub struct ParamLayout {
    pub scalar_size: u32,
    pub scalars: Vec<ScalarSlot>,     // ordered; static for instance lifetime
    pub buffer_slots: Vec<BufferSlot>,
    pub descriptor_hash: u64,         // SHA of descriptor; checked at load
}

pub struct ScalarSlot {
    pub key: ParameterKey,
    pub offset: u32,
    pub tag: ScalarTag,  // Float | Int | Bool | Enum
}

pub struct BufferSlot {
    pub key: ParameterKey,
    pub slot_index: u16,  // offset in tail slot array
}
```

Layout is deterministic: sort by `(name, index)`, assign offsets
greedily with natural alignment. Both sides compute the same layout
from the same descriptor; host verifies plugin agreement by
comparing descriptor hashes at load. Mismatch = refuse to load.

The buffer wire format is:

```
[ scalar area: fixed-size struct at scalar_size bytes ]
[ buffer slot table: buffer_slots.len() × u64 id ]
```

No index tags, no keys on the wire, no length prefixes for scalars.
Reading a `Float` at offset `o` is `*(buffer.add(o) as *const f32)`.
Reading a buffer slot is `buffer_tail[slot_index] as FloatBufferId`.

Scratch buffers (`Vec<u8>`) are owned by frames. Each frame travels
through three SPSCs:

```
 control thread              audio thread                cleanup thread
 ──────────────              ────────────                ──────────────
 pop free Vec<u8>
  (pre-sized)  ──frame SPSC──▶ vtable call into plugin
 pack layout                   (plugin reads view)
 (alloc-free)                  push consumed ──cleanup SPSC──▶
                                                              release Arcs
                                                              zero len, keep cap
                               ◀───── free SPSC ────────── return Vec<u8>
```

Steady state: no allocation anywhere. A burst of new frames beyond
the free-list capacity can allocate on the control thread — never on
the audio thread. The free-list capacity is sized from the expected
update rate plus headroom; undersizing is a performance bug, not a
correctness bug.

**Back-pressure:** param updates coalesce per `(module_idx,
ParameterKey)`. A slot table in front of the frame SPSC holds at
most one pending frame per key; a newer update overwrites the
pending frame (parameter updates are last-wins). This keeps queue
depth bounded regardless of controller rate.

### 4. Read-only `ParamView` with name-based lookup

Modules see `ParamView<'a>`, a zero-sized wrapper over
`(&'a ParamLayout, &'a [u8])` providing:

```rust
impl<'a> ParamView<'a> {
    pub fn float(&self, key: impl Into<ParameterKey>) -> f32 { ... }
    pub fn int(&self, key: impl Into<ParameterKey>) -> i64 { ... }
    pub fn bool(&self, key: impl Into<ParameterKey>) -> bool { ... }
    pub fn enum_variant(&self, key: impl Into<ParameterKey>) -> u32 { ... }
    pub fn buffer(&self, key: impl Into<ParameterKey>) -> Option<FloatBufferId> { ... }
}
```

Name-based lookup resolves via a perfect-hash table built at
`prepare` from the descriptor; O(1) with no hashing collisions, no
allocation, and no fallback path. Typed enum support comes from a
`#[derive(ParamEnum)]` macro that generates a Rust enum whose
discriminants match the descriptor variant order.

The `Module` trait signature becomes:

```rust
fn update_validated_parameters(&mut self, params: &ParamView<'_>);
```

(`&mut ParameterMap` is dropped from the signature — without `String`
there is nothing to destructively take, and the view is now
read-only.) The migration is mechanical: modules that used
`params.take_scalar("name")` become `params.float("name")`, etc.

### 5. Port bindings

Port geometry is also fixed at `prepare` time from the descriptor's
port counts. Ports travel as:

```rust
#[repr(C)]
pub struct PortFrame {
    pub idx: u32,
    pub input_count: u32,
    pub output_count: u32,
    // followed by: [FfiInputPort; input_count], [FfiOutputPort; output_count]
}
```

The frame is a single pre-allocated `Vec<u8>` sized to the
descriptor's port count at `prepare`. It flows through the same SPSC
triplet as parameter frames (or its own triplet, depending on
coalescing policy). The plugin sees a borrowed view:

```rust
fn set_ports(&mut self, ports: &PortView<'_>);
```

No allocation on either side. Port updates are rare (hot-reload,
voice allocation) but the rule is uniform.

### 6. No JSON, no serde, no allocation on the audio thread

The `patches-ffi` loader stops using `json::serialize_parameter_map`
on the audio path. JSON is retained only for:

- Descriptor and manifest exchange at load time (control thread).
- Human-readable error reporting (control thread, error path only).

The audio-thread ABI surface consists of three functions plus the
host environment vtable:

```c
void update_validated_parameters(
    Handle plugin,
    const uint8_t* bytes, size_t len,
    const HostEnv* env
);

void set_ports(
    Handle plugin,
    const uint8_t* bytes, size_t len,
    const HostEnv* env
);

void process(
    Handle plugin,
    void* cables, size_t cable_count, uint32_t write_index
);
```

`HostEnv`:

```c
typedef struct HostEnv {
    void  (*float_buffer_release)(uint64_t id);
    void  (*song_data_release)(uint64_t id);
    // one release callback per payload type; future: logging, tap emission, etc.
} HostEnv;
```

Every release callback is audio-safe: a single atomic `fetch_sub` on
the payload type's fixed-capacity refcount map, plus a lock-free
queue push if the count reached zero. No locking, no allocation, no
blocking. `arc_retain` is not exposed to the plugin — buffers arrive
already retained on its behalf (section 2, lifecycle point 3), so
retention from plugin code is neither possible nor necessary.

## ABI contract

The plugin's obligations:

1. Pointers received in `update_validated_parameters` and `set_ports`
   are valid only for the duration of the call. Scalars and port
   structs must be copied if kept. Buffer ids are delivered
   already-retained on the plugin's behalf; no `arc_retain` is
   required to keep a buffer alive past the call.
2. Descriptor hash mismatch between host and plugin at load is a
   fatal error; the plugin must refuse to initialise.
3. The plugin must not allocate, lock, deallocate, or block inside
   `process`, `update_validated_parameters`, or `set_ports`. A debug
   build may install an allocator shim that traps to verify this.
4. The plugin must call `arc_release` exactly once per id it no
   longer needs. A missed release is a leak (observable); a double
   release is undefined behaviour (debug builds trap via the
   refcount audit).

The host's obligations:

1. Never invoke plugin audio-thread entry points while holding a
   lock the plugin's `env` callbacks might contend on.
2. Guarantee that every id present in a frame's buffer slots has a
   live `Arc` in the `ArcTable` at the moment the frame is
   dispatched.
3. Issue exactly one `arc_release` per id per frame dispatch during
   cleanup, cancelling the mint-time refcount.
4. Never deallocate an `Arc<[f32]>` on the audio thread, even
   indirectly through a refcount decrement.

## Safety verification

The rules above are enforced through a layered strategy:

**Compile-time (Rust where possible):**

- `FloatBufferId` is a `#[repr(transparent)]` newtype over `u64`, not
  constructible outside the `patches-ffi` crate. Plugins get ids
  through the ABI only.
- `ParamView<'a>` borrows the layout and bytes with a single
  lifetime; modules cannot retain it past the call.
- `ParameterValue` loses its `String` and `File` variants on the
  audio-thread path via a type split (`ParameterValueStatic`); the
  full `ParameterValue` remains available off-thread.

**Load-time (runtime asserts, control thread):**

- Descriptor hash comparison between host-computed and
  plugin-reported `ParamLayout`.
- Manifest ABI version check (already present; ADR 0044 retains it).
- Plugin smoke-test: host calls `update_validated_parameters` with a
  default-filled frame and confirms no error return.

**Runtime asserts (debug builds only):**

- An audio-thread allocator shim (`#[global_allocator]`
  `TrappingAllocator`) panics on any allocation during `process`,
  `update_validated_parameters`, or `set_ports`. Gated behind a
  `audio-thread-allocator-trap` feature. Both in-process modules
  and FFI modules are checked.
- A per-id refcount audit in the `ArcTable` drain: every id release
  must correspond to a prior retain or mint. Double-release trips
  the audit.
- Frame SPSC length assertions: a frame's scalar area size must
  equal the module's `ParamLayout::scalar_size`.

**Property tests (CI):**

- `ParamLayout` computation is deterministic and reproducible: the
  same descriptor always produces the same layout and hash.
- `pack_into(layout, &ParameterMap, &mut scratch)` round-trips to a
  `ParamView` that returns equal values for every key the map
  contains.
- FFI round-trip: encode on the host, decode in a mock plugin via
  the real ABI, assert values match.

**Fuzz tests (CI, longer):**

- Malformed frames (wrong size, wrong layout hash) are rejected at
  load or frame-dispatch time, never produce a decoded `ParamView`.
- Arc retain/release sequences under randomised controller rates
  maintain refcount invariants (no leaks, no premature drops).

**Integration test (hardware-free):**

- `patches-integration-tests` gets a dedicated suite that loads a
  test plugin, runs 10 000 `process` cycles with randomised
  parameter updates, and asserts: (a) no allocation occurred on the
  audio thread, (b) all `Arc`s reach zero refcount on shutdown, (c)
  the plugin's output matches a reference in-process
  implementation.

## Consequences

### Positive

- External modules become a first-class deployment target, satisfying
  the same real-time invariants as in-process modules.
- The ABI surface shrinks dramatically: three audio-thread functions
  plus two host-env callbacks, no JSON, no serde, no descriptive
  strings on the wire.
- Module implementations are simpler: read-only `ParamView` is easier
  to reason about than `&mut ParameterMap` with destructive semantics.
- `String` and `File` leave the runtime-update path, eliminating a
  class of allocation hazards that had been held at bay only by
  convention.

### Negative

- Migration cost: every existing module must switch from
  `ParameterMap` access to `ParamView`. Mechanical but wide —
  ~25 modules. A thin compatibility shim can stage the migration.
- Loss of `&'static str` enum variant matching: modules match on
  `u32` indices (wrapped in typed enums via macro). Ergonomically
  close but not identical.
- `ParamLayout` and the ArcTable add code; the runtime surface is
  larger, even as the ABI surface shrinks.
- A bug in `ParamLayout` hashing or determinism can silently
  mismatch. Mitigated by property tests but not eliminated.

### Alternatives considered and rejected

- **Keep JSON on the ABI.** Simple but violates the allocation rule
  and is the reason for this ADR.
- **Flat kv-stream wire format.** Simpler to encode but requires
  decoding passes and tagged dispatch on the audio thread. Given
  that layout is fully static per instance, offset-indexed is
  strictly better.
- **Plugin-owned scratch buffer.** Briefly sketched; rejected
  because the buffer is written by the control thread and read by
  the audio thread, which is precisely the sharing problem the
  frame-as-message pattern avoids.
- **`String` support via `Arc<str>`.** Would work but adds a variant
  to the audio-thread type for a use case that has no real
  consumers. Cleaner to exclude.

## Suggested spike sequence

The design has many interlocking parts. Building them in the wrong
order risks landing an unstable intermediate where audio breaks
subtly and the regression is hard to localise. The following
sequence keeps each stage verifiable in isolation and leaves the
tree green at every step. Each spike ends with its own tests and
merges to main before the next begins.

### Spike 0 — Retire `ParameterValue::String` for enum selections

Scope: modules that match `ParameterValue::Enum(&'static str)`
against string variant names migrate to a `u32` variant index,
consumed through a typed Rust enum generated by a `params_enum!`
macro so module code stays readable. Audit (2026-04-19) found no
shipping module declares `ParameterKind::String`, so the variant
and its supporting infrastructure (interpreter arm, FFI JSON
codec, LSP completions, descriptor builder helper) are removed
entirely in the same spike rather than deferred. DSL source-text
strings resolve only against `ParameterKind::Enum` after this
spike.

This spike is standalone: it does not depend on anything else in
this ADR and delivers value on its own by removing unnecessary
string handling from module update paths, shrinking the surface
of every subsequent spike.

Tests:

- Every module that previously matched on string variants produces
  identical behaviour on a curated parameter-sweep fixture.
- DSL parser round-trips enum values correctly (source name →
  variant index → canonical name).
- LSP completions for enum parameters still surface variant names.

Deliverable: modules no longer consume string variant names on any
update path. `ParameterKind::String` and `ParameterValue::String`
are gone from the codebase. `Module::update_validated_parameters`
takes `&ParameterMap` rather than `&mut ParameterMap` — with
`String` gone, no variant on the update path requires destructive
take, so the signature becomes read-only two spikes ahead of the
`ParamView` migration (Spike 5).

### Spike 1 — `ParamLayout` as a pure function

Scope: compute `ParamLayout` deterministically from a
`ModuleDescriptor`. Produce `scalar_size`, slot table, buffer slot
table, and a stable `descriptor_hash`. No runtime behaviour changes.

Tests:

- Determinism: same descriptor ⇒ same layout, same hash, across
  runs and across machines (hash uses a stable serialiser).
- Alignment: every scalar offset respects its type's natural
  alignment; `scalar_size` is the minimum required.
- Coverage: every parameter in the descriptor appears exactly once
  in the layout.

Deliverable: a `patches-ffi-common` module, no dependents yet.

### Spike 2 — Per-type `ArcTable` with fixed-capacity refcount map

Scope: implement `ArcTable<T>` (control-thread owned) plus the
lock-free refcount map with slot-encoded ids. No growth yet —
fixed capacity, exhaustion returns a control-thread error.
Implement `retain` / `release` on the refcount map; release pushes
to `pending_release`; control-thread drain decrements `Arc` and
drops.

Tests:

- Single-threaded unit tests for mint / retain / release balance.
- Multi-threaded soak: one control thread minting, one thread
  simulating audio (retain/release at load), verify refcounts
  settle to zero and `Arc`s all drop on shutdown.
- Exhaustion: mint until capacity, verify clean failure.
- Miri pass (no UB in the atomic-heavy code).

Deliverable: a `patches-ffi-common::arc_table` module usable by
later spikes. No wiring into audio path yet.

### Spike 3 — `ParamFrame`, SPSC triplet, pack + view (in-process only)

Scope: define `ParamFrame` (owned `Vec<u8>` with scalar area + tail
slot table), the three SPSCs (dispatch / cleanup / free-list), and
the `pack_into(layout, &ParameterMap, &mut scratch)` encoder on the
control thread. Define `ParamView<'a>` with name-based lookup via a
perfect hash built at `prepare`. Wire this alongside the existing
`ParameterMap` path as a **shadow path**: for every in-process
parameter update, the engine encodes a frame, decodes through
`ParamView`, compares field-by-field against the live
`ParameterMap`, and asserts equality in debug builds.

Tests:

- Round-trip property tests: random `ParameterMap` → pack → view →
  equals original for every key.
- Shadow assertion is quiet on the full existing test suite.
- Free-list recycling: 10 000 iterations allocate nothing after
  warm-up.

Deliverable: the data-plane mechanism exists and is verified to
match existing semantics, but production reads still go through
`ParameterMap`. No audio-thread behaviour change yet.

### Spike 4 — Audio-thread allocator trap

Scope: add a `#[global_allocator]` shim behind a
`audio-thread-allocator-trap` feature that panics on any
allocation on threads tagged as audio-thread. Tag the engine's
audio thread at startup. Gate the trap on this feature in debug
CI runs.

Tests:

- With trap enabled, existing test suite still passes (no false
  positives).
- Deliberate allocation in a test module traps as expected.

Deliverable: the enforcement mechanism for the no-alloc rule. Used
by every subsequent spike.

### Spike 5 — Migrate in-process modules to `ParamView`

Scope: change the `Module` trait signature to
`fn update_validated_parameters(&mut self, params: &ParamView<'_>)`.
Update every in-process module. Retire the shadow comparison from
Spike 3. Remove `ParameterValue::String` and
`ParameterValue::File` from the runtime-update path (the variants
may persist off-thread but cannot reach the audio thread; assert
at frame-build time). Introduce the `#[derive(ParamEnum)]` /
`params_enum!` macro for typed enum access.

Tests:

- Full test suite passes with allocator trap on.
- Every module's behaviour is unchanged in integration tests.
- A regression test confirms any attempt to encode a `String` or
  `File` into a frame panics at frame-build time in debug, errors
  in release.

Deliverable: the in-process path is now wholly on the new data
plane. FFI still uses JSON at this point; that's the next spike.

### Spike 6 — Growth via atomic pointer swap

Scope: implement table growth. Control thread allocates a
strictly-larger slot array, copies existing slots, atomic pointer
swap. Audio thread reloads the pointer each call. Old arrays
retire via a quiescent-pass barrier; dropped on the control
thread.

**Quiescence mechanism (RCU-style, specialised to one audio
thread).** Two atomic counters per runtime:

```rust
struct Quiescence {
    started:   AtomicU64,  // fetch_add at callback entry
    completed: AtomicU64,  // fetch_add at callback exit
}
```

Audio-thread callback wrapper:

```rust
let _g = quiescence.started.fetch_add(1, AcqRel);
let tbl = refcount_table.load(Acquire);
// ... retain/release against tbl ...
quiescence.completed.fetch_add(1, Release);
```

Control-thread retirement:

1. Store new pointer with `Release`.
2. Sample `n = started.load(Acquire)` immediately after the swap.
3. Queue the old table for drop, tagged with `n`.
4. In the drain loop, drop the old table once
   `completed.load(Acquire) >= n`.

Why this is tight:

- Any callback that could hold the old pointer incremented
  `started` *before* step 2 — so its generation is strictly less
  than `n`.
- When `completed >= n`, every generation less than `n` has
  finished (on a single audio thread, completion is monotonic and
  gap-free).
- New callbacks that started after the swap read the new pointer,
  so they never touched the old table.
- `Acquire` on `completed` pairs with the audio thread's `Release`
  on the end-of-callback increment, giving happens-before on
  anything the callback did against the old table.

Idle audio thread: `completed` stops advancing and the retired
table sits in the queue. Memory cost is one obsolete table per
growth event — harmless in practice. On runtime teardown the
ArcTable drops whole and takes any queued retired tables with it.

Cost on the hot path: two uncontended atomic increments per
callback (`started` at entry, `completed` at exit). Imperceptible.

Alternative considered: full epoch-based reclamation
(crossbeam_epoch). Rejected — EBR generalises to many readers and
writers; we have one audio thread and one control-thread retirer,
so the two-counter specialisation is simpler and its invariants
are directly inspectable.

Tests:

- Stress test: synthetic accretion pattern mirroring live-coding
  (start with 4 ids of capacity, grow to 4096 over many steps),
  audio-thread retain/release running continuously, verify no
  corruption and no audio-thread allocation.
- Id validity across growth: ids minted before growth continue to
  resolve correctly after.
- Retire correctness: old array is dropped exactly once, after all
  in-flight audio-thread calls that saw it have returned. Assert
  via a drop counter on the slot array wrapper.
- Idle-retirement: retire a table, stop the audio thread, verify
  the queued table drops on runtime teardown.

Deliverable: tables now adapt to in-session graph growth.
`patches-player` live-coding works end-to-end on the new path.

### Spike 7 — FFI ABI redesign and first external plugin

Scope: define the new C ABI (`update_validated_parameters`,
`set_ports`, `process`, `HostEnv` with per-type `release`
callbacks). Implement descriptor-hash check at load. Rewrite one
existing test plugin (`gain`) against the new ABI. Remove the JSON
path from the FFI audio-thread hot loop. Retain JSON only for
manifest/descriptor exchange at load and for error reporting.

Tests:

- FFI round-trip: encode on host, decode in plugin via the real
  ABI, assert values match.
- Descriptor-hash mismatch refuses to load.
- Allocator trap stays clean across plugin calls (plugin runs
  inside the trap via a debug build).
- Double-release in the plugin trips the refcount audit in debug.

Deliverable: external modules run on the new data plane with zero
allocation/deallocation on the audio thread.

### Spike 8 — Migrate `patches-vintage` to a bundle

Scope: the forcing-function exercise from ADR 0044. Build
`patches-vintage` as an FFI bundle, load it through the new ABI
into `patches-player`, `patches-clap`, and the LSP-backed editor.
Confirm parity with the in-process reference on audio output and
parameter behaviour.

Tests:

- Bit-identical audio output vs the in-process version for a fixed
  input (pre-migration baseline captured in Spike 5).
- Full hot-reload cycle through the FFI path, including parameter
  changes and port rebinds.

Deliverable: the first real external bundle in production use.

### Spike 9 — Fuzzing, property tests, observability

Scope: malformed frame fuzzing (wrong size, wrong layout hash,
corrupted tail — all rejected, never decoded). Randomised
retain/release sequence fuzzing against the `ArcTable`.
Long-running integration test (10 000+ cycles with randomised
parameter updates) asserting no allocation and clean Arc cleanup
on shutdown.

Add observability: per-runtime counters for table capacity, high
watermark, growth events, frame-dispatch rate, pending-release
queue depth. Exposed via the tap/observation infrastructure (ADR
0043).

Deliverable: the data plane is verified against adversarial inputs
and observable in production.

### Ordering constraints

- Spike 0 is standalone (string→enum migration).
- Spike 1 has no prerequisites.
- Spike 2 depends on 1 (needs layout to know id types per payload).
- Spike 3 depends on 1 and 2.
- Spike 4 is independent; land as early as convenient.
- Spike 5 depends on 0, 3, and 4.
- Spike 6 depends on 2 and 5; required before Spike 8 ships on
  `patches-player`.
- Spike 7 depends on 5 (the trait and type surface must have
  stabilised).
- Spike 8 depends on 6 and 7.
- Spike 9 runs in parallel from Spike 6 onwards.

Each spike merges to main independently with tests green and the
existing user-visible behaviour preserved. No spike leaves the
runtime in a partly-migrated state that only works for some module
kinds — the hard switches (Spike 5, Spike 7) move the whole
population at once.

## Relationship to other ADRs

- **ADR 0025** (original FFI plugin format): this ADR supersedes its
  audio-thread data-plane section. Control-plane (discovery,
  manifest, ABI versioning) is unchanged.
- **ADR 0039** (multi-module bundles): orthogonal. Bundles compose
  along the control plane; this ADR is about the data plane.
- **ADR 0044** (dynamic module loading/reload): complements. Dylib
  lifetime and rescan live there; frame/ArcTable mechanics live
  here.

## Resolved design points

1. **Per-type ArcTables, per runtime.** Each payload type
   (`Arc<[f32]>`, `Arc<SongData>`, `Arc<PatternBlock>`, …) gets its
   own `ArcTable` and its own id space. Buffer ids and song ids are
   not interchangeable, and the type system reflects this via
   distinct newtypes (`FloatBufferId`, `SongDataId`, …). Each table
   has its own refcount map and its own capacity budget.

   The full set of tables is owned by the runtime (the object that
   owns the `ExecutionPlan`), not the process. Each runtime has its
   own id space, its own capacity envelope, and its own `HostEnv`
   instances bound to its tables. Rationale: rule 2 sharing is
   intra-patch only (two modules in the same graph holding one IR);
   cross-patch sharing has no use case and would couple independent
   runtimes through a shared failure mode. Per-runtime tables drop
   all held `Arc`s in one shot when the runtime is torn down, align
   naturally with testing, and prevent one patch's leak from
   exhausting another's capacity. Cross-patch caches (e.g. sample
   content deduplication by file hash) live above the runtime in
   the loader/planner, not inside the audio-plane ArcTable.
2. **Open-addressed refcount map with slot index encoded in the id,
   sized by the planner per runtime.** Id format is
   `(generation << 32) | slot`. Each slot stores
   `{ AtomicU64 id_and_gen, AtomicU32 refcount }`. Audio-thread
   retain/release are single `fetch_add` / `fetch_sub` operations on
   a slot reached by direct indexing — no probing, no locks,
   wait-free. Linear probing is confined to the control thread at
   insertion time.

   Capacity is not a global constant. At plan build, the planner
   computes an upper bound on in-flight ids per payload type from
   the graph:

   ```text
   capacity(type) =
       Σ (descriptor.params_of(type).count × module.poly_channels)
       × frame_depth
       + headroom
   ```

   Each runtime's tables are sized accordingly. A small patch gets
   small tables; a 128-voice sampler gets a large one. Exhaustion
   is a control-thread error (frame refused) and observable in
   tests.

   On hot-reload, the planner recomputes the bound. If the new
   graph fits the existing capacity, tables are reused. If it
   exceeds, tables grow via atomic pointer swap: control thread
   allocates a strictly-larger slot array, copies existing slots at
   their original indices (ids remain valid because they encode
   slot index and generation), atomically stores the new pointer.
   Audio thread loads the pointer on entry to each retain/release
   and operates on whichever array it sees; old arrays retire after
   one quiescent audio-thread pass and are dropped on the control
   thread.

   Growth is on the critical path, not a future optimisation. The
   primary workflow in both hosts is live-coding: `patches-player`
   holds a single long-lived runtime while the user adds modules
   incrementally, and the CLAP plugin — though it could in principle
   discard and rebuild its runtime on patch-file change — is
   subject to the same in-session accretion pattern when the user
   live-codes inside a DAW session. Initial table capacity is
   therefore sized small (the starting graph may be near-empty),
   and the growth mechanism ships in the first milestone.

   `DashMap` was rejected: its sharded `RwLock`s are not strictly
   audio-safe under writer/reader contention. Third-party lock-free
   hashmaps were rejected because their memory reclamation schemes
   (e.g. epoch-based) can defer drops onto the audio thread.

   `DashMap` was rejected: its sharded `RwLock`s are not strictly
   audio-safe under writer/reader contention, and "usually
   audio-safe" does not meet the invariant. Third-party lock-free
   hashmaps were rejected for the same bar-of-quality reason and
   because memory reclamation schemes like epoch-based reclamation
   can defer drops onto the audio thread.
3. **`HostEnv` is per plugin instance.** One vtable pointer per
   instance, set at load. `HostEnv` is stateless from the plugin's
   view; a single instance suffices.
4. **`arc_retain` is not on the plugin-facing vtable.** Under
   retain-by-default delivery (section 2, lifecycle point 3) the
   plugin never needs to retain — ids arrive already held on its
   behalf. The plugin-facing `HostEnv` exposes only `arc_release`
   (one per payload type). This closes an entire class of ABI
   misuse at the source.
