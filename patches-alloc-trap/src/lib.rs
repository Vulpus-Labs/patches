//! Audio-thread allocator trap (ADR 0045 spike 4).
//!
//! Provides a `TrappingAllocator` type that wraps the system allocator and,
//! when the `audio-thread-allocator-trap` feature is enabled, aborts the
//! process on any allocation performed by a thread tagged as audio-thread.
//!
//! Binaries that want the trap installed depend on this crate with the
//! feature on and declare:
//!
//! ```ignore
//! #[global_allocator]
//! static A: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;
//! ```
//!
//! Audio-thread entry points (CPAL callback, CLAP process callback, or a
//! headless test loop) call [`mark_audio_thread`] once on the thread that
//! will drive audio. From that point on, any allocation on that thread
//! aborts via `std::process::abort()`.
//!
//! When the feature is off, all APIs compile to no-ops and
//! `TrappingAllocator` forwards every call to `System` unconditionally —
//! zero runtime cost.

use std::alloc::{GlobalAlloc, Layout, System};

#[cfg(feature = "audio-thread-allocator-trap")]
use std::cell::Cell;
#[cfg(feature = "audio-thread-allocator-trap")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Transparent wrapper over `System` that, with the feature on, aborts if
/// the current thread is tagged as audio-thread and the process-wide trap
/// is armed.
pub struct TrappingAllocator;

#[cfg(feature = "audio-thread-allocator-trap")]
mod imp {
    use super::*;

    thread_local! {
        pub(crate) static AUDIO_THREAD: Cell<bool> = const { Cell::new(false) };
    }

    /// Process-wide latch. Allocations before the first `mark_audio_thread`
    /// call pass through unconditionally so static initialisers and
    /// pre-audio setup never trip the trap.
    pub(crate) static TRAP_ARMED: AtomicBool = AtomicBool::new(false);

    /// Count of allocations that would have been trapped. In `Abort` mode
    /// the process aborts before this ever increments past one; in `Count`
    /// mode (see [`set_mode`]) the counter records hits without aborting.
    pub(crate) static TRAP_HITS: AtomicUsize = AtomicUsize::new(0);

    /// Soft-mode toggle used only by the negative test in ticket 0594.
    /// When true, a trap hit increments `TRAP_HITS` and returns without
    /// aborting so the test can assert the counter moved.
    pub(crate) static COUNT_ONLY: AtomicBool = AtomicBool::new(false);

    #[inline]
    pub(crate) fn maybe_trap() -> bool {
        if !TRAP_ARMED.load(Ordering::Acquire) {
            return false;
        }
        AUDIO_THREAD.with(|f| {
            if !f.get() {
                return false;
            }
            TRAP_HITS.fetch_add(1, Ordering::Relaxed);
            if COUNT_ONLY.load(Ordering::Relaxed) {
                false
            } else {
                // `panic!` allocates; abort instead so the debugger catches
                // the exact allocation stack.
                std::process::abort();
            }
        })
    }
}

unsafe impl GlobalAlloc for TrappingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        #[cfg(feature = "audio-thread-allocator-trap")]
        {
            let _ = imp::maybe_trap();
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(feature = "audio-thread-allocator-trap")]
        {
            let _ = imp::maybe_trap();
        }
        System.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        #[cfg(feature = "audio-thread-allocator-trap")]
        {
            let _ = imp::maybe_trap();
        }
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        #[cfg(feature = "audio-thread-allocator-trap")]
        {
            let _ = imp::maybe_trap();
        }
        System.realloc(ptr, layout, new_size)
    }
}

// ── Tagging API ──────────────────────────────────────────────────────────────

