//! Grow-only chunked lock-free refcount slot storage.
//!
//! ADR 0045 §2 + spike 6. Each slot holds `{ AtomicU64 id_and_gen,
//! AtomicU32 refcount }`. Slot index is encoded in the id
//! (`id as u32`), so audio-thread retain/release decode the slot
//! directly — no probing, no locks, wait-free.
//!
//! Storage layout is a vector of pinned 64-slot chunks. A
//! `ChunkIndex` is a small fixed-size array of chunk pointers
//! behind an `AtomicPtr`; growth appends new chunks and publishes a
//! fresh index via Release swap. Existing chunks never move and are
//! referenced by both old and new indices. Only the index itself is
//! retired (RCU-style) via a two-counter quiescence barrier (ADR
//! 0045 spike 6 § "Quiescence mechanism").
//!
//! Control thread owns the chunks (via `Vec<Box<Chunk>>`) and the
//! retire queue; audio thread holds a cheap clone of the shared
//! `Arc<SlotsShared>` and reaches slots by loading the index each
//! retain/release inside a quiescence bracket.

use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

/// log2 of chunk size. 64-slot chunks = 1 KiB per chunk.
const CHUNK_LOG2: u32 = 6;
pub(crate) const CHUNK_SIZE: usize = 1 << CHUNK_LOG2;
const CHUNK_MASK: u32 = (CHUNK_SIZE as u32) - 1;

/// Hard ceiling on per-table chunk count. 1024 × 64 = 65 536 slots.
/// Sized to fit the ChunkIndex on the heap comfortably; raise if a
/// session ever needs more.
const MAX_CHUNKS: usize = 1024;

#[repr(C)]
pub(crate) struct Slot {
    pub id_and_gen: AtomicU64,
    pub refcount: AtomicU32,
}

impl Slot {
    const fn empty() -> Self {
        Self {
            id_and_gen: AtomicU64::new(0),
            refcount: AtomicU32::new(0),
        }
    }
}

type Chunk = [Slot; CHUNK_SIZE];

fn empty_chunk() -> Box<Chunk> {
    // `Slot` is not Copy, so build via from_fn.
    Box::new(std::array::from_fn(|_| Slot::empty()))
}

/// Small metadata array of chunk pointers. Published via
/// `AtomicPtr`. `len` chunks are live; remaining entries are null.
#[repr(C)]
struct ChunkIndex {
    len: u32,
    chunks: [*const Chunk; MAX_CHUNKS],
}

impl ChunkIndex {
    fn empty() -> Self {
        Self {
            len: 0,
            chunks: [ptr::null(); MAX_CHUNKS],
        }
    }

    #[inline]
    fn slot_ptr(&self, slot_idx: u32) -> *const Slot {
        let chunk_idx = (slot_idx >> CHUNK_LOG2) as usize;
        let offset = (slot_idx & CHUNK_MASK) as usize;
        debug_assert!(
            chunk_idx < self.len as usize,
            "slot {} out of bounds (len={})",
            slot_idx,
            self.len
        );
        // SAFETY: chunk pointers at positions < len are live Box<Chunk>
        // pointers owned by the control thread's chunk vector.
        unsafe { (*self.chunks[chunk_idx]).as_ptr().add(offset) }
    }
}

// ChunkIndex holds raw pointers to heap-allocated arrays of atomics.
// The pointees are thread-safe; the pointers themselves are stable
// for the life of the index.
unsafe impl Send for ChunkIndex {}
unsafe impl Sync for ChunkIndex {}

/// RCU-style quiescence barrier. Audio-thread retain/release bracket
/// their access with `started`/`completed` fetch-adds; the control
/// thread retires old chunk indices once `completed >= retire_at`.
struct Quiescence {
    started: AtomicU64,
    completed: AtomicU64,
}

impl Quiescence {
    const fn new() -> Self {
        Self {
            started: AtomicU64::new(0),
            completed: AtomicU64::new(0),
        }
    }
}

/// Shared state between the control and audio halves.
struct SlotsShared {
    index: AtomicPtr<ChunkIndex>,
    quiescence: Quiescence,
}

