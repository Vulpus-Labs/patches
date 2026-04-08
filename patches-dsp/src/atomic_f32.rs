//! Lock-free `f32` wrapper using bit-encoding into `AtomicU32`.
//!
//! Intended for real-time parameter transport between audio and processing
//! threads where `Relaxed` ordering is sufficient.

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// An `f32` value stored atomically via `AtomicU32` bit-encoding.
///
/// All operations use `Relaxed` ordering — suitable for parameter values read
/// by a single consumer where eventual consistency is acceptable.
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    /// Create a new `AtomicF32` with the given initial value.
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    /// Store a value (Relaxed ordering).
    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed);
    }

    /// Load the current value (Relaxed ordering).
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
}

impl Default for AtomicF32 {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_load_round_trip_normal_values() {
        let a = AtomicF32::new(0.0);
        for &v in &[0.0f32, 1.0, -1.0, std::f32::consts::PI, 1e-38, 1e38, 0.123_456_7] {
            a.store(v);
            assert_eq!(a.load(), v, "round-trip failed for {v}");
        }
    }

    #[test]
    fn store_load_round_trip_special_values() {
        let a = AtomicF32::new(0.0);

        // Infinity
        a.store(f32::INFINITY);
        assert_eq!(a.load(), f32::INFINITY);
        a.store(f32::NEG_INFINITY);
        assert_eq!(a.load(), f32::NEG_INFINITY);

        // Negative zero
        a.store(-0.0);
        assert!(a.load().is_sign_negative(), "negative zero lost its sign");
        assert_eq!(a.load().to_bits(), (-0.0f32).to_bits());

        // Subnormal
        let subnormal = f32::MIN_POSITIVE / 2.0;
        a.store(subnormal);
        assert_eq!(a.load().to_bits(), subnormal.to_bits());

        // NaN: bit-exact comparison since NaN != NaN
        a.store(f32::NAN);
        assert_eq!(a.load().to_bits(), f32::NAN.to_bits());
    }

    #[test]
    fn new_then_load_returns_initial_value() {
        for &v in &[0.0f32, 42.5, -1.0, f32::INFINITY] {
            let a = AtomicF32::new(v);
            assert_eq!(a.load(), v);
        }
    }

    #[test]
    fn default_produces_zero() {
        let a = AtomicF32::default();
        assert_eq!(a.load(), 0.0);
        assert!(!a.load().is_sign_negative());
    }
}
