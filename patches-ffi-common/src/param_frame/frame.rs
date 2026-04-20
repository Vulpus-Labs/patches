//! `ParamFrame` — owned byte buffer carrying packed scalars + tail u64 buffer
//! slots, sized from a `ParamLayout`.
//!
//! Backing storage is a `Vec<u64>` so the tail slot area is naturally 8-byte
//! aligned. The scalar area is viewed as `&[u8]` via a safe transmute of the
//! leading `Vec<u64>` region (u64 is POD, any byte pattern is valid).

use crate::param_layout::ParamLayout;

pub const U64_SIZE: usize = std::mem::size_of::<u64>();

/// Owned, fixed-size packed parameter frame.
///
/// Length and capacity are set at construction and never change.
#[derive(Debug)]
pub struct ParamFrame {
    /// Backing storage. Length = `scalar_words + buffer_slot_count`.
    storage: Vec<u64>,
    /// Effective (unpadded) scalar byte count as declared by the layout.
    scalar_size: u32,
    /// Number of u64 words that cover the scalar area (scalar_size padded up to
    /// multiple of 8, divided by 8). Padding is internal to the frame only.
    scalar_words: u32,
    /// Number of tail buffer slots.
    buffer_slot_count: u32,
    /// Layout descriptor hash, captured at construction for sanity checks.
    layout_hash: u64,
}

impl ParamFrame {
    /// Allocate a zero-filled frame shaped for `layout`. Capacity = length.
    pub fn with_layout(layout: &ParamLayout) -> Self {
        let scalar_size = layout.scalar_size;
        let scalar_words = scalar_size.div_ceil(U64_SIZE as u32);
        let buffer_slot_count = layout.buffer_slots.len() as u32;
        let total_words = scalar_words as usize + buffer_slot_count as usize;

        let storage = vec![0u64; total_words];
        debug_assert_eq!(storage.len(), storage.capacity());

        Self {
            storage,
            scalar_size,
            scalar_words,
            buffer_slot_count,
            layout_hash: layout.descriptor_hash,
        }
    }

    /// Effective scalar byte length (as declared by the layout).
    #[inline]
    pub fn scalar_size(&self) -> usize {
        self.scalar_size as usize
    }

    /// Number of tail buffer slots.
    #[inline]
    pub fn buffer_slot_count(&self) -> usize {
        self.buffer_slot_count as usize
    }

    /// Layout descriptor hash captured at construction.
    #[inline]
    pub fn layout_hash(&self) -> u64 {
        self.layout_hash
    }

    /// Scalar area as a byte slice of exactly `scalar_size` bytes.
    #[inline]
    pub fn scalar_area(&self) -> &[u8] {
        let words = &self.storage[..self.scalar_words as usize];
        // SAFETY: u64 is POD; all byte patterns are valid. The cast produces
        // `scalar_words * 8` bytes; we truncate to the effective `scalar_size`.
        let bytes = unsafe {
            std::slice::from_raw_parts(words.as_ptr() as *const u8, words.len() * U64_SIZE)
        };
        &bytes[..self.scalar_size as usize]
    }

    /// Mutable view of the scalar area.
    #[inline]
    pub fn scalar_area_mut(&mut self) -> &mut [u8] {
        let words = &mut self.storage[..self.scalar_words as usize];
        let len = words.len() * U64_SIZE;
        // SAFETY: u64 is POD; all byte patterns are valid for writes.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, len)
        };
        &mut bytes[..self.scalar_size as usize]
    }

    /// Tail slot table as a `u64` slice.
    #[inline]
    pub fn buffer_slots(&self) -> &[u64] {
        &self.storage[self.scalar_words as usize..]
    }

    /// Mutable tail slot table.
    #[inline]
    pub fn buffer_slots_mut(&mut self) -> &mut [u64] {
        &mut self.storage[self.scalar_words as usize..]
    }

    /// Zero all bytes. No reallocation, no capacity change.
    pub fn reset(&mut self) {
        for w in &mut self.storage {
            *w = 0;
        }
    }
}
