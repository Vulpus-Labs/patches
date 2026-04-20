//! Fixed-capacity lock-free refcount slot array.
//!
//! ADR 0045 §2 / resolved design point 2. Each slot holds
//! `{ AtomicU64 id_and_gen, AtomicU32 refcount }`. The slot index
//! is encoded in the id (`id as u32`), so audio-thread retain /
//! release decode the slot directly and perform a single atomic —
//! no probing, no locks, wait-free.
//!
//! Control-thread slot allocation goes through a free-list
//! (`Vec<u32>`) on the [`RefcountTable`] owner; probing is not
//! needed because ids carry their own slot. A later spike
//! (ADR 0045 spike 6) replaces `Arc<[Slot]>` with an `AtomicPtr`
//! + RCU pair to support growth.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[repr(C)]
pub(crate) struct Slot {
    pub id_and_gen: AtomicU64,
    pub refcount: AtomicU32,
}

impl Slot {
    fn empty() -> Self {
        Self {
            id_and_gen: AtomicU64::new(0),
            refcount: AtomicU32::new(0),
        }
    }
}

/// Shared atomic slot array. Cloned cheaply between the control
/// and audio handles of an `ArcTable`.
#[derive(Clone)]
pub(crate) struct Slots {
    slots: Arc<[Slot]>,
}

impl Slots {
    pub fn with_capacity(capacity: u32) -> Self {
        let vec: Vec<Slot> = (0..capacity).map(|_| Slot::empty()).collect();
        Self {
            slots: vec.into_boxed_slice().into(),
        }
    }

    pub fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }

    fn slot(&self, idx: u32) -> &Slot {
        &self.slots[idx as usize]
    }

    /// Control-thread: populate a free slot with a freshly minted id.
    /// Refcount is initialised to 1 (the mint-time reference).
    pub fn install(&self, slot: u32, id_and_gen: u64) {
        let s = self.slot(slot);
        s.id_and_gen.store(id_and_gen, Ordering::Release);
        s.refcount.store(1, Ordering::Release);
    }

    /// Control-thread: clear a slot once its refcount has reached zero.
    /// Returns the generation that was stored, for audit.
    pub fn clear(&self, slot: u32) -> u32 {
        let s = self.slot(slot);
        let prev = s.id_and_gen.swap(0, Ordering::AcqRel);
        (prev >> 32) as u32
    }

    /// Audio-thread: bump the refcount. Wait-free, no probing.
    ///
    /// In debug builds asserts the stored id matches the caller's
    /// id (catches stale / forged ids). In release builds the
    /// decode is trusted — the frame dispatcher has already
    /// validated the id before delivering it.
    #[inline]
    pub fn retain(&self, id_and_gen: u64) {
        let slot = id_and_gen as u32;
        let s = self.slot(slot);
        debug_assert_eq!(
            s.id_and_gen.load(Ordering::Acquire),
            id_and_gen,
            "refcount retain on stale/forged id"
        );
        s.refcount.fetch_add(1, Ordering::Relaxed);
    }

    /// Audio-thread: decrement the refcount. Returns `true` if the
    /// caller was the last reference and the slot should be queued
    /// for drain.
    #[inline]
    pub fn release(&self, id_and_gen: u64) -> bool {
        let slot = id_and_gen as u32;
        let s = self.slot(slot);
        debug_assert_eq!(
            s.id_and_gen.load(Ordering::Acquire),
            id_and_gen,
            "refcount release on stale/forged id"
        );
        let prev = s.refcount.fetch_sub(1, Ordering::AcqRel);
        prev == 1
    }

    pub(crate) fn refcount_of(&self, slot: u32) -> u32 {
        self.slot(slot).refcount.load(Ordering::Acquire)
    }

    pub(crate) fn id_of(&self, slot: u32) -> u64 {
        self.slot(slot).id_and_gen.load(Ordering::Acquire)
    }
}

/// Control-thread free-list + generation counter wrapped around a
/// shared [`Slots`] array. Not `Clone` — the control side is
/// single-owner.
pub(crate) struct RefcountTable {
    slots: Slots,
    free: Vec<u32>,
    next_generation: u32,
}

#[derive(Debug)]
pub struct Exhausted;

impl RefcountTable {
    pub fn with_capacity(capacity: u32) -> Self {
        assert!(capacity > 0, "RefcountTable capacity must be non-zero");
        let slots = Slots::with_capacity(capacity);
        // Free list is LIFO; push high-to-low so we pop low-index
        // slots first — nicer in logs and tests.
        let free: Vec<u32> = (0..capacity).rev().collect();
        Self {
            slots,
            free,
            next_generation: 1,
        }
    }

    pub fn slots(&self) -> &Slots {
        &self.slots
    }

    pub fn capacity(&self) -> u32 {
        self.slots.capacity()
    }

    /// Control-thread mint: grab a free slot, bump the generation,
    /// write id_and_gen with refcount = 1, return the encoded id.
    pub fn insert(&mut self) -> Result<u64, Exhausted> {
        let slot = self.free.pop().ok_or(Exhausted)?;
        let generation = self.next_generation;
        // 0 is reserved as "empty" in the slot's id_and_gen field,
        // so the packed id must never be 0. Our free slots start
        // at generation 1, and we advance by 1 each mint; skip
        // wraparound back to 0 if we ever reach u32::MAX.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_until_exhaustion() {
        let mut t = RefcountTable::with_capacity(4);
        let ids: Vec<u64> = (0..4).map(|_| t.insert().unwrap()).collect();
        assert!(matches!(t.insert(), Err(Exhausted)));
        for id in ids {
            let was_last = t.slots().release(id);
            assert!(was_last);
            t.remove(id);
        }
        assert_eq!(t.free_len(), 4);
        // Can mint again after release.
        let _ = t.insert().unwrap();
    }

    #[test]
    fn retain_release_balance() {
        let mut t = RefcountTable::with_capacity(2);
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
        // Same slot (0), different generation.
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
}
