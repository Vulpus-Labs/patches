//! Audio-thread reader over a `ParamFrame`.
//!
//! `ParamViewIndex` is the prepare-time perfect-hash table mapping
//! `ParameterKey` to slot info. `ParamView` borrows the index + frame bytes
//! and exposes O(1) typed accessors.
//!
//! The perfect hash is a deterministic seed-search MPH: we try increasing
//! `u64` seeds until every declared key maps to a distinct bucket in a
//! table of size `next_power_of_two(keys.len()) * 2`. Key sets are small
//! (tens, rarely >100) so the search converges quickly.

use crate::modules::parameter_map::ParameterKey;

use crate::ids::FloatBufferId;
use crate::param_layout::{ParamLayout, ScalarTag};

use super::ParamFrame;

/// Per-key entry in the perfect-hash table.
#[derive(Debug, Clone, Copy)]
enum Entry {
    Empty,
    Scalar { name_hash: u64, tag: ScalarTag, offset: u32 },
    Buffer { name_hash: u64, slot_index: u16 },
}

/// Prepare-time perfect-hash index. Built once per module instance and
/// reused for every `ParamFrame` decoded against the same layout.
#[derive(Debug, Clone)]
pub struct ParamViewIndex {
    seed: u64,
    mask: u64, // bucket index mask (table.len() - 1)
    table: Vec<Entry>,
    scalar_size: u32,
    buffer_slot_count: u32,
    descriptor_hash: u64,
}

impl ParamViewIndex {
    /// Build an index from a layout. Deterministic: same layout ⇒ same index
    /// across runs and threads.
    pub fn from_layout(layout: &ParamLayout) -> Self {
        let total = layout.scalars.len() + layout.buffer_slots.len();

        if total == 0 {
            return Self {
                seed: 0,
                mask: 0,
                table: Vec::new(),
                scalar_size: layout.scalar_size,
                buffer_slot_count: 0,
                descriptor_hash: layout.descriptor_hash,
            };
        }

        // Over-size to ~4× keys so random seed search converges quickly for
        // the small key sets we see (tens of params per module).
        let bucket_count = (total * 4).next_power_of_two().max(2);
        let mask = (bucket_count - 1) as u64;

        let raw: Vec<(&ParameterKey, EntryKind)> = layout
            .scalars
            .iter()
            .map(|s| (&s.key, EntryKind::Scalar(s.tag, s.offset)))
            .chain(
                layout
                    .buffer_slots
                    .iter()
                    .map(|b| (&b.key, EntryKind::Buffer(b.slot_index))),
            )
            .collect();

        let mut seed: u64 = 0;
        let mut table: Vec<Entry> = vec![Entry::Empty; bucket_count];
        'outer: loop {
            // Reset table.
            table.fill(Entry::Empty);
            for (k, kind) in &raw {
                let h = key_hash_seed(k, seed);
                let bucket = (h & mask) as usize;
                match table[bucket] {
                    Entry::Empty => {
                        table[bucket] = match *kind {
                            EntryKind::Scalar(tag, offset) => Entry::Scalar {
                                name_hash: h,
                                tag,
                                offset,
                            },
                            EntryKind::Buffer(slot_index) => Entry::Buffer {
                                name_hash: h,
                                slot_index,
                            },
                        };
                    }
                    _ => {
                        seed = seed.wrapping_add(1);
                        debug_assert!(seed != 0, "perfect-hash seed search wrapped");
                        continue 'outer;
                    }
                }
            }
            break;
        }