impl Drop for SlotsShared {
    fn drop(&mut self) {
        // Last Arc holder reclaims the current index. Retired
        // indices are drained before this via `RefcountTable::drop`.
        let ptr = self.index.swap(ptr::null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            // SAFETY: index was allocated via Box::into_raw in
            // RefcountTable::with_capacity / grow. No other owner.
            unsafe { drop(Box::from_raw(ptr)) };
        }
    }
}

/// Cheap-to-clone handle over the shared slots state. One lives in
/// each of `ArcTableControl` and `ArcTableAudio`; clones share the
/// same underlying atomics and chunk pointers.
#[derive(Clone)]
pub(crate) struct Slots {
    shared: Arc<SlotsShared>,
}

impl Slots {
    fn new() -> Self {
        let empty = Box::new(ChunkIndex::empty());
        Self {
            shared: Arc::new(SlotsShared {
                index: AtomicPtr::new(Box::into_raw(empty)),
                quiescence: Quiescence::new(),
            }),
        }
    }

    /// Audio-thread retain. Wait-free, two uncontended atomics for
    /// quiescence + one `fetch_add` on the slot refcount.
    #[inline]
    pub fn retain(&self, id_and_gen: u64) {
        let _gen = self.shared.quiescence.started.fetch_add(1, Ordering::AcqRel);
        // SAFETY: between started++ and completed++ the control
        // thread will not free any index whose retire_at <= _gen,
        // so the pointer we load here stays valid for this call.
        let idx_ptr = self.shared.index.load(Ordering::Acquire);
        let idx = unsafe { &*idx_ptr };
        let slot = id_and_gen as u32;
        let s = unsafe { &*idx.slot_ptr(slot) };
        debug_assert_eq!(
            s.id_and_gen.load(Ordering::Acquire),
            id_and_gen,
            "refcount retain on stale/forged id"
        );
        s.refcount.fetch_add(1, Ordering::Relaxed);
        self.shared.quiescence.completed.fetch_add(1, Ordering::Release);
    }

    /// Audio-thread release. Wait-free. Returns `true` if the caller
    /// was the last reference.
    #[inline]
    pub fn release(&self, id_and_gen: u64) -> bool {
        let _gen = self.shared.quiescence.started.fetch_add(1, Ordering::AcqRel);
        let idx_ptr = self.shared.index.load(Ordering::Acquire);
        let idx = unsafe { &*idx_ptr };
        let slot = id_and_gen as u32;
        let s = unsafe { &*idx.slot_ptr(slot) };
        debug_assert_eq!(
            s.id_and_gen.load(Ordering::Acquire),
            id_and_gen,
            "refcount release on stale/forged id"
        );
        let prev = s.refcount.fetch_sub(1, Ordering::AcqRel);
        self.shared.quiescence.completed.fetch_add(1, Ordering::Release);
        prev == 1
    }

    /// Control-thread install: populate a free slot with a freshly
    /// minted id. Refcount is initialised to 1 (mint-time
    /// reference). Reaches the slot via the current index.
    pub fn install(&self, slot: u32, id_and_gen: u64) {
        // Control thread is the sole writer of `index`, so a
        // Relaxed load is sufficient — but we pay Acquire for
        // uniformity with the audio side and to pair with the last
        // growth's Release swap.
        let idx = unsafe { &*self.shared.index.load(Ordering::Acquire) };
        let s = unsafe { &*idx.slot_ptr(slot) };
        s.id_and_gen.store(id_and_gen, Ordering::Release);
        s.refcount.store(1, Ordering::Release);
    }

    /// Control-thread clear: zero a slot once its refcount is zero.
    pub fn clear(&self, slot: u32) {
        let idx = unsafe { &*self.shared.index.load(Ordering::Acquire) };
        let s = unsafe { &*idx.slot_ptr(slot) };
        s.id_and_gen.store(0, Ordering::Release);
    }

