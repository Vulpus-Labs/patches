//! `ArcTable<T>`: generic host-side refcounted handle table.
//!
//! The control-thread half (`ArcTableControl<T>`) owns the live
//! `Arc<T>` values, the chunked refcount storage, and the retire
//! queue for grown-out chunk indices. The audio-thread half
//! (`ArcTableAudio`) owns a shared `Slots` handle (cheap Arc clone)
//! and the producer end of the release queue.
//!
//! ADR 0045 §2 / resolved design point 2; growable storage per
//! spike 6.

use std::collections::HashMap;
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::refcount::{CHUNK_SIZE, Exhausted, RefcountTable, Slots};

#[derive(Debug)]
pub enum ArcTableError {
    Exhausted,
}

impl From<Exhausted> for ArcTableError {
    fn from(_: Exhausted) -> Self {
        ArcTableError::Exhausted
    }
}

/// Release-queue sizing. The queue must hold at most one in-flight
/// release per live slot. Since the table is grow-only and capped
/// at `MAX_CHUNKS * CHUNK_SIZE` slots by `RefcountTable`, sizing the
/// ring to the same upper bound lets growth proceed without queue
/// overflow. 1024 chunks × 64 slots × 8 bytes = 512 KiB per table.
const RELEASE_RING_CAPACITY: usize = 1024 * CHUNK_SIZE;

/// Control-thread half. Owns the `Arc<T>` values and services the
/// drain queue. Not `Send` across audio-thread boundaries.
pub struct ArcTableControl<T: ?Sized> {
    refcount: RefcountTable,
    entries: HashMap<u64, Arc<T>>,
    release_rx: Consumer<u64>,
}

/// Audio-thread half. Retain and release are wait-free.
pub struct ArcTableAudio {
    slots: Slots,
    release_tx: Producer<u64>,
}

pub struct ArcTable;

impl ArcTable {
    /// Build a table with initial capacity rounded up to a whole
    /// chunk. Returns the control and audio halves. The table can
    /// grow via `ArcTableControl::grow`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: ?Sized>(initial_capacity: u32) -> (ArcTableControl<T>, ArcTableAudio) {
        let refcount = RefcountTable::with_capacity(initial_capacity);
        let slots = refcount.slots().clone();
        let (tx, rx) = RingBuffer::new(RELEASE_RING_CAPACITY);
        (
            ArcTableControl {
                refcount,
                entries: HashMap::with_capacity(refcount_capacity(&slots) as usize),
                release_rx: rx,
            },
            ArcTableAudio {
                slots,
                release_tx: tx,
            },
        )
    }
}

fn refcount_capacity(_slots: &Slots) -> u32 {
    // Can't cheaply query from a Slots clone; caller knows capacity
    // via RefcountTable. The HashMap just needs a reasonable seed.
    64
}

impl<T: ?Sized> ArcTableControl<T> {
    /// Control-thread mint. Stores the `Arc` and returns a raw id
    /// (`(generation << 32) | slot`). Typed wrappers live on
    /// `RuntimeArcTables`.
    pub fn mint(&mut self, value: Arc<T>) -> Result<u64, ArcTableError> {
        let id = self.refcount.insert()?;
        self.entries.insert(id, value);
        Ok(id)
    }

    /// Control-thread drain. Pops everything the audio thread has
    /// marked for release, drops the matching `Arc`, and returns
    /// the slot to the free list. Also retires any chunk indices
    /// whose quiescence grace period has elapsed.
    pub fn drain_released(&mut self) {
        while let Ok(id) = self.release_rx.pop() {
            if self.entries.remove(&id).is_some() {
                self.refcount.remove(id);
            } else {
                debug_assert!(false, "drain saw id {id:#x} not in entries");
            }
        }
        self.refcount.drain_retired();
    }

    /// Grow the table by at least `additional_slots` more capacity.
    /// Rounds up to a whole chunk. Returns the new total capacity.
    /// Old chunk indices are retired via the quiescence barrier.
    pub fn grow(&mut self, additional_slots: u32) -> u32 {
        self.refcount.grow(additional_slots)
    }