        Self {
            seed,
            mask,
            table,
            scalar_size: layout.scalar_size,
            buffer_slot_count: layout.buffer_slots.len() as u32,
            descriptor_hash: layout.descriptor_hash,
        }
    }

    /// Layout descriptor hash the index was built for.
    pub fn descriptor_hash(&self) -> u64 {
        self.descriptor_hash
    }

    /// Non-allocating lookup by static name + index. Used by the typed
    /// `ParamKey` path (ADR 0046).
    #[inline]
    fn lookup_static(&self, name: &'static str, index: u16) -> Entry {
        self.lookup_raw(name.as_bytes(), index as u64)
    }

    #[inline]
    fn lookup_raw(&self, name_bytes: &[u8], index: u64) -> Entry {
        if self.table.is_empty() {
            return Entry::Empty;
        }
        let h = hash_raw(name_bytes, index, self.seed);
        let bucket = (h & self.mask) as usize;
        let entry = self.table[bucket];
        // In debug, verify the full hash matches — catches unknown keys.
        match entry {
            Entry::Scalar { name_hash, .. } | Entry::Buffer { name_hash, .. } => {
                debug_assert_eq!(
                    name_hash, h,
                    "ParamView: unknown key (name={:?} index={}) bucket collision with a declared key",
                    std::str::from_utf8(name_bytes).unwrap_or("<non-utf8>"),
                    index,
                );
                if name_hash != h {
                    return Entry::Empty;
                }
            }
            Entry::Empty => {
                debug_assert!(
                    false,
                    "ParamView: unknown key name={:?} index={}",
                    std::str::from_utf8(name_bytes).unwrap_or("<non-utf8>"),
                    index,
                );
            }
        }
        entry
    }
}

#[derive(Debug, Clone, Copy)]
enum EntryKind {
    Scalar(ScalarTag, u32),
    Buffer(u16),
}

/// Borrowed typed view over a `ParamFrame`, driven by a `ParamViewIndex`.
#[derive(Debug, Clone, Copy)]
pub struct ParamView<'a> {
    index: &'a ParamViewIndex,
    scalar_area: &'a [u8],
    buffer_slots: &'a [u64],
    wire_bytes: &'a [u8],
}

impl<'a> ParamView<'a> {
    /// Borrow a view over raw wire bytes with the same on-the-wire layout as
    /// `ParamFrame::storage_bytes()`: padded scalar area (`scalar_size`
    /// rounded up to multiple of 8) followed by `buffer_slot_count` `u64`
    /// slots. Plugin-side counterpart of `ParamView::new` (ADR 0045 §6).
    ///
    /// `bytes.as_ptr()` must be 8-byte aligned — satisfied when bytes come
    /// from a `Vec<u64>`-backed `ParamFrame` across FFI, which is the only
    /// supported producer.
    pub fn from_wire_bytes(index: &'a ParamViewIndex, bytes: &'a [u8]) -> Self {
        use super::U64_SIZE;
        let scalar_words = (index.scalar_size as usize).div_ceil(U64_SIZE);
        let scalar_padded = scalar_words * U64_SIZE;
        let total = scalar_padded + (index.buffer_slot_count as usize) * U64_SIZE;
        debug_assert_eq!(
            bytes.len(),
            total,
            "ParamView::from_wire_bytes: byte length mismatch",
        );
        debug_assert_eq!(
            bytes.as_ptr() as usize & (U64_SIZE - 1),
            0,
            "ParamView::from_wire_bytes: bytes must be 8-byte aligned",
        );
        let scalar_area = &bytes[..index.scalar_size as usize];
        let tail = &bytes[scalar_padded..];
        // SAFETY: aligned (debug-asserted), u64 is POD, slice covers
        // buffer_slot_count * 8 bytes of the input buffer.
        let buffer_slots = unsafe {
            std::slice::from_raw_parts(
                tail.as_ptr() as *const u64,
                index.buffer_slot_count as usize,
            )
        };
        Self { index, scalar_area, buffer_slots, wire_bytes: bytes }
    }

    /// Expected wire-byte length for the given index: padded scalar area +
    /// tail slot bytes. Used by decode helpers for length checks.
    pub fn wire_size_for(index: &ParamViewIndex) -> usize {
        use super::U64_SIZE;
        let scalar_words = (index.scalar_size as usize).div_ceil(U64_SIZE);
        scalar_words * U64_SIZE + (index.buffer_slot_count as usize) * U64_SIZE
    }

    /// Borrow the frame. Asserts shape consistency with the index.
    pub fn new(index: &'a ParamViewIndex, frame: &'a ParamFrame) -> Self {
        debug_assert_eq!(
            frame.layout_hash(),
            index.descriptor_hash,
            "ParamView::new: frame / index descriptor_hash mismatch",
        );
        debug_assert_eq!(
            frame.scalar_size(),
            index.scalar_size as usize,
            "ParamView::new: scalar_size mismatch",
        );
        debug_assert_eq!(
            frame.buffer_slot_count(),
            index.buffer_slot_count as usize,
            "ParamView::new: buffer_slot_count mismatch",
        );
        Self {
            index,
            scalar_area: frame.scalar_area(),
            buffer_slots: frame.buffer_slots(),
            wire_bytes: frame.storage_bytes(),
        }
    }