    pub(crate) fn refcount_of(&self, slot: u32) -> u32 {
        let idx = unsafe { &*self.shared.index.load(Ordering::Acquire) };
        let s = unsafe { &*idx.slot_ptr(slot) };
        s.refcount.load(Ordering::Acquire)
    }

    pub(crate) fn id_of(&self, slot: u32) -> u64 {
        let idx = unsafe { &*self.shared.index.load(Ordering::Acquire) };
        let s = unsafe { &*idx.slot_ptr(slot) };
        s.id_and_gen.load(Ordering::Acquire)
    }
}

/// A retired chunk index, waiting on the quiescence barrier before
/// its backing Box can be freed.
struct RetiredIndex {
    ptr: *mut ChunkIndex,
    retire_at: u64,
}

// Safe: `RetiredIndex` is only ever held on the control thread.
// The pointer is never dereferenced until drop.
unsafe impl Send for RetiredIndex {}

#[derive(Debug)]
pub struct Exhausted;

/// Control-thread free-list + chunk ownership wrapped around a
/// shared `Slots` handle. Not `Clone` — the control side is
/// single-owner.
pub(crate) struct RefcountTable {
    slots: Slots,
    free: Vec<u32>,
    next_generation: u32,
    // Box for address stability: ChunkIndex stores raw pointers
    // into these chunks. A bare `Vec<Chunk>` would move its
    // elements on reallocation and invalidate those pointers.
    #[allow(clippy::vec_box)]
    chunks: Vec<Box<Chunk>>,
    retire_queue: Vec<RetiredIndex>,
}

impl RefcountTable {
    pub fn with_capacity(capacity: u32) -> Self {
        assert!(capacity > 0, "RefcountTable capacity must be non-zero");
        let slots = Slots::new();
        let mut t = Self {
            slots,
            free: Vec::new(),
            next_generation: 1,
            chunks: Vec::new(),
            retire_queue: Vec::new(),
        };
        // Initial allocation: round up to whole chunks.
        let needed = chunks_for_slots(capacity);
        t.append_chunks(needed);
        t
    }

    /// Append `n` new chunks, publish a new index, retire the old.
    /// Extends the free list with the new slot indices (LIFO by
    /// descending index so low indices pop first).
    fn append_chunks(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        let old_ptr = self.slots.shared.index.load(Ordering::Acquire);
        let old = unsafe { &*old_ptr };
        let old_len = old.len;
        let new_len = old_len + n;
        assert!(
            (new_len as usize) <= MAX_CHUNKS,
            "ArcTable chunk cap exceeded: requested {} chunks, cap is {}",
            new_len,
            MAX_CHUNKS
        );

        let mut new_index = Box::new(ChunkIndex::empty());
        // Copy existing chunk pointers forward.
        new_index.chunks[..old_len as usize]
            .copy_from_slice(&old.chunks[..old_len as usize]);

        // Allocate new chunks; record their pointers before we move
        // ownership into `self.chunks`.
        for i in 0..n {
            let chunk = empty_chunk();
            let ptr: *const Chunk = &*chunk as *const Chunk;
            new_index.chunks[(old_len + i) as usize] = ptr;
            self.chunks.push(chunk);
        }
        new_index.len = new_len;

        let new_raw = Box::into_raw(new_index);
        // Publish.
        let old_raw = self.slots.shared.index.swap(new_raw, Ordering::Release);
        // Sample quiescence AFTER the swap: any in-flight audio op
        // that might still hold the old pointer has already
        // incremented `started` to a value <= retire_at.
        let retire_at = self.slots.shared.quiescence.started.load(Ordering::Acquire);

        debug_assert_eq!(old_raw, old_ptr);
        if old_len == 0 {
            // Initial empty index was never observed by any audio
            // op (no slots yet for them to reach). Skip the queue
            // and drop immediately — no quiescence wait needed.
            unsafe { drop(Box::from_raw(old_raw)) };
            let _ = retire_at;
        } else {
            self.retire_queue.push(RetiredIndex {
                ptr: old_raw,
                retire_at,
            });
        }

        // Extend the free list with newly-available slots.
        let new_start = old_len * (CHUNK_SIZE as u32);
        let new_end = new_len * (CHUNK_SIZE as u32);
        self.free.reserve(n as usize * CHUNK_SIZE);
        for s in (new_start..new_end).rev() {
            self.free.push(s);
        }
    }

