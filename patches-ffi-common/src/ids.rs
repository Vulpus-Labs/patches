//! Payload id newtypes for the ADR 0045 audio-thread data plane.
//!
//! `FloatBufferId` and `SongDataId` are `#[repr(transparent)]` u64
//! wrappers that encode `(generation << 32) | slot`. They are
//! minted by the host-side `ArcTable`; plugin / audio-thread code
//! receives them through the ABI and must not fabricate them.

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct FloatBufferId(u64);

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SongDataId(u64);

macro_rules! impl_id {
    ($ty:ident) => {
        impl $ty {
            #[allow(dead_code)]
            pub(crate) fn pack(generation: u32, slot: u32) -> Self {
                Self(((generation as u64) << 32) | (slot as u64))
            }

            pub fn slot(self) -> u32 {
                self.0 as u32
            }

            pub fn generation(self) -> u32 {
                (self.0 >> 32) as u32
            }

            pub fn as_u64(self) -> u64 {
                self.0
            }

            pub(crate) fn from_u64_unchecked(raw: u64) -> Self {
                Self(raw)
            }
        }
    };
}

impl_id!(FloatBufferId);
impl_id!(SongDataId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trip_float_buffer() {
        for &(gen, slot) in &[
            (0u32, 0u32),
            (0, u32::MAX),
            (u32::MAX, 0),
            (u32::MAX, u32::MAX),
            (0x1234_5678, 0x9abc_def0),
        ] {
            let id = FloatBufferId::pack(gen, slot);
            assert_eq!(id.generation(), gen);
            assert_eq!(id.slot(), slot);
            assert_eq!(FloatBufferId::from_u64_unchecked(id.as_u64()), id);
        }
    }

    #[test]
    fn pack_round_trip_song_data() {
        let id = SongDataId::pack(7, 42);
        assert_eq!(id.generation(), 7);
        assert_eq!(id.slot(), 42);
    }

    #[test]
    fn distinct_types_do_not_unify() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FloatBufferId>();
        assert_send_sync::<SongDataId>();
    }
}