    /// Borrowed wire-format bytes of the underlying frame (padded scalar
    /// area + tail slot table). Used by FFI dispatch on the audio thread;
    /// plugin-side reconstructs a matching `ParamView` from these bytes and
    /// its own layout (ADR 0045 §6).
    #[inline]
    pub fn wire_bytes(&self) -> &'a [u8] {
        self.wire_bytes
    }

    // ── Typed (ADR 0046) getters ─────────────────────────────────────────

    /// Typed generic access. Return type is driven by the key's
    /// [`ParamKey::Value`].
    #[inline]
    pub fn get<K: crate::params::ParamKey>(&self, key: K) -> K::Value {
        key.fetch(self)
    }

    #[doc(hidden)]
    #[inline]
    pub fn fetch_float_static(&self, name: &'static str, index: u16) -> f32 {
        match self.index.lookup_static(name, index) {
            Entry::Scalar { tag: ScalarTag::Float, offset, .. } => {
                read_unaligned::<f32>(self.scalar_area, offset as usize)
            }
            _ => {
                debug_assert!(false, "ParamView::get::<Float>: {name} idx={index} not a Float slot");
                0.0
            }
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn fetch_int_static(&self, name: &'static str, index: u16) -> i64 {
        match self.index.lookup_static(name, index) {
            Entry::Scalar { tag: ScalarTag::Int, offset, .. } => {
                read_unaligned::<i64>(self.scalar_area, offset as usize)
            }
            _ => {
                debug_assert!(false, "ParamView::get::<Int>: {name} idx={index} not an Int slot");
                0
            }
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn fetch_bool_static(&self, name: &'static str, index: u16) -> bool {
        match self.index.lookup_static(name, index) {
            Entry::Scalar { tag: ScalarTag::Bool, offset, .. } => {
                self.scalar_area[offset as usize] != 0
            }
            _ => {
                debug_assert!(false, "ParamView::get::<Bool>: {name} idx={index} not a Bool slot");
                false
            }
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn fetch_enum_static(&self, name: &'static str, index: u16) -> u32 {
        match self.index.lookup_static(name, index) {
            Entry::Scalar { tag: ScalarTag::Enum, offset, .. } => {
                read_unaligned::<u32>(self.scalar_area, offset as usize)
            }
            _ => {
                debug_assert!(false, "ParamView::get::<Enum>: {name} idx={index} not an Enum slot");
                0
            }
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn fetch_buffer_static(&self, name: &'static str, index: u16) -> Option<FloatBufferId> {
        match self.index.lookup_static(name, index) {
            Entry::Buffer { slot_index, .. } => {
                let raw = self.buffer_slots[slot_index as usize];
                if raw == 0 {
                    None
                } else {
                    Some(FloatBufferId::from_u64_unchecked(raw))
                }
            }
            _ => {
                debug_assert!(false, "ParamView::get::<Buffer>: {name} idx={index} not a Buffer slot");
                None
            }
        }
    }

}

#[inline]
fn read_unaligned<T: Copy>(area: &[u8], offset: usize) -> T {
    let size = std::mem::size_of::<T>();
    debug_assert!(offset + size <= area.len());
    // SAFETY: bounds checked; write_unaligned/read_unaligned tolerate any
    // alignment; T is Copy (POD in our usage).
    unsafe { std::ptr::read_unaligned(area.as_ptr().add(offset) as *const T) }
}

/// Deterministic seeded hash over a `ParameterKey`. Simple FNV-1a-ish mixer
/// — no allocation, no dependencies.
#[inline]
fn key_hash_seed(key: &ParameterKey, seed: u64) -> u64 {
    hash_raw(key.name.as_bytes(), key.index as u64, seed)
}

#[inline]
fn hash_raw(name: &[u8], index: u64, seed: u64) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
    for &b in name {
        h ^= b as u64;
        h = h.wrapping_mul(K);
    }
    h ^= index.wrapping_add(0x9e37_79b9_7f4a_7c15);
    h = h.wrapping_mul(K);
    h ^= h >> 33;
    h
}