    pub fn slots(&self) -> &Slots {
        &self.slots
    }

    pub fn capacity(&self) -> u32 {
        // Live chunk count × CHUNK_SIZE. Read the current index.
        let idx = unsafe { &*self.slots.shared.index.load(Ordering::Acquire) };
        idx.len * (CHUNK_SIZE as u32)
    }

    /// Grow the table by at least `additional_slots` capacity.
    /// Rounds up to a whole number of chunks. Returns the new
    /// total capacity.
    pub fn grow(&mut self, additional_slots: u32) -> u32 {
        if additional_slots == 0 {
            return self.capacity();
        }
        let add_chunks = chunks_for_slots(additional_slots);
        self.append_chunks(add_chunks);
        self.capacity()
    }

    /// Drop any retired indices whose grace period has elapsed.
    /// Call alongside the release drain on each housekeeping tick.
    pub fn drain_retired(&mut self) {
        let done = self.slots.shared.quiescence.completed.load(Ordering::Acquire);
        self.retire_queue.retain(|r| {
            if done >= r.retire_at {
                // SAFETY: ptr came from Box::into_raw above; no
                // other owner; audio-thread quiescence guarantees
                // no live reader.
                unsafe { drop(Box::from_raw(r.ptr)) };
                false
            } else {
                true
            }
        });
    }

    /// Control-thread mint: grab a free slot, bump the generation,
    /// write id_and_gen with refcount = 1, return the encoded id.
    pub fn insert(&mut self) -> Result<u64, Exhausted> {
        let slot = self.free.pop().ok_or(Exhausted)?;
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);
        if self.next_generation == 0 {
            self.next_generation = 1;
        }
        let id_and_gen = ((generation as u64) << 32) | (slot as u64);
        self.slots.install(slot, id_and_gen);
        Ok(id_and_gen)
    }

    /// Control-thread remove: clear the slot and return it to the
    /// free list. Caller must have observed refcount == 0.
    pub fn remove(&mut self, id_and_gen: u64) {
        let slot = id_and_gen as u32;
        debug_assert_eq!(
            self.slots.id_of(slot),
            id_and_gen,
            "remove on non-matching id"
        );
        debug_assert_eq!(
            self.slots.refcount_of(slot),
            0,
            "remove on live refcount"
        );
        self.slots.clear(slot);
        self.free.push(slot);
    }

    #[cfg(test)]
    pub fn free_len(&self) -> usize {
        self.free.len()
    }

    #[cfg(test)]
    pub fn retire_queue_len(&self) -> usize {
        self.retire_queue.len()
    }
}

impl Drop for RefcountTable {
    fn drop(&mut self) {
        // Force-retire everything. Host contract: audio thread is
        // stopped before ArcTableControl drops.
        while let Some(r) = self.retire_queue.pop() {
            unsafe { drop(Box::from_raw(r.ptr)) };
        }
        // SlotsShared::drop frees the current index. Chunks drop
        // with `self.chunks`.
    }
}

