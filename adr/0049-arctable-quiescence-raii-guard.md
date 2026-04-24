# ADR 0049 — ArcTable quiescence as an RAII guard

**Date:** 2026-04-23
**Status:** accepted

---

## Context

ADR 0045 §2 (spike 6) specifies the ArcTable refcount machinery: a grow-only
chunked slot store shared between the control thread and the audio thread,
with an RCU-style quiescence barrier so that the control thread can retire
old `ChunkIndex` allocations after concurrent audio ops have drained.

The audio-side retain/release path is the hot, unsafe core of this design.
Each call must:

1. Open a quiescence bracket: `started.fetch_add(1, AcqRel)`.
2. Acquire-load the current `ChunkIndex` pointer.
3. Decode the slot index from `id_and_gen` and dereference into the chunk.
4. Debug-assert the slot's `id_and_gen` matches (stale/forged-id guard).
5. `fetch_add` / `fetch_sub` the slot's refcount.
6. Close the bracket: `completed.fetch_add(1, Release)`.

The current implementation ([`patches-ffi-common/src/arc_table/refcount.rs`])
inlines all six steps in each of `retain` and `release`, and in both it is
the programmer's responsibility — not the compiler's — to ensure the bracket
closes on every exit path. The `release` path has a panic on refcount
underflow (debug-only double-release detection), and that panic must be
preceded by an *explicit* `completed.fetch_add` or else a panicking op would
leave `started > completed` forever, stalling every subsequent
`drain_retired` call and leaking every retired index until the table drops.

The cost of this structure is:

- **The pairing is by convention, not by type.** A future refactor that
  adds an early-return (e.g. treating a generation mismatch as a benign
  no-op instead of a debug panic) can silently skip `completed++`.
- **The panic-recovery dance is hand-rolled.** Release has to restore the
  refcount *and* bump `completed` before `panic!`, duplicating what a
  `Drop` impl would do anyway.
- **Orderings are scattered across six call sites.** Weakening any one of
  `started`/`completed`/`index` to `Relaxed` is a local edit; catching the
  resulting bug requires weak-memory hardware and a concurrent soak run.
- **Slot decoding and the stale-id debug check are duplicated** across
  retain, release, `install`, `clear`, `id_of`, `refcount_of` — six copies
  of `id_and_gen as u32` plus five copies of the `debug_assert_eq!` on the
  slot's stored id.

A review of the code asked whether these invariants could be made
structural rather than conventional. They can.

## Decision

Wrap the quiescence bracket and the index snapshot in an RAII guard. The
guard's constructor performs `started.fetch_add`, loads the index, and
exposes a slot accessor; its `Drop` impl performs `completed.fetch_add`.
Retain, release, and all control-side readers go through the guard.

```rust
struct QuiescenceGuard<'s> {
    shared: &'s SlotsShared,
    index: &'s ChunkIndex,
}

impl<'s> QuiescenceGuard<'s> {
    #[inline]
    fn enter(shared: &'s SlotsShared) -> Self {
        shared.quiescence.started.fetch_add(1, AcqRel);
        let ptr = shared.index.load(Acquire);
        // SAFETY: started++ pins the index against retirement until Drop.
        Self { shared, index: unsafe { &*ptr } }
    }

    #[inline]
    fn slot(&self, id_and_gen: u64) -> &Slot {
        let s = unsafe { &*self.index.slot_ptr(id_and_gen as u32) };
        debug_assert_eq!(
            s.id_and_gen.load(Acquire),
            id_and_gen,
            "stale/forged id {id_and_gen:#x}"
        );
        s
    }
}

impl Drop for QuiescenceGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.shared.quiescence.completed.fetch_add(1, Release);
    }
}
```

Retain and release collapse to their substance:

```rust
pub fn retain(&self, id_and_gen: u64) {
    let g = QuiescenceGuard::enter(&self.shared);
    g.slot(id_and_gen).refcount.fetch_add(1, Relaxed);
}

pub fn release(&self, id_and_gen: u64) -> bool {
    let g = QuiescenceGuard::enter(&self.shared);
    let s = g.slot(id_and_gen);
    let prev = s.refcount.fetch_sub(1, AcqRel);
    debug_assert!(prev > 0, "refcount release underflow for id {id_and_gen:#x}");
    prev == 1
}
```

Control-side readers (`install`, `clear`, `id_of`, `refcount_of`) do not
need the quiescence bracket — they run on the single control thread, which
is the sole writer of `index`. They get a sibling `ControlAccess` type that
shares the same `slot(id_and_gen)` helper (dedup) but no `Drop` quiescence.

