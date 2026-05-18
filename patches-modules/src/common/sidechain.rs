//! Sidechain convention helpers (ADR 0076).
//!
//! Dynamics modules expose a `sidechain` input. When unconnected, the detector
//! reads from `in` instead (self-key) — standard hardware behaviour and the
//! common case. Both cables are read each tick and the source is selected
//! via `is_connected()`. No `Option`-typed cable read on the audio path.

/// Detector source for a mono dynamics module: `sidechain` if connected,
/// else `self_val` (read from `in`).
#[inline]
pub fn mono_key(self_val: f32, sidechain_val: f32, sidechain_connected: bool) -> f32 {
    if sidechain_connected { sidechain_val } else { self_val }
}

/// Detector source for a stereo dynamics module — pair-wise select for `(L, R)`.
/// A mono source feeding a stereo `sidechain` port is broadcast automatically
/// by the cable builder (ADR 0059), so no special-casing here.
#[inline]
pub fn stereo_key(
    self_lr: (f32, f32),
    sidechain_lr: (f32, f32),
    sidechain_connected: bool,
) -> (f32, f32) {
    if sidechain_connected { sidechain_lr } else { self_lr }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_self_key_when_unconnected() {
        assert_eq!(mono_key(0.5, 0.9, false), 0.5);
    }

    #[test]
    fn mono_sidechain_when_connected() {
        assert_eq!(mono_key(0.5, 0.9, true), 0.9);
    }

    #[test]
    fn stereo_self_key_when_unconnected() {
        assert_eq!(stereo_key((0.5, 0.3), (0.9, 0.1), false), (0.5, 0.3));
    }

    #[test]
    fn stereo_sidechain_when_connected() {
        assert_eq!(stereo_key((0.5, 0.3), (0.9, 0.1), true), (0.9, 0.1));
    }
}
