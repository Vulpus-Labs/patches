//! Cable-pool test helpers.
//!
//! Tests that only exercise the cycle region still need a scratch
//! slice big enough to satisfy disconnected sink reads
//! (`MONO_READ_SINK = 0`, `POLY_READ_SINK = 1` — both in scratch under
//! ADR 0072 phase 5) and any backplane reads
//! (`[SINK_SLOTS, RESERVED_SLOTS)`). [`reserved_scratch`] returns a
//! zero-initialised `Vec<CableValue>` sized exactly for the reserved
//! range, which is the smallest slice that keeps every default port
//! read in-bounds.

use crate::cables::{CableValue, RESERVED_SLOTS};

/// Zero-initialised scratch slice covering the sink + backplane
/// reserved range (`[0, RESERVED_SLOTS)`). Suitable for any test
/// that does not exercise dyn-scratch slots.
pub fn reserved_scratch() -> Vec<CableValue> {
    vec![CableValue::mono(0.0); RESERVED_SLOTS]
}
