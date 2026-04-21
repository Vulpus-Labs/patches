//! Payload id newtypes for the ADR 0045 audio-thread data plane.
//!
//! `FloatBufferId` is a `#[repr(transparent)]` u64 wrapper that
//! encodes `(generation << 32) | slot`. Minted by the host-side
//! `ArcTable`; plugin / audio-thread code receives it through the
//! ABI and must not fabricate them.

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct FloatBufferId(u64);

impl FloatBufferId {
    #[allow(dead_code)]
    pub fn pack(generation: u32, slot: u32) -> Self {
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

    pub fn from_u64_unchecked(raw: u64) -> Self {
        Self(raw)
    }
}

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
    fn float_buffer_id_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FloatBufferId>();
    }
}
