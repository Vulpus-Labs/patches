//! Backend-agnostic cleanup action type.
//!
//! Used by the audio thread to send evicted modules and replaced
//! [`ExecutionPlan`](crate::ExecutionPlan)s to a background cleanup thread
//! for deallocation off the real-time path. The concrete cleanup-thread
//! spawn lives in [`crate::kernel`].

use patches_core::Module;

use patches_ffi_common::param_frame::ParamFrame;
use patches_planner::{ExecutionPlan, ParamState};

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
    /// Per-instance parameter-plane state evicted alongside a tombstoned
    /// module. Carries the module's [`ParamLayout`], [`ParamViewIndex`], and
    /// current [`ParamFrame`] — all of which hold owned heap allocations
    /// that must not drop on the audio thread.
    DropParamState(Box<ParamState>),
    /// A `ParamFrame` displaced from a pool-resident `ParamState` when a
    /// parameter update installs a newer frame. The old frame's `Vec<u64>`
    /// storage drops off-thread to keep the update path allocation-free.
    DropParamFrame(Box<ParamFrame>),
}
