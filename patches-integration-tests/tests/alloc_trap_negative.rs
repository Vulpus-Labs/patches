//! Negative test for the audio-thread allocator trap.
//!
//! Confirms the trap actually fires on a deliberate allocation from a
//! tagged thread. The abort-on-hit default would terminate the test
//! process, so we switch the trap into `TrapMode::Count` for the
//! duration of the test and assert that `trap_hits` moved.
//!
//! With the `audio-thread-allocator-trap` feature off, tagging and hit
//! counting are no-ops; the test is a trivial pass in that case.

#[global_allocator]
static A: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;

#[cfg(feature = "audio-thread-allocator-trap")]
#[test]
fn deliberate_allocation_on_tagged_thread_is_observed() {
    use patches_alloc_trap::{set_mode, trap_hits, NoAllocGuard, TrapMode};

    set_mode(TrapMode::Count);
    let before = trap_hits();

    // Tag the thread and deliberately allocate. The allocator should
    // observe the alloc; in Count mode it just increments the hit
    // counter rather than aborting.
    {
        let _g = NoAllocGuard::enter();
        // Force a heap allocation that cannot be optimised away.
        let b = Box::new(0u64);
        std::hint::black_box(&*b);
        drop(b);
    }

    set_mode(TrapMode::Abort);
    let after = trap_hits();
    assert!(
        after > before,
        "trap did not observe allocation: before={before} after={after}"
    );
}

#[cfg(not(feature = "audio-thread-allocator-trap"))]
#[test]
fn deliberate_allocation_on_tagged_thread_is_observed() {
    // Trap compiled out — nothing to verify. The test exists so the file
    // compiles cleanly in both feature states.
}