    pub fn capacity(&self) -> u32 {
        self.refcount.capacity()
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn strong_count_of(&self, id: u64) -> Option<usize>
    where
        T: Sized,
    {
        self.entries.get(&id).map(Arc::strong_count)
    }

    #[cfg(test)]
    pub fn strong_count_of_unsized(&self, id: u64) -> Option<usize> {
        self.entries.get(&id).map(Arc::strong_count)
    }
}

impl<T: ?Sized> Drop for ArcTableControl<T> {
    fn drop(&mut self) {
        self.drain_released();
        if !self.entries.is_empty() {
            tracing::warn!(
                leaked = self.entries.len(),
                "ArcTableControl dropped with live ids",
            );
        }
    }
}

impl ArcTableAudio {
    /// Audio-thread retain. Wait-free.
    #[inline]
    pub fn retain(&self, id: u64) {
        self.slots.retain(id);
    }

    /// Audio-thread release. Wait-free. If the caller was the last
    /// reference the id is pushed onto the drain queue.
    #[inline]
    pub fn release(&mut self, id: u64) {
        if self.slots.release(id) {
            // Ring is sized to the maximum table capacity, so push
            // cannot fail under grow-only storage.
            let _ = self.release_tx.push(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_release_drain_round_trip() {
        let (mut control, mut audio) = ArcTable::new::<[u8]>(4);
        let data: Arc<[u8]> = Arc::from(vec![1u8, 2, 3].into_boxed_slice());
        let id = control.mint(Arc::clone(&data)).unwrap();
        assert_eq!(control.live_count(), 1);
        audio.release(id);
        control.drain_released();
        assert_eq!(control.live_count(), 0);
        assert_eq!(Arc::strong_count(&data), 1);
    }

    #[test]
    fn exhaustion_and_grow() {
        // Initial capacity rounds up to 64 slots.
        let (mut control, _audio) = ArcTable::new::<u32>(2);
        let cap = control.capacity();
        let ids: Vec<u64> = (0..cap)
            .map(|i| control.mint(Arc::new(i)).unwrap())
            .collect();
        assert!(matches!(control.mint(Arc::new(0u32)), Err(ArcTableError::Exhausted)));

        // Grow adds at least one chunk.
        let new_cap = control.grow(1);
        assert!(new_cap > cap);

        // Post-grow mint succeeds.
        let _ = control.mint(Arc::new(999u32)).unwrap();

        // Drop ids cleanly.
        drop(ids);
    }

    #[test]
    fn drain_leaves_no_leaks_on_drop() {
        let payload = Arc::new(99u32);
        {
            let (mut control, mut audio) = ArcTable::new::<u32>(2);
            let id = control.mint(Arc::clone(&payload)).unwrap();
            audio.release(id);
            control.drain_released();
        }
        assert_eq!(Arc::strong_count(&payload), 1);
    }

    #[test]
    fn retain_bumps_refcount() {
        let (mut control, mut audio) = ArcTable::new::<u32>(2);
        let v = Arc::new(7u32);
        let id = control.mint(Arc::clone(&v)).unwrap();
        audio.retain(id);
        audio.release(id); // back to 1
        audio.release(id); // back to 0 -> queue
        control.drain_released();
        assert_eq!(Arc::strong_count(&v), 1);
    }

    #[test]
    fn ids_remain_valid_across_growth() {
        let (mut control, mut audio) = ArcTable::new::<u32>(1);
        let v1 = Arc::new(1u32);
        let v2 = Arc::new(2u32);
        let id1 = control.mint(Arc::clone(&v1)).unwrap();
        // Fill first chunk, then grow and mint more.
        let cap = control.capacity();
        let filler: Vec<u64> = (1..cap)
            .map(|_| control.mint(Arc::new(0u32)).unwrap())
            .collect();
        let _ = control.grow(1);
        let id2 = control.mint(Arc::clone(&v2)).unwrap();

        // Pre-growth id still retains/releases correctly.
        audio.retain(id1);
        audio.release(id1); // back to 1
        audio.release(id1); // back to 0 -> queue
        // Post-growth id likewise.
        audio.release(id2);

        // Filler goes too.
        for id in filler {
            audio.release(id);
        }
        control.drain_released();
        assert_eq!(Arc::strong_count(&v1), 1);
        assert_eq!(Arc::strong_count(&v2), 1);
    }
}