/// Tag the current thread as an audio thread. Idempotent. With the feature
/// off this is an inline no-op.
#[inline]
pub fn mark_audio_thread() {
    #[cfg(feature = "audio-thread-allocator-trap")]
    {
        imp::AUDIO_THREAD.with(|f| f.set(true));
        imp::TRAP_ARMED.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// Clear the audio-thread tag on the current thread. Used by [`NoAllocGuard`]
/// drop; rarely needed by production callers.
#[inline]
pub fn clear_audio_thread() {
    #[cfg(feature = "audio-thread-allocator-trap")]
    {
        imp::AUDIO_THREAD.with(|f| f.set(false));
    }
}

/// Is the current thread tagged as audio-thread? Always returns `false`
/// when the feature is off.
#[inline]
#[cfg(feature = "audio-thread-allocator-trap")]
pub fn is_audio_thread() -> bool {
    imp::AUDIO_THREAD.with(|f| f.get())
}

#[inline]
#[cfg(not(feature = "audio-thread-allocator-trap"))]
pub fn is_audio_thread() -> bool {
    false
}

/// Number of allocations the trap has observed on tagged threads. Always
/// zero when the feature is off.
#[inline]
#[cfg(feature = "audio-thread-allocator-trap")]
pub fn trap_hits() -> usize {
    imp::TRAP_HITS.load(std::sync::atomic::Ordering::Relaxed)
}

#[inline]
#[cfg(not(feature = "audio-thread-allocator-trap"))]
pub fn trap_hits() -> usize {
    0
}

/// Behaviour of the trap when a tagged-thread allocation is observed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrapMode {
    /// Abort the process (default).
    Abort,
    /// Increment `trap_hits()` and return. Used only by tests that need to
    /// confirm the trap is live without losing the process.
    Count,
}

/// Set the trap mode. No effect with the feature off.
#[inline]
pub fn set_mode(mode: TrapMode) {
    #[cfg(feature = "audio-thread-allocator-trap")]
    {
        imp::COUNT_ONLY.store(matches!(mode, TrapMode::Count), std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(feature = "audio-thread-allocator-trap"))]
    {
        let _ = mode;
    }
}

// ── Scope guard ──────────────────────────────────────────────────────────────

/// RAII guard that tags the current thread as audio-thread for its
/// lifetime. Useful for driving `HeadlessEngine::tick` from a test thread
/// without tagging the whole test-runner thread.
///
/// With the feature off this is a ZST whose constructor and destructor do
/// nothing.
pub struct NoAllocGuard {
    #[cfg(feature = "audio-thread-allocator-trap")]
    was_tagged: bool,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl NoAllocGuard {
    #[inline]
    #[cfg(feature = "audio-thread-allocator-trap")]
    pub fn enter() -> Self {
        let was_tagged = is_audio_thread();
        mark_audio_thread();
        Self {
            was_tagged,
            _not_send: std::marker::PhantomData,
        }
    }

    #[inline]
    #[cfg(not(feature = "audio-thread-allocator-trap"))]
    pub fn enter() -> Self {
        Self {
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Drop for NoAllocGuard {
    #[inline]
    fn drop(&mut self) {
        #[cfg(feature = "audio-thread-allocator-trap")]
        {
            if !self.was_tagged {
                clear_audio_thread();
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "audio-thread-allocator-trap")]
mod tests {
    use super::*;

    #[test]
    fn fresh_thread_is_not_audio() {
        std::thread::spawn(|| {
            assert!(!is_audio_thread());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn mark_is_per_thread() {
        std::thread::spawn(|| {
            mark_audio_thread();
            assert!(is_audio_thread());
        })
        .join()
        .unwrap();
        // Back on the test runner thread: tag is separate.
        std::thread::spawn(|| {
            assert!(!is_audio_thread());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn mark_is_idempotent() {
        std::thread::spawn(|| {
            mark_audio_thread();
            mark_audio_thread();
            assert!(is_audio_thread());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn guard_sets_and_clears() {
        std::thread::spawn(|| {
            assert!(!is_audio_thread());
            {
                let _g = NoAllocGuard::enter();
                assert!(is_audio_thread());
            }
            assert!(!is_audio_thread());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn guard_preserves_prior_tag() {
        std::thread::spawn(|| {
            mark_audio_thread();
            {
                let _g = NoAllocGuard::enter();
                assert!(is_audio_thread());
            }
            // Outer tag was already set — guard drop must not clear it.
            assert!(is_audio_thread());
            clear_audio_thread();
        })
        .join()
        .unwrap();
    }
}