## What each invariant becomes

| Invariant | Before | After |
|-----------|--------|-------|
| `started` paired with `completed` on every exit path | Conventional; manual `completed++` before `panic!` | Structural: `Drop` fires on every path including panic |
| Index pointer valid for the duration of the op | Comment-only lifetime; raw `*const ChunkIndex` in a local | `&'s ChunkIndex` tied to guard scope; can't be stashed or returned |
| `Acquire` load of index + `AcqRel` of `started` + `Release` of `completed` | Scattered across every call site | Co-located in `enter`/`Drop` |
| Stale-id debug check | Duplicated in every accessor | One site: `guard.slot(id)` |
| Slot decoding (`id_and_gen as u32`) | Duplicated in every accessor | One site |

The panic-recovery dance in `release` disappears entirely. Underflow used
to require: `fetch_add` to restore the refcount, `fetch_add` to close
quiescence, then `panic!`. The restore existed only so that `Drop` wouldn't
see an underflowed slot during table teardown. With RAII:

- `Drop` closes quiescence regardless of how the function exits.
- The refcount restore becomes optional (belt-and-braces for debug builds)
  since no later reader relies on the invariant before the panic unwinds
  the process.

## Consequences

### Positive

- **The worst class of silent bug is eliminated.** Forgetting to close
  quiescence stalls retirement forever and leaks every old index; the
  current code has three sites where this could be introduced by an
  innocent-looking refactor. After this change there are zero.
- **Lifetimes enforce pointer discipline.** The index reference cannot
  outlive the guard, so a future helper that takes `&ChunkIndex` and
  returns something derived from it cannot accidentally create a dangling
  pointer past the quiescence window — the borrow checker refuses.
- **Ordering audits become local.** The set of atomics that must be
  Acquire/Release/AcqRel for quiescence correctness lives in one ~20-line
  type. The adversarial "silently weaken to Relaxed" change identified in
  review becomes a single-file review instead of four call sites that all
  have to be read together to see the pattern.
- **De-duplication.** Five copies of the stale-id debug check and six of
  the slot decode collapse to one each.
- **Reads better.** `retain` and `release` become two lines of actual
  refcount logic, which is what the reviewer wants to see.

### Negative

- **One extra type and one extra Drop impl.** The guard is zero-sized at
  runtime beyond the two references it holds, and both `enter` and `drop`
  are `#[inline]`, so codegen should match the hand-inlined version. This
  needs to be confirmed by inspecting the release-mode disassembly for
  `retain` before and after; if the compiler fails to inline `Drop`, the
  extra call in the hot path is unacceptable and we revert.
- **Slight loss of flexibility.** A hypothetical op that wanted to
  conditionally skip `completed++` (there is no such op today and the
  design does not admit one) would have to `mem::forget` the guard. This
  is a feature, not a bug — it forces the rare case to be explicit.
- **Drop order matters for the refcount-restore-on-underflow debug path.**
  The restore `fetch_add` must happen before the guard drops, which is the
  natural ordering (it runs in the function body before the guard goes
  out of scope). A future refactor that moved the restore into a helper
  with its own scope would need to be careful. Worth a comment at the
  restore site.

### Neutral

- **No change to the ADR 0045 design or contract.** This is a refactor of
  the same invariants, not a revision of them. The audio thread is still
  wait-free; the quiescence protocol is unchanged; the retire queue works
  identically. Soak tests and fuzz tests need no modification.
- **Control-side accessors lose nothing.** `install`/`clear`/`id_of`/
  `refcount_of` currently perform the same `index.load(Acquire)` +
  `slot_ptr` dance; the `ControlAccess` sibling type preserves that
  without the quiescence overhead it never needed.

## Validation

1. **No behavioural change under existing tests.** All refcount unit
   tests, the multi-threaded soak, and the proptest fuzz suite must pass
   unchanged.
2. **Codegen parity.** Compare release-mode assembly of `retain`/`release`
   before and after on x86_64 and aarch64. Acceptance criterion:
   instruction count and atomic-op ordering match the hand-inlined version
   to within noise. If not, investigate `#[inline(always)]` on
   `QuiescenceGuard::enter` and `Drop`.
3. **Panic-path check.** Add a debug-build-only test that triggers the
   release underflow panic inside a `catch_unwind` and asserts `started
   == completed` afterwards. This codifies the invariant the RAII guard
   is there to preserve.

[`patches-ffi-common/src/arc_table/refcount.rs`]: ../patches-ffi-common/src/arc_table/refcount.rs
