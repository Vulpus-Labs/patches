//! `ArcTable<T>`: generic host-side refcounted handle table.
//!
//! The control-thread half (`ArcTableControl<T>`) owns the live
//! `Arc<T>` values and the refcount table. The audio-thread half
//! (`ArcTableAudio`) owns only a clone of the shared atomic slot
//! array and the producer end of the release queue. A single
//! [`ArcTable::new`] call returns the pair.
//!
//! ADR 0045 §2 / resolved design point 2.

use std::collections::HashMap;
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::refcount::{Exhausted, RefcountTable, Slots};

#[derive(Debug)]
pub enum ArcTableError {
    Exhausted,
}

impl From<Exhausted> for ArcTableError {
    fn from(_: Exhausted) -> Self {
        ArcTableError::Exhausted
    }
}

/// Control-thread half. Owns the `Arc<T>` values and services the
/// drain queue. Not `Send` across audio-thread boundaries.
pub struct ArcTableControl<T: ?Sized> {
    refcount: RefcountTable,
    entries: HashMap<u64, Arc<T>>,
    release_rx: Consumer<u64>,
}

/// Audio-thread half. Retain is never needed under
/// retain-by-default delivery (ADR 0045 §2 point 3) but exposed
/// for the host-side frame dispatcher. Release pushes to the
/// drain queue when the caller was the last reference.
pub struct ArcTableAudio {
    slots: Slots,
    release_tx: Producer<u64>,
}

pub struct ArcTable;

impl ArcTable {
    /// Build a table sized to `capacity` slots.
    /// Returns the
    /// control and audio halves. The release ring buffer is sized
    /// to `capacity`: the audio thread can produce at most one
    /// release per live id, so this is sufficient and never
    /// overflows.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: ?Sized>(capacity: u32) -> (ArcTableControl<T>, ArcTableAudio) {
        let refcount = RefcountTable::with_capacity(capacity);
        let slots = refcount.slots().clone();
        let (tx, rx) = RingBuffer::new(capacity as usize);
        (
            ArcTableControl {
                refcount,
                entries: HashMap::with_capacity(capacity as usize),
                release_rx: rx,
            },
            ArcTableAudio {
                slots,
                release_tx: tx,
            },
        )
    }
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
    /// the slot to the free list.
    pub fn drain_released(&mut self) {
        while let Ok(id) = self.release_rx.pop() {
            if self.entries.remove(&id).is_some() {
                self.refcount.remove(id);
            } else {
                // Double-release or already drained: a bug, but
                // not one to panic over in release builds.
                debug_assert!(false, "drain saw id {id:#x} not in entries");
            }
        }
    }

    pub fn capacity(&self) -> u32 {
        self.refcount.capacity()
    }

    #[cfg(test)]
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
    /// Audio-thread retain. Wait-free; exposed for host-side
    /// dispatch code that retains on the plugin's behalf.
    #[inline]
    pub fn retain(&self, id: u64) {
        self.slots.retain(id);
    }

    /// Audio-thread release. Wait-free. If the caller was the
    /// last reference the id is pushed onto the drain queue; the
    /// push is guaranteed to succeed because the queue is sized
    /// to the table capacity and ids are unique while live.
    #[inline]
    pub fn release(&mut self, id: u64) {
        if self.slots.release(id) {
            // `push` returns Err if the ring is full. That cannot
            // happen: capacity == slots capacity, and a slot can
            // only contribute one in-flight release at a time.
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
    fn exhaustion_clean_error() {
        let (mut control, _audio) = ArcTable::new::<u32>(2);
        let _a = control.mint(Arc::new(1)).unwrap();
        let _b = control.mint(Arc::new(2)).unwrap();
        assert!(matches!(
            control.mint(Arc::new(3)),
            Err(ArcTableError::Exhausted)
        ));
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
}
