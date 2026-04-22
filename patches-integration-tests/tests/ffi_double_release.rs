//! E107 ticket 0624 — debug-build audit trap on double-release.
//!
//! Real ABI callers that double-release end up invoking
//! [`ArcTableAudio::release`] twice for the same id. In debug builds
//! the refcount audit inside `Slots::release` asserts the refcount was
//! non-zero before the decrement; a double-release underflows and
//! trips the debug_assert.
//!
//! We exercise that code path directly (the extern-"C" indirection
//! aborts rather than unwinds, so `#[should_panic]` cannot catch it
//! when the release is invoked from a plugin trampoline — the audit
//! still fires, but as an abort. Driving the ArcTable directly lets
//! the unit-test harness observe the panic).

use std::sync::Arc;

use patches_ffi_common::arc_table::{RuntimeArcTables, RuntimeArcTablesConfig};

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "refcount release underflow")]
fn double_release_trips_refcount_audit() {
    let (mut control, mut audio) =
        RuntimeArcTables::new(RuntimeArcTablesConfig { float_buffers: 4 });
    let payload: Arc<[f32]> = Arc::from(vec![0.0f32].into_boxed_slice());
    let id = control.mint_float_buffer(Arc::clone(&payload)).unwrap();

    // First release: refcount 1 → 0.
    audio.release_float_buffer(id);
    // Second release: refcount 0 → wrap. Debug audit fires.
    audio.release_float_buffer(id);
}
