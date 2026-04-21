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

    #[inline]
    fn lookup(&self, key: &ParameterKey) -> Entry {
        if self.table.is_empty() {
            return Entry::Empty;
        }
        let h = key_hash_seed(key, self.seed);
        let bucket = (h & self.mask) as usize;
        let entry = self.table[bucket];
        // In debug, verify the full hash matches — catches unknown keys.
        match entry {
            Entry::Scalar { name_hash, .. } | Entry::Buffer { name_hash, .. } => {
                debug_assert_eq!(
                    name_hash, h,
                    "ParamView: unknown key {:?} (bucket collision with a declared key)",
                    key,
                );
                if name_hash != h {
                    return Entry::Empty;
                }
            }
            Entry::Empty => {
                debug_assert!(false, "ParamView: unknown key {:?}", key);
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
}

impl<'a> ParamView<'a> {
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
        }
    }

    #[inline]
    pub fn float(&self, key: impl Into<ParameterKey>) -> f32 {
        let k = key.into();
        match self.index.lookup(&k) {
            Entry::Scalar { tag: ScalarTag::Float, offset, .. } => {
                read_unaligned::<f32>(self.scalar_area, offset as usize)
            }
            _ => {
                debug_assert!(false, "ParamView::float: key {:?} not a Float slot", k);
                0.0
            }
        }
    }

    #[inline]
    pub fn int(&self, key: impl Into<ParameterKey>) -> i64 {
        let k = key.into();
        match self.index.lookup(&k) {
            Entry::Scalar { tag: ScalarTag::Int, offset, .. } => {
                read_unaligned::<i64>(self.scalar_area, offset as usize)
            }
            _ => {
                debug_assert!(false, "ParamView::int: key {:?} not an Int slot", k);
                0
            }
        }
    }

    #[inline]
    pub fn bool(&self, key: impl Into<ParameterKey>) -> bool {
        let k = key.into();
        match self.index.lookup(&k) {
            Entry::Scalar { tag: ScalarTag::Bool, offset, .. } => {
                self.scalar_area[offset as usize] != 0
            }
            _ => {
                debug_assert!(false, "ParamView::bool: key {:?} not a Bool slot", k);
                false
            }
        }
    }

    #[inline]
    pub fn enum_variant(&self, key: impl Into<ParameterKey>) -> u32 {
        let k = key.into();
        match self.index.lookup(&k) {
            Entry::Scalar { tag: ScalarTag::Enum, offset, .. } => {
                read_unaligned::<u32>(self.scalar_area, offset as usize)
            }
            _ => {
                debug_assert!(false, "ParamView::enum_variant: key {:?} not an Enum slot", k);
                0
            }
        }
    }

    #[inline]
    pub fn buffer(&self, key: impl Into<ParameterKey>) -> Option<FloatBufferId> {
        let k = key.into();
        match self.index.lookup(&k) {
            Entry::Buffer { slot_index, .. } => {
                let raw = self.buffer_slots[slot_index as usize];
                if raw == 0 {
                    None
                } else {
                    Some(FloatBufferId::from_u64_unchecked(raw))
                }
            }
            _ => {
                debug_assert!(false, "ParamView::buffer: key {:?} not a Buffer slot", k);
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
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
    for &b in key.name.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(K);
    }
    h ^= (key.index as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    h = h.wrapping_mul(K);
    h ^= h >> 33;
    h
}
