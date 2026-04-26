//! Cross-thread/cross-crate ring transports shared by the engine and
//! observer (and, later, host→engine control signals).
//!
//! Both ends of a lock-free SPSC ring must be minted from the same
//! factory call — they share private storage. Hosting those types in
//! either `patches-engine` or `patches-observation` would force one to
//! depend on the other; this crate exists to break that cycle. It
//! depends only on `patches-core` (for wire-format types like
//! `TapBlockFrame`) and `rtrb`.

pub mod tap_ring;

pub use tap_ring::{tap_ring, TapRingConsumer, TapRingProducer, TapRingShared};