#[inline]
fn chunks_for_slots(slots: u32) -> u32 {
    (slots as usize).div_ceil(CHUNK_SIZE) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_capacity_rounds_up_to_chunk() {
        let t = RefcountTable::with_capacity(1);
        assert_eq!(t.capacity(), CHUNK_SIZE as u32);
        let t = RefcountTable::with_capacity(CHUNK_SIZE as u32);
        assert_eq!(t.capacity(), CHUNK_SIZE as u32);
        let t = RefcountTable::with_capacity(CHUNK_SIZE as u32 + 1);
        assert_eq!(t.capacity(), 2 * CHUNK_SIZE as u32);
    }

    #[test]
    fn insert_until_exhaustion_within_one_chunk() {
        let mut t = RefcountTable::with_capacity(4);
        // Capacity actually 64; fill it.
        let cap = t.capacity();
        let ids: Vec<u64> = (0..cap).map(|_| t.insert().unwrap()).collect();
        assert!(matches!(t.insert(), Err(Exhausted)));
        for id in ids {
            let was_last = t.slots().release(id);
            assert!(was_last);
            t.remove(id);
        }
        assert_eq!(t.free_len(), cap as usize);
    }

    #[test]
    fn retain_release_balance() {
        let mut t = RefcountTable::with_capacity(1);
        let id = t.insert().unwrap();
        let slots = t.slots().clone();
        slots.retain(id);
        slots.retain(id);
        assert_eq!(slots.refcount_of(id as u32), 3);
        assert!(!slots.release(id));
        assert!(!slots.release(id));
        assert!(slots.release(id));
        t.remove(id);
    }

    #[test]
    fn generations_advance() {
        let mut t = RefcountTable::with_capacity(1);
        let id1 = t.insert().unwrap();
        assert!(t.slots().release(id1));
        t.remove(id1);
        let id2 = t.insert().unwrap();
        assert_eq!((id1 as u32), (id2 as u32));
        assert_ne!((id1 >> 32) as u32, (id2 >> 32) as u32);
    }

    #[test]
    #[should_panic(expected = "stale")]
    #[cfg(debug_assertions)]
    fn stale_id_debug_asserts() {
        let mut t = RefcountTable::with_capacity(1);
        let id = t.insert().unwrap();
        let stale = ((id >> 32).wrapping_add(1) << 32) | (id as u32 as u64);
        t.slots().retain(stale);
    }

    #[test]
    fn grow_extends_capacity_and_preserves_ids() {
        let mut t = RefcountTable::with_capacity(CHUNK_SIZE as u32);
        // Fill the first chunk.
        let ids: Vec<u64> = (0..CHUNK_SIZE as u32).map(|_| t.insert().unwrap()).collect();
        assert!(matches!(t.insert(), Err(Exhausted)));

        let new_cap = t.grow(1); // rounds up to one more chunk
        assert_eq!(new_cap, 2 * CHUNK_SIZE as u32);

        // Existing ids still resolve.
        for &id in &ids {
            assert_eq!(t.slots().id_of(id as u32), id);
            assert_eq!(t.slots().refcount_of(id as u32), 1);
        }

        // New slots are mintable.
        let new_ids: Vec<u64> = (0..CHUNK_SIZE as u32)
            .map(|_| t.insert().unwrap())
            .collect();
        assert!(matches!(t.insert(), Err(Exhausted)));

        // Release everything.
        for id in ids.into_iter().chain(new_ids.into_iter()) {
            assert!(t.slots().release(id));
            t.remove(id);
        }
    }

    #[test]
    fn retired_index_drops_after_quiescence() {
        let mut t = RefcountTable::with_capacity(1);
        // Do some retains/releases to bump completed counter past
        // any retire_at we see.
        let id = t.insert().unwrap();
        t.slots().retain(id);
        t.slots().release(id);

        // Grow — the old index is queued for retirement.
        let _ = t.grow(1);
        assert_eq!(t.retire_queue_len(), 1);

        // One more audio op advances `completed` past retire_at.
        t.slots().retain(id);
        t.slots().release(id);

        t.drain_retired();
        assert_eq!(t.retire_queue_len(), 0);

        // Cleanup.
        assert!(t.slots().release(id));
        t.remove(id);
    }

    #[test]
    fn drop_frees_retired_queue() {
        // Ensure Drop reclaims pending retired indices even if
        // drain_retired was never called.
        let mut t = RefcountTable::with_capacity(1);
        let _ = t.grow(1);
        let _ = t.grow(1);
        assert!(t.retire_queue_len() >= 1);
        // Drop here — Miri / ASan would flag a leak if this failed.
        drop(t);
    }

    #[test]
    fn grow_zero_is_noop() {
        let mut t = RefcountTable::with_capacity(1);
        let cap = t.capacity();
        assert_eq!(t.grow(0), cap);
        assert_eq!(t.retire_queue_len(), 0);
    }
}
