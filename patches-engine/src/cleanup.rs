//! Backend-agnostic cleanup action type.
//!
//! Used by the audio thread to send evicted modules and replaced
//! [`ExecutionPlan`](crate::ExecutionPlan)s to a background cleanup thread
//! for deallocation off the real-time path. The concrete cleanup-thread
//! spawn lives in [`crate::kernel`].

use patches_core::Module;

use patches_planner::ExecutionPlan;

/// Default module pool capacity: number of `Option<Box<dyn Module>>` slots
/// pre-allocated on the audio thread.
///
/// 1024 was chosen as a round power-of-two that comfortably covers any
/// realistic patch (typical patches use tens of modules; even dense
/// live-coding setups rarely exceed a few hundred). If a patch exceeds this
/// limit, callers can pass their own capacity at construction time.
pub const DEFAULT_MODULE_POOL_CAPACITY: usize = 1024;

/// A value sent to the `"patches-cleanup"` thread for deallocation off the
/// audio thread.
///
/// Introduced in T-0169 to replace the bare `Box<dyn Module>` ring buffer
/// element type. The cleanup thread simply drops whichever variant it
/// receives.
pub enum CleanupAction {
    /// A module evicted from the pool via [`crate::ModulePool::tombstone`].
    DropModule(Box<dyn Module>),
    /// An [`ExecutionPlan`] replaced by a newer one.
    DropPlan(Box<ExecutionPlan>),
}
